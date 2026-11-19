use crate::webgl::FastMap;
use std::rc::Rc;

use crate::webgl::consts::*;
use crate::webgl::context::Context;
use crate::webgl::format::Class;
use crate::webgl::objects::*;
use crate::webgl::pass::{Call, Draw, Source, Target};
use crate::webgl::pipeline::{Binding, BlendKey, DepthKey, GroupKey, PipelineKey, SampleKind, SamplerKey, SamplerKind};
use crate::webgl::shader::{Kind, Sampled};
use crate::webgl::state::Generic;

fn compare(f: u32) -> wgpu::CompareFunction {
    use wgpu::CompareFunction as C;
    match f {
        NEVER => C::Never,
        LESS => C::Less,
        EQUAL => C::Equal,
        LEQUAL => C::LessEqual,
        GREATER => C::Greater,
        NOTEQUAL => C::NotEqual,
        GEQUAL => C::GreaterEqual,
        _ => C::Always,
    }
}

fn stencil_op(op: u32) -> wgpu::StencilOperation {
    use wgpu::StencilOperation as S;
    match op {
        ZERO => S::Zero,
        REPLACE => S::Replace,
        INCR => S::IncrementClamp,
        DECR => S::DecrementClamp,
        INVERT => S::Invert,
        INCR_WRAP => S::IncrementWrap,
        DECR_WRAP => S::DecrementWrap,
        _ => S::Keep,
    }
}

fn factor(f: u32) -> wgpu::BlendFactor {
    use wgpu::BlendFactor as B;
    match f {
        ZERO => B::Zero,
        ONE => B::One,
        SRC_COLOR => B::Src,
        ONE_MINUS_SRC_COLOR => B::OneMinusSrc,
        DST_COLOR => B::Dst,
        ONE_MINUS_DST_COLOR => B::OneMinusDst,
        SRC_ALPHA => B::SrcAlpha,
        ONE_MINUS_SRC_ALPHA => B::OneMinusSrcAlpha,
        DST_ALPHA => B::DstAlpha,
        ONE_MINUS_DST_ALPHA => B::OneMinusDstAlpha,
        CONSTANT_COLOR | CONSTANT_ALPHA => B::Constant,
        ONE_MINUS_CONSTANT_COLOR | ONE_MINUS_CONSTANT_ALPHA => B::OneMinusConstant,
        _ => B::SrcAlphaSaturated,
    }
}

fn operation(e: u32) -> wgpu::BlendOperation {
    match e {
        FUNC_SUBTRACT => wgpu::BlendOperation::Subtract,
        FUNC_REVERSE_SUBTRACT => wgpu::BlendOperation::ReverseSubtract,
        MIN => wgpu::BlendOperation::Min,
        MAX => wgpu::BlendOperation::Max,
        _ => wgpu::BlendOperation::Add,
    }
}

fn address(w: u32) -> wgpu::AddressMode {
    match w {
        CLAMP_TO_EDGE => wgpu::AddressMode::ClampToEdge,
        MIRRORED_REPEAT => wgpu::AddressMode::MirrorRepeat,
        _ => wgpu::AddressMode::Repeat,
    }
}

fn direct_format(a: &Attrib) -> Option<wgpu::VertexFormat> {
    use wgpu::VertexFormat as V;
    Some(match (a.ty, a.size, a.normalized, a.integer) {
        (FLOAT, 1, ..) => V::Float32,
        (FLOAT, 2, ..) => V::Float32x2,
        (FLOAT, 3, ..) => V::Float32x3,
        (FLOAT, 4, ..) => V::Float32x4,
        (HALF_FLOAT | HALF_FLOAT_OES, 1, ..) => V::Float16,
        (HALF_FLOAT | HALF_FLOAT_OES, 2, ..) => V::Float16x2,
        (HALF_FLOAT | HALF_FLOAT_OES, 4, ..) => V::Float16x4,
        (UNSIGNED_BYTE, 1, true, _) => V::Unorm8,
        (UNSIGNED_BYTE, 2, true, _) => V::Unorm8x2,
        (UNSIGNED_BYTE, 4, true, _) => V::Unorm8x4,
        (BYTE, 1, true, _) => V::Snorm8,
        (BYTE, 2, true, _) => V::Snorm8x2,
        (BYTE, 4, true, _) => V::Snorm8x4,
        (UNSIGNED_SHORT, 1, true, _) => V::Unorm16,
        (UNSIGNED_SHORT, 2, true, _) => V::Unorm16x2,
        (UNSIGNED_SHORT, 4, true, _) => V::Unorm16x4,
        (SHORT, 1, true, _) => V::Snorm16,
        (SHORT, 2, true, _) => V::Snorm16x2,
        (SHORT, 4, true, _) => V::Snorm16x4,
        (UNSIGNED_BYTE, 1, _, true) => V::Uint8,
        (UNSIGNED_BYTE, 2, _, true) => V::Uint8x2,
        (UNSIGNED_BYTE, 4, _, true) => V::Uint8x4,
        (BYTE, 1, _, true) => V::Sint8,
        (BYTE, 2, _, true) => V::Sint8x2,
        (BYTE, 4, _, true) => V::Sint8x4,
        (UNSIGNED_SHORT, 1, _, true) => V::Uint16,
        (UNSIGNED_SHORT, 2, _, true) => V::Uint16x2,
        (UNSIGNED_SHORT, 4, _, true) => V::Uint16x4,
        (SHORT, 1, _, true) => V::Sint16,
        (SHORT, 2, _, true) => V::Sint16x2,
        (SHORT, 4, _, true) => V::Sint16x4,
        (INT, 1, _, true) => V::Sint32,
        (INT, 2, _, true) => V::Sint32x2,
        (INT, 3, _, true) => V::Sint32x3,
        (INT, 4, _, true) => V::Sint32x4,
        (UNSIGNED_INT, 1, _, true) => V::Uint32,
        (UNSIGNED_INT, 2, _, true) => V::Uint32x2,
        (UNSIGNED_INT, 3, _, true) => V::Uint32x3,
        (UNSIGNED_INT, 4, _, true) => V::Uint32x4,
        (UNSIGNED_INT_2_10_10_10_REV, 4, true, _) => V::Unorm10_10_10_2,
        _ => return None,
    })
}

pub(crate) struct Resolved {
    pub view: wgpu::TextureView,
    pub key: (u64, u64, u64),
    pub sample: SampleKind,
    pub sampler: SamplerKey,
}

