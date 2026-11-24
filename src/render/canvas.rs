use std::f64::consts::TAU;
use std::sync::Arc;

use crate::gpu::{GpuImage, PaintScene, Scene};
use blitz_dom::BaseDocument;
use kurbo::{Affine, BezPath, Cap, Join, PathEl, Point, Rect, RoundedRect, RoundedRectRadii, Shape, Stroke, Vec2};
use peniko::{BlendMode, Color, Compose, Fill, Gradient, Mix, StyleRef};

use crate::render::error::{Error, Result};
use crate::render::gpu;

const MAX_COMMANDS: usize = 20_000;
pub(crate) const MAX_AREA: u64 = 1 << 28;
const MAX_SIDE: u32 = 32_767;
const MAX_EFFECT_AREA: f64 = 4096.0 * 4096.0;

pub(crate) fn limits() -> (u32, u64) {
    let side = crate::gpu::Gpu::shared().map_or(MAX_SIDE, |gpu| gpu.shared.device.limits().max_texture_dimension_2d.min(MAX_SIDE));
    (side, MAX_AREA)
}

pub(crate) fn fits(width: u32, height: u32) -> bool {
    let (side, area) = limits();
    width > 0 && height > 0 && width <= side && height <= side && u64::from(width) * u64::from(height) <= area
}

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().ok()?.write_image_data(rgba).ok()?;
    Some(out)
}

fn crop(rgba: &[u8], width: u32, [x, y, w, h]: [u32; 4]) -> Vec<u8> {
    let (stride, row) = (width as usize * 4, w as usize * 4);
    (y as usize..(y + h) as usize).flat_map(|line| &rgba[line * stride + x as usize * 4..][..row]).copied().collect()
}

pub(crate) enum Source {
    Raster(peniko::ImageBrush),
    Picture(Arc<Scene>, (f64, f64)),
}

impl Source {
    fn size(&self) -> (f64, f64) {
        match self {
            Source::Raster(b) => (b.image.width.into(), b.image.height.into()),
            Source::Picture(_, size) => *size,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Font {
    family: String,
    size: f32,
    weight: f32,
    italic: bool,
    spacing: Spacing,
}

impl Default for Font {
    fn default() -> Self {
        Self { family: "sans-serif".into(), size: 10.0, weight: 400.0, italic: false, spacing: Spacing::default() }
    }
}

/// `letterSpacing` / `wordSpacing` of a 2D context: lengths in canvas pixels, or in `em` of the
/// current font when the matching bit of `units` is set (1 = letter, 2 = word).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Spacing {
    letter: f32,
    word: f32,
    letter_em: bool,
    word_em: bool,
}

impl Spacing {
    pub(crate) fn new(letter: f64, word: f64, units: u8) -> Self {
        let clean = |v: f64| if v.is_finite() { v as f32 } else { 0.0 };
        Self { letter: clean(letter), word: clean(word), letter_em: units & 1 != 0, word_em: units & 2 != 0 }
    }

    fn px(self, size: f32) -> (f32, f32) {
        let resolve = |v: f32, em: bool| if em { v * size } else { v };
        (resolve(self.letter, self.letter_em), resolve(self.word, self.word_em))
    }
}

impl Font {
    pub(crate) fn parse(css: &str) -> Option<Self> {
        let mut font = Self::default();
        let mut rest = css.trim();
        loop {
            let (word, tail) = rest.split_once(char::is_whitespace)?;
            match word {
                "normal" | "small-caps" | "condensed" | "expanded" | "semi-condensed" | "semi-expanded" => {}
                "italic" | "oblique" => font.italic = true,
                "bold" | "bolder" => font.weight = 700.0,
                "lighter" => font.weight = 300.0,
                w if w.parse::<f32>().is_ok_and(|n| (1.0..=1000.0).contains(&n)) && !w.contains('.') => font.weight = w.parse().ok()?,
                size => {
                    let size = size.split('/').next()?;
                    let (number, unit) = size.split_at(size.find(|c: char| c.is_ascii_alphabetic() || c == '%')?);
                    let n: f32 = number.parse().ok()?;
                    font.size = match unit {
                        "px" => n,
                        "pt" => n * 4.0 / 3.0,
                        "em" | "rem" => n * 16.0,
                        _ => return None,
                    };
                    font.family = tail.trim().to_owned();
                    return (!font.family.is_empty() && font.size > 0.0).then_some(font);
                }
            }
            rest = tail.trim_start();
        }
    }
}

#[derive(Clone)]
struct State {
    transform: Affine,
    fill: Style,
    stroke_paint: Style,
    shadow: Shadow,
    stroke: Stroke,
    alpha: f32,
    blend: BlendMode,
    font: Font,
    align: u8,
    baseline: u8,
    rtl: bool,
    clips: Vec<(BezPath, Fill)>,
    own: usize,
    filter: Option<Arc<crate::gpu::Filter>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            transform: Affine::IDENTITY,
            fill: Style::solid(Color::BLACK),
            stroke_paint: Style::solid(Color::BLACK),
            shadow: Shadow { color: Color::TRANSPARENT, blur: 0.0, offset: Vec2::ZERO },
            stroke: Stroke::new(1.0),
            alpha: 1.0,
            blend: BlendMode::default(),
            font: Font::default(),
            align: 0,
            baseline: 0,
            rtl: false,
            clips: Vec::new(),
            own: 0,
            filter: None,
        }
    }
}

