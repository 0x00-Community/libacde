use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::graph::hermite;
use super::mixer::{Mixer, RATE};
use super::output::{Ctl, Idle, Web, control, deliver, lock};
use crate::host::rt::Msg;

const CHUNK: usize = 256;

pub struct Device {
    device: cpal::Device,
    config: cpal::StreamConfig,
}

impl Device {
    pub fn open() -> Result<Device, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no default output device")?;
        let default = device.default_output_config().map_err(|e| format!("output config: {e}"))?;
        let (rate, channels) = (default.sample_rate(), default.channels());
        if default.sample_format() != cpal::SampleFormat::F32 {
            let f32 = device.supported_output_configs().map_err(|e| format!("output configs: {e}"))?.any(|c| {
                c.sample_format() == cpal::SampleFormat::F32 && c.channels() == channels && (c.min_sample_rate()..=c.max_sample_rate()).contains(&rate)
            });
            if !f32 {
                return Err(format!("the output takes {} samples, not f32", default.sample_format()));
            }
        }
        let name = device.description().map(|d| d.name().to_owned()).unwrap_or_else(|_| "?".into());
        log::info!("sound: {name} ({} host), {rate} Hz, {channels} channels; pages render at {RATE} Hz", host.id().name());
        Ok(Device { device, config: cpal::StreamConfig { channels, sample_rate: rate, buffer_size: cpal::BufferSize::Default } })
    }

    pub fn run(self, ctl: mpsc::Sender<Ctl>, rx: mpsc::Receiver<Ctl>) {
        let mut webs = HashMap::new();
        while let Ok(message) = rx.recv() {
            control(&mut webs, message, |id, web| self.stream(id, web, &ctl));
        }
    }

    fn stream(&self, id: u64, web: &Web<cpal::Stream>, ctl: &mpsc::Sender<Ctl>) -> Result<cpal::Stream, String> {
        let mut feed = Feed {
            mixer: web.mixer.clone(),
            notify: web.notify.clone(),
            ctl: ctl.clone(),
            resampler: Resampler::new(usize::from(self.config.channels), RATE / f64::from(self.config.sample_rate)),
            idle: Idle::new(id),
            missed: 0,
        };
        let name = web.name.clone();
        let stream = self
            .device
            .build_output_stream(
                self.config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| feed.fill(out),
                move |e| log::warn!("[{name}] sound output: {e}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        lock(&web.mixer).set_rendering(true);
        if let Err(e) = stream.play() {
            lock(&web.mixer).set_rendering(false);
            return Err(e.to_string());
        }
        Ok(stream)
    }
}

struct Feed {
    mixer: Arc<Mutex<Mixer>>,
    notify: mpsc::Sender<Msg>,
    ctl: mpsc::Sender<Ctl>,
    resampler: Resampler,
    idle: Idle,
    missed: usize,
}

impl Feed {
    fn fill(&mut self, out: &mut [f32]) {
        let Ok(mut mixer) = self.mixer.try_lock() else {
            out.fill(0.0);
            self.missed += out.len() / self.resampler.channels;
            return;
        };
        mixer.missed(std::mem::take(&mut self.missed));
        let pulled = self.resampler.fill(out, |chunk| mixer.render(chunk));
        deliver(&mut mixer, &self.notify);
        let active = mixer.active();
        drop(mixer);
        self.idle.tick(active, pulled, |ctl| self.ctl.send(ctl).is_ok());
    }
}

pub(super) struct Resampler {
    channels: usize,
    step: f64,
    pos: f64,
    taps: [[f32; 2]; 4],
    chunk: Vec<f32>,
    next: usize,
}

impl Resampler {
    pub(super) fn new(channels: usize, step: f64) -> Resampler {
        Resampler { channels: channels.max(1), step, pos: 0.0, taps: [[0.0; 2]; 4], chunk: vec![0.0; CHUNK * 2], next: CHUNK }
    }

    fn pull(&mut self, render: &mut impl FnMut(&mut [f32])) {
        if self.next == CHUNK {
            render(&mut self.chunk);
            self.next = 0;
        }
        let frame = [self.chunk[self.next * 2], self.chunk[self.next * 2 + 1]];
        self.next += 1;
        self.taps.rotate_left(1);
        self.taps[3] = frame;
    }

    pub(super) fn fill(&mut self, out: &mut [f32], mut render: impl FnMut(&mut [f32])) -> usize {
        let mut pulled = 0;
        for frame in out.chunks_mut(self.channels) {
            while self.pos >= 1.0 {
                self.pull(&mut render);
                pulled += 1;
                self.pos -= 1.0;
            }
            let t = self.pos as f32;
            let [a, b, c, d] = self.taps;
            let left = hermite(a[0], b[0], c[0], d[0], t);
            let right = hermite(a[1], b[1], c[1], d[1], t);
            match frame {
                [mono] => *mono = 0.5 * (left + right),
                [l, r, rest @ ..] => {
                    *l = left;
                    *r = right;
                    rest.fill(0.0);
                }
                [] => {}
            }
            self.pos += self.step;
        }
        pulled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(start: &mut f32) -> impl FnMut(&mut [f32]) + '_ {
        move |chunk: &mut [f32]| {
            for frame in chunk.chunks_mut(2) {
                frame[0] = *start;
                frame[1] = -*start;
                *start += 1.0;
            }
        }
    }

    #[test]
    fn same_rate_passes_frames_through() {
        let mut next = 0.0;
        let mut resampler = Resampler::new(2, 1.0);
        let mut out = vec![0.0; 20];
        let pulled = resampler.fill(&mut out, ramp(&mut next));
        assert_eq!(pulled, 9);
        assert_eq!(&out[8..12], &[1.0, -1.0, 2.0, -2.0]);
    }

    #[test]
    fn slower_device_consumes_source_at_the_ratio() {
        let mut next = 0.0;
        let mut resampler = Resampler::new(2, 48000.0 / 44100.0);
        let mut out = vec![0.0; 44100 * 2];
        let pulled = resampler.fill(&mut out, ramp(&mut next));
        assert!((47998..=48001).contains(&pulled), "{pulled}");
        let tail = &out[out.len() - 2..];
        assert!((tail[0] - (pulled as f32 - 3.0)).abs() < 1.5 && (tail[0] + tail[1]).abs() < 1e-2, "{pulled} {tail:?}");
    }

    #[test]
    fn extra_channels_are_silent_and_mono_mixes() {
        let mut next = 0.0;
        let mut four = Resampler::new(4, 1.0);
        let mut out = vec![9.0; 16];
        four.fill(&mut out, ramp(&mut next));
        assert!(out.chunks(4).all(|f| f[2] == 0.0 && f[3] == 0.0));
        let mut mono = Resampler::new(1, 1.0);
        let mut out = vec![9.0; 4];
        mono.fill(&mut out, ramp(&mut next));
        assert!(out.iter().all(|v| *v == 0.0));
    }
}
