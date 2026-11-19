mod compositor;
mod gpu;

#[cfg(target_os = "linux")]
pub use crate::vello::{Device, Fence};
pub use crate::vello::{GpuImage, ImageSet};
pub use acde_scene::*;
pub use compositor::{Compositor, Offscreen};
pub use gpu::{DeviceRequest, ErrorSink, Gpu, adapters, lock, set_error_sink, texture, webgpu_features};
pub use {kurbo, peniko, wgpu};

pub const BACKEND: wgpu::Backends = cfg_select! {
    windows => wgpu::Backends::DX12,
    target_os = "macos" => wgpu::Backends::METAL,
    _ => wgpu::Backends::VULKAN,
};
