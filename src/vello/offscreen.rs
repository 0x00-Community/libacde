use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

#[cfg(target_os = "linux")]
use crate::vello::Exported;

#[cfg(not(target_os = "linux"))]
pub enum Exported {}

const WAIT: Duration = Duration::from_secs(10);
const READ_BAND: u64 = 64 << 20;
const WHOLE: u32 = super::fit::LARGE;
const TILE: u32 = 4064;
const TILE_WITH_IMAGES: u32 = 2032;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const MAX_RADIUS: f64 = 256.0;

pub struct Shared {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub images: Arc<ImageSet>,
    submit: Mutex<()>,
    #[cfg_attr(not(target_os = "linux"), expect(dead_code))]
    export: bool,
}

impl Shared {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Arc<Self> {
        Arc::new(Self { device, queue, images: Arc::default(), submit: Mutex::new(()), export: false })
    }

    #[cfg(target_os = "linux")]
    pub fn exporting(device: wgpu::Device, queue: wgpu::Queue) -> Arc<Self> {
        Arc::new(Self { device, queue, images: Arc::default(), submit: Mutex::new(()), export: true })
    }

    pub fn open() -> Result<Arc<Self>, String> {
        Ok(crate::gpu::Gpu::open("acde offscreen")?.2.shared.clone())
    }

    pub fn submit_lock(&self) -> MutexGuard<'_, ()> {
        self.submit.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn submit_mutex(&self) -> &Mutex<()> {
        &self.submit
    }

    pub fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(&self, buffers: I) -> wgpu::SubmissionIndex {
        let _guard = self.submit_lock();
        self.queue.submit(buffers)
    }

    fn image(&self, size: (u32, u32), label: &str) -> (wgpu::Texture, Option<Exported>) {
        #[cfg(target_os = "linux")]
        if self.export
            && let Ok((texture, exported)) = crate::vello::Exported::texture(&self.device, size)
        {
            return (texture, Some(exported));
        }
        (self.texture(size, label), None)
    }

    fn texture_with(&self, size: (u32, u32), usage: wgpu::TextureUsages, label: &str) -> wgpu::Texture {
        super::texture(&self.device, size, usage, label)
    }

    fn texture(&self, (width, height): (u32, u32), label: &str) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    pub fn write(&self, image: &GpuImage, data: &[u8], (width, height): (u32, u32), from: [u32; 2], [x, y, w, h]: [u32; 4]) {
        let row = u64::from(width) * 4;
        if w == 0 || h == 0 || (data.len() as u64) < row * u64::from(height) {
            return;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &image.texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: u64::from(from[1]) * row + u64::from(from[0]) * 4,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    pub fn read(&self, texture: &wgpu::Texture, [x, y, w, h]: [u32; 4]) -> Result<Vec<u8>, String> {
        if w == 0 || h == 0 || x + w > texture.width() || y + h > texture.height() {
            return Err(format!("{w}x{h} at ({x}, {y}) is outside the {}x{} texture", texture.width(), texture.height()));
        }
        let stride = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let band = (READ_BAND / u64::from(stride)).clamp(1, u64::from(h)) as u32;
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        for top in (0..h).step_by(band as usize) {
            self.read_band(texture, [x, y + top, w, band.min(h - top)], stride, &mut out)?;
        }
        Ok(out)
    }

    fn read_band(&self, texture: &wgpu::Texture, [x, y, w, h]: [u32; 4], stride: u32, out: &mut Vec<u8>) -> Result<(), String> {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acde read back"),
            size: u64::from(stride) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde read back") });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let index = {
            let _lock = self.submit_lock();
            self.queue.submit([encoder.finish()])
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |r| _ = tx.send(r));
        self.device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: Some(WAIT) }).map_err(|e| format!("waiting for the read back: {e}"))?;
        rx.recv_timeout(WAIT).map_err(|e| e.to_string())?.map_err(|e| format!("read back: {e}"))?;
        let mapped = slice.get_mapped_range().map_err(|e| format!("read back: {e}"))?;
        for row in mapped.chunks_exact(stride as usize) {
            out.extend_from_slice(&row[..(w * 4) as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(())
    }
}

#[derive(Default)]
pub struct ImageSet(Mutex<HashMap<u64, Weak<GpuImage>>>);

impl ImageSet {
    pub fn find(&self, id: u64) -> Option<Arc<GpuImage>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).get(&id)?.upgrade()
    }

