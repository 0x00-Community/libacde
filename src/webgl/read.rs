use std::sync::Arc;

use crate::gpu::GpuImage;

use crate::webgl::consts::*;
use crate::webgl::context::Context;
use crate::webgl::format::{self, Class};
use crate::webgl::framebuffer::View;

const MAX_IMAGES: usize = 4;

pub(crate) fn map(gpu: &crate::gpu::Gpu, buffer: &wgpu::Buffer, index: wgpu::SubmissionIndex) -> Option<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, move |r| _ = tx.send(r));
    gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: Some(std::time::Duration::from_secs(10)) }).ok()?;
    rx.recv_timeout(std::time::Duration::from_secs(10)).ok()?.ok()?;
    let data = slice.get_mapped_range().ok()?.to_vec();
    buffer.unmap();
    Some(data)
}

impl Context {
    pub fn implementation_read_format(&mut self) -> (u32, u32) {
        let Ok(view) = self.read_view() else { return (RGBA, UNSIGNED_BYTE) };
        match view.format.class {
            Class::Sint => (RGBA_INTEGER, INT),
            Class::Uint => (RGBA_INTEGER, UNSIGNED_INT),
            _ if matches!(view.format.component_type, FLOAT) => (RGBA, FLOAT),
            _ => (RGBA, UNSIGNED_BYTE),
        }
    }

