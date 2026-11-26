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
  .merge { --acde-glass-merge: 10px; }
  .masked { position: absolute; left: 100px; top: 40px; width: 80px; height: 80px; background: #fff;
            mask-image: linear-gradient(to right, #000 50%, transparent 50%); }
  .masked.shadowed { filter: drop-shadow(0 0 4px black); }
  .masked.blurred { filter: blur(2px) drop-shadow(0 0 4px black); }
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
        assert_eq!(group.spacing, if container.is_empty() { 0.0 } else { 10.0 });
    }
}

#[test]
fn a_named_glass_leaving_its_group_shrinks_away_over_frames() {
    let page = Page::new(1.0);
    let row = page.div("merge");
    let pill = |left: &str, name: &str| {
        let id = page.doc.create_element("div").unwrap();
        page.doc.set_attribute(id, "class", "pill").unwrap();
        page.doc.set_attribute(id, "style", &format!("left: {left}; --acde-glass-id: {name}")).unwrap();
        page.doc.append_child(row, id).unwrap();
        id
    };
    pill("20px", "a");
    let leaving = pill("86px", "b");
    let pieces = |frame: &Frame| frame.plane.layers.iter().filter_map(|l| l.backdrop.as_ref()?.group()).map(|g| g.len()).sum::<usize>();
    assert_eq!(pieces(&page.frame()), 2);
    page.doc.remove(leaving).unwrap();
    let frame = page.frame();
    assert!(frame.animating, "the ghost of b is still shrinking");
    assert_eq!(pieces(&frame), 2, "a and the ghost of b");
    std::thread::sleep(Duration::from_millis(700));
    page.handle.tick(0.0);
    let settled = page.frame();
    assert_eq!(pieces(&settled), 1);
    assert!(!settled.animating);
    let d = settled.plane.damage;
    assert!(d.x <= 86.0 && d.x + d.width >= 146.0, "the frame that drops the ghost repaints where it was: {d:?}");
}

#[test]
fn a_union_groups_a_circle_and_different_sized_pills_across_dom_branches() {
    let page = Page::new(1.0);
    let mut nodes = Vec::new();
    for (wrapper_left, glass) in [
        ("0px", "left: 20px; top: 20px; width: 120px; height: 60px; border-radius: 30px"),
        ("10px", "left: 146px; top: 34px; width: 50px; height: 32px; border-radius: 16px"),
        ("20px", "left: 202px; top: 35px; width: 30px; height: 30px; border-radius: 50%"),
    ] {
        let wrapper = page.doc.create_element("section").unwrap();
        page.doc.set_attribute(wrapper, "style", &format!("position: absolute; left: {wrapper_left}")).unwrap();
        page.doc.append_child(page.body, wrapper).unwrap();
        let id = page.doc.create_element("div").unwrap();
        page.doc.set_attribute(id, "class", "pill").unwrap();
        page.doc.set_attribute(id, "style", &format!("{glass}; --acde-glass-union: tuner")).unwrap();
        page.doc.append_child(wrapper, id).unwrap();
        nodes.push(id);
    }
    let frame = page.frame();
    let groups: Vec<_> = frame.plane.layers.iter().filter_map(|l| l.backdrop.as_ref()?.group()).collect();
    assert_eq!(groups.len(), 1);
    let pieces = groups[0].pieces();
    assert_eq!(pieces.len(), 3);
    assert_eq!(pieces.iter().map(|p| p.half).collect::<Vec<_>>(), [[60.0, 30.0], [25.0, 16.0], [15.0, 15.0]]);
    assert_eq!(pieces.iter().map(|p| p.radius).collect::<Vec<_>>(), [30.0, 16.0, 15.0]);
    assert_eq!(nodes.len(), 3);
}

fn glass_groups(page: &Page) -> Vec<usize> {
    group_sizes(&page.frame())
}

