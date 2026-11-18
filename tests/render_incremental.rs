use acde::render::{Options, Stats};

mod common;
use common::{Doc, Handle};

const CSS: &str = "html, body { margin: 0; font: 14px sans-serif; }
    .card { position: relative; width: 200px; padding: 8px; margin: 6px; background: #eef; border-radius: 8px; box-shadow: 0 2px 6px rgba(0,0,0,.2); }
    .fade { opacity: .6; }
    .scroller { height: 60px; overflow: auto; background: #efe; }
    .row { height: 24px; }
    .moved { transform: translate(30px, 4px) rotate(3deg); }
    .bg { width: 20px; height: 20px; background: url(data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAYAAACp8Z5+AAAAEklEQVR4nGP4z8DwHxkzkC4AADxAH+HggXe0AAAAAElFTkSuQmCC); background-size: 100% 100%; }";

struct Page {
    doc: Doc,
    body: u32,
}

impl Page {
    fn new() -> Self {
        let options = Options { verify_paint: Some(true), ..Options::default() };
        let doc = Doc::new(400, 300, 1.0, &options).unwrap();
        doc.add_stylesheet(CSS).unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        Self { doc, body }
    }

    fn el(&self, parent: u32, tag: &str, class: &str, text: Option<&str>) -> u32 {
        let n = self.doc.create_element(tag).unwrap();
        self.doc.set_attribute(n, "class", class).unwrap();
        self.doc.append_child(parent, n).unwrap();
        if let Some(text) = text {
            let t = self.doc.create_text(text).unwrap();
            self.doc.append_child(n, t).unwrap();
        }
        n
    }

    fn frame(&self, label: &str) -> Stats {
        self.doc.scene();
        let stats = self.doc.stats();
        assert_eq!(stats.paint_mismatches, 0, "{label}: incremental frame differs from a full recording");
        stats
    }

    fn sample() -> (Self, [u32; 6]) {
        let page = Self::new();
        let a = page.el(page.body, "div", "card", Some("plain card"));
        let b = page.el(page.body, "div", "card fade", Some("faded card"));
        let text = page.el(b, "span", "", Some(" more"));
        let scroller = page.el(page.body, "div", "scroller", None);
        for i in 0..6 {
            page.el(scroller, "div", "row", Some(&format!("row {i}")));
        }
        let svg = page.doc.create_element_ns(1, "svg").unwrap();
        page.doc.set_attribute(svg, "width", "20").unwrap();
        page.doc.set_attribute(svg, "height", "20").unwrap();
        let circle = page.doc.create_element_ns(1, "circle").unwrap();
        for (k, v) in [("cx", "10"), ("cy", "10"), ("r", "8"), ("fill", "#96f")] {
            page.doc.set_attribute(circle, k, v).unwrap();
        }
        page.doc.append_child(svg, circle).unwrap();
        page.doc.append_child(a, svg).unwrap();
        page.frame("first");
        page.frame("second");
        (page, [a, b, text, scroller, svg, circle])
    }
}

#[test]
fn unchanged_frame_reuses_everything() {
    let (page, _) = Page::sample();
    let s = page.frame("unchanged");
    assert_eq!(s.fragments_recorded, 0, "{s:?}");
    assert!(s.fragments_reused >= 4, "{s:?}");
}

#[test]
fn text_change_rerecords_only_its_fragment() {
    let (page, [_, b, ..]) = Page::sample();
    let t = page.doc.create_text(" changed").unwrap();
    page.doc.append_child(b, t).unwrap();
    let s = page.frame("text appended");
    assert!(s.fragments_recorded <= 2, "{s:?}");
}

#[test]
fn text_edited_in_place() {
    let page = Page::new();
    let p = page.el(page.body, "p", "", None);
    let t = page.doc.create_text("before").unwrap();
    page.doc.append_child(p, t).unwrap();
    page.frame("first");
    page.frame("second");
    for (i, text) in ["after", "after!", "a much longer text than before"].into_iter().enumerate() {
        page.doc.set_data(t, text).unwrap();
        page.frame(&format!("set_text #{i}"));
    }
}

#[test]
fn style_and_transform_changes() {
    let (page, [a, b, text, ..]) = Page::sample();
    page.doc.set_attribute(text, "style", "color: red").unwrap();
    page.frame("inline style on a span");
    page.doc.set_attribute(a, "class", "card moved").unwrap();
    page.frame("transform added");
    page.doc.set_attribute(a, "style", "transform: translate(50px, 0)").unwrap();
    page.frame("transform changed");
    page.doc.set_attribute(b, "style", "background: #fee").unwrap();
    page.frame("background of an opacity layer");
}

#[test]
fn boundary_set_changes() {
    let (page, [a, ..]) = Page::sample();
    page.doc.set_attribute(a, "class", "card fade").unwrap();
    page.frame("becomes a stacking context");
    page.doc.set_attribute(a, "class", "card").unwrap();
    page.frame("no longer a stacking context");
    page.doc.set_attribute(a, "style", "overflow: hidden").unwrap();
    page.frame("becomes a clip");
    page.doc.remove_attribute(a, "style").unwrap();
    page.frame("no longer a clip");
}

#[test]
fn scrolling_inserting_removing() {
    let (page, [a, _, _, scroller, svg, circle]) = Page::sample();
    page.doc.scroll_by(scroller, 0.0, 30.0).unwrap();
    page.frame("scrolled");
    let row = page.el(scroller, "div", "row", Some("new row"));
    page.frame("row inserted");
    page.doc.remove(row).unwrap();
    page.frame("row removed");
    page.doc.set_attribute(circle, "fill", "#f00").unwrap();
    page.frame("svg attribute");
    page.doc.remove(svg).unwrap();
    page.frame("svg removed");
    page.doc.remove(a).unwrap();
    page.frame("card removed");
}

#[test]
fn image_arrival_and_resize() {
    let (page, _) = Page::sample();
    page.el(page.body, "div", "bg", None);
    for i in 0..60 {
        page.frame(&format!("waiting for the image, frame {i}"));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    page.doc.resize(320, 240, 2.0);
    page.frame("resized and rescaled");
}

#[test]
fn damage_by_fragment_covers_full_diff() {
    let (page, [a, b, text, scroller, svg, circle]) = Page::sample();
    let handle: Handle = page.doc.handle();
    let frame = |label: &str| {
        handle.commit();
        let start = std::time::Instant::now();
        while handle.frame().is_none() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5), "{label}: no frame");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let s = page.doc.stats();
        assert_eq!(s.paint_mismatches, 0, "{label}: paint");
        assert_eq!(s.damage_mismatches, 0, "{label}: damage");
        s
    };
    page.doc.set_attribute(page.body, "style", "background: #fafafa").unwrap();
    frame("first");
    type Step<'a> = (&'a str, Box<dyn Fn() + 'a>);
    let steps: [Step; 8] = [
        ("text", Box::new(|| page.doc.set_attribute(text, "style", "color: red").map(drop).unwrap())),
        ("class", Box::new(|| page.doc.set_attribute(a, "class", "card moved").map(drop).unwrap())),
        ("opacity layer", Box::new(|| page.doc.set_attribute(b, "style", "background: #fee").map(drop).unwrap())),
        ("scroll", Box::new(|| page.doc.scroll_by(scroller, 0.0, 20.0).unwrap())),
        ("svg attribute", Box::new(|| page.doc.set_attribute(circle, "fill", "#0a0").map(drop).unwrap())),
        ("becomes a stacking context", Box::new(|| page.doc.set_attribute(a, "class", "card fade").map(drop).unwrap())),
        ("svg removed", Box::new(|| page.doc.remove(svg).unwrap())),
        ("card removed", Box::new(|| page.doc.remove(a).unwrap())),
    ];
    for (label, step) in &steps {
        step();
        frame(label);
    }
    let s = page.doc.stats();
    assert!(s.damage_by_fragment >= steps.len() as u32, "{s:?}");
}

#[test]
fn animated_scale_with_positioned_children() {
    let page = Page::new();
    page.doc
        .add_stylesheet(
            "@keyframes pop { from { transform: scale(.9); opacity: .5 } to { transform: scale(1); opacity: 1 } }
             .win { position: absolute; left: 40px; top: 30px; width: 300px; height: 200px; background: #fff;
                    border-radius: 10px; animation: pop 400ms ease-out; }
             .bar { position: absolute; right: 12px; top: 8px; width: 24px; height: 24px; border-radius: 6px;
                    background: linear-gradient(#fff, #ddd); }
             .ico { position: absolute; left: 10px; top: 40px; width: 40px; height: 40px; overflow: hidden; }",
        )
        .unwrap();
    let win = page.el(page.body, "div", "win", Some("title"));
    page.el(win, "div", "bar", None);
    let ico = page.el(win, "div", "ico", None);
    page.el(ico, "div", "bar", None);
    let handle = page.doc.handle();
    for i in 0..40 {
        handle.tick(f64::from(i) * 16.0);
        handle.commit();
        let start = std::time::Instant::now();
        while handle.frame().is_none() && start.elapsed() < std::time::Duration::from_millis(200) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let s = page.doc.stats();
        assert_eq!(s.paint_mismatches, 0, "frame {i}");
    }
}

#[test]
fn offsets_follow_a_scaled_ancestor() {
    let page = Page::new();
    page.doc
        .add_stylesheet(
            ".zoom { position: absolute; left: 20px; top: 20px; width: 150px; height: 120px; transform: scale(2); transform-origin: 0 0;
                     transition: transform 400ms linear; }
             .zoom.small { transform: scale(1.5) rotate(2deg); }
             .shadow { position: absolute; left: 5px; top: 5px; width: 30px; height: 20px; opacity: .9; background: #9cf; box-shadow: 8px 6px 0 #c33; }
             .pad { position: absolute; left: 60px; top: 5px; width: 20px; height: 20px; padding: 4px; }
             .list { position: absolute; left: 5px; top: 40px; width: 50px; height: 40px; overflow: auto; }
             .item { height: 15px; margin-bottom: 5px; background: #69c; }",
        )
        .unwrap();
    let zoom = page.el(page.body, "div", "zoom", None);
    page.el(zoom, "div", "shadow", None);
    let pad = page.el(zoom, "div", "pad", None);
    let svg = page.doc.create_element_ns(1, "svg").unwrap();
    for (k, v) in [("width", "20"), ("height", "20"), ("viewBox", "0 0 20 20")] {
        page.doc.set_attribute(svg, k, v).unwrap();
    }
    let rect = page.doc.create_element_ns(1, "rect").unwrap();
    for (k, v) in [("width", "20"), ("height", "20"), ("fill", "#f80")] {
        page.doc.set_attribute(rect, k, v).unwrap();
    }
    page.doc.append_child(svg, rect).unwrap();
    page.doc.append_child(pad, svg).unwrap();
    let tree = page.doc.create_element_ns(1, "svg").unwrap();
    for (k, v) in [("width", "24"), ("height", "24"), ("viewBox", "0 0 24 24")] {
        page.doc.set_attribute(tree, k, v).unwrap();
    }
    let defs = page.doc.create_element_ns(1, "defs").unwrap();
    let gradient = page.doc.create_element_ns(1, "linearGradient").unwrap();
    page.doc.set_attribute(gradient, "id", "g").unwrap();
    for (offset, color) in [("0", "#fff"), ("1", "#96f")] {
        let stop = page.doc.create_element_ns(1, "stop").unwrap();
        page.doc.set_attribute(stop, "offset", offset).unwrap();
        page.doc.set_attribute(stop, "stop-color", color).unwrap();
        page.doc.append_child(gradient, stop).unwrap();
    }
    page.doc.append_child(defs, gradient).unwrap();
    page.doc.append_child(tree, defs).unwrap();
    let group = page.doc.create_element_ns(1, "g").unwrap();
    page.doc.set_attribute(group, "opacity", ".5").unwrap();
    let filled = page.doc.create_element_ns(1, "rect").unwrap();
    for (k, v) in [("x", "2"), ("y", "2"), ("width", "20"), ("height", "20"), ("fill", "url(#g)")] {
        page.doc.set_attribute(filled, k, v).unwrap();
    }
    page.doc.append_child(group, filled).unwrap();
    page.doc.append_child(tree, group).unwrap();
    let holder = page.el(zoom, "div", "pad", None);
    page.doc.set_attribute(holder, "style", "left: 100px").unwrap();
    page.doc.append_child(holder, tree).unwrap();
    let list = page.el(zoom, "div", "list", None);
    for _ in 0..4 {
        page.el(list, "div", "item", None);
    }
    let handle = page.doc.handle();
    let mut now = 0.0;
    let mut tick = |label: &str| {
        now += 16.0;
        handle.tick(now);
        handle.commit();
        let start = std::time::Instant::now();
        while handle.frame().is_none() && start.elapsed() < std::time::Duration::from_millis(200) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let s = page.doc.stats();
        assert_eq!(s.paint_mismatches, 0, "{label}");
        s
    };
    tick("first");
    page.doc.scroll_by(list, 0.0, 10.0).unwrap();
    tick("scrolled");
    page.doc.set_attribute(zoom, "class", "zoom small").unwrap();
    let mut reused = 0;
    for i in 0..20 {
        reused += tick(&format!("transition frame {i}")).fragments_reused;
    }
    assert!(reused > 0, "the inner fragments should be reused while only the ancestor animates");
}
