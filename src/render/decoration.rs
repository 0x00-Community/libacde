use crate::gpu::{Glyph, PaintScene, Scene};
use blitz_dom::parley::fontique::{Collection, CollectionOptions, SourceCache};
use blitz_dom::parley::{FontContext, FontFamily, FontWeight, Layout, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty};
use kurbo::{Affine, BezPath, Circle, Line, Rect, RoundedRect, Shape, Stroke, Vec2};
use peniko::{Color, Fill};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, PoisonError, RwLock};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameMetrics {
    pub inset: f64,
    pub dot: f64,
    pub gap: f64,
    pub caption: f64,
    pub title_size: f32,
    pub title_weight: f32,
}

impl Default for FrameMetrics {
    fn default() -> Self {
        Self { inset: 12.0, dot: 12.0, gap: 8.0, caption: 46.0, title_size: 13.0, title_weight: 600.0 }
    }
}

static METRICS: RwLock<Option<FrameMetrics>> = RwLock::new(None);

impl FrameMetrics {
    pub const NUMBERS: usize = 6;

    pub fn current() -> Self {
        METRICS.read().unwrap_or_else(PoisonError::into_inner).unwrap_or_default()
    }

    pub fn set(metrics: Option<Self>) {
        *METRICS.write().unwrap_or_else(PoisonError::into_inner) = metrics;
    }

    pub fn from_numbers(numbers: &[f64]) -> Option<Self> {
        let &[inset, dot, gap, caption, size, weight] = numbers else { return None };
        let d = Self::default();
        let or = |given: f64, fallback: f64| if given >= 0.0 { given } else { fallback };
        Some(Self {
            inset: or(inset, d.inset),
            dot: or(dot, d.dot),
            gap: or(gap, d.gap),
            caption: or(caption, d.caption),
            title_size: or(size, f64::from(d.title_size)) as f32,
            title_weight: or(weight, f64::from(d.title_weight)) as f32,
        })
    }

