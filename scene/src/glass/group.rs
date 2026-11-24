use super::Glass;
use super::cpu::{Look, Under, packed};
use crate::blur::Pixels;
use crate::corner;

pub const WGSL: &str = include_str!("group.wgsl");

pub const MOST_PIECES: usize = 16;
pub const PIECE_BYTES: usize = PIECE_FLOATS * 4;
pub const UNIFORM_BYTES: usize = MOST_PIECES * PIECE_BYTES;
const PIECE_FLOATS: usize = 36;
const LEAST_SPACING: f64 = 1e-4;
const GRADIENT_STEP: f64 = 0.5;
const MERGE_NORMAL: f64 = 4.0;
const LEAST_SLOPE: f64 = 0.05;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Piece {
    pub center: [f32; 2],
    pub half: [f32; 2],
    pub radius: f32,
    pub exponent: f32,
    pub glass: Glass,
    pub id: Option<super::Name>,
}

impl Piece {
    pub fn of_rect([x, y, w, h]: [f32; 4], radius: f32, exponent: f32, glass: Glass) -> Self {
        Self { center: [x + w / 2.0, y + h / 2.0], half: [w / 2.0, h / 2.0], radius, exponent, glass, id: None }
    }

    pub fn rect(&self) -> [f32; 4] {
        let [cx, cy] = self.center;
        let [hx, hy] = self.half;
        [cx - hx, cy - hy, 2.0 * hx, 2.0 * hy]
    }

    fn relative(&self, p: (f64, f64)) -> (f64, f64) {
        (p.0 - f64::from(self.center[0]), p.1 - f64::from(self.center[1]))
    }

    fn halves(&self) -> (f64, f64) {
        (f64::from(self.half[0]), f64::from(self.half[1]))
    }

    fn half_short(&self) -> f64 {
        f64::from(self.half[0].min(self.half[1]))
    }

    fn distance(&self, p: (f64, f64)) -> f64 {
        corner::distance(self.relative(p), self.halves(), f64::from(self.radius), f64::from(self.exponent))
    }

    fn toward(&self, p: (f64, f64)) -> (f64, f64) {
        super::cpu::toward(self.relative(p), self.halves())
    }

