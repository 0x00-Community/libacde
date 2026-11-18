use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureAudioDataOutput, AVCaptureAudioDataOutputSampleBufferDelegate, AVCaptureConnection, AVCaptureDevice,
    AVCaptureDeviceDiscoverySession, AVCaptureDeviceInput, AVCaptureDevicePosition, AVCaptureDeviceType, AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureDeviceTypeContinuityCamera, AVCaptureDeviceTypeExternal, AVCaptureDeviceTypeMicrophone, AVCaptureOutput, AVCaptureSession,
    AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaType, AVMediaTypeAudio, AVMediaTypeVideo,
};
use objc2_core_media::{CMSampleBuffer, CMVideoFormatDescriptionGetDimensions};
use objc2_foundation::NSArray;

use super::{Device, Error, Feed, Kind, Mode, Source, stable_id};
use crate::media::apple::{BGRA, LINEAR_PCM, Turn, frame, lpcm, number, settings};
use crate::media::{CHANNELS, Frame, RATE, lock};

struct Ivars {
    feed: Arc<Feed>,
    kind: Kind,
    size: Option<(u32, u32)>,
    start: Instant,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "AcdeCaptureDelegate"]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for Delegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn output(&self, _output: &AVCaptureOutput, sample: &CMSampleBuffer, _connection: &AVCaptureConnection) {
            let ivars = self.ivars();
            match ivars.kind {
                Kind::Camera => {
                    if let Some(captured) = frame(sample, Turn::None)
                        && ivars.size.is_none_or(|size| size == (captured.width, captured.height))
                    {
                        ivars.feed.frame(Frame { time: ivars.start.elapsed().as_secs_f64(), ..captured });
                    }
                }
                Kind::Microphone => match lpcm(sample) {
                    Ok(samples) => ivars.feed.audio(&samples[..samples.len() / CHANNELS * CHANNELS]),
                    Err(e) => log::warn!("capture: {e}"),
                },
            }
        }
    }

    unsafe impl AVCaptureAudioDataOutputSampleBufferDelegate for Delegate {}
);

impl Delegate {
    fn new(feed: Arc<Feed>, kind: Kind, size: Option<(u32, u32)>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { feed, kind, size, start: Instant::now() });
        unsafe { msg_send![super(this), init] }
    }
}

fn media_type(kind: Kind) -> &'static AVMediaType {
    unsafe {
        match kind {
            Kind::Camera => AVMediaTypeVideo.expect("AVMediaTypeVideo"),
            Kind::Microphone => AVMediaTypeAudio.expect("AVMediaTypeAudio"),
        }
    }
}

fn captured(kind: Kind) -> Vec<Retained<AVCaptureDevice>> {
    let types: Vec<&AVCaptureDeviceType> = unsafe {
        match kind {
            Kind::Camera => vec![AVCaptureDeviceTypeBuiltInWideAngleCamera, AVCaptureDeviceTypeExternal, AVCaptureDeviceTypeContinuityCamera],
            Kind::Microphone => vec![AVCaptureDeviceTypeMicrophone],
        }
    };
    let types = NSArray::from_slice(&types);
    let session = unsafe {
        AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &types,
            Some(media_type(kind)),
            AVCaptureDevicePosition::Unspecified,
        )
    };
    unsafe { session.devices() }.to_vec()
}

fn modes(device: &AVCaptureDevice) -> Vec<Mode> {
    let mut modes: Vec<Mode> = Vec::new();
    for format in unsafe { device.formats() }.iter() {
        let description = unsafe { format.formatDescription() };
        let size = unsafe { CMVideoFormatDescriptionGetDimensions(&description) };
        let frame_rate = unsafe { format.videoSupportedFrameRateRanges() }.iter().map(|r| unsafe { r.maxFrameRate() }).fold(0.0, f64::max);
        let (width, height) = (size.width.max(0) as u32, size.height.max(0) as u32);
        if width == 0 || height > width {
            continue;
        }
        match modes.iter_mut().find(|m| (m.width, m.height) == (width, height)) {
            Some(mode) => mode.frame_rate = mode.frame_rate.max(frame_rate),
            None => modes.push(Mode { width, height, frame_rate }),
        }
    }
    modes.sort_by_key(|m| std::cmp::Reverse(m.width * m.height));
    modes
}