pub(crate) struct Canvas {
    size: (f64, f64),
    scene: Arc<Scene>,
    closed: usize,
    state: State,
    stack: Vec<State>,
    path: BezPath,
    bitmap: std::cell::OnceCell<Option<Arc<GpuImage>>>,
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let bytes = self.buf.get(self.pos..self.pos + n).ok_or_else(|| Error::new("canvas: truncated"))?;
        self.pos += n;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f(&mut self) -> Result<f64> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()).into())
    }

    fn fs<const N: usize>(&mut self) -> Result<[f64; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f()?;
        }
        Ok(out)
    }

    fn str(&mut self) -> Result<&'a str> {
        let len = self.u32()? as usize;
        std::str::from_utf8(self.take(len)?).map_err(|e| Error::new(format!("canvas: {e}")))
    }

    fn color(&mut self) -> Result<Color> {
        let [r, g, b, a] = self.fs::<4>()?;
        Ok(Color::new([r as f32, g as f32, b as f32, a as f32]))
    }

    fn paint(&mut self) -> Result<Painted> {
        let kind = self.u8()?;
        let gradient = match kind {
            0 => return Ok(Painted::Paint(crate::gpu::Paint::from(self.color()?))),
            3 => {
                let (kind, id, repeat) = (self.u8()?, self.u32()?, self.u8()?);
                let [a, b, c, d, e, f] = self.fs::<6>()?;
                return Ok(Painted::Pattern { kind, id, repeat, matrix: Affine::new([a, b, c, d, e, f]) });
            }
            1 => {
                let [x0, y0, x1, y1] = self.fs::<4>()?;
                Gradient::new_linear((x0, y0), (x1, y1))
            }
            2 => {
                let [x0, y0, r0, x1, y1, r1] = self.fs::<6>()?;
                Gradient::new_two_point_radial((x0, y0), r0 as f32, (x1, y1), r1 as f32)
            }
            k => return Err(Error::new(format!("canvas: unknown paint {k}"))),
        };
        let n = self.u32()? as usize;
        let mut stops = Vec::with_capacity(n);
        for _ in 0..n {
            let offset = self.f()? as f32;
            stops.push((offset, self.color()?));
        }
        Ok(Painted::Paint(crate::gpu::Paint::Gradient(gradient.with_stops(stops.as_slice()))))
    }
}

enum Painted {
    Paint(crate::gpu::Paint),
    Pattern { kind: u8, id: u32, repeat: u8, matrix: Affine },
}

#[derive(Clone)]
struct Style {
    paint: crate::gpu::Paint,
    pattern: Option<Pattern>,
}

impl Style {
    fn solid(color: Color) -> Self {
        Self { paint: crate::gpu::Paint::from(color), pattern: None }
    }

    fn brush(&self, user: Affine) -> Option<Affine> {
        match (&self.paint, &self.pattern) {
            (_, Some(p)) => Some(user * p.matrix),
            (crate::gpu::Paint::Gradient(_), None) => Some(user),
            _ => None,
        }
    }

    fn clip(&self, user: Affine) -> Option<BezPath> {
        let p = self.pattern.as_ref()?;
        const FAR: f64 = 1e7;
        let (w, h) = p.size;
        let rect = match p.repeat {
            1 => Rect::new(-FAR, 0.0, FAR, h),
            2 => Rect::new(0.0, -FAR, w, FAR),
            3 => Rect::new(0.0, 0.0, w, h),
            _ => return None,
        };
        let mut path = rect.to_path(Scene::TOLERANCE);
        path.apply_affine(user * p.matrix);
        Some(path)
    }
}

#[derive(Clone)]
struct Pattern {
    size: (f64, f64),
    matrix: Affine,
    repeat: u8,
}

#[derive(Clone, Copy)]
struct Shadow {
    color: Color,
    blur: f64,
    offset: Vec2,
}

impl Shadow {
    fn on(&self) -> bool {
        self.color.components[3] > 0.0 && (self.blur > 0.0 || self.offset != Vec2::ZERO)
    }
}

impl Canvas {
    pub(crate) fn new(size: (f64, f64)) -> Self {
        Self { size, scene: Arc::new(Scene::new()), closed: 0, state: State::default(), stack: Vec::new(), path: BezPath::new(), bitmap: Default::default() }
    }

    pub(crate) fn size(&self) -> (f64, f64) {
        self.size
    }

    fn max_commands() -> usize {
        static MAX: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        *MAX.get_or_init(|| match std::env::var("ACDE_CANVAS_MAX_COMMANDS").ok().and_then(|v| v.parse::<usize>().ok()) {
            Some(0) => usize::MAX,
            Some(n) => n,
            None => MAX_COMMANDS,
        })
    }

    fn scene(&mut self) -> &mut Scene {
        self.bitmap.take();
        Arc::make_mut(&mut self.scene)
    }

    pub(crate) fn picture(&self) -> Arc<Scene> {
        let open = self.state.clips.len();
        if self.closed == open {
            return self.scene.clone();
        }
        let mut commands = self.scene.commands[..self.scene.commands.len() - self.closed].to_vec();
        commands.extend((0..open).map(|_| crate::gpu::RenderCommand::PopLayer));
        Arc::new(Scene { tolerance: self.scene.tolerance, commands })
    }

    pub(crate) fn size_of(doc: &BaseDocument, node: blitz_dom::NodeId) -> (f64, f64) {
        let attr = |name: &str, default: f64| {
            doc.get_node(node).and_then(|n| n.attr(blitz_dom::LocalName::from(name))).and_then(|v| v.trim().parse::<u32>().ok()).map_or(default, f64::from)
        };
        (attr("width", 300.0), attr("height", 150.0))
    }

