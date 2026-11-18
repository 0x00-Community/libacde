use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::gpu::wgpu;
use crate::gpu::{Gpu, GpuImage};

use super::{Id, Kind, Obj, Out, Ret, State, Texture, Throw, desc, enter, finish, json, report, with_state};
use crate::host::bind;
use crate::host::rt::{self, Msg};

const MAX_IMAGES: usize = 6;

pub struct Context {
    device: Id,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
    premultiplied: bool,
    view_formats: Vec<wgpu::TextureFormat>,
    images: Vec<Arc<GpuImage>>,
    page: Option<(wgpu::Texture, wgpu::BindGroup)>,
    current: Option<Current>,
    front: Option<Arc<GpuImage>>,
}

struct Current {
    id: Id,
    texture: wgpu::Texture,
    image: Option<Arc<GpuImage>>,
    clear: bool,
}

impl Context {
    fn direct(&self) -> bool {
        self.format == wgpu::TextureFormat::Rgba8Unorm && !self.premultiplied
    }

    fn free_image(&mut self, gpu: &Gpu, size: (u32, u32), usage: wgpu::TextureUsages, view_formats: &[wgpu::TextureFormat]) -> Arc<GpuImage> {
        self.images.retain(|i| (i.texture.width(), i.texture.height()) == size || Arc::strong_count(i) > 1);
        let front = |i: &Arc<GpuImage>| self.front.as_ref().is_some_and(|f| Arc::ptr_eq(f, i));
        let free = self.images.iter().position(|i| (i.texture.width(), i.texture.height()) == size && !front(i) && (Arc::strong_count(i) == 1 || i.copied()));
        if let Some(i) = free {
            return self.images[i].clone();
        }
        if self.images.len() >= MAX_IMAGES
            && let Some(i) = self.images.iter().position(|i| (i.texture.width(), i.texture.height()) == size && !front(i))
        {
            return self.images[i].clone();
        }
        let texture = texture(gpu, size, wgpu::TextureFormat::Rgba8Unorm, usage | wgpu::TextureUsages::COPY_SRC, view_formats);
        let image = GpuImage::new(texture, &gpu.images);
        self.images.push(image.clone());
        image
    }
}

fn texture(
    gpu: &Gpu,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
    view_formats: &[wgpu::TextureFormat],
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("acde canvas"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: usage | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats,
    })
}

struct Passes {
    // the device they were made on (held, so its address is not reused): after a device loss the next present makes them
    // again on the new one. wgpu::Device's own == does not tell two devices apart.
    gpu: Arc<Gpu>,
    opaque: wgpu::RenderPipeline,
    keep: wgpu::RenderPipeline,
    unpremultiply: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

thread_local! {
    static PASSES: RefCell<Option<Rc<Passes>>> = const { RefCell::new(None) };
}

const SHADER: &str = r"
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn opaque() -> @location(0) vec4f {
    return vec4f(0.0, 0.0, 0.0, 1.0);
}
@group(0) @binding(0) var source: texture_2d<f32>;
@fragment fn keep(@builtin(position) at: vec4f) -> @location(0) vec4f {
    return vec4f(textureLoad(source, vec2i(at.xy), 0).rgb, 1.0);
}
@fragment fn unpremultiply(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let c = textureLoad(source, vec2i(at.xy), 0);
    if c.a <= 0.0 { return vec4f(0.0); }
    return vec4f(c.rgb / c.a, c.a);
}
";

impl Passes {
    fn get(gpu: &Arc<Gpu>) -> Rc<Passes> {
        PASSES.with(|p| {
            let mut slot = p.borrow_mut();
            if slot.as_ref().is_some_and(|passes| !Arc::ptr_eq(&passes.gpu, gpu)) {
                *slot = None;
            }
            slot.get_or_insert_with(|| Rc::new(Self::new(gpu))).clone()
        })
    }

