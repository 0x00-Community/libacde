use std::f32::consts::FRAC_PI_2;

use super::graph::{Bus, QUANTUM};

const PRE_DELAY: f32 = 0.006;
const MAX_PRE_DELAY_FRAMES: usize = 1024;
const METERING_RELEASE: f64 = 0.325;
const SAT_RELEASE: f32 = 0.0025;
const DIVISION: usize = 32;
const ZONES: [f32; 4] = [0.09, 0.16, 0.42, 0.98];

fn polynomial(c: [f32; 4]) -> f32 {
    c[0] * ZONES[0] + c[1] * ZONES[1] + c[2] * ZONES[2] + c[3] * ZONES[3]
}

fn db_to_linear(db: f32) -> f32 {
    10f32.powf(0.05 * db)
}

fn linear_to_db(linear: f32) -> f32 {
    20.0 * linear.log10()
}

fn finite(x: f32, default: f32) -> f32 {
    if x.is_finite() { x } else { default }
}

struct Curve {
    db_threshold: f32,
    db_knee: f32,
    ratio: f32,
    linear_threshold: f32,
    slope: f32,
    knee_threshold: f32,
    db_knee_threshold: f32,
    db_yknee_threshold: f32,
    k: f32,
}

impl Curve {
    fn knee(&self, x: f32, k: f32) -> f32 {
        if x < self.linear_threshold {
            return x;
        }
        self.linear_threshold + (1.0 - (f64::from(-k * (x - self.linear_threshold))).exp() as f32) / k
    }

    fn saturate(&self, x: f32, k: f32) -> f32 {
        if x < self.knee_threshold {
            return self.knee(x, k);
        }
        let db_y = self.db_yknee_threshold + self.slope * (linear_to_db(x) - self.db_knee_threshold);
        db_to_linear(db_y)
    }

    fn k_at_slope(&self, desired: f32) -> f32 {
        let db_x = self.db_threshold + self.db_knee;
        let x = db_to_linear(db_x);
        let (mut x2, mut db_x2) = (1.0f32, 0.0f32);
        if x >= self.linear_threshold {
            x2 = (f64::from(x) * 1.001) as f32;
            db_x2 = linear_to_db(x2);
        }
        let (mut min_k, mut max_k, mut k) = (0.1f32, 10000.0f32, 5.0f32);
        let mut slope = 1.0f32;
        for _ in 0..15 {
            if x >= self.linear_threshold {
                let db_y = linear_to_db(self.knee(x, k));
                let db_y2 = linear_to_db(self.knee(x2, k));
                slope = (db_y2 - db_y) / (db_x2 - db_x);
            }
            if slope < desired {
                max_k = k;
            } else {
                min_k = k;
            }
            k = (min_k * max_k).sqrt();
        }
        k
    }

    fn update(&mut self, db_threshold: f32, db_knee: f32, ratio: f32) -> f32 {
        if db_threshold != self.db_threshold || db_knee != self.db_knee || ratio != self.ratio {
            self.db_threshold = db_threshold;
            self.linear_threshold = db_to_linear(db_threshold);
            self.db_knee = db_knee;
            self.ratio = ratio;
            self.slope = 1.0 / ratio;
            let k = self.k_at_slope(1.0 / ratio);
            self.db_knee_threshold = db_threshold + db_knee;
            self.knee_threshold = db_to_linear(self.db_knee_threshold);
            self.db_yknee_threshold = linear_to_db(self.knee(self.knee_threshold, k));
            self.k = k;
        }
        self.k
    }
}

pub struct Compressor {
    curve: Curve,
    pre_delay: [[f32; MAX_PRE_DELAY_FRAMES]; 2],
    read: usize,
    write: usize,
    last_pre_delay: Option<usize>,
    detector_average: f32,
    compressor_gain: f32,
    metering_gain: f32,
    max_attack_diff: f32,
    metering_release: f32,
}

impl Compressor {
    pub const TAIL_SECONDS: f64 = 5.0 * METERING_RELEASE;

    pub fn new(rate: f64) -> Compressor {
        let unset = -1.0;
        Compressor {
            curve: Curve {
                db_threshold: unset,
                db_knee: unset,
                ratio: unset,
                linear_threshold: unset,
                slope: unset,
                knee_threshold: unset,
                db_knee_threshold: unset,
                db_yknee_threshold: unset,
                k: unset,
            },
            pre_delay: [[0.0; MAX_PRE_DELAY_FRAMES]; 2],
            read: 0,
            write: 256,
            last_pre_delay: None,
            detector_average: 0.0,
            compressor_gain: 1.0,
            metering_gain: 1.0,
            max_attack_diff: -1.0,
            metering_release: (1.0 - (-1.0 / (rate * METERING_RELEASE)).exp()) as f32,
        }
    }

    pub fn reduction(&self) -> f32 {
        self.metering_gain
    }

