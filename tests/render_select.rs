use acde::render::{NONE, Options, Rect};

mod common;
use common::{Doc, WaitFrame};

const W: u32 = 480;
const H: u32 = 320;

const STYLE: &str = "<!doctype html><style>
    body { margin: 0; font: 16px/20px sans-serif; }
    select { font: 16px/20px sans-serif; margin: 0; }
</style>";

fn page(html: &str) -> Doc {
    let doc = Doc::new(W, H, 1.0, &Options::default()).unwrap();
    doc.dom.parse_document(&format!("{STYLE}{html}")).unwrap();
    doc.commit();
    doc
}

fn id(doc: &Doc, name: &str) -> u32 {
    let found = doc.dom.get_element_by_id(name);
    assert_ne!(found, NONE, "#{name}");
    found
}

fn rect(doc: &Doc, name: &str) -> Rect {
    doc.rect(id(doc, name)).unwrap()
}

fn pixels(doc: &Doc) -> Vec<u8> {
    common::rasterize(doc.scene(), (W, H))
}

fn rgb(pixels: &[u8], x: f64, y: f64) -> [u8; 3] {
    let i = ((y as usize) * W as usize + x as usize) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

fn dark_in(pixels: &[u8], r: Rect) -> usize {
    let mut n = 0;
    for y in r.y as usize..(r.y + r.height) as usize {
        for x in r.x as usize..(r.x + r.width) as usize {
            let [red, green, blue] = rgb(pixels, x as f64, y as f64);
            if u32::from(red) + u32::from(green) + u32::from(blue) < 200 {
                n += 1;
            }
        }
    }
    n
}

fn selected(doc: &Doc, select: &str) -> Vec<bool> {
    doc.options(id(doc, select)).unwrap().into_iter().map(|o| doc.option_selected(o).unwrap()).collect()
}

#[test]
fn a_drop_down_box_is_as_wide_as_its_widest_option() {
    let doc = page(
        "<select id=a><option>a</option><option>a much longer label</option></select>\
         <select id=b><option>a</option></select>",
    );
    let (a, b) = (rect(&doc, "a"), rect(&doc, "b"));
    assert!(a.width > b.width + 100.0, "the widest label sets the width: {a:?} {b:?}");
    assert!((a.height - (20.0 + 2.0 + 2.0)).abs() < 0.6, "a line, padding and border: {a:?}");
    assert!(b.width > 20.0, "a label and an arrow: {b:?}");
    assert!((a.y - b.y).abs() < 0.6, "inline-block, on one line: {a:?} {b:?}");
}

#[test]
fn a_drop_down_box_draws_its_selected_label_and_an_arrow() {
    let doc = page(
        "<select id=s style='color: black'><option>xx</option><option selected>MMMMMMMM</option>\
         <option>x</option></select>",
    );
    let s = rect(&doc, "s");
    let px = pixels(&doc);
    let label = Rect { x: s.x + 5.0, y: s.y + 2.0, width: s.width / 2.0, height: s.height - 4.0 };
    assert!(dark_in(&px, label) > 40, "the selected label is drawn");
    let arrow = Rect { x: s.x + s.width - 20.0, y: s.y + 2.0, width: 15.0, height: s.height - 4.0 };
    assert!(dark_in(&px, arrow) > 4, "the arrow is drawn");
}

#[test]
fn changing_the_selection_or_a_label_rebuilds_the_box() {
    let doc = page("<select id=s><option>short</option><option id=o>tiny</option></select>");
    let before = rect(&doc, "s");
    let o = id(&doc, "o");
    let text = doc.dom.first_child(o).unwrap();
    doc.dom.set_data(text, "a label that is much longer than before").unwrap();
    doc.commit();
    let after = rect(&doc, "s");
    assert!(after.width > before.width + 100.0, "{before:?} {after:?}");
    doc.set_selected_index(id(&doc, "s"), 1).unwrap();
    doc.commit();
    let px = pixels(&doc);
    let tail = Rect { x: after.x + 150.0, y: after.y + 2.0, width: 100.0, height: after.height - 4.0 };
    assert!(dark_in(&px, tail) > 20, "the long label is the shown one now");
}

#[test]
fn a_list_box_shows_its_display_size_in_rows() {
    let doc = page(
        "<select id=m multiple><option>a</option><option selected>b</option><option>c</option></select>\
         <select id=s size=3><option>a</option><option>b</option></select>",
    );
    let (m, s) = (rect(&doc, "m"), rect(&doc, "s"));
    assert!((m.height - (4.0 * 20.0 + 2.0)).abs() < 0.6, "multiple: 4 rows: {m:?}");
    assert!((s.height - (3.0 * 20.0 + 2.0)).abs() < 0.6, "size=3: {s:?}");
    let px = pixels(&doc);
    let row = rgb(&px, m.x + m.width - 4.0, m.y + 1.0 + 20.0 + 10.0);
    assert_eq!(row, [206, 206, 206], "the selected row of an unfocused list box");
    let other = rgb(&px, m.x + m.width - 4.0, m.y + 1.0 + 10.0);
    assert_eq!(other, [255, 255, 255]);
}

#[test]
fn a_list_box_scrolls_through_its_rows_and_finds_them_at_a_point() {
    let doc = page("<select id=s size=2><option>a</option><option>b</option><option>c</option><option>d</option></select>");
    let s = id(&doc, "s");
    let r = doc.rect(s).unwrap();
    assert_eq!(doc.list_box_option_at(s, r.x + 5.0, r.y + 1.0 + 25.0).unwrap(), 1);
    doc.scroll_by(s, 0.0, 40.0).unwrap();
    doc.commit();
    assert_eq!(doc.list_box_option_at(s, r.x + 5.0, r.y + 1.0 + 5.0).unwrap(), 2, "scrolled by two rows");
    assert_eq!(doc.list_box_option_at(s, r.x + 5.0, r.y + r.height + 5.0).unwrap(), -1);
}

#[test]
fn optgroups_label_and_indent_their_options() {
    let doc = page(
        "<select id=s size=5><optgroup label=Fruit><option>apple</option><option>pear</option></optgroup>\
         <option>loose</option><div><option>not in the list</option></div></select>",
    );
    let s = id(&doc, "s");
    assert_eq!(doc.options(s).unwrap().len(), 3, "options and options in optgroups, not deeper");
    let r = doc.rect(s).unwrap();
    assert_eq!(doc.list_box_option_at(s, r.x + 5.0, r.y + 1.0 + 5.0).unwrap(), -1, "the group label");
    assert_eq!(doc.list_box_option_at(s, r.x + 5.0, r.y + 1.0 + 25.0).unwrap(), 0);
    assert_eq!(doc.list_box_option_at(s, r.x + 5.0, r.y + 1.0 + 65.0).unwrap(), 2);
}

#[test]
fn a_select_without_multiple_keeps_one_option_selected() {
    let doc = page(
        "<select id=a><option>a</option><option selected>b</option><option selected>c</option></select>\
         <select id=b><option disabled>a</option><option>b</option></select>\
         <select id=c size=2><option>a</option><option>b</option></select>",
    );
    assert_eq!(selected(&doc, "a"), [false, false, true], "the last selected one wins");
    assert_eq!(selected(&doc, "b"), [false, true], "the first enabled option");
    assert_eq!(selected(&doc, "c"), [false, false], "a list box may have none");
    let a = id(&doc, "a");
    let options = doc.options(a).unwrap();
    doc.set_option_selected(options[0], true).unwrap();
    assert_eq!(selected(&doc, "a"), [true, false, false]);
    assert!(doc.matches(options[0], ":checked").unwrap());
    assert!(!doc.matches(options[2], ":checked").unwrap());
    doc.set_option_selected(options[0], false).unwrap();
    assert_eq!(selected(&doc, "a"), [true, false, false], "asking for a reset selects the first again");

    let extra = doc.create_element("option").unwrap();
    doc.set_attribute(extra, "selected", "").unwrap();
    doc.append_child(a, extra).unwrap();
    assert_eq!(selected(&doc, "a"), [false, false, false, true], "an inserted selected option takes over");
    doc.remove(extra).unwrap();
    assert_eq!(selected(&doc, "a"), [true, false, false], "removing it selects the first");

    doc.set_selected_index(a, -1).unwrap();
    assert_eq!(doc.selected_index(a).unwrap(), -1, "selectedIndex = -1 empties it");
    assert_eq!(doc.value(a).unwrap(), "");
}

#[test]
fn the_selected_attribute_moves_only_clean_options() {
    let doc = page("<select id=s multiple><option>a</option><option>b</option></select>");
    let s = id(&doc, "s");
    let options = doc.options(s).unwrap();
    doc.set_attribute(options[0], "selected", "").unwrap();
    assert_eq!(selected(&doc, "s"), [true, false], "a clean option follows its attribute");
    doc.set_option_selected(options[1], true).unwrap();
    doc.remove_attribute(options[1], "selected").unwrap();
    doc.set_attribute(options[1], "selected", "").unwrap();
    doc.remove_attribute(options[1], "selected").unwrap();
    assert_eq!(selected(&doc, "s"), [true, true], "a dirty one keeps what was set");
    doc.reset_select(s).unwrap();
    assert_eq!(selected(&doc, "s"), [true, false], "reset goes back to the attributes");
}

#[test]
fn turning_multiple_off_keeps_the_last_selected_option() {
    let doc = page("<select id=s multiple><option selected>a</option><option selected>b</option><option>c</option></select>");
    assert_eq!(selected(&doc, "s"), [true, true, false]);
    doc.remove_attribute(id(&doc, "s"), "multiple").unwrap();
    assert_eq!(selected(&doc, "s"), [false, true, false]);
}

#[test]
fn the_picker_opens_below_the_select_and_finds_options() {
    let doc = page(
        "<div style='height: 40px'></div><select id=s><option>one</option><option selected>two</option>\
         <optgroup label=More><option>three</option></optgroup></select>",
    );
    let s = id(&doc, "s");
    let r = doc.rect(s).unwrap();
    assert!(doc.open_picker(s).unwrap());
    doc.commit();
    let [owner, active, page] = doc.picker_state();
    assert_eq!(owner, f64::from(s));
    assert_eq!(active, 1.0, "the selected option is highlighted");
    assert!(page >= 4.0, "all four rows fit: {page}");
    let below = r.y + r.height + 2.0 + 4.0;
    let row = 20.0 + 6.0;
    assert_eq!(doc.picker_option_at(r.x + 10.0, below + 0.5 * row), 0);
    assert_eq!(doc.picker_option_at(r.x + 10.0, below + 2.5 * row), -1, "the group label");
    assert_eq!(doc.picker_option_at(r.x + 10.0, below + 3.5 * row), 2);
    assert_eq!(doc.picker_option_at(r.x + 10.0, r.y + 2.0), -2, "outside the picker");
    let px = pixels(&doc);
    assert_eq!(rgb(&px, r.x + r.width - 6.0, below + 1.5 * row), [25, 103, 210], "the highlighted row");
    doc.set_picker_active(0);
    doc.commit();
    let px = pixels(&doc);
    assert_eq!(rgb(&px, r.x + r.width - 6.0, below + 1.5 * row), [255, 255, 255]);
    doc.close_picker();
    doc.commit();
    assert_eq!(doc.picker_state()[0], -1.0);
    let px = pixels(&doc);
    let (x, y) = ((r.x + r.width - 6.0) as usize, (below + 1.5 * row) as usize);
    assert_eq!(px[(y * W as usize + x) * 4 + 3], 0, "nothing drawn there once it is closed");
}

#[test]
fn the_picker_opens_above_a_select_near_the_bottom_and_scrolls() {
    let options: String = (0..30).map(|i| format!("<option>item {i}</option>")).collect();
    let doc = page(&format!("<div style='height: 280px'></div><select id=s>{options}</select>"));
    let s = id(&doc, "s");
    let r = doc.rect(s).unwrap();
    assert!(doc.open_picker(s).unwrap());
    let top = doc.picker_option_at(r.x + 10.0, r.y - 10.0);
    assert!(top >= 0, "rows above the select: {top}");
    assert_eq!(doc.picker_option_at(r.x + 10.0, r.y + r.height + 6.0), -2, "nothing below it");
    let first = doc.picker_option_at(r.x + 10.0, 10.0);
    let handle = doc.handle();
    handle.commit();
    handle.scroll_at((r.x + 10.0) as f32, 10.0, 0.0, 26.0 * 3.0);
    handle.wait_frame();
    assert_eq!(doc.picker_option_at(r.x + 10.0, 10.0), first + 3, "the wheel scrolls the picker, not the page");
    assert_eq!(doc.metrics(doc.document()).unwrap().scroll_top, 0.0);
    doc.set_picker_active(29);
    assert_eq!(doc.picker_state()[1], 29.0);
    assert_eq!(doc.picker_option_at(r.x + 10.0, r.y - 10.0), 29, "the active row is scrolled into view");
}

#[test]
fn a_list_box_has_no_picker_and_a_removed_select_closes_it() {
    let doc = page("<select id=m multiple><option>a</option></select><select id=s><option>a</option></select>");
    assert!(!doc.open_picker(id(&doc, "m")).unwrap());
    let s = id(&doc, "s");
    assert!(doc.open_picker(s).unwrap());
    doc.remove(s).unwrap();
    doc.commit();
    doc.scene();
    assert_eq!(doc.picker_state()[0], -1.0);
}

#[test]
fn list_box_options_have_their_rows_as_boxes_and_are_hit() {
    let doc = page("<select id=s size=2><option id=a>a</option><option id=b>b</option><option id=c>c</option></select>");
    let (s, b, c) = (rect(&doc, "s"), rect(&doc, "b"), rect(&doc, "c"));
    assert!((b.y - (s.y + 1.0 + 20.0)).abs() < 0.6 && (b.height - 20.0).abs() < 0.6, "{s:?} {b:?}");
    assert!((b.x - (s.x + 1.0)).abs() < 0.6 && (b.width - (s.width - 2.0)).abs() < 0.6, "the padding box's width: {s:?} {b:?}");
    assert!(c.y >= s.y + s.height - 1.0, "scrolled out, still where it is: {c:?}");
    assert_eq!(doc.hit_test(s.x + 8.0, s.y + 30.0), Some(id(&doc, "b")), "a row hits its option");
    let all = doc.hit_test_all(s.x + 8.0, s.y + 30.0);
    assert_eq!(&all[..2], &[id(&doc, "b"), id(&doc, "s")]);
    let d = page("<select id=s><option id=a>a</option></select>");
    let a = rect(&d, "a");
    assert_eq!((a.width, a.height), (0.0, 0.0), "a drop-down box's options have no box");
}

#[test]
fn the_semantics_tree_shows_an_open_pickers_options() {
    let doc = page("<select id=s><option>one</option><option>two</option></select>");
    let s = id(&doc, "s");
    let closed = doc.semantics(NONE, 100, true, false).unwrap();
    assert!(closed.contains("\"collapsed\"") && !closed.contains("\"option\""), "{closed}");
    doc.open_picker(s).unwrap();
    let open = doc.semantics(NONE, 100, true, false).unwrap();
    assert!(open.contains("\"expanded\"") && open.matches("\"option\"").count() == 2, "{open}");
    let two = doc.options(s).unwrap()[1];
    let [x, y, w, h] = doc.picker_option_rect(two).unwrap().unwrap();
    assert_eq!(doc.picker_option_at(x + w / 2.0, y + h / 2.0), 1);
}

#[test]
fn the_wheel_scrolls_a_list_box() {
    let doc = page("<select id=s size=2><option>a</option><option>b</option><option>c</option><option>d</option></select>");
    let s = id(&doc, "s");
    let r = doc.rect(s).unwrap();
    let handle = doc.handle();
    handle.commit();
    handle.scroll_at((r.x + 5.0) as f32, (r.y + 10.0) as f32, 0.0, 30.0);
    handle.wait_frame();
    assert_eq!(doc.metrics(s).unwrap().scroll_top, 30.0);
    assert_eq!(doc.take_scrolled(), vec![s]);
}
