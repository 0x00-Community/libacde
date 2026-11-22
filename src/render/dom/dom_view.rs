use super::*;

impl Dom {
    pub fn value(&self, id: u32) -> Result<String> {
        self.on(id, |e, n| e.value(n))
    }

    pub fn set_value(&self, id: u32, value: &str) -> Result<()> {
        self.edit(id, |e, n| e.set_value(id, n, value)).flatten()
    }

    pub fn checked(&self, id: u32) -> Result<bool> {
        self.on(id, |e, n| e.checked(n))
    }

    pub fn set_checked(&self, id: u32, checked: bool) -> Result<()> {
        self.edit(id, |e, n| e.set_checked(n, checked))
    }

    pub fn selected_index(&self, id: u32) -> Result<i32> {
        self.on(id, |e, n| e.selected_index(n))
    }

    pub fn set_selected_index(&self, id: u32, index: i32) -> Result<()> {
        self.edit(id, |e, node| e.set_selected_index(node, index))
    }

    pub fn options(&self, id: u32) -> Result<Vec<u32>> {
        self.read(|e| {
            let node = e.n(id)?;
            let options = e.options(node);
            Ok(e.outs(options))
        })
    }

    pub fn option_selected(&self, id: u32) -> Result<bool> {
        self.on(id, |e, n| e.selectedness(n))
    }

    pub fn set_option_selected(&self, id: u32, selected: bool) -> Result<()> {
        self.edit(id, |e, n| e.set_option_selected(n, selected))
    }

    pub fn reset_select(&self, id: u32) -> Result<()> {
        self.edit(id, |e, n| e.reset_select(n))
    }

    pub fn list_box_option_at(&self, id: u32, x: f64, y: f64) -> Result<i32> {
        self.on(id, |e, n| e.list_box_option_at(n, x, y))
    }

    pub fn open_picker(&self, id: u32) -> Result<bool> {
        self.write(|e| {
            let node = e.n(id)?;
            Ok(e.open_picker(node))
        })
    }

    pub fn close_picker(&self) {
        let _ = self.update(|e| e.close_picker());
    }

    pub fn picker_state(&self) -> [f64; 3] {
        self.read(|e| Ok(e.picker_state())).unwrap_or([-1.0, -1.0, 0.0])
    }

    pub fn picker_option_at(&self, x: f64, y: f64) -> i32 {
        self.read(|e| Ok(e.picker_option_at(x, y))).unwrap_or(-2)
    }

    pub fn picker_option_rect(&self, id: u32) -> Result<Option<Quad>> {
        self.on(id, |e, option| {
            let select = e.owner_select(option)?;
            e.picker_rects(select).into_iter().find(|(o, _)| *o == option).map(|(_, r)| quad(r))
        })
    }

    pub fn set_picker_active(&self, option: i32) {
        let _ = self.update(|e| e.set_picker_active(option));
    }

    pub fn set_selection_range(&self, id: u32, start: u32, end: u32) -> Result<()> {
        self.edit(id, |e, n| e.set_selection_range(n, start, end))
    }

    pub fn set_focus(&self, id: u32, visible: bool) -> Result<()> {
        self.write(|e| e.maybe(id).map(|n| e.set_focus(n, visible)))
    }

    pub fn set_open_state(&self, id: u32, modal: bool, open: bool) -> Result<()> {
        self.write(|e| {
            let node = e.n(id)?;
            let state = if modal { ElementState::MODAL } else { ElementState::POPOVER_OPEN };
            e.doc.change_state(node, |n| {
                if let Some(data) = n.element_data_mut() {
                    data.element_state.set(state, open);
                }
            });
            Ok(())
        })
    }

    pub fn set_active(&self, id: u32) -> Result<()> {
        self.write(|e| e.maybe(id).map(|n| e.set_active(n)))
    }

    pub fn set_selection(&self, anchor: u32, anchor_offset: u32, focus: u32, focus_offset: u32) -> Result<()> {
        self.write(|e| {
            if anchor == NONE || focus == NONE {
                e.doc.clear_text_selection();
                return Ok(());
            }
            let (anchor, focus) = (e.n(anchor)?, e.n(focus)?);
            e.set_selection(anchor, anchor_offset, focus, focus_offset);
            Ok(())
        })
    }

    pub fn rect(&self, id: u32) -> Result<Quad> {
        self.on(id, |e, n| {
            let rect = Geometry { doc: e.resolved() }.client_rect(n);
            e.option_row(n).or_else(|| e.svg_rect(n)).or(rect).map_or([0.0; 4], quad)
        })
    }

