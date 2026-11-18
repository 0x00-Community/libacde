use std::time::{Duration, Instant};

use acde::media::capture::{self, Capture, Kind, fake};
use acde::media::{CHANNELS, RATE};

static CAMERA: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn camera_alone() -> std::sync::MutexGuard<'static, ()> {
    CAMERA.lock().unwrap_or_else(|e| e.into_inner())
}

fn fake_devices() -> Vec<capture::Device> {
    capture::use_fake_devices();
    capture::devices()
}

fn device(kind: Kind) -> capture::Device {
    fake_devices().into_iter().find(|d| d.kind == kind).expect("a fake device")
}

#[test]
fn the_fake_devices_are_a_microphone_and_a_camera_of_one_group() {
    let devices = fake_devices();
    assert_eq!(devices.iter().map(|d| d.kind).collect::<Vec<_>>(), [Kind::Microphone, Kind::Camera]);
    assert_eq!(devices[0].group, devices[1].group);
    assert_ne!(devices[0].id, devices[1].id);
    assert!(devices[1].modes.iter().any(|m| (m.width, m.height) == (640, 480)));
}

#[test]
fn a_camera_gives_frames_of_its_mode() {
    let _alone = camera_alone();
    let camera = device(Kind::Camera);
    let mode = camera.modes.iter().copied().find(|m| m.width == 320).unwrap();
    let capture = Capture::open(&camera, Some(mode)).unwrap();
    let first = capture.first_frame(Duration::from_secs(2)).expect("a frame");
    assert_eq!((first.width, first.height), (320, 240));
    let at = ((first.height / 2 * first.width + 4) * 4) as usize;
    let px = &first.rgba[at..at + 3];
    assert!(px == fake::BACKGROUND || px == [0xff; 3], "{px:?}");
    let (seq, _) = capture.frame().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let (later, _) = capture.frame().unwrap();
    assert!(later >= seq + 3, "{seq} then {later}");
    assert!(capture.ended().is_none());
}

#[test]
fn every_tap_hears_the_microphone_from_when_it_was_made() {
    let microphone = device(Kind::Microphone);
    let capture = Capture::open(&microphone, None).unwrap();
    let mut early = capture.tap();
    std::thread::sleep(Duration::from_millis(100));
    let mut late = capture.tap();
    let mut buffer = vec![0f32; RATE as usize * CHANNELS];
    let deadline = Instant::now() + Duration::from_secs(2);
    while early.available() < RATE as usize / 5 * CHANNELS && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let n = early.take(&mut buffer);
    assert!(n >= RATE as usize / 5 * CHANNELS, "{n}");
    let peak = buffer[..n].iter().fold(0f32, |a, v| a.max(v.abs()));
    assert!((peak - fake::LEVEL).abs() < 0.01, "{peak}");
    let crossings = buffer[..n].as_chunks::<CHANNELS>().0.iter().map(|f| f[0]).collect::<Vec<_>>().windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    let seconds = (n / CHANNELS) as f64 / f64::from(RATE);
    assert!((crossings as f64 / seconds - fake::TONE).abs() < 20.0, "{crossings} in {seconds}");
    let behind = late.take(&mut buffer);
    assert!(behind < n, "{behind} {n}");
    std::thread::sleep(Duration::from_millis(100));
    assert!(early.available() > 480 * CHANNELS);
    early.keep_latest(480 * CHANNELS);
    assert!(early.available() <= 480 * CHANNELS + RATE as usize / 100 * CHANNELS, "{}", early.available());
}

#[test]
fn dropping_a_capture_stops_it() {
    let _alone = camera_alone();
    let capture = Capture::open(&device(Kind::Camera), None).unwrap();
    capture.first_frame(Duration::from_secs(2)).unwrap();
    let started = Instant::now();
    drop(capture);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_source_that_stops_tells_its_listeners_once() {
    let _alone = camera_alone();
    let camera = device(Kind::Camera);
    let capture = Capture::open(&camera, Some(capture::Mode { width: 320, height: 240, frame_rate: 30.0 })).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    capture.on_end(move |reason| tx.send(reason).unwrap());
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    fake::unplug(&camera.id);
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "the device was unplugged");
    assert_eq!(capture.ended().as_deref(), Some("the device was unplugged"));
    let (tx, rx) = std::sync::mpsc::channel();
    capture.on_end(move |reason| tx.send(reason).unwrap());
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "the device was unplugged");
}
