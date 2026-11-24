macro_rules! wire {
    ($(#[$m:meta])* $what:literal $name:ident { $($(#[$vm:meta])* $v:ident = $s:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u32)]
        pub enum $name {
            $($(#[$vm])* $v),*
        }

        impl $name {
            pub const ALL: &[Self] = &[$(Self::$v),*];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$v => $s),*
                }
            }
        }

        impl TryFrom<u32> for $name {
            type Error = u32;

            fn try_from(v: u32) -> Result<Self, u32> {
                Self::ALL.get(v as usize).copied().ok_or(v)
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, String> {
                let found = Self::ALL.iter().copied().find(|v| v.as_str() == s);
                found.ok_or_else(|| format!(concat!("unknown ", $what, " {}"), s))
            }
        }
    };
}

mod event;
mod files;
mod motion;
mod pace;
mod plan;
mod screen;
mod shortcut;
mod state;

pub use event::*;
pub use files::{FileFilter, OpenRequest, SaveRequest};
pub use motion::*;
pub use pace::Pacer;
pub use plan::*;
pub use screen::*;
pub use shortcut::*;
pub use state::*;

use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn covers(&self, o: &Rect) -> bool {
        self.x <= o.x && self.y <= o.y && self.right() >= o.right() && self.bottom() >= o.bottom()
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.intersect(o).is_some()
    }

    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let r = Rect::new(self.x.max(o.x), self.y.max(o.y), self.right().min(o.right()), self.bottom().min(o.bottom()));
        let r = Rect::new(r.x, r.y, r.w - r.x, r.h - r.y);
        (!r.is_empty()).then_some(r)
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }

    pub fn translate(&self, x: f64, y: f64) -> Rect {
        Rect::new(self.x + x, self.y + y, self.w, self.h)
    }

    pub fn scale(&self, s: f64) -> Rect {
        Rect::new(self.x * s, self.y * s, self.w * s, self.h * s)
    }

    pub fn round_out(&self) -> Rect {
        let (x, y) = (self.x.floor(), self.y.floor());
        Rect::new(x, y, self.right().ceil() - x, self.bottom().ceil() - y)
    }

    pub fn dragged(&self, edges: u32, dx: f64, dy: f64, (min_w, min_h): (f64, f64)) -> Rect {
        let mut r = *self;
        if edges == 0 {
            return r.translate(dx, dy);
        }
        if edges & edge::LEFT != 0 {
            let dx = dx.min(r.w - min_w);
            r.x += dx;
            r.w -= dx;
        }
        if edges & edge::RIGHT != 0 {
            r.w = (r.w + dx).max(min_w);
        }
        if edges & edge::TOP != 0 {
            let dy = dy.min(r.h - min_h);
            r.y += dy;
            r.h -= dy;
        }
        if edges & edge::BOTTOM != 0 {
            r.h = (r.h + dy).max(min_h);
        }
        r
    }
}

pub mod edge {
    pub const TOP: u32 = 1;
    pub const BOTTOM: u32 = 2;
    pub const LEFT: u32 = 4;
    pub const RIGHT: u32 = 8;
}

wire!(#[derive(Default)] "mode" Mode {
    Fullscreen = "fullscreen",
    #[default]
    Free = "free",
    Fixed = "fixed",
    Pinned = "pinned",
});

impl Mode {
    pub fn can_move(self) -> bool {
        matches!(self, Mode::Free | Mode::Fixed)
    }

    pub fn can_resize(self) -> bool {
        matches!(self, Mode::Free)
    }
}

wire!("source" WheelSource { Wheel = "wheel", Finger = "finger", Continuous = "continuous" });

pub const BTN_LEFT: u32 = 0x110;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Injected {
    Move(f64, f64),
    Motion(f64, f64),
    Button { button: u32, pressed: bool },
    Wheel { dx: f64, dy: f64, source: WheelSource },
    Key { code: u32, pressed: bool },
    Pen(Option<Pen>),
}

#[cfg_attr(feature = "napi", napi_derive::napi(object))]
#[derive(Default)]
pub struct InjectEvent {
    #[cfg_attr(feature = "napi", napi(js_name = "type"))]
    pub kind: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub button: Option<u32>,
    pub dx: Option<f64>,
    pub dy: Option<f64>,
    pub source: Option<String>,
    pub code: Option<u32>,
}

impl TryFrom<InjectEvent> for Injected {
    type Error = String;