    #[allow(clippy::too_many_arguments, reason = "mirrors the GL entry point")]
    pub fn read_pixels(&mut self, x: i32, y: i32, width: i32, height: i32, format: u32, ty: u32, out: &mut [u8]) {
        if width < 0 || height < 0 {
            return self.error(INVALID_VALUE);
        }
        let view = match self.read_view() {
            Ok(v) => v,
            Err(e) => return self.error(e),
        };
        if view.samples > 1 {
            return self.error(INVALID_OPERATION);
        }
        let class = view.format.class;
        let float = matches!(view.format.component_type, FLOAT);
        let allowed = match (format, ty) {
            (RGBA, UNSIGNED_BYTE) => matches!(class, Class::Float | Class::Unfilterable) && !float || !self.webgl2,
            (RGBA, FLOAT) => float,
            (RGBA_INTEGER, INT) => class == Class::Sint,
            (RGBA_INTEGER, UNSIGNED_INT) => class == Class::Uint,
            _ => {
                let (f, t) = self.implementation_read_format();
                f == format && t == ty
            }
        };
        if !allowed {
            let known = format::texel_bytes(format, ty).is_some();
            return self.error(if known { INVALID_OPERATION } else { INVALID_ENUM });
        }
        let texel = format::texel_bytes(format, ty).unwrap_or(4) as usize;
        let alignment = self.state.pack_alignment as usize;
        let row_texels = if self.state.pack_row_length > 0 { self.state.pack_row_length as usize } else { width as usize };
        let row = (row_texels * texel).next_multiple_of(alignment);
        let skip = self.state.pack_skip_rows as usize * row + self.state.pack_skip_pixels as usize * texel;
        if width == 0 || height == 0 {
            return;
        }
        let needed = skip + (height as usize - 1) * row + width as usize * texel;
        if out.len() < needed {
            return self.error(INVALID_OPERATION);
        }
        let (fw, fh) = (view.width as i64, view.height as i64);
        let (x0, y0) = (i64::from(x).clamp(0, fw), i64::from(y).clamp(0, fh));
        let (x1, y1) = ((i64::from(x) + i64::from(width)).clamp(0, fw), (i64::from(y) + i64::from(height)).clamp(0, fh));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let (cw, ch) = ((x1 - x0) as u32, (y1 - y0) as u32);
        let src_texel = format::wgpu_texel_bytes(view.format.wgpu);
        let stride = (cw * src_texel).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let device = self.gpu.device.clone();
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acde webgl readPixels"),
            size: u64::from(stride * ch),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        self.flush_pass();
        self.encoder().copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &view.raw,
                mip_level: view.level,
                origin: wgpu::Origin3d { x: x0 as u32, y: y0 as u32, z: view.layer },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(ch) },
            },
            wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
        );
        let Some(index) = self.submit() else { return };
        let Some(data) = map(&self.gpu, &staging, index) else { return self.error(OUT_OF_MEMORY) };
        let opaque = self.state.read_framebuffer.is_none() && !self.attrs.alpha;
        let fmt = view.format.wgpu;
        for r in 0..ch as usize {
            let src = &data[r * stride as usize..r * stride as usize + (cw * src_texel) as usize];
            let dst_row = skip + (r + (y0 - i64::from(y)) as usize) * row + (x0 - i64::from(x)) as usize * texel;
            for c in 0..cw as usize {
                let px = &src[c * src_texel as usize..(c + 1) * src_texel as usize];
                let rgba = texel_rgba(fmt, px);
                let dst = &mut out[dst_row + c * texel..dst_row + (c + 1) * texel];
                write_texel(dst, rgba, ty, opaque);
            }
        }
    }

    pub fn copy_tex_image(&mut self, target: u32, level: i32, internal: Option<u32>, dst: [i32; 3], src: [i32; 4]) {
        let [x, y, w, h] = src;
        if level < 0 || w < 0 || h < 0 {
            return self.error(INVALID_VALUE);
        }
        let view = match self.read_view() {
            Ok(v) => v,
            Err(e) => return self.error(e),
        };
        if view.samples > 1 {
            return self.error(INVALID_OPERATION);
        }
        if let Some(internal) = internal {
            let format = match internal {
                RGB | RGBA | LUMINANCE | LUMINANCE_ALPHA | ALPHA => internal,
                i => match format::sized(i) {
                    Some(_) if self.webgl2 => i,
                    _ => return self.error(INVALID_ENUM),
                },
            };
            let (fmt, ty) = if matches!(format, RGB | RGBA | LUMINANCE | LUMINANCE_ALPHA | ALPHA) { (format, UNSIGNED_BYTE) } else { (RGBA, UNSIGNED_BYTE) };
            let saved = self.state.pixel_unpack.take();
            if matches!(format, RGB | RGBA | LUMINANCE | LUMINANCE_ALPHA | ALPHA) {
                self.tex_image(target, level, format, w, h, 1, 0, fmt, ty, None);
            } else {
                let Some(f) = format::sized(format) else { return };
                let Some((id, face)) = self.bound_texture(target) else { return };
                let _ = (id, face, f);
                self.tex_storage_level(target, level, format, w, h);
            }
            self.state.pixel_unpack = saved;
        }
        let Some((id, face)) = self.bound_texture(target) else { return };
        let Some(t) = self.texture(id) else { return };
        let layer = if t.target == TEXTURE_CUBE_MAP { face } else { dst[2].max(0) as u32 };
        let Some(image) = t.images.get(&(face, level as u32)).copied() else { return self.error(INVALID_OPERATION) };
        let Some(alloc) = t.alloc.as_ref() else { return };
        let texture = alloc.texture.clone();
        let (fw, fh) = (i64::from(view.width), i64::from(view.height));
        let (sx0, sy0) = (i64::from(x).max(0), i64::from(y).max(0));
        let (sx1, sy1) = ((i64::from(x) + i64::from(w)).min(fw), (i64::from(y) + i64::from(h)).min(fh));
        if dst[0] < 0 || dst[1] < 0 || dst[0] as u32 + w as u32 > image.width || dst[1] as u32 + h as u32 > image.height {
            return self.error(INVALID_VALUE);
        }
        if sx1 <= sx0 || sy1 <= sy0 {
            return;
        }
        let (dx, dy) = (dst[0] as i64 + sx0 - i64::from(x), dst[1] as i64 + sy0 - i64::from(y));
        let (cw, ch) = ((sx1 - sx0) as u32, (sy1 - sy0) as u32);
        let generation = self.generation;
        if let Some(t) = self.texture_mut(id) {
            t.used = generation;
            t.version += 1;
        }
        self.flush_pass();
        if image.format.wgpu == view.format.wgpu {
            self.encoder().copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &view.raw,
                    mip_level: view.level,
                    origin: wgpu::Origin3d { x: sx0 as u32, y: sy0 as u32, z: view.layer },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d { x: dx as u32, y: dy as u32, z: layer },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
            );
        } else {
            let target_view = texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("acde webgl copyTexImage"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_mip_level: level as u32,
                mip_level_count: Some(1),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            });
            let from = [sx0 as f32 / view.width as f32, sy0 as f32 / view.height as f32, cw as f32 / view.width as f32, ch as f32 / view.height as f32];
            self.blit(
                &view.view,
                from,
                &target_view,
                image.format.wgpu,
                [dx as f32, dy as f32, cw as f32, ch as f32],
                false,
                false,
                false,
                false,
                wgpu::FilterMode::Nearest,
                0.0,
            );
        }
    }

    fn tex_storage_level(&mut self, target: u32, level: i32, internal: u32, w: i32, h: i32) {
        let Some(fmt) = format::sized(internal) else { return self.error(INVALID_ENUM) };
        let ty = match fmt.component_type {
            FLOAT => FLOAT,
            _ => UNSIGNED_BYTE,
        };
        let format = match format::components(match internal {
            R8 | R16F | R32F => RED,
            RG8 | RG16F | RG32F => RG,
            RGB8 | RGB16F | RGB32F | SRGB8 => RGB,
            _ => RGBA,
        }) {
            Some(1) => RED,
            Some(2) => RG,
            Some(3) => RGB,
            _ => RGBA,
        };
        self.tex_image(
            target,
            level,
            internal,
            w,
            h,
            1,
            0,
            format,
            if fmt.component_type == FLOAT
                && matches!(fmt.wgpu, wgpu::TextureFormat::Rgba16Float | wgpu::TextureFormat::R16Float | wgpu::TextureFormat::Rg16Float)
            {
                HALF_FLOAT
            } else {
                ty
            },
            None,
        );
    }

    #[allow(clippy::too_many_arguments, reason = "mirrors the GL entry point")]
    pub fn blit_framebuffer(&mut self, src: [i32; 4], dst: [i32; 4], mask: u32, filter: u32) {
        if mask & !(COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT | STENCIL_BUFFER_BIT) != 0 {
            return self.error(INVALID_VALUE);
        }
        if !matches!(filter, NEAREST | LINEAR) {
            return self.error(INVALID_ENUM);
        }
        if mask & (DEPTH_BUFFER_BIT | STENCIL_BUFFER_BIT) != 0 && filter != NEAREST {
            return self.error(INVALID_OPERATION);
        }
        let target = match self.draw_target() {
            Ok(t) => t,
            Err(e) => return self.error(e),
        };
        self.mark_used(&target.textures);
        let read_fb = self.state.read_framebuffer;
        if mask & COLOR_BUFFER_BIT != 0 {
            let source = match read_fb {
                None => {
                    self.flush_pass();
                    let d = &self.drawing;
                    let raw = d.color.clone();
                    raw.map(|r| (r, d.samples, d.width, d.height, wgpu::TextureFormat::Rgba8Unorm, 0u32, 0u32))
                }
                Some(fb) => {
                    if self.status(fb) != FRAMEBUFFER_COMPLETE {
                        return self.error(INVALID_FRAMEBUFFER_OPERATION);
                    }
                    let f = self.framebuffer(fb);
                    let a = f.and_then(|f| (f.read_buffer != NONE).then(|| f.color[(f.read_buffer - COLOR_ATTACHMENT0) as usize]).flatten());
                    a.and_then(|a| self.view_of(a)).map(|v| (v.raw, v.samples, v.width, v.height, v.format.wgpu, v.level, v.layer))
                }
            };
            if let Some((raw, samples, sw, sh, sformat, slevel, slayer)) = source {
                let src_view = |level, layer| {
                    raw.create_view(&wgpu::TextureViewDescriptor {
                        label: Some("acde webgl blit source"),
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        base_mip_level: level,
                        mip_level_count: Some(1),
                        base_array_layer: layer,
                        array_layer_count: Some(1),
                        ..Default::default()
                    })
                };
                let full = src == dst && src == [0, 0, sw as i32, sh as i32] && target.width == sw && target.height == sh;
                for color in target.colors.iter().flatten() {
                    if samples > 1 {
                        if !full || color.format != sformat {
                            if self.webgl2 {
                                return self.error(INVALID_OPERATION);
                            }
                            continue;
                        }
                        self.flush_pass();
                        let source = src_view(slevel, slayer);
                        let encoder = self.encoder();
                        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("acde webgl blit resolve"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &source,
                                depth_slice: None,
                                resolve_target: Some(&color.view),
                                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        }));
                        continue;
                    }
                    let [x0, y0, x1, y1] = src.map(|v| v as f32);
                    let from = [x0 / sw as f32, y0 / sh as f32, (x1 - x0) / sw as f32, (y1 - y0) / sh as f32];
                    let [dx0, dy0, dx1, dy1] = dst.map(|v| v as f32);
                    let (mut from, mut viewport) = (from, [dx0, dy0, dx1 - dx0, dy1 - dy0]);
                    if viewport[2] < 0.0 {
                        viewport[0] += viewport[2];
                        viewport[2] = -viewport[2];
                        from[0] += from[2];
                        from[2] = -from[2];
                    }
                    if viewport[3] < 0.0 {
                        viewport[1] += viewport[3];
                        viewport[3] = -viewport[3];
                        from[1] += from[3];
                        from[3] = -from[3];
                    }
                    let source = src_view(slevel, slayer);
                    let f = if filter == LINEAR { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
                    let view = color.view.clone();
                    self.blit(&source, from, &view, color.format, viewport, false, false, false, false, f, 0.0);
                }
            }
        }
        if mask & (DEPTH_BUFFER_BIT | STENCIL_BUFFER_BIT) != 0 {
            let whole_texture = |t: wgpu::Texture| DepthPlace { width: t.width(), height: t.height(), samples: t.sample_count(), level: 0, layer: 0, raw: t };
            let place = |v: View| DepthPlace { width: v.width, height: v.height, samples: v.samples, level: v.level, layer: v.layer, raw: v.raw };
            let source = match read_fb {
                None => self.drawing.depth.clone().map(whole_texture),
                Some(fb) => self.framebuffer(fb).and_then(|f| f.depth.or(f.stencil)).and_then(|a| self.view_of(a)).map(place),
            };
            let dest = match self.state.draw_framebuffer {
                None => self.drawing.depth.clone().map(whole_texture),
                Some(fb) => self.framebuffer(fb).and_then(|f| f.depth.or(f.stencil)).and_then(|a| self.view_of(a)).map(place),
            };
            if let (Some(s), Some(d)) = (source, dest) {
                let whole = (s.width, s.height) == (d.width, d.height)
                    && s.raw.format() == d.raw.format()
                    && src == dst
                    && src == [0, 0, s.width as i32, s.height as i32];
                if whole && s.samples == 1 && d.samples == 1 {
                    self.flush_pass();
                    let size = wgpu::Extent3d { width: s.width, height: s.height, depth_or_array_layers: 1 };
                    self.encoder().copy_texture_to_texture(s.copy(), d.copy(), size);
                } else if whole && s.samples > 1 && d.samples == 1 && mask & DEPTH_BUFFER_BIT != 0 {
                    self.resolve_depth(&s, &d);
                } else if self.webgl2 && !self.warned_depth_blit {
                    self.warned_depth_blit = true;
                    log::warn!("webgl: blitFramebuffer of depth/stencil only copies whole buffers of the same size (and resolves depth, not stencil)");
                }
            }
        }
    }

    fn resolve_depth(&mut self, source: &DepthPlace, dest: &DepthPlace) {
        let format = dest.raw.format();
        let name = format!("depth resolve {format:?}");
        let pipeline = self.helper(&name, |device| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("acde webgl depth resolve"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        "{}@group(0) @binding(0) var source: texture_depth_multisampled_2d;\n@fragment fn fs(@builtin(position) at: vec4f) -> @builtin(frag_depth) f32 {{ return textureLoad(source, vec2i(at.xy), 0); }}\n",
                        crate::webgl::helpers::FULLSCREEN
                    )
                    .into(),
                ),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("acde webgl depth resolve"),
                layout: None,
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[] }),
                multiview_mask: None,
                cache: None,
            })
        });
        let Some(pipeline) = pipeline else { return };
        let one = |p: &DepthPlace, aspect| wgpu::TextureViewDescriptor {
            label: Some("acde webgl depth resolve"),
            dimension: Some(wgpu::TextureViewDimension::D2),
            aspect,
            base_mip_level: p.level,
            mip_level_count: Some(1),
            base_array_layer: p.layer,
            array_layer_count: Some(1),
            ..Default::default()
        };
        let view = source.raw.create_view(&one(source, wgpu::TextureAspect::DepthOnly));
        let target = dest.raw.create_view(&one(dest, wgpu::TextureAspect::All));
        let group = self.scoped(|device| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("acde webgl depth resolve"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
            })
        });
        let Ok(group) = group else { return };
        let stencil = format.has_stencil_aspect();
        self.flush_pass();
        let encoder = self.encoder();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("acde webgl depth resolve"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                stencil_ops: stencil.then_some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn present(&mut self) -> Option<Arc<GpuImage>> {
        if !self.drawing.dirty {
            self.hand_off();
            return None;
        }
        self.resolve_drawing();
        self.drawing.dirty = false;
        let source = self.drawing.resolve.clone().or_else(|| self.drawing.color.clone())?;
        let (w, h) = (self.drawing.width, self.drawing.height);
        let image = self.free_image(w, h)?;
        let mode = if !self.attrs.alpha {
            "opaque"
        } else if self.attrs.premultiplied_alpha {
            "unpremultiply"
        } else {
            "straight"
        };
        let pipeline = self.helper(&format!("present {mode}"), |device| {
            let source = format!("{}{}", crate::webgl::helpers::FULLSCREEN, crate::webgl::helpers::PRESENT);
            let module = device
                .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("acde webgl present"), source: wgpu::ShaderSource::Wgsl(source.into()) });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("acde webgl present"),
                layout: None,
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(mode),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        let pipeline = pipeline?;
        let device = self.gpu.device.clone();
        let view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("acde webgl present"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
        });
        let target = image.texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let encoder = self.encoder();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("acde webgl present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.hand_off();
        image.painted();
        if let Some(i) = self.drawing.images.iter().position(|i| Arc::ptr_eq(i, &image)) {
            let shown = self.drawing.images.remove(i);
            self.drawing.images.push(shown);
        }
        self.drawing.front = Some(image.clone());
        if !self.attrs.preserve_drawing_buffer {
            self.drawing.cleared = false;
        }
        Some(image)
    }

    pub fn front(&self) -> Option<Arc<GpuImage>> {
        self.drawing.front.clone()
    }

    fn free_image(&mut self, w: u32, h: u32) -> Option<Arc<GpuImage>> {
        let d = &mut self.drawing;
        d.images.retain(|i| (i.texture.width(), i.texture.height()) == (w, h) || Arc::strong_count(i) > 1);
        let front = |i: &Arc<GpuImage>| d.front.as_ref().is_some_and(|f| Arc::ptr_eq(f, i));
        if let Some(i) =
            d.images.iter().position(|i| (i.texture.width(), i.texture.height()) == (w, h) && !front(i) && (Arc::strong_count(i) == 1 || i.copied()))
        {
            return Some(d.images[i].clone());
        }
        if d.images.len() >= MAX_IMAGES
            && let Some(i) = d.images.iter().position(|i| (i.texture.width(), i.texture.height()) == (w, h) && !front(i))
        {
            return Some(d.images[i].clone());
        }
        let texture = self
            .scoped(|device| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("acde webgl canvas"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::STORAGE_BINDING,
                    view_formats: &[],
                })
            })
            .ok()?;
        let image = GpuImage::new(texture, &self.gpu.images);
        self.drawing.images.push(image.clone());
        Some(image)
    }

    pub fn front_pixels(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        let image = self.drawing.front.clone()?;
        let (w, h) = (image.texture.width(), image.texture.height());
        self.flush_pass();
        self.submit();
        self.gpu.read(&image.texture, [0, 0, w, h]).ok().map(|p| (w, h, p))
    }
}

