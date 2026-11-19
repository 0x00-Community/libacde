use std::cell::RefCell;
use std::collections::HashMap;

use crate::webgl::FastMap;
use std::rc::Rc;

use crate::webgl::consts::*;
use crate::webgl::format::Format;
use crate::webgl::shader;

pub type Id = u32;

pub struct Buffer {
    pub raw: Option<wgpu::Buffer>,
    pub size: u64,
    pub usage: u32,
    pub element: Option<bool>,
    pub shadow: Option<Vec<u8>>,
    pub used: u64,
    pub version: u64,
    pub ranges: FastMap<(u32, u64, u32), u32>,
}

impl Buffer {
    pub fn new() -> Self {
        Buffer { raw: None, size: 0, usage: STATIC_DRAW, element: None, shadow: None, used: 0, version: 0, ranges: FastMap::default() }
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SamplerParams {
    pub min: u32,
    pub mag: u32,
    pub wrap: [u32; 3],
    pub min_lod: f32,
    pub max_lod: f32,
    pub compare_mode: u32,
    pub compare_func: u32,
    pub anisotropy: f32,
}

impl Default for SamplerParams {
    fn default() -> Self {
        SamplerParams {
            min: NEAREST_MIPMAP_LINEAR,
            mag: LINEAR,
            wrap: [REPEAT; 3],
            min_lod: -1000.0,
            max_lod: 1000.0,
            compare_mode: NONE,
            compare_func: LEQUAL,
            anisotropy: 1.0,
        }
    }
}

impl SamplerParams {
    pub fn mipmapped(&self) -> bool {
        !matches!(self.min, NEAREST | LINEAR)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub format: Format,
}

impl PartialEq for Format {
    fn eq(&self, other: &Self) -> bool {
        self.internal == other.internal && self.wgpu == other.wgpu
    }
}

pub struct Alloc {
    pub texture: wgpu::Texture,
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub mips: u32,
    pub samples: u32,
    pub serial: u64,
    pub views: RefCell<FastMap<ViewKey, wgpu::TextureView>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewKey {
    pub dim: wgpu::TextureViewDimension,
    pub depth_only: bool,
    pub level: u32,
    pub levels: u32,
    pub layer: Option<u32>,
}

impl Alloc {
    pub fn view(&self, key: ViewKey) -> wgpu::TextureView {
        self.views
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                self.texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("acde webgl view"),
                    dimension: Some(key.dim),
                    aspect: if key.depth_only { wgpu::TextureAspect::DepthOnly } else { wgpu::TextureAspect::All },
                    base_mip_level: key.level,
                    mip_level_count: Some(key.levels),
                    base_array_layer: key.layer.unwrap_or(0),
                    array_layer_count: key.layer.map(|_| 1),
                    ..Default::default()
                })
            })
            .clone()
    }
}

pub struct Texture {
    pub target: u32,
    pub images: FastMap<(u32, u32), Image>,
    pub alloc: Option<Alloc>,
    pub immutable: bool,
    pub immutable_levels: u32,
    pub params: SamplerParams,
    pub base_level: u32,
    pub max_level: u32,
    pub used: u64,
    pub version: u64,
}

impl Texture {
    pub fn new() -> Self {
        Texture {
            target: 0,
            images: FastMap::default(),
            alloc: None,
            immutable: false,
            immutable_levels: 0,
            params: SamplerParams::default(),
            base_level: 0,
            max_level: 1000,
            used: 0,
            version: 0,
        }
    }

    pub fn faces(&self) -> u32 {
        if self.target == TEXTURE_CUBE_MAP { 6 } else { 1 }
    }

    pub fn base(&self) -> Option<Image> {
        self.images.get(&(0, self.base_level)).copied()
    }
}

