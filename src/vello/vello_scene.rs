use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::{Overrides, Shared};
use crate::gpu::{Filter, Glyph, NormalizedCoord, PaintRef, PaintScene, Scene};
use kurbo::{Affine, Rect, Shape, Stroke, Vec2};
use peniko::{BlendMode, BrushRef, Color, Fill, FontData, StyleRef};

static BUILDS: AtomicU64 = AtomicU64::new(1);

struct VelloScene<'s>(&'s mut vello::Scene);

pub struct Vello {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello::Renderer,
    scene: vello::Scene,
    threads: Option<NonZeroUsize>,
    build: u64,
    rendered: bool,
    fit: super::fit::Fit,
}

impl Vello {
    const STORAGE_BUFFERS: u32 = 8;
    const INVOCATIONS: u32 = 256;

    pub fn probe(device: &wgpu::Device, queue: &wgpu::Queue, downlevel: &wgpu::DownlevelCapabilities) -> Result<(), String> {
        let limits = device.limits();
        if !downlevel.flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
            return Err("no compute shaders".into());
        }
        if limits.max_storage_buffers_per_shader_stage < Self::STORAGE_BUFFERS {
            return Err(format!("{} storage buffers per stage, vello needs {}", limits.max_storage_buffers_per_shader_stage, Self::STORAGE_BUFFERS));
        }
        if limits.max_compute_invocations_per_workgroup < Self::INVOCATIONS || limits.max_compute_workgroup_size_x < Self::INVOCATIONS {
            return Err(format!("workgroups of {} invocations, vello needs {}", limits.max_compute_invocations_per_workgroup, Self::INVOCATIONS));
        }
        let started = std::time::Instant::now();
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let build = || Self::on_this_thread(device, queue).map(drop);
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)).unwrap_or_else(|_| Err("building vello's pipelines panicked".into()));
        let internal = pollster::block_on(internal.pop());
        let validation = pollster::block_on(validation.pop());
        let errors: Vec<String> = [built.err(), internal.map(|e| e.to_string()), validation.map(|e| e.to_string())].into_iter().flatten().collect();
        if !errors.is_empty() {
            return Err(format!("vello's pipelines do not build: {}", errors.join("; ")));
        }
        log::info!("acde: vello's pipelines built in {:.1}ms", started.elapsed().as_secs_f64() * 1000.0);
        Ok(())
    }

    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self, String> {
        Self::with_threads(device, queue, None)
    }

    pub fn on_this_thread(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self, String> {
        Self::with_threads(device, queue, NonZeroUsize::new(1))
    }

    fn with_threads(device: &wgpu::Device, queue: &wgpu::Queue, threads: Option<NonZeroUsize>) -> Result<Self, String> {
        let options = vello::RendererOptions {
            use_cpu: false,
            antialiasing_support: vello::AaSupport::area_only(),
            num_init_threads: threads,
            pipeline_cache: super::pipelines::of(device),
        };
        let renderer = vello::Renderer::new(device, options).map_err(|e| format!("vello: {e}"))?;
        let build = BUILDS.fetch_add(1, Ordering::Relaxed);
        Ok(Self {
            device: device.clone(),
            queue: queue.clone(),
            renderer,
            scene: vello::Scene::new(),
            threads,
            build,
            rendered: false,
            fit: Default::default(),
        })
    }

    pub fn trim(&mut self) -> Result<bool, String> {
        if !self.rendered {
            return Ok(false);
        }
        let started = Instant::now();
        *self = Self::with_threads(&self.device, &self.queue, self.threads)?;
        let _ = self.device.poll(wgpu::PollType::Poll);
        log::debug!(target: "perf::raster", "vello trim ms={:.3}", started.elapsed().as_secs_f64() * 1000.0);
        Ok(true)
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
        overrides.follow(self.build);
        overrides.forget(&mut self.renderer);
        self.scene.reset();
        let images = overrides.images().clone();
        let bounds = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
        let mut crops = None;
        for (scene, place, clip) in scenes {
            let view = clip.map_or(bounds, |c| c.intersect(bounds));
            let scene = self.fit.scene(&self.device, scene, place, view, &images, &mut crops);
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
        self.rendered = true;
        let rendered = {
            let _lock = lock.map(Shared::submit_lock);
            if let Some(crops) = crops {
                self.queue.submit([crops.finish()]);
            }
            self.renderer.render_to_texture(&self.device, &self.queue, &self.scene, target, &params)
        };
        self.fit.done();
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
        let outlined = super::fonts::outlined(font);
        let font = outlined.as_ref().unwrap_or(font);
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
