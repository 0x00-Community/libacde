use std::collections::VecDeque;
use std::sync::Arc;

use crate::gpu::peniko;
use crate::gpu::{Gpu, GpuImage, wgpu};
use crate::media::{CHANNELS, Frame, Player};

use super::graph::{Bus, QUANTUM, hermite};
use super::mixer::{MediaSource, RATE};

pub struct VideoSound {
    player: Arc<Player>,
    duration: f64,
    queue: VecDeque<[f32; 2]>,
    frac: f64,
    chunk: Vec<f32>,
    clock: Option<f64>,
}

impl VideoSound {
    pub fn new(player: Arc<Player>) -> VideoSound {
        let info = player.info();
        let (duration, audio) = (info.duration, info.audio);
        VideoSound { player, duration, queue: VecDeque::new(), frac: 0.0, chunk: vec![0.0; 1024 * CHANNELS], clock: (!audio).then_some(0.0) }
    }

    fn fill(&mut self) -> bool {
        while self.queue.len() < 4 {
            let n = self.player.take_audio(&mut self.chunk);
            if n == 0 {
                return false;
            }
            self.queue.extend(self.chunk[..n].as_chunks::<CHANNELS>().0.iter().copied());
        }
        true
    }

    fn finished(&self, time: f64) -> bool {
        if self.duration.is_finite() && self.duration > 0.0 { time >= self.duration } else { self.player.video_drained() }
    }
}

impl MediaSource for VideoSound {
    fn time(&self) -> f64 {
        match self.clock {
            Some(clock) => clock,
            None => self.player.audio_time() - (self.queue.len() as f64 - self.frac) / RATE,
        }
    }

    fn seek(&mut self, seconds: f64) {
        self.player.seek(seconds);
        self.queue.clear();
        self.frac = 0.0;
        self.clock = (!self.player.info().audio).then_some(seconds);
    }

    fn render(&mut self, out: &mut Bus, out_rate: f64, speed: f64, gain: f32, looping: bool) -> bool {
        if let Some(clock) = self.clock {
            let next = clock + QUANTUM as f64 * speed.max(0.0) / out_rate;
            if self.finished(next) {
                if looping {
                    self.seek(0.0);
                    return false;
                }
                self.clock = Some(if self.duration.is_finite() { self.duration.max(clock) } else { next });
                return true;
            }
            self.clock = Some(next);
            return false;
        }
        let step = speed.max(0.0) * RATE / out_rate;
        let [left, right] = out;
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            if !self.fill() && self.queue.len() < 4 {
                if self.player.audio_drained() {
                    let now = self.time();
                    if !self.finished(now) {
                        self.clock = Some(now);
                        return false;
                    }
                    if looping {
                        self.seek(0.0);
                        return false;
                    }
                    return true;
                }
                return false;
            }
            let t = self.frac as f32;
            let (a, b, c, d) = (self.queue[0], self.queue[1], self.queue[2], self.queue[3]);
            *left += gain * hermite(a[0], b[0], c[0], d[0], t);
            *right += gain * hermite(a[1], b[1], c[1], d[1], t);
            self.frac += step;
            while self.frac >= 1.0 && !self.queue.is_empty() {
                self.queue.pop_front();
                self.frac -= 1.0;
            }
        }
        false
    }
}

pub struct Picture {
    node: u32,
    gpu: Option<Arc<Gpu>>,
    image: Option<Arc<GpuImage>>,
    pub size: Option<(u32, u32)>,
}

impl Picture {
    pub fn new(node: u32, gpu: Option<Arc<Gpu>>) -> Picture {
        Picture { node, gpu, image: None, size: None }
    }

