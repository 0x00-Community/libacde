use super::*;

impl Engine {
    pub(in crate::render::dom) fn control(&self, node: NodeId) -> Control {
        let Some(e) = self.doc.get_node(node).and_then(|n| n.element_data()) else { return Control::None };
        if e.name.ns != ns!(html) {
            return Control::None;
        }
        match &*e.name.local {
            "textarea" => Control::Editor(true),
            "select" => Control::Select,
            "input" => {
                let kind = e.attr(blitz_dom::local_name!("type")).map(str::to_ascii_lowercase);
                match kind.as_deref() {
                    None | Some("text" | "password" | "email" | "number" | "search" | "tel" | "url") => Control::Editor(false),
                    Some("checkbox" | "radio") => Control::Checkable,
                    Some("range") => Control::Range,
                    Some("hidden" | "submit" | "reset" | "button" | "image") => Control::Default,
                    Some(_) => Control::Value,
                }
            }
            "option" | "button" => Control::Default,
            _ => Control::None,
        }
    }

    pub(in crate::render::dom) fn value(&self, node: NodeId) -> String {
        let n = &self.doc.tree()[node];
        let attr = |name| n.attr(name).map(str::to_owned);
        let dirty = self.ids.forms.get(&node).and_then(|f| f.value.clone());
        match self.control(node) {
            Control::Editor(true) => dirty.unwrap_or_else(|| n.text_content()),
            Control::Editor(false) | Control::Value => dirty.or_else(|| attr(blitz_dom::local_name!("value"))).unwrap_or_default(),
            Control::Range => {
                let raw = dirty.or_else(|| attr(blitz_dom::local_name!("value")));
                let range = RangeValue::new(
                    n.attr(blitz_dom::local_name!("min")),
                    n.attr(blitz_dom::local_name!("max")),
                    n.attr(blitz_dom::local_name!("step")),
                    raw.as_deref(),
                );
                number_string(range.value)
            }
            Control::Checkable => attr(blitz_dom::local_name!("value")).unwrap_or_else(|| "on".into()),
            Control::Select => self.options(node).into_iter().find(|&o| self.selectedness(o)).map(|o| self.value(o)).unwrap_or_default(),
            Control::Default if n.data.is_element_with_tag_name(&blitz_dom::local_name!("option")) => {
                attr(blitz_dom::local_name!("value")).unwrap_or_else(|| collapse_whitespace(&n.text_content()))
            }
            Control::Default | Control::None => attr(blitz_dom::local_name!("value")).unwrap_or_default(),
        }
    }

    pub(in crate::render::dom) fn set_value(&mut self, id: u32, node: NodeId, value: &str) -> Result<()> {
        match self.control(node) {
            Control::Editor(multiline) => {
                let value = match multiline {
                    true => value.to_owned(),
                    false => value.replace(['\r', '\n'], ""),
                };
                self.ids.forms.entry(node).or_default().value = Some(value);
                self.show_form_state(node);
            }
            Control::Value => self.ids.forms.entry(node).or_default().value = Some(value.to_owned()),
            Control::Range => {
                let sanitized = self.range_of(node, Some(value)).map(|r| number_string(r.value)).unwrap_or_default();
                self.ids.forms.entry(node).or_default().value = Some(sanitized);
                self.show_form_state(node);
            }
            Control::Select => {
                let index = self.options(node).iter().position(|&o| self.value(o) == value).map_or(-1, |i| i as i32);
                self.set_selected_index(node, index);
            }
            Control::Checkable | Control::Default | Control::None => {
                self.set_attribute_dom(id, "value", Some(value))?;
            }
        }
        Ok(())
    }

    pub(in crate::render::dom) fn checked(&self, node: NodeId) -> bool {
        match self.ids.forms.get(&node).and_then(|f| f.checked) {
            Some(checked) => checked,
            None => self.doc.tree()[node].attr(blitz_dom::local_name!("checked")).is_some(),
        }
    }

    pub(in crate::render::dom) fn set_checked(&mut self, node: NodeId, checked: bool) {
        self.ids.forms.entry(node).or_default().checked = Some(checked);
        self.show_form_state(node);
        let n = &self.doc.tree()[node];
        let radio = n.attr(blitz_dom::local_name!("type")).is_some_and(|t| t.eq_ignore_ascii_case("radio"));
        let name = n.attr(blitz_dom::local_name!("name")).unwrap_or_default().to_owned();
        if !(checked && radio && !name.is_empty()) {
            return;
        }
        let form = self.form_of(node);
        let root = self.tree_root(node);
        let group: Vec<NodeId> = self
            .subtree(root)
            .into_iter()
            .filter(|&other| {
                other != node
                    && self.doc.get_node(other).is_some_and(|o| {
                        o.data.is_element_with_tag_name(&blitz_dom::local_name!("input"))
                            && o.attr(blitz_dom::local_name!("type")).is_some_and(|t| t.eq_ignore_ascii_case("radio"))
                            && o.attr(blitz_dom::local_name!("name")) == Some(&*name)
                    })
                    && self.form_of(other) == form
            })
            .collect();
        for other in group {
            if self.checked(other) {
                self.ids.forms.entry(other).or_default().checked = Some(false);
                self.show_form_state(other);
            }
        }
    }

