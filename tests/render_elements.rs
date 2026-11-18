use acde::gpu::RenderCommand;
use acde::render::{Options, Rect};

mod common;
use common::{Doc, near};

const W: u32 = 480;
const H: u32 = 320;

fn page(html: &str) -> Doc {
    let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
    doc.dom.parse_document(html).unwrap();
    doc.commit();
    doc
}

fn rect(doc: &Doc, id: &str) -> Rect {
    doc.rect(doc.dom.get_element_by_id(id)).unwrap()
}

fn pixels(doc: &Doc) -> Vec<u8> {
    common::rasterize(doc.scene(), (W, H))
}

fn rgb(pixels: &[u8], x: f64, y: f64) -> [u8; 3] {
    let i = ((y as usize) * W as usize + x as usize) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

const FIELDSET: &str = "<!doctype html><style>
    body { margin: 0; font: 16px/20px monospace; }
    fieldset { margin: 10px; padding: 8px; border: 4px solid rgb(255, 0, 0); width: 300px; }
    legend { padding: 0 2px; }
    .item { width: 50px; height: 30px; }
</style>";

#[test]
fn legend_of_a_flex_fieldset_sits_on_the_border_not_in_the_row() {
    let doc = page(&format!(
        "{FIELDSET}<style>fieldset {{ display: flex; gap: 6px; }}</style>
        <fieldset id=f><legend id=l>Title</legend><div class=item id=a></div><div class=item id=b></div></fieldset>"
    ));
    let (f, l, a, b) = (rect(&doc, "f"), rect(&doc, "l"), rect(&doc, "a"), rect(&doc, "b"));
    assert!(near(l.y, f.y), "the legend's top is the fieldset's: {l:?} {f:?}");
    assert!(near(l.height, 20.0), "{l:?}");
    assert!(near(l.x, f.x + 4.0 + 8.0), "the legend starts at the content edge: {l:?} {f:?}");
    assert!(l.width < 100.0, "the legend shrinks to fit: {l:?}");
    assert!(near(a.x, f.x + 4.0 + 8.0), "the first item starts the row: {a:?} {f:?}");
    assert!(near(b.x, a.x + 50.0 + 6.0), "the items share one row: {a:?} {b:?}");
    assert!(near(a.y, b.y), "{a:?} {b:?}");
    assert!(near(a.y, f.y + 20.0 + 8.0), "the row starts below the legend: {a:?} {f:?}");
    assert!(near(f.height, 20.0 + 8.0 + 30.0 + 8.0 + 4.0), "{f:?}");
}

#[test]
fn legend_of_a_block_fieldset_is_above_the_content() {
    let doc = page(&format!("{FIELDSET}<fieldset id=f><legend id=l>Title</legend><div class=item id=a></div></fieldset>"));
    let (f, l, a) = (rect(&doc, "f"), rect(&doc, "l"), rect(&doc, "a"));
    assert!(near(l.y, f.y), "{l:?} {f:?}");
    assert!(l.width < 100.0, "the legend shrinks to fit: {l:?}");
    assert!(near(a.y, f.y + 20.0 + 8.0), "{a:?} {f:?}");
    assert!(near(f.height, 20.0 + 8.0 + 30.0 + 8.0 + 4.0), "{f:?}");
}

#[test]
fn a_thick_border_centres_the_legend() {
    let doc = page(&format!(
        "{FIELDSET}<style>fieldset {{ border-width: 30px; }}</style>
        <fieldset id=f><legend id=l>Title</legend><div class=item id=a></div></fieldset>"
    ));
    let (f, l, a) = (rect(&doc, "f"), rect(&doc, "l"), rect(&doc, "a"));
    assert!(near(l.y, f.y + 5.0), "{l:?} {f:?}");
    assert!(near(a.y, f.y + 30.0 + 8.0), "{a:?} {f:?}");
}

#[test]
fn floated_or_positioned_legends_are_not_rendered_legends() {
    let doc = page(&format!(
        "{FIELDSET}<style>#l {{ float: left; }}</style>
        <fieldset id=f><legend id=l>Title</legend><div class=item id=a></div></fieldset>"
    ));
    let (f, l) = (rect(&doc, "f"), rect(&doc, "l"));
    assert!(near(l.y, f.y + 4.0 + 8.0), "a floated legend is in the content: {l:?} {f:?}");
}

#[test]
fn a_definite_height_keeps_the_border_box() {
    let doc = page(&format!(
        "{FIELDSET}<style>fieldset {{ height: 100px; }}</style>
        <fieldset id=f><legend id=l>Title</legend><div class=item id=a></div></fieldset>"
    ));
    let f = rect(&doc, "f");
    assert!(near(f.height, 100.0 + 8.0 + 8.0 + 4.0 + 4.0), "{f:?}");
}

#[test]
fn the_border_is_centred_on_the_legend_and_open_behind_it() {
    let doc = page(&format!(
        "{FIELDSET}<style>legend {{ color: transparent; }}</style>
        <fieldset id=f><legend id=l>Title</legend><div class=item id=a></div></fieldset>"
    ));
    let (f, l) = (rect(&doc, "f"), rect(&doc, "l"));
    let px = pixels(&doc);
    let mid = l.y + l.height / 2.0;
    let red = |p: [u8; 3]| p[0] > 200 && p[1] < 60 && p[2] < 60;
    assert!(red(rgb(&px, f.x + 1.0, mid)), "the left border reaches the legend's middle");
    assert!(!red(rgb(&px, f.x + 1.0, f.y + 1.0)), "nothing above the border line");
    assert!(red(rgb(&px, l.x + l.width + 10.0, mid)), "the top border runs at the legend's middle");
    assert!(!red(rgb(&px, l.x + l.width / 2.0, mid)), "no border behind the legend");
}

fn glyph_xs(doc: &Doc) -> Vec<f64> {
    doc.scene()
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::GlyphRun(run) => run.glyphs.first().map(|g| (run.transform * kurbo::Point::new(g.x.into(), g.y.into())).x),
            _ => None,
        })
        .collect()
}