fn group_sizes(frame: &Frame) -> Vec<usize> {
    let mut sizes: Vec<usize> = frame.plane.layers.iter().filter_map(|l| l.backdrop.as_ref()?.group()).map(|g| g.len()).collect();
    sizes.sort_unstable();
    sizes
}

fn glass_in(page: &Page, parent: u32, style: &str) -> u32 {
    let id = page.doc.create_element("div").unwrap();
    page.doc.set_attribute(id, "class", "pill").unwrap();
    page.doc.set_attribute(id, "style", style).unwrap();
    page.doc.append_child(parent, id).unwrap();
    id
}

#[test]
fn glass_inside_glass_is_its_own_layer_and_wrappers_make_none() {
    let page = Page::new(1.0);
    let row = page.div("merge");
    let bar = glass_in(&page, row, "left: 10px; top: 10px; width: 300px; height: 120px; border-radius: 30px");
    glass_in(&page, row, "left: 316px; top: 10px; width: 60px; height: 40px");
    for left in ["20px", "86px"] {
        let wrapper = page.doc.create_element("div").unwrap();
        page.doc.append_child(bar, wrapper).unwrap();
        glass_in(&page, wrapper, &format!("left: {left}; top: 20px"));
    }
    let frame = page.frame();
    assert_eq!(group_sizes(&frame), [2, 2], "the bar merges with its neighbour, its two buttons with each other, never across");
    let mut names: Vec<&str> = frame.plane.layers.iter().filter_map(|l| l.backdrop.as_ref()?.group_name.as_deref()).collect();
    names.sort_unstable();
    assert_eq!(names.len(), 2, "each connected group gets its own name");
    assert_ne!(names[0], names[1]);
    assert!(names.iter().all(|n| n.starts_with('c')), "groups made by a container are named after it: {names:?}");
}

#[test]
fn a_union_name_does_not_reach_across_layers() {
    let page = Page::new(1.0);
    let bar = glass_in(&page, page.body, "left: 10px; top: 10px; width: 300px; height: 120px; border-radius: 30px; --acde-glass-union: u");
    glass_in(&page, page.body, "left: 316px; top: 10px; --acde-glass-union: u");
    glass_in(&page, bar, "left: 20px; top: 20px; --acde-glass-union: u");
    glass_in(&page, bar, "left: 86px; top: 20px; --acde-glass-union: u");
    assert_eq!(glass_groups(&page), [2, 2]);
}

fn pixel(pixels: &[u8], width: u32, (x, y): (u32, u32)) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

#[test]
fn a_masked_drop_shadow_leaves_nothing_outside_the_mask() {
    let composed = |class: Option<&str>| {
        let page = Page::new(1.0);
        if let Some(class) = class {
            page.div(class);
        }
        let frame = page.frame();
        let scenes =
            if frame.plane.layers.is_empty() { vec![frame.plane.drawing.scene()] } else { frame.plane.layers.iter().map(|l| l.drawing.scene()).collect() };
        (frame.size, raster(frame.size, scenes))
    };
    let ((width, _), masked) = composed(Some("masked shadowed"));
    let (_, empty) = composed(None);
    for at in [(160, 80), (178, 60), (184, 80), (120, 124), (96, 80)] {
        assert_eq!(pixel(&masked, width, at), pixel(&empty, width, at), "outside the mask at {at:?} the page shows through");
    }
    assert_eq!(pixel(&masked, width, (120, 80)), [255, 255, 255, 255], "inside the mask the element is drawn");
}

#[test]
fn a_masked_filter_keeps_its_other_effects_and_masks_inside_its_segment() {
    let page = Page::new(1.0);
    page.div("masked blurred");
    let frame = page.frame();
    let layer = frame.plane.layers.iter().find(|l| l.filter.is_some()).expect("the blurred element is its own segment");
    let effects = layer.filter.as_ref().unwrap().filter.effects();
    assert!(effects.iter().all(|e| !matches!(e, FilterEffect::DropShadow(_))), "{effects:?}");
    assert!(matches!(effects, [FilterEffect::GaussianBlur(_)]), "{effects:?}");
    let alone = raster(frame.size, vec![layer.drawing.scene()]);
    assert_eq!(pixel(&alone, frame.size.0, (160, 80))[3], 0, "the mask is applied before the blur, inside the segment");
    assert_eq!(pixel(&alone, frame.size.0, (120, 80)), [255, 255, 255, 255]);
}

