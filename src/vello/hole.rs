use std::sync::Arc;

use super::{GpuImage, ImageSet};
use crate::gpu::{FillCommand, LayerCommand, Paint, RenderCommand, Scene};
use kurbo::{Affine, BezPath, PathEl, Rect, Shape};
use peniko::{BlendMode, ImageQuality};

const EXACT: f64 = 1e-3;

pub struct Hole {
    pub index: usize,
    pub rect: [u32; 4],
    pub texel: [u32; 2],
    pub image: Arc<GpuImage>,
    pub map: [f64; 4],
    pub smooth: bool,
    open: Vec<usize>,
}

pub struct Kept {
    scene: Scene,
    index: usize,
    images: Vec<(u64, u64)>,
    rect: [u32; 4],
}

impl Hole {
    pub fn find(scene: &Scene, images: &ImageSet) -> Option<Self> {
        let mut found = None;
        for candidate in Self::candidates(scene, images, false) {
            if found.is_some() {
                return None;
            }
            found = Some(candidate?);
        }
        found
    }

    pub fn largest(scene: &Scene, images: &ImageSet) -> Option<Self> {
        Self::candidates(scene, images, true).flatten().max_by_key(|h| u64::from(h.rect[2]) * u64::from(h.rect[3]))
    }

    fn candidates<'s>(scene: &'s Scene, images: &'s ImageSet, scaled: bool) -> impl Iterator<Item = Option<Self>> + 's {
        let mut open: Vec<(usize, Option<Rect>)> = Vec::new();
        scene.commands.iter().enumerate().filter_map(move |(i, command)| {
            match command {
                RenderCommand::PushLayer(l) => open.push((i, plain(l))),
                RenderCommand::PushClipLayer(c) => open.push((i, rect_of(&c.clip, c.transform))),
                RenderCommand::PopLayer => _ = open.pop(),
                RenderCommand::Fill(f) => {
                    let Paint::Image(brush) = &f.brush else { return None };
                    let image = images.find(brush.image.data.id()).filter(|i| i.opaque())?;
                    return Some(Self::of(i, f, image, &open, scaled));
                }
                _ => {}
            }
            None
        })
    }

    fn of(index: usize, fill: &FillCommand, image: Arc<GpuImage>, open: &[(usize, Option<Rect>)], scaled: bool) -> Option<Self> {
        let Paint::Image(brush) = &fill.brush else { return None };
        let sampler = &brush.sampler;
        if sampler.alpha != 1.0 || sampler.quality == ImageQuality::High {
            return None;
        }
        let drawn = rect_of(&fill.shape, fill.transform)?;
        let [a, b, c, d, e, f] = (fill.transform * fill.brush_transform.unwrap_or(Affine::IDENTITY)).as_coeffs();
        let (width, height) = (f64::from(image.texture.width()), f64::from(image.texture.height()));
        if b != 0.0 || c != 0.0 || a <= 0.0 || d <= 0.0 {
            return None;
        }
        let one = (a - 1.0).abs() * width.max(height) <= EXACT && (d - 1.0).abs() * width.max(height) <= EXACT;
        let pad = sampler.x_extend == peniko::Extend::Pad && sampler.y_extend == peniko::Extend::Pad;
        let (a, d, e, f, smooth) = match (one, whole(e).zip(whole(f))) {
            (true, Some((e, f))) => (1.0, 1.0, e, f, false),
            _ if scaled && pad => (a, d, e, f, sampler.quality != ImageQuality::Low),
            _ => return None,
        };
        let mut rect = drawn.intersect(Rect::new(e, f, e + width * a, f + height * d));
        if rect != drawn {
            return None;
        }
        for (_, clip) in open {
            rect = rect.intersect((*clip)?);
        }
        let [x0, y0, x1, y1] = [whole(rect.x0)?, whole(rect.y0)?, whole(rect.x1)?, whole(rect.y1)?];
        if x1 <= x0 || y1 <= y0 || x0 < 0.0 || y0 < 0.0 {
            return None;
        }
        let rect = [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32];
        let texel = [((x0 - e) / a).floor().max(0.0) as u32, ((y0 - f) / d).floor().max(0.0) as u32];
        let map = [-e / a, -f / d, 1.0 / a, 1.0 / d];
        Some(Self { index, rect, texel, image, map, smooth, open: open.iter().map(|(i, _)| *i).collect() })
    }

    pub fn blends_alone(&self, scene: &Scene) -> bool {
        let mut depth = 0usize;
        scene.commands.get(self.index + 1..).unwrap_or_default().iter().all(|c| match c {
            RenderCommand::PushLayer(l) => {
                let alone = depth > 0 || l.blend == BlendMode::default();
                depth += 1;
                alone
            }
            RenderCommand::PushClipLayer(_) => {
                depth += 1;
                true
            }
            RenderCommand::PopLayer => {
                depth = depth.saturating_sub(1);
                true
            }
            _ => true,
        })
    }