#[test]
fn ordered_markers_keep_their_space_before_the_text() {
    let doc = page(
        "<!doctype html><style>body { margin: 0; font: 20px monospace; } ol, ul { margin: 0; padding-left: 100px; }</style>
        <span id=probe>1. </span><ol id=o><li>abc</li></ol>",
    );
    let probe = doc.dom.first_child(doc.dom.get_element_by_id("probe")).unwrap();
    let advance = doc.text_rects(probe, 0, 2).unwrap()[0].width / 2.0;
    let xs = glyph_xs(&doc);
    let (text, marker) = (xs[xs.len() - 2], xs[xs.len() - 1]);
    assert!(near(text, 100.0), "{xs:?}");
    assert!(near(text - marker, 3.0 * advance), "\"1. \" ends where the text starts: {xs:?}, advance {advance}");
}

#[test]
fn the_gap_follows_the_legend_when_it_changes() {
    let doc = Doc::new(W, H, 1.0, &Options { verify_paint: Some(true), ..Options::default() }).unwrap();
    let rows: String = (0..16).map(|i| format!("<div>row {i}</div>")).collect();
    doc.dom
        .parse_document(&format!(
            "{FIELDSET}<style>legend {{ color: transparent; }} div {{ height: 4px; overflow: hidden; }}</style>
            <fieldset id=f><legend id=l>T</legend>{rows}</fieldset>"
        ))
        .unwrap();
    doc.commit();
    let before = rect(&doc, "l");
    pixels(&doc);
    let text = doc.dom.first_child(doc.dom.get_element_by_id("l")).unwrap();
    doc.set_data(text, "A much longer title").unwrap();
    doc.commit();
    let (f, l) = (rect(&doc, "f"), rect(&doc, "l"));
    assert!(l.width > before.width + 50.0, "{before:?} {l:?}");
    let px = pixels(&doc);
    assert_eq!(doc.stats().paint_mismatches, 0, "incremental frame differs from a full recording");
    let red = |p: [u8; 3]| p[0] > 200 && p[1] < 60 && p[2] < 60;
    let mid = l.y + l.height / 2.0;
    assert!(!red(rgb(&px, l.x + l.width - 4.0, mid)), "no border behind the longer legend");
    assert!(red(rgb(&px, f.x + f.width - 10.0, mid)), "the border after it");
}

#[test]
fn css_supports_answers_from_the_parser() {
    let doc = page("<!doctype html><p>x</p>");
    let dom = &doc.dom;
    assert!(dom.css_supports("(display: grid)").unwrap());
    assert!(dom.css_supports("display: grid").unwrap(), "a bare declaration");
    assert!(dom.css_supports("(display: grid) and (not (display: nope))").unwrap());
    assert!(dom.css_supports("selector(:hover)").unwrap());
    assert!(!dom.css_supports("selector(:nope)").unwrap());
    assert!(!dom.css_supports("(no-such-property: zzz)").unwrap());
    assert!(!dom.css_supports("").unwrap());
    assert!(dom.css_supports_declaration("display", "grid").unwrap());
    assert!(dom.css_supports_declaration("--anything", "1 2 3").unwrap());
    assert!(!dom.css_supports_declaration("no-such-property", "zzz").unwrap());
    assert!(!dom.css_supports_declaration("display", "nope").unwrap());
    assert!(!dom.css_supports_declaration("color", "red !important").unwrap(), "no !important in the two-argument form");
    assert!(!dom.css_supports_declaration(" color", "red").unwrap());
    for property in ["", "123", "color:", "(color"] {
        assert!(!dom.css_supports_declaration(property, "red").unwrap(), "{property:?} is not a property name");
    }
}

