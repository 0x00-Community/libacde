use blitz_dom::node::{TextBrush, TextInputData};
use blitz_dom::parley::{Affinity, BoundingBox, Cursor, Layout, Selection};
use blitz_dom::{BaseDocument, Node, NodeId};

use crate::render::geometry::Geometry;
use crate::render::{Error, Rect, Result};

const LOOKAHEAD: usize = 64;

pub(crate) struct Caret<'a> {
    pub(crate) doc: &'a BaseDocument,
}

#[derive(Clone, Copy)]
struct Mark {
    byte: usize,
    node: NodeId,
    units: u32,
}

struct Inline<'a> {
    doc: &'a BaseDocument,
    root: NodeId,
    layout: &'a Layout<TextBrush>,
    origin: Rect,
    scale: f64,
    marks: Vec<Mark>,
}

impl Inline<'_> {
    fn byte_of(&self, node: NodeId, units: u32) -> usize {
        let mut own = self.marks.iter().filter(|m| m.node == node);
        let last = own.clone().next_back().map(|m| m.byte);
        own.find(|m| m.units >= units).map(|m| m.byte).or(last).unwrap_or_default()
    }

    fn position_of(&self, byte: usize) -> Option<(NodeId, u32)> {
        let i = self.marks.partition_point(|m| m.byte <= byte);
        let exact = i.checked_sub(1).and_then(|j| self.marks.get(j)).filter(|m| m.byte == byte);
        let near = exact.or_else(|| {
            let candidates = [i.checked_sub(1).and_then(|j| self.marks.get(j)), self.marks.get(i)];
            candidates.into_iter().flatten().min_by_key(|m| m.byte.abs_diff(byte))
        })?;
        Some((near.node, near.units))
    }

    fn css(&self, b: BoundingBox) -> Rect {
        let local = Rect {
            x: self.origin.x + b.x0 / self.scale,
            y: self.origin.y + b.y0 / self.scale,
            width: (b.x1 - b.x0) / self.scale,
            height: (b.y1 - b.y0) / self.scale,
        };
        Geometry { doc: self.doc }.map(self.root, local)
    }

    fn cursor(&self, byte: usize) -> Rect {
        let cursor = Cursor::from_byte_index(self.layout, byte, Affinity::Downstream);
        self.css(cursor.geometry(self.layout, self.scale as f32))
    }
}

impl<'a> Caret<'a> {
    pub(crate) fn rect(&self, node: NodeId, offset: u32) -> Result<Rect> {
        let n = self.node(node)?;
        if let Some(input) = n.element_data().and_then(|e| e.text_input_data()) {
            return self.input(n, offset, input);
        }
        if n.is_text_node() {
            let inline = self.inline_of(n)?;
            return Ok(inline.cursor(inline.byte_of(node, offset)));
        }
        let mut left = offset;
        let mut last = None;
        for id in self.text_nodes(n) {
            let len = self.utf16_len(id);
            last = Some((id, len));
            if left <= len {
                return self.rect(id, left);
            }
            left -= len;
        }
        match last {
            Some((id, len)) => self.rect(id, len),
            None => Ok(Geometry { doc: self.doc }.map(n.id, self.content_origin(n, 0.0))),
        }
    }

    pub(crate) fn position_at(&self, x: f32, y: f32) -> Option<(NodeId, u32)> {
        let hit = self.doc.hit(x, y).and_then(|h| self.doc.get_node(h.node_id));
        let root = hit.and_then(|n| n.inline_root_ancestor()).filter(|r| self.has_text(r));
        let start = hit.unwrap_or(self.doc.root_element());
        let root = root.or_else(|| self.nearest_inline_root(start, x, y))?;
        let inline = self.inline_at(root)?;
        let (x, y) = Geometry { doc: self.doc }.unmap(root.id, x.into(), y.into())?;
        let (lx, ly) = (x - inline.origin.x, y - inline.origin.y);
        let (s, layout) = (inline.scale, inline.layout);
        let byte = Selection::from_point(layout, (lx * s) as f32, (ly * s) as f32).focus().index();
        inline.position_of(byte)
    }

    pub(crate) fn text_rects(&self, node: NodeId, start: u32, end: u32) -> Result<Vec<Rect>> {
        let inline = self.inline_of(self.node(node)?)?;
        let (a, b) = (inline.byte_of(node, start.min(end)), inline.byte_of(node, start.max(end)));
        if a == b {
            return Ok(vec![inline.cursor(a)]);
        }
        let layout = inline.layout;
        let range = Selection::new(Cursor::from_byte_index(layout, a, Affinity::Downstream), Cursor::from_byte_index(layout, b, Affinity::Upstream));
        Ok(range.geometry(layout).into_iter().map(|(r, _)| inline.css(r)).collect())
    }

    pub(crate) fn layout_position(&self, node: NodeId, offset: u32) -> Option<(NodeId, usize)> {
        let inline = self.inline_of(self.node(node).ok()?).ok()?;
        Some((inline.root, inline.byte_of(node, offset)))
    }

