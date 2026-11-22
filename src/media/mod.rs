#[cfg(target_os = "macos")]
mod apple;
pub mod capture;
#[cfg(target_os = "linux")]
mod gio_stream;
#[cfg(target_os = "linux")]
mod gst;

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;

#[cfg(target_os = "macos")]
pub const TYPES: &[&str] = &["video/mp4", "video/quicktime", "video/x-m4v", "audio/mp4", "audio/x-m4a"];
#[cfg(target_os = "linux")]
pub const TYPES: &[&str] =
    &["video/mp4", "video/quicktime", "video/x-m4v", "audio/mp4", "audio/x-m4a", "video/webm", "audio/webm", "video/ogg", "video/x-matroska"];
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub const TYPES: &[&str] = &[];

const AUDIO_AHEAD: usize = RATE as usize * CHANNELS / 2;
const VIDEO_AHEAD: usize = 4;

#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub audio: bool,
    pub video: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pcm {
    pub time: f64,
    pub samples: Vec<f32>,
}

pub enum Source {
    Bytes(Arc<[u8]>),
    File(std::fs::File),
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Source {
    pub(crate) fn len(&self) -> u64 {
        match self {
            Source::Bytes(bytes) => bytes.len() as u64,
            Source::File(file) => file.metadata().map_or(0, |m| m.len()),
        }
    }

    pub(crate) fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Source::Bytes(bytes) => {
                let rest = bytes.get(usize::try_from(offset).unwrap_or(usize::MAX)..).unwrap_or_default();
                let n = rest.len().min(buf.len());
                buf[..n].copy_from_slice(&rest[..n]);
                Ok(n)
            }
            Source::File(file) => std::os::unix::fs::FileExt::read_at(file, buf, offset),
        }
    }

    pub(crate) fn fill_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.read_at(offset + filled as u64, &mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(filled)
    }

    fn head(&self) -> Vec<u8> {
        let mut head = vec![0; 12];
        let n = self.fill_at(0, &mut head).unwrap_or(0);
        head.truncate(n);
        head
    }
}

impl From<Arc<[u8]>> for Source {
    fn from(bytes: Arc<[u8]>) -> Self {
        Source::Bytes(bytes)
    }
}

impl From<std::fs::File> for Source {
    fn from(file: std::fs::File) -> Self {
        Source::File(file)
    }
}

pub enum Pull<T> {
    Item(T),
    Wait,
    End,
    Failed(String),
}

pub(crate) trait Backend: Send + Sync {
    fn restart(&self, from: f64) -> Result<(), String>;
    fn next_video(&self) -> Pull<Frame>;
    fn next_audio(&self) -> Pull<Pcm>;
    fn stop(&self);
}

