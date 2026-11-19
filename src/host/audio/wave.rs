use std::f32::consts::PI;
use std::sync::Arc;

use realfft::RealFftPlanner;
use realfft::num_complex::Complex32;

use super::budget::{Budget, Ticket, zeroed};

const RANGES_PER_OCTAVE: f32 = 3.0;
const CENTS_PER_RANGE: f32 = 1200.0 / RANGES_PER_OCTAVE;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Wave {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

impl Wave {
    pub fn from_code(code: u32) -> Wave {
        match code {
            1 => Wave::Square,
            2 => Wave::Sawtooth,
            3 => Wave::Triangle,
            _ => Wave::Sine,
        }
    }

    fn coefficient(self, n: usize) -> f32 {
        let odd = n & 1 == 1;
        let pi_factor = 2.0 / (n as f32 * PI);
        match self {
            Wave::Sine => f32::from(u8::from(n == 1)),
            Wave::Square => pi_factor * if odd { 2.0 } else { 0.0 },
            Wave::Sawtooth => pi_factor * if odd { 1.0 } else { -1.0 },
            Wave::Triangle if odd => 8.0 / (PI * PI * n as f32 * n as f32) * if ((n - 1) >> 1) & 1 == 1 { -1.0 } else { 1.0 },
            Wave::Triangle => 0.0,
        }
    }
}

pub struct PeriodicWave {
    size: usize,
    lowest: f32,
    rate_scale: f32,
    tables: Vec<Vec<f32>>,
    _ticket: Ticket,
}

pub struct Tables<'w> {
    lower: &'w [f32],
    higher: &'w [f32],
    mix: f32,
}

fn size_for(rate: f64) -> usize {
    if rate <= 24000.0 {
        2048
    } else if rate <= 88200.0 {
        4096
    } else {
        16384
    }
}

fn partials(range: usize, half: usize) -> usize {
    let cents = range as f32 * CENTS_PER_RANGE;
    let culling = 2f64.powf(f64::from(-cents / 1200.0)) as f32;
    (culling * half as f32) as usize
}

impl PeriodicWave {
    pub fn new(real: &[f32], imag: &[f32], normalize: bool, rate: f64, budget: &Arc<Budget>) -> Option<PeriodicWave> {
        let size = size_for(rate);
        let half = size / 2;
        let ranges = (RANGES_PER_OCTAVE * (size as f32).log2()).round() as usize;
        let ticket = Ticket::take(budget, ranges * size)?;
        let components = real.len().min(imag.len()).min(half);
        let inverse = RealFftPlanner::<f32>::new().plan_fft_inverse(size);
        let mut spectrum = vec![Complex32::default(); half + 1];
        let mut scale = 1.0f32;
        let mut tables = Vec::with_capacity(ranges);
        for range in 0..ranges {
            spectrum.iter_mut().for_each(|s| *s = Complex32::default());
            for k in 1..components.min(partials(range, half) + 1) {
                spectrum[k] = Complex32::new(0.5 * real[k], -0.5 * imag[k]);
            }
            let mut table = zeroed(size, 0.0f32)?;
            let _ = inverse.process(&mut spectrum, &mut table);
            if normalize && range == 0 {
                let max = table.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                if max > 0.0 {
                    scale = 1.0 / max;
                }
            }
            table.iter_mut().for_each(|v| *v *= scale);
            tables.push(table);
        }
        let nyquist = (rate / 2.0) as f32;
        Some(PeriodicWave { size, lowest: nyquist / half as f32, rate_scale: (size as f64 / rate) as f32, tables, _ticket: ticket })
    }

    pub fn basic(wave: Wave, rate: f64, budget: &Arc<Budget>) -> Option<PeriodicWave> {
        let half = size_for(rate) / 2;
        let imag: Vec<f32> = (0..half).map(|n| if n == 0 { 0.0 } else { wave.coefficient(n) }).collect();
        PeriodicWave::new(&vec![0.0; half], &imag, true, rate, budget)
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn rate_scale(&self) -> f32 {
        self.rate_scale
    }

    pub fn tables(&self, frequency: f32) -> Tables<'_> {
        let f = frequency.abs();
        let ratio = if f > 0.0 { f / self.lowest } else { 0.5 };
        let cents = ratio.log2() * 1200.0;
        let last = self.tables.len() - 1;
        let range = (1.0 + cents / CENTS_PER_RANGE).max(0.0).min(last as f32);
        let higher = range as usize;
        let lower = if higher < last { higher + 1 } else { higher };
        Tables { lower: &self.tables[lower], higher: &self.tables[higher], mix: range - higher as f32 }
    }

    pub fn sample(&self, tables: &Tables, index: f64) -> f32 {
        let r0 = index as usize;
        let r1 = (r0 + 1) & (self.size - 1);
        let t = index as f32 - r0 as f32;
        let higher = (1.0 - t) * tables.higher[r0] + t * tables.higher[r1];
        let lower = (1.0 - t) * tables.lower[r0] + t * tables.lower[r1];
        (1.0 - tables.mix) * higher + tables.mix * lower
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave(w: Wave) -> PeriodicWave {
        PeriodicWave::basic(w, 48000.0, &Arc::new(Budget::default())).unwrap()
    }

    #[test]
    fn the_basic_waves_peak_at_one_and_start_as_chrome_does() {
        for w in [Wave::Sine, Wave::Square, Wave::Sawtooth, Wave::Triangle] {
            let p = wave(w);
            let peak = p.tables[0].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((peak - 1.0).abs() < 1e-6, "{w:?} {peak}");
            assert!(p.tables[0][0].abs() < 1e-6, "{w:?} starts at zero");
        }
        let sine = wave(Wave::Sine);
        assert!((sine.tables[0][1024] - 1.0).abs() < 1e-6, "a quarter of the way through a 4096 table");
        let plateau = wave(Wave::Square).tables[0][1024];
        assert!((plateau - 1.0 / 1.179).abs() < 0.01, "the Gibbs peak is scaled to one: {plateau}");
        assert!(wave(Wave::Triangle).tables[0][1024] > 0.99);
    }

    #[test]
    fn higher_pitches_use_tables_with_fewer_partials() {
        let saw = wave(Wave::Sawtooth);
        assert_eq!(saw.tables.len(), 36);
        assert_eq!([0, 3, 33, 35].map(|r| partials(r, 2048)), [2048, 1024, 1, 0]);
        let low = saw.tables(5.0);
        assert_eq!((low.higher.as_ptr(), low.mix), (saw.tables[0].as_ptr(), 0.0));
        let high = saw.tables(13000.0);
        assert!(std::ptr::eq(high.higher, &saw.tables[31][..]) && high.mix > 0.0, "{}", high.mix);
    }

    #[test]
    fn without_normalization_the_coefficients_are_the_amplitudes() {
        let budget = Arc::new(Budget::default());
        let p = PeriodicWave::new(&[0.0, 0.25], &[0.0, 0.5], false, 48000.0, &budget).unwrap();
        assert!((p.tables[0][0] - 0.25).abs() < 1e-6 && (p.tables[0][1024] - 0.5).abs() < 1e-6);
        assert_eq!(budget.used(), 36 * 4096);
        drop(p);
        assert_eq!(budget.used(), 0);
    }
}