    fn try_from(e: InjectEvent) -> Result<Self, String> {
        let need = |v: Option<f64>, name: &str| v.ok_or_else(|| format!("inject {}: missing {name}", e.kind));
        Ok(match e.kind.as_str() {
            "move" => Injected::Move(need(e.x, "x")?, need(e.y, "y")?),
            "motion" => Injected::Motion(need(e.dx, "dx")?, need(e.dy, "dy")?),
            "down" | "up" => Injected::Button { button: e.button.unwrap_or(BTN_LEFT), pressed: e.kind == "down" },
            "wheel" => {
                let source = match e.source.as_deref() {
                    None => WheelSource::Wheel,
                    Some(s) => s.parse().map_err(|_| format!("inject wheel: unknown source {s}"))?,
                };
                Injected::Wheel { dx: e.dx.unwrap_or(0.0), dy: e.dy.unwrap_or(0.0), source }
            }
            "keydown" | "keyup" => {
                let code = e.code.ok_or_else(|| format!("inject {}: missing code", e.kind))?;
                Injected::Key { code, pressed: e.kind == "keydown" }
            }
            other => return Err(format!("inject: unknown type {other}")),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanvasId(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u32);

pub struct Canvas {
    pub id: CanvasId,
    pub rect: Rect,
    pub scale: f64,
    pub z: Vec<WindowId>,
    pub focus: Option<WindowId>,
}

impl Canvas {
    pub fn physical(&self) -> Rect {
        self.rect.scale(self.scale)
    }
}

pub struct Window {
    pub id: WindowId,
    pub canvas: CanvasId,
    pub mode: Mode,
    pub rect: Rect,
    pub parent: Option<WindowId>,
    pub modal: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnknownCanvas,
    UnknownWindow,
    Taken,
    NotOnCanvas,
    ModeRefusesMove,
    ModeRefusesResize,
    EmptyRect,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::UnknownCanvas => "unknown canvas",
            Error::UnknownWindow => "unknown window",
            Error::Taken => "the window id is taken",
            Error::NotOnCanvas => "window is on another canvas",
            Error::ModeRefusesMove => "this mode does not move",
            Error::ModeRefusesResize => "this mode does not resize",
            Error::EmptyRect => "empty rect",
        })
    }
}

impl std::error::Error for Error {}

#[derive(Default)]
pub struct WindowTable {
    canvases: BTreeMap<CanvasId, Canvas>,
    windows: BTreeMap<WindowId, Window>,
    next_canvas: u64,
}

impl WindowTable {
    pub fn add_canvas(&mut self, rect: Rect, scale: f64) -> CanvasId {
        self.next_canvas += 1;
        let id = CanvasId(self.next_canvas);
        self.canvases.insert(id, Canvas { id, rect, scale, z: Vec::new(), focus: None });
        id
    }

    pub fn replace_canvas(&mut self, old: CanvasId, rect: Rect, scale: f64) -> Result<CanvasId, Error> {
        let before = self.canvases.remove(&old).ok_or(Error::UnknownCanvas)?;
        self.next_canvas += 1;
        let id = CanvasId(self.next_canvas);
        for w in self.windows.values_mut().filter(|w| w.canvas == old) {
            w.canvas = id;
            if w.mode == Mode::Fullscreen {
                w.rect = rect;
            }
        }
        self.canvases.insert(id, Canvas { id, rect, scale, z: before.z, focus: before.focus });
        Ok(id)
    }

    pub fn canvas(&self, id: CanvasId) -> Option<&Canvas> {
        self.canvases.get(&id)
    }

    pub fn window(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id)
    }

    pub fn create(&mut self, id: WindowId, canvas: CanvasId, mode: Mode, rect: Rect, parent: Option<WindowId>) -> Result<(), Error> {
        let c = self.canvases.get(&canvas).ok_or(Error::UnknownCanvas)?;
        if self.windows.contains_key(&id) {
            return Err(Error::Taken);
        }
        if let Some(p) = parent
            && self.windows.get(&p).is_none_or(|w| w.canvas != canvas)
        {
            return Err(Error::NotOnCanvas);
        }
        let rect = match mode {
            Mode::Fullscreen => c.rect,
            _ => rect,
        };
        if rect.is_empty() {
            return Err(Error::EmptyRect);
        }
        self.windows.insert(id, Window { id, canvas, mode, rect, parent, modal: false });
        self.canvases.get_mut(&canvas).expect("checked above").z.push(id);
        Ok(())
    }

