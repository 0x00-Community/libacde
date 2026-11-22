use std::collections::HashMap;

use super::Blur;
use crate::gpu::glass::Glass;
use crate::gpu::glass::group::{GlassGroup, UNIFORM_BYTES};
use crate::gpu::glass::lens::{self, Key, LensTable};
use crate::gpu::{Backdrop, Effect, Px, Shadow};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Params {
    texel: [f32; 2],
    offset: f32,
    spare0: f32,
    rect: [f32; 4],
    radius: f32,
    exponent: f32,
    shift: [f32; 2],
    color: [f32; 4],
    matrix: [f32; 16],
    bias: [f32; 4],
    outline: [f32; 4],
    table: [f32; 4],
}

impl Params {
    const SIZE: u64 = 176;

    fn bytes(&self) -> [u8; Self::SIZE as usize] {
        let head = [
            self.texel[0],
            self.texel[1],
            self.offset,
            self.spare0,
            self.rect[0],
            self.rect[1],
            self.rect[2],
            self.rect[3],
            self.radius,
            self.exponent,
            self.shift[0],
            self.shift[1],
        ];
        let f = head.iter().chain(&self.color).chain(&self.matrix).chain(&self.bias).chain(&self.outline).chain(&self.table);
        let mut out = [0u8; Self::SIZE as usize];
        for (chunk, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(f) {
            *chunk = v.to_le_bytes();
        }
        out
    }
}

const KEEP_FRAMES: u64 = 8;
const GLASS_TAG: u32 = 1 << 20;
const CACHE_BYTES: u64 = 96 << 20;
const MAX_BINDS: usize = 256;

#[derive(Clone, Copy)]
enum Pipe {
    Down,
    Up,
    Backdrop,
    Shadow,
    Place,
    Flatten,
    Filter,
    Clear,
    Mix,
    Glass,
    GlassGroup,
}

struct Draw<'a> {
    pipe: Pipe,
    src: &'a wgpu::TextureView,
    params: Params,
    scissor: Option<[u32; 4]>,
    mix: Option<f32>,
    group: Option<&'a GlassGroup>,
}

struct Cached {
    levels: Vec<(wgpu::Texture, wgpu::TextureView)>,
    bytes: u64,
    used: u64,
}

struct Uniforms {
    buffer: wgpu::Buffer,
    capacity: u64,
    stride: u64,
    size: u64,
    pending: Vec<u8>,
    written: Vec<u8>,
}

impl Uniforms {
    fn new(device: &wgpu::Device, size: u64, capacity: u64) -> Self {
        let stride = size.next_multiple_of(u64::from(device.limits().min_uniform_buffer_offset_alignment));
        Self { buffer: Self::buffer(device, capacity * stride), capacity, stride, size, pending: Vec::new(), written: Vec::new() }
    }

    fn buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acde compose params"),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn push(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> (u32, bool) {
        let grew = self.pending.len() as u64 / self.stride == self.capacity;
        if grew {
            queue.write_buffer(&self.buffer, 0, &self.pending);
            self.pending.clear();
            self.written.clear();
            self.capacity *= 2;
            self.buffer = Self::buffer(device, self.capacity * self.stride);
        }
        let index = self.pending.len() as u64 / self.stride;
        self.pending.extend_from_slice(bytes);
        self.pending.resize(((index + 1) * self.stride) as usize, 0);
        ((index * self.stride) as u32, grew)
    }

    fn reuse(&self, bytes: &[u8]) -> Option<u32> {
        let stride = self.stride as usize;
        self.pending.chunks(stride).position(|slot| slot[..bytes.len()] == *bytes).map(|i| (i * stride) as u32)
    }

    fn flush(&mut self, queue: &wgpu::Queue) {
        if self.pending.is_empty() {
            return;
        }
        if !self.written.starts_with(&self.pending) {
            queue.write_buffer(&self.buffer, 0, &self.pending);
            self.written.clone_from(&self.pending);
        }
        self.pending.clear();
    }

    fn binding(&self) -> wgpu::BindingResource<'_> {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.buffer, offset: 0, size: wgpu::BufferSize::new(self.size) })
    }
}

pub enum Op {
    Place { origin: [u32; 2], at: [u32; 2], clip: [u32; 4] },
    Shadow { shadow: Shadow, rect: [u32; 4], at: [u32; 2], clip: [u32; 4], blurred: wgpu::TextureView },
}

