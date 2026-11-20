use wgpu::util::DeviceExt;

use crate::webgl::context::Context;

#[derive(Clone, PartialEq, Eq, Debug, Default, Hash)]
pub struct TargetKey(pub Vec<u64>);

#[derive(Clone)]
pub struct ColorTarget {
    pub view: wgpu::TextureView,
    pub resolve: Option<wgpu::TextureView>,
    pub format: wgpu::TextureFormat,
    pub class: crate::webgl::format::Class,
    pub depth_slice: Option<u32>,
}

#[derive(Clone)]
pub struct DepthTarget {
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub depth: bool,
    pub stencil: bool,
}

#[derive(Clone)]
pub struct Target {
    pub key: TargetKey,
    pub colors: Vec<Option<ColorTarget>>,
    pub depth: Option<DepthTarget>,
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub textures: Vec<u32>,
}

#[derive(Clone)]
pub enum Source {
    Buffer(wgpu::Buffer),
    Data,
}

pub enum Call {
    Draw { vertices: std::ops::Range<u32>, instances: std::ops::Range<u32> },
    Indexed { indices: std::ops::Range<u32>, base_vertex: i32, instances: std::ops::Range<u32> },
}

pub struct Draw {
    pub pipeline: wgpu::RenderPipeline,
    pub resources: Option<wgpu::BindGroup>,
    pub uniforms: [u32; 2],
    pub vertex: Vec<(u32, Source, u64)>,
    pub index: Option<(Source, u64, wgpu::IndexFormat)>,
    pub call: Call,
    pub viewport: [f32; 6],
    pub scissor: [u32; 4],
    pub stencil: u32,
    pub blend: [f64; 4],
    pub occlusion: Option<u32>,
}

pub struct Pass {
    pub target: Target,
    pub colors: Vec<Option<wgpu::Color>>,
    pub depth: Option<f32>,
    pub stencil: Option<u32>,
    pub draws: Vec<Draw>,
    pub data: Vec<u8>,
    pub span: u32,
    pub queries: Vec<crate::webgl::objects::Id>,
    pub uniform_data: crate::webgl::FastMap<(u64, u64), [u32; 2]>,
}

#[derive(Default)]
struct Bound {
    pipeline: Option<wgpu::RenderPipeline>,
    uniforms: Option<[u32; 2]>,
    resources: Option<wgpu::BindGroup>,
    vertex: Vec<Option<(wgpu::Buffer, u64)>>,
    index: Option<(wgpu::Buffer, u64, wgpu::IndexFormat)>,
    viewport: Option<[f32; 6]>,
    scissor: Option<[u32; 4]>,
    stencil: Option<u32>,
    blend: Option<[f64; 4]>,
}

#[derive(Default)]
pub struct Recorder {
    pub encoder: Option<wgpu::CommandEncoder>,
    pub pass: Option<Pass>,
    pub passes: u32,
    pub readbacks: Vec<std::rc::Rc<crate::webgl::objects::Readback>>,
}

impl Pass {
    pub fn push_data(&mut self, bytes: &[u8], align: u32) -> u32 {
        let at = (self.data.len() as u32).next_multiple_of(align.max(4));
        self.data.resize(at as usize, 0);
        self.data.extend_from_slice(bytes);
        at
    }
}

