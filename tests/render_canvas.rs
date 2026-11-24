use acde::render::Options;

mod common;
use common::Doc;

const W: u32 = 200;
const H: u32 = 100;

#[derive(Default)]
struct Ops(Vec<u8>);

impl Ops {
    fn op(mut self, code: u8, args: &[f32]) -> Self {
        self.0.push(code);
        args.iter().for_each(|a| self.0.extend(a.to_le_bytes()));
        self
    }
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn str(mut self, s: &str) -> Self {
        self.0.extend((s.len() as u32).to_le_bytes());
        self.0.extend(s.as_bytes());
        self
    }
    fn fill_style(self, rgba: [f32; 4]) -> Self {
        self.op(4, &[]).u8(0).op_args(&rgba)
    }
    fn op_args(mut self, args: &[f32]) -> Self {
        args.iter().for_each(|a| self.0.extend(a.to_le_bytes()));
        self
    }
    fn fill_rect(self, x: f32, y: f32, w: f32, h: f32) -> Self {
        self.op(43, &[]).u8(0).op_args(&[x, y, w, h])
    }
    fn clone_ops(&self) -> Self {
        Self(self.0.clone())
    }
    fn apply(self, doc: &Doc, id: u32) {
        doc.canvas(id, &self.0).unwrap();
    }
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

struct Page {
    doc: Doc,
    canvas: u32,
}

impl Page {
    fn new() -> Self {
        let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
        doc.add_stylesheet("html, body { margin: 0; background: #fff } canvas { display: block }").unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        let canvas = doc.create_element("canvas").unwrap();
        doc.set_attribute(canvas, "width", "100").unwrap();
        doc.set_attribute(canvas, "height", "50").unwrap();
        doc.append_child(body, canvas).unwrap();
        Self { doc, canvas }
    }

    fn pixels(&self) -> Vec<u8> {
        common::rasterize(self.doc.scene(), (W, H))
    }