    pub(in crate::render::dom) fn form_of(&self, node: NodeId) -> Option<NodeId> {
        self.ancestors(node)
            .into_iter()
            .skip(1)
            .find(|&a| self.doc.get_node(a).is_some_and(|n| n.data.is_element_with_tag_name(&blitz_dom::local_name!("form"))))
    }

    pub(in crate::render::dom) fn range_of(&self, node: NodeId, value: Option<&str>) -> Option<RangeValue> {
        let e = self.doc.get_node(node).and_then(|n| n.element_data())?;
        let attr = |name: &str| e.attrs().iter().find(|a| a.name.ns == ns!() && &*a.name.local == name).map(|a| a.value.as_str());
        Some(RangeValue::new(attr("min"), attr("max"), attr("step"), value.or_else(|| attr("value"))))
    }

    pub(in crate::render::dom) fn show_form_state(&mut self, node: NodeId) {
        let control = self.control(node);
        if !matches!(control, Control::Range)
            && let Some(e) = self.doc.get_node_mut(node).and_then(|n| n.element_data_mut())
            && matches!(e.special_data, SpecialElementData::RangeInput(_))
        {
            e.special_data = SpecialElementData::None;
            e.repaint();
        }
        match control {
            Control::Range => {
                let dirty = self.ids.forms.get(&node).and_then(|f| f.value.clone());
                let Some(range) = self.range_of(node, dirty.as_deref()) else { return };
                if dirty.is_some() {
                    self.ids.forms.entry(node).or_default().value = Some(number_string(range.value));
                }
                let fraction = range.fraction();
                let Some(e) = self.doc.get_node_mut(node).and_then(|n| n.element_data_mut()) else { return };
                if e.range_input_fraction() != Some(fraction) {
                    e.special_data = SpecialElementData::RangeInput(fraction);
                    e.repaint();
                }
            }
            Control::Editor(multiline) => {
                let password = !multiline && self.is_password(node);
                let value = match self.ids.forms.get(&node).and_then(|f| f.value.clone()) {
                    Some(value) => value,
                    None if password || self.ids.masked.contains(&node) => self.value(node),
                    None => return,
                };
                let value = match password {
                    true => {
                        self.ids.masked.insert(node);
                        mask(&value)
                    }
                    false => {
                        self.ids.masked.remove(&node);
                        value
                    }
                };
                let Some(e) = self.doc.get_node_mut(node).and_then(|n| n.element_data_mut()) else { return };
                if !matches!(e.special_data, SpecialElementData::TextInput(_)) {
                    e.special_data = SpecialElementData::TextInput(TextInputData::new(multiline));
                }
                if let Some(input) = e.text_input_data_mut()
                    && input.editor.raw_text() != value
                {
                    input.editor.set_text(&value);
                    e.repaint();
                    self.doc.with_text_input(node, |mut driver| driver.refresh_layout());
                }
            }
            Control::Checkable => {
                let checked = self.checked(node);
                self.doc.change_state(node, |n| {
                    if let Some(e) = n.element_data_mut() {
                        if !matches!(e.special_data, SpecialElementData::CheckboxInput(_)) {
                            e.special_data = SpecialElementData::CheckboxInput(checked);
                        }
                        e.set_checkbox_input_checked(checked);
                    }
                });
            }
            _ => {}
        }
    }

    fn is_password(&self, node: NodeId) -> bool {
        let e = self.doc.get_node(node).and_then(|n| n.element_data());
        e.and_then(|e| e.attr(blitz_dom::local_name!("type"))).is_some_and(|t| t.eq_ignore_ascii_case("password"))
    }

    pub(in crate::render::dom) fn set_selection_range(&mut self, node: NodeId, start: u32, end: u32) {
        let Some(text) = self.doc.get_node(node).and_then(|n| n.element_data()?.text_input_data()).map(|t| t.editor.raw_text().to_owned()) else {
            return;
        };
        let (start, end) = (utf16_to_byte(&text, start), utf16_to_byte(&text, end));
        let (start, end) = (start.min(end), end.max(start));
        self.doc.with_text_input(node, |mut driver| driver.select_byte_range(start, end));
        if let Some(e) = self.doc.get_node_mut(node).and_then(|n| n.element_data_mut()) {
            e.repaint();
        }
    }

