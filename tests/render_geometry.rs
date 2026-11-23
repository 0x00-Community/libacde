use acde::render::{Options, Rect};

mod common;
use common::Doc;

struct Page {
    doc: Doc,
    body: u32,
}

impl Page {
    fn new(css: &str) -> Self {
        let doc = Doc::new(800, 600, 1.0, &Options::default()).unwrap();
        doc.add_stylesheet("html, body { margin: 0; height: 100%; font: 20px monospace; }").unwrap();
        doc.add_stylesheet(css).unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        Self { doc, body }
    }

    fn el(&self, tag: &str, parent: u32, id: &str) -> u32 {
        let n = self.doc.create_element(tag).unwrap();
        self.doc.set_attribute(n, "id", id).unwrap();
        self.doc.append_child(parent, n).unwrap();
        n
    }

    fn hit(&self, x: f64, y: f64) -> Option<u32> {
        self.doc.hit_test(x, y)
    }

    fn center(r: Rect) -> (f64, f64) {
        (r.x + r.width / 2.0, r.y + r.height / 2.0)
    }
}

fn near(a: Rect, b: [f64; 4]) -> bool {
    [a.x, a.y, a.width, a.height].iter().zip(b).all(|(x, y)| (x - y).abs() < 0.6)
}

const STAGE: &str = "
    #stage { position: absolute; left: 0; top: 0; width: 400px; height: 300px; transform-origin: 0 0; }
    #wall { position: absolute; inset: 0; }
    #wallpaper { position: absolute; inset: 0; width: 100%; height: 100%; }
    #spaces { position: absolute; inset: 0; z-index: 1; isolation: isolate; }
    #win { position: absolute; left: 100px; top: 50px; width: 200px; height: 100px; z-index: 11; }
";

#[test]
fn singular_stage_is_not_hittable_and_scaled_stage_hits_window() {
    let page = Page::new(STAGE);
    let stage = page.el("div", page.body, "stage");
    let wall = page.el("div", stage, "wall");
    let wallpaper = page.el("canvas", wall, "wallpaper");
    let spaces = page.el("div", stage, "spaces");
    let win = page.el("div", spaces, "win");

    page.doc.set_attribute(stage, "style", "transform: translate(0px, 0px) scale(0)").unwrap();
    let hit = page.hit(200.0, 100.0);
    assert!(hit != Some(wallpaper) && hit != Some(win), "scale(0) stage must not be hit: {hit:?}");
    assert_eq!(page.doc.rect(win).unwrap().width, 0.0);

    page.doc.set_attribute(stage, "style", "transform: translate(100px, 50px) scale(2)").unwrap();
    let r = page.doc.rect(win).unwrap();
    assert!(near(r, [300.0, 150.0, 400.0, 200.0]), "{r:?}");
    let (x, y) = Page::center(r);
    assert_eq!(page.hit(x, y), Some(win));
    assert_eq!(page.hit(120.0, 60.0), Some(spaces));
}

#[test]
fn rect_follows_rotation_and_nested_transforms() {
    let page = Page::new(
        "#a { position: absolute; left: 100px; top: 100px; width: 100px; height: 50px; transform: rotate(90deg); }
         #b { position: absolute; left: 300px; top: 100px; width: 200px; height: 200px; transform: scale(0.5); }
         #c { position: absolute; left: 0; top: 0; width: 100px; height: 100px; transform: translateX(100px); }",
    );
    let a = page.el("div", page.body, "a");
    let b = page.el("div", page.body, "b");
    let c = page.el("div", b, "c");
    assert!(near(page.doc.rect(a).unwrap(), [125.0, 75.0, 50.0, 100.0]), "{:?}", page.doc.rect(a));
    assert!(near(page.doc.rect(b).unwrap(), [350.0, 150.0, 100.0, 100.0]));
    let rc = page.doc.rect(c).unwrap();
    assert!(near(rc, [400.0, 150.0, 50.0, 50.0]), "{rc:?}");
    let (x, y) = Page::center(rc);
    assert_eq!(page.hit(x, y), Some(c));
}

#[test]
fn caret_under_scaled_ancestor() {
    let page = Page::new("#s { position: absolute; left: 0; top: 0; transform-origin: 0 0; transform: translate(50px, 20px) scale(2); }");
    let s = page.el("div", page.body, "s");
    let p = page.el("p", s, "p");
    let t = page.doc.create_text("abcd").unwrap();
    page.doc.append_child(p, t).unwrap();
    let (c0, c1) = (page.doc.caret_rect(t, 0).unwrap(), page.doc.caret_rect(t, 1).unwrap());
    let pr = page.doc.rect(p).unwrap();
    assert!((c0.x - pr.x).abs() < 0.6 && c0.height > 30.0, "{c0:?} {pr:?}");
    let w = c1.x - c0.x;
    let at = page.doc.caret_from_point(c1.x + w * 0.25, c1.y + c1.height / 2.0).unwrap();
    assert_eq!((at.node, at.offset), (t, 1));
}

#[test]
fn metrics_ignore_transform() {
    let page = Page::new(
        "#box { position: absolute; left: 10px; top: 20px; width: 100px; height: 60px; border: 3px solid; padding: 5px;
                overflow: scroll; transform: scale(3); }
         #inner { height: 300px; }",
    );
    let b = page.el("div", page.body, "box");
    page.el("div", b, "inner");
    let m = page.doc.metrics(b).unwrap();
    assert_eq!((m.offset_left, m.offset_top, m.offset_width, m.offset_height), (10.0, 20.0, 116.0, 76.0));
    assert_eq!((m.client_left, m.client_top), (3.0, 3.0));
    assert!(m.client_width <= 110.0 && m.client_width > 90.0, "{m:?}");
    assert!(m.scroll_height >= 300.0, "{m:?}");
    page.doc.scroll_by(b, 0.0, 40.0).unwrap();
    assert_eq!(page.doc.metrics(b).unwrap().scroll_top, 40.0);
    page.doc.hit_test(5.0, 5.0);
    assert!(page.doc.rect(b).unwrap().width > 300.0);
}

