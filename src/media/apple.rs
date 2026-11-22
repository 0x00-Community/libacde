use std::sync::mpsc::sync_channel;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetReferenceRestrictions, AVAssetResourceLoader, AVAssetResourceLoaderDelegate,
    AVAssetResourceLoadingRequest, AVAssetTrack, AVAsynchronousKeyValueLoading, AVMediaTypeAudio, AVMediaTypeVideo, AVURLAsset,
    AVURLAssetAllowsCellularAccessKey, AVURLAssetAllowsConstrainedNetworkAccessKey, AVURLAssetAllowsExpensiveNetworkAccessKey,
    AVURLAssetReferenceRestrictionsKey,
};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeRange, kCMTimePositiveInfinity};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSError, NSNumber, NSString, NSURL};

use crate::media::{Backend, Frame, Info, Pcm, Pull, RATE, Source};

pub(crate) const BGRA: u32 = u32::from_be_bytes(*b"BGRA");
pub(crate) const LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
const CHUNK: u64 = 4 << 20;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "AcdeMediaLoader"]
    #[ivars = (Source, Retained<NSString>)]
    struct Loader;

    unsafe impl NSObjectProtocol for Loader {}

    unsafe impl AVAssetResourceLoaderDelegate for Loader {
        #[unsafe(method(resourceLoader:shouldWaitForLoadingOfRequestedResource:))]
        fn should_wait(&self, _loader: &AVAssetResourceLoader, request: &AVAssetResourceLoadingRequest) -> bool {
            let (source, kind) = self.ivars();
            let length = source.len();
            unsafe {
                if let Some(info) = request.contentInformationRequest() {
                    info.setContentType(Some(kind));
                    info.setContentLength(length as i64);
                    info.setByteRangeAccessSupported(true);
                }
                if let Some(data) = request.dataRequest() {
                    let start = data.requestedOffset().max(0) as u64;
                    let wanted = if data.requestsAllDataToEndOfResource() { length.saturating_sub(start) } else { data.requestedLength().max(0) as u64 };
                    let mut chunk = vec![0; wanted.min(CHUNK) as usize];
                    match source.fill_at(start, &mut chunk) {
                        Ok(n) => data.respondWithData(&NSData::with_bytes(&chunk[..n])),
                        Err(e) => log::warn!("media: reading the source failed: {e}"),
                    }
                }
                request.finishLoading();
            }
            true
        }
    }
);

impl Loader {
    fn new(source: Source, kind: Retained<NSString>) -> Retained<Self> {
        let this = Self::alloc().set_ivars((source, kind));
        unsafe { msg_send![super(this), init] }
    }
}

fn container(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Ok(if &bytes[8..12] == b"qt  " { "com.apple.quicktime-movie" } else { "public.mpeg-4" });
    }
    Err("AVFoundation plays MP4 and QuickTime files; this one is neither".into())
}

#[derive(Clone, Copy)]
pub(crate) enum Turn {
    None,
    Quarter,
    Half,
    ThreeQuarters,
}

struct Tracks {
    reader: Option<Retained<AVAssetReader>>,
}

pub(crate) struct Apple {
    asset: Retained<AVURLAsset>,
    _loader: Retained<Loader>,
    _queue: DispatchRetained<DispatchQueue>,
    video_track: Option<Retained<AVAssetTrack>>,
    audio_track: Option<Retained<AVAssetTrack>>,
    turn: Turn,
    tracks: Mutex<Tracks>,
    video: Mutex<Option<Retained<AVAssetReaderTrackOutput>>>,
    audio: Mutex<Option<Retained<AVAssetReaderTrackOutput>>>,
}

unsafe impl Send for Apple {}
unsafe impl Sync for Apple {}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn number(value: impl Into<f64>) -> Retained<AnyObject> {
    let n = NSNumber::new_f64(value.into());
    Retained::into_super(Retained::into_super(Retained::into_super(n)))
}