    fn node(&self, node: NodeId) -> Result<&'a Node> {
        self.doc.get_node(node).ok_or_else(|| Error::new("node not found"))
    }

    fn input(&self, n: &Node, offset: u32, input: &TextInputData) -> Result<Rect> {
        let layout = input.editor.try_layout().ok_or_else(|| Error::new("input not laid out yet"))?;
        let byte = utf16_to_byte(input.editor.raw_text(), offset);
        let scale = self.doc.viewport().scale_f64();
        let mut origin = self.content_origin(n, n.text_input_v_centering_offset(scale));
        match input.is_multiline {
            true => origin.y -= f64::from(input.scroll_offset),
            false => origin.x -= f64::from(input.scroll_offset),
        }
        let (doc, root) = (self.doc, n.id);
        Ok(Inline { doc, root, layout, origin, scale, marks: Vec::new() }.cursor(byte))
    }

    fn inline_of(&self, text: &'a Node) -> Result<Inline<'a>> {
        let root = text.inline_root_ancestor().or_else(|| text.parent.and_then(|p| self.doc.get_node(p)?.inline_root_ancestor()));
        root.and_then(|r| self.inline_at(r)).ok_or_else(|| Error::new("text node is not laid out"))
    }

    fn inline_at(&self, root: &'a Node) -> Option<Inline<'a>> {
        let data = root.element_data()?.inline_layout_data.as_ref()?;
        Some(Inline {
            doc: self.doc,
            root: root.id,
            layout: &data.layout,
            origin: self.content_origin(root, f64::from(data.block_offset)),
            scale: self.doc.viewport().scale_f64(),
            marks: self.align(root, &data.text),
        })
    }

    fn has_text(&self, root: &Node) -> bool {
        !self.text_nodes(root).is_empty()
    }

    fn nearest_inline_root(&self, start: &'a Node, x: f32, y: f32) -> Option<&'a Node> {
        let (x, y) = (f64::from(x), f64::from(y));
        let mut best: Option<(f64, &Node)> = None;
        let mut stack = vec![start.id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else {
                continue;
            };
            let laid = n.element_data().is_some_and(|e| e.inline_layout_data.is_some());
            if n.flags.is_inline_root() && laid && self.has_text(n) {
                let geometry = Geometry { doc: self.doc };
                let visible = geometry.unmap(id, x, y).is_some();
                if let Some(r) = geometry.client_rect(id).filter(|_| visible) {
                    let dx = (r.x - x).max(x - (r.x + r.width)).max(0.0);
                    let dy = (r.y - y).max(y - (r.y + r.height)).max(0.0);
                    let d = dx * dx + dy * dy;
                    if best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, n));
                    }
                }
                continue;
            }
            stack.extend(n.children.iter().copied());
        }
        best.map(|(_, n)| n)
    }

    fn content_origin(&self, n: &Node, dy: f64) -> Rect {
        let border_box = self.doc.get_client_bounding_rect(n.id).map(Rect::from).unwrap_or_default();
        let l = n.final_layout();
        Rect {
            x: border_box.x + f64::from(l.padding.left + l.border.left),
            y: border_box.y + f64::from(l.padding.top + l.border.top) + dy,
            width: 0.0,
            height: 0.0,
        }
    }

    fn text_nodes(&self, root: &Node) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = root.children.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else {
                continue;
            };
            if n.is_text_node() {
                out.push(id);
                continue;
            }
            let hidden = n.primary_styles().is_some_and(|s| s.get_box().display.is_none());
            if hidden || n.flags.is_inline_root() {
                continue;
            }
            stack.extend(n.children.iter().rev().copied());
        }
        out
    }

    fn utf16_len(&self, id: NodeId) -> u32 {
        self.doc.tree()[id].text_data().map_or(0, |t| t.content.encode_utf16().count() as u32)
    }

    fn align(&self, root: &Node, laid: &str) -> Vec<Mark> {
        let laid: Vec<(usize, char)> = laid.char_indices().collect();
        let end = laid.last().map_or(0, |&(b, c)| b + c.len_utf8());
        let at = |i: usize| laid.get(i).map_or(end, |&(b, _)| b);
        let find = |i: usize, c: char| {
            let same = |l: char| l == c || l.to_lowercase().eq(c.to_lowercase());
            let rest = laid[i.min(laid.len())..].iter().take(LOOKAHEAD);
            rest.clone().position(|&(_, l)| same(l)).map_or(i, |skip| i + skip)
        };
        let mut marks = Vec::new();
        let mut i = 0;
        for node in self.text_nodes(root) {
            let Some(text) = self.doc.tree()[node].text_data() else {
                continue;
            };
            let mut units = 0;
            for c in text.content.chars() {
                let ws = c.is_whitespace();
                let before = if ws { i } else { find(i, c) };
                marks.push(Mark { byte: at(before), node, units });
                units += c.len_utf16() as u32;
                if ws {
                    if laid.get(i).is_some_and(|(_, l)| l.is_whitespace()) {
                        i += 1;
                    }
                } else {
                    i = before + 1;
                }
            }
            marks.push(Mark { byte: at(i), node, units });
        }
        marks
    }
}

pub(crate) fn utf16_to_byte(text: &str, offset: u32) -> usize {
    let mut units = 0;
    for (byte, c) in text.char_indices() {
        if units >= offset {
            return byte;
        }
        units += c.len_utf16() as u32;
    }
    text.len()
}
