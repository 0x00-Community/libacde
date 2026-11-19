use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::webgl::consts::*;
use crate::webgl::context::Context;
use crate::webgl::format::{self, Class, Expand, Format, Unpack};
use crate::webgl::objects::*;

pub enum TexSource {
    Cpu { width: u32, height: u32, rgba: Vec<u8> },
    Gpu(Arc<crate::gpu::GpuImage>),
}

pub(crate) fn mip_count(width: u32, height: u32, depth: u32) -> u32 {
    32 - width.max(height).max(depth).max(1).leading_zeros()
}

fn face_index(target: u32) -> Option<u32> {
    (TEXTURE_CUBE_MAP_POSITIVE_X..=TEXTURE_CUBE_MAP_NEGATIVE_Z).contains(&target).then(|| target - TEXTURE_CUBE_MAP_POSITIVE_X)
}

impl Context {
    pub fn create_texture(&mut self) -> Id {
        self.add(Object::Texture(Texture::new()))
    }

    fn unit_slot(&mut self, target: u32) -> Option<&mut Option<Id>> {
        let webgl2 = self.webgl2;
        let unit = &mut self.state.units[self.state.active_texture as usize];
        Some(match target {
            TEXTURE_2D => &mut unit.texture_2d,
            TEXTURE_CUBE_MAP => &mut unit.cube,
            TEXTURE_3D if webgl2 => &mut unit.texture_3d,
            TEXTURE_2D_ARRAY if webgl2 => &mut unit.array,
            _ => return None,
        })
    }

    pub fn bind_texture(&mut self, target: u32, id: Option<Id>) {
        if self.unit_slot(target).is_none() {
            return self.error(INVALID_ENUM);
        }
        if let Some(id) = id {
            let Some(t) = self.texture_mut(id) else { return self.error(INVALID_OPERATION) };
            if t.target != 0 && t.target != target {
                return self.error(INVALID_OPERATION);
            }
            t.target = target;
        }
        if let Some(slot) = self.unit_slot(target) {
            *slot = id;
        }
    }

    pub fn delete_texture(&mut self, id: Id) {
        if self.texture(id).is_none() {
            return;
        }
        for unit in &mut self.state.units {
            for slot in [&mut unit.texture_2d, &mut unit.cube, &mut unit.texture_3d, &mut unit.array] {
                if *slot == Some(id) {
                    *slot = None;
                }
            }
        }
        let draw = self.state.draw_framebuffer;
        let read = self.state.read_framebuffer;
        for fb in [draw, read].into_iter().flatten() {
            if let Some(f) = self.framebuffer_mut(fb) {
                let hit = |a: &Option<Attachment>| matches!(a, Some(Attachment::Texture { id: t, .. }) if *t == id);
                for c in &mut f.color {
                    if hit(c) {
                        *c = None;
                    }
                }
                if hit(&f.depth) {
                    f.depth = None;
                }
                if hit(&f.stencil) {
                    f.stencil = None;
                }
            }
        }
        self.objects.remove(&id);
    }

    pub(crate) fn bound_texture(&mut self, target: u32) -> Option<(Id, u32)> {
        let (bind, face) = match face_index(target) {
            Some(f) => (TEXTURE_CUBE_MAP, f),
            None => (target, 0),
        };
        let Some(slot) = self.unit_slot(bind) else {
            self.error(INVALID_ENUM);
            return None;
        };
        let Some(id) = *slot else {
            self.error(INVALID_OPERATION);
            return None;
        };
        Some((id, face))
    }

    pub fn tex_parameter(&mut self, target: u32, name: u32, value: f32) {
        if !matches!(target, TEXTURE_2D | TEXTURE_CUBE_MAP) && !(self.webgl2 && matches!(target, TEXTURE_3D | TEXTURE_2D_ARRAY)) {
            return self.error(INVALID_ENUM);
        }
        let Some(slot) = self.unit_slot(target) else { return self.error(INVALID_ENUM) };
        let Some(id) = *slot else { return self.error(INVALID_OPERATION) };
        let webgl2 = self.webgl2;
        let anisotropy = self.extension("EXT_texture_filter_anisotropic");
        let Some(t) = self.texture_mut(id) else { return };
        match name {
            TEXTURE_BASE_LEVEL if webgl2 => match value < 0.0 {
                true => return self.error(INVALID_VALUE),
                false => t.base_level = value as u32,
            },
            TEXTURE_MAX_LEVEL if webgl2 => match value < 0.0 {
                true => return self.error(INVALID_VALUE),
                false => t.max_level = value as u32,
            },
            _ => {
                let mut params = t.params;
                match set_sampler_param(&mut params, name, value, webgl2, anisotropy) {
                    Ok(()) => {
                        if let Some(t) = self.texture_mut(id) {
                            t.params = params;
                        }
                    }
                    Err(code) => return self.error(code),
                }
            }
        }
        if let Some(t) = self.texture_mut(id) {
            t.version += 1;
        }
    }

    pub fn get_tex_parameter(&mut self, target: u32, name: u32) -> Option<f32> {
        let Some(slot) = self.unit_slot(target) else {
            self.error(INVALID_ENUM);
            return None;
        };
        let id = (*slot)?;
        let webgl2 = self.webgl2;
        let t = self.texture(id)?;
        Some(match name {
            TEXTURE_BASE_LEVEL if webgl2 => t.base_level as f32,
            TEXTURE_MAX_LEVEL if webgl2 => t.max_level as f32,
            TEXTURE_IMMUTABLE_FORMAT if webgl2 => f32::from(u8::from(t.immutable)),
            TEXTURE_IMMUTABLE_LEVELS if webgl2 => t.immutable_levels as f32,
            _ => match get_sampler_param(&t.params, name, webgl2) {
                Some(v) => v,
                None => {
                    self.error(INVALID_ENUM);
                    return None;
                }
            },
        })
    }

