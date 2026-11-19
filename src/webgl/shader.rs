use std::collections::{HashMap, HashSet};

use crate::webgl::glsl::{Frontend, Options, WebGlOptions, WebGlShader};
use naga::valid::{Capabilities, ModuleInfo, ValidationFlags, Validator};
use naga::{Handle, ImageClass, ImageDimension, Module, ScalarKind, ShaderStage, Type, TypeInner, VectorSize};

use crate::webgl::consts::*;
use crate::webgl::live::Live;

pub const VERTEX_UNIFORMS: u32 = 0;
pub const FRAGMENT_UNIFORMS: u32 = 1;
pub const RESOURCES: u32 = 1;

pub struct Compiled {
    pub stage: ShaderStage,
    pub source: String,
    pub version: u16,
    pub info: WebGlShader,
    pub module: Module,
    pub live: Live,
}

pub fn version(source: &str) -> u16 {
    let mut rest = source;
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest.strip_prefix("//") {
            rest = r.split_once('\n').map_or("", |(_, r)| r);
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.split_once("*/").map_or("", |(_, r)| r);
        } else {
            break;
        }
    }
    let Some(r) = rest.strip_prefix('#') else { return 100 };
    let r = r.trim_start_matches([' ', '\t']);
    let Some(r) = r.strip_prefix("version") else { return 100 };
    let digits: String = r.trim_start_matches([' ', '\t']).chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(100)
}

fn options(stage: ShaderStage, version: u16, extensions: &[&str]) -> Options {
    let mut options = Options::from(stage);
    for (name, value) in [("GL_ES", "1"), ("GL_FRAGMENT_PRECISION_HIGH", "1"), ("__VERSION__", if version == 300 { "300" } else { "100" })] {
        options.defines.insert(name.into(), value.into());
    }
    for name in extensions {
        options.defines.insert((*name).into(), "1".into());
    }
    options
}

fn log(source: &str, errors: crate::webgl::glsl::ParseErrors) -> String {
    let mut out = String::new();
    for error in errors.errors {
        let line = error.location(source).map_or(0, |l| l.line_number);
        out.push_str(&format!("ERROR: 0:{line}: {}\n", error.kind));
    }
    out
}

fn compacted(module: &Module) -> Module {
    let mut module = module.clone();
    naga::compact::compact(&mut module, naga::compact::KeepUnused::Yes);
    module
}

fn validate(module: &Module) -> Result<ModuleInfo, String> {
    Validator::new(ValidationFlags::all(), Capabilities::empty()).validate(module).map_err(|e| {
        let mut text = format!("ERROR: {}", e.as_inner());
        let mut source = std::error::Error::source(e.as_inner());
        while let Some(s) = source {
            text.push_str(&format!(": {s}"));
            source = s.source();
        }
        text.push('\n');
        text
    })
}

pub fn compile(stage: ShaderStage, source: &str, extensions: &[&str], varyings: u32) -> Result<Compiled, String> {
    let version = version(source);
    if version != 100 && version != 300 {
        return Err(format!("ERROR: 0:1: '{version}' : version number not supported\n"));
    }
    let options = options(stage, version, extensions);
    let webgl = WebGlOptions { uniform_binding: 0, group: RESOURCES, first_binding: 0, draw_buffers: 8, varyings, ..Default::default() };
    let (module, info) = Frontend::default().parse_webgl(&options, webgl, source).map_err(|e| log(source, e))?;
    validate(&compacted(&module))?;
    let live = Live::of(&module);
    Ok(Compiled { stage, source: source.into(), version, info, module, live })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Float,
    Sint,
    Uint,
}

