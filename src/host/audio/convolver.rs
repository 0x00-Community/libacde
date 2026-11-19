use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use super::budget::{Budget, Ticket};
use super::graph::{AudioData, Bus, QUANTUM};

const LEVELS: [(usize, usize, usize); 3] = [(QUANTUM, 0, 1024), (1024, 1024, 8192), (8192, 8192, usize::MAX)];
const RING: usize = 32768;

pub fn normalization(data: &AudioData) -> f32 {
    let (channels, length) = (data.channels.len(), data.len());
    let power: f64 = data.channels.iter().flatten().map(|v| f64::from(*v) * f64::from(*v)).sum();
    let mut power = (power / (channels * length).max(1) as f64).sqrt();
    if !power.is_finite() || power < 0.000_125 {
        power = 0.000_125;
    }
    let mut scale = 10f64.powf(-58.0 / 20.0) / power;
    if data.rate > 0.0 {
        scale *= 44100.0 / data.rate;
    }
    scale as f32
}

struct Level {
    block: usize,
    start: usize,
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    spectra: Arc<Vec<Vec<Complex32>>>,
    history: Vec<Vec<Complex32>>,
    newest: usize,
    input: Vec<f32>,
    filled: usize,
    time: Vec<f32>,
    sum: Vec<Complex32>,
    scratch: Vec<Complex32>,
}

impl Level {
    fn floats(response: usize, (block, start, end): (usize, usize, usize)) -> usize {
        let partitions = end.min(response).saturating_sub(start).div_ceil(block);
        partitions * (block + 1) * 2 + 8 * block
    }

    fn new(planner: &mut RealFftPlanner<f32>, response: &[f32], scale: f32, (block, start, end): (usize, usize, usize)) -> Option<Level> {
        let segment = response.get(start..end.min(response.len())).filter(|s| !s.is_empty())?;
        let size = 2 * block;
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        let mut scratch = vec![Complex32::default(); forward.get_scratch_len().max(inverse.get_scratch_len())];
        let spectra = segment
            .chunks(block)
            .map(|part| {
                let mut time = vec![0.0; size];
                for (t, v) in time.iter_mut().zip(part) {
                    *t = v * scale / size as f32;
                }
                let mut spectrum = vec![Complex32::default(); block + 1];
                let _ = forward.process_with_scratch(&mut time, &mut spectrum, &mut scratch);
                spectrum
            })
            .collect();
        Some(Level::with(block, start, forward, inverse, Arc::new(spectra)))
    }

    fn with(
        block: usize,
        start: usize,
        forward: Arc<dyn RealToComplex<f32>>,
        inverse: Arc<dyn ComplexToReal<f32>>,
        spectra: Arc<Vec<Vec<Complex32>>>,
    ) -> Level {
        let size = 2 * block;
        let bins = block + 1;
        let scratch_len = forward.get_scratch_len().max(inverse.get_scratch_len());
        Level {
            block,
            start,
            history: vec![vec![Complex32::default(); bins]; spectra.len()],
            forward,
            inverse,
            spectra,
            newest: 0,
            input: vec![0.0; size],
            filled: 0,
            time: vec![0.0; size],
            sum: vec![Complex32::default(); bins],
            scratch: vec![Complex32::default(); scratch_len],
        }
    }

    fn sibling(&self) -> Level {
        Level::with(self.block, self.start, self.forward.clone(), self.inverse.clone(), self.spectra.clone())
    }

    fn push(&mut self, samples: &[f32], now: u64, ring: &mut [f32]) {
        let size = 2 * self.block;
        self.input[self.block + self.filled..self.block + self.filled + samples.len()].copy_from_slice(samples);
        self.filled += samples.len();
        if self.filled < self.block {
            return;
        }
        self.filled = 0;
        let block_start = now + samples.len() as u64 - self.block as u64;
        self.time.copy_from_slice(&self.input);
        self.input.copy_within(self.block..size, 0);
        let partitions = self.spectra.len();
        self.newest = (self.newest + 1) % partitions;
        let newest = self.newest;
        let _ = self.forward.process_with_scratch(&mut self.time, &mut self.history[newest], &mut self.scratch);
        self.sum.iter_mut().for_each(|s| *s = Complex32::default());
        for (p, h) in self.spectra.iter().enumerate() {
            let x = &self.history[(newest + partitions - p) % partitions];
            for ((s, a), b) in self.sum.iter_mut().zip(x).zip(h) {
                *s += a * b;
            }
        }
        self.sum[0].im = 0.0;
        if let Some(last) = self.sum.last_mut() {
            last.im = 0.0;
        }
        let _ = self.inverse.process_with_scratch(&mut self.sum, &mut self.time, &mut self.scratch);
        let at = block_start + self.start as u64;
        for (i, v) in self.time[self.block..].iter().enumerate() {
            ring[((at + i as u64) % RING as u64) as usize] += v;
        }
    }
}

struct Path {
    levels: Vec<Level>,
    ring: Vec<f32>,
}

