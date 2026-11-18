use std::sync::Arc;

use crate::gpu::detour::fallback;
use crate::gpu::{Filter, Gpu, GpuImage, Scene};
use kurbo::Affine;

#[cfg(target_os = "linux")]
pub use fallback::Raster;
pub use fallback::{Composer, Op, Overrides, Vello};
pub use {crate::gpu, vello};

/// Turns image brushes that stand for GPU images of a replaced device into transparent paint, in place. Such an image is no
/// longer in the device's image set and carries no bytes, so vello would read it as an empty image and panic; it shows
/// nothing until whatever drew it (a canvas, a video, WebGL) draws again on the new device. Only after a device was
/// replaced (`Gpu::replaced`) is the scene walked at all.
pub fn clear_stale_images(scene: &mut Scene, gpu: &Gpu) {
    if !Gpu::replaced() {
        return;
    }
    for command in &mut scene.commands {
        let brush = match command {
            crate::gpu::RenderCommand::Fill(c) => &mut c.brush,
            crate::gpu::RenderCommand::Stroke(c) => &mut c.brush,
            crate::gpu::RenderCommand::GlyphRun(c) => &mut c.brush,
            _ => continue,
        };
        let crate::gpu::Paint::Image(image) = brush else { continue };
        let data = &image.image;
        if data.data.data().is_empty() && data.width > 0 && data.height > 0 && gpu.images.find(data.data.id()).is_none() {
            *brush = crate::gpu::Paint::Solid(peniko::Color::TRANSPARENT);
        }
    }
}

pub struct Offscreen(fallback::Offscreen, Arc<Gpu>);

impl Offscreen {
    pub fn new(gpu: Arc<Gpu>) -> Result<Self, String> {
        fallback::Offscreen::new(gpu.shared.clone()).map(|o| Self(o, gpu))
    }

    fn clear_stale(&self, scene: &mut Scene) {
        clear_stale_images(scene, &self.1);
    }
}

impl std::ops::Deref for Offscreen {
    type Target = fallback::Offscreen;

    fn deref(&self) -> &fallback::Offscreen {
        &self.0
    }
}

impl std::ops::DerefMut for Offscreen {
    fn deref_mut(&mut self) -> &mut fallback::Offscreen {
        &mut self.0
    }
}

impl crate::gpu::Offscreen for Offscreen {
    fn draw(&mut self, mut scene: Scene, size: (u32, u32), place: Affine) -> Result<Arc<GpuImage>, String> {
        self.clear_stale(&mut scene);
        self.0.draw(scene, size, place)
    }

    fn shadow(&mut self, mut scene: Scene, size: (u32, u32), place: Affine, sigma: f64, color: [f32; 4]) -> Result<Arc<GpuImage>, String> {
        self.clear_stale(&mut scene);
        self.0.shadow(scene, size, place, sigma, color)
    }

    fn filtered(&mut self, mut scene: Scene, size: (u32, u32), place: Affine, filter: &Filter) -> Result<Arc<GpuImage>, String> {
        self.clear_stale(&mut scene);
        self.0.filtered(scene, size, place, filter)
    }

    fn rasterize(&mut self, mut scene: Scene, size: (u32, u32)) -> Vec<u8> {
        self.clear_stale(&mut scene);
        self.0.rasterize(scene, size)
    }
}

pub struct VelloCompositor;

impl crate::gpu::Compositor for VelloCompositor {
    fn offscreen(&self, gpu: Arc<Gpu>) -> Result<Box<dyn crate::gpu::Offscreen>, String> {
        Offscreen::new(gpu).map(|o| Box::new(o) as Box<dyn crate::gpu::Offscreen>)
    }
}
