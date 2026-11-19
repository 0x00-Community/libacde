use crate::webgl::consts::*;
use crate::webgl::context::Context;
use crate::webgl::format::{self, Class, Format};
use crate::webgl::objects::*;
use crate::webgl::pass::{ColorTarget, DepthTarget, Target, TargetKey};

pub(crate) struct View {
    pub view: wgpu::TextureView,
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub depth_slice: Option<u32>,
    pub key: [u64; 4],
    pub texture: Option<Id>,
    pub raw: wgpu::Texture,
    pub level: u32,
    pub layer: u32,
}

impl Context {
    pub fn create_framebuffer(&mut self) -> Id {
        self.add(Object::Framebuffer(Framebuffer::new()))
    }

    pub fn create_renderbuffer(&mut self) -> Id {
        self.add(Object::Renderbuffer(Renderbuffer { format: None, samples: 0, width: 0, height: 0, texture: None, view: None, version: 0, used: 0 }))
    }

    pub fn bind_framebuffer(&mut self, target: u32, id: Option<Id>) {
        if id.is_some_and(|id| self.framebuffer(id).is_none()) {
            return self.error(INVALID_OPERATION);
        }
        match target {
            FRAMEBUFFER => {
                self.state.draw_framebuffer = id;
                self.state.read_framebuffer = id;
            }
            DRAW_FRAMEBUFFER if self.webgl2 => self.state.draw_framebuffer = id,
            READ_FRAMEBUFFER if self.webgl2 => self.state.read_framebuffer = id,
            _ => self.error(INVALID_ENUM),
        }
    }

    pub fn bind_renderbuffer(&mut self, target: u32, id: Option<Id>) {
        if target != RENDERBUFFER {
            return self.error(INVALID_ENUM);
        }
        if id.is_some_and(|id| self.renderbuffer(id).is_none()) {
            return self.error(INVALID_OPERATION);
        }
        self.state.renderbuffer = id;
    }

    pub fn delete_framebuffer(&mut self, id: Id) {
        if self.state.draw_framebuffer == Some(id) {
            self.state.draw_framebuffer = None;
        }
        if self.state.read_framebuffer == Some(id) {
            self.state.read_framebuffer = None;
        }
        if self.framebuffer(id).is_some() {
            self.objects.remove(&id);
        }
    }

