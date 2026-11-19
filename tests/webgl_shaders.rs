use std::collections::HashMap;

use acde::webgl::shader::{self, Limits};
use naga::ShaderStage;

const LIMITS: Limits = Limits { attributes: 16, varyings: 15, draw_buffers: 8, textures: 32 };

fn backends(module: &naga::Module) {
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(module)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let options = naga::back::msl::Options { lang_version: (2, 1), ..Default::default() };
    let pipeline = naga::back::msl::PipelineOptions::default();
    let (msl, _) = naga::back::msl::write_string(module, &info, &options, &pipeline).unwrap_or_else(|e| panic!("msl: {e:?}"));
    assert!(!msl.is_empty());
    naga::back::spv::write_vec(module, &info, &naga::back::spv::Options::default(), None).unwrap_or_else(|e| panic!("spv: {e:?}"));
    let mut hlsl = String::new();
    let options = naga::back::hlsl::Options { shader_model: naga::back::hlsl::ShaderModel::V6_0, ..Default::default() };
    naga::back::hlsl::Writer::new(&mut hlsl, &options, &Default::default()).write(module, &info, None).unwrap_or_else(|e| panic!("hlsl: {e:?}"));
}

fn link(vs: &str, fs: &str) -> shader::Linked {
    let v = shader::compile(ShaderStage::Vertex, vs, &[], LIMITS.varyings).unwrap_or_else(|e| panic!("vertex: {e}"));
    let f = shader::compile(ShaderStage::Fragment, fs, &[], LIMITS.varyings).unwrap_or_else(|e| panic!("fragment: {e}"));
    let linked = shader::link(&v, &f, &HashMap::new(), &LIMITS).unwrap_or_else(|e| panic!("link: {e}"));
    backends(&linked.vertex);
    backends(&linked.fragment);
    linked
}

#[test]
fn es100_basic() {
    let linked = link(
        "attribute vec3 position; attribute vec2 uv; uniform mat4 mvp; varying vec2 vUv;
         void main() { vUv = uv; gl_Position = mvp * vec4(position, 1.0); }",
        "precision mediump float; varying vec2 vUv; uniform sampler2D map; uniform vec4 tint; uniform bool useTint;
         void main() { vec4 c = texture2D(map, vUv); if (useTint) c *= tint; gl_FragColor = c; }",
    );
    let names: Vec<_> = linked.uniforms.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, ["mvp", "tint", "useTint"]);
    assert_eq!(linked.attributes.len(), 2);
    assert_eq!(linked.samplers[0].name, "map");
    assert!(linked.uniforms[2].stages[1].unwrap().boolean);
}

#[test]
fn es300_three_like() {
    let linked = link(
        "#version 300 es
         #define attribute in
         #define varying out
         precision highp float;
         uniform mat4 modelViewMatrix; uniform mat4 projectionMatrix;
         struct Light { vec3 direction; vec3 color; };
         uniform Light lights[2];
         in vec3 position; in vec3 normal; in mat4 instanceMatrix;
         varying vec3 vNormal; flat out int vId;
         void main() { vNormal = normal * lights[1].direction; vId = gl_VertexID + gl_InstanceID;
           gl_Position = projectionMatrix * modelViewMatrix * instanceMatrix * vec4(position, 1.0); }",
        "#version 300 es
         #define varying in
         #define texture2D texture
         precision highp float; precision highp sampler2DShadow;
         layout(location = 0) out highp vec4 pc_fragColor;
         #define gl_FragColor pc_fragColor
         varying vec3 vNormal; flat in int vId;
         uniform sampler2D maps[2]; uniform sampler2DShadow shadowMap; uniform samplerCube env;
         uniform float weights[4]; uniform mat2 rot; uniform mat3 nm;
         vec4 sample2(sampler2D s, vec2 uv) { return texture(s, uv); }
         void main() { vec2 uv = rot * vNormal.xy;
           float lit = texture(shadowMap, vec3(uv, 0.5));
           gl_FragColor = sample2(maps[0], uv) + texture2D(maps[1], uv) * weights[2] + texture(env, nm * vNormal) * lit + float(vId); }",
    );
    let names: Vec<_> = linked.uniforms.iter().map(|u| u.name.as_str()).collect();
    assert!(names.contains(&"lights[1].direction"), "{names:?}");
    assert!(names.contains(&"weights[0]"), "{names:?}");
    let instance = linked.attributes.iter().find(|a| a.name == "instanceMatrix").unwrap();
    assert_eq!(instance.slots, 4);
    let samplers: Vec<_> = linked.samplers.iter().map(|s| (s.name.as_str(), s.size)).collect();
    assert_eq!(samplers, [("maps[0]", 2), ("shadowMap", 1), ("env", 1)]);
}

