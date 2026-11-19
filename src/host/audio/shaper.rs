use std::f64::consts::PI;
use std::sync::Arc;

use super::graph::{Bus, QUANTUM};

const UP_KERNEL: usize = 128;
const DOWN_KERNEL: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Oversample {
    #[default]
    None,
    Twice,
    Four,
}

impl Oversample {
    pub fn from_code(code: u32) -> Oversample {
        match code {
            2 => Oversample::Twice,
            4 => Oversample::Four,
            _ => Oversample::None,
        }
    }
}

fn blackman(x: f64) -> f64 {
    let alpha = 0.16;
    0.5 * (1.0 - alpha) - 0.5 * (2.0 * PI * x).cos() + 0.5 * alpha * (2.0 * PI * 2.0 * x).cos()
}

fn up_kernel() -> Vec<f32> {
    let n = UP_KERNEL as f64;
    let half = (UP_KERNEL / 2) as f64;
    (0..UP_KERNEL)
        .map(|i| {
            let s = PI * (i as f64 - half + 0.5);
            let sinc = if s == 0.0 { 1.0 } else { s.sin() / s };
            (sinc * blackman((i as f64 + 0.5) / n)) as f32
        })
        .collect()
}

fn down_kernel() -> Vec<f32> {
    let n = DOWN_KERNEL as f64;
    let half = (DOWN_KERNEL / 2) as f64;
    (1..DOWN_KERNEL)
        .step_by(2)
        .map(|i| {
            let s = 0.5 * PI * (i as f64 - half);
            let sinc = if s == 0.0 { 1.0 } else { s.sin() / s } * 0.5;
            (sinc * blackman(i as f64 / n)) as f32
        })
        .collect()
}

struct Fir {
    kernel: Arc<[f32]>,
    history: Vec<f32>,
}

impl Fir {
    fn new(kernel: Arc<[f32]>) -> Fir {
        let history = vec![0.0; kernel.len() - 1];
        Fir { kernel, history }
    }

    fn process(&mut self, source: &[f32], dest: &mut [f32]) {
        let taps = self.kernel.len();
        let mut buffer = std::mem::take(&mut self.history);
        buffer.extend_from_slice(source);
        for (i, out) in dest.iter_mut().enumerate().take(source.len()) {
            let mut sum = 0.0f32;
            for (j, k) in self.kernel.iter().enumerate() {
                sum += k * buffer[i + taps - 1 - j];
            }
            *out = sum;
        }
        self.history = buffer.split_off(source.len());
    }
}

struct Up {
    delay: Vec<f32>,
    odd: Fir,
}

impl Up {
    fn new(kernel: Arc<[f32]>) -> Up {
        Up { delay: vec![0.0; UP_KERNEL / 2], odd: Fir::new(kernel) }
    }

    fn process(&mut self, source: &[f32], dest: &mut [f32]) {
        let n = source.len();
        let mut buffer = std::mem::take(&mut self.delay);
        buffer.extend_from_slice(source);
        let mut odd = vec![0.0; n];
        self.odd.process(source, &mut odd);
        for i in 0..n {
            dest[2 * i] = buffer[i];
            dest[2 * i + 1] = odd[i];
        }
        self.delay = buffer.split_off(n);
    }
}

struct Down {
    delay: Vec<f32>,
    odd: Fir,
}

impl Down {
    fn new(kernel: Arc<[f32]>) -> Down {
        Down { delay: vec![0.0; DOWN_KERNEL / 2], odd: Fir::new(kernel) }
    }

    fn process(&mut self, source: &[f32], dest: &mut [f32]) {
        let half = DOWN_KERNEL / 2;
        let n = source.len();
        let mut buffer = std::mem::take(&mut self.delay);
        buffer.extend_from_slice(source);
        let odd: Vec<f32> = (0..n / 2).map(|i| buffer[half - 1 + 2 * i]).collect();
        self.odd.process(&odd, dest);
        for i in 0..n / 2 {
            dest[i] += 0.5 * buffer[2 * i];
        }
        self.delay = buffer.split_off(n);
    }
}

struct Channel {
    up: Up,
    down: Down,
    up2: Up,
    down2: Down,
}

pub struct Shaper {
    curve: Option<Arc<[f32]>>,
    oversample: Oversample,
    channels: [Channel; 2],
}

impl Default for Shaper {
    fn default() -> Self {
        let (up, down): (Arc<[f32]>, Arc<[f32]>) = (up_kernel().into(), down_kernel().into());
        let channel = || Channel { up: Up::new(up.clone()), down: Down::new(down.clone()), up2: Up::new(up.clone()), down2: Down::new(down.clone()) };
        Shaper { curve: None, oversample: Oversample::None, channels: [channel(), channel()] }
    }
}