    fn new(gpu: &Arc<Gpu>) -> Self {
        let device = &gpu.device;
        let module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("acde canvas present"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("acde canvas present"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let with_source = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("acde canvas present"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let none =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("acde canvas present"), bind_group_layouts: &[], immediate_size: 0 });
        let pipeline = |entry: &str, layout: &wgpu::PipelineLayout, write_mask: wgpu::ColorWrites| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("acde canvas present"),
                layout: Some(layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            gpu: gpu.clone(),
            opaque: pipeline("opaque", &none, wgpu::ColorWrites::ALPHA),
            keep: pipeline("keep", &with_source, wgpu::ColorWrites::ALL),
            unpremultiply: pipeline("unpremultiply", &with_source, wgpu::ColorWrites::ALL),
            layout,
        }
    }
}

fn pass(encoder: &mut wgpu::CommandEncoder, target: &wgpu::Texture, load: bool, draw: Option<(&wgpu::RenderPipeline, Option<&wgpu::BindGroup>)>) {
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let load = if load { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) };
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("acde canvas"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some((pipeline, group)) = draw {
        pass.set_pipeline(pipeline);
        if let Some(group) = group {
            pass.set_bind_group(0, Some(group), &[]);
        }
        pass.draw(0..3, 0..1);
    }
}

pub fn touch(s: &mut State, encoder: &mut wgpu::CommandEncoder, id: Id, clears: bool) {
    let Some(Obj::Texture(Texture { canvas: Some(node), .. })) = s.objects.get(&id) else { return };
    let Some(current) = s.canvases.get_mut(node).and_then(|c| c.current.as_mut()).filter(|c| c.id == id && c.clear) else { return };
    current.clear = false;
    if !clears {
        pass(encoder, &current.texture, false, None);
    }
}

pub fn touch_now(s: &mut State, gpu: &Gpu, id: Id) {
    let Some(Obj::Texture(Texture { canvas: Some(node), .. })) = s.objects.get(&id) else { return };
    let Some(current) = s.canvases.get_mut(node).and_then(|c| c.current.as_mut()).filter(|c| c.id == id && c.clear) else { return };
    current.clear = false;
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde canvas clear") });
    pass(&mut encoder, &current.texture, false, None);
    gpu.submit([encoder.finish()]);
}

super::natives! {
    methods = methods;

    "preferredFormat" => fn preferred_format(scope, _args) {
        Ok(Ret::Val(bind::string(scope, "rgba8unorm").into()))
    }

    "configure" => fn configure(scope, args) {
        let (node, device) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1));
        let config: desc::CanvasConfig = json(scope, &args, 2)?;
        if !matches!(config.format, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba16Float) {
            return Err(Throw::Type(format!("{:?} is not a canvas format (rgba8unorm, bgra8unorm, rgba16float)", config.format)));
        }
        let premultiplied = config.alpha_mode == desc::AlphaMode::Premultiplied;
        let Some(usage) = wgpu::TextureUsages::from_bits(config.usage) else {
            return Err(Throw::Type(format!("invalid GPUTextureUsage {:#x}", config.usage)));
        };
        enter(device);
        with_state(|s| {
            let front = s.canvases.remove(&node).and_then(|c| c.front);
            s.canvases.insert(node, Context { device, format: config.format, usage, premultiplied, view_formats: config.view_formats, images: Vec::new(), page: None, current: None, front });
        });
        Ok(().into())
    }

    "unconfigure" => fn unconfigure(scope, args) {
        let node = bind::arg_u32(scope, &args, 0);
        with_state(|s| {
            if let Some(Context { current: Some(current), .. }) = s.canvases.remove(&node) {
                s.objects.remove(&current.id);
            }
        });
        Ok(().into())
    }

