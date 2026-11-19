use crate::webgl::{FastMap, FastSet};
use std::sync::Arc;

use crate::gpu::Gpu;

use crate::webgl::consts::*;
use crate::webgl::objects::*;
use crate::webgl::pass::Recorder;
use crate::webgl::pipeline::Caches;
use crate::webgl::state::State;

#[derive(Clone, Copy, Debug)]
pub struct Attributes {
    pub alpha: bool,
    pub depth: bool,
    pub stencil: bool,
    pub antialias: bool,
    pub premultiplied_alpha: bool,
    pub preserve_drawing_buffer: bool,
}

impl Default for Attributes {
    fn default() -> Self {
        Attributes { alpha: true, depth: true, stencil: false, antialias: true, premultiplied_alpha: true, preserve_drawing_buffer: false }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub texture_size: u32,
    pub texture_3d: u32,
    pub array_layers: u32,
    pub attributes: u32,
    pub units: u32,
    pub varyings: u32,
    pub uniform_vectors: u32,
    pub draw_buffers: u32,
    pub samples: u32,
    pub uniform_bindings: u32,
    pub uniform_blocks: u32,
    pub block_size: u32,
    pub uniform_alignment: u32,
    pub anisotropy: f32,
}

impl Limits {
    fn of(gpu: &Gpu) -> Self {
        let l = gpu.device.limits();
        let units = 16.min(l.max_sampled_textures_per_shader_stage).min(l.max_samplers_per_shader_stage);
        Limits {
            texture_size: l.max_texture_dimension_2d.min(16384),
            texture_3d: l.max_texture_dimension_3d.min(2048),
            array_layers: l.max_texture_array_layers.min(2048),
            attributes: 16.min(l.max_vertex_attributes).min(l.max_vertex_buffers),
            units,
            varyings: l.max_inter_stage_shader_variables.saturating_sub(1),
            uniform_vectors: (l.max_uniform_buffer_binding_size / 16).min(4096) as u32,
            draw_buffers: 8.min(l.max_color_attachments),
            samples: 4,
            uniform_bindings: 24.min(l.max_uniform_buffers_per_shader_stage.saturating_sub(1) * 2),
            uniform_blocks: 12.min(l.max_uniform_buffers_per_shader_stage.saturating_sub(1)),
            block_size: (l.max_uniform_buffer_binding_size as u32).min(65536),
            uniform_alignment: l.min_uniform_buffer_offset_alignment,
            anisotropy: 16.0,
        }
    }
}

pub(crate) type ConvertKey = (Id, u64, u32, u32, bool, bool, u64, u32, u32, u32);

pub struct Drawing {
    pub width: u32,
    pub height: u32,
    pub color: Option<wgpu::Texture>,
    pub color_view: Option<wgpu::TextureView>,
    pub resolve: Option<wgpu::Texture>,
    pub depth: Option<wgpu::Texture>,
    pub depth_view: Option<wgpu::TextureView>,
    pub depth_format: Option<wgpu::TextureFormat>,
    pub samples: u32,
    pub version: u64,
    pub dirty: bool,
    pub cleared: bool,
    pub unresolved: bool,
    pub images: Vec<Arc<crate::gpu::GpuImage>>,
    pub front: Option<Arc<crate::gpu::GpuImage>>,
}

pub struct Context {
    pub gpu: Arc<Gpu>,
    pub webgl2: bool,
    pub attrs: Attributes,
    pub(crate) objects: FastMap<Id, Object>,
    next: Id,
    errors: Vec<u32>,
    pub lost: bool,
    pub(crate) state: State,
    pub(crate) default_vao: VertexArray,
    pub(crate) drawing: Drawing,
    pub(crate) recorder: Recorder,
    pub(crate) caches: Caches,
    pub limits: Limits,
    pub(crate) enabled: FastSet<String>,
    pub(crate) serial: u64,
    pub(crate) generation: u64,
    pub(crate) features: wgpu::Features,
    pub(crate) converted: FastMap<ConvertKey, wgpu::Buffer>,
    pub(crate) warned_depth_blit: bool,
}

impl Context {
    pub fn new(gpu: Arc<Gpu>, webgl2: bool, attrs: Attributes, width: u32, height: u32) -> Self {
        let limits = Limits::of(&gpu);
        let features = gpu.device.features();
        let state = State::new(width, height, (limits.units * 2) as usize, limits.uniform_bindings as usize);
        let caches = Caches::new(&gpu);
        let mut context = Context {
            gpu,
            webgl2,
            attrs,
            objects: FastMap::default(),
            next: 0,
            errors: Vec::new(),
            lost: false,
            state,
            default_vao: VertexArray::new(),
            drawing: Drawing {
                width: 0,
                height: 0,
                color: None,
                color_view: None,
                resolve: None,
                depth: None,
                depth_view: None,
                depth_format: None,
                samples: if attrs.antialias { 4 } else { 1 },
                version: 0,
                dirty: false,
                cleared: true,
                unresolved: false,
                images: Vec::new(),
                front: None,
            },
            recorder: Recorder::default(),
            caches,
            limits,
            enabled: FastSet::default(),
            serial: 0,
            generation: 1,
            features,
            converted: FastMap::default(),
            warned_depth_blit: false,
        };
        context.resize(width, height);
        context
    }

