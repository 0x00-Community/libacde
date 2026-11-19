struct Matrix {
    rows: array<vec4<f32>, 4>,
    bias: vec4<f32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> m: Matrix;

@compute @workgroup_size(8, 8)
fn color_matrix(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(textureDimensions(src));
    let at = vec2<i32>(id.xy);
    if at.x >= size.x || at.y >= size.y {
        return;
    }
    let c = textureLoad(src, at, 0);
    let out = vec4<f32>(dot(m.rows[0], c), dot(m.rows[1], c), dot(m.rows[2], c), dot(m.rows[3], c)) + m.bias;
    textureStore(dst, at, clamp(out, vec4<f32>(0.0), vec4<f32>(1.0)));
}