impl Path {
    fn new(planner: &mut RealFftPlanner<f32>, response: &[f32], scale: f32) -> Path {
        let levels = LEVELS.iter().filter_map(|level| Level::new(planner, response, scale, *level)).collect();
        Path { levels, ring: vec![0.0; RING] }
    }

    fn floats(response: usize) -> usize {
        LEVELS.iter().map(|level| Level::floats(response, *level)).sum::<usize>() + RING
    }

    fn sibling(&self) -> Path {
        Path { levels: self.levels.iter().map(Level::sibling).collect(), ring: vec![0.0; RING] }
    }

    fn process(&mut self, input: &[f32; QUANTUM], now: u64, out: &mut [f32; QUANTUM]) {
        for level in &mut self.levels {
            level.push(input, now, &mut self.ring);
        }
        for (i, o) in out.iter_mut().enumerate() {
            let at = ((now + i as u64) % RING as u64) as usize;
            *o = self.ring[at];
            self.ring[at] = 0.0;
        }
    }
}

pub struct Convolver {
    paths: Vec<Path>,
    stereo_response: bool,
    length: usize,
    now: u64,
    _ticket: Ticket,
}

impl Convolver {
    pub fn new(response: &AudioData, normalize: bool, budget: &Arc<Budget>) -> Option<Convolver> {
        let length = response.len();
        if length == 0 || response.channels.is_empty() {
            return None;
        }
        let stereo_response = response.channels.len() >= 2;
        let responses = if stereo_response { &response.channels[..2] } else { &response.channels[..1] };
        let spectra = LEVELS.iter().map(|(block, start, end)| end.min(&length).saturating_sub(*start).div_ceil(*block) * (block + 1) * 2).sum::<usize>();
        let floats = Path::floats(length).checked_mul(2)?.checked_add(spectra.checked_mul(responses.len())?)?;
        let ticket = Ticket::take(budget, floats)?;
        let scale = if normalize { normalization(response) } else { 1.0 };
        let mut planner = RealFftPlanner::new();
        let mut paths: Vec<Path> = responses.iter().map(|r| Path::new(&mut planner, r, scale)).collect();
        if !stereo_response {
            paths.push(paths[0].sibling());
        }
        Some(Convolver { paths, stereo_response, length, now: 0, _ticket: ticket })
    }

    pub fn tail_frames(&self) -> u64 {
        self.length as u64 + QUANTUM as u64
    }

    pub fn process(&mut self, input: &Bus, mono: bool, out: &mut Bus) -> bool {
        let mono_out = mono && !self.stereo_response;
        for (c, path) in self.paths.iter_mut().enumerate().take(if mono_out { 1 } else { 2 }) {
            let source = if mono { &input[0] } else { &input[c] };
            path.process(source, self.now, &mut out[c]);
        }
        self.now += QUANTUM as u64;
        mono_out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct(signal: &[f32], response: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0; signal.len()];
        for n in 0..signal.len() {
            for (k, h) in response.iter().enumerate() {
                if k <= n {
                    out[n] += h * signal[n - k];
                }
            }
        }
        out
    }

    #[test]
    fn partitioned_convolution_equals_direct_convolution_across_all_levels() {
        let length = 20000;
        let response: Vec<f32> = (0..length).map(|i| ((i * 7919 % 1000) as f32 / 1000.0 - 0.5) * (-(i as f32) / 6000.0).exp()).collect();
        let data = AudioData { rate: 48000.0, channels: vec![response.clone()] };
        let budget = Arc::new(Budget::default());
        let mut convolver = Convolver::new(&data, false, &budget).unwrap();
        assert!(Arc::ptr_eq(&convolver.paths[0].levels[2].spectra, &convolver.paths[1].levels[2].spectra), "a mono response's paths share its spectra");
        let charged = budget.used();
        let path = Path::floats(length);
        assert!(charged > 2 * path && charged < 3 * path - 2 * RING, "two paths, the spectra charged once: {charged}");
        let quanta = 200;
        let signal: Vec<f32> = (0..quanta * QUANTUM).map(|i| if i % 997 == 0 { 1.0 } else { ((i as f32) * 0.01).sin() * 0.1 }).collect();
        let mut got = Vec::new();
        for q in 0..quanta {
            let mut input = [[0.0; QUANTUM]; 2];
            input[0].copy_from_slice(&signal[q * QUANTUM..(q + 1) * QUANTUM]);
            let mut out = [[0.0; QUANTUM]; 2];
            assert!(convolver.process(&input, true, &mut out), "mono in, mono response: mono out");
            got.extend_from_slice(&out[0]);
        }
        let want = direct(&signal, &response);
        let worst = got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 2e-3, "{worst}");
        drop(convolver);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn normalization_follows_the_spec() {
        let data = AudioData { rate: 44100.0, channels: vec![vec![0.5; 100]] };
        assert!((normalization(&data) - 0.001_258_925_4 / 0.5).abs() < 1e-7);
        let quiet = AudioData { rate: 88200.0, channels: vec![vec![0.0; 100]] };
        assert!((normalization(&quiet) - 0.001_258_925_4 / 0.000_125 * 0.5).abs() < 1e-5);
    }
}