pub(crate) fn settings(pairs: &[(&str, Retained<AnyObject>)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = pairs.iter().map(|(k, _)| NSString::from_str(k)).collect();
    let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    let values: Vec<Retained<AnyObject>> = pairs.iter().map(|(_, v)| v.clone()).collect();
    NSDictionary::from_retained_objects(&keys, &values)
}

struct Sendable(Retained<AVAssetTrack>);

unsafe impl Send for Sendable {}

fn first_track(asset: &AVURLAsset, kind: Option<&'static NSString>) -> Result<Option<Retained<AVAssetTrack>>, String> {
    let Some(kind) = kind else { return Ok(None) };
    let (tx, rx) = sync_channel(1);
    let done = RcBlock::new(move |tracks: *mut NSArray<AVAssetTrack>, error: *mut NSError| {
        let result = match unsafe { tracks.as_ref() } {
            Some(tracks) => Ok(tracks.firstObject().map(Sendable)),
            None => Err(unsafe { error.as_ref() }.map_or_else(|| "unknown".to_string(), |e| e.localizedDescription().to_string())),
        };
        _ = tx.try_send(result);
    });
    unsafe { asset.loadTracksWithMediaType_completionHandler(kind, &done) };
    let loaded = rx.recv_timeout(Duration::from_secs(20)).map_err(|_| "AVFoundation did not finish reading the file".to_string())?;
    loaded.map(|track| track.map(|t| t.0)).map_err(|why| format!("AVFoundation cannot read the file: {why}"))
}

fn seconds(time: CMTime) -> f64 {
    unsafe { time.seconds() }
}

pub(crate) fn open(source: Source) -> Result<(Box<dyn Backend>, Info), String> {
    let kind = container(&source.head())?;
    let loader = Loader::new(source, NSString::from_str(kind));
    let queue = DispatchQueue::new("acde.media.loader", None);
    let url = NSURL::URLWithString(&NSString::from_str("acde-media://media/source")).ok_or("no media URL")?;
    let options = NSDictionary::from_retained_objects(
        &[
            unsafe { AVURLAssetReferenceRestrictionsKey },
            unsafe { AVURLAssetAllowsCellularAccessKey },
            unsafe { AVURLAssetAllowsExpensiveNetworkAccessKey },
            unsafe { AVURLAssetAllowsConstrainedNetworkAccessKey },
        ],
        &[number(AVAssetReferenceRestrictions::ForbidAll.0 as f64), number(0), number(0), number(0)],
    );
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, Some(&options)) };
    unsafe { asset.resourceLoader().setDelegate_queue(Some(ProtocolObject::from_ref(&*loader)), Some(&queue)) };
    let video_track = first_track(&asset, unsafe { AVMediaTypeVideo })?;
    let audio_track = first_track(&asset, unsafe { AVMediaTypeAudio })?;
    let keys = NSArray::from_retained_slice(&[NSString::from_str("duration")]);
    let (tx, rx) = sync_channel(1);
    let done = RcBlock::new(move || _ = tx.try_send(()));
    unsafe { asset.loadValuesAsynchronouslyForKeys_completionHandler(&keys, Some(&done)) };
    rx.recv_timeout(Duration::from_secs(20)).map_err(|_| "AVFoundation did not finish reading the file".to_string())?;
    if video_track.is_none() && audio_track.is_none() {
        return Err("the file has neither video nor audio".into());
    }
    let (mut width, mut height, mut turn) = (0, 0, Turn::None);
    if let Some(track) = &video_track {
        let size = unsafe { track.naturalSize() };
        let t = unsafe { track.preferredTransform() };
        turn = match (t.a.round() as i32, t.b.round() as i32, t.c.round() as i32, t.d.round() as i32) {
            (0, 1, -1, 0) => Turn::Quarter,
            (-1, 0, 0, -1) => Turn::Half,
            (0, -1, 1, 0) => Turn::ThreeQuarters,
            _ => Turn::None,
        };
        (width, height) = (size.width.round() as u32, size.height.round() as u32);
        if matches!(turn, Turn::Quarter | Turn::ThreeQuarters) {
            (width, height) = (height, width);
        }
    }
    let duration = seconds(unsafe { asset.duration() });
    let info =
        Info { duration: if duration.is_finite() { duration } else { f64::NAN }, width, height, audio: audio_track.is_some(), video: video_track.is_some() };
    let backend = Apple {
        asset,
        _loader: loader,
        _queue: queue,
        video_track,
        audio_track,
        turn,
        tracks: Mutex::new(Tracks { reader: None }),
        video: Mutex::new(None),
        audio: Mutex::new(None),
    };
    backend.restart(0.0)?;
    Ok((Box::new(backend), info))
}