impl Context {
    pub(crate) fn encoder(&mut self) -> &mut wgpu::CommandEncoder {
        if self.recorder.encoder.is_none() {
            self.recorder.encoder = Some(self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("acde webgl") }));
        }
        self.recorder.encoder.as_mut().unwrap_or_else(|| unreachable!())
    }

    pub(crate) fn open_pass(&mut self, target: Target) -> &mut Pass {
        let same = self.recorder.pass.as_ref().is_some_and(|p| p.target.key == target.key);
        if !same {
            self.flush_pass();
            let colors = vec![None; target.colors.len()];
            self.recorder.pass = Some(Pass {
                target,
                colors,
                depth: None,
                stencil: None,
                draws: Vec::new(),
                data: Vec::new(),
                span: 16,
                queries: Vec::new(),
                uniform_data: Default::default(),
            });
        }
        self.recorder.pass.as_mut().unwrap_or_else(|| unreachable!())
    }

    pub(crate) fn flush_pass(&mut self) {
        let Some(pass) = self.recorder.pass.take() else { return };
        if pass.draws.is_empty() && pass.colors.iter().all(Option::is_none) && pass.depth.is_none() && pass.stencil.is_none() {
            return;
        }
        let align = self.limits.uniform_alignment;
        let span = pass.span.next_multiple_of(16);
        let mut data = pass.data;
        let end = (data.len() as u32).next_multiple_of(align) + span;
        data.resize(end as usize, 0);
        let device = self.gpu.device.clone();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("acde webgl pass data"),
            contents: &data,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX,
        });
        let binding = |b| wgpu::BindGroupEntry {
            binding: b,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &buffer, offset: 0, size: wgpu::BufferSize::new(u64::from(span)) }),
        };
        let uniforms = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("acde webgl uniforms"),
            layout: &self.caches.uniforms,
            entries: &[binding(0), binding(1)],
        });
        let target = &pass.target;
        let colors: Vec<Option<wgpu::RenderPassColorAttachment>> = target
            .colors
            .iter()
            .zip(&pass.colors)
            .map(|(c, clear)| {
                c.as_ref().map(|c| wgpu::RenderPassColorAttachment {
                    view: &c.view,
                    depth_slice: c.depth_slice,
                    resolve_target: c.resolve.as_ref(),
                    ops: wgpu::Operations { load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear), store: wgpu::StoreOp::Store },
                })
            })
            .collect();
        let depth = target.depth.as_ref().map(|d| wgpu::RenderPassDepthStencilAttachment {
            view: &d.view,
            depth_ops: d.depth.then(|| wgpu::Operations { load: pass.depth.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear), store: wgpu::StoreOp::Store }),
            stencil_ops: d
                .stencil
                .then(|| wgpu::Operations { load: pass.stencil.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear), store: wgpu::StoreOp::Store }),
        });
        let occlusion = (!pass.queries.is_empty()).then(|| {
            device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("acde webgl occlusion"),
                ty: wgpu::QueryType::Occlusion,
                count: pass.queries.len() as u32,
            })
        });
        let encoder = self.encoder();
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("acde webgl"),
            color_attachments: &colors,
            depth_stencil_attachment: depth,
            timestamp_writes: None,
            occlusion_query_set: occlusion.as_ref(),
            multiview_mask: None,
        });
        let mut query = None;
        let mut last = Bound::default();
        for draw in &pass.draws {
            if draw.occlusion != query {
                if query.is_some() {
                    rp.end_occlusion_query();
                }
                if let Some(q) = draw.occlusion {
                    rp.begin_occlusion_query(q);
                }
                query = draw.occlusion;
            }
            if last.pipeline.as_ref() != Some(&draw.pipeline) {
                rp.set_pipeline(&draw.pipeline);
                last.pipeline = Some(draw.pipeline.clone());
            }
            if last.uniforms != Some(draw.uniforms) {
                rp.set_bind_group(0, &uniforms, &draw.uniforms);
                last.uniforms = Some(draw.uniforms);
            }
            if let Some(resources) = &draw.resources
                && last.resources.as_ref() != Some(resources)
            {
                rp.set_bind_group(1, resources, &[]);
                last.resources = Some(resources.clone());
            }
            for (slot, source, offset) in &draw.vertex {
                let b = match source {
                    Source::Buffer(b) => b,
                    Source::Data => &buffer,
                };
                let at = *slot as usize;
                if last.vertex.len() <= at {
                    last.vertex.resize(at + 1, None);
                }
                if last.vertex[at].as_ref().is_some_and(|(lb, lo)| lb == b && lo == offset) {
                    continue;
                }
                rp.set_vertex_buffer(*slot, b.slice(*offset..));
                last.vertex[at] = Some((b.clone(), *offset));
            }
            if let Some((source, offset, format)) = &draw.index {
                let b = match source {
                    Source::Buffer(b) => b,
                    Source::Data => &buffer,
                };
                if last.index.as_ref().is_none_or(|(lb, lo, lf)| lb != b || lo != offset || lf != format) {
                    rp.set_index_buffer(b.slice(*offset..), *format);
                    last.index = Some((b.clone(), *offset, *format));
                }
            }
            if last.viewport != Some(draw.viewport) {
                let [x, y, w, h, near, far] = draw.viewport;
                rp.set_viewport(x, y, w, h, near, far);
                last.viewport = Some(draw.viewport);
            }
            if last.scissor != Some(draw.scissor) {
                let [sx, sy, sw, sh] = draw.scissor;
                rp.set_scissor_rect(sx, sy, sw, sh);
                last.scissor = Some(draw.scissor);
            }
            if last.stencil != Some(draw.stencil) {
                rp.set_stencil_reference(draw.stencil);
                last.stencil = Some(draw.stencil);
            }
            if last.blend != Some(draw.blend) {
                let [r, g, b, a] = draw.blend;
                rp.set_blend_constant(wgpu::Color { r, g, b, a });
                last.blend = Some(draw.blend);
            }
            match &draw.call {
                Call::Draw { vertices, instances } => rp.draw(vertices.clone(), instances.clone()),
                Call::Indexed { indices, base_vertex, instances } => rp.draw_indexed(indices.clone(), *base_vertex, instances.clone()),
            }
        }
        if query.is_some() {
            rp.end_occlusion_query();
        }
        drop(rp);
        if let Some(set) = occlusion {
            let n = pass.queries.len() as u64;
            let resolve = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("acde webgl occlusion"),
                size: n * 8,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("acde webgl occlusion"),
                size: n * 8,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let encoder = self.encoder();
            encoder.resolve_query_set(&set, 0..n as u32, &resolve, 0);
            encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, n * 8);
            let readback = std::rc::Rc::new(crate::webgl::objects::Readback { buffer: read, state: Default::default() });
            for (slot, id) in pass.queries.iter().enumerate() {
                if let Some(q) = self.objects.get_mut(id).and_then(crate::webgl::objects::Object::query_mut) {
                    q.slots.push((readback.clone(), slot as u32));
                }
            }
            self.recorder.readbacks.push(readback);
        }
        self.recorder.passes += 1;
        if self.recorder.passes >= 256 {
            self.submit();
        }
    }

    pub fn submit(&mut self) -> Option<wgpu::SubmissionIndex> {
        let Some(ticket) = self.hand_off() else {
            self.settle();
            return None;
        };
        self.inflight = None;
        ticket.wait()
    }

    pub(crate) fn hand_off(&mut self) -> Option<std::sync::Arc<crate::webgl::submit::Ticket>> {
        self.flush_pass();
        let encoder = self.recorder.encoder.take()?;
        self.recorder.passes = 0;
        self.generation += 1;
        let readbacks = std::mem::take(&mut self.recorder.readbacks);
        let ticket = crate::webgl::submit::submit(self.gpu.clone(), encoder, &readbacks);
        self.inflight = Some(ticket.clone());
        Some(ticket)
    }

    pub(crate) fn settle(&mut self) {
        if let Some(ticket) = self.inflight.take() {
            ticket.wait();
        }
    }

    pub fn after_submitted(&mut self, then: Box<dyn FnOnce() + Send>) {
        crate::webgl::submit::then(then);
    }
}
