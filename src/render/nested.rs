use std::sync::{Arc, Mutex};

use kurbo::{Affine, Rect, Shape};
use peniko::{BlendMode, Fill};

use crate::gpu::{DropShadow, Filter, FilterEffect, GaussianBlur, GpuImage, LayerCommand, PaintScene, RenderCommand, Scene};

const MAX_AREA: f64 = 4096.0 * 4096.0;
const KEPT: usize = 24;

struct Kept {
    commands: Vec<RenderCommand>,
    filter: Filter,
    area: Rect,
    image: Arc<GpuImage>,
}

static KEEP: Mutex<Vec<Kept>> = Mutex::new(Vec::new());

pub(crate) fn has_effects<'a>(mut commands: impl Iterator<Item = &'a RenderCommand>) -> bool {
    commands.any(|c| matches!(c, RenderCommand::PushLayer(l) if l.filter.is_some() || l.backdrop_filter.is_some()))
}

pub(crate) fn closing(commands: &[RenderCommand], push: usize) -> Option<usize> {
    let mut depth = 0usize;
    (push..commands.len()).find(|&j| match commands[j] {
        RenderCommand::PushLayer(_) | RenderCommand::PushClipLayer(_) => {
            depth += 1;
            false
        }
        RenderCommand::PopLayer => {
            depth = depth.saturating_sub(1);
            depth == 0
        }
        _ => false,
    })
}

pub(crate) struct Flatten {
    pub device: f64,
    pub tolerance: f64,
    pub viewport: Rect,
}

impl Flatten {
    pub(crate) fn run(&self, commands: &[RenderCommand]) -> Vec<RenderCommand> {
        let mut out: Vec<RenderCommand> = Vec::with_capacity(commands.len());
        let mut k = 0;
        while k < commands.len() {
            let RenderCommand::PushLayer(l) = &commands[k] else {
                out.push(commands[k].clone());
                k += 1;
                continue;
            };
            if l.filter.is_none() && l.backdrop_filter.is_none() {
                out.push(commands[k].clone());
                k += 1;
                continue;
            }
            if let Some(backdrop) = l.backdrop_filter.as_deref() {
                out.extend(self.backdrop(&out, l, backdrop));
            }
            let filter = l.filter.as_deref().filter(|f| !f.effects().is_empty());
            let plain = RenderCommand::PushLayer(LayerCommand { filter: None, backdrop_filter: None, ..l.clone() });
            match (filter, closing(commands, k)) {
                (Some(filter), Some(end)) => {
                    let mut inner = Vec::with_capacity(end + 1 - k);
                    inner.push(RenderCommand::PushLayer(LayerCommand { filter: None, backdrop_filter: None, blend: BlendMode::default(), ..l.clone() }));
                    inner.extend(self.run(&commands[k + 1..end]));
                    inner.push(RenderCommand::PopLayer);
                    out.extend(self.filtered(inner, l, filter));
                    k = end + 1;
                }
                _ => {
                    out.push(plain);
                    k += 1;
                }
            }
        }
        out
    }

    fn filtered(&self, inner: Vec<RenderCommand>, l: &LayerCommand, filter: &Filter) -> Vec<RenderCommand> {
        let filter = self.scaled(filter);
        let content = (l.transform * l.clip.clone()).bounding_box();
        let e = filter.expansion();
        let grown = Rect::new(content.x0 + e.x0, content.y0 + e.y0, content.x1 + e.x1, content.y1 + e.y1);
        let Some(area) = self.area(grown) else { return Vec::new() };
        let Some(image) = self.image(inner.clone(), &filter, area) else { return inner };
        let mut scene = Scene::new();
        let rect = Rect::new(0.0, 0.0, area.width(), area.height());
        let blended = l.blend != BlendMode::default();
        if blended {
            scene.push_layer(Fill::NonZero, l.blend, 1.0, Affine::translate(area.origin().to_vec2()), &rect, None, None);
        }
        scene.draw_image(image.brush().as_ref(), Affine::translate(area.origin().to_vec2()));
        if blended {
            scene.pop_layer();
        }
        scene.commands
    }

    fn backdrop(&self, below: &[RenderCommand], l: &LayerCommand, filter: &Filter) -> Vec<RenderCommand> {
        let filter = self.scaled(filter);
        let shape = l.transform * l.clip.clone();
        let e = filter.expansion();
        let b = shape.bounding_box();
        let Some(area) = self.area(Rect::new(b.x0 + e.x0, b.y0 + e.y0, b.x1 + e.x1, b.y1 + e.y1)) else { return Vec::new() };
        let mut drawn = below.to_vec();
        let open = drawn.iter().fold(0isize, |depth, c| match c {
            RenderCommand::PushLayer(_) | RenderCommand::PushClipLayer(_) => depth + 1,
            RenderCommand::PopLayer => depth - 1,
            _ => depth,
        });
        drawn.extend((0..open.max(0)).map(|_| RenderCommand::PopLayer));
        let Some(image) = self.image(drawn, &filter, area) else { return Vec::new() };
        let mut scene = Scene::new();
        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &shape);
        scene.draw_image(image.brush().as_ref(), Affine::translate(area.origin().to_vec2()));
        scene.pop_layer();
        scene.commands
    }

    fn area(&self, wanted: Rect) -> Option<Rect> {
        let area = wanted.intersect(self.viewport).expand();
        (area.width() >= 1.0 && area.height() >= 1.0 && area.area() <= MAX_AREA).then_some(area)
    }

    fn scaled(&self, filter: &Filter) -> Filter {
        let s = self.device as f32;
        filter
            .effects()
            .iter()
            .map(|e| match *e {
                FilterEffect::GaussianBlur(b) => FilterEffect::GaussianBlur(GaussianBlur { std_deviation: b.std_deviation * s }),
                FilterEffect::DropShadow(d) => {
                    FilterEffect::DropShadow(DropShadow { dx: d.dx * s, dy: d.dy * s, std_deviation: d.std_deviation * s, color: d.color })
                }
                FilterEffect::ColorMatrix(m) => FilterEffect::ColorMatrix(m),
            })
            .collect()
    }

    fn image(&self, commands: Vec<RenderCommand>, filter: &Filter, area: Rect) -> Option<Arc<GpuImage>> {
        let mut kept = KEEP.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = kept.iter().position(|k| k.area == area && k.filter == *filter && k.commands == commands) {
            let hit = kept.remove(at);
            let image = hit.image.clone();
            kept.push(hit);
            return Some(image);
        }
        drop(kept);
        let size = (area.width() as u32, area.height() as u32);
        let scene = Scene { tolerance: self.tolerance, commands: commands.clone() };
        let image = crate::render::gpu::filtered(scene, size, Affine::translate(-area.origin().to_vec2()), filter)?;
        let mut kept = KEEP.lock().unwrap_or_else(|e| e.into_inner());
        if kept.len() >= KEPT {
            kept.remove(0);
        }
        kept.push(Kept { commands, filter: filter.clone(), area, image: image.clone() });
        Some(image)
    }
}