#[test]
fn frag_data_and_blocks() {
    let linked = link(
        "attribute vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }",
        "#extension GL_EXT_draw_buffers : require
         precision mediump float; void main() { gl_FragData[0] = vec4(1.0); gl_FragData[1] = vec4(0.5); }",
    );
    assert_eq!(linked.outputs.len(), 2);
    let linked = link(
        "#version 300 es
         layout(std140) uniform Camera { mat4 view; vec4 eye; } camera;
         in vec3 p; void main() { gl_Position = camera.view * vec4(p, 1.0) + camera.eye; }",
        "#version 300 es
         precision highp float; out vec4 color; uniform Material { vec4 diffuse; float shininess; };
         void main() { color = diffuse * shininess; }",
    );
    let blocks: Vec<_> = linked.blocks.iter().map(|b| (b.name.as_str(), b.size)).collect();
    assert_eq!(blocks, [("Camera", 80), ("Material", 32)]);
}

fn compile_on_web_stack(source: String) -> Result<(), String> {
    let mb = std::env::var("ACDE_TEST_STACK_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(8usize);
    std::thread::Builder::new()
        .stack_size(mb << 20)
        .spawn(move || shader::compile(ShaderStage::Fragment, &source, &[], LIMITS.varyings).map(drop))
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn errors() {
    let e = shader::compile(ShaderStage::Fragment, "void main() { gl_FragColor = vec4(undefinedThing); }", &[], LIMITS.varyings).err().unwrap();
    assert!(e.contains("ERROR"), "{e}");
    let n = 5000;
    for deep in [
        format!("void main() {{ float x = {}1.0{}; gl_FragColor = vec4(x); }}", "(".repeat(n), ")".repeat(n)),
        format!("void main() {{ float x = {}1.0; gl_FragColor = vec4(x); }}", "-".repeat(n)),
        format!("void main() {{ float x; x = {}1.0; gl_FragColor = vec4(x); }}", "x = ".repeat(n)),
        format!("void main() {{ bool b = true; float x = {}1.0; gl_FragColor = vec4(x); }}", "b ? 1.0 : ".repeat(n)),
        format!("void main() {{ float x[1] = {}1.0{}; gl_FragColor = vec4(x[0]); }}", "{".repeat(n), "}".repeat(n)),
        format!("{} float x; {} void main() {{ gl_FragColor = vec4(0.0); }}", "struct S { ".repeat(n), "} s; };".repeat(n)),
    ] {
        let head = deep[..48].to_owned();
        let e = compile_on_web_stack(deep).err();
        assert!(e.as_deref().is_some_and(|e| e.contains("too deep")), "{head}: {e:?}");
    }
    let long = format!("void main() {{ float x = 1.0{}; gl_FragColor = vec4(x); }}", " + 1.0".repeat(n));
    assert!(compile_on_web_stack(long).is_err());
    let arrays = "#version 300 es\nprecision highp float; out vec4 o; void main() { float a[2][2]; o = vec4(a[0][0]); }".to_owned();
    assert!(compile_on_web_stack(arrays).err().is_some_and(|e| e.contains("arrays of arrays")));
    for fine in [
        format!("void main() {{ float x = {}1.0{}; gl_FragColor = vec4(x); }}", "(".repeat(100), ")".repeat(100)),
        format!("void main() {{ float x = {}1.0; gl_FragColor = vec4(x); }}", "- ".repeat(100)),
    ] {
        assert_eq!(compile_on_web_stack(fine), Ok(()));
    }
}

#[test]
fn captured() {
    let Some(dir) = std::env::var_os("ACDE_WEBGL_SHADERS") else { return };
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "vert")).collect();
    files.sort();
    let mut failed = 0;
    for vert in &files {
        let frag = vert.with_extension("frag");
        let frag = match frag.exists() {
            true => frag,
            false => {
                let stem = vert.file_stem().unwrap().to_string_lossy().to_string();
                let (prefix, n) = stem.rsplit_once('_').unwrap();
                vert.with_file_name(format!("{prefix}_{:03}.frag", n.parse::<u32>().unwrap() + 1))
            }
        };
        let vs = std::fs::read_to_string(vert).unwrap();
        let fs = std::fs::read_to_string(&frag).unwrap();
        let result = std::panic::catch_unwind(|| {
            let v = shader::compile(ShaderStage::Vertex, &vs, &[], LIMITS.varyings).map_err(|e| format!("vertex: {e}"))?;
            let f = shader::compile(ShaderStage::Fragment, &fs, &[], LIMITS.varyings).map_err(|e| format!("fragment: {e}"))?;
            let linked = shader::link(&v, &f, &HashMap::new(), &LIMITS).map_err(|e| format!("link: {e}"))?;
            backends(&linked.vertex);
            backends(&linked.fragment);
            Ok::<_, String>(linked.uniforms.len())
        });
        match result {
            Ok(Ok(n)) => println!("ok   {} ({n} uniforms)", vert.display()),
            Ok(Err(e)) => {
                failed += 1;
                println!("FAIL {}: {}", vert.display(), e.lines().take(3).collect::<Vec<_>>().join(" | "));
            }
            Err(_) => {
                failed += 1;
                println!("PANIC {}", vert.display());
            }
        }
    }
    assert_eq!(failed, 0);
}

