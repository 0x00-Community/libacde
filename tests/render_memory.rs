use std::time::{Duration, Instant};

use acde::render::{Options, Resource};

mod common;
use common::Doc;

fn page() -> (Doc, u32) {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/vendor/blitz/packages/blitz-dom/assets");
    let options = Options { resources: Some(vec![Resource { prefix: "/fonts/".into(), dirs: vec![assets.into()] }]), ..Options::default() };
    let doc = Doc::new(400, 300, 1.0, &options).unwrap();
    let body = doc.create_element("body").unwrap();
    doc.append_child(0, body).unwrap();
    (doc, body)
}

fn settle(doc: &Doc, done: impl Fn(&acde::render::Stats) -> bool) -> acde::render::Stats {
    settle_for(doc, Duration::from_secs(5), done)
}

fn settle_for(doc: &Doc, at_most: Duration, done: impl Fn(&acde::render::Stats) -> bool) -> acde::render::Stats {
    let start = Instant::now();
    loop {
        doc.scene();
        let s = doc.stats();
        if done(&s) || start.elapsed() > at_most {
            return s;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn same_font_face_registers_once() {
    let (doc, body) = page();
    for round in 0..5 {
        let style = doc.create_element("style").unwrap();
        let css = doc
            .create_text(&format!("@font-face {{ font-family: Bullet; src: url(/fonts/moz-bullet-font.otf) }} .r{round} {{ font-family: Bullet }}"))
            .unwrap();
        doc.append_child(style, css).unwrap();
        doc.append_child(body, style).unwrap();
        settle(&doc, |s| s.fonts >= 1);
        doc.remove(style).unwrap();
        doc.release(css).unwrap();
        doc.release(style).unwrap();
    }
    let s = settle_for(&doc, Duration::from_millis(300), |_| false);
    assert_eq!(s.fonts, 1, "{s:?}");
}

#[test]
fn image_cache_is_bounded() {
    let (doc, body) = page();
    for round in 0..300 {
        let img = doc.create_element("img").unwrap();
        let svg = format!(
            "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><rect width='8' height='8' fill='%23{:06x}'/></svg>",
            round * 4099
        );
        doc.set_attribute(img, "src", &svg).unwrap();
        doc.append_child(body, img).unwrap();
        doc.scene();
        doc.remove(img).unwrap();
        doc.release(img).unwrap();
    }
    let s = settle(&doc, |s| s.images >= 256);
    assert!(s.images <= 256, "{s:?}");
}
