use std::sync::Arc;

use kurbo::Affine;

use super::{Filter, Gpu, GpuImage, Scene};

pub trait Compositor: Send + Sync {
    fn offscreen(&self, gpu: Arc<Gpu>) -> Result<Box<dyn Offscreen>, String>;
}

pub trait Offscreen: Send {
    fn draw(&mut self, scene: Scene, size: (u32, u32), place: Affine) -> Result<Arc<GpuImage>, String>;
    fn shadow(&mut self, scene: Scene, size: (u32, u32), place: Affine, sigma: f64, color: [f32; 4]) -> Result<Arc<GpuImage>, String>;
    fn filtered(&mut self, scene: Scene, size: (u32, u32), place: Affine, filter: &Filter) -> Result<Arc<GpuImage>, String>;
    fn rasterize(&mut self, scene: Scene, size: (u32, u32)) -> Vec<u8>;
    fn trim(&mut self) -> Result<bool, String> {
        Ok(false)
    }
}
