use core::f64::consts::TAU;

const RESPONSE: f64 = 0.35;
const SETTLING: f64 = 9.233_413_476_451_586;
const REDUCED_FADE: f64 = 0.15;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Preset {
    #[default]
    Standard,
    Reduced,
}

impl Preset {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "standard" => Some(Self::Standard),
            "reduced" => Some(Self::Reduced),
            _ => None,
        }
    }

    pub fn from_wire(value: u32) -> Self {
        match value {
            1 => Self::Reduced,
            _ => Self::Standard,
        }
    }

    pub fn to_wire(self) -> u32 {
        match self {
            Self::Standard => 0,
            Self::Reduced => 1,
        }
    }

    pub fn moves(self) -> bool {
        self == Self::Standard
    }

    pub fn curve(self) -> Curve {
        match self {
            Self::Standard => Curve::Spring { response: RESPONSE },
            Self::Reduced => Curve::Eased { seconds: REDUCED_FADE, ease: Ease::LINEAR },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Curve {
    Spring { response: f64 },
    Eased { seconds: f64, ease: Ease },
}

impl Curve {
    pub fn at(&self, seconds: f64) -> f64 {
        if seconds >= self.length() {
            return 1.0;
        }
        if seconds <= 0.0 {
            return 0.0;
        }
        match *self {
            Self::Spring { response } => {
                let w = TAU / response;
                1.0 - (1.0 + w * seconds) * (-w * seconds).exp()
            }
            Self::Eased { seconds: length, ease } => ease.at(seconds / length),
        }
    }

    pub fn css(&self, samples: usize) -> String {
        let steps = samples.max(2);
        let length = self.length();
        let points: Vec<String> = (0..=steps).map(|i| format!("{:.4}", self.at(length * i as f64 / steps as f64))).collect();
        format!("linear({})", points.join(", "))
    }

    pub fn length(&self) -> f64 {
        match *self {
            Self::Spring { response } => SETTLING * response / TAU,
            Self::Eased { seconds, .. } => seconds.max(0.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Ease {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl Ease {
    pub const LINEAR: Self = Self { x1: 0.0, y1: 0.0, x2: 1.0, y2: 1.0 };
    const NAMED: [(&str, Self); 5] = [
        ("linear", Self::LINEAR),
        ("ease", Self { x1: 0.25, y1: 0.1, x2: 0.25, y2: 1.0 }),
        ("ease-in", Self { x1: 0.42, y1: 0.0, x2: 1.0, y2: 1.0 }),
        ("ease-out", Self { x1: 0.0, y1: 0.0, x2: 0.58, y2: 1.0 }),
        ("ease-in-out", Self { x1: 0.42, y1: 0.0, x2: 0.58, y2: 1.0 }),
    ];

    pub fn named(name: &str) -> Option<Self> {
        Self::NAMED.iter().find(|(n, _)| *n == name).map(|(_, ease)| *ease)
    }

    pub fn from_wire(value: u32) -> Self {
        Self::NAMED.get(value as usize).map_or(Self::LINEAR, |(_, ease)| *ease)
    }

    pub fn to_wire(self) -> u32 {
        Self::NAMED.iter().position(|(_, ease)| *ease == self).map_or(0, |at| at as u32)
    }

    pub fn wire_of(name: &str) -> Option<u32> {
        Self::NAMED.iter().position(|(n, _)| *n == name).map(|at| at as u32)
    }

    pub fn at(&self, progress: f64) -> f64 {
        let x = progress.clamp(0.0, 1.0);
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..40 {
            let t = (low + high) / 2.0;
            if bezier(self.x1, self.x2, t) < x {
                low = t;
            } else {
                high = t;
            }
        }
        bezier(self.y1, self.y2, (low + high) / 2.0)
    }
}

fn bezier(a: f64, b: f64, t: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
}

pub fn mix(from: f64, to: f64, progress: f64) -> f64 {
    from + (to - from) * progress
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settling_point_is_where_the_spring_is_within_a_thousandth() {
        let left = (1.0 + SETTLING) * (-SETTLING).exp();
        assert!((left - 0.001).abs() < 1e-12, "{left}");
    }

    #[test]
    fn the_standard_spring_settles_in_about_half_a_second_without_overshoot() {
        let curve = Preset::Standard.curve();
        assert!((0.45..0.55).contains(&curve.length()), "{}", curve.length());
        let samples: Vec<f64> = (0..=60).map(|i| curve.at(f64::from(i) / 100.0)).collect();
        assert!(samples.windows(2).all(|w| w[0] <= w[1] && w[1] <= 1.0));
        assert_eq!(curve.at(0.0), 0.0);
        assert_eq!(curve.at(curve.length()), 1.0);
    }

    #[test]
    fn a_curve_reads_as_a_css_linear_easing() {
        let css = Preset::Standard.curve().css(4);
        assert!(css.starts_with("linear(0.0000, ") && css.ends_with(", 1.0000)"), "{css}");
        assert_eq!(css.matches(',').count(), 4);
        assert_eq!(Preset::Reduced.curve().css(2), "linear(0.0000, 0.5000, 1.0000)");
    }

    #[test]
    fn reduced_motion_is_a_short_linear_fade() {
        let curve = Preset::Reduced.curve();
        assert_eq!(curve.length(), 0.15);
        assert!((curve.at(0.075) - 0.5).abs() < 1e-6);
        assert!(!Preset::Reduced.moves());
    }

    #[test]
    fn css_eases_by_name_and_wire() {
        let ease = Ease::named("ease-in-out").unwrap();
        assert!((ease.at(0.5) - 0.5).abs() < 1e-6);
        assert!(ease.at(0.2) < 0.2);
        assert_eq!(Ease::from_wire(Ease::wire_of("ease-out").unwrap()), Ease::named("ease-out").unwrap());
        assert_eq!(Ease::named("ease-in").unwrap().to_wire(), Ease::wire_of("ease-in").unwrap());
        assert_eq!(Ease::from_wire(99), Ease::LINEAR);
        assert!((Ease::LINEAR.at(0.3) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn presets_parse_and_round_trip() {
        assert_eq!(Preset::parse("reduced"), Some(Preset::Reduced));
        assert_eq!(Preset::parse("fast"), None);
        assert_eq!(Preset::from_wire(Preset::Reduced.to_wire()), Preset::Reduced);
        assert_eq!(Preset::from_wire(7), Preset::Standard);
    }
}
