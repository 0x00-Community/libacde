use blitz_dom::{BaseDocument, Node, NodeData, NodeId};
use kurbo::Affine;

use crate::render::Rect;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub offset_left: f64,
    pub offset_top: f64,
    pub offset_width: f64,
    pub offset_height: f64,
    pub client_left: f64,
    pub client_top: f64,
    pub client_width: f64,
    pub client_height: f64,
    pub scroll_left: f64,
    pub scroll_top: f64,
    pub scroll_width: f64,
    pub scroll_height: f64,
    pub offset_parent: Option<u32>,
}

pub(crate) struct Geometry<'a> {
    pub(crate) doc: &'a BaseDocument,
}

impl Geometry<'_> {
    fn css_transform(&self, n: &Node) -> Option<Affine> {
        if !matches!(n.data, NodeData::Element(_) | NodeData::AnonymousBlock(_)) {
            return None;
        }
        let scale = self.doc.viewport().scale_f64();
        n.transform().as_deref().copied().map(|t| {
            let [a, b, c, d, e, f] = t.as_coeffs();
            Affine::new([a, b, c, d, e / scale, f / scale])
        })
    }

    pub(crate) fn matrix(&self, node: NodeId) -> Affine {
        let mut m = Affine::IDENTITY;
        let mut current = self.doc.get_node(node);
        while let Some(n) = current {
            if let Some(t) = self.css_transform(n)
                && let Some(origin) = self.doc.get_client_bounding_rect(n.id)
            {
                m = Affine::translate((origin.x, origin.y)) * t * Affine::translate((-origin.x, -origin.y)) * m;
            }
            current = n.containing_block().and_then(|p| self.doc.get_node(p));
        }
        m
    }

    pub(crate) fn map(&self, node: NodeId, rect: Rect) -> Rect {
        let r = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
        self.matrix(node).transform_rect_bbox(r).into()
    }

    pub(crate) fn unmap(&self, node: NodeId, x: f64, y: f64) -> Option<(f64, f64)> {
        let m = self.matrix(node);
        let det = m.determinant();
        if det.abs() < 1e-12 || !det.is_finite() {
            return None;
        }
        let p = m.inverse() * kurbo::Point::new(x, y);
        Some((p.x, p.y))
    }

    pub(crate) fn client_rect(&self, node: NodeId) -> Option<Rect> {
        self.doc.get_node(node).filter(|n| n.is_element() || n.is_anonymous())?;
        Some(self.map(node, self.doc.get_client_bounding_rect(node)?.into()))
    }

    pub(crate) fn hidden(&self, n: &Node) -> bool {
        let mut current = Some(n);
        while let Some(a) = current.filter(|a| a.is_element()) {
            if a.style().is_none_or(|s| s.is_display_none()) {
                return true;
            }
            current = a.parent.and_then(|p| self.doc.get_node(p));
        }
        false
    }

    pub(crate) fn offset_parent(&self, node: NodeId) -> Option<NodeId> {
        use blitz_dom::acss::properties::keywords::Position;
        let n = self.doc.get_node(node)?;
        let e = n.element_data()?;
        if !n.flags.is_in_document() || matches!(&*e.name.local, "html" | "body") {
            return None;
        }
        if self.hidden(n) {
            return None;
        }
        let position = n.style().map(|s| *s.groups.position());
        if position == Some(Position::Fixed) {
            return None;
        }
        let static_self = position == Some(Position::Static);
        let mut current = n.parent.and_then(|p| self.doc.get_node(p));
        while let Some(a) = current.filter(|a| a.is_element()) {
            let tag = a.element_data().map(|e| &*e.name.local);
            let positioned = a.style().is_some_and(|s| *s.groups.position() != Position::Static);
            if positioned || tag == Some("body") || (static_self && matches!(tag, Some("td" | "th" | "table"))) {
                return Some(a.id);
            }
            current = a.parent.and_then(|p| self.doc.get_node(p));
        }
        None
    }

    pub(crate) fn metrics(&self, node: NodeId) -> Option<Metrics> {
        let n = self.doc.get_node(node).filter(|n| n.is_element() || n.is_text_node())?;
        if n.is_element() && self.hidden(n) {
            return Some(Metrics::default());
        }
        let offset = self.doc.offset_rect(node)?;
        if !n.is_element() {
            return Some(Metrics { offset_left: offset.x, offset_top: offset.y, ..Default::default() });
        }
        let l = n.final_layout();
        let client_width = f64::from(l.size.width - l.border.left - l.border.right - l.scrollbar_size.width);
        let client_height = f64::from(l.size.height - l.border.top - l.border.bottom - l.scrollbar_size.height);
        let scroll = n.scroll_offset();
        Some(Metrics {
            offset_left: offset.x,
            offset_top: offset.y,
            offset_width: offset.width,
            offset_height: offset.height,
            client_left: l.border.left.into(),
            client_top: l.border.top.into(),
            client_width: client_width.max(0.0),
            client_height: client_height.max(0.0),
            scroll_left: scroll.x,
            scroll_top: scroll.y,
            scroll_width: client_width.max(0.0) + f64::from(l.scroll_width()),
            scroll_height: client_height.max(0.0) + f64::from(l.scroll_height()),
            offset_parent: None,
        })
    }
}