fn describe(device: &AVCaptureDevice, kind: Kind) -> Device {
    let unique = unsafe { device.uniqueID() }.to_string();
    let built_in = unsafe { &*device.deviceType() == AVCaptureDeviceTypeBuiltInWideAngleCamera };
    let facing = match unsafe { device.position() } {
        AVCaptureDevicePosition::Front => Some("user"),
        AVCaptureDevicePosition::Back => Some("environment"),
        _ if kind == Kind::Camera && built_in => Some("user"),
        _ => None,
    };
    Device {
        id: stable_id(kind, &unique),
        kind,
        label: unsafe { device.localizedName() }.to_string(),
        group: stable_id(Kind::Microphone, &unsafe { device.modelID() }.to_string()),
        facing,
        modes: if kind == Kind::Camera { modes(device) } else { Vec::new() },
        channels: if kind == Kind::Microphone { 1 } else { 0 },
    }
}

pub fn devices() -> Vec<Device> {
    [Kind::Microphone, Kind::Camera].into_iter().flat_map(|kind| captured(kind).into_iter().map(move |d| describe(&d, kind))).collect()
}

pub fn permission(kind: Kind) -> super::Permission {
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type(kind)) } {
        AVAuthorizationStatus::Authorized => super::Permission::Granted,
        AVAuthorizationStatus::NotDetermined => super::Permission::Prompt,
        _ => super::Permission::Denied,
    }
}

pub fn authorize(kind: Kind) -> Result<(), Error> {
    let what = if kind == Kind::Camera { "camera" } else { "microphone" };
    let denied = || Error::NotAllowed(format!("macOS does not let acde use the {what} (System Settings > Privacy & Security)"));
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type(kind)) } {
        AVAuthorizationStatus::Authorized => Ok(()),
        AVAuthorizationStatus::NotDetermined => {
            let (tx, rx) = sync_channel(1);
            let answered = RcBlock::new(move |granted: Bool| _ = tx.try_send(granted.as_bool()));
            unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type(kind), &answered) };
            match rx.recv() {
                Ok(true) => Ok(()),
                _ => Err(denied()),
            }
        }
        _ => Err(denied()),
    }
}