    fn floats(&self) -> [f32; PIECE_FLOATS] {
        let g = &self.glass;
        let (r, bleed, light) = (g.refraction, g.bleed, g.highlight);
        [
            self.center[0],
            self.center[1],
            self.half[0],
            self.half[1],
            self.radius,
            self.exponent,
            g.lens.index,
            g.outline.width,
            r.inner_amount,
            r.inner_height,
            r.outer_amount,
            r.outer_height,
            bleed.amount,
            bleed.radius,
            bleed.opacity,
            light.angle,
            light.amount,
            g.white_point,
            g.smoothing,
            light.width,
            g.tint[0],
            g.tint[1],
            g.tint[2],
            g.tint[3],
            g.lens.thickness,
            g.lens.dispersion,
            g.darken,
            g.lens.bezel,
            g.outline.color[0],
            g.outline.color[1],
            g.outline.color[2],
            g.outline.color[3],
            if g.outline.merged { 1.0 } else { 0.0 },
            g.film.amount,
            g.film.cell,
            g.film.uniform()[2],
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassGroup {
    pieces: [Piece; MOST_PIECES],
    count: usize,
    pub spacing: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    pub d: f64,
    pub line: f32,
    pub toward: (f64, f64),
    pub merging: f64,
    pub bevel: f64,
    pub glass: Glass,
}

impl GlassGroup {
    pub fn new(spacing: f32) -> Self {
        Self { pieces: [Piece::default(); MOST_PIECES], count: 0, spacing: if spacing.is_finite() { spacing.max(0.0) } else { 0.0 } }
    }

    pub fn push(&mut self, piece: Piece) -> bool {
        let Some(slot) = self.pieces.get_mut(self.count) else { return false };
        *slot = piece;
        self.count += 1;
        true
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces[..self.count]
    }

    pub fn pieces_mut(&mut self) -> &mut [Piece] {
        &mut self.pieces[..self.count]
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn glass(&self) -> Option<&Glass> {
        self.pieces().first().map(|p| &p.glass)
    }

    pub fn bounds(&self) -> Option<[f32; 4]> {
        let (x0, y0, x1, y1) = self.pieces().iter().map(Piece::rect).fold(None, |acc: Option<(f32, f32, f32, f32)>, [x, y, w, h]| {
            Some(acc.map_or((x, y, x + w, y + h), |(a, b, c, d)| (a.min(x), b.min(y), c.max(x + w), d.max(y + h))))
        })?;
        Some([x0, y0, x1 - x0, y1 - y0])
    }

    pub fn reach(&self) -> f64 {
        self.pieces().iter().map(|p| p.glass.reach()).fold(0.0, f64::max)
    }

    pub fn moved(&self, (dx, dy): (f32, f32)) -> Self {
        let mut moved = *self;
        moved.pieces[..self.count].iter_mut().for_each(|p| p.center = [p.center[0] + dx, p.center[1] + dy]);
        moved
    }

    pub fn to_uniform(&self) -> [u8; UNIFORM_BYTES] {
        let mut out = [0u8; UNIFORM_BYTES];
        let floats = self.pieces().iter().flat_map(Piece::floats);
        for (chunk, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(floats) {
            *chunk = v.to_le_bytes();
        }
        out
    }

    pub fn distance(&self, p: (f64, f64)) -> f64 {
        let k = self.k();
        let mut pieces = self.pieces().iter();
        let Some(first) = pieces.next() else { return f64::INFINITY };
        pieces.fold(first.distance(p), |d, piece| {
            let di = piece.distance(p);
            let h = weight(d, di, k);
            lerp(d, di, h) - k * h * (1.0 - h)
        })
    }

    pub fn field(&self, p: (f64, f64)) -> Option<Field> {
        let k = self.k();
        let (first, rest) = self.pieces().split_first()?;
        let d = first.distance(p);
        let mut field = Field { d, line: 0.0, toward: first.toward(p), merging: 0.0, bevel: first.half_short(), glass: first.glass };
        let mut pieces_line = first.glass.outline.coverage(d);
        for piece in rest {
            let di = piece.distance(p);
            let h = weight(field.d, di, k);
            if h >= 1.0 {
                field.toward = piece.toward(p);
            } else if h > 0.0 {
                field.toward = lerp2(unit(field.toward), unit(piece.toward(p)), h);
            }
            field.glass = field.glass.mixed(&piece.glass, h as f32);
            field.bevel = lerp(field.bevel, piece.half_short(), h);
            pieces_line = pieces_line.max(piece.glass.outline.coverage(di));
            field.d = lerp(field.d, di, h) - k * h * (1.0 - h);
            field.merging = field.merging.max(4.0 * h * (1.0 - h));
        }
        let mut line_d = field.d;
        if field.merging > 0.0 {
            let slope = self.slope(p);
            field.toward = lerp2(unit(field.toward), unit(slope), (MERGE_NORMAL * field.merging).min(1.0));
            line_d = field.d / slope.0.hypot(slope.1).max(LEAST_SLOPE);
        }
        field.line = if field.glass.outline.merged { field.glass.outline.coverage(line_d) } else { pieces_line };
        Some(field)
    }

    pub fn frost(&self) -> Option<&Glass> {
        self.pieces().iter().map(|p| &p.glass).max_by(|a, b| solidity(a).cmp(&solidity(b)))
    }

    pub fn paint_cpu(&self, source: &Pixels) -> Pixels {
        let (width, height) = (source.width, source.height);
        let Some(glass) = self.frost() else { return Pixels { width, height, data: vec![0; width * height] } };
        let under = Under::of(source, glass);
        let mut data = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let p = (x as f64 + 0.5, y as f64 + 0.5);
                let field = self.field(p).expect("the group has a piece");
                let shaded = Look::of(&field.glass).pixel_lined(under.at(y * width + x), field.toward, field.d, field.line, (p, field.bevel), &field.glass);
                data.push(packed(shaded));
            }
        }
        Pixels { width, height, data }
    }

    fn k(&self) -> f64 {
        f64::from(self.spacing).max(LEAST_SPACING)
    }

    fn slope(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let e = GRADIENT_STEP;
        let dx = self.distance((x + e, y)) - self.distance((x - e, y));
        let dy = self.distance((x, y + e)) - self.distance((x, y - e));
        (dx / (2.0 * e), dy / (2.0 * e))
    }
}

fn solidity(glass: &Glass) -> (Total, Total, [Total; super::WIRE_FLOATS]) {
    let shown = || glass.blur.iter().filter(|l| l.opacity > 0.0);
    let widest = shown().map(|l| l.radius).fold(0.0, f32::max);
    let weighted = shown().map(|l| l.radius * l.opacity).sum::<f32>();
    (Total(widest), Total(weighted), glass.floats().map(Total))
}

#[derive(Clone, Copy, PartialEq)]
struct Total(f32);

impl Eq for Total {}

impl PartialOrd for Total {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Total {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

fn weight(d: f64, di: f64, k: f64) -> f64 {
    (0.5 + 0.5 * (d - di) / k).clamp(0.0, 1.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}

fn lerp2(a: (f64, f64), b: (f64, f64), t: f64) -> (f64, f64) {
    (lerp(a.0, b.0, t), lerp(a.1, b.1, t))
}

fn unit((x, y): (f64, f64)) -> (f64, f64) {
    let length = x.hypot(y).max(1e-6);
    (x / length, y / length)
}

#[cfg(test)]
mod tests {
    use super::super::cpu;
    use super::*;
    use crate::blur::channels;
    use crate::corner::Corners;
    use crate::glass::{Level, Role};

    fn regular() -> Glass {
        Glass { outline: super::super::Outline::default(), ..Glass::preset(Role::Regular, Level::Tinted) }
    }

    fn ground(width: usize, height: usize) -> Pixels {
        let data = (0..width * height).map(|i| 0xff00_0000 | ((i % width * 3 % 256) as u32) << 16 | ((i / width * 5 % 256) as u32) << 8 | 0x40).collect();
        Pixels { width, height, data }
    }

    fn group(spacing: f32, pieces: &[Piece]) -> GlassGroup {
        let mut group = GlassGroup::new(spacing);
        pieces.iter().for_each(|p| assert!(group.push(*p)));
        group
    }

    #[test]
    fn the_most_solid_piece_frosts_the_group_whatever_the_order() {
        let (width, height) = (160, 60);
        let source = ground(width, height);
        let clear = Glass { outline: super::super::Outline::default(), ..Glass::preset(Role::Clear, Level::Clear) };
        let left = Piece::of_rect([16.0, 12.0, 60.0, 36.0], 12.0, 2.0, clear);
        let right = Piece::of_rect([70.0, 12.0, 60.0, 36.0], 12.0, 2.0, regular());
        let (forward, backward) = (group(10.0, &[left, right]), group(10.0, &[right, left]));
        assert_eq!(forward.frost(), Some(&right.glass));
        assert_eq!(backward.frost(), Some(&right.glass));
        assert_eq!(forward.paint_cpu(&source).data, backward.paint_cpu(&source).data);
        let twin = Piece { glass: Glass { tint: [1.0, 0.0, 0.0, 0.3], ..right.glass }, ..left };
        assert_eq!(group(10.0, &[twin, right]).frost(), group(10.0, &[right, twin]).frost(), "equally solid pieces still pick one the same way");
    }

    #[test]
    fn one_piece_paints_exactly_like_the_single_glass() {
        let (width, height, margin) = (96, 72, 24);
        let source = ground(width, height);
        let corners = Corners { radius: 8.0, exponent: 4.0 };
        let rect = [margin as f32, margin as f32, (width - 2 * margin) as f32, (height - 2 * margin) as f32];
        let one = group(10.0, &[Piece::of_rect(rect, 8.0, 4.0, regular())]);
        assert_eq!(one.paint_cpu(&source).data, cpu::paint(&source, margin, corners, &regular()).data);
    }

    #[test]
    fn a_piece_out_of_reach_leaves_the_other_exactly_as_it_was() {
        let (width, height) = (200, 60);
        let source = ground(width, height);
        let left = Piece::of_rect([16.0, 12.0, 48.0, 36.0], 10.0, 2.0, regular());
        let right = Piece::of_rect([120.0, 12.0, 48.0, 36.0], 10.0, 2.0, Glass { tint: [1.0, 0.0, 0.0, 0.4], ..regular() });
        let alone = group(20.0, &[left]).paint_cpu(&source);
        let both = group(20.0, &[left, right]).paint_cpu(&source);
        for y in 0..height {
            for x in 0..80 {
                assert_eq!(both.data[y * width + x], alone.data[y * width + x], "({x}, {y})");
            }
        }
    }

    #[test]
    fn pieces_closer_than_the_spacing_flow_into_one() {
        let left = Piece::of_rect([0.0, 0.0, 40.0, 40.0], 20.0, 2.0, regular());
        let right = Piece::of_rect([46.0, 0.0, 40.0, 40.0], 20.0, 2.0, regular());
        let neck = (43.0, 20.0);
        assert!(group(0.0, &[left, right]).distance(neck) > 0.0, "a 6 px gap without merging");
        assert!(group(16.0, &[left, right]).distance(neck) < 0.0, "a 6 px gap within a 16 px spacing is bridged");
        let far = Piece::of_rect([60.0, 0.0, 40.0, 40.0], 20.0, 2.0, regular());
        let gap = group(16.0, &[left, far]);
        assert_eq!(gap.distance((40.0, 20.0)), 0.0, "past the spacing the outline of each piece is its own");
        assert_eq!(gap.distance((60.0, 20.0)), 0.0);
    }

    #[test]
    fn the_neck_takes_the_normal_of_the_merged_field_and_the_mean_of_the_looks() {
        let left = Piece::of_rect([0.0, 0.0, 40.0, 40.0], 20.0, 2.0, Glass { tint: [1.0, 0.0, 0.0, 0.2], ..regular() });
        let right = Piece::of_rect([46.0, 0.0, 40.0, 40.0], 20.0, 2.0, Glass { tint: [0.0, 0.0, 1.0, 0.4], ..regular() });
        let pair = group(16.0, &[left, right]);
        let middle = pair.field((43.0, 20.0)).unwrap();
        assert!((middle.glass.tint[0] - 0.5).abs() < 1e-6 && (middle.glass.tint[3] - 0.3).abs() < 1e-6, "{:?}", middle.glass.tint);
        assert_eq!(middle.merging, 1.0);
        let top = (43.0, (0..40).map(f64::from).find(|&y| pair.distance((43.0, y)) < 0.0).unwrap());
        let field = pair.field(top).unwrap();
        assert!(field.toward.1 < -0.9 && field.toward.0.abs() < 0.2, "the top of the neck faces up: {:?}", field.toward);
    }

    #[test]
    fn sixteen_pieces_at_most_and_the_uniform_layout() {
        let mut full = GlassGroup::new(8.0);
        for i in 0..MOST_PIECES {
            assert!(full.push(Piece::of_rect([i as f32 * 10.0, 0.0, 8.0, 8.0], 2.0, 2.0, regular())));
        }
        assert!(!full.push(Piece::default()));
        assert_eq!(full.len(), MOST_PIECES);
        assert_eq!(full.bounds(), Some([0.0, 0.0, 158.0, 8.0]));
        let bytes = full.to_uniform();
        assert_eq!(bytes.len(), 2304);
        let float = |i: usize| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!([float(0), float(1), float(2), float(3), float(4), float(5)], [4.0, 4.0, 4.0, 4.0, 2.0, 2.0]);
        assert_eq!(float(PIECE_FLOATS), 14.0);
        assert_eq!(float(9), regular().refraction.inner_height);
        assert_eq!(float(23), regular().tint[3]);
    }

    #[test]
    fn overlapping_pieces_have_no_alpha_or_highlight_on_their_old_edges() {
        let glass = Glass {
            highlight: super::super::Highlight { angle: 90f32.to_radians(), amount: 0.8, width: 4.0 },
            lens: super::super::Lens::default(),
            tint: [1.0, 1.0, 1.0, 0.0],
            white_point: 0.0,
            ..regular()
        };
        let pair = group(10.0, &[Piece::of_rect([10.0, 10.0, 60.0, 40.0], 20.0, 2.0, glass), Piece::of_rect([50.0, 10.0, 60.0, 40.0], 20.0, 2.0, glass)]);
        let source = Pixels { width: 120, height: 60, data: vec![0xff40_4040; 120 * 60] };
        let out = pair.paint_cpu(&source);
        let pixel = |x: usize, y: usize| channels(out.data[y * 120 + x]);
        for x in [49, 50, 69, 70] {
            assert_eq!(pixel(x, 30), [255, 64, 64, 64], "the old edge at x={x} stays plain and opaque");
        }
        assert!(pixel(107, 30)[1] > 64, "the new outside edge catches the light");
    }

    fn outlined_pair(merged: bool, width: f32) -> (Pixels, Pixels) {
        let outline = super::super::Outline { width, color: [1.0, 0.0, 0.0, 1.0], merged };
        let plain = [
            super::super::BlurLayer { radius: 0.0, opacity: 1.0 },
            super::super::BlurLayer::default(),
            super::super::BlurLayer::default(),
            super::super::BlurLayer::default(),
            super::super::BlurLayer::default(),
        ];
        let glass = Glass { blur: plain, outline, smoothing: 0.0, ..Glass::default() };
        let left = Piece::of_rect([10.0, 10.0, 40.0, 40.0], 20.0, 2.0, glass);
        let source = Pixels { width: 104, height: 60, data: vec![0xff00_4000; 104 * 60] };
        let pair = group(10.0, &[left, Piece::of_rect([54.0, 10.0, 40.0, 40.0], 20.0, 2.0, glass)]).paint_cpu(&source);
        (pair, group(10.0, &[left]).paint_cpu(&source))
    }

    fn red(out: &Pixels, x: usize, y: usize) -> u32 {
        channels(out.data[y * out.width + x])[1]
    }

    fn first_lit(out: &Pixels) -> Option<(usize, usize, u32)> {
        out.data.iter().enumerate().map(|(i, &p)| (i % out.width, i / out.width, channels(p)[1])).find(|&(_, _, r)| r > 1)
    }

    #[test]
    fn a_merged_outline_follows_the_merged_edge_through_the_neck_and_not_the_old_edges() {
        let (pair, single) = outlined_pair(true, 1.0);
        for x in 40..=64 {
            let column: Vec<u32> = (0..30).map(|y| red(&pair, x, y)).collect();
            let lit: Vec<usize> = (0..30).filter(|&y| column[y] > 1).collect();
            let (first, last) = (lit.first().copied(), lit.last().copied());
            let unbroken = first.zip(last).is_some_and(|(a, b)| b - a + 1 == lit.len());
            assert!(unbroken && (1..=4).contains(&lit.len()), "one thin, unbroken line crosses column {x} on top: {column:?}");
        }
        for (x, y) in [(49, 30), (54, 30), (30, 30), (74, 30)] {
            assert!(red(&pair, x, y) <= 1, "no outline inside at ({x}, {y}): {}", red(&pair, x, y));
        }
        assert!(red(&single, 49, 30) > 1, "a lone piece outlines its own edge");
    }

    #[test]
    fn piece_outlines_keep_each_edge_and_leave_the_gap_between_pieces_bare() {
        let (pair, _) = outlined_pair(false, 1.0);
        assert!(red(&pair, 49, 30) > 128, "the left piece keeps its right edge");
        assert!(red(&pair, 54, 30) > 128, "the right piece keeps its left edge");
        assert!(red(&pair, 52, 30) <= 1, "no piece has an edge in the gap: {}", red(&pair, 52, 30));
        for (x, y) in [(30, 30), (74, 30)] {
            assert!(red(&pair, x, y) <= 1, "no line in the middle of a piece at ({x}, {y})");
        }
        assert!(channels(pair.data[30 * 104 + 52])[0] > 0, "the glass still flows through the gap");
    }

    #[test]
    fn overlapping_pieces_draw_both_outlines_across_each_other() {
        let outline = super::super::Outline { width: 1.0, color: [1.0, 0.0, 0.0, 1.0], merged: false };
        let plain = [
            super::super::BlurLayer { radius: 0.0, opacity: 1.0 },
            super::super::BlurLayer::default(),
            super::super::BlurLayer::default(),
            super::super::BlurLayer::default(),
            super::super::BlurLayer::default(),
        ];
        let glass = Glass { blur: plain, outline, smoothing: 0.0, ..Glass::default() };
        let source = Pixels { width: 104, height: 60, data: vec![0xff00_4000; 104 * 60] };
        let pair = group(10.0, &[Piece::of_rect([10.0, 10.0, 50.0, 40.0], 20.0, 2.0, glass), Piece::of_rect([40.0, 10.0, 50.0, 40.0], 20.0, 2.0, glass)])
            .paint_cpu(&source);
        assert!(red(&pair, 59, 30) > 128, "the left piece's right edge runs through the overlap");
        assert!(red(&pair, 40, 30) > 128, "the right piece's left edge runs through the overlap");
        assert!(red(&pair, 50, 30) <= 1, "between the two crossing lines the overlap is plain glass");
    }

    #[test]
    fn no_outline_draws_no_line() {
        let (pair, single) = outlined_pair(false, 0.0);
        assert_eq!((first_lit(&pair), first_lit(&single)), (None, None), "no red above rounding without an outline");
    }

    #[test]
    fn the_outline_never_touches_the_middle_of_the_glass() {
        let middle = |outline: super::super::Outline| {
            let glass = Glass { outline, ..Glass::preset(Role::Regular, Level::Clear) };
            let source = ground(104, 60);
            let pair = group(10.0, &[Piece::of_rect([10.0, 10.0, 40.0, 40.0], 20.0, 2.0, glass), Piece::of_rect([54.0, 10.0, 40.0, 40.0], 20.0, 2.0, glass)]);
            let out = pair.paint_cpu(&source);
            (out.data[30 * 104 + 30], out.data[30 * 104 + 74])
        };
        let line = super::super::Outline { width: 1.0, color: [1.0, 1.0, 1.0, 0.5], merged: false };
        let none = middle(super::super::Outline::default());
        assert_eq!(middle(line), none);
        assert_eq!(middle(super::super::Outline { merged: true, ..line }), none);
    }

    #[test]
    fn changing_only_the_outline_keeps_every_other_uniform_float() {
        let glass = Glass::preset(Role::Regular, Level::Clear);
        let piece = Piece::of_rect([1.0, 2.0, 30.0, 20.0], 6.0, 2.0, glass);
        let floats = |p: Piece| p.floats();
        let changed = Piece { glass: Glass { outline: super::super::Outline { width: 3.0, color: [0.1, 0.2, 0.3, 0.4], merged: true }, ..glass }, ..piece };
        let outline_slots = [7, 28, 29, 30, 31, 32];
        for (i, (a, b)) in floats(piece).iter().zip(floats(changed)).enumerate() {
            if !outline_slots.contains(&i) {
                assert_eq!(a.to_bits(), b.to_bits(), "uniform float {i} moved when only the outline changed");
            }
        }
    }

    #[test]
    fn an_empty_group_paints_nothing_and_moves_with_its_pieces() {
        let source = ground(8, 8);
        assert!(GlassGroup::new(4.0).paint_cpu(&source).data.iter().all(|&p| channels(p)[0] == 0));
        let one = group(4.0, &[Piece::of_rect([1.0, 2.0, 3.0, 4.0], 0.0, 2.0, regular())]).moved((10.0, 20.0));
        assert_eq!(one.bounds(), Some([11.0, 22.0, 3.0, 4.0]));
    }

    fn validate(source: &str) {
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        let flags = naga::valid::ValidationFlags::all();
        naga::valid::Validator::new(flags, naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    }

    #[test]
    fn the_glass_wgsl_validates_and_the_group_with_its_pieces_declared_by_the_user() {
        validate(&[crate::corner::WGSL, super::super::WGSL].join("\n"));
        let user = "@group(0) @binding(0) var<uniform> acde_glass_pieces: array<AcdeGlassPiece, ACDE_GLASS_MOST_PIECES>;
@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let f = acde_glass_group_field(pos.xy, 3u, 10.0);
    let under = vec4<f32>(acde_glass_group_offset(f) + acde_glass_group_lens(f, pos.xy, 3u, 10.0).green, 0.0, 1.0);
    return acde_glass_group_shade(under, f);
}";
        validate(&[crate::corner::WGSL, super::super::WGSL, WGSL, user].join("\n"));
        let rows = "@group(0) @binding(1) var acde_glass_lens_rows: texture_2d<f32>;
@fragment
fn fs_tabled(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let f = acde_glass_group_field(pos.xy, 3u, 10.0);
    let bend = acde_glass_group_lens_tabled(f, pos.xy, 3u, 10.0, vec4<f32>(0.0, 0.6, 1.5, 0.01));
    return vec4<f32>(bend.red, bend.blue);
}";
        validate(&[crate::corner::WGSL, super::super::WGSL, WGSL, super::super::lens::TABLE_WGSL, user, rows].join("\n"));
    }
}