    pub fn remove(&mut self, id: WindowId) -> Option<Window> {
        let w = self.windows.remove(&id)?;
        if let Some(c) = self.canvases.get_mut(&w.canvas) {
            c.z.retain(|&x| x != id);
            if c.focus == Some(id) {
                c.focus = None;
            }
        }
        for other in self.windows.values_mut() {
            if other.parent == Some(id) {
                other.parent = None;
            }
        }
        Some(w)
    }

    pub fn set_parent(&mut self, id: WindowId, parent: Option<WindowId>, modal: bool) -> Result<(), Error> {
        let canvas = self.windows.get(&id).ok_or(Error::UnknownWindow)?.canvas;
        if let Some(p) = parent
            && (p == id || self.windows.get(&p).is_none_or(|w| w.canvas != canvas) || self.descendants(id).contains(&p))
        {
            return Err(Error::NotOnCanvas);
        }
        let w = self.windows.get_mut(&id).ok_or(Error::UnknownWindow)?;
        w.parent = parent;
        w.modal = modal && parent.is_some();
        Ok(())
    }

    pub fn descendants(&self, id: WindowId) -> Vec<WindowId> {
        let Some(canvas) = self.windows.get(&id).and_then(|w| self.canvases.get(&w.canvas)) else { return Vec::new() };
        let mut family = vec![id];
        let mut grew = true;
        while grew {
            grew = false;
            for w in self.windows.values() {
                if w.parent.is_some_and(|p| family.contains(&p)) && !family.contains(&w.id) {
                    family.push(w.id);
                    grew = true;
                }
            }
        }
        canvas.z.iter().copied().filter(|w| *w != id && family.contains(w)).collect()
    }

    pub fn front_family(&mut self, id: WindowId) -> Result<(), Error> {
        self.front(id)?;
        for child in self.descendants(id) {
            self.front(child)?;
        }
        Ok(())
    }

    fn modal_of(&self, window: WindowId, parent: WindowId) -> bool {
        self.windows.get(&window).is_some_and(|w| w.parent == Some(parent) && w.modal)
    }

    pub fn modal_child(&self, id: WindowId) -> Option<WindowId> {
        let canvas = self.canvases.get(&self.windows.get(&id)?.canvas)?;
        let mut found = None;
        let mut current = id;
        while let Some(&child) = canvas.z.iter().rev().find(|c| self.modal_of(**c, current)) {
            if Some(child) == found {
                break;
            }
            found = Some(child);
            current = child;
        }
        found
    }

    pub fn set_mode(&mut self, id: WindowId, mode: Mode) -> Result<(), Error> {
        let w = self.windows.get_mut(&id).ok_or(Error::UnknownWindow)?;
        if mode == Mode::Fullscreen {
            let c = self.canvases.get(&w.canvas).ok_or(Error::UnknownCanvas)?;
            w.rect = c.rect;
        }
        w.mode = mode;
        Ok(())
    }

    pub fn place(&mut self, id: WindowId, x: f64, y: f64) -> Result<(), Error> {
        let w = self.windows.get_mut(&id).ok_or(Error::UnknownWindow)?;
        if !w.mode.can_move() {
            return Err(Error::ModeRefusesMove);
        }
        w.rect.x = x;
        w.rect.y = y;
        Ok(())
    }

    pub fn resize(&mut self, id: WindowId, w: f64, h: f64) -> Result<(), Error> {
        let win = self.windows.get_mut(&id).ok_or(Error::UnknownWindow)?;
        if !win.mode.can_resize() {
            return Err(Error::ModeRefusesResize);
        }
        if w <= 0.0 || h <= 0.0 {
            return Err(Error::EmptyRect);
        }
        win.rect.w = w;
        win.rect.h = h;
        Ok(())
    }