fn texel_rgba(format: wgpu::TextureFormat, px: &[u8]) -> [f64; 4] {
    use wgpu::TextureFormat as T;
    let u8n = |b: u8| f64::from(b) / 255.0;
    let half = |i: usize| f64::from(format::half_to_f32(u16::from_le_bytes([px[i], px[i + 1]])));
    let f = |i: usize| f64::from(f32::from_le_bytes([px[i], px[i + 1], px[i + 2], px[i + 3]]));
    let i32_ = |i: usize| f64::from(i32::from_le_bytes([px[i], px[i + 1], px[i + 2], px[i + 3]]));
    let u32_ = |i: usize| f64::from(u32::from_le_bytes([px[i], px[i + 1], px[i + 2], px[i + 3]]));
    match format {
        T::Rgba8Unorm | T::Rgba8UnormSrgb => [u8n(px[0]), u8n(px[1]), u8n(px[2]), u8n(px[3])],
        T::Bgra8Unorm => [u8n(px[2]), u8n(px[1]), u8n(px[0]), u8n(px[3])],
        T::R8Unorm => [u8n(px[0]), 0.0, 0.0, 1.0],
        T::Rg8Unorm => [u8n(px[0]), u8n(px[1]), 0.0, 1.0],
        T::R16Float => [half(0), 0.0, 0.0, 1.0],
        T::Rg16Float => [half(0), half(2), 0.0, 1.0],
        T::Rgba16Float => [half(0), half(2), half(4), half(6)],
        T::R32Float => [f(0), 0.0, 0.0, 1.0],
        T::Rg32Float => [f(0), f(4), 0.0, 1.0],
        T::Rgba32Float => [f(0), f(4), f(8), f(12)],
        T::Rgb10a2Unorm => {
            let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
            [f64::from(v & 1023) / 1023.0, f64::from((v >> 10) & 1023) / 1023.0, f64::from((v >> 20) & 1023) / 1023.0, f64::from(v >> 30) / 3.0]
        }
        T::Rgba8Uint => [f64::from(px[0]), f64::from(px[1]), f64::from(px[2]), f64::from(px[3])],
        T::Rgba8Sint => [f64::from(px[0] as i8), f64::from(px[1] as i8), f64::from(px[2] as i8), f64::from(px[3] as i8)],
        T::R32Sint => [i32_(0), 0.0, 0.0, 1.0],
        T::R32Uint => [u32_(0), 0.0, 0.0, 1.0],
        T::Rgba32Sint => [i32_(0), i32_(4), i32_(8), i32_(12)],
        T::Rgba32Uint => [u32_(0), u32_(4), u32_(8), u32_(12)],
        T::Rg32Uint => [u32_(0), u32_(4), 0.0, 1.0],
        T::Rg32Sint => [i32_(0), i32_(4), 0.0, 1.0],
        _ => [0.0, 0.0, 0.0, 1.0],
    }
}

