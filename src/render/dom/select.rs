use blitz_dom::node::{SelectItemKind, SelectPicker};
use blitz_dom::style_dom::ElementState;
use blitz_dom::{NodeId, local_name, ns};

use crate::render::engine::Engine;
use crate::render::geometry::Geometry;

const PICKER_PADDING: f64 = 4.0;
const PICKER_ROW_EXTRA: f64 = 6.0;
const PICKER_GAP: f64 = 2.0;
const PICKER_MARGIN: f64 = 4.0;
const PICKER_TEXT_INSET: f64 = 10.0;

impl Engine {
    fn is_html(&self, node: NodeId, name: &str) -> bool {
        self.doc.get_node(node).and_then(|n| n.element_data()).is_some_and(|e| e.name.ns == ns!(html) && *e.name.local == *name)
    }

    fn children_of(&self, node: NodeId) -> Vec<NodeId> {
        self.doc.get_node(node).map(|n| n.children.to_vec()).unwrap_or_default()
    }

    pub(super) fn is_select(&self, node: NodeId) -> bool {
        self.is_html(node, "select")
    }

    pub(super) fn options(&self, select: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        for child in self.children_of(select) {
            if self.is_html(child, "option") {
                out.push(child);
            } else if self.is_html(child, "optgroup") {
                out.extend(self.children_of(child).into_iter().filter(|&c| self.is_html(c, "option")));
            }
        }
        out
    }

    pub(super) fn list_owner(&self, parent: NodeId) -> Option<NodeId> {
        if self.is_select(parent) {
            return Some(parent);
        }
        if !self.is_html(parent, "optgroup") {
            return None;
        }
        self.doc.get_node(parent).and_then(|p| p.parent).filter(|&g| self.is_select(g))
    }

    pub(super) fn owner_select(&self, option: NodeId) -> Option<NodeId> {
        self.doc.get_node(option).and_then(|o| o.parent).and_then(|p| self.list_owner(p))
    }

    fn has(&self, node: NodeId, name: blitz_dom::LocalName) -> bool {
        self.doc.get_node(node).is_some_and(|n| n.attr(name).is_some())
    }

    fn multiple(&self, select: NodeId) -> bool {
        self.has(select, local_name!("multiple"))
    }

    pub(super) fn display_size(&self, select: NodeId) -> (u32, bool) {
        let size = self.doc.get_node(select).and_then(|n| n.attr(local_name!("size")));
        blitz_dom::select_mode(self.multiple(select), size)
    }

    pub(super) fn selectedness(&self, option: NodeId) -> bool {
        match self.ids.forms.get(&option).and_then(|f| f.selected) {
            Some(selected) => selected,
            None => self.has(option, local_name!("selected")),
        }
    }

    pub(super) fn option_disabled(&self, option: NodeId) -> bool {
        self.has(option, local_name!("disabled"))
            || self.doc.get_node(option).and_then(|o| o.parent).is_some_and(|p| self.is_html(p, "optgroup") && self.has(p, local_name!("disabled")))
    }

    fn store_selectedness(&mut self, option: NodeId, selected: bool, dirty: bool) {
        let form = self.ids.forms.entry(option).or_default();
        form.selected = Some(selected);
        form.dirty |= dirty;
        self.show_selectedness(option);
    }

    pub(super) fn show_selectedness(&mut self, option: NodeId) {
        let selected = self.selectedness(option);
        let shown = self.doc.get_node(option).and_then(|n| n.element_data()).is_some_and(|e| e.element_state.contains(ElementState::CHECKED));
        if selected == shown {
            return;
        }
        self.doc.snapshot_node_and(option, ElementState::CHECKED, |n| {
            if let Some(e) = n.element_data_mut() {
                e.element_state.set(ElementState::CHECKED, selected);
            }
            n.mark_ancestors_dirty();
        });
        if let Some(select) = self.owner_select(option) {
            self.doc.rebuild_select(select);
        }
    }

