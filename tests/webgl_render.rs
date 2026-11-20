use acde::webgl::consts::*;
use acde::webgl::{Attributes, Context, Setter};

fn gpu() -> Option<std::sync::Arc<acde::gpu::Gpu>> {
    acde::gpu::Gpu::open("webgl test").ok().map(|(.., g)| g)
}

fn program(gl: &mut Context, vs: &str, fs: &str) -> u32 {
    let v = gl.create_shader(VERTEX_SHADER).unwrap();
    gl.shader_source(v, vs);
    gl.compile_shader(v);
    assert_eq!(gl.shader_info_log(v).unwrap(), "");
    let f = gl.create_shader(FRAGMENT_SHADER).unwrap();
    gl.shader_source(f, fs);
    gl.compile_shader(f);
    assert_eq!(gl.shader_info_log(f).unwrap(), "");
    let p = gl.create_program();
    gl.attach_shader(p, v);
    gl.attach_shader(p, f);
    gl.link_program(p);
    assert_eq!(gl.program_info_log(p).unwrap(), "");
    p
}

fn pixel(gl: &mut Context, x: i32, y: i32) -> [u8; 4] {
    let mut out = [0u8; 4];
    gl.read_pixels(x, y, 1, 1, RGBA, UNSIGNED_BYTE, &mut out);
    out
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[test]
fn triangle_and_clear() {
    let _ = env_logger::builder().is_test(true).try_init();
    let Some(gpu) = gpu() else { return };
    for (webgl2, antialias) in [(false, false), (true, true)] {
        let mut gl = Context::new(gpu.clone(), webgl2, Attributes { antialias, ..Default::default() }, 64, 64);
        gl.clear_color(0.0, 0.0, 1.0, 1.0);
        gl.clear(COLOR_BUFFER_BIT);
        assert_eq!(pixel(&mut gl, 1, 1), [0, 0, 255, 255]);
        let p = program(
            &mut gl,
            "attribute vec2 p; attribute vec3 c; varying vec3 v; void main() { v = c; gl_Position = vec4(p, 0.0, 1.0); }",
            "precision mediump float; varying vec3 v; uniform float k; void main() { gl_FragColor = vec4(v * k, 1.0); }",
        );
        gl.use_program(Some(p));
        let b = gl.create_buffer();
        gl.bind_buffer(ARRAY_BUFFER, Some(b));
        gl.buffer_data(ARRAY_BUFFER, Some(&floats(&[-1.0, -1.0, 1.0, 0.0, 0.0, 1.0, -1.0, 0.0, 1.0, 0.0, -1.0, 1.0, 0.0, 0.0, 1.0])), 0, STATIC_DRAW);
        let pl = gl.attrib_location(p, "p") as u32;
        let cl = gl.attrib_location(p, "c") as u32;
        gl.vertex_attrib_pointer(pl, 2, FLOAT, false, 20, 0, false);
        gl.vertex_attrib_pointer(cl, 3, FLOAT, false, 20, 8, false);
        gl.enable_vertex_attrib(pl, true);
        gl.enable_vertex_attrib(cl, true);
        let k = gl.uniform_location(p, "k").unwrap();
        let serial = gl.program_serial(p);
        gl.uniform(Some(p), k, serial, Setter::Float(1), false, &floats(&[1.0]));
        gl.draw(TRIANGLES, 0, 3, 1, None);
        assert_eq!(gl.get_error(), NO_ERROR);
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| (i16::from(*x) - i16::from(y)).abs() < 24);
        let bl = pixel(&mut gl, 2, 2);
        assert!(near(bl, [255, 0, 0, 255]), "bottom left is red (GL origin): {bl:?}");
        let br = pixel(&mut gl, 60, 1);
        assert!(near(br, [0, 255, 0, 255]), "bottom right is green: {br:?}");
        let tl = pixel(&mut gl, 1, 59);
        assert!(near(tl, [0, 0, 255, 255]), "top left is blue: {tl:?}");
        assert_eq!(pixel(&mut gl, 62, 62), [0, 0, 255, 255], "top right is the clear colour");
        gl.uniform(Some(p), k, serial, Setter::Float(1), false, &floats(&[0.0]));
        gl.enable(SCISSOR_TEST, true);
        gl.scissor(0, 0, 8, 8);
        gl.clear_color(1.0, 1.0, 0.0, 1.0);
        gl.clear(COLOR_BUFFER_BIT);
        gl.enable(SCISSOR_TEST, false);
        assert_eq!(pixel(&mut gl, 3, 3), [255, 255, 0, 255], "scissored clear");
        gl.draw(TRIANGLES, 0, 3, 1, None);
        assert_eq!(pixel(&mut gl, 3, 3), [0, 0, 0, 255], "uniform k = 0");
        assert!(gl.present().is_some());
    }
}