    pub fn live(&self) -> Vec<Arc<GpuImage>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).values().filter_map(Weak::upgrade).collect()
    }

    pub fn any_wide(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).values().filter_map(Weak::upgrade).any(|i| i.wide().is_some())
    }

    fn is_empty(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }
}

pub struct GpuImage {
    pub texture: wgpu::Texture,
    exported: Option<Exported>,
    id: u64,
    set: Weak<ImageSet>,
    generation: AtomicU64,
    copied: AtomicU64,
    opaque: AtomicBool,
    unsettled: Mutex<Option<Settle>>,
    wide: Mutex<Option<wgpu::Texture>>,
}

type Settle = Box<dyn FnOnce() + Send>;

impl AsRef<[u8]> for GpuImage {
    fn as_ref(&self) -> &[u8] {
        &[]
    }
}

impl GpuImage {
    pub fn data(data: peniko::Blob<u8>, width: u32, height: u32) -> peniko::ImageData {
        peniko::ImageData { data, format: peniko::ImageFormat::Rgba8, alpha_type: peniko::ImageAlphaType::Alpha, width, height }
    }

    pub fn new(texture: wgpu::Texture, set: &Arc<ImageSet>) -> Arc<Self> {
        Self::with_export(texture, None, set)
    }

    pub fn with_export(texture: wgpu::Texture, exported: Option<Exported>, set: &Arc<ImageSet>) -> Arc<Self> {
        let id = peniko::Blob::<u8>::new(Arc::new(Vec::<u8>::new())).id();
        let image = Arc::new(Self {
            texture,
            exported,
            id,
            set: Arc::downgrade(set),
            generation: AtomicU64::new(0),
            copied: AtomicU64::new(0),
            opaque: AtomicBool::new(false),
            unsettled: Mutex::new(None),
            wide: Mutex::new(None),
        });
        set.0.lock().unwrap_or_else(|e| e.into_inner()).insert(id, Arc::downgrade(&image));
        image
    }

    pub fn image(self: &Arc<Self>) -> peniko::ImageData {
        peniko::ImageData {
            data: peniko::Blob::from_raw_parts(self.clone(), self.id),
            format: peniko::ImageFormat::Rgba8,
            alpha_type: peniko::ImageAlphaType::Alpha,
            width: self.texture.width(),
            height: self.texture.height(),
        }
    }