    "currentTexture" => fn current_texture(scope, args) {
        let node = bind::arg_u32(scope, &args, 0);
        let size = (bind::arg_u32(scope, &args, 1).max(1), bind::arg_u32(scope, &args, 2).max(1));
        let id = with_state(|s| -> Result<Id, Throw> {
            let gpu = s.gpu()?;
            let id = s.id();
            let Some(c) = s.canvases.get_mut(&node) else { return Ok(0) };
            if let Some(current) = &c.current {
                return Ok(current.id);
            }
            enter(c.device);
            let (texture, image) = if c.direct() {
                let view_formats = c.view_formats.clone();
                let image = c.free_image(&gpu, size, c.usage, &view_formats);
                (image.texture.clone(), Some(image))
            } else {
                let fits = c.page.as_ref().is_some_and(|(t, _)| (t.width(), t.height()) == size);
                if !fits {
                    let texture = texture(&gpu, size, c.format, c.usage | wgpu::TextureUsages::TEXTURE_BINDING, &c.view_formats);
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let passes = Passes::get(&gpu);
                    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("acde canvas present"),
                        layout: &passes.layout,
                        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
                    });
                    c.page = Some((texture, group));
                }
                (c.page.as_ref().map(|(t, _)| t.clone()).unwrap_or_else(|| unreachable!()), None)
            };
            c.current = Some(Current { id, texture: texture.clone(), image, clear: true });
            let device = c.device;
            s.objects.insert(id, Obj::Texture(Texture { texture, device, canvas: Some(node) }));
            Ok(id)
        })?;
        Ok(id.into())
    }

    "present" => fn present(scope, args) {
        let node = bind::arg_u32(scope, &args, 0);
        let shown = with_state(|s| -> Result<Option<Arc<GpuImage>>, Throw> {
            let gpu = s.gpu()?;
            let Some(c) = s.canvases.get_mut(&node) else { return Ok(None) };
            let Some(current) = c.current.take() else { return Ok(None) };
            s.objects.remove(&current.id);
            let c = s.canvases.get_mut(&node).unwrap_or_else(|| unreachable!());
            enter(c.device);
            let passes = Passes::get(&gpu);
            let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde canvas present") });
            if current.clear {
                pass(&mut encoder, &current.texture, false, None);
            }
            let image = match current.image {
                Some(image) => {
                    pass(&mut encoder, &image.texture, true, Some((&passes.opaque, None)));
                    image
                }
                None => {
                    let size = (current.texture.width(), current.texture.height());
                    let image = c.free_image(&gpu, size, wgpu::TextureUsages::empty(), &[]);
                    let Some((_, group)) = &c.page else { return Ok(None) };
                    let pipeline = if c.premultiplied { &passes.unpremultiply } else { &passes.keep };
                    pass(&mut encoder, &image.texture, false, Some((pipeline, Some(group))));
                    image
                }
            };
            gpu.submit([encoder.finish()]);
            image.painted();
            if let Some(i) = c.images.iter().position(|i| Arc::ptr_eq(i, &image)) {
                let shown = c.images.remove(i);
                c.images.push(shown);
            }
            c.front = Some(image.clone());
            Ok(Some(image))
        })?;
        if let Some(image) = shown {
            let data = image.image();
            if let Some(Err(e)) = crate::host::dom::with_dom(|dom| dom.canvas_image(node, Some(data))) {
                return Err(Throw::Error(e.to_string()));
            }
        }
        Ok(().into())
    }

    "pixels" => fn pixels(scope, args) {
        let read = front_pixels(bind::arg_u32(scope, &args, 0))?;
        Ok(Ret::Val(pixels_value(scope, read)))
    }

    "png" => fn png(scope, args) {
        let (node, width, height) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1), bind::arg_u32(scope, &args, 2));
        let png = png_of(front_pixels(node)?, width, height);
        Ok(Ret::Val(bind::uint8_array(scope, png).into()))
    }

    "pngAsync" => fn png_async(scope, args) {
        let (node, width, height) = (bind::arg_u32(scope, &args, 0), bind::arg_u32(scope, &args, 1), bind::arg_u32(scope, &args, 2));
        let Some((token, promise)) = super::promise(scope) else { return Ok(().into()) };
        let tx = rt::current().sender();
        with_state(|s| -> Result<(), Throw> {
            let gpu = s.gpu()?;
            let Some(image) = s.canvases.get(&node).and_then(|c| c.front.clone()) else {
                encode_then_resolve(tx, token, None, (width, height), || None);
                return Ok(());
            };
            let read = Readback::copy(&gpu, &image.texture);
            let buffer = read.buffer.clone();
            let device = super::current_device();
            let held = gpu.clone();
            held.hold_poll();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                let ok = result.is_ok();
                encode_then_resolve(tx, token, Some(device), (width, height), move || if ok { read.pixels() } else { None });
                held.release_poll();
            });
            Ok(())
        })?;
        Ok(Ret::Val(promise))
    }
}