#[test]
fn the_ime_float_is_drawn_as_its_own_panel_until_it_is_dropped() {
    let page = Page::new(2.0);
    let float = page.div("ds");
    page.doc.dom.set_ime_float(float, Some([20.0, 20.0, 40.0, 40.0])).unwrap();
    let frame = page.frame();
    let panel = frame.panels.iter().find(|p| p.id == acde::render::IME_FLOAT_PANEL).expect("the float panel");
    assert_eq!(panel.size, (80, 80), "the float rect in device pixels");
    page.doc.dom.set_ime_float(float, None).unwrap();
    assert!(page.frame().panels.iter().all(|p| p.id != acde::render::IME_FLOAT_PANEL), "a dropped float leaves no panel");
}

#[test]
fn a_shorter_preedit_leaves_the_vacated_part_of_the_ime_float_clear() {
    let page = Page::new(1.0);
    let root = page.div("");
    page.doc.set_attribute(root, "style", "position:fixed;inset:0;z-index:2147483647;pointer-events:none;overflow:visible").unwrap();
    let preedit = page.doc.create_element("span").unwrap();
    let look = "z-index:2147483647;text-decoration:underline;background:Canvas;color:CanvasText;pointer-events:none;white-space:pre";
    page.doc.set_attribute(preedit, "style", &format!("position:absolute;left:20px;top:20px;height:18px;line-height:18px;{look}")).unwrap();
    page.doc.append_child(root, preedit).unwrap();
    page.doc.set_text_content(preedit, "nihaonihaonihao").unwrap();
    let candidates = page.doc.create_element("div").unwrap();
    page.doc.set_attribute(candidates, "style", "position:absolute;left:10px;top:44px;width:150px;height:30px;background:#eee").unwrap();
    page.doc.append_child(root, candidates).unwrap();
    page.doc.dom.set_ime_float(root, Some([10.0, 20.0, 150.0, 54.0])).unwrap();
    page.frame();
    let wide = page.doc.rect(preedit).unwrap();
    page.doc.set_text_content(preedit, "你hao").unwrap();
    let frame = page.frame();
    let narrow = page.doc.rect(preedit).unwrap();
    assert!(narrow.width < wide.width / 2.0, "the preedit box follows its text: {wide:?} then {narrow:?}");
    let float = frame.panels.iter().find(|p| p.id == acde::render::IME_FLOAT_PANEL).expect("the float panel");
    let vacated = (narrow.x + narrow.width + 4.0 - 10.0, 8.0);
    let covered = float.plane.damage_rects.iter().any(|r| r.x <= vacated.0 && vacated.0 < r.x + r.width && r.y <= vacated.1 && vacated.1 < r.y + r.height);
    assert!(covered, "the vacated part is repainted: {:?}", float.plane.damage_rects);
    let pixels = raster(float.size, vec![float.plane.drawing.scene()]);
    let at = |(x, y): (f64, f64)| {
        let i = ((y as u32 * float.size.0 + x as u32) * 4) as usize;
        pixels[i + 3]
    };
    assert_eq!(at(vacated), 0, "nothing is drawn where the longer preedit was");
    assert_eq!(at((3.0, 8.0)), 0, "nor in the float rect around the preedit");
    assert_eq!(at((100.0, 40.0)), 255, "the candidates are still drawn");
}

fn styled(page: &Page, parent: u32, style: &str) -> u32 {
    let id = page.doc.create_element("div").unwrap();
    page.doc.set_attribute(id, "style", style).unwrap();
    page.doc.append_child(parent, id).unwrap();
    id
}

