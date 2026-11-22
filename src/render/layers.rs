use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::gpu::ColorMatrix;
use crate::gpu::Effect;
use crate::gpu::Filter;
use crate::gpu::FilterEffect;
use crate::gpu::Scene;
use crate::gpu::{LayerCommand, RenderCommand};
use acde_scene::corner::Corners;
use acde_scene::glass::group::{GlassGroup, Piece};
use acde_scene::glass::{Appearance, Glass, Membership};
use kurbo::{Affine, BezPath, PathEl, Rect, Shape};

use crate::render::commands::{Builder, Commands};
use crate::render::damage::{Damage, Kind};

#[derive(Default)]
pub struct Drawing {
    pub commands: Commands,
    pub tolerance: f64,
    pub(crate) draw: Vec<Rect>,
}

impl Drawing {
    pub fn scene_for(&self, rect: Rect) -> Scene {
        crate::render::region::scene_for(&self.commands, &self.draw, self.tolerance, rect)
    }

    pub fn scene(&self) -> Scene {
        self.commands.to_scene(self.tolerance)
    }
}

pub struct Layer {
    pub drawing: Drawing,
    pub bounds: Option<Rect>,
    pub areas: Vec<Rect>,
    pub backdrop: Option<Backdrop>,
    pub filter: Option<LayerFilter>,
}

pub struct Backdrop {
    pub shape: BezPath,
    pub bounds: Rect,
    pub clip: Rect,
    pub filter: Arc<Filter>,
    pub scale: f64,
    pub corners: Option<Corners>,
    pub glass: Option<Glass>,
    pub group: Option<GlassGroup>,
    membership: Option<Membership>,
    shaped: bool,
}

impl Backdrop {
    pub fn radius(&self) -> f64 {
        let b = self.shape.bounding_box();
        let max = (b.width() / 2.0).min(b.height() / 2.0);
        if let Some(corners) = self.corners {
            return corners.radius.clamp(0.0, max);
        }
        self.shape
            .elements()
            .iter()
            .filter_map(|el| match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => Some(*p),
                PathEl::ClosePath => None,
            })
            .filter(|p| (p.y - b.y0).abs() < 0.01)
            .map(|p| p.x - b.x0)
            .fold(f64::MAX, f64::min)
            .clamp(0.0, max)
    }

    pub fn exponent(&self) -> f64 {
        self.corners.map_or(2.0, |c| c.exponent)
    }

    pub fn glass(&self) -> Option<&Glass> {
        self.glass.as_ref()
    }

    pub fn group(&self) -> Option<&GlassGroup> {
        self.group.as_ref()
    }

    pub fn compose(&self) -> crate::gpu::Backdrop {
        let r = self.bounds.intersect(self.clip);
        crate::gpu::Backdrop {
            rect: [r.x0 as f32, r.y0 as f32, r.width().max(0.0) as f32, r.height().max(0.0) as f32],
            radius: self.radius() as f32,
            exponent: self.exponent() as f32,
            glass: self.glass,
            group: self.group,
            effect: self.effect(),
        }
    }

    fn piece(&self) -> Option<Piece> {
        let b = self.bounds;
        let rect = [b.x0 as f32, b.y0 as f32, b.width() as f32, b.height() as f32];
        let piece = Piece::of_rect(rect, self.radius() as f32, self.exponent() as f32, self.glass?);
        Some(Piece { id: self.membership.and_then(|m| m.id), ..piece })
    }

    pub fn blur(&self) -> f64 {
        self.filter
            .effects()
            .iter()
            .map(|e| match e {
                FilterEffect::GaussianBlur(b) => f64::from(b.std_deviation) * self.scale,
                _ => 0.0,
            })
            .fold(0.0, f64::hypot)
    }

    pub fn effect(&self) -> Effect {
        let mut effect = Effect::default();
        for e in self.filter.effects() {
            match e {
                FilterEffect::GaussianBlur(b) => effect.blur(b.std_deviation * self.scale as f32),
                FilterEffect::ColorMatrix(m) => effect.matrix(m.0),
                FilterEffect::DropShadow(_) => unsupported(&self.filter),
            }
        }
        effect
    }

    pub fn saturate(&self) -> f32 {
        self.matrices().filter_map(|m| Self::amount(m, ColorMatrix::saturate, 1.0 - m[1] / 0.7152)).product()
    }

    pub fn brightness(&self) -> f32 {
        self.matrices().filter_map(|m| Self::amount(m, ColorMatrix::brightness, m[0])).product()
    }

    fn matrices(&self) -> impl Iterator<Item = &[f32; 20]> {
        self.filter.effects().iter().filter_map(|e| match e {
            FilterEffect::ColorMatrix(m) => Some(&m.0),
            _ => None,
        })
    }

    fn amount(m: &[f32; 20], build: fn(f32) -> ColorMatrix, guess: f32) -> Option<f32> {
        build(guess).0.iter().zip(m).all(|(a, b)| (a - b).abs() < 1e-3).then_some(guess)
    }
}

