use acde::render::Options;
use std::time::{Duration, Instant};

mod common;
use common::Doc;
#[test]
#[ignore = "benchmark, run explicitly"]
fn latency() {
    let doc = Doc::new(1920, 1080, 1.0, &Options::default()).unwrap();
    doc.add_stylesheet(
        "html,body{margin:0} .r{display:flex;gap:4px;padding:2px} .c{flex:1;background:#eef;border-radius:3px;box-shadow:0 1px 3px #0003}
      @keyframes p{from{opacity:1}to{opacity:.5}} .a{animation:p 1s infinite alternate}",
    )
    .unwrap();
    let body = doc.create_element("body").unwrap();
    doc.append_child(0, body).unwrap();
    let mut cells = vec![];
    for r in 0..110 {
        let row = doc.create_element("div").unwrap();
        doc.set_attribute(row, "class", "r").unwrap();
        doc.append_child(body, row).unwrap();
        for k in 0..4 {
            let c = doc.create_element("span").unwrap();
            doc.set_attribute(c, "class", if k == 0 && r % 10 == 0 { "c a" } else { "c" }).unwrap();
            doc.append_child(row, c).unwrap();
            let t = doc.create_text("cell text").unwrap();
            doc.append_child(c, t).unwrap();
            cells.push(c);
        }
    }
    doc.rect(cells[0]).unwrap();
    let handle = doc.handle();
    handle.commit();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s2 = stop.clone();
    let h2 = handle.clone();
    let t0 = Instant::now();
    let ticker = std::thread::spawn(move || {
        while !s2.load(std::sync::atomic::Ordering::Relaxed) {
            h2.tick(t0.elapsed().as_secs_f64() * 1e3);
            while h2.frame().is_none() && !s2.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(Duration::from_micros(200));
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    });
    std::thread::sleep(Duration::from_millis(300));
    let mut lat: Vec<f64> = (0..400)
        .map(|i| {
            let s = Instant::now();
            doc.metrics(cells[i % cells.len()]).unwrap();
            let d = s.elapsed().as_secs_f64() * 1e3;
            std::thread::sleep(Duration::from_micros(700));
            d
        })
        .collect();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    ticker.join().unwrap();
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let st = doc.stats();
    eprintln!("metrics latency ms: p50 {:.3} p95 {:.3} max {:.3}; resolve {:.2}ms paint {:.2}ms", lat[200], lat[380], lat[399], st.resolve_ms, st.paint_ms);
}