    pub fn delete_renderbuffer(&mut self, id: Id) {
        if self.renderbuffer(id).is_none() {
            return;
        }
        if self.state.renderbuffer == Some(id) {
            self.state.renderbuffer = None;
        }
        for fb in [self.state.draw_framebuffer, self.state.read_framebuffer].into_iter().flatten() {
            if let Some(f) = self.framebuffer_mut(fb) {
                let hit = |a: &Option<Attachment>| *a == Some(Attachment::Renderbuffer(id));
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

    pub fn renderbuffer_storage(&mut self, target: u32, samples: i32, internal: u32, width: i32, height: i32) {
        if target != RENDERBUFFER {
            return self.error(INVALID_ENUM);
        }
        let Some(id) = self.state.renderbuffer else { return self.error(INVALID_OPERATION) };
        if width < 0 || height < 0 || samples < 0 {
            return self.error(INVALID_VALUE);
        }
        let max = self.limits.texture_size as i32;
        if width > max || height > max {
            return self.error(INVALID_VALUE);
        }
        let internal = match internal {
            DEPTH_STENCIL => DEPTH24_STENCIL8,
            RGBA4 | RGB5_A1 => RGBA8,
            RGB565 => RGB8,
            i => i,
        };
        let Some(mut fmt) = format::sized(internal).filter(|_| self.format_allowed(internal)) else { return self.error(INVALID_ENUM) };
        if !self.color_renderable(&fmt) && !fmt.wgpu.is_depth_stencil_format() {
            return self.error(INVALID_ENUM);
        }
        if fmt.expand != format::Expand::None {
            fmt.expand = format::Expand::None;
        }
        if samples > self.limits.samples as i32 {
            return self.error(INVALID_OPERATION);
        }
        let samples = if samples > 1 { 4 } else { 1 };
        if samples > 1 && matches!(fmt.class, Class::Sint | Class::Uint) {
            return self.error(INVALID_OPERATION);
        }
        let texture = (width > 0 && height > 0).then(|| {
            self.scoped(|device| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("acde webgl renderbuffer"),
                    size: wgpu::Extent3d { width: width as u32, height: height as u32, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format: fmt.wgpu,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | if samples == 1 { wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST } else { wgpu::TextureUsages::empty() },
                    view_formats: &[],
                })
            })
        });
        let texture = match texture {
            Some(Err(e)) => {
                log::warn!("webgl: renderbuffer: {e}");
                return self.error(OUT_OF_MEMORY);
            }
            Some(Ok(t)) => Some(t),
            None => None,
        };
        if let Some(texture) = &texture {
            self.clear_to_defaults(texture, 1, 1);
        }
        let serial = self.next_serial();
        if let Some(rb) = self.renderbuffer_mut(id) {
            rb.format = Some(fmt);
            rb.samples = if samples > 1 { samples } else { 0 };
            rb.width = width as u32;
            rb.height = height as u32;
            rb.view = texture.as_ref().map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
            rb.texture = texture;
            rb.version = serial;
        }
    }

    pub(crate) fn mark_used(&mut self, textures: &[Id]) {
        let generation = self.generation;
        for id in textures {
            if let Some(t) = self.texture_mut(*id) {
                t.used = generation;
            }
        }
    }

    pub(crate) fn color_renderable(&self, fmt: &Format) -> bool {
        if fmt.wgpu.is_depth_stencil_format() || fmt.wgpu.is_compressed() {
            return false;
        }
        if !fmt.wgpu.guaranteed_format_features(self.features).allowed_usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT) {
            return false;
        }
        let float =
            matches!(fmt.component_type, FLOAT) && !matches!(fmt.wgpu, wgpu::TextureFormat::Rg11b10Ufloat) || fmt.wgpu == wgpu::TextureFormat::Rg11b10Ufloat;
        if float {
            let half = matches!(fmt.wgpu, wgpu::TextureFormat::R16Float | wgpu::TextureFormat::Rg16Float | wgpu::TextureFormat::Rgba16Float);
            return match self.webgl2 {
                true => self.extension("EXT_color_buffer_float") || (half && self.extension("EXT_color_buffer_half_float")),
                false => {
                    (half && (self.extension("EXT_color_buffer_half_float") || self.extension("OES_texture_half_float")))
                        || (!half && (self.extension("WEBGL_color_buffer_float") || self.extension("OES_texture_float")))
                }
            } && fmt.expand == format::Expand::None
                || (!self.webgl2 && fmt.expand == format::Expand::Rgb && (self.extension("OES_texture_float") || self.extension("OES_texture_half_float")));
        }
        fmt.renderable || (!self.webgl2 && matches!(fmt.internal, RGB | RGBA))
    }

    fn framebuffer_for(&mut self, target: u32) -> Option<Option<Id>> {
        match target {
            FRAMEBUFFER | DRAW_FRAMEBUFFER if target == FRAMEBUFFER || self.webgl2 => Some(self.state.draw_framebuffer),
            READ_FRAMEBUFFER if self.webgl2 => Some(self.state.read_framebuffer),
            _ => {
                self.error(INVALID_ENUM);
                None
            }
        }
    }

    fn attach(&mut self, target: u32, attachment: u32, value: Option<Attachment>) {
        let Some(fb) = self.framebuffer_for(target) else { return };
        let Some(fb) = fb else { return self.error(INVALID_OPERATION) };
        let max = self.limits.draw_buffers;
        let webgl2 = self.webgl2;
        let draw_buffers = self.extension("WEBGL_draw_buffers") || webgl2;
        let Some(f) = self.framebuffer_mut(fb) else { return };
        match attachment {
            DEPTH_ATTACHMENT => f.depth = value,
            STENCIL_ATTACHMENT => f.stencil = value,
            DEPTH_STENCIL_ATTACHMENT => {
                f.depth = value;
                f.stencil = value;
            }
            a if (COLOR_ATTACHMENT0..COLOR_ATTACHMENT0 + max).contains(&a) && (a == COLOR_ATTACHMENT0 || draw_buffers) => {
                f.color[(a - COLOR_ATTACHMENT0) as usize] = value
            }
            _ => self.error(INVALID_ENUM),
        }
    }

