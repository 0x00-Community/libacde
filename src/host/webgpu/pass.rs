use crate::gpu::wgpu;
use wgpu::util::RenderEncoder;

use super::{Id, Kind, State, desc, maybe, report};

pub fn commands(args: &v8::FunctionCallbackArguments, i: i32, length: u32) -> Vec<f64> {
    let Ok(array) = v8::Local::<v8::Float64Array>::try_from(args.get(i)) else { return Vec::new() };
    let length = (length as usize).min(array.length());
    let mut out = vec![0.0; length];
    let bytes = unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, length * 8) };
    array.copy_contents(bytes);
    out
}

pub fn strings(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> Vec<String> {
    let Ok(list) = v8::Local::<v8::Array>::try_from(args.get(i)) else { return Vec::new() };
    (0..list.length()).map(|k| list.get_index(scope, k).map(|v| v.to_rust_string_lossy(scope)).unwrap_or_default()).collect()
}

struct Reader<'a> {
    commands: &'a [f64],
    at: usize,
}

impl Reader<'_> {
    fn next(&mut self) -> Option<f64> {
        let value = self.commands.get(self.at).copied();
        self.at += 1;
        value
    }

    fn u32(&mut self) -> u32 {
        self.next().filter(|n| *n >= 0.0).map_or(0, |n| n as u32)
    }

    fn u64(&mut self) -> u64 {
        self.next().filter(|n| *n >= 0.0).map_or(0, |n| n as u64)
    }

    fn f32(&mut self) -> f32 {
        self.next().unwrap_or(0.0) as f32
    }

    fn index(&mut self) -> Option<u32> {
        self.next().filter(|n| *n >= 0.0).map(|n| n as u32)
    }

    fn id(&mut self) -> Option<u32> {
        Some(self.u32()).filter(|id| *id != 0)
    }

    fn count(&mut self) -> usize {
        (self.u32() as usize).min(self.commands.len().saturating_sub(self.at))
    }

    fn load(&mut self) -> Option<desc::LoadOp> {
        match self.next()? as i32 {
            0 => Some(desc::LoadOp::Clear),
            1 => Some(desc::LoadOp::Load),
            _ => None,
        }
    }

    fn store(&mut self) -> Option<desc::StoreOp> {
        match self.next()? as i32 {
            0 => Some(desc::StoreOp::Store),
            1 => Some(desc::StoreOp::Discard),
            _ => None,
        }
    }

    fn label(&mut self, strings: &[String]) -> Option<String> {
        self.index().and_then(|i| strings.get(i as usize)).cloned()
    }

    fn timestamps(&mut self) -> Option<desc::TimestampWritesDesc> {
        (self.u32() == 1).then(|| desc::TimestampWritesDesc {
            query_set: self.u32(),
            beginning_of_pass_write_index: self.index(),
            end_of_pass_write_index: self.index(),
        })
    }

    fn size(&mut self) -> Option<wgpu::BufferSize> {
        self.next().filter(|n| *n >= 0.0).and_then(|n| wgpu::BufferSize::new(n as u64))
    }

    fn done(&self) -> bool {
        self.at >= self.commands.len()
    }
}

pub fn render_desc(commands: &[f64], strings: &[String]) -> (desc::RenderPassDesc, usize) {
    let mut r = Reader { commands, at: 0 };
    let label = r.label(strings);
    let colors = r.count();
    let color_attachments = (0..colors)
        .map(|_| {
            (r.u32() == 1).then(|| desc::ColorAttachmentDesc {
                view: r.u32(),
                depth_slice: r.index(),
                resolve_target: r.id(),
                clear_value: [r.next().unwrap_or(0.0), r.next().unwrap_or(0.0), r.next().unwrap_or(0.0), r.next().unwrap_or(0.0)],
                load_op: r.load().unwrap_or(desc::LoadOp::Load),
                store_op: r.store().unwrap_or(desc::StoreOp::Store),
            })
        })
        .collect();
    let depth_stencil_attachment = (r.u32() == 1).then(|| desc::DepthStencilAttachmentDesc {
        view: r.u32(),
        depth_clear_value: r.next().filter(|n| !n.is_nan()).map(|n| n as f32),
        depth_load_op: r.load(),
        depth_store_op: r.store(),
        depth_read_only: r.u32() == 1,
        stencil_clear_value: r.u32(),
        stencil_load_op: r.load(),
        stencil_store_op: r.store(),
        stencil_read_only: r.u32() == 1,
    });
    let occlusion_query_set = r.id();
    let timestamp_writes = r.timestamps();
    let at = r.at.min(commands.len());
    (desc::RenderPassDesc { label, color_attachments, depth_stencil_attachment, occlusion_query_set, timestamp_writes }, at)
}

