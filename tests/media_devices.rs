use acde::media::capture;

#[test]
#[ignore = "lists this machine's real cameras and microphones"]
fn lists_the_system_devices() {
    let devices = capture::devices();
    for d in &devices {
        println!(
            "{:?} {:?} id={} group={} facing={:?} channels={} modes={:?}",
            d.kind,
            d.label,
            d.id,
            d.group,
            d.facing,
            d.channels,
            d.modes.iter().take(6).map(|m| format!("{}x{}@{:.0}", m.width, m.height, m.frame_rate)).collect::<Vec<_>>()
        );
    }
    for kind in [capture::Kind::Microphone, capture::Kind::Camera] {
        println!("{kind:?} permission: {:?}", capture::permission(kind));
    }
    let ids: std::collections::HashSet<_> = devices.iter().map(|d| &d.id).collect();
    assert_eq!(ids.len(), devices.len());
}

#[test]
#[ignore = "opens this machine's real devices for a moment"]
fn the_system_devices_can_be_read() {
    use std::time::{Duration, Instant};
    for device in capture::devices() {
        if capture::permission(device.kind) != capture::Permission::Granted {
            println!("{:?}: skipped, permission {:?}", device.label, capture::permission(device.kind));
            continue;
        }
        let mode = device.modes.iter().copied().find(|m| m.width <= 1280);
        let opened = match capture::Capture::open(&device, mode) {
            Ok(c) => c,
            Err(e) => {
                println!("{:?}: {e}", device.label);
                continue;
            }
        };
        match device.kind {
            capture::Kind::Camera => {
                let frame = opened.first_frame(Duration::from_secs(5));
                println!("{:?} {:?}: first frame {:?}", device.label, mode, frame.map(|f| (f.width, f.height)));
            }
            capture::Kind::Microphone => {
                let mut tap = opened.tap();
                let deadline = Instant::now() + Duration::from_millis(500);
                let mut got = 0;
                let mut buffer = vec![0f32; 48000];
                while Instant::now() < deadline {
                    got += tap.take(&mut buffer);
                    std::thread::sleep(Duration::from_millis(20));
                }
                println!("{:?}: {} samples in 0.5 s", device.label, got);
            }
        }
        println!("{:?}: ended {:?}", device.label, opened.ended());
    }
}

#[test]
#[ignore = "asks macOS for camera and microphone access (a dialog for a person to answer)"]
fn asks_for_permission() {
    for kind in [capture::Kind::Camera, capture::Kind::Microphone] {
        println!("{kind:?} before: {:?}", capture::permission(kind));
        println!("{kind:?} asked: {:?}", capture::authorize(kind));
        println!("{kind:?} after: {:?}", capture::permission(kind));
    }
}

#[test]
#[ignore = "opens this machine's cameras in each of their modes"]
fn each_camera_mode_gives_frames_of_its_size() {
    use std::time::Duration;
    for device in capture::devices().into_iter().filter(|d| d.kind == capture::Kind::Camera) {
        for mode in device.modes.clone() {
            let opened = capture::Capture::open(&device, Some(mode));
            let size = opened.as_ref().ok().and_then(|c| c.first_frame(Duration::from_secs(5))).map(|f| (f.width, f.height));
            println!("{:?} {}x{}@{}: {:?}", device.label, mode.width, mode.height, mode.frame_rate, size.ok_or(opened.err()));
        }
    }
}
