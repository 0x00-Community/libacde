use std::collections::HashMap;
use std::sync::Arc;

use super::{GpuImage, ImageSet};
use crate::gpu::{FillCommand, Paint, RenderCommand, Scene};
use kurbo::{Affine, Rect, Shape};
use wgpu::util::DeviceExt;

pub const LARGE: u32 = 4096;
const MARGIN: f64 = 2.0;
const MOST_FACTOR: u32 = 256;
const USAGE: wgpu::TextureUsages =
    wgpu::TextureUsages::STORAGE_BINDING.union(wgpu::TextureUsages::TEXTURE_BINDING).union(wgpu::TextureUsages::COPY_SRC).union(wgpu::TextureUsages::COPY_DST);

type Key = (u64, u64, [u32; 4], u32);

#[derive(Default)]
pub struct Fit {
    shrink: Option<(wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
    last: HashMap<Key, Arc<GpuImage>>,
    used: HashMap<Key, Arc<GpuImage>>,
}

struct Cut {
    rect: [u32; 4],
    factor: u32,
}

impl Fit {
    pub fn has_large(scene: &Scene, images: &ImageSet) -> bool {
        scene.commands.iter().any(|c| matches!(c, RenderCommand::Fill(f) if Self::large(&f.brush, images).is_some()))
    }

    fn large(paint: &Paint, images: &ImageSet) -> Option<Arc<GpuImage>> {
        let Paint::Image(brush) = paint else { return None };
        if brush.image.width <= LARGE && brush.image.height <= LARGE {
            return None;
        }
        images.find(brush.image.data.id())
    }

    pub fn scene(
        &mut self,
        device: &wgpu::Device,
        scene: Scene,
        place: Affine,
        view: Rect,
        images: &Arc<ImageSet>,
        encoder: &mut Option<wgpu::CommandEncoder>,
    ) -> Scene {
        if !Self::has_large(&scene, images) {
            return scene;
        }
        let mut scene = scene;
        for command in &mut scene.commands {
            let RenderCommand::Fill(fill) = command else { continue };
            let Some(image) = Self::large(&fill.brush, images) else { continue };
            let Paint::Image(brush) = &fill.brush else { continue };
            if brush.sampler.x_extend != peniko::Extend::Pad || brush.sampler.y_extend != peniko::Extend::Pad {
                continue;
            }
            match Self::cut(fill, place, view, &image) {
                None => fill.brush = Paint::Solid(peniko::Color::TRANSPARENT),
                Some(cut) => {
                    let crop = self.crop(device, &image, &cut, images, encoder);
                    let mut fitted = crop.brush();
                    fitted.sampler = brush.sampler;
                    let [x, y, ..] = cut.rect;
                    let inner = fill.brush_transform.unwrap_or(Affine::IDENTITY);
                    fill.brush_transform = Some(inner * Affine::translate((f64::from(x), f64::from(y))) * Affine::scale(f64::from(cut.factor)));
                    fill.brush = Paint::Image(fitted);
                }
            }
        }
        scene
    }

    pub fn done(&mut self) {
        self.last = std::mem::take(&mut self.used);
    }

    fn cut(fill: &FillCommand, place: Affine, view: Rect, image: &GpuImage) -> Option<Cut> {
        let to_view = place * fill.transform * fill.brush_transform.unwrap_or(Affine::IDENTITY);
        if to_view.determinant().abs() < 1e-12 || !to_view.is_finite() {
            return None;
        }
        let shown = (place * fill.transform).transform_rect_bbox(fill.shape.bounding_box()).intersect(view);
        if shown.width() <= 0.0 || shown.height() <= 0.0 {
            return None;
        }
        let back = to_view.inverse();
        let [a, b, c, d, _, _] = back.as_coeffs();
        let step = f64::hypot(a, b).min(f64::hypot(c, d));
        let seen = back.transform_rect_bbox(shown);
        let whole = Rect::new(0.0, 0.0, f64::from(image.texture.width()), f64::from(image.texture.height()));
        let mut factor = if step >= 2.0 { 1u32 << (step.log2().floor() as u32).min(MOST_FACTOR.trailing_zeros()) } else { 1 };
        loop {
            let reach = MARGIN * f64::from(factor);
            let rect = seen.inflate(reach, reach).intersect(whole).expand();
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                return None;
            }
            let side = |v: f64| (v / f64::from(factor)).ceil();
            if (side(rect.width()) <= f64::from(LARGE) && side(rect.height()) <= f64::from(LARGE)) || factor >= MOST_FACTOR {
                return Some(Cut { rect: [rect.x0 as u32, rect.y0 as u32, rect.width() as u32, rect.height() as u32], factor });
            }
            factor *= 2;
        }
    }

    fn crop(
        &mut self,
        device: &wgpu::Device,
        image: &Arc<GpuImage>,
        cut: &Cut,
        images: &Arc<ImageSet>,
        encoder: &mut Option<wgpu::CommandEncoder>,
    ) -> Arc<GpuImage> {
        let key = (image.id(), image.generation(), cut.rect, cut.factor);
        if let Some(kept) = self.used.get(&key).or_else(|| self.last.get(&key)).cloned() {
            self.used.insert(key, kept.clone());
            return kept;
        }
        image.settle();
        let [x, y, w, h] = cut.rect;
        let size = (w.div_ceil(cut.factor), h.div_ceil(cut.factor));
        let texture = super::texture(device, size, USAGE, "acde image crop");
        let encoder = encoder.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde image crop") }));
        if cut.factor == 1 {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: &image.texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        } else {
            let (pipeline, layout) = self.shrink_pipeline(device);
            let mut bytes = Vec::with_capacity(32);
            for v in [x, y, x + w, y + h, cut.factor, 0, 0, 0] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("acde image shrink"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let input = image.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let output = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("acde image shrink"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&output) },
                    wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("acde image shrink"), timestamp_writes: None });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(size.0.div_ceil(8), size.1.div_ceil(8), 1);
        }
        let crop = GpuImage::new(texture, images);
        self.used.insert(key, crop.clone());
        crop
    }

    fn shrink_pipeline(&mut self, device: &wgpu::Device) -> (wgpu::ComputePipeline, wgpu::BindGroupLayout) {
        let (pipeline, layout) = self.shrink.get_or_insert_with(|| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("acde image shrink"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shrink.wgsl").into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("acde image shrink"),
                layout: None,
                module: &module,
                entry_point: Some("shrink"),
                compilation_options: Default::default(),
                cache: super::pipelines::of(device).as_ref(),
            });
            let layout = pipeline.get_bind_group_layout(0);
            (pipeline, layout)
        });
        (pipeline.clone(), layout.clone())
    }
}
