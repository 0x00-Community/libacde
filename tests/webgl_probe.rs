use acde::webgl::consts::*;
use acde::webgl::{Attributes, Context};
fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}
#[test]
fn probe() {
    let gpu = acde::gpu::Gpu::open("probe").unwrap().2;
    for (stride, swap) in [(20usize, false), (20, true)] {
        let mut gl = Context::new(gpu.clone(), false, Attributes { antialias: false, ..Default::default() }, 64, 64);
        let v = gl.create_shader(VERTEX_SHADER).unwrap();
        gl.shader_source(v, "attribute vec2 p; attribute vec3 c; varying vec3 v; void main() { v = c; gl_Position = vec4(p, 0.0, 1.0); }");
        gl.compile_shader(v);
        let f = gl.create_shader(FRAGMENT_SHADER).unwrap();
        gl.shader_source(f, "precision mediump float; varying vec3 v; void main() { gl_FragColor = vec4(v, 1.0); }");
        gl.compile_shader(f);
        let p = gl.create_program();
        gl.attach_shader(p, v);
        gl.attach_shader(p, f);
        gl.link_program(p);
        gl.use_program(Some(p));
        let (pl, cl) = (gl.attrib_location(p, "p") as u32, gl.attrib_location(p, "c") as u32);
        let n = stride / 4;
        let mut data = vec![0f32; 3 * n];
        for (i, (pos, col)) in [([-1.0, -1.0], [1.0, 0.0, 0.0]), ([1.0, -1.0], [0.0, 1.0, 0.0]), ([-1.0, 1.0], [0.0, 0.0, 1.0])].iter().enumerate() {
            if swap {
                data[i * n + 3..i * n + 5].copy_from_slice(pos);
                data[i * n..i * n + 3].copy_from_slice(col);
            } else {
                data[i * n..i * n + 2].copy_from_slice(pos);
                data[i * n + 2..i * n + 5].copy_from_slice(col);
            }
        }
        let b = gl.create_buffer();
        gl.bind_buffer(ARRAY_BUFFER, Some(b));
        gl.buffer_data(ARRAY_BUFFER, Some(&floats(&data)), 0, STATIC_DRAW);
        gl.vertex_attrib_pointer(pl, 2, FLOAT, false, stride as i32, if swap { 12 } else { 0 }, false);
        gl.vertex_attrib_pointer(cl, 3, FLOAT, false, stride as i32, if swap { 0 } else { 8 }, false);
        gl.enable_vertex_attrib(pl, true);
        gl.enable_vertex_attrib(cl, true);
        gl.draw(TRIANGLES, 0, 3, 1, None);
        let mut px = [0u8; 4];
        gl.read_pixels(1, 59, 1, 1, RGBA, UNSIGNED_BYTE, &mut px);
        let mut q = [0u8; 4];
        gl.read_pixels(60, 1, 1, 1, RGBA, UNSIGNED_BYTE, &mut q);
        let mut r = [0u8; 4];
        gl.read_pixels(1, 1, 1, 1, RGBA, UNSIGNED_BYTE, &mut r);
        println!("stride {stride} swap {swap}: tl {px:?} br {q:?} bl {r:?}");
    }
}