    pub fn client_rects(&self, id: u32) -> Result<Vec<Quad>> {
        self.on(id, |e, node| {
            e.resolved();
            if let Some(row) = e.option_row(node).or_else(|| e.svg_rect(node)) {
                return vec![quad(row)];
            }
            let Some(geometry) = shown(&e.doc, node) else { return Vec::new() };
            geometry.doc.node_client_rects(node).into_iter().map(|r| quad(geometry.map(node, r.into()))).collect()
        })
    }

    pub fn metrics(&self, id: u32) -> Result<Option<Metrics>> {
        self.on(id, Engine::dom_metrics)
    }

    pub fn hit_test(&self, x: f64, y: f64) -> Option<u32> {
        self.read(|e| {
            let hit = e.resolved().element_from_point(x as f32, y as f32);
            Ok(hit.and_then(|n| e.element_of(n)).map(|node| e.hit_option(node, x, y)).map(|node| e.ids.id_of(node)))
        })
        .ok()
        .flatten()
    }

    pub fn hit_test_all(&self, x: f64, y: f64) -> Vec<u32> {
        self.read(|e| {
            let hits = e.resolved().elements_from_point(x as f32, y as f32);
            let mut nodes: Vec<NodeId> = Vec::new();
            for hit in hits {
                if let Some(node) = e.element_of(hit)
                    && !nodes.contains(&node)
                {
                    let option = e.hit_option(node, x, y);
                    if option != node {
                        nodes.push(option);
                    }
                    nodes.push(node);
                }
            }
            Ok(e.outs(nodes))
        })
        .unwrap_or_default()
    }

    pub fn computed_style(&self, id: u32, pseudo: &str, property: &str) -> Result<String> {
        self.on(id, |e, node| {
            e.resolved();
            e.styled(node, pseudo).map(|n| e.doc.resolved_style_value(n, property)).unwrap_or_default()
        })
    }

    pub fn css_supports(&self, condition: &str) -> Result<bool> {
        self.read(|e| Ok(e.doc.css_supports_condition(condition)))
    }

    /// Whether the engine has the CSS property `name` (not a custom property): the attributes a
    /// `CSSStyleDeclaration` exposes
    pub fn css_property_supported(&self, name: &str) -> bool {
        blitz_dom::css_property_is_supported(name)
    }

    pub fn css_supports_declaration(&self, property: &str, value: &str) -> Result<bool> {
        self.read(|e| Ok(e.doc.css_supports_declaration(property, value)))
    }

    pub fn computed_style_all(&self, id: u32, pseudo: &str) -> Result<Vec<String>> {
        self.on(id, |e, node| {
            e.resolved();
            let Some(target) = e.styled(node, pseudo) else { return Vec::new() };
            blitz_dom::resolved_style_property_names().iter().flat_map(|&name| [name.to_owned(), e.doc.resolved_style_value(target, name)]).collect()
        })
    }

    pub fn caret_rect(&self, id: u32, offset: u32) -> Result<Quad> {
        self.on(id, |e, n| Caret { doc: e.resolved() }.rect(n, offset).map_or([0.0; 4], quad))
    }

    pub fn caret_from_point(&self, x: f64, y: f64) -> Option<CaretPosition> {
        self.read(|e| {
            let found = Caret { doc: e.resolved() }.position_at(x as f32, y as f32);
            Ok(found.map(|(node, offset)| CaretPosition { node: e.ids.id_of(node), offset }))
        })
        .ok()
        .flatten()
    }

    pub fn text_rects(&self, id: u32, start: u32, end: u32) -> Result<Vec<Quad>> {
        self.on(id, |e, n| Caret { doc: e.resolved() }.text_rects(n, start, end).map(|rects| rects.into_iter().map(quad).collect()).unwrap_or_default())
    }

    pub fn match_media(&self, query: &str) -> bool {
        self.read(|e| Ok(e.match_media(query))).unwrap_or(false)
    }

    pub fn scroll_to(&self, id: u32, left: f64, top: f64, smooth: bool) -> Result<()> {
        self.write(|e| e.n(id).map(|n| e.scroll_to(n, left, top, smooth)))
    }

    pub fn take_scrolled(&self) -> Vec<u32> {
        self.read(|e| Ok(e.take_scrolled())).unwrap_or_default()
    }

    pub fn image_state(&self, id: u32) -> Result<[f64; 3]> {
        self.on(id, |e, node| {
            e.doc.handle_messages();
            let (width, height, state, _) = Engine::image(&e.doc, node);
            let state = if state == NO_SRC { FAILED } else { state };
            [width, height, f64::from(state)]
        })
    }

    pub fn take_loaded(&self) -> Vec<u32> {
        self.read(|e| Ok(e.take_loaded())).unwrap_or_default()
    }

    /// The page listens for CSS animation and transition events: from now on the engine keeps them
    /// for `take_animation_events`
    pub fn want_animation_events(&self) -> Result<()> {
        self.read(|e| {
            e.doc.want_animation_events();
            Ok(())
        })
    }

