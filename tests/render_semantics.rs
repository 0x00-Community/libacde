use acde::render::{NONE, Options};
use serde_json::Value;

mod common;
use common::Doc;

fn page(html: &str) -> Doc {
    let doc = Doc::new(480, 320, 1.0, &Options::default()).unwrap();
    doc.dom.parse_document(&format!("<!doctype html><style>body {{ margin: 0; font: 16px/20px sans-serif; }}</style>{html}")).unwrap();
    doc.commit();
    doc
}

fn nodes(doc: &Doc, invisible: bool) -> Vec<Value> {
    let tree: Value = serde_json::from_str(&doc.semantics(NONE, 600, true, invisible).unwrap()).unwrap();
    let mut out = Vec::new();
    let mut stack: Vec<Value> = tree["nodes"].as_array().cloned().unwrap_or_default();
    while let Some(node) = stack.pop() {
        stack.extend(node["children"].as_array().cloned().unwrap_or_default());
        out.push(node);
    }
    out
}

fn named(doc: &Doc, name: &str, invisible: bool) -> Option<Value> {
    nodes(doc, invisible).into_iter().find(|n| n["name"] == name)
}

fn states(node: &Value) -> Vec<&str> {
    node["states"].as_array().map(|s| s.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
}

#[test]
fn transparent_and_invisible_elements_show_only_when_asked() {
    let doc = page(
        "<button style='opacity: 0'>Delete</button>\
         <div style='opacity: 0'><button>Edit</button></div>\
         <button style='visibility: hidden' aria-label=Hidden>Hidden</button>\
         <button style='display: none'>Gone</button>\
         <button>Plain</button>",
    );
    for name in ["Delete", "Edit", "Hidden", "Gone"] {
        assert!(named(&doc, name, false).is_none(), "{name} is left out by default");
    }
    let plain = named(&doc, "Plain", true).unwrap();
    assert!(states(&plain).is_empty(), "{plain}");
    assert_eq!(states(&named(&doc, "Delete", true).unwrap()), ["transparent"]);
    assert_eq!(states(&named(&doc, "Edit", true).unwrap()), ["transparent"], "inside a transparent element");
    assert_eq!(states(&named(&doc, "Hidden", true).unwrap()), ["invisible"]);
    assert!(named(&doc, "Gone", true).is_none(), "display: none has no box");
    assert!(nodes(&doc, true).iter().all(|n| n["role"] != "generic"), "a transparent wrapper is not listed on its own");
}

fn id_of(doc: &Doc, name: &str) -> Option<u64> {
    named(doc, name, false).and_then(|n| n["id"].as_u64())
}

#[test]
fn a_ref_follows_its_node_across_a_list_re_render() {
    let doc = page("<ul id=l><li id=a>Alpha</li><li id=b>Beta</li></ul>");
    let (list, a, b) = (doc.get_element_by_id("l"), doc.get_element_by_id("a"), doc.get_element_by_id("b"));
    assert_eq!((id_of(&doc, "Alpha"), id_of(&doc, "Beta")), (Some(u64::from(a)), Some(u64::from(b))));
    let c = doc.create_element("li").unwrap();
    let text = doc.create_text("Gamma").unwrap();
    doc.append_child(c, text).unwrap();
    doc.insert_before(list, c, Some(a)).unwrap();
    doc.insert_before(list, b, Some(a)).unwrap();
    doc.set_text_content(a, "Alpha again").unwrap();
    doc.set_attribute(b, "class", "done").unwrap();
    doc.commit();
    assert_eq!(id_of(&doc, "Alpha again"), Some(u64::from(a)), "new text, same node");
    assert_eq!(id_of(&doc, "Beta"), Some(u64::from(b)), "moved, same node");
    assert_eq!(id_of(&doc, "Gamma"), Some(u64::from(c)));
    let fresh = doc.create_element("li").unwrap();
    let text = doc.create_text("Alpha again").unwrap();
    doc.append_child(fresh, text).unwrap();
    doc.replace_children(list, &[c, b, fresh]).unwrap();
    doc.commit();
    assert_eq!(id_of(&doc, "Alpha again"), Some(u64::from(fresh)), "a replaced node has a new ref");
    assert!(doc.semantics(a, 10, true, false).is_err(), "the old ref is gone with its node");
    assert_eq!(id_of(&doc, "Beta"), Some(u64::from(b)));
}

#[test]
fn output_is_a_status() {
    let doc = page("<form><input id=a value=2><output for=a>4</output></form>");
    assert!(nodes(&doc, false).iter().any(|n| n["role"] == "status"), "{:?}", nodes(&doc, false));
}

#[test]
fn a_label_names_a_button_and_the_other_labelable_elements() {
    let doc = page(
        "<button role=checkbox id=agree aria-checked=false><svg width=10 height=10></svg></button><label for=agree>I agree</label>\
         <label>Volume <progress value=3 max=10></progress></label>\
         <label for=save>Keep it</label><button id=save>Save</button>",
    );
    let role = |role: &str| nodes(&doc, false).into_iter().find(|n| n["role"] == role).map(|n| n["name"].clone());
    assert_eq!(role("checkbox").unwrap(), "I agree", "Radix's checkbox button, named by <label for>");
    assert_eq!(role("progressbar").unwrap(), "Volume", "a wrapping label");
    assert_eq!(role("button").unwrap(), "Keep it", "a label names a button before its text");
}

#[test]
fn adjacent_text_nodes_join_as_written() {
    let doc = page("<p>100<!---->%</p><p>a <!---->b</p><p>c<!----> d</p>");
    for name in ["100%", "a b", "c d"] {
        assert!(named(&doc, name, false).is_some(), "{name}: {:?}", nodes(&doc, false).iter().map(|n| n["name"].clone()).collect::<Vec<_>>());
    }
}

#[test]
fn hidden_descendants_stay_out_of_names() {
    let doc = page(
        "<button><span aria-hidden=true>\u{e121}</span>Move</button>\
         <ul role=listbox><li role=option aria-selected=true>Layer A <i aria-hidden=true>eye</i></li></ul>\
         <button title=Icon><i aria-hidden=true title=glyph>x</i></button>\
         <span id=lbl>Size <b inert>(px)</b></span><input aria-labelledby=lbl>\
         <label>Opacity <span aria-hidden=true>%</span><input type=number></label>",
    );
    assert!(named(&doc, "Move", false).is_some(), "the hidden glyph is not part of the button's name");
    let option = named(&doc, "Layer A", false).expect("the option is named by its visible text only");
    assert_eq!(states(&option), ["selected"]);
    assert!(named(&doc, "Icon", false).is_some(), "a hidden descendant's title does not name the button; its own title does");
    assert!(named(&doc, "Size", false).is_some(), "aria-labelledby skips inert parts of the label");
    assert!(named(&doc, "Opacity", false).is_some(), "a wrapping label skips its hidden parts");
    assert!(nodes(&doc, false).iter().all(|n| n["name"] != "eye" && n["name"] != "glyph"), "hidden nodes are not listed either");
}