    pub(crate) fn data_url(&self) -> Option<String> {
        let (w, h) = self.pixel_size()?;
        let png = encode_png(w, h, &self.pixels([0, 0, w, h])?)?;
        Some(format!("data:image/png;base64,{}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png)))
    }

    fn pixel_size(&self) -> Option<(u32, u32)> {
        let (w, h) = (self.size.0 as u32, self.size.1 as u32);
        fits(w, h).then_some((w, h))
    }

    fn pixels(&self, rect: [u32; 4]) -> Option<Vec<u8>> {
        if let Some(image) = self.bitmap() {
            return gpu::read(&image, rect);
        }
        let size = self.pixel_size()?;
        let all = gpu::cpu_raster(Arc::unwrap_or_clone(self.picture()), size)?;
        Some(crop(&all, size.0, rect))
    }

    fn render(&self) -> Option<Arc<GpuImage>> {
        let (w, h) = self.pixel_size()?;
        gpu::draw(Arc::unwrap_or_clone(self.picture()), (w, h), Affine::IDENTITY)
    }

    pub(crate) fn bitmap(&self) -> Option<Arc<GpuImage>> {
        self.bitmap.get_or_init(|| self.render()).clone()
    }

    pub(crate) fn source(&self) -> Source {
        match self.bitmap() {
            Some(image) => Source::Raster(image.brush()),
            None => Source::Picture(self.picture(), self.size),
        }
    }

    pub(crate) fn run(&mut self, doc: &mut BaseDocument, ops: &[u8], source: &dyn Fn(&BaseDocument, u8, u32) -> Option<Source>) -> Result<()> {
        if self.pixel_size().is_none() {
            self.drop_commands();
            return Ok(());
        }
        if self.closed > 0 {
            let closed = std::mem::take(&mut self.closed);
            let commands = &mut self.scene().commands;
            commands.truncate(commands.len() - closed);
        }
        let result = self.record(doc, ops, source);
        let open = self.state.clips.len();
        if open > 0 {
            self.scene().commands.extend((0..open).map(|_| crate::gpu::RenderCommand::PopLayer));
        }
        self.closed = open;
        result
    }

    fn record(&mut self, doc: &mut BaseDocument, ops: &[u8], source: &dyn Fn(&BaseDocument, u8, u32) -> Option<Source>) -> Result<()> {
        let mut r = Reader { buf: ops, pos: 0 };
        while r.pos < ops.len() {
            match r.u8()? {
                1 => self.save(),
                2 => self.restore(),
                3 => {
                    let [a, b, c, d, e, f] = r.fs::<6>()?;
                    self.state.transform = Affine::new([a, b, c, d, e, f]);
                }
                4 => {
                    if let Some(style) = Self::style(r.paint()?, doc, source) {
                        self.state.fill = style;
                    }
                }
                5 => {
                    if let Some(style) = Self::style(r.paint()?, doc, source) {
                        self.state.stroke_paint = style;
                    }
                }
                13 => {
                    let color = r.color()?;
                    let [blur, x, y] = r.fs::<3>()?;
                    if [blur, x, y].iter().all(|v| v.is_finite()) && blur >= 0.0 {
                        self.state.shadow = Shadow { color, blur, offset: Vec2::new(x, y) };
                    }
                }
                6 => {
                    let w = r.f()?;
                    if w.is_finite() && w > 0.0 {
                        self.state.stroke.width = w;
                    }
                }
                7 => {
                    let (cap, join, miter) = (r.u8()?, r.u8()?, r.f()?);
                    let cap = [Cap::Butt, Cap::Round, Cap::Square].get(cap as usize).copied().unwrap_or(Cap::Butt);
                    let join = [Join::Miter, Join::Round, Join::Bevel].get(join as usize).copied().unwrap_or(Join::Miter);
                    let s = &mut self.state.stroke;
                    (s.start_cap, s.end_cap, s.join) = (cap, cap, join);
                    if miter.is_finite() && miter > 0.0 {
                        s.miter_limit = miter;
                    }
                }
                8 => {
                    let n = r.u32()? as usize;
                    let mut dashes = Vec::with_capacity(n);
                    for _ in 0..n {
                        dashes.push(r.f()?);
                    }
                    let offset = r.f()?;
                    let s = std::mem::replace(&mut self.state.stroke, Stroke::new(1.0));
                    self.state.stroke = s.with_dashes(offset, dashes);
                }
                9 => {
                    let a = r.f()?;
                    if (0.0..=1.0).contains(&a) {
                        self.state.alpha = a as f32;
                    }
                }
                10 => self.state.blend = Self::blend(r.u8()?),
                11 => {
                    if let Some(font) = Font::parse(r.str()?) {
                        self.state.font = Font { spacing: self.state.font.spacing, ..font };
                    }
                }
                15 => self.state.font.spacing = Spacing::new(r.f()?, r.f()?, r.u8()?),
                14 => self.state.filter = Self::filter(&mut r)?,
                12 => {
                    self.state.align = r.u8()?;
                    self.state.baseline = r.u8()?;
                    self.state.rtl = r.u8()? == 2;
                }
                20 => self.path = BezPath::new(),
                21 => {
                    let [x, y] = r.fs::<2>()?;
                    self.path.move_to(self.state.transform * Point::new(x, y));
                }
                22 => {
                    let [x, y] = r.fs::<2>()?;
                    self.line_to(self.state.transform * Point::new(x, y));
                }
                23 => {
                    let [cx, cy, x, y] = r.fs::<4>()?;
                    let t = self.state.transform;
                    self.ensure_start(t * Point::new(cx, cy));
                    self.path.quad_to(t * Point::new(cx, cy), t * Point::new(x, y));
                }
                24 => {
                    let [c1x, c1y, c2x, c2y, x, y] = r.fs::<6>()?;
                    let t = self.state.transform;
                    self.ensure_start(t * Point::new(c1x, c1y));
                    self.path.curve_to(t * Point::new(c1x, c1y), t * Point::new(c2x, c2y), t * Point::new(x, y));
                }
                25 => {
                    let [x, y, radius, a0, a1] = r.fs::<5>()?;
                    let ccw = r.u8()? != 0;
                    self.ellipse(Point::new(x, y), (radius, radius), 0.0, a0, a1, ccw);
                }
                26 => {
                    let [x1, y1, x2, y2, radius] = r.fs::<5>()?;
                    self.arc_to(Point::new(x1, y1), Point::new(x2, y2), radius);
                }
                27 => {
                    let [x, y, rx, ry, rotation, a0, a1] = r.fs::<7>()?;
                    let ccw = r.u8()? != 0;
                    self.ellipse(Point::new(x, y), (rx, ry), rotation, a0, a1, ccw);
                }
                28 => {
                    let [x, y, w, h] = r.fs::<4>()?;
                    self.append(Rect::new(x, y, x + w, y + h).path_elements(Scene::TOLERANCE));
                    self.path.move_to(self.state.transform * Point::new(x, y));
                }
                29 => {
                    let [x, y, w, h, tl, tr, br, bl] = r.fs::<8>()?;
                    let rect = Rect::new(x, y, x + w, y + h).abs();
                    let shape = RoundedRect::from_rect(rect, RoundedRectRadii::new(tl, tr, br, bl));
                    self.append(shape.path_elements(Scene::TOLERANCE));
                    self.path.move_to(self.state.transform * Point::new(x, y));
                }
                30 => self.path.close_path(),
                31 => {
                    let d = r.str()?;
                    let [a, b, c, dd, e, f] = r.fs::<6>()?;
                    let mut path = Self::svg_path(d);
                    path.apply_affine(Affine::new([a, b, c, dd, e, f]));
                    self.append(path.elements().iter().copied());
                }
                40 => {
                    let rule = Self::rule(r.u8()?);
                    let path = std::mem::take(&mut self.path);
                    self.fill(&path, rule);
                    self.path = path;
                }
                41 => {
                    let path = std::mem::take(&mut self.path);
                    self.stroke(&path);
                    self.path = path;
                }
                42 => {
                    let rule = Self::rule(r.u8()?);
                    let path = self.path.clone();
                    self.scene().push_clip_layer(rule, Affine::IDENTITY, &path);
                    self.state.clips.push((path, rule));
                    self.state.own += 1;
                }
                43 => {
                    let kind = r.u8()?;
                    let [x, y, w, h] = r.fs::<4>()?;
                    let mut rect = Rect::new(x, y, x + w, y + h).to_path(Scene::TOLERANCE);
                    rect.apply_affine(self.state.transform);
                    match kind {
                        0 => {
                            if self.opaque_fill() && self.covers_canvas(&rect) {
                                self.drop_commands();
                            }
                            self.fill(&rect, Fill::NonZero)
                        }
                        1 => self.stroke(&rect),
                        _ => self.clear(&rect),
                    }
                }
                44 => {
                    let kind = r.u8()?;
                    let text = r.str()?;
                    let [x, y, max] = r.fs::<3>()?;
                    self.text(doc, text, Point::new(x, y), max, kind == 1);
                }
                45 => {
                    let (kind, id) = (r.u8()?, r.u32()?);
                    let [sx, sy, sw, sh, dx, dy, dw, dh] = r.fs::<8>()?;
                    if let Some(src) = source(doc, kind, id) {
                        self.draw_image(&src, Rect::new(sx, sy, sw, sh), Rect::new(dx, dy, dw, dh));
                    }
                }
                46 => {
                    let [dx, dy, x, y, w, h] = r.fs::<6>()?;
                    let (width, height) = (r.u32()?, r.u32()?);
                    let data = r.take(width as usize * height as usize * 4)?;
                    self.put_image_data(data, (width, height), Point::new(dx, dy), Rect::new(x, y, x + w, y + h));
                }
                op => return Err(Error::new(format!("canvas: unknown command {op}"))),
            }
        }
        if self.scene.commands.len() > Self::max_commands() {
            self.flatten();
        }
        Ok(())
    }

    fn svg_path(d: &str) -> BezPath {
        if let Ok(path) = BezPath::from_svg(d) {
            return path;
        }
        let cuts: Vec<usize> = d.char_indices().filter(|&(i, c)| i > 0 && c.is_ascii_alphabetic() && !matches!(c, 'e' | 'E')).map(|(i, _)| i).collect();
        let (mut lo, mut hi, mut best) = (0, cuts.len(), BezPath::new());
        while lo < hi {
            let mid = (lo + hi) / 2;
            match BezPath::from_svg(&d[..cuts[mid]]) {
                Ok(path) => (best, lo) = (path, mid + 1),
                Err(_) => hi = mid,
            }
        }
        best
    }

    fn rule(rule: u8) -> Fill {
        match rule {
            1 => Fill::EvenOdd,
            _ => Fill::NonZero,
        }
    }

    fn blend(op: u8) -> BlendMode {
        const COMPOSE: [Compose; 11] = [
            Compose::SrcOver,
            Compose::SrcIn,
            Compose::SrcOut,
            Compose::SrcAtop,
            Compose::DestOver,
            Compose::DestIn,
            Compose::DestOut,
            Compose::DestAtop,
            Compose::Plus,
            Compose::Copy,
            Compose::Xor,
        ];
        const MIX: [Mix; 15] = [
            Mix::Multiply,
            Mix::Screen,
            Mix::Overlay,
            Mix::Darken,
            Mix::Lighten,
            Mix::ColorDodge,
            Mix::ColorBurn,
            Mix::HardLight,
            Mix::SoftLight,
            Mix::Difference,
            Mix::Exclusion,
            Mix::Hue,
            Mix::Saturation,
            Mix::Color,
            Mix::Luminosity,
        ];
        match op as usize {
            i if i < COMPOSE.len() => BlendMode::new(Mix::Normal, COMPOSE[i]),
            i => MIX.get(i - COMPOSE.len()).map_or(BlendMode::default(), |&m| BlendMode::new(m, Compose::SrcOver)),
        }
    }

    fn save(&mut self) {
        let mut inner = self.state.clone();
        inner.own = 0;
        self.stack.push(std::mem::replace(&mut self.state, inner));
    }

    fn restore(&mut self) {
        let Some(outer) = self.stack.pop() else { return };
        for _ in 0..self.state.own {
            self.scene().pop_layer();
        }
        self.state = outer;
    }

    fn ensure_start(&mut self, p: Point) {
        if self.path.elements().is_empty() {
            self.path.move_to(p);
        }
    }

    fn line_to(&mut self, p: Point) {
        match self.path.elements().is_empty() {
            true => self.path.move_to(p),
            false => self.path.line_to(p),
        }
    }

    fn append(&mut self, els: impl Iterator<Item = PathEl>) {
        let t = self.state.transform;
        for el in els {
            match el {
                PathEl::MoveTo(p) => self.path.move_to(t * p),
                PathEl::LineTo(p) => self.path.line_to(t * p),
                PathEl::QuadTo(a, b) => self.path.quad_to(t * a, t * b),
                PathEl::CurveTo(a, b, c) => self.path.curve_to(t * a, t * b, t * c),
                PathEl::ClosePath => self.path.close_path(),
            }
        }
    }

    fn ellipse(&mut self, center: Point, radii: (f64, f64), rotation: f64, a0: f64, a1: f64, ccw: bool) {
        if !(radii.0 >= 0.0 && radii.1 >= 0.0) || ![center.x, center.y, rotation, a0, a1].iter().all(|v| v.is_finite()) {
            return;
        }
        let sweep = match ccw {
            false if a1 - a0 >= TAU => TAU,
            false => (a1 - a0).rem_euclid(TAU),
            true if a0 - a1 >= TAU => -TAU,
            true => -(a0 - a1).rem_euclid(TAU),
        };
        let arc = kurbo::Arc::new(center, Vec2::new(radii.0, radii.1), a0, sweep, rotation);
        let start = self.state.transform
            * arc.path_elements(Scene::TOLERANCE).next().map_or(center, |el| match el {
                PathEl::MoveTo(p) => p,
                _ => center,
            });
        self.line_to(start);
        self.append(arc.append_iter(Scene::TOLERANCE));
    }

    fn arc_to(&mut self, p1: Point, p2: Point, radius: f64) {
        let t = self.state.transform;
        match self.path.elements().last().copied() {
            Some(PathEl::MoveTo(last) | PathEl::LineTo(last) | PathEl::QuadTo(_, last) | PathEl::CurveTo(_, _, last)) => {
                self.arc_to_from(t.inverse() * last, p1, p2, radius)
            }
            Some(PathEl::ClosePath) | None => self.path.move_to(t * p1),
        }
    }

    fn arc_to_from(&mut self, p0: Point, p1: Point, p2: Point, radius: f64) {
        let t = self.state.transform;
        let (v1, v2) = (p0 - p1, p2 - p1);
        let (l1, l2) = (v1.hypot(), v2.hypot());
        let angle = (v1.dot(v2) / (l1 * l2)).clamp(-1.0, 1.0).acos();
        if radius <= 0.0 || l1 == 0.0 || l2 == 0.0 || angle.abs() < 1e-9 || (angle - std::f64::consts::PI).abs() < 1e-9 {
            self.line_to(t * p1);
            return;
        }
        let d = radius / (angle / 2.0).tan();
        let (u1, u2) = (v1 / l1, v2 / l2);
        let (t1, t2) = (p1 + u1 * d, p1 + u2 * d);
        let bisect = (u1 + u2).normalize();
        let center = p1 + bisect * (radius / (angle / 2.0).sin());
        let (a0, a1) = ((t1 - center).atan2(), (t2 - center).atan2());
        let ccw = v1.cross(v2) > 0.0;
        self.line_to(t * t1);
        self.ellipse(center, (radius, radius), 0.0, a0, a1, ccw);
    }

    fn filter(r: &mut Reader) -> Result<Option<Arc<crate::gpu::Filter>>> {
        use crate::gpu::{ColorMatrix, DropShadow, FilterEffect, GaussianBlur};
        let n = r.u32()?;
        let mut effects = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let kind = r.u8()?;
            let a = r.f()? as f32;
            let effect: FilterEffect = match kind {
                0 => GaussianBlur { std_deviation: a }.into(),
                1 => ColorMatrix::brightness(a).into(),
                2 => ColorMatrix::contrast(a).into(),
                3 => ColorMatrix::grayscale(a).into(),
                4 => ColorMatrix::hue_rotate(a).into(),
                5 => ColorMatrix::invert(a).into(),
                6 => ColorMatrix::opacity(a).into(),
                7 => ColorMatrix::saturate(a).into(),
                8 => ColorMatrix::sepia(a).into(),
                _ => {
                    let [dy, blur] = r.fs::<2>()?;
                    let color = r.color()?;
                    DropShadow { dx: a, dy: dy as f32, std_deviation: blur as f32 / 2.0, color }.into()
                }
            };
            effects.push(effect);
        }
        Ok((!effects.is_empty()).then(|| Arc::new(effects.into_iter().collect())))
    }

    fn layered(&mut self, bounds: Rect, draw: impl Fn(&mut Scene)) {
        let plain = self.state.alpha >= 1.0 && self.state.blend == BlendMode::default();
        if !plain {
            let bounds = Rect::new(0.0, 0.0, self.size.0, self.size.1);
            let (blend, alpha) = (self.state.blend, self.state.alpha);
            self.scene().push_layer(Fill::NonZero, blend, alpha, Affine::IDENTITY, &bounds, None, None);
        }
        if let Some((tile, place)) = self.shadow_tile(bounds, &draw) {
            self.scene().draw_image(tile.as_ref(), place);
        }
        match self.filtered(bounds, &draw) {
            Some((image, place)) => self.scene().draw_image(image.as_ref(), place),
            None if self.state.filter.is_some() => {}
            None => draw(self.scene()),
        }
        if !plain {
            self.scene().pop_layer();
        }
    }

    fn filtered(&self, bounds: Rect, draw: &dyn Fn(&mut Scene)) -> Option<(peniko::ImageBrush, Affine)> {
        let filter = self.state.filter.as_ref()?;
        let reach = filter.expansion();
        let canvas = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        let grown = Rect::new(bounds.x0 + reach.x0, bounds.y0 + reach.y0, bounds.x1 + reach.x1, bounds.y1 + reach.y1);
        let area = if bounds.is_finite() { grown.intersect(canvas).expand() } else { canvas };
        if area.width() < 1.0 || area.height() < 1.0 || area.width() * area.height() > MAX_EFFECT_AREA {
            return None;
        }
        let mut shape = Scene::new();
        draw(&mut shape);
        let size = (area.width() as u32, area.height() as u32);
        let image = gpu::filtered(shape, size, Affine::translate(-area.origin().to_vec2()), filter)?;
        Some((image.brush(), Affine::translate(area.origin().to_vec2())))
    }

    fn shadow_tile(&self, bounds: Rect, draw: &dyn Fn(&mut Scene)) -> Option<(peniko::ImageBrush, Affine)> {
        let shadow = self.state.shadow;
        if !shadow.on() || !bounds.is_finite() {
            return None;
        }
        let sigma = shadow.blur / 2.0;
        let pad = (3.0 * sigma).ceil();
        let canvas = Rect::new(-pad, -pad, self.size.0 + pad, self.size.1 + pad);
        let area = (bounds + shadow.offset).inflate(pad, pad).intersect(canvas).expand();
        if area.width() < 1.0 || area.height() < 1.0 || area.width() * area.height() > MAX_EFFECT_AREA {
            return None;
        }
        let k = (sigma / 3.0).floor().clamp(1.0, 8.0);
        let (w, h) = ((area.width() / k).ceil() as u32, (area.height() / k).ceil() as u32);
        let mut shape = Scene::new();
        draw(&mut shape);
        let place = Affine::scale(1.0 / k) * Affine::translate(shadow.offset - area.origin().to_vec2());
        let image = gpu::shadow(shape, (w, h), place, sigma / k, shadow.color.components)?;
        let mut tile = image.brush();
        tile.sampler.quality = peniko::ImageQuality::High;
        Some((tile, Affine::translate(area.origin().to_vec2()) * Affine::scale(k)))
    }

    fn fill(&mut self, path: &BezPath, rule: Fill) {
        let style = self.state.fill.clone();
        let t = self.state.transform;
        let (brush, clip) = (style.brush(t), style.clip(t));
        self.layered(path.bounding_box(), |s| Self::clipped(s, clip.as_ref(), |s| s.fill(rule, Affine::IDENTITY, &style.paint, brush, path)));
    }

    fn stroke(&mut self, path: &BezPath) {
        let t = self.state.transform;
        if t.determinant().abs() < 1e-12 {
            return;
        }
        let mut user = path.clone();
        user.apply_affine(t.inverse());
        let (style, stroke) = (self.state.stroke_paint.clone(), self.state.stroke.clone());
        let (brush, clip) = (style.brush(Affine::IDENTITY), style.clip(t));
        let reach = stroke.width * t.as_coeffs().iter().take(4).map(|c| c.abs()).fold(0.0, f64::max) * stroke.miter_limit.max(1.0);
        self.layered(path.bounding_box().inflate(reach, reach), |s| Self::clipped(s, clip.as_ref(), |s| s.stroke(&stroke, t, &style.paint, brush, &user)));
    }

    fn clipped(s: &mut Scene, clip: Option<&BezPath>, draw: impl FnOnce(&mut Scene)) {
        if let Some(clip) = clip {
            s.push_clip_layer(Fill::NonZero, Affine::IDENTITY, clip);
        }
        draw(s);
        if clip.is_some() {
            s.pop_layer();
        }
    }

    fn style(painted: Painted, doc: &BaseDocument, source: &dyn Fn(&BaseDocument, u8, u32) -> Option<Source>) -> Option<Style> {
        let (kind, id, repeat, matrix) = match painted {
            Painted::Paint(paint) => return Some(Style { paint, pattern: None }),
            Painted::Pattern { kind, id, repeat, matrix } => (kind, id, repeat, matrix),
        };
        let Source::Raster(mut brush) = source(doc, kind, id)? else { return None };
        let size = (f64::from(brush.image.width), f64::from(brush.image.height));
        let extend = |on: bool| if on { peniko::Extend::Repeat } else { peniko::Extend::Pad };
        brush.sampler.x_extend = extend(matches!(repeat, 0 | 1));
        brush.sampler.y_extend = extend(matches!(repeat, 0 | 2));
        Some(Style {
            paint: crate::gpu::Paint::Image(brush),
            pattern: Some(Pattern { size, matrix: if matrix.is_finite() { matrix } else { Default::default() }, repeat }),
        })
    }

    fn put_image_data(&mut self, data: &[u8], (w, h): (u32, u32), at: Point, dirty: Rect) {
        if ![at.x, at.y, dirty.x0, dirty.y0, dirty.x1, dirty.y1].iter().all(|v| v.is_finite()) || data.len() < (w * h * 4) as usize {
            return;
        }
        let Some(image) = self.render() else { return };
        let (cw, ch) = (i64::from(image.texture.width()), i64::from(image.texture.height()));
        let dirty = dirty.abs().intersect(Rect::new(0.0, 0.0, f64::from(w), f64::from(h))).round();
        let (dx, dy) = (at.x.trunc() as i64, at.y.trunc() as i64);
        let (x0, y0) = ((dirty.x0 as i64).max(-dx), (dirty.y0 as i64).max(-dy));
        let (x1, y1) = ((dirty.x1 as i64).min(cw - dx), (dirty.y1 as i64).min(ch - dy));
        if x1 > x0 && y1 > y0 {
            gpu::write(&image, data, (w, h), [x0 as u32, y0 as u32], [(x0 + dx) as u32, (y0 + dy) as u32, (x1 - x0) as u32, (y1 - y0) as u32]);
        }
        self.restart(image);
    }

    pub(crate) fn image_data(&self, area: Rect) -> Vec<u8> {
        let area = area.abs().round();
        let (w, h) = (area.width() as usize, area.height() as usize);
        let mut out = vec![0u8; w * h * 4];
        let Some((cw, ch)) = self.pixel_size() else { return out };
        let (cw, ch) = (i64::from(cw), i64::from(ch));
        let (x0, y0) = (area.x0 as i64, area.y0 as i64);
        let (from_x, to_x) = (x0.clamp(0, cw), (x0 + w as i64).clamp(0, cw));
        let (from_y, to_y) = (y0.clamp(0, ch), (y0 + h as i64).clamp(0, ch));
        if from_x >= to_x || from_y >= to_y {
            return out;
        }
        let Some(data) = self.pixels([from_x as u32, from_y as u32, (to_x - from_x) as u32, (to_y - from_y) as u32]) else { return out };
        let row = ((to_x - from_x) * 4) as usize;
        for (i, line) in data.chunks_exact(row).enumerate() {
            let to = (((from_y - y0) as usize + i) * w + (from_x - x0) as usize) * 4;
            out[to..to + row].copy_from_slice(line);
        }
        out
    }

    fn covers_canvas(&self, rect: &BezPath) -> bool {
        let canvas = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        let t = self.state.transform.as_coeffs();
        self.state.clips.is_empty() && rect.bounding_box().contains_rect(canvas) && rect.elements().len() <= 6 && t[1].abs() < 1e-9 && t[2].abs() < 1e-9
    }

    fn drop_commands(&mut self) {
        self.scene = Arc::new(Scene { tolerance: self.scene.tolerance, commands: Vec::new() });
        self.bitmap.take();
    }

    fn opaque_fill(&self) -> bool {
        let solid = matches!(&self.state.fill.paint, crate::gpu::Paint::Solid(c) if c.components[3] >= 1.0) && self.state.fill.pattern.is_none();
        solid
            && self.state.alpha >= 1.0
            && self.state.blend == BlendMode::default()
            && self.state.filter.is_none()
            && self.state.shadow.color.components[3] <= 0.0
    }

    fn clear(&mut self, rect: &BezPath) {
        if self.covers_canvas(rect) {
            self.drop_commands();
            return;
        }
        let scene = self.scene();
        scene.push_layer(Fill::NonZero, BlendMode::new(Mix::Normal, Compose::DestOut), 1.0, Affine::IDENTITY, rect, None, None);
        scene.fill(Fill::NonZero, Affine::IDENTITY, Color::BLACK, None, rect);
        scene.pop_layer();
    }

    fn text(&mut self, doc: &mut BaseDocument, text: &str, at: Point, max: f64, stroke: bool) {
        let font = self.state.font.clone();
        let (letter, word) = font.spacing.px(font.size);
        let layout = doc.layout_canvas_text(text, &font.family, font.size, font.weight, font.italic, letter, word);
        let width = f64::from(layout.width());
        let Some(line) = layout.lines().next() else { return };
        let (ascent, descent) = Self::extents(&line);
        let baseline = f64::from(line.metrics().baseline);
        let (left, right) = (0.0, -width);
        let dx = match (self.state.align, self.state.rtl) {
            (0, false) | (2, _) | (1, true) => left,
            (4, _) => -width / 2.0,
            _ => right,
        };
        let dy = match self.state.baseline {
            1 => ascent,
            2 => ascent * 0.8,
            3 => (ascent - descent) / 2.0,
            4 | 5 => -descent,
            _ => 0.0,
        };
        let squeeze = if max.is_finite() && max > 0.0 && width > max { max / width } else { 1.0 };
        let transform =
            self.state.transform * Affine::translate((at.x, at.y)) * Affine::scale_non_uniform(squeeze, 1.0) * Affine::translate((dx, dy - baseline));
        let paint = if stroke { self.state.stroke_paint.clone() } else { self.state.fill.clone() };
        let (brush, clip) = (paint.brush(Affine::IDENTITY), paint.clip(self.state.transform));
        let paint = paint.paint;
        let style = self.state.stroke.clone();
        let glyph_style = if stroke { StyleRef::Stroke(&style) } else { StyleRef::Fill(Fill::NonZero) };
        let height = f64::from(line.metrics().line_height);
        let bounds = transform.transform_rect_bbox(Rect::new(0.0, 0.0, width, height)).inflate(style.width, style.width);
        self.layered(bounds, |s| {
            Self::clipped(s, clip.as_ref(), |s| {
                for item in line.items() {
                    let blitz_dom::parley::PositionedLayoutItem::GlyphRun(run) = item else { continue };
                    let glyphs = run.positioned_glyphs().map(|g| crate::gpu::Glyph { id: g.id as _, x: g.x, y: g.y });
                    let r = run.run();
                    let coords: &[i16] = bytemuck::cast_slice(r.normalized_coords());
                    s.draw_glyphs(r.font(), r.font_size(), false, coords, Vec2::ZERO, glyph_style, &paint, 1.0, transform, brush, glyphs);
                }
            })
        });
    }

    fn draw_image(&mut self, src: &Source, from: Rect, to: Rect) {
        let (w, h) = src.size();
        let (sx, sy) = (from.x0, from.y0);
        let sw = if from.x1.is_nan() { w } else { from.x1 };
        let sh = if from.y1.is_nan() { h } else { from.y1 };
        let dw = if to.x1.is_nan() { sw } else { to.x1 };
        let dh = if to.y1.is_nan() { sh } else { to.y1 };
        if sw == 0.0 || sh == 0.0 || ![sx, sy, sw, sh, to.x0, to.y0, dw, dh].iter().all(|v| v.is_finite()) {
            return;
        }
        let t = self.state.transform;
        let dst = Rect::new(to.x0, to.y0, to.x0 + dw, to.y0 + dh);
        let place = t * Affine::translate((to.x0, to.y0)) * Affine::scale_non_uniform(dw / sw, dh / sh) * Affine::translate((-sx, -sy));
        self.layered(t.transform_rect_bbox(dst), |s| {
            s.push_clip_layer(Fill::NonZero, t, &dst);
            match src {
                Source::Raster(brush) => s.draw_image(brush.as_ref(), place),
                Source::Picture(picture, _) => s.append_scene((**picture).clone(), place),
            }
            s.pop_layer();
        });
    }

    fn flatten(&mut self) {
        if let Some(image) = self.render() {
            self.restart(image);
        }
    }

    fn restart(&mut self, image: Arc<GpuImage>) {
        let mut scene = Scene::new();
        scene.draw_image(image.brush().as_ref(), Affine::IDENTITY);
        for (path, rule) in &self.state.clips {
            scene.push_clip_layer(*rule, Affine::IDENTITY, path);
        }
        self.scene = Arc::new(scene);
        self.bitmap = if self.state.clips.is_empty() { std::cell::OnceCell::from(Some(image)) } else { Default::default() };
    }

    fn extents(line: &blitz_dom::parley::Line<blitz_dom::node::TextBrush>) -> (f64, f64) {
        line.items()
            .filter_map(|item| match item {
                blitz_dom::parley::PositionedLayoutItem::GlyphRun(run) => Some(*run.run().font_metrics()),
                _ => None,
            })
            .fold((0.0, 0.0), |(a, d), m| (f64::max(a, m.ascent.into()), f64::max(d, m.descent.into())))
    }

    fn ink(line: &blitz_dom::parley::Line<blitz_dom::node::TextBrush>) -> Option<Rect> {
        use skrifa::MetadataProvider;
        let baseline = line.metrics().baseline;
        let mut ink: Option<Rect> = None;
        for item in line.items() {
            let blitz_dom::parley::PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let r = run.run();
            let font = r.font();
            let Ok(face) = skrifa::FontRef::from_index(font.data.as_ref(), font.index) else { continue };
            let bits: &[i16] = bytemuck::cast_slice(r.normalized_coords());
            let coords: Vec<_> = bits.iter().map(|c| skrifa::instance::NormalizedCoord::from_bits(*c)).collect();
            let metrics = face.glyph_metrics(skrifa::instance::Size::new(r.font_size()), coords.as_slice());
            for g in run.positioned_glyphs() {
                let Some(b) = metrics.bounds(skrifa::GlyphId::new(g.id)) else { continue };
                if b.x_max <= b.x_min || b.y_max <= b.y_min {
                    continue;
                }
                let y = g.y - baseline;
                let rect = Rect::new(f64::from(g.x + b.x_min), f64::from(y - b.y_max), f64::from(g.x + b.x_max), f64::from(y - b.y_min));
                ink = Some(ink.map_or(rect, |i| i.union(rect)));
            }
        }
        ink
    }

    pub(crate) fn measure(doc: &mut BaseDocument, font: &str, text: &str, spacing: Spacing) -> crate::render::TextMetrics {
        let font = Font::parse(font).unwrap_or_default();
        let (letter, word) = spacing.px(font.size);
        let layout = doc.layout_canvas_text(text, &font.family, font.size, font.weight, font.italic, letter, word);
        let width = f64::from(layout.width());
        let line = layout.lines().next();
        let (ascent, descent) = line.as_ref().map_or((0.0, 0.0), Self::extents);
        let ink = line.as_ref().and_then(Self::ink).unwrap_or(Rect::ZERO);
        crate::render::TextMetrics {
            width,
            actual_bounding_box_left: -ink.x0,
            actual_bounding_box_right: ink.x1,
            actual_bounding_box_ascent: -ink.y0,
            actual_bounding_box_descent: ink.y1,
            font_bounding_box_ascent: ascent,
            font_bounding_box_descent: descent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::crop;

    #[test]
    fn crop_takes_the_rows_and_columns_of_the_rectangle() {
        let rgba: Vec<u8> = (0..3 * 2).flat_map(|i| [i, i, i, 255]).collect();
        assert_eq!(crop(&rgba, 3, [1, 0, 2, 2]), [1, 1, 1, 255, 2, 2, 2, 255, 4, 4, 4, 255, 5, 5, 5, 255]);
        assert_eq!(crop(&rgba, 3, [0, 0, 3, 2]), rgba);
    }
}