#[test]
fn culling_and_front_facing() {
    let Some(gpu) = gpu() else { return };
    let mut gl = Context::new(gpu, true, Attributes { antialias: false, ..Default::default() }, 8, 8);
    let p = program(
        &mut gl,
        "#version 300 es\nin vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }",
        "#version 300 es\nprecision mediump float; out vec4 o; void main() { o = gl_FrontFacing ? vec4(0.0, 1.0, 0.0, 1.0) : vec4(1.0, 0.0, 0.0, 1.0); }",
    );
    gl.use_program(Some(p));
    let b = gl.create_buffer();
    gl.bind_buffer(ARRAY_BUFFER, Some(b));
    gl.buffer_data(ARRAY_BUFFER, Some(&floats(&[-1.0, -1.0, 3.0, -1.0, -1.0, 3.0])), 0, STATIC_DRAW);
    gl.vertex_attrib_pointer(0, 2, FLOAT, false, 0, 0, false);
    gl.enable_vertex_attrib(0, true);
    gl.draw(TRIANGLES, 0, 3, 1, None);
    assert_eq!(pixel(&mut gl, 4, 4), [0, 255, 0, 255], "a CCW triangle is front facing");
    gl.enable(CULL_FACE, true);
    gl.clear(COLOR_BUFFER_BIT);
    gl.draw(TRIANGLES, 0, 3, 1, None);
    assert_eq!(pixel(&mut gl, 4, 4), [0, 255, 0, 255], "back-face culling keeps a CCW triangle");
    gl.cull_face(FRONT);
    gl.clear(COLOR_BUFFER_BIT);
    gl.draw(TRIANGLES, 0, 3, 1, None);
    assert_eq!(pixel(&mut gl, 4, 4), [0, 0, 0, 0], "front-face culling drops it");
}

#[test]
fn converted_attributes() {
    let Some(gpu) = gpu() else { return };
    let mut gl = Context::new(gpu, true, Attributes { antialias: false, ..Default::default() }, 8, 8);
    let p = program(
        &mut gl,
        "#version 300 es\nin vec2 p; in vec4 k; out vec4 v; void main() { v = k; gl_Position = vec4(p, 0.0, 1.0); }",
        "#version 300 es\nprecision mediump float; in vec4 v; out vec4 o; void main() { o = v / 255.0; }",
    );
    gl.use_program(Some(p));
    let b = gl.create_buffer();
    gl.bind_buffer(ARRAY_BUFFER, Some(b));
    gl.buffer_data(ARRAY_BUFFER, Some(&floats(&[-1.0, -1.0, 3.0, -1.0, -1.0, 3.0])), 0, STATIC_DRAW);
    let pl = gl.attrib_location(p, "p") as u32;
    let kl = gl.attrib_location(p, "k") as u32;
    gl.vertex_attrib_pointer(pl, 2, FLOAT, false, 0, 0, false);
    gl.enable_vertex_attrib(pl, true);
    let k = gl.create_buffer();
    gl.bind_buffer(ARRAY_BUFFER, Some(k));
    let shorts: Vec<u8> = [255u16, 0, 128, 255].repeat(3).iter().flat_map(|s| s.to_le_bytes()).collect();
    gl.buffer_data(ARRAY_BUFFER, Some(&shorts), 0, STATIC_DRAW);
    gl.vertex_attrib_pointer(kl, 4, UNSIGNED_SHORT, false, 0, 0, false);
    gl.enable_vertex_attrib(kl, true);
    gl.draw(TRIANGLES, 0, 3, 1, None);
    assert_eq!(gl.get_error(), 0);
    assert_eq!(pixel(&mut gl, 4, 4), [255, 0, 128, 255], "UNSIGNED_SHORT x4 non-normalized becomes float");
}

