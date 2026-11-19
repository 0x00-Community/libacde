use wgpu::util::DeviceExt;

use crate::webgl::context::Context;

const COPY_BYTES: &str = r"
struct Params { src: u32, dst: u32, len: u32, pad: u32 }
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<uniform> p: Params;
fn byte_at(i: u32) -> u32 { return (src[i / 4u] >> ((i % 4u) * 8u)) & 0xffu; }
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3u) {
    let first = p.dst / 4u;
    let last = (p.dst + p.len + 3u) / 4u;
    let w = first + id.x;
    if w >= last { return; }
    var word = dst[w];
    for (var k = 0u; k < 4u; k++) {
        let g = w * 4u + k;
        if g >= p.dst && g < p.dst + p.len {
            let b = byte_at(p.src + g - p.dst);
            word = (word & ~(0xffu << (k * 8u))) | (b << (k * 8u));
        }
    }
    dst[w] = word;
}
";

pub const CONVERT: &str = r"
struct Params { offset: u32, stride: u32, count: u32, components: u32, ty: u32, normalized: u32, integer: u32, divisor: u32 }
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<vec4u>;
@group(0) @binding(2) var<uniform> p: Params;
fn byte_at(i: u32) -> u32 { return (src[i / 4u] >> ((i % 4u) * 8u)) & 0xffu; }
fn bits(at: u32, size: u32) -> u32 {
    var v = 0u;
    for (var k = 0u; k < size; k++) { v = v | (byte_at(at + k) << (k * 8u)); }
    return v;
}
fn sext(v: u32, b: u32) -> i32 { return (i32(v << (32u - b))) >> (32u - b); }
fn to_snorm(v: i32, b: u32) -> f32 { return max(f32(v) / f32((1u << (b - 1u)) - 1u), -1.0); }
fn to_unorm(v: u32, b: u32) -> f32 { return f32(v) / f32((1u << b) - 1u); }
fn component(base: u32, c: u32) -> u32 {
    let one = select(bitcast<u32>(1.0), 1u, p.integer != 0u);
    if c >= p.components { return select(0u, one, c == 3u); }
    switch p.ty {
        case 0x1400u: { let v = sext(bits(base + c, 1u), 8u); if p.integer != 0u { return bitcast<u32>(v); } if p.normalized != 0u { return bitcast<u32>(to_snorm(v, 8u)); } return bitcast<u32>(f32(v)); }
        case 0x1401u: { let v = bits(base + c, 1u); if p.integer != 0u { return v; } if p.normalized != 0u { return bitcast<u32>(to_unorm(v, 8u)); } return bitcast<u32>(f32(v)); }
        case 0x1402u: { let v = sext(bits(base + 2u * c, 2u), 16u); if p.integer != 0u { return bitcast<u32>(v); } if p.normalized != 0u { return bitcast<u32>(to_snorm(v, 16u)); } return bitcast<u32>(f32(v)); }
        case 0x1403u: { let v = bits(base + 2u * c, 2u); if p.integer != 0u { return v; } if p.normalized != 0u { return bitcast<u32>(to_unorm(v, 16u)); } return bitcast<u32>(f32(v)); }
        case 0x1404u: { let v = bitcast<i32>(bits(base + 4u * c, 4u)); if p.integer != 0u { return bitcast<u32>(v); } if p.normalized != 0u { return bitcast<u32>(max(f32(v) / 2147483647.0, -1.0)); } return bitcast<u32>(f32(v)); }
        case 0x1405u: { let v = bits(base + 4u * c, 4u); if p.integer != 0u { return v; } if p.normalized != 0u { return bitcast<u32>(f32(v) / 4294967295.0); } return bitcast<u32>(f32(v)); }
        case 0x1406u: { return bits(base + 4u * c, 4u); }
        case 0x140Bu, 0x8D61u: { return bitcast<u32>(unpack2x16float(bits(base + 2u * c, 2u)).x); }
        case 0x8D9Fu: {
            let w = bits(base, 4u);
            let shift = array<u32, 4>(0u, 10u, 20u, 30u)[c];
            let size = select(10u, 2u, c == 3u);
            let v = sext((w >> shift) & ((1u << size) - 1u), size);
            if p.normalized != 0u { return bitcast<u32>(to_snorm(v, size)); }
            return bitcast<u32>(f32(v));
        }
        default: {
            let w = bits(base, 4u);
            let shift = array<u32, 4>(0u, 10u, 20u, 30u)[c];
            let size = select(10u, 2u, c == 3u);
            let v = (w >> shift) & ((1u << size) - 1u);
            if p.normalized != 0u { return bitcast<u32>(to_unorm(v, size)); }
            return bitcast<u32>(f32(v));
        }
    }
}
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3u) {
    let i = id.x;
    if i >= p.count { return; }
    let element = select(i, i / p.divisor, p.divisor > 0u);
    let base = p.offset + element * p.stride;
    dst[i] = vec4u(component(base, 0u), component(base, 1u), component(base, 2u), component(base, 3u));
}
";

pub const FULLSCREEN: &str = r"
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.0, 1.0);
}
";