pub fn compute_desc(commands: &[f64], strings: &[String]) -> (desc::ComputePassDesc, usize) {
    let mut r = Reader { commands, at: 0 };
    let label = r.label(strings);
    let timestamp_writes = r.timestamps();
    let at = r.at.min(commands.len());
    (desc::ComputePassDesc { label, timestamp_writes }, at)
}

trait Pass {
    fn bind(&mut self, index: u32, group: Option<&wgpu::BindGroup>, offsets: &[u32]);
    fn debug(&mut self, op: u32, label: &str);
}

macro_rules! pass {
    ($($ty:ty),*) => {$(
        impl Pass for $ty {
            fn bind(&mut self, index: u32, group: Option<&wgpu::BindGroup>, offsets: &[u32]) {
                self.set_bind_group(index, group, offsets);
            }

            fn debug(&mut self, op: u32, label: &str) {
                match op {
                    16 => self.push_debug_group(label),
                    17 => self.pop_debug_group(),
                    _ => self.insert_debug_marker(label),
                }
            }
        }
    )*};
}

pass!(wgpu::RenderPass<'_>, wgpu::ComputePass<'_>);

impl Pass for wgpu::RenderBundleEncoder<'_> {
    fn bind(&mut self, index: u32, group: Option<&wgpu::BindGroup>, offsets: &[u32]) {
        self.set_bind_group(index, group, offsets);
    }

    fn debug(&mut self, _: u32, _: &str) {}
}

fn shared(s: &State, device: Id, op: u32, r: &mut Reader, strings: &[String], pass: &mut impl Pass) -> bool {
    match op {
        2 => {
            let (index, id, n) = (r.u32(), r.u32(), r.u32());
            let offsets: Vec<u32> = (0..n).map(|_| r.u32()).collect();
            if id == 0 {
                pass.bind(index, None, &[]);
            } else if let Some(g) = s.bind_group(device, id) {
                pass.bind(index, Some(&g.group), &offsets);
            }
        }
        16 | 18 => pass.debug(op, strings.get(r.u32() as usize).map_or("", String::as_str)),
        17 => pass.debug(op, ""),
        _ => return false,
    }
    true
}

fn timestamps<'a, T>(
    s: &'a State,
    device: Id,
    t: &Option<desc::TimestampWritesDesc>,
    make: impl FnOnce(&'a wgpu::QuerySet, Option<u32>, Option<u32>) -> T,
) -> Option<Option<T>> {
    let Some(t) = t else { return Some(None) };
    let q = s.query_set(device, t.query_set)?;
    Some(Some(make(&q.set, t.beginning_of_pass_write_index, t.end_of_pass_write_index)))
}