impl Context {
    fn complete(&self, t: &Texture, params: &SamplerParams) -> bool {
        let Some(base) = t.base() else { return false };
        let Some(alloc) = t.alloc.as_ref() else { return false };
        if base.width == 0 || base.height == 0 || alloc.format != base.format {
            return false;
        }
        let faces = t.faces();
        let mipmapped = params.mipmapped();
        if !self.webgl2
            && !(base.width.is_power_of_two() && base.height.is_power_of_two())
            && (mipmapped || params.wrap[0] != CLAMP_TO_EDGE || params.wrap[1] != CLAMP_TO_EDGE)
        {
            return false;
        }
        if !base.format.filterable
            && (params.mag != NEAREST || !matches!(params.min, NEAREST | NEAREST_MIPMAP_NEAREST))
            && !(base.format.class == Class::Depth || base.format.class == Class::DepthStencil)
        {
            return false;
        }
        for face in 0..faces {
            let Some(img) = t.images.get(&(face, t.base_level)) else { return false };
            if img.width != base.width || img.height != base.height || img.format != base.format {
                return false;
            }
        }
        if mipmapped {
            let last = (t.base_level + crate::webgl::texture::mip_count(base.width, base.height, if t.target == TEXTURE_3D { base.depth } else { 1 }) - 1)
                .min(t.max_level);
            for level in t.base_level..=last {
                let k = level - t.base_level;
                for face in 0..faces {
                    let Some(img) = t.images.get(&(face, level)) else { return false };
                    if img.width != (base.width >> k).max(1) || img.height != (base.height >> k).max(1) || img.format != base.format {
                        return false;
                    }
                }
                if level >= alloc.mips {
                    return false;
                }
            }
        }
        true
    }

    fn dummy(&mut self, dim: wgpu::TextureViewDimension, sample: SampleKind) -> wgpu::TextureView {
        if let Some((_, v)) = self.caches.dummies.get(&(dim, sample)) {
            return v.clone();
        }
        let format = match sample {
            SampleKind::Float | SampleKind::Unfilterable => wgpu::TextureFormat::Rgba8Unorm,
            SampleKind::Sint => wgpu::TextureFormat::Rgba8Sint,
            SampleKind::Uint => wgpu::TextureFormat::Rgba8Uint,
            SampleKind::Depth => wgpu::TextureFormat::Depth32Float,
        };
        let layers = match dim {
            wgpu::TextureViewDimension::Cube => 6,
            _ => 1,
        };
        let device = &self.gpu.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("acde webgl incomplete"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: if dim == wgpu::TextureViewDimension::D3 { wgpu::TextureDimension::D3 } else { wgpu::TextureDimension::D2 },
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        if sample != SampleKind::Depth {
            let one = if matches!(sample, SampleKind::Float | SampleKind::Unfilterable) { 255 } else { 1 };
            let pixel: Vec<u8> = (0..layers).flat_map(|_| [0, 0, 0, one]).collect();
            self.gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &pixel,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
                wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(dim), ..Default::default() });
        self.caches.dummies.insert((dim, sample), (texture, view.clone()));
        view
    }

    fn resolve_texture(&mut self, unit: u32, dim: wgpu::TextureViewDimension, sampled: Sampled) -> Resolved {
        let u = self.state.units.get(unit as usize).copied().unwrap_or_default();
        let id = match dim {
            wgpu::TextureViewDimension::Cube => u.cube,
            wgpu::TextureViewDimension::D3 => u.texture_3d,
            wgpu::TextureViewDimension::D2Array => u.array,
            _ => u.texture_2d,
        };
        let sampler_params = u.sampler.and_then(|s| self.sampler_object(s)).map(|s| s.params);
        let shadow = sampled == Sampled::Depth;
        let incomplete = |this: &mut Self| {
            let sample = SampleKind::of(sampled, true);
            Resolved {
                view: this.dummy(dim, sample),
                key: (u64::MAX, dim as u64, sample as u64),
                sample,
                sampler: SamplerKey {
                    address: [wgpu::AddressMode::ClampToEdge; 3],
                    mag: wgpu::FilterMode::Nearest,
                    min: wgpu::FilterMode::Nearest,
                    mip: wgpu::MipmapFilterMode::Nearest,
                    lod: (0, 0),
                    compare: shadow.then_some(wgpu::CompareFunction::LessEqual),
                    anisotropy: 1,
                },
            }
        };
        let Some(t) = id.and_then(|id| self.texture(id)) else { return incomplete(self) };
        let params = sampler_params.unwrap_or(t.params);
        if !self.complete(t, &params) {
            return incomplete(self);
        }
        let Some(alloc) = t.alloc.as_ref() else { return incomplete(self) };
        let format = alloc.format;
        let class_ok = matches!(
            (sampled, format.class),
            (Sampled::Float, Class::Float | Class::Unfilterable | Class::Depth | Class::DepthStencil)
                | (Sampled::Depth, Class::Depth | Class::DepthStencil)
                | (Sampled::Sint, Class::Sint)
                | (Sampled::Uint, Class::Uint)
        );
        if !class_ok {
            return incomplete(self);
        }
        let depth_texture = matches!(format.class, Class::Depth | Class::DepthStencil);
        let filterable = format.filterable && !depth_texture;
        let sample = if shadow { SampleKind::Depth } else { SampleKind::of(sampled, filterable) };
        let mipmapped = params.mipmapped();
        let base = t.base_level.min(alloc.mips - 1);
        let base_image = t.base().map_or((1, 1, 1), |b| (b.width, b.height, b.depth));
        let levels = if mipmapped {
            (crate::webgl::texture::mip_count(base_image.0, base_image.1, if t.target == TEXTURE_3D { base_image.2 } else { 1 }))
                .min(t.max_level.saturating_sub(t.base_level) + 1)
                .min(alloc.mips - base)
        } else {
            1
        };
        let view = alloc.view(ViewKey { dim, depth_only: depth_texture, level: base, levels: levels.max(1), layer: None });
        let nearest = !filterable || matches!(sample, SampleKind::Sint | SampleKind::Uint | SampleKind::Unfilterable);
        let filter = |f: u32| {
            if nearest || matches!(f, NEAREST | NEAREST_MIPMAP_NEAREST | NEAREST_MIPMAP_LINEAR) { wgpu::FilterMode::Nearest } else { wgpu::FilterMode::Linear }
        };
        let mip = if nearest || matches!(params.min, NEAREST_MIPMAP_NEAREST | LINEAR_MIPMAP_NEAREST | NEAREST | LINEAR) {
            wgpu::MipmapFilterMode::Nearest
        } else {
            wgpu::MipmapFilterMode::Linear
        };
        let (mag, min) = (filter(params.mag), filter(params.min));
        let shadow_filter = shadow && params.mag == LINEAR;
        let (mag, min) = if shadow {
            if shadow_filter { (wgpu::FilterMode::Linear, wgpu::FilterMode::Linear) } else { (wgpu::FilterMode::Nearest, wgpu::FilterMode::Nearest) }
        } else {
            (mag, min)
        };
        let lod_min = params.min_lod.clamp(0.0, 32.0);
        let lod_max = if mipmapped { params.max_lod.clamp(lod_min, 32.0) } else { lod_min.max(0.0) };
        let anisotropy = if mag == wgpu::FilterMode::Linear && min == wgpu::FilterMode::Linear && mip == wgpu::MipmapFilterMode::Linear {
            params.anisotropy.clamp(1.0, 16.0) as u16
        } else {
            1
        };
        let sampler = SamplerKey {
            address: [address(params.wrap[0]), address(params.wrap[1]), address(params.wrap[2])],
            mag,
            min,
            mip,
            lod: ((lod_min * 256.0) as u32, (lod_max * 256.0) as u32),
            compare: shadow.then(|| compare(params.compare_func)),
            anisotropy,
        };
        let key = (u64::from(id.unwrap_or(0)), alloc.serial, (u64::from(base) << 32) | (u64::from(levels) << 8) | dim as u64);
        Resolved { view, key, sample, sampler }
    }

