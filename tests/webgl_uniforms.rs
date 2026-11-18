use acde::webgl::consts::*;
use acde::webgl::{Attributes, Context, Setter};

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn run(fs_body: &str, decls: &str, set: impl Fn(&mut Context, u32, u64)) -> [u8; 4] {
    let gpu = acde::gpu::Gpu::open("uniforms").unwrap().2;
    let mut gl = Context::new(gpu, true, Attributes { antialias: false, ..Default::default() }, 4, 4);
    let v = gl.create_shader(VERTEX_SHADER).unwrap();
    gl.shader_source(v, "#version 300 es\nin vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }");
    gl.compile_shader(v);
    let f = gl.create_shader(FRAGMENT_SHADER).unwrap();
    gl.shader_source(f, &format!("#version 300 es\nprecision highp float; out vec4 o; {decls}\nvoid main() {{ {fs_body} }}"));
    gl.compile_shader(f);
    assert_eq!(gl.shader_info_log(f).unwrap(), "");
    let p = gl.create_program();
    gl.attach_shader(p, v);
    gl.attach_shader(p, f);
    gl.link_program(p);
    assert_eq!(gl.program_info_log(p).unwrap(), "");
    gl.use_program(Some(p));
    let serial = gl.program_serial(p);
    set(&mut gl, p, serial);
    let b = gl.create_buffer();
    gl.bind_buffer(ARRAY_BUFFER, Some(b));
    gl.buffer_data(ARRAY_BUFFER, Some(&floats(&[-1.0, -1.0, 3.0, -1.0, -1.0, 3.0])), 0, STATIC_DRAW);
    gl.vertex_attrib_pointer(0, 2, FLOAT, false, 0, 0, false);
    gl.enable_vertex_attrib(0, true);
    gl.draw(TRIANGLES, 0, 3, 1, None);
    assert_eq!(gl.get_error(), 0);
    let mut px = [0u8; 4];
    gl.read_pixels(1, 1, 1, 1, RGBA, UNSIGNED_BYTE, &mut px);
    px
}

#[test]
fn mat3_and_friends() {
    let px = run("o = vec4(m * vec3(1.0, 0.0, 0.0), 1.0);", "uniform mat3 m;", |gl, p, s| {
        let l = gl.uniform_location(p, "m").unwrap();
        gl.uniform(Some(p), l, s, Setter::Matrix(3, 3), false, &floats(&[0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]));
    });
    assert_eq!(px, [0, 255, 0, 255], "mat3 column 0 is (0,1,0)");
    let px = run("o = vec4(a.x, b.y, c, 1.0);", "uniform vec3 a; uniform vec2 b; uniform float c;", |gl, p, s| {
        let a = gl.uniform_location(p, "a").unwrap();
        gl.uniform(Some(p), a, s, Setter::Float(3), false, &floats(&[1.0, 0.0, 0.0]));
        let b = gl.uniform_location(p, "b").unwrap();
        gl.uniform(Some(p), b, s, Setter::Float(2), false, &floats(&[0.0, 0.5]));
        let c = gl.uniform_location(p, "c").unwrap();
        gl.uniform(Some(p), c, s, Setter::Float(1), false, &floats(&[0.25]));
    });
    assert_eq!(px, [255, 128, 64, 255]);
    let px = run("o = vec4(w[1], w[2], 0.0, 1.0) + vec4(l[1].c, 0.0);", "uniform float w[3]; struct L { vec3 d; vec3 c; }; uniform L l[2];", |gl, p, s| {
        let w = gl.uniform_location(p, "w").unwrap();
        gl.uniform(Some(p), w, s, Setter::Float(1), false, &floats(&[0.0, 1.0, 0.5]));
        let c = gl.uniform_location(p, "l[1].c").unwrap();
        gl.uniform(Some(p), c, s, Setter::Float(3), false, &floats(&[0.0, 0.0, 1.0]));
    });
    assert_eq!(px, [255, 128, 255, 255]);
}

#[test]
fn arrays_alone() {
    let px = run("o = vec4(w[1], w[2], 0.0, 1.0);", "uniform float w[3];", |gl, p, s| {
        let w = gl.uniform_location(p, "w").unwrap();
        gl.uniform(Some(p), w, s, Setter::Float(1), false, &floats(&[0.0, 1.0, 0.5]));
        assert_eq!(gl.get_error(), 0);
        eprintln!("info {:?}", gl.active_uniform(p, 0).map(|i| (i.name, i.size, i.ty)));
    });
    eprintln!("w {px:?}");
    let px = run("o = vec4(l[1].c, 1.0);", "struct L { vec3 d; vec3 c; }; uniform L l[2];", |gl, p, s| {
        let c = gl.uniform_location(p, "l[1].c").unwrap();
        gl.uniform(Some(p), c, s, Setter::Float(3), false, &floats(&[0.0, 0.0, 1.0]));
        assert_eq!(gl.get_error(), 0);
        eprintln!("info {:?}", (0..4).filter_map(|i| gl.active_uniform(p, i)).map(|i| (i.name, i.size)).collect::<Vec<_>>());
    });
    eprintln!("l {px:?}");
}
