use crate::blur::{Pixels, blur, channels, pack};
use crate::corner::{self, Corners};

mod appear;
pub mod group;
mod membership;
mod preset;

pub use appear::Appearance;
pub use membership::{Membership, Name};
pub use preset::{Level, Role};

pub const WGSL: &str = include_str!("glass.wgsl");

pub const LAYERS: usize = 5;
const WIRE_FLOATS: usize = 30;
const MOST_LENGTH: f32 = 256.0;
const FAR_EDGE: f64 = 0.33;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Refraction {
    pub inner_amount: f32,
    pub inner_height: f32,
    pub outer_amount: f32,
    pub outer_height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlurLayer {
    pub radius: f32,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bleed {
    pub amount: f32,
    pub radius: f32,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Highlight {
    pub angle: f32,
    pub amount: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lens {
    pub strength: f32,
    pub width: f32,
    pub dispersion: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Glass {
    pub refraction: Refraction,
    pub blur: [BlurLayer; LAYERS],
    pub bleed: Bleed,
    pub highlight: Highlight,
    pub white_point: f32,
    pub smoothing: f32,
    pub tint: [f32; 4],
    pub lens: Lens,
    pub darken: f32,
}

impl Glass {
    pub fn from_properties(property: impl Fn(&str) -> Option<String>, warn: impl Fn(String)) -> Option<Self> {
        Self::from_properties_at(Level::default(), property, warn)
    }

    pub fn from_properties_at(level: Level, property: impl Fn(&str) -> Option<String>, warn: impl Fn(String)) -> Option<Self> {
        let name = property("acde-glass")?;
        let name = name.trim();
        if name == "none" || name.is_empty() {
            return None;
        }
        let Some(mut glass) = Role::parse(name).map(|role| Self::preset(role, level)) else {
            warn(format!("--acde-glass: no glass called {name:?} (none, regular, clear)"));
            return None;
        };
        let mut read = |key: &str, apply: &mut dyn FnMut(&mut Glass, &str) -> Option<()>| {
            if let Some(text) = property(key)
                && apply(&mut glass, text.trim()).is_none()
            {
                warn(format!("--{key}: {:?} is left out (see the glass parameters)", text.trim()));
            }
        };
        read("acde-glass-refraction", &mut |g, t| {
            let [a, b, c, d] = words::<4>(t)?;
            g.refraction = Refraction { inner_amount: number(a)?, inner_height: px(b)?, outer_amount: number(c)?, outer_height: px(d)? };
            Some(())
        });
        read("acde-glass-lens", &mut |g, t| {
            g.lens = match *t.split_whitespace().collect::<Vec<_>>() {
                [strength, width] => Lens { strength: px(strength)?, width: px(width)?, ..g.lens },
                [strength, width, dispersion] => Lens { strength: px(strength)?, width: px(width)?, dispersion: number(dispersion)? },
                _ => return None,
            };
            Some(())
        });
        read("acde-glass-blur", &mut |g, t| {
            let mut layers = [BlurLayer::default(); LAYERS];
            let given: Vec<&str> = t.split(',').map(str::trim).collect();
            if given.is_empty() || given.len() > LAYERS {
                return None;
            }
            for (layer, text) in layers.iter_mut().zip(given) {
                let [radius, opacity] = words::<2>(text)?;
                *layer = BlurLayer { radius: px(radius)?, opacity: number(opacity)? };
            }
            g.blur = layers;
            Some(())
        });
        read("acde-glass-bleed", &mut |g, t| {
            let [a, b, c] = words::<3>(t)?;
            g.bleed = Bleed { amount: number(a)?, radius: px(b)?, opacity: number(c)? };
            Some(())
        });
        read("acde-glass-highlight", &mut |g, t| {
            g.highlight = match *t.split_whitespace().collect::<Vec<_>>() {
                [a, b] => Highlight { angle: degrees(a)?, amount: number(b)?, ..g.highlight },
                [a, b, c] => Highlight { angle: degrees(a)?, amount: number(b)?, width: px(c)? },
                _ => return None,
            };
            Some(())
        });
        read("acde-glass-white-point", &mut |g, t| {
            g.white_point = number(t)?;
            Some(())
        });
        read("acde-glass-smoothing", &mut |g, t| {
            g.smoothing = number(t)?;
            Some(())
        });
        read("acde-glass-tint", &mut |g, t| {
            g.tint = color(t)?;
            Some(())
        });
        read("acde-glass-darken", &mut |g, t| {
            g.darken = number(t)?;
            Some(())
        });
        Some(glass.clamped())
    }

    pub fn clamped(self) -> Self {
        let unit = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let length = |v: f32| if v.is_finite() { v.clamp(0.0, MOST_LENGTH) } else { 0.0 };
        let r = self.refraction;
        Self {
            refraction: Refraction {
                inner_amount: unit(r.inner_amount),
                inner_height: length(r.inner_height),
                outer_amount: unit(r.outer_amount),
                outer_height: length(r.outer_height),
            },
            blur: self.blur.map(|l| BlurLayer { radius: length(l.radius), opacity: unit(l.opacity) }),
            bleed: Bleed { amount: unit(self.bleed.amount), radius: length(self.bleed.radius), opacity: unit(self.bleed.opacity) },
            highlight: Highlight {
                angle: if self.highlight.angle.is_finite() { self.highlight.angle } else { 0.0 },
                amount: unit(self.highlight.amount),
                width: length(self.highlight.width),
            },
            white_point: unit(self.white_point),
            smoothing: unit(self.smoothing),
            tint: self.tint.map(unit),
            lens: Lens { strength: length(self.lens.strength), width: length(self.lens.width), dispersion: unit(self.lens.dispersion) },
            darken: unit(self.darken),
        }
    }

    pub fn scaled(self, factor: f64) -> Self {
        let f = factor as f32;
        let mut g = self;
        g.refraction.inner_height *= f;
        g.refraction.outer_height *= f;
        g.blur.iter_mut().for_each(|l| l.radius *= f);
        g.bleed.radius *= f;
        g.lens.strength *= f;
        g.lens.width *= f;
        g.highlight.width *= f;
        g
    }

    pub fn mixed(&self, other: &Self, t: f32) -> Self {
        let (a, b) = (self.floats(), other.floats());
        Self::from_floats(core::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t))
    }

    pub fn reach(&self) -> f64 {
        let blur = self.blur.iter().filter(|l| l.opacity > 0.0).map(|l| l.radius).fold(0.0, f32::max);
        f64::from((3.0 * blur).max(self.refraction.outer_height).max(self.bleed.radius))
    }

    pub fn to_wire(&self) -> Vec<u8> {
        self.floats().iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    pub fn from_wire(bytes: &[u8]) -> Self {
        let mut floats = Self::default().floats();
        for (slot, chunk) in floats.iter_mut().zip(bytes.chunks_exact(4)) {
            *slot = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        Self::from_floats(floats).clamped()
    }

    fn floats(&self) -> [f32; WIRE_FLOATS] {
        let r = self.refraction;
        let b = self.blur;
        [
            r.inner_amount,
            r.inner_height,
            r.outer_amount,
            r.outer_height,
            b[0].radius,
            b[0].opacity,
            b[1].radius,
            b[1].opacity,
            b[2].radius,
            b[2].opacity,
            b[3].radius,
            b[3].opacity,
            b[4].radius,
            b[4].opacity,
            self.bleed.amount,
            self.bleed.radius,
            self.bleed.opacity,
            self.highlight.angle,
            self.highlight.amount,
            self.white_point,
            self.smoothing,
            self.tint[0],
            self.tint[1],
            self.tint[2],
            self.tint[3],
            self.lens.strength,
            self.lens.dispersion,
            self.darken,
            self.lens.width,
            self.highlight.width,
        ]
    }

    fn from_floats(f: [f32; WIRE_FLOATS]) -> Self {
        let layer = |i: usize| BlurLayer { radius: f[4 + 2 * i], opacity: f[5 + 2 * i] };
        Self {
            refraction: Refraction { inner_amount: f[0], inner_height: f[1], outer_amount: f[2], outer_height: f[3] },
            blur: [layer(0), layer(1), layer(2), layer(3), layer(4)],
            bleed: Bleed { amount: f[14], radius: f[15], opacity: f[16] },
            highlight: Highlight { angle: f[17], amount: f[18], width: f[29] },
            white_point: f[19],
            smoothing: f[20],
            tint: [f[21], f[22], f[23], f[24]],
            lens: Lens { strength: f[25], width: f[28], dispersion: f[26] },
            darken: f[27],
        }
    }

    fn two_layers(&self) -> ((f32, f32), (f32, f32)) {
        let shown: Vec<BlurLayer> = self.blur.iter().copied().filter(|l| l.opacity > 0.0).collect();
        let small = shown.iter().copied().min_by(|a, b| a.radius.total_cmp(&b.radius)).unwrap_or_default();
        let large = shown.iter().copied().max_by(|a, b| a.radius.total_cmp(&b.radius)).unwrap_or_default();
        ((small.radius, small.opacity), (large.radius, large.opacity))
    }
}

fn words<const N: usize>(text: &str) -> Option<[&str; N]> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    parts.try_into().ok()
}

fn number(text: &str) -> Option<f32> {
    text.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn px(text: &str) -> Option<f32> {
    match text {
        "0" => Some(0.0),
        _ => number(text.strip_suffix("px")?),
    }
}

fn degrees(text: &str) -> Option<f32> {
    Some(number(text.strip_suffix("deg")?)?.to_radians())
}

fn color(text: &str) -> Option<[f32; 4]> {
    if let Some(hex) = text.strip_prefix('#') {
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok().map(|b| f32::from(b) / 255.0);
        return match hex.len() {
            3 | 4 => {
                let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok().map(|d| f32::from(d * 17) / 255.0);
                Some([digit(0)?, digit(1)?, digit(2)?, if hex.len() == 4 { digit(3)? } else { 1.0 }])
            }
            6 => Some([byte(0)?, byte(2)?, byte(4)?, 1.0]),
            8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
            _ => None,
        };
    }
    let inner = text.strip_prefix("rgb(")?.strip_suffix(')')?;
    let (rgb, alpha) = inner.split_once('/').map_or((inner, None), |(c, a)| (c, Some(a.trim())));
    let [r, g, b] = words::<3>(rgb.trim())?;
    let channel = |t: &str| number(t).map(|v| (v / 255.0).clamp(0.0, 1.0));
    let alpha = match alpha {
        None => 1.0,
        Some(a) => match a.strip_suffix('%') {
            Some(p) => number(p)? / 100.0,
            None => number(a)?,
        },
    };
    Some([channel(r)?, channel(g)?, channel(b)?, alpha.clamp(0.0, 1.0)])
}

mod preference;
pub use preference::{Preference, preference, set_preference};

pub mod cpu {
    use super::*;

    pub fn paint(source: &Pixels, margin: usize, corners: Corners, glass: &Glass) -> Pixels {
        let under = Under::of(source, glass);
        let (width, height) = (source.width, source.height);
        let half = ((width - 2 * margin) as f64 / 2.0, (height - 2 * margin) as f64 / 2.0);
        let center = (width as f64 / 2.0, height as f64 / 2.0);
        let look = Look::of(glass);
        let mut data = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let p = (x as f64 + 0.5 - center.0, y as f64 + 0.5 - center.1);
                let d = corner::distance(p, half, corners.radius, corners.exponent);
                data.push(packed(look.pixel(under.at(y * width + x), toward(p, half), d, glass)));
            }
        }
        Pixels { width, height, data }
    }

    pub(super) struct Under {
        small: Pixels,
        large: Pixels,
        weight: f32,
    }

    impl Under {
        pub(super) fn of(source: &Pixels, glass: &Glass) -> Self {
            let ((small_radius, small_opacity), (large_radius, large_opacity)) = glass.two_layers();
            let weight = match small_opacity + large_opacity {
                sum if sum > 0.0 => large_opacity / sum,
                _ => 0.0,
            };
            Self { small: blur(source, f64::from(small_radius)), large: blur(source, f64::from(large_radius)), weight }
        }

        pub(super) fn at(&self, i: usize) -> [f32; 4] {
            mix(channels(self.small.data[i]), channels(self.large.data[i]), self.weight)
        }
    }

    pub(super) fn toward(p: (f64, f64), half: (f64, f64)) -> (f64, f64) {
        (p.0 / half.0.max(1.0), p.1 / half.1.max(1.0))
    }

    pub(super) fn packed(pixel: [f32; 4]) -> u32 {
        let [r, g, b, a] = pixel.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u32);
        pack([a, r, g, b])
    }

    pub(super) struct Look {
        soft: f64,
        light: (f64, f64),
        band: f64,
        reach: f64,
    }

    impl Look {
        pub(super) fn of(glass: &Glass) -> Self {
            let angle = f64::from(glass.highlight.angle);
            Self {
                soft: 1.0 + 4.0 * f64::from(glass.smoothing),
                light: (angle.sin(), -angle.cos()),
                band: f64::from(glass.highlight.width.max(0.5)),
                reach: f64::from(glass.bleed.radius.max(1.0)),
            }
        }

        pub(super) fn pixel(&self, under: [f32; 4], toward: (f64, f64), d: f64, glass: &Glass) -> [f32; 4] {
            let inside = (0.5 - d / self.soft).clamp(0.0, 1.0) as f32;
            if inside <= 0.0 {
                let glow = (1.0 - d / self.reach).clamp(0.0, 1.0) as f32 * glass.bleed.amount * glass.bleed.opacity;
                return under.map(|v| v * glow);
            }
            let shine = self.highlight(toward, d) * glass.highlight.amount;
            let a = under[3];
            let straight = |v: f32| if a > 0.0 { (v / a).min(1.0) } else { 0.0 };
            let [r, g, b] = tone([straight(under[0]), straight(under[1]), straight(under[2])], glass, shine);
            let alpha = a.max(glass.tint[3]) * inside;
            [r * alpha, g * alpha, b * alpha, alpha]
        }

        fn highlight(&self, (x, y): (f64, f64), d: f64) -> f32 {
            if -d > self.band {
                return 0.0;
            }
            let length = x.hypot(y).max(1e-6);
            let toward = (x * self.light.0 + y * self.light.1) / length;
            let (facing, away) = (toward.max(0.0), (-toward).max(0.0));
            ((facing * facing + FAR_EDGE * away * away) * (1.0 + d.min(0.0) / self.band)) as f32
        }
    }

    fn mix(a: [u32; 4], b: [u32; 4], t: f32) -> [f32; 4] {
        let rgba = |[a, r, g, b]: [u32; 4]| [r, g, b, a].map(|v| v as f32 / 255.0);
        let (a, b) = (rgba(a), rgba(b));
        [0, 1, 2, 3].map(|k| a[k] * (1.0 - t) + b[k] * t)
    }

    fn tone(rgb: [f32; 3], glass: &Glass, shine: f32) -> [f32; 3] {
        let tint = glass.tint;
        let mut k = 0;
        rgb.map(|v| {
            let lifted = v + (1.0 - v) * glass.white_point;
            let tinted = lifted * (1.0 - tint[3]) + tint[k] * tint[3];
            k += 1;
            (tinted * (1.0 - glass.darken) + shine).clamp(0.0, 1.0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
    }

    #[test]
    fn a_preset_with_parameters_over_it() {
        let given = [
            ("acde-glass", " regular "),
            ("acde-glass-refraction", "0.6 14px 0.25 28px"),
            ("acde-glass-blur", "2px 1, 6px 0.8"),
            ("acde-glass-highlight", "90deg 0.5"),
            ("acde-glass-tint", "rgb(255 128 0 / 0.5)"),
        ];
        let glass = Glass::from_properties(properties(&given), |w| panic!("{w}")).expect("glass");
        assert_eq!(glass.refraction, Refraction { inner_amount: 0.6, inner_height: 14.0, outer_amount: 0.25, outer_height: 28.0 });
        assert_eq!(glass.blur[1], BlurLayer { radius: 6.0, opacity: 0.8 });
        assert_eq!(glass.blur[2], BlurLayer::default());
        assert!((glass.highlight.angle - core::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(glass.tint[3], 0.5);
        assert_eq!(glass.bleed, Glass::preset(Role::Regular, Level::Clear).bleed);
    }

    #[test]
    fn no_glass_without_the_main_property_or_with_a_bad_preset() {
        assert_eq!(Glass::from_properties(properties(&[("acde-glass-smoothing", "0.5")]), |_| {}), None);
        assert_eq!(Glass::from_properties(properties(&[("acde-glass", "none")]), |_| {}), None);
        let warned = std::cell::Cell::new(0);
        assert_eq!(Glass::from_properties(properties(&[("acde-glass", "shiny")]), |_| warned.set(warned.get() + 1)), None);
        assert_eq!(warned.get(), 1);
    }

    #[test]
    fn a_bad_parameter_is_left_out_with_a_warning() {
        let warned = std::cell::Cell::new(0);
        let given = [("acde-glass", "clear"), ("acde-glass-refraction", "0.6 14em 0.2 2px")];
        let glass = Glass::from_properties(properties(&given), |_| warned.set(warned.get() + 1)).unwrap();
        assert_eq!(glass.refraction, Glass::preset(Role::Clear, Level::Clear).refraction);
        assert_eq!(warned.get(), 1);
    }

    #[test]
    fn the_wire_round_trips_and_is_forgiving() {
        let glass = Glass::preset(Role::Regular, Level::Tinted);
        let wire = glass.to_wire();
        assert_eq!(wire.len(), WIRE_FLOATS * 4);
        assert_eq!(Glass::from_wire(&wire), glass);
        assert_eq!(Glass::from_wire(&wire[..8]).refraction.inner_height, glass.refraction.inner_height);
        assert_eq!(Glass::from_wire(&wire[..8]).bleed, Bleed::default());
        let mut longer = wire.clone();
        longer.extend_from_slice(&1.0f32.to_le_bytes());
        assert_eq!(Glass::from_wire(&longer), glass);
        let wild = 9.0f32.to_le_bytes().repeat(WIRE_FLOATS);
        assert_eq!(Glass::from_wire(&wild).white_point, 1.0);
    }

    #[test]
    fn scaled_and_reach() {
        let glass = Glass::preset(Role::Regular, Level::Tinted);
        let twice = glass.scaled(2.0);
        assert_eq!(twice.refraction.outer_height, 40.0);
        assert_eq!(twice.blur[4].radius, 112.0);
        assert_eq!(glass.reach(), 168.0);
    }

    #[test]
    fn the_bleed_keeps_the_premultiplied_colour_of_what_is_under() {
        let margin = 8;
        let half_white = 0x8080_8080;
        let source = Pixels { width: 40, height: 30, data: vec![half_white; 40 * 30] };
        let corners = Corners { radius: 0.0, exponent: 2.0 };
        let out = cpu::paint(&source, margin, corners, &Glass::preset(Role::Regular, Level::Tinted));
        let [a, r, ..] = channels(out.data[15 * 40 + 2]);
        assert!(a > 0 && r <= a, "premultiplied: colour {r} within alpha {a}");
    }

    #[test]
    fn the_cpu_glass_covers_the_area_bleeds_near_it_and_leaves_the_far_corner_out() {
        let glass = Glass::preset(Role::Regular, Level::Tinted);
        let margin = 24;
        assert!(margin as f32 > glass.bleed.radius, "the margin reaches past the bleed");
        let (width, height) = (80, 70);
        let source = Pixels { width, height, data: vec![0xff20_4080; width * height] };
        let corners = Corners { radius: 8.0, exponent: 4.0 };
        let out = cpu::paint(&source, margin, corners, &glass);
        assert_eq!((out.width, out.height), (width, height));
        assert_eq!(channels(out.data[35 * width + 40])[0], 255);
        assert!(channels(out.data[35 * width + margin - 4])[0] > 0, "the bleed shows 4 px outside the edge");
        assert_eq!(channels(out.data[0])[0], 0);
    }

    #[test]
    fn the_lens_has_its_own_property_and_an_older_wire_has_none() {
        let given = [("acde-glass", "regular"), ("acde-glass-lens", "24px 6px 0.04"), ("acde-glass-darken", "0.05")];
        let glass = Glass::from_properties_at(Level::Tinted, properties(&given), |w| panic!("{w}")).unwrap();
        assert_eq!(glass.lens, Lens { strength: 24.0, width: 6.0, dispersion: 0.04 });
        assert_eq!(glass.darken, 0.05);
        assert_eq!((glass.scaled(2.0).lens.strength, glass.scaled(2.0).lens.width), (48.0, 12.0));
        let older = &glass.to_wire()[..25 * 4];
        assert_eq!((Glass::from_wire(older).lens, Glass::from_wire(older).darken), (Lens::default(), 0.0));
        let clear = Glass::preset(Role::Clear, Level::Clear);
        let without_dispersion = [("acde-glass", "clear"), ("acde-glass-lens", "9px 4px")];
        let read = Glass::from_properties(properties(&without_dispersion), |w| panic!("{w}")).unwrap();
        assert_eq!(read.lens, Lens { strength: 9.0, width: 4.0, dispersion: clear.lens.dispersion });
        let warned = std::cell::Cell::new(0);
        let bad = [("acde-glass", "clear"), ("acde-glass-lens", "9px 4em"), ("acde-glass-refraction", "0.4 10px 0.1 8px 9px")];
        let read = Glass::from_properties(properties(&bad), |_| warned.set(warned.get() + 1)).unwrap();
        assert_eq!((read.refraction, read.lens, warned.get()), (clear.refraction, clear.lens, 2));
    }

    #[test]
    fn darkening_scales_the_glass_colour_and_nothing_darkens_without_it() {
        let source = Pixels { width: 40, height: 30, data: vec![0xffc0_c0c0; 40 * 30] };
        let corners = Corners { radius: 4.0, exponent: 2.0 };
        let plain = Glass { highlight: Highlight::default(), ..Glass::preset(Role::Regular, Level::Clear) };
        let red = |glass: &Glass| channels(cpu::paint(&source, 8, corners, glass).data[15 * 40 + 20])[1];
        assert_eq!(red(&Glass { darken: 0.0, ..plain }), 0xc0);
        assert_eq!(red(&Glass { darken: 0.05, ..plain }), (0xc0 as f32 * 0.95).round() as u32);
    }

    #[test]
    fn short_hex_colours_read_like_the_long_ones() {
        assert_eq!(color("#fff0"), Some([1.0, 1.0, 1.0, 0.0]));
        assert_eq!(color("#0f8"), color("#00ff88"));
        assert_eq!(color("#0f88"), color("#00ff8888"));
        assert_eq!(color("#ff"), None);
    }

    #[test]
    fn the_highlight_angle_is_where_the_light_comes_from_as_in_css() {
        let glass = Glass { highlight: Highlight { angle: 315f32.to_radians(), amount: 1.0, width: 12.0 }, ..Glass::preset(Role::Regular, Level::Tinted) };
        let margin = 24;
        let (width, height) = (96, 72);
        let source = Pixels { width, height, data: vec![0xff40_4040; width * height] };
        let out = cpu::paint(&source, margin, Corners { radius: 8.0, exponent: 2.0 }, &glass);
        let red = |x: usize, y: usize| channels(out.data[y * width + x])[1];
        let (top_left, bottom_right) = (red(margin + 6, margin + 6), red(width - margin - 7, height - margin - 7));
        assert!(top_left > bottom_right, "315deg lights the top left edge: {top_left} against {bottom_right}");
        let below = Glass { highlight: Highlight { angle: 180f32.to_radians(), amount: 1.0, width: 12.0 }, ..glass };
        let out = cpu::paint(&source, margin, Corners { radius: 8.0, exponent: 2.0 }, &below);
        let red = |x: usize, y: usize| channels(out.data[y * width + x])[1];
        assert!(red(width / 2, height - margin - 2) > red(width / 2, margin + 1), "180deg lights the bottom edge");
    }
}
