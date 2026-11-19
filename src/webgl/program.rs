use std::collections::HashMap;
use std::rc::Rc;

use naga::ShaderStage;

use crate::webgl::consts::*;
use crate::webgl::context::Context;
use crate::webgl::objects::*;
use crate::webgl::shader::{self, Uniform};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Setter {
    Float(u32),
    Int(u32),
    Uint(u32),
    Matrix(u32, u32),
}

pub enum Value {
    Floats(Vec<f32>),
    Ints(Vec<i32>),
    Uints(Vec<u32>),
    Bools(Vec<bool>),
}

pub type BlockInfo = (String, u32, u32, u32, Vec<u32>, bool, bool);

pub struct ActiveInfo {
    pub name: String,
    pub size: u32,
    pub ty: u32,
}

fn components(ty: u32) -> (u32, u32) {
    match ty {
        FLOAT | INT | UNSIGNED_INT | BOOL => (1, 1),
        FLOAT_VEC2 | INT_VEC2 | UNSIGNED_INT_VEC2 | BOOL_VEC2 => (1, 2),
        FLOAT_VEC3 | INT_VEC3 | UNSIGNED_INT_VEC3 | BOOL_VEC3 => (1, 3),
        FLOAT_VEC4 | INT_VEC4 | UNSIGNED_INT_VEC4 | BOOL_VEC4 => (1, 4),
        FLOAT_MAT2 => (2, 2),
        FLOAT_MAT3 => (3, 3),
        FLOAT_MAT4 => (4, 4),
        FLOAT_MAT2X3 => (2, 3),
        FLOAT_MAT2X4 => (2, 4),
        FLOAT_MAT3X2 => (3, 2),
        FLOAT_MAT3X4 => (3, 4),
        FLOAT_MAT4X2 => (4, 2),
        FLOAT_MAT4X3 => (4, 3),
        _ => (1, 1),
    }
}

fn base_kind(ty: u32) -> u8 {
    match ty {
        FLOAT | FLOAT_VEC2 | FLOAT_VEC3 | FLOAT_VEC4 => 0,
        INT | INT_VEC2 | INT_VEC3 | INT_VEC4 => 1,
        UNSIGNED_INT | UNSIGNED_INT_VEC2 | UNSIGNED_INT_VEC3 | UNSIGNED_INT_VEC4 => 2,
        BOOL | BOOL_VEC2 | BOOL_VEC3 | BOOL_VEC4 => 3,
        _ => 4,
    }
}

impl Context {
    pub fn create_shader(&mut self, kind: u32) -> Option<Id> {
        if !matches!(kind, VERTEX_SHADER | FRAGMENT_SHADER) {
            self.error(INVALID_ENUM);
            return None;
        }
        Some(self.add(Object::Shader(Shader {
            kind,
            source: String::new(),
            compiled: None,
            log: String::new(),
            compiled_ok: false,
            deleted: false,
            attached: 0,
        })))
    }

    pub fn shader_source(&mut self, id: Id, source: &str) {
        match self.shader_mut(id) {
            Some(s) => s.source = source.to_owned(),
            None => self.error(INVALID_OPERATION),
        }
    }

    pub fn get_shader_source(&mut self, id: Id) -> Option<String> {
        self.shader(id).map(|s| s.source.clone())
    }

