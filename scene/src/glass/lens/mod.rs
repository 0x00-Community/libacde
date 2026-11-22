pub const TABLE_SIZE: usize = 2048;
const STEPS: u32 = 64;
const LEAST_SLOPE: f64 = 0.1;
const SPACING_POWER: i32 = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lens {
    pub bezel: f32,
    pub thickness: f32,
    pub index: f32,
    pub dispersion: f32,
}

impl Lens {
    pub const MOST_DISPERSION: f32 = 0.1;
    pub const MOST_INDEX: f32 = 3.0;

    pub fn bends(self) -> bool {
        self.thickness > 0.0 && self.index > 1.0
    }

    pub fn source_depth(self, depth: f64, half_short_side: f64) -> f64 {
        let bezel = f64::from(self.bezel).max(0.5).min(half_short_side.max(0.5));
        if depth <= 0.0 || depth >= bezel || !self.bends() {
            return depth;
        }
        bezel * lens_depth(depth / bezel, f64::from(self.thickness) / bezel, f64::from(self.index))
    }
}

fn lens_depth(x: f64, ratio: f64, index: f64) -> f64 {
    let steps = f64::from(STEPS);
    let node = |j: u32| bent_depth(f64::from(j) / steps, ratio, index);
    let at = x.clamp(0.0, 1.0) * steps;
    let i = (at.floor() as u32).min(STEPS - 1);
    let here = node(i) + (node(i + 1) - node(i)) * (at - f64::from(i));
    (i + 1..=STEPS).fold(here, |best, j| best.min(node(j) - LEAST_SLOPE * (f64::from(j) / steps - x)))
}

fn bezel_height(x: f64) -> f64 {
    let u = 1.0 - x.clamp(0.0, 1.0);
    (1.0 - u.powi(4)).max(0.0).powf(0.25)
}

fn bezel_slope(x: f64) -> f64 {
    let u = 1.0 - x.clamp(1e-6, 1.0);
    u.powi(3) * (1.0 - u.powi(4)).max(1e-6).powf(-0.75)
}

fn bent_depth(x: f64, ratio: f64, index: f64) -> f64 {
    let into = (ratio * bezel_slope(x)).atan();
    let out = (into.sin() / index).clamp(-1.0, 1.0).asin();
    x + ratio * bezel_height(x) * (into - out).tan()
}

pub const TABLE_SLOTS: usize = 8;
pub const TABLE_WGSL: &str = include_str!("table.wgsl");

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub ratio: f32,
    pub index: f32,
    pub dispersion: f32,
}

impl Key {
    pub fn of(lens: Lens, half_short_side: f32) -> Self {
        let bezel = lens.bezel.max(0.5).min(half_short_side.max(0.5));
        Self { ratio: lens.thickness / bezel, index: lens.index, dispersion: lens.dispersion }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub row: usize,
    pub key: Key,
}

#[derive(Debug, Default)]
pub struct LensTable {
    slots: Vec<(Key, u64)>,
}

impl LensTable {
    pub fn slot(&mut self, key: Key, frame: u64) -> Option<(Slot, Option<Vec<f32>>)> {
        if let Some(row) = self.slots.iter().position(|(k, _)| *k == key) {
            self.slots[row].1 = frame;
            return Some((Slot { row, key }, None));
        }
        let row = match self.slots.len() < TABLE_SLOTS {
            true => {
                self.slots.push((key, frame));
                self.slots.len() - 1
            }
            false => {
                let (row, _) = self.slots.iter().enumerate().filter(|(_, (_, used))| *used != frame).min_by_key(|(_, (_, used))| *used)?;
                self.slots[row] = (key, frame);
                row
            }
        };
        Some((Slot { row, key }, Some(build(key))))
    }