    pub fn black(&mut self) -> bool {
        let Some((width, height)) = self.size else { return false };
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 0xff;
        }
        self.show(&Frame { time: 0.0, width, height, rgba })
    }

    pub fn show(&mut self, frame: &Frame) -> bool {
        let size = (frame.width, frame.height);
        if size.0 == 0 || size.1 == 0 || frame.rgba.len() < (size.0 * size.1 * 4) as usize {
            return false;
        }
        let data = match &self.gpu {
            Some(gpu) => {
                let image = match &self.image {
                    Some(image) if (image.texture.width(), image.texture.height()) == size => image.clone(),
                    _ => {
                        let texture = crate::gpu::texture(&gpu.device, size, wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC, "acde video");
                        GpuImage::new(texture, &gpu.images)
                    }
                };
                gpu.write(&image, &frame.rgba, size, [0, 0], [0, 0, size.0, size.1]);
                image.painted();
                self.image = Some(image.clone());
                image.image()
            }
            None => peniko::ImageData {
                data: peniko::Blob::new(Arc::new(frame.rgba.clone())),
                format: peniko::ImageFormat::Rgba8,
                alpha_type: peniko::ImageAlphaType::Alpha,
                width: size.0,
                height: size.1,
            },
        };
        if let Some(Err(e)) = crate::host::dom::with_dom(|dom| dom.video_image(self.node, Some(data))) {
            log::debug!("video: node {}: {e}", self.node);
        }
        let resized = self.size != Some(size);
        self.size = Some(size);
        resized
    }

    pub fn clear(&mut self) {
        self.image = None;
        self.size = None;
        if let Some(Err(e)) = crate::host::dom::with_dom(|dom| dom.video_image(self.node, None)) {
            log::debug!("video: node {}: {e}", self.node);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::host::audio::mixer::{Cmd, MediaOp, Mixer, Note};

    fn mixer_with(name: &str) -> (Mixer, Arc<AtomicU64>) {
        let bytes = std::fs::read(format!("{}/tests/fixtures/media/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let player = Arc::new(Player::open(bytes.into()).unwrap());
        player.first_frame(std::time::Duration::from_secs(5)).unwrap();
        let mut mixer = Mixer::default();
        let time = Arc::new(AtomicU64::new(0));
        mixer.apply(Cmd::AddMedia { media: 7, stream: Box::new(VideoSound::new(player)), time: time.clone() });
        (mixer, time)
    }

    fn play_out(mixer: &mut Mixer, seconds: f64) -> (bool, f32) {
        let mut out = vec![0.0; 2 * 480];
        let mut peak = 0.0f32;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds);
        while std::time::Instant::now() < deadline {
            mixer.render(&mut out);
            peak = out.iter().fold(peak, |a, v| a.max(v.abs()));
            if mixer.take_notes().contains(&Note::MediaEnded(7)) {
                return (true, peak);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        (false, peak)
    }

    fn seconds(time: &AtomicU64) -> f64 {
        f64::from_bits(time.load(Ordering::Acquire))
    }

    #[test]
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), ignore = "acde-media has no video decoder on this platform")]
    fn a_video_s_sound_plays_to_the_end() {
        let (mut mixer, time) = mixer_with("clip.mp4");
        mixer.apply(Cmd::Media { media: 7, op: MediaOp::Play });
        let (ended, peak) = play_out(&mut mixer, 10.0);
        assert!(ended, "never ended, at {}", seconds(&time));
        assert!(peak > 0.05, "peak {peak}");
        assert!((seconds(&time) - 1.0).abs() < 0.05, "ended at {}", seconds(&time));
    }

    #[test]
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), ignore = "acde-media has no video decoder on this platform")]
    fn a_silent_video_runs_on_its_own_clock() {
        let (mut mixer, time) = mixer_with("silent-rotated.mp4");
        mixer.apply(Cmd::Media { media: 7, op: MediaOp::Play });
        let (ended, peak) = play_out(&mut mixer, 10.0);
        assert!(ended, "never ended, at {}", seconds(&time));
        assert_eq!(peak, 0.0);
        assert!(seconds(&time) > 0.5, "ended at {}", seconds(&time));
    }

    #[test]
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), ignore = "acde-media has no video decoder on this platform")]
    fn seeking_moves_the_media_clock() {
        let (mut mixer, time) = mixer_with("clip.mp4");
        mixer.apply(Cmd::Media { media: 7, op: MediaOp::Seek(0.5) });
        assert!((seconds(&time) - 0.5).abs() < 1e-9, "{}", seconds(&time));
        mixer.apply(Cmd::Media { media: 7, op: MediaOp::Play });
        let (ended, _) = play_out(&mut mixer, 10.0);
        assert!(ended);
        assert!((seconds(&time) - 1.0).abs() < 0.05, "ended at {}", seconds(&time));
    }
}