    pub fn set_rect(&mut self, id: WindowId, rect: Rect) -> Result<(), Error> {
        let win = self.windows.get_mut(&id).ok_or(Error::UnknownWindow)?;
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return Err(Error::EmptyRect);
        }
        win.rect = rect;
        Ok(())
    }

    pub fn lower(&mut self, id: WindowId) -> Result<(), Error> {
        let canvas = self.windows.get(&id).ok_or(Error::UnknownWindow)?.canvas;
        let z = &mut self.canvases.get_mut(&canvas).ok_or(Error::UnknownCanvas)?.z;
        z.retain(|&x| x != id);
        z.insert(0, id);
        Ok(())
    }

    pub fn front(&mut self, id: WindowId) -> Result<(), Error> {
        let window = self.windows.get(&id).ok_or(Error::UnknownWindow)?;
        let canvas = window.canvas;
        let pinned = window.mode == Mode::Pinned;
        let modes: BTreeMap<WindowId, Mode> = self.windows.iter().map(|(k, w)| (*k, w.mode)).collect();
        let z = &mut self.canvases.get_mut(&canvas).ok_or(Error::UnknownCanvas)?.z;
        z.retain(|&x| x != id);
        let at = match pinned {
            true => z.len(),
            false => z.iter().position(|x| modes.get(x) == Some(&Mode::Pinned)).unwrap_or(z.len()),
        };
        z.insert(at, id);
        Ok(())
    }

    pub fn focus(&mut self, canvas: CanvasId, id: Option<WindowId>) -> Result<(), Error> {
        let c = self.canvases.get_mut(&canvas).ok_or(Error::UnknownCanvas)?;
        if let Some(id) = id
            && !c.z.contains(&id)
        {
            return Err(Error::NotOnCanvas);
        }
        c.focus = id;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_is_none_at_zero() {
        assert_eq!(Blur::of(0.0), None);
        assert_eq!(Blur::of(-1.0), None);
        assert_eq!(Blur::of(f32::NAN), None);
    }

    #[test]
    fn blur_strength_follows_sigma() {
        let mut last = 0.0;
        for i in 1..=900 {
            let sigma = i as f32 * 0.1;
            let blur = Blur::of(sigma).unwrap();
            let got = blur.sigma();
            assert!((got - sigma).abs() <= sigma * 0.01 + 1e-3, "sigma {sigma} gives {got} ({blur:?})");
            assert!(got > last, "sigma {sigma} gives {got}, not more than {last}");
            last = got;
        }
    }

    #[test]
    fn blur_levels_grow_with_sigma() {
        assert_eq!(Blur::of(0.5).unwrap().levels, 1);
        assert_eq!(Blur::of(3.0).unwrap().levels, 1);
        assert_eq!(Blur::of(6.0).unwrap().levels, 2);
        assert_eq!(Blur::of(40.0).unwrap().levels, 5);
        assert!(Blur::of(1.0).unwrap().offset < Blur::of(3.0).unwrap().offset);
    }

    fn add(t: &mut WindowTable, c: CanvasId, mode: Mode, rect: Rect) -> WindowId {
        let id = WindowId(t.windows.len() as u32 + 1);
        t.create(id, c, mode, rect, None).unwrap();
        id
    }

    fn table() -> (WindowTable, CanvasId) {
        let mut t = WindowTable::default();
        let c = t.add_canvas(Rect::new(0.0, 0.0, 100.0, 100.0), 2.0);
        (t, c)
    }

    #[test]
    fn canvas_gives_the_only_absolute_size() {
        let (mut t, c) = table();
        let canvas = t.canvas(c).unwrap();
        assert_eq!(canvas.physical(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let w = add(&mut t, c, Mode::Free, Rect::new(10.0, 10.0, 20.0, 10.0));
        let plan = CompositePlan::build(&t, &[input(w, false, Rect::default())], &[]);
        assert_eq!(plan.layer(w).unwrap().dst, Rect::new(20.0, 20.0, 40.0, 20.0));
    }

    #[test]
    fn fullscreen_fills_the_canvas() {
        let (mut t, c) = table();
        let w = add(&mut t, c, Mode::Fullscreen, Rect::new(1.0, 2.0, 3.0, 4.0));
        assert_eq!(t.window(w).unwrap().rect, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert!(!t.place(w, 5.0, 5.0).is_ok());
        assert!(!t.resize(w, 10.0, 10.0).is_ok());
        assert!(t.set_mode(w, Mode::Free).is_ok());
        assert!(t.place(w, 5.0, 5.0).is_ok());
    }

    #[test]
    fn fixed_moves_but_does_not_resize() {
        let (mut t, c) = table();
        let w = add(&mut t, c, Mode::Fixed, Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(t.place(w, 20.0, 20.0).is_ok());
        assert_eq!(t.resize(w, 30.0, 30.0), Err(Error::ModeRefusesResize));
        assert_eq!(t.window(w).unwrap().rect, Rect::new(20.0, 20.0, 10.0, 10.0));
    }

    #[test]
    fn pinned_never_moves() {
        let (mut t, c) = table();
        let w = add(&mut t, c, Mode::Pinned, Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(t.place(w, 20.0, 20.0), Err(Error::ModeRefusesMove));
        assert_eq!(t.resize(w, 30.0, 30.0), Err(Error::ModeRefusesResize));
    }

    #[test]
    fn windows_may_exceed_the_canvas_and_are_clipped() {
        let (mut t, c) = table();
        let w = add(&mut t, c, Mode::Free, Rect::new(90.0, 10.0, 30.0, 10.0));
        let probe = [input(w, false, Rect::default())];
        let shown = |t: &WindowTable| CompositePlan::build(t, &probe, &[]).layer(w).map(|l| l.clip);
        assert_eq!(shown(&t), Some(Rect::new(180.0, 20.0, 20.0, 20.0)));
        t.place(w, 200.0, 0.0).unwrap();
        assert_eq!(shown(&t), None);
    }

    #[test]
    fn focus_is_per_canvas() {
        let (mut t, c) = table();
        let w = add(&mut t, c, Mode::Free, Rect::new(0.0, 0.0, 10.0, 10.0));
        t.focus(c, Some(w)).unwrap();
        assert_eq!(t.canvas(c).unwrap().focus, Some(w));
        t.remove(w);
        assert_eq!(t.canvas(c).unwrap().focus, None);
        assert_eq!(t.focus(c, Some(w)), Err(Error::NotOnCanvas));
    }

    #[test]
    fn damage_scales_and_clips_to_the_canvas() {
        let (mut t, c) = table();
        let w = add(&mut t, c, Mode::Free, Rect::new(50.0, 50.0, 20.0, 20.0));
        let damaged = |t: &WindowTable, local: Rect| {
            let mut i = input(w, false, Rect::default());
            i.damage = vec![local];
            CompositePlan::build(t, &[i], &[]).damage
        };
        assert_eq!(damaged(&t, Rect::new(0.0, 0.0, 20.0, 20.0)), vec![Rect::new(100.0, 100.0, 20.0, 20.0)]);
        t.place(w, 90.0, 50.0).unwrap();
        assert_eq!(damaged(&t, Rect::new(0.0, 0.0, 40.0, 40.0)), vec![Rect::new(180.0, 100.0, 20.0, 40.0)]);
    }

    #[test]
    fn moving_merges_the_old_and_new_rects() {
        let (mut t, c) = table();
        let w = add(&mut t, c, Mode::Free, Rect::new(0.0, 0.0, 10.0, 10.0));
        let old = t.window(w).unwrap().rect;
        t.place(w, 40.0, 0.0).unwrap();
        let mut i = input(w, false, Rect::default());
        i.moved_from = Some(old);
        let d = CompositePlan::build(&t, &[i], &[]).damage;
        assert_eq!(d.len(), 2);
        assert!(d.contains(&Rect::new(0.0, 0.0, 20.0, 20.0)));
        assert!(d.contains(&Rect::new(80.0, 0.0, 20.0, 20.0)));
    }

    fn input(w: WindowId, opaque: bool, rect: Rect) -> Input {
        let opaque = if opaque { vec![Rect::new(0.0, 0.0, rect.w * 2.0, rect.h * 2.0)] } else { Vec::new() };
        Input { window: w, opaque, ..Default::default() }
    }

    #[test]
    fn plan_culls_what_opaque_windows_cover() {
        let (mut t, c) = table();
        let below = add(&mut t, c, Mode::Free, Rect::new(10.0, 10.0, 20.0, 20.0));
        let top = add(&mut t, c, Mode::Fullscreen, Rect::default());
        let full = t.window(top).unwrap().rect;
        let mut inputs = vec![input(below, false, Rect::new(10.0, 10.0, 20.0, 20.0)), input(top, true, full)];
        inputs[0].damage = vec![Rect::new(0.0, 0.0, 4.0, 4.0)];
        let plan = CompositePlan::build(&t, &inputs, &[]);
        assert_eq!(plan.layers.iter().map(|l| l.window).collect::<Vec<_>>(), vec![top]);
        assert!(plan.damage.is_empty(), "damage under an opaque window is not shown: {:?}", plan.damage);
        assert_eq!(plan.route, Route::Composite);
        inputs[1].direct_buffer = true;
        assert_eq!(CompositePlan::build(&t, &inputs, &[]).route, Route::Direct);
        inputs[1].look.opacity = 0.5;
        let plan = CompositePlan::build(&t, &inputs, &[]);
        assert_eq!(plan.layers.len(), 2);
        assert_eq!(plan.route, Route::Composite);
    }

    #[test]
    fn plan_keeps_at_most_eight_damage_rects() {
        let (mut t, c) = table();
        let w = add(&mut t, c, Mode::Fullscreen, Rect::default());
        let mut i = input(w, false, Rect::default());
        i.damage = (0..20).map(|k| Rect::new(f64::from(k) * 10.0, 0.0, 2.0, 2.0)).collect();
        let plan = CompositePlan::build(&t, &[i], &[]);
        assert!(plan.damage.len() <= MAX_DAMAGE);
        for k in 0..20 {
            let x = f64::from(k) * 10.0;
            assert!(plan.damage.iter().any(|d| d.covers(&Rect::new(x, 0.0, 2.0, 2.0))));
        }
    }

    #[test]
    fn backdrop_redraws_when_what_is_under_it_changes() {
        let (mut t, c) = table();
        let below = add(&mut t, c, Mode::Fullscreen, Rect::default());
        let glass = add(&mut t, c, Mode::Free, Rect::new(10.0, 10.0, 40.0, 40.0));
        let mut under = input(below, true, Rect::default());
        under.damage = vec![Rect::new(30.0, 30.0, 2.0, 2.0)];
        let mut top = input(glass, false, Rect::default());
        let rect = Rect::new(0.0, 0.0, 80.0, 80.0);
        top.backdrop = Some(Effect::Backdrop { rect, radius: 0.0, blur: 8.0, saturate: 1.0, brightness: 1.0 });
        let plan = CompositePlan::build(&t, &[under.clone(), top.clone()], &[]);
        assert!(plan.layers[1].redraw);
        assert!(plan.damage.iter().any(|d| d.covers(&Rect::new(20.0, 20.0, 80.0, 80.0))), "{:?}", plan.damage);
        under.damage.clear();
        top.damage = vec![Rect::new(0.0, 0.0, 2.0, 2.0)];
        let plan = CompositePlan::build(&t, &[under, top], &[]);
        assert!(!plan.layers[1].redraw, "the glass window's own content does not blur again");
    }

    #[test]
    fn backdrop_redraws_when_a_window_leaves_from_under_it() {
        let (mut t, c) = table();
        let moving = add(&mut t, c, Mode::Free, Rect::new(10.0, 10.0, 20.0, 20.0));
        let glass = add(&mut t, c, Mode::Free, Rect::new(0.0, 0.0, 50.0, 50.0));
        let old = t.window(moving).unwrap().rect;
        t.place(moving, 300.0, 0.0).unwrap();
        let mut under = input(moving, false, Rect::default());
        under.moved_from = Some(old);
        let mut top = input(glass, false, Rect::default());
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        top.backdrop = Some(Effect::Backdrop { rect, radius: 0.0, blur: 8.0, saturate: 1.0, brightness: 1.0 });
        let plan = CompositePlan::build(&t, &[under, top], &[]);
        assert_eq!(plan.layers.iter().map(|l| l.window).collect::<Vec<_>>(), vec![glass]);
        assert!(plan.layers[0].redraw);
        assert!(plan.damage.iter().any(|d| d.covers(&Rect::new(20.0, 20.0, 40.0, 40.0))), "{:?}", plan.damage);
    }

    #[test]
    fn shortcuts_first_come_session_wins() {
        let mut s = Shortcuts::default();
        let combo = Combo { mods: Combo::CTRL, key: 20 };
        let b = |scope, owner| Binding { combo, scope, owner };
        assert!(s.register(b(Scope::Desktop, 1), "").is_ok());
        assert!(s.register(b(Scope::Desktop, 2), "").is_err());
        assert!(s.register(b(Scope::App, 7), "").is_ok());
        assert!(s.register(b(Scope::App, 7), "").is_ok());
        assert!(s.register(b(Scope::App, 8), "").is_ok());
        assert!(s.register(b(Scope::Session, 0), "").is_err());
        assert_eq!(s.find(combo, Some(8)).map(|b| b.owner), Some(8));
        assert_eq!(s.find(combo, Some(3)).map(|b| b.scope), Some(Scope::Desktop));
        s.forget(8);
        assert_eq!(s.find(combo, Some(8)).map(|b| b.scope), Some(Scope::Desktop));
        let other = Combo { mods: Combo::ALT, key: 20 };
        assert!(s.register(Binding { combo: other, scope: Scope::Session, owner: 0 }, "").is_ok());
        assert!(s.register(Binding { combo: other, scope: Scope::App, owner: 9 }, "").is_err());
    }

    #[test]
    fn mod_is_command_on_a_mac_and_control_elsewhere() {
        let key = |_: &str| Some(31);
        let mods = Combo::parse("shift+mod+s", key).unwrap().mods;
        let command = if cfg!(target_os = "macos") { Combo::SUPER } else { Combo::CTRL };
        assert_eq!(mods, Combo::SHIFT | command);
    }

    #[test]
    fn a_menu_item_shortcut_beats_the_app_one_and_keeps_its_own_name() {
        let mut s = Shortcuts::default();
        let combo = Combo { mods: Combo::CTRL, key: 49 };
        let b = |scope, owner| Binding { combo, scope, owner };
        assert!(s.register(b(Scope::App, 7), "ctrl+n").is_ok());
        assert!(s.register(b(Scope::Menu, 7), "Ctrl+N").is_ok());
        assert!(s.register(b(Scope::Menu, 7), "Ctrl+N").is_ok());
        assert!(s.register(b(Scope::Menu, 8), "ctrl+n").is_ok());
        let found = s.find(combo, Some(7)).unwrap();
        assert_eq!(found, b(Scope::Menu, 7));
        assert_eq!(s.name(found), Some("Ctrl+N"));
        assert_eq!(s.find(combo, Some(3)), None, "only while the owner is focused");
        s.unregister(combo, Scope::Menu, 7);
        assert_eq!(s.find(combo, Some(7)), Some(b(Scope::App, 7)));
        assert_eq!(s.name(b(Scope::App, 7)), Some("ctrl+n"));
        s.forget(8);
        assert_eq!(s.find(combo, Some(8)), None);
        assert!(s.register(Binding { combo, scope: Scope::Session, owner: 0 }, "").is_err(), "taken by the app and menu bindings");
    }

    #[test]
    fn transition_runs_from_delay_to_end() {
        let to = Look { opacity: 0.0, scale: 0.5, ..Look::default() };
        let tr = Transition { from: Look::default(), to, start_ms: 100.0, duration_ms: 200.0, easing: Easing::Linear };
        assert_eq!(tr.at(50.0), (Look::default(), false));
        let (mid, done) = tr.at(200.0);
        assert!(!done && (mid.scale - 0.75).abs() < 1e-6 && (mid.opacity - 0.5).abs() < 1e-6);
        assert_eq!(tr.at(300.0), (to, true));
        assert!((Easing::EaseInOut.at(0.5) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn modal_children_take_the_parents_turn() {
        let (mut t, c) = table();
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let parent = add(&mut t, c, Mode::Free, r);
        let dialog = add(&mut t, c, Mode::Free, r);
        let inner = add(&mut t, c, Mode::Free, r);
        assert_eq!(t.modal_child(parent), None);
        t.set_parent(dialog, Some(parent), false).unwrap();
        assert_eq!(t.modal_child(parent), None);
        t.set_parent(dialog, Some(parent), true).unwrap();
        t.set_parent(inner, Some(dialog), true).unwrap();
        assert_eq!(t.modal_child(parent), Some(inner));
        assert_eq!(t.set_parent(parent, Some(parent), true), Err(Error::NotOnCanvas));
        t.remove(dialog);
        assert_eq!(t.modal_child(parent), None);
    }

    #[test]
    fn children_follow_their_parent_up() {
        let (mut t, c) = table();
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let parent = add(&mut t, c, Mode::Free, r);
        let child = add(&mut t, c, Mode::Free, r);
        let other = add(&mut t, c, Mode::Free, r);
        let inner = add(&mut t, c, Mode::Free, r);
        t.set_parent(child, Some(parent), false).unwrap();
        t.set_parent(inner, Some(child), true).unwrap();
        assert_eq!(t.descendants(parent), vec![child, inner]);
        assert_eq!(t.set_parent(parent, Some(inner), false), Err(Error::NotOnCanvas));
        t.front(parent).unwrap();
        assert_eq!(t.canvas(c).unwrap().z, vec![child, other, inner, parent]);
        t.front_family(parent).unwrap();
        assert_eq!(t.canvas(c).unwrap().z, vec![other, parent, child, inner]);
    }
}