fn common<'a>(s: &'a State, device: Id, op: u32, r: &mut Reader, pass: &mut impl RenderEncoder<'a>) -> bool {
    match op {
        1 => {
            if let Some(p) = s.render_pipeline(device, r.u32()) {
                pass.set_pipeline(p);
            }
        }
        3 => {
            let (slot, id, offset, size) = (r.u32(), r.u32(), r.u64(), r.size());
            if id == 0 {
                pass.set_vertex_buffer(slot, None);
            } else if let Some(b) = s.buffer(device, id) {
                let end = size.map_or(b.buffer.size(), |size| offset.saturating_add(size.get()));
                if offset > end || end > b.buffer.size() {
                    report(device, Kind::Validation, format!("setVertexBuffer: range {offset}..{end} is outside the buffer"));
                } else {
                    pass.set_vertex_buffer(slot, Some(b.buffer.slice(offset..end)));
                }
            }
        }
        4 => {
            let (id, format, offset, size) = (r.u32(), r.u32(), r.u64(), r.size());
            let format = if format == 1 { wgpu::IndexFormat::Uint32 } else { wgpu::IndexFormat::Uint16 };
            if let Some(b) = s.buffer(device, id) {
                let end = size.map_or(b.buffer.size(), |size| offset.saturating_add(size.get()));
                if offset > end || end > b.buffer.size() {
                    report(device, Kind::Validation, format!("setIndexBuffer: range {offset}..{end} is outside the buffer"));
                } else {
                    pass.set_index_buffer(b.buffer.slice(offset..end), format);
                }
            }
        }
        5 => {
            let (vertices, instances, first, first_instance) = (r.u32(), r.u32(), r.u32(), r.u32());
            pass.draw(first..first.saturating_add(vertices), first_instance..first_instance.saturating_add(instances));
        }
        6 => {
            let (indices, instances, first, base) = (r.u32(), r.u32(), r.u32(), r.next().unwrap_or(0.0) as i32);
            let first_instance = r.u32();
            pass.draw_indexed(first..first.saturating_add(indices), base, first_instance..first_instance.saturating_add(instances));
        }
        7 | 8 => {
            let (id, offset) = (r.u32(), r.u64());
            if let Some(b) = s.buffer(device, id) {
                if op == 7 {
                    pass.draw_indirect(&b.buffer, offset);
                } else {
                    pass.draw_indexed_indirect(&b.buffer, offset);
                }
            }
        }
        _ => return false,
    }
    true
}

fn touch_bind_groups(s: &mut State, encoder: &mut wgpu::CommandEncoder, commands: &[f64]) {
    let mut r = Reader { commands, at: 0 };
    let mut textures = Vec::new();
    while let Some(op) = r.next() {
        match op as u32 {
            2 => {
                let (_, id, n) = (r.u32(), r.u32(), r.u32());
                r.at += n as usize;
                if let Some(super::Obj::BindGroup(g)) = s.objects.get(&id) {
                    textures.extend_from_slice(&g.canvas_textures);
                }
            }
            op => r.at += operands(op, &r),
        }
    }
    for texture in textures {
        super::canvas::touch(s, encoder, texture, false);
    }
}

fn operands(op: u32, r: &Reader) -> usize {
    match op {
        1 | 12 | 13 | 16 | 18 => 1,
        7 | 8 | 20 => 2,
        19 => 3,
        3 | 4 | 5 | 10 | 11 => 4,
        6 => 5,
        9 => 6,
        15 => 1 + r.commands.get(r.at).copied().unwrap_or(0.0).max(0.0) as usize,
        _ => 0,
    }
}