    pub(super) fn settle_select(&mut self, select: NodeId, chosen: Option<NodeId>) {
        let options = self.options(select);
        for &option in &options {
            self.show_selectedness(option);
        }
        self.doc.rebuild_select(select);
        if self.multiple(select) {
            return;
        }
        let selected: Vec<NodeId> = options.iter().copied().filter(|&o| self.selectedness(o)).collect();
        let keep = chosen.filter(|c| selected.contains(c)).or_else(|| selected.last().copied());
        for option in selected.into_iter().filter(|&o| Some(o) != keep) {
            self.store_selectedness(option, false, false);
        }
        if keep.is_none()
            && self.display_size(select).0 == 1
            && let Some(first) = options.into_iter().find(|&o| !self.option_disabled(o))
        {
            self.store_selectedness(first, true, false);
        }
    }

    pub(super) fn settle_selects_in(&mut self, root: NodeId) {
        for node in self.subtree(root) {
            if self.is_html(node, "option") {
                self.show_selectedness(node);
            } else if self.is_select(node) {
                self.settle_select(node, None);
            }
        }
    }

    pub(super) fn last_selected_option(&self, nodes: &[NodeId]) -> Option<NodeId> {
        nodes
            .iter()
            .flat_map(|&n| match self.is_html(n, "optgroup") {
                true => self.children_of(n),
                false => vec![n],
            })
            .filter(|&n| self.is_html(n, "option") && self.selectedness(n))
            .last()
    }

    pub(super) fn lists_left(&self, nodes: &[NodeId]) -> Vec<NodeId> {
        let mut owners: Vec<NodeId> = nodes.iter().filter_map(|&n| self.doc.get_node(n).and_then(|n| n.parent)).filter_map(|p| self.list_owner(p)).collect();
        owners.dedup();
        owners
    }

    pub(super) fn lists_changed(&mut self, parent: NodeId, added: &[NodeId], left: Vec<NodeId>) {
        let into = self.list_owner(parent);
        for select in left {
            if Some(select) != into && self.doc.get_node(select).is_some() {
                self.settle_select(select, None);
            }
        }
        if let Some(select) = into {
            let chosen = self.last_selected_option(added);
            self.settle_select(select, chosen);
        }
    }

    pub(super) fn select_attribute_changed(&mut self, node: NodeId, name: &str) {
        if self.is_html(node, "option") {
            let owner = self.owner_select(node);
            if name == "selected" && !self.ids.forms.get(&node).is_some_and(|f| f.dirty) {
                if let Some(form) = self.ids.forms.get_mut(&node) {
                    form.selected = None;
                }
                self.show_selectedness(node);
                if let Some(select) = owner {
                    let chosen = self.selectedness(node).then_some(node);
                    self.settle_select(select, chosen);
                }
            } else if let Some(select) = owner {
                self.doc.rebuild_select(select);
            }
        } else if self.is_html(node, "optgroup") {
            if let Some(select) = self.doc.get_node(node).and_then(|n| n.parent).and_then(|p| self.list_owner(p)) {
                self.doc.rebuild_select(select);
            }
        } else if self.is_select(node) && matches!(name, "multiple" | "size") {
            self.settle_select(node, None);
            if self.picker.as_ref().is_some_and(|p| p.select == node) && self.display_size(node).1 {
                self.picker = None;
            }
        }
    }

    pub(super) fn set_option_selected(&mut self, option: NodeId, selected: bool) {
        self.store_selectedness(option, selected, true);
        if let Some(select) = self.owner_select(option) {
            self.settle_select(select, selected.then_some(option));
        }
    }

    pub(super) fn selected_index(&self, select: NodeId) -> i32 {
        if !self.is_select(select) {
            return -1;
        }
        self.options(select).into_iter().position(|o| self.selectedness(o)).map_or(-1, |i| i as i32)
    }

    pub(super) fn set_selected_index(&mut self, select: NodeId, index: i32) {
        let options = self.options(select);
        let chosen = usize::try_from(index).ok().and_then(|i| options.get(i).copied());
        for &option in &options {
            if Some(option) != chosen && self.selectedness(option) {
                self.store_selectedness(option, false, false);
            }
        }
        if let Some(option) = chosen {
            self.store_selectedness(option, true, true);
        }
        self.doc.rebuild_select(select);
    }

    pub(super) fn reset_select(&mut self, select: NodeId) {
        for option in self.options(select) {
            if let Some(form) = self.ids.forms.get_mut(&option) {
                form.selected = None;
                form.dirty = false;
            }
            self.show_selectedness(option);
        }
        self.settle_select(select, None);
    }

    pub(super) fn list_box_rows(&self, select: NodeId) -> Vec<(NodeId, usize, crate::render::Rect)> {
        self.rows_of(select, true)
    }

