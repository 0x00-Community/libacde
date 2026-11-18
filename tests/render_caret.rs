use acde::render::{Options, Rect};

mod common;
use common::{Doc, near};

struct Page {
    doc: Doc,
    body: u32,
}

impl Page {
    fn new(scale: f32) -> Self {
        let doc = Doc::new((400.0 * scale) as u32, (300.0 * scale) as u32, scale, &Options::default()).unwrap();
        doc.add_stylesheet("html, body { margin: 0; } body { font: 20px monospace; } p { margin: 10px 30px; }").unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        Self { doc, body }
    }

    fn el(&self, tag: &str, parent: u32, style: &str) -> u32 {
        let id = self.doc.create_element(tag).unwrap();
        self.doc.set_attribute(id, "style", style).unwrap();
        self.doc.append_child(parent, id).unwrap();
        id
    }

    fn text(&self, parent: u32, text: &str) -> u32 {
        let id = self.doc.create_text(text).unwrap();
        self.doc.append_child(parent, id).unwrap();
        id
    }

    fn caret(&self, id: u32, offset: u32) -> Rect {
        self.doc.caret_rect(id, offset).unwrap()
    }
}

#[test]
fn monospace_offsets_advance_linearly_from_content_box() {
    let page = Page::new(1.0);
    let p = page.el("p", page.body, "");
    let t = page.text(p, "abcdef");
    let (c0, c1) = (page.caret(t, 0), page.caret(t, 1));
    let w = c1.x - c0.x;
    assert!(w > 5.0, "glyph width {w}");
    assert!(near(c0.x, 30.0), "{c0:?}");
    assert!(c0.height > 15.0 && c0.y >= 9.0, "{c0:?}");
    for k in 2..=6 {
        assert!(near(page.caret(t, k).x, c0.x + f64::from(k) * w), "offset {k}");
    }
    assert!(near(page.caret(t, 99).x, c0.x + 6.0 * w));
}

#[test]
fn caret_is_continuous_across_text_nodes_and_elements() {
    let page = Page::new(1.0);
    let p = page.el("p", page.body, "");
    let a = page.text(p, "abc");
    let b = page.el("b", p, "");
    let d = page.text(b, "def");
    let g = page.text(p, "gh");
    assert!(near(page.caret(d, 0).x, page.caret(a, 3).x));
    assert!(near(page.caret(g, 0).x, page.caret(d, 3).x));
    assert!(near(page.caret(p, 4).x, page.caret(d, 1).x));
    assert!(near(page.caret(p, 7).x, page.caret(g, 1).x));
}

#[test]
fn collapsed_whitespace_br_and_wrapping() {
    let page = Page::new(1.0);
    let p = page.el("p", page.body, "");
    let t = page.text(p, "a     b");
    let w = page.caret(t, 1).x - page.caret(t, 0).x;
    assert!(near(page.caret(t, 6).x, page.caret(t, 0).x + 2.0 * w), "{:?}", page.caret(t, 6));

    let q = page.el("p", page.body, "");
    let ab = page.text(q, "ab");
    page.el("br", q, "");
    let cd = page.text(q, "cd");
    let (top, next) = (page.caret(ab, 0), page.caret(cd, 0));
    assert!(near(next.x, top.x) && next.y > top.y + 10.0, "{top:?} {next:?}");

    let narrow = page.el("p", page.body, "width: 100px");
    let long = page.text(narrow, "aaaa bbbb cccc dddd");
    let (first, last) = (page.caret(long, 0), page.caret(long, 15));
    assert!(last.y > first.y + 10.0, "should wrap: {first:?} {last:?}");
}

#[test]
fn scale_does_not_change_css_coordinates() {
    let (one, two) = (Page::new(1.0), Page::new(2.0));
    let carets = [&one, &two].map(|page| {
        let p = page.el("p", page.body, "");
        let t = page.text(p, "hello");
        page.caret(t, 3)
    });
    assert!(near(carets[0].x, carets[1].x) && near(carets[0].y, carets[1].y), "{carets:?}");
}

#[test]
fn utf16_offsets() {
    let page = Page::new(1.0);
    let p = page.el("p", page.body, "");
    let t = page.text(p, "x😀y");
    let (before, after) = (page.caret(t, 1).x, page.caret(t, 3).x);
    assert!(after > before);
    assert!(page.caret(t, 2).x <= after);
    assert!(page.caret(t, 4).x > after);
}

#[test]
fn input_value() {
    let page = Page::new(1.0);
    let input = page.el("input", page.body, "position: absolute; left: 50px; top: 100px; width: 200px; font: 20px monospace");
    page.doc.set_attribute(input, "value", "hello").unwrap();
    let rect = page.doc.rect(input).unwrap();
    let (c0, c5) = (page.caret(input, 0), page.caret(input, 5));
    assert!(c0.x >= rect.x && c0.x < rect.x + 10.0, "{c0:?} in {rect:?}");
    assert!(c0.y >= rect.y && c0.y + c0.height <= rect.y + rect.height + 0.5, "{c0:?} in {rect:?}");
    let w = page.caret(input, 1).x - c0.x;
    assert!(w > 5.0 && near(c5.x, c0.x + 5.0 * w), "{c0:?} {c5:?}");
}

use acde::render::CaretPosition;

impl Page {
    fn at(&self, x: f64, y: f64) -> Option<CaretPosition> {
        self.doc.caret_from_point(x, y)
    }
}

#[test]
fn caret_from_point_round_trips_and_snaps_from_blank_space() {
    let page = Page::new(1.0);
    let p = page.el("p", page.body, "");
    let a = page.text(p, "abc");
    let b = page.el("b", p, "");
    let d = page.text(b, "def");
    for (node, len) in [(a, 3), (d, 3)] {
        for k in 0..len {
            let c = page.caret(node, k);
            let w = page.caret(node, k + 1).x - c.x;
            let got = page.at(c.x + w * 0.25, c.y + c.height / 2.0).unwrap();
            assert_eq!((got.node, got.offset), (node, k), "{node}:{k}");
        }
    }
    let end = page.caret(d, 3);
    let got = page.at(end.x + 100.0, end.y + end.height / 2.0).unwrap();
    assert_eq!((got.node, got.offset), (d, 3));
    let low = page.at(31.0, 290.0).unwrap();
    assert_eq!(low.node, a);
}

#[test]
fn text_rects_one_per_line() {
    let page = Page::new(1.0);
    let p = page.el("p", page.body, "");
    let t = page.text(p, "abcdef");
    let rects = page.doc.text_rects(t, 1, 4).unwrap();
    assert_eq!(rects.len(), 1, "{rects:?}");
    let (c1, c4) = (page.caret(t, 1), page.caret(t, 4));
    assert!(near(rects[0].x, c1.x) && near(rects[0].x + rects[0].width, c4.x), "{rects:?}");
    assert!(near(rects[0].y, c1.y) && near(rects[0].height, c1.height));
    let glyph = page.caret(t, 1).x - page.caret(t, 0).x;
    let narrow = page.el("p", page.body, &format!("width: {}px", glyph * 6.5));
    let long = page.text(narrow, "aaaa bbbb cccc");
    let rects = page.doc.text_rects(long, 0, 14).unwrap();
    assert_eq!(rects.len(), 3, "{rects:?}");
    assert!(rects[1].y > rects[0].y && rects[2].y > rects[1].y);
    let empty = page.doc.text_rects(t, 2, 2).unwrap();
    assert!(empty.len() == 1 && near(empty[0].x, page.caret(t, 2).x));
}
