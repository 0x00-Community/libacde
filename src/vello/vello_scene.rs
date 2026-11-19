use std::sync::Arc;

use super::{Overrides, Shared};
use crate::gpu::{Filter, Glyph, NormalizedCoord, PaintRef, PaintScene, Scene};
use kurbo::{Affine, Rect, Shape, Stroke, Vec2};
use peniko::{BlendMode, BrushRef, Color, Fill, FontData, StyleRef};

struct VelloScene<'s>(&'s mut vello::Scene);

pub struct Vello {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello::Renderer,
    scene: vello::Scene,
}

impl Vello {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self, String> {
        let options =
            vello::RendererOptions { use_cpu: false, antialiasing_support: vello::AaSupport::area_only(), num_init_threads: None, pipeline_cache: None };
        let renderer = vello::Renderer::new(device, options).map_err(|e| format!("vello: {e}"))?;
        Ok(Self { device: device.clone(), queue: queue.clone(), renderer, scene: vello::Scene::new() })
    }

    pub fn render(
        &mut self,
        scenes: impl IntoIterator<Item = (Scene, Affine)>,
        overrides: &mut Overrides,
        target: &wgpu::TextureView,
        size: (u32, u32),
        base: Color,
        lock: Option<&Shared>,
    ) -> Result<(), String> {
        self.render_clipped(scenes.into_iter().map(|(scene, place)| (scene, place, None)), overrides, target, size, base, lock)
    }

    pub fn render_clipped(
        &mut self,
        scenes: impl IntoIterator<Item = (Scene, Affine, Option<Rect>)>,
        overrides: &mut Overrides,
        target: &wgpu::TextureView,
        (width, height): (u32, u32),
        base: Color,
        lock: Option<&Shared>,
    ) -> Result<(), String> {
        overrides.forget(&mut self.renderer);
        self.scene.reset();
        for (scene, place, clip) in scenes {
            overrides.apply(&mut self.renderer, &scene);
            if let Some(clip) = clip {
                self.scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &clip);
            }
            VelloScene(&mut self.scene).append_scene(scene, place);
            if clip.is_some() {
                self.scene.pop_layer();
            }
        }
        let clear = peniko::Gradient::new_linear((0.0, 0.0), (1.0, 0.0)).with_stops([Color::TRANSPARENT, Color::TRANSPARENT]);
        self.scene.fill(Fill::NonZero, Affine::IDENTITY, &clear, None, &Rect::new(0.0, 0.0, 1.0, 1.0));
        let params = vello::RenderParams { base_color: base, width, height, antialiasing_method: vello::AaConfig::Area };
        let rendered = {
            let _lock = lock.map(Shared::submit_lock);
            self.renderer.render_to_texture(&self.device, &self.queue, &self.scene, target, &params)
        };
        self.scene.reset();
        rendered.map_err(|e| format!("vello: {e}"))?;
        overrides.submitted();
        Ok(())
    }
}

impl PaintScene for VelloScene<'_> {
    fn reset(&mut self) {
        self.0.reset();
    }

    fn push_layer(
        &mut self,
        fill: Fill,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        _filter: Option<Arc<Filter>>,
        _backdrop_filter: Option<Arc<Filter>>,
    ) {
        self.0.push_layer(fill, blend, alpha, transform, clip);
    }

    fn push_clip_layer(&mut self, fill: Fill, transform: Affine, clip: &impl Shape) {
        self.0.push_clip_layer(fill, transform, clip);
    }

    fn pop_layer(&mut self) {
        self.0.pop_layer();
    }

    fn stroke<'a>(&mut self, style: &Stroke, transform: Affine, paint: impl Into<PaintRef<'a>>, paint_transform: Option<Affine>, shape: &impl Shape) {
        self.0.stroke(style, transform, BrushRef::from(paint.into()), paint_transform, shape);
    }

    fn fill<'a>(&mut self, fill: Fill, transform: Affine, paint: impl Into<PaintRef<'a>>, paint_transform: Option<Affine>, shape: &impl Shape) {
        self.0.fill(fill, transform, BrushRef::from(paint.into()), paint_transform, shape);
    }

    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        embolden: Vec2,
        style: impl Into<StyleRef<'a>>,
        paint: impl Into<PaintRef<'a>>,
        paint_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    ) {
        self.0
            .draw_glyphs(font)
            .font_size(font_size)
            .hint(hint)
            .normalized_coords(normalized_coords)
            .font_embolden(vello::FontEmbolden::new(kurbo::Diagonal2::new(embolden.x, embolden.y)))
            .brush(BrushRef::from(paint.into()))
            .brush_alpha(paint_alpha)
            .transform(transform)
            .glyph_transform(glyph_transform)
            .draw(style, glyphs.map(|g| vello::Glyph { id: g.id, x: g.x, y: g.y }));
    }

    fn draw_box_shadow(&mut self, transform: Affine, rect: Rect, color: Color, radius: f64, std_dev: f64) {
        self.0.draw_blurred_rounded_rect(transform, rect, color, radius, std_dev);
    }
}
