use super::graph::{Bus, QUANTUM};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FilterType {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
    Lowshelf,
    Highshelf,
    Peaking,
    Notch,
    Allpass,
}

impl FilterType {
    pub fn from_code(code: u32) -> FilterType {
        match code {
            1 => FilterType::Highpass,
            2 => FilterType::Bandpass,
            3 => FilterType::Lowshelf,
            4 => FilterType::Highshelf,
            5 => FilterType::Peaking,
            6 => FilterType::Notch,
            7 => FilterType::Allpass,
            _ => FilterType::Lowpass,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Coefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coefficients {
    fn normalized(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Coefficients {
        let k = 1.0 / a0;
        Coefficients { b0: b0 * k, b1: b1 * k, b2: b2 * k, a1: a1 * k, a2: a2 * k }
    }

    fn gain(g: f64) -> Coefficients {
        Coefficients { b0: g, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 }
    }

    pub fn of(kind: FilterType, frequency: f32, detune: f32, q: f32, gain: f32, rate: f64) -> Coefficients {
        let nyquist = rate / 2.0;
        let mut f = f64::from(frequency);
        if detune != 0.0 {
            f *= (f64::from(detune) / 1200.0).exp2();
        }
        let cutoff = (f / nyquist).clamp(0.0, 1.0);
        let (q, db) = (f64::from(q), f64::from(gain));
        let w0 = std::f64::consts::PI * cutoff;
        let (sin, cos) = w0.sin_cos();
        match kind {
            FilterType::Lowpass if cutoff >= 1.0 => Self::gain(1.0),
            FilterType::Lowpass if cutoff <= 0.0 => Self::gain(0.0),
            FilterType::Lowpass => {
                let alpha = 0.5 * sin * 10f64.powf(-0.05 * q);
                let b1 = 1.0 - cos;
                Self::normalized(0.5 * b1, b1, 0.5 * b1, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            FilterType::Highpass if cutoff >= 1.0 => Self::gain(0.0),
            FilterType::Highpass if cutoff <= 0.0 => Self::gain(1.0),
            FilterType::Highpass => {
                let alpha = 0.5 * sin * 10f64.powf(-0.05 * q);
                let b1 = -1.0 - cos;
                Self::normalized(-0.5 * b1, b1, -0.5 * b1, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            FilterType::Bandpass if cutoff <= 0.0 || cutoff >= 1.0 => Self::gain(0.0),
            FilterType::Bandpass if q <= 0.0 => Self::gain(1.0),
            FilterType::Bandpass => {
                let alpha = sin / (2.0 * q);
                Self::normalized(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            FilterType::Notch if cutoff <= 0.0 || cutoff >= 1.0 => Self::gain(1.0),
            FilterType::Notch if q <= 0.0 => Self::gain(0.0),
            FilterType::Notch => {
                let alpha = sin / (2.0 * q);
                Self::normalized(1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            FilterType::Allpass if cutoff <= 0.0 || cutoff >= 1.0 => Self::gain(1.0),
            FilterType::Allpass if q <= 0.0 => Self::gain(-1.0),
            FilterType::Allpass => {
                let alpha = sin / (2.0 * q);
                Self::normalized(1.0 - alpha, -2.0 * cos, 1.0 + alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            FilterType::Peaking => {
                let a = 10f64.powf(db / 40.0);
                if cutoff <= 0.0 || cutoff >= 1.0 {
                    return Self::gain(1.0);
                }
                if q <= 0.0 {
                    return Self::gain(a * a);
                }
                let alpha = sin / (2.0 * q);
                Self::normalized(1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
            }
            FilterType::Lowshelf | FilterType::Highshelf => {
                let a = 10f64.powf(db / 40.0);
                let low = kind == FilterType::Lowshelf;
                if cutoff >= 1.0 {
                    return Self::gain(if low { a * a } else { 1.0 });
                }
                if cutoff <= 0.0 {
                    return Self::gain(if low { 1.0 } else { a * a });
                }
                let alpha = 0.5 * sin * 2f64.sqrt();
                let k2 = 2.0 * a.sqrt() * alpha;
                let (plus, minus) = (a + 1.0, a - 1.0);
                if low {
                    Self::normalized(
                        a * (plus - minus * cos + k2),
                        2.0 * a * (minus - plus * cos),
                        a * (plus - minus * cos - k2),
                        plus + minus * cos + k2,
                        -2.0 * (minus + plus * cos),
                        plus + minus * cos - k2,
                    )
                } else {
                    Self::normalized(
                        a * (plus + minus * cos + k2),
                        -2.0 * a * (minus + plus * cos),
                        a * (plus + minus * cos - k2),
                        plus - minus * cos + k2,
                        2.0 * (minus - plus * cos),
                        plus - minus * cos - k2,
                    )
                }
            }
        }
    }
}

#[derive(Default)]
pub struct Biquad {
    pub kind: FilterType,
    state: [[f64; 4]; 2],
}

impl Biquad {
    pub fn process(&mut self, input: &Bus, mono: bool, values: &[[f32; QUANTUM]], still: bool, rate: f64, out: &mut Bus) {
        let at = |i: usize| Coefficients::of(self.kind, values[0][i], values[1][i], values[2][i], values[3][i], rate);
        let fixed = still.then(|| at(0));
        let mut coefficients = [fixed.unwrap_or_else(|| at(0)); QUANTUM];
        if fixed.is_none() {
            for (i, c) in coefficients.iter_mut().enumerate().skip(1) {
                *c = at(i);
            }
        }
        for c in 0..if mono { 1 } else { 2 } {
            let [mut x1, mut x2, mut y1, mut y2] = self.state[c];
            for i in 0..QUANTUM {
                let k = &coefficients[i];
                let x = f64::from(input[c][i]);
                let y = k.b0 * x + k.b1 * x1 + k.b2 * x2 - k.a1 * y1 - k.a2 * y2;
                (x2, x1, y2, y1) = (x1, x, y1, flush(y));
                out[c][i] = y as f32;
            }
            self.state[c] = [x1, x2, y1, y2];
        }
        if mono {
            self.state[1] = self.state[0];
        }
    }
}

fn flush(v: f64) -> f64 {
    if v.abs() < 1e-30 || !v.is_finite() { 0.0 } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(c: &Coefficients, f: f64, rate: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * f / rate;
        let (z1r, z1i) = (w.cos(), -w.sin());
        let (z2r, z2i) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let (nr, ni) = (c.b0 + c.b1 * z1r + c.b2 * z2r, c.b1 * z1i + c.b2 * z2i);
        let (dr, di) = (1.0 + c.a1 * z1r + c.a2 * z2r, c.a1 * z1i + c.a2 * z2i);
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }

    #[test]
    fn each_type_shapes_the_spectrum_as_its_name_says() {
        let rate = 48000.0;
        let lp = Coefficients::of(FilterType::Lowpass, 1000.0, 0.0, 0.0, 0.0, rate);
        assert!((response(&lp, 20.0, rate) - 1.0).abs() < 1e-3 && response(&lp, 10000.0, rate) < 0.02);
        let hp = Coefficients::of(FilterType::Highpass, 1000.0, 0.0, 0.0, 0.0, rate);
        assert!(response(&hp, 20.0, rate) < 1e-3 && (response(&hp, 20000.0, rate) - 1.0).abs() < 1e-2);
        let peak = Coefficients::of(FilterType::Peaking, 2000.0, 0.0, 1.0, 12.0, rate);
        assert!((20.0 * response(&peak, 2000.0, rate).log10() - 12.0).abs() < 1e-6);
        let notch = Coefficients::of(FilterType::Notch, 3000.0, 0.0, 5.0, 0.0, rate);
        assert!(response(&notch, 3000.0, rate) < 1e-9);
        let shelf = Coefficients::of(FilterType::Lowshelf, 500.0, 0.0, 1.0, -6.0, rate);
        assert!((20.0 * response(&shelf, 10.0, rate).log10() + 6.0).abs() < 0.05);
        let detuned = Coefficients::of(FilterType::Bandpass, 1000.0, 1200.0, 1.0, 0.0, rate);
        assert_eq!(detuned, Coefficients::of(FilterType::Bandpass, 2000.0, 0.0, 1.0, 0.0, rate));
    }

    #[test]
    fn edge_frequencies_and_qs_follow_chrome() {
        let rate = 48000.0;
        assert_eq!(Coefficients::of(FilterType::Lowpass, 0.0, 0.0, 1.0, 0.0, rate), Coefficients::gain(0.0));
        assert_eq!(Coefficients::of(FilterType::Lowpass, 30000.0, 0.0, 1.0, 0.0, rate), Coefficients::gain(1.0));
        assert_eq!(Coefficients::of(FilterType::Bandpass, 1000.0, 0.0, 0.0, 0.0, rate), Coefficients::gain(1.0));
        assert_eq!(Coefficients::of(FilterType::Allpass, 1000.0, 0.0, -1.0, 0.0, rate), Coefficients::gain(-1.0));
        assert!((Coefficients::of(FilterType::Highshelf, 0.0, 0.0, 1.0, 6.0, rate).b0 - 10f64.powf(6.0 / 20.0)).abs() < 1e-12);
    }
}
