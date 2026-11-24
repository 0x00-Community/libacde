const ACDE_GLASS_MOST_PIECES: u32 = 16u;
const ACDE_GLASS_MERGE_NORMAL: f32 = 4.0;

struct AcdeGlassPiece {
    shape: vec4<f32>,
    corner: vec4<f32>,
    refraction: vec4<f32>,
    bleed: vec4<f32>,
    look: vec4<f32>,
    tint: vec4<f32>,
    lens: vec4<f32>,
    outline: vec4<f32>,
    mode: vec4<f32>,
}

struct AcdeGlassLook {
    refraction: vec4<f32>,
    bleed: vec4<f32>,
    look: vec4<f32>,
    tint: vec4<f32>,
    lens: vec4<f32>,
    outline: vec4<f32>,
    mode: vec4<f32>,
}

struct AcdeGlassField {
    d: f32,
    toward: vec2<f32>,
    merging: f32,
    bevel: f32,
    index: f32,
    line: f32,
    look: AcdeGlassLook,
    at: vec2<f32>,
}

fn acde_glass_piece_distance(i: u32, p: vec2<f32>) -> f32 {
    let piece = acde_glass_pieces[i];
    let half = piece.shape.zw;
    return acde_corner_distance(p - piece.shape.xy, half, min(piece.corner.x, min(half.x, half.y)), piece.corner.y);
}

fn acde_glass_piece_look(i: u32) -> AcdeGlassLook {
    let piece = acde_glass_pieces[i];
    return AcdeGlassLook(piece.refraction, piece.bleed, piece.look, piece.tint, piece.lens, piece.outline, piece.mode);
}

fn acde_glass_look_mix(a: AcdeGlassLook, b: AcdeGlassLook, t: f32) -> AcdeGlassLook {
    return AcdeGlassLook(mix(a.refraction, b.refraction, t), mix(a.bleed, b.bleed, t), mix(a.look, b.look, t), mix(a.tint, b.tint, t), mix(a.lens, b.lens, t), mix(a.outline, b.outline, t), mix(a.mode, b.mode, t));
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

fn acde_glass_group_slope(p: vec2<f32>, count: u32, spacing: f32) -> vec2<f32> {
    let e = 0.5;
    let dx = acde_glass_group_distance(p + vec2<f32>(e, 0.0), count, spacing) - acde_glass_group_distance(p - vec2<f32>(e, 0.0), count, spacing);
    let dy = acde_glass_group_distance(p + vec2<f32>(0.0, e), count, spacing) - acde_glass_group_distance(p - vec2<f32>(0.0, e), count, spacing);
    return vec2<f32>(dx, dy) / (2.0 * e);
}

fn acde_glass_group_gradient(p: vec2<f32>, count: u32, spacing: f32) -> vec2<f32> {
    let g = acde_glass_group_slope(p, count, spacing);
    return g / max(length(g), 0.000001);
}

fn acde_glass_group_field(p: vec2<f32>, count: u32, spacing: f32) -> AcdeGlassField {
    let k = max(spacing, 0.0001);
    let first = acde_glass_pieces[0];
    var d = acde_glass_piece_distance(0u, p);
    var toward = (p - first.shape.xy) / max(first.shape.zw, vec2<f32>(1.0, 1.0));
    var look = acde_glass_piece_look(0u);
    var bevel = min(first.shape.z, first.shape.w);
    var index = first.corner.z;
    var outline_width = first.corner.w;
    var merged_outline = first.mode.x;
    var pieces_line = acde_glass_outline_coverage(d, first.corner.w, first.outline.a);
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
        index = mix(index, piece.corner.z, h);
        outline_width = mix(outline_width, piece.corner.w, h);
        merged_outline = mix(merged_outline, piece.mode.x, h);
        pieces_line = max(pieces_line, acde_glass_outline_coverage(di, piece.corner.w, piece.outline.a));
        d = mix(d, di, h) - k * h * (1.0 - h);
        merging = max(merging, 4.0 * h * (1.0 - h));
    }
    var line_d = d;
    if merging > 0.0 {
        let slope = acde_glass_group_slope(p, count, spacing);
        toward = mix(toward / max(length(toward), 0.000001), slope / max(length(slope), 0.000001), min(1.0, ACDE_GLASS_MERGE_NORMAL * merging));
        line_d = d / max(length(slope), 0.05);
    }
    let line = select(pieces_line, acde_glass_outline_coverage(line_d, outline_width, look.outline.a), merged_outline > 0.5);
    return AcdeGlassField(d, toward, merging, bevel, index, line, look, p);
}

fn acde_glass_group_offset(f: AcdeGlassField) -> vec2<f32> {
    return acde_glass_offset_along(f.toward / max(length(f.toward), 0.000001), f.d, f.look.refraction);
}

fn acde_glass_group_lens(f: AcdeGlassField, p: vec2<f32>, count: u32, spacing: f32) -> AcdeGlassBend {
    if f.look.lens.x <= 0.0 || f.d > 0.0 || -f.d >= f.look.lens.w {
        return AcdeGlassBend(vec2<f32>(0.0), vec2<f32>(0.0), vec2<f32>(0.0));
    }
    return acde_glass_lens_rgb(acde_glass_group_gradient(p, count, spacing), f.d, f.bevel, f.look.lens, f.index);
}

fn acde_glass_group_shade(under: vec4<f32>, f: AcdeGlassField) -> vec4<f32> {
    let inside = acde_glass_inside(f.d, f.look.look.z);
    if inside <= 0.0 {
        return under * acde_glass_bleed(f.d, f.look.bleed.xyz);
    }
    let n = f.toward / max(length(f.toward), 0.000001);
    let shine = acde_glass_shine(n, f.d, f.look.look.w, f.look.bleed.w, f.look.look.x);
    let toned = acde_glass_over(under, f.look.look.y, f.look.tint, shine, f.look.lens.z);
    let rgb = acde_glass_film(toned.rgb, f.at, f.d, f.bevel, f.look.mode.yzw);
    let lined = acde_glass_lined(rgb, toned.a, f.line, f.look.outline.rgb);
    let alpha = lined.a * inside;
    return vec4<f32>(lined.rgb * alpha, alpha);
}