struct Capture {
    stop: Sender<()>,
    thread: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl Source for Capture {
    fn stop(&self) {
        _ = self.stop.send(());
        if let Some(thread) = lock(&self.thread).take() {
            _ = thread.join();
        }
    }
}

struct Running {
    session: Retained<AVCaptureSession>,
    device: Retained<AVCaptureDevice>,
    _delegate: Retained<Delegate>,
}

fn session(wanted: &Device, mode: Option<Mode>, feed: Arc<Feed>) -> Result<Running, Error> {
    let kind = wanted.kind;
    let device = captured(kind)
        .into_iter()
        .find(|d| stable_id(kind, &unsafe { d.uniqueID() }.to_string()) == wanted.id)
        .ok_or_else(|| Error::NotFound(format!("{} is gone", wanted.label)))?;
    let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) }
        .map_err(|e| Error::NotReadable(format!("{}: {}", wanted.label, e.localizedDescription())))?;
    let session = unsafe { AVCaptureSession::new() };
    let delegate = Delegate::new(feed, kind, mode.filter(|_| kind == Kind::Camera).map(|m| (m.width, m.height)));
    unsafe { session.beginConfiguration() };
    let configured = (|| {
        if !unsafe { session.canAddInput(&input) } {
            return Err(Error::NotReadable(format!("{} cannot be captured", wanted.label)));
        }
        unsafe { session.addInput(&input) };
        let queue = DispatchQueue::new("acde-capture", None);
        let output: Retained<AVCaptureOutput> = match kind {
            Kind::Camera => {
                let output = unsafe { AVCaptureVideoDataOutput::new() };
                unsafe {
                    let size = mode.map(|m| [("Width", number(m.width)), ("Height", number(m.height))]);
                    let mut wanted = vec![("PixelFormatType", number(BGRA))];
                    wanted.extend(size.into_iter().flatten());
                    output.setVideoSettings(Some(&settings(&wanted)));
                    output.setAlwaysDiscardsLateVideoFrames(true);
                    output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*delegate)), Some(&queue));
                }
                Retained::into_super(output)
            }
            Kind::Microphone => {
                let output = unsafe { AVCaptureAudioDataOutput::new() };
                unsafe {
                    output.setAudioSettings(Some(&settings(&[
                        ("AVFormatIDKey", number(LINEAR_PCM)),
                        ("AVSampleRateKey", number(RATE)),
                        ("AVNumberOfChannelsKey", number(CHANNELS as u32)),
                        ("AVLinearPCMBitDepthKey", number(32)),
                        ("AVLinearPCMIsFloatKey", number(1)),
                        ("AVLinearPCMIsNonInterleaved", number(0)),
                        ("AVLinearPCMIsBigEndianKey", number(0)),
                    ])));
                    output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*delegate)), Some(&queue));
                }
                Retained::into_super(output)
            }
        };
        if !unsafe { session.canAddOutput(&output) } {
            return Err(Error::NotReadable(format!("{} cannot be read", wanted.label)));
        }
        unsafe { session.addOutput(&output) };
        if let Some(mode) = mode.filter(|_| kind == Kind::Camera) {
            let format = unsafe { device.formats() }.iter().find(|f| {
                let size = unsafe { CMVideoFormatDescriptionGetDimensions(&f.formatDescription()) };
                let rate = unsafe { f.videoSupportedFrameRateRanges() }.iter().map(|r| unsafe { r.maxFrameRate() }).fold(0.0, f64::max);
                (size.width.max(0) as u32, size.height.max(0) as u32) == (mode.width, mode.height) && rate + 0.5 >= mode.frame_rate
            });
            let format = format.ok_or_else(|| Error::NotReadable(format!("{} has no {}x{} mode", wanted.label, mode.width, mode.height)))?;
            unsafe { device.lockForConfiguration() }.map_err(|e| Error::NotReadable(format!("{}: {}", wanted.label, e.localizedDescription())))?;
            unsafe {
                device.setActiveFormat(&format);
                device.unlockForConfiguration();
            }
        }
        Ok(())
    })();
    unsafe { session.commitConfiguration() };
    configured?;
    Ok(Running { session, device, _delegate: delegate })
}

pub(crate) fn open(wanted: &Device, mode: Option<Mode>, feed: Arc<Feed>) -> Result<Box<dyn Source>, Error> {
    authorize(wanted.kind)?;
    let (ready_tx, ready_rx) = sync_channel::<Result<(), Error>>(1);
    let (stop, stopped): (Sender<()>, Receiver<()>) = channel();
    let (wanted, ended) = (wanted.clone(), feed.clone());
    let thread = std::thread::Builder::new()
        .name("acde-capture".into())
        .spawn(move || {
            let running = match session(&wanted, mode, feed) {
                Ok(opened) => opened,
                Err(e) => {
                    _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let (session, device) = (&running.session, &running.device);
            unsafe { session.startRunning() };
            if !unsafe { session.isRunning() } {
                _ = ready_tx.send(Err(Error::NotReadable(format!("{} does not start (in use by another app?)", wanted.label))));
                return;
            }
            _ = ready_tx.send(Ok(()));
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(Duration::from_millis(250)) {
                if !unsafe { device.isConnected() } {
                    ended.end(format!("{} was disconnected", wanted.label));
                    break;
                }
                if !unsafe { session.isRunning() } {
                    ended.end(format!("{} stopped", wanted.label));
                    break;
                }
            }
            unsafe { session.stopRunning() };
        })
        .map_err(|e| Error::NotReadable(e.to_string()))?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(Box::new(Capture { stop, thread: std::sync::Mutex::new(Some(thread)) })),
        Ok(Err(e)) => {
            _ = thread.join();
            Err(e)
        }
        Err(_) => {
            _ = thread.join();
            Err(Error::NotReadable("the capture thread stopped".into()))
        }
    }
}
