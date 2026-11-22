use super::Glass;
use super::cpu::{Look, Under, packed};
use crate::blur::Pixels;
use crate::corner;

pub const WGSL: &str = include_str!("group.wgsl");

pub const MOST_PIECES: usize = 16;
pub const PIECE_BYTES: usize = PIECE_FLOATS * 4;
pub const UNIFORM_BYTES: usize = MOST_PIECES * PIECE_BYTES;
const PIECE_FLOATS: usize = 24;
const LEAST_SPACING: f64 = 1e-4;
const GRADIENT_STEP: f64 = 0.5;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Piece {
    pub center: [f32; 2],
    pub half: [f32; 2],
    pub radius: f32,
    pub exponent: f32,
    pub glass: Glass,
}

impl Piece {
    pub fn of_rect([x, y, w, h]: [f32; 4], radius: f32, exponent: f32, glass: Glass) -> Self {
        Self { center: [x + w / 2.0, y + h / 2.0], half: [w / 2.0, h / 2.0], radius, exponent, glass }
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
            0.0,
            0.0,
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
            0.0,
            g.tint[0],
            g.tint[1],
            g.tint[2],
            g.tint[3],
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
    pub toward: (f64, f64),
    pub merging: f64,
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
        let mut field = Field { d: first.distance(p), toward: first.toward(p), merging: 0.0, glass: first.glass };
        for piece in rest {
            let di = piece.distance(p);
            let h = weight(field.d, di, k);
            if h >= 1.0 {
                field.toward = piece.toward(p);
            } else if h > 0.0 {
                field.toward = lerp2(unit(field.toward), unit(piece.toward(p)), h);
            }
            field.glass = field.glass.mixed(&piece.glass, h as f32);
            field.d = lerp(field.d, di, h) - k * h * (1.0 - h);
            field.merging = field.merging.max(4.0 * h * (1.0 - h));
        }
        if field.merging > 0.0 {
            field.toward = lerp2(unit(field.toward), self.gradient(p), field.merging);
        }
        Some(field)
    }

    pub fn paint_cpu(&self, source: &Pixels) -> Pixels {
        let (width, height) = (source.width, source.height);
        let Some(glass) = self.glass() else { return Pixels { width, height, data: vec![0; width * height] } };
        let under = Under::of(source, glass);
        let mut data = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let field = self.field((x as f64 + 0.5, y as f64 + 0.5)).expect("the group has a piece");
                data.push(packed(Look::of(&field.glass).pixel(under.at(y * width + x), field.toward, field.d, &field.glass)));
            }
        }
        Pixels { width, height, data }
    }

    fn k(&self) -> f64 {
        f64::from(self.spacing).max(LEAST_SPACING)
    }

    fn gradient(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let e = GRADIENT_STEP;
        let dx = self.distance((x + e, y)) - self.distance((x - e, y));
        let dy = self.distance((x, y + e)) - self.distance((x, y - e));
        unit((dx, dy))
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

    fn regular() -> Glass {
        Glass::preset("regular").unwrap()
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
    fn one_piece_paints_exactly_like_the_single_glass() {
        let (width, height, margin) = (96, 72, 24);
        let source = ground(width, height);
        let corners = Corners { radius: 8.0, exponent: 4.0 };
        let rect = [margin as f32, margin as f32, (width - 2 * margin) as f32, (height - 2 * margin) as f32];
        let one = group(12.0, &[Piece::of_rect(rect, 8.0, 4.0, regular())]);
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
        assert_eq!(bytes.len(), 1536);
        let float = |i: usize| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!([float(0), float(1), float(2), float(3), float(4), float(5)], [4.0, 4.0, 4.0, 4.0, 2.0, 2.0]);
        assert_eq!(float(PIECE_FLOATS), 14.0);
        assert_eq!(float(9), regular().refraction.inner_height);
        assert_eq!(float(23), regular().tint[3]);
    }

    #[test]
    fn an_empty_group_paints_nothing_and_moves_with_its_pieces() {
        let source = ground(8, 8);
        assert!(GlassGroup::new(4.0).paint_cpu(&source).data.iter().all(|&p| channels(p)[0] == 0));
        let one = group(4.0, &[Piece::of_rect([1.0, 2.0, 3.0, 4.0], 0.0, 2.0, regular())]).moved((10.0, 20.0));
        assert_eq!(one.bounds(), Some([11.0, 22.0, 3.0, 4.0]));
    }
}