    pub(super) fn option_row(&self, option: NodeId) -> Option<crate::render::Rect> {
        let select = self.owner_select(option)?;
        self.rows_of(select, false).into_iter().find(|(o, _, _)| *o == option).map(|(_, _, r)| r)
    }

    pub(super) fn hit_option(&self, node: NodeId, x: f64, y: f64) -> NodeId {
        if !self.is_select(node) {
            return node;
        }
        let inside = |r: &crate::render::Rect| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
        self.list_box_rows(node).into_iter().find(|(_, _, r)| inside(r)).map_or(node, |(o, _, _)| o)
    }

    fn rows_of(&self, select: NodeId, clip: bool) -> Vec<(NodeId, usize, crate::render::Rect)> {
        let doc = &self.doc;
        let Some(node) = doc.get_node(select) else { return Vec::new() };
        let Some(data) = node.element_data().and_then(|e| e.select_data()).filter(|d| d.list_box) else { return Vec::new() };
        let Some(bounds) = doc.get_client_bounding_rect(select) else { return Vec::new() };
        let layout = node.final_layout();
        let (border, padding) = (layout.border, layout.padding);
        let top = bounds.y + f64::from(border.top);
        let bottom = bounds.y + bounds.height - f64::from(border.bottom);
        let x = bounds.x + f64::from(border.left);
        let width = (bounds.width - f64::from(border.left + border.right)).max(0.0);
        let row = f64::from(data.row_height);
        let first = top + f64::from(padding.top) - node.scroll_offset().y;
        let geometry = Geometry { doc };
        let mut rows = Vec::new();
        for (index, (item, option)) in data.options().enumerate() {
            let (mut y0, mut y1) = (first + item as f64 * row, first + (item + 1) as f64 * row);
            if clip {
                (y0, y1) = (y0.max(top), y1.min(bottom));
                if y1 - y0 < 0.5 {
                    continue;
                }
            }
            rows.push((option.node, index, geometry.map(select, crate::render::Rect { x, y: y0, width, height: y1 - y0 })));
        }
        rows
    }

    pub(super) fn list_box_option_at(&mut self, select: NodeId, x: f64, y: f64) -> i32 {
        self.resolved();
        let inside = |r: &crate::render::Rect| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
        self.list_box_rows(select).into_iter().find(|(_, _, r)| inside(r)).map_or(-1, |(_, index, _)| index as i32)
    }

    pub(super) fn open_picker(&mut self, select: NodeId) -> bool {
        let doc = self.resolved();
        let Some(data) = doc.get_node(select).and_then(|n| n.element_data()).and_then(|e| e.select_data()).filter(|d| !d.list_box) else {
            return false;
        };
        let active = data.shown;
        let row_height = f64::from(data.row_height) + PICKER_ROW_EXTRA;
        self.picker = Some(SelectPicker { select, rect: kurbo::Rect::ZERO, padding: PICKER_PADDING, row_height, scroll: 0.0, active });
        if !self.place_picker() {
            self.picker = None;
            return false;
        }
        if let Some(item) = active {
            self.reveal_picker(item);
        }
        self.touch();
        true
    }

    pub(crate) fn close_picker(&mut self) {
        if self.picker.take().is_some() {
            self.touch();
        }
    }

    pub(crate) fn place_picker(&mut self) -> bool {
        let Some(select) = self.picker.as_ref().map(|p| p.select) else { return false };
        let placed = self.connected(select).then(|| self.picker_anchor(select)).flatten();
        let Some((anchor, (vw, vh), rows, label)) = placed else {
            self.picker = None;
            return false;
        };
        let picker = self.picker.as_mut().expect("checked above");
        let needed = rows as f64 * picker.row_height + 2.0 * picker.padding;
        let width = anchor.width.max(label).min(vw);
        let x = anchor.x.clamp(0.0, (vw - width).max(0.0));
        let top = anchor.y + anchor.height + PICKER_GAP;
        let below = vh - top - PICKER_MARGIN;
        let above = anchor.y - PICKER_GAP - PICKER_MARGIN;
        let (y, height) = if needed <= below || below >= above {
            (top, needed.min(below.max(picker.row_height + 2.0 * picker.padding)))
        } else {
            let height = needed.min(above);
            (anchor.y - PICKER_GAP - height, height)
        };
        picker.rect = kurbo::Rect::new(x, y, x + width, y + height);
        picker.scroll = picker.scroll.clamp(0.0, picker.max_scroll(rows));
        true
    }