    pub fn clear(&mut self) {
        self.slots.clear();
    }
}

fn build(key: Key) -> Vec<f32> {
    let row = |index: f32| (0..TABLE_SIZE).map(move |k| sample_depth(table_x(k), f64::from(key.ratio), f64::from(index)) as f32);
    [key.index - key.dispersion, key.index, key.index + key.dispersion].into_iter().flat_map(row).collect()
}

fn sample_depth(x: f64, ratio: f64, index: f64) -> f64 {
    if ratio <= 0.0 || index <= 1.0 { x } else { lens_depth(x, ratio, index) }
}

pub fn table_x(k: usize) -> f64 {
    (k as f64 / (TABLE_SIZE - 1) as f64).powi(SPACING_POWER)
}

pub fn table_depth(row: &[f32], x: f64) -> f64 {
    let at = x.clamp(0.0, 1.0).powf(1.0 / f64::from(SPACING_POWER)) * (TABLE_SIZE - 1) as f64;
    let k = (at.floor() as usize).min(TABLE_SIZE - 2);
    let (a, b) = (f64::from(row[k]), f64::from(row[k + 1]));
    a + (b - a) * (at - k as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bezel_lens_mapping_never_folds_and_leaves_the_middle_still() {
        for half in [10.0, 40.0, 120.0] {
            for (bezel, thickness) in [(34.0, 22.0), (34.0, 200.0), (4.0, 22.0), (2.0, 80.0), (256.0, 60.0)] {
                for index in [1.0, 1.2, 1.5, 2.0, 3.0] {
                    let lens = Lens { bezel, thickness, index, dispersion: 0.01 };
                    let mut last = -1.0;
                    for i in 0..=2000 {
                        let depth = half * i as f64 / 2000.0;
                        let source = lens.source_depth(depth, half);
                        assert!(source > last, "half={half} bezel={bezel} thickness={thickness} index={index}: {last} then {source} at {depth}");
                        assert!(source <= half + 1e-9, "never past the middle: {source} at {depth}");
                        last = source;
                    }
                }
            }
        }
        let lens = Lens { bezel: 34.0, thickness: 22.0, index: 1.5, dispersion: 0.01 };
        for depth in [34.0, 35.0, 60.0, 100.0] {
            assert_eq!(lens.source_depth(depth, 100.0), depth, "the flat middle does not move at {depth}");
        }
        let most = (0..=340).map(|i| f64::from(i) / 10.0).map(|e| lens.source_depth(e, 100.0) - e).fold(0.0, f64::max);
        assert!(most > 2.0, "the default bezel bends visibly: {most}");
        let off = Lens { thickness: 0.0, ..lens };
        assert!((0..=100).all(|i| off.source_depth(f64::from(i), 100.0) == f64::from(i)));
        let flat = Lens { index: 1.0, ..lens };
        assert!((0..=100).all(|i| flat.source_depth(f64::from(i), 100.0) == f64::from(i)));
    }

    #[test]
    fn the_table_matches_the_profile_within_a_level_and_stays_monotonic() {
        for (bezel, thickness, index) in [(34.0, 22.0, 1.5), (34.0, 200.0, 1.5), (4.0, 22.0, 1.5), (2.0, 80.0, 3.0), (34.0, 120.0, 2.5), (256.0, 60.0, 1.2)] {
            let key = Key::of(Lens { bezel, thickness, index, dispersion: 0.02 }, 200.0);
            let rows = build(key);
            for (n, dn) in [-0.02f32, 0.0, 0.02].into_iter().enumerate() {
                let row = &rows[n * TABLE_SIZE..(n + 1) * TABLE_SIZE];
                let (ratio, index) = (f64::from(key.ratio), f64::from(index + dn));
                let mut last = -1.0;
                for i in 0..=4000 {
                    let x = f64::from(i) / 4000.0;
                    let got = table_depth(row, x);
                    let want = sample_depth(x, ratio, index);
                    assert!((got - want).abs() <= 1.0 / 255.0, "bezel={bezel} thickness={thickness} index={index}: {got} vs {want} at {x}");
                    assert!(got > last, "the table stays increasing at {x}");
                    last = got;
                }
            }
        }
    }

    #[test]
    fn a_table_slot_is_reused_while_the_parameters_stay_the_same() {
        let mut table = LensTable::default();
        let key = Key::of(Lens { bezel: 34.0, thickness: 22.0, index: 1.5, dispersion: 0.01 }, 100.0);
        let (first, rows) = table.slot(key, 0).unwrap();
        assert_eq!(rows.map(|r| r.len()), Some(3 * TABLE_SIZE));
        assert_eq!(table.slot(key, 1), Some((first, None)));
        let other = Key { ratio: 2.0, ..key };
        let (second, rows) = table.slot(other, 1).unwrap();
        assert!(second.row != first.row && rows.is_some());
    }

    #[test]
    fn a_slot_drawn_this_frame_is_never_overwritten() {
        let mut table = LensTable::default();
        let key = |i: usize| Key { ratio: i as f32, index: 1.5, dispersion: 0.0 };
        for i in 0..TABLE_SLOTS {
            assert!(table.slot(key(i), 7).is_some());
        }
        assert_eq!(table.slot(key(99), 7), None, "every slot is in use this frame");
        let (slot, rows) = table.slot(key(99), 8).unwrap();
        assert_eq!((slot.row, rows.is_some()), (0, true));
    }
}
