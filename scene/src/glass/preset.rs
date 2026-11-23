use super::{Bleed, BlurLayer, Glass, Highlight, LAYERS, Lens, Outline, Refraction};

const HIGHLIGHT_DEGREES: f32 = 330.0;
const HIGHLIGHT_AMOUNT: f32 = 0.45;
const HIGHLIGHT_WIDTH: f32 = 4.0;
const OUTLINE: Outline = Outline { width: 1.0, color: [1.0, 1.0, 1.0, 0.5], merged: false };
const DARK_OUTLINE: Outline = Outline { color: [1.0, 1.0, 1.0, 0.25], ..OUTLINE };
const LIGHT_TINT: [f32; 3] = [0.97, 0.97, 0.98];
const DARK_TINT: [f32; 3] = [0.11, 0.11, 0.12];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Regular,
    Clear,
}

impl Role {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "regular" => Some(Self::Regular),
            "clear" => Some(Self::Clear),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Level {
    #[default]
    Clear,
    Medium,
    Tinted,
    Opaque,
}

impl Level {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "clear" => Some(Self::Clear),
            "medium" => Some(Self::Medium),
            "tinted" => Some(Self::Tinted),
            "opaque" => Some(Self::Opaque),
            _ => None,
        }
    }

    pub fn from_wire(value: u32) -> Self {
        match value {
            1 => Self::Medium,
            2 => Self::Tinted,
            3 => Self::Opaque,
            _ => Self::Clear,
        }
    }

    pub fn to_wire(self) -> u32 {
        match self {
            Self::Clear => 0,
            Self::Medium => 1,
            Self::Tinted => 2,
            Self::Opaque => 3,
        }
    }

    pub fn clearer(self) -> Self {
        match self {
            Self::Clear | Self::Medium => Self::Clear,
            Self::Tinted => Self::Medium,
            Self::Opaque => Self::Opaque,
        }
    }

    pub fn of(preference: super::Preference) -> Self {
        match preference.reduced_transparency {
            true => Self::Opaque,
            false => Self::from_wire(preference.level),
        }
    }

    pub fn preferred() -> Self {
        Self::of(super::preference())
    }

    pub fn for_role(self, role: Role) -> Self {
        match role {
            Role::Regular => self,
            Role::Clear => self.clearer(),
        }
    }
}

impl Glass {
    pub fn preset(role: Role, level: Level) -> Self {
        Self::preset_in(role, level, false)
    }

    pub fn preset_in(role: Role, level: Level, dark: bool) -> Self {
        let glass = Self::table_preset(role, level, dark);
        Self { outline: if dark { DARK_OUTLINE } else { OUTLINE }, ..glass }
    }

    fn table_preset(role: Role, level: Level, dark: bool) -> Self {
        match level.for_role(role) {
            Level::Clear => Self {
                refraction: Refraction { inner_amount: 0.8, inner_height: 18.0, outer_amount: 0.0, outer_height: 0.0 },
                blur: layers([(0.5, 1.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]),
                bleed: Bleed::default(),
                highlight: highlight(),
                white_point: 0.0,
                smoothing: 0.35,
                tint: [1.0, 1.0, 1.0, 0.0],
                lens: Lens { bezel: 34.0, thickness: 22.0, index: 1.5, dispersion: 0.01 },
                darken: 0.05,
                outline: OUTLINE,
            },
            Level::Medium => Self {
                refraction: Refraction { inner_amount: 0.8, inner_height: 10.0, outer_amount: 0.15, outer_height: 18.0 },
                blur: layers([(2.0, 1.0), (5.0, 0.8), (10.0, 0.6), (16.0, 0.4), (24.0, 0.2)]),
                bleed: Bleed { amount: 0.2, radius: 12.0, opacity: 0.3 },
                highlight: highlight(),
                white_point: 0.05,
                smoothing: 0.6,
                tint: [1.0, 1.0, 1.0, 0.05],
                outline: OUTLINE,
                ..Self::default()
            },
            Level::Tinted => Self {
                refraction: Refraction { inner_amount: 0.7, inner_height: 10.0, outer_amount: 0.15, outer_height: 20.0 },
                blur: layers([(8.0, 1.0), (16.0, 0.8), (28.0, 0.7), (40.0, 0.5), (56.0, 0.3)]),
                bleed: Bleed { amount: 0.2, radius: 12.0, opacity: 0.3 },
                highlight: highlight(),
                white_point: 0.12,
                smoothing: 0.65,
                tint: [1.0, 1.0, 1.0, 0.18],
                outline: OUTLINE,
                ..Self::default()
            },
            Level::Opaque => Self::opaque(dark),
        }
    }

    pub fn opaque(dark: bool) -> Self {
        Self {
            refraction: Refraction { inner_amount: 0.2, inner_height: 8.0, outer_amount: 0.0, outer_height: 0.0 },
            blur: layers([(24.0, 1.0), (48.0, 1.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]),
            bleed: Bleed::default(),
            highlight: highlight(),
            white_point: 0.0,
            smoothing: 0.5,
            tint: tinted(dark, 0.9),
            outline: OUTLINE,
            ..Self::default()
        }
    }

    pub fn blur(level: Level, dark: bool) -> Self {
        let (pairs, tint) = match level {
            Level::Clear => ([(6.0, 1.0), (12.0, 0.6), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)], 0.15),
            Level::Medium => ([(10.0, 1.0), (20.0, 0.8), (32.0, 0.5), (0.0, 0.0), (0.0, 0.0)], 0.35),
            Level::Tinted => ([(16.0, 1.0), (32.0, 0.8), (48.0, 0.6), (0.0, 0.0), (0.0, 0.0)], 0.6),
            Level::Opaque => ([(24.0, 1.0), (48.0, 1.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)], 1.0),
        };
        Self { blur: layers(pairs), tint: tinted(dark, tint), smoothing: 0.5, ..Self::default() }
    }
}

fn layers(pairs: [(f32, f32); LAYERS]) -> [BlurLayer; LAYERS] {
    pairs.map(|(radius, opacity)| BlurLayer { radius, opacity })
}

fn tinted(dark: bool, alpha: f32) -> [f32; 4] {
    let [r, g, b] = if dark { DARK_TINT } else { LIGHT_TINT };
    [r, g, b, alpha]
}

fn highlight() -> Highlight {
    Highlight { angle: HIGHLIGHT_DEGREES.to_radians(), amount: HIGHLIGHT_AMOUNT, width: HIGHLIGHT_WIDTH }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
    }