    pub fn brush(self: &Arc<Self>) -> peniko::ImageBrush {
        peniko::ImageBrush { image: self.image(), sampler: peniko::ImageSampler::default() }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn exported(&self) -> Option<&Exported> {
        self.exported.as_ref()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn painted(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    pub fn copied(&self) -> bool {
        self.copied.load(Ordering::Acquire) >= self.generation()
    }

    pub fn mark_copied(&self, generation: u64) {
        self.copied.fetch_max(generation, Ordering::Release);
    }

    pub fn opaque(&self) -> bool {
        self.opaque.load(Ordering::Acquire)
    }

    pub fn set_opaque(&self, opaque: bool) {
        self.opaque.store(opaque, Ordering::Release);
    }

    pub fn defer(&self, settle: Option<Settle>) {
        *self.unsettled.lock().unwrap_or_else(|e| e.into_inner()) = settle;
    }

    pub fn settle(&self) {
        let settle = self.unsettled.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(settle) = settle {
            settle();
        }
    }

    pub fn wide(&self) -> Option<wgpu::Texture> {
        self.wide.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_wide(&self, texture: Option<wgpu::Texture>) {
        *self.wide.lock().unwrap_or_else(|e| e.into_inner()) = texture;
    }
}

impl Drop for GpuImage {
    fn drop(&mut self) {
        if let Some(set) = self.set.upgrade() {
            set.0.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id);
        }
    }
}

use super::Vello;
use crate::gpu::{Paint, PaintScene as _, RenderCommand, Scene};
use kurbo::Affine;
use wgpu::util::DeviceExt;

const USAGE: wgpu::TextureUsages =
    wgpu::TextureUsages::STORAGE_BINDING.union(wgpu::TextureUsages::TEXTURE_BINDING).union(wgpu::TextureUsages::COPY_SRC).union(wgpu::TextureUsages::COPY_DST);

pub struct Overrides {
    set: Arc<ImageSet>,
    seen: HashMap<u64, (u64, u32, u32)>,
    copying: Vec<(Arc<GpuImage>, u64)>,
    build: u64,
}

impl From<Arc<ImageSet>> for Overrides {
    fn from(set: Arc<ImageSet>) -> Self {
        Self { set, seen: HashMap::new(), copying: Vec::new(), build: 0 }
    }
}

impl Overrides {
    pub fn images(&self) -> &Arc<ImageSet> {
        &self.set
    }

    pub(super) fn follow(&mut self, build: u64) {
        if self.build != build {
            self.seen.clear();
            self.build = build;
        }
    }

    pub fn apply(&mut self, vello: &mut vello::Renderer, scene: &Scene) {
        if self.set.is_empty() {
            return;
        }
        for command in &scene.commands {
            let brush = match command {
                RenderCommand::Fill(c) => &c.brush,
                RenderCommand::Stroke(c) => &c.brush,
                RenderCommand::GlyphRun(c) => &c.brush,
                _ => continue,
            };
            let Paint::Image(brush) = brush else { continue };
            let id = brush.image.data.id();
            let Some(image) = self.set.find(id) else { continue };
            image.settle();
            let generation = image.generation();
            match self.seen.get_mut(&id) {
                Some((seen, ..)) if *seen == generation => {}
                Some((seen, ..)) => {
                    *seen = generation;
                    vello.mark_override_image_dirty(&brush.image);
                    self.copying.push((image, generation));
                }
                None => {
                    let texture = wgpu::TexelCopyTextureInfoBase {
                        texture: image.texture.clone(),
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    };
                    vello.override_image(&brush.image, Some(texture));
                    self.seen.insert(id, (generation, brush.image.width, brush.image.height));
                    self.copying.push((image, generation));
                }
            }
        }
    }

    pub fn submitted(&mut self) {
        for (image, generation) in self.copying.drain(..) {
            image.mark_copied(generation);
        }
    }

    pub fn forget(&mut self, vello: &mut vello::Renderer) {
        let set = &self.set;
        let gone: Vec<_> = self.seen.iter().filter(|(id, _)| set.find(**id).is_none()).map(|(id, (_, w, h))| (*id, *w, *h)).collect();
        for (id, width, height) in gone {
            self.seen.remove(&id);
            vello.override_image(&GpuImage::data(peniko::Blob::from_raw_parts(Arc::new(Vec::<u8>::new()), id), width, height), None);
        }
    }
}

struct Blur {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

pub struct Offscreen {
    pub shared: Arc<Shared>,
    vello: Vello,
    overrides: Overrides,
    blur: Option<Blur>,
    filters: HashMap<&'static str, Blur>,
}

impl Offscreen {
    pub fn new(shared: Arc<Shared>) -> Result<Self, String> {
        let vello = Vello::new(&shared.device, &shared.queue)?;
        super::pipelines::save(&shared.device);
        let overrides = shared.images.clone().into();
        Ok(Self { shared, vello, overrides, blur: None, filters: HashMap::new() })
    }

    pub fn draw(&mut self, scene: Scene, size: (u32, u32), place: Affine) -> Result<Arc<GpuImage>, String> {
        if size.0 == 0 || size.1 == 0 {
            return Err("an empty image".into());
        }
        let (texture, exported) = self.shared.image(size, "acde offscreen");
        if size.0 <= WHOLE && size.1 <= WHOLE {
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.vello.render([(scene, place)], &mut self.overrides, &view, size, peniko::Color::TRANSPARENT, Some(&self.shared))?;
        } else {
            self.tiled(&scene, size, place, &texture)?;
        }
        Ok(GpuImage::with_export(texture, exported, &self.shared.images))
    }

    fn tiled(&mut self, scene: &Scene, (width, height): (u32, u32), place: Affine, target: &wgpu::Texture) -> Result<(), String> {
        let side = if super::fit::Fit::has_large(scene, &self.shared.images) { TILE_WITH_IMAGES } else { TILE };
        let scratch = self.shared.texture((width.min(side), height.min(side)), "acde offscreen tile");
        let view = scratch.create_view(&wgpu::TextureViewDescriptor::default());
        for y in (0..height).step_by(side as usize) {
            for x in (0..width).step_by(side as usize) {
                let part = ((width - x).min(side), (height - y).min(side));
                let at = Affine::translate((-f64::from(x), -f64::from(y))) * place;
                self.vello.render([(scene.clone(), at)], &mut self.overrides, &view, part, peniko::Color::TRANSPARENT, Some(&self.shared))?;
                let mut encoder = self.shared.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde offscreen tile") });
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo { texture: &scratch, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyTextureInfo { texture: target, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
                    wgpu::Extent3d { width: part.0, height: part.1, depth_or_array_layers: 1 },
                );
                let index = self.shared.submit([encoder.finish()]);
                self.shared
                    .device
                    .poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: Some(WAIT) })
                    .map_err(|e| format!("waiting for a tile: {e}"))?;
            }
        }
        Ok(())
    }

    pub fn shadow(&mut self, scene: Scene, size: (u32, u32), place: Affine, sigma: f64, color: [f32; 4]) -> Result<Arc<GpuImage>, String> {
        let shape = self.draw(scene, size, place)?;
        Ok(self.blurred(&shape.texture, sigma, color))
    }

    pub fn filtered(&mut self, scene: Scene, size: (u32, u32), place: Affine, filter: &crate::gpu::Filter) -> Result<Arc<GpuImage>, String> {
        let mut image = self.draw(scene, size, place)?;
        for effect in filter.effects() {
            image = match *effect {
                crate::gpu::FilterEffect::GaussianBlur(b) => self.blur_rgba(&image.texture, f64::from(b.std_deviation)),
                crate::gpu::FilterEffect::ColorMatrix(m) => self.color_matrix(&image.texture, &m.0),
                crate::gpu::FilterEffect::DropShadow(d) => {
                    let c = d.color.components;
                    let shadow = self.blurred(&image.texture, f64::from(d.std_deviation), c);
                    let mut both = Scene::new();
                    both.draw_image(shadow.brush().as_ref(), Affine::translate((f64::from(d.dx), f64::from(d.dy))));
                    both.draw_image(image.brush().as_ref(), Affine::IDENTITY);
                    self.draw(both, size, Affine::IDENTITY)?
                }
            };
        }
        Ok(image)
    }

    fn filter_pipeline(&mut self, source: &'static str, entry: &'static str) -> (wgpu::ComputePipeline, wgpu::BindGroupLayout) {
        let device = &self.shared.device;
        let made = self.filters.entry(entry).or_insert_with(|| {
            let cache = super::pipelines::of(device);
            let module = device
                .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("acde canvas filter"), source: wgpu::ShaderSource::Wgsl(source.into()) });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("acde canvas filter"),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: cache.as_ref(),
            });
            let layout = pipeline.get_bind_group_layout(0);
            Blur { pipeline, layout }
        });
        (made.pipeline.clone(), made.layout.clone())
    }

    fn compute(&mut self, source: &'static str, entry: &'static str, passes: &[(&wgpu::Texture, &wgpu::Texture, Vec<u8>)]) {
        let (pipeline, layout) = self.filter_pipeline(source, entry);
        let device = &self.shared.device;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde canvas filter") });
        for (from, to, bytes) in passes {
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("acde canvas filter"),
                contents: bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let input = from.create_view(&wgpu::TextureViewDescriptor::default());
            let output = to.create_view(&wgpu::TextureViewDescriptor::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("acde canvas filter"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&output) },
                    wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("acde canvas filter"), timestamp_writes: None });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(from.width().div_ceil(8), from.height().div_ceil(8), 1);
        }
        self.shared.submit([encoder.finish()]);
    }

    fn blur_rgba(&mut self, src: &wgpu::Texture, sigma: f64) -> Arc<GpuImage> {
        let size = (src.width(), src.height());
        let tmp = self.shared.texture_with(size, USAGE, "acde canvas blur");
        let out = self.shared.texture_with(size, USAGE, "acde canvas blur");
        let radius = (3.0 * sigma).ceil().clamp(0.0, MAX_RADIUS) as i32;
        let params = |dir: [i32; 2]| {
            let mut bytes = Vec::with_capacity(32);
            for v in [0.0f32, 0.0, 0.0, 0.0, sigma.max(0.01) as f32] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            for v in [radius].into_iter().chain(dir) {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            bytes
        };
        self.compute(include_str!("blur.wgsl"), "blur_rgba", &[(src, &tmp, params([1, 0])), (&tmp, &out, params([0, 1]))]);
        GpuImage::new(out, &self.shared.images)
    }

    fn color_matrix(&mut self, src: &wgpu::Texture, m: &[f32; 20]) -> Arc<GpuImage> {
        let out = self.shared.texture_with((src.width(), src.height()), USAGE, "acde canvas color matrix");
        let mut bytes = Vec::with_capacity(80);
        for row in 0..4 {
            for v in &m[row * 5..row * 5 + 4] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        for row in 0..4 {
            bytes.extend_from_slice(&m[row * 5 + 4].to_le_bytes());
        }
        self.compute(include_str!("matrix.wgsl"), "color_matrix", &[(src, &out, bytes)]);
        GpuImage::new(out, &self.shared.images)
    }

    pub fn trim(&mut self) -> Result<bool, String> {
        self.vello.trim()
    }

    pub fn rasterize(&mut self, scene: Scene, (width, height): (u32, u32)) -> Vec<u8> {
        let drawn = self.draw(scene, (width, height), Affine::IDENTITY).and_then(|image| self.shared.read(&image.texture, [0, 0, width, height]));
        let mut rgba = drawn.unwrap_or_else(|_| vec![0; width as usize * height as usize * 4]);
        for px in rgba.as_chunks_mut::<4>().0 {
            let a = u32::from(px[3]);
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * a + 127) / 255) as u8;
            }
        }
        rgba
    }

    fn blur_pipeline(&mut self) -> &Blur {
        let device = &self.shared.device;
        self.blur.get_or_insert_with(|| {
            let cache = super::pipelines::of(device);
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("acde shadow blur"),
                source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("acde shadow blur"),
                layout: None,
                module: &module,
                entry_point: Some("blur"),
                compilation_options: Default::default(),
                cache: cache.as_ref(),
            });
            let layout = pipeline.get_bind_group_layout(0);
            Blur { pipeline, layout }
        })
    }

    fn blurred(&mut self, src: &wgpu::Texture, sigma: f64, color: [f32; 4]) -> Arc<GpuImage> {
        let size = (src.width(), src.height());
        let tmp = self.shared.texture_with(size, USAGE, "acde shadow blur");
        let out = self.shared.texture_with(size, USAGE, "acde shadow");
        let radius = (3.0 * sigma).ceil().clamp(0.0, MAX_RADIUS) as i32;
        let sigma = sigma.max(0.01) as f32;
        let passes = [(src, &tmp, [1i32, 0], [0.0, 0.0, 0.0, 1.0]), (&tmp, &out, [0, 1], color)];
        let blur = self.blur_pipeline();
        let (pipeline, layout) = (blur.pipeline.clone(), blur.layout.clone());
        let device = &self.shared.device;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde shadow blur") });
        for (from, to, dir, color) in passes {
            let mut bytes = Vec::with_capacity(32);
            for v in color.into_iter().chain([sigma]) {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            for v in [radius].into_iter().chain(dir) {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("acde shadow blur"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let input = from.create_view(&wgpu::TextureViewDescriptor::default());
            let output = to.create_view(&wgpu::TextureViewDescriptor::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("acde shadow blur"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&output) },
                    wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("acde shadow blur"), timestamp_writes: None });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(size.0.div_ceil(8), size.1.div_ceil(8), 1);
        }
        self.shared.submit([encoder.finish()]);
        GpuImage::new(out, &self.shared.images)
    }
}