#[test]
fn robustness_and_mrt() {
    let Some(gpu) = gpu() else { return };
    let mut gl = Context::new(gpu, true, Attributes { antialias: false, ..Default::default() }, 8, 8);
    let t = gl.create_texture();
    gl.bind_texture(TEXTURE_2D, Some(t));
    gl.tex_image(TEXTURE_2D, 40, RGBA8, 1, 1, 1, 0, RGBA, UNSIGNED_BYTE, None);
    assert_eq!(gl.get_error(), INVALID_VALUE, "a level past the chain is INVALID_VALUE");
    gl.tex_image(TEXTURE_2D, 0, RGBA8, 1 << 20, 1, 1, 0, RGBA, UNSIGNED_BYTE, None);
    assert_eq!(gl.get_error(), INVALID_VALUE, "a texture past MAX_TEXTURE_SIZE is INVALID_VALUE");
    gl.tex_image(TEXTURE_2D, 0, RGBA8, 4, 4, 1, 0, RGBA, UNSIGNED_BYTE, None);
    let t2 = gl.create_texture();
    gl.bind_texture(TEXTURE_2D, Some(t2));
    gl.tex_image(TEXTURE_2D, 0, RGBA8, 4, 4, 1, 0, RGBA, UNSIGNED_BYTE, None);
    assert_eq!(gl.get_error(), NO_ERROR);
    let fb = gl.create_framebuffer();
    gl.bind_framebuffer(FRAMEBUFFER, Some(fb));
    gl.framebuffer_texture(FRAMEBUFFER, COLOR_ATTACHMENT0, TEXTURE_2D, Some(t), 0, 0);
    gl.framebuffer_texture(FRAMEBUFFER, COLOR_ATTACHMENT0 + 1, TEXTURE_2D, Some(t2), 0, 0);
    assert_eq!(gl.check_framebuffer_status(FRAMEBUFFER), FRAMEBUFFER_COMPLETE);
    let p = program(
        &mut gl,
        "#version 300 es\nin vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }",
        "#version 300 es\nprecision mediump float; layout(location = 1) out vec4 b; void main() { b = vec4(0.0, 0.0, 1.0, 1.0); }",
    );
    gl.use_program(Some(p));
    let b = gl.create_buffer();
    gl.bind_buffer(ARRAY_BUFFER, Some(b));
    gl.buffer_data(ARRAY_BUFFER, Some(&floats(&[-1.0, -1.0, 3.0, -1.0, -1.0, 3.0])), 0, STATIC_DRAW);
    gl.vertex_attrib_pointer(0, 2, FLOAT, false, 0, 0, false);
    gl.enable_vertex_attrib(0, true);
    gl.draw_buffers(&[COLOR_ATTACHMENT0, COLOR_ATTACHMENT0 + 1]);
    gl.clear_color(1.0, 0.0, 0.0, 1.0);
    gl.clear(COLOR_BUFFER_BIT);
    gl.draw(TRIANGLES, 0, 3, 1, None);
    assert_eq!(gl.get_error(), NO_ERROR, "a fragment shader without output 0 draws into attachment 1");
    gl.read_buffer(COLOR_ATTACHMENT0 + 1);
    assert_eq!(pixel(&mut gl, 1, 1), [0, 0, 255, 255]);
    gl.read_buffer(COLOR_ATTACHMENT0);
    assert_eq!(pixel(&mut gl, 1, 1), [255, 0, 0, 255], "attachment 0 keeps its clear");
    let q = gl.create_query();
    gl.begin_query(ANY_SAMPLES_PASSED, q);
    gl.draw(TRIANGLES, 0, 3, 1, None);
    gl.end_query(ANY_SAMPLES_PASSED);
    gl.submit();
    let mut available = 0;
    for _ in 0..200 {
        available = gl.query_result(q, QUERY_RESULT_AVAILABLE).unwrap();
        if available == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(available, 1);
    assert_eq!(gl.query_result(q, QUERY_RESULT), Some(1), "the draw passed samples");
    let s = gl.fence_sync();
    let mut signaled = false;
    for _ in 0..200 {
        if gl.sync_status(s) == Some(true) {
            signaled = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(signaled);
}

#[test]
fn multisampled_depth_resolve() {
    let _ = env_logger::builder().is_test(true).try_init();
    let Some(gpu) = gpu() else { return };
    let mut gl = Context::new(gpu, true, Attributes { antialias: false, ..Default::default() }, 8, 8);
    let p = program(
        &mut gl,
        "#version 300 es\nin vec3 p; void main() { gl_Position = vec4(p, 1.0); }",
        "#version 300 es\nprecision mediump float; uniform vec4 c; out vec4 o; void main() { o = c; }",
    );
    gl.use_program(Some(p));
    let serial = gl.program_serial(p);
    let c = gl.uniform_location(p, "c").unwrap();
    let b = gl.create_buffer();
    gl.bind_buffer(ARRAY_BUFFER, Some(b));
    gl.buffer_data(
        ARRAY_BUFFER,
        Some(&floats(&[-1.0, -1.0, -0.4, 3.0, -1.0, -0.4, -1.0, 3.0, -0.4, -1.0, -1.0, 0.0, 3.0, -1.0, 0.0, -1.0, 3.0, 0.0])),
        0,
        STATIC_DRAW,
    );
    gl.vertex_attrib_pointer(0, 3, FLOAT, false, 0, 0, false);
    gl.enable_vertex_attrib(0, true);
    let rb = |gl: &mut Context, samples: i32, format: u32| {
        let r = gl.create_renderbuffer();
        gl.bind_renderbuffer(RENDERBUFFER, Some(r));
        gl.renderbuffer_storage(RENDERBUFFER, samples, format, 8, 8);
        r
    };
    let (msaa_color, msaa_depth) = (rb(&mut gl, 4, RGBA8), rb(&mut gl, 4, DEPTH24_STENCIL8));
    let msaa = gl.create_framebuffer();
    gl.bind_framebuffer(FRAMEBUFFER, Some(msaa));
    gl.framebuffer_renderbuffer(FRAMEBUFFER, COLOR_ATTACHMENT0, RENDERBUFFER, Some(msaa_color));
    gl.framebuffer_renderbuffer(FRAMEBUFFER, DEPTH_STENCIL_ATTACHMENT, RENDERBUFFER, Some(msaa_depth));
    gl.enable(DEPTH_TEST, true);
    gl.clear(COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT);
    gl.uniform(Some(p), c, serial, Setter::Float(4), false, &floats(&[1.0, 0.0, 0.0, 1.0]));
    gl.draw(TRIANGLES, 0, 3, 1, None);
    let (color, depth) = (rb(&mut gl, 0, RGBA8), rb(&mut gl, 0, DEPTH24_STENCIL8));
    let single = gl.create_framebuffer();
    gl.bind_framebuffer(FRAMEBUFFER, Some(single));
    gl.framebuffer_renderbuffer(FRAMEBUFFER, COLOR_ATTACHMENT0, RENDERBUFFER, Some(color));
    gl.framebuffer_renderbuffer(FRAMEBUFFER, DEPTH_STENCIL_ATTACHMENT, RENDERBUFFER, Some(depth));
    gl.clear_depth(1.0);
    gl.clear(COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT);
    gl.bind_framebuffer(READ_FRAMEBUFFER, Some(msaa));
    gl.blit_framebuffer([0, 0, 8, 8], [0, 0, 8, 8], COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT, NEAREST);
    assert_eq!(gl.get_error(), NO_ERROR);
    gl.bind_framebuffer(FRAMEBUFFER, Some(single));
    assert_eq!(pixel(&mut gl, 2, 2), [255, 0, 0, 255], "colour resolved");
    gl.uniform(Some(p), c, serial, Setter::Float(4), false, &floats(&[0.0, 1.0, 0.0, 1.0]));
    gl.draw(TRIANGLES, 3, 3, 1, None);
    assert_eq!(pixel(&mut gl, 2, 2), [255, 0, 0, 255], "the resolved depth hides a farther triangle");
}

#[test]
fn wide_points() {
    let _ = env_logger::builder().is_test(true).try_init();
    let Some(gpu) = gpu() else { return };
    let mut gl = Context::new(gpu, false, Attributes { antialias: false, ..Default::default() }, 8, 8);
    let p = program(
        &mut gl,
        "attribute vec2 p; void main() { gl_PointSize = 4.0; gl_Position = vec4(p, 0.0, 1.0); }",
        "precision mediump float; void main() { gl_FragColor = vec4(gl_PointCoord, 0.0, 1.0); }",
    );
    gl.use_program(Some(p));
    let b = gl.create_buffer();
    gl.bind_buffer(ARRAY_BUFFER, Some(b));
    gl.buffer_data(ARRAY_BUFFER, Some(&floats(&[0.0, 0.0])), 0, STATIC_DRAW);
    gl.vertex_attrib_pointer(0, 2, FLOAT, false, 0, 0, false);
    gl.enable_vertex_attrib(0, true);
    gl.draw(POINTS, 0, 1, 1, None);
    assert_eq!(gl.get_error(), NO_ERROR);
    let mut all = [0u8; 8 * 8 * 4];
    gl.read_pixels(0, 0, 8, 8, RGBA, UNSIGNED_BYTE, &mut all);
    let lit = all.chunks(4).filter(|p| p[3] == 255).count();
    assert_eq!(lit, 16, "a 4 px point covers 4 x 4 pixels");
    let top_left = pixel(&mut gl, 2, 5);
    let bottom_right = pixel(&mut gl, 5, 2);
    assert!(top_left[0] < 64 && top_left[1] < 64, "gl_PointCoord starts at the top left: {top_left:?}");
    assert!(bottom_right[0] > 191 && bottom_right[1] > 191, "and ends at the bottom right: {bottom_right:?}");
}

#[test]
fn attributes_sharing_a_buffer() {
    let Some(gpu) = gpu() else { return };
    let mut gl = Context::new(gpu, true, Attributes { antialias: false, ..Default::default() }, 8, 8);
    let p = program(
        &mut gl,
        "#version 300 es\nin vec2 shift; in vec4 tint; in vec2 p; in vec2 q; out vec4 v; void main() { v = vec4(tint.rgb * q.x, 1.0); gl_Position = vec4(p + shift, 0.0, 1.0); }",
        "#version 300 es\nprecision mediump float; in vec4 v; out vec4 o; void main() { o = v; }",
    );
    gl.use_program(Some(p));
    let mut data = Vec::new();
    for (shift, tint) in [([0.0f32, 0.0], [255u8, 0, 0, 255]), ([1.0, 0.0], [0, 255, 0, 255])] {
        data.extend(floats(&shift));
        data.extend(tint);
    }
    data.resize(64, 0);
    let quad = [-1.0, -1.0, 0.0, -1.0, 0.0, 1.0, -1.0, -1.0, 0.0, 1.0, -1.0, 1.0];
    data.extend(floats(&quad));
    data.extend(floats(&[1.0, 0.0].repeat(6)));
    let b = gl.create_buffer();
    gl.bind_buffer(ARRAY_BUFFER, Some(b));
    gl.buffer_data(ARRAY_BUFFER, Some(&data), 0, STATIC_DRAW);
    let at = |gl: &mut Context, name: &str| gl.attrib_location(p, name) as u32;
    let (shift, tint, pos, q) = (at(&mut gl, "shift"), at(&mut gl, "tint"), at(&mut gl, "p"), at(&mut gl, "q"));
    gl.vertex_attrib_pointer(shift, 2, FLOAT, false, 12, 0, false);
    gl.vertex_attrib_pointer(tint, 4, UNSIGNED_BYTE, true, 12, 8, false);
    gl.vertex_attrib_pointer(pos, 2, FLOAT, false, 8, 64, false);
    gl.vertex_attrib_pointer(q, 2, FLOAT, false, 8, 64 + 48, false);
    for l in [shift, tint, pos, q] {
        gl.enable_vertex_attrib(l, true);
    }
    gl.vertex_attrib_divisor(shift, 1);
    gl.vertex_attrib_divisor(tint, 1);
    gl.draw(TRIANGLES, 0, 6, 2, None);
    assert_eq!(gl.get_error(), NO_ERROR);
    assert_eq!(pixel(&mut gl, 1, 4), [255, 0, 0, 255], "instance 0 reads its shift and tint from one stride");
    assert_eq!(pixel(&mut gl, 6, 4), [0, 255, 0, 255], "instance 1 reads the next stride");
    gl.vertex_attrib_pointer(tint, 4, UNSIGNED_BYTE, true, 12, 0, false);
    gl.draw(TRIANGLES, 0, 6, 2, None);
    assert_eq!(pixel(&mut gl, 1, 4), [0, 0, 0, 255], "tint now reads the shift's bytes: zero floats are zero bytes");
}
