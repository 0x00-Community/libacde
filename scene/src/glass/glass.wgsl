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

const ACDE_GLASS_LENS_STEPS: u32 = 64u;
const ACDE_GLASS_LENS_LEAST_SLOPE: f32 = 0.1;

fn acde_glass_bezel_height(x: f32) -> f32 {
    let u = 1.0 - clamp(x, 0.0, 1.0);
    return pow(max(1.0 - u * u * u * u, 0.0), 0.25);
}

fn acde_glass_bezel_slope(x: f32) -> f32 {
    let u = 1.0 - clamp(x, 0.000001, 1.0);
    return u * u * u * pow(max(1.0 - u * u * u * u, 0.000001), -0.75);
}

fn acde_glass_bent_depth(x: f32, ratio: f32, index: f32) -> f32 {
    let into = atan(ratio * acde_glass_bezel_slope(x));
    let out = asin(clamp(sin(into) / index, -1.0, 1.0));
    return x + ratio * acde_glass_bezel_height(x) * tan(into - out);
}

fn acde_glass_lens_node(j: u32, ratio: f32, index: f32) -> f32 {
    return acde_glass_bent_depth(f32(j) / f32(ACDE_GLASS_LENS_STEPS), ratio, index);
}

fn acde_glass_lens_depth(x: f32, ratio: f32, index: f32) -> f32 {
    let at = clamp(x, 0.0, 1.0) * f32(ACDE_GLASS_LENS_STEPS);
    let i = min(u32(floor(at)), ACDE_GLASS_LENS_STEPS - 1u);
    var best = mix(acde_glass_lens_node(i, ratio, index), acde_glass_lens_node(i + 1u, ratio, index), at - f32(i));
    for (var j = i + 1u; j <= ACDE_GLASS_LENS_STEPS; j++) {
        let ahead = f32(j) / f32(ACDE_GLASS_LENS_STEPS) - x;
        best = min(best, acde_glass_lens_node(j, ratio, index) - ACDE_GLASS_LENS_LEAST_SLOPE * ahead);
    }
    return best;
}

fn acde_glass_bends(d: f32, half_short: f32, lens: vec4<f32>, index: f32) -> bool {
    let bezel = min(max(lens.w, 0.5), max(half_short, 0.5));
    return d <= 0.0 && -d < bezel && lens.x > 0.0 && index > 1.0;
}

fn acde_glass_lens(n: vec2<f32>, d: f32, half_short: f32, lens: vec4<f32>, index: f32) -> vec2<f32> {
    let bezel = min(max(lens.w, 0.5), max(half_short, 0.5));
    let e = -d;
    if !acde_glass_bends(d, half_short, lens, index) {
        return vec2<f32>(0.0);
    }
    let source = bezel * acde_glass_lens_depth(e / bezel, lens.x / bezel, index);
    return -n * (source - e);
}

struct AcdeGlassBend {
    red: vec2<f32>,
    green: vec2<f32>,
    blue: vec2<f32>,
}

fn acde_glass_lens_rgb(n: vec2<f32>, d: f32, half_short: f32, lens: vec4<f32>, index: f32) -> AcdeGlassBend {
    let green = acde_glass_lens(n, d, half_short, lens, index);
    if lens.y <= 0.0 {
        return AcdeGlassBend(green, green, green);
    }
    let red = acde_glass_lens(n, d, half_short, lens, index - lens.y);
    let blue = acde_glass_lens(n, d, half_short, lens, index + lens.y);
    return AcdeGlassBend(red, green, blue);
}

fn acde_glass_tone(rgb: vec3<f32>, white_point: f32, tint: vec4<f32>, shine: f32) -> vec3<f32> {
    return acde_glass_tone_darkened(rgb, white_point, tint, shine, 0.0);
}

fn acde_glass_tone_darkened(rgb: vec3<f32>, white_point: f32, tint: vec4<f32>, shine: f32, darken: f32) -> vec3<f32> {
    let lifted = rgb + (vec3<f32>(1.0) - rgb) * white_point;
    let tinted = lifted * (1.0 - tint.a) + tint.rgb * tint.a;
    return clamp(tinted * (1.0 - darken) + vec3<f32>(shine), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn acde_glass_over(under: vec4<f32>, white_point: f32, tint: vec4<f32>, shine: f32, darken: f32) -> vec4<f32> {
    let a = under.a;
    let straight = select(vec3<f32>(0.0), under.rgb / a, a > 0.0);
    let seen = acde_glass_tone_darkened(straight, white_point, tint, shine, darken);
    let bare = clamp(tint.rgb * (1.0 - darken) + vec3<f32>(shine), vec3<f32>(0.0), vec3<f32>(1.0));
    let bare_alpha = tint.a * (1.0 - a);
    let alpha = a + bare_alpha;
    return vec4<f32>(select(bare, (seen * a + bare * bare_alpha) / alpha, alpha > 0.0), alpha);
}

fn acde_glass_outline_coverage(d: f32, width: f32, alpha: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    return clamp(width + 0.5 + d, 0.0, 1.0) * clamp(0.5 - d, 0.0, 1.0) * alpha;
}

fn acde_glass_lined(rgb: vec3<f32>, base_alpha: f32, line: f32, color: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(mix(rgb, color, line), max(base_alpha, line));
}

fn acde_glass_outlined(rgb: vec3<f32>, base_alpha: f32, d: f32, width: f32, outline: vec4<f32>) -> vec4<f32> {
    return acde_glass_lined(rgb, base_alpha, acde_glass_outline_coverage(d, width, outline.a), outline.rgb);
}

fn acde_glass_inside(d: f32, smoothing: f32) -> f32 {
    return clamp(0.5 - d / (1.0 + 4.0 * smoothing), 0.0, 1.0);
}

fn acde_glass_bleed(d: f32, bleed: vec3<f32>) -> f32 {
    return clamp(1.0 - d / max(bleed.y, 1.0), 0.0, 1.0) * bleed.x * bleed.z;
}
