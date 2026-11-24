const ACDE_GLASS_FILM_INDEX: f32 = 1.33;
const ACDE_GLASS_FILM_WAVES: vec3<f32> = vec3<f32>(650.0, 532.0, 450.0);
const ACDE_GLASS_FILM_BASE: f32 = 240.0;
const ACDE_GLASS_FILM_SWING: f32 = 260.0;
const ACDE_GLASS_FILM_DEPTH: f32 = 120.0;
const ACDE_GLASS_FILM_CENTER: f32 = 0.25;
const ACDE_GLASS_FILM_MOST_SLOPE: f32 = 7.0;
const ACDE_GLASS_FILM_EDGE_FROM: f32 = 0.2;
const ACDE_GLASS_FILM_EDGE_TO: f32 = 0.95;
const ACDE_GLASS_FILM_FLOOR: f32 = 0.55;
const ACDE_GLASS_FILM_SPAN: f32 = 0.9;
const ACDE_GLASS_FILM_DRIFT: vec2<f32> = vec2<f32>(0.05, -0.035);

fn acde_glass_film_hash(c: vec2<f32>) -> f32 {
    var q = fract(c * vec2<f32>(233.34, 851.73));
    q += dot(q, q + 23.45);
    return fract(q.x * q.y);
}

fn acde_glass_film_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let s = f * f * (3.0 - 2.0 * f);
    let top = mix(acde_glass_film_hash(i), acde_glass_film_hash(i + vec2<f32>(1.0, 0.0)), s.x);
    let bottom = mix(acde_glass_film_hash(i + vec2<f32>(0.0, 1.0)), acde_glass_film_hash(i + vec2<f32>(1.0, 1.0)), s.x);
    return mix(top, bottom, s.y);
}

fn acde_glass_film(rgb: vec3<f32>, p: vec2<f32>, d: f32, half_short: f32, film: vec3<f32>) -> vec3<f32> {
    if film.x <= 0.0 || d > 0.0 {
        return rgb;
    }
    let x = clamp(-d / max(half_short, 0.5), 0.0, 1.0);
    let h = sqrt(max(1.0 - (1.0 - x) * (1.0 - x), 0.0));
    let slope = select(0.0, min((1.0 - x) / max(h, 0.001), ACDE_GLASS_FILM_MOST_SLOPE), x < 1.0);
    let sin_in = slope / sqrt(1.0 + slope * slope);
    let cos_through = sqrt(max(1.0 - sin_in * sin_in / (ACDE_GLASS_FILM_INDEX * ACDE_GLASS_FILM_INDEX), 0.0));
    let thickness = ACDE_GLASS_FILM_BASE + ACDE_GLASS_FILM_SWING * acde_glass_film_noise(p / max(film.y, 1.0) + film.z * ACDE_GLASS_FILM_DRIFT) + ACDE_GLASS_FILM_DEPTH * h;
    let phase = 6.2831855 * 2.0 * ACDE_GLASS_FILM_INDEX * thickness * cos_through / ACDE_GLASS_FILM_WAVES;
    let color = 0.5 - 0.5 * cos(phase);
    let weight = film.x * (ACDE_GLASS_FILM_CENTER + (1.0 - ACDE_GLASS_FILM_CENTER) * smoothstep(ACDE_GLASS_FILM_EDGE_FROM, ACDE_GLASS_FILM_EDGE_TO, sin_in));
    return clamp(mix(rgb, rgb * (ACDE_GLASS_FILM_FLOOR + ACDE_GLASS_FILM_SPAN * color), weight), vec3<f32>(0.0), vec3<f32>(1.0));
}