    fn picker_anchor(&mut self, select: NodeId) -> Option<(crate::render::Rect, (f64, f64), usize, f64)> {
        let doc = self.resolved();
        let geometry = Geometry { doc };
        let node = doc.get_node(select).filter(|n| !geometry.hidden(n))?;
        let data = node.element_data().and_then(|e| e.select_data()).filter(|d| !d.list_box)?;
        let anchor = geometry.client_rect(select)?;
        let viewport = doc.viewport();
        let scale = viewport.scale_f64();
        let size = (f64::from(viewport.window_size.0) / scale, f64::from(viewport.window_size.1) / scale);
        Some((anchor, size, data.items.len(), f64::from(data.label_width) + 2.0 * PICKER_TEXT_INSET))
    }

    fn picker_rows(&self) -> Option<(usize, &blitz_dom::node::SelectData)> {
        let picker = self.picker.as_ref().filter(|p| self.connected(p.select))?;
        let data = self.doc.get_node(picker.select)?.element_data()?.select_data()?;
        Some((data.items.len(), data))
    }

    fn reveal_picker(&mut self, item: usize) {
        let Some((rows, _)) = self.picker_rows() else { return };
        if let Some(picker) = self.picker.as_mut() {
            picker.reveal(rows, item);
        }
    }

    pub(super) fn picker_rects(&self, select: NodeId) -> Vec<(NodeId, crate::render::Rect)> {
        let Some((_, data)) = self.picker_rows() else { return Vec::new() };
        let picker = self.picker.as_ref().expect("picker_rows checked it");
        if picker.select != select {
            return Vec::new();
        }
        let first = picker.rect.y0 + picker.padding - picker.scroll;
        let mut rows = Vec::new();
        for (item, option) in data.options() {
            let y0 = (first + item as f64 * picker.row_height).max(picker.rect.y0);
            let y1 = (first + (item + 1) as f64 * picker.row_height).min(picker.rect.y1);
            if y1 - y0 >= 0.5 {
                rows.push((option.node, crate::render::Rect { x: picker.rect.x0, y: y0, width: picker.rect.width(), height: y1 - y0 }));
            }
        }
        rows
    }

    pub(crate) fn picker_option_at(&mut self, x: f64, y: f64) -> i32 {
        self.resolved();
        let Some((rows, data)) = self.picker_rows() else { return -2 };
        let picker = self.picker.as_ref().expect("picker_rows checked it");
        match picker.item_at(rows, x, y) {
            Err(()) => -2,
            Ok(Some(item)) if data.items[item].kind == SelectItemKind::Option => data.option_of_item(item).map_or(-1, |o| o as i32),
            Ok(_) => -1,
        }
    }

    pub(super) fn picker_state(&self) -> [f64; 3] {
        let Some((_, data)) = self.picker_rows() else { return [-1.0, -1.0, 0.0] };
        let picker = self.picker.as_ref().expect("picker_rows checked it");
        let active = picker.active.and_then(|i| data.option_of_item(i)).map_or(-1.0, |o| o as f64);
        let page = ((picker.rect.height() - 2.0 * picker.padding) / picker.row_height).floor().max(1.0);
        [self.ids.handle(picker.select).map_or(-1.0, f64::from), active, page]
    }

    pub(super) fn set_picker_active(&mut self, option: i32) {
        let Some((_, data)) = self.picker_rows() else { return };
        let item = usize::try_from(option).ok().and_then(|o| data.item_of_option(o));
        if self.picker.as_ref().is_some_and(|p| p.active == item) {
            return;
        }
        if let Some(picker) = self.picker.as_mut() {
            picker.active = item;
        }
        if let Some(item) = item {
            self.reveal_picker(item);
        }
        self.touch();
    }

    pub(crate) fn scroll_picker(&mut self, dy: f64) {
        let Some((rows, _)) = self.picker_rows() else { return };
        let Some(picker) = self.picker.as_mut() else { return };
        let scroll = (picker.scroll + dy).clamp(0.0, picker.max_scroll(rows));
        if scroll != picker.scroll {
            picker.scroll = scroll;
            self.touch();
        }
    }
}