    pub fn take_animation_events(&self) -> AnimationEvents {
        self.read(|e| Ok(e.take_animation_events())).unwrap_or_default()
    }

    pub fn sheet_state(&self, id: u32) -> Result<u8> {
        self.on(id, |e, node| {
            e.doc.handle_messages();
            match e.doc.stylesheet_load(node) {
                Some(blitz_dom::net::SheetLoadState::Loading) => LOADING,
                Some(blitz_dom::net::SheetLoadState::Loaded) => LOADED,
                Some(blitz_dom::net::SheetLoadState::Failed) => FAILED,
                None => NO_SRC,
            }
        })
    }

    pub fn sheet_url(&self, id: u32) -> Result<Option<String>> {
        self.on(id, |e, node| e.doc.stylesheet_source(node).map(|(url, _, _)| url.to_owned()))
    }

    pub fn sheet_opaque(&self, id: u32) -> Result<bool> {
        self.on(id, |e, node| e.doc.stylesheet_source(node).is_some_and(|(_, opaque, _)| opaque))
    }

    pub fn sheet_serial(&self, id: u32) -> Result<Option<u32>> {
        self.on(id, |e, node| e.doc.stylesheet_source(node).map(|(_, _, request)| request as u32))
    }

    pub fn canvas(&self, id: u32, commands: &[u8]) -> Result<()> {
        self.write_paint(id, |e| {
            e.n(id)?;
            e.tree().canvas(id, commands).map_err(|err| fail("InvalidStateError", err))
        })
    }

    pub fn canvas_image(&self, id: u32, image: Option<peniko::ImageData>) -> Result<()> {
        self.write_paint(id, |e| {
            e.n(id)?;
            e.tree().canvas_image(id, image).map_err(|err| fail("InvalidStateError", err))
        })
    }

    pub fn video_image(&self, id: u32, image: Option<peniko::ImageData>) -> Result<()> {
        self.touched();
        self.shared.script_call(&self.wake, |engine| {
            engine.uncommitted = true;
            let (stale, dirty) = (engine.stale, engine.dirty);
            let resized = engine.n(id).and_then(|_| engine.tree().video_image(id, image).map_err(|err| fail("InvalidStateError", err)));
            let shown = engine.ids.node(id).is_ok_and(|n| engine.connected(n));
            engine.stale = stale || (shown && matches!(resized, Ok(true)));
            engine.dirty = dirty || shown;
            resized.map(|_| ())
        })
    }

    pub fn image_pixels(&self, id: u32) -> Result<Option<crate::render::Pixels>> {
        self.on(id, |e, node| e.image_pixels(id, node))
    }

    pub fn measure_text(&self, id: u32, font: &str, text: &str) -> Result<TextMetrics> {
        self.on(id, |e, _| Canvas::measure(&mut e.doc, font, text))
    }

    pub fn to_data_url(&self, id: u32, _type: &str) -> Result<String> {
        self.on(id, |e, node| e.with_canvas(id, node, |c| c.data_url().unwrap_or_else(|| "data:,".into())))
    }

    pub fn get_image_data(&self, id: u32, x: f64, y: f64, width: f64, height: f64) -> Result<Vec<u8>> {
        self.on(id, |e, node| {
            if ![x, y, width, height].iter().all(|v| v.is_finite()) || (width * height).abs() > f64::from(crate::render::canvas::MAX_SIDE).powi(2) {
                return Err(fail("IndexSizeError", "getImageData: bad rectangle"));
            }
            Ok(e.with_canvas(id, node, |c| c.image_data(kurbo::Rect::new(x, y, x + width, y + height))))
        })
        .flatten()
    }

    pub fn capture(&self, id: u32, scale: f64) -> Result<Option<Capture>> {
        self.on(id, |e, n| Capture::of(e.resolved(), n, scale))
    }

    pub fn set_panel(&self, id: u32, rect: Option<[f64; 4]>) -> Result<()> {
        self.write(|e| e.n(id).map(|n| e.set_panel(n, id, rect)))
    }

    pub fn pin(&self, id: u32) -> Result<()> {
        self.on(id, |e, n| e.ids.pin(n, true))
    }

    pub fn unpin(&self, id: u32) -> Result<()> {
        self.touched();
        self.on(id, |e, n| {
            e.ids.pin(n, false);
            e.ids.unpinned.push(n);
        })
    }

    pub fn pinned_in(&self, root: u32) -> Result<Vec<u32>> {
        self.on(root, |e, root| e.subtree(root).into_iter().filter(|&n| e.ids.pinned(n)).filter_map(|n| e.ids.handle(n)).collect())
    }

    pub fn input(&self, trace: crate::log::Trace) {
        _ = self.read(|e| {
            e.input = crate::log::Trace::earliest(e.input, Some(trace));
            Ok(())
        });
    }