    pub fn draw(&mut self, mode: u32, first: i32, count: i32, instances: i32, element: Option<(u32, i64)>) {
        if mode > TRIANGLE_FAN {
            return self.error(INVALID_ENUM);
        }
        if first < 0 || count < 0 || instances < 0 {
            return self.error(INVALID_VALUE);
        }
        let Some(program) = self.state.program else { return self.error(INVALID_OPERATION) };
        let Some(linked) = self.linked(program) else { return self.error(INVALID_OPERATION) };
        let target = match self.draw_target() {
            Ok(t) => t,
            Err(e) => return self.error(e),
        };
        let mut vertex_count = (first as u64) + count as u64;
        let mut index = None;
        if let Some((ty, offset)) = element {
            let size = match ty {
                UNSIGNED_BYTE => 1,
                UNSIGNED_SHORT => 2,
                UNSIGNED_INT if self.webgl2 || self.extension("OES_element_index_uint") => 4,
                _ => return self.error(INVALID_ENUM),
            };
            if offset < 0 {
                return self.error(INVALID_VALUE);
            }
            if !(offset as u64).is_multiple_of(size) {
                return self.error(INVALID_OPERATION);
            }
            let Some(id) = self.vao().element else { return self.error(INVALID_OPERATION) };
            let Some(b) = self.buffer(id) else { return self.error(INVALID_OPERATION) };
            if (offset as u64).checked_add(count as u64 * size).is_none_or(|end| end > b.size) {
                return self.error(INVALID_OPERATION);
            }
            if count > 0 {
                let restart = self.webgl2 && matches!(mode, LINE_STRIP | TRIANGLE_STRIP);
                let Some(max) = self.max_index(id, ty, offset as u64, count as u32, restart) else { return self.error(INVALID_OPERATION) };
                vertex_count = u64::from(max) + 1;
            }
            index = Some((id, ty, offset as u64, size));
        }
        if self.state.stencil_test && target.depth.as_ref().is_some_and(|d| d.stencil) {
            let [f, b] = self.state.stencil;
            if f.value_mask != b.value_mask || f.write_mask != b.write_mask || f.reference != b.reference {
                return self.error(INVALID_OPERATION);
            }
        }
        let info = &linked.info;
        for output in &info.outputs {
            let Some(Some(c)) = target.colors.get(output.location as usize) else { continue };
            let ok = match c.class {
                Class::Sint => output.kind == Kind::Sint,
                Class::Uint => output.kind == Kind::Uint,
                _ => output.kind == Kind::Float,
            };
            if !ok {
                return self.error(INVALID_OPERATION);
            }
        }
        let instances = instances as u32;
        let mut vertex_layouts = Vec::new();
        let mut sources: Vec<(u32, Source, u64)> = Vec::new();
        let mut generics: Vec<(u32, [u8; 16])> = Vec::new();
        let attribs = self.vao().attribs;
        for attribute in &info.attributes {
            for slot in 0..attribute.slots {
                let location = attribute.location + slot;
                let a = attribs[location as usize];
                if !a.enabled {
                    let g = self.state.generic[location as usize];
                    let kind_ok =
                        matches!((g, attribute.kind), (Generic::Float(_), Kind::Float) | (Generic::Int(_), Kind::Sint) | (Generic::Uint(_), Kind::Uint));
                    if !kind_ok && self.webgl2 {
                        return self.error(INVALID_OPERATION);
                    }
                    let format = match g {
                        Generic::Float(_) => wgpu::VertexFormat::Float32x4,
                        Generic::Int(_) => wgpu::VertexFormat::Sint32x4,
                        Generic::Uint(_) => wgpu::VertexFormat::Uint32x4,
                    };
                    vertex_layouts.push((location, 0, false, format));
                    generics.push((location, g.bytes()));
                    continue;
                }
                let Some(buffer_id) = a.buffer else { return self.error(INVALID_OPERATION) };
                let Some(b) = self.buffer(buffer_id) else { return self.error(INVALID_OPERATION) };
                if a.integer != (attribute.kind != Kind::Float) && self.webgl2 {
                    return self.error(INVALID_OPERATION);
                }
                let elements = if a.divisor == 0 { vertex_count } else { u64::from(instances.div_ceil(a.divisor)) };
                if elements == 0 || count == 0 {
                    vertex_layouts.push((location, u64::from(a.effective_stride()), a.divisor > 0, direct_format(&a).unwrap_or(wgpu::VertexFormat::Float32x4)));
                    continue;
                }
                let stride = u64::from(a.effective_stride());
                let needed = a.offset + (elements - 1) * stride + u64::from(a.element_size());
                if needed > b.size {
                    return self.error(INVALID_OPERATION);
                }
                let Some(raw) = b.raw.clone() else { return self.error(INVALID_OPERATION) };
                let version = b.version;
                let direct = direct_format(&a).filter(|_| a.offset.is_multiple_of(4) && stride % 4 == 0 && a.divisor <= 1);
                let direct = direct.or_else(|| {
                    (a.size == 3
                        && matches!(a.ty, UNSIGNED_BYTE | BYTE | UNSIGNED_SHORT | SHORT)
                        && (a.normalized || a.integer)
                        && a.offset.is_multiple_of(4)
                        && stride % 4 == 0
                        && a.divisor <= 1
                        && stride > u64::from(a.element_size())
                        && needed < b.size.next_multiple_of(4))
                    .then(|| direct_format(&Attrib { size: 4, ..a }))
                    .flatten()
                    .filter(|_| {
                        matches!(
                            attribute.gl_type,
                            FLOAT_VEC3 | FLOAT_VEC2 | FLOAT | INT_VEC3 | UNSIGNED_INT_VEC3 | INT_VEC2 | UNSIGNED_INT_VEC2 | INT | UNSIGNED_INT
                        )
                    })
                });
                match direct {
                    Some(format) => {
                        vertex_layouts.push((location, stride, a.divisor > 0, format));
                        sources.push((location, Source::Buffer(raw), a.offset));
                    }
                    None => {
                        let converted = self.convert_attribute(buffer_id, version, &raw, &a, elements as u32);
                        let format = match (a.integer, a.ty) {
                            (true, BYTE | SHORT | INT) => wgpu::VertexFormat::Sint32x4,
                            (true, _) => wgpu::VertexFormat::Uint32x4,
                            _ => wgpu::VertexFormat::Float32x4,
                        };
                        vertex_layouts.push((location, 16, a.divisor > 0, format));
                        sources.push((location, Source::Buffer(converted), 0));
                    }
                }
                let generation = self.generation;
                if let Some(b) = self.buffer_mut(buffer_id) {
                    b.used = generation;
                }
            }
        }
        if count == 0 || instances == 0 || self.state.rasterizer_discard {
            return;
        }
        let (layout_bindings, resources, sampled) = match self.resources(&linked, program, &target) {
            Ok(r) => r,
            Err(e) => return self.error(e),
        };
        let generation = self.generation;
        for id in sampled {
            if let Some(t) = self.texture_mut(id) {
                t.used = generation;
            }
        }
        let wide = mode == POINTS && index.is_none() && instances == 1 && linked.points.is_some() && vertex_layouts.iter().all(|l| !l.2);
        if wide {
            for layout in &mut vertex_layouts {
                layout.2 = true;
            }
        }
        let (topology, generated) = match mode {
            POINTS if wide => (wgpu::PrimitiveTopology::TriangleList, false),
            POINTS => (wgpu::PrimitiveTopology::PointList, false),
            LINES => (wgpu::PrimitiveTopology::LineList, false),
            LINE_STRIP => (wgpu::PrimitiveTopology::LineStrip, false),
            TRIANGLES => (wgpu::PrimitiveTopology::TriangleList, false),
            TRIANGLE_STRIP => (wgpu::PrimitiveTopology::TriangleStrip, false),
            LINE_LOOP => (wgpu::PrimitiveTopology::LineList, true),
            _ => (wgpu::PrimitiveTopology::TriangleList, true),
        };
        let indexed_strip = index.is_some() && !generated && matches!(topology, wgpu::PrimitiveTopology::LineStrip | wgpu::PrimitiveTopology::TriangleStrip);
        let mut index_format = match index {
            Some((_, UNSIGNED_INT, ..)) => wgpu::IndexFormat::Uint32,
            Some(_) => wgpu::IndexFormat::Uint16,
            None => wgpu::IndexFormat::Uint32,
        };
        let mut generated_indices: Option<Vec<u8>> = None;
        let mut index_source: Option<(Source, u64)> = None;
        if generated {
            let list: Vec<u32> = match index {
                Some((id, ty, offset, size)) => {
                    let Some(bytes) = self.element_bytes(id) else { return self.error(INVALID_OPERATION) };
                    let bytes = &bytes[offset as usize..(offset + count as u64 * size) as usize];
                    let values: Vec<u32> = match ty {
                        UNSIGNED_BYTE => bytes.iter().map(|&b| u32::from(b)).collect(),
                        UNSIGNED_SHORT => bytes.as_chunks::<2>().0.iter().map(|c| u32::from(u16::from_le_bytes([c[0], c[1]]))).collect(),
                        _ => bytes.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
                    };
                    expand(mode, &values)
                }
                None => expand(mode, &(first as u32..(first + count) as u32).collect::<Vec<_>>()),
            };
            index_format = wgpu::IndexFormat::Uint32;
            generated_indices = Some(list.iter().flat_map(|i| i.to_le_bytes()).collect());
        } else if let Some((id, ty, offset, _)) = index {
            if ty == UNSIGNED_BYTE {
                let Some(bytes) = self.element_bytes(id) else { return self.error(INVALID_OPERATION) };
                let restart = self.webgl2 && indexed_strip;
                let mut out: Vec<u8> = bytes[offset as usize..(offset + count as u64) as usize]
                    .iter()
                    .flat_map(|&b| if restart && b == 0xff { 0xffff } else { u16::from(b) }.to_le_bytes())
                    .collect();
                out.resize(out.len().next_multiple_of(4), 0);
                generated_indices = Some(out);
            } else {
                let Some(raw) = self.buffer(id).and_then(|b| b.raw.clone()) else { return self.error(INVALID_OPERATION) };
                let generation = self.generation;
                if let Some(b) = self.buffer_mut(id) {
                    b.used = generation;
                }
                index_source = Some((Source::Buffer(raw), offset));
            }
        }
        let call_count = match &generated_indices {
            Some(g) if generated => (g.len() / 4) as u32,
            _ => count as u32,
        };
        if call_count == 0 {
            return;
        }
        let pipeline = match self.pipeline(&linked, &target, &vertex_layouts, &layout_bindings, (topology, indexed_strip.then_some(index_format), wide)) {
            Some(p) => p,
            None => return,
        };
        let scissor = match self.scissor_rect(&target) {
            Some(s) => s,
            None => return,
        };
        let [vx, vy, vw, vh] = self.state.viewport;
        let [near, far] = self.state.depth_range;
        let viewport = [vx as f32, vy as f32, vw as f32, vh as f32, near, far];
        let [sf, ..] = self.state.stencil;
        let blend = self.blend_constant();
        let align = self.limits.uniform_alignment;
        let mut data = self.program(program).map(|p| (p.data.clone(), p.data_version)).unwrap_or_default();
        let mut sizes = info.uniform_sizes;
        if let Some(points) = info.points.as_ref().filter(|_| wide) {
            let vertex = &mut data.0[0];
            vertex.resize(points.size as usize, 0);
            let at = points.viewport as usize;
            vertex[at..at + 4].copy_from_slice(&(vw.max(1) as f32).to_le_bytes());
            vertex[at + 4..at + 8].copy_from_slice(&(vh.max(1) as f32).to_le_bytes());
            sizes[0] = points.size;
        }
        self.pass_for_draw(target);
        let pass = self.recorder.pass.as_mut().unwrap_or_else(|| unreachable!());
        let uniforms = [pass.push_data(&data.0[0], align), pass.push_data(&data.0[1], align)];
        pass.span = pass.span.max(sizes[0]).max(sizes[1]).max(16);
        let mut vertex: Vec<(u32, Source, u64)> = sources;
        for (location, bytes) in generics {
            let at = pass.push_data(&bytes, 4);
            vertex.push((location, Source::Data, u64::from(at)));
        }
        vertex.sort_by_key(|v| v.0);
        let occlusion = self.state.queries[..2].iter().find_map(|q| q.1).map(|id| {
            pass.queries.push(id);
            pass.queries.len() as u32 - 1
        });
        let index = match (generated_indices, index_source) {
            (Some(g), _) => Some((Source::Data, u64::from(pass.push_data(&g, 4)), index_format)),
            (None, Some((s, offset))) => Some((s, offset, index_format)),
            _ => None,
        };
        let slots: FastMap<u32, u32> = vertex_layouts.iter().enumerate().map(|(i, l)| (l.0, i as u32)).collect();
        let vertex = vertex.into_iter().map(|(location, s, o)| (slots[&location], s, o)).collect();
        let call = match &index {
            Some(_) if generated => Call::Indexed { indices: 0..call_count, base_vertex: 0, instances: 0..instances },
            Some(_) => Call::Indexed { indices: 0..call_count, base_vertex: 0, instances: 0..instances },
            None if wide => Call::Draw { vertices: 0..6, instances: first as u32..(first + count) as u32 },
            None => Call::Draw { vertices: first as u32..(first + count) as u32, instances: 0..instances },
        };
        pass.draws.push(Draw {
            pipeline,
            resources,
            uniforms,
            vertex,
            index,
            call,
            viewport,
            scissor,
            stencil: (sf.reference.max(0) as u32) & 0xff,
            blend,
            occlusion,
        });
    }

