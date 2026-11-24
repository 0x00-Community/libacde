struct Shrink {
    first: vec2<u32>,
    stop: vec2<u32>,
    factor: u32,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> s: Shrink;

@compute @workgroup_size(8, 8)
fn shrink(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let start = s.first + id.xy * s.factor;
    let end = min(start + vec2<u32>(s.factor), s.stop);
    var sum = vec4<f32>(0.0);
    for (var y = start.y; y < end.y; y++) {
        for (var x = start.x; x < end.x; x++) {
            let c = textureLoad(src, vec2<i32>(i32(x), i32(y)), 0);
            sum += vec4<f32>(c.rgb * c.a, c.a);
        }
    }
    let n = f32((end.x - start.x) * (end.y - start.y));
    var out = vec4<f32>(0.0);
    if sum.a > 0.0 {
        out = vec4<f32>(sum.rgb / sum.a, sum.a / n);
    }
    textureStore(dst, vec2<i32>(id.xy), out);
}
