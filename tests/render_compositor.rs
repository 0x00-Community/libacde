use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use acde::render::Options;

mod common;
use common::{Doc, WaitFrame};

struct Page {
    doc: Doc,
    body: u32,
}

impl Page {
    fn new(css: &str) -> Self {
        let doc = Doc::new(800, 600, 2.0, &Options::default()).unwrap();
        doc.add_stylesheet(css).unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        Self { doc, body }
    }

    fn div(&self, parent: u32, class: &str) -> u32 {
        let id = self.doc.create_element("div").unwrap();
        self.doc.set_attribute(id, "class", class).unwrap();
        self.doc.append_child(parent, id).unwrap();
        id
    }
}

const CSS: &str = "
    html, body { height: 100%; margin: 0; }
    .mover { position: absolute; left: 0; top: 0; width: 10px; height: 10px; transition: left 1000ms linear; }
    .mover.go { left: 100px; }
    .list { position: absolute; left: 200px; top: 0; width: 100px; height: 100px; overflow: scroll; }
    .item { height: 50px; }
    .h { position: absolute; left: 0; top: 200px; width: 20px; height: 20px; }
    .h:hover { width: 60px; }
";

#[test]
fn transition_runs_on_compositor_ticks_while_js_is_blocked() {
    let page = Page::new(CSS);
    let mover = page.div(page.body, "mover");
    let handle = page.doc.handle();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    handle.set_waker(Box::new(move || _ = counter.fetch_add(1, Ordering::SeqCst)));

    handle.commit();
    let first = handle.wait_frame();
    assert_eq!(first.size, (800, 600));
    assert!(!first.animating);

    page.doc.set_attribute(mover, "class", "mover go").unwrap();
    handle.commit();
    assert!(handle.wait_frame().animating);

    let compositor = handle.clone();
    let frames = std::thread::spawn(move || {
        let mut frames = Vec::new();
        for i in 1..=120 {
            compositor.tick(f64::from(i) * 1000.0 / 60.0);
            let frame = compositor.wait_frame();
            let done = !frame.animating;
            frames.push(frame);
            if done {
                break;
            }
        }
        frames
    })
    .join()
    .unwrap();
    assert!((59..=62).contains(&frames.len()), "{} frames", frames.len());
    assert!(!frames.last().unwrap().animating);
    assert!(wakes.load(Ordering::SeqCst) >= 61);
    assert_eq!(page.doc.rect(mover).unwrap().x.round(), 100.0);
    handle.tick(2000.0);
    assert!(handle.frame().is_none());
}

#[test]
fn transition_follows_tick_time() {
    let page = Page::new(CSS);
    let mover = page.div(page.body, "mover");
    page.doc.rect(mover).unwrap();
    page.doc.set_attribute(mover, "class", "mover go").unwrap();
    page.doc.rect(mover).unwrap();
    let handle = page.doc.handle();
    handle.tick(250.0);
    assert!((page.doc.rect(mover).unwrap().x - 25.0).abs() < 2.0);
    handle.tick(500.0);
    assert!((page.doc.rect(mover).unwrap().x - 50.0).abs() < 2.0);
}

#[test]
fn scroll_and_hover_from_compositor() {
    let page = Page::new(CSS);
    let list = page.div(page.body, "list");
    let items: Vec<u32> = (0..4).map(|_| page.div(list, "item")).collect();
    let hover = page.div(page.body, "h");
    let handle = page.doc.handle();
    handle.commit();
    assert_eq!(page.doc.rect(items[1]).unwrap().y, 50.0);

    handle.scroll_at(500.0, 50.0, 0.0, 30.0);
    handle.wait_frame();
    assert_eq!(page.doc.rect(items[1]).unwrap().y, 35.0);

    assert_eq!(page.doc.rect(hover).unwrap().width, 20.0);
    handle.pointer(20.0, 420.0);
    handle.wait_frame();
    assert_eq!(page.doc.rect(hover).unwrap().width, 60.0);
}

#[test]
fn panic_on_render_thread_is_an_error_not_an_abort() {
    let options = Options { base_url: Some("data:text/css;base64,".into()), ..Default::default() };
    let doc = Doc::new(100, 100, 1.0, &options).unwrap();
    let head = doc.create_element("head").unwrap();
    let link = doc.create_element("link").unwrap();
    doc.set_attribute(link, "rel", "stylesheet").unwrap();
    doc.set_attribute(link, "href", "/x.css").unwrap();
    doc.append_child(head, link).unwrap();
    let error = doc.append_child(0, head).unwrap_err();
    assert!(error.0.contains("panicked"), "{error}");
    assert!(doc.create_element("div").is_ok());
}

