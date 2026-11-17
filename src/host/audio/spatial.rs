use std::f64::consts::PI;

use super::graph::QUANTUM;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistanceModel {
    Linear,
    Inverse,
    Exponential,
}

impl DistanceModel {
    pub fn from_code(code: u32) -> DistanceModel {
        match code {
            0 => DistanceModel::Linear,
            2 => DistanceModel::Exponential,
            _ => DistanceModel::Inverse,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PannerConfig {
    pub hrtf: bool,
    pub distance: DistanceModel,
    pub ref_distance: f64,
    pub max_distance: f64,
    pub rolloff: f64,
    pub cone_inner: f64,
    pub cone_outer: f64,
    pub cone_outer_gain: f64,
}

impl Default for PannerConfig {
    fn default() -> Self {
        PannerConfig {
            hrtf: false,
            distance: DistanceModel::Inverse,
            ref_distance: 1.0,
            max_distance: 10000.0,
            rolloff: 1.0,
            cone_inner: 360.0,
            cone_outer: 360.0,
            cone_outer_gain: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub fn new(x: f64, y: f64, z: f64) -> V3 {
        V3 { x, y, z }
    }

    fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    fn scale(self, k: f64) -> V3 {
        V3::new(self.x * k, self.y * k, self.z * k)
    }

    fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    fn cross(self, o: V3) -> V3 {
        V3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    fn len(self) -> f64 {
        self.dot(self).sqrt()
    }

    fn unit(self) -> Option<V3> {
        let n = self.len();
        (n > 0.0 && n.is_finite()).then(|| self.scale(1.0 / n))
    }
}

fn degrees_between(a: V3, b: V3) -> f64 {
    a.dot(b).clamp(-1.0, 1.0).acos() * 180.0 / PI
}

pub struct Listener {
    pub position: V3,
    pub forward: V3,
    pub up: V3,
}

impl Default for Listener {
    fn default() -> Self {
        Listener { position: V3::default(), forward: V3::new(0.0, 0.0, -1.0), up: V3::new(0.0, 1.0, 0.0) }
    }
}

pub fn azimuth_elevation(source: V3, listener: &Listener) -> (f64, f64) {
    let Some(to_source) = source.sub(listener.position).unit() else { return (0.0, 0.0) };
    let (Some(right), Some(forward)) = (listener.forward.cross(listener.up).unit(), listener.forward.unit()) else { return (0.0, 0.0) };
    let up = right.cross(forward);
    let projected = to_source.sub(up.scale(to_source.dot(up)));
    let mut azimuth = match projected.unit() {
        Some(p) => {
            let a = degrees_between(p, right);
            if p.dot(forward) < 0.0 { 360.0 - a } else { a }
        }
        None => 90.0,
    };
    azimuth = if (0.0..=270.0).contains(&azimuth) { 90.0 - azimuth } else { 450.0 - azimuth };
    let mut elevation = 90.0 - degrees_between(to_source, up);
    if elevation > 90.0 {
        elevation = 180.0 - elevation;
    } else if elevation < -90.0 {
        elevation = -180.0 - elevation;
    }
    (azimuth, elevation)
}

pub fn distance_gain(c: &PannerConfig, distance: f64) -> f64 {
    let (dref, f) = (c.ref_distance, c.rolloff);
    match c.distance {
        DistanceModel::Linear => {
            let (lo, hi) = (dref.min(c.max_distance), dref.max(c.max_distance));
            let f = f.clamp(0.0, 1.0);
            if hi == lo {
                return 1.0 - f;
            }
            1.0 - f * (distance.clamp(lo, hi) - lo) / (hi - lo)
        }
        DistanceModel::Inverse => {
            if dref == 0.0 {
                return 0.0;
            }
            dref / (dref + f * (distance.max(dref) - dref))
        }
        DistanceModel::Exponential => {
            if dref == 0.0 {
                return 0.0;
            }
            (distance.max(dref) / dref).powf(-f)
        }
    }
}

pub fn cone_gain(c: &PannerConfig, source: V3, orientation: V3, listener: V3) -> f64 {
    if c.cone_inner == 360.0 && c.cone_outer == 360.0 {
        return 1.0;
    }
    let Some(orientation) = orientation.unit() else { return 1.0 };
    let Some(to_listener) = listener.sub(source).unit() else { return 1.0 };
    let angle = degrees_between(to_listener, orientation).abs();
    let (inner, outer) = (c.cone_inner.abs() / 2.0, c.cone_outer.abs() / 2.0);
    if angle <= inner {
        1.0
    } else if angle >= outer {
        c.cone_outer_gain
    } else {
        let x = (angle - inner) / (outer - inner);
        (1.0 - x) + c.cone_outer_gain * x
    }
}

pub fn equal_power(azimuth: f64, mono: bool) -> (f64, f64, bool) {
    let mut a = azimuth.clamp(-180.0, 180.0);
    if a < -90.0 {
        a = -180.0 - a;
    } else if a > 90.0 {
        a = 180.0 - a;
    }
    let x = if mono {
        (a + 90.0) / 180.0
    } else if a <= 0.0 {
        (a + 90.0) / 90.0
    } else {
        a / 90.0
    };
    ((x * PI / 2.0).cos(), (x * PI / 2.0).sin(), a <= 0.0)
}

pub struct Spot {
    pub azimuth: f64,
    pub elevation: f64,
    pub gain: f64,
}

pub fn locate(c: &PannerConfig, source: V3, orientation: V3, listener: &Listener) -> Spot {
    let (azimuth, elevation) = azimuth_elevation(source, listener);
    let distance = source.sub(listener.position).len();
    let gain = cone_gain(c, source, orientation, listener.position) * distance_gain(c, distance);
    Spot { azimuth, elevation, gain: if gain.is_finite() { gain } else { 0.0 } }
}

const HEAD_RADIUS: f64 = 0.10;
const SOUND_SPEED: f64 = 343.0;
const LEVEL: f64 = 0.42;
const EAR_ANGLES: [f64; 7] = [0.0, 30.0, 60.0, 90.0, 120.0, 150.0, 180.0];
const EAR_LEVELS: [f64; 7] = [0.817, 0.697, 0.900, 0.540, 0.286, 0.199, 0.204];
const REAR_ANGLES: [f64; 3] = [90.0, 135.0, 180.0];
const REAR_LEVELS: [f64; 3] = [1.0, 0.72, 0.632];
const ALPHA_STEP: f64 = 0.005;
const ALPHA_STEPS: usize = 800;
const TAPS: usize = 8;
const PHASES: usize = 64;
const LOOKAHEAD: f32 = (TAPS / 2) as f32;

fn sinc_table() -> Vec<[f32; TAPS]> {
    (0..=PHASES)
        .map(|phase| {
            let frac = phase as f64 / PHASES as f64;
            let mut taps = [0.0f64; TAPS];
            for (k, tap) in taps.iter_mut().enumerate() {
                let x = (TAPS / 2 - 1) as f64 - k as f64 + frac;
                let sinc = if x.abs() < 1e-12 { 1.0 } else { (PI * x).sin() / (PI * x) };
                let half = TAPS as f64 / 2.0;
                let window = 0.42 + 0.5 * (PI * x / half).cos() + 0.08 * (2.0 * PI * x / half).cos();
                *tap = sinc * window;
            }
            let sum: f64 = taps.iter().sum();
            taps.map(|t| (t / sum) as f32)
        })
        .collect()
}

fn interpolate(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let x = x.clamp(xs[0], xs[xs.len() - 1]);
    let i = xs.partition_point(|&v| v <= x).clamp(1, xs.len() - 1);
    let t = (x - xs[i - 1]) / (xs[i] - xs[i - 1]);
    ys[i - 1] + (ys[i] - ys[i - 1]) * t
}

pub struct HrtfModel {
    rate: f64,
    k: f64,
    a1: f32,
    rms: Vec<f64>,
    max_delay: f64,
    sinc: Vec<[f32; TAPS]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Ear {
    delay: f32,
    b0: f32,
    b1: f32,
}

impl HrtfModel {
    pub fn new(rate: f64) -> HrtfModel {
        let k = rate * HEAD_RADIUS / SOUND_SPEED;
        let a1 = (1.0 - k) / (1.0 + k);
        let n = 512;
        let grid: Vec<(f64, f64, f64)> = (0..n)
            .map(|j| {
                let w = PI * (j as f64 + 0.5) / n as f64;
                let (c, s) = (w.cos(), w.sin());
                (c, s, (1.0 + a1 * c).powi(2) + (a1 * s).powi(2))
            })
            .collect();
        let rms = (0..=ALPHA_STEPS)
            .map(|i| {
                let alpha = i as f64 * ALPHA_STEP;
                let (b0, b1) = ((1.0 + alpha * k) / (1.0 + k), (1.0 - alpha * k) / (1.0 + k));
                let power: f64 = grid.iter().map(|&(c, s, den)| ((b0 + b1 * c).powi(2) + (b1 * s).powi(2)) / den).sum::<f64>() / n as f64;
                power.sqrt()
            })
            .collect();
        let max_delay = HEAD_RADIUS / SOUND_SPEED * (PI / 2.0 + 1.0) * rate;
        HrtfModel { rate, k, a1: a1 as f32, rms, max_delay, sinc: sinc_table() }
    }

    fn alpha_for(&self, level: f64) -> f64 {
        let target = level / LEVEL;
        let i = self.rms.partition_point(|&r| r < target).clamp(1, self.rms.len() - 1);
        let (r0, r1) = (self.rms[i - 1], self.rms[i]);
        let t = if r1 > r0 { ((target - r0) / (r1 - r0)).clamp(0.0, 1.0) } else { 0.0 };
        (i as f64 - 1.0 + t) * ALPHA_STEP
    }

    fn ears(&self, azimuth: f64, elevation: f64, gain: f64) -> [Ear; 2] {
        let finite = azimuth.is_finite() && elevation.is_finite();
        let (az, el) = if finite { (azimuth.to_radians(), elevation.to_radians()) } else { (0.0, 0.0) };
        let gain = if gain.is_finite() { gain } else { 0.0 };
        let lateral = (az.sin() * el.cos()).clamp(-1.0, 1.0);
        let front = (az.cos() * el.cos()).clamp(-1.0, 1.0);
        let rear = interpolate(&REAR_ANGLES, &REAR_LEVELS, front.acos().to_degrees());
        let psi = lateral.asin();
        let itd = HEAD_RADIUS / SOUND_SPEED * (psi + psi.sin()) * self.rate;
        let base = self.max_delay / 2.0;
        [(-lateral, base + itd / 2.0), (lateral, base - itd / 2.0)].map(|(toward, delay)| {
            let angle = toward.acos().to_degrees();
            let alpha = self.alpha_for(interpolate(&EAR_ANGLES, &EAR_LEVELS, angle) * rear);
            let scale = LEVEL * gain / (1.0 + self.k);
            Ear { delay: delay as f32 + LOOKAHEAD, b0: (scale * (1.0 + alpha * self.k)) as f32, b1: (scale * (1.0 - alpha * self.k)) as f32 }
        })
    }
}

pub struct Hrtf {
    lines: [Vec<f32>; 2],
    write: usize,
    x1: [f32; 2],
    y1: [f32; 2],
    last: Option<[Ear; 2]>,
}

impl Hrtf {
    pub fn new(model: &HrtfModel) -> Hrtf {
        let size = ((model.max_delay.ceil() as usize) + 2 * TAPS).next_power_of_two();
        Hrtf { lines: [vec![0.0; size], vec![0.0; size]], write: 0, x1: [0.0; 2], y1: [0.0; 2], last: None }
    }

    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.x1 = [0.0; 2];
        self.y1 = [0.0; 2];
        self.last = None;
    }

    pub fn process(&mut self, model: &HrtfModel, spot: &Spot, input: &[[f32; QUANTUM]; 2], mono: bool, out: &mut [[f32; QUANTUM]; 2]) {
        let next = model.ears(spot.azimuth, spot.elevation, spot.gain);
        let prev = self.last.unwrap_or(next);
        self.last = Some(next);
        let mask = self.lines[0].len() - 1;
        let a1 = model.a1;
        for i in 0..QUANTUM {
            let t = (i + 1) as f32 / QUANTUM as f32;
            let w = self.write;
            for ear in 0..2 {
                let source = if mono { 0 } else { ear };
                self.lines[ear][w] = input[source][i];
                let (p, n) = (prev[ear], next[ear]);
                let delay = p.delay + (n.delay - p.delay) * t;
                let b0 = p.b0 + (n.b0 - p.b0) * t;
                let b1 = p.b1 + (n.b1 - p.b1) * t;
                let back = delay.floor();
                let phase = (((delay - back) * PHASES as f32).round() as usize).min(PHASES);
                let taps = &model.sinc[phase];
                let newest = w.wrapping_sub(back as usize).wrapping_add(TAPS / 2 - 1);
                let line = &self.lines[ear];
                let x: f32 = taps.iter().enumerate().map(|(k, h)| line[newest.wrapping_sub(k) & mask] * h).sum();
                let x = if x.is_finite() { x } else { 0.0 };
                let y = b0 * x + b1 * self.x1[ear] - a1 * self.y1[ear];
                let y = if y.is_finite() { y } else { 0.0 };
                self.x1[ear] = x;
                self.y1[ear] = y;
                out[ear][i] = y;
            }
            self.write = (w + 1) & mask;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(a: &[f32]) -> f64 {
        (a.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / a.len() as f64).sqrt()
    }

    fn noise(n: usize) -> Vec<f32> {
        let mut seed = 1u32;
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (f64::from(seed) / 4_294_967_296.0 * 2.0 - 1.0) as f32 * 0.5
            })
            .collect()
    }

    fn render_hrtf(azimuth: f64, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let model = HrtfModel::new(48000.0);
        let mut h = Hrtf::new(&model);
        let spot = Spot { azimuth, elevation: 0.0, gain: 1.0 };
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for chunk in input.chunks_exact(QUANTUM) {
            let mut bus = [[0.0; QUANTUM]; 2];
            bus[0].copy_from_slice(chunk);
            let mut out = [[0.0; QUANTUM]; 2];
            h.process(&model, &spot, &bus, true, &mut out);
            l.extend_from_slice(&out[0]);
            r.extend_from_slice(&out[1]);
        }
        (l, r)
    }

    fn lag(a: &[f32], b: &[f32]) -> i32 {
        (-48..=48)
            .max_by(|&x, &y| {
                let score = |k: i32| (4096..20000).map(|i| f64::from(a[i]) * f64::from(b[(i as i32 + k) as usize])).sum::<f64>();
                score(x).total_cmp(&score(y))
            })
            .unwrap_or(0)
    }

    #[test]
    fn hrtf_levels_and_delays_follow_the_measured_ones() {
        let input = noise(QUANTUM * 375);
        let level = rms(&input[2048..]);
        for (azimuth, left, right, itd) in
            [(0.0, 0.54, 0.54, 0), (90.0, 0.205, 0.819, -36), (-90.0, 0.817, 0.203, 36), (30.0, 0.286, 0.900, -13), (180.0, 0.341, 0.341, 0)]
        {
            let (l, r) = render_hrtf(azimuth, &input);
            let (gl, gr) = (rms(&l[2048..]) / level, rms(&r[2048..]) / level);
            assert!((gl - left).abs() < 0.05 && (gr - right).abs() < 0.05, "{azimuth}: {gl} {gr}");
            let measured = lag(&l, &r);
            assert!((measured - itd).abs() <= 3, "{azimuth}: itd {measured}");
        }
    }

    #[test]
    fn building_the_model_is_cheap_enough_for_the_render_thread() {
        let start = std::time::Instant::now();
        let model = HrtfModel::new(48000.0);
        let took = start.elapsed();
        assert_eq!(model.rms.len(), ALPHA_STEPS + 1);
        assert!(model.rms.iter().all(|rms| rms.is_finite() && *rms > 0.0));
        assert!((model.rms[(1.0 / ALPHA_STEP).round() as usize] - 1.0).abs() < 1e-9);
        assert!(!model.sinc.is_empty());
        if cfg!(not(debug_assertions)) {
            assert!(took < std::time::Duration::from_millis(3), "{took:?}");
        }
    }

    #[test]
    fn hrtf_never_produces_nan() {
        let model = HrtfModel::new(48000.0);
        let mut h = Hrtf::new(&model);
        let input = [[1.0; QUANTUM]; 2];
        let mut out = [[0.0; QUANTUM]; 2];
        for (az, el) in [(f64::NAN, 0.0), (0.0, 90.0), (180.0, -90.0), (1e30, 1e30)] {
            h.process(&model, &Spot { azimuth: az, elevation: el, gain: 1.0 }, &input, true, &mut out);
            assert!(out.iter().flatten().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn degenerate_geometry_is_front() {
        let l = Listener::default();
        assert_eq!(azimuth_elevation(V3::default(), &l), (0.0, 0.0));
        let flat = Listener { forward: V3::new(0.0, 1.0, 0.0), ..Listener::default() };
        assert_eq!(azimuth_elevation(V3::new(1.0, 0.0, 0.0), &flat), (0.0, 0.0));
        let (az, el) = azimuth_elevation(V3::new(0.0, 5.0, 0.0), &l);
        assert!(az.is_finite() && (el - 90.0).abs() < 1e-9, "{az} {el}");
    }
}