static WARNED_FULL_GROUP: AtomicBool = AtomicBool::new(false);

pub(crate) fn animate_glass(layers: &mut [Layer], appearance: &mut Appearance, now: f64) -> Option<Rect> {
    let mut groups: Vec<&mut GlassGroup> = layers.iter_mut().filter_map(|l| l.backdrop.as_mut()?.group.as_mut()).collect();
    if !appearance.step(&mut groups, now, acde_scene::motion::system()) {
        return None;
    }
    let mut area: Option<Rect> = None;
    for b in layers.iter_mut().filter_map(|l| l.backdrop.as_mut()) {
        let Some(([x, y, w, h], reach)) = b.group.as_ref().and_then(|g| Some((g.bounds()?, g.reach()))) else { continue };
        b.bounds = b.bounds.union(Rect::new(x.into(), y.into(), f64::from(x + w), f64::from(y + h)));
        let drawn = b.bounds.inflate(reach, reach);
        area = Some(area.map_or(drawn, |a| a.union(drawn)));
    }
    area
}

fn merge_glass(layers: &mut [Layer]) {
    let members: Vec<(usize, Membership)> =
        layers.iter().enumerate().filter_map(|(i, l)| l.backdrop.as_ref().filter(|b| b.glass.is_some()).and_then(|b| Some((i, b.membership?)))).collect();
    let mut taken = vec![false; members.len()];
    for start in 0..members.len() {
        if !taken[start] {
            let joined = joined_with(&members, &mut taken, start);
            let spacing = joined.iter().map(|&k| members[k].1.spacing).fold(0.0, f32::max);
            merge_into_first(layers, joined.iter().map(|&k| members[k].0), spacing);
        }
    }
}

fn joined_with(members: &[(usize, Membership)], taken: &mut [bool], start: usize) -> Vec<usize> {
    taken[start] = true;
    let mut joined = vec![start];
    let mut next = 0;
    while let Some(&at) = joined.get(next) {
        for k in 0..members.len() {
            if !taken[k] && members[k].1.joins(&members[at].1) {
                taken[k] = true;
                joined.push(k);
            }
        }
        next += 1;
    }
    joined.sort_unstable();
    joined
}

fn merge_into_first(layers: &mut [Layer], members: impl Iterator<Item = usize>, spacing: f32) {
    let mut group = GlassGroup::new(spacing);
    let mut merged = Vec::new();
    for i in members {
        let Some(piece) = layers[i].backdrop.as_ref().and_then(Backdrop::piece) else { continue };
        if group.push(piece) {
            merged.push(i);
        } else if !WARNED_FULL_GROUP.swap(true, Ordering::Relaxed) {
            log::warn!("glass: more than {} pieces merge together; the rest are drawn on their own", acde_scene::glass::group::MOST_PIECES);
        }
    }
    let [first, rest @ ..] = merged.as_slice() else { return };
    let area = |i: usize| layers[i].backdrop.as_ref().map(|b| (b.bounds, b.clip)).expect("a member has a backdrop");
    let (bounds, clip) = rest.iter().map(|&i| area(i)).fold(area(*first), |(b, c), (bi, ci)| (b.union(bi), c.union(ci)));
    rest.iter().for_each(|&i| layers[i].backdrop = None);
    if let Some(b) = layers[*first].backdrop.as_mut() {
        (b.group, b.bounds, b.clip) = (Some(group), bounds, clip);
    }
}

const LOGGED_FILTERS: usize = 64;

static LOGGED: Mutex<Vec<Filter>> = Mutex::new(Vec::new());

fn unsupported(filter: &Filter) {
    once(filter, || log::warn!("backdrop-filter: drop-shadow() is not supported and is left out ({:?})", filter.effects()));
}

fn nested(filter: &Filter, what: &str) {
    once(filter, || log::warn!("{what} inside an element with a filter is not supported yet and is left out ({:?})", filter.effects()));
}

