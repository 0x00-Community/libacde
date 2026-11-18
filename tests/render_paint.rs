use acde::render::Options;

mod common;
use common::Doc;

const W: u32 = 120;
const H: u32 = 80;

fn red_pixels(doc: &Doc) -> usize {
    let rgba = common::rasterize(doc.scene(), (W, H));
    rgba.chunks(4).filter(|p| p[0] > 200 && p[1] < 80 && p[2] < 80).count()
}

fn page(css: &str, div: bool) -> Doc {
    let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
    doc.add_stylesheet(css).unwrap();
    let body = doc.create_element("body").unwrap();
    doc.append_child(0, body).unwrap();
    if div {
        let d = doc.create_element("div").unwrap();
        doc.set_attribute(d, "class", "bg").unwrap();
        doc.append_child(body, d).unwrap();
    }
    doc
}

const STRIPES: &str = "repeating-linear-gradient(90deg, #f00 0 10px, #00f 10px 20px)";

#[test]
fn body_background_image_is_painted() {
    let on_div = page(&format!("html, body {{ margin: 0; height: 100%; }} .bg {{ position: absolute; inset: 0; background: {STRIPES}; }}"), true);
    let reference = red_pixels(&on_div);
    assert!(reference > 1000, "control: {reference}");
    let on_body = page(&format!("html, body {{ margin: 0; height: 100%; }} body {{ background: {STRIPES}, #fff; }}"), false);
    let got = red_pixels(&on_body);
    assert!(got * 10 >= reference * 9, "body background: {got} red px, expected about {reference}");
}