fn front_pixels(node: Id) -> Result<Option<(u32, u32, Vec<u8>)>, Throw> {
    with_state(|s| {
        let gpu = s.gpu()?;
        let Some(image) = s.canvases.get(&node).and_then(|c| c.front.clone()) else { return Ok(None) };
        Ok(read_back(&gpu, &image.texture))
    })
}

fn png_of(pixels: Option<(u32, u32, Vec<u8>)>, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width.max(1), height.max(1));
    let (w, h, rgba) = pixels.unwrap_or_else(|| (w, h, vec![0; w as usize * h as usize * 4]));
    crate::render::encode_png(w, h, &rgba).unwrap_or_default()
}

fn encode_then_resolve(
    tx: std::sync::mpsc::Sender<Msg>,
    token: u32,
    device: Option<Id>,
    (width, height): (u32, u32),
    pixels: impl FnOnce() -> Option<(u32, u32, Vec<u8>)> + Send + 'static,
) {
    _ = std::thread::Builder::new().name("acde-gpu-png".into()).spawn(move || {
        let pixels = pixels();
        let failed = device.filter(|_| pixels.is_none());
        let png = png_of(pixels, width, height);
        _ = tx.send(Msg::Task(Box::new(move |scope| {
            if let Some(device) = failed {
                report(device, Kind::Internal, "reading the canvas back failed".into());
            }
            super::resolve(scope, token, |scope| bind::uint8_array(scope, png).into());
        })));
    });
}

fn pixels_value<'s>(scope: &mut v8::PinScope<'s, '_>, read: Option<(u32, u32, Vec<u8>)>) -> v8::Local<'s, v8::Value> {
    let Some((width, height, rgba)) = read else { return v8::null(scope).into() };
    let obj = v8::Object::new(scope);
    let w = v8::Number::new(scope, f64::from(width));
    bind::set(scope, obj, "width", w.into());
    let h = v8::Number::new(scope, f64::from(height));
    bind::set(scope, obj, "height", h.into());
    let rgba = bind::uint8_array(scope, rgba);
    bind::set(scope, obj, "rgba", rgba.into());
    obj.into()
}

struct Readback {
    buffer: wgpu::Buffer,
    index: wgpu::SubmissionIndex,
    width: u32,
    height: u32,
    padded: u32,
}

impl Readback {
    fn copy(gpu: &Gpu, texture: &wgpu::Texture) -> Self {
        let (width, height) = (texture.width(), texture.height());
        let padded = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acde canvas read"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde canvas read") });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(height) },
            },
            texture.size(),
        );
        let index = gpu.submit([encoder.finish()]);
        Self { buffer, index, width, height, padded }
    }

    fn pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        let row = (self.width * 4) as usize;
        let mapped = self.buffer.slice(..).get_mapped_range().ok()?;
        let mut rgba = Vec::with_capacity(row * self.height as usize);
        for y in 0..self.height as usize {
            rgba.extend_from_slice(&mapped[y * self.padded as usize..][..row]);
        }
        Some((self.width, self.height, rgba))
    }
}

fn read_back(gpu: &Gpu, texture: &wgpu::Texture) -> Option<(u32, u32, Vec<u8>)> {
    let read = Readback::copy(gpu, texture);
    let (tx, rx) = std::sync::mpsc::channel();
    read.buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| _ = tx.send(result.is_ok()));
    let timeout = std::time::Duration::from_secs(5);
    _ = gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(read.index.clone()), timeout: Some(timeout) });
    if !rx.recv_timeout(timeout).unwrap_or(false) {
        report(super::current_device(), Kind::Internal, "reading the canvas back failed".into());
        return None;
    }
    read.pixels()
}