    fn extension_macros(&self, kind: u32) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.webgl2 {
            return out;
        }
        let map = [
            ("OES_standard_derivatives", "GL_OES_standard_derivatives"),
            ("EXT_shader_texture_lod", "GL_EXT_shader_texture_lod"),
            ("EXT_frag_depth", "GL_EXT_frag_depth"),
            ("WEBGL_draw_buffers", "GL_EXT_draw_buffers"),
        ];
        for (ext, macro_name) in map {
            if self.extension(ext) && (kind == FRAGMENT_SHADER || ext == "OES_standard_derivatives") {
                out.push(macro_name);
            }
        }
        out
    }

    pub fn compile_shader(&mut self, id: Id) {
        let Some(s) = self.shader(id) else { return self.error(INVALID_OPERATION) };
        let (kind, source) = (s.kind, s.source.clone());
        let stage = if kind == VERTEX_SHADER { ShaderStage::Vertex } else { ShaderStage::Fragment };
        let macros = self.extension_macros(kind);
        let varyings = self.limits.varyings;
        let version = shader::version(&source);
        let result = if version == 300 && !self.webgl2 {
            Err("ERROR: 0:1: '300' : version number not supported\n".to_owned())
        } else {
            std::panic::catch_unwind(|| shader::compile(stage, &source, &macros, varyings)).unwrap_or_else(|_| Err("ERROR: internal compiler error\n".into()))
        };
        let Some(s) = self.shader_mut(id) else { return };
        match result {
            Ok(c) => {
                s.compiled = Some(Rc::new(c));
                s.compiled_ok = true;
                s.log.clear();
            }
            Err(log) => {
                s.compiled = None;
                s.compiled_ok = false;
                s.log = log;
            }
        }
    }

    pub fn shader_info_log(&mut self, id: Id) -> Option<String> {
        match self.shader(id) {
            Some(s) => Some(s.log.clone()),
            None => {
                self.error(INVALID_VALUE);
                None
            }
        }
    }

    pub fn delete_shader(&mut self, id: Id) {
        let Some(s) = self.shader_mut(id) else { return };
        s.deleted = true;
        if s.attached == 0 {
            self.objects.remove(&id);
        }
    }

    pub fn create_program(&mut self) -> Id {
        self.add(Object::Program(Program {
            vertex: None,
            fragment: None,
            bound_attributes: HashMap::new(),
            linked: None,
            link_ok: false,
            log: String::new(),
            validated: false,
            deleted: false,
            data: [Vec::new(), Vec::new()],
            data_version: 0,
            units: Vec::new(),
            block_bindings: Vec::new(),
            transform_feedback: (Vec::new(), INTERLEAVED_ATTRIBS),
        }))
    }

    pub fn attach_shader(&mut self, program: Id, shader: Id) {
        let Some(kind) = self.shader(shader).map(|s| s.kind) else { return self.error(INVALID_OPERATION) };
        let Some(p) = self.program_mut(program) else { return self.error(INVALID_OPERATION) };
        let slot = if kind == VERTEX_SHADER { &mut p.vertex } else { &mut p.fragment };
        if slot.is_some() {
            return self.error(INVALID_OPERATION);
        }
        *slot = Some(shader);
        if let Some(s) = self.shader_mut(shader) {
            s.attached += 1;
        }
    }

    pub fn detach_shader(&mut self, program: Id, shader: Id) {
        let Some(p) = self.program_mut(program) else { return self.error(INVALID_OPERATION) };
        let slot = if p.vertex == Some(shader) {
            &mut p.vertex
        } else if p.fragment == Some(shader) {
            &mut p.fragment
        } else {
            return self.error(INVALID_OPERATION);
        };
        *slot = None;
        if let Some(s) = self.shader_mut(shader) {
            s.attached = s.attached.saturating_sub(1);
            if s.deleted && s.attached == 0 {
                self.objects.remove(&shader);
            }
        }
    }

    pub fn attached_shaders(&self, program: Id) -> Vec<Id> {
        self.program(program).map(|p| p.vertex.into_iter().chain(p.fragment).collect()).unwrap_or_default()
    }

    pub fn bind_attrib_location(&mut self, program: Id, index: u32, name: &str) {
        if index >= self.limits.attributes {
            return self.error(INVALID_VALUE);
        }
        if name.starts_with("webgl_") || name.starts_with("_webgl_") || name.starts_with("gl_") {
            return self.error(INVALID_OPERATION);
        }
        match self.program_mut(program) {
            Some(p) => {
                p.bound_attributes.insert(name.to_owned(), index);
            }
            None => self.error(INVALID_OPERATION),
        }
    }

    pub fn link_program(&mut self, program: Id) {
        let Some(p) = self.program(program) else { return self.error(INVALID_OPERATION) };
        let shaders =
            (p.vertex.and_then(|v| self.shader(v)).and_then(|s| s.compiled.clone()), p.fragment.and_then(|f| self.shader(f)).and_then(|s| s.compiled.clone()));
        let bound = p.bound_attributes.clone();
        let limits = shader::Limits {
            attributes: self.limits.attributes,
            varyings: self.limits.varyings,
            draw_buffers: self.limits.draw_buffers,
            textures: self.limits.units * 2,
        };
        let result = match shaders {
            (Some(vs), Some(fs)) => {
                std::panic::catch_unwind(|| shader::link(&vs, &fs, &bound, &limits)).unwrap_or_else(|_| Err("internal linker error".into()))
            }
            _ => Err("a program needs a compiled vertex and fragment shader".into()),
        };
        let result = result.and_then(|info| {
            for (stage, size) in info.uniform_sizes.iter().enumerate() {
                if *size > self.limits.block_size {
                    return Err(format!("{} uniforms take {size} bytes, more than {}", ["vertex", "fragment"][stage], self.limits.block_size));
                }
            }
            let modules = self.scoped(|device| {
                let module = |m: &naga::Module, label| {
                    device.create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some(label),
                        source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(m.clone())),
                    })
                };
                let points =
                    info.points.as_ref().map(|p| (module(&p.vertex, "acde webgl vertex (points)"), module(&p.fragment, "acde webgl fragment (points)")));
                (module(&info.vertex, "acde webgl vertex"), module(&info.fragment, "acde webgl fragment"), points)
            });
            modules.map(|(vertex, fragment, points)| (info, vertex, fragment, points))
        });
        let serial = self.next_serial();
        let Some(p) = self.program_mut(program) else { return };
        match result {
            Ok((info, vertex, fragment, points)) => {
                let mut locations = Vec::new();
                for (i, u) in info.uniforms.iter().enumerate() {
                    push_locations(&mut locations, &u.name, u.size, i);
                }
                let base = info.uniforms.len();
                for (i, s) in info.samplers.iter().enumerate() {
                    push_locations(&mut locations, &s.name, s.size, base + i);
                }
                p.data = [vec![0; info.uniform_sizes[0] as usize], vec![0; info.uniform_sizes[1] as usize]];
                p.units = info.samplers.iter().map(|s| vec![0; s.size as usize]).collect();
                p.block_bindings = vec![0; info.blocks.len()];
                p.data_version += 1;
                p.linked = Some(Rc::new(Linked { info, serial, vertex, fragment, points, locations }));
                p.link_ok = true;
                p.log.clear();
            }
            Err(log) => {
                p.linked = None;
                p.link_ok = false;
                p.log = log;
            }
        }
    }

    pub fn program_info_log(&mut self, id: Id) -> Option<String> {
        match self.program(id) {
            Some(p) => Some(p.log.clone()),
            None => {
                self.error(INVALID_VALUE);
                None
            }
        }
    }

    pub fn use_program(&mut self, id: Option<Id>) {
        if let Some(id) = id {
            match self.program(id) {
                None => return self.error(INVALID_OPERATION),
                Some(p) if !p.link_ok => return self.error(INVALID_OPERATION),
                _ => {}
            }
        }
        let old = self.state.program;
        self.state.program = id;
        if let Some(old) = old.filter(|o| Some(*o) != id)
            && self.program(old).is_some_and(|p| p.deleted)
        {
            self.release_program(old);
        }
    }

    pub fn delete_program(&mut self, id: Id) {
        let Some(p) = self.program_mut(id) else { return };
        p.deleted = true;
        if self.state.program != Some(id) {
            self.release_program(id);
        }
    }

    fn release_program(&mut self, id: Id) {
        let Some(Object::Program(p)) = self.objects.remove(&id) else { return };
        for s in p.vertex.into_iter().chain(p.fragment) {
            if let Some(sh) = self.shader_mut(s) {
                sh.attached = sh.attached.saturating_sub(1);
                if sh.deleted && sh.attached == 0 {
                    self.objects.remove(&s);
                }
            }
        }
    }

    pub fn validate_program(&mut self, id: Id) {
        match self.program_mut(id) {
            Some(p) => p.validated = p.link_ok,
            None => self.error(INVALID_OPERATION),
        }
    }

    pub(crate) fn linked(&self, program: Id) -> Option<Rc<Linked>> {
        self.program(program)?.linked.clone()
    }

    pub fn attrib_location(&mut self, program: Id, name: &str) -> i32 {
        let Some(p) = self.program(program) else {
            self.error(INVALID_OPERATION);
            return -1;
        };
        let Some(linked) = &p.linked else {
            self.error(INVALID_OPERATION);
            return -1;
        };
        linked.info.attributes.iter().find(|a| a.name == name).map_or(-1, |a| a.location as i32)
    }

    pub fn active_attrib(&mut self, program: Id, index: u32) -> Option<ActiveInfo> {
        let linked = self.linked(program)?;
        match linked.info.attributes.get(index as usize) {
            Some(a) => Some(ActiveInfo { name: a.name.clone(), size: 1, ty: a.gl_type }),
            None => {
                self.error(INVALID_VALUE);
                None
            }
        }
    }

    pub fn active_uniform(&mut self, program: Id, index: u32) -> Option<ActiveInfo> {
        let linked = self.linked(program)?;
        let info = &linked.info;
        let i = index as usize;
        if let Some(u) = info.uniforms.get(i) {
            return Some(ActiveInfo { name: u.name.clone(), size: u.size, ty: u.gl_type });
        }
        if let Some(s) = info.samplers.get(i - info.uniforms.len()) {
            return Some(ActiveInfo { name: s.name.clone(), size: s.size, ty: s.gl_type });
        }
        let mut k = i - info.uniforms.len() - info.samplers.len();
        for b in &info.blocks {
            if let Some(m) = b.members.get(k) {
                return Some(ActiveInfo { name: m.name.clone(), size: m.size, ty: m.gl_type });
            }
            k -= b.members.len();
        }
        self.error(INVALID_VALUE);
        None
    }

    pub fn active_uniform_count(&self, program: Id) -> u32 {
        self.linked(program)
            .map_or(0, |l| (l.info.uniforms.len() + l.info.samplers.len() + l.info.blocks.iter().map(|b| b.members.len()).sum::<usize>()) as u32)
    }

    pub fn uniform_location(&mut self, program: Id, name: &str) -> Option<u32> {
        let Some(p) = self.program(program) else {
            self.error(INVALID_OPERATION);
            return None;
        };
        let Some(linked) = &p.linked else {
            self.error(INVALID_OPERATION);
            return None;
        };
        linked.locations.iter().position(|(n, _)| n == name).map(|i| i as u32)
    }

    pub fn uniform(&mut self, program: Option<Id>, location: u32, serial: u64, setter: Setter, transpose: bool, data: &[u8]) {
        let Some(current) = self.state.program else { return self.error(INVALID_OPERATION) };
        if program != Some(current) {
            return self.error(INVALID_OPERATION);
        }
        let Some(linked) = self.linked(current) else { return self.error(INVALID_OPERATION) };
        if linked.serial != serial {
            return self.error(INVALID_OPERATION);
        }
        if transpose && !self.webgl2 {
            return self.error(INVALID_VALUE);
        }
        let Some((_, loc)) = linked.locations.get(location as usize) else { return self.error(INVALID_OPERATION) };
        let info = &linked.info;
        let units = self.state.units.len() as u32;
        if loc.uniform >= info.uniforms.len() {
            let index = loc.uniform - info.uniforms.len();
            let s = &info.samplers[index];
            if setter != Setter::Int(1) {
                return self.error(INVALID_OPERATION);
            }
            let values: Vec<i32> = data.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            if values.is_empty() || (values.len() > 1 && s.size == 1) {
                return self.error(INVALID_OPERATION);
            }
            if values.iter().any(|&v| v < 0 || v as u32 >= units) {
                return self.error(INVALID_VALUE);
            }
            let Some(p) = self.program_mut(current) else { return };
            for (k, v) in values.into_iter().enumerate() {
                if let Some(slot) = p.units[index].get_mut(loc.element as usize + k) {
                    *slot = v as u32;
                }
            }
            return;
        }
        let u: &Uniform = &info.uniforms[loc.uniform];
        let (columns, rows) = components(u.gl_type);
        let kind = base_kind(u.gl_type);
        let ok = match setter {
            Setter::Float(n) => columns == 1 && n == rows && (kind == 0 || kind == 3),
            Setter::Int(n) => columns == 1 && n == rows && (kind == 1 || kind == 3),
            Setter::Uint(n) => columns == 1 && n == rows && (kind == 2 || kind == 3),
            Setter::Matrix(c, r) => kind == 4 && c == columns && r == rows,
        };
        if !ok {
            return self.error(INVALID_OPERATION);
        }
        let per = (columns * rows * 4) as usize;
        if data.is_empty() || !data.len().is_multiple_of(per) {
            return self.error(INVALID_VALUE);
        }
        let count = data.len() / per;
        if count > 1 && u.size == 1 {
            return self.error(INVALID_OPERATION);
        }
        let stages = u.stages;
        let element = loc.element;
        let size = u.size;
        let Some(p) = self.program_mut(current) else { return };
        for (stage, slot) in stages.iter().enumerate() {
            let Some(slot) = slot else { continue };
            let bytes = &mut p.data[stage];
            for e in 0..count {
                let index = element + e as u32;
                if index >= size {
                    break;
                }
                let base = (slot.offset + index * slot.stride) as usize;
                let src = &data[e * per..(e + 1) * per];
                let column_stride = if rows == 2 && columns > 1 { 8 } else { 16 };
                for c in 0..columns as usize {
                    for r in 0..rows as usize {
                        let from = if transpose { (r * columns as usize + c) * 4 } else { (c * rows as usize + r) * 4 };
                        let mut word = [src[from], src[from + 1], src[from + 2], src[from + 3]];
                        if slot.boolean {
                            let truthy = match setter {
                                Setter::Float(_) => f32::from_le_bytes(word) != 0.0,
                                _ => u32::from_le_bytes(word) != 0,
                            };
                            word = u32::from(truthy).to_le_bytes();
                        }
                        let at = base + c * column_stride + r * 4;
                        if at + 4 <= bytes.len() {
                            bytes[at..at + 4].copy_from_slice(&word);
                        }
                    }
                }
            }
        }
        p.data_version += 1;
    }

    pub fn get_uniform(&mut self, program: Id, location: u32, serial: u64) -> Option<(u32, Value)> {
        let linked = self.linked(program)?;
        if linked.serial != serial {
            self.error(INVALID_OPERATION);
            return None;
        }
        let (_, loc) = linked.locations.get(location as usize)?;
        let p = self.program(program)?;
        let info = &linked.info;
        if loc.uniform >= info.uniforms.len() {
            let i = loc.uniform - info.uniforms.len();
            let v = *p.units[i].get(loc.element as usize)?;
            return Some((info.samplers[i].gl_type, Value::Ints(vec![v as i32])));
        }
        let u = &info.uniforms[loc.uniform];
        let (columns, rows) = components(u.gl_type);
        let kind = base_kind(u.gl_type);
        let (stage, slot) = u.stages.iter().enumerate().find_map(|(i, s)| s.map(|s| (i, s)))?;
        let bytes = &p.data[stage];
        let base = (slot.offset + loc.element * slot.stride) as usize;
        let column_stride = if rows == 2 && columns > 1 { 8 } else { 16 };
        let mut words = Vec::new();
        for c in 0..columns as usize {
            for r in 0..rows as usize {
                let at = base + c * column_stride + r * 4;
                words.push(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
            }
        }
        Some((
            u.gl_type,
            match kind {
                1 => Value::Ints(words.into_iter().map(|w| w as i32).collect()),
                2 => Value::Uints(words),
                3 => Value::Bools(words.into_iter().map(|w| w != 0).collect()),
                _ => Value::Floats(words.into_iter().map(f32::from_bits).collect()),
            },
        ))
    }

    pub fn current_program(&self) -> Option<Id> {
        self.state.program
    }

    pub fn active_attrib_count(&self, program: Id) -> u32 {
        self.linked(program).map_or(0, |l| l.info.attributes.len() as u32)
    }

    pub fn program_serial(&self, program: Id) -> u64 {
        self.linked(program).map_or(0, |l| l.serial)
    }

    pub fn uniform_block_index(&mut self, program: Id, name: &str) -> u32 {
        self.linked(program).and_then(|l| l.info.blocks.iter().position(|b| b.name == name)).map_or(INVALID_INDEX, |i| i as u32)
    }

    pub fn uniform_block_binding(&mut self, program: Id, index: u32, binding: u32) {
        if binding >= self.limits.uniform_bindings {
            return self.error(INVALID_VALUE);
        }
        let Some(p) = self.program_mut(program) else { return self.error(INVALID_OPERATION) };
        match p.block_bindings.get_mut(index as usize) {
            Some(b) => *b = binding,
            None => self.error(INVALID_VALUE),
        }
    }

    pub fn block_info(&self, program: Id, index: u32) -> Option<BlockInfo> {
        let linked = self.linked(program)?;
        let p = self.program(program)?;
        let b = linked.info.blocks.get(index as usize)?;
        let first = (linked.info.uniforms.len()
            + linked.info.samplers.len()
            + linked.info.blocks[..index as usize].iter().map(|b| b.members.len()).sum::<usize>()) as u32;
        let indices = (first..first + b.members.len() as u32).collect();
        Some((b.name.clone(), b.size, p.block_bindings[index as usize], b.members.len() as u32, indices, b.bindings[0].is_some(), b.bindings[1].is_some()))
    }

    pub fn block_member(&self, program: Id, index: u32) -> Option<(i32, u32, u32, u32, u32, u32)> {
        let linked = self.linked(program)?;
        let info = &linked.info;
        let i = index as usize;
        if let Some(u) = info.uniforms.get(i) {
            return Some((-1, u.gl_type, u.size, u32::MAX, 0, 0));
        }
        if let Some(s) = info.samplers.get(i - info.uniforms.len()) {
            return Some((-1, s.gl_type, s.size, u32::MAX, 0, 0));
        }
        let mut k = i - info.uniforms.len() - info.samplers.len();
        for (bi, b) in info.blocks.iter().enumerate() {
            if let Some(m) = b.members.get(k) {
                return Some((bi as i32, m.gl_type, m.size, m.offset, m.array_stride, m.matrix_stride));
            }
            k -= b.members.len();
        }
        None
    }

    pub fn uniform_indices(&self, program: Id, names: &[String]) -> Option<Vec<u32>> {
        let linked = self.linked(program)?;
        let count = self.active_uniform_count(program);
        Some(
            names
                .iter()
                .map(|n| {
                    (0..count)
                        .find(|&i| {
                            let info = &linked.info;
                            let name = if (i as usize) < info.uniforms.len() {
                                info.uniforms[i as usize].name.as_str()
                            } else if (i as usize) < info.uniforms.len() + info.samplers.len() {
                                info.samplers[i as usize - info.uniforms.len()].name.as_str()
                            } else {
                                let mut k = i as usize - info.uniforms.len() - info.samplers.len();
                                let mut found = "";
                                for b in &info.blocks {
                                    if let Some(m) = b.members.get(k) {
                                        found = m.name.as_str();
                                        break;
                                    }
                                    k -= b.members.len();
                                }
                                found
                            };
                            name == n || name.strip_suffix("[0]") == Some(n)
                        })
                        .unwrap_or(INVALID_INDEX)
                })
                .collect(),
        )
    }

    pub fn frag_data_location(&self, program: Id, name: &str) -> i32 {
        let Some(linked) = self.linked(program) else { return -1 };
        if name.starts_with("gl_") {
            return -1;
        }
        linked.info.outputs.iter().find(|o| o.name == name).map_or(-1, |o| o.location as i32)
    }

    pub fn transform_feedback_varyings(&mut self, program: Id, varyings: Vec<String>, mode: u32) {
        if !matches!(mode, INTERLEAVED_ATTRIBS | SEPARATE_ATTRIBS) {
            return self.error(INVALID_ENUM);
        }
        match self.program_mut(program) {
            Some(p) => p.transform_feedback = (varyings, mode),
            None => self.error(INVALID_OPERATION),
        }
    }
}

fn push_locations(out: &mut Vec<(String, UniformLocation)>, name: &str, size: u32, uniform: usize) {
    match name.strip_suffix("[0]") {
        Some(base) => {
            out.push((base.to_owned(), UniformLocation { uniform, element: 0 }));
            for i in 0..size {
                out.push((format!("{base}[{i}]"), UniformLocation { uniform, element: i }));
            }
        }
        None => out.push((name.to_owned(), UniformLocation { uniform, element: 0 })),
    }
}