    pub fn process(&mut self, input: &Bus, mono: bool, params: [f32; 5], rate: f64, out: &mut Bus) {
        let [db_threshold, db_knee, ratio, attack, release] = params;
        let rate = rate as f32;
        let k = self.curve.update(db_threshold, db_knee, ratio);
        let post_gain = (1.0 / self.curve.saturate(1.0, k)).powf(0.6);
        let attack_frames = attack.max(0.001) * rate;
        let release_frames = rate * release;
        let sat_release_frames = SAT_RELEASE * rate;
        let a = release_frames * polynomial([0.999_999_999_999_999_8, 1.843_221_968_432_392_3e-16, -1.937_339_435_167_642_3e-16, 8.824_516_011_816_245e-18]);
        let b = release_frames * polynomial([-1.578_832_035_284_588_8, 2.330_583_703_207_428_6, -0.914_119_420_484_042_9, 0.162_367_752_561_203_2]);
        let c = release_frames * polynomial([0.533_414_286_910_642_4, -1.272_736_789_213_631, 0.925_885_604_220_751_2, -0.186_563_101_917_762_26]);
        let d = release_frames * polynomial([0.087_834_631_382_072_34, -0.169_416_296_792_562_2, 0.085_880_579_515_952_72, -0.004_298_914_105_462_83]);
        let e = release_frames * polynomial([-0.042_416_883_008_123_074, 0.111_569_382_798_760_2, -0.097_646_763_252_658_72, 0.028_494_263_462_021_576]);
        let pre_delay_frames = ((PRE_DELAY * rate) as usize).min(MAX_PRE_DELAY_FRAMES - 1);
        if self.last_pre_delay != Some(pre_delay_frames) {
            self.last_pre_delay = Some(pre_delay_frames);
            self.pre_delay = [[0.0; MAX_PRE_DELAY_FRAMES]; 2];
            self.read = 0;
            self.write = pre_delay_frames;
        }
        let mask = MAX_PRE_DELAY_FRAMES - 1;
        let mut frame = 0;
        while frame < QUANTUM {
            self.detector_average = finite(self.detector_average, 1.0);
            let desired = self.detector_average;
            let scaled_desired = desired.asin() / FRAC_PI_2;
            let releasing = scaled_desired > self.compressor_gain;
            let mut diff = if scaled_desired == 0.0 { if releasing { -1.0 } else { 1.0 } } else { linear_to_db(self.compressor_gain / scaled_desired) };
            let envelope_rate = if releasing {
                self.max_attack_diff = -1.0;
                diff = finite(diff, -1.0);
                let x = 0.25 * (diff.clamp(-12.0, 0.0) + 12.0);
                let (x2, x3) = (x * x, x * x * x);
                let frames = a + b * x + c * x2 + d * x3 + e * x2 * x2;
                db_to_linear(5.0 / frames)
            } else {
                diff = finite(diff, 1.0);
                if self.max_attack_diff == -1.0 || self.max_attack_diff < diff {
                    self.max_attack_diff = diff;
                }
                let effective = self.max_attack_diff.max(0.5);
                1.0 - (0.25 / effective).powf(1.0 / attack_frames)
            };
            let (mut detector_average, mut compressor_gain) = (self.detector_average, self.compressor_gain);
            for _ in 0..DIVISION.min(QUANTUM - frame) {
                let mut level = 0.0f32;
                for ch in 0..2 {
                    let source = input[if mono { 0 } else { ch }][frame];
                    self.pre_delay[ch][self.write] = source;
                    level = level.max(source.abs());
                }
                let shaped = self.curve.saturate(level, k);
                let attenuation = if level <= 0.0001 { 1.0 } else { shaped / level };
                let db_attenuation = 2.0f32.max(-linear_to_db(attenuation));
                let sat_release_rate = db_to_linear(db_attenuation / sat_release_frames) - 1.0;
                let rate = if attenuation > detector_average { sat_release_rate } else { 1.0 };
                detector_average += (attenuation - detector_average) * rate;
                detector_average = finite(detector_average.min(1.0), 1.0);
                if envelope_rate < 1.0 {
                    compressor_gain += (scaled_desired - compressor_gain) * envelope_rate;
                } else {
                    compressor_gain = (compressor_gain * envelope_rate).min(1.0);
                }
                let warped = (f64::from(FRAC_PI_2 * compressor_gain)).sin() as f32;
                let total = post_gain * warped;
                let db_real = linear_to_db(warped);
                if db_real < self.metering_gain {
                    self.metering_gain = db_real;
                } else {
                    self.metering_gain += (db_real - self.metering_gain) * self.metering_release;
                }
                for ch in 0..2 {
                    out[ch][frame] = self.pre_delay[ch][self.read] * total;
                }
                frame += 1;
                self.read = (self.read + 1) & mask;
                self.write = (self.write + 1) & mask;
            }
            self.detector_average = flush(detector_average);
            self.compressor_gain = flush(compressor_gain);
        }
    }
}

fn flush(v: f32) -> f32 {
    if v.is_subnormal() { 0.0 } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loud_tone_is_turned_down_and_the_reduction_shows_it() {
        let rate = 48000.0;
        let mut compressor = Compressor::new(rate);
        let params = [-24.0, 30.0, 12.0, 0.003, 0.25];
        let mut out = [[0.0; QUANTUM]; 2];
        let mut peak = 0.0f32;
        for q in 0..375 {
            let mut input = [[0.0; QUANTUM]; 2];
            for i in 0..QUANTUM {
                input[0][i] = (((q * QUANTUM + i) as f32) * 2.0 * std::f32::consts::PI * 440.0 / 48000.0).sin();
            }
            compressor.process(&input, true, params, rate, &mut out);
            if q > 300 {
                peak = out[0].iter().fold(peak, |m, v| m.max(v.abs()));
            }
        }
        assert!(peak < 0.8 && peak > 0.2, "{peak}");
        assert!(compressor.reduction() < -3.0, "{}", compressor.reduction());
        assert_eq!(out[0], out[1], "mono input reaches both channels");
    }
}