impl Default for Texture {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Renderbuffer {
    pub format: Option<Format>,
    pub samples: u32,
    pub width: u32,
    pub height: u32,
    pub texture: Option<wgpu::Texture>,
    pub view: Option<wgpu::TextureView>,
    pub version: u64,
    pub used: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Attachment {
    Texture { id: Id, level: u32, layer: u32 },
    Renderbuffer(Id),
}

pub struct Framebuffer {
    pub color: [Option<Attachment>; 8],
    pub depth: Option<Attachment>,
    pub stencil: Option<Attachment>,
    pub draw_buffers: [u32; 8],
    pub read_buffer: u32,
}

impl Framebuffer {
    pub fn new() -> Self {
        let mut draw_buffers = [NONE; 8];
        draw_buffers[0] = COLOR_ATTACHMENT0;
        Framebuffer { color: [None; 8], depth: None, stencil: None, draw_buffers, read_buffer: COLOR_ATTACHMENT0 }
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Shader {
    pub kind: u32,
    pub source: String,
    pub compiled: Option<Rc<shader::Compiled>>,
    pub log: String,
    pub compiled_ok: bool,
    pub deleted: bool,
    pub attached: u32,
}

pub struct UniformLocation {
    pub uniform: usize,
    pub element: u32,
}

pub struct Linked {
    pub info: shader::Linked,
    pub serial: u64,
    pub vertex: wgpu::ShaderModule,
    pub fragment: wgpu::ShaderModule,
    pub points: Option<(wgpu::ShaderModule, wgpu::ShaderModule)>,
    pub locations: Vec<(String, UniformLocation)>,
}

pub struct Program {
    pub vertex: Option<Id>,
    pub fragment: Option<Id>,
    pub bound_attributes: HashMap<String, u32>,
    pub linked: Option<Rc<Linked>>,
    pub link_ok: bool,
    pub log: String,
    pub validated: bool,
    pub deleted: bool,
    pub data: [Vec<u8>; 2],
    pub data_version: u64,
    pub units: Vec<Vec<u32>>,
    pub block_bindings: Vec<u32>,
    pub transform_feedback: (Vec<String>, u32),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Attrib {
    pub enabled: bool,
    pub buffer: Option<Id>,
    pub size: u32,
    pub ty: u32,
    pub normalized: bool,
    pub integer: bool,
    pub stride: u32,
    pub offset: u64,
    pub divisor: u32,
}

impl Default for Attrib {
    fn default() -> Self {
        Attrib { enabled: false, buffer: None, size: 4, ty: FLOAT, normalized: false, integer: false, stride: 0, offset: 0, divisor: 0 }
    }
}

impl Attrib {
    pub fn element_size(&self) -> u32 {
        match self.ty {
            BYTE | UNSIGNED_BYTE => self.size,
            SHORT | UNSIGNED_SHORT | HALF_FLOAT | HALF_FLOAT_OES => 2 * self.size,
            INT_2_10_10_10_REV | UNSIGNED_INT_2_10_10_10_REV => 4,
            _ => 4 * self.size,
        }
    }

    pub fn effective_stride(&self) -> u32 {
        if self.stride == 0 { self.element_size() } else { self.stride }
    }
}

pub struct VertexArray {
    pub attribs: [Attrib; 16],
    pub element: Option<Id>,
    pub bound_once: bool,
}

impl VertexArray {
    pub fn new() -> Self {
        VertexArray { attribs: [Attrib::default(); 16], element: None, bound_once: false }
    }
}

impl Default for VertexArray {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Sampler {
    pub params: SamplerParams,
}

pub struct Readback {
    pub buffer: wgpu::Buffer,
    pub state: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

impl Readback {
    pub const PENDING: u8 = 0;
    pub const MAPPED: u8 = 1;
    pub const FAILED: u8 = 2;
}

pub struct Query {
    pub target: u32,
    pub active: bool,
    pub slots: Vec<(std::rc::Rc<Readback>, u32)>,
    pub result: Option<u64>,
}

pub struct Sync {
    pub done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub struct TransformFeedback {
    pub active: bool,
    pub paused: bool,
}

#[allow(clippy::large_enum_variant, reason = "every object lives in one map, so boxing would only add an allocation each")]
pub enum Object {
    Buffer(Buffer),
    Texture(Texture),
    Renderbuffer(Renderbuffer),
    Framebuffer(Framebuffer),
    Shader(Shader),
    Program(Program),
    VertexArray(VertexArray),
    Sampler(Sampler),
    Query(Query),
    Sync(Sync),
    TransformFeedback(TransformFeedback),
}

macro_rules! accessors {
    ($($get:ident, $get_mut:ident: $variant:ident => $ty:ty;)*) => {
        impl Object {
            $(
                pub fn $get(&self) -> Option<&$ty> {
                    match self {
                        Object::$variant(x) => Some(x),
                        _ => None,
                    }
                }
                pub fn $get_mut(&mut self) -> Option<&mut $ty> {
                    match self {
                        Object::$variant(x) => Some(x),
                        _ => None,
                    }
                }
            )*
        }
    };
}

accessors! {
    buffer, buffer_mut: Buffer => Buffer;
    texture, texture_mut: Texture => Texture;
    renderbuffer, renderbuffer_mut: Renderbuffer => Renderbuffer;
    framebuffer, framebuffer_mut: Framebuffer => Framebuffer;
    shader, shader_mut: Shader => Shader;
    program, program_mut: Program => Program;
    vertex_array, vertex_array_mut: VertexArray => VertexArray;
    sampler, sampler_mut: Sampler => Sampler;
    query, query_mut: Query => Query;
    sync, sync_mut: Sync => Sync;
    transform_feedback, transform_feedback_mut: TransformFeedback => TransformFeedback;
}