impl Kind {
    fn of(kind: ScalarKind) -> Self {
        match kind {
            ScalarKind::Sint => Kind::Sint,
            ScalarKind::Uint => Kind::Uint,
            _ => Kind::Float,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: String,
    pub gl_type: u32,
    pub location: u32,
    pub slots: u32,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug)]
pub struct Slot {
    pub offset: u32,
    pub stride: u32,
    pub boolean: bool,
}

#[derive(Clone, Debug)]
pub struct Uniform {
    pub name: String,
    pub gl_type: u32,
    pub size: u32,
    pub stages: [Option<Slot>; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sampled {
    Float,
    Sint,
    Uint,
    Depth,
}

#[derive(Clone, Debug)]
pub struct Sampler {
    pub name: String,
    pub gl_type: u32,
    pub size: u32,
    pub dim: wgpu::TextureViewDimension,
    pub sampled: Sampled,
    pub bindings: [Vec<(u32, u32)>; 2],
}

#[derive(Clone, Debug)]
pub struct BlockMember {
    pub name: String,
    pub gl_type: u32,
    pub size: u32,
    pub offset: u32,
    pub array_stride: u32,
    pub matrix_stride: u32,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub name: String,
    pub size: u32,
    pub bindings: [Option<u32>; 2],
    pub members: Vec<BlockMember>,
}

#[derive(Clone, Debug)]
pub struct Output {
    pub name: String,
    pub location: u32,
    pub kind: Kind,
}

pub struct Points {
    pub vertex: Module,
    pub fragment: Module,
    pub viewport: u32,
    pub size: u32,
}

pub struct Linked {
    pub vertex: Module,
    pub fragment: Module,
    pub points: Option<Points>,
    pub attributes: Vec<Attribute>,
    pub uniforms: Vec<Uniform>,
    pub samplers: Vec<Sampler>,
    pub blocks: Vec<Block>,
    pub outputs: Vec<Output>,
    pub uniform_sizes: [u32; 2],
    pub resources: u32,
}

pub struct Limits {
    pub attributes: u32,
    pub varyings: u32,
    pub draw_buffers: u32,
    pub textures: u32,
}

fn unpadded(types: &naga::UniqueArena<Type>, ty: Handle<Type>) -> Handle<Type> {
    match types[ty].inner {
        TypeInner::Struct { ref members, .. } if types[ty].name.as_deref() == Some(crate::webgl::glsl::PADDED) => members[0].ty,
        _ => ty,
    }
}

fn slots(types: &naga::UniqueArena<Type>, ty: Handle<Type>) -> u32 {
    let ty = unpadded(types, ty);
    match types[ty].inner {
        TypeInner::Matrix { columns, .. } => columns as u32,
        TypeInner::Array { base, size: naga::ArraySize::Constant(n), .. } => n.get().saturating_mul(slots(types, base)),
        TypeInner::Struct { ref members, .. } => members.iter().fold(0u32, |n, m| n.saturating_add(slots(types, m.ty))),
        _ => 1,
    }
}

fn scalar_kind(types: &naga::UniqueArena<Type>, ty: Handle<Type>) -> Kind {
    match types[unpadded(types, ty)].inner {
        TypeInner::Scalar(s) | TypeInner::Vector { scalar: s, .. } | TypeInner::Matrix { scalar: s, .. } => Kind::of(s.kind),
        TypeInner::Array { base, .. } => scalar_kind(types, base),
        _ => Kind::Float,
    }
}

fn same_type(a: &naga::UniqueArena<Type>, x: Handle<Type>, b: &naga::UniqueArena<Type>, y: Handle<Type>) -> bool {
    let (x, y) = (unpadded(a, x), unpadded(b, y));
    match (&a[x].inner, &b[y].inner) {
        (&TypeInner::Array { base: p, size: m, .. }, &TypeInner::Array { base: q, size: n, .. }) => m == n && same_type(a, p, b, q),
        (TypeInner::Struct { members: m, .. }, TypeInner::Struct { members: n, .. }) => {
            m.len() == n.len() && m.iter().zip(n).all(|(m, n)| m.name == n.name && same_type(a, m.ty, b, n.ty))
        }
        (p, q) => p == q,
    }
}

pub fn gl_type(inner: &TypeInner) -> u32 {
    use VectorSize::*;
    match *inner {
        TypeInner::Scalar(s) => match s.kind {
            ScalarKind::Float => FLOAT,
            ScalarKind::Sint => INT,
            ScalarKind::Uint => UNSIGNED_INT,
            _ => BOOL,
        },
        TypeInner::Vector { size, scalar } => {
            let n = size as usize - 2;
            match scalar.kind {
                ScalarKind::Float => [FLOAT_VEC2, FLOAT_VEC3, FLOAT_VEC4][n],
                ScalarKind::Sint => [INT_VEC2, INT_VEC3, INT_VEC4][n],
                ScalarKind::Uint => [UNSIGNED_INT_VEC2, UNSIGNED_INT_VEC3, UNSIGNED_INT_VEC4][n],
                _ => [BOOL_VEC2, BOOL_VEC3, BOOL_VEC4][n],
            }
        }
        TypeInner::Matrix { columns, rows, .. } => match (columns, rows) {
            (Bi, Bi) => FLOAT_MAT2,
            (Tri, Tri) => FLOAT_MAT3,
            (Quad, Quad) => FLOAT_MAT4,
            (Bi, Tri) => FLOAT_MAT2X3,
            (Bi, Quad) => FLOAT_MAT2X4,
            (Tri, Bi) => FLOAT_MAT3X2,
            (Tri, Quad) => FLOAT_MAT3X4,
            (Quad, Bi) => FLOAT_MAT4X2,
            (Quad, Tri) => FLOAT_MAT4X3,
        },
        TypeInner::Image { dim, arrayed, class } => {
            let (kind, shadow) = match class {
                ImageClass::Sampled { kind, .. } => (kind, false),
                ImageClass::Depth { .. } => (ScalarKind::Float, true),
                _ => (ScalarKind::Float, false),
            };
            match (dim, arrayed, shadow, kind) {
                (ImageDimension::D2, false, true, _) => SAMPLER_2D_SHADOW,
                (ImageDimension::Cube, false, true, _) => SAMPLER_CUBE_SHADOW,
                (ImageDimension::D2, true, true, _) => SAMPLER_2D_ARRAY_SHADOW,
                (ImageDimension::D2, false, _, ScalarKind::Sint) => INT_SAMPLER_2D,
                (ImageDimension::D3, _, _, ScalarKind::Sint) => INT_SAMPLER_3D,
                (ImageDimension::Cube, _, _, ScalarKind::Sint) => INT_SAMPLER_CUBE,
                (ImageDimension::D2, true, _, ScalarKind::Sint) => INT_SAMPLER_2D_ARRAY,
                (ImageDimension::D2, false, _, ScalarKind::Uint) => UNSIGNED_INT_SAMPLER_2D,
                (ImageDimension::D3, _, _, ScalarKind::Uint) => UNSIGNED_INT_SAMPLER_3D,
                (ImageDimension::Cube, _, _, ScalarKind::Uint) => UNSIGNED_INT_SAMPLER_CUBE,
                (ImageDimension::D2, true, _, ScalarKind::Uint) => UNSIGNED_INT_SAMPLER_2D_ARRAY,
                (ImageDimension::D3, ..) => SAMPLER_3D,
                (ImageDimension::Cube, ..) => SAMPLER_CUBE,
                (ImageDimension::D2, true, ..) => SAMPLER_2D_ARRAY,
                _ => SAMPLER_2D,
            }
        }
        _ => 0,
    }
}

fn leaves(
    types: &naga::UniqueArena<Type>,
    declared: Handle<Type>,
    stored: Handle<Type>,
    name: String,
    offset: u32,
    out: &mut Vec<(String, u32, u32, u32, u32, bool)>,
) {
    match (&types[declared].inner, &types[stored].inner) {
        (TypeInner::Struct { members: d, .. }, TypeInner::Struct { members: s, .. }) => {
            for (dm, sm) in d.iter().zip(s) {
                let field = format!("{name}.{}", dm.name.as_deref().unwrap_or(""));
                leaves(types, dm.ty, sm.ty, field, offset + sm.offset, out);
            }
        }
        (&TypeInner::Array { base: db, size, .. }, &TypeInner::Array { base: sb, stride, .. }) => {
            let sb = unpadded(types, sb);
            let n = match size {
                naga::ArraySize::Constant(n) => n.get(),
                _ => 1,
            };
            if matches!(types[db].inner, TypeInner::Struct { .. } | TypeInner::Array { .. }) {
                for i in 0..n {
                    leaves(types, db, sb, format!("{name}[{i}]"), offset + i * stride, out);
                }
            } else {
                let boolean = gl_type(&types[db].inner) != gl_type(&types[sb].inner);
                out.push((format!("{name}[0]"), gl_type(&types[db].inner), n, offset, stride, boolean));
            }
        }
        (d, s) => {
            let boolean = gl_type(d) != gl_type(s);
            out.push((name, gl_type(d), 1, offset, 0, boolean));
        }
    }
}

fn uniform_leaves(compiled: &Compiled) -> Vec<(String, u32, u32, Slot)> {
    let Some(block) = compiled.info.uniforms else { return Vec::new() };
    let module = &compiled.module;
    let TypeInner::Struct { ref members, .. } = module.types[module.global_variables[block].ty].inner else { return Vec::new() };
    let mut out = Vec::new();
    for (i, (member, &declared)) in members.iter().zip(&compiled.info.uniform_types).enumerate() {
        if !compiled.live.member(block, i as u32) {
            continue;
        }
        let mut leaves_of = Vec::new();
        leaves(&module.types, declared, member.ty, member.name.clone().unwrap_or_default(), member.offset, &mut leaves_of);
        for (name, gl_type, size, offset, stride, boolean) in leaves_of {
            out.push((name, gl_type, size, Slot { offset, stride, boolean }));
        }
    }
    out
}

fn struct_size(module: &Module, global: Option<Handle<naga::GlobalVariable>>) -> u32 {
    global.map_or(0, |g| match module.types[module.global_variables[g].ty].inner {
        TypeInner::Struct { span, .. } => span,
        _ => 0,
    })
}

fn sampled(class: ImageClass) -> Sampled {
    match class {
        ImageClass::Depth { .. } => Sampled::Depth,
        ImageClass::Sampled { kind: ScalarKind::Sint, .. } => Sampled::Sint,
        ImageClass::Sampled { kind: ScalarKind::Uint, .. } => Sampled::Uint,
        _ => Sampled::Float,
    }
}

fn view_dimension(dim: ImageDimension, arrayed: bool) -> wgpu::TextureViewDimension {
    match (dim, arrayed) {
        (ImageDimension::D3, _) => wgpu::TextureViewDimension::D3,
        (ImageDimension::Cube, false) => wgpu::TextureViewDimension::Cube,
        (ImageDimension::Cube, true) => wgpu::TextureViewDimension::CubeArray,
        (_, true) => wgpu::TextureViewDimension::D2Array,
        _ => wgpu::TextureViewDimension::D2,
    }
}

fn block_members(types: &naga::UniqueArena<Type>, ty: Handle<Type>, prefix: &str, base: u32, out: &mut Vec<BlockMember>) {
    let TypeInner::Struct { ref members, .. } = types[ty].inner else { return };
    for m in members {
        let name = match prefix {
            "" => m.name.clone().unwrap_or_default(),
            p => format!("{p}.{}", m.name.as_deref().unwrap_or("")),
        };
        let offset = base + m.offset;
        match types[m.ty].inner {
            TypeInner::Struct { .. } => block_members(types, m.ty, &name, offset, out),
            TypeInner::Array { base: element, size, stride } => {
                let element = unpadded(types, element);
                let n = match size {
                    naga::ArraySize::Constant(n) => n.get(),
                    _ => 1,
                };
                if let TypeInner::Struct { .. } = types[element].inner {
                    for i in 0..n {
                        block_members(types, element, &format!("{name}[{i}]"), offset + i * stride, out);
                    }
                } else {
                    let matrix_stride = matrix_stride(&types[element].inner);
                    out.push(BlockMember {
                        name: format!("{name}[0]"),
                        gl_type: gl_type(&types[element].inner),
                        size: n,
                        offset,
                        array_stride: stride,
                        matrix_stride,
                    });
                }
            }
            ref inner => out.push(BlockMember { name, gl_type: gl_type(inner), size: 1, offset, array_stride: 0, matrix_stride: matrix_stride(inner) }),
        }
    }
}

fn matrix_stride(inner: &TypeInner) -> u32 {
    match *inner {
        TypeInner::Matrix { .. } => 16,
        _ => 0,
    }
}

pub fn link(vs: &Compiled, fs: &Compiled, bound: &HashMap<String, u32>, limits: &Limits) -> Result<Linked, String> {
    if vs.version != fs.version {
        return Err("Versions of linked shaders have to match.".into());
    }
    let mut taken = vec![false; limits.attributes as usize];
    let mut attributes = Vec::new();
    let mut locations: HashMap<String, u32> = HashMap::new();
    let vm = &vs.module;
    let mut pending = Vec::new();
    for input in &vs.info.inputs {
        let n = slots(&vm.types, input.ty);
        let location = if input.explicit { Some(input.location) } else { bound.get(&input.name).copied() };
        match location {
            Some(l) => {
                if l.saturating_add(n) > limits.attributes {
                    return Err(format!("attribute '{}' does not fit at location {l}", input.name));
                }
                for s in l..l + n {
                    if taken[s as usize] && vs.live.globals.contains(&input.handle) {
                        return Err(format!("attribute '{}' overlaps another attribute at location {s}", input.name));
                    }
                    taken[s as usize] = true;
                }
                locations.insert(input.name.clone(), l);
            }
            None => pending.push((input, n)),
        }
    }
    for (input, n) in pending {
        let Some(l) = (0..limits.attributes.saturating_sub(n - 1)).find(|&l| (l..l + n).all(|s| !taken[s as usize])) else {
            return Err(format!("too many attributes ({})", input.name));
        };
        (l..l + n).for_each(|s| taken[s as usize] = true);
        locations.insert(input.name.clone(), l);
    }
    for input in &vs.info.inputs {
        if !vs.live.globals.contains(&input.handle) {
            continue;
        }
        attributes.push(Attribute {
            name: input.name.clone(),
            gl_type: gl_type(
                &vm.types[match vm.types[input.ty].inner {
                    TypeInner::Array { base, .. } => base,
                    _ => input.ty,
                }]
                .inner,
            ),
            location: locations[&input.name],
            slots: slots(&vm.types, input.ty),
            kind: scalar_kind(&vm.types, input.ty),
        });
    }

    let fm = &fs.module;
    let mut varyings: u32 = 0;
    let mut dropped_vs: HashSet<String> = vs.info.inputs.iter().filter(|i| !vs.live.globals.contains(&i.handle)).map(|i| i.name.clone()).collect();
    let mut dropped_fs = HashSet::new();
    let fs_inputs: HashMap<&str, &crate::webgl::glsl::WebGlInterface> = fs.info.inputs.iter().map(|i| (i.name.as_str(), i)).collect();
    let mut varying_locations = HashMap::new();
    for output in &vs.info.outputs {
        let Some(input) = fs_inputs.get(output.name.as_str()) else {
            dropped_vs.insert(output.name.clone());
            continue;
        };
        if !same_type(&vm.types, output.ty, &fm.types, input.ty) {
            return Err(format!("varying '{}' has different types in the vertex and fragment shaders", output.name));
        }
        varying_locations.insert(output.name.clone(), varyings);
        varyings = varyings.saturating_add(slots(&vm.types, output.ty));
    }
    if varyings > limits.varyings {
        return Err(format!("too many varyings ({varyings} vectors, at most {})", limits.varyings));
    }
    for input in &fs.info.inputs {
        if varying_locations.contains_key(&input.name) {
            continue;
        }
        if fs.live.globals.contains(&input.handle) {
            return Err(format!("varying '{}' is read by the fragment shader but not declared by the vertex shader", input.name));
        }
        dropped_fs.insert(input.name.clone());
    }

    let draw_buffers = match fs.info.frag_data {
        Some(g) if fs.live.dynamic.contains(&g) => limits.draw_buffers,
        Some(g) => fs.live.members.get(&g).and_then(|m| m.iter().max()).map_or(1, |m| m + 1),
        None => 1,
    };

    let vertex_resources = vs.info.samplers.iter().map(|s| 2 * s.globals.len() as u32).sum::<u32>() + vs.info.blocks.len() as u32;
    let mut vertex_locations: HashMap<String, u32> = locations.clone();
    vertex_locations.extend(varying_locations.iter().map(|(k, v)| (k.clone(), *v)));
    let vertex_options = WebGlOptions {
        locations: vertex_locations.into_iter().collect(),
        uniform_binding: VERTEX_UNIFORMS,
        group: RESOURCES,
        first_binding: 0,
        draw_buffers,
        dropped: dropped_vs.into_iter().collect(),
        points: None,
        varyings: limits.varyings,
    };
    let fragment_options = WebGlOptions {
        locations: varying_locations.into_iter().collect(),
        uniform_binding: FRAGMENT_UNIFORMS,
        group: RESOURCES,
        first_binding: vertex_resources,
        draw_buffers,
        dropped: dropped_fs.into_iter().collect(),
        points: None,
        varyings: limits.varyings,
    };
    let extensions: Vec<&str> = Vec::new();
    let wide = vs.info.point_size.is_some_and(|g| vs.live.globals.contains(&g));
    let point_options = |options: &WebGlOptions| WebGlOptions { points: Some(varyings), ..options.clone() };
    let points = match wide {
        true => {
            let (pv, pvinfo) = Frontend::default()
                .parse_webgl(&options(ShaderStage::Vertex, vs.version, &extensions), point_options(&vertex_options), &vs.source)
                .map_err(|e| log(&vs.source, e))?;
            let (pf, _) = Frontend::default()
                .parse_webgl(&options(ShaderStage::Fragment, fs.version, &extensions), point_options(&fragment_options), &fs.source)
                .map_err(|e| log(&fs.source, e))?;
            let (viewport, size) = match pvinfo.uniforms.map(|g| &pv.types[pv.global_variables[g].ty].inner) {
                Some(TypeInner::Struct { members, span }) => (members.iter().find(|m| m.name.as_deref() == Some("webgl_point")).map_or(0, |m| m.offset), *span),
                _ => (0, 0),
            };
            let (pv, pf) = (compacted(&pv), compacted(&pf));
            validate(&pv)?;
            validate(&pf)?;
            Some(Points { vertex: pv, fragment: pf, viewport, size })
        }
        false => None,
    };
    let (vertex, vinfo) =
        Frontend::default().parse_webgl(&options(ShaderStage::Vertex, vs.version, &extensions), vertex_options, &vs.source).map_err(|e| log(&vs.source, e))?;
    let (fragment, finfo) = Frontend::default()
        .parse_webgl(&options(ShaderStage::Fragment, fs.version, &extensions), fragment_options, &fs.source)
        .map_err(|e| log(&fs.source, e))?;
    let vertex_final = compacted(&vertex);
    let fragment_final = compacted(&fragment);
    validate(&vertex_final)?;
    validate(&fragment_final)?;
    let vertex = Compiled { stage: ShaderStage::Vertex, source: String::new(), version: vs.version, live: Live::of(&vertex), info: vinfo, module: vertex };
    let fragment =
        Compiled { stage: ShaderStage::Fragment, source: String::new(), version: fs.version, live: Live::of(&fragment), info: finfo, module: fragment };

    let mut uniforms: Vec<Uniform> = Vec::new();
    for (stage, compiled) in [&vertex, &fragment].into_iter().enumerate() {
        for (name, gl_type, size, slot) in uniform_leaves(compiled) {
            match uniforms.iter_mut().find(|u| u.name == name) {
                Some(u) if u.gl_type != gl_type || u.size != size => {
                    return Err(format!("uniform '{name}' differs between the vertex and fragment shaders"));
                }
                Some(u) => u.stages[stage] = Some(slot),
                None => {
                    let mut stages = [None, None];
                    stages[stage] = Some(slot);
                    uniforms.push(Uniform { name, gl_type, size, stages });
                }
            }
        }
    }

    let mut samplers: Vec<Sampler> = Vec::new();
    for (stage, compiled) in [&vertex, &fragment].into_iter().enumerate() {
        let module = &compiled.module;
        for s in &compiled.info.samplers {
            if !s.globals.iter().any(|(image, _)| compiled.live.globals.contains(image)) {
                continue;
            }
            let TypeInner::Image { dim, arrayed, class } = module.types[s.ty].inner else { continue };
            let gl_type = gl_type(&module.types[s.ty].inner);
            let name = match s.array {
                Some(_) => format!("{}[0]", s.name),
                None => s.name.clone(),
            };
            let size = s.array.unwrap_or(1);
            match samplers.iter_mut().find(|u| u.name == name) {
                Some(u) if u.gl_type != gl_type || u.size != size => {
                    return Err(format!("uniform '{}' differs between the vertex and fragment shaders", s.name));
                }
                Some(u) => u.bindings[stage] = s.bindings.clone(),
                None => {
                    let mut bindings = [Vec::new(), Vec::new()];
                    bindings[stage] = s.bindings.clone();
                    samplers.push(Sampler { name, gl_type, size, dim: view_dimension(dim, arrayed), sampled: sampled(class), bindings });
                }
            }
        }
    }
    let textures = samplers.iter().map(|s| s.size).sum::<u32>();
    if textures > limits.textures {
        return Err(format!("too many samplers ({textures})"));
    }

    let mut blocks: Vec<Block> = Vec::new();
    for (stage, compiled) in [&vertex, &fragment].into_iter().enumerate() {
        let module = &compiled.module;
        for b in &compiled.info.blocks {
            let ty = module.global_variables[b.handle].ty;
            let TypeInner::Struct { span, .. } = module.types[ty].inner else { continue };
            match blocks.iter_mut().find(|x| x.name == b.name) {
                Some(x) if x.size != span => return Err(format!("uniform block '{}' differs between the shaders", b.name)),
                Some(x) => x.bindings[stage] = Some(b.binding),
                None => {
                    let mut members = Vec::new();
                    block_members(&module.types, ty, if b.instance.is_some() { &b.name } else { "" }, 0, &mut members);
                    if b.instance.is_some() {
                        for m in &mut members {
                            if let Some(rest) = m.name.strip_prefix(&format!("{}.", b.name)) {
                                m.name = format!("{}.{rest}", b.name);
                            }
                        }
                    }
                    let mut bindings = [None, None];
                    bindings[stage] = Some(b.binding);
                    blocks.push(Block { name: b.name.clone(), size: span, bindings, members });
                }
            }
        }
    }

    let outputs = fragment
        .info
        .outputs
        .iter()
        .flat_map(|o| {
            let n = slots(&fragment.module.types, o.ty);
            let kind = scalar_kind(&fragment.module.types, o.ty);
            let name = o.name.clone();
            (o.location..o.location + n).map(move |location| Output { name: name.clone(), location, kind })
        })
        .collect();
    let resources = vertex_resources + fragment.info.samplers.iter().map(|s| 2 * s.globals.len() as u32).sum::<u32>() + fragment.info.blocks.len() as u32;
    Ok(Linked {
        uniform_sizes: [struct_size(&vertex.module, vertex.info.uniforms), struct_size(&fragment.module, fragment.info.uniforms)],
        vertex: vertex_final,
        fragment: fragment_final,
        points,
        attributes,
        uniforms,
        samplers,
        blocks,
        outputs,
        resources,
    })
}
