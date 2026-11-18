use acde::render::Options;

mod common;
use common::Doc;

const W: u32 = 200;
const H: u32 = 100;

struct Page {
    doc: Doc,
}

impl Page {
    fn new(css: &str) -> Self {
        let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
        doc.add_stylesheet(css).unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        Self { doc }
    }

    fn svg(&self, parent: u32, tag: &str, attrs: &[(&str, &str)]) -> u32 {
        let id = self.doc.create_element_ns(1, tag).unwrap();
        for (k, v) in attrs {
            self.doc.set_attribute(id, k, v).unwrap();
        }
        self.doc.append_child(parent, id).unwrap();
        id
    }

    fn count(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> (usize, usize) {
        let rgba = common::rasterize(self.doc.scene(), (W, H));
        let (mut dark, mut red) = (0, 0);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * W + x) * 4) as usize;
                let (r, g, b) = (rgba[i], rgba[i + 1], rgba[i + 2]);
                dark += usize::from(r < 100 && g < 100 && b < 100);
                red += usize::from(r > 200 && g < 80 && b < 80);
            }
        }
        (dark, red)
    }
}

const CSS: &str = "html, body { margin: 0; background: #fff; } :root { --mono: monospace; }
    svg { position: absolute; top: 0; width: 100px; height: 100px; }
    .ic .mo { fill: none; stroke: #f00; stroke-width: 4; }
    .cc { color: #f00; } .cc .mo { fill: none; stroke: currentColor; stroke-width: 4; }";

#[test]
fn class_rules_reach_svg_children() {
    let page = Page::new(CSS);
    let root = 0;
    let body = page.doc.create_element("div").unwrap();
    page.doc.set_attribute(body, "class", "ic").unwrap();
    page.doc.append_child(root, body).unwrap();
    let svg = page.svg(body, "svg", &[("viewBox", "0 0 100 100")]);
    page.svg(svg, "rect", &[("class", "mo"), ("x", "10"), ("y", "10"), ("width", "80"), ("height", "80")]);
    let (dark, red) = page.count(0, 0, 100, 100);
    assert!(dark < 50, "rect filled black: {dark} dark px");
    assert!(red > 500, "no red stroke: {red}");
}

#[test]
fn text_with_var_font_is_drawn() {
    let page = Page::new(CSS);
    let root = 0;
    let svg = page.svg(root, "svg", &[("viewBox", "0 0 100 100")]);
    let text = page.svg(svg, "text", &[("x", "50"), ("y", "60"), ("text-anchor", "middle"), ("fill", "#000"), ("style", "font: 700 40px var(--mono)")]);
    let t = page.doc.create_text("{}").unwrap();
    page.doc.append_child(text, t).unwrap();
    let (dark, _) = page.count(0, 0, 100, 100);
    assert!(dark > 80, "text not drawn: {dark} dark px");
}

#[test]
fn current_color_stroke_from_css() {
    let page = Page::new(CSS);
    let root = 0;
    let div = page.doc.create_element("div").unwrap();
    page.doc.set_attribute(div, "class", "cc").unwrap();
    page.doc.append_child(root, div).unwrap();
    let svg = page.svg(div, "svg", &[("viewBox", "0 0 100 100")]);
    page.svg(svg, "rect", &[("class", "mo"), ("x", "10"), ("y", "10"), ("width", "80"), ("height", "80")]);
    let (dark, red) = page.count(0, 0, 100, 100);
    assert!(dark < 50 && red > 500, "dark {dark} red {red}");
}

#[test]
fn spread_box_shadow_follows_border_radius() {
    let page = Page::new(
        "html, body { margin: 0; background: #fff; }
        #d { position: absolute; left: 40px; top: 40px; width: 20px; height: 20px; border-radius: 50%; background: #fff; box-shadow: 0 0 0 10px #f00; }",
    );
    let root = 0;
    let d = page.doc.create_element("div").unwrap();
    page.doc.set_attribute(d, "id", "d").unwrap();
    page.doc.append_child(root, d).unwrap();
    let (_, corner) = page.count(31, 31, 35, 35);
    let (_, ring) = page.count(48, 31, 52, 35);
    assert!(ring > 0, "no ring at top");
    assert_eq!(corner, 0, "square shadow corner");
}

#[test]
fn animation_none_clears_animated_value() {
    let page = Page::new(
        "html, body { margin: 0; background: #fff; }
        @keyframes p { from { background: #f00; } to { background: #f00; } }
        #d { position: absolute; left: 0; top: 0; width: 50px; height: 50px; background: #fff; animation: p 2s infinite; }
        #d.off { animation: none; }",
    );
    let root = 0;
    let d = page.doc.create_element("div").unwrap();
    page.doc.set_attribute(d, "id", "d").unwrap();
    page.doc.append_child(root, d).unwrap();
    assert!(page.count(0, 0, 50, 50).1 > 2000, "animation not running");
    page.doc.set_attribute(d, "class", "off").unwrap();
    assert_eq!(page.count(0, 0, 50, 50).1, 0, "animated value stayed after animation: none");
}

#[test]
fn css_geometry_on_svg_rect() {
    let page = Page::new(
        "html, body { margin: 0; background: #fff; } svg { position: absolute; top: 0; width: 100px; height: 100px; }
         .g { x: 10px; y: 20px; width: calc(100% - 20px); height: 30%; fill: #f00; }",
    );
    let root = 0;
    let svg = page.svg(root, "svg", &[("viewBox", "0 0 100 100")]);
    page.svg(svg, "rect", &[("class", "g")]);
    let (_, red) = page.count(0, 0, 100, 100);
    assert!((2300..=2500).contains(&red), "{red} red px");
    assert_eq!(page.count(0, 0, 9, 100).1, 0, "x not applied");
}

#[test]
fn zero_sized_shapes_are_skipped() {
    let page = Page::new(CSS);
    let root = 0;
    let svg = page.svg(root, "svg", &[("viewBox", "0 0 100 100")]);
    page.svg(svg, "circle", &[("cx", "50"), ("cy", "50"), ("fill", "#00f")]);
    page.svg(svg, "rect", &[("x", "10"), ("y", "10"), ("width", "0"), ("height", "50"), ("fill", "#00f")]);
    page.svg(svg, "rect", &[("x", "10"), ("y", "10"), ("width", "20"), ("height", "20"), ("fill", "#f00")]);
    let (_, red) = page.count(0, 0, 100, 100);
    assert!((350..=450).contains(&red), "{red} red px");
}

#[test]
fn changing_a_path_inside_an_svg_redraws_it() {
    let page = Page::new(CSS);
    let root = 0;
    let svg = page.svg(root, "svg", &[("viewBox", "0 0 100 100"), ("class", "ic")]);
    let path = page.svg(svg, "path", &[("d", "M0 0H50V50H0Z"), ("fill", "#f00")]);
    let left = |page: &Page| page.count(0, 0, 50, 50).1;
    assert!(left(&page) > 2000);
    page.doc.set_attribute(path, "d", "M50 50H100V100H50Z").unwrap();
    assert_eq!(left(&page), 0);
    assert!(page.count(50, 50, 100, 100).1 > 2000);
}

#[test]
fn css_transforms_move_svg_children_about_their_user_space_origin() {
    let page = Page::new(
        "html, body { margin: 0; background: #fff; } svg { position: absolute; top: 0; width: 100px; height: 100px; }
         .big { transform: scale(2); } .moved { transform: translate(50px, 50px); }",
    );
    let svg = page.svg(0, "svg", &[("viewBox", "0 0 100 100")]);
    let rect = page.svg(svg, "rect", &[("width", "25"), ("height", "25"), ("fill", "#f00"), ("transform", "translate(75 75)"), ("class", "big")]);
    assert!(page.count(0, 0, 50, 50).1 > 2000, "scale(2) about 0 0 covers the top left quarter");
    assert_eq!(page.count(50, 50, 100, 100).1, 0, "the transform attribute is overridden by CSS");
    page.doc.set_attribute(rect, "class", "moved").unwrap();
    assert_eq!(page.count(0, 0, 50, 50).1, 0);
    assert!((500..=700).contains(&page.count(50, 50, 100, 100).1), "translated by CSS after the class change");
}

#[test]
fn oklch_and_color_mix_colors_paint() {
    for (style, attrs) in [
        ("color: oklch(0.63 0.26 29)", vec![("fill", "none"), ("stroke", "currentColor")]),
        ("color: color-mix(in oklab, red 90%, black)", vec![("fill", "none"), ("stroke", "currentColor")]),
        ("", vec![("fill", "none"), ("stroke", "oklch(0.63 0.26 29)")]),
        ("color: oklch(0.63 0.26 29)", vec![("fill", "currentColor")]),
    ] {
        let page = Page::new(CSS);
        let div = page.doc.create_element("div").unwrap();
        page.doc.set_attribute(div, "style", style).unwrap();
        page.doc.append_child(0, div).unwrap();
        let svg = page.svg(div, "svg", &[("viewBox", "0 0 100 100")]);
        let mut rect = vec![("x", "10"), ("y", "10"), ("width", "80"), ("height", "80"), ("stroke-width", "6")];
        rect.extend(attrs.iter().copied());
        page.svg(svg, "rect", &rect);
        let (_, red) = page.count(0, 0, 100, 100);
        assert!(red > 300, "{style} {attrs:?}: {red} red pixels");
    }
}

#[test]
fn use_and_paint_refer_to_another_svg_in_the_page() {
    let page = Page::new("html, body { margin: 0; background: #fff; } svg.show { position: absolute; top: 0; width: 100px; height: 100px; }");
    let sheet = page.svg(0, "svg", &[("style", "display: none")]);
    let symbol = page.svg(sheet, "symbol", &[("id", "box"), ("viewBox", "0 0 10 10")]);
    page.svg(symbol, "rect", &[("width", "10"), ("height", "10"), ("fill", "#f00")]);
    let gradient = page.svg(sheet, "linearGradient", &[("id", "red")]);
    page.svg(gradient, "stop", &[("offset", "0"), ("stop-color", "#f00")]);
    page.svg(gradient, "stop", &[("offset", "1"), ("stop-color", "#f00")]);

    let with_use = page.svg(0, "svg", &[("class", "show"), ("viewBox", "0 0 100 100")]);
    page.svg(with_use, "use", &[("href", "#box"), ("width", "100"), ("height", "100")]);
    let (_, red) = page.count(0, 0, 100, 100);
    assert!(red > 9000, "<use href=#box> into a hidden sheet: {red} red pixels");

    page.doc.set_attribute(with_use, "style", "display: none").unwrap();
    let painted = page.svg(0, "svg", &[("class", "show"), ("viewBox", "0 0 100 100")]);
    page.svg(painted, "rect", &[("width", "100"), ("height", "100"), ("fill", "url(#red)")]);
    let (_, red) = page.count(0, 0, 100, 100);
    assert!(red > 9000, "fill=url(#red) from the sheet: {red} red pixels");
}

#[test]
fn a_mask_of_one_opaque_path_clips_to_it() {
    let page = Page::new(CSS);
    let svg = page.svg(0, "svg", &[("viewBox", "0 0 100 100")]);
    let mask = page.svg(
        svg,
        "mask",
        &[("id", "half"), ("maskUnits", "userSpaceOnUse"), ("x", "0"), ("y", "0"), ("width", "100"), ("height", "100"), ("style", "mask-type: alpha")],
    );
    page.svg(mask, "path", &[("d", "M0 0H50V100H0Z"), ("fill", "#000")]);
    let group = page.svg(svg, "g", &[("mask", "url(#half)")]);
    page.svg(group, "rect", &[("width", "100"), ("height", "100"), ("fill", "#f00")]);
    let (_, left) = page.count(0, 0, 50, 100);
    let (_, right) = page.count(50, 0, 100, 100);
    assert!(left > 4500 && right < 100, "left {left} right {right}");
}

#[test]
fn a_luminance_mask_of_a_black_path_hides_everything() {
    let page = Page::new(CSS);
    let svg = page.svg(0, "svg", &[("viewBox", "0 0 100 100")]);
    let mask = page.svg(svg, "mask", &[("id", "dark"), ("maskUnits", "userSpaceOnUse"), ("x", "0"), ("y", "0"), ("width", "100"), ("height", "100")]);
    page.svg(mask, "path", &[("d", "M0 0H50V100H0Z"), ("fill", "#000")]);
    let group = page.svg(svg, "g", &[("mask", "url(#dark)")]);
    page.svg(group, "rect", &[("width", "100"), ("height", "100"), ("fill", "#f00")]);
    let (_, red) = page.count(0, 0, 100, 100);
    assert_eq!(red, 0);
}