fn write_texel(dst: &mut [u8], rgba: [f64; 4], ty: u32, opaque: bool) {
    let mut v = rgba;
    if opaque {
        v[3] = 1.0;
    }
    match ty {
        UNSIGNED_BYTE => {
            for (i, c) in v.iter().enumerate() {
                dst[i] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        FLOAT => {
            for (i, c) in v.iter().enumerate() {
                dst[i * 4..i * 4 + 4].copy_from_slice(&(*c as f32).to_le_bytes());
            }
        }
        INT => {
            for (i, c) in v.iter().enumerate() {
                dst[i * 4..i * 4 + 4].copy_from_slice(&(*c as i32).to_le_bytes());
            }
        }
        UNSIGNED_INT => {
            for (i, c) in v.iter().enumerate() {
                dst[i * 4..i * 4 + 4].copy_from_slice(&(*c as u32).to_le_bytes());
            }
        }
        _ => {}
    }
}

struct DepthPlace {
    raw: wgpu::Texture,
    width: u32,
    height: u32,
    samples: u32,
    level: u32,
    layer: u32,
}

impl DepthPlace {
    fn copy(&self) -> wgpu::TexelCopyTextureInfo<'_> {
        wgpu::TexelCopyTextureInfo {
            texture: &self.raw,
            mip_level: self.level,
            origin: wgpu::Origin3d { x: 0, y: 0, z: self.layer },
            aspect: wgpu::TextureAspect::All,
        }
    }
}
