use crate::glass::{Glass, Level, Preference, Role};
use std::borrow::Cow;
use std::sync::{Mutex, OnceLock, RwLock};

pub const MOST_NAME: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Look {
    Glass,
    Blur,
    Nothing,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Material {
    name: Cow<'static, str>,
    look: Look,
}

static BUILT_IN: [Material; 3] = [
    Material { name: Cow::Borrowed("glass"), look: Look::Glass },
    Material { name: Cow::Borrowed("blur"), look: Look::Blur },
    Material { name: Cow::Borrowed("none"), look: Look::Nothing },
];
static WARNED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn table() -> &'static RwLock<Vec<&'static Material>> {
    static TABLE: OnceLock<RwLock<Vec<&'static Material>>> = OnceLock::new();
    TABLE.get_or_init(|| RwLock::new(BUILT_IN.iter().collect()))
}

impl Material {
    pub fn valid(name: &str) -> bool {
        let allowed = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-');
        !name.is_empty() && name.len() <= MOST_NAME && name.bytes().all(allowed)
    }

    pub fn lookup(name: &str) -> Option<&'static Material> {
        table().read().unwrap_or_else(|e| e.into_inner()).iter().copied().find(|material| material.name == name)
    }

    pub fn register(name: &str, look: Look) -> Option<&'static Material> {
        if !Self::valid(name) {
            return None;
        }
        let mut table = table().write().unwrap_or_else(|e| e.into_inner());
        if table.iter().any(|material| material.name == name) {
            return None;
        }
        let material: &'static Material = Box::leak(Box::new(Material { name: Cow::Owned(name.to_owned()), look }));
        table.push(material);
        Some(material)
    }

    pub fn none() -> &'static Material {
        &BUILT_IN[2]
    }

    pub fn resolve(name: &str, warn: impl FnOnce(String)) -> &'static Material {
        if name.is_empty() {
            return Self::none();
        }
        if let Some(found) = Self::lookup(name) {
            return found;
        }
        let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
        if !warned.iter().any(|known| known == name) {
            warned.push(name.to_owned());
            let reason = if Self::valid(name) { "no material by this name" } else { "not a material name ([a-z0-9._-], at most 64 bytes)" };
            warn(format!("material {name:?}: {reason}; drawn as none"));
        }
        Self::none()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn look(&self) -> Look {
        self.look
    }

    pub fn glass(&self, role: Role, preference: Preference) -> Option<Glass> {
        let level = Level::of(preference);
        match self.look {
            Look::Glass => Some(Glass::preset_in(role, level, preference.dark)),
            Look::Blur => Some(Glass::blur(level, preference.dark)),
            Look::Nothing => None,
        }
    }

    pub fn with_overrides(&self, role: Role, preference: Preference, overrides: &[f32]) -> Option<Glass> {
        self.glass(role, preference).map(|base| Glass::with_overrides(&base, overrides))
    }
}

const OVERRIDES: [(&str, &[usize]); 10] = [
    ("refraction", &[0, 1, 2, 3]),
    ("blur", &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13]),
    ("bleed", &[14, 15, 16]),
    ("highlight", &[17, 18, 29]),
    ("white-point", &[19]),
    ("smoothing", &[20]),
    ("tint", &[21, 22, 23, 24]),
    ("lens", &[28, 25, 30, 26]),
    ("darken", &[27]),
    ("outline", &[31, 32, 33, 34, 35, 36]),
];

pub fn overrides<'a>(given: impl IntoIterator<Item = (&'a str, &'a str)>, warn: impl Fn(String)) -> Vec<f32> {
    let mut out = Vec::new();
    for (key, text) in given {
        let Some((_, slots)) = OVERRIDES.iter().find(|(name, _)| *name == key) else {
            let known: Vec<&str> = OVERRIDES.iter().map(|(name, _)| *name).collect();
            warn(format!("overrides: no parameter called {key:?} ({})", known.join(", ")));
            continue;
        };
        let Some(floats) = parsed(key, text, &warn) else { continue };
        out.resize(floats.len(), f32::NAN);
        let words = text.split_whitespace().count();
        let given = match key {
            "highlight" => &slots[..if words == 2 { 2 } else { 3 }],
            "lens" => &slots[..words.clamp(2, 4)],
            _ => slots,
        };
        for &slot in given {
            out[slot] = floats[slot];
        }
    }
    out
}