    pub fn framebuffer_renderbuffer(&mut self, target: u32, attachment: u32, rbtarget: u32, id: Option<Id>) {
        if rbtarget != RENDERBUFFER {
            return self.error(INVALID_ENUM);
        }
        if id.is_some_and(|id| self.renderbuffer(id).is_none()) {
            return self.error(INVALID_OPERATION);
        }
        self.attach(target, attachment, id.map(Attachment::Renderbuffer));
    }

    pub fn framebuffer_texture(&mut self, target: u32, attachment: u32, textarget: u32, id: Option<Id>, level: i32, layer: i32) {
        if level < 0 || layer < 0 {
            return self.error(INVALID_VALUE);
        }
        let value = match id {
            None => None,
            Some(id) => {
                let Some(t) = self.texture(id) else { return self.error(INVALID_OPERATION) };
                let textarget = if textarget == 0 && matches!(t.target, TEXTURE_3D | TEXTURE_2D_ARRAY) { t.target } else { textarget };
                let face = (TEXTURE_CUBE_MAP_POSITIVE_X..=TEXTURE_CUBE_MAP_NEGATIVE_Z).contains(&textarget);
                let ok = match textarget {
                    TEXTURE_2D => t.target == TEXTURE_2D,
                    TEXTURE_3D | TEXTURE_2D_ARRAY => t.target == textarget,
                    _ if face => t.target == TEXTURE_CUBE_MAP,
                    _ => return self.error(INVALID_ENUM),
                };
                if !ok {
                    return self.error(INVALID_OPERATION);
                }
                if level > 0 && !self.webgl2 && !self.extension("OES_fbo_render_mipmap") {
                    return self.error(INVALID_VALUE);
                }
                let layer = if face { textarget - TEXTURE_CUBE_MAP_POSITIVE_X } else { layer as u32 };
                Some(Attachment::Texture { id, level: level as u32, layer })
            }
        };
        self.attach(target, attachment, value);
    }

    pub(crate) fn view_of(&self, a: Attachment) -> Option<View> {
        match a {
            Attachment::Renderbuffer(id) => {
                let rb = self.renderbuffer(id)?;
                let texture = rb.texture.clone()?;
                Some(View {
                    view: rb.view.clone()?,
                    format: rb.format?,
                    width: rb.width,
                    height: rb.height,
                    samples: rb.samples.max(1),
                    depth_slice: None,
                    key: [1, u64::from(id), rb.version, 0],
                    texture: None,
                    raw: texture,
                    level: 0,
                    layer: 0,
                })
            }
            Attachment::Texture { id, level, layer } => {
                let t = self.texture(id)?;
                let alloc = t.alloc.as_ref()?;
                let face = if t.target == TEXTURE_CUBE_MAP { layer } else { 0 };
                let image = t.images.get(&(face, level))?;
                if level >= alloc.mips {
                    return None;
                }
                let is_3d = t.target == TEXTURE_3D;
                let view = alloc.view(ViewKey {
                    dim: if is_3d { wgpu::TextureViewDimension::D3 } else { wgpu::TextureViewDimension::D2 },
                    depth_only: false,
                    level,
                    levels: 1,
                    layer: Some(if is_3d { 0 } else { layer }),
                });
                Some(View {
                    view,
                    format: image.format,
                    width: image.width,
                    height: image.height,
                    samples: 1,
                    depth_slice: is_3d.then_some(layer),
                    key: [2, u64::from(id), alloc.serial, u64::from(level) << 32 | u64::from(layer)],
                    texture: Some(id),
                    raw: alloc.texture.clone(),
                    level,
                    layer,
                })
            }
        }
    }

    pub fn check_framebuffer_status(&mut self, target: u32) -> u32 {
        let Some(fb) = self.framebuffer_for(target) else { return 0 };
        match fb {
            None => FRAMEBUFFER_COMPLETE,
            Some(fb) => self.status(fb),
        }
    }