fn segments(frame: &Frame) -> Vec<u8> {
    let scenes = if frame.plane.layers.is_empty() { vec![frame.plane.drawing.scene()] } else { frame.plane.layers.iter().map(|l| l.drawing.scene()).collect() };
    raster(frame.size, scenes)
}

#[test]
fn a_filter_inside_a_filtered_element_is_drawn_with_its_own_effects() {
    let page = Page::new(1.0);
    let outer = styled(&page, page.body, "position: absolute; left: 0; top: 0; width: 400px; height: 200px; filter: grayscale(1)");
    styled(&page, outer, "position: absolute; left: 100px; top: 60px; width: 60px; height: 60px; background: #f00; filter: blur(6px)");
    let frame = page.frame();
    let layer = frame.plane.layers.iter().find(|l| l.filter.is_some()).expect("the grayscale element is its own segment");
    let alone = raster(frame.size, vec![layer.drawing.scene()]);
    let width = frame.size.0;
    assert!(pixel(&alone, width, (96, 90))[3] > 20, "the inner blur spills past the square: {:?}", pixel(&alone, width, (96, 90)));
    assert!(pixel(&alone, width, (101, 90))[3] < 250, "the square's edge is soft: {:?}", pixel(&alone, width, (101, 90)));
    assert_eq!(pixel(&alone, width, (130, 90))[0], 255, "the middle stays red before the outer grayscale");
}

#[test]
fn a_drop_shadow_inside_a_filtered_element_is_drawn() {
    let page = Page::new(2.0);
    let outer = styled(&page, page.body, "position: absolute; left: 0; top: 0; width: 400px; height: 200px; filter: blur(0.5px)");
    styled(&page, outer, "position: absolute; left: 100px; top: 60px; width: 40px; height: 40px; background: #fff; filter: drop-shadow(0 10px 0 #000)");
    let frame = page.frame();
    let layer = frame.plane.layers.iter().find(|l| l.filter.is_some()).expect("the blurred element is its own segment");
    let alone = raster(frame.size, vec![layer.drawing.scene()]);
    let under = pixel(&alone, frame.size.0, (240, 214));
    assert!(under[3] > 200 && under[0] < 40, "the shadow 10 css px below the box, at device scale 2: {under:?}");
}

#[test]
fn a_backdrop_filter_inside_a_filtered_element_blurs_what_the_element_drew_below_it() {
    let page = Page::new(1.0);
    let outer = styled(&page, page.body, "position: absolute; left: 0; top: 0; width: 400px; height: 200px; filter: grayscale(1)");
    styled(&page, outer, "position: absolute; left: 0; top: 0; width: 200px; height: 200px; background: #000");
    styled(&page, outer, "position: absolute; left: 150px; top: 50px; width: 100px; height: 100px; backdrop-filter: blur(10px)");
    let frame = page.frame();
    let layer = frame.plane.layers.iter().find(|l| l.filter.is_some()).expect("the grayscale element is its own segment");
    let alone = raster(frame.size, vec![layer.drawing.scene()]);
    let width = frame.size.0;
    let edge = pixel(&alone, width, (205, 100));
    assert!(edge[3] > 20 && edge[3] < 250, "the black edge is blurred into the glass, and only inside it: {edge:?}");
    assert_eq!(pixel(&alone, width, (260, 100))[3], 0, "outside the glass nothing is drawn");
    assert_eq!(pixel(&alone, width, (160, 40)), [0, 0, 0, 255], "above the glass the black is sharp");
}

fn list_under_a_mask(scale: f32) -> (Vec<u8>, u32) {
    let page = Page::new(scale);
    page.doc.set_attribute(page.body, "style", "background: #fff").unwrap();
    let list = styled(
        &page,
        page.body,
        "position: absolute; left: 10px; top: 10px; width: 100px; height: 120px; overflow: auto; \
         -webkit-mask-image: linear-gradient(#000 calc(100% - 28px), transparent); mask-image: linear-gradient(#000 calc(100% - 28px), transparent)",
    );
    styled(&page, list, "height: 400px; background: #000");
    let frame = page.frame();
    (segments(&frame), frame.size.0)
}