#[test]
fn the_varying_limit_is_the_contexts() {
    let declare = |n: usize, out: &str| (0..n).map(|i| format!("{out} vec4 v{i};")).collect::<String>();
    let sum = |n: usize| (0..n).map(|i| format!("v{i}")).collect::<Vec<_>>().join(" + ");
    let vs = format!(
        "attribute vec3 position; {} void main() {{ {} gl_Position = vec4(position, 1.0); }}",
        declare(17, "varying"),
        (0..17).map(|i| format!("v{i} = vec4({i}.0);")).collect::<String>()
    );
    let fs = format!("precision mediump float; {} void main() {{ gl_FragColor = {}; }}", declare(17, "varying"), sum(17));
    let v = shader::compile(ShaderStage::Vertex, &vs, &[], 30).unwrap();
    let f = shader::compile(ShaderStage::Fragment, &fs, &[], 30).unwrap();
    let narrow = shader::link(&v, &f, &HashMap::new(), &LIMITS).err().unwrap();
    assert!(narrow.contains("too many varyings (17 vectors, at most 15)"), "{narrow}");
    let wide = Limits { varyings: 30, ..LIMITS };
    let linked = shader::link(&v, &f, &HashMap::new(), &wide).unwrap();
    backends(&linked.vertex);
    backends(&linked.fragment);
    let max = shader::compile(ShaderStage::Vertex, "attribute vec4 p; void main() { gl_Position = p * float(gl_MaxVaryingVectors); }", &[], 30).unwrap();
    assert!(format!("{:?}", max.module).contains("I32(30)"));
}