#[test]
fn the_root_element_measures_the_viewport() {
    let root = |doc: &Doc| doc.dom.metrics(doc.dom.query_selector(doc.dom.document(), "html").unwrap()).unwrap().unwrap();
    let tall = page("<!doctype html><style>body { margin: 0; }</style><div style='height: 1000px'></div>");
    let m = root(&tall);
    assert_eq!((m.client_width, m.client_height), (480.0, 320.0), "clientHeight of the root is the viewport's, not the content's");
    assert_eq!((m.scroll_width, m.scroll_height), (480.0, 1000.0));
    let short = page("<!doctype html><style>body { margin: 0; }</style><div style='height: 100px'></div>");
    let m = root(&short);
    assert_eq!((m.client_width, m.client_height), (480.0, 320.0));
    assert_eq!((m.scroll_width, m.scroll_height), (480.0, 320.0), "scrollHeight of the root is at least the viewport's");
}

#[test]
fn element_from_point_follows_the_viewport_scroll() {
    let doc = page(
        "<!doctype html><style>body { margin: 0; } #sp { height: 1000px; } button { display: block; width: 100px; height: 40px; }</style>
        <div id=sp></div><button id=far>Far</button>",
    );
    let (sp, far) = (doc.dom.get_element_by_id("sp"), doc.dom.get_element_by_id("far"));
    assert_eq!(doc.dom.hit_test(10.0, 110.0), Some(sp));
    doc.dom.scroll_to(doc.dom.document(), 0.0, 700.0, false).unwrap();
    doc.commit();
    assert_eq!(doc.dom.hit_test(10.0, 310.0), Some(far), "page y 1010 is the button once the viewport is scrolled by 700");
    assert_eq!(doc.dom.hit_test(10.0, 110.0), Some(sp));
    assert_eq!(doc.dom.hit_test_all(10.0, 310.0).first(), Some(&far));
}

#[test]
fn an_inline_svg_without_a_view_box_draws_in_css_pixels() {
    let doc = page(
        "<!doctype html><style>body { margin: 0; background: #fff; } svg { display: block; width: 100px; height: 100px; }</style>
        <svg><rect id=r x=10 y=10 width=30 height=30 fill=red /><circle cx=150 cy=50 r=20 fill=red /></svg>",
    );
    let px = pixels(&doc);
    assert_eq!(rgb(&px, 25.0, 25.0), [255, 0, 0]);
    assert_eq!(rgb(&px, 45.0, 45.0), [255, 255, 255], "not stretched to the 100px box");
    assert_eq!(rgb(&px, 135.0, 50.0), [255, 255, 255], "clipped to the svg's box");
    let r = rect(&doc, "r");
    assert!([(r.x, 10.0), (r.y, 10.0), (r.width, 30.0), (r.height, 30.0)].iter().all(|&(a, b)| near(a, b)), "{r:?}");
}

#[test]
fn simple_svg_images_paint_as_backgrounds_and_masks() {
    let svg = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'%3E%3Crect width='10' height='10' fill='red'/%3E%3C/svg%3E";
    let doc = page(&format!(
        "<!doctype html><style>body {{ margin: 0; background: #fff; }} div {{ width: 40px; height: 40px; }}
        #bg {{ background: url(\"{svg}\") no-repeat; background-size: 20px 20px; }}
        #mask {{ background: blue; mask-image: url(\"{svg}\"); mask-size: 20px 20px; mask-repeat: no-repeat; }}</style>
        <div id=bg></div><div id=mask></div>"
    ));
    doc.sync();
    doc.commit();
    let px = pixels(&doc);
    assert_eq!(rgb(&px, 10.0, 10.0), [255, 0, 0], "background");
    assert_eq!(rgb(&px, 30.0, 30.0), [255, 255, 255], "background-size");
    assert_eq!(rgb(&px, 10.0, 50.0), [0, 0, 255], "mask lets the blue through");
    assert_eq!(rgb(&px, 30.0, 70.0), [255, 255, 255], "outside the mask");
}