    #[track_caller]
    pub fn error(&mut self, code: u32) {
        log::debug!("webgl: error {code:#x} at {}", std::panic::Location::caller());
        if !self.errors.contains(&code) {
            self.errors.push(code);
        }
    }

    pub fn get_error(&mut self) -> u32 {
        if self.errors.is_empty() { NO_ERROR } else { self.errors.remove(0) }
    }

    pub(crate) fn next_serial(&mut self) -> u64 {
        self.serial += 1;
        self.serial
    }

    pub(crate) fn add(&mut self, object: Object) -> Id {
        self.next += 1;
        self.objects.insert(self.next, object);
        self.next
    }

    pub fn exists(&self, id: Id) -> bool {
        self.objects.contains_key(&id)
    }

    pub(crate) fn buffer(&self, id: Id) -> Option<&Buffer> {
        self.objects.get(&id).and_then(Object::buffer)
    }
    pub(crate) fn buffer_mut(&mut self, id: Id) -> Option<&mut Buffer> {
        self.objects.get_mut(&id).and_then(Object::buffer_mut)
    }
    pub(crate) fn texture(&self, id: Id) -> Option<&Texture> {
        self.objects.get(&id).and_then(Object::texture)
    }
    pub(crate) fn texture_mut(&mut self, id: Id) -> Option<&mut Texture> {
        self.objects.get_mut(&id).and_then(Object::texture_mut)
    }
    pub(crate) fn renderbuffer(&self, id: Id) -> Option<&Renderbuffer> {
        self.objects.get(&id).and_then(Object::renderbuffer)
    }
    pub(crate) fn renderbuffer_mut(&mut self, id: Id) -> Option<&mut Renderbuffer> {
        self.objects.get_mut(&id).and_then(Object::renderbuffer_mut)
    }
    pub(crate) fn framebuffer(&self, id: Id) -> Option<&Framebuffer> {
        self.objects.get(&id).and_then(Object::framebuffer)
    }
    pub(crate) fn framebuffer_mut(&mut self, id: Id) -> Option<&mut Framebuffer> {
        self.objects.get_mut(&id).and_then(Object::framebuffer_mut)
    }
    pub(crate) fn shader(&self, id: Id) -> Option<&Shader> {
        self.objects.get(&id).and_then(Object::shader)
    }
    pub(crate) fn shader_mut(&mut self, id: Id) -> Option<&mut Shader> {
        self.objects.get_mut(&id).and_then(Object::shader_mut)
    }
    pub(crate) fn program(&self, id: Id) -> Option<&Program> {
        self.objects.get(&id).and_then(Object::program)
    }
    pub(crate) fn program_mut(&mut self, id: Id) -> Option<&mut Program> {
        self.objects.get_mut(&id).and_then(Object::program_mut)
    }
    pub(crate) fn sampler_object(&self, id: Id) -> Option<&Sampler> {
        self.objects.get(&id).and_then(Object::sampler)
    }

    pub(crate) fn vao(&self) -> &VertexArray {
        match self.state.vertex_array.and_then(|id| self.objects.get(&id)).and_then(Object::vertex_array) {
            Some(v) => v,
            None => &self.default_vao,
        }
    }