    pub(in crate::render::dom) fn set_focus(&mut self, node: Option<NodeId>, visible: bool) {
        let Some(node) = node else {
            self.doc.clear_focus();
            return;
        };
        self.doc.set_focus_to(node);
        let Some(focused) = self.doc.get_focussed_node_id() else { return };
        self.doc.change_state(focused, |n| {
            if let Some(e) = n.element_data_mut() {
                e.element_state.set(ElementState::FOCUSRING, visible);
            }
        });
    }

    pub(in crate::render::dom) fn set_active(&mut self, node: Option<NodeId>) {
        let elements = |e: &Engine, n: Option<NodeId>| -> Vec<NodeId> {
            n.filter(|&n| e.doc.get_node(n).is_some())
                .map(|n| e.ancestors(n))
                .unwrap_or_default()
                .into_iter()
                .filter(|&a| e.doc.get_node(a).is_some_and(|a| a.is_element()))
                .collect()
        };
        let old = elements(self, self.active);
        let new = elements(self, node);
        for &a in old.iter().filter(|a| !new.contains(a)) {
            self.doc.change_state(a, |n| n.unactive());
        }
        for &a in new.iter().filter(|a| !old.contains(a)) {
            self.doc.change_state(a, |n| n.active());
        }
        self.active = node;
    }

    pub(in crate::render::dom) fn set_selection(&mut self, anchor: NodeId, anchor_offset: u32, focus: NodeId, focus_offset: u32) {
        self.resolved();
        let a = self.selection_point(anchor, anchor_offset);
        let f = self.selection_point(focus, focus_offset);
        match (a, f) {
            (Some((ar, ab)), Some((fr, fb))) => self.doc.set_text_selection(ar, ab, fr, fb),
            _ => self.doc.clear_text_selection(),
        }
    }

    pub(in crate::render::dom) fn selection_point(&self, node: NodeId, offset: u32) -> Option<(NodeId, usize)> {
        let caret = Caret { doc: &self.doc };
        let n = self.doc.get_node(node)?;
        if n.is_text_node() {
            return caret.layout_position(node, offset);
        }
        let texts = |from: &[NodeId]| -> Vec<NodeId> {
            from.iter().flat_map(|&c| self.subtree(c)).filter(|&d| self.doc.get_node(d).is_some_and(|d| d.is_text_node())).collect()
        };
        let split = (offset as usize).min(n.children.len());
        if let Some(&first) = texts(&n.children[split..]).first() {
            return caret.layout_position(first, 0);
        }
        let last = *texts(&n.children[..split]).last()?;
        let len = self.doc.get_node(last)?.text_data()?.content.encode_utf16().count() as u32;
        caret.layout_position(last, len)
    }

    pub(in crate::render::dom) fn dom_metrics(&mut self, node: NodeId) -> Option<Metrics> {
        let geometry = shown(self.resolved(), node)?;
        let doc = geometry.doc;
        let mut m = geometry.metrics(node)?;
        let parent = geometry.offset_parent(node);
        if let Some(root) = doc.try_root_element().filter(|r| r.id == node) {
            let scroll = doc.viewport_scroll();
            m.scroll_left = scroll.x;
            m.scroll_top = scroll.y;
            let viewport = doc.viewport();
            let scale = viewport.scale_f64();
            let (width, height) = (f64::from(viewport.window_size.0) / scale, f64::from(viewport.window_size.1) / scale);
            let layout = root.final_layout();
            m.client_width = width;
            m.client_height = height;
            m.scroll_width = width.max(f64::from(layout.size.width.max(layout.scrollable_overflow_rect.right)));
            m.scroll_height = height.max(f64::from(layout.size.height.max(layout.scrollable_overflow_rect.bottom)));
        }
        m.offset_parent = parent.map(|p| self.ids.id_of(p));
        Some(m)
    }

    pub(in crate::render::dom) fn styled(&self, node: NodeId, pseudo: &str) -> Option<NodeId> {
        let n = self.doc.get_node(node)?;
        match pseudo {
            "" => Some(node),
            "::before" | ":before" => n.before(),
            "::after" | ":after" => n.after(),
            _ => None,
        }
    }

    pub(in crate::render::dom) fn match_media(&mut self, query: &str) -> bool {
        self.doc.media_matches_now(query)
    }

