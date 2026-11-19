use crate::gpu::PaintScene;
use kurbo::{Affine, BezPath, Shape};
use peniko::{Color, Fill};

use super::Engine;
use crate::render::geometry::Geometry;
use crate::render::{Dom, NONE, Result};

const CONTENT: Color = Color::from_rgba8(111, 168, 220, 168);
const PADDING: Color = Color::from_rgba8(147, 196, 125, 140);
const BORDER: Color = Color::from_rgba8(255, 229, 153, 168);
const MARGIN: Color = Color::from_rgba8(246, 178, 107, 168);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoxModel {
    pub(crate) margin: kurbo::Rect,
    pub(crate) border: kurbo::Rect,
    pub(crate) padding: kurbo::Rect,
    pub(crate) content: kurbo::Rect,
}

impl BoxModel {
    pub(crate) fn of(border: kurbo::Rect, margin: [f64; 4], borders: [f64; 4], padding: [f64; 4]) -> Self {
        let grow = |r: kurbo::Rect, [left, top, right, bottom]: [f64; 4]| {
            let x0 = r.x0 - left;
            let y0 = r.y0 - top;
            kurbo::Rect::new(x0, y0, (r.x1 + right).max(x0), (r.y1 + bottom).max(y0))
        };
        let shrink = |r: kurbo::Rect, sides: [f64; 4]| grow(r, sides.map(|s| -s));
        let padding_box = shrink(border, borders);
        Self { margin: grow(border, margin), border, padding: padding_box, content: shrink(padding_box, padding) }
    }

    pub(crate) fn paint(&self, scene: &mut impl PaintScene, scale: f64) {
        let transform = Affine::scale(scale);
        let ring = |outer: kurbo::Rect, inner: kurbo::Rect| {
            let mut path: BezPath = outer.to_path(0.1);
            path.extend(inner.to_path(0.1));
            path
        };
        scene.fill(Fill::EvenOdd, transform, MARGIN, None, &ring(self.margin, self.border));
        scene.fill(Fill::EvenOdd, transform, BORDER, None, &ring(self.border, self.padding));
        scene.fill(Fill::EvenOdd, transform, PADDING, None, &ring(self.padding, self.content));
        scene.fill(Fill::NonZero, transform, CONTENT, None, &self.content);
    }
}

impl Engine {
    pub(crate) fn overlay(&mut self, scale: f64) -> Option<Vec<crate::gpu::RenderCommand>> {
        let picker = self.picker_scene(scale).map(|p| p.commands);
        let highlight = self.highlight_scene(scale).map(|h| h.commands);
        match (picker, highlight) {
            (None, None) => None,
            (picker, highlight) => Some(picker.into_iter().chain(highlight).flatten().collect()),
        }
    }

    fn highlight_scene(&mut self, scale: f64) -> Option<crate::gpu::Scene> {
        let node = self.highlight?;
        let doc = self.resolved();
        let geometry = Geometry { doc };
        let n = doc.get_node(node).filter(|n| n.is_element() && n.flags.is_in_document() && !geometry.hidden(n))?;
        let rect = geometry.client_rect(node)?;
        let l = n.final_layout();
        let margin = [l.margin.left, l.margin.top, l.margin.right, l.margin.bottom].map(f64::from);
        let borders = [l.border.left, l.border.top, l.border.right, l.border.bottom].map(f64::from);
        let padding = [l.padding.left, l.padding.top, l.padding.right, l.padding.bottom].map(f64::from);
        let border = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
        let model = BoxModel::of(border, margin, borders, padding);
        let mut scene = crate::gpu::Scene::new();
        model.paint(&mut scene, scale);
        Some(scene)
    }
}

impl Dom {
    pub fn highlight(&self, id: u32) -> Result<()> {
        let changed = self.shared.script_call(&self.wake, |e| {
            let node = match id {
                NONE => None,
                id => Some(e.n(id)?),
            };
            if e.highlight == node {
                return Ok(false);
            }
            e.highlight = node;
            e.dirty = true;
            e.want_present = true;
            Ok(true)
        })?;
        if changed {
            _ = self.wake.send(());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_areas_nest_around_the_border_box() {
        let border = kurbo::Rect::new(10.0, 20.0, 110.0, 70.0);
        let model = BoxModel::of(border, [5.0, 6.0, 7.0, 8.0], [1.0, 2.0, 3.0, 4.0], [10.0, 10.0, 10.0, 10.0]);
        assert_eq!(model.margin, kurbo::Rect::new(5.0, 14.0, 117.0, 78.0));
        assert_eq!(model.border, border);
        assert_eq!(model.padding, kurbo::Rect::new(11.0, 22.0, 107.0, 66.0));
        assert_eq!(model.content, kurbo::Rect::new(21.0, 32.0, 97.0, 56.0));
        let thin = BoxModel::of(kurbo::Rect::new(0.0, 0.0, 4.0, 4.0), [0.0; 4], [3.0; 4], [3.0; 4]);
        assert!(thin.content.width() >= 0.0 && thin.content.height() >= 0.0, "a box smaller than its edges does not turn inside out");
    }

    #[test]
    fn painting_fills_four_shapes() {
        let model = BoxModel::of(kurbo::Rect::new(0.0, 0.0, 40.0, 20.0), [4.0; 4], [1.0; 4], [2.0; 4]);
        let mut scene = crate::gpu::Scene::new();
        model.paint(&mut scene, 2.0);
        assert!(scene.commands.len() >= 4, "{} commands", scene.commands.len());
    }
}
