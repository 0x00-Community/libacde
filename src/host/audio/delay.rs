use super::graph::{Bus, QUANTUM};

pub struct Delay {
    lines: [Vec<f32>; 2],
    write: usize,
    max: f64,
}

impl Delay {
    pub fn new(max_seconds: f64, rate: f64) -> Delay {
        let len = (max_seconds * rate).ceil() as usize + 2 * QUANTUM + 1;
        Delay { lines: [vec![0.0; len], vec![0.0; len]], write: 0, max: max_seconds }
    }

    pub fn max_seconds(&self) -> f64 {
        self.max
    }

    pub fn tail_frames(&self, rate: f64) -> u64 {
        (self.max * rate).ceil() as u64 + QUANTUM as u64
    }

    pub fn write(&mut self, input: &Bus, mono: bool) {
        let len = self.lines[0].len();
        for c in 0..2 {
            let from = if mono { 0 } else { c };
            for i in 0..QUANTUM {
                self.lines[c][(self.write + i) % len] = input[from][i];
            }
        }
    }

    pub fn advance(&mut self) {
        self.write = (self.write + QUANTUM) % self.lines[0].len();
    }

    pub fn read(&self, delays: &[f32; QUANTUM], minimum: f64, rate: f64, out: &mut Bus) {
        let len = self.lines[0].len();
        for i in 0..QUANTUM {
            let frames = (f64::from(delays[i]).clamp(0.0, self.max) * rate).max(minimum);
            let position = (self.write + i) as f64 + len as f64 - frames;
            let first = position.floor();
            let fraction = (position - first) as f32;
            let (a, b) = (first as usize % len, (first as usize + 1) % len);
            for c in 0..2 {
                let line = &self.lines[c];
                out[c][i] = line[a] + (line[b] - line[a]) * fraction;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delay_reads_what_was_written_that_many_frames_before() {
        let rate = 32768.0;
        let mut d = Delay::new(1.0, rate);
        let mut input = [[0.0; QUANTUM]; 2];
        input[0][5] = 1.0;
        let mut out = [[0.0; QUANTUM]; 2];
        d.write(&input, true);
        d.read(&[64.0 / 32768.0; QUANTUM], 0.0, rate, &mut out);
        d.advance();
        assert_eq!((out[0][69], out[1][69]), (1.0, 1.0), "2^-9 s is exactly 64 frames at 2^15 Hz");
        assert_eq!(out[0].iter().sum::<f32>(), 1.0);
        d.write(&[[0.0; QUANTUM]; 2], true);
        d.read(&[64.5 / 32768.0; QUANTUM], 0.0, rate, &mut out);
        assert!(out[0].iter().all(|v| *v == 0.0), "the impulse has passed");
    }
}