    pub(in crate::render::dom) fn scroll_to(&mut self, node: NodeId, left: f64, top: f64, smooth: bool) {
        self.resolved();
        let document = self.doc.root_node().id;
        let target = match node == document {
            true => match self.doc.try_root_element() {
                Some(root) => root.id,
                None => return,
            },
            false => node,
        };
        let viewport = self.doc.try_root_element().is_some_and(|r| r.id == target);
        let before = self.scroll_offset(node, viewport);
        let behavior = match smooth {
            true => blitz_dom::ScrollBehavior::Smooth,
            false => blitz_dom::ScrollBehavior::Instant,
        };
        self.doc.scroll_to(target, left, top, behavior);
        let key = if viewport { document } else { node };
        self.ids.scrolls.entry(key).or_insert(before);
        self.touch();
    }

    pub(in crate::render::dom) fn scroll_offset(&self, node: NodeId, viewport: bool) -> (f64, f64) {
        if viewport || node == self.doc.root_node().id {
            let s = self.doc.viewport_scroll();
            return (s.x, s.y);
        }
        self.doc.get_node(node).map_or((0.0, 0.0), |n| (n.scroll_offset().x, n.scroll_offset().y))
    }

    pub(in crate::render::dom) fn take_animation_events(&mut self) -> AnimationEvents {
        self.laid_out();
        let mut events = Vec::new();
        let mut taken = std::mem::take(&mut self.animation_events);
        taken.extend(self.doc.take_animation_events());
        for event in taken {
            let connected = self.doc.get_node(event.node).is_some_and(|n| n.flags.is_in_document());
            let target = if connected { Some(self.ids.id_of(event.node)) } else { self.ids.handle(event.node) };
            let Some(target) = target else { continue };
            events.push(AnimationEvent {
                kind: event.kind.name(),
                transition: event.kind.is_transition(),
                target,
                name: event.name,
                elapsed: event.elapsed,
                pseudo: event.pseudo,
            });
        }
        AnimationEvents { animating: self.doc.has_css_animations(), events }
    }

    pub(in crate::render::dom) fn take_scrolled(&mut self) -> Vec<u32> {
        let document = self.doc.root_node().id;
        let mut changed = Vec::new();
        let nodes: Vec<NodeId> = self.ids.scrolls.keys().copied().collect();
        for node in nodes {
            if self.doc.get_node(node).is_none() {
                self.ids.scrolls.remove(&node);
                continue;
            }
            let now = self.scroll_offset(node, node == document);
            let last = self.ids.scrolls.insert(node, now);
            if last != Some(now) {
                changed.push(node);
            }
        }
        changed.sort();
        self.outs(changed)
    }

    pub(in crate::render::dom) fn image(doc: &BaseDocument, node: NodeId) -> (f64, f64, u8, &str) {
        let Some(n) = doc.get_node(node) else { return (0.0, 0.0, FAILED, "") };
        let Some(e) = n.element_data() else { return (0.0, 0.0, FAILED, "") };
        let src = e.attr(blitz_dom::local_name!("src")).unwrap_or_default();
        match e.image_data() {
            Some(ImageData::Raster(r)) => (f64::from(r.width), f64::from(r.height), LOADED, src),
            Some(ImageData::Svg(svg)) => {
                let size = svg.tree.size();
                (f64::from(size.width()), f64::from(size.height()), LOADED, src)
            }
            _ if src.is_empty() => (0.0, 0.0, NO_SRC, src),
            _ if doc.is_image_pending(node) => (0.0, 0.0, LOADING, src),
            _ => (0.0, 0.0, FAILED, src),
        }
    }

    pub(in crate::render::dom) fn take_loaded(&mut self) -> Vec<u32> {
        self.doc.handle_messages();
        let mut changed = Vec::new();
        let ids: Vec<u32> = self.ids.images.keys().copied().collect();
        for id in ids {
            let Ok(node) = self.ids.node(id) else { continue };
            let (_, _, state, src) = Self::image(&self.doc, node);
            let loads = self.doc.get_node(node).and_then(|n| n.element_data()).map_or(0, |e| e.image_loads);
            let Some(last) = self.ids.images.get_mut(&id) else { continue };
            // every ended attempt is reported once (it fires load or error), and a newly pending image too, so the page keeps
            // asking until its answer comes from the loader
            let ended = loads != last.2 && matches!(state, LOADED | FAILED);
            let report = ended || (state == LOADING && (last.1 != state || last.0 != src));
            last.2 = loads;
            if last.0 != src {
                last.0 = src.to_owned();
            }
            last.1 = state;
            if report {
                changed.push(id);
            }
        }
        for node in self.doc.take_settled_stylesheets() {
            changed.push(self.ids.id_of(node));
        }
        changed.sort_unstable();
        changed
    }