pub fn render(s: &mut State, encoder: &mut wgpu::CommandEncoder, device: Id, d: &desc::RenderPassDesc, commands: &[f64], strings: &[String]) {
    for attachment in d.color_attachments.iter().flatten() {
        if let Some(super::Obj::View(v)) = s.objects.get(&attachment.view) {
            let texture = v.texture;
            super::canvas::touch(s, encoder, texture, attachment.load_op == desc::LoadOp::Clear);
        }
        if let Some(super::Obj::View(v)) = attachment.resolve_target.and_then(|id| s.objects.get(&id)) {
            let texture = v.texture;
            super::canvas::touch(s, encoder, texture, true);
        }
    }
    touch_bind_groups(s, encoder, commands);
    let s = &*s;
    let mut colors = Vec::with_capacity(d.color_attachments.len());
    for attachment in &d.color_attachments {
        let Some(a) = attachment else {
            colors.push(None);
            continue;
        };
        let Some(view) = s.view(device, a.view) else { return };
        let Some(resolve_target) = maybe(a.resolve_target, |id| s.view(device, id).map(|v| &v.view)) else { return };
        let [r, g, b, alpha] = a.clear_value;
        let ops = desc::ops(a.load_op, a.store_op, wgpu::Color { r, g, b, a: alpha });
        colors.push(Some(wgpu::RenderPassColorAttachment { view: &view.view, depth_slice: a.depth_slice, resolve_target, ops }));
    }
    let depth_stencil = match &d.depth_stencil_attachment {
        Some(a) => {
            let Some(view) = s.view(device, a.view) else { return };
            Some(wgpu::RenderPassDepthStencilAttachment {
                view: &view.view,
                depth_ops: a
                    .depth_load_op
                    .zip(a.depth_store_op)
                    .filter(|_| !a.depth_read_only)
                    .map(|(l, o)| desc::ops(l, o, a.depth_clear_value.unwrap_or(0.0))),
                stencil_ops: a.stencil_load_op.zip(a.stencil_store_op).filter(|_| !a.stencil_read_only).map(|(l, o)| desc::ops(l, o, a.stencil_clear_value)),
            })
        }
        None => None,
    };
    let Some(occlusion_query_set) = maybe(d.occlusion_query_set, |id| s.query_set(device, id).map(|q| &q.set)) else { return };
    let Some(timestamp_writes) = timestamps(s, device, &d.timestamp_writes, |query_set, beginning_of_pass_write_index, end_of_pass_write_index| {
        wgpu::RenderPassTimestampWrites { query_set, beginning_of_pass_write_index, end_of_pass_write_index }
    }) else {
        return;
    };
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: desc::label(&d.label),
        color_attachments: &colors,
        depth_stencil_attachment: depth_stencil,
        timestamp_writes,
        occlusion_query_set,
        multiview_mask: None,
    });
    let mut r = Reader { commands, at: 0 };
    while !r.done() {
        let op = r.u32();
        if common(s, device, op, &mut r, &mut pass) || shared(s, device, op, &mut r, strings, &mut pass) {
            continue;
        }
        match op {
            9 => {
                let (x, y, w, h, min, max) = (r.f32(), r.f32(), r.f32(), r.f32(), r.f32(), r.f32());
                pass.set_viewport(x, y, w, h, min, max);
            }
            10 => {
                let (x, y, w, h) = (r.u32(), r.u32(), r.u32(), r.u32());
                pass.set_scissor_rect(x, y, w, h);
            }
            11 => {
                let (red, g, b, a) = (r.next().unwrap_or(0.0), r.next().unwrap_or(0.0), r.next().unwrap_or(0.0), r.next().unwrap_or(0.0));
                pass.set_blend_constant(wgpu::Color { r: red, g, b, a });
            }
            12 => pass.set_stencil_reference(r.u32()),
            13 => pass.begin_occlusion_query(r.u32()),
            14 => pass.end_occlusion_query(),
            15 => {
                let n = r.u32();
                let bundles: Vec<&wgpu::RenderBundle> = (0..n).filter_map(|_| s.render_bundle(device, r.u32())).collect();
                pass.execute_bundles(bundles);
            }
            other => {
                report(device, Kind::Validation, format!("render pass: unknown command {other}"));
                break;
            }
        }
    }
}

pub fn compute(s: &mut State, encoder: &mut wgpu::CommandEncoder, device: Id, d: &desc::ComputePassDesc, commands: &[f64], strings: &[String]) {
    touch_bind_groups(s, encoder, commands);
    let s = &*s;
    let Some(timestamp_writes) = timestamps(s, device, &d.timestamp_writes, |query_set, beginning_of_pass_write_index, end_of_pass_write_index| {
        wgpu::ComputePassTimestampWrites { query_set, beginning_of_pass_write_index, end_of_pass_write_index }
    }) else {
        return;
    };
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: desc::label(&d.label), timestamp_writes });
    let mut r = Reader { commands, at: 0 };
    while !r.done() {
        let op = r.u32();
        if shared(s, device, op, &mut r, strings, &mut pass) {
            continue;
        }
        match op {
            1 => {
                if let Some(p) = s.compute_pipeline(device, r.u32()) {
                    pass.set_pipeline(p);
                }
            }
            19 => {
                let (x, y, z) = (r.u32(), r.u32(), r.u32());
                pass.dispatch_workgroups(x, y, z);
            }
            20 => {
                let (id, offset) = (r.u32(), r.u64());
                if let Some(b) = s.buffer(device, id) {
                    pass.dispatch_workgroups_indirect(&b.buffer, offset);
                }
            }
            other => {
                report(device, Kind::Validation, format!("compute pass: unknown command {other}"));
                break;
            }
        }
    }
}