    pub fn buttons(&self) -> f64 {
        2.0 * self.inset + 3.0 * self.dot + 2.0 * self.gap
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrameStyle {
    #[default]
    Mac,
    Windows,
}

static SESSION: AtomicBool = AtomicBool::new(false);

impl FrameStyle {
    pub fn current() -> Self {
        static NAMED: OnceLock<Option<FrameStyle>> = OnceLock::new();
        let named = *NAMED.get_or_init(|| Self::named(std::env::var("ACDE_FRAME_STYLE").ok().as_deref()));
        named.unwrap_or_else(|| Self::native(SESSION.load(Ordering::Relaxed)))
    }

    pub fn follow_session() {
        SESSION.store(true, Ordering::Relaxed);
    }

    pub fn parse(value: Option<&str>, session: bool) -> Self {
        Self::named(value).unwrap_or_else(|| Self::native(session))
    }

    fn named(value: Option<&str>) -> Option<Self> {
        match value {
            Some("mac") => Some(FrameStyle::Mac),
            Some("windows") => Some(FrameStyle::Windows),
            _ => None,
        }
    }

    fn native(session: bool) -> Self {
        match cfg!(windows) && !session {
            true => FrameStyle::Windows,
            false => FrameStyle::Mac,
        }
    }

    pub fn reserved(self, m: &FrameMetrics) -> f64 {
        match self {
            FrameStyle::Mac => m.buttons(),
            FrameStyle::Windows => 3.0 * m.caption,
        }
    }

    pub fn side(self) -> ButtonSide {
        match self {
            FrameStyle::Mac => ButtonSide::Left,
            FrameStyle::Windows => ButtonSide::Right,
        }
    }

    pub fn align(self) -> TitleAlign {
        match self {
            FrameStyle::Mac => TitleAlign::Center,
            FrameStyle::Windows => TitleAlign::Start,
        }
    }

    pub fn grab_hints(self, w: f64, h: f64, top: f64) -> [([f64; 4], u32); 9] {
        FrameTheme { style: Some(self), ..FrameTheme::default() }.grab_hints(w, h, top)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleAlign {
    Center,
    Start,
}

pub type Rgba = [u8; 4];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameColors {
    pub background: Option<Rgba>,
    pub title: Option<Rgba>,
    pub line: Option<Rgba>,
    pub buttons: Option<Rgba>,
}

impl FrameColors {
    fn or(self, other: FrameColors) -> FrameColors {
        FrameColors {
            background: self.background.or(other.background),
            title: self.title.or(other.title),
            line: self.line.or(other.line),
            buttons: self.buttons.or(other.buttons),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameTheme {
    pub active: FrameColors,
    pub inactive: FrameColors,
    pub light: [FrameColors; 2],
    pub style: Option<FrameStyle>,
    pub side: Option<ButtonSide>,
    pub align: Option<TitleAlign>,
}

impl FrameTheme {
    const DEFAULT: FrameColors =
        FrameColors { background: Some([0xf4, 0xf2, 0xf7, 0xff]), title: Some([0x3a, 0x35, 0x42, 0xff]), line: Some([0, 0, 0, 20]), buttons: None };
    pub const DEFAULT_DARK: FrameColors =
        FrameColors { background: Some([0x2b, 0x28, 0x31, 0xff]), title: Some([0xe6, 0xe2, 0xec, 0xff]), line: Some([0xff, 0xff, 0xff, 24]), buttons: None };

    pub fn style(&self) -> FrameStyle {
        self.style.unwrap_or_else(FrameStyle::current)
    }

    pub fn side(&self) -> ButtonSide {
        self.side.unwrap_or_else(|| self.style().side())
    }

    pub fn colors(&self, active: bool, dark_page: bool) -> FrameColors {
        let none = FrameColors::default();
        let unset = self.active == none && self.inactive == none && self.light == [none; 2];
        let [on, off] = if dark_page || self.light == [none; 2] { [self.active, self.inactive] } else { self.light };
        let chosen = if active { on } else { off.or(on) };
        chosen.or(if unset && dark_page { Self::DEFAULT_DARK } else { Self::DEFAULT })
    }

    pub fn grab_hints(&self, w: f64, h: f64, top: f64) -> [([f64; 4], u32); 9] {
        self.grab_hints_by(&FrameMetrics::current(), w, h, top)
    }

    pub fn grab_hints_by(&self, m: &FrameMetrics, w: f64, h: f64, top: f64) -> [([f64; 4], u32); 9] {
        let (e, c) = (Bar::EDGE, Bar::CORNER);
        let reserved = self.style().reserved(m);
        let bar = match self.side() {
            ButtonSide::Left => [reserved, 0.0, w - reserved, top],
            ButtonSide::Right => [0.0, 0.0, w - reserved, top],
        };
        [
            (bar, 0),
            ([0.0, 0.0, w, e], 1),
            ([0.0, h - e, w, e], 2),
            ([0.0, 0.0, e, h], 4),
            ([w - e, 0.0, e, h], 8),
            ([0.0, 0.0, c, c], 1 | 4),
            ([w - c, 0.0, c, c], 1 | 8),
            ([0.0, h - c, c, c], 2 | 4),
            ([w - c, h - c, c, c], 2 | 8),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameButton {
    Close,
    Minimize,
    Fullscreen,
}

impl FrameButton {
    pub const ALL: [FrameButton; 3] = [FrameButton::Close, FrameButton::Minimize, FrameButton::Fullscreen];

    pub fn key(self, fullscreen: bool) -> &'static str {
        match self {
            FrameButton::Close => "close",
            FrameButton::Minimize => "minimize",
            FrameButton::Fullscreen if fullscreen => "restore",
            FrameButton::Fullscreen => "fullscreen",
        }
    }

    fn index(self) -> usize {
        match self {
            FrameButton::Close => 0,
            FrameButton::Minimize => 1,
            FrameButton::Fullscreen => 2,
        }
    }

    fn caption(self) -> usize {
        match self {
            FrameButton::Minimize => 0,
            FrameButton::Fullscreen => 1,
            FrameButton::Close => 2,
        }
    }

    fn centre(self, m: &FrameMetrics) -> f64 {
        m.inset + m.dot / 2.0 + self.index() as f64 * (m.dot + m.gap)
    }

    fn slot(self, style: FrameStyle, side: ButtonSide, width: f64, m: &FrameMetrics) -> (f64, f64) {
        let start = match side {
            ButtonSide::Left => 0.0,
            ButtonSide::Right => width - style.reserved(m),
        };
        match style {
            FrameStyle::Mac => {
                let reach = (m.dot + m.gap) / 2.0;
                (start + self.centre(m) - reach, start + self.centre(m) + reach)
            }
            FrameStyle::Windows => {
                let x = start + self.caption() as f64 * m.caption;
                (x, x + m.caption)
            }
        }
    }

    fn rgba(self) -> Rgba {
        match self {
            FrameButton::Close => [0xff, 0x5f, 0x57, 0xff],
            FrameButton::Minimize => [0xfe, 0xbc, 0x2e, 0xff],
            FrameButton::Fullscreen => [0x28, 0xc8, 0x40, 0xff],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    pub button: FrameButton,
    pub centre: (f64, f64),
    pub radius: f64,
    pub color: Rgba,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub style: FrameStyle,
    pub fullscreen: bool,
    pub theme: FrameTheme,
    pub active: bool,
    pub dark_page: bool,
    pub minimize: bool,
    pub menus: Vec<String>,
    pub open: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuRow {
    pub label: String,
    pub shortcut: Option<String>,
    pub checked: bool,
    pub disabled: bool,
    pub separator: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuList {
    pub rows: Vec<MenuRow>,
    pub hover: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuPlace {
    pub rect: [f64; 4],
    pub rows: Vec<[f64; 4]>,
}

impl Bar {
    const EDGE: f64 = 4.0;
    const CORNER: f64 = 10.0;

    fn style(&self) -> FrameStyle {
        self.theme.style.unwrap_or(self.style)
    }

    fn side(&self) -> ButtonSide {
        self.theme.side.unwrap_or_else(|| self.style().side())
    }

    fn align(&self) -> TitleAlign {
        self.theme.align.unwrap_or_else(|| self.style().align())
    }

    pub fn pixels(&self) -> (u32, u32) {
        ((self.width * self.scale).round().max(1.0) as u32, (self.height * self.scale).round().max(1.0) as u32)
    }

    fn menus_start(&self, m: &FrameMetrics) -> f64 {
        match self.side() {
            ButtonSide::Left => self.style().reserved(m) + Decoration::MENU_LEAD,
            ButtonSide::Right => m.inset - Decoration::MENU_PAD,
        }
    }

    pub fn shortcut_text(&self, combo: &str) -> String {
        let mut parts: Vec<String> = combo.split('+').map(|p| p.trim().to_ascii_lowercase()).filter(|p| !p.is_empty()).collect();
        let Some(key) = parts.pop() else { return String::new() };
        let command = match self.style() {
            FrameStyle::Mac => "super",
            FrameStyle::Windows => "ctrl",
        };
        for part in parts.iter_mut().filter(|p| *p == "mod") {
            *part = command.to_owned();
        }
        let order = ["ctrl", "control", "alt", "shift", "super", "meta", "logo"];
        parts.sort_by_key(|m| order.iter().position(|o| o == m).unwrap_or(order.len()));
        let mut chars = key.chars();
        let key: String = match (chars.next(), key.chars().count()) {
            (_, 1) => key.to_uppercase(),
            (Some(first), _) => first.to_uppercase().chain(chars).collect(),
            (None, _) => String::new(),
        };
        match self.style() {
            FrameStyle::Mac => {
                let symbol = |m: &str| match m {
                    "ctrl" | "control" => "⌃",
                    "alt" => "⌥",
                    "shift" => "⇧",
                    _ => "⌘",
                };
                parts.iter().map(|m| symbol(m)).collect::<String>() + &key
            }
            FrameStyle::Windows => {
                let name = |m: &str| match m {
                    "ctrl" | "control" => "Ctrl",
                    "alt" => "Alt",
                    "shift" => "Shift",
                    _ => "Win",
                };
                parts.iter().map(|m| name(m)).chain(std::iter::once(key.as_str())).collect::<Vec<_>>().join("+")
            }
        }
    }

    fn dots(&self, m: &FrameMetrics) -> Vec<Dot> {
        if self.style() != FrameStyle::Mac {
            return Vec::new();
        }
        let colors = self.theme.colors(self.active, self.dark_page);
        let off = colors.title.map_or([0; 4], |[r, g, b, a]| [r, g, b, (f32::from(a) * Decoration::OFF_ALPHA).round() as u8]);
        let dot = |button: FrameButton| {
            let (from, to) = button.slot(FrameStyle::Mac, self.side(), self.width, m);
            let lit = self.minimize || button != FrameButton::Minimize;
            let color = if lit { colors.buttons.unwrap_or_else(|| button.rgba()) } else { off };
            Dot { button, centre: ((from + to) / 2.0, self.height / 2.0), radius: m.dot / 2.0, color }
        };
        FrameButton::ALL.into_iter().map(dot).collect()
    }

    pub fn button_at(&self, x: f64, y: f64) -> Option<FrameButton> {
        self.button_by(&FrameMetrics::current(), x, y)
    }

    fn button_by(&self, m: &FrameMetrics, x: f64, y: f64) -> Option<FrameButton> {
        if y < 0.0 || y >= self.height {
            return None;
        }
        FrameButton::ALL.into_iter().filter(|b| self.minimize || *b != FrameButton::Minimize).find(|b| {
            let (from, to) = b.slot(self.style(), self.side(), self.width, m);
            x >= from && x < to
        })
    }
}

pub struct Decoration {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    widths: Option<(Vec<String>, Vec<f64>)>,
    glass_dots: bool,
    metrics: FrameMetrics,
}

impl Default for Decoration {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoration {
    const GLYPH: f64 = 10.0;
    const FAMILY: &'static str = "system-ui, sans-serif";
    const ELLIPSIS: &'static str = "…";
    const RING: Color = Color::from_rgba8(0, 0, 0, 46);
    const MENU_LEAD: f64 = 2.0;
    const MENU_PAD: f64 = 8.0;
    const MENU_WEIGHT: f32 = 400.0;
    const LIST_ROW: f64 = 24.0;
    const LIST_SEPARATOR: f64 = 9.0;
    const LIST_INSET: f64 = 5.0;
    const LIST_TEXT: f64 = 24.0;
    const LIST_END: f64 = 14.0;
    const LIST_GAP: f64 = 28.0;
    const LIST_MIN: f64 = 180.0;
    const LIST_RADIUS: f64 = 8.0;
    const ACCENT: Color = Color::from_rgb8(0x2f, 0x6f, 0xe8);
    const OFF_ALPHA: f32 = 0.16;

    pub fn new() -> Self {
        let fonts =
            FontContext { source_cache: SourceCache::new_shared(), collection: Collection::new(CollectionOptions { shared: false, system_fonts: true }) };
        Self { fonts, layouts: LayoutContext::new(), widths: None, glass_dots: false, metrics: FrameMetrics::current() }
    }

    pub fn glass_dots(&mut self, on: bool) {
        self.glass_dots = on;
    }

    pub fn dots(&self, bar: &Bar) -> Vec<Dot> {
        bar.dots(&self.metrics)
    }

    pub fn button_at(&self, bar: &Bar, x: f64, y: f64) -> Option<FrameButton> {
        bar.button_by(&self.metrics, x, y)
    }

    pub fn grab_hints(&self, theme: &FrameTheme, w: f64, h: f64, top: f64) -> [([f64; 4], u32); 9] {
        theme.grab_hints_by(&self.metrics, w, h, top)
    }

    pub fn scene(&mut self, bar: &Bar) -> Scene {
        let mut scene = Scene::new();
        let scale = Affine::scale(bar.scale);
        let (w, h) = (bar.width, bar.height);
        let colors = bar.theme.colors(bar.active, bar.dark_page);
        let paint = |rgba: Rgba| Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
        let text = colors.title.map_or(Color::TRANSPARENT, paint);
        let off = text.multiply_alpha(Self::OFF_ALPHA);
        scene.fill(Fill::NonZero, scale, colors.background.map_or(Color::TRANSPARENT, paint), None, &Rect::new(0.0, 0.0, w, h));
        scene.fill(Fill::NonZero, scale, colors.line.map_or(Color::TRANSPARENT, paint), None, &Rect::new(0.0, h - 1.0, w, h));
        let (style, side) = (bar.style(), bar.side());
        for dot in self.dots(bar).into_iter().filter(|_| !self.glass_dots) {
            scene.fill(Fill::NonZero, scale, paint(dot.color), None, &Circle::new(dot.centre, dot.radius));
            scene.stroke(&Stroke::new(0.5), scale, Self::RING, None, &Circle::new(dot.centre, dot.radius - 0.25));
        }
        for button in FrameButton::ALL.into_iter().filter(|_| style == FrameStyle::Windows) {
            let (from, to) = button.slot(style, side, w, &self.metrics);
            let glyph = Self::glyph(button, bar.fullscreen, ((from + to) / 2.0 - Self::GLYPH / 2.0).round(), ((h - Self::GLYPH) / 2.0).round());
            let ink = if bar.minimize || button != FrameButton::Minimize { colors.buttons.map_or(text, paint) } else { off };
            scene.stroke(&Stroke::new(1.0), scale, ink, None, &glyph);
        }
        let labels = self.menu_labels(bar);
        for (i, (label, [x, y, w, h])) in bar.menus.iter().zip(&labels).enumerate() {
            if bar.open == Some(i) {
                let shape = RoundedRect::new(*x, y + 4.0, x + w, y + h - 4.0, 5.0);
                scene.fill(Fill::NonZero, scale, text.multiply_alpha(0.12), None, &shape);
            }
            self.text(&mut scene, label, bar.scale, (x + Self::MENU_PAD, *y), *h, text);
        }
        let lead = labels.last().map(|[x, _, w, _]| x + w);
        self.title(&mut scene, bar, text, lead);
        scene
    }

    fn glyph(button: FrameButton, fullscreen: bool, x: f64, y: f64) -> BezPath {
        let g = Self::GLYPH;
        let (l, t, r, b) = (x + 0.5, y + 0.5, x + g - 0.5, y + g - 0.5);
        let mut path = BezPath::new();
        match button {
            FrameButton::Minimize => path.extend(Line::new((l, y + g / 2.0 + 0.5), (r, y + g / 2.0 + 0.5)).path_elements(0.1)),
            FrameButton::Close => {
                path.extend(Line::new((l, t), (r, b)).path_elements(0.1));
                path.extend(Line::new((r, t), (l, b)).path_elements(0.1));
            }
            FrameButton::Fullscreen if fullscreen => {
                path.extend(Rect::new(l, t + 2.0, r - 2.0, b).path_elements(0.1));
                path.move_to((l + 2.0, t + 2.0));
                path.line_to((l + 2.0, t));
                path.line_to((r, t));
                path.line_to((r, b - 2.0));
                path.line_to((r - 2.0, b - 2.0));
            }
            FrameButton::Fullscreen => path.extend(Rect::new(l, t, r, b).path_elements(0.1)),
        }
        path
    }

    pub fn menu_labels(&mut self, bar: &Bar) -> Vec<[f64; 4]> {
        if self.widths.as_ref().is_none_or(|(labels, _)| *labels != bar.menus) {
            let widths = bar.menus.iter().map(|label| f64::from(self.styled(label, Self::MENU_WEIGHT, 1.0).width()) + 2.0 * Self::MENU_PAD).collect();
            self.widths = Some((bar.menus.clone(), widths));
        }
        let widths = self.widths.as_ref().map(|(_, w)| w.as_slice()).unwrap_or_default();
        let mut x = bar.menus_start(&self.metrics);
        widths
            .iter()
            .map(|&w| {
                let rect = [x, 0.0, w, bar.height];
                x += w;
                rect
            })
            .collect()
    }

    pub fn menu_list(&mut self, bar: &Bar, list: &MenuList, at: f64, below: f64) -> (Scene, MenuPlace) {
        let mut widths = (0.0f64, 0.0f64);
        for row in list.rows.iter().filter(|r| !r.separator) {
            widths.0 = widths.0.max(f64::from(self.styled(&row.label, Self::MENU_WEIGHT, 1.0).width()));
            if let Some(keys) = &row.shortcut {
                widths.1 = widths.1.max(f64::from(self.styled(&bar.shortcut_text(keys), Self::MENU_WEIGHT, 1.0).width()));
            }
        }
        let gap = if widths.1 > 0.0 { Self::LIST_GAP } else { 0.0 };
        let width = (Self::LIST_TEXT + widths.0 + gap + widths.1 + Self::LIST_END + 2.0 * Self::LIST_INSET).max(Self::LIST_MIN).ceil();
        let x = at.min(bar.width - width).max(0.0);
        let mut y = below + Self::LIST_INSET;
        let mut rows = Vec::with_capacity(list.rows.len());
        for row in &list.rows {
            let h = if row.separator { Self::LIST_SEPARATOR } else { Self::LIST_ROW };
            rows.push([x + Self::LIST_INSET, y, width - 2.0 * Self::LIST_INSET, h]);
            y += h;
        }
        let rect = [x, below, width, y + Self::LIST_INSET - below];
        let colors = bar.theme.colors(true, bar.dark_page);
        let dark = colors.background.is_some_and(|[r, g, b, _]| u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 < 128_000);
        let (panel, ink) = match dark {
            true => (Color::from_rgb8(0x2f, 0x2c, 0x35), Color::from_rgb8(0xec, 0xe8, 0xf2)),
            false => (Color::from_rgb8(0xff, 0xff, 0xff), Color::from_rgb8(0x24, 0x21, 0x2b)),
        };
        let mut scene = Scene::new();
        let scale = Affine::scale(bar.scale);
        let [rx, ry, rw, rh] = rect;
        let outline = Rect::new(rx, ry, rx + rw, ry + rh);
        scene.draw_box_shadow(scale, outline.with_origin((rx, ry + 3.0)), Color::from_rgba8(0, 0, 0, 60), Self::LIST_RADIUS, 8.0);
        let shape = RoundedRect::from_rect(outline, Self::LIST_RADIUS);
        scene.fill(Fill::NonZero, scale, panel, None, &shape);
        scene.stroke(&Stroke::new(1.0), scale, ink.multiply_alpha(0.14), None, &shape);
        for (i, (row, &[x, y, w, h])) in list.rows.iter().zip(&rows).enumerate() {
            if row.separator {
                let mid = (y + h / 2.0).floor() + 0.5;
                scene.stroke(&Stroke::new(1.0), scale, ink.multiply_alpha(0.12), None, &Line::new((x + 6.0, mid), (x + w - 6.0, mid)));
                continue;
            }
            let lit = list.hover == Some(i) && !row.disabled;
            if lit {
                scene.fill(Fill::NonZero, scale, Self::ACCENT, None, &RoundedRect::new(x, y, x + w, y + h, 5.0));
            }
            let color = match (lit, row.disabled) {
                (true, _) => Color::WHITE,
                (false, true) => ink.multiply_alpha(0.35),
                (false, false) => ink,
            };
            if row.checked {
                self.text(&mut scene, "✓", bar.scale, (x + 7.0, y), h, color);
            }
            self.text(&mut scene, &row.label, bar.scale, (x + Self::LIST_TEXT - Self::LIST_INSET, y), h, color);
            if let Some(keys) = &row.shortcut {
                let keys = bar.shortcut_text(keys);
                let kw = f64::from(self.styled(&keys, Self::MENU_WEIGHT, 1.0).width());
                let dim = if lit { Color::WHITE.multiply_alpha(0.85) } else { color.multiply_alpha(0.6) };
                self.text(&mut scene, &keys, bar.scale, (x + w - Self::LIST_END + Self::LIST_INSET - kw, y), h, dim);
            }
        }
        (scene, MenuPlace { rect, rows })
    }

    fn text(&mut self, scene: &mut Scene, text: &str, scale: f64, (x, y): (f64, f64), height: f64, color: Color) {
        let layout = self.styled(text, Self::MENU_WEIGHT, scale);
        let Some(line) = layout.lines().next() else { return };
        let top = (height * scale - f64::from(line.metrics().line_height)) / 2.0;
        draw_line(scene, &line, Affine::translate((x * scale, y * scale + top)), color);
    }

    fn title(&mut self, scene: &mut Scene, bar: &Bar, color: Color, lead: Option<f64>) {
        let (reserved, inset) = (bar.style().reserved(&self.metrics), self.metrics.inset);
        let (mut left, right) = match (bar.align(), bar.side()) {
            (TitleAlign::Center, _) => (reserved, reserved),
            (TitleAlign::Start, ButtonSide::Right) => (inset, reserved + inset),
            (TitleAlign::Start, ButtonSide::Left) => (reserved + inset, inset),
        };
        if let Some(lead) = lead {
            left = left.max(lead + inset);
        }
        let room = (bar.width - left - right) * bar.scale;
        if bar.title.is_empty() || room <= 0.0 {
            return;
        }
        let mut layout = self.layout(&bar.title, bar.scale);
        if f64::from(layout.width()) > room {
            let cuts: Vec<usize> = bar.title.char_indices().map(|(i, _)| i).collect();
            let (mut fits, mut over) = (0, cuts.len());
            while over - fits > 1 {
                let mid = (fits + over) / 2;
                match f64::from(self.layout(&Self::cut(&bar.title, cuts[mid]), bar.scale).width()) <= room {
                    true => fits = mid,
                    false => over = mid,
                }
            }
            layout = self.layout(&Self::cut(&bar.title, cuts[fits]), bar.scale);
            if f64::from(layout.width()) > room {
                return;
            }
        }
        let Some(line) = layout.lines().next() else { return };
        let x = match (bar.align(), lead) {
            (TitleAlign::Center, None) => (bar.width * bar.scale - f64::from(layout.width())) / 2.0,
            (TitleAlign::Center, Some(_)) => ((bar.width * bar.scale - f64::from(layout.width())) / 2.0).max(left * bar.scale),
            (TitleAlign::Start, _) => left * bar.scale,
        };
        let y = (bar.height * bar.scale - f64::from(line.metrics().line_height)) / 2.0;
        draw_line(scene, &line, Affine::translate((x, y)), color);
    }

    fn cut(title: &str, at: usize) -> String {
        format!("{}{}", title[..at].trim_end(), Self::ELLIPSIS)
    }

    fn layout(&mut self, text: &str, scale: f64) -> Layout<()> {
        self.styled(text, self.metrics.title_weight, scale)
    }

    fn styled(&mut self, text: &str, weight: f32, scale: f64) -> Layout<()> {
        let mut builder = self.layouts.ranged_builder(&mut self.fonts, text, scale as f32, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::from(Self::FAMILY)));
        builder.push_default(StyleProperty::FontSize(self.metrics.title_size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.0)));
        builder.push_default(StyleProperty::Locale(blitz_dom::parley::Language::parse("zh-Hans").ok()));
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        layout
    }
}

pub(crate) fn draw_line(scene: &mut Scene, line: &blitz_dom::parley::Line<'_, ()>, transform: Affine, color: Color) {
    for item in line.items() {
        let PositionedLayoutItem::GlyphRun(run) = item else { continue };
        let glyphs = run.positioned_glyphs().map(|g| Glyph { id: g.id as _, x: g.x, y: g.y });
        let r = run.run();
        let coords: &[i16] = bytemuck::cast_slice(r.normalized_coords());
        scene.draw_glyphs(r.font(), r.font_size(), false, coords, Vec2::ZERO, Fill::NonZero, color, 1.0, transform, None, glyphs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> Bar {
        Bar {
            title: String::new(),
            width: 300.0,
            height: 32.0,
            scale: 1.0,
            style: FrameStyle::Mac,
            fullscreen: false,
            theme: FrameTheme::default(),
            active: true,
            dark_page: false,
            minimize: true,
            menus: Vec::new(),
            open: None,
        }
    }

    #[test]
    fn a_frame_keeps_the_metrics_it_was_made_with() {
        let open = Decoration::new();
        FrameMetrics::set(Some(FrameMetrics { inset: 20.0, ..FrameMetrics::default() }));
        let later = Decoration::new();
        FrameMetrics::set(None);
        assert_eq!(open.dots(&bar())[0].centre.0, 18.0, "a window open before keeps its buttons where they were");
        assert_eq!(later.dots(&bar())[0].centre.0, 26.0, "a window opened after takes the new measures");
        let theme = FrameTheme { style: Some(FrameStyle::Mac), ..FrameTheme::default() };
        assert_eq!(open.grab_hints(&theme, 640.0, 480.0, 32.0)[0].0[0], FrameMetrics::default().buttons());
        assert_eq!(later.grab_hints(&theme, 640.0, 480.0, 32.0)[0].0[0], FrameMetrics::default().buttons() + 16.0);
    }
}
