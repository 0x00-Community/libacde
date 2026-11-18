mod common;

use acde::render::{Options, Resource};
use common::Doc;

fn page(dir: &std::path::Path) -> (Doc, u32, u32, u32) {
    let options = Options { resources: Some(vec![Resource { prefix: "/".into(), dirs: vec![dir.to_string_lossy().into_owned()] }]), ..Default::default() };
    let doc = Doc::new(400, 300, 1.0, &options).unwrap();
    let html = doc.create_element("html").unwrap();
    let head = doc.create_element("head").unwrap();
    let link = doc.create_element("link").unwrap();
    doc.set_attribute(link, "rel", "stylesheet").unwrap();
    doc.set_attribute(link, "href", "/a.css").unwrap();
    let style = doc.create_element("style").unwrap();
    let css = doc.create_text("h1 { font-size: 22px; margin: 0 } p { font-size: 10px }").unwrap();
    doc.append_child(style, css).unwrap();
    doc.append_child(head, style).unwrap();
    doc.append_child(head, link).unwrap();
    let body = doc.create_element("body").unwrap();
    let h1 = doc.create_element("h1").unwrap();
    let text = doc.create_text("hello").unwrap();
    doc.append_child(h1, text).unwrap();
    let p = doc.create_element("p").unwrap();
    let ptext = doc.create_text("world").unwrap();
    doc.append_child(p, ptext).unwrap();
    doc.append_child(body, h1).unwrap();
    doc.append_child(body, p).unwrap();
    doc.append_child(html, head).unwrap();
    doc.append_child(html, body).unwrap();
    doc.append_child(0, html).unwrap();
    doc.commit();
    (doc, link, css, h1)
}

#[test]
fn swapped_stylesheets_relayout() {
    let dir = std::env::temp_dir().join(format!("acde-swap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.css"), "h1 { color: tomato }").unwrap();
    std::fs::write(dir.join("b.css"), "h1 { color: blue; font-size: 60px }").unwrap();
    let (doc, link, css, h1) = page(&dir);
    let p = h1 + 2;
    let before = doc.rect(h1).unwrap().height;
    let p_before = doc.rect(p).unwrap().height;
    doc.set_attribute(link, "href", "/b.css").unwrap();
    doc.set_data(css, "h1 { font-size: 22px; margin: 0 } p { font-size: 30px }").unwrap();
    doc.commit();
    doc.sync();
    let after = doc.rect(h1).unwrap().height;
    let p_after = doc.rect(p).unwrap().height;
    std::fs::remove_dir_all(&dir).ok();
    assert!(after > before * 2.0, "link: h1 {before} -> {after}");
    assert!(p_after > p_before * 2.0, "style: p {p_before} -> {p_after}");
}

#[test]
fn swapped_link_in_a_parsed_page_wins_over_an_earlier_style() {
    let dir = std::env::temp_dir().join(format!("acde-swap-parsed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.css"), "h1 { color: tomato }").unwrap();
    std::fs::write(dir.join("b.css"), "h1 { color: blue; font-size: 60px }").unwrap();
    let options = Options { resources: Some(vec![Resource { prefix: "/".into(), dirs: vec![dir.to_string_lossy().into_owned()] }]), ..Default::default() };
    let doc = Doc::new(400, 300, 1.0, &options).unwrap();
    doc.dom.parse_document("<!doctype html><html><head><style>h1 { margin: 0; font-size: 22px }</style><link rel=stylesheet href=/a.css></head><body><h1>hello</h1></body></html>").unwrap();
    doc.commit();
    doc.sync();
    let h1 = doc.dom.query_selector(doc.dom.document(), "h1").unwrap();
    let link = doc.dom.query_selector(doc.dom.document(), "link").unwrap();
    let before = doc.rect(h1).unwrap().height;
    doc.dom.set_attribute(link, "href", "/b.css?acde-dev=1").unwrap();
    let style = doc.dom.query_selector(doc.dom.document(), "style").unwrap();
    doc.dom.set_text_content(style, "h1 { margin: 0; font-size: 22px }").unwrap();
    doc.commit();
    doc.sync();
    let after = doc.rect(h1).unwrap().height;
    std::fs::remove_dir_all(&dir).ok();
    assert!(after > before * 2.0, "parsed: h1 {before} -> {after}");
}

#[test]
fn styles_in_a_parsed_inert_document_do_not_apply() {
    let doc = Doc::new(400, 300, 1.0, &Options::default()).unwrap();
    doc.dom.parse_document("<!doctype html><html><head><style>h1 { margin: 0; font-size: 60px }</style></head><body><h1>hello</h1></body></html>").unwrap();
    doc.commit();
    doc.sync();
    let h1 = doc.dom.query_selector(doc.dom.document(), "h1").unwrap();
    let before = doc.rect(h1).unwrap().height;
    doc.dom.parse_inert("<!doctype html><html><head><style>h1 { font-size: 10px }</style></head><body></body></html>", "about:blank").unwrap();
    doc.commit();
    doc.sync();
    let after = doc.rect(h1).unwrap().height;
    assert_eq!(before, after, "inert: h1 {before} -> {after}");
}

#[test]
fn a_link_in_a_parsed_inert_document_does_not_reorder_the_page() {
    let dir = std::env::temp_dir().join(format!("acde-swap-inert-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.css"), "h1 { color: tomato }").unwrap();
    std::fs::write(dir.join("b.css"), "h1 { color: blue; font-size: 60px }").unwrap();
    let options = Options { resources: Some(vec![Resource { prefix: "/".into(), dirs: vec![dir.to_string_lossy().into_owned()] }]), ..Default::default() };
    let doc = Doc::new(400, 300, 1.0, &options).unwrap();
    let page =
        "<!doctype html><html><head><style>h1 { margin: 0; font-size: 22px }</style><link rel=stylesheet href=/a.css></head><body><h1>hello</h1></body></html>";
    doc.dom.parse_document(page).unwrap();
    doc.commit();
    doc.sync();
    let h1 = doc.dom.query_selector(doc.dom.document(), "h1").unwrap();
    let link = doc.dom.query_selector(doc.dom.document(), "link").unwrap();
    let before = doc.rect(h1).unwrap().height;
    doc.dom.parse_inert(page, "about:blank").unwrap();
    doc.commit();
    doc.dom.set_attribute(link, "href", "/b.css?acde-dev=1").unwrap();
    doc.commit();
    doc.sync();
    let after = doc.rect(h1).unwrap().height;
    std::fs::remove_dir_all(&dir).ok();
    assert!(after > before * 2.0, "inert link: h1 {before} -> {after}");
}
