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

const ACDE_GLASS_FAR_EDGE: f32 = 0.33;

fn acde_glass_shine(n: vec2<f32>, d: f32, band: f32, angle: f32, amount: f32) -> f32 {
    let b = max(band, 0.5);
    if -d > b || d > 0.0 {
        return 0.0;
    }
    let toward = dot(n, vec2<f32>(sin(angle), -cos(angle)));
    let facing = max(toward, 0.0);
    let away = max(-toward, 0.0);
    return (facing * facing + ACDE_GLASS_FAR_EDGE * away * away) * (1.0 + d / b) * amount;
}

fn acde_glass_highlight(p: vec2<f32>, half: vec2<f32>, d: f32, band: f32, angle: f32, amount: f32) -> f32 {
    return acde_glass_shine(acde_glass_normal(p, half), d, band, angle, amount);
}

fn acde_glass_corner_gradient(p: vec2<f32>, half: vec2<f32>, radius: f32, exponent: f32) -> vec2<f32> {
    let e = vec2<f32>(0.5, 0.0);
    let dx = acde_corner_distance(p + e.xy, half, radius, exponent) - acde_corner_distance(p - e.xy, half, radius, exponent);
    let dy = acde_corner_distance(p + e.yx, half, radius, exponent) - acde_corner_distance(p - e.yx, half, radius, exponent);
    let g = vec2<f32>(dx, dy);
    return g / max(length(g), 0.000001);
}

const ACDE_GLASS_LENS_MOST_STRENGTH: f32 = 100.0;
const ACDE_GLASS_LENS_MOST_PULL: f32 = 0.95;
const ACDE_GLASS_LENS_MOST_STEEPNESS: f32 = 20.0;

fn acde_glass_lens(n: vec2<f32>, d: f32, bevel: f32, lens: vec4<f32>) -> vec2<f32> {
    let width = min(max(lens.w, 0.5), max(bevel, 0.5));
    let t = clamp(-d / width, 0.0, 1.0);
    let k = clamp(lens.x, 0.0, ACDE_GLASS_LENS_MOST_STRENGTH) / ACDE_GLASS_LENS_MOST_STRENGTH;
    let pull = ACDE_GLASS_LENS_MOST_PULL * k;
    let power = 1.0 + ACDE_GLASS_LENS_MOST_STEEPNESS * k;
    let shift = width * pull * (1.0 - pow(1.0 - t, power) - t);
    return select(-n * shift, vec2<f32>(0.0), d > 0.0 || -d >= width || lens.x <= 0.0);
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
