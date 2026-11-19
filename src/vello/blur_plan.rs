#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blur {
    pub levels: u32,
    pub offset: f32,
    pub mix: f32,
}

const MAX_LEVELS: u32 = 5;

const SPREAD: [(f32, f32); 8] = [(0.0, 0.92), (0.25, 1.17), (0.5, 2.32), (0.75, 4.07), (1.0, 6.82), (1.25, 9.93), (1.5, 13.985), (2.0, 24.31)];

const SWITCH: f32 = SPREAD[2].1;

impl Blur {
    pub fn of(sigma: f32) -> Option<Self> {
        if sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
            return None;
        }
        let variance = sigma * sigma;
        let least = SPREAD[0].1;
        if variance < least {
            return Some(Self { levels: 1, offset: 0.0, mix: variance / least });
        }
        let mut levels = 1;
        while levels < MAX_LEVELS && variance >= Self::chain(levels + 1) * SWITCH {
            levels += 1;
        }
        Some(Self { levels, offset: Self::offset_for(variance / Self::chain(levels)), mix: 1.0 })
    }

    pub fn sigma(&self) -> f32 {
        (Self::chain(self.levels) * Self::spread(self.offset) * self.mix).sqrt()
    }

    pub fn margin(&self) -> i32 {
        ((2 << self.levels) as f32 * self.offset.max(1.0)).ceil() as i32
    }

    fn chain(levels: u32) -> f32 {
        ((1u64 << (2 * levels)) - 1) as f32 / 3.0
    }

    fn spread(offset: f32) -> f32 {
        let at = SPREAD.windows(2).find(|w| offset <= w[1].0).unwrap_or(&SPREAD[SPREAD.len() - 2..]);
        let ((o0, s0), (o1, s1)) = (at[0], at[1]);
        s0 + (s1 - s0) * (offset - o0) / (o1 - o0)
    }

    fn offset_for(spread: f32) -> f32 {
        let at = SPREAD.windows(2).find(|w| spread <= w[1].1).unwrap_or(&SPREAD[SPREAD.len() - 2..]);
        let ((o0, s0), (o1, s1)) = (at[0], at[1]);
        (o0 + (o1 - o0) * (spread - s0) / (s1 - s0)).clamp(0.0, SPREAD[SPREAD.len() - 1].0)
    }
}
