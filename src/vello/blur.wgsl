struct Params {
    color: vec4<f32>,
    sigma: f32,
    radius: i32,
    dir: vec2<i32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> p: Params;

@compute @workgroup_size(8, 8)
fn blur(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(textureDimensions(src));
    let at = vec2<i32>(id.xy);
    if at.x >= size.x || at.y >= size.y {
        return;
    }
    var sum = 0.0;
    var total = 0.0;
    for (var i = -p.radius; i <= p.radius; i++) {
        let w = exp(-f32(i * i) / (2.0 * p.sigma * p.sigma));
        let q = at + p.dir * i;
        total += w;
        if q.x >= 0 && q.y >= 0 && q.x < size.x && q.y < size.y {
            sum += w * textureLoad(src, q, 0).a;
        }
    }
    textureStore(dst, at, vec4<f32>(p.color.rgb, p.color.a * sum / total));
}

@compute @workgroup_size(8, 8)
fn blur_rgba(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(textureDimensions(src));
    let at = vec2<i32>(id.xy);
    if at.x >= size.x || at.y >= size.y {
        return;
    }
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var i = -p.radius; i <= p.radius; i++) {
        let w = exp(-f32(i * i) / (2.0 * p.sigma * p.sigma));
        let q = at + p.dir * i;
        total += w;
        if q.x >= 0 && q.y >= 0 && q.x < size.x && q.y < size.y {
            let c = textureLoad(src, q, 0);
            sum += w * vec4<f32>(c.rgb * c.a, c.a);
        }
    }
    let r = sum / total;
    var out = vec4<f32>(0.0);
    if r.a > 0.0 {
        out = vec4<f32>(r.rgb / r.a, r.a);
    }
    textureStore(dst, at, out);
}