#[test]
fn caret_from_point_ignores_text_under_singular_transform() {
    let page = Page::new("#hidden { transform: scale(0); } #shown { margin-top: 200px; }");
    let hidden = page.el("p", page.body, "hidden");
    let secret = page.doc.create_text("invisible").unwrap();
    page.doc.append_child(hidden, secret).unwrap();
    let shown = page.el("p", page.body, "shown");
    let text = page.doc.create_text("visible").unwrap();
    page.doc.append_child(shown, text).unwrap();
    let at = page.doc.caret_from_point(5.0, 5.0).unwrap();
    assert_eq!(at.node, text);
}

#[test]
fn button_is_border_box() {
    let page = Page::new("#box { width: 300px; } #b { width: 100%; padding: 7px 10px; }");
    let b = page.el("div", page.body, "box");
    let button = page.el("button", b, "b");
    assert_eq!(page.doc.rect(button).unwrap().width, 300.0);
}

#[test]
fn own_scroll_does_not_move_the_container() {
    let page = Page::new(".s { position: absolute; left: 20px; top: 20px; width: 200px; height: 100px; overflow: auto } .tall { height: 400px }");
    let s = page.el("div", page.body, "s");
    page.doc.set_attribute(s, "class", "s").unwrap();
    let tall = page.el("div", s, "tall");
    page.doc.set_attribute(tall, "class", "tall").unwrap();
    page.doc.scene();
    page.doc.scroll_by(s, 0.0, 30.0).unwrap();
    let (rs, rt) = (page.doc.rect(s).unwrap(), page.doc.rect(tall).unwrap());
    assert_eq!((rs.x, rs.y, rs.height), (20.0, 20.0, 100.0));
    assert_eq!(rt.y, -10.0);
}

#[test]
fn absolute_child_of_empty_body_is_hit() {
    let page = Page::new("html, body { height: 0 } .a { position: absolute; left: 10px; top: 10px; width: 50px; height: 50px }");
    let a = page.el("div", page.body, "a");
    page.doc.set_attribute(a, "class", "a").unwrap();
    assert_eq!(page.hit(20.0, 20.0), Some(a));
}

#[test]
fn shrink_to_fit_box_with_percentage_padding_keeps_one_line() {
    let page = Page::new(
        ".box { position: relative; width: 301px; height: 188px }
         .empty { position: absolute; left: 50%; top: 46%; padding: 7% 10%; line-height: 0 }
         .empty span { display: inline-block; width: 11px; height: 10px; vertical-align: top }",
    );
    let doc = &page.doc;
    let b = page.el("div", page.body, "box");
    doc.set_attribute(b, "class", "box").unwrap();
    let empty = page.el("div", b, "empty");
    doc.set_attribute(empty, "class", "empty").unwrap();
    page.el("span", empty, "a");
    page.el("span", empty, "b");
    let r = doc.rect(empty).unwrap();
    assert!((r.width - 82.2).abs() < 0.01 && (r.height - 52.14).abs() < 0.01, "{r:?}");
}

#[test]
fn prefers_color_scheme_follows_the_option_and_set_dark() {
    for dark in [false, true] {
        let doc = Doc::new(800, 600, 1.0, &Options { dark, ..Default::default() }).unwrap();
        doc.add_stylesheet("#dot { width: 10px; height: 10px; } @media (prefers-color-scheme: dark) { #dot { width: 30px; } }").unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        let dot = doc.create_element("div").unwrap();
        doc.set_attribute(dot, "id", "dot").unwrap();
        doc.append_child(body, dot).unwrap();
        assert_eq!(doc.rect(dot).unwrap().width, if dark { 30.0 } else { 10.0 });
        assert_eq!(doc.dom.match_media("(prefers-color-scheme: dark)"), dark);
        doc.dom.set_dark(!dark);
        assert_eq!(doc.rect(dot).unwrap().width, if dark { 10.0 } else { 30.0 });
        assert_eq!(doc.dom.match_media("(prefers-color-scheme: light)"), dark);
    }
}

#[test]
fn prefers_reduced_transparency_follows_the_glass_preference() {
    let doc = Doc::new(800, 600, 1.0, &Options::default()).unwrap();
    doc.add_stylesheet("#dot { width: 10px; height: 10px; } @media (prefers-reduced-transparency: reduce) { #dot { width: 30px; } }").unwrap();
    let body = doc.create_element("body").unwrap();
    doc.append_child(0, body).unwrap();
    let dot = doc.create_element("div").unwrap();
    doc.set_attribute(dot, "id", "dot").unwrap();
    doc.append_child(body, dot).unwrap();
    for reduced in [false, true, false] {
        doc.dom.set_glass(0, reduced);
        assert_eq!(doc.rect(dot).unwrap().width, if reduced { 30.0 } else { 10.0 }, "the style follows {reduced}");
        assert_eq!(doc.dom.match_media("(prefers-reduced-transparency: reduce)"), reduced);
        assert_eq!(doc.dom.match_media("(prefers-reduced-transparency: no-preference)"), !reduced);
    }
}