fn once(filter: &Filter, warn: impl FnOnce()) {
    let mut logged = LOGGED.lock().unwrap_or_else(|e| e.into_inner());
    if logged.len() < LOGGED_FILTERS && !logged.contains(filter) {
        logged.push(filter.clone());
        warn();
    }
}

pub struct LayerFilter {
    pub filter: Arc<Filter>,
    pub bounds: Rect,
    pub scale: f64,
}

enum Pushed {
    At(usize),
    Own(Box<RenderCommand>),
}

pub(crate) struct Split<'a> {
    source: &'a Commands,
    layers: Vec<Layer>,
    current: Builder<'a>,
    current_draws: bool,
    current_draw: Vec<Rect>,
    current_kinds: Vec<Kind>,
    this: Rect,
    this_kind: Kind,
    open: Vec<Pushed>,
    open_kinds: Vec<Kind>,
    backdrop: Option<Backdrop>,
    filter: Option<LayerFilter>,
    filter_depth: Option<usize>,
    tolerance: f64,
    device: f64,
    viewport: Rect,
}

impl<'a> Split<'a> {
    pub(crate) fn layers(commands: Commands, tolerance: f64, kinds: &[Kind], draw: &[Rect], device: f64, viewport: Rect) -> Result<Vec<Layer>, Commands> {
        let effects = commands.iter().any(|(c, _)| match c {
            RenderCommand::PushLayer(l) => l.filter.is_some() || l.backdrop_filter.is_some(),
            _ => false,
        });
        if !effects {
            return Err(commands);
        }
        let mut split = Split {
            source: &commands,
            layers: Vec::new(),
            current: commands.builder(),
            current_draws: false,
            current_draw: Vec::new(),
            current_kinds: Vec::new(),
            this: Rect::ZERO,
            this_kind: Kind::Draw,
            open: Vec::new(),
            open_kinds: Vec::new(),
            backdrop: None,
            filter: None,
            filter_depth: None,
            tolerance,
            device,
            viewport,
        };
        for ((i, (command, t)), (&this, &kind)) in commands.iter().enumerate().zip(draw.iter().zip(kinds)) {
            (split.this, split.this_kind) = (this, kind);
            split.command(i, command, t);
        }
        split.cut();
        merge_glass(&mut split.layers);
        Ok(split.layers)
    }

    fn command(&mut self, i: usize, command: &RenderCommand, t: Affine) {
        match command {
            RenderCommand::PushLayer(l) if self.filter_depth.is_none() && l.backdrop_filter.is_some() => {
                let shape = (t * l.transform) * l.clip.clone();
                let bounds = shape.bounding_box();
                let backdrop = Backdrop {
                    shape,
                    bounds,
                    clip: self.clip(),
                    filter: l.backdrop_filter.clone().unwrap(),
                    scale: self.scale(t * l.transform),
                    corners: l.shape.map(|s| Corners { radius: s.corners.radius * Self::linear(t * l.transform), ..s.corners }),
                    glass: l.shape.and_then(|s| s.glass).map(|g| g.scaled(Self::linear(t * l.transform))),
                    group: None,
                    membership: l.shape.and_then(|s| s.group).map(|m| m.scaled(Self::linear(t * l.transform))),
                    shaped: false,
                };
                self.cut();
                self.backdrop = Some(backdrop);
                self.push_plain(command, t, |l| l.backdrop_filter = None);
            }
            RenderCommand::PushLayer(l) if self.filter_depth.is_none() && l.filter.is_some() => {
                let filter = l.filter.clone().unwrap();
                let content = ((t * l.transform) * l.clip.clone()).bounding_box();
                let e = filter.linear_bounds_expansion(&(Affine::scale(self.device) * t * l.transform));
                let bounds = Rect::new(content.x0 + e.x0, content.y0 + e.y0, content.x1 + e.x1, content.y1 + e.y1);
                self.cut();
                self.filter = Some(LayerFilter { filter, bounds: bounds.intersect(self.clip()), scale: self.scale(t * l.transform) });
                self.filter_depth = Some(self.open.len());
                self.push_plain(command, t, |l| l.filter = None);
            }
            RenderCommand::PushLayer(l) if l.filter.is_some() || l.backdrop_filter.is_some() => {
                if let Some(filter) = &l.filter {
                    nested(filter, "filter");
                }
                if let Some(filter) = &l.backdrop_filter {
                    nested(filter, "backdrop-filter");
                }
                self.push(Pushed::At(i), self.this_kind);
            }
            RenderCommand::PushLayer(_) | RenderCommand::PushClipLayer(_) => self.push(Pushed::At(i), self.this_kind),
            RenderCommand::PopLayer => {
                self.current.source(i);
                self.current_draw.push(Rect::ZERO);
                self.current_kinds.push(Kind::Pop);
                self.open.pop();
                self.open_kinds.pop();
                if self.filter_depth == Some(self.open.len()) {
                    self.filter_depth = None;
                    self.cut();
                }
            }
            RenderCommand::Fill(f) => {
                if let Some(b) = self.backdrop.as_mut().filter(|b| !b.shaped) {
                    let shape = (t * f.transform) * f.shape.clone();
                    let r = shape.bounding_box();
                    if (r.x0 - b.bounds.x0).abs() < 1.0
                        && (r.y0 - b.bounds.y0).abs() < 1.0
                        && (r.x1 - b.bounds.x1).abs() < 1.0
                        && (r.y1 - b.bounds.y1).abs() < 1.0
                    {
                        b.shape = shape;
                    }
                    b.shaped = true;
                }
                self.draws(i);
            }
            _ => self.draws(i),
        }
    }

