use std::sync::Mutex;

use realfft::RealFftPlanner;
use realfft::num_complex::Complex32;

use super::graph::{Bus, QUANTUM};

pub const MAX_FFT: usize = 32768;

pub struct Tap {
    samples: Vec<f32>,
    pub written: u64,
}

impl Default for Tap {
    fn default() -> Self {
        Tap { samples: vec![0.0; MAX_FFT], written: 0 }
    }
}

impl Tap {
    pub fn record(tap: &Mutex<Tap>, input: &Bus, mono: bool, silent: bool) {
        let Ok(mut tap) = tap.lock() else { return };
        let at = (tap.written % MAX_FFT as u64) as usize;
        for i in 0..QUANTUM {
            tap.samples[at + i] = match (silent, mono) {
                (true, _) => 0.0,
                (false, true) => input[0][i],
                (false, false) => 0.5 * (input[0][i] + input[1][i]),
            };
        }
        tap.written += QUANTUM as u64;
    }

    pub fn latest(&self, n: usize, out: &mut [f32]) {
        let end = (self.written % MAX_FFT as u64) as usize + MAX_FFT;
        for (i, o) in out.iter_mut().enumerate().take(n) {
            *o = self.samples[(end - n + i) % MAX_FFT];
        }
    }
}

#[derive(Default)]
pub struct Spectrum {
    fft_size: usize,
    smoothed: Vec<f32>,
    decibels: Vec<f32>,
    computed: Option<u64>,
}

impl Spectrum {
    pub fn decibels(&mut self, tap: &Tap, fft_size: usize, smoothing: f32) -> &[f32] {
        if fft_size != self.fft_size {
            *self = Spectrum { fft_size, smoothed: vec![0.0; fft_size / 2], decibels: vec![f32::NEG_INFINITY; fft_size / 2], computed: None };
        }
        if self.computed == Some(tap.written) {
            return &self.decibels;
        }
        self.computed = Some(tap.written);
        let mut time = vec![0.0; fft_size];
        tap.latest(fft_size, &mut time);
        let n = fft_size as f64;
        for (i, v) in time.iter_mut().enumerate() {
            let x = i as f64 / n;
            let window = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * x).cos() + 0.08 * (4.0 * std::f64::consts::PI * x).cos();
            *v = (f64::from(*v) * window) as f32;
        }
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(fft_size);
        let mut spectrum = vec![Complex32::default(); fft_size / 2 + 1];
        let _ = fft.process(&mut time, &mut spectrum);
        let scale = 1.0 / fft_size as f32;
        for k in 0..fft_size / 2 {
            let magnitude = spectrum[k].norm() * scale;
            let previous = if self.smoothed[k].is_finite() { self.smoothed[k] } else { 0.0 };
            let value = smoothing * previous + (1.0 - smoothing) * magnitude;
            self.smoothed[k] = if value.is_finite() { value } else { 0.0 };
            self.decibels[k] = 20.0 * self.smoothed[k].log10();
        }
        &self.decibels
    }
}

pub fn byte_time(sample: f32) -> u8 {
    (128.0 * (1.0 + sample)).floor().clamp(0.0, 255.0) as u8
}

pub fn byte_frequency(db: f32, min: f32, max: f32) -> u8 {
    (255.0 / (max - min) * (db - min)).floor().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_peaks_in_its_bin_and_the_tap_keeps_the_latest_samples() {
        let tap = Mutex::new(Tap::default());
        for q in 0..64 {
            let mut input = [[0.0; QUANTUM]; 2];
            for i in 0..QUANTUM {
                input[0][i] = (((q * QUANTUM + i) as f32) * 2.0 * std::f32::consts::PI * 1500.0 / 48000.0).sin();
            }
            Tap::record(&tap, &input, true, false);
        }
        let tap = tap.into_inner().unwrap();
        let mut spectrum = Spectrum::default();
        let db = spectrum.decibels(&tap, 2048, 0.0).to_vec();
        let peak = (0..db.len()).max_by(|a, b| db[*a].total_cmp(&db[*b])).unwrap();
        assert_eq!(peak, 64, "1500 Hz in 2048 bins at 48 kHz is bin 64");
        let blackman = 20.0 * (0.42f32 * 0.5).log10();
        assert!((db[peak] - blackman).abs() < 0.1 && db[200] < -80.0, "{} {}", db[peak], db[200]);
        let mut last = [0.0; 4];
        tap.latest(4, &mut last);
        let expected = ((64 * QUANTUM - 1) as f32 * 2.0 * std::f32::consts::PI * 1500.0 / 48000.0).sin();
        assert!((last[3] - expected).abs() < 1e-5);
        assert_eq!(byte_time(-2.0), 0);
        assert_eq!(byte_time(0.0), 128);
        assert_eq!(byte_frequency(-30.0, -100.0, -30.0), 255);
    }
}