    pub(in crate::render::dom) fn inner_text(&self, node: NodeId) -> String {
        let Some(n) = self.doc.get_node(node) else { return String::new() };
        if !n.is_element() || n.style().is_none() {
            return n.text_content();
        }
        let mut pieces = Vec::new();
        self.inner_text_walk(n, &mut pieces);
        let mut out = String::new();
        let mut breaks = 0;
        for piece in pieces {
            match piece {
                Piece::Break(k) => breaks = breaks.max(k),
                Piece::Text(t) if t.is_empty() => {}
                Piece::Text(t) => {
                    let mut t = t.as_str();
                    if !out.is_empty() && breaks > 0 {
                        while out.ends_with(' ') {
                            out.pop();
                        }
                        out.extend(std::iter::repeat_n('\n', breaks));
                    }
                    if (out.is_empty() || out.ends_with(['\n', ' '])) && !t.starts_with('\n') {
                        t = t.trim_start_matches(' ');
                    }
                    breaks = 0;
                    out.push_str(t);
                }
            }
        }
        out.truncate(out.trim_end_matches(' ').len());
        out
    }

    pub(in crate::render::dom) fn inner_text_walk(&self, n: &Node, out: &mut Vec<Piece>) {
        use blitz_dom::DisplayOutside;
        use blitz_dom::acss::properties::keywords::Visibility;
        for &child in &n.children {
            let Some(c) = self.doc.get_node(child) else { continue };
            match &c.data {
                NodeData::Text(t) => {
                    let visible = n.style().is_some_and(|s| *s.groups.visibility() == Visibility::Visible);
                    if !visible {
                        continue;
                    }
                    let collapse = self.doc.resolved_style_value(n.id, "white-space-collapse");
                    out.push(Piece::Text(match collapse.as_str() {
                        "collapse" | "" => {
                            let mut s = String::with_capacity(t.content.len());
                            for c in t.content.chars() {
                                let c = if c.is_ascii_whitespace() { ' ' } else { c };
                                if !(c == ' ' && s.ends_with(' ')) {
                                    s.push(c);
                                }
                            }
                            s
                        }
                        _ => t.content.clone(),
                    }));
                }
                NodeData::Element(e) => {
                    let Some(display) = c.display_style() else { continue };
                    if display.is_none() {
                        continue;
                    }
                    let tag = &*e.name.local;
                    if tag == "br" {
                        out.push(Piece::Text("\n".into()));
                        continue;
                    }
                    let block = display.outside() == DisplayOutside::Block || display.is_list_item();
                    let gap = match tag {
                        "p" => 2,
                        _ if block => 1,
                        _ => 0,
                    };
                    out.push(Piece::Break(gap));
                    self.inner_text_walk(c, out);
                    out.push(Piece::Break(gap));
                    let css = self.doc.resolved_style_value(c.id, "display");
                    match css.as_str() {
                        "table-cell" if c.forward(1).is_some() => out.push(Piece::Text("\t".into())),
                        "table-row" => out.push(Piece::Break(1)),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    pub(in crate::render::dom) fn parse_color(&mut self, text: &str) -> Option<[f64; 4]> {
        // currentcolor is opaque black, as before (canvas resolves it against the canvas element)
        blitz_dom::acss::parse_color(text)
    }

    pub(in crate::render::dom) fn svg_rect(&mut self, node: NodeId) -> Option<crate::render::Rect> {
        let (svg, [fx, fy, fw, fh]) = self.doc.svg_box(node)?;
        let doc = self.resolved();
        let geometry = Geometry { doc };
        let border_box: crate::render::Rect = doc.get_client_bounding_rect(svg)?.into();
        let l = doc.get_node(svg)?.final_layout();
        let (left, top) = (f64::from(l.border.left + l.padding.left), f64::from(l.border.top + l.padding.top));
        let width = f64::from(l.size.width - l.border.left - l.border.right - l.padding.left - l.padding.right);
        let height = f64::from(l.size.height - l.border.top - l.border.bottom - l.padding.top - l.padding.bottom);
        let rect = crate::render::Rect {
            x: border_box.x + left + f64::from(fx) * width,
            y: border_box.y + top + f64::from(fy) * height,
            width: f64::from(fw) * width,
            height: f64::from(fh) * height,
        };
        Some(geometry.map(svg, rect))
    }
}

fn mask(value: &str) -> String {
    "\u{2022}".repeat(value.encode_utf16().count())
}