pub fn bundle(s: &State, gpu: &crate::gpu::Gpu, device: Id, d: &desc::RenderBundleDesc, commands: &[f64], strings: &[String]) -> Option<wgpu::RenderBundle> {
    let mut encoder = gpu.device.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
        label: desc::label(&d.label),
        color_formats: &d.color_formats,
        depth_stencil: d.depth_stencil_format.map(|format| wgpu::RenderBundleDepthStencil {
            format,
            depth_read_only: d.depth_read_only,
            stencil_read_only: d.stencil_read_only,
        }),
        sample_count: d.sample_count,
        multiview: None,
    });
    let mut r = Reader { commands, at: 0 };
    while !r.done() {
        let op = r.u32();
        if !common(s, device, op, &mut r, &mut encoder) && !shared(s, device, op, &mut r, strings, &mut encoder) {
            report(device, Kind::Validation, format!("render bundle: unknown command {op}"));
            return None;
        }
    }
    Some(encoder.finish(&wgpu::RenderBundleDescriptor { label: desc::label(&d.label) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_render_pass_descriptor_comes_ahead_of_the_commands() {
        let strings = vec!["sky".to_owned()];
        let commands = [
            0.0,
            2.0,
            1.0,
            11.0,
            -1.0,
            0.0,
            0.25,
            0.5,
            0.75,
            1.0,
            0.0,
            1.0,
            0.0,
            1.0,
            12.0,
            f64::NAN,
            1.0,
            -1.0,
            1.0,
            3.0,
            -1.0,
            0.0,
            0.0,
            7.0,
            1.0,
            9.0,
            0.0,
            -1.0,
            5.0,
            3.0,
            1.0,
            0.0,
            0.0,
        ];
        let (d, at) = render_desc(&commands, &strings);
        assert_eq!(d.label.as_deref(), Some("sky"));
        assert_eq!(d.color_attachments.len(), 2);
        let color = d.color_attachments[0].as_ref().unwrap();
        assert_eq!((color.view, color.depth_slice, color.resolve_target, color.clear_value), (11, None, None, [0.25, 0.5, 0.75, 1.0]));
        assert!(color.load_op == desc::LoadOp::Clear && matches!(color.store_op, desc::StoreOp::Discard));
        assert!(d.color_attachments[1].is_none());
        let depth = d.depth_stencil_attachment.as_ref().unwrap();
        assert_eq!((depth.view, depth.depth_clear_value, depth.depth_read_only, depth.stencil_clear_value), (12, None, true, 3));
        assert!(depth.depth_load_op == Some(desc::LoadOp::Load) && depth.depth_store_op.is_none() && depth.stencil_load_op.is_none());
        assert_eq!(d.occlusion_query_set, Some(7));
        let t = d.timestamp_writes.as_ref().unwrap();
        assert_eq!((t.query_set, t.beginning_of_pass_write_index, t.end_of_pass_write_index), (9, Some(0), None));
        assert_eq!(&commands[at..], &[5.0, 3.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_short_descriptor_reads_as_defaults_and_does_not_run_past_the_end() {
        let (d, at) = render_desc(&[-1.0, 1e9], &[]);
        assert!(d.label.is_none() && d.color_attachments.is_empty() && d.depth_stencil_attachment.is_none());
        assert_eq!(at, 2);
        let (c, at) = compute_desc(&[-1.0, 0.0, 19.0, 1.0, 1.0, 1.0], &[]);
        assert!(c.label.is_none() && c.timestamp_writes.is_none());
        assert_eq!(at, 2);
    }
}
