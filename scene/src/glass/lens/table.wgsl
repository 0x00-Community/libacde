const ACDE_GLASS_TABLE_SIZE: u32 = 2048u;
const ACDE_GLASS_TABLE_POWER: f32 = 3.0;

fn acde_glass_table_depth(row: u32, x: f32) -> f32 {
    let at = pow(clamp(x, 0.0, 1.0), 1.0 / ACDE_GLASS_TABLE_POWER) * f32(ACDE_GLASS_TABLE_SIZE - 1u);
    let k = min(u32(floor(at)), ACDE_GLASS_TABLE_SIZE - 2u);
    let a = textureLoad(acde_glass_lens_rows, vec2<u32>(k, row), 0).r;
    let b = textureLoad(acde_glass_lens_rows, vec2<u32>(k + 1u, row), 0).r;
    return mix(a, b, at - f32(k));
}

fn acde_glass_lens_tabled(n: vec2<f32>, d: f32, half_short: f32, lens: vec4<f32>, index: f32, table: vec4<f32>, channel: u32) -> vec2<f32> {
    let bezel = min(max(lens.w, 0.5), max(half_short, 0.5));
    let e = -d;
    if d > 0.0 || e >= bezel || lens.x <= 0.0 || index <= 1.0 {
        return vec2<f32>(0.0);
    }
    let fits = table.x >= 0.0 && abs(lens.x / bezel - table.y) <= 0.0001 * max(table.y, 1.0) && abs(index - table.z) <= 0.0001 && abs(lens.y - table.w) <= 0.0001;
    if !fits {
        return acde_glass_lens(n, d, half_short, lens, index + lens.y * (f32(channel) - 1.0));
    }
    let source = bezel * acde_glass_table_depth(u32(table.x) * 3u + channel, e / bezel);
    return -n * (source - e);
}

fn acde_glass_lens_rgb_tabled(n: vec2<f32>, d: f32, half_short: f32, lens: vec4<f32>, index: f32, table: vec4<f32>) -> AcdeGlassBend {
    let green = acde_glass_lens_tabled(n, d, half_short, lens, index, table, 1u);
    if lens.y <= 0.0 {
        return AcdeGlassBend(green, green, green);
    }
    return AcdeGlassBend(acde_glass_lens_tabled(n, d, half_short, lens, index, table, 0u), green, acde_glass_lens_tabled(n, d, half_short, lens, index, table, 2u));
}

fn acde_glass_group_lens_tabled(f: AcdeGlassField, p: vec2<f32>, count: u32, spacing: f32, table: vec4<f32>) -> AcdeGlassBend {
    if f.look.lens.x <= 0.0 || f.index <= 1.0 || f.d > 0.0 || -f.d >= f.look.lens.w {
        return AcdeGlassBend(vec2<f32>(0.0), vec2<f32>(0.0), vec2<f32>(0.0));
    }
    return acde_glass_lens_rgb_tabled(acde_glass_group_gradient(p, count, spacing), f.d, f.bevel, f.look.lens, f.index, table);
}