fn curve_value(input: f32, curve: &[f32]) -> f32 {
    let last = curve.len() - 1;
    let index = 0.5 * (f64::from(input) + 1.0) * last as f64;
    if index < 0.0 || index.is_nan() {
        curve[0]
    } else if index >= last as f64 {
        curve[last]
    } else {
        let first = index as usize;
        let f = index - first as f64;
        ((1.0 - f) * f64::from(curve[first]) + f * f64::from(curve[first + 1])) as f32
    }
}

impl Shaper {
    pub fn set_curve(&mut self, curve: Option<Arc<[f32]>>) {
        self.curve = curve.filter(|c| c.len() >= 2);
    }

    pub fn set_oversample(&mut self, oversample: Oversample) {
        if oversample != self.oversample {
            *self = Shaper { curve: self.curve.take(), oversample, ..Shaper::default() };
        }
    }

    pub fn sounds_when_silent(&self) -> bool {
        self.curve.as_ref().is_some_and(|curve| curve_value(0.0, curve) != 0.0)
    }

    pub fn tail_frames(&self) -> u64 {
        match &self.curve {
            _ if self.sounds_when_silent() => u64::MAX,
            _ => match self.oversample {
                Oversample::None => 0,
                Oversample::Twice | Oversample::Four => (UP_KERNEL + DOWN_KERNEL) as u64,
            },
        }
    }

    pub fn process(&mut self, input: &Bus, mono: bool, out: &mut Bus) {
        let Some(curve) = self.curve.clone() else {
            *out = *input;
            return;
        };
        for c in 0..if mono { 1 } else { 2 } {
            let source = &input[c];
            let k = &mut self.channels[c];
            match self.oversample {
                Oversample::None => {
                    for i in 0..QUANTUM {
                        out[c][i] = curve_value(source[i], &curve);
                    }
                }
                Oversample::Twice => {
                    let mut wide = [0.0; QUANTUM * 2];
                    k.up.process(source, &mut wide);
                    for v in wide.iter_mut() {
                        *v = curve_value(*v, &curve);
                    }
                    k.down.process(&wide, &mut out[c]);
                }
                Oversample::Four => {
                    let mut wide = [0.0; QUANTUM * 2];
                    let mut wider = [0.0; QUANTUM * 4];
                    k.up.process(source, &mut wide);
                    k.up2.process(&wide, &mut wider);
                    for v in wider.iter_mut() {
                        *v = curve_value(*v, &curve);
                    }
                    k.down2.process(&wider, &mut wide);
                    k.down.process(&wide, &mut out[c]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_maps_minus_one_to_one_and_clamps_outside() {
        let curve = [-1.0, 0.0, 0.5];
        assert_eq!(curve_value(-1.0, &curve), -1.0);
        assert_eq!(curve_value(-2.0, &curve), -1.0);
        assert_eq!(curve_value(0.0, &curve), 0.0);
        assert_eq!(curve_value(0.5, &curve), 0.25);
        assert_eq!(curve_value(3.0, &curve), 0.5);
    }

    #[test]
    fn oversampling_keeps_a_straight_curve_straight_after_its_latency() {
        let mut shaper = Shaper::default();
        shaper.set_curve(Some(vec![-1.0, 1.0].into()));
        shaper.set_oversample(Oversample::Twice);
        let mut input = [[0.0; QUANTUM]; 2];
        let mut outputs = Vec::new();
        for q in 0..8 {
            for i in 0..QUANTUM {
                input[0][i] = (((q * QUANTUM + i) as f32) * 0.05).sin() * 0.5;
            }
            let mut out = [[0.0; QUANTUM]; 2];
            shaper.process(&input, true, &mut out);
            outputs.extend_from_slice(&out[0]);
        }
        let latency = UP_KERNEL / 2 + DOWN_KERNEL / 4;
        for n in 400..800 {
            let expected = (((n - latency) as f32) * 0.05).sin() * 0.5;
            assert!((outputs[n] - expected).abs() < 2e-3, "{n}: {} vs {expected}", outputs[n]);
        }
        assert_eq!(shaper.tail_frames(), (UP_KERNEL + DOWN_KERNEL) as u64);
        shaper.set_curve(Some(vec![0.5, 0.5].into()));
        assert_eq!(shaper.tail_frames(), u64::MAX, "a curve that is not 0 at 0 never goes quiet");
    }
}