fn parsed(key: &str, text: &str, warn: &impl Fn(String)) -> Option<Vec<f32>> {
    let failed = std::cell::Cell::new(false);
    let property = |name: &str| match name.strip_prefix("acde-glass") {
        Some("") => Some("regular".to_owned()),
        Some(rest) if rest.strip_prefix('-') == Some(key) => Some(text.to_owned()),
        _ => None,
    };
    let glass = Glass::from_properties_at(Level::Clear, property, |message| {
        failed.set(true);
        warn(message);
    })?;
    let bytes = glass.to_wire();
    (!failed.get()).then(|| bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preference(level: u32, reduced_transparency: bool, dark: bool) -> Preference {
        Preference { level, reduced_transparency, dark }
    }

    #[test]
    fn the_built_in_materials_are_registered_by_name() {
        assert_eq!(Material::lookup("glass").map(Material::look), Some(Look::Glass));
        assert_eq!(Material::lookup("blur").map(Material::look), Some(Look::Blur));
        assert_eq!(Material::lookup("none").map(Material::name), Some("none"));
        assert!(Material::lookup("mica").is_none());
        assert!(Material::register("blur", Look::Glass).is_none(), "a built-in name is taken");
    }

    #[test]
    fn names_are_short_lowercase_identifiers() {
        assert!(Material::valid("com.example.frost_2-b"));
        for bad in ["", "Glass", "a b", "glass/clear", &"x".repeat(MOST_NAME + 1)] {
            assert!(!Material::valid(bad), "{bad:?}");
            assert!(Material::register(bad, Look::Blur).is_none());
        }
        let frost = Material::register("com.example.frost", Look::Blur).unwrap();
        assert_eq!(Material::lookup("com.example.frost"), Some(frost));
    }

    #[test]
    fn an_unknown_name_draws_as_none_and_warns_once() {
        let warnings = Mutex::new(Vec::new());
        for _ in 0..2 {
            let material = Material::resolve("com.example.missing", |w| warnings.lock().unwrap().push(w));
            assert_eq!(material, Material::none());
        }
        assert_eq!(warnings.lock().unwrap().len(), 1);
        assert_eq!(Material::resolve("", |w| panic!("{w}")), Material::none());
        assert!(Material::none().glass(Role::Regular, Preference::default()).is_none());
    }

    #[test]
    fn blur_has_no_lens_highlight_or_outline_and_follows_the_preference() {
        let blur = Material::lookup("blur").unwrap();
        for level in 0..3 {
            let glass = blur.glass(Role::Regular, preference(level, false, false)).unwrap();
            assert_eq!((glass.lens, glass.highlight, glass.outline, glass.refraction), Default::default());
            assert!(glass.blur[0].radius > 0.0 && glass.tint[3] < 1.0);
        }
        let tinted = blur.glass(Role::Regular, preference(2, false, false)).unwrap();
        let clear = blur.glass(Role::Regular, preference(0, false, false)).unwrap();
        assert!(tinted.tint[3] > clear.tint[3] && tinted.blur[0].radius > clear.blur[0].radius);
        let dark = blur.glass(Role::Regular, preference(1, false, true)).unwrap();
        assert_eq!(&dark.tint[..3], &Glass::opaque(true).tint[..3]);
    }

    #[test]
    fn reduced_transparency_makes_every_material_opaque() {
        let reduced = preference(0, true, false);
        assert_eq!(Material::lookup("blur").unwrap().glass(Role::Regular, reduced).unwrap().tint[3], 1.0);
        let glass = Material::lookup("glass").unwrap().glass(Role::Clear, reduced).unwrap();
        assert_eq!(glass, Glass::preset_in(Role::Clear, Level::Opaque, false));
    }

    #[test]
    fn css_overrides_become_wire_slots_and_leave_the_rest_alone() {
        let warnings = Mutex::new(Vec::new());
        let given = [("outline", "none"), ("blur", "3px 1"), ("highlight", "90deg 0.2"), ("tint", "nonsense"), ("shine", "1")];
        let floats = overrides(given, |w| warnings.lock().unwrap().push(w));
        assert_eq!(warnings.lock().unwrap().len(), 2, "the bad tint and the unknown name");
        let base = Glass::preset(Role::Regular, Level::Medium);
        let glass = Glass::with_overrides(&base, &floats);
        assert_eq!(glass.outline.width, 0.0);
        assert_eq!((glass.blur[0].radius, glass.blur[1].radius), (3.0, 0.0));
        assert_eq!(glass.highlight.width, base.highlight.width, "two words keep the width");
        assert!((glass.highlight.amount - 0.2).abs() < 1e-6);
        assert_eq!((glass.tint, glass.lens, glass.refraction), (base.tint, base.lens, base.refraction));
        assert!(overrides([], |w| panic!("{w}")).is_empty());
    }

    #[test]
    fn overrides_reach_blur_through_the_shared_entry() {
        let blur = Material::lookup("blur").unwrap();
        let mut overrides = [f32::NAN; 6];
        overrides[4] = 3.0;
        let given = blur.with_overrides(Role::Regular, Preference::default(), &overrides).unwrap();
        assert_eq!(given.blur[0].radius, 3.0);
        assert_eq!(given.tint, blur.glass(Role::Regular, Preference::default()).unwrap().tint);
    }
}
