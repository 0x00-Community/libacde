use std::sync::Arc;
use std::time::{Duration, Instant};

use acde::media::{CHANNELS, Player, RATE};

fn fixture(name: &str) -> Arc<[u8]> {
    std::fs::read(format!("{}/tests/fixtures/media/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap().into()
}

fn near(a: u8, b: u8) -> bool {
    a.abs_diff(b) <= 16
}

fn centre(frame: &acde::media::Frame) -> [u8; 3] {
    let at = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
    [frame.rgba[at], frame.rgba[at + 1], frame.rgba[at + 2]]
}

fn drain_audio(player: &Player) -> Vec<f32> {
    let mut all = Vec::new();
    let mut chunk = vec![0f32; 4096];
    let deadline = Instant::now() + Duration::from_secs(10);
    while !player.audio_drained() && Instant::now() < deadline {
        let n = player.take_audio(&mut chunk);
        all.extend_from_slice(&chunk[..n]);
        if n == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    all
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn an_mp4_gives_its_size_frames_and_sound() {
    let player = Player::open(fixture("clip.mp4")).unwrap();
    let info = player.info().clone();
    assert!(info.video && info.audio);
    assert_eq!((info.width, info.height), (64, 48));
    assert!((info.duration - 1.0).abs() < 0.1, "duration {}", info.duration);
    let frame = player.first_frame(Duration::from_secs(5)).expect("a first frame");
    assert_eq!((frame.width, frame.height), (64, 48));
    let [r, g, b] = centre(&frame);
    assert!(near(r, 0x33) && near(g, 0x66) && near(b, 0xcc), "colour {r:x} {g:x} {b:x}");
    let mut frames = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !player.video_drained() && Instant::now() < deadline {
        if player.frame_at(f64::MAX).is_some() {
            frames += 1;
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    assert!(frames >= 1, "frames {frames}");
    let audio = drain_audio(&player);
    let seconds = audio.len() as f64 / CHANNELS as f64 / f64::from(RATE);
    assert!((seconds - 1.0).abs() < 0.1, "audio seconds {seconds}");
    let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
    assert!(rms > 0.05, "rms {rms}");
    assert!(player.error().is_none());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn seeking_restarts_both_tracks_near_the_target() {
    let player = Player::open(fixture("clip.mp4")).unwrap();
    player.first_frame(Duration::from_secs(5)).expect("a first frame");
    player.seek(0.5);
    let frame = player.first_frame(Duration::from_secs(5)).expect("a frame after seeking");
    assert!((0.35..=0.65).contains(&frame.time), "frame at {}", frame.time);
    let audio = drain_audio(&player);
    let seconds = audio.len() as f64 / CHANNELS as f64 / f64::from(RATE);
    assert!((seconds - 0.5).abs() < 0.15, "audio seconds after seeking {seconds}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_rotated_video_is_shown_upright() {
    let player = Player::open(fixture("silent-rotated.mp4")).unwrap();
    let info = player.info().clone();
    assert!(info.video && !info.audio);
    assert_eq!((info.width, info.height), (48, 64));
    let frame = player.first_frame(Duration::from_secs(5)).expect("a first frame");
    assert_eq!((frame.width, frame.height), (48, 64));
    let [r, g, b] = centre(&frame);
    assert!(near(r, 0xcc) && near(g, 0x33) && near(b, 0x66), "colour {r:x} {g:x} {b:x}");
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn playback_is_not_supported_without_a_platform_decoder() {
    assert!(acde::media::TYPES.is_empty());
    let error = Player::open(fixture("clip.mp4")).err().expect("no decoder here");
    assert!(error.contains("not supported"), "{error}");
}

#[test]
fn bytes_that_are_not_media_are_an_error() {
    assert!(Player::open(Arc::from(&b"definitely not a video file"[..])).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn a_webm_plays_on_linux() {
    let player = Player::open(fixture("clip.webm")).unwrap();
    assert!(player.info().video && player.info().audio);
    let frame = player.first_frame(Duration::from_secs(5)).expect("a first frame");
    let [r, g, b] = centre(&frame);
    assert!(near(r, 0x33) && near(g, 0x66) && near(b, 0xcc), "colour {r:x} {g:x} {b:x}");
}

#[cfg(target_os = "macos")]
#[test]
fn a_webm_is_refused_on_macos() {
    assert!(Player::open(fixture("clip.webm")).is_err());
}

#[test]
fn playlists_and_other_containers_that_point_elsewhere_are_refused() {
    for bytes in [
        &b"#EXTM3U\n#EXTINF:1,\nfile:///etc/passwd\n"[..],
        b"<?xml version=\"1.0\"?><MPD><BaseURL>file:///etc/</BaseURL></MPD>",
        b"[playlist]\nFile1=file:///etc/passwd\n",
    ] {
        assert!(Player::open(Arc::from(bytes)).is_err(), "{}", String::from_utf8_lossy(bytes));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn frames_follow_the_picture_and_a_seek_lands_on_the_new_one() {
    let player = Player::open(fixture("two-colours.mp4")).unwrap();
    let first = player.first_frame(Duration::from_secs(5)).expect("a first frame");
    let [r, g, b] = centre(&first);
    assert!(near(r, 0xcc) && near(g, 0x33) && near(b, 0x33), "colour at 0: {r:x} {g:x} {b:x}");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut later = None;
    while later.is_none() && Instant::now() < deadline {
        later = player.frame_at(0.8).filter(|f| f.time >= 0.5);
        if later.is_none() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let [r, g, b] = centre(&later.expect("a frame after half a second"));
    assert!(near(r, 0x33) && near(g, 0xcc) && near(b, 0x33), "colour at 0.8: {r:x} {g:x} {b:x}");
    player.seek(0.2);
    let back = player.first_frame(Duration::from_secs(5)).expect("a frame after seeking back");
    let [r, g, b] = centre(&back);
    assert!(back.time < 0.5 && near(r, 0xcc) && near(g, 0x33), "frame at {} colour {r:x} {g:x} {b:x}", back.time);
    let epoch = player.epoch();
    player.seek(0.7);
    assert_eq!(player.epoch(), epoch + 1);
    let ahead = player.first_frame(Duration::from_secs(5)).expect("a frame after seeking ahead");
    let [r, g, b] = centre(&ahead);
    assert!(ahead.time >= 0.5 && near(r, 0x33) && near(g, 0xcc), "frame at {} colour {r:x} {g:x} {b:x}", ahead.time);
}
