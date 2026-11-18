use std::time::{Duration, Instant};

use acde::gpu::Scene;
use acde::render::{Frame, Options, Rect};

mod common;
use common::{Doc, Handle};

const W: u32 = 400;
const H: u32 = 300;

const CSS: &str = "
    html, body { height: 100%; margin: 0; font: 14px sans-serif; background: #fff; }
    .bar { display: flex; gap: 4px; padding: 4px; background: #234; }
    .bar div { width: 40px; height: 20px; background: #8ac; border-radius: 4px; }
    p { margin: 4px; color: #222; }
    .hot { color: #c20; }
    .list { position: absolute; left: 250px; top: 60px; width: 120px; height: 100px; overflow: scroll; }
    .item { height: 30px; border-bottom: 2px solid #999; background: #eef; }
    .btn { position: absolute; left: 20px; top: 200px; width: 60px; height: 24px; background: #4a4; box-shadow: 0 2px 6px #0008; }
    .btn:hover { background: #a44; }
    .fade { position: absolute; left: 120px; top: 200px; width: 40px; height: 40px; background: #00f; transition: background-color 1000ms linear; }
    .fade.on { background: #f00; }
    .blur { position: absolute; left: 200px; top: 220px; width: 40px; height: 40px; filter: blur(4px); }
    .blur div { width: 20px; height: 20px; background: #000; }
";

struct Check {
    doc: Doc,
    handle: Handle,
    last: Vec<u8>,
}

impl Check {
    fn raster(scene: Scene) -> Vec<u8> {
        common::rasterize(scene, (W, H))
    }

    fn frame(&self) -> Frame {
        self.frame_within(Duration::from_secs(5)).expect("no frame")
    }

    fn frame_within(&self, wait: Duration) -> Option<Frame> {
        let start = Instant::now();
        loop {
            if let Some(frame) = self.handle.frame() {
                return Some(frame);
            }
            if start.elapsed() > wait {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn step(&mut self, what: &str) -> Rect {
        self.step_rects(what).0
    }

    fn step_rects(&mut self, what: &str) -> (Rect, Vec<Rect>) {
        let rects_of = |f: &Frame| f.plane.damage_rects.clone();
        let seq = self.handle.commit();
        let Some(mut frame) = self.frame_within(Duration::from_secs(2)) else {
            eprintln!("{what}: no frame");
            return (Rect::default(), Vec::new());
        };
        let (mut damage, mut rects) = (frame.plane.damage, rects_of(&frame));
        while frame.commit < seq {
            frame = self.frame();
            if frame.plane.damage.width > 0.0 {
                let (a, b) = (damage, frame.plane.damage);
                damage = match a.width > 0.0 {
                    false => b,
                    true => {
                        let (x, y) = (a.x.min(b.x), a.y.min(b.y));
                        Rect { x, y, width: (a.x + a.width).max(b.x + b.width) - x, height: (a.y + a.height).max(b.y + b.height) - y }
                    }
                };
            }
            rects.extend(rects_of(&frame));
        }
        let now = Self::raster(frame.whole());
        let mut changed = 0;
        for (i, (a, b)) in self.last.chunks(4).zip(now.chunks(4)).enumerate() {
            if a != b {
                changed += 1;
                let (x, y) = ((i as u32 % W) as f64 + 0.5, (i as u32 / W) as f64 + 0.5);
                let inside = |r: &Rect| x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height;
                assert!(rects.iter().any(inside), "{what}: pixel ({x}, {y}) changed outside damage {rects:?}");
                assert!(inside(&damage));
            }
        }
        eprintln!("{what}: {changed} px changed, damage {damage:?} in {} rects", rects.len());
        self.last = now;
        (damage, rects)
    }

    fn el(&self, tag: &str, parent: u32, class: &str) -> u32 {
        let id = self.doc.create_element(tag).unwrap();
        self.doc.set_attribute(id, "class", class).unwrap();
        self.doc.append_child(parent, id).unwrap();
        id
    }
}

fn small(damage: Rect) -> bool {
    damage.width * damage.height < f64::from(W * H) / 4.0
}

#[test]
fn damage_covers_every_changed_pixel() {
    let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
    let handle = doc.handle();
    let mut c = Check { doc, handle, last: vec![0; (W * H * 4) as usize] };
    c.doc.add_stylesheet(CSS).unwrap();
    let body = c.el("body", 0, "");
    let bar = c.el("div", body, "bar");
    let tabs: Vec<u32> = (0..4).map(|_| c.el("div", bar, "")).collect();
    let p = c.el("p", body, "");
    let text = c.doc.create_text("hello damage").unwrap();
    c.doc.append_child(p, text).unwrap();
    let p2 = c.el("p", body, "");
    let text2 = c.doc.create_text("second line").unwrap();
    c.doc.append_child(p2, text2).unwrap();
    let list = c.el("div", body, "list");
    (0..8).for_each(|_| _ = c.el("div", list, "item"));
    c.el("div", body, "btn");
    let fade = c.el("div", body, "fade");
    let blur = c.el("div", body, "blur");
    let blurred = c.el("div", blur, "");

    let first = c.step("first frame");
    assert_eq!((first.width, first.height), (f64::from(W), f64::from(H)));

    c.doc.set_data(text, "hello DAMAGE!!").unwrap();
    assert!(small(c.step("text")));

    c.doc.set_attribute(p2, "class", "hot").unwrap();
    assert!(small(c.step("color")));

    c.doc.set_attribute(tabs[1], "style", "width: 80px").unwrap();
    assert!(small(c.step("flex reflow")));

    c.handle.scroll_at(300.0, 100.0, 0.0, 25.0);
    assert!(small(c.step("scroll")));

    c.handle.pointer(30.0, 210.0);
    assert!(small(c.step("hover")));

    c.doc.set_attribute(fade, "class", "fade on").unwrap();
    c.step("transition start");
    for t in [250.0, 500.0, 1100.0, 10_000.0] {
        c.handle.tick(t);
        assert!(small(c.step("transition")));
    }

    c.doc.set_attribute(blurred, "style", "margin-left: 10px").unwrap();
    assert!(small(c.step("inside blur")));

    c.doc.set_data(text, "far apart").unwrap();
    c.handle.pointer(395.0, 295.0);
    let (union, rects) = c.step_rects("two far apart changes");
    let area: f64 = rects.iter().map(|r| r.width * r.height).sum();
    assert!(rects.len() >= 2 && area < union.width * union.height / 2.0, "{rects:?} vs {union:?}");

    c.doc.set_attribute(p2, "class", "hot").unwrap();
    let none = c.step("no-op");
    assert_eq!(none.width * none.height, 0.0);
}

#[test]
fn reordering_overlapping_boxes_is_damage() {
    let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
    let handle = doc.handle();
    let mut c = Check { doc, handle, last: vec![0; (W * H * 4) as usize] };
    c.doc
        .add_stylesheet(
            "html, body { margin: 0 } .a, .b { position: absolute; width: 80px; height: 80px }
         .a { left: 20px; top: 20px; background: #f00; z-index: 1 } .b { left: 60px; top: 60px; background: #00f; z-index: 2 }
         .a.up { z-index: 3 }",
        )
        .unwrap();
    let body = c.el("body", 0, "");
    let a = c.el("div", body, "a");
    c.el("div", body, "b");
    c.step("first frame");
    c.doc.set_attribute(a, "class", "a up").unwrap();
    let d = c.step("a above b");
    assert!(d.width > 0.0 && small(d), "{d:?}");
}