#[derive(Default)]
struct State {
    frames: VecDeque<Frame>,
    audio: VecDeque<f32>,
    audio_time: f64,
    video_done: bool,
    audio_done: bool,
    error: Option<String>,
    seek: Option<f64>,
    parked: usize,
    epoch: u64,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    backend: Box<dyn Backend>,
    readers: usize,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Player {
    shared: Arc<Shared>,
    info: Info,
    threads: Vec<std::thread::JoinHandle<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Track {
    Video,
    Audio,
}

impl Player {
    pub fn open(source: impl Into<Source>) -> Result<Player, String> {
        let (backend, info) = open_backend(source.into())?;
        let readers = usize::from(info.video) + usize::from(info.audio);
        let state = State { video_done: !info.video, audio_done: !info.audio, ..State::default() };
        let shared = Arc::new(Shared { state: Mutex::new(state), wake: Condvar::new(), backend, readers });
        let mut threads = Vec::new();
        for (track, wanted) in [(Track::Video, info.video), (Track::Audio, info.audio)] {
            if !wanted {
                continue;
            }
            let reader = shared.clone();
            let name = if track == Track::Video { "acde-video" } else { "acde-video-audio" };
            match std::thread::Builder::new().name(name.into()).spawn(move || read(&reader, track)) {
                Ok(thread) => threads.push(thread),
                Err(e) => {
                    shared.backend.stop();
                    return Err(format!("no decoding thread: {e}"));
                }
            }
        }
        Ok(Player { shared, info, threads })
    }

    pub fn info(&self) -> &Info {
        &self.info
    }

    pub fn seek(&self, seconds: f64) {
        let mut state = lock(&self.shared.state);
        state.frames.clear();
        state.audio.clear();
        state.audio_time = seconds;
        state.video_done = !self.info.video;
        state.audio_done = !self.info.audio;
        state.error = None;
        state.epoch += 1;
        if self.shared.readers > 0 {
            state.seek = Some(seconds);
            state.parked = 0;
        }
        self.shared.wake.notify_all();
    }

    pub fn epoch(&self) -> u64 {
        lock(&self.shared.state).epoch
    }

    pub fn audio_time(&self) -> f64 {
        lock(&self.shared.state).audio_time
    }

    pub fn take_audio(&self, out: &mut [f32]) -> usize {
        let mut state = lock(&self.shared.state);
        if state.seek.is_some() {
            return 0;
        }
        let n = out.len().min(state.audio.len()) / CHANNELS * CHANNELS;
        for (slot, sample) in out.iter_mut().zip(state.audio.drain(..n)) {
            *slot = sample;
        }
        state.audio_time += (n / CHANNELS) as f64 / f64::from(RATE);
        if n > 0 {
            self.shared.wake.notify_all();
        }
        n
    }

    pub fn audio_drained(&self) -> bool {
        let state = lock(&self.shared.state);
        state.seek.is_none() && state.audio_done && state.audio.is_empty()
    }

    pub fn frame_at(&self, time: f64) -> Option<Frame> {
        let mut state = lock(&self.shared.state);
        if state.seek.is_some() {
            return None;
        }
        let mut due = None;
        while state.frames.front().is_some_and(|f| f.time <= time + 0.001) {
            due = state.frames.pop_front();
        }
        if due.is_some() {
            self.shared.wake.notify_all();
        }
        due
    }

    pub fn first_frame(&self, timeout: Duration) -> Option<Frame> {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = lock(&self.shared.state);
        loop {
            if state.seek.is_none() {
                if let Some(frame) = state.frames.front() {
                    return Some(frame.clone());
                }
                if state.video_done || state.error.is_some() {
                    return None;
                }
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return None;
            }
            state = self.shared.wake.wait_timeout(state, left).map_or_else(|e| e.into_inner().0, |(s, _)| s);
        }
    }

    pub fn video_drained(&self) -> bool {
        let state = lock(&self.shared.state);
        state.seek.is_none() && state.video_done && state.frames.is_empty()
    }

    pub fn error(&self) -> Option<String> {
        lock(&self.shared.state).error.clone()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        {
            let mut state = lock(&self.shared.state);
            state.stop = true;
            self.shared.wake.notify_all();
        }
        let (shared, threads) = (self.shared.clone(), std::mem::take(&mut self.threads));
        let stopping = std::thread::Builder::new().name("acde-video-stop".into()).spawn(move || {
            shared.backend.stop();
            for thread in threads {
                _ = thread.join();
            }
        });
        if let Err(e) = stopping {
            log::warn!("media: stopping inline ({e})");
            self.shared.backend.stop();
        }
    }
}

fn read(shared: &Shared, track: Track) {
    loop {
        let epoch = {
            let mut state = lock(&shared.state);
            loop {
                if state.stop {
                    return;
                }
                if let Some(target) = state.seek {
                    state.parked += 1;
                    if state.parked == shared.readers {
                        drop(state);
                        let restarted = shared.backend.restart(target);
                        state = lock(&shared.state);
                        if state.seek == Some(target) {
                            state.seek = None;
                            state.parked = 0;
                            if let Err(e) = restarted {
                                state.error = Some(e);
                            }
                        }
                        shared.wake.notify_all();
                    } else {
                        let seen = state.epoch;
                        while state.seek.is_some() && state.epoch == seen && !state.stop {
                            state = shared.wake.wait(state).unwrap_or_else(|e| e.into_inner());
                        }
                        if state.seek.is_some() && state.epoch != seen {
                            continue;
                        }
                    }
                    continue;
                }
                let full = match track {
                    Track::Video => state.video_done || state.frames.len() >= VIDEO_AHEAD,
                    Track::Audio => state.audio_done || state.audio.len() >= AUDIO_AHEAD,
                };
                if !full && state.error.is_none() {
                    break state.epoch;
                }
                state = shared.wake.wait_timeout(state, Duration::from_millis(200)).map_or_else(|e| e.into_inner().0, |(s, _)| s);
            }
        };
        let pulled = match track {
            Track::Video => shared.backend.next_video().map(Pulled::Video),
            Track::Audio => shared.backend.next_audio().map(Pulled::Audio),
        };
        let mut state = lock(&shared.state);
        if state.epoch != epoch || state.seek.is_some() {
            continue;
        }
        match pulled {
            Pull::Item(Pulled::Video(frame)) => state.frames.push_back(frame),
            Pull::Item(Pulled::Audio(pcm)) => {
                if state.audio.is_empty() {
                    state.audio_time = state.audio_time.max(pcm.time);
                }
                state.audio.extend(pcm.samples);
            }
            Pull::Wait => {}
            Pull::End => match track {
                Track::Video => state.video_done = true,
                Track::Audio => state.audio_done = true,
            },
            Pull::Failed(e) => {
                log::warn!("media: decoding failed: {e}");
                state.error = Some(e);
                state.video_done = true;
                state.audio_done = true;
            }
        }
        shared.wake.notify_all();
    }
}

enum Pulled {
    Video(Frame),
    Audio(Pcm),
}

impl<T> Pull<T> {
    fn map<U>(self, f: impl FnOnce(T) -> U) -> Pull<U> {
        match self {
            Pull::Item(t) => Pull::Item(f(t)),
            Pull::Wait => Pull::Wait,
            Pull::End => Pull::End,
            Pull::Failed(e) => Pull::Failed(e),
        }
    }
}

#[cfg(target_os = "macos")]
fn open_backend(source: Source) -> Result<(Box<dyn Backend>, Info), String> {
    apple::open(source)
}

#[cfg(target_os = "linux")]
fn open_backend(source: Source) -> Result<(Box<dyn Backend>, Info), String> {
    gst::open(source)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn open_backend(_: Source) -> Result<(Box<dyn Backend>, Info), String> {
    Err("video playback is not supported on this platform".into())
}
