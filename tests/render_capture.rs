use acde::render::{Capture, Options};

mod common;
use common::Doc;

struct Page {
    doc: Doc,
    body: u32,
}

impl Page {
    fn new(css: &str) -> Self {
        let doc = Doc::new(800, 600, 1.0, &Options::default()).unwrap();
        doc.add_stylesheet("html, body { margin: 0; } .box { position: absolute; left: 10px; top: 30px; width: 40px; height: 20px; background: #f00; }")
            .unwrap();
        doc.add_stylesheet(css).unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        Self { doc, body }
    }

    fn el(&self, parent: u32, class: &str) -> u32 {
        let n = self.doc.create_element("div").unwrap();
        self.doc.set_attribute(n, "class", class).unwrap();
        self.doc.append_child(parent, n).unwrap();
        n
    }

    fn capture(&self, id: u32, scale: f64) -> Option<Capture> {
        self.doc.capture(id, scale).unwrap()
    }
}

#[test]
fn solid_box_is_its_border_box() {
    let page = Page::new("");
    let el = page.el(page.body, "box");
    let c = page.capture(el, 0.0).unwrap();
    assert_eq!((c.x, c.y, c.width, c.height), (10.0, 30.0, 40.0, 20.0));
    assert_eq!((c.pixel_width, c.pixel_height, c.scale), (40, 20, 1.0));
    let rgba = c.pixels();
    assert_eq!(rgba.len(), 40 * 20 * 4);
    assert!(rgba.chunks(4).all(|p| p == [255, 0, 0, 255]));
}

#[test]
fn opacity_gives_straight_alpha() {
    let page = Page::new(".box { opacity: .5; }");
    let el = page.el(page.body, "box");
    let [r, g, b, a] = page.capture(el, 0.0).unwrap().pixel(20, 10);
    assert_eq!((r, g, b), (255, 0, 0));
    assert!(a.abs_diff(128) <= 1, "alpha {a}");
}

#[test]
fn ancestor_transform_and_opacity_are_left_out() {
    let page = Page::new(".outer { transform: scale(3); opacity: .2; }");
    let outer = page.el(page.body, "outer");
    let el = page.el(outer, "box");
    let c = page.capture(el, 0.0).unwrap();
    assert_eq!((c.pixel_width, c.pixel_height), (40, 20));
    assert_eq!(c.pixel(20, 10), [255, 0, 0, 255]);
}

#[test]
fn shadow_is_part_of_the_bounds() {
    let page = Page::new(".box { box-shadow: 8px 0 0 #00f; }");
    let el = page.el(page.body, "box");
    let c = page.capture(el, 0.0).unwrap();
    assert_eq!((c.x, c.y, c.width, c.height), (10.0, 30.0, 48.0, 20.0));
    assert_eq!(c.pixel(45, 10), [0, 0, 255, 255]);
}

#[test]
fn scale_and_size_cap() {
    let page = Page::new(".wide { width: 3000px; }");
    let el = page.el(page.body, "box");
    let c = page.capture(el, 2.0).unwrap();
    assert_eq!((c.pixel_width, c.pixel_height, c.scale), (80, 40, 2.0));
    assert_eq!((c.width, c.height), (40.0, 20.0));

    let wide = page.el(page.body, "box wide");
    let c = page.capture(wide, 0.0).unwrap();
    assert_eq!(c.pixel_width, 2048);
    assert!((c.scale - 2048.0 / 3000.0).abs() < 1e-9, "{}", c.scale);
    assert_eq!(c.width, 3000.0);
}

#[test]
fn nothing_painted_is_none() {
    let page = Page::new(".gone { display: none; } .hidden { visibility: hidden; } .empty { background: none; }");
    for class in ["box gone", "box hidden", "box empty"] {
        let el = page.el(page.body, class);
        assert!(page.capture(el, 0.0).is_none(), "{class}");
    }
}

#[test]
fn file_row_timing() {
    let page = Page::new(
        ".row { position: absolute; left: 0; top: 0; width: 420px; height: 36px; display: flex; align-items: center; gap: 8px; padding: 0 10px; border-radius: 8px; background: rgba(150, 92, 254, .12); font: 13px sans-serif; } .ic { width: 20px; height: 24px; border-radius: 4px; background: #b9a0e8; }",
    );
    let row = page.el(page.body, "row");
    page.el(row, "ic");
    let name = page.el(row, "name");
    let text = page.doc.create_text("季度报告-Q3-final-version-包含所有附件和图表的完整版本.docx").unwrap();
    page.doc.append_child(name, text).unwrap();
    page.capture(row, 0.0).unwrap();
    let start = std::time::Instant::now();
    let n = 20;
    for _ in 0..n {
        page.capture(row, 2.0).unwrap();
    }
    eprintln!("capture 420x36 @2x: {:.2} ms each", start.elapsed().as_secs_f64() * 1e3 / f64::from(n));
}