impl Apple {
    fn output(
        reader: &AVAssetReader,
        track: &AVAssetTrack,
        settings: &NSDictionary<NSString, AnyObject>,
    ) -> Result<Retained<AVAssetReaderTrackOutput>, String> {
        let output = unsafe { AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(track, Some(settings)) };
        unsafe { output.setAlwaysCopiesSampleData(false) };
        if !unsafe { reader.canAddOutput(&output) } {
            return Err("AVFoundation cannot decode this track".into());
        }
        unsafe { reader.addOutput(&output) };
        Ok(output)
    }

    fn failure(&self) -> String {
        let tracks = lock(&self.tracks);
        let error = tracks.reader.as_ref().and_then(|r| unsafe { r.error() });
        error.map_or_else(|| "decoding stopped".to_string(), |e| e.localizedDescription().to_string())
    }

    fn reader_failed(&self) -> bool {
        lock(&self.tracks).reader.as_ref().is_some_and(|r| unsafe { r.status() } == AVAssetReaderStatus::Failed)
    }

    fn sample(&self, output: &Mutex<Option<Retained<AVAssetReaderTrackOutput>>>) -> Pull<Retained<CMSampleBuffer>> {
        let output = lock(output);
        let Some(output) = output.as_ref() else { return Pull::End };
        match unsafe { output.copyNextSampleBuffer() } {
            Some(sample) => Pull::Item(sample),
            None if self.reader_failed() => Pull::Failed(self.failure()),
            None => Pull::End,
        }
    }

    fn frame(&self, sample: &CMSampleBuffer) -> Option<Frame> {
        frame(sample, self.turn)
    }
}