    fn at(px: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * W + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    fn red(px: &[u8], x: u32, y: u32) -> bool {
        let [r, g, b, _] = Self::at(px, x, y);
        r > 200 && g < 60 && b < 60
    }

    fn reds(px: &[u8]) -> usize {
        (0..H).flat_map(|y| (0..W).map(move |x| (x, y))).filter(|&(x, y)| Self::red(px, x, y)).count()
    }
}

#[test]
fn fill_rect_path_and_circle() {
    let page = Page::new();
    Ops::default().fill_style(RED).fill_rect(10.0, 10.0, 20.0, 20.0).apply(&page.doc, page.canvas);
    let px = page.pixels();
    assert!(Page::red(&px, 20, 20) && !Page::red(&px, 35, 20));
    assert_eq!(Page::reds(&px), 400);
    Ops::default().op(20, &[]).op(25, &[70.0, 25.0, 15.0, 0.0, std::f32::consts::TAU]).u8(0).op(40, &[]).u8(0).apply(&page.doc, page.canvas);
    let px = page.pixels();
    assert!(Page::red(&px, 70, 25) && !Page::red(&px, 57, 12) && Page::red(&px, 20, 20));
}

#[test]
fn transform_scales_shapes_and_stroke_width() {
    let page = Page::new();
    Ops::default().op(3, &[2.0, 0.0, 0.0, 2.0, 50.0, 0.0]).fill_style(RED).fill_rect(0.0, 0.0, 10.0, 10.0).apply(&page.doc, page.canvas);
    let px = page.pixels();
    assert!(Page::red(&px, 55, 15) && !Page::red(&px, 45, 5) && !Page::red(&px, 75, 5));
    assert_eq!(Page::reds(&px), 400);
    let page = Page::new();
    Ops::default()
        .op(3, &[2.0, 0.0, 0.0, 2.0, 0.0, 0.0])
        .op(5, &[])
        .u8(0)
        .op_args(&RED)
        .op(6, &[2.0])
        .op(20, &[])
        .op(21, &[5.0, 20.0])
        .op(22, &[45.0, 20.0])
        .op(41, &[])
        .apply(&page.doc, page.canvas);
    let px = page.pixels();
    let column: Vec<bool> = (35..46).map(|y| Page::red(&px, 50, y)).collect();
    assert_eq!(column.iter().filter(|r| **r).count(), 4, "{column:?}");
}

#[test]
fn clear_clip_and_alpha() {
    let page = Page::new();
    Ops::default().fill_style(RED).fill_rect(0.0, 0.0, 100.0, 50.0).apply(&page.doc, page.canvas);
    Ops::default().op(43, &[]).u8(2).op_args(&[10.0, 10.0, 10.0, 10.0]).apply(&page.doc, page.canvas);
    let px = page.pixels();
    assert!(!Page::red(&px, 15, 15) && Page::red(&px, 25, 15));
    assert_eq!(Page::at(&px, 15, 15), [255, 255, 255, 255]);
    Ops::default().op(43, &[]).u8(2).op_args(&[0.0, 0.0, 100.0, 50.0]).apply(&page.doc, page.canvas);
    assert_eq!(Page::reds(&page.pixels()), 0);
    Ops::default()
        .op(1, &[])
        .op(20, &[])
        .op(28, &[0.0, 0.0, 50.0, 50.0])
        .op(42, &[])
        .u8(0)
        .fill_style(RED)
        .fill_rect(0.0, 0.0, 100.0, 50.0)
        .op(2, &[])
        .fill_rect(90.0, 0.0, 10.0, 10.0)
        .apply(&page.doc, page.canvas);
    let px = page.pixels();
    assert!(Page::red(&px, 25, 25) && !Page::red(&px, 75, 25) && Page::red(&px, 95, 5));
    let page = Page::new();
    Ops::default().op(9, &[0.5]).fill_style(RED).fill_rect(0.0, 0.0, 10.0, 10.0).apply(&page.doc, page.canvas);
    let [r, g, b, _] = Page::at(&page.pixels(), 5, 5);
    assert!(r == 255 && (120..=136).contains(&g) && (120..=136).contains(&b), "{r} {g} {b}");
}

#[test]
fn draw_image_from_another_canvas_and_reset_on_resize() {
    let page = Page::new();
    let doc = &page.doc;
    let other = doc.create_element("canvas").unwrap();
    doc.set_attribute(other, "width", "10").unwrap();
    doc.set_attribute(other, "height", "10").unwrap();
    Ops::default().fill_style(RED).fill_rect(0.0, 0.0, 10.0, 10.0).apply(doc, other);
    Ops::default().op(45, &[]).u8(1).op_args(&[]).apply_draw(doc, page.canvas, other);
    let px = page.pixels();
    assert!(Page::red(&px, 75, 25) && !Page::red(&px, 55, 25));
    assert_eq!(Page::reds(&px), 900);
    Ops::default().fill_style([0.0, 0.0, 1.0, 1.0]).fill_rect(0.0, 0.0, 10.0, 10.0).apply(doc, other);
    Ops::default().op(43, &[]).u8(2).op_args(&[0.0, 0.0, 100.0, 50.0]).apply(doc, page.canvas);
    Ops::default().op(45, &[]).u8(1).op_args(&[]).apply_draw(doc, page.canvas, other);
    let px = page.pixels();
    let [r, g, b, _] = Page::at(&px, 75, 25);
    assert!(r < 60 && g < 60 && b > 200, "{r} {g} {b}");
    assert_eq!(Page::reds(&px), 0);
    doc.set_attribute(page.canvas, "width", "100").unwrap();
    assert_eq!(Page::reds(&page.pixels()), 0);
}

trait Draw {
    fn apply_draw(self, doc: &Doc, canvas: u32, source: u32);
}

impl Draw for Ops {
    fn apply_draw(mut self, doc: &Doc, canvas: u32, source: u32) {
        self.0.extend(source.to_le_bytes());
        self.op_args(&[0.0, 0.0, f32::NAN, f32::NAN, 60.0, 10.0, 30.0, 30.0]).apply(doc, canvas);
    }
}

#[test]
fn text_is_drawn_and_measured_like_dom_text() {
    let page = Page::new();
    let doc = &page.doc;
    Ops::default().op(11, &[]).str("20px monospace").fill_style(RED).op(44, &[]).u8(0).str("MMMM").op_args(&[5.0, 30.0, f32::NAN]).apply(doc, page.canvas);
    let reds = Page::reds(&page.pixels());
    assert!(reds > 40, "{reds} red text pixels");
    let metrics = doc.measure_text(page.canvas, "20px monospace", "MMMM").unwrap();
    let span = doc.create_element("span").unwrap();
    doc.set_attribute(span, "style", "font: 20px monospace; position: absolute").unwrap();
    let text = doc.create_text("MMMM").unwrap();
    doc.append_child(span, text).unwrap();
    doc.append_child(0, span).unwrap();
    let dom = doc.rect(span).unwrap().width;
    assert!((metrics.width - dom).abs() < 1.0, "canvas {} vs DOM {dom}", metrics.width);
    assert!(metrics.font_bounding_box_ascent > 10.0 && metrics.font_bounding_box_descent > 2.0, "{metrics:?}");
}

#[test]
fn measure_text_actual_bounding_box_is_the_ink() {
    let page = Page::new();
    let doc = &page.doc;
    let xo = doc.measure_text(page.canvas, "30px sans-serif", "xo").unwrap();
    let eg = doc.measure_text(page.canvas, "30px sans-serif", "\u{c9}g").unwrap();
    assert!(xo.actual_bounding_box_ascent < xo.font_bounding_box_ascent - 5.0, "{xo:?}");
    assert!(xo.actual_bounding_box_descent < 2.0, "{xo:?}");
    assert!(eg.actual_bounding_box_ascent > xo.actual_bounding_box_ascent + 6.0, "{eg:?}");
    assert!(eg.actual_bounding_box_descent > 4.0, "{eg:?}");
    assert!(xo.actual_bounding_box_right <= xo.width + 1.0 && xo.actual_bounding_box_right > xo.width - 6.0, "{xo:?}");
    assert!(xo.actual_bounding_box_left < 0.0 && xo.actual_bounding_box_left > -6.0, "{xo:?}");
    let blank = doc.measure_text(page.canvas, "30px sans-serif", " ").unwrap();
    assert_eq!((blank.actual_bounding_box_ascent, blank.actual_bounding_box_right), (0.0, 0.0));
    Ops::default().op(11, &[]).str("30px sans-serif").fill_style(RED).op(44, &[]).u8(0).str("xo").op_args(&[10.0, 35.0, f32::NAN]).apply(doc, page.canvas);
    let px = page.pixels();
    let (mut top, mut bottom) = (u32::MAX, 0);
    for y in 0..50 {
        for x in 0..100 {
            if Page::red(&px, x, y) {
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }
    let (ink_top, ink_bottom) = (35.0 - xo.actual_bounding_box_ascent, 35.0 + xo.actual_bounding_box_descent);
    assert!(
        (f64::from(top) - ink_top).abs() <= 1.5 && (f64::from(bottom) + 1.0 - ink_bottom).abs() <= 1.5,
        "pixels {top}..{bottom} ink {ink_top}..{ink_bottom}"
    );
}

#[test]
fn to_data_url_is_a_png() {
    let page = Page::new();
    Ops::default().fill_style(RED).fill_rect(0.0, 0.0, 10.0, 10.0).apply(&page.doc, page.canvas);
    let url = page.doc.to_data_url(page.canvas, "image/png").unwrap();
    assert!(url.starts_with("data:image/png;base64,iVBORw0KGgo"), "{}", &url[..40.min(url.len())]);
}

#[test]
fn canvas_damage_only_when_its_picture_changes() {
    let page = Page::new();
    let handle = page.doc.handle();
    let frame = |seq: u64| loop {
        if let Some(f) = handle.frame().filter(|f| f.commit >= seq) {
            return f;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    let draw = |x: f32| {
        Ops::default().op(43, &[]).u8(2).op_args(&[0.0, 0.0, 100.0, 50.0]).fill_style(RED).fill_rect(x, 10.0, 10.0, 10.0).apply(&page.doc, page.canvas)
    };
    draw(10.0);
    frame(handle.commit());
    draw(10.0);
    let same = frame(handle.commit());
    assert!(same.plane.damage_rects.is_empty());
    draw(60.0);
    let moved = frame(handle.commit());
    let d = moved.plane.damage;
    assert!(d.width > 0.0 && d.x + d.width <= 103.0 && d.y + d.height <= 53.0, "{d:?}");
}

impl Ops {
    fn shadow(self, rgba: [f32; 4], blur: f32, x: f32, y: f32) -> Self {
        self.op(13, &rgba).op_args(&[blur, x, y])
    }
    fn put(mut self, at: (f32, f32), (w, h): (u32, u32), data: &[u8]) -> Self {
        self = self.op(46, &[at.0, at.1, 0.0, 0.0, w as f32, h as f32]);
        self.0.extend(w.to_le_bytes());
        self.0.extend(h.to_le_bytes());
        self.0.extend(data);
        self
    }
    fn pattern(mut self, canvas: u32, repeat: u8) -> Self {
        self = self.op(4, &[]).u8(3).u8(1);
        self.0.extend(canvas.to_le_bytes());
        self.u8(repeat).op_args(&[1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
    }
}

const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

fn pixel(doc: &Doc, canvas: u32, x: f64, y: f64) -> [u8; 4] {
    doc.get_image_data(canvas, x, y, 1.0, 1.0).unwrap().try_into().unwrap()
}

#[test]
fn shadows_land_at_the_offset_and_blur_softens_them() {
    let page = Page::new();
    let (doc, c) = (&page.doc, page.canvas);
    Ops::default().shadow(BLUE, 0.0, 10.0, 10.0).fill_style(RED).fill_rect(10.0, 10.0, 20.0, 20.0).apply(doc, c);
    assert_eq!(pixel(doc, c, 15.0, 15.0), [255, 0, 0, 255]);
    assert_eq!(pixel(doc, c, 35.0, 35.0), [0, 0, 255, 255]);
    assert_eq!(pixel(doc, c, 45.0, 45.0)[3], 0);
    Ops::default().op(43, &[]).u8(2).op_args(&[0.0, 0.0, 100.0, 50.0]).apply(doc, c);
    Ops::default().shadow(BLUE, 0.0, 1010.0, 0.0).fill_rect(-1000.0, 10.0, 20.0, 20.0).apply(doc, c);
    assert_eq!(pixel(doc, c, 15.0, 15.0), [0, 0, 255, 255]);
    assert_eq!(pixel(doc, c, 35.0, 15.0)[3], 0);
    Ops::default().op(43, &[]).u8(2).op_args(&[0.0, 0.0, 100.0, 50.0]).apply(doc, c);
    Ops::default().shadow(BLUE, 8.0, 50.0, 0.0).fill_rect(5.0, 10.0, 30.0, 30.0).apply(doc, c);
    assert!(pixel(doc, c, 70.0, 25.0)[3] > 240);
    let edge = pixel(doc, c, 55.0, 25.0)[3];
    assert!((90..170).contains(&edge), "{edge}");
    assert_eq!(pixel(doc, c, 42.0, 25.0)[3], 0);
}

#[test]
fn put_and_get_image_data_round_trip() {
    let page = Page::new();
    let (doc, c) = (&page.doc, page.canvas);
    Ops::default().fill_style(RED).fill_rect(0.0, 0.0, 100.0, 50.0).apply(doc, c);
    let data: Vec<u8> = (0..4u8).flat_map(|i| [i * 60, 255 - i * 60, 7, 255]).collect();
    Ops::default().op(3, &[2.0, 0.0, 0.0, 2.0, 30.0, 30.0]).op(9, &[0.5]).put((5.0, 6.0), (2, 2), &data).apply(doc, c);
    assert_eq!(doc.get_image_data(c, 5.0, 6.0, 2.0, 2.0).unwrap(), data);
    assert_eq!(pixel(doc, c, 7.0, 6.0), [255, 0, 0, 255]);
    assert_eq!(doc.get_image_data(c, -1.0, 0.0, 1.0, 1.0).unwrap(), vec![0, 0, 0, 0]);
}

#[test]
fn patterns_repeat_and_no_repeat_stays_in_its_tile() {
    let page = Page::new();
    let (doc, c) = (&page.doc, page.canvas);
    let tile = doc.create_element("canvas").unwrap();
    doc.set_attribute(tile, "width", "4").unwrap();
    doc.set_attribute(tile, "height", "4").unwrap();
    Ops::default().fill_style(RED).fill_rect(0.0, 0.0, 2.0, 4.0).fill_style(BLUE).fill_rect(2.0, 0.0, 2.0, 4.0).apply(doc, tile);
    Ops::default().pattern(tile, 0).fill_rect(0.0, 0.0, 40.0, 20.0).apply(doc, c);
    assert_eq!(pixel(doc, c, 9.0, 10.0), [255, 0, 0, 255]);
    assert_eq!(pixel(doc, c, 15.0, 10.0), [0, 0, 255, 255]);
    Ops::default().op(43, &[]).u8(2).op_args(&[0.0, 0.0, 100.0, 50.0]).apply(doc, c);
    Ops::default().pattern(tile, 3).fill_rect(0.0, 0.0, 40.0, 20.0).apply(doc, c);
    assert_eq!(pixel(doc, c, 1.0, 1.0), [255, 0, 0, 255]);
    assert_eq!(pixel(doc, c, 9.0, 1.0)[3], 0);
}

#[test]
fn draw_image_sees_the_source_as_of_that_point_in_the_batch() {
    let page = Page::new();
    let (doc, b) = (&page.doc, page.canvas);
    let a = doc.create_element("canvas").unwrap();
    doc.set_attribute(a, "width", "10").unwrap();
    doc.set_attribute(a, "height", "10").unwrap();
    let mut draw = Ops::default().op(45, &[]).u8(1);
    draw.0.extend(a.to_le_bytes());
    let draw = draw.op_args(&[0.0, 0.0, f32::NAN, f32::NAN, 0.0, 0.0, 10.0, 10.0]);
    Ops::default().op(43, &[]).u8(2).op_args(&[0.0, 0.0, 100.0, 50.0]).apply(doc, b);
    Ops::default().fill_style(RED).fill_rect(0.0, 0.0, 10.0, 10.0).apply(doc, a);
    draw.apply(doc, b);
    Ops::default().fill_style(BLUE).fill_rect(0.0, 0.0, 10.0, 10.0).apply(doc, a);
    assert_eq!(pixel(doc, b, 5.0, 5.0), [255, 0, 0, 255]);
    assert_eq!(pixel(doc, a, 5.0, 5.0), [0, 0, 255, 255]);
}

#[test]
fn svg_path_strings_with_their_transform() {
    let page = Page::new();
    Ops::default()
        .fill_style(RED)
        .op(20, &[])
        .op(31, &[])
        .str("M10 10 h20 v20 h-20 z")
        .op_args(&[1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
        .op(31, &[])
        .str("M0 0 L10 0 L10 10 L0 10 Z")
        .op_args(&[2.0, 0.0, 0.0, 1.0, 60.0, 20.0])
        .op(40, &[])
        .u8(0)
        .apply(&page.doc, page.canvas);
    let px = page.pixels();
    assert!(Page::red(&px, 20, 20) && Page::red(&px, 78, 25) && !Page::red(&px, 35, 20) && !Page::red(&px, 82, 25));
    assert_eq!(Page::reds(&px), 600);
    let page = Page::new();
    Ops::default()
        .op(3, &[2.0, 0.0, 0.0, 2.0, 0.0, 0.0])
        .fill_style(RED)
        .op(20, &[])
        .op(31, &[])
        .str("M5 5 h10 v10 h-10 z M30 5 h10 L v10")
        .op_args(&[1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
        .op(40, &[])
        .u8(0)
        .apply(&page.doc, page.canvas);
    let px = page.pixels();
    assert!(Page::red(&px, 20, 20) && !Page::red(&px, 70, 20));
    assert_eq!(Page::reds(&px), 400);
}

fn filter(ops: Ops, effects: &[(u8, &[f32])]) -> Ops {
    let mut ops = ops.u8(14);
    ops.0.extend((effects.len() as u32).to_le_bytes());
    for (kind, values) in effects {
        ops = ops.u8(*kind).op_args(values);
    }
    ops
}

#[test]
fn filter_blurs_and_recolors_what_is_drawn() {
    let page = Page::new();
    let ops = filter(Ops::default(), &[(0, &[3.0])]).fill_style(RED).fill_rect(30.0, 10.0, 40.0, 30.0);
    ops.apply(&page.doc, page.canvas);
    let px = page.pixels();
    let [r, g, b, _] = Page::at(&px, 28, 25);
    assert!(r > 240 && g < 245 && g > 120 && b > 120, "blur spreads the red past its edge: {:?}", [r, g, b]);
    assert!(Page::red(&px, 50, 25), "and keeps its middle red");

    let page = Page::new();
    let ops = filter(Ops::default(), &[(3, &[1.0])]).fill_style(RED).fill_rect(30.0, 10.0, 40.0, 30.0);
    ops.apply(&page.doc, page.canvas);
    let [r, g, b, _] = Page::at(&page.pixels(), 50, 25);
    assert!(r == g && g == b && (40..70).contains(&r), "grayscale(1) of red is its luma: {:?}", [r, g, b]);

    let page = Page::new();
    let ops = filter(Ops::default(), &[]).fill_style(RED).fill_rect(30.0, 10.0, 40.0, 30.0);
    ops.apply(&page.doc, page.canvas);
    assert_eq!(Page::reds(&page.pixels()), 1200, "no effects: no filter");
}

#[test]
fn draw_image_of_an_svg_image() {
    let page = Page::new();
    let img = page.doc.create_element("img").unwrap();
    let svg = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10'%3E%3Crect width='10' height='10' fill='red'/%3E%3C/svg%3E";
    page.doc.set_attribute(img, "src", svg).unwrap();
    page.doc.commit();
    page.doc.sync();
    let mut ops = Ops::default().op(45, &[]).u8(0);
    ops.0.extend(img.to_le_bytes());
    ops.op_args(&[0.0, 0.0, f32::NAN, f32::NAN, 20.0, 10.0, 30.0, 30.0]).apply(&page.doc, page.canvas);
    let px = page.pixels();
    assert!(Page::red(&px, 35, 25), "the SVG drawn scaled into 20,10 30x30");
    assert!(!Page::red(&px, 15, 25) && !Page::red(&px, 55, 25));
}

#[test]
fn a_put_image_is_the_source_until_something_is_drawn_over_it() {
    let page = Page::new();
    let (doc, b) = (&page.doc, page.canvas);
    let a = doc.create_element("canvas").unwrap();
    doc.set_attribute(a, "width", "4").unwrap();
    doc.set_attribute(a, "height", "4").unwrap();
    let data: Vec<u8> = (0..16u8).flat_map(|i| [i * 16, 255 - i * 16, 40, 255]).collect();
    let mut draw = Ops::default().op(45, &[]).u8(1);
    draw.0.extend(a.to_le_bytes());
    let draw = draw.op_args(&[0.0, 0.0, f32::NAN, f32::NAN, 0.0, 0.0, 4.0, 4.0]);
    Ops::default().put((0.0, 0.0), (4, 4), &data).apply(doc, a);
    Ops::default().op(43, &[]).u8(2).op_args(&[0.0, 0.0, 100.0, 50.0]).apply(doc, b);
    draw.clone_ops().apply(doc, b);
    assert_eq!(doc.get_image_data(b, 0.0, 0.0, 4.0, 4.0).unwrap(), data);
    Ops::default().put((0.0, 0.0), (4, 4), &data).fill_style(BLUE).fill_rect(0.0, 0.0, 2.0, 4.0).apply(doc, a);
    draw.apply(doc, b);
    assert_eq!(pixel(doc, b, 1.0, 1.0), [0, 0, 255, 255]);
    assert_eq!(&doc.get_image_data(b, 3.0, 1.0, 1.0, 1.0).unwrap()[..], &data[(4 + 3) * 4..(4 + 3) * 4 + 4]);
}

#[test]
fn an_opaque_fill_over_the_whole_canvas_hides_what_was_under_it_and_a_translucent_one_does_not() {
    let page = Page::new();
    let (doc, c) = (&page.doc, page.canvas);
    Ops::default().fill_style(RED).fill_rect(10.0, 10.0, 20.0, 20.0).fill_style(BLUE).fill_rect(-5.0, -5.0, 200.0, 100.0).apply(doc, c);
    assert_eq!(pixel(doc, c, 15.0, 15.0), [0, 0, 255, 255]);
    Ops::default().fill_style(RED).fill_rect(10.0, 10.0, 20.0, 20.0).fill_style([0.0, 1.0, 0.0, 0.5]).fill_rect(0.0, 0.0, 100.0, 50.0).apply(doc, c);
    let p = pixel(doc, c, 15.0, 15.0);
    assert!(p[0] > 100 && p[1] > 100 && p[2] < 10, "{p:?}");
    let q = pixel(doc, c, 60.0, 40.0);
    assert!(q[2] > 100 && q[1] > 100, "{q:?}");
}

#[test]
fn letter_and_word_spacing_widen_measure_and_drawing() {
    let page = Page::new();
    let doc = &page.doc;
    let font = "20px monospace";
    let plain = doc.measure_text(page.canvas, font, "abcd").unwrap().width;
    let wide = doc.measure_text_spaced(page.canvas, font, "abcd", 10.0, 0.0, 0).unwrap().width;
    let tight = doc.measure_text_spaced(page.canvas, font, "abcd", -2.0, 0.0, 0).unwrap().width;
    let em = doc.measure_text_spaced(page.canvas, font, "abcd", 0.5, 0.0, 1).unwrap().width;
    assert!((wide - plain - 40.0).abs() < 0.01, "letter 10px: {plain} -> {wide}");
    assert!((tight - plain + 8.0).abs() < 0.01, "letter -2px: {plain} -> {tight}");
    assert!((em - wide).abs() < 0.01, "0.5em of 20px is 10px: {em} vs {wide}");
    let words = doc.measure_text(page.canvas, font, "a b c").unwrap().width;
    let spaced = doc.measure_text_spaced(page.canvas, font, "a b c", 0.0, 20.0, 0).unwrap().width;
    assert!((spaced - words - 40.0).abs() < 0.01, "word 20px over two spaces: {words} -> {spaced}");

    let measured = doc.measure_text_spaced(page.canvas, font, "MMMM", 6.0, 0.0, 0).unwrap();
    Ops::default()
        .op(11, &[])
        .str(font)
        .op(15, &[6.0, 0.0])
        .u8(0)
        .fill_style(RED)
        .op(44, &[])
        .u8(0)
        .str("MMMM")
        .op_args(&[5.0, 30.0, f32::NAN])
        .apply(doc, page.canvas);
    let px = page.pixels();
    let (mut left, mut right) = (u32::MAX, 0);
    for y in 0..50 {
        for x in 0..100 {
            if Page::red(&px, x, y) {
                left = left.min(x);
                right = right.max(x);
            }
        }
    }
    let (ink_left, ink_right) = (5.0 - measured.actual_bounding_box_left, 5.0 + measured.actual_bounding_box_right);
    assert!(
        (f64::from(left) - ink_left).abs() <= 1.5 && (f64::from(right) + 1.0 - ink_right).abs() <= 1.5,
        "pixels {left}..{right} ink {ink_left}..{ink_right} ({measured:?})"
    );
    let unspaced = doc.measure_text(page.canvas, font, "MMMM").unwrap();
    assert!(measured.actual_bounding_box_right - unspaced.actual_bounding_box_right > 17.0, "{measured:?} vs {unspaced:?}");
}

#[test]
fn a_new_font_keeps_the_spacing() {
    let page = Page::new();
    let doc = &page.doc;
    Ops::default()
        .op(15, &[8.0, 0.0])
        .u8(0)
        .op(11, &[])
        .str("20px monospace")
        .fill_style(RED)
        .op(44, &[])
        .u8(0)
        .str("MM")
        .op_args(&[5.0, 30.0, f32::NAN])
        .apply(doc, page.canvas);
    let px = page.pixels();
    let right = (0..100).filter(|&x| (0..50).any(|y| Page::red(&px, x, y))).max().unwrap_or(0);
    let spaced = doc.measure_text_spaced(page.canvas, "20px monospace", "MM", 8.0, 0.0, 0).unwrap();
    assert!((f64::from(right) + 1.0 - (5.0 + spaced.actual_bounding_box_right)).abs() <= 1.5, "right {right} vs {spaced:?}");
}

fn sized_canvas(doc: &Doc, (w, h): (u32, u32)) -> u32 {
    let canvas = doc.create_element("canvas").unwrap();
    doc.set_attribute(canvas, "width", &w.to_string()).unwrap();
    doc.set_attribute(canvas, "height", &h.to_string()).unwrap();
    canvas
}

fn draw_canvas(doc: &Doc, canvas: u32, source: u32, to: [f32; 4]) {
    let mut ops = Ops::default().op(45, &[]).u8(1);
    ops.0.extend(source.to_le_bytes());
    ops.op_args(&[0.0, 0.0, f32::NAN, f32::NAN]).op_args(&to).apply(doc, canvas);
}

#[test]
fn canvases_wider_or_taller_than_4096_draw_put_and_read_back_at_the_far_corner() {
    let page = Page::new();
    let doc = &page.doc;
    let yellow: Vec<u8> = (0..16 * 16).flat_map(|_| [255, 255, 0, 255]).collect();
    for (w, h) in [(4097, 100), (9000, 64), (64, 9000), (6000, 6000)] {
        let c = sized_canvas(doc, (w, h));
        let (fw, fh) = (f64::from(w), f64::from(h));
        Ops::default().fill_style(RED).fill_rect(0.0, 0.0, w as f32, h as f32).apply(doc, c);
        assert_eq!(pixel(doc, c, fw - 1.0, fh - 1.0), [255, 0, 0, 255], "{w}x{h} fill");
        Ops::default().put(((w - 16) as f32, (h - 16) as f32), (16, 16), &yellow).apply(doc, c);
        assert_eq!(doc.get_image_data(c, fw - 16.0, fh - 16.0, 16.0, 16.0).unwrap(), yellow, "{w}x{h} put");
        assert_eq!(pixel(doc, c, fw - 17.0, fh - 17.0), [255, 0, 0, 255], "{w}x{h} beside the put");
        Ops::default().fill_style(BLUE).fill_rect(0.0, 0.0, 8.0, 8.0).apply(doc, c);
        assert_eq!(pixel(doc, c, 0.0, 0.0), [0, 0, 255, 255], "{w}x{h} drawn after the put");
        assert_eq!(pixel(doc, c, fw - 1.0, fh - 1.0), [255, 255, 0, 255], "{w}x{h} the put after a draw");
        Ops::default().op(43, &[]).u8(2).op_args(&[0.0, 0.0, 100.0, 50.0]).apply(doc, page.canvas);
        draw_canvas(doc, page.canvas, c, [0.0, 0.0, 100.0, 50.0]);
        let [r, g, b, a] = pixel(doc, page.canvas, 50.0, 25.0);
        assert!(r > 240 && g < 16 && b < 16 && a > 240, "{w}x{h} as a source: {r} {g} {b} {a}");
        doc.set_attribute(c, "width", "0").unwrap();
    }
}

#[test]
fn a_large_canvas_drawn_one_to_one_keeps_every_pixel() {
    let page = Page::new();
    let doc = &page.doc;
    let (w, h) = (9000u32, 300u32);
    let a = sized_canvas(doc, (w, h));
    let b = sized_canvas(doc, (w, h));
    let ramp: Vec<u8> = (0..w).flat_map(|x| [(x % 251) as u8, (x / 251) as u8, 77, 255]).collect();
    Ops::default().put((0.0, 0.0), (w, 1), &ramp).apply(doc, a);
    draw_canvas(doc, b, a, [0.0, 0.0, w as f32, h as f32]);
    assert_eq!(doc.get_image_data(b, 0.0, 0.0, f64::from(w), 1.0).unwrap(), ramp);
}

#[test]
fn a_canvas_past_the_limits_has_no_bitmap() {
    let page = Page::new();
    let doc = &page.doc;
    let side = acde::gpu::Gpu::shared().map_or(32_767, |g| g.shared.device.limits().max_texture_dimension_2d.min(32_767));
    let c = sized_canvas(doc, (side + 1, 10));
    Ops::default().fill_style(RED).fill_rect(0.0, 0.0, 100.0, 10.0).apply(doc, c);
    assert_eq!(pixel(doc, c, 5.0, 5.0), [0, 0, 0, 0]);
    assert_eq!(doc.to_data_url(c, "image/png").unwrap(), "data:,");
    assert!(doc.get_image_data(c, 0.0, 0.0, 16385.0, 16385.0).is_err());
}