#[test]
fn a_scrolling_list_fades_out_under_its_mask() {
    for scale in [1u32, 2] {
        let (pixels, width) = list_under_a_mask(scale as f32);
        let at = |x: u32, y: u32| pixel(&pixels, width, (x * scale, y * scale));
        assert_eq!(at(50, 60), [0, 0, 0, 255], "above the fade the list is drawn (scale {scale})");
        assert!(at(50, 104)[0] < 120, "the fade starts 28 css px above the bottom (scale {scale}): {:?}", at(50, 104));
        assert!(at(50, 126)[0] > 150, "near the bottom it has faded out (scale {scale}): {:?}", at(50, 126));
        assert_eq!(at(50, 140), [255, 255, 255, 255], "below the box nothing of the list shows (scale {scale})");
    }
}

fn panel_pixels(glass: bool) -> (Vec<u8>, u32) {
    let page = Page::new(2.0);
    page.doc.set_attribute(page.body, "style", "background: #fff").unwrap();
    let pad =
        styled(&page, page.body, "position: absolute; left: 0; top: 0; width: 200px; height: 180px; display: flex; flex-direction: column; isolation: isolate");
    if glass {
        styled(&page, pad, "flex: none; height: 20px; background: rgba(255, 255, 255, .5); backdrop-filter: blur(8px)");
    }
    let list = styled(
        &page,
        pad,
        "flex: 1; min-height: 0; margin: 10px; overflow: auto; \
         -webkit-mask-image: linear-gradient(#000 calc(100% - 28px), transparent); mask-image: linear-gradient(#000 calc(100% - 28px), transparent)",
    );
    styled(&page, list, "height: 400px; background: #000");
    page.doc.dom.set_panel(pad, Some([0.0, 0.0, 200.0, 180.0])).unwrap();
    let frame = page.frame();
    let panel = frame.panels.iter().find(|p| p.id == pad).expect("the pinned panel");
    let scenes = if panel.plane.layers.is_empty() { vec![panel.plane.drawing.scene()] } else { panel.plane.layers.iter().map(|l| l.drawing.scene()).collect() };
    (raster(panel.size, scenes), panel.size.0)
}

#[test]
fn a_masked_list_in_a_pinned_panel_fades_out() {
    for glass in [false, true] {
        let (pixels, width) = panel_pixels(glass);
        let bottom = 170;
        let at = |y: u32| pixel(&pixels, width, (100, y * 2));
        assert_eq!(at(bottom - 60)[3], 255, "above the fade the list is drawn (glass {glass}): {:?}", at(bottom - 60));
        assert!(at(bottom - 3)[3] < 100, "the bottom of the list has faded out (glass {glass}): {:?}", at(bottom - 3));
    }
}

#[test]
fn a_masked_list_fades_out_its_filtered_children_too() {
    for scale in [1u32, 2] {
        let page = Page::new(scale as f32);
        page.doc.set_attribute(page.body, "style", "background: #fff").unwrap();
        let list = styled(
            &page,
            page.body,
            "position: absolute; left: 10px; top: 10px; width: 100px; height: 120px; overflow: auto; \
             mask-image: linear-gradient(#000 calc(100% - 28px), transparent)",
        );
        for _ in 0..8 {
            styled(&page, list, "height: 50px; background: #000; filter: drop-shadow(0 2px 2px rgba(0, 0, 0, .3))");
        }
        let frame = page.frame();
        let pixels = segments(&frame);
        let at = |y: u32| pixel(&pixels, frame.size.0, (50 * scale, y * scale));
        assert_eq!(at(30), [0, 0, 0, 255], "above the fade a child is drawn (scale {scale})");
        assert!(at(126)[0] > 150, "a filtered child fades out with the list (scale {scale}): {:?}", at(126));
        assert_eq!(at(140), [255, 255, 255, 255], "nothing below the list (scale {scale})");
    }
}
