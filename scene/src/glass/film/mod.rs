use std::sync::LazyLock;
use std::time::Instant;

use super::number;

static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Film {
    pub amount: f32,
    pub cell: f32,
}

impl Film {
    pub const INDEX: f32 = 1.33;
    pub const WAVES: [f32; 3] = [650.0, 532.0, 450.0];
    pub const BASE: f32 = 240.0;
    pub const SWING: f32 = 260.0;
    pub const DEPTH: f32 = 120.0;
    pub const CENTER: f32 = 0.25;
    pub const MOST_SLOPE: f32 = 7.0;
    pub const EDGE_FROM: f32 = 0.2;
    pub const EDGE_TO: f32 = 0.95;
    pub const FLOOR: f32 = 0.55;
    pub const SPAN: f32 = 0.9;
    pub const AMOUNT: f32 = 0.2;
    pub const CELL: f32 = 64.0;
    pub const NIGHT: f32 = 0.6;
    pub const DRIFT: [f32; 2] = [0.05, -0.035];

    pub fn clock() -> f32 {
        if crate::motion::system().moves() { STARTED.elapsed().as_secs_f32() } else { 0.0 }
    }

    pub fn uniform(self) -> [f32; 4] {
        [self.amount, self.cell, if self.amount > 0.0 { Self::clock() } else { 0.0 }, 0.0]
    }

    pub fn parse(text: &str, dark: bool) -> Option<Self> {
        let amount = match text {
            "none" => return Some(Self::default()),
            "auto" => Self::AMOUNT,
            given => number(given)?,
        };
        let night = if dark { 1.0 - Self::NIGHT } else { 1.0 };
        Some(Self { amount: amount.clamp(0.0, 1.0) * night, cell: Self::CELL })
    }