    pub fn above(&self, scene: &Scene) -> Option<Scene> {
        let after = scene.commands.get(self.index + 1..)?;
        let draws = after.iter().any(|c| !matches!(c, RenderCommand::PushLayer(_) | RenderCommand::PushClipLayer(_) | RenderCommand::PopLayer));
        let opened = self.open.iter().filter_map(|i| scene.commands.get(*i).cloned());
        draws.then(|| Scene { tolerance: scene.tolerance, commands: opened.chain(after.iter().cloned()).collect() })
    }

    pub fn keep(&self, scene: &Scene, images: &ImageSet) -> Kept {
        Kept { scene: scene.clone(), index: self.index, images: generations(scene, images, self.index), rect: self.rect }
    }
}

impl Kept {
    pub fn matches(&self, scene: &Scene, hole: &Hole, images: &ImageSet) -> bool {
        self.index == hole.index
            && self.rect == hole.rect
            && self.scene.tolerance == scene.tolerance
            && self.scene.commands.len() == scene.commands.len()
            && self.scene.commands.iter().zip(&scene.commands).enumerate().all(|(i, (a, b))| match (i == self.index, a, b) {
                (true, RenderCommand::Fill(a), RenderCommand::Fill(b)) => same_place(a, b),
                (true, ..) => false,
                (false, a, b) => a == b,
            })
            && self.images == generations(scene, images, self.index)
    }
}

fn same_place(a: &FillCommand, b: &FillCommand) -> bool {
    let (Paint::Image(x), Paint::Image(y)) = (&a.brush, &b.brush) else { return false };
    let (p, q) = (&x.image, &y.image);
    a.fill == b.fill
        && a.transform == b.transform
        && a.brush_transform == b.brush_transform
        && a.shape == b.shape
        && x.sampler == y.sampler
        && (p.width, p.height, p.format, p.alpha_type) == (q.width, q.height, q.format, q.alpha_type)
}

fn generations(scene: &Scene, images: &ImageSet, skip: usize) -> Vec<(u64, u64)> {
    scene
        .commands
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != skip)
        .filter_map(|(_, c)| match c {
            RenderCommand::Fill(c) => Some(&c.brush),
            RenderCommand::Stroke(c) => Some(&c.brush),
            RenderCommand::GlyphRun(c) => Some(&c.brush),
            _ => None,
        })
        .filter_map(|brush| match brush {
            Paint::Image(brush) => images.find(brush.image.data.id()).map(|i| (i.id(), i.generation())),
            _ => None,
        })
        .collect()
}

fn plain(layer: &LayerCommand) -> Option<Rect> {
    let plain = layer.alpha == 1.0 && layer.blend == BlendMode::default() && layer.filter.is_none() && layer.backdrop_filter.is_none();
    plain.then(|| rect_of(&layer.clip, layer.transform)).flatten()
}

fn rect_of(path: &BezPath, transform: Affine) -> Option<Rect> {
    let [_, b, c, ..] = transform.as_coeffs();
    if b != 0.0 || c != 0.0 {
        return None;
    }
    let bounds = path.bounding_box();
    let corner = |p: kurbo::Point| (p.x == bounds.x0 || p.x == bounds.x1) && (p.y == bounds.y0 || p.y == bounds.y1);
    let straight = |a: kurbo::Point, b: kurbo::Point| a.x == b.x || a.y == b.y;
    let mut points: Vec<kurbo::Point> = Vec::new();
    for (i, el) in path.elements().iter().enumerate() {
        match el {
            PathEl::MoveTo(p) if i == 0 && corner(*p) => points.push(*p),
            PathEl::LineTo(p) if corner(*p) && points.last().is_some_and(|q| straight(*q, *p)) => points.push(*p),
            PathEl::ClosePath if i + 1 == path.elements().len() => {}
            _ => return None,
        }
    }
    let closes = points.first().zip(points.last()).is_some_and(|(a, b)| straight(*a, *b));
    let mut distinct = points.clone();
    distinct.dedup();
    distinct.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    distinct.dedup();
    (closes && distinct.len() == 4 && points.len() <= 5 && bounds.area() > 0.0).then(|| transform.transform_rect_bbox(bounds))
}

fn whole(v: f64) -> Option<f64> {
    let r = v.round();
    ((v - r).abs() <= EXACT).then_some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_axis_aligned_rectangle_is_a_rect() {
        let scale = Affine::scale(2.0);
        let rect = Rect::new(1.0, 2.0, 11.0, 7.0);
        assert_eq!(rect_of(&rect.to_path(0.1), scale), Some(Rect::new(2.0, 4.0, 22.0, 14.0)));
        assert_eq!(rect_of(&rect.to_path(0.1), Affine::rotate(0.1)), None);
        assert_eq!(rect_of(&kurbo::RoundedRect::from_rect(rect, 2.0).to_path(0.1), scale), None);
        let mut bowtie = BezPath::new();
        bowtie.move_to((0.0, 0.0));
        bowtie.line_to((10.0, 10.0));
        bowtie.line_to((10.0, 0.0));
        bowtie.line_to((0.0, 10.0));
        bowtie.close_path();
        assert_eq!(rect_of(&bowtie, Affine::IDENTITY), None);
    }

    #[test]
    fn whole_pixels_only() {
        assert_eq!(whole(80.0000001), Some(80.0));
        assert_eq!(whole(80.5), None);
    }
}
