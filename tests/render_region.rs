use std::time::{Duration, Instant};

use acde::gpu::Scene;
use acde::render::{Frame, Options};
use kurbo::Rect;

mod common;
use common::Doc;

const W: u32 = 480;
const H: u32 = 360;

const CSS: &str = "
    html, body { margin: 0; height: 100%; font: 13px sans-serif; background: #f4f2f8; }
    .card { position: absolute; width: 120px; height: 70px; padding: 6px; border-radius: 12px; background: #fff;
            box-shadow: 0 4px 12px rgba(0,0,0,.25); }
    .fade { opacity: .7; }
    .mix { mix-blend-mode: multiply; background: #f9c; }
    .rot { transform: rotate(8deg) scale(1.1); }
    .clip { overflow: hidden; border-radius: 16px; }
    .list { position: absolute; left: 300px; top: 20px; width: 140px; height: 120px; overflow: auto; background: #efe; }
    .item { height: 26px; margin: 4px; background: linear-gradient(90deg, #69c, #9cf); border-radius: 4px; }
    .blur { position: absolute; left: 40px; top: 230px; width: 90px; height: 60px; background: #c63; filter: blur(4px); }
    .glass { position: absolute; left: 180px; top: 200px; width: 200px; height: 120px; border-radius: 20px;
             background: rgba(255,255,255,.3); backdrop-filter: blur(8px) saturate(1.6); }
";

fn page(effects: bool) -> (Doc, common::Handle) {
    let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
    doc.add_stylesheet(CSS).unwrap();
    let body = doc.create_element("body").unwrap();
    doc.append_child(0, body).unwrap();
    let el = |parent: u32, class: &str, style: &str, text: Option<&str>| {
        let n = doc.create_element("div").unwrap();
        doc.set_attribute(n, "class", class).unwrap();
        doc.set_attribute(n, "style", style).unwrap();
        doc.append_child(parent, n).unwrap();
        if let Some(t) = text {
            let t = doc.create_text(t).unwrap();
            doc.append_child(n, t).unwrap();
        }
        n
    };
    el(body, "card", "left: 20px; top: 20px", Some("plain card with some text"));
    el(body, "card fade", "left: 160px; top: 30px", Some("faded"));
    let rot = el(body, "card rot clip", "left: 40px; top: 120px", None);
    el(rot, "item", "", Some("rotated clip"));
    el(body, "card mix", "left: 170px; top: 120px", Some("multiply"));
    let list = el(body, "list", "", None);
    for i in 0..10 {
        el(list, "item", "", Some(&format!("row {i}")));
    }
    let svg = doc.create_element_ns(1, "svg").unwrap();
    for (k, v) in [("width", "40"), ("height", "40"), ("viewBox", "0 0 40 40")] {
        doc.set_attribute(svg, k, v).unwrap();
    }
    let c = doc.create_element_ns(1, "circle").unwrap();
    for (k, v) in [("cx", "20"), ("cy", "20"), ("r", "16"), ("fill", "#96f")] {
        doc.set_attribute(c, k, v).unwrap();
    }
    doc.append_child(svg, c).unwrap();
    let holder = el(body, "", "position: absolute; left: 420px; top: 300px", None);
    doc.append_child(holder, svg).unwrap();
    if effects {
        el(body, "blur", "", None);
        el(body, "glass", "", Some("glass"));
    }
    let handle = doc.handle();
    (doc, handle)
}

fn frame(handle: &common::Handle) -> Frame {
    handle.commit();
    let start = Instant::now();
    loop {
        if let Some(f) = handle.frame() {
            return f;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "no frame");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn raster(scene: Scene) -> Vec<u8> {
    common::rasterize(scene, (W, H))
}

fn rects(n: usize) -> Vec<Rect> {
    let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = |m: u64| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s % m
    };
    (0..n)
        .map(|_| {
            let (x, y) = (next(u64::from(W) - 4) as f64, next(u64::from(H) - 4) as f64);
            let (w, h) = (4.0 + next(160) as f64, 4.0 + next(120) as f64);
            Rect::new(x, y, (x + w).min(f64::from(W)), (y + h).min(f64::from(H)))
        })
        .collect()
}

fn differing(a: &[u8], b: &[u8], r: Rect) -> usize {
    let (x0, y0, x1, y1) = (r.x0 as u32, r.y0 as u32, r.x1.ceil() as u32, r.y1.ceil() as u32);
    (y0..y1.min(H)).flat_map(|y| (x0..x1.min(W)).map(move |x| ((y * W + x) * 4) as usize)).filter(|&i| (0..4).any(|k| a[i + k].abs_diff(b[i + k]) > 2)).count()
}

#[test]
fn frame_scene_for_matches_whole_scene() {
    let (_doc, handle) = page(false);
    let f = frame(&handle);
    assert!(f.plane.layers.is_empty());
    let whole = raster(f.plane.drawing.scene());
    let mut kept = 0;
    for r in rects(50) {
        let part = f.plane.drawing.scene_for(r);
        kept += part.commands.len();
        assert_eq!(differing(&whole, &raster(part), r), 0, "{r:?}");
    }
    eprintln!("whole scene {} commands, scene_for kept {} on average", f.plane.drawing.scene().commands.len(), kept / 50);
}

#[test]
fn layer_scene_for_matches_each_layer() {
    let (_doc, handle) = page(true);
    let f = frame(&handle);
    assert!(!f.plane.layers.is_empty());
    for (i, layer) in f.plane.layers.iter().enumerate() {
        let whole = raster(layer.drawing.scene());
        for r in rects(50) {
            assert_eq!(differing(&whole, &raster(layer.drawing.scene_for(r)), r), 0, "layer {i} {r:?}");
        }
    }
}

#[test]
fn scene_for_is_cheap() {
    let (_doc, handle) = page(true);
    let f = frame(&handle);
    let rs = rects(200);
    let start = Instant::now();
    for r in &rs {
        for layer in &f.plane.layers {
            std::hint::black_box(layer.drawing.scene_for(*r));
        }
    }
    let per = start.elapsed().as_secs_f64() * 1e3 / (rs.len() * f.plane.layers.len()) as f64;
    eprintln!("scene_for: {per:.4} ms per layer and rect ({} layers)", f.plane.layers.len());
}
