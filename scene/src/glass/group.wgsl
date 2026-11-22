const ACDE_GLASS_MOST_PIECES: u32 = 16u;

struct AcdeGlassPiece {
    shape: vec4<f32>,
    corner: vec4<f32>,
    refraction: vec4<f32>,
    bleed: vec4<f32>,
    look: vec4<f32>,
    tint: vec4<f32>,
    lens: vec4<f32>,
}

struct AcdeGlassLook {
    refraction: vec4<f32>,
    bleed: vec4<f32>,
    look: vec4<f32>,
    tint: vec4<f32>,
    lens: vec4<f32>,
}

struct AcdeGlassField {
    d: f32,
    toward: vec2<f32>,
    merging: f32,
    bevel: f32,
    look: AcdeGlassLook,
}

fn acde_glass_piece_distance(i: u32, p: vec2<f32>) -> f32 {
    let piece = acde_glass_pieces[i];
    let half = piece.shape.zw;
    return acde_corner_distance(p - piece.shape.xy, half, min(piece.corner.x, min(half.x, half.y)), piece.corner.y);
}

fn acde_glass_piece_look(i: u32) -> AcdeGlassLook {
    let piece = acde_glass_pieces[i];
    return AcdeGlassLook(piece.refraction, piece.bleed, piece.look, piece.tint, piece.lens);
}

fn acde_glass_look_mix(a: AcdeGlassLook, b: AcdeGlassLook, t: f32) -> AcdeGlassLook {
    return AcdeGlassLook(mix(a.refraction, b.refraction, t), mix(a.bleed, b.bleed, t), mix(a.look, b.look, t), mix(a.tint, b.tint, t), mix(a.lens, b.lens, t));
}

fn acde_glass_smooth_weight(d: f32, di: f32, k: f32) -> f32 {
    return clamp(0.5 + 0.5 * (d - di) / k, 0.0, 1.0);
}

fn acde_glass_group_distance(p: vec2<f32>, count: u32, spacing: f32) -> f32 {
    let k = max(spacing, 0.0001);
    var d = acde_glass_piece_distance(0u, p);
    for (var i = 1u; i < min(count, ACDE_GLASS_MOST_PIECES); i++) {
        let di = acde_glass_piece_distance(i, p);
        let h = acde_glass_smooth_weight(d, di, k);
        d = mix(d, di, h) - k * h * (1.0 - h);
    }
    return d;
}

fn acde_glass_group_gradient(p: vec2<f32>, count: u32, spacing: f32) -> vec2<f32> {
    let e = 0.5;
    let dx = acde_glass_group_distance(p + vec2<f32>(e, 0.0), count, spacing) - acde_glass_group_distance(p - vec2<f32>(e, 0.0), count, spacing);
    let dy = acde_glass_group_distance(p + vec2<f32>(0.0, e), count, spacing) - acde_glass_group_distance(p - vec2<f32>(0.0, e), count, spacing);
    let g = vec2<f32>(dx, dy);
    return g / max(length(g), 0.000001);
}

fn acde_glass_group_field(p: vec2<f32>, count: u32, spacing: f32) -> AcdeGlassField {
    let k = max(spacing, 0.0001);
    let first = acde_glass_pieces[0];
    var d = acde_glass_piece_distance(0u, p);
    var toward = (p - first.shape.xy) / max(first.shape.zw, vec2<f32>(1.0, 1.0));
    var look = acde_glass_piece_look(0u);
    var bevel = min(first.shape.z, first.shape.w);
    var merging = 0.0;
    for (var i = 1u; i < min(count, ACDE_GLASS_MOST_PIECES); i++) {
        let piece = acde_glass_pieces[i];
        let di = acde_glass_piece_distance(i, p);
        let h = acde_glass_smooth_weight(d, di, k);
        let own = (p - piece.shape.xy) / max(piece.shape.zw, vec2<f32>(1.0, 1.0));
        if h >= 1.0 {
            toward = own;
        } else if h > 0.0 {
            toward = mix(toward / max(length(toward), 0.000001), own / max(length(own), 0.000001), h);
        }
        look = acde_glass_look_mix(look, acde_glass_piece_look(i), h);
        bevel = mix(bevel, min(piece.shape.z, piece.shape.w), h);
        d = mix(d, di, h) - k * h * (1.0 - h);
        merging = max(merging, 4.0 * h * (1.0 - h));
    }
    if merging > 0.0 {
        toward = mix(toward / max(length(toward), 0.000001), acde_glass_group_gradient(p, count, spacing), merging);
    }
    return AcdeGlassField(d, toward, merging, bevel, look);
}

fn acde_glass_group_offset(f: AcdeGlassField) -> vec2<f32> {
    return acde_glass_offset_along(f.toward / max(length(f.toward), 0.000001), f.d, f.look.refraction);
}

fn acde_glass_group_lens(f: AcdeGlassField) -> vec2<f32> {
    return acde_glass_lens(f.toward / max(length(f.toward), 0.000001), f.d, f.bevel, f.look.lens.x);
}

fn acde_glass_group_shade(under: vec4<f32>, f: AcdeGlassField) -> vec4<f32> {
    let inside = acde_glass_inside(f.d, f.look.look.z);
    if inside <= 0.0 {
        return under * acde_glass_bleed(f.d, f.look.bleed.xyz);
    }
    let straight = select(vec3<f32>(0.0), under.rgb / under.a, under.a > 0.0);
    let n = f.toward / max(length(f.toward), 0.000001);
    let shine = acde_glass_shine(n, f.d, f.look.refraction.y, f.look.bleed.w, f.look.look.x);
    let rgb = acde_glass_tone_darkened(straight, f.look.look.y, f.look.tint, shine, f.look.lens.z);
    let alpha = max(under.a, f.look.tint.a) * inside;
    return vec4<f32>(rgb * alpha, alpha);
}