    #[test]
    fn regular_glass_at_the_clear_level_is_its_lens_sample() {
        let sample = [
            ("acde-glass", "clear"),
            ("acde-glass-refraction", "0.8 18px 0 0"),
            ("acde-glass-lens", "34px 22px 1.5 0.01"),
            ("acde-glass-blur", "0.5px 1"),
            ("acde-glass-bleed", "0 0 0"),
            ("acde-glass-highlight", "330deg 0.45 4px"),
            ("acde-glass-darken", "0.05"),
            ("acde-glass-outline", "1px rgb(255 255 255 / 0.5)"),
            ("acde-glass-white-point", "0"),
            ("acde-glass-smoothing", "0.35"),
            ("acde-glass-tint", "rgb(255 255 255 / 0)"),
        ];
        let read = Glass::from_properties_at(Level::Medium, properties(&sample), |w| panic!("{w}")).unwrap();
        assert_eq!(Glass::preset(Role::Regular, Level::Clear), read);
    }

    #[test]
    fn clear_glass_is_one_level_clearer_and_stops_at_the_end() {
        assert_eq!(Glass::preset(Role::Clear, Level::Tinted), Glass::preset(Role::Regular, Level::Medium));
        assert_eq!(Glass::preset(Role::Clear, Level::Medium), Glass::preset(Role::Regular, Level::Clear));
        assert_eq!(Glass::preset(Role::Clear, Level::Clear), Glass::preset(Role::Regular, Level::Clear));
        assert_eq!(Glass::preset(Role::Clear, Level::Opaque), Glass::opaque(false));
    }

    #[test]
    fn every_level_lights_from_330deg_and_the_wire_round_trips() {
        for level in [Level::Clear, Level::Medium, Level::Tinted, Level::Opaque] {
            assert_eq!(Glass::preset(Role::Regular, level).highlight, highlight());
            assert_eq!(Level::from_wire(level.to_wire()), level);
        }
        let preference = |level: u32, reduced_transparency: bool| super::super::Preference { level, reduced_transparency, dark: false };
        assert_eq!(Level::of(preference(2, false)), Level::Tinted);
        assert_eq!(Level::of(preference(1, true)), Level::Opaque);
        assert_eq!(Level::parse("tinted"), Some(Level::Tinted));
        assert_eq!(Level::from_wire(99), Level::Clear);
        assert_ne!(Glass::opaque(true).tint, Glass::opaque(false).tint);
    }

    #[test]
    fn the_default_outline_is_dimmer_on_a_dark_page() {
        for level in [Level::Clear, Level::Medium, Level::Tinted, Level::Opaque] {
            assert_eq!(Glass::preset_in(Role::Regular, level, false).outline.color, [1.0, 1.0, 1.0, 0.5]);
            let dark = Glass::preset_in(Role::Regular, level, true);
            assert_eq!(dark.outline, Outline { color: [1.0, 1.0, 1.0, 0.25], ..OUTLINE });
            assert_eq!(Glass { outline: OUTLINE, ..dark }.lens, Glass::preset(Role::Regular, level).lens);
        }
        assert_eq!(Glass::preset_in(Role::Regular, Level::Opaque, true).tint, Glass::opaque(true).tint);
    }

    #[test]
    fn the_level_picks_the_preset_and_defaults_to_clear() {
        let given = [("acde-glass", "regular")];
        let tinted = Glass::from_properties_at(Level::Tinted, properties(&given), |w| panic!("{w}")).unwrap();
        assert_eq!(tinted, Glass::preset(Role::Regular, Level::Tinted));
        assert_eq!(Level::default(), Level::Clear);
    }
}
