#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Filter {
    pub sigma: f64,
    pub saturate: f64,
    pub brightness: f64,
}

pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u32>,
}

const MOST_SHRINK: usize = 4;
const SIGMA_PER_SHRINK: f64 = 8.0;

pub fn margin(sigma: f64) -> i32 {
    (sigma * 3.0).ceil() as i32
}

pub fn frost(source: &Pixels, filter: Filter) -> Pixels {
    let mut out = match filter.sigma >= 0.5 {
        true => blur(source, filter.sigma),
        false => Pixels { width: source.width, height: source.height, data: source.data.clone() },
    };
    if filter.saturate != 1.0 || filter.brightness != 1.0 {
        out.data.iter_mut().for_each(|pixel| *pixel = tone(*pixel, filter.saturate, filter.brightness));
    }
    out
}

pub fn steps(sigma: f64) -> (usize, [usize; 3]) {
    let shrink = ((sigma / SIGMA_PER_SHRINK) as usize).clamp(1, MOST_SHRINK);
    (shrink, boxes(sigma / shrink as f64))
}

pub fn blur(source: &Pixels, sigma: f64) -> Pixels {
    let (shrink, sizes) = steps(sigma);
    let mut small = shrunk(source, shrink);
    let mut spare = vec![0u32; small.data.len()];
    for size in sizes {
        let radius = size / 2;
        box_pass(&small.data, &mut spare, small.width, small.height, radius, true);
        box_pass(&spare, &mut small.data, small.width, small.height, radius, false);
    }
    grown(&small, shrink, source.width, source.height)
}

fn boxes(sigma: f64) -> [usize; 3] {
    let n = 3.0;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let upper = lower + 2;
    let l = lower as f64;
    let m = ((12.0 * sigma * sigma - n * l * l - 4.0 * n * l - 3.0 * n) / (-4.0 * l - 4.0)).round() as i64;
    [0, 1, 2].map(|i| if i < m { lower as usize } else { upper as usize })
}

pub fn channels(pixel: u32) -> [u32; 4] {
    [pixel >> 24, (pixel >> 16) & 0xff, (pixel >> 8) & 0xff, pixel & 0xff]
}

pub fn pack([a, r, g, b]: [u32; 4]) -> u32 {
    (a.min(255) << 24) | (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)
}

fn box_pass(from: &[u32], to: &mut [u32], width: usize, height: usize, radius: usize, across: bool) {
    let (lines, length) = if across { (height, width) } else { (width, height) };
    let at = |line: usize, i: usize| if across { line * width + i } else { i * width + line };
    let span = (2 * radius + 1) as u32;
    for line in 0..lines {
        let pick = |i: isize| channels(from[at(line, i.clamp(0, length as isize - 1) as usize)]);
        let mut sum = [0u32; 4];
        for i in -(radius as isize)..=(radius as isize) {
            let c = pick(i);
            (0..4).for_each(|k| sum[k] += c[k]);
        }
        for i in 0..length {
            to[at(line, i)] = pack(sum.map(|s| (s + span / 2) / span));
            let (enter, leave) = (pick((i + radius + 1) as isize), pick(i as isize - radius as isize));
            (0..4).for_each(|k| sum[k] = sum[k] + enter[k] - leave[k]);
        }
    }
}

fn shrunk(source: &Pixels, by: usize) -> Pixels {
    if by == 1 {
        return Pixels { width: source.width, height: source.height, data: source.data.clone() };
    }
    let (width, height) = (source.width.div_ceil(by), source.height.div_ceil(by));
    let mut data = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let (mut sum, mut count) = ([0u32; 4], 0u32);
            for sy in y * by..((y + 1) * by).min(source.height) {
                for sx in x * by..((x + 1) * by).min(source.width) {
                    let c = channels(source.data[sy * source.width + sx]);
                    (0..4).for_each(|k| sum[k] += c[k]);
                    count += 1;
                }
            }
            data.push(pack(sum.map(|s| (s + count / 2) / count)));
        }
    }
    Pixels { width, height, data }
}

fn grown(small: &Pixels, by: usize, width: usize, height: usize) -> Pixels {
    if by == 1 {
        return Pixels { width, height, data: small.data.clone() };
    }
    let sample = |x: f64, y: f64| {
        let (fx, fy) = (x.clamp(0.0, (small.width - 1) as f64), y.clamp(0.0, (small.height - 1) as f64));
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(small.width - 1), (y0 + 1).min(small.height - 1));
        let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
        let c = |x: usize, y: usize| channels(small.data[y * small.width + x]).map(f64::from);
        let (a, b, c0, d) = (c(x0, y0), c(x1, y0), c(x0, y1), c(x1, y1));
        let mix = |k: usize| {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bottom = c0[k] + (d[k] - c0[k]) * tx;
            (top + (bottom - top) * ty).round() as u32
        };
        pack([mix(0), mix(1), mix(2), mix(3)])
    };
    let scale = by as f64;
    let mut data = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            data.push(sample((x as f64 + 0.5) / scale - 0.5, (y as f64 + 0.5) / scale - 0.5));
        }
    }
    Pixels { width, height, data }
}

pub fn tone(pixel: u32, s: f64, brightness: f64) -> u32 {
    let [a, r, g, b] = channels(pixel).map(f64::from);
    let red = (0.213 + 0.787 * s) * r + (0.715 - 0.715 * s) * g + (0.072 - 0.072 * s) * b;
    let green = (0.213 - 0.213 * s) * r + (0.715 + 0.285 * s) * g + (0.072 - 0.072 * s) * b;
    let blue = (0.213 - 0.213 * s) * r + (0.715 - 0.715 * s) * g + (0.072 + 0.928 * s) * b;
    let fit = |v: f64| (v * brightness).round().clamp(0.0, a) as u32;
    pack([a as u32, fit(red), fit(green), fit(blue)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: usize, height: usize, pixel: u32) -> Pixels {
        Pixels { width, height, data: vec![pixel; width * height] }
    }

    #[test]
    fn a_flat_colour_stays_flat_under_any_blur() {
        for sigma in [1.0, 6.0, 20.0, 40.0] {
            let out = frost(&flat(37, 23, 0xff40_8020), Filter { sigma, saturate: 1.0, brightness: 1.0 });
            assert!(out.data.iter().all(|p| *p == 0xff40_8020), "sigma {sigma}");
            assert_eq!((out.width, out.height), (37, 23));
        }
    }

    #[test]
    fn a_sharp_edge_turns_into_a_ramp() {
        let mut source = flat(40, 1, 0xff00_0000);
        source.data[20..].fill(0xffff_ffff);
        let out = frost(&source, Filter { sigma: 4.0, saturate: 1.0, brightness: 1.0 });
        let red = |x: usize| (out.data[x] >> 16) & 0xff;
        assert!(red(0) < 5 && red(39) > 250);
        assert!((100..160).contains(&red(19)) || (100..160).contains(&red(20)), "{} {}", red(19), red(20));
        assert!((0..39).all(|x| red(x) <= red(x + 1)));
    }

    #[test]
    fn saturate_zero_is_grey_and_brightness_scales() {
        let out = frost(&flat(1, 1, 0xffff_0000), Filter { sigma: 0.0, saturate: 0.0, brightness: 1.0 });
        let [_, r, g, b] = channels(out.data[0]);
        assert!(r == g && g == b, "{:08x}", out.data[0]);
        let out = frost(&flat(1, 1, 0xff80_8080), Filter { sigma: 0.0, saturate: 1.0, brightness: 0.5 });
        assert_eq!(out.data[0], 0xff40_4040);
    }
}
