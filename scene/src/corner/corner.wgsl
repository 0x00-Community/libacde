fn acde_corner_distance(p: vec2<f32>, half: vec2<f32>, radius: f32, exponent: f32) -> f32 {
    let a = abs(p);
    let r = clamp(radius, 0.0, min(half.x, half.y));
    let c = half - vec2<f32>(r, r);
    if r > 0.0 && a.x > c.x && a.y > c.y {
        let n = max(exponent, 1.0);
        let u = (a - c) / r;
        return (pow(pow(u.x, n) + pow(u.y, n), 1.0 / n) - 1.0) * r;
    }
    return max(a.x - half.x, a.y - half.y);
}

fn acde_corner_coverage(p: vec2<f32>, half: vec2<f32>, radius: f32, exponent: f32) -> f32 {
    return clamp(0.5 - acde_corner_distance(p, half, radius, exponent), 0.0, 1.0);
}
