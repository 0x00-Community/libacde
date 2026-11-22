fn acde_glass_normal(p: vec2<f32>, half: vec2<f32>) -> vec2<f32> {
    let q = p / max(half, vec2<f32>(1.0, 1.0));
    return q / max(length(q), 0.000001);
}

fn acde_glass_offset_along(n: vec2<f32>, d: f32, refraction: vec4<f32>) -> vec2<f32> {
    let inner = select(0.0, refraction.x * refraction.y * pow(1.0 + d / max(refraction.y, 0.001), 2.0), d <= 0.0 && -d < refraction.y);
    let outer = select(0.0, -refraction.z * refraction.w * pow(1.0 - d / max(refraction.w, 0.001), 2.0), d > 0.0 && d < refraction.w);
    return -n * (inner + outer);
}

fn acde_glass_offset(p: vec2<f32>, half: vec2<f32>, d: f32, refraction: vec4<f32>) -> vec2<f32> {
    return acde_glass_offset_along(acde_glass_normal(p, half), d, refraction);
}

fn acde_glass_shine(n: vec2<f32>, d: f32, band: f32, angle: f32, amount: f32) -> f32 {
    let b = max(band, 2.0);
    if -d > b || d > 0.0 {
        return 0.0;
    }
    let facing = max(dot(n, vec2<f32>(sin(angle), -cos(angle))), 0.0);
    return facing * facing * (1.0 + d / b) * amount;
}

fn acde_glass_highlight(p: vec2<f32>, half: vec2<f32>, d: f32, band: f32, angle: f32, amount: f32) -> f32 {
    return acde_glass_shine(acde_glass_normal(p, half), d, band, angle, amount);
}

const ACDE_GLASS_EDGE_BOOST: f32 = 5.0;
const ACDE_GLASS_EDGE_FALLOFF: f32 = 0.08;
const ACDE_GLASS_MOST_SHIFT: f32 = 1.3;

fn acde_glass_lens(n: vec2<f32>, d: f32, bevel: f32, strength: f32) -> vec2<f32> {
    let b = max(bevel, 1.0);
    let e = max(-d, 0.0);
    let rim = 1.0 - clamp(e / b, 0.0, 1.0);
    let boost = (1.0 + ACDE_GLASS_EDGE_BOOST * exp(-e / (ACDE_GLASS_EDGE_FALLOFF * b))) / (1.0 + ACDE_GLASS_EDGE_BOOST);
    let shift = min(max(strength, 0.0) * rim * rim * rim * boost, ACDE_GLASS_MOST_SHIFT * b);
    return select(-n * shift, vec2<f32>(0.0), d > 0.0);
}

fn acde_glass_tone(rgb: vec3<f32>, white_point: f32, tint: vec4<f32>, shine: f32) -> vec3<f32> {
    return acde_glass_tone_darkened(rgb, white_point, tint, shine, 0.0);
}

fn acde_glass_tone_darkened(rgb: vec3<f32>, white_point: f32, tint: vec4<f32>, shine: f32, darken: f32) -> vec3<f32> {
    let lifted = rgb + (vec3<f32>(1.0) - rgb) * white_point;
    let tinted = lifted * (1.0 - tint.a) + tint.rgb * tint.a;
    return clamp(tinted * (1.0 - darken) + vec3<f32>(shine), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn acde_glass_inside(d: f32, smoothing: f32) -> f32 {
    return clamp(0.5 - d / (1.0 + 4.0 * smoothing), 0.0, 1.0);
}

fn acde_glass_bleed(d: f32, bleed: vec3<f32>) -> f32 {
    return clamp(1.0 - d / max(bleed.y, 1.0), 0.0, 1.0) * bleed.x * bleed.z;
}
