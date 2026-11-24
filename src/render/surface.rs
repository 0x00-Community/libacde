use std::collections::HashMap;

pub struct DragImage {
    pub width: u32,
    pub height: u32,
    pub scene: crate::gpu::Scene,
    pub hotspot: (i32, i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardEntry {
    pub mime: String,
    pub data: Vec<u8>,
}

impl ClipboardEntry {
    pub const TEXT: &str = "text/plain";
    pub const PNG: &str = "image/png";
    pub const URI_LIST: &str = "text/uri-list";
    pub const TYPES: [&str; 3] = [Self::TEXT, Self::PNG, Self::URI_LIST];
    const WAYLAND_TEXT: [&str; 3] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];

    pub fn new(mime: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self { mime: mime.into(), data: data.into() }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::new(Self::TEXT, text.into())
    }

    pub fn wayland_mimes(mime: &str) -> &'static [&'static str] {
        match mime {
            Self::TEXT => &Self::WAYLAND_TEXT,
            Self::PNG => &[Self::PNG],
            Self::URI_LIST => &[Self::URI_LIST],
            _ => &[],
        }
    }

    pub fn web_type(native: &str) -> Option<&'static str> {
        Self::TYPES.into_iter().find(|web| Self::wayland_mimes(web).contains(&native))
    }

    pub fn web_types<'a>(offered: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut types: Vec<String> = Vec::new();
        for web in offered.into_iter().filter_map(Self::web_type) {
            if !types.iter().any(|known| known == web) {
                types.push(web.to_owned());
            }
        }
        types
    }

    pub fn types(entries: &[ClipboardEntry]) -> Vec<String> {
        Self::web_types(entries.iter().map(|entry| entry.mime.as_str()))
    }

    pub fn find<'a>(entries: &'a [ClipboardEntry], mime: &str) -> Option<&'a ClipboardEntry> {
        entries.iter().find(|entry| entry.mime == mime)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowShadow {
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    pub spread: f64,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowLook {
    pub radius: f64,
    pub superellipse: f64,
    pub focused: Vec<WindowShadow>,
    pub unfocused: Vec<WindowShadow>,
    pub motion: acde_scene::motion::Preset,
    pub titlebar: Option<f64>,
    pub frame: Option<crate::render::FrameMetrics>,
}

impl WindowLook {
    const LAYER: usize = 5;
    const MOST_LAYERS: usize = 4;

    pub fn from_numbers(numbers: &[f64]) -> Option<Self> {
        let (&[radius, superellipse], rest) = numbers.split_first_chunk::<2>()?;
        let (focused, rest) = Self::layers(rest)?;
        let (unfocused, rest) = Self::layers(rest)?;
        let (&motion, rest) = rest.split_first()?;
        let (titlebar, frame) = match rest {
            [] => (None, None),
            [titlebar] => (Some(*titlebar), None),
            [titlebar, metrics @ ..] => (Some(*titlebar), Some(crate::render::FrameMetrics::from_numbers(metrics)?)),
        };
        let titlebar = titlebar.filter(|t| *t > 0.0);
        Some(Self { radius, superellipse, focused, unfocused, motion: acde_scene::motion::Preset::from_wire(motion as u32), titlebar, frame })
    }

    fn layers(numbers: &[f64]) -> Option<(Vec<WindowShadow>, &[f64])> {
        let (&count, rest) = numbers.split_first()?;
        let count = Some(count as usize).filter(|n| *n <= Self::MOST_LAYERS)?;
        let length = count.checked_mul(Self::LAYER).filter(|n| *n <= rest.len())?;
        let (layers, rest) = rest.split_at(length);
        let shadow = |l: &[f64]| WindowShadow { x: l[0], y: l[1], blur: l[2], spread: l[3], color: (l[4] as u32).to_be_bytes() };
        Some((layers.chunks_exact(Self::LAYER).map(shadow).collect(), rest))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowMaterial {
    pub name: String,
    pub role: acde_scene::glass::Role,
    pub group: Option<String>,
    pub spacing: f32,
    pub overrides: Vec<f32>,
    pub titlebar: bool,
}

impl WindowMaterial {
    const SPACING: f32 = 10.0;
    const MOST_SPACING: f64 = 256.0;

    pub fn from_options(name: &str, options: &serde_json::Value) -> Result<Self, String> {
        use acde_scene::material::Material;
        if !name.is_empty() && !Material::valid(name) {
            return Err(format!("setMaterial: {name:?} is not a material name ([a-z0-9._-], at most 64 bytes)"));
        }
        let field = |key: &str| options.get(key).filter(|v| !v.is_null());
        let text = |key: &str| field(key).map(|v| v.as_str().ok_or_else(|| format!("setMaterial: {key} is a string")).map(str::to_owned)).transpose();
        let role = match text("role")?.as_deref() {
            None => acde_scene::glass::Role::Regular,
            Some(given) => acde_scene::glass::Role::parse(given).ok_or_else(|| format!("setMaterial: role {given:?} is regular or clear"))?,
        };
        let group = text("group")?;
        if let Some(group) = group.as_deref().filter(|g| !Material::valid(g)) {
            return Err(format!("setMaterial: group {group:?} is [a-z0-9._-], at most 64 bytes"));
        }
        let spacing = match field("spacing") {
            None => Self::SPACING,
            Some(v) => v.as_f64().filter(|s| (0.0..=Self::MOST_SPACING).contains(s)).ok_or("setMaterial: spacing is 0 to 256 logical pixels")? as f32,
        };
        let titlebar = match text("region")?.as_deref() {
            None | Some("window") => false,
            Some("titlebar") => true,
            Some(given) => return Err(format!("setMaterial: region {given:?} is window or titlebar")),
        };
        let overrides = match field("overrides") {
            None => Vec::new(),
            Some(serde_json::Value::Object(map)) => {
                let mut pairs = Vec::with_capacity(map.len());
                for (key, value) in map {
                    pairs.push((key.as_str(), value.as_str().ok_or_else(|| format!("setMaterial: overrides.{key} is a string (CSS)"))?));
                }
                acde_scene::material::overrides(pairs, |warning| log::warn!("setMaterial: {warning}"))
            }
            Some(_) => return Err("setMaterial: overrides maps parameter names to CSS strings".into()),
        };
        let material = Material::resolve(name, |warning| log::warn!("setMaterial: {warning}"));
        Ok(Self { name: material.name().to_owned(), role, group, spacing, overrides, titlebar })
    }
}

pub type ClipboardTypesReply = Box<dyn FnOnce(Vec<String>) + Send>;
pub type ClipboardReadReply = Box<dyn FnOnce(Option<Vec<u8>>) + Send>;

pub enum SurfaceCommand {
    RequestFrame,
    SetClipboard(Vec<ClipboardEntry>),
    ClipboardTypes(ClipboardTypesReply),
    ReadClipboard { mime: String, reply: ClipboardReadReply },
    StartDrag(HashMap<String, String>, Option<DragImage>),
    Mode(u32),
    State(u32),
    GrabHint([i32; 4], u32),
    Backdrop([i32; 4], [f64; 5]),
    Activate,
    Grab(u32, bool),
    PointerLock(bool),
    OpenPanel { id: u32, token: u64, rect: [i32; 4], exclusive: bool, user_only: bool, popup: bool, parent: Option<u32> },
    PlacePanel { id: u32, rect: [i32; 4], input: [i32; 4] },
    ClosePanel(u32),
    ReservePanel { id: u32, edge: u32, thickness: f64 },
    DoubleClick(u32),
    FrameTheme(crate::render::FrameTheme),
    PageDark(bool),
    Title(String),
    ImeArea([f64; 4]),
    WindowLook(Option<WindowLook>),
    Material(WindowMaterial),
}

#[cfg(test)]
mod tests {
    use super::{ClipboardEntry, WindowLook, WindowShadow};

    #[test]
    fn wayland_types_become_web_types_in_order_without_unknown_or_repeated_ones() {
        let offered = ["image/png", "UTF8_STRING", "text/html", "text/plain;charset=utf-8", "text/uri-list"];
        assert_eq!(ClipboardEntry::web_types(offered), ["image/png", "text/plain", "text/uri-list"]);
        assert_eq!(ClipboardEntry::wayland_mimes("text/plain"), ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"]);
        assert!(ClipboardEntry::wayland_mimes("text/html").is_empty());
    }

    #[test]
    fn a_window_look_comes_from_the_numbers_the_page_hands_over() {
        use acde_scene::motion::Preset;
        let numbers = [12.0, 2.0, 1.0, 0.0, 8.0, 24.0, -4.0, f64::from(0x0000_0040u32), 0.0, 1.0];
        let look = WindowLook::from_numbers(&numbers).expect("a look");
        let shadow = WindowShadow { x: 0.0, y: 8.0, blur: 24.0, spread: -4.0, color: [0, 0, 0, 0x40] };
        let expected =
            WindowLook { radius: 12.0, superellipse: 2.0, focused: vec![shadow], unfocused: Vec::new(), motion: Preset::Reduced, titlebar: None, frame: None };
        assert_eq!(look, expected);
        assert_eq!(WindowLook::from_numbers(&[1.0, 2.0, 0.0, 0.0, 0.0]).map(|l| l.motion), Some(Preset::Standard));
        assert_eq!(WindowLook::from_numbers(&numbers[..8]), None, "a layer cut short");
        assert_eq!(WindowLook::from_numbers(&[1.0, 2.0, 0.0, 0.0]), None, "no motion");
        assert_eq!(WindowLook::from_numbers(&[1.0, 2.0, 0.0, 0.0, 0.0, 40.0]).and_then(|l| l.titlebar), Some(40.0), "the desktop's titlebar height");
        assert_eq!(WindowLook::from_numbers(&[1.0, 2.0, 0.0, 0.0, 0.0, 40.0, 7.0]), None, "numbers left over");
        let framed = WindowLook::from_numbers(&[1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 14.0, -1.0, 6.0, -1.0, 12.0, -1.0]).expect("a look with frame metrics");
        assert_eq!(framed.titlebar, None, "0 is the runtime's titlebar");
        let metrics = framed.frame.expect("metrics");
        let d = crate::render::FrameMetrics::default();
        assert_eq!((metrics.inset, metrics.dot, metrics.gap, metrics.caption), (14.0, d.dot, 6.0, d.caption));
        assert_eq!((metrics.title_size, metrics.title_weight), (12.0, d.title_weight));
        assert_eq!(metrics.buttons(), 2.0 * 14.0 + 3.0 * d.dot + 2.0 * 6.0);
        let five: Vec<f64> = [12.0, 2.0, 5.0].into_iter().chain([0.0; 25]).chain([0.0, 0.0]).collect();
        assert_eq!(WindowLook::from_numbers(&five), None, "more than four layers");
    }
}
