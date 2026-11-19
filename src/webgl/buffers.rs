use wgpu::util::DeviceExt;

use crate::webgl::consts::*;
use crate::webgl::context::Context;
use crate::webgl::objects::*;

const USAGE: wgpu::BufferUsages = wgpu::BufferUsages::COPY_SRC
    .union(wgpu::BufferUsages::COPY_DST)
    .union(wgpu::BufferUsages::VERTEX)
    .union(wgpu::BufferUsages::INDEX)
    .union(wgpu::BufferUsages::UNIFORM)
    .union(wgpu::BufferUsages::STORAGE);

const TAIL: u64 = 256;

impl Context {
    pub fn create_buffer(&mut self) -> Id {
        self.add(Object::Buffer(Buffer::new()))
    }

    fn buffer_slot(&mut self, target: u32) -> Option<&mut Option<Id>> {
        let webgl2 = self.webgl2;
        Some(match target {
            ARRAY_BUFFER => &mut self.state.array_buffer,
            ELEMENT_ARRAY_BUFFER => &mut self.vao_mut().element,
            COPY_READ_BUFFER if webgl2 => &mut self.state.copy_read,
            COPY_WRITE_BUFFER if webgl2 => &mut self.state.copy_write,
            PIXEL_PACK_BUFFER if webgl2 => &mut self.state.pixel_pack,
            PIXEL_UNPACK_BUFFER if webgl2 => &mut self.state.pixel_unpack,
            UNIFORM_BUFFER if webgl2 => &mut self.state.uniform_buffer,
            TRANSFORM_FEEDBACK_BUFFER if webgl2 => &mut self.state.transform_feedback_buffer,
            _ => return None,
        })
    }

    pub(crate) fn bound_buffer(&mut self, target: u32) -> Option<Option<Id>> {
        self.buffer_slot(target).map(|s| *s)
    }

    fn element_compatible(&mut self, id: Id, target: u32) -> bool {
        let element = target == ELEMENT_ARRAY_BUFFER;
        let Some(b) = self.buffer_mut(id) else { return false };
        match b.element {
            None => {
                b.element = Some(element);
                if element {
                    b.shadow = Some(Vec::new());
                }
                true
            }
            Some(true) => element || matches!(target, COPY_READ_BUFFER | COPY_WRITE_BUFFER),
            Some(false) => !element,
        }
    }

    pub fn bind_buffer(&mut self, target: u32, id: Option<Id>) {
        if self.buffer_slot(target).is_none() {
            return self.error(INVALID_ENUM);
        }
        if let Some(id) = id {
            if self.buffer(id).is_none() {
                return self.error(INVALID_OPERATION);
            }
            if !self.element_compatible(id, target) {
                return self.error(INVALID_OPERATION);
            }
        }
        if let Some(slot) = self.buffer_slot(target) {
            *slot = id;
        }
    }

    pub fn bind_buffer_range(&mut self, target: u32, index: u32, id: Option<Id>, offset: u64, size: Option<u64>) {
        if target != UNIFORM_BUFFER && target != TRANSFORM_FEEDBACK_BUFFER {
            return self.error(INVALID_ENUM);
        }
        if target == UNIFORM_BUFFER && index as usize >= self.state.uniform_buffers.len() {
            return self.error(INVALID_VALUE);
        }
        if let Some(id) = id {
            if self.buffer(id).is_none() || !self.element_compatible(id, target) {
                return self.error(INVALID_OPERATION);
            }
            if target == UNIFORM_BUFFER && !offset.is_multiple_of(256) {
                return self.error(INVALID_VALUE);
            }
            if size == Some(0) {
                return self.error(INVALID_VALUE);
            }
        }
        if target == UNIFORM_BUFFER {
            self.state.uniform_buffers[index as usize] = crate::webgl::state::IndexedBuffer { buffer: id, offset, size: size.unwrap_or(0) };
            self.state.uniform_buffer = id;
        } else {
            self.state.transform_feedback_buffer = id;
        }
    }

