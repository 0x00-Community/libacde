struct Params {
    texel: vec2<f32>,
    offset: f32,
    spare0: f32,
    rect: vec4<f32>,
    radius: f32,
    spare1: f32,
    shift: vec2<f32>,
    color: vec4<f32>,
    matrix: mat4x4<f32>,
    bias: vec4<f32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VsOut;
    out.pos = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_clear(in: VsOut) -> @location(0) vec4<f32> {
    return params.color;
}

@fragment
fn fs_place(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(in.pos.xy - params.rect.xy), 0);
    return vec4<f32>(c.rgb * c.a, c.a);
}

@fragment
fn fs_down(in: VsOut) -> @location(0) vec4<f32> {
    let h = params.texel * params.offset;
    var sum = textureSample(src, samp, in.uv) * 4.0;
    sum += textureSample(src, samp, in.uv - h);
    sum += textureSample(src, samp, in.uv + h);
    sum += textureSample(src, samp, in.uv + vec2<f32>(h.x, -h.y));
    sum += textureSample(src, samp, in.uv - vec2<f32>(h.x, -h.y));
    return sum / 8.0;
}

@fragment
fn fs_up(in: VsOut) -> @location(0) vec4<f32> {
    let h = params.texel * params.offset;
    var sum = textureSample(src, samp, in.uv + vec2<f32>(-h.x * 2.0, 0.0));
    sum += textureSample(src, samp, in.uv + vec2<f32>(-h.x, h.y)) * 2.0;
    sum += textureSample(src, samp, in.uv + vec2<f32>(0.0, h.y * 2.0));
    sum += textureSample(src, samp, in.uv + vec2<f32>(h.x, h.y)) * 2.0;
    sum += textureSample(src, samp, in.uv + vec2<f32>(h.x * 2.0, 0.0));
    sum += textureSample(src, samp, in.uv + vec2<f32>(h.x, -h.y)) * 2.0;
    sum += textureSample(src, samp, in.uv + vec2<f32>(0.0, -h.y * 2.0));
    sum += textureSample(src, samp, in.uv + vec2<f32>(-h.x, -h.y)) * 2.0;
    return sum / 12.0;
}

fn rounded_rect(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_backdrop(in: VsOut) -> @location(0) vec4<f32> {
    let px = in.pos.xy;
    let local = (px - params.rect.xy) / params.rect.zw;
    let p = textureSample(src, samp, clamp(local, vec2<f32>(0.0), vec2<f32>(1.0)));
    let straight = select(vec4<f32>(0.0), vec4<f32>(p.rgb / p.a, p.a), p.a > 0.0);
    let m = clamp(params.matrix * straight + params.bias, vec4<f32>(0.0), vec4<f32>(1.0));
    let c = vec4<f32>(m.rgb * m.a, m.a);
    let half = params.rect.zw * 0.5;
    let d = rounded_rect(px - params.rect.xy - half, half, min(params.radius, min(half.x, half.y)));
    let mask = clamp(0.5 - d, 0.0, 1.0);
    return c * mask;
}

@fragment
fn fs_shadow(in: VsOut) -> @location(0) vec4<f32> {
    let local = (in.pos.xy - params.shift - params.rect.xy) / params.rect.zw;
    if any(local < vec2<f32>(0.0)) || any(local > vec2<f32>(1.0)) {
        return vec4<f32>(0.0);
    }
    let a = textureSample(src, samp, params.bias.xy + local * params.bias.zw).a * params.color.a;
    return vec4<f32>(params.color.rgb * a, a);
}

@fragment
fn fs_filter(in: VsOut) -> @location(0) vec4<f32> {
    let p = textureSample(src, samp, (in.pos.xy - params.rect.xy) / params.rect.zw);
    let c = select(vec4<f32>(0.0), vec4<f32>(p.rgb / p.a, p.a), p.a > 0.0);
    let m = clamp(params.matrix * c + params.bias, vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(m.rgb * m.a, m.a);
}