pub struct Composer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipelines: Vec<wgpu::RenderPipeline>,
    layer: Option<((u32, u32), wgpu::Texture, wgpu::TextureView)>,
    uniforms: Uniforms,
    pieces: Uniforms,
    lens_table: LensTable,
    lens_rows: wgpu::TextureView,
    binds: HashMap<wgpu::TextureView, (wgpu::BindGroup, u64)>,
    textures: HashMap<(u32, u32, u32, u32), Cached>,
    frame: u64,
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Composer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("acde compose"), source: wgpu::ShaderSource::Wgsl(shader_source().into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("acde compose"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(Params::SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES as u64),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("acde compose"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let over = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let toward = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::OneMinusConstant,
            operation: wgpu::BlendOperation::Add,
        };
        let mix = wgpu::BlendState { color: toward, alpha: toward };
        let pipeline = |entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = vec![
            pipeline("fs_down", None),
            pipeline("fs_up", None),
            pipeline("fs_backdrop", Some(over)),
            pipeline("fs_shadow", Some(over)),
            pipeline("fs_place", Some(over)),
            pipeline("fs_place", None),
            pipeline("fs_filter", Some(over)),
            pipeline("fs_clear", None),
            pipeline("fs_up", Some(mix)),
            pipeline("fs_glass", Some(over)),
            pipeline("fs_glass_group", Some(over)),
        ];
        Self {
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("acde compose"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            uniforms: Uniforms::new(device, Params::SIZE, 64),
            pieces: Uniforms::new(device, UNIFORM_BYTES as u64, 4),
            lens_table: LensTable::default(),
            lens_rows: lens_rows(device),
            device: device.clone(),
            queue: queue.clone(),
            pipelines,
            layout,
            layer: None,
            binds: HashMap::new(),
            textures: HashMap::new(),
            frame: 0,
        }
    }

    pub fn layer_view(&mut self, (width, height): (u32, u32)) -> wgpu::TextureView {
        if self.layer.as_ref().is_none_or(|((w, h), _, _)| *w < width || *h < height) {
            let size = self.layer.as_ref().map_or((width, height), |((w, h), _, _)| ((*w).max(width), (*h).max(height)));
            let usage =
                wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
            let texture = super::texture(&self.device, size, usage, "acde layer");
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.layer = Some((size, texture, view));
        }
        self.layer.as_ref().map(|(_, _, v)| v.clone()).expect("made just above")
    }

    fn push(&mut self, params: &Params) -> u32 {
        let (offset, grew) = self.uniforms.push(&self.device, &self.queue, &params.bytes());
        if grew {
            self.binds.clear();
        }
        offset
    }

    fn push_group(&mut self, group: &GlassGroup) -> u32 {
        let bytes = group.to_uniform();
        if let Some(offset) = self.pieces.reuse(&bytes) {
            return offset;
        }
        let (offset, grew) = self.pieces.push(&self.device, &self.queue, &bytes);
        if grew {
            self.binds.clear();
        }
        offset
    }

    fn bind(&mut self, src: &wgpu::TextureView) -> wgpu::BindGroup {
        if self.binds.len() >= MAX_BINDS && !self.binds.contains_key(src) {
            self.binds.clear();
        }
        let frame = self.frame;
        let (layout, sampler, device) = (&self.layout, &self.sampler, &self.device);
        let (params, pieces, rows) = (self.uniforms.binding(), self.pieces.binding(), &self.lens_rows);
        let entry = self.binds.entry(src.clone()).or_insert_with(|| {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("acde compose"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: params },
                    wgpu::BindGroupEntry { binding: 3, resource: pieces },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(rows) },
                ],
            });
            (group, frame)
        });
        entry.1 = frame;
        entry.0.clone()
    }

    pub fn flush(&mut self) {
        self.uniforms.flush(&self.queue);
        self.pieces.flush(&self.queue);
    }

    pub fn end_frame(&mut self) {
        let frame = self.frame;
        self.binds.retain(|_, (_, used)| *used + KEEP_FRAMES > frame);
        self.textures.retain(|_, c| c.used + KEEP_FRAMES > frame);
        self.frame += 1;
    }

    pub fn trim(&mut self) {
        self.binds.clear();
        self.textures.clear();
        self.layer = None;
        let _ = self.device.poll(wgpu::PollType::Poll);
    }

    fn pass(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, clear: bool, draws: &[Draw]) {
        if draws.is_empty() && !clear {
            return;
        }
        let prepared: Vec<(wgpu::BindGroup, [u32; 2], &Draw)> = draws
            .iter()
            .map(|d| {
                let offset = self.push(&d.params);
                let pieces = d.group.map_or(0, |g| self.push_group(g));
                (self.bind(d.src), [offset, pieces], d)
            })
            .collect();
        let load = if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("acde compose"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let size = target.texture().size();
        for (bind, offsets, d) in &prepared {
            pass.set_pipeline(&self.pipelines[d.pipe as usize]);
            pass.set_bind_group(0, bind, offsets);
            if let Some(mix) = d.mix {
                let m = f64::from(mix);
                pass.set_blend_constant(wgpu::Color { r: m, g: m, b: m, a: m });
            }
            let [x, y, w, h] = d.scissor.unwrap_or([0, 0, size.width, size.height]);
            pass.set_scissor_rect(x, y, w, h);
            pass.draw(0..3, 0..1);
        }
    }

    pub fn clear(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView) {
        self.pass(encoder, canvas, true, &[]);
    }

    fn region(rect: [f32; 4], size: wgpu::Extent3d) -> Option<[u32; 4]> {
        let x0 = rect[0].max(0.0).floor() as u32;
        let y0 = rect[1].max(0.0).floor() as u32;
        let x1 = ((rect[0] + rect[2]).max(0.0).ceil() as u32).min(size.width);
        let y1 = ((rect[1] + rect[3]).max(0.0).ceil() as u32).min(size.height);
        (x1 > x0 + 1 && y1 > y0 + 1).then(|| [x0, y0, x1 - x0, y1 - y0])
    }

    fn textures(&mut self, w: u32, h: u32, levels: u32, tag: u32) -> Vec<wgpu::TextureView> {
        let frame = self.frame;
        if !self.textures.contains_key(&(w, h, levels, tag)) {
            let chain: Vec<(wgpu::Texture, wgpu::TextureView)> = (0..=levels)
                .map(|i| {
                    let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::COPY_SRC;
                    let texture = super::texture(&self.device, ((w >> i).max(1), (h >> i).max(1)), usage, "acde blur");
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    (texture, view)
                })
                .collect();
            let bytes = chain.iter().map(|(t, _)| u64::from(t.width()) * u64::from(t.height()) * 4).sum();
            self.evict(bytes);
            self.textures.insert((w, h, levels, tag), Cached { levels: chain, bytes, used: frame });
        }
        let cached = self.textures.get_mut(&(w, h, levels, tag)).expect("made just above");
        cached.used = frame;
        cached.levels.iter().map(|(_, v)| v.clone()).collect()
    }

    fn evict(&mut self, bytes: u64) {
        let mut total: u64 = self.textures.values().map(|c| c.bytes).sum();
        while total + bytes > CACHE_BYTES {
            let Some(key) = self.textures.iter().min_by_key(|(_, c)| c.used).map(|(k, _)| *k) else { break };
            if let Some(c) = self.textures.remove(&key) {
                total -= c.bytes;
                for (_, view) in &c.levels {
                    self.binds.remove(view);
                }
            }
        }
    }

    fn blur(&mut self, encoder: &mut wgpu::CommandEncoder, src: &wgpu::Texture, region: [u32; 4], sigma: f32) -> wgpu::TextureView {
        self.blur_tagged(encoder, src, region, sigma, 0)
    }

    fn blur_tagged(&mut self, encoder: &mut wgpu::CommandEncoder, src: &wgpu::Texture, [x0, y0, w, h]: [u32; 4], sigma: f32, tag: u32) -> wgpu::TextureView {
        let blur = Blur::of(sigma);
        let levels = blur.map_or(0, |b| b.levels);
        let views = self.textures(w, h, levels, tag);
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo { texture: src, mip_level: 0, origin: wgpu::Origin3d { x: x0, y: y0, z: 0 }, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyTextureInfo { texture: views[0].texture(), mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let Some(blur) = blur else { return views.into_iter().next().expect("at least one level") };
        let texel = |v: &wgpu::TextureView| [1.0 / v.texture().width() as f32, 1.0 / v.texture().height() as f32];
        let offset = blur.offset;
        for i in 0..levels as usize {
            let params = Params { texel: texel(&views[i]), offset, ..Default::default() };
            self.pass(encoder, &views[i + 1], true, &[Draw { pipe: Pipe::Down, src: &views[i], params, scissor: None, mix: None, group: None }]);
        }
        for i in (0..levels as usize).rev() {
            let params = Params { texel: texel(&views[i + 1]), offset, ..Default::default() };
            let mix = (i == 0 && blur.mix < 1.0).then_some(blur.mix);
            let pipe = if mix.is_some() { Pipe::Mix } else { Pipe::Up };
            self.pass(encoder, &views[i], mix.is_none(), &[Draw { pipe, src: &views[i + 1], params, scissor: None, mix, group: None }]);
        }
        views.into_iter().next().expect("at least one level")
    }

    pub fn place(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView, origin: [u32; 2], at: [u32; 2], clip: [u32; 4]) {
        self.draw(encoder, canvas, &[Op::Place { origin, at, clip }]);
    }

    pub fn place_from(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView, src: &wgpu::TextureView, origin: [u32; 2], clip: [u32; 4]) {
        let params = Params { rect: [origin[0] as f32, origin[1] as f32, 0.0, 0.0], ..Default::default() };
        self.pass(encoder, canvas, false, &[Draw { pipe: Pipe::Place, src, params, scissor: Some(clip), mix: None, group: None }]);
    }

    pub fn draw(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView, ops: &[Op]) {
        let Some((_, _, layer)) = self.layer.clone() else { return };
        let size = canvas.texture().size();
        let draws: Vec<Draw> = ops
            .iter()
            .filter_map(|op| match *op {
                Op::Place { origin, at, clip } => {
                    let params = Params { rect: [origin[0] as f32 - at[0] as f32, origin[1] as f32 - at[1] as f32, 0.0, 0.0], ..Default::default() };
                    Some(Draw { pipe: Pipe::Place, src: &layer, params, scissor: Some(clip), mix: None, group: None })
                }
                Op::Shadow { shadow: s, rect: [x, y, w, h], at, clip, ref blurred } => {
                    if w < 2 || h < 2 {
                        return None;
                    }
                    let rect = [x as f32, y as f32, w as f32, h as f32];
                    let shifted = Self::region([rect[0] + s.dx, rect[1] + s.dy, rect[2], rect[3]], size)?;
                    let scissor = Px::of(shifted).intersection(Px::of(clip))?;
                    let atlas = blurred.texture().size();
                    let (bw, bh) = (atlas.width as f32, atlas.height as f32);
                    let uv = [at[0] as f32 / bw, at[1] as f32 / bh, w as f32 / bw, h as f32 / bh];
                    let params = Params { rect, shift: [s.dx, s.dy], color: s.color, bias: uv, ..Default::default() };
                    Some(Draw { pipe: Pipe::Shadow, src: blurred, params, scissor: Some(scissor.xywh()), mix: None, group: None })
                }
            })
            .collect();
        self.pass(encoder, canvas, false, &draws);
    }

    pub fn filtered(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        canvas: &wgpu::TextureView,
        e: &Effect,
        [x, y, w, h]: [u32; 4],
        at: [u32; 2],
        clip: [u32; 4],
    ) {
        let Some((_, _, layer)) = self.layer.clone() else { return };
        if w < 2 || h < 2 {
            return;
        }
        let flat = self.textures(w, h, 0, 0).swap_remove(0);
        let params = Params { rect: [-(at[0] as f32), -(at[1] as f32), 0.0, 0.0], ..Default::default() };
        self.pass(encoder, &flat, true, &[Draw { pipe: Pipe::Flatten, src: &layer, params, scissor: None, mix: None, group: None }]);
        let src = if e.blur > 0.0 { self.blur(encoder, flat.texture(), [0, 0, w, h], e.blur) } else { flat };
        let (matrix, bias) = e.columns();
        let params = Params { rect: [x as f32, y as f32, w as f32, h as f32], matrix, bias, ..Default::default() };
        self.pass(encoder, canvas, false, &[Draw { pipe: Pipe::Filter, src: &src, params, scissor: Some(clip), mix: None, group: None }]);
    }

    pub fn clear_rects(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView, rects: impl IntoIterator<Item = [u32; 4]>) {
        self.fill_rects(encoder, canvas, [0.0; 4], rects);
    }

    pub fn fill_rects(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView, color: [f32; 4], rects: impl IntoIterator<Item = [u32; 4]>) {
        let Some((_, _, layer)) = self.layer.clone() else { return };
        let params = Params { color, ..Default::default() };
        let draws: Vec<Draw> = rects.into_iter().map(|r| Draw { pipe: Pipe::Clear, src: &layer, params, scissor: Some(r), mix: None, group: None }).collect();
        self.pass(encoder, canvas, false, &draws);
    }

    pub fn backdrop(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView, b: &Backdrop) {
        if let Some(group) = b.group.as_ref().filter(|g| !g.is_empty()) {
            return self.glass_group(encoder, canvas, b, group);
        }
        if let Some(glass) = b.glass {
            return self.glass(encoder, canvas, b, &glass);
        }
        let Some(region) = Self::region(b.rect, canvas.texture().size()) else { return };
        let blurred = self.blur(encoder, canvas.texture(), region, b.effect.blur);
        let (matrix, bias) = b.effect.columns();
        let params = Params { rect: b.rect, radius: b.radius, exponent: b.exponent, matrix, bias, ..Default::default() };
        self.pass(encoder, canvas, false, &[Draw { pipe: Pipe::Backdrop, src: &blurred, params, scissor: Some(region), mix: None, group: None }]);
    }

    fn glass_under(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        canvas: &wgpu::TextureView,
        b: &Backdrop,
        glass: &Glass,
        reach: f32,
    ) -> Option<([u32; 4], wgpu::TextureView)> {
        let outer = [b.rect[0] - reach, b.rect[1] - reach, b.rect[2] + 2.0 * reach, b.rect[3] + 2.0 * reach];
        let region = Self::region(outer, canvas.texture().size())?;
        let [_, _, w, h] = region;
        let mixed = self.mixed_blurs(encoder, canvas, region, glass, b.effect.blur);
        if b.effect.matrix.is_none() {
            return Some((region, mixed));
        }
        let filtered = self.textures(w, h, 0, GLASS_TAG + 8).swap_remove(0);
        let (matrix, bias) = b.effect.columns();
        let params = Params { rect: [0.0, 0.0, w as f32, h as f32], matrix, bias, ..Default::default() };
        self.pass(encoder, &filtered, true, &[Draw { pipe: Pipe::Filter, src: &mixed, params, scissor: None, mix: None, group: None }]);
        Some((region, filtered))
    }

    fn glass(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView, b: &Backdrop, glass: &Glass) {
        let Some((region, filtered)) = self.glass_under(encoder, canvas, b, glass, glass.reach() as f32) else { return };
        let [x, y, w, h] = region;
        let (r, bleed, light) = (glass.refraction, glass.bleed, glass.highlight);
        let matrix = [
            r.inner_amount,
            r.inner_height,
            r.outer_amount,
            r.outer_height,
            bleed.amount,
            bleed.radius,
            bleed.opacity,
            light.angle,
            light.amount,
            glass.white_point,
            glass.smoothing,
            light.width,
            glass.lens.thickness,
            glass.lens.dispersion,
            glass.darken,
            glass.lens.bezel,
        ];
        let bias = [x as f32, y as f32, w as f32, h as f32];
        let params = Params {
            rect: b.rect,
            radius: b.radius,
            exponent: b.exponent,
            shift: [glass.lens.index, glass.outline.width],
            color: glass.tint,
            matrix,
            bias,
            outline: glass.outline.color,
            table: self.lens_slot(glass.lens, b.rect[2].min(b.rect[3]) / 2.0),
            ..Default::default()
        };
        self.pass(encoder, canvas, false, &[Draw { pipe: Pipe::Glass, src: &filtered, params, scissor: Some(region), mix: None, group: None }]);
    }

    fn glass_group(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView, b: &Backdrop, group: &GlassGroup) {
        let Some(glass) = group.glass() else { return };
        let Some((region, filtered)) = self.glass_under(encoder, canvas, b, glass, group.reach() as f32) else { return };
        let half_short = group.pieces().first().map_or(0.0, |p| p.half[0].min(p.half[1]));
        let table = self.lens_slot(glass.lens, half_short);
        let params = Params { shift: [group.len() as f32, group.spacing], bias: region.map(|v| v as f32), table, ..Default::default() };
        let draw = Draw { pipe: Pipe::GlassGroup, src: &filtered, params, scissor: Some(region), mix: None, group: Some(group) };
        self.pass(encoder, canvas, false, &[draw]);
    }

    fn lens_slot(&mut self, glass_lens: crate::gpu::glass::Lens, half_short: f32) -> [f32; 4] {
        let key = Key::of(glass_lens, half_short);
        let missing = [-1.0, key.ratio, key.index, key.dispersion];
        if !glass_lens.bends() {
            return missing;
        }
        let Some((slot, rows)) = self.lens_table.slot(key, self.frame) else { return missing };
        if let Some(rows) = rows {
            let bytes: Vec<u8> = rows.iter().flat_map(|v| v.to_le_bytes()).collect();
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: self.lens_rows.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: (slot.row * 3) as u32, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some((lens::TABLE_SIZE * 4) as u32), rows_per_image: Some(3) },
                wgpu::Extent3d { width: lens::TABLE_SIZE as u32, height: 3, depth_or_array_layers: 1 },
            );
        }
        [slot.row as f32, key.ratio, key.index, key.dispersion]
    }

    fn mixed_blurs(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        canvas: &wgpu::TextureView,
        region: [u32; 4],
        glass: &Glass,
        fallback: f32,
    ) -> wgpu::TextureView {
        let [_, _, w, h] = region;
        let layers: Vec<(usize, f32, f32)> = glass.blur.iter().enumerate().filter(|(_, l)| l.opacity > 0.0).map(|(i, l)| (i, l.radius, l.opacity)).collect();
        let layers = if layers.is_empty() { vec![(0, fallback, 1.0)] } else { layers };
        if let [(i, radius, _)] = layers[..] {
            return self.blur_tagged(encoder, canvas.texture(), region, radius, GLASS_TAG + 1 + i as u32);
        }
        let mixed = self.textures(w, h, 0, GLASS_TAG).swap_remove(0);
        let mut total = 0.0;
        for (n, (i, radius, opacity)) in layers.into_iter().enumerate() {
            let blurred = self.blur_tagged(encoder, canvas.texture(), region, radius, GLASS_TAG + 1 + i as u32);
            total += opacity;
            let texel = [1.0 / w as f32, 1.0 / h as f32];
            let params = Params { texel, offset: 0.0, ..Default::default() };
            let (pipe, mix) = if n == 0 { (Pipe::Up, None) } else { (Pipe::Mix, Some(opacity / total)) };
            self.pass(encoder, &mixed, n == 0, &[Draw { pipe, src: &blurred, params, scissor: None, mix, group: None }]);
        }
        mixed
    }

    pub fn shadow(&mut self, encoder: &mut wgpu::CommandEncoder, canvas: &wgpu::TextureView, s: &Shadow, [x, y, w, h]: [u32; 4], at: [u32; 2], clip: [u32; 4]) {
        let Some((_, layer, _)) = self.layer.clone() else { return };
        if w < 2 || h < 2 {
            return;
        }
        let rect = [x as f32, y as f32, w as f32, h as f32];
        let Some(shifted) = Self::region([rect[0] + s.dx, rect[1] + s.dy, rect[2], rect[3]], layer.size()) else { return };
        let Some(scissor) = Px::of(shifted).intersection(Px::of(clip)) else { return };
        let blurred = self.blur(encoder, &layer, [at[0], at[1], w, h], s.blur);
        let params = Params { rect, shift: [s.dx, s.dy], color: s.color, bias: [0.0, 0.0, 1.0, 1.0], ..Default::default() };
        self.pass(encoder, canvas, false, &[Draw { pipe: Pipe::Shadow, src: &blurred, params, scissor: Some(scissor.xywh()), mix: None, group: None }]);
    }

    pub fn blur_layers(&mut self, encoder: &mut wgpu::CommandEncoder, extent: (u32, u32), sigma: f32, tag: u32) -> Option<wgpu::TextureView> {
        let (_, layer, _) = self.layer.clone()?;
        Some(self.blur_tagged(encoder, &layer, [0, 0, extent.0, extent.1], sigma, tag + 1))
    }
}

fn shader_source() -> String {
    [crate::gpu::corner::WGSL, crate::gpu::glass::WGSL, crate::gpu::glass::group::WGSL, lens::TABLE_WGSL, include_str!("compose.wgsl")].join("\n")
}

fn lens_rows(device: &wgpu::Device) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("acde glass lens rows"),
        size: wgpu::Extent3d { width: lens::TABLE_SIZE as u32, height: (lens::TABLE_SLOTS * 3) as u32, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_compose_shader_validates() {
        let source = super::shader_source();
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        let flags = naga::valid::ValidationFlags::all();
        naga::valid::Validator::new(flags, naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    }
}