#[test]
fn release_drops_subtree_and_ids_stay_unique() {
    let page = Page::new(CSS);
    let box_ = page.div(page.body, "h");
    let child = page.div(box_, "");
    let frame = page.doc.create_element("iframe").unwrap();
    page.doc.append_child(box_, frame).unwrap();

    page.doc.release(box_).unwrap();
    assert!(page.doc.rect(child).unwrap_err().0.contains("released"));
    assert!(page.doc.rect(frame).unwrap_err().0.contains("released"));
    page.doc.release(child).unwrap();
    assert!(page.doc.release(9999).is_err());
    assert!(page.doc.release(0).is_err());
    assert!(page.doc.create_element("div").unwrap() > frame);
}

#[cfg(target_os = "linux")]
fn resident_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    Some(statm.split_whitespace().nth(1).unwrap().parse::<u64>().unwrap() * 4096)
}

#[cfg(target_os = "macos")]
fn resident_bytes() -> Option<u64> {
    #[repr(C, packed(4))]
    #[derive(Default)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: [i32; 2],
        system_time: [i32; 2],
        policy: i32,
        suspend_count: i32,
    }
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: i32, info: *mut MachTaskBasicInfo, count: *mut u32) -> i32;
    }
    const MACH_TASK_BASIC_INFO: i32 = 20;
    let mut info = MachTaskBasicInfo::default();
    let mut count = (size_of::<MachTaskBasicInfo>() / size_of::<u32>()) as u32;
    let status = unsafe { task_info(mach_task_self_, MACH_TASK_BASIC_INFO, &mut info, &mut count) };
    assert_eq!(status, 0, "task_info failed with {status}");
    Some(info.resident_size)
}

#[cfg(windows)]
fn resident_bytes() -> Option<u64> {
    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
    }
    let mut counters = ProcessMemoryCounters { cb: size_of::<ProcessMemoryCounters>() as u32, ..Default::default() };
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    assert_ne!(ok, 0, "GetProcessMemoryInfo failed: {}", std::io::Error::last_os_error());
    Some(counters.working_set_size as u64)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn resident_bytes() -> Option<u64> {
    None
}

#[test]
fn release_keeps_memory_flat() {
    if resident_bytes().is_none() {
        eprintln!("skipped: no resident memory reading on this platform");
        return;
    }
    let rss = || resident_bytes().unwrap();
    let page = Page::new(CSS);
    let round = |page: &Page| {
        let root = page.div(page.body, "list");
        (0..49).for_each(|_| _ = page.div(root, "item"));
        page.doc.rect(root).unwrap();
        page.doc.remove(root).unwrap();
        page.doc.release(root).unwrap();
    };
    (0..200).for_each(|_| round(&page));
    let before = rss();
    (0..2000).for_each(|_| round(&page));
    let grown = rss().saturating_sub(before);
    eprintln!("rss grew {} KiB over 2000 rounds (100k nodes)", grown / 1024);
    assert!(grown < 16 << 20, "rss grew {grown} bytes");
}

#[test]
fn transition_after_idle_has_intermediate_frames() {
    let page = Page::new(".box { position: absolute; left: 0; top: 0; width: 10px; height: 10px; transition: left 600ms linear; } .box.moved { left: 300px; }");
    let b = page.div(page.body, "box");
    let handle = page.doc.handle();
    let start = Instant::now();
    let now = || start.elapsed().as_secs_f64() * 1e3;
    handle.commit();
    handle.wait_frame();
    handle.tick(now());
    std::thread::sleep(Duration::from_millis(300));

    page.doc.set_attribute(b, "class", "box moved").unwrap();
    handle.commit();
    assert!(handle.wait_frame().animating, "the frame after the class change must be animating");
    let mut xs = Vec::new();
    loop {
        std::thread::sleep(Duration::from_millis(16));
        handle.tick(now());
        let frame = handle.wait_frame();
        xs.push(page.doc.rect(b).unwrap().x);
        if !frame.animating {
            break;
        }
        assert!(xs.len() < 200);
    }
    assert!(xs.len() >= 25, "only {} frames: {xs:?}", xs.len());
    assert!(xs.windows(2).all(|w| w[1] >= w[0]), "{xs:?}");
    assert!(xs[0] < 60.0, "first tick jumped to {}", xs[0]);
    assert_eq!(xs.last().unwrap().round(), 300.0);
}

#[test]
fn queries_do_not_resolve_every_time_while_animating() {
    let page = Page::new(
        "@keyframes spin { from { left: 0; } to { left: 100px; } }
        .spin { position: absolute; width: 10px; height: 10px; animation: spin 1s infinite linear; }",
    );
    let spin = page.div(page.body, "spin");
    page.doc.rect(spin).unwrap();
    let before = page.doc.stats().resolves;
    let start = Instant::now();
    for _ in 0..200 {
        page.doc.metrics(spin).unwrap();
    }
    let elapsed = start.elapsed().as_secs_f64();
    let resolves = page.doc.stats().resolves - before;
    assert!(f64::from(resolves) <= elapsed * 60.0 + 2.0, "{resolves} resolves in {elapsed:.3}s");
    std::thread::sleep(Duration::from_millis(100));
    assert!(page.doc.rect(spin).unwrap().x > 0.0);
}