#[test]
fn data_url_image_is_loaded() {
    let doc = page(
        ".bg { position: absolute; inset: 0; background: url(data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAYAAACp8Z5+AAAAEklEQVR4nGP4z8DwHxkzkC4AADxAH+HggXe0AAAAAElFTkSuQmCC); background-size: 100% 100%; }",
        true,
    );
    let start = std::time::Instant::now();
    let mut red = 0;
    while start.elapsed() < std::time::Duration::from_secs(3) {
        red = red_pixels(&doc);
        if red > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(red as u32 > W * H * 9 / 10, "{red} red px");
}

fn pixels(doc: &Doc) -> Vec<u8> {
    common::rasterize(doc.scene(), (W, H))
}

#[test]
fn body_background_image_covers_the_canvas() {
    let doc = page("html, body { margin: 0 } body { background: linear-gradient(#f00, #f00) } .bg { height: 10px }", true);
    assert!(red_pixels(&doc) as u32 > W * H * 9 / 10, "{} red px", red_pixels(&doc));
}

#[test]
fn propagated_background_is_not_painted_twice() {
    let doc = page("html, body { margin: 0 } body { background: rgba(0, 0, 255, .5) } .bg { height: 10px }", true);
    let rgba = pixels(&doc);
    let at = |x: u32, y: u32| &rgba[((y * W + x) * 4) as usize..((y * W + x) * 4 + 4) as usize];
    assert_eq!(at(60, 5), at(60, 60));
    assert!(at(60, 60)[3] > 100 && at(60, 60)[3] < 160, "{:?}", at(60, 60));
}

#[test]
fn empty_input_paints_its_placeholder() {
    let painted = |css: &str, value: Option<&str>| {
        let doc = page(&format!("body {{ margin: 0 }} input {{ font: 30px monospace; width: 110px; border: 0; color: #00f }} {css}"), false);
        let input = doc.create_element("input").unwrap();
        doc.set_attribute(input, "placeholder", "MMMM").unwrap();
        if let Some(v) = value {
            doc.set_attribute(input, "value", v).unwrap();
        }
        doc.append_child(0, input).unwrap();
        let px = pixels(&doc);
        let count = |f: &dyn Fn(&[u8]) -> bool| px.chunks(4).filter(|p| f(p)).count();
        (count(&|p| p[0] > 200 && p[1] < 80 && p[2] < 80), count(&|p| p[2] > 200 && p[0] < 80), count(&|p| p[0] > 200 && p[1] > 90 && p[1] < 200 && p[2] < 200))
    };
    let (red, ..) = painted("input::placeholder { color: #f00; opacity: 1 }", None);
    assert!(red > 50, "placeholder: {red} red pixels");
    let (red, blue, _) = painted("input::placeholder { color: #f00; opacity: 1 }", Some("MMMM"));
    assert!(red == 0 && blue > 50, "value: {red} red, {blue} blue pixels");
    let (red, ..) = painted("input::placeholder { color: #f00 }", None);
    assert!(red > 50, "page colour: {red} red pixels");
    let (red, _, pale) = painted("input { color: #f00 }", None);
    assert!(red < 10 && pale > 50, "default: {red} red, {pale} pale red pixels");
}

#[test]
fn gradient_stops_outside_the_line() {
    let at = |px: &[u8], x: u32, y: u32| -> [u8; 4] {
        let i = ((y * W + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    let masked = page(
        "html, body { margin: 0; background: #fff } .bg { position: absolute; inset: 0; background: #f00;
         mask-image: linear-gradient(to bottom, #000 -32px, transparent 0px) }",
        true,
    );
    assert_eq!(red_pixels(&masked), 0);
    let wide = page("html, body { margin: 0 } .bg { position: absolute; inset: 0; background: linear-gradient(to right, #f00 -100%, #00f 200%) }", true);
    let [r, g, b, _] = at(&pixels(&wide), W / 2, H / 2);
    assert!((118..=138).contains(&r) && g < 8 && (118..=138).contains(&b), "{r} {g} {b}");
}

#[test]
fn inline_padding_takes_space_and_rounds_the_background() {
    let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
    doc.add_stylesheet(
        "body { margin: 0; font: 16px sans-serif; background: #fff }
         .k { background: #f00; } .k.pad { padding: 0 6px; border-radius: 6px; }",
    )
    .unwrap();
    let body = doc.create_element("body").unwrap();
    doc.append_child(0, body).unwrap();
    let line = |class: &str| {
        let div = doc.create_element("div").unwrap();
        doc.append_child(body, div).unwrap();
        let k = doc.create_element("span").unwrap();
        doc.set_attribute(k, "class", class).unwrap();
        doc.append_child(div, k).unwrap();
        doc.append_child(k, doc.create_text("Esc").unwrap()).unwrap();
        let after = doc.create_element("span").unwrap();
        doc.append_child(div, after).unwrap();
        doc.append_child(after, doc.create_text("x").unwrap()).unwrap();
        (k, after)
    };
    let (plain, plain_after) = line("k");
    let (padded, padded_after) = line("k pad");
    let (a, b) = (doc.rect(plain).unwrap(), doc.rect(padded).unwrap());
    assert!((b.width - a.width - 12.0).abs() < 0.01, "{a:?} {b:?}");
    let shift = doc.rect(padded_after).unwrap().x - doc.rect(plain_after).unwrap().x;
    assert!((shift - 12.0).abs() < 0.01, "{shift}");
    let px = pixels(&doc);
    let red = |x: f64, y: f64| {
        let i = ((y as u32 * W + x as u32) * 4) as usize;
        px[i] > 200 && px[i + 1] < 80 && px[i + 2] < 80
    };
    let mid = b.y + b.height / 2.0;
    assert!(red(b.x + 2.0, mid));
    assert!(!red(b.x, b.y + 1.0));
}

#[test]
fn inline_percent_padding_and_border_sides() {
    let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
    doc.add_stylesheet(
        "body { margin: 0; font: 16px sans-serif; background: #fff } div { width: 100px }
         .k { padding: 0 10%; border: 3px solid #f00; border-left-color: #00f; }",
    )
    .unwrap();
    let body = doc.create_element("body").unwrap();
    doc.append_child(0, body).unwrap();
    let div = doc.create_element("div").unwrap();
    doc.append_child(body, div).unwrap();
    let k = doc.create_element("span").unwrap();
    doc.set_attribute(k, "class", "k").unwrap();
    doc.append_child(div, k).unwrap();
    doc.append_child(k, doc.create_text("ab").unwrap()).unwrap();
    let plain = doc.create_element("span").unwrap();
    doc.append_child(div, plain).unwrap();
    doc.append_child(plain, doc.create_text("ab").unwrap()).unwrap();
    let (a, b) = (doc.rect(k).unwrap(), doc.rect(plain).unwrap());
    assert!((a.width - b.width - 26.0).abs() < 0.01, "{a:?} {b:?}");
    let px = pixels(&doc);
    let at = |x: f64, y: f64| {
        let i = ((y as u32 * W + x as u32) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    let mid = a.y + a.height / 2.0;
    assert_eq!(at(a.x + 1.0, mid), [0, 0, 255]);
    assert_eq!(at(a.x + a.width - 2.0, mid), [255, 0, 0]);
}

fn control(css: &str, kind: &str, checked: bool) -> usize {
    let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
    doc.add_stylesheet(&format!("body {{ margin: 0; }} input {{ margin: 10px; }} {css}")).unwrap();
    let body = doc.create_element("body").unwrap();
    doc.append_child(0, body).unwrap();
    let input = doc.create_element("input").unwrap();
    doc.set_attribute(input, "type", kind).unwrap();
    if checked {
        doc.set_attribute(input, "checked", "").unwrap();
    }
    doc.append_child(body, input).unwrap();
    red_pixels(&doc)
}

#[test]
fn appearance_none_draws_the_element_s_own_boxes() {
    let own = "input { width: 30px; height: 30px; background: #f00; }";
    let native = control(own, "checkbox", false);
    assert!(native < 100, "the checkbox widget covers the background ({native} red px)");
    for kind in ["checkbox", "radio"] {
        let plain = control(&format!("{own} input {{ appearance: none; }}"), kind, false);
        assert!(plain > 850, "{kind}: {plain} red px");
    }
    let before = concat!(
        "input { appearance: none; width: 30px; height: 30px; } ",
        "input::before { content: ''; display: block; width: 20px; height: 20px; background: #f00; }",
    );
    let shown = control(before, "checkbox", true);
    assert!(shown > 350, "::before of an appearance: none checkbox: {shown} red px");
}
