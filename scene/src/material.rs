use crate::glass::{BlurLayer, Glass, LAYERS, Level, Preference, Role};
use std::borrow::Cow;
use std::sync::{Mutex, OnceLock, RwLock};

pub const MOST_NAME: usize = 64;
const LIGHT_TINT: [f32; 3] = [0.97, 0.97, 0.98];
const DARK_TINT: [f32; 3] = [0.11, 0.11, 0.12];

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

impl Glass {
    pub fn blur(level: Level, dark: bool) -> Self {
        let (layers, tint) = match level {
            Level::Clear => ([(6.0, 1.0), (12.0, 0.6), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)], 0.15),
            Level::Medium => ([(10.0, 1.0), (20.0, 0.8), (32.0, 0.5), (0.0, 0.0), (0.0, 0.0)], 0.35),
            Level::Tinted => ([(16.0, 1.0), (32.0, 0.8), (48.0, 0.6), (0.0, 0.0), (0.0, 0.0)], 0.6),
            Level::Opaque => ([(24.0, 1.0), (48.0, 1.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)], 1.0),
        };
        let [r, g, b] = if dark { DARK_TINT } else { LIGHT_TINT };
        Self { blur: blur_layers(layers), tint: [r, g, b, tint], smoothing: 0.5, ..Self::default() }
    }
}

fn blur_layers(pairs: [(f32, f32); LAYERS]) -> [BlurLayer; LAYERS] {
    pairs.map(|(radius, opacity)| BlurLayer { radius, opacity })
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
        assert_eq!(&dark.tint[..3], &DARK_TINT[..]);
    }

    #[test]
    fn reduced_transparency_makes_every_material_opaque() {
        let reduced = preference(0, true, false);
        assert_eq!(Material::lookup("blur").unwrap().glass(Role::Regular, reduced).unwrap().tint[3], 1.0);
        let glass = Material::lookup("glass").unwrap().glass(Role::Clear, reduced).unwrap();
        assert_eq!(glass, Glass::preset_in(Role::Clear, Level::Opaque, false));
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