    pub(crate) fn status(&self, fb: Id) -> u32 {
        let Some(f) = self.framebuffer(fb) else { return FRAMEBUFFER_UNSUPPORTED };
        let mut size = None;
        let mut samples = None;
        let mut any = false;
        let mut check = |v: &View| -> Result<(), u32> {
            if v.width == 0 || v.height == 0 {
                return Err(FRAMEBUFFER_INCOMPLETE_ATTACHMENT);
            }
            match size {
                None => size = Some((v.width, v.height)),
                Some(s) if s != (v.width, v.height) => return Err(if self.webgl2 { FRAMEBUFFER_UNSUPPORTED } else { FRAMEBUFFER_INCOMPLETE_DIMENSIONS }),
                _ => {}
            }
            match samples {
                None => samples = Some(v.samples),
                Some(s) if s != v.samples => return Err(FRAMEBUFFER_INCOMPLETE_MULTISAMPLE),
                _ => {}
            }
            any = true;
            Ok(())
        };
        for a in f.color.iter().flatten() {
            let Some(v) = self.view_of(*a) else { return FRAMEBUFFER_INCOMPLETE_ATTACHMENT };
            if !self.color_renderable(&v.format) {
                return FRAMEBUFFER_INCOMPLETE_ATTACHMENT;
            }
            if let Err(e) = check(&v) {
                return e;
            }
        }
        if let (Some(d), Some(s)) = (f.depth, f.stencil)
            && d != s
        {
            return FRAMEBUFFER_UNSUPPORTED;
        }
        for (a, need_depth) in [(f.depth, true), (f.stencil, false)] {
            let Some(a) = a else { continue };
            let Some(v) = self.view_of(a) else { return FRAMEBUFFER_INCOMPLETE_ATTACHMENT };
            let ok = if need_depth { v.format.wgpu.has_depth_aspect() } else { v.format.wgpu.has_stencil_aspect() };
            if !ok {
                return FRAMEBUFFER_INCOMPLETE_ATTACHMENT;
            }
            if let Err(e) = check(&v) {
                return e;
            }
        }
        if !any {
            return FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT;
        }
        FRAMEBUFFER_COMPLETE
    }

    pub(crate) fn draw_target(&mut self) -> Result<Target, u32> {
        let Some(fb) = self.state.draw_framebuffer else { return Ok(self.default_target()) };
        let status = self.status(fb);
        if status != FRAMEBUFFER_COMPLETE {
            return Err(INVALID_FRAMEBUFFER_OPERATION);
        }
        let Some(f) = self.framebuffer(fb) else { return Err(INVALID_FRAMEBUFFER_OPERATION) };
        let mut key = vec![u64::from(fb)];
        let mut colors = Vec::new();
        let (mut width, mut height, mut samples) = (0, 0, 1);
        let mut textures = Vec::new();
        let draw_buffers = f.draw_buffers;
        let color = f.color;
        let (depth, stencil) = (f.depth, f.stencil);
        for (i, buffer) in draw_buffers.iter().enumerate() {
            let attachment = (*buffer != NONE).then(|| color.get((buffer - COLOR_ATTACHMENT0) as usize).copied().flatten()).flatten();
            match attachment.and_then(|a| self.view_of(a)) {
                Some(v) => {
                    key.extend(v.key);
                    (width, height, samples) = (v.width, v.height, v.samples);
                    textures.extend(v.texture);
                    colors.push(Some(ColorTarget { view: v.view, resolve: None, format: v.format.wgpu, class: v.format.class, depth_slice: v.depth_slice }));
                }
                None => {
                    key.push(0);
                    if i < colors.len() || draw_buffers[i..].iter().any(|b| *b != NONE) {
                        colors.push(None);
                    }
                }
            }
        }
        while colors.last().is_some_and(Option::is_none) {
            colors.pop();
        }
        let depth_attachment = depth.or(stencil);
        let depth = match depth_attachment.and_then(|a| self.view_of(a)) {
            Some(v) => {
                key.extend(v.key);
                (width, height, samples) = (v.width, v.height, v.samples);
                textures.extend(v.texture);
                Some(DepthTarget { view: v.view, format: v.format.wgpu, depth: v.format.wgpu.has_depth_aspect(), stencil: v.format.wgpu.has_stencil_aspect() })
            }
            None => None,
        };
        Ok(Target { key: TargetKey(key), colors, depth, width, height, samples, textures })
    }

    pub(crate) fn default_target(&mut self) -> Target {
        let d = &self.drawing;
        let color = d.color_view.clone();
        let draw = self.state.default_draw_buffer != NONE;
        let colors = vec![color.filter(|_| draw).map(|view| ColorTarget {
            view,
            resolve: None,
            format: wgpu::TextureFormat::Rgba8Unorm,
            class: Class::Float,
            depth_slice: None,
        })];
        let depth =
            d.depth_view.clone().zip(d.depth_format).map(|(view, format)| DepthTarget { view, format, depth: true, stencil: format.has_stencil_aspect() });
        Target {
            key: TargetKey(vec![0, d.version, u64::from(draw)]),
            colors,
            depth,
            width: d.width,
            height: d.height,
            samples: d.samples,
            textures: Vec::new(),
        }
    }

