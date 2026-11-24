use acde::gpu::DeviceRequest;
use acde::gpu::wgpu::{Features, Limits};

fn limits() -> Limits {
    Limits { max_texture_dimension_2d: 16384, ..Limits::default() }
}

#[test]
fn tiers_go_from_webgpu_features_to_downlevel_limits() {
    let tiers = DeviceRequest::tiers(Features::FLOAT32_FILTERABLE, limits());
    let names: Vec<_> = tiers.iter().map(|t| t.tier).collect();
    assert_eq!(names, ["WebGPU features", "no optional features", "downlevel limits"]);
    assert_eq!(tiers[0].features, Features::FLOAT32_FILTERABLE);
    assert_eq!(tiers[1].features, Features::empty());
    assert_eq!(tiers[1].limits, limits());
    assert_eq!(tiers[2].limits.max_texture_dimension_2d, 16384, "the fallback keeps the adapter's texture size");
}

#[test]
fn an_adapter_without_optional_features_is_not_asked_twice() {
    let tiers = DeviceRequest::tiers(Features::empty(), limits());
    let names: Vec<_> = tiers.iter().map(|t| t.tier).collect();
    assert_eq!(names, ["WebGPU features", "downlevel limits"]);
}

#[test]
fn open_falls_back_in_order_and_reports_the_tier() {
    let tiers = DeviceRequest::tiers(Features::FLOAT32_FILTERABLE, limits());
    let mut asked = Vec::new();
    let opened = DeviceRequest::open("test", &tiers, |request| {
        asked.push(request.tier);
        if request.features.is_empty() { Ok(7) } else { Err("feature not supported".to_owned()) }
    });
    assert_eq!(opened, Ok((7, "no optional features")));
    assert_eq!(asked, ["WebGPU features", "no optional features"]);
}

#[test]
fn open_fails_only_when_every_tier_fails() {
    let tiers = DeviceRequest::tiers(Features::FLOAT32_FILTERABLE, limits());
    let opened: Result<(u8, _), _> = DeviceRequest::open("test", &tiers, |request| Err(format!("no {}", request.tier)));
    let error = opened.unwrap_err();
    assert!(error.contains("WebGPU features: no WebGPU features"), "{error}");
    assert!(error.contains("downlevel limits: no downlevel limits"), "{error}");
}

#[test]
fn presented_frames_leave_the_count_once_the_gpu_is_done_with_them() {
    use std::sync::atomic::Ordering;
    let Ok((_, _, gpu)) = acde::gpu::Gpu::open("frames test") else { return };
    for _ in 0..2 {
        let encoder = gpu.device.create_command_encoder(&Default::default());
        gpu.submit([encoder.finish()]);
        let frame = gpu.frame();
        gpu.queue.on_submitted_work_done(move || frame.store(true, Ordering::Release));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while acde::gpu::Gpu::frames_in_flight(2) > 0 {
        assert!(std::time::Instant::now() < deadline, "the frames never finished");
        std::thread::yield_now();
    }
    let unfinished = gpu.frame();
    assert_eq!(acde::gpu::Gpu::frames_in_flight(2), 1, "a frame whose work was never registered stays in flight");
    unfinished.store(true, Ordering::Release);
    assert_eq!(acde::gpu::Gpu::frames_in_flight(2), 0);
}