    pub fn commit(&self) {
        self.shared.tasks.fetch_add(1, Ordering::AcqRel);
        if !self.shared.touched.swap(false, Ordering::Relaxed) && self.shared.commits.load(Ordering::Relaxed) != 0 {
            return;
        }
        let committed = self.read(|e| {
            let mut candidates = std::mem::take(&mut e.ids.fresh);
            candidates.append(&mut e.ids.unpinned);
            e.collect(&candidates);
            let changed = std::mem::take(&mut e.uncommitted);
            self.shared.requeue(std::mem::take(&mut e.held), &self.wake);
            let now = crate::log::now_ms();
            if !changed && e.committed != 0 {
                e.input = e.input.filter(|t| now - t.input < INPUT_TTL_MS);
                return Ok(false);
            }
            let seq = self.shared.commits.fetch_add(1, Ordering::Relaxed) + 1;
            e.committed = e.committed.max(seq);
            e.trace = crate::log::Trace::earliest(
                e.trace,
                e.input.take().filter(|t| now - t.input < INPUT_TTL_MS).map(|t| crate::log::Trace { committed: now, ..t }),
            );
            e.want_present = true;
            Ok(true)
        });
        if committed.unwrap_or(false) {
            _ = self.wake.send(());
        }
    }

    pub fn stats(&self) -> Stats {
        self.read(|e| Ok(e.stats())).unwrap_or_default()
    }

    pub fn reset_timings(&self) {
        _ = self.read(|e| {
            e.reset_timings();
            Ok(())
        });
    }

    pub fn describe(&self, id: u32) -> Result<(NodeType, Ns, bool, String)> {
        self.on(id, |e, node| {
            let node_type = e.kind(node);
            let (namespace, local_name) = match node_type {
                NodeType::Element => match e.doc.get_node(node).and_then(|n| n.element_data()) {
                    Some(data) => (ns_of(&data.name), data.name.local.to_string()),
                    None => (Ns::Html, String::new()),
                },
                NodeType::Document => match e.ids.documents.get(&node) {
                    Some(info) => (Ns::Html, info.interface.to_owned()),
                    None => (Ns::Html, String::new()),
                },
                _ => (Ns::Html, String::new()),
            };
            let connected = e.connected(node);
            e.ids.pin(node, true);
            (node_type, namespace, connected, local_name)
        })
    }

    pub fn parse_inert(&self, html: &str, url: &str) -> Result<u32> {
        self.create(|e| {
            let holder = e.new_fragment();
            e.ids.documents.insert(holder, DocumentInfo::new("html", "text/html", url));
            let parsed = {
                let mut tree = e.tree();
                crate::render::parse::document(&mut tree.m, holder, html)
            };
            e.adopt(parsed);
            e.adopt_into(holder, Some(holder));
            e.register(holder);
            Ok(holder)
        })
    }

    pub fn parse_color(&self, color: &str) -> Option<[f64; 4]> {
        self.read(|e| Ok(e.parse_color(color))).ok().flatten()
    }

    pub fn sheet_rule_count(&self, id: u32) -> Result<Option<u32>> {
        self.on(id, |e, node| e.connected(node).then(|| e.doc.stylesheet_rule_count(node, &[])).flatten().map(|n| n as u32))
    }

    pub fn sheet_rule(&self, id: u32, index: u32) -> Result<String> {
        self.read(|e| {
            e.doc.stylesheet_rule_info(e.n(id)?, &[index as usize]).map(|info| info.css_text).ok_or_else(|| fail("IndexSizeError", format!("no rule {index}")))
        })
    }

    pub fn sheet_insert_rule(&self, id: u32, rule: &str, index: u32) -> Result<u32> {
        self.write(|e| {
            e.doc
                .stylesheet_insert_rule(e.n(id)?, &[], rule, index as usize)
                .map(|at| at as u32)
                .map_err(|err| fail(err.dom_exception_name(), format!("cannot insert '{rule}' at {index}")))
        })
    }

    pub fn sheet_disabled(&self, id: u32) -> Result<bool> {
        self.on(id, |e, node| e.doc.stylesheet_disabled(node))
    }

    pub fn set_sheet_disabled(&self, id: u32, disabled: bool) -> Result<()> {
        self.write(|e| {
            let node = e.n(id)?;
            e.doc.set_stylesheet_disabled(node, disabled);
            Ok(())
        })
    }

    pub fn sheet_delete_rule(&self, id: u32, index: u32) -> Result<()> {
        self.write(|e| {
            e.doc.stylesheet_delete_rule(e.n(id)?, &[], index as usize).map_err(|err| fail(err.dom_exception_name(), format!("cannot delete rule {index}")))
        })
    }
}