    pub(crate) fn pass_for_draw(&mut self, target: Target) {
        self.mark_used(&target.textures);
        let default = self.state.draw_framebuffer.is_none();
        let fresh = default && !self.drawing.cleared;
        let alpha = self.attrs.alpha;
        let pass = self.open_pass(target);
        if fresh {
            pass.colors.fill(Some(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: if alpha { 0.0 } else { 1.0 } }));
            pass.depth = pass.target.depth.as_ref().filter(|d| d.depth).map(|_| 1.0);
            pass.stencil = pass.target.depth.as_ref().filter(|d| d.stencil).map(|_| 0);
            self.drawing.cleared = true;
        }
        if default {
            self.drawing.dirty = true;
            self.drawing.unresolved = true;
        }
    }

    pub fn draw_buffers(&mut self, buffers: &[u32]) {
        match self.state.draw_framebuffer {
            None => {
                if buffers.len() != 1 || !matches!(buffers[0], BACK | NONE) {
                    return self.error(INVALID_OPERATION);
                }
                self.state.default_draw_buffer = buffers[0];
            }
            Some(fb) => {
                if buffers.len() as u32 > self.limits.draw_buffers {
                    return self.error(INVALID_VALUE);
                }
                for (i, b) in buffers.iter().enumerate() {
                    if *b != NONE && *b != COLOR_ATTACHMENT0 + i as u32 {
                        return self.error(INVALID_OPERATION);
                    }
                }
                if let Some(f) = self.framebuffer_mut(fb) {
                    f.draw_buffers = [NONE; 8];
                    f.draw_buffers[..buffers.len()].copy_from_slice(buffers);
                }
            }
        }
    }

    pub fn read_buffer(&mut self, src: u32) {
        match self.state.read_framebuffer {
            None => {
                if !matches!(src, BACK | NONE) {
                    return self.error(INVALID_OPERATION);
                }
                self.state.default_read_buffer = src;
            }
            Some(fb) => {
                if src != NONE && !(COLOR_ATTACHMENT0..COLOR_ATTACHMENT0 + self.limits.draw_buffers).contains(&src) {
                    return self.error(INVALID_OPERATION);
                }
                if let Some(f) = self.framebuffer_mut(fb) {
                    f.read_buffer = src;
                }
            }
        }
    }

    pub(crate) fn read_view(&mut self) -> Result<View, u32> {
        match self.state.read_framebuffer {
            None => {
                if self.state.default_read_buffer == NONE {
                    return Err(INVALID_OPERATION);
                }
                self.resolve_drawing();
                let d = &self.drawing;
                let texture = d.resolve.clone().or_else(|| d.color.clone()).ok_or(INVALID_OPERATION)?;
                let format = format::sized(RGBA8).ok_or(INVALID_OPERATION)?;
                Ok(View {
                    view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
                    format,
                    width: d.width,
                    height: d.height,
                    samples: 1,
                    depth_slice: None,
                    key: [0; 4],
                    texture: None,
                    raw: texture,
                    level: 0,
                    layer: 0,
                })
            }
            Some(fb) => {
                if self.status(fb) != FRAMEBUFFER_COMPLETE {
                    return Err(INVALID_FRAMEBUFFER_OPERATION);
                }
                let f = self.framebuffer(fb).ok_or(INVALID_OPERATION)?;
                if f.read_buffer == NONE {
                    return Err(INVALID_OPERATION);
                }
                let a = f.color[(f.read_buffer - COLOR_ATTACHMENT0) as usize].ok_or(INVALID_OPERATION)?;
                self.view_of(a).ok_or(INVALID_OPERATION)
            }
        }
    }

    pub(crate) fn resolve_drawing(&mut self) {
        self.flush_pass();
        if self.drawing.samples <= 1 || !self.drawing.unresolved {
            return;
        }
        self.drawing.unresolved = false;
        let (Some(color), Some(resolve)) = (self.drawing.color.clone(), self.drawing.resolve.clone()) else { return };
        let view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let target = resolve.create_view(&wgpu::TextureViewDescriptor::default());
        let encoder = self.encoder();
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("acde webgl resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: Some(&target),
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
    }
}