#[test]
fn offset_parent() {
    let page = Page::new(".pos { position: relative; } .fixed { position: fixed; } .none { display: none; }");
    let pos = page.div(page.body, "pos");
    let inner = page.div(pos, "");
    let plain = page.div(page.body, "");
    let fixed = page.div(page.body, "fixed");
    let hidden = page.div(page.body, "none");
    let under_hidden = page.div(hidden, "");
    let parent = |id| page.doc.metrics(id).ok().and_then(|m| m.offset_parent);
    assert_eq!(parent(inner), Some(pos));
    assert_eq!(parent(plain), Some(page.body));
    assert_eq!(parent(fixed), None);
    assert_eq!(parent(hidden), None);
    assert_eq!(parent(under_hidden), None);
    assert_eq!(parent(page.body), None);
    assert!(page.doc.metrics(9999).is_err());
}

#[test]
fn absolute_without_positioned_ancestor_has_body_as_offset_parent() {
    let page = Page::new(".abs { position: absolute; }");
    let wrap = page.div(page.body, "");
    let abs = page.div(wrap, "abs");
    assert_eq!(page.doc.metrics(abs).unwrap().offset_parent, Some(page.body));
}

#[test]
fn paused_and_undisplayed_animations_do_not_keep_ticking() {
    let page = Page::new(
        "@keyframes march { to { stroke-dashoffset: -20 } }
         .dash { stroke: red; stroke-dasharray: 12 8; animation: march .6s linear infinite paused }
         .dash.on { animation-play-state: running }
         .gone { display: none }
         .spin { animation: march 1s infinite }",
    );
    let svg = page.doc.create_element_ns(1, "svg").unwrap();
    page.doc.append_child(page.body, svg).unwrap();
    let rect = page.doc.create_element_ns(1, "rect").unwrap();
    page.doc.set_attribute(rect, "class", "dash").unwrap();
    page.doc.append_child(svg, rect).unwrap();
    let gone = page.div(page.body, "gone");
    page.div(gone, "spin");
    let handle = page.doc.handle();
    handle.commit();
    assert!(!handle.wait_frame().animating);
    assert!(!page.doc.stats().animating);
    page.doc.set_attribute(rect, "class", "dash on").unwrap();
    handle.commit();
    assert!(handle.wait_frame().animating);
    page.doc.set_attribute(rect, "class", "dash").unwrap();
    handle.commit();
    assert!(!handle.wait_frame().animating, "paused again");
}

fn waapi(names: &[&str], delays: &[&str]) -> String {
    let n = names.len();
    let list = |v: &str| vec![v; n].join(", ");
    format!(
        "position: absolute; top: 0; width: 10px; height: 10px; animation-name: {}; animation-duration: {}; animation-timing-function: {}; animation-delay: {}; animation-iteration-count: {}; animation-direction: {}; animation-fill-mode: {}; animation-play-state: {};",
        names.join(", "),
        list("1000ms"),
        list("linear"),
        delays.join(", "),
        list("1"),
        list("normal"),
        list("forwards"),
        list("running"),
    )
}

#[test]
fn an_animation_takes_a_new_value_on_every_tick_at_120_hz() {
    let page =
        Page::new(".box { position: absolute; left: 0; top: 0; width: 10px; height: 10px; transition: left 2000ms linear; } .box.moved { left: 1000px; }");
    let b = page.div(page.body, "box");
    let handle = page.doc.handle();
    let start = Instant::now();
    let now = || start.elapsed().as_secs_f64() * 1e3;
    handle.commit();
    handle.wait_frame();
    page.doc.set_attribute(b, "class", "box moved").unwrap();
    handle.commit();
    handle.wait_frame();
    let mut xs = Vec::new();
    for _ in 0..40 {
        std::thread::sleep(Duration::from_micros(8_333));
        handle.tick(now());
        handle.wait_frame();
        xs.push(page.doc.rect(b).unwrap().x);
    }
    let repeated = xs.windows(2).filter(|w| w[1] <= w[0]).count();
    assert_eq!(repeated, 0, "ticks that kept the previous value: {xs:?}");
}

#[test]
fn ticks_do_not_restyle_an_idle_document() {
    let page = Page::new(".box { position: absolute; width: 10px; height: 10px; }");
    page.div(page.body, "box");
    let handle = page.doc.handle();
    let start = Instant::now();
    handle.commit();
    handle.wait_frame();
    let before = page.doc.stats().resolves;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_micros(8_333));
        handle.tick(start.elapsed().as_secs_f64() * 1e3);
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(page.doc.stats().resolves, before);
}