    pub(crate) fn vao_mut(&mut self) -> &mut VertexArray {
        match self.state.vertex_array {
            Some(id) if self.objects.get(&id).and_then(Object::vertex_array).is_some() => {
                self.objects.get_mut(&id).and_then(Object::vertex_array_mut).unwrap_or_else(|| unreachable!())
            }
            _ => &mut self.default_vao,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let max = self.limits.texture_size;
        let (width, height) = (width.clamp(1, max), height.clamp(1, max));
        if self.drawing.color.is_some() && self.drawing.width == width && self.drawing.height == height {
            return;
        }
        self.flush_pass();
        let samples = self.drawing.samples;
        let depth_format = match (self.attrs.depth, self.attrs.stencil) {
            (_, true) => Some(wgpu::TextureFormat::Depth24PlusStencil8),
            (true, false) => Some(wgpu::TextureFormat::Depth24Plus),
            _ => None,
        };
        let created = self.scoped(|device| {
            let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
            let texture = |label, format, samples, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
            let readable = attachment | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
            let color = texture("acde webgl drawing buffer", wgpu::TextureFormat::Rgba8Unorm, samples, if samples > 1 { attachment } else { readable });
            let resolve = (samples > 1).then(|| texture("acde webgl resolve", wgpu::TextureFormat::Rgba8Unorm, 1, readable));
            let depth = depth_format.map(|f| texture("acde webgl depth", f, samples, attachment | wgpu::TextureUsages::TEXTURE_BINDING));
            (color, resolve, depth)
        });
        let (color, resolve, depth) = match created {
            Ok(c) => c,
            Err(e) => {
                log::error!("webgl: drawing buffer {width}x{height}: {e}");
                self.error(OUT_OF_MEMORY);
                return;
            }
        };
        self.drawing.color_view = Some(color.create_view(&wgpu::TextureViewDescriptor::default()));
        self.drawing.color = Some(color);
        self.drawing.resolve = resolve;
        self.drawing.depth_format = depth_format;
        self.drawing.depth_view = depth.as_ref().map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        self.drawing.depth = depth;
        self.drawing.width = width;
        self.drawing.height = height;
        self.drawing.version += 1;
        self.drawing.cleared = false;
        self.drawing.dirty = true;
    }

    pub fn drawing_dirty(&self) -> bool {
        self.drawing.dirty
    }

    pub fn drawing_size(&self) -> (u32, u32) {
        (self.drawing.width, self.drawing.height)
    }

    pub fn lose(&mut self) {
        self.recorder = Recorder::default();
        self.lost = true;
        self.objects.clear();
    }

    pub fn enable_extension(&mut self, name: &str) {
        self.enabled.insert(name.to_owned());
    }

    pub fn extension(&self, name: &str) -> bool {
        self.enabled.contains(name)
    }

    pub fn supported_extensions(&self) -> Vec<&'static str> {
        let f = self.features;
        let mut out = vec![
            "EXT_texture_filter_anisotropic",
            "WEBGL_lose_context",
            "WEBGL_debug_renderer_info",
            "OES_texture_float_linear",
            "EXT_float_blend",
            "KHR_parallel_shader_compile",
            "WEBGL_multi_draw",
            "EXT_texture_norm16",
        ];
        if !f.contains(wgpu::Features::FLOAT32_FILTERABLE) {
            out.retain(|e| *e != "OES_texture_float_linear");
        }
        if !f.contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM) {
            out.retain(|e| *e != "EXT_texture_norm16");
        }
        if !f.contains(wgpu::Features::FLOAT32_BLENDABLE) {
            out.retain(|e| *e != "EXT_float_blend");
        }
        if self.webgl2 {
            out.extend(["EXT_color_buffer_float", "EXT_color_buffer_half_float"]);
        } else {
            out.extend([
                "ANGLE_instanced_arrays",
                "OES_vertex_array_object",
                "OES_element_index_uint",
                "OES_standard_derivatives",
                "OES_texture_float",
                "OES_texture_half_float",
                "OES_texture_half_float_linear",
                "WEBGL_depth_texture",
                "EXT_blend_minmax",
                "EXT_frag_depth",
                "EXT_shader_texture_lod",
                "EXT_sRGB",
                "WEBGL_draw_buffers",
                "WEBGL_color_buffer_float",
                "EXT_color_buffer_half_float",
                "OES_fbo_render_mipmap",
            ]);
            out.retain(|e| *e != "EXT_texture_norm16");
        }
        if f.contains(wgpu::Features::TEXTURE_COMPRESSION_BC) {
            out.extend(["WEBGL_compressed_texture_s3tc", "WEBGL_compressed_texture_s3tc_srgb", "EXT_texture_compression_rgtc", "EXT_texture_compression_bptc"]);
        }
        if f.contains(wgpu::Features::TEXTURE_COMPRESSION_ETC2) {
            out.push("WEBGL_compressed_texture_etc");
        }
        if f.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC) {
            out.push("WEBGL_compressed_texture_astc");
        }
        out
    }

    pub(crate) fn format_allowed(&self, internal: u32) -> bool {
        match internal {
            R16_EXT | RG16_EXT | RGB16_EXT | RGBA16_EXT | R16_SNORM_EXT | RG16_SNORM_EXT | RGB16_SNORM_EXT | RGBA16_SNORM_EXT => {
                self.extension("EXT_texture_norm16")
            }
            DEPTH32F_STENCIL8 => self.features.contains(wgpu::Features::DEPTH32FLOAT_STENCIL8),
            _ => true,
        }
    }

    pub fn renderer(&self) -> String {
        self.gpu.info.name.clone()
    }

    pub fn vendor(&self) -> String {
        match self.gpu.info.vendor {
            0x10de => "NVIDIA Corporation".into(),
            0x1002 => "AMD".into(),
            0x8086 => "Intel".into(),
            0x106b => "Apple".into(),
            _ => self.gpu.info.driver.clone(),
        }
    }
}
