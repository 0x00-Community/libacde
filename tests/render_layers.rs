use std::time::{Duration, Instant};

use acde::gpu::FilterEffect;
use acde::gpu::{ColorMatrix, Effect};
use acde::gpu::{PaintScene, Scene};
use acde::render::{Frame, Options};

mod common;
use common::{Doc, Handle};

const CSS: &str = "html, body { margin: 0; height: 100%; }
  body { background: repeating-linear-gradient(45deg, #ff3d7f 0 20px, #ffd23d 20px 40px, #3dd2ff 40px 60px), #fff; }
  .card { position: absolute; left: 50px; top: 40px; width: 200px; height: 120px; border-radius: 16px;
          background: rgba(255, 255, 255, .25); backdrop-filter: blur(20px) saturate(1.8); }
  .shadow { position: absolute; left: 300px; top: 60px; width: 80px; height: 80px; border-radius: 12px; background: #fff;
            filter: drop-shadow(0 8px 10px rgba(40, 16, 80, .45)); }
  .glass { position: absolute; left: 20px; top: 20px; width: 100px; height: 60px; }
  .contrast { backdrop-filter: contrast(1.5); }
  .gray { backdrop-filter: grayscale(1); }
  .hue { backdrop-filter: hue-rotate(90deg); }
  .chain { backdrop-filter: sepia(.5) blur(2px) invert(.25) drop-shadow(0 2px 3px black); }
  .blurs { backdrop-filter: blur(3px) blur(4px); }
  .ds { position: absolute; left: 20px; top: 20px; width: 40px; height: 40px; background: #fff; }
  .ds4 { filter: drop-shadow(0 0 4px black); }
  .ds10 { filter: drop-shadow(2px 3px 10px black); }
  .ds25 { filter: drop-shadow(0 0 25px black); }
  .bl6 { filter: blur(6px); }
  .merge { --acde-glass-merge: 16px; }
  .pill { position: absolute; top: 20px; width: 60px; height: 40px; border-radius: 20px; backdrop-filter: blur(2px); --acde-glass: regular; }";

struct Page {
    doc: Doc,
    handle: Handle,
    body: u32,
}

impl Page {
    fn new(scale: f32) -> Self {
        let doc = Doc::new((400.0 * scale) as u32, (200.0 * scale) as u32, scale, &Options::default()).unwrap();
        doc.add_stylesheet(CSS).unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        let handle = doc.handle();
        Self { doc, handle, body }
    }

    fn div(&self, class: &str) -> u32 {
        let id = self.doc.create_element("div").unwrap();
        self.doc.set_attribute(id, "class", class).unwrap();
        self.doc.append_child(self.body, id).unwrap();
        id
    }

    fn frame(&self) -> Frame {
        self.handle.commit();
        let start = Instant::now();
        loop {
            if let Some(f) = self.handle.frame() {
                return f;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn raster(size: (u32, u32), scenes: Vec<Scene>) -> Vec<u8> {
    let mut all = Scene::new();
    for scene in scenes {
        all.append_scene(scene, Default::default());
    }
    common::rasterize(all, size)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.5
}

#[test]
fn no_filters_no_layers() {
    let page = Page::new(1.0);
    page.div("plain");
    assert!(page.frame().plane.layers.is_empty());
}

#[test]
fn backdrop_card_splits_into_two_segments() {
    let page = Page::new(2.0);
    page.div("card");
    let frame = page.frame();
    assert_eq!(frame.plane.layers.len(), 2, "the page, then the card over a filtered backdrop");
    let b = frame.plane.layers[1].backdrop.as_ref().unwrap();
    assert!(near(b.bounds.x0, 100.0) && near(b.bounds.y0, 80.0) && near(b.bounds.width(), 400.0) && near(b.bounds.height(), 240.0), "{:?}", b.bounds);
    assert_eq!(b.scale, 2.0);
    let effects: Vec<&FilterEffect> = b.filter.effects().iter().collect();
    assert!(matches!(effects[0], FilterEffect::GaussianBlur(_)), "{effects:?}");
    assert_eq!(effects.len(), 2, "blur + saturate: {effects:?}");
    assert!(near(b.radius(), 32.0), "radius {}", b.radius());
    assert!(near(b.blur(), 40.0), "blur {}", b.blur());
    assert!((b.saturate() - 1.8).abs() < 1e-3, "saturate {}", b.saturate());
    assert_eq!(b.brightness(), 1.0);
}

#[test]
fn drop_shadow_element_is_its_own_segment() {
    let page = Page::new(1.0);
    page.div("shadow");
    page.div("plain");
    let frame = page.frame();
    assert_eq!(frame.plane.layers.len(), 2, "the page, then the filtered element (nothing visible after it)");
    let f = frame.plane.layers[1].filter.as_ref().unwrap();
    assert!(matches!(f.filter.effects()[0], FilterEffect::DropShadow(_)));
    assert!(f.bounds.x0 < 300.0 && f.bounds.y1 > 140.0 + 8.0, "{:?}", f.bounds);
}

#[test]
fn segments_compose_to_the_whole_scene() {
    let page = Page::new(1.0);
    page.div("card");
    page.div("shadow");
    let frame = page.frame();
    assert_eq!(frame.plane.layers.len(), 3);
    assert!(frame.plane.drawing.scene().commands.is_empty());
    let whole = raster(frame.size, vec![page.doc.scene()]);
    let parts = raster(frame.size, frame.plane.layers.iter().map(|l| l.drawing.scene()).collect());
    let differing = whole.chunks(4).zip(parts.chunks(4)).filter(|(a, b)| a != b).count();
    assert_eq!(differing, 0);
}

fn filter_of(class: &str) -> FilterEffect {
    let page = Page::new(1.0);
    page.div(&format!("ds {class}"));
    let frame = page.frame();
    let layer = frame.plane.layers.iter().find_map(|l| l.filter.as_ref()).expect("a filtered layer");
    layer.filter.effects()[0].clone()
}

#[test]
fn drop_shadow_deviation_is_half_its_blur_radius() {
    for (class, radius, offset) in [("ds4", 4.0, (0.0, 0.0)), ("ds10", 10.0, (2.0, 3.0)), ("ds25", 25.0, (0.0, 0.0))] {
        let FilterEffect::DropShadow(shadow) = filter_of(class) else { panic!("{class}: not a drop shadow") };
        assert!(near(f64::from(shadow.std_deviation), radius / 2.0), "{class}: deviation {}", shadow.std_deviation);
        assert!(near(f64::from(shadow.dx), offset.0) && near(f64::from(shadow.dy), offset.1), "{class}: offset {} {}", shadow.dx, shadow.dy);
    }
    let FilterEffect::GaussianBlur(blur) = filter_of("bl6") else { panic!("not a blur") };
    assert!(near(f64::from(blur.std_deviation), 6.0), "blur() keeps its radius as the deviation: {}", blur.std_deviation);
}

fn backdrop_of(class: &str) -> Effect {
    let page = Page::new(1.0);
    page.div(&format!("glass {class}"));
    let frame = page.frame();
    frame.plane.layers.iter().find_map(|l| l.backdrop.as_ref()).expect("a backdrop").effect()
}

#[test]
fn backdrop_keeps_every_color_function_in_order() {
    let cases = [
        ("contrast", vec![ColorMatrix::contrast(1.5)]),
        ("gray", vec![ColorMatrix::grayscale(1.0)]),
        ("hue", vec![ColorMatrix::hue_rotate(std::f32::consts::FRAC_PI_2)]),
        ("chain", vec![ColorMatrix::sepia(0.5), ColorMatrix::invert(0.25)]),
    ];
    for (class, matrices) in cases {
        let got = backdrop_of(class);
        let mut want = Effect::default();
        for m in matrices {
            want.matrix(m.0);
        }
        let (got_m, want_m) = (got.matrix.expect("a color matrix"), want.matrix.unwrap());
        assert!(got_m.iter().zip(want_m).all(|(a, b)| (a - b).abs() < 1e-4), "{class}: {got_m:?}, want {want_m:?}");
    }
    assert!(near(f64::from(backdrop_of("chain").blur), 2.0));
}

#[test]
fn backdrop_blurs_combine_as_root_sum_of_squares() {
    let page = Page::new(1.0);
    page.div("glass blurs");
    let frame = page.frame();
    let b = frame.plane.layers.iter().find_map(|l| l.backdrop.as_ref()).unwrap();
    assert!((b.blur() - 5.0).abs() < 1e-3, "blur {}", b.blur());
    assert!((b.effect().blur - 5.0).abs() < 1e-3, "blur {}", b.effect().blur);
    assert!(b.effect().matrix.is_none());
}

#[test]
fn glass_merges_within_its_container_or_by_union_into_one_group() {
    for (container, pill, groups) in [("merge", "", 1), ("", "", 2), ("", "--acde-glass-union: pair;", 1)] {
        let page = Page::new(1.0);
        let row = page.div(container);
        for left in ["20px", "86px"] {
            let id = page.doc.create_element("div").unwrap();
            page.doc.set_attribute(id, "class", "pill").unwrap();
            page.doc.set_attribute(id, "style", &format!("left: {left}; {pill}")).unwrap();
            page.doc.append_child(row, id).unwrap();
        }
        let frame = page.frame();
        let backdrops: Vec<_> = frame.plane.layers.iter().filter_map(|l| l.backdrop.as_ref()).collect();
        assert_eq!(backdrops.len(), groups, "{container:?} {pill:?}");
        if groups == 2 {
            assert!(backdrops.iter().all(|b| b.group().is_none()));
            continue;
        }
        let group = backdrops[0].group().expect("one group");
        assert_eq!(group.len(), 2);
        assert!(near(backdrops[0].bounds.width(), 126.0), "the group covers both pills: {:?}", backdrops[0].bounds);
        assert_eq!(group.spacing, if container.is_empty() { 0.0 } else { 16.0 });
    }
}