    pub fn delete_buffer(&mut self, id: Id) {
        if self.buffer(id).is_none() {
            return;
        }
        let s = &mut self.state;
        for slot in [
            &mut s.array_buffer,
            &mut s.copy_read,
            &mut s.copy_write,
            &mut s.pixel_pack,
            &mut s.pixel_unpack,
            &mut s.uniform_buffer,
            &mut s.transform_feedback_buffer,
        ] {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        for b in &mut s.uniform_buffers {
            if b.buffer == Some(id) {
                b.buffer = None;
            }
        }
        let vao = self.vao_mut();
        if vao.element == Some(id) {
            vao.element = None;
        }
        for a in &mut vao.attribs {
            if a.buffer == Some(id) {
                a.buffer = None;
            }
        }
        let referenced =
            self.objects.values().any(|o| o.vertex_array().is_some_and(|v| v.element == Some(id) || v.attribs.iter().any(|a| a.buffer == Some(id))))
                || self.default_vao.attribs.iter().any(|a| a.buffer == Some(id));
        if !referenced {
            self.objects.remove(&id);
        }
    }

    pub fn buffer_data(&mut self, target: u32, data: Option<&[u8]>, size: u64, usage: u32) {
        if !matches!(usage, STREAM_DRAW | STATIC_DRAW | DYNAMIC_DRAW)
            && !(self.webgl2 && matches!(usage, STREAM_READ | STREAM_COPY | STATIC_READ | STATIC_COPY | DYNAMIC_READ | DYNAMIC_COPY))
        {
            return self.error(INVALID_ENUM);
        }
        let Some(bound) = self.bound_buffer(target) else { return self.error(INVALID_ENUM) };
        let Some(id) = bound else { return self.error(INVALID_OPERATION) };
        let size = data.map_or(size, |d| d.len() as u64);
        if size > (1u64 << 31) {
            return self.error(OUT_OF_MEMORY);
        }
        let device = self.gpu.device.clone();
        let raw = (size > 0).then(|| {
            let alloc = size.next_multiple_of(4) + TAIL;
            match data {
                Some(d) => {
                    let mut padded;
                    let contents = if d.len() as u64 == alloc {
                        d
                    } else {
                        padded = d.to_vec();
                        padded.resize(alloc as usize, 0);
                        &padded[..]
                    };
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("acde webgl buffer"), contents, usage: USAGE })
                }
                None => {
                    device.create_buffer(&wgpu::BufferDescriptor { label: Some("acde webgl buffer"), size: alloc, usage: USAGE, mapped_at_creation: false })
                }
            }
        });
        let Some(b) = self.buffer_mut(id) else { return };
        b.raw = raw;
        b.size = size;
        b.usage = usage;
        b.version += 1;
        b.used = 0;
        b.ranges.clear();
        if let Some(shadow) = b.shadow.as_mut() {
            shadow.clear();
            match data {
                Some(d) => shadow.extend_from_slice(d),
                None => shadow.resize(size as usize, 0),
            }
        }
    }

    pub fn buffer_sub_data(&mut self, target: u32, offset: u64, data: &[u8]) {
        let Some(bound) = self.bound_buffer(target) else { return self.error(INVALID_ENUM) };
        let Some(id) = bound else { return self.error(INVALID_OPERATION) };
        let Some(b) = self.buffer(id) else { return self.error(INVALID_OPERATION) };
        if offset.checked_add(data.len() as u64).is_none_or(|end| end > b.size) {
            return self.error(INVALID_VALUE);
        }
        if data.is_empty() {
            return;
        }
        self.write_buffer(id, offset, data);
    }

    pub(crate) fn write_buffer(&mut self, id: Id, offset: u64, data: &[u8]) {
        let generation = self.generation;
        let Some(b) = self.buffer_mut(id) else { return };
        b.ranges.clear();
        if let Some(shadow) = b.shadow.as_mut() {
            shadow[offset as usize..offset as usize + data.len()].copy_from_slice(data);
        }
        let Some(raw) = b.raw.clone() else { return };
        let ordered = b.used == generation;
        let in_flight = b.used + 1 == generation;
        let end = offset + data.len() as u64;
        let (start, stop) = (offset & !3, end.next_multiple_of(4));
        let aligned: std::borrow::Cow<[u8]> = if start == offset && stop == end {
            data.into()
        } else if let Some(shadow) = b.shadow.as_ref() {
            let mut v = shadow[start as usize..(stop as usize).min(shadow.len())].to_vec();
            v.resize((stop - start) as usize, 0);
            v.into()
        } else {
            b.used = generation;
            b.version += 1;
            return self.patch_buffer(&raw, offset, data);
        };
        b.version += 1;
        if ordered {
            self.flush_pass();
            let device = self.gpu.device.clone();
            let staging = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("acde webgl upload"),
                contents: &aligned,
                usage: wgpu::BufferUsages::COPY_SRC,
            });
            self.encoder().copy_buffer_to_buffer(&staging, 0, &raw, start, stop - start);
        } else {
            if in_flight {
                self.settle();
            }
            self.gpu.queue.write_buffer(&raw, start, &aligned);
        }
    }

    pub fn copy_buffer_sub_data(&mut self, read: u32, write: u32, read_offset: u64, write_offset: u64, size: u64) {
        let (Some(r), Some(w)) = (self.bound_buffer(read), self.bound_buffer(write)) else { return self.error(INVALID_ENUM) };
        let (Some(r), Some(w)) = (r, w) else { return self.error(INVALID_OPERATION) };
        let (Some(rb), Some(wb)) = (self.buffer(r), self.buffer(w)) else { return self.error(INVALID_OPERATION) };
        if rb.element.is_some() && wb.element.is_some() && rb.element != wb.element {
            return self.error(INVALID_OPERATION);
        }
        if read_offset.checked_add(size).is_none_or(|e| e > rb.size) || write_offset.checked_add(size).is_none_or(|e| e > wb.size) {
            return self.error(INVALID_VALUE);
        }
        if r == w && read_offset < write_offset + size && write_offset < read_offset + size {
            return self.error(INVALID_VALUE);
        }
        if size == 0 {
            return;
        }
        let (Some(src), Some(dst)) = (rb.raw.clone(), wb.raw.clone()) else { return };
        if let Some(shadow) = rb.shadow.clone()
            && let Some(ws) = self.buffer_mut(w).and_then(|b| b.shadow.as_mut())
        {
            ws[write_offset as usize..(write_offset + size) as usize].copy_from_slice(&shadow[read_offset as usize..(read_offset + size) as usize]);
        }
        let generation = self.generation;
        if let Some(b) = self.buffer_mut(w) {
            b.used = generation;
            b.version += 1;
            b.ranges.clear();
        }
        if let Some(b) = self.buffer_mut(r) {
            b.used = generation;
        }
        self.flush_pass();
        if read_offset.is_multiple_of(4) && write_offset.is_multiple_of(4) && size.is_multiple_of(4) {
            self.encoder().copy_buffer_to_buffer(&src, read_offset, &dst, write_offset, size);
        } else {
            self.copy_bytes(&src, read_offset, &dst, write_offset, size);
        }
    }

    pub fn get_buffer_sub_data(&mut self, target: u32, offset: u64, out: &mut [u8]) {
        let Some(bound) = self.bound_buffer(target) else { return self.error(INVALID_ENUM) };
        let Some(id) = bound else { return self.error(INVALID_OPERATION) };
        let Some(b) = self.buffer(id) else { return self.error(INVALID_OPERATION) };
        if offset.checked_add(out.len() as u64).is_none_or(|e| e > b.size) {
            return self.error(INVALID_VALUE);
        }
        if out.is_empty() {
            return;
        }
        if let Some(shadow) = &b.shadow {
            out.copy_from_slice(&shadow[offset as usize..offset as usize + out.len()]);
            return;
        }
        let Some(raw) = b.raw.clone() else { return };
        let start = offset & !3;
        let stop = (offset + out.len() as u64).next_multiple_of(4);
        match self.read_back(&raw, start, stop - start) {
            Some(bytes) => out.copy_from_slice(&bytes[(offset - start) as usize..(offset - start) as usize + out.len()]),
            None => self.error(OUT_OF_MEMORY),
        }
    }

    pub(crate) fn read_back(&mut self, raw: &wgpu::Buffer, offset: u64, size: u64) -> Option<Vec<u8>> {
        let device = self.gpu.device.clone();
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acde webgl read"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        self.flush_pass();
        self.encoder().copy_buffer_to_buffer(raw, offset, &staging, 0, size);
        let index = self.submit()?;
        crate::webgl::read::map(&self.gpu, &staging, index)
    }

    pub(crate) fn max_index(&mut self, id: Id, ty: u32, offset: u64, count: u32, restart: bool) -> Option<u32> {
        let b = self.buffer_mut(id)?;
        let key = (ty | u32::from(restart) << 31, offset, count);
        if let Some(&m) = b.ranges.get(&key) {
            return Some(m);
        }
        let shadow = b.shadow.as_ref()?;
        let size = match ty {
            UNSIGNED_BYTE => 1,
            UNSIGNED_SHORT => 2,
            _ => 4,
        };
        let start = offset as usize;
        let end = start.checked_add(count as usize * size)?;
        let bytes = shadow.get(start..end)?;
        let max = match size {
            1 => bytes.iter().filter(|&&b| !restart || b != 0xff).map(|&b| u32::from(b)).max(),
            2 => bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).filter(|&i| !restart || i != 0xffff).map(u32::from).max(),
            _ => bytes.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).filter(|&i| !restart || i != u32::MAX).max(),
        }
        .unwrap_or(0);
        b.ranges.insert(key, max);
        Some(max)
    }

    pub(crate) fn element_bytes(&self, id: Id) -> Option<&[u8]> {
        self.buffer(id)?.shadow.as_deref()
    }
}
