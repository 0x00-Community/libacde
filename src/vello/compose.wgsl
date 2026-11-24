struct Params {
    texel: vec2<f32>,
    offset: f32,
    spare0: f32,
    rect: vec4<f32>,
    radius: f32,
    exponent: f32,
    shift: vec2<f32>,
    color: vec4<f32>,
    matrix: mat4x4<f32>,
    bias: vec4<f32>,
    outline: vec4<f32>,
    table: vec4<f32>,
    film: vec4<f32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var<uniform> acde_glass_pieces: array<AcdeGlassPiece, ACDE_GLASS_MOST_PIECES>;
@group(0) @binding(4) var acde_glass_lens_rows: texture_2d<f32>;

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

@fragment
fn fs_backdrop(in: VsOut) -> @location(0) vec4<f32> {
    let px = in.pos.xy;
    let local = (px - params.rect.xy) / params.rect.zw;
    let p = textureSample(src, samp, clamp(local, vec2<f32>(0.0), vec2<f32>(1.0)));
    let straight = select(vec4<f32>(0.0), vec4<f32>(p.rgb / p.a, p.a), p.a > 0.0);
    let m = clamp(params.matrix * straight + params.bias, vec4<f32>(0.0), vec4<f32>(1.0));
    let c = vec4<f32>(m.rgb * m.a, m.a);
    let half = params.rect.zw * 0.5;
    let mask = acde_corner_coverage(px - params.rect.xy - half, half, params.radius, params.exponent);
    return c * mask;
}

fn glass_sample(at: vec2<f32>, region: vec4<f32>) -> vec4<f32> {
    return textureSample(src, samp, clamp((at - region.xy) / region.zw, vec2<f32>(0.0), vec2<f32>(1.0)));
}

fn glass_under(at: vec2<f32>, bend: AcdeGlassBend, region: vec4<f32>) -> vec4<f32> {
    let red = glass_sample(at + bend.red, region);
    let green = glass_sample(at + bend.green, region);
    let blue = glass_sample(at + bend.blue, region);
    return vec4<f32>(red.r, green.g, blue.b, green.a);
}

@fragment
fn fs_glass(in: VsOut) -> @location(0) vec4<f32> {
    let px = in.pos.xy;
    let half = params.rect.zw * 0.5;
    let p = px - params.rect.xy - half;
    let radius = min(params.radius, min(half.x, half.y));
    let d = acde_corner_distance(p, half, radius, params.exponent);
    let refraction = params.matrix[0];
    let g1 = params.matrix[1];
    let g2 = params.matrix[2];
    let lens = params.matrix[3];
    let region = params.bias;
    let at = px + acde_glass_offset(p, half, d, refraction);
    var bend = AcdeGlassBend(vec2<f32>(0.0), vec2<f32>(0.0), vec2<f32>(0.0));
    if acde_glass_bends(d, min(half.x, half.y), lens, params.shift.x) {
        bend = acde_glass_lens_rgb_tabled(acde_glass_corner_gradient(p, half, radius, params.exponent), d, min(half.x, half.y), lens, params.shift.x, params.table);
    }
    let under = glass_under(at, bend, region);
    let inside = acde_glass_inside(d, g2.z);
    if inside <= 0.0 {
        return under * acde_glass_bleed(d, g1.xyz);
    }
    let straight = select(vec3<f32>(0.0), under.rgb / under.a, under.a > 0.0);
    let shine = acde_glass_highlight(p, half, d, g2.w, g1.w, g2.x);
    let toned = acde_glass_tone_darkened(straight, g2.y, params.color, shine, lens.z);
    let rgb = acde_glass_film(toned, px, d, min(half.x, half.y), params.film.xyz);
    let lined = acde_glass_outlined(rgb, max(under.a, params.color.a), d, params.shift.y, params.outline);
    let alpha = lined.a * inside;
    return vec4<f32>(lined.rgb * alpha, alpha);
}

@fragment
fn fs_glass_group(in: VsOut) -> @location(0) vec4<f32> {
    let px = in.pos.xy;
    let count = u32(params.shift.x);
    let f = acde_glass_group_field(px, count, params.shift.y);
    let at = px + acde_glass_group_offset(f);
    let under = glass_under(at, acde_glass_group_lens_tabled(f, px, count, params.shift.y, params.table), params.bias);
    return acde_glass_group_shade(under, f);
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