    fn blend_constant(&self) -> [f64; 4] {
        let c = self.state.blend_color.map(f64::from);
        let alpha = self.state.blend_func.iter().any(|f| matches!(*f, CONSTANT_ALPHA | ONE_MINUS_CONSTANT_ALPHA));
        if alpha { [c[3]; 4] } else { c }
    }

    pub(crate) fn scissor_rect(&self, target: &Target) -> Option<[u32; 4]> {
        let (w, h) = (target.width as i64, target.height as i64);
        if !self.state.scissor_test {
            return Some([0, 0, w as u32, h as u32]);
        }
        let [x, y, sw, sh] = self.state.scissor.map(i64::from);
        let (x0, y0) = (x.clamp(0, w), y.clamp(0, h));
        let (x1, y1) = ((x + sw).clamp(0, w), (y + sh).clamp(0, h));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some([x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
    }

    fn convert_attribute(&mut self, id: Id, version: u64, raw: &wgpu::Buffer, a: &Attrib, elements: u32) -> wgpu::Buffer {
        let key = (id, version, a.ty, a.size, a.normalized, a.integer, a.offset, a.effective_stride(), a.divisor, elements);
        if let Some(b) = self.converted.get(&key) {
            return b.clone();
        }
        if self.converted.len() > 256 {
            self.converted.clear();
        }
        let device = self.gpu.device.clone();
        let out = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acde webgl converted attribute"),
            size: u64::from(elements.max(1)) * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        let divisor = if a.divisor > 1 { a.divisor } else { 0 };
        let params = [a.offset as u32, a.effective_stride(), elements, a.size, a.ty, u32::from(a.normalized), u32::from(a.integer), divisor];
        self.run_compute("convert", raw, &out, &params, elements);
        self.converted.insert(key, out.clone());
        out
    }

    fn resources(&mut self, linked: &Rc<Linked>, program: Id, target: &Target) -> Result<Resources, u32> {
        let info = &linked.info;
        let units = self.program(program).map(|p| p.units.clone()).unwrap_or_default();
        let block_bindings = self.program(program).map(|p| p.block_bindings.clone()).unwrap_or_default();
        let mut bindings = Vec::new();
        let mut entries: Vec<Entry> = Vec::new();
        let mut sampled = Vec::new();
        let mut used_units: FastMap<u32, (wgpu::TextureViewDimension, Sampled)> = FastMap::default();
        for (si, s) in info.samplers.iter().enumerate() {
            for element in 0..s.size as usize {
                let unit = units.get(si).and_then(|u| u.get(element)).copied().unwrap_or(0);
                if let Some(&(dim, kind)) = used_units.get(&unit)
                    && (dim != s.dim || kind != s.sampled)
                {
                    return Err(INVALID_OPERATION);
                }
                used_units.insert(unit, (s.dim, s.sampled));
                let resolved = self.resolve_texture(unit, s.dim, s.sampled);
                let unit_texture = {
                    let u = self.state.units.get(unit as usize).copied().unwrap_or_default();
                    match s.dim {
                        wgpu::TextureViewDimension::Cube => u.cube,
                        wgpu::TextureViewDimension::D3 => u.texture_3d,
                        wgpu::TextureViewDimension::D2Array => u.array,
                        _ => u.texture_2d,
                    }
                };
                if let Some(t) = unit_texture {
                    if target.textures.contains(&t) && resolved.key.0 != u64::MAX {
                        return Err(INVALID_OPERATION);
                    }
                    sampled.push(t);
                }
                let sampler_kind = match resolved.sample {
                    SampleKind::Depth => SamplerKind::Comparison,
                    SampleKind::Float => SamplerKind::Filtering,
                    _ => SamplerKind::NonFiltering,
                };
                let mut stage_bindings: Vec<(u32, u32, u8)> = Vec::new();
                for (stage, list) in s.bindings.iter().enumerate() {
                    if let Some(&(t, smp)) = list.get(element) {
                        stage_bindings.push((t, smp, 1 << stage));
                    }
                }
                for (t, smp, stage) in stage_bindings {
                    let tb = Binding::Texture { binding: t, dim: s.dim, sample: resolved.sample, stages: stage };
                    let sb = Binding::Sampler { binding: smp, kind: sampler_kind, stages: stage };
                    entries.push((tb, Some(resolved.view.clone()), None, None, resolved.key));
                    entries.push((sb, None, Some(resolved.sampler), None, sampler_hash(&resolved.sampler)));
                }
            }
        }
        for (bi, b) in info.blocks.iter().enumerate() {
            let point = block_bindings.get(bi).copied().unwrap_or(0) as usize;
            let indexed = self.state.uniform_buffers.get(point).copied().unwrap_or_default();
            let Some(buffer_id) = indexed.buffer else { return Err(INVALID_OPERATION) };
            let Some(buffer) = self.buffer(buffer_id) else { return Err(INVALID_OPERATION) };
            let size =
                if indexed.size == 0 { buffer.size.saturating_sub(indexed.offset) } else { indexed.size.min(buffer.size.saturating_sub(indexed.offset)) };
            if size < u64::from(b.size) {
                return Err(INVALID_OPERATION);
            }
            let Some(raw) = buffer.raw.clone() else { return Err(INVALID_OPERATION) };
            let version = buffer.version;
            let generation = self.generation;
            if let Some(buffer) = self.buffer_mut(buffer_id) {
                buffer.used = generation;
            }
            for (stage, binding) in b.bindings.iter().enumerate() {
                let Some(binding) = binding else { continue };
                let ub = Binding::Uniform { binding: *binding, size: b.size, stages: 1 << stage };
                let bound = u64::from(b.size).max(size.min(u64::from(self.limits.block_size)));
                entries.push((ub, None, None, Some((raw.clone(), indexed.offset, bound)), (u64::from(buffer_id), version, indexed.offset << 24 | bound)));
            }
        }
        if entries.is_empty() {
            return Ok((bindings, None, sampled));
        }
        entries.sort_by_key(|e| binding_number(&e.0));
        bindings = entries.iter().map(|e| e.0).collect();
        let key = GroupKey { layout: bindings.clone(), entries: entries.iter().map(|e| e.4).collect() };
        if let Some(group) = self.caches.groups.get(&key) {
            return Ok((bindings, Some(group.clone()), sampled));
        }
        let device = self.gpu.device.clone();
        let layout = self.caches.layout(&device, &bindings);
        let samplers: Vec<Option<wgpu::Sampler>> = entries.iter().map(|e| e.2.map(|k| self.caches.sampler(&device, k))).collect();
        let group = self.scoped(|device| {
            let resources: Vec<wgpu::BindGroupEntry> = entries
                .iter()
                .zip(&samplers)
                .map(|(e, sampler)| wgpu::BindGroupEntry {
                    binding: binding_number(&e.0),
                    resource: match (&e.1, sampler, &e.3) {
                        (Some(view), ..) => wgpu::BindingResource::TextureView(view),
                        (_, Some(s), _) => wgpu::BindingResource::Sampler(s),
                        (_, _, Some((buffer, offset, size))) => {
                            wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: *offset, size: wgpu::BufferSize::new(*size) })
                        }
                        _ => unreachable!(),
                    },
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("acde webgl resources"), layout: &layout.0, entries: &resources })
        });
        let group = match group {
            Ok(g) => g,
            Err(e) => {
                log::warn!("webgl: bind group: {e}");
                return Err(INVALID_OPERATION);
            }
        };
        if self.caches.groups.len() > 4096 {
            self.caches.groups.clear();
        }
        self.caches.groups.insert(key, group.clone());
        Ok((bindings, Some(group), sampled))
    }