pub(crate) fn frame(sample: &CMSampleBuffer, turn: Turn) -> Option<Frame> {
    let time = seconds(unsafe { sample.presentation_time_stamp() });
    let image = unsafe { sample.image_buffer() }?;
    let pixels: &CVPixelBuffer = &image;
    unsafe { CVPixelBufferLockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly) };
    let (w, h, stride) = (CVPixelBufferGetWidth(pixels), CVPixelBufferGetHeight(pixels), CVPixelBufferGetBytesPerRow(pixels));
    let base = CVPixelBufferGetBaseAddress(pixels) as *const u8;
    let frame = (!base.is_null()).then(|| {
        let source = unsafe { std::slice::from_raw_parts(base, stride * h) };
        let (out_w, out_h) = if matches!(turn, Turn::Quarter | Turn::ThreeQuarters) { (h, w) } else { (w, h) };
        let mut rgba = vec![0u8; out_w * out_h * 4];
        for y in 0..h {
            let row = &source[y * stride..y * stride + w * 4];
            for x in 0..w {
                let (ox, oy) = match turn {
                    Turn::None => (x, y),
                    Turn::Quarter => (h - 1 - y, x),
                    Turn::Half => (w - 1 - x, h - 1 - y),
                    Turn::ThreeQuarters => (y, w - 1 - x),
                };
                let px = &row[x * 4..x * 4 + 4];
                let at = (oy * out_w + ox) * 4;
                rgba[at..at + 4].copy_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        Frame { time, width: out_w as u32, height: out_h as u32, rgba }
    });
    unsafe { CVPixelBufferUnlockBaseAddress(pixels, CVPixelBufferLockFlags::ReadOnly) };
    frame
}

pub(crate) fn lpcm(sample: &CMSampleBuffer) -> Result<Vec<f32>, String> {
    let Some(block) = (unsafe { sample.data_buffer() }) else { return Ok(Vec::new()) };
    let length = unsafe { block.data_length() };
    let mut samples = vec![0f32; length / 4];
    let status = unsafe { block.copy_data_bytes(0, samples.len() * 4, std::ptr::NonNull::new(samples.as_mut_ptr().cast()).expect("a vector's pointer")) };
    if status != 0 {
        return Err(format!("audio samples could not be read ({status})"));
    }
    Ok(samples)
}

impl Backend for Apple {
    fn restart(&self, from: f64) -> Result<(), String> {
        let mut tracks = lock(&self.tracks);
        let mut video = lock(&self.video);
        let mut audio = lock(&self.audio);
        if let Some(old) = tracks.reader.take() {
            unsafe { old.cancelReading() };
        }
        *video = None;
        *audio = None;
        let reader = unsafe { AVAssetReader::assetReaderWithAsset_error(&self.asset) }.map_err(|e| e.localizedDescription().to_string())?;
        if let Some(track) = &self.video_track {
            *video = Some(Self::output(&reader, track, &settings(&[("PixelFormatType", number(BGRA))]))?);
        }
        if let Some(track) = &self.audio_track {
            let pcm = settings(&[
                ("AVFormatIDKey", number(LINEAR_PCM)),
                ("AVSampleRateKey", number(RATE)),
                ("AVNumberOfChannelsKey", number(2)),
                ("AVLinearPCMBitDepthKey", number(32)),
                ("AVLinearPCMIsFloatKey", number(1)),
                ("AVLinearPCMIsNonInterleaved", number(0)),
                ("AVLinearPCMIsBigEndianKey", number(0)),
            ]);
            *audio = Some(Self::output(&reader, track, &pcm)?);
        }
        let start = unsafe { CMTime::with_seconds(from.max(0.0), 90_000) };
        unsafe { reader.setTimeRange(CMTimeRange::new(start, kCMTimePositiveInfinity)) };
        if !unsafe { reader.startReading() } {
            let why = unsafe { reader.error() }.map_or_else(|| "unknown".to_string(), |e| e.localizedDescription().to_string());
            return Err(format!("AVFoundation cannot start decoding: {why}"));
        }
        tracks.reader = Some(reader);
        Ok(())
    }

    fn next_video(&self) -> Pull<Frame> {
        loop {
            match self.sample(&self.video) {
                Pull::Item(sample) => match self.frame(&sample) {
                    Some(frame) => return Pull::Item(frame),
                    None => continue,
                },
                Pull::Wait => return Pull::Wait,
                Pull::End => return Pull::End,
                Pull::Failed(e) => return Pull::Failed(e),
            }
        }
    }

    fn next_audio(&self) -> Pull<Pcm> {
        match self.sample(&self.audio) {
            Pull::Item(sample) => {
                let time = seconds(unsafe { sample.presentation_time_stamp() });
                match lpcm(&sample) {
                    Ok(samples) if samples.is_empty() => Pull::Wait,
                    Ok(samples) => Pull::Item(Pcm { time, samples }),
                    Err(e) => Pull::Failed(e),
                }
            }
            Pull::Wait => Pull::Wait,
            Pull::End => Pull::End,
            Pull::Failed(e) => Pull::Failed(e),
        }
    }

    fn stop(&self) {
        if let Some(reader) = lock(&self.tracks).reader.take() {
            unsafe { reader.cancelReading() };
        }
    }
}