pub const PRESENT: &str = r"
@group(0) @binding(0) var source: texture_2d<f32>;
@fragment fn opaque(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let size = vec2i(textureDimensions(source));
    return vec4f(textureLoad(source, vec2i(i32(at.x), size.y - 1 - i32(at.y)), 0).rgb, 1.0);
}
@fragment fn straight(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let size = vec2i(textureDimensions(source));
    return textureLoad(source, vec2i(i32(at.x), size.y - 1 - i32(at.y)), 0);
}
@fragment fn unpremultiply(@builtin(position) at: vec4f) -> @location(0) vec4f {
    let size = vec2i(textureDimensions(source));
    let c = textureLoad(source, vec2i(i32(at.x), size.y - 1 - i32(at.y)), 0);
    if c.a <= 0.0 { return vec4f(0.0); }
    return vec4f(c.rgb / c.a, c.a);
}
";

pub const BLIT: &str = r"
struct Params { rect: vec4f, flip: u32, premultiply: u32, unpremultiply: u32, lod: f32, decode: u32 }
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> p: Params;
struct Out { @builtin(position) pos: vec4f, @location(0) uv: vec2f }
@vertex fn vs(@builtin(vertex_index) i: u32) -> Out {
    let q = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.pos = vec4f(q * 2.0 - 1.0, 0.0, 1.0);
    var v = 1.0 - q.y;
    if p.flip != 0u { v = 1.0 - v; }
    out.uv = p.rect.xy + vec2f(q.x, v) * p.rect.zw;
    return out;
}
@fragment fn fs(in: Out) -> @location(0) vec4f {
    var c = textureSampleLevel(source, smp, in.uv, p.lod);
    if p.premultiply != 0u { c = vec4f(c.rgb * c.a, c.a); }
    if p.unpremultiply != 0u && c.a > 0.0 { c = vec4f(c.rgb / c.a, c.a); }
    if p.decode != 0u { c = vec4f(select(pow((c.rgb + 0.055) / 1.055, vec3f(2.4)), c.rgb / 12.92, c.rgb <= vec3f(0.04045)), c.a); }
    return c;
}
";

impl Context {
    pub(crate) fn compute(&mut self, name: &'static str) -> Option<(wgpu::ComputePipeline, wgpu::BindGroupLayout)> {
        if let Some(c) = self.caches.compute.get(name) {
            return Some(c.clone());
        }
        let source = match name {
            "copy" => COPY_BYTES,
            _ => CONVERT,
        };
        let built = self.scoped(|device| {
            let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            };
            let uniform = wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            };
            let group = device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(name), entries: &[storage(0, true), storage(1, false), uniform] });
            let layout =
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(name), bind_group_layouts: &[Some(&group)], immediate_size: 0 });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(name), source: wgpu::ShaderSource::Wgsl(source.into()) });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: Some(&layout),
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            (pipeline, group)
        });
        match built {
            Ok(c) => {
                self.caches.compute.insert(name, c.clone());
                Some(c)
            }
            Err(e) => {
                log::error!("webgl: {name} pipeline: {e}");
                None
            }
        }
    }

    pub(crate) fn run_compute(&mut self, name: &'static str, src: &wgpu::Buffer, dst: &wgpu::Buffer, params: &[u32], invocations: u32) {
        if invocations == 0 {
            return;
        }
        let Some((pipeline, layout)) = self.compute(name) else { return };
        let device = self.gpu.device.clone();
        let mut bytes = Vec::with_capacity(32);
        for p in params {
            bytes.extend_from_slice(&p.to_le_bytes());
        }
        bytes.resize(bytes.len().next_multiple_of(16), 0);
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(name), contents: &bytes, usage: wgpu::BufferUsages::UNIFORM });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: src.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: dst.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
            ],
        });
        self.flush_pass();
        let encoder = self.encoder();
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some(name), timestamp_writes: None });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(invocations.div_ceil(64), 1, 1);
    }

    pub(crate) fn copy_bytes(&mut self, src: &wgpu::Buffer, src_offset: u64, dst: &wgpu::Buffer, dst_offset: u64, len: u64) {
        let words = ((dst_offset % 4) + len).div_ceil(4) as u32;
        self.run_compute("copy", src, dst, &[src_offset as u32, dst_offset as u32, len as u32, 0], words);
    }

    pub(crate) fn patch_buffer(&mut self, dst: &wgpu::Buffer, offset: u64, data: &[u8]) {
        let mut padded = data.to_vec();
        padded.resize(data.len().next_multiple_of(4).max(4), 0);
        let device = self.gpu.device.clone();
        let src = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("acde webgl patch"),
            contents: &padded,
            usage: wgpu::BufferUsages::STORAGE,
        });
        self.copy_bytes(&src, 0, dst, offset, data.len() as u64);
    }

    pub(crate) fn helper(&mut self, name: &str, build: impl FnOnce(&wgpu::Device) -> wgpu::RenderPipeline) -> Option<wgpu::RenderPipeline> {
        if let Some(p) = self.caches.helpers.get(name) {
            return Some(p.clone());
        }
        match self.scoped(build) {
            Ok(p) => {
                self.caches.helpers.insert(name.to_owned(), p.clone());
                Some(p)
            }
            Err(e) => {
                log::error!("webgl: {name} pipeline: {e}");
                None
            }
        }
    }
}

impl Context {
    pub(crate) fn scoped<T>(&self, f: impl FnOnce(&wgpu::Device) -> T) -> Result<T, String> {
        let device = &self.gpu.device;
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let value = f(device);
        let errors = [pollster::block_on(internal.pop()), pollster::block_on(memory.pop()), pollster::block_on(validation.pop())];
        match errors.into_iter().flatten().next() {
            Some(wgpu::Error::Validation { description, .. } | wgpu::Error::Internal { description, .. }) => Err(description),
            Some(e) => Err(e.to_string()),
            None => Ok(value),
        }
    }
}