    pub fn shaded(self, rgb: [f32; 3], (px, py): (f64, f64), d: f64, half_short: f64, time: f32) -> [f32; 3] {
        if self.amount <= 0.0 || d > 0.0 {
            return rgb;
        }
        let x = (-d as f32 / (half_short as f32).max(0.5)).clamp(0.0, 1.0);
        let h = (1.0 - (1.0 - x) * (1.0 - x)).max(0.0).sqrt();
        let slope = if x < 1.0 { ((1.0 - x) / h.max(0.001)).min(Self::MOST_SLOPE) } else { 0.0 };
        let sin_in = slope / (1.0 + slope * slope).sqrt();
        let cos_through = (1.0 - sin_in * sin_in / (Self::INDEX * Self::INDEX)).max(0.0).sqrt();
        let cell = self.cell.max(1.0);
        let thickness = Self::BASE + Self::SWING * noise(px as f32 / cell + time * Self::DRIFT[0], py as f32 / cell + time * Self::DRIFT[1]) + Self::DEPTH * h;
        let weight = self.amount * (Self::CENTER + (1.0 - Self::CENTER) * smoothstep(Self::EDGE_FROM, Self::EDGE_TO, sin_in));
        let mut k = 0;
        rgb.map(|v| {
            let phase = std::f32::consts::TAU * 2.0 * Self::INDEX * thickness * cos_through / Self::WAVES[k];
            k += 1;
            let colored = v * (Self::FLOOR + Self::SPAN * (0.5 - 0.5 * phase.cos()));
            (v + (colored - v) * weight).clamp(0.0, 1.0)
        })
    }
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fract(v: f32) -> f32 {
    v - v.floor()
}

fn hash(x: f32, y: f32) -> f32 {
    let (qx, qy) = (fract(x * 233.34), fract(y * 851.73));
    let dot = qx * (qx + 23.45) + qy * (qy + 23.45);
    fract((qx + dot) * (qy + dot))
}

fn noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let top = mix(hash(ix, iy), hash(ix + 1.0, iy), sx);
    let bottom = mix(hash(ix, iy + 1.0), hash(ix + 1.0, iy + 1.0), sx);
    mix(top, bottom, sy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glass::WGSL;

    fn constant(name: &str) -> Vec<f32> {
        let line = WGSL.lines().find(|l| l.starts_with(&format!("const ACDE_GLASS_FILM_{name}:"))).unwrap_or_else(|| panic!("no {name} in film.wgsl"));
        let value = line.split_once('=').expect("a value").1.trim().trim_end_matches(';');
        let inner = value.strip_prefix("vec3<f32>(").or_else(|| value.strip_prefix("vec2<f32>(")).and_then(|v| v.strip_suffix(')')).unwrap_or(value);
        inner.split(',').map(|v| v.trim().parse().expect("a number")).collect()
    }

    #[test]
    fn the_shader_and_the_cpu_share_one_set_of_constants() {
        let named = [
            ("INDEX", vec![Film::INDEX]),
            ("WAVES", Film::WAVES.to_vec()),
            ("BASE", vec![Film::BASE]),
            ("SWING", vec![Film::SWING]),
            ("DEPTH", vec![Film::DEPTH]),
            ("CENTER", vec![Film::CENTER]),
            ("MOST_SLOPE", vec![Film::MOST_SLOPE]),
            ("EDGE_FROM", vec![Film::EDGE_FROM]),
            ("EDGE_TO", vec![Film::EDGE_TO]),
            ("FLOOR", vec![Film::FLOOR]),
            ("SPAN", vec![Film::SPAN]),
            ("DRIFT", Film::DRIFT.to_vec()),
        ];
        for (name, value) in &named {
            assert_eq!(&constant(name), value, "{name}");
        }
        assert_eq!(WGSL.lines().filter(|l| l.starts_with("const ACDE_GLASS_FILM_")).count(), named.len());
    }

    #[test]
    fn it_reads_a_strength_auto_or_none_and_fades_at_night() {
        assert_eq!(Film::parse("auto", false), Some(Film { amount: Film::AMOUNT, cell: Film::CELL }));
        assert_eq!(Film::parse("0.5", false), Some(Film { amount: 0.5, cell: Film::CELL }));
        assert_eq!(Film::parse("3", false).map(|f| f.amount), Some(1.0));
        assert_eq!(Film::parse("none", false), Some(Film::default()));
        assert_eq!(Film::parse("lots", false), None);
        let night = Film::parse("auto", true).expect("film").amount;
        assert!((night - Film::AMOUNT * (1.0 - Film::NIGHT)).abs() < 1e-6);
    }

    #[test]
    fn off_or_outside_it_changes_nothing() {
        let rgb = [0.9, 0.8, 0.7];
        assert_eq!(Film::default().shaded(rgb, (3.0, 4.0), -10.0, 40.0, 0.0), rgb);
        assert_eq!(Film::parse("auto", false).unwrap().shaded(rgb, (3.0, 4.0), 1.0, 40.0, 0.0), rgb);
    }

    #[test]
    fn the_middle_stays_near_white_and_the_edge_takes_more_color() {
        let film = Film { amount: 1.0, cell: Film::CELL };
        let tinted = |[r, g, b]: [f32; 3]| 1.0 - (r + g + b) / 3.0 + r.max(g).max(b) - r.min(g).min(b);
        let (mut middle, mut edge) = (0.0, 0.0);
        for i in 0..64 {
            let p = (f64::from(i) * 37.0, f64::from(i) * 11.0);
            middle += tinted(film.shaded([1.0; 3], p, -40.0, 40.0, 0.0));
            edge += tinted(film.shaded([1.0; 3], p, -1.0, 40.0, 0.0));
        }
        assert!(edge > 2.0 * middle, "edge {edge} middle {middle}");
    }

    #[test]
    fn the_noise_is_smooth_and_stays_in_range() {
        for i in 0..400 {
            let (x, y) = (i as f32 * 0.173, i as f32 * -0.291);
            let n = noise(x, y);
            assert!((0.0..=1.0).contains(&n));
            assert!((noise(x + 0.001, y) - n).abs() < 0.02);
        }
    }
}