    fn pipeline(
        &mut self,
        linked: &Rc<Linked>,
        target: &Target,
        vertex: &[(u32, u64, bool, wgpu::VertexFormat)],
        bindings: &[Binding],
        (topology, strip, wide): (wgpu::PrimitiveTopology, Option<wgpu::IndexFormat>, bool),
    ) -> Option<wgpu::RenderPipeline> {
        let s = &self.state;
        let default = s.draw_framebuffer.is_none();
        let outputs: Vec<u32> = linked.info.outputs.iter().map(|o| o.location).collect();
        let mask_alpha = default && !self.attrs.alpha;
        let mut write = 0u32;
        for (i, on) in s.color_mask.iter().enumerate() {
            if *on && !(i == 3 && mask_alpha) {
                write |= 1 << i;
            }
        }
        let blend_on = s.blend;
        let blend = BlendKey {
            color: (factor(s.blend_func[0]), factor(s.blend_func[1]), operation(s.blend_equation[0])),
            alpha: (factor(s.blend_func[2]), factor(s.blend_func[3]), operation(s.blend_equation[1])),
        };
        let blend = BlendKey {
            color: if matches!(blend.color.2, wgpu::BlendOperation::Min | wgpu::BlendOperation::Max) {
                (wgpu::BlendFactor::One, wgpu::BlendFactor::One, blend.color.2)
            } else {
                blend.color
            },
            alpha: if matches!(blend.alpha.2, wgpu::BlendOperation::Min | wgpu::BlendOperation::Max) {
                (wgpu::BlendFactor::One, wgpu::BlendFactor::One, blend.alpha.2)
            } else {
                blend.alpha
            },
        };
        let float32_blend = self.features.contains(wgpu::Features::FLOAT32_BLENDABLE);
        let targets: Vec<Option<(wgpu::TextureFormat, Option<BlendKey>, u32)>> = target
            .colors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                c.as_ref().map(|c| {
                    let blendable = matches!(c.class, Class::Float) || (c.class == Class::Unfilterable && float32_blend);
                    let has_output = outputs.contains(&(i as u32));
                    (c.format, (blend_on && blendable && has_output).then_some(blend), if has_output { write } else { 0 })
                })
            })
            .collect();
        let depth = target.depth.as_ref().map(|d| {
            let test = s.depth_test && d.depth;
            let stencil_test = s.stencil_test && d.stencil;
            let face = |f: &crate::webgl::state::StencilFace| {
                if stencil_test {
                    (compare(f.func), stencil_op(f.fail), stencil_op(f.depth_fail), stencil_op(f.pass))
                } else {
                    (wgpu::CompareFunction::Always, wgpu::StencilOperation::Keep, wgpu::StencilOperation::Keep, wgpu::StencilOperation::Keep)
                }
            };
            let fill = s.polygon_offset_fill && matches!(topology, wgpu::PrimitiveTopology::TriangleList | wgpu::PrimitiveTopology::TriangleStrip);
            DepthKey {
                format: d.format,
                write: test && s.depth_mask,
                compare: if test { compare(s.depth_func) } else { wgpu::CompareFunction::Always },
                stencil: [face(&s.stencil[0]), face(&s.stencil[1])],
                read_mask: if stencil_test { s.stencil[0].value_mask & 0xff } else { 0xff },
                write_mask: if stencil_test { s.stencil[0].write_mask & 0xff } else { 0 },
                bias: if fill { (s.polygon_offset[1] as i32, s.polygon_offset[0].to_bits(), 0) } else { (0, 0, 0) },
            }
        });
        let triangles = matches!(topology, wgpu::PrimitiveTopology::TriangleList | wgpu::PrimitiveTopology::TriangleStrip);
        let cull = if s.cull_face && triangles {
            match s.cull_mode {
                FRONT => Some(wgpu::Face::Front),
                BACK => Some(wgpu::Face::Back),
                _ => None,
            }
        } else {
            None
        };
        let cull_all = s.cull_face && triangles && s.cull_mode == FRONT_AND_BACK;
        let mut mask = u64::MAX;
        if s.sample_coverage && target.samples > 1 {
            let n = (s.sample_coverage_value * target.samples as f32).round() as u32;
            let bits = (1u64 << n) - 1;
            mask = if s.sample_coverage_invert { !bits } else { bits };
        }
        let key = PipelineKey {
            program: linked.serial,
            layout: bindings.to_vec(),
            vertex: vertex.to_vec(),
            topology,
            strip,
            front_face: if s.front_face == CCW { wgpu::FrontFace::Cw } else { wgpu::FrontFace::Ccw },
            cull,
            depth,
            samples: target.samples,
            alpha_to_coverage: s.alpha_to_coverage && target.samples > 1,
            mask,
            targets,
            discard: cull_all,
            points: wide,
        };
        if cull_all {
            return None;
        }
        if let Some(p) = self.caches.pipelines.get(&key) {
            return Some(p.clone());
        }
        let device = self.gpu.device.clone();
        let layout = self.caches.layout(&device, bindings);
        let attributes: Vec<[wgpu::VertexAttribute; 1]> =
            key.vertex.iter().map(|(location, _, _, format)| [wgpu::VertexAttribute { format: *format, offset: 0, shader_location: *location }]).collect();
        let buffers: Vec<Option<wgpu::VertexBufferLayout>> = key
            .vertex
            .iter()
            .zip(&attributes)
            .map(|((_, stride, instance, _), attrs)| {
                Some(wgpu::VertexBufferLayout {
                    array_stride: *stride,
                    step_mode: if *instance { wgpu::VertexStepMode::Instance } else { wgpu::VertexStepMode::Vertex },
                    attributes: attrs,
                })
            })
            .collect();
        let color_targets: Vec<Option<wgpu::ColorTargetState>> = key
            .targets
            .iter()
            .map(|t| {
                t.map(|(format, blend, write)| wgpu::ColorTargetState {
                    format,
                    blend: blend.map(|b| wgpu::BlendState {
                        color: wgpu::BlendComponent { src_factor: b.color.0, dst_factor: b.color.1, operation: b.color.2 },
                        alpha: wgpu::BlendComponent { src_factor: b.alpha.0, dst_factor: b.alpha.1, operation: b.alpha.2 },
                    }),
                    write_mask: wgpu::ColorWrites::from_bits_truncate(write),
                })
            })
            .collect();
        let depth_stencil = key.depth.map(|d| {
            let face = |f: (wgpu::CompareFunction, wgpu::StencilOperation, wgpu::StencilOperation, wgpu::StencilOperation)| wgpu::StencilFaceState {
                compare: f.0,
                fail_op: f.1,
                depth_fail_op: f.2,
                pass_op: f.3,
            };
            wgpu::DepthStencilState {
                format: d.format,
                depth_write_enabled: d.format.has_depth_aspect().then_some(d.write),
                depth_compare: d.format.has_depth_aspect().then_some(d.compare),
                stencil: wgpu::StencilState { front: face(d.stencil[0]), back: face(d.stencil[1]), read_mask: d.read_mask, write_mask: d.write_mask },
                bias: wgpu::DepthBiasState { constant: d.bias.0, slope_scale: f32::from_bits(d.bias.1), clamp: 0.0 },
            }
        });
        let modules = match (&linked.points, wide) {
            (Some((v, f)), true) => (v, f),
            _ => (&linked.vertex, &linked.fragment),
        };
        let started = std::time::Instant::now();
        let result = self.scoped(|device| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("acde webgl"),
                layout: Some(&layout.1),
                vertex: wgpu::VertexState { module: modules.0, entry_point: Some("main"), compilation_options: Default::default(), buffers: &buffers },
                primitive: wgpu::PrimitiveState {
                    topology,
                    strip_index_format: strip,
                    front_face: key.front_face,
                    cull_mode: cull,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil,
                multisample: wgpu::MultisampleState { count: key.samples, mask: key.mask, alpha_to_coverage_enabled: key.alpha_to_coverage },
                fragment: Some(wgpu::FragmentState {
                    module: modules.1,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    targets: &color_targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        match result {
            Ok(p) => {
                log::debug!(target: "perf::webgl", "pipeline program={} cached={} ms={:.1} key={key:?}", key.program, self.caches.pipelines.len(), crate::log::since_ms(started));
                if self.caches.pipelines.len() > 2048 {
                    self.caches.pipelines.clear();
                }
                self.caches.pipelines.insert(key, p.clone());
                Some(p)
            }
            Err(e) => {
                log::warn!("webgl: render pipeline: {e}");
                self.error(INVALID_OPERATION);
                None
            }
        }
    }

    pub fn clear(&mut self, mask: u32) {
        if mask & !(COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT | STENCIL_BUFFER_BIT) != 0 {
            return self.error(INVALID_VALUE);
        }
        let target = match self.draw_target() {
            Ok(t) => t,
            Err(e) => return self.error(e),
        };
        if self.state.rasterizer_discard {
            return;
        }
        let color = mask & COLOR_BUFFER_BIT != 0;
        let depth = mask & DEPTH_BUFFER_BIT != 0 && target.depth.as_ref().is_some_and(|d| d.depth);
        let stencil = mask & STENCIL_BUFFER_BIT != 0 && target.depth.as_ref().is_some_and(|d| d.stencil);
        if color && target.colors.iter().flatten().any(|c| matches!(c.class, Class::Sint | Class::Uint)) {
            return self.error(INVALID_OPERATION);
        }
        let value = self.state.clear_color;
        self.clear_buffers(
            target,
            if color { Some(ClearColor::Float(value)) } else { None },
            None,
            depth.then_some(self.state.clear_depth),
            stencil.then_some(self.state.clear_stencil),
        );
    }

    pub fn clear_buffer(&mut self, buffer: u32, drawbuffer: i32, value: ClearColor, depth: f32, stencil: i32) {
        let target = match self.draw_target() {
            Ok(t) => t,
            Err(e) => return self.error(e),
        };
        if drawbuffer < 0 {
            return self.error(INVALID_VALUE);
        }
        match buffer {
            COLOR => {
                if drawbuffer as u32 >= self.limits.draw_buffers {
                    return self.error(INVALID_VALUE);
                }
                let Some(Some(c)) = target.colors.get(drawbuffer as usize) else { return };
                let ok = matches!(
                    (value, c.class),
                    (ClearColor::Float(_), Class::Float | Class::Unfilterable) | (ClearColor::Int(_), Class::Sint) | (ClearColor::Uint(_), Class::Uint)
                );
                if !ok {
                    return self.error(INVALID_OPERATION);
                }
                self.clear_buffers(target, None, Some((drawbuffer as usize, value)), None, None);
            }
            DEPTH | STENCIL | DEPTH_STENCIL => {
                if drawbuffer != 0 {
                    return self.error(INVALID_VALUE);
                }
                let d = target.depth.as_ref();
                let depth = (buffer != STENCIL && d.is_some_and(|d| d.depth)).then_some(depth.clamp(0.0, 1.0));
                let stencil = (buffer != DEPTH && d.is_some_and(|d| d.stencil)).then_some(stencil);
                self.clear_buffers(target, None, None, depth, stencil);
            }
            _ => self.error(INVALID_ENUM),
        }
    }

    fn clear_buffers(&mut self, target: Target, all: Option<ClearColor>, one: Option<(usize, ClearColor)>, depth: Option<f32>, stencil: Option<i32>) {
        if all.is_none() && one.is_none() && depth.is_none() && stencil.is_none() {
            return;
        }
        let Some(scissor) = self.scissor_rect(&target) else { return };
        let full = scissor == [0, 0, target.width, target.height];
        let default = self.state.draw_framebuffer.is_none();
        let mask_alpha = default && !self.attrs.alpha;
        let color_full = self.state.color_mask.iter().enumerate().all(|(i, on)| *on || (i == 3 && mask_alpha));
        let stencil_full = self.state.stencil[0].write_mask & 0xff == 0xff;
        let depth_mask = self.state.depth_mask;
        let colors: Vec<(usize, ClearColor)> = match (all, one) {
            (Some(c), _) => (0..target.colors.len()).filter(|i| target.colors[*i].is_some()).map(|i| (i, c)).collect(),
            (_, Some(o)) => vec![o],
            _ => Vec::new(),
        };
        let load_ok = full && (colors.is_empty() || color_full) && (depth.is_none() || depth_mask) && (stencil.is_none() || stencil_full);
        if load_ok {
            self.pass_for_draw(target.clone());
            let has_draws = self.recorder.pass.as_ref().is_some_and(|p| !p.draws.is_empty());
            if has_draws {
                self.flush_pass();
                self.pass_for_draw(target);
            }
            let alpha = self.attrs.alpha;
            let pass = self.recorder.pass.as_mut().unwrap_or_else(|| unreachable!());
            for (i, c) in colors {
                let v = match c {
                    ClearColor::Float(v) => v.map(f64::from),
                    ClearColor::Int(v) => v.map(f64::from),
                    ClearColor::Uint(v) => v.map(f64::from),
                };
                let a = if mask_alpha || (default && !alpha) { 1.0 } else { v[3] };
                if let Some(slot) = pass.colors.get_mut(i) {
                    *slot = Some(wgpu::Color { r: v[0], g: v[1], b: v[2], a });
                }
            }
            if let Some(d) = depth {
                pass.depth = Some(d);
            }
            if let Some(s) = stencil {
                pass.stencil = Some((s as u32) & 0xff);
            }
            return;
        }
        self.clear_draw(target, colors, depth.filter(|_| depth_mask), stencil, scissor);
    }

    fn clear_draw(&mut self, target: Target, colors: Vec<(usize, ClearColor)>, depth: Option<f32>, stencil: Option<i32>, scissor: [u32; 4]) {
        let default = self.state.draw_framebuffer.is_none();
        let mask_alpha = default && !self.attrs.alpha;
        let mut write = 0u32;
        for (i, on) in self.state.color_mask.iter().enumerate() {
            if *on && !(i == 3 && mask_alpha) {
                write |= 1 << i;
            }
        }
        let mut source = String::from("struct C { f: vec4f, i: vec4i, u: vec4u, d: f32 }\n@group(0) @binding(1) var<uniform> c: C;\n");
        source.push_str(crate::webgl::helpers::FULLSCREEN);
        source.push_str("struct Out {\n");
        let mut targets = Vec::new();
        let mut name = String::from("clear");
        for (i, t) in target.colors.iter().enumerate() {
            let chosen = colors.iter().find(|(k, _)| *k == i);
            match (t, chosen) {
                (Some(t), Some((_, value))) => {
                    let ty = match value {
                        ClearColor::Float(_) => "vec4f",
                        ClearColor::Int(_) => "vec4i",
                        ClearColor::Uint(_) => "vec4u",
                    };
                    source.push_str(&format!("@location({i}) o{i}: {ty},\n"));
                    targets.push(Some(wgpu::ColorTargetState { format: t.format, blend: None, write_mask: wgpu::ColorWrites::from_bits_truncate(write) }));
                    name.push_str(&format!(" {i}:{:?}:{write}", t.format));
                }
                (Some(t), None) => {
                    targets.push(Some(wgpu::ColorTargetState { format: t.format, blend: None, write_mask: wgpu::ColorWrites::empty() }));
                    name.push_str(&format!(" {i}:{:?}:-", t.format));
                }
                (None, _) => targets.push(None),
            }
        }
        if depth.is_some() {
            source.push_str("@builtin(frag_depth) depth: f32,\n");
        }
        source.push_str("pad: f32 }\n");
        let source = source.replace("pad: f32 }\n", "}\n");
        let mut body = String::from("@fragment fn fs() -> Out {\nvar o: Out;\n");
        for (i, value) in &colors {
            let field = match value {
                ClearColor::Float(_) => "f",
                ClearColor::Int(_) => "i",
                ClearColor::Uint(_) => "u",
            };
            body.push_str(&format!("o.o{i} = c.{field};\n"));
        }
        if depth.is_some() {
            body.push_str("o.depth = c.d;\n");
        }
        body.push_str("return o;\n}\n");
        let mut source = source;
        if colors.is_empty() && depth.is_none() {
            source = source.replace("struct Out {\n}\n", "");
            body = String::from("@fragment fn fs() {}\n");
        }
        source.push_str(&body);
        let depth_state = target.depth.as_ref().map(|d| wgpu::DepthStencilState {
            format: d.format,
            depth_write_enabled: d.depth.then_some(depth.is_some()),
            depth_compare: d.depth.then_some(wgpu::CompareFunction::Always),
            stencil: match stencil.is_some() && d.stencil {
                true => {
                    let face = wgpu::StencilFaceState {
                        compare: wgpu::CompareFunction::Always,
                        fail_op: wgpu::StencilOperation::Keep,
                        depth_fail_op: wgpu::StencilOperation::Keep,
                        pass_op: wgpu::StencilOperation::Replace,
                    };
                    wgpu::StencilState { front: face, back: face, read_mask: 0xff, write_mask: self.state.stencil[0].write_mask & 0xff }
                }
                false => wgpu::StencilState::default(),
            },
            bias: wgpu::DepthBiasState::default(),
        });
        name.push_str(&format!(
            " d{:?}:{}:{} s{}",
            depth_state.as_ref().map(|d| d.format),
            depth.is_some(),
            target.samples,
            depth_state.as_ref().map_or(0, |d| d.stencil.write_mask)
        ));
        let layout = self.caches.empty.clone();
        let samples = target.samples;
        let pipeline = {
            if let Some(p) = self.caches.helpers.get(&name) {
                p.clone()
            } else {
                let result = self.scoped(|device| {
                    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("acde webgl clear"),
                        source: wgpu::ShaderSource::Wgsl(source.clone().into()),
                    });
                    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("acde webgl clear"),
                        layout: Some(&layout.1),
                        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                        primitive: wgpu::PrimitiveState::default(),
                        depth_stencil: depth_state.clone(),
                        multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                        fragment: Some(wgpu::FragmentState {
                            module: &module,
                            entry_point: Some("fs"),
                            compilation_options: Default::default(),
                            targets: &targets,
                        }),
                        multiview_mask: None,
                        cache: None,
                    })
                });
                match result {
                    Ok(p) => {
                        self.caches.helpers.insert(name, p.clone());
                        p
                    }
                    Err(e) => {
                        log::warn!("webgl: clear pipeline: {e}");
                        return;
                    }
                }
            }
        };
        let mut data = Vec::with_capacity(64);
        let value = colors.first().map(|c| c.1).unwrap_or(ClearColor::Float([0.0; 4]));
        let alpha = !mask_alpha;
        let (f, i, u) = match value {
            ClearColor::Float(v) => (if alpha { v } else { [v[0], v[1], v[2], 1.0] }, [0; 4], [0; 4]),
            ClearColor::Int(v) => ([0.0; 4], v, [0; 4]),
            ClearColor::Uint(v) => ([0.0; 4], [0; 4], v),
        };
        f.iter().for_each(|x| data.extend_from_slice(&x.to_le_bytes()));
        i.iter().for_each(|x| data.extend_from_slice(&x.to_le_bytes()));
        u.iter().for_each(|x| data.extend_from_slice(&x.to_le_bytes()));
        data.extend_from_slice(&depth.unwrap_or(1.0).to_le_bytes());
        data.resize(64, 0);
        let align = self.limits.uniform_alignment;
        let (w, h) = (target.width as f32, target.height as f32);
        let empty = self.empty_group();
        self.pass_for_draw(target);
        let pass = self.recorder.pass.as_mut().unwrap_or_else(|| unreachable!());
        let at = pass.push_data(&data, align);
        pass.span = pass.span.max(64);
        pass.draws.push(Draw {
            pipeline,
            resources: Some(empty),
            uniforms: [at, at],
            vertex: Vec::new(),
            index: None,
            call: Call::Draw { vertices: 0..3, instances: 0..1 },
            viewport: [0.0, 0.0, w, h, 0.0, 1.0],
            scissor,
            stencil: stencil.map_or(0, |s| (s as u32) & 0xff),
            blend: [0.0; 4],
            occlusion: None,
        });
    }

    fn empty_group(&mut self) -> wgpu::BindGroup {
        let key = GroupKey { layout: Vec::new(), entries: Vec::new() };
        if let Some(g) = self.caches.groups.get(&key) {
            return g.clone();
        }
        let g = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("acde webgl empty"), layout: &self.caches.empty.0, entries: &[] });
        self.caches.groups.insert(key, g.clone());
        g
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ClearColor {
    Float([f32; 4]),
    Int([i32; 4]),
    Uint([u32; 4]),
}

fn expand(mode: u32, v: &[u32]) -> Vec<u32> {
    let mut out = Vec::new();
    match mode {
        LINE_LOOP if v.len() >= 2 => {
            for i in 0..v.len() {
                out.push(v[i]);
                out.push(v[(i + 1) % v.len()]);
            }
        }
        TRIANGLE_FAN if v.len() >= 3 => {
            for i in 1..v.len() - 1 {
                out.extend([v[0], v[i], v[i + 1]]);
            }
        }
        _ => {}
    }
    out
}

fn binding_number(b: &Binding) -> u32 {
    match *b {
        Binding::Texture { binding, .. } | Binding::Sampler { binding, .. } | Binding::Uniform { binding, .. } => binding,
    }
}

fn sampler_hash(k: &SamplerKey) -> (u64, u64, u64) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    k.hash(&mut h);
    (u64::MAX - 1, h.finish(), 0)
}

type Resources = (Vec<Binding>, Option<wgpu::BindGroup>, Vec<Id>);

type Entry = (Binding, Option<wgpu::TextureView>, Option<SamplerKey>, Option<(wgpu::Buffer, u64, u64)>, (u64, u64, u64));