    fn draws(&mut self, i: usize) {
        self.current.source(i);
        self.current_draws = true;
        self.current_draw.push(self.this);
        self.current_kinds.push(self.this_kind);
    }

    fn push_plain(&mut self, command: &RenderCommand, t: Affine, strip: fn(&mut LayerCommand)) {
        let RenderCommand::PushLayer(mut plain) = Commands::placed(command, t) else { unreachable!() };
        strip(&mut plain);
        let plain = RenderCommand::PushLayer(plain);
        let kind = Kind::of(&plain, Affine::IDENTITY);
        self.push(Pushed::Own(Box::new(plain)), kind);
    }

    fn push(&mut self, pushed: Pushed, kind: Kind) {
        self.reopen(&pushed);
        self.current_draw.push(Rect::ZERO);
        self.current_kinds.push(kind);
        self.open.push(pushed);
        self.open_kinds.push(kind);
    }

    fn reopen(&mut self, pushed: &Pushed) {
        match pushed {
            Pushed::At(i) => self.current.source(*i),
            Pushed::Own(c) => self.current.owned((**c).clone()),
        }
    }

    fn shape(&self, pushed: &Pushed) -> Option<BezPath> {
        let (command, t) = match pushed {
            Pushed::At(i) => self.source.get(*i)?,
            Pushed::Own(c) => (&**c, Affine::IDENTITY),
        };
        match command {
            RenderCommand::PushLayer(l) => Some((t * l.transform) * l.clip.clone()),
            RenderCommand::PushClipLayer(c) => Some((t * c.transform) * c.clip.clone()),
            _ => None,
        }
    }

    fn cut(&mut self) {
        let draws = std::mem::take(&mut self.current_draws);
        let mut current = std::mem::replace(&mut self.current, self.source.builder());
        let open = std::mem::take(&mut self.open);
        for pushed in &open {
            self.reopen(pushed);
            current.owned(RenderCommand::PopLayer);
        }
        self.open = open;
        let len = current.len();
        let commands = current.finish();
        let mut draw = std::mem::replace(&mut self.current_draw, vec![Rect::ZERO; self.open.len()]);
        draw.resize(len, Rect::ZERO);
        let mut kinds = std::mem::replace(&mut self.current_kinds, self.open_kinds.clone());
        kinds.resize(len, Kind::Pop);
        let (backdrop, filter) = (self.backdrop.take(), self.filter.take());
        if draws || backdrop.is_some() {
            let bounds = Damage::content_bounds(&kinds, &draw, self.viewport);
            let spills = commands.iter().any(|(c, _)| matches!(c, RenderCommand::PushLayer(l) if l.blend.compose != peniko::Compose::SrcOver));
            let areas = if spills { Vec::new() } else { Damage::content_areas(&kinds, &draw, self.viewport) };
            self.layers.push(Layer { drawing: Drawing { commands, tolerance: self.tolerance, draw }, bounds, areas, backdrop, filter });
        }
    }

    fn clip(&self) -> Rect {
        let infinite = Rect::new(f64::MIN / 2.0, f64::MIN / 2.0, f64::MAX / 2.0, f64::MAX / 2.0);
        self.open.iter().filter_map(|o| self.shape(o)).fold(infinite, |r, shape| r.intersect(shape.bounding_box()))
    }

    fn scale(&self, t: Affine) -> f64 {
        self.device * Self::linear(t)
    }

    fn linear(t: Affine) -> f64 {
        t.determinant().abs().sqrt()
    }
}
