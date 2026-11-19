use std::time::Duration;

use acde::render::Options;

mod common;
use common::Doc;

fn page() -> (Doc, u32, u32) {
    let doc = Doc::new(400, 300, 1.0, &Options::default()).unwrap();
    doc.add_stylesheet(
        "@keyframes va { from { vertical-align: 0px } to { vertical-align: 100px } }
         .a { animation: va 100s -50s linear }",
    )
    .unwrap();
    let body = doc.create_element("body").unwrap();
    doc.append_child(0, body).unwrap();
    let animated = doc.create_element("div").unwrap();
    doc.set_attribute(animated, "class", "a").unwrap();
    doc.append_child(body, animated).unwrap();
    let other = doc.create_element("div").unwrap();
    doc.append_child(body, other).unwrap();
    (doc, animated, other)
}

fn vertical_align(doc: &Doc, id: u32) -> String {
    doc.computed_style(id, "", "vertical-align").unwrap()
}

#[test]
fn animation_time_holds_still_within_a_script_task() {
    let (doc, animated, other) = page();
    assert_eq!(vertical_align(&doc, animated), "50px");
    std::thread::sleep(Duration::from_millis(40));
    doc.set_attribute(other, "class", "changed").unwrap();
    assert_eq!(vertical_align(&doc, animated), "50px", "a restyle later in the same task samples at the task's time");
    std::thread::sleep(Duration::from_millis(40));
    assert_eq!(vertical_align(&doc, animated), "50px", "an animation frame's worth of time passing inside a task does not resample");
}

#[test]
fn animation_time_moves_on_after_the_task_ends() {
    let (doc, animated, _) = page();
    assert_eq!(vertical_align(&doc, animated), "50px");
    doc.commit();
    std::thread::sleep(Duration::from_millis(40));
    let later = vertical_align(&doc, animated);
    let px: f64 = later.trim_end_matches("px").parse().unwrap();
    assert!(px > 50.03 && px < 51.0, "the next task sees the time that passed: {later}");
}