    fn alloc(&mut self, id: Id, format: Format, width: u32, height: u32, layers: u32, mips: u32) -> bool {
        let Some(t) = self.texture(id) else { return false };
        let dimension = if t.target == TEXTURE_3D { wgpu::TextureDimension::D3 } else { wgpu::TextureDimension::D2 };
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC;
        let allowed = format.wgpu.guaranteed_format_features(self.features).allowed_usages;
        if allowed.contains(wgpu::TextureUsages::RENDER_ATTACHMENT) {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }
        let created = self.scoped(|device| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("acde webgl texture"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: layers },
                mip_level_count: mips,
                sample_count: 1,
                dimension,
                format: format.wgpu,
                usage,
                view_formats: &[],
            })
        });
        let texture = match created {
            Ok(t) => t,
            Err(e) => {
                log::warn!("webgl: texture: {e}");
                self.error(OUT_OF_MEMORY);
                return false;
            }
        };
        if usage.contains(wgpu::TextureUsages::RENDER_ATTACHMENT) {
            self.clear_to_defaults(&texture, mips, if dimension == wgpu::TextureDimension::D3 { 1 } else { layers });
        }
        let old = self.texture_mut(id).and_then(|t| t.alloc.take());
        if let Some(old) = old.filter(|o| o.format == format && o.layers == layers) {
            self.flush_pass();
            let encoder = self.encoder();
            for level in 0..old.mips {
                let (ow, oh) = ((old.width >> level).max(1), (old.height >> level).max(1));
                for new in 0..mips {
                    if (width >> new).max(1) == ow && (height >> new).max(1) == oh && (new != level || old.width != width) {
                        encoder.copy_texture_to_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture: &old.texture,
                                mip_level: level,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: new, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                            wgpu::Extent3d {
                                width: ow,
                                height: oh,
                                depth_or_array_layers: if dimension == wgpu::TextureDimension::D3 { (old.layers >> level).max(1) } else { layers },
                            },
                        );
                        break;
                    }
                }
            }
        }
        let generation = self.generation;
        let serial = self.next_serial();
        if let Some(t) = self.texture_mut(id) {
            t.alloc = Some(Alloc { texture, format, width, height, layers, mips, samples: 1, serial, views: Default::default() });
            t.version += 1;
            t.used = generation;
        }
        true
    }

    pub(crate) fn clear_to_defaults(&mut self, texture: &wgpu::Texture, mips: u32, layers: u32) {
        let format = texture.format();
        if !format.is_depth_stencil_format() {
            return;
        }
        self.flush_pass();
        let encoder = self.encoder();
        for level in 0..mips {
            for layer in 0..layers {
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("acde webgl depth defaults"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("acde webgl depth defaults"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &view,
                        depth_ops: format.has_depth_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: format.has_stencil_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                }));
            }
        }
    }

    fn ensure_level(&mut self, id: Id, face: u32, level: u32, [width, height, depth]: [u32; 3], format: Format) -> bool {
        if level >= mip_count(self.limits.texture_size, self.limits.texture_size, 1) {
            self.error(INVALID_VALUE);
            return false;
        }
        let Some(t) = self.texture_mut(id) else { return false };
        t.images.insert((face, level), Image { width, height, depth, format });
        let is_3d = t.target == TEXTURE_3D;
        let layers = match t.target {
            TEXTURE_CUBE_MAP => 6,
            TEXTURE_2D_ARRAY | TEXTURE_3D => depth,
            _ => 1,
        };
        let fits = t.alloc.as_ref().is_some_and(|a| {
            a.format == format
                && level < a.mips
                && (a.width >> level).max(1) == width
                && (a.height >> level).max(1) == height
                && if is_3d { (a.layers >> level).max(1) == depth } else { a.layers == layers }
        });
        if fits {
            return true;
        }
        let (bw, bh, bd) = (width << level, height << level, if is_3d { depth << level } else { 1 });
        let base = t.images.get(&(face, t.base_level)).copied();
        let (w, h, d) = match base {
            Some(b) if b.format == format && t.base_level <= level => {
                let k = level - t.base_level;
                if (b.width >> k).max(1) == width && (b.height >> k).max(1) == height {
                    (b.width << t.base_level, b.height << t.base_level, if is_3d { b.depth << t.base_level } else { 1 })
                } else {
                    (bw, bh, bd)
                }
            }
            _ => (bw, bh, bd),
        };
        let max = self.limits.texture_size;
        if w > max * 2 || h > max * 2 {
            self.error(INVALID_VALUE);
            return false;
        }
        let mips = mip_count(w, h, d);
        let layers = if is_3d { d } else { layers };
        self.alloc(id, format, w.max(1), h.max(1), layers.max(1), mips)
    }

    #[allow(clippy::too_many_arguments, reason = "mirrors the GL entry point")]
    pub fn tex_image(
        &mut self,
        target: u32,
        level: i32,
        internal: u32,
        width: i32,
        height: i32,
        depth: i32,
        border: i32,
        format: u32,
        ty: u32,
        data: Option<&[u8]>,
    ) {
        let three = matches!(target, TEXTURE_3D | TEXTURE_2D_ARRAY);
        if three && !self.webgl2 {
            return self.error(INVALID_ENUM);
        }
        if level < 0 || level as u32 >= mip_count(self.limits.texture_size, self.limits.texture_size, 1) || width < 0 || height < 0 || depth < 0 || border != 0
        {
            return self.error(INVALID_VALUE);
        }
        let Some((id, face)) = self.bound_texture(target) else { return };
        if self.texture(id).is_some_and(|t| t.immutable) {
            return self.error(INVALID_OPERATION);
        }
        if target == TEXTURE_CUBE_MAP || (face_index(target).is_some() && width != height) {
            return self.error(if target == TEXTURE_CUBE_MAP { INVALID_ENUM } else { INVALID_VALUE });
        }
        let Some(fmt) = self.resolve_format(internal, format, ty) else { return };
        let (w, h, d) = (width as u32, height as u32, if three { depth as u32 } else { 1 });
        let max = if three { self.limits.texture_3d } else { self.limits.texture_size };
        if w > max >> level || h > max >> level {
            return self.error(INVALID_VALUE);
        }
        if (target == TEXTURE_2D_ARRAY && d > self.limits.array_layers) || (target == TEXTURE_3D && d > max >> level) {
            return self.error(INVALID_VALUE);
        }
        if !self.ensure_level(id, face, level as u32, [w, h, d], fmt) {
            return;
        }
        if w == 0 || h == 0 || d == 0 {
            return;
        }
        self.upload(id, face, level as u32, [0, 0, 0], [w, h, d], format, ty, data, &fmt, data.is_none());
    }

    #[allow(clippy::too_many_arguments, reason = "mirrors the GL entry point")]
    pub fn tex_sub_image(
        &mut self,
        target: u32,
        level: i32,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        height: i32,
        depth: i32,
        format: u32,
        ty: u32,
        data: Option<&[u8]>,
    ) {
        let three = matches!(target, TEXTURE_3D | TEXTURE_2D_ARRAY);
        if level < 0 || x < 0 || y < 0 || z < 0 || width < 0 || height < 0 || depth < 0 {
            return self.error(INVALID_VALUE);
        }
        let Some((id, face)) = self.bound_texture(target) else { return };
        let Some(image) = self.texture(id).and_then(|t| t.images.get(&(face, level as u32)).copied()) else { return self.error(INVALID_OPERATION) };
        let (x, y, z) = (x as u32, y as u32, if three { z as u32 } else { 0 });
        let (w, h, d) = (width as u32, height as u32, if three { depth as u32 } else { 1 });
        if x + w > image.width || y + h > image.height || z + d > image.depth {
            return self.error(INVALID_VALUE);
        }
        let fmt = image.format;
        if !format::valid_upload(fmt.internal, format, ty) && !(self.webgl2 && fmt.internal == RGBA && format == RGBA) {
            let unsized_ok = format::unsized_internal(format, ty).is_some_and(|u| format::sized(u).is_some_and(|f| f.wgpu == fmt.wgpu));
            if !unsized_ok {
                return self.error(INVALID_OPERATION);
            }
        }
        if w == 0 || h == 0 || d == 0 {
            return;
        }
        if data.is_none() {
            return self.error(INVALID_VALUE);
        }
        self.upload(id, face, level as u32, [x, y, z], [w, h, d], format, ty, data, &fmt, false);
    }

    pub(crate) fn resolve_format(&mut self, internal: u32, format: u32, ty: u32) -> Option<Format> {
        let float_ok = self.webgl2 || self.extension("OES_texture_float");
        let half_ok = self.webgl2 || self.extension("OES_texture_half_float");
        let depth_ok = self.webgl2 || self.extension("WEBGL_depth_texture");
        let srgb_ok = self.webgl2 || self.extension("EXT_sRGB");
        match ty {
            FLOAT if !float_ok => {
                self.error(INVALID_ENUM);
                return None;
            }
            HALF_FLOAT_OES if !half_ok || self.webgl2 => {
                self.error(INVALID_ENUM);
                return None;
            }
            _ => {}
        }
        if matches!(format, DEPTH_COMPONENT | DEPTH_STENCIL) && !depth_ok {
            self.error(INVALID_ENUM);
            return None;
        }
        if matches!(format, SRGB_EXT | SRGB_ALPHA_EXT) && !srgb_ok {
            self.error(INVALID_ENUM);
            return None;
        }
        let sized = if matches!(internal, RGBA | RGB | LUMINANCE | LUMINANCE_ALPHA | ALPHA | DEPTH_COMPONENT | DEPTH_STENCIL | SRGB_EXT | SRGB_ALPHA_EXT) {
            if internal != format {
                self.error(INVALID_OPERATION);
                return None;
            }
            match format::unsized_internal(format, ty) {
                Some(s) => s,
                None => {
                    self.error(if format::texel_bytes(format, ty).is_none() { INVALID_ENUM } else { INVALID_OPERATION });
                    return None;
                }
            }
        } else {
            if !self.webgl2 {
                self.error(INVALID_VALUE);
                return None;
            }
            if !format::valid_upload(internal, format, ty) {
                self.error(if format::sized(internal).is_none() { INVALID_VALUE } else { INVALID_OPERATION });
                return None;
            }
            internal
        };
        if !self.format_allowed(sized) {
            self.error(INVALID_ENUM);
            return None;
        }
        let mut f = format::sized(sized).or_else(|| {
            self.error(INVALID_VALUE);
            None
        })?;
        f.internal = if matches!(internal, RGBA | RGB | LUMINANCE | LUMINANCE_ALPHA | ALPHA | SRGB_EXT | SRGB_ALPHA_EXT)
            && matches!(ty, UNSIGNED_BYTE | UNSIGNED_SHORT_4_4_4_4 | UNSIGNED_SHORT_5_5_5_1 | UNSIGNED_SHORT_5_6_5)
        {
            internal
        } else {
            f.internal
        };
        if f.class == Class::Unfilterable && (self.extension("OES_texture_float_linear")) && self.features.contains(wgpu::Features::FLOAT32_FILTERABLE) {
            f.class = Class::Float;
            f.filterable = true;
        }
        Some(f)
    }

    #[allow(clippy::too_many_arguments, reason = "mirrors the GL entry point")]
    fn upload(&mut self, id: Id, face: u32, level: u32, origin: [u32; 3], size: [u32; 3], format: u32, ty: u32, data: Option<&[u8]>, fmt: &Format, zero: bool) {
        let Some(texel) = format::texel_bytes(format, ty) else { return self.error(INVALID_ENUM) };
        let unpack = Unpack {
            alignment: self.state.unpack_alignment,
            row_length: self.state.unpack_row_length,
            image_height: self.state.unpack_image_height,
            skip_pixels: self.state.unpack_skip_pixels,
            skip_rows: self.state.unpack_skip_rows,
            skip_images: self.state.unpack_skip_images,
        };
        let [w, h, d] = size;
        let out_texel = format::wgpu_texel_bytes(fmt.wgpu);
        let row_out = (w * out_texel) as usize;
        let mut out = vec![0u8; row_out * (h * d) as usize];
        if let Some(data) = data.filter(|_| !zero) {
            let (start, row, image, needed) = unpack.layout(w, h, d, texel);
            if (data.len() as u64) < needed {
                return self.error(INVALID_OPERATION);
            }
            let passthrough = format::passthrough(format, ty, fmt);
            let flip = self.state.unpack_flip_y;
            let premultiply = self.state.unpack_premultiply && fmt.wgpu == wgpu::TextureFormat::Rgba8Unorm && ty == UNSIGNED_BYTE && format == RGBA;
            for z in 0..d {
                for y in 0..h {
                    let src_y = if flip { h - 1 - y } else { y };
                    let at = (start + u64::from(z) * image + u64::from(src_y) * row) as usize;
                    let src = &data[at..at + (w * texel) as usize];
                    let dst = &mut out[((z * h + y) as usize) * row_out..((z * h + y) as usize + 1) * row_out];
                    if passthrough {
                        dst.copy_from_slice(src);
                    } else {
                        dst.copy_from_slice(&format::convert(src, format, ty, fmt, w));
                    }
                    if premultiply {
                        for px in dst.as_chunks_mut::<4>().0 {
                            let a = u16::from(px[3]);
                            for c in &mut px[..3] {
                                *c = ((u16::from(*c) * a + 127) / 255) as u8;
                            }
                        }
                    }
                }
            }
        }
        self.write_texels(id, (face, level), origin, size, &out, fmt);
    }

    pub(crate) fn write_texels(&mut self, id: Id, (face, level): (u32, u32), origin: [u32; 3], size: [u32; 3], tight: &[u8], fmt: &Format) {
        let generation = self.generation;
        let Some(t) = self.texture_mut(id) else { return };
        let Some(alloc) = t.alloc.as_ref() else { return };
        let texture = alloc.texture.clone();
        let is_3d = t.target == TEXTURE_3D;
        let ordered = t.used == generation;
        let in_flight = t.used + 1 == generation;
        t.version += 1;
        let [x, y, z] = origin;
        let [w, h, d] = size;
        let layer = if t.target == TEXTURE_CUBE_MAP { face } else { z };
        if matches!(
            fmt.wgpu,
            wgpu::TextureFormat::Depth24Plus
                | wgpu::TextureFormat::Depth24PlusStencil8
                | wgpu::TextureFormat::Depth32Float
                | wgpu::TextureFormat::Depth32FloatStencil8
        ) {
            if tight.iter().any(|&b| b != 0) {
                log::warn!("webgl: uploading data into a {:?} texture is not supported; it stays cleared", fmt.wgpu);
            }
            return;
        }
        let texel = format::wgpu_texel_bytes(fmt.wgpu);
        let (bw, bh) = fmt.wgpu.block_dimensions();
        let row = w.div_ceil(bw) * texel;
        let rows = h.div_ceil(bh);
        let destination = wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: level,
            origin: wgpu::Origin3d { x, y, z: if is_3d { z } else { layer } },
            aspect: wgpu::TextureAspect::All,
        };
        let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: d };
        if ordered {
            let aligned = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let mut staged = vec![0u8; (aligned * rows * d) as usize];
            for r in 0..(rows * d) as usize {
                staged[r * aligned as usize..r * aligned as usize + row as usize].copy_from_slice(&tight[r * row as usize..(r + 1) * row as usize]);
            }
            let device = self.gpu.device.clone();
            let staging = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("acde webgl upload"),
                contents: &staged,
                usage: wgpu::BufferUsages::COPY_SRC,
            });
            self.flush_pass();
            self.encoder().copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(aligned), rows_per_image: Some(rows) },
                },
                destination,
                extent,
            );
        } else {
            if in_flight {
                self.settle();
            }
            self.gpu.queue.write_texture(
                destination,
                tight,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(rows) },
                extent,
            );
        }
    }

    pub fn tex_storage(&mut self, target: u32, levels: i32, internal: u32, width: i32, height: i32, depth: i32) {
        if !self.webgl2 {
            return self.error(INVALID_OPERATION);
        }
        if !matches!(target, TEXTURE_2D | TEXTURE_CUBE_MAP | TEXTURE_3D | TEXTURE_2D_ARRAY) {
            return self.error(INVALID_ENUM);
        }
        if levels < 1 || width < 1 || height < 1 || depth < 1 {
            return self.error(INVALID_VALUE);
        }
        let Some(slot) = self.unit_slot(target) else { return self.error(INVALID_ENUM) };
        let Some(id) = *slot else { return self.error(INVALID_OPERATION) };
        if self.texture(id).is_some_and(|t| t.immutable) {
            return self.error(INVALID_OPERATION);
        }
        let (w, h, d) = (width as u32, height as u32, if matches!(target, TEXTURE_3D | TEXTURE_2D_ARRAY) { depth as u32 } else { 1 });
        if target == TEXTURE_CUBE_MAP && w != h {
            return self.error(INVALID_VALUE);
        }
        let fmt = match format::sized(internal) {
            Some(_) if !self.format_allowed(internal) => return self.error(INVALID_ENUM),
            Some(f) if !matches!(internal, RGBA | RGB | LUMINANCE | LUMINANCE_ALPHA | ALPHA | DEPTH_COMPONENT | DEPTH_STENCIL) => f,
            _ => match format::compressed(internal) {
                Some((wgpu_format, feature)) if self.features.contains(feature) => format::compressed_format(internal, wgpu_format),
                _ => return self.error(INVALID_ENUM),
            },
        };
        let mut fmt = fmt;
        if fmt.class == Class::Unfilterable && self.extension("OES_texture_float_linear") && self.features.contains(wgpu::Features::FLOAT32_FILTERABLE) {
            fmt.class = Class::Float;
            fmt.filterable = true;
        }
        if levels as u32 > mip_count(w, h, if target == TEXTURE_3D { d } else { 1 }) {
            return self.error(INVALID_OPERATION);
        }
        let layers = match target {
            TEXTURE_CUBE_MAP => 6,
            _ => d,
        };
        if !self.alloc(id, fmt, w, h, layers, levels as u32) {
            return;
        }
        let faces = if target == TEXTURE_CUBE_MAP { 6 } else { 1 };
        let Some(t) = self.texture_mut(id) else { return };
        t.immutable = true;
        t.immutable_levels = levels as u32;
        for level in 0..levels as u32 {
            for face in 0..faces {
                t.images.insert(
                    (face, level),
                    Image {
                        width: (w >> level).max(1),
                        height: (h >> level).max(1),
                        depth: if target == TEXTURE_3D { (d >> level).max(1) } else { d },
                        format: fmt,
                    },
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments, reason = "mirrors the GL entry point")]
    pub fn compressed_tex_image(&mut self, target: u32, level: i32, internal: u32, width: i32, height: i32, depth: i32, data: &[u8], sub: Option<[i32; 3]>) {
        if level < 0 || width < 0 || height < 0 || depth < 0 {
            return self.error(INVALID_VALUE);
        }
        let Some((wgpu_format, feature)) = format::compressed(internal).filter(|(_, f)| self.features.contains(*f)) else { return self.error(INVALID_ENUM) };
        let Some((id, face)) = self.bound_texture(target) else { return };
        let fmt = format::compressed_format(internal, wgpu_format);
        let _ = feature;
        let (w, h, d) = (width as u32, height as u32, depth.max(1) as u32);
        let (bw, bh) = wgpu_format.block_dimensions();
        let block = wgpu_format.block_copy_size(None).unwrap_or(16);
        let expected = w.div_ceil(bw) * h.div_ceil(bh) * block * d;
        if data.len() as u32 != expected {
            return self.error(INVALID_VALUE);
        }
        let origin = match sub {
            Some([x, y, z]) => {
                let Some(image) = self.texture(id).and_then(|t| t.images.get(&(face, level as u32)).copied()) else { return self.error(INVALID_OPERATION) };
                if image.format.internal != internal || !(x as u32).is_multiple_of(bw) || !(y as u32).is_multiple_of(bh) {
                    return self.error(INVALID_OPERATION);
                }
                [x as u32, y as u32, z as u32]
            }
            None => {
                if self.texture(id).is_some_and(|t| t.immutable) {
                    return self.error(INVALID_OPERATION);
                }
                if !self.ensure_level(id, face, level as u32, [w, h, d], fmt) {
                    return;
                }
                [0, 0, 0]
            }
        };
        if w == 0 || h == 0 {
            return;
        }
        let Some(alloc) = self.texture(id).and_then(|t| t.alloc.as_ref()) else { return };
        let (lw, lh) = ((alloc.width >> level).max(1), (alloc.height >> level).max(1));
        if lw.next_multiple_of(bw) < origin[0] + w || lh.next_multiple_of(bh) < origin[1] + h {
            return self.error(INVALID_VALUE);
        }
        let (pw, ph) = (w.next_multiple_of(bw).min(lw.next_multiple_of(bw) - origin[0]), h.next_multiple_of(bh).min(lh.next_multiple_of(bh) - origin[1]));
        self.write_texels(id, (face, level as u32), origin, [pw.max(w), ph.max(h), d], data, &fmt);
    }

    #[allow(clippy::too_many_arguments, reason = "mirrors the GL entry point")]
    pub fn tex_image_source(
        &mut self,
        target: u32,
        level: i32,
        internal: u32,
        format: u32,
        ty: u32,
        source: TexSource,
        sub: Option<[i32; 3]>,
        bitmap: Option<bool>,
    ) {
        if let Some(premultiplied) = bitmap {
            let saved = (self.state.unpack_flip_y, self.state.unpack_premultiply);
            (self.state.unpack_flip_y, self.state.unpack_premultiply) = (false, premultiplied);
            self.tex_image_source(target, level, internal, format, ty, source, sub, None);
            (self.state.unpack_flip_y, self.state.unpack_premultiply) = saved;
            return;
        }
        let (width, height) = match &source {
            TexSource::Cpu { width, height, .. } => (*width, *height),
            TexSource::Gpu(image) => (image.texture.width(), image.texture.height()),
        };
        let gpu_ok = matches!(source, TexSource::Gpu(_)) && format == RGBA && matches!(ty, UNSIGNED_BYTE | HALF_FLOAT | FLOAT | HALF_FLOAT_OES);
        let rgba = match source {
            TexSource::Cpu { rgba, .. } => rgba,
            TexSource::Gpu(image) if gpu_ok => return self.tex_image_gpu(target, level, internal, format, ty, &image, sub),
            TexSource::Gpu(image) => {
                self.flush_pass();
                match self.gpu.read(&image.texture, [0, 0, width, height]) {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!("webgl: reading a canvas for texImage: {e}");
                        return self.error(INVALID_VALUE);
                    }
                }
            }
        };
        let data = from_rgba(&rgba, width, height, format, ty, self.state.unpack_premultiply);
        let Some(data) = data else { return self.error(INVALID_OPERATION) };
        let saved = (
            self.state.unpack_alignment,
            self.state.unpack_row_length,
            self.state.unpack_skip_pixels,
            self.state.unpack_skip_rows,
            self.state.unpack_premultiply,
        );
        self.state.unpack_alignment = 1;
        self.state.unpack_row_length = 0;
        self.state.unpack_skip_pixels = 0;
        self.state.unpack_skip_rows = 0;
        self.state.unpack_premultiply = false;
        match sub {
            Some([x, y, z]) => self.tex_sub_image(target, level, x, y, z, width as i32, height as i32, 1, format, ty, Some(&data)),
            None => {
                self.tex_image(target, level, internal, width as i32, height as i32, 1, 0, format, ty, Some(&data));
            }
        }
        (
            self.state.unpack_alignment,
            self.state.unpack_row_length,
            self.state.unpack_skip_pixels,
            self.state.unpack_skip_rows,
            self.state.unpack_premultiply,
        ) = saved;
    }

    #[allow(clippy::too_many_arguments, reason = "mirrors the GL entry point")]
    fn tex_image_gpu(&mut self, target: u32, level: i32, internal: u32, format: u32, ty: u32, image: &Arc<crate::gpu::GpuImage>, sub: Option<[i32; 3]>) {
        let (w, h) = (image.texture.width(), image.texture.height());
        let Some((id, face)) = self.bound_texture(target) else { return };
        let (fmt, origin) = match sub {
            Some([x, y, z]) => {
                let Some(img) = self.texture(id).and_then(|t| t.images.get(&(face, level as u32)).copied()) else { return self.error(INVALID_OPERATION) };
                if x < 0 || y < 0 || x as u32 + w > img.width || y as u32 + h > img.height {
                    return self.error(INVALID_VALUE);
                }
                (img.format, [x as u32, y as u32, z.max(0) as u32])
            }
            None => {
                if self.texture(id).is_some_and(|t| t.immutable) {
                    return self.error(INVALID_OPERATION);
                }
                let Some(fmt) = self.resolve_format(internal, format, ty) else { return };
                if !self.ensure_level(id, face, level.max(0) as u32, [w, h, 1], fmt) {
                    return;
                }
                (fmt, [0, 0, 0])
            }
        };
        if !(fmt.renderable || fmt.expand == Expand::Rgb) || fmt.class != Class::Float && fmt.class != Class::Unfilterable {
            let pixels = match self.gpu.read(&image.texture, [0, 0, w, h]) {
                Ok(p) => p,
                Err(_) => return self.error(INVALID_VALUE),
            };
            return self.tex_image_source(target, level, internal, format, ty, TexSource::Cpu { width: w, height: h, rgba: pixels }, sub, None);
        }
        let Some(texture) = self.texture(id).and_then(|t| t.alloc.as_ref()).map(|a| a.texture.clone()) else { return };
        let layer = if self.texture(id).is_some_and(|t| t.target == TEXTURE_CUBE_MAP) { face } else { origin[2] };
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("acde webgl upload"),
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_mip_level: level as u32,
            mip_level_count: Some(1),
            base_array_layer: layer,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let source = image.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let flip = self.state.unpack_flip_y;
        let premultiply = self.state.unpack_premultiply;
        let viewport = [origin[0] as f32, origin[1] as f32, w as f32, h as f32];
        let decode = fmt.wgpu.is_srgb() && !image.texture.format().is_srgb();
        self.blit(&source, [0.0, 0.0, 1.0, 1.0], &view, fmt.wgpu, viewport, flip, premultiply, false, decode, wgpu::FilterMode::Nearest, 0.0);
        self.mark_used(&[id]);
        if let Some(t) = self.texture_mut(id) {
            t.version += 1;
        }
    }

    #[allow(clippy::too_many_arguments, reason = "mirrors the GL entry point")]
    pub(crate) fn blit(
        &mut self,
        source: &wgpu::TextureView,
        from: [f32; 4],
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        viewport: [f32; 4],
        flip: bool,
        premultiply: bool,
        unpremultiply: bool,
        decode: bool,
        filter: wgpu::FilterMode,
        lod: f32,
    ) {
        let name = format!("blit {format:?}");
        let pipeline = self.helper(&name, |device| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("acde webgl blit"),
                source: wgpu::ShaderSource::Wgsl(crate::webgl::helpers::BLIT.into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("acde webgl blit"),
                layout: None,
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        let Some(pipeline) = pipeline else { return };
        let device = self.gpu.device.clone();
        let mut params = Vec::with_capacity(32);
        for v in from {
            params.extend_from_slice(&v.to_le_bytes());
        }
        for v in [u32::from(flip), u32::from(premultiply), u32::from(unpremultiply)] {
            params.extend_from_slice(&v.to_le_bytes());
        }
        params.extend_from_slice(&lod.to_le_bytes());
        for v in [u32::from(decode), 0, 0, 0] {
            params.extend_from_slice(&v.to_le_bytes());
        }
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("acde webgl blit"),
            contents: &params,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let sampler = self.caches.sampler(
            &device,
            crate::webgl::pipeline::SamplerKey {
                address: [wgpu::AddressMode::ClampToEdge; 3],
                mag: filter,
                min: filter,
                mip: wgpu::MipmapFilterMode::Nearest,
                lod: (0, 32 * 256),
                compare: None,
                anisotropy: 1,
            },
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("acde webgl blit"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
            ],
        });
        self.flush_pass();
        let encoder = self.encoder();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("acde webgl blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        let [x, y, w, h] = viewport;
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        pass.set_scissor_rect(x.max(0.0) as u32, y.max(0.0) as u32, w as u32, h as u32);
        pass.draw(0..3, 0..1);
    }

    pub fn generate_mipmap(&mut self, target: u32) {
        let Some(slot) = self.unit_slot(target) else { return self.error(INVALID_ENUM) };
        let Some(id) = *slot else { return self.error(INVALID_OPERATION) };
        let webgl2 = self.webgl2;
        let Some(t) = self.texture(id) else { return };
        let Some(base) = t.base() else { return self.error(INVALID_OPERATION) };
        let pot = base.width.is_power_of_two() && base.height.is_power_of_two();
        if !webgl2 && !pot {
            return self.error(INVALID_OPERATION);
        }
        if !base.format.filterable
            || !(base.format.renderable || base.format.expand != Expand::None)
            || base.format.wgpu.is_compressed()
            || base.format.wgpu.is_depth_stencil_format()
        {
            return self.error(INVALID_OPERATION);
        }
        if t.target == TEXTURE_CUBE_MAP && (0..6).any(|f| t.images.get(&(f, t.base_level)).is_none_or(|i| i.width != base.width || i.format != base.format)) {
            return self.error(INVALID_OPERATION);
        }
        let base_level = t.base_level;
        let faces = t.faces();
        let target_kind = t.target;
        let Some(alloc) = t.alloc.as_ref() else { return };
        let texture = alloc.texture.clone();
        let mips = alloc.mips;
        let layers = if target_kind == TEXTURE_2D_ARRAY { alloc.layers } else { faces };
        let format = alloc.format;
        let last = (base_level + mip_count(base.width, base.height, 1) - 1).min(mips - 1);
        if target_kind == TEXTURE_3D {
            log::warn!("webgl: generateMipmap on a 3D texture only fills the levels' records");
        } else {
            for level in base_level + 1..=last {
                for layer in 0..layers {
                    let view = |mip| {
                        texture.create_view(&wgpu::TextureViewDescriptor {
                            label: Some("acde webgl mipmap"),
                            dimension: Some(wgpu::TextureViewDimension::D2),
                            base_mip_level: mip,
                            mip_level_count: Some(1),
                            base_array_layer: layer,
                            array_layer_count: Some(1),
                            ..Default::default()
                        })
                    };
                    let (src, dst) = (view(level - 1), view(level));
                    let (w, h) = ((base.width >> (level - base_level)).max(1), (base.height >> (level - base_level)).max(1));
                    self.blit(
                        &src,
                        [0.0, 0.0, 1.0, 1.0],
                        &dst,
                        format.wgpu,
                        [0.0, 0.0, w as f32, h as f32],
                        false,
                        false,
                        false,
                        false,
                        wgpu::FilterMode::Linear,
                        0.0,
                    );
                }
            }
        }
        self.mark_used(&[id]);
        let Some(t) = self.texture_mut(id) else { return };
        for level in base_level + 1..=last {
            for face in 0..faces {
                let k = level - base_level;
                t.images.insert(
                    (face, level),
                    Image {
                        width: (base.width >> k).max(1),
                        height: (base.height >> k).max(1),
                        depth: if target_kind == TEXTURE_3D { (base.depth >> k).max(1) } else { base.depth },
                        format: base.format,
                    },
                );
            }
        }
        t.version += 1;
    }

    pub fn create_sampler(&mut self) -> Id {
        self.add(Object::Sampler(Sampler { params: SamplerParams::default() }))
    }

    pub fn delete_sampler(&mut self, id: Id) {
        for unit in &mut self.state.units {
            if unit.sampler == Some(id) {
                unit.sampler = None;
            }
        }
        self.objects.remove(&id);
    }

    pub fn bind_sampler(&mut self, unit: u32, id: Option<Id>) {
        if unit as usize >= self.state.units.len() {
            return self.error(INVALID_VALUE);
        }
        if id.is_some_and(|id| self.sampler_object(id).is_none()) {
            return self.error(INVALID_OPERATION);
        }
        self.state.units[unit as usize].sampler = id;
    }

    pub fn sampler_parameter(&mut self, id: Id, name: u32, value: f32) {
        let anisotropy = self.extension("EXT_texture_filter_anisotropic");
        let Some(s) = self.objects.get_mut(&id).and_then(Object::sampler_mut) else { return self.error(INVALID_OPERATION) };
        let mut params = s.params;
        match set_sampler_param(&mut params, name, value, true, anisotropy) {
            Ok(()) => s.params = params,
            Err(code) => self.error(code),
        }
    }

    pub fn get_sampler_parameter(&mut self, id: Id, name: u32) -> Option<f32> {
        let s = self.objects.get(&id).and_then(Object::sampler)?;
        let v = get_sampler_param(&s.params, name, true);
        if v.is_none() {
            self.error(INVALID_ENUM);
        }
        v
    }
}

fn set_sampler_param(p: &mut SamplerParams, name: u32, value: f32, webgl2: bool, anisotropy: bool) -> Result<(), u32> {
    let e = value as u32;
    let wrap = |e| matches!(e, REPEAT | CLAMP_TO_EDGE | MIRRORED_REPEAT);
    match name {
        TEXTURE_MIN_FILTER if matches!(e, NEAREST | LINEAR | NEAREST_MIPMAP_NEAREST | LINEAR_MIPMAP_NEAREST | NEAREST_MIPMAP_LINEAR | LINEAR_MIPMAP_LINEAR) => {
            p.min = e
        }
        TEXTURE_MAG_FILTER if matches!(e, NEAREST | LINEAR) => p.mag = e,
        TEXTURE_WRAP_S if wrap(e) => p.wrap[0] = e,
        TEXTURE_WRAP_T if wrap(e) => p.wrap[1] = e,
        TEXTURE_WRAP_R if webgl2 && wrap(e) => p.wrap[2] = e,
        TEXTURE_MIN_LOD if webgl2 => p.min_lod = value,
        TEXTURE_MAX_LOD if webgl2 => p.max_lod = value,
        TEXTURE_COMPARE_MODE if webgl2 && matches!(e, NONE | COMPARE_REF_TO_TEXTURE) => p.compare_mode = e,
        TEXTURE_COMPARE_FUNC if webgl2 && (NEVER..=ALWAYS).contains(&e) => p.compare_func = e,
        TEXTURE_MAX_ANISOTROPY_EXT if anisotropy => {
            if value < 1.0 {
                return Err(INVALID_VALUE);
            }
            p.anisotropy = value.min(16.0)
        }
        TEXTURE_MIN_FILTER | TEXTURE_MAG_FILTER | TEXTURE_WRAP_S | TEXTURE_WRAP_T | TEXTURE_WRAP_R | TEXTURE_COMPARE_MODE | TEXTURE_COMPARE_FUNC => {
            return Err(INVALID_ENUM);
        }
        _ => return Err(INVALID_ENUM),
    }
    Ok(())
}

fn get_sampler_param(p: &SamplerParams, name: u32, webgl2: bool) -> Option<f32> {
    Some(match name {
        TEXTURE_MIN_FILTER => p.min as f32,
        TEXTURE_MAG_FILTER => p.mag as f32,
        TEXTURE_WRAP_S => p.wrap[0] as f32,
        TEXTURE_WRAP_T => p.wrap[1] as f32,
        TEXTURE_WRAP_R if webgl2 => p.wrap[2] as f32,
        TEXTURE_MIN_LOD if webgl2 => p.min_lod,
        TEXTURE_MAX_LOD if webgl2 => p.max_lod,
        TEXTURE_COMPARE_MODE if webgl2 => p.compare_mode as f32,
        TEXTURE_COMPARE_FUNC if webgl2 => p.compare_func as f32,
        TEXTURE_MAX_ANISOTROPY_EXT => p.anisotropy,
        _ => return None,
    })
}

fn from_rgba(rgba: &[u8], width: u32, height: u32, format: u32, ty: u32, premultiply: bool) -> Option<Vec<u8>> {
    let n = (width * height) as usize;
    let comps = format::components(format)? as usize;
    let mut out = Vec::with_capacity(n * 16);
    for i in 0..n {
        let mut p = [rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2], rgba[i * 4 + 3]];
        if premultiply {
            let a = u16::from(p[3]);
            for c in &mut p[..3] {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
        let channels: [u8; 4] = match format {
            ALPHA => [p[3], 0, 0, 0],
            LUMINANCE | RED | RED_INTEGER => [p[0], 0, 0, 0],
            LUMINANCE_ALPHA => [p[0], p[3], 0, 0],
            _ => p,
        };
        match ty {
            UNSIGNED_BYTE => out.extend_from_slice(&channels[..comps]),
            FLOAT => channels[..comps].iter().for_each(|&c| out.extend_from_slice(&(f32::from(c) / 255.0).to_le_bytes())),
            HALF_FLOAT | HALF_FLOAT_OES => {
                channels[..comps].iter().for_each(|&c| out.extend_from_slice(&format::f32_to_half(f32::from(c) / 255.0).to_le_bytes()))
            }
            UNSIGNED_SHORT_5_6_5 => out.extend_from_slice(&((u16::from(p[0] >> 3) << 11) | (u16::from(p[1] >> 2) << 5) | u16::from(p[2] >> 3)).to_le_bytes()),
            UNSIGNED_SHORT_4_4_4_4 => out.extend_from_slice(
                &((u16::from(p[0] >> 4) << 12) | (u16::from(p[1] >> 4) << 8) | (u16::from(p[2] >> 4) << 4) | u16::from(p[3] >> 4)).to_le_bytes(),
            ),
            UNSIGNED_SHORT_5_5_5_1 => out.extend_from_slice(
                &((u16::from(p[0] >> 3) << 11) | (u16::from(p[1] >> 3) << 6) | (u16::from(p[2] >> 3) << 1) | u16::from(p[3] >> 7)).to_le_bytes(),
            ),
            UNSIGNED_INT_2_10_10_10_REV => {
                let c = |v: u8| u32::from(v) * 1023 / 255;
                out.extend_from_slice(&(c(p[0]) | (c(p[1]) << 10) | (c(p[2]) << 20) | ((u32::from(p[3]) * 3 / 255) << 30)).to_le_bytes())
            }
            _ => return None,
        }
    }
    Some(out)
}
