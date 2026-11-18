use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use acde::render::{Document, Dom, NONE, NodeType, Ns, Options};

mod common;
use common::WaitFrame;

const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAADCAYAAAC56t6BAAAAEUlEQVR4nGP4z8DwH4QZMBgAoXkL9U3EmgcAAAAASUVORK5CYII=";

fn page(html: &str) -> (Document, Dom) {
    let doc = Document::new(800, 600, 1.0, &Options::default()).unwrap();
    let dom = doc.dom();
    dom.parse_document(html).unwrap();
    (doc, dom)
}

fn by_id(dom: &Dom, id: &str) -> u32 {
    let found = dom.get_element_by_id(id);
    assert_ne!(found, NONE, "#{id}");
    found
}

fn error_name<T: std::fmt::Debug>(result: acde::render::Result<T>) -> String {
    let e = result.unwrap_err().0;
    e.split(':').next().unwrap().to_owned()
}

#[test]
fn document_and_tree_reads() {
    let (_doc, dom) = page(
        "<!DOCTYPE html><html><head><title>t</title></head>\
         <body><div id=a class='x y'>Hi<b>there</b></div><!--note--><p>last</p></body></html>",
    );
    let document = dom.document();
    assert_eq!(dom.node_type(document).unwrap(), NodeType::Document);
    assert_eq!(dom.document(), document, "the same id every time");
    let doctype = dom.first_child(document).unwrap();
    assert_eq!(dom.node_type(doctype).unwrap(), NodeType::DocumentType);
    assert_eq!(dom.document_type(doctype).unwrap().0, "html");
    let html = dom.next_sibling(doctype).unwrap();
    assert_eq!(dom.local_name(html).unwrap(), "html");
    assert_eq!(dom.last_child(document).unwrap(), html);
    assert_eq!(dom.previous_sibling(html).unwrap(), doctype);
    assert_eq!(dom.parent(html).unwrap(), document);
    assert_eq!(dom.parent(document).unwrap(), NONE);
    assert_eq!(dom.children(document).unwrap(), vec![html]);
    assert_eq!(dom.child_nodes(document).unwrap(), vec![doctype, html]);

    let a = by_id(&dom, "a");
    let body = dom.parent(a).unwrap();
    assert_eq!(dom.local_name(body).unwrap(), "body");
    assert_eq!(dom.child_count(body).unwrap(), 3);
    let kids = dom.child_nodes(body).unwrap();
    assert_eq!(dom.node_type(kids[1]).unwrap(), NodeType::Comment);
    assert_eq!(dom.data(kids[1]).unwrap(), "note");
    assert_eq!(dom.children(body).unwrap(), vec![a, kids[2]]);
    let hi = dom.first_child(a).unwrap();
    assert_eq!(dom.node_type(hi).unwrap(), NodeType::Text);
    assert_eq!(dom.data(hi).unwrap(), "Hi");
    assert_eq!(dom.next_sibling(dom.last_child(a).unwrap()).unwrap(), NONE);
    assert_eq!(dom.previous_sibling(hi).unwrap(), NONE);
    assert_eq!(dom.first_child(hi).unwrap(), NONE);
    assert!(dom.is_connected(hi).unwrap());
    assert!(dom.is_connected(document).unwrap());
    assert_eq!(dom.namespace(a).unwrap(), Ns::Html);
    assert_eq!(dom.id(a).unwrap(), "a");
    assert_eq!(dom.class_name(a).unwrap(), "x y");
    assert_eq!(dom.class_name(body).unwrap(), "");
    assert_eq!(dom.text_content(a).unwrap(), "Hithere");
    assert_eq!(dom.text_content(document).unwrap(), "");
    assert_eq!(dom.node_type(9999).map_err(|e| e.0.split(':').next().unwrap().to_owned()), Err("NotFoundError".into()));
    assert_eq!(dom.get_element_by_id("missing"), NONE);
    let viewport = dom.viewport();
    assert_eq!((viewport.width, viewport.height, viewport.scale), (800.0, 600.0, 1.0));
}

#[test]
fn noscript_is_text_and_scripts_stay() {
    let (_doc, dom) = page("<body><noscript><b>x</b></noscript><script id=s>let a = 1 < 2</script></body>");
    let noscript = dom.query_selector(dom.document(), "noscript").unwrap();
    let text = dom.first_child(noscript).unwrap();
    assert_eq!(dom.node_type(text).unwrap(), NodeType::Text);
    assert_eq!(dom.data(text).unwrap(), "<b>x</b>");
    assert_eq!(dom.text_content(by_id(&dom, "s")).unwrap(), "let a = 1 < 2");
}

#[test]
fn creation_and_tree_mutation() {
    let (_doc, dom) = page("<body><div id=a></div><div id=b></div></body>");
    let (a, b) = (by_id(&dom, "a"), by_id(&dom, "b"));
    let span = dom.create_element(Ns::Html, "SPAN").unwrap();
    assert_eq!(dom.local_name(span).unwrap(), "span");
    assert!(!dom.is_connected(span).unwrap());
    let text = dom.create_text("hello").unwrap();
    let comment = dom.create_comment("c").unwrap();
    assert_eq!(dom.node_type(comment).unwrap(), NodeType::Comment);
    dom.insert_before(span, text, NONE).unwrap();
    dom.insert_before(a, span, NONE).unwrap();
    assert!(dom.is_connected(text).unwrap());
    assert_eq!(dom.text_content(a).unwrap(), "hello");

    dom.insert_before(b, span, NONE).unwrap();
    assert_eq!(dom.child_count(a).unwrap(), 0);
    assert_eq!(dom.child_nodes(b).unwrap(), vec![span]);
    dom.insert_before(b, comment, span).unwrap();
    assert_eq!(dom.child_nodes(b).unwrap(), vec![comment, span]);
    dom.insert_before(b, comment, comment).unwrap();
    assert_eq!(dom.child_nodes(b).unwrap(), vec![comment, span]);

    let fragment = dom.create_fragment().unwrap();
    assert_eq!(dom.node_type(fragment).unwrap(), NodeType::DocumentFragment);
    let (x, y) = (dom.create_element(Ns::Html, "i").unwrap(), dom.create_element(Ns::Html, "u").unwrap());
    dom.insert_before(fragment, x, NONE).unwrap();
    dom.insert_before(fragment, y, NONE).unwrap();
    dom.insert_before(b, fragment, span).unwrap();
    assert_eq!(dom.child_nodes(b).unwrap(), vec![comment, x, y, span]);
    assert_eq!(dom.child_count(fragment).unwrap(), 0);
    assert_eq!(dom.parent(x).unwrap(), b);

    dom.pin(y).unwrap();
    dom.remove(y).unwrap();
    assert_eq!(dom.parent(y).unwrap(), NONE);
    assert!(!dom.is_connected(y).unwrap());
    dom.remove(y).unwrap();

    dom.pin(x).unwrap();
    dom.replace_children(b, &[y, x]).unwrap();
    assert_eq!(dom.child_nodes(b).unwrap(), vec![y, x]);
    dom.replace_children(b, &[]).unwrap();
    assert_eq!(dom.child_count(b).unwrap(), 0);

    dom.insert_before(a, b, NONE).unwrap();
    assert_eq!(error_name(dom.insert_before(b, a, NONE)), "HierarchyRequestError");
    assert_eq!(error_name(dom.insert_before(b, b, NONE)), "HierarchyRequestError");
    assert_eq!(error_name(dom.insert_before(a, x, span)), "NotFoundError");
    let leaf = dom.create_text("leaf").unwrap();
    assert_eq!(error_name(dom.insert_before(leaf, x, NONE)), "HierarchyRequestError");
    let document = dom.document();
    assert_eq!(error_name(dom.insert_before(document, x, NONE)), "HierarchyRequestError");
    let t = dom.create_text("t").unwrap();
    assert_eq!(error_name(dom.insert_before(document, t, NONE)), "HierarchyRequestError");
    assert_eq!(error_name(dom.insert_before(a, document, NONE)), "HierarchyRequestError");
    let doctype = dom.create_document_type("html", "", "").unwrap();
    assert_eq!(error_name(dom.insert_before(a, doctype, NONE)), "HierarchyRequestError");
    assert_eq!(error_name(dom.create_element(Ns::Html, "a b")), "InvalidCharacterError");
    assert_eq!(error_name(dom.create_element(Ns::Html, "")), "InvalidCharacterError");
    assert_eq!(error_name(dom.set_attribute(a, "a=b", "")), "InvalidCharacterError");
    dom.create_element(Ns::Html, "my-element").unwrap();
    dom.create_element(Ns::Html, "_x").unwrap();
}

#[test]
fn document_children_rules() {
    let (_doc, dom) = page("<!doctype html><html></html>");
    let document = dom.document();
    let html = dom.children(document).unwrap()[0];
    let doctype = dom.create_document_type("html", "", "").unwrap();
    assert_eq!(error_name(dom.insert_before(document, doctype, NONE)), "HierarchyRequestError");
    let first = dom.first_child(document).unwrap();
    let element = dom.create_element(Ns::Html, "x").unwrap();
    assert_eq!(error_name(dom.insert_before(document, element, first)), "HierarchyRequestError");
    dom.replace_children(document, &[first, element]).unwrap();
    assert_eq!(dom.children(document).unwrap(), vec![element]);
    assert!(!dom.is_connected(html).unwrap_or(false));
}

#[test]
fn compare_document_position_bits() {
    let (_doc, dom) = page("<body><div id=a><span id=b></span></div><p id=c></p></body>");
    let (a, b, c) = (by_id(&dom, "a"), by_id(&dom, "b"), by_id(&dom, "c"));
    assert_eq!(dom.compare_document_position(a, a).unwrap(), 0);
    assert_eq!(dom.compare_document_position(a, b).unwrap(), 16 | 4);
    assert_eq!(dom.compare_document_position(b, a).unwrap(), 8 | 2);
    assert_eq!(dom.compare_document_position(b, c).unwrap(), 4);
    assert_eq!(dom.compare_document_position(c, b).unwrap(), 2);
    let detached = dom.create_element(Ns::Html, "div").unwrap();
    let bits = dom.compare_document_position(a, detached).unwrap();
    assert_eq!(bits & (1 | 32), 1 | 32);
    let back = dom.compare_document_position(detached, a).unwrap();
    assert_eq!((bits & 6) ^ (back & 6), 6, "consistent order");
}

#[test]
fn attributes() {
    let (_doc, dom) = page(
        "<body><div id=a data-x='1' TITLE=t></div>\
         <svg id=s viewBox='0 0 10 10'><a id=l xlink:href='#x'></a></svg></body>",
    );
    let a = by_id(&dom, "a");
    assert_eq!(dom.get_attribute(a, "title").unwrap().as_deref(), Some("t"));
    assert_eq!(dom.get_attribute(a, "TITLE").unwrap().as_deref(), Some("t"));
    assert_eq!(dom.get_attribute(a, "nope").unwrap(), None);
    assert!(dom.has_attribute(a, "data-x").unwrap());
    dom.set_attribute(a, "Data-Y", "2").unwrap();
    assert_eq!(dom.get_attribute(a, "data-y").unwrap().as_deref(), Some("2"));
    dom.set_attribute(a, "data-x", "3").unwrap();
    assert_eq!(dom.attributes(a).unwrap(), vec!["id", "a", "data-x", "3", "title", "t", "data-y", "2"]);
    dom.remove_attribute(a, "DATA-X").unwrap();
    assert!(!dom.has_attribute(a, "data-x").unwrap());
    dom.remove_attribute(a, "never").unwrap();

    let s = by_id(&dom, "s");
    assert_eq!(dom.namespace(s).unwrap(), Ns::Svg);
    assert_eq!(dom.get_attribute(s, "viewBox").unwrap().as_deref(), Some("0 0 10 10"));
    assert_eq!(dom.get_attribute(s, "viewbox").unwrap(), None);
    let l = by_id(&dom, "l");
    assert_eq!(dom.get_attribute(l, "xlink:href").unwrap().as_deref(), Some("#x"));
    dom.set_attribute(l, "xlink:href", "#y").unwrap();
    assert_eq!(dom.attributes(l).unwrap(), vec!["id", "l", "xlink:href", "#y"]);
    let gradient = dom.create_element(Ns::Svg, "linearGradient").unwrap();
    assert_eq!(dom.local_name(gradient).unwrap(), "linearGradient");

    dom.set_attribute(a, "id", "renamed").unwrap();
    assert_eq!(dom.get_element_by_id("renamed"), a);
    assert_eq!(dom.get_element_by_id("a"), NONE);
    dom.set_attribute(a, "style", "width: 40px; height: 10px").unwrap();
    assert_eq!(dom.rect(a).unwrap()[2], 40.0);
    let text = dom.create_text("t").unwrap();
    assert_eq!(error_name(dom.get_attribute(text, "x")), "InvalidNodeTypeError");
}

#[test]
fn character_data_and_text_content() {
    let (_doc, dom) = page("<body><div id=a>x<b>y</b></div></body>");
    let a = by_id(&dom, "a");
    let x = dom.first_child(a).unwrap();
    dom.set_data(x, "changed").unwrap();
    assert_eq!(dom.text_content(a).unwrap(), "changedy");
    let c = dom.create_comment("one").unwrap();
    dom.set_data(c, "two").unwrap();
    assert_eq!(dom.data(c).unwrap(), "two");
    assert_eq!(dom.text_content(c).unwrap(), "two");
    dom.set_text_content(a, "plain").unwrap();
    assert_eq!(dom.child_count(a).unwrap(), 1);
    assert_eq!(dom.text_content(a).unwrap(), "plain");
    assert!(dom.node_type(x).is_err(), "the old unpinned children are freed");
    dom.set_text_content(a, "").unwrap();
    assert_eq!(dom.child_count(a).unwrap(), 0);
    dom.set_text_content(c, "three").unwrap();
    assert_eq!(dom.data(c).unwrap(), "three");
}

#[test]
fn inner_text_follows_layout() {
    let (_doc, dom) =
        page("<body><div id=a>one <span>two</span>\n   three<div>block</div><p>para</p><span style='display:none'>hidden</span>x<br>y</div></body>");
    assert_eq!(dom.inner_text(by_id(&dom, "a")).unwrap(), "one two three\nblock\n\npara\n\nx\ny");
}

#[test]
fn serialization() {
    let (_doc, dom) = page(
        "<body><div id=a title='a\"b&amp;c&lt;'>x &lt; y &amp; z &gt; \u{a0}<br><img src=q><style>a > b {}</style>\
         <!--c--><svg viewBox='0 0 1 1'><linearGradient></linearGradient></svg></div>\
         <template id=t><p>in</p></template></body>",
    );
    let a = by_id(&dom, "a");
    assert_eq!(
        dom.inner_html(a).unwrap(),
        "x &lt; y &amp; z &gt; &nbsp;<br><img src=\"q\"><style>a > b {}</style><!--c-->\
         <svg viewBox=\"0 0 1 1\"><linearGradient></linearGradient></svg>"
    );
    assert!(dom.outer_html(a).unwrap().starts_with("<div id=\"a\" title=\"a&quot;b&amp;c&lt;\">x &lt;"));
    let t = by_id(&dom, "t");
    assert_eq!(dom.child_count(t).unwrap(), 0, "template children go into its content");
    assert_eq!(dom.inner_html(t).unwrap(), "<p>in</p>");
    assert_eq!(dom.outer_html(t).unwrap(), "<template id=\"t\"><p>in</p></template>");
    let text = dom.create_text("<&>").unwrap();
    assert_eq!(dom.outer_html(text).unwrap(), "&lt;&amp;&gt;");
    let (_doc2, dom2) = page("<!DOCTYPE html><html><head></head><body></body></html>");
    let document = dom2.document();
    assert_eq!(dom2.outer_html(document).unwrap(), "<!DOCTYPE html><html><head></head><body></body></html>");
}

#[test]
fn fragment_parsing() {
    let (_doc, dom) = page("<body><div id=a></div><table><tbody id=tb></tbody></table></body>");
    let a = by_id(&dom, "a");
    let fragment = dom.parse_fragment(a, "<b id=x>bold</b> tail<img src='nowhere.png'>").unwrap();
    assert_eq!(dom.node_type(fragment).unwrap(), NodeType::DocumentFragment);
    assert!(!dom.is_connected(fragment).unwrap());
    let kids = dom.child_nodes(fragment).unwrap();
    assert_eq!(kids.len(), 3);
    assert_eq!(dom.local_name(kids[0]).unwrap(), "b");
    assert_eq!(dom.get_element_by_id("x"), NONE, "not in the document yet");
    assert_eq!(dom.query_selector(fragment, "#x").unwrap(), kids[0]);
    dom.replace_children(a, &[fragment]).unwrap();
    assert_eq!(dom.get_element_by_id("x"), kids[0]);
    assert_eq!(dom.inner_html(a).unwrap(), "<b id=\"x\">bold</b> tail<img src=\"nowhere.png\">");

    let tb = by_id(&dom, "tb");
    let rows = dom.parse_fragment(tb, "<tr><td>1</td></tr>").unwrap();
    let tr = dom.first_child(rows).unwrap();
    assert_eq!(dom.local_name(tr).unwrap(), "tr");
    assert_eq!(dom.local_name(dom.first_child(tr).unwrap()).unwrap(), "td");
    let style = dom.create_element(Ns::Html, "style").unwrap();
    let css = dom.parse_fragment(style, "a < b").unwrap();
    assert_eq!(dom.data(dom.first_child(css).unwrap()).unwrap(), "a < b");
    let in_doc = dom.parse_fragment(dom.document(), "<p>p</p>").unwrap();
    assert_eq!(dom.local_name(dom.first_child(in_doc).unwrap()).unwrap(), "p");
}

#[test]
fn templates_and_clone() {
    let (_doc, dom) = page("<body><template id=t><p class=k>in</p></template><input id=i value=v></body>");
    let t = by_id(&dom, "t");
    let content = dom.template_content(t).unwrap();
    assert_eq!(dom.template_content(t).unwrap(), content);
    assert_eq!(dom.node_type(content).unwrap(), NodeType::DocumentFragment);
    assert_eq!(dom.parent(content).unwrap(), NONE);
    assert_eq!(dom.inner_html(content).unwrap(), "<p class=\"k\">in</p>");
    let fresh = dom.create_element(Ns::Html, "template").unwrap();
    assert_eq!(dom.child_count(dom.template_content(fresh).unwrap()).unwrap(), 0);

    let shallow = dom.clone(t, false).unwrap();
    assert_eq!(dom.child_count(dom.template_content(shallow).unwrap()).unwrap(), 0);
    let deep = dom.clone(t, true).unwrap();
    assert!(!dom.is_connected(deep).unwrap());
    assert_eq!(dom.get_attribute(deep, "id").unwrap().as_deref(), Some("t"));
    let copy = dom.template_content(deep).unwrap();
    assert_ne!(copy, content);
    assert_eq!(dom.inner_html(copy).unwrap(), "<p class=\"k\">in</p>");

    let p = dom.first_child(content).unwrap();
    let p2 = dom.clone(p, true).unwrap();
    assert_eq!(dom.class_name(p2).unwrap(), "k");
    assert_eq!(dom.text_content(p2).unwrap(), "in");
    let i = by_id(&dom, "i");
    dom.set_value(i, "typed").unwrap();
    let i2 = dom.clone(i, false).unwrap();
    assert_eq!(dom.value(i2).unwrap(), "typed");
    assert_eq!(error_name(dom.clone(dom.document(), true)), "NotSupportedError");
}

#[test]
fn selectors() {
    let (_doc, dom) = page(
        "<body><div id=a class='x y'><p class=x>1</p><p class='y x'>2</p><svg><linearGradient id=g /></svg></div>\
         <p class=x id=out>3</p></body>",
    );
    let a = by_id(&dom, "a");
    let document = dom.document();
    let ps = dom.query_selector_all(document, "p.x").unwrap();
    assert_eq!(ps.len(), 3);
    let inside = dom.query_selector_all(a, "p").unwrap();
    assert_eq!(inside, ps[..2].to_vec());
    assert_eq!(dom.query_selector(a, ".x").unwrap(), ps[0], "the root itself never matches");
    assert_eq!(dom.query_selector(a, "div p").unwrap(), ps[0], "ancestors outside the root take part");
    assert_eq!(dom.query_selector(a, "table").unwrap(), NONE);
    assert!(dom.matches(a, "div.y").unwrap());
    assert!(!dom.matches(a, "p").unwrap());
    assert_eq!(dom.closest(ps[1], "div").unwrap(), a);
    assert_eq!(dom.closest(ps[1], "p").unwrap(), ps[1]);
    assert_eq!(dom.closest(ps[1], "table").unwrap(), NONE);
    assert_eq!(error_name(dom.query_selector(a, "p[")), "SyntaxError");
    assert_eq!(error_name(dom.matches(a, "::nope(")), "SyntaxError");

    assert_eq!(dom.get_elements_by_class_name(document, "x y").unwrap(), vec![a, ps[1]]);
    assert_eq!(dom.get_elements_by_class_name(a, "x").unwrap(), ps[..2].to_vec());
    assert!(dom.get_elements_by_class_name(document, "  ").unwrap().is_empty());
    assert_eq!(dom.get_elements_by_tag_name(document, "P").unwrap(), ps);
    assert_eq!(dom.get_elements_by_tag_name(a, "*").unwrap().len(), 4);
    let g = by_id(&dom, "g");
    assert_eq!(dom.get_elements_by_tag_name(a, "linearGradient").unwrap(), vec![g]);
    assert!(dom.get_elements_by_tag_name(a, "lineargradient").unwrap().is_empty());
}

#[test]
fn form_state() {
    let (_doc, dom) = page(
        "<body style='margin:0'><input id=i value=default style='width:300px;font:16px sans-serif'>\
         <textarea id=t>area</textarea>\
         <form><input type=radio name=r id=r1 checked><input type=radio name=r id=r2></form>\
         <input type=radio name=r id=r3 checked><input type=checkbox id=c>\
         <select id=s><option>a</option><option value=bv selected>b</option><option>c</option></select>\
         <select id=e></select><input type=hidden id=h value=hv></body>",
    );
    let i = by_id(&dom, "i");
    assert_eq!(dom.value(i).unwrap(), "default");
    let short = dom.caret_rect(i, 7).unwrap()[0];
    dom.set_value(i, "a much longer value\n").unwrap();
    assert_eq!(dom.value(i).unwrap(), "a much longer value");
    assert_eq!(dom.get_attribute(i, "value").unwrap().as_deref(), Some("default"));
    assert!(dom.caret_rect(i, 19).unwrap()[0] > short + 20.0);
    dom.set_attribute(i, "value", "attr").unwrap();
    assert_eq!(dom.value(i).unwrap(), "a much longer value");
    dom.set_selection_range(i, 2, 6).unwrap();

    let t = by_id(&dom, "t");
    assert_eq!(dom.value(t).unwrap(), "area");
    dom.set_value(t, "two\nlines").unwrap();
    assert_eq!(dom.value(t).unwrap(), "two\nlines");
    assert_eq!(dom.text_content(t).unwrap(), "area");

    let (r1, r2, r3, c) = (by_id(&dom, "r1"), by_id(&dom, "r2"), by_id(&dom, "r3"), by_id(&dom, "c"));
    assert!(dom.checked(r1).unwrap());
    assert!(dom.matches(r1, ":checked").unwrap());
    assert!(!dom.checked(c).unwrap());
    dom.set_checked(c, true).unwrap();
    assert!(dom.checked(c).unwrap());
    assert!(dom.matches(c, ":checked").unwrap());
    assert!(!dom.has_attribute(c, "checked").unwrap());
    dom.set_checked(r2, true).unwrap();
    assert!(!dom.checked(r1).unwrap(), "same group (same form) unchecked");
    assert!(!dom.matches(r1, ":checked").unwrap());
    assert!(dom.checked(r3).unwrap(), "outside the form: another group");
    assert_eq!(dom.value(c).unwrap(), "on");

    let s = by_id(&dom, "s");
    assert_eq!(dom.selected_index(s).unwrap(), 1);
    assert_eq!(dom.value(s).unwrap(), "bv");
    dom.set_selected_index(s, 2).unwrap();
    assert_eq!(dom.value(s).unwrap(), "c");
    dom.set_value(s, "a").unwrap();
    assert_eq!(dom.selected_index(s).unwrap(), 0);
    dom.set_selected_index(s, 7).unwrap();
    assert_eq!(dom.selected_index(s).unwrap(), -1);
    assert_eq!(dom.selected_index(by_id(&dom, "e")).unwrap(), -1);

    let h = by_id(&dom, "h");
    assert_eq!(dom.value(h).unwrap(), "hv");
    dom.set_value(h, "new").unwrap();
    assert_eq!(dom.get_attribute(h, "value").unwrap().as_deref(), Some("new"), "default mode: the attribute");
}

#[test]
fn focus_active_and_selection() {
    let (doc, dom) = page(
        "<body style='margin:0'><div id=w><button id=b>go</button></div>\
         <p id=p style='font: 20px sans-serif'>select some of this text</p></body>",
    );
    let (w, b) = (by_id(&dom, "w"), by_id(&dom, "b"));
    dom.set_focus(b, true).unwrap();
    assert!(dom.matches(b, ":focus").unwrap());
    assert!(dom.matches(b, ":focus-visible").unwrap());
    assert!(dom.matches(w, ":focus-within").unwrap());
    dom.set_focus(b, false).unwrap();
    assert!(dom.matches(b, ":focus").unwrap());
    assert!(!dom.matches(b, ":focus-visible").unwrap());
    dom.set_focus(NONE, false).unwrap();
    assert!(!dom.matches(b, ":focus").unwrap());
    assert!(!dom.matches(w, ":focus-within").unwrap());

    dom.set_active(b).unwrap();
    assert!(dom.matches(b, ":active").unwrap());
    assert!(dom.matches(w, ":active").unwrap());
    dom.set_active(NONE).unwrap();
    assert!(!dom.matches(b, ":active").unwrap());

    let handle = doc.handle();
    dom.commit();
    let before = handle.wait_frame();
    assert_ne!(before.commit, 0);
    let p = by_id(&dom, "p");
    let text = dom.first_child(p).unwrap();
    dom.set_selection(text, 7, text, 11).unwrap();
    dom.commit();
    let after = handle.wait_frame();
    let rect = dom.rect(p).unwrap();
    let damage = after.plane.damage;
    assert!(damage.width > 0.0 && damage.y < rect[1] + rect[3] && damage.y + damage.height > rect[1]);
    dom.set_selection(p, 0, p, 1).unwrap();
    dom.set_selection(NONE, 0, NONE, 0).unwrap();
}

#[test]
fn a_stylesheet_added_while_an_element_is_active_restyles() {
    let (_doc, dom) = page(
        "<head><style>div { width: 10px; height: 20px } #b:active { height: 40px }</style></head>\
         <body style='margin:0'><div id=b class=late></div></body>",
    );
    let b = by_id(&dom, "b");
    assert_eq!(dom.rect(b).unwrap()[3], 20.0);
    dom.set_active(b).unwrap();
    let head = dom.query_selector(dom.document(), "head").unwrap();
    let style = dom.create_element(Ns::Html, "style").unwrap();
    let css = dom.create_text(".late { width: 50px } #b { margin-left: 5px }").unwrap();
    dom.insert_before(style, css, NONE).unwrap();
    dom.insert_before(head, style, NONE).unwrap();
    assert_eq!(dom.rect(b).unwrap(), [5.0, 0.0, 50.0, 40.0]);
}

#[test]
fn layout_reads() {
    let (_doc, dom) = page(
        "<head><style>
            body { margin: 0; font: 20px/30px sans-serif }
            #box { position: absolute; left: 10px; top: 20px; width: 100px; height: 50px; color: red; padding: 5px }
            #box::before { content: 'b' }
            #wrap { position: absolute; left: 0; top: 200px; width: 60px }
            #t { position: absolute; left: 300px; top: 0; transform: translateX(50px) }
            iframe { position: absolute; left: 400px; top: 300px; width: 100px; height: 100px; border: 0 }
         </style></head>
         <body><div id=box>x</div><div id=wrap><span id=s>several words on lines</span></div>
         <div id=t style='width:10px;height:10px'></div><div id=none style='display:none'><i id=deep></i></div>
         <iframe id=f></iframe></body>",
    );
    let bx = by_id(&dom, "box");
    assert_eq!(dom.rect(bx).unwrap(), [10.0, 20.0, 110.0, 60.0]);
    assert_eq!(dom.rect(by_id(&dom, "t")).unwrap()[0], 350.0, "after transforms");
    assert_eq!(dom.rect(by_id(&dom, "none")).unwrap(), [0.0; 4]);
    let s = by_id(&dom, "s");
    let lines = dom.client_rects(s).unwrap();
    assert!(lines.len() >= 2, "{lines:?}");
    assert!(dom.client_rects(by_id(&dom, "deep")).unwrap().is_empty());

    let m = dom.metrics(bx).unwrap().unwrap();
    assert_eq!((m.offset_left, m.offset_top, m.offset_width, m.client_width), (10.0, 20.0, 110.0, 110.0));
    assert_eq!(m.offset_parent, Some(dom.query_selector(dom.document(), "body").unwrap()));
    assert!(dom.metrics(by_id(&dom, "deep")).unwrap().is_none());
    assert!(dom.metrics(dom.create_element(Ns::Html, "div").unwrap()).unwrap().is_none());

    assert_eq!(dom.hit_test(20.0, 30.0), Some(bx));
    let f = by_id(&dom, "f");
    assert_eq!(dom.hit_test(450.0, 350.0), Some(f));
    assert_eq!(dom.hit_test(-5.0, 10.0), None);
    let all = dom.hit_test_all(20.0, 30.0);
    assert_eq!(all[0], bx);
    assert!(all.contains(&dom.query_selector(dom.document(), "body").unwrap()));

    assert_eq!(dom.computed_style(bx, "", "color").unwrap(), "rgb(255, 0, 0)");
    assert_eq!(dom.computed_style(bx, "", "width").unwrap(), "100px");
    assert_eq!(dom.computed_style(bx, "", "not-a-property").unwrap(), "");
    assert_eq!(dom.computed_style(bx, "::before", "content").unwrap(), "\"b\"");
    let all = dom.computed_style_all(bx, "").unwrap();
    let display = all.chunks(2).find(|p| p[0] == "display").unwrap();
    assert_eq!(display[1], "block");

    let text = dom.first_child(s).unwrap();
    let caret = dom.caret_rect(text, 0).unwrap();
    assert_eq!(caret[1], 200.0);
    assert!(caret[3] > 0.0);
    let point = dom.caret_from_point(caret[0] + 1.0, caret[1] + 5.0).unwrap();
    assert_eq!((point.node, point.offset), (text, 0));
    assert!(dom.text_rects(text, 0, 20).unwrap().len() >= 2);

    assert!(dom.match_media("(min-width: 500px)"));
    assert!(!dom.match_media("(max-width: 500px)"));
    assert!(dom.match_media("screen and (orientation: landscape)"));
    assert!(!dom.match_media("print"));
    dom.resize(400, 600, 1.0);
    assert!(dom.match_media("(max-width: 500px)"));
    assert_eq!(dom.viewport().width, 400.0);
}

#[test]
fn scrolling() {
    let (doc, dom) = page(
        "<body style='margin:0;height:3000px'><div id=l style='position:absolute;top:0;left:0;width:100px;height:100px;overflow:scroll'>\
         <div style='height:1000px'></div></div></body>",
    );
    let l = by_id(&dom, "l");
    assert!(dom.take_scrolled().is_empty());
    dom.scroll_to(l, 0.0, 300.0, false).unwrap();
    assert_eq!(dom.metrics(l).unwrap().unwrap().scroll_top, 300.0);
    assert_eq!(dom.take_scrolled(), vec![l]);
    assert!(dom.take_scrolled().is_empty(), "reported once");
    dom.scroll_to(l, 0.0, 99999.0, false).unwrap();
    let m = dom.metrics(l).unwrap().unwrap();
    assert_eq!(m.scroll_top, m.scroll_height - m.client_height, "clamped");
    assert_eq!(dom.take_scrolled(), vec![l]);

    let document = dom.document();
    dom.scroll_to(document, 0.0, 500.0, false).unwrap();
    assert_eq!(dom.take_scrolled(), vec![document]);
    let html = dom.children(document).unwrap()[0];
    assert_eq!(dom.metrics(html).unwrap().unwrap().scroll_top, 500.0);
    dom.scroll_to(html, 0.0, 0.0, false).unwrap();
    assert_eq!(dom.take_scrolled(), vec![document]);

    dom.commit();
    let handle = doc.handle();
    handle.scroll_at(50.0, 50.0, 0.0, -40.0);
    doc.sync();
    assert_eq!(dom.take_scrolled(), vec![l]);
}

#[test]
fn images() {
    let (_doc, dom) = page(&format!("<body><img id=ok src='{PNG}'><img id=bad src='/nothing-here.png'><img id=none></body>"));
    let (ok, bad, none) = (by_id(&dom, "ok"), by_id(&dom, "bad"), by_id(&dom, "none"));
    let mut loaded = dom.take_loaded();
    loaded.sort();
    let mut expected = vec![ok, bad];
    expected.sort();
    assert_eq!(loaded, expected);
    assert!(dom.take_loaded().is_empty(), "reported once");
    assert_eq!(dom.image_state(ok).unwrap(), [2.0, 3.0, 1.0]);
    assert_eq!(dom.image_state(bad).unwrap(), [0.0, 0.0, 2.0]);
    assert_eq!(dom.image_state(none).unwrap(), [0.0, 0.0, 2.0]);

    let img = dom.create_element(Ns::Html, "img").unwrap();
    dom.set_attribute(img, "src", PNG).unwrap();
    assert_eq!(dom.take_loaded(), vec![img], "an image out of the document loads too, as new Image() does");
    assert_eq!(dom.image_state(img).unwrap(), [2.0, 3.0, 1.0]);
    dom.insert_before(dom.query_selector(dom.document(), "body").unwrap(), img, NONE).unwrap();
    assert!(dom.take_loaded().is_empty(), "inserting it does not load it again");
    assert_eq!(dom.image_state(img).unwrap(), [2.0, 3.0, 1.0]);
    let missing = dom.create_element(Ns::Html, "img").unwrap();
    dom.set_attribute(missing, "src", "/nothing-here.png").unwrap();
    assert_eq!(dom.take_loaded(), vec![missing]);
    assert_eq!(dom.image_state(missing).unwrap(), [0.0, 0.0, 2.0], "and fails out of the document too");
    dom.set_attribute(none, "src", PNG).unwrap();
    assert_eq!(dom.take_loaded(), vec![none]);
}

#[test]
fn canvas_and_capture() {
    let (_doc, dom) =
        page("<body style='margin:0'><canvas id=c width=20 height=10></canvas><div id=d style='width:30px;height:30px;background:blue'></div></body>");
    let c = by_id(&dom, "c");
    let mut ops = vec![4u8, 0];
    [1.0f32, 0.0, 0.0, 1.0].iter().for_each(|v| ops.extend(v.to_le_bytes()));
    ops.extend([43u8, 0]);
    [0.0f32, 0.0, 5.0, 5.0].iter().for_each(|v| ops.extend(v.to_le_bytes()));
    dom.canvas(c, &ops).unwrap();
    let pixels = dom.get_image_data(c, 0.0, 0.0, 20.0, 10.0).unwrap();
    assert_eq!(pixels.len(), 20 * 10 * 4);
    assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    assert_eq!(&pixels[(10 * 4)..(10 * 4 + 4)], &[0, 0, 0, 0]);
    assert!(dom.to_data_url(c, "image/png").unwrap().starts_with("data:image/png;base64,"));
    assert!(dom.measure_text(c, "10px sans-serif", "hello").unwrap().width > 10.0);
    assert_eq!(error_name(dom.canvas(by_id(&dom, "d"), &ops)), "InvalidStateError");
    let shot = dom.capture(by_id(&dom, "d"), 1.0).unwrap().unwrap();
    assert_eq!((shot.pixel_width, shot.pixel_height), (30, 30));
    assert_eq!(shot.pixel(15, 15), [0, 0, 255, 255]);
}

#[test]
fn lifetime() {
    let (_doc, dom) = page("<body><div id=list><p><b>1</b></p><p>2</p></div></body>");
    let list = by_id(&dom, "list");
    let ps = dom.children(list).unwrap();
    let b = dom.first_child(ps[0]).unwrap();
    let live = || dom.stats().live_ids;

    dom.pin(list).unwrap();
    dom.unpin(list).unwrap();
    assert_eq!(dom.local_name(list).unwrap(), "div");

    let before = live();
    dom.remove(ps[1]).unwrap();
    assert_eq!(error_name(dom.node_type(ps[1])), "NotFoundError");
    assert!(live() < before);

    dom.pin(b).unwrap();
    dom.remove(ps[0]).unwrap();
    assert_eq!(dom.local_name(ps[0]).unwrap(), "p", "kept: a node in its tree is pinned");
    assert_eq!(dom.pinned_in(ps[0]).unwrap(), vec![b]);
    assert_eq!(dom.pinned_in(list).unwrap(), Vec::<u32>::new());
    dom.unpin(b).unwrap();
    assert!(dom.node_type(b).is_ok(), "freed at commit, not at unpin");
    dom.commit();
    assert!(dom.node_type(b).is_err());
    assert!(dom.node_type(ps[0]).is_err());

    let lost = dom.create_element(Ns::Html, "div").unwrap();
    let kept = dom.create_element(Ns::Html, "div").unwrap();
    let used = dom.create_text("used").unwrap();
    dom.pin(kept).unwrap();
    dom.insert_before(list, used, NONE).unwrap();
    dom.commit();
    assert!(dom.node_type(lost).is_err());
    assert!(dom.node_type(kept).is_ok());
    assert!(dom.node_type(used).is_ok());

    let template = dom.create_element(Ns::Html, "template").unwrap();
    let content = dom.template_content(template).unwrap();
    let inner = dom.create_element(Ns::Html, "i").unwrap();
    dom.insert_before(content, inner, NONE).unwrap();
    dom.pin(content).unwrap();
    dom.commit();
    assert!(dom.node_type(template).is_ok());
    assert!(dom.node_type(inner).is_ok());
    dom.unpin(content).unwrap();
    dom.commit();
    assert!(dom.node_type(template).is_err());
    assert!(dom.node_type(inner).is_err());

    let next = dom.create_element(Ns::Html, "div").unwrap();
    assert!(next > lost && next > inner);
    dom.pin(kept).unwrap();
    dom.parse_document("<body><p id=n>new</p></body>").unwrap();
    assert!(dom.node_type(list).is_err());
    assert!(dom.node_type(kept).is_ok());
    assert_eq!(dom.text_content(by_id(&dom, "n")).unwrap(), "new");
}

#[test]
fn pins_are_flags() {
    let (_doc, dom) = page("<body><div id=list><p><b>1</b></p></div></body>");
    let p = dom.first_child(by_id(&dom, "list")).unwrap();
    let b = dom.first_child(p).unwrap();
    dom.pin(b).unwrap();
    dom.remove(p).unwrap();
    dom.pin(b).unwrap();
    dom.commit();
    assert_eq!(dom.local_name(b).unwrap(), "b", "pinned: its detached tree lives");
    dom.unpin(b).unwrap();
    dom.commit();
    assert_eq!(error_name(dom.node_type(b)), "NotFoundError", "no wrapper left: the detached tree is freed");
    assert_eq!(error_name(dom.node_type(p)), "NotFoundError");
}

#[test]
fn commit_gate() {
    let doc = Document::new(400, 300, 1.0, &Options::default()).unwrap();
    let dom = doc.dom();
    let handle = doc.handle();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    handle.set_waker(Box::new(move || _ = counter.fetch_add(1, Ordering::SeqCst)));

    dom.parse_document(
        "<head><style>body{margin:0} #m{position:absolute;left:0;top:0;width:10px;height:10px;background:red;\
         transition:left 1000ms linear} #m.go{left:100px}</style></head><body><div id=m></div></body>",
    )
    .unwrap();
    handle.tick(16.0);
    handle.invalidate();
    doc.sync();
    assert!(handle.frame().is_none(), "uncommitted document painted");
    assert_eq!(wakes.load(Ordering::SeqCst), 0);

    dom.commit();
    let first = handle.wait_frame();
    assert_ne!(first.commit, 0);
    assert!(first.plane.damage.width > 0.0);

    let m = by_id(&dom, "m");
    dom.set_attribute(m, "class", "go").unwrap();
    for t in 2..10 {
        handle.tick(f64::from(t) * 16.0);
    }
    handle.invalidate();
    doc.sync();
    assert!(handle.frame().is_none(), "half-done task painted");
    dom.commit();
    let frame = handle.wait_frame();
    assert!(frame.commit > first.commit);
    assert!(frame.animating);
    let mut frames = 0;
    for t in 10..30 {
        handle.tick(f64::from(t) * 16.0);
        doc.sync();
        if handle.frame().is_some() {
            frames += 1;
        }
    }
    assert!(frames >= 15, "{frames} frames while animating");
    let seq = doc.handle().commit();
    dom.commit();
    doc.sync();
    _ = handle.frame();
    assert_eq!(doc.handle().commit(), seq + 1, "Dom::commit without changes takes no number");
    dom.set_attribute(m, "class", "").unwrap();
    dom.commit();
    let frame = handle.wait_frame_after(seq + 1);
    assert_eq!(frame.commit, seq + 2);
}

#[test]
fn js_mutates_while_render_animates() {
    let doc = Document::new(400, 300, 1.0, &Options::default()).unwrap();
    let dom = doc.dom();
    dom.parse_document(
        "<head><style>@keyframes spin { from { transform: rotate(0deg) } to { transform: rotate(360deg) } }
         #s { width: 50px; height: 50px; background: blue; animation: spin 500ms linear infinite }
         .row { height: 4px; background: green }</style></head>
         <body><div id=s></div><div id=list></div></body>",
    )
    .unwrap();
    dom.commit();
    let handle = doc.handle();
    let done = Arc::new(AtomicBool::new(false));
    let compositor = {
        let (handle, done) = (handle.clone(), done.clone());
        std::thread::spawn(move || {
            let mut frames = 0;
            let mut t = 0.0;
            while !done.load(Ordering::SeqCst) {
                t += 8.0;
                handle.tick(t);
                if let Some(frame) = handle.frame() {
                    assert!(frame.animating);
                    assert_ne!(frame.commit, 0);
                    frames += 1;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            frames
        })
    };
    let list = by_id(&dom, "list");
    let start = Instant::now();
    let mut rows = Vec::new();
    for i in 0..400 {
        let row = dom.create_element(Ns::Html, "div").unwrap();
        dom.set_attribute(row, "class", "row").unwrap();
        dom.insert_before(list, row, NONE).unwrap();
        rows.push(row);
        if i % 3 == 0 && rows.len() > 1 {
            let gone = rows.remove(0);
            dom.remove(gone).unwrap();
        }
        let r = dom.rect(row).unwrap();
        assert_eq!(r[3], 4.0);
        if i % 10 == 0 {
            dom.commit();
        }
    }
    dom.commit();
    std::thread::sleep(Duration::from_millis(150));
    done.store(true, Ordering::SeqCst);
    let frames = compositor.join().unwrap();
    assert!(frames > 20, "{frames} frames in {:?}", start.elapsed());
    assert_eq!(dom.child_count(list).unwrap() as usize, rows.len());
    assert_eq!(dom.stats().panics, 0);
}

#[test]
#[ignore]
fn call_cost() {
    let (_doc, dom) = page("<body><div id=a><p>x</p><p>y</p><p>z</p></div></body>");
    let a = by_id(&dom, "a");
    let p = dom.first_child(a).unwrap();
    let n = 200_000;
    let time = |name: &str, f: &dyn Fn()| {
        let start = Instant::now();
        for _ in 0..n {
            f();
        }
        println!("{name}: {:.0} ns/call", start.elapsed().as_nanos() as f64 / f64::from(n));
    };
    time("parent", &|| _ = dom.parent(p).unwrap());
    time("next_sibling", &|| _ = dom.next_sibling(p).unwrap());
    time("node_type", &|| _ = dom.node_type(p).unwrap());
    time("get_attribute", &|| _ = dom.get_attribute(a, "id").unwrap());
    time("set_attribute", &|| _ = dom.set_attribute(a, "data-x", "1").unwrap());
    time("rect (layout clean)", &|| _ = dom.rect(a).unwrap());
    let t = dom.first_child(p).unwrap();
    time("set_data", &|| dom.set_data(t, "x").unwrap());
}

#[test]
fn describe_is_type_namespace_connectedness_and_name() {
    let (_doc, dom) = page("<!doctype html><html><body><p id=p>x</p><svg id=s><linearGradient/></svg></body></html>");
    let p = by_id(&dom, "p");
    assert_eq!(dom.describe(p).unwrap(), (NodeType::Element, Ns::Html, true, "p".to_owned()));
    let gradient = dom.first_child(by_id(&dom, "s")).unwrap();
    assert_eq!(dom.describe(gradient).unwrap(), (NodeType::Element, Ns::Svg, true, "linearGradient".to_owned()));
    let text = dom.first_child(p).unwrap();
    assert_eq!(dom.describe(text).unwrap(), (NodeType::Text, Ns::Html, true, String::new()));
    let fragment = dom.create_fragment().unwrap();
    assert_eq!(dom.describe(fragment).unwrap(), (NodeType::DocumentFragment, Ns::Html, false, String::new()));
    dom.commit();
    assert!(dom.node_type(fragment).is_ok());
}

#[test]
fn parse_inert_is_a_whole_detached_page() {
    let (_doc, dom) = page("<!doctype html><html><body></body></html>");
    let root = dom
        .parse_inert("<!doctype html><html lang=fr><head><title>T</title></head><body><p>a</p><!-- </body> --><p>b</p></body></html>", "about:blank")
        .unwrap();
    assert_eq!(dom.node_type(root).unwrap(), NodeType::Document);
    assert!(!dom.is_connected(root).unwrap());
    let kids = dom.child_nodes(root).unwrap();
    assert_eq!(kids.len(), 2);
    assert_eq!(dom.node_type(kids[0]).unwrap(), NodeType::DocumentType);
    let html = kids[1];
    assert_eq!(dom.get_attribute(html, "lang").unwrap().as_deref(), Some("fr"));
    let parts = dom.children(html).unwrap();
    assert_eq!(dom.inner_html(parts[0]).unwrap(), "<title>T</title>");
    assert_eq!(dom.inner_html(parts[1]).unwrap(), "<p>a</p><!-- </body> --><p>b</p>");
    assert_eq!(dom.query_selector_all(dom.document(), "p").unwrap().len(), 0);
}

#[test]
fn parse_color_knows_every_css_colour() {
    let (_doc, dom) = page("<!doctype html><html><body></body></html>");
    let rgba = |text: &str| dom.parse_color(text).map(|c| c.map(|v| (v * 255.0).round() as u8));
    assert_eq!(rgba("darkblue"), Some([0, 0, 139, 255]));
    assert_eq!(rgba("RebeccaPurple"), Some([102, 51, 153, 255]));
    assert_eq!(rgba("#0f08"), Some([0, 255, 0, 136]));
    assert_eq!(rgba("hsl(120 100% 25% / 50%)"), Some([0, 128, 0, 128]));
    assert_eq!(rgba("oklch(0.628 0.2577 29.23)"), Some([255, 0, 0, 255]));
    assert_eq!(rgba("transparent"), Some([0, 0, 0, 0]));
    assert_eq!(rgba("currentcolor"), Some([0, 0, 0, 255]));
    assert_eq!(rgba("not-a-colour"), None);
    assert_eq!(rgba("red blue"), None);
}

#[test]
fn style_sheet_rules_style_the_page() {
    let (_doc, dom) = page("<!doctype html><html><head><style id=s></style></head><body><p id=p>x</p></body></html>");
    let style = by_id(&dom, "s");
    let p = by_id(&dom, "p");
    assert_eq!(dom.sheet_rule_count(style).unwrap(), Some(0));
    assert_eq!(dom.sheet_insert_rule(style, "p { color: rgb(0, 128, 0) }", 0).unwrap(), 0);
    assert_eq!(dom.sheet_insert_rule(style, ".x { color: blue }", 1).unwrap(), 1);
    assert_eq!(dom.sheet_rule_count(style).unwrap(), Some(2));
    assert_eq!(dom.sheet_rule(style, 0).unwrap(), "p { color: rgb(0, 128, 0); }");
    assert_eq!(dom.computed_style(p, "", "color").unwrap(), "rgb(0, 128, 0)");
    assert_eq!(dom.text_content(style).unwrap(), "", "the element's text stays as it was");
    let name = |e: acde::render::Error| e.0.split(':').next().unwrap().to_owned();
    assert_eq!(dom.sheet_insert_rule(style, "p", 0).map_err(name), Err("SyntaxError".to_owned()));
    assert_eq!(dom.sheet_insert_rule(style, "p {}", 9).map_err(name), Err("IndexSizeError".to_owned()));
    dom.sheet_delete_rule(style, 0).unwrap();
    assert_eq!(dom.computed_style(p, "", "color").unwrap(), "rgb(0, 0, 0)");
    let detached = dom.create_element(Ns::Html, "style").unwrap();
    assert_eq!(dom.sheet_rule_count(detached).unwrap(), None);
}

#[test]
fn a_scroll_box_whose_content_shrinks_is_scrolled_back_into_range() {
    let (_doc, dom) = page(
        "<body style='margin:0'><div id=box style='height:100px;overflow:auto'><div id=content style='height:400px'></div></div>\
         <div id=tall style='height:2000px'></div></body>",
    );
    let (b, content, tall) = (by_id(&dom, "box"), by_id(&dom, "content"), by_id(&dom, "tall"));
    dom.scroll_to(b, 0.0, 250.0, false).unwrap();
    dom.scroll_to(dom.document(), 0.0, 1500.0, false).unwrap();
    dom.take_scrolled();
    dom.set_attribute(content, "style", "height:150px").unwrap();
    dom.set_attribute(tall, "style", "height:100px").unwrap();
    assert_eq!(dom.metrics(b).unwrap().unwrap().scroll_top, 50.0, "clamped to the new end");
    let html = dom.children(dom.document()).unwrap()[0];
    assert_eq!(dom.metrics(html).unwrap().unwrap().scroll_top, 0.0, "the viewport too");
    let mut scrolled = dom.take_scrolled();
    scrolled.sort();
    let mut expected = vec![b, dom.document()];
    expected.sort();
    assert_eq!(scrolled, expected, "reported as scrolls");
    dom.set_attribute(content, "style", "height:50px").unwrap();
    assert_eq!(dom.metrics(b).unwrap().unwrap().scroll_top, 0.0);
}

#[test]
fn named_sections_are_regions_and_outputs_are_status() {
    let (_doc, dom) = page("<body><section aria-label='Notes'><p>a</p></section><section><p>b</p></section><output>28</output></body>");
    let tree = dom.semantics(NONE, 100, true, false).unwrap();
    assert!(tree.contains("\"role\":\"region\",\"name\":\"Notes\""), "{tree}");
    assert_eq!(tree.matches("\"region\"").count(), 1, "{tree}");
    assert!(tree.contains("\"role\":\"status\""), "{tree}");
}

#[test]
fn range_inputs_sanitize_their_value_and_paint_a_thumb() {
    let (_doc, dom) = page(
        "<body><input type=range id=a><input type=range id=b min=1.2 max=2.4 step=0.1 value=1.8><input type=range id=c min=0 max=10 step=3 value=10>\
         <input type=range id=d min=5 max=1 value=7><input type=range id=e step=any value=33.3333></body>",
    );
    let value = |id: &str| dom.value(by_id(&dom, id)).unwrap();
    assert_eq!(value("a"), "50");
    assert_eq!(value("b"), "1.8");
    assert_eq!(value("c"), "9", "the largest step value not above max");
    assert_eq!(value("d"), "5", "max below min becomes min");
    assert_eq!(value("e"), "33.3333");
    let b = by_id(&dom, "b");
    dom.set_value(b, "9").unwrap();
    assert_eq!(value("b"), "2.4");
    dom.set_value(b, "1.86").unwrap();
    assert_eq!(value("b"), "1.9");
    dom.set_value(b, "nonsense").unwrap();
    assert_eq!(value("b"), "1.8");
    dom.set_attribute(b, "max", "1.5").unwrap();
    assert_eq!(value("b"), "1.5");
    let r = dom.rect(by_id(&dom, "a")).unwrap();
    assert_eq!((r[2], r[3]), (129.0, 16.0));
}

fn animation_events(dom: &Dom) -> Vec<String> {
    dom.take_animation_events().events.into_iter().map(|e| format!("{} {} {} {}{}", e.kind, dom.id(e.target).unwrap(), e.name, e.elapsed, e.pseudo)).collect()
}

#[test]
fn css_animation_and_transition_events() {
    let doc = Document::new(400, 300, 1.0, &Options::default()).unwrap();
    let dom = doc.dom();
    let handle = doc.handle();
    dom.parse_document(
        "<head><style>@keyframes fade{to{opacity:0}} .out{animation:fade 100ms 2 forwards} \
         #t{transition:left 200ms 50ms linear;position:absolute;left:0} #t.go{left:50px} \
         #p::before{content:'';animation:fade 300ms}</style></head>\
         <body><div id=a></div><div id=t></div><div id=p></div></body>",
    )
    .unwrap();
    handle.tick(0.0);
    assert!(dom.take_animation_events().events.is_empty());
    let (a, t) = (by_id(&dom, "a"), by_id(&dom, "t"));
    dom.set_attribute(a, "class", "out").unwrap();
    dom.set_attribute(t, "class", "go").unwrap();
    let mut seen = Vec::new();
    for ms in (16..=400).step_by(16) {
        handle.tick(f64::from(ms));
        seen.extend(animation_events(&dom));
    }
    assert_eq!(
        seen,
        [
            "animationstart p fade 0::before",
            "transitionrun t left 0",
            "animationstart a fade 0",
            "transitionstart t left 0",
            "animationiteration a fade 0.1",
            "animationend a fade 0.2",
            "transitionend t left 0.2",
            "animationend p fade 0.3::before",
        ]
    );
    assert!(!dom.take_animation_events().animating);

    dom.set_attribute(a, "class", "").unwrap();
    handle.tick(500.0);
    assert!(animation_events(&dom).is_empty(), "an animation that ended is not canceled");

    dom.set_attribute(a, "class", "out").unwrap();
    handle.tick(516.0);
    assert!(animation_events(&dom).is_empty(), "made in this style pass, started by the next");
    handle.tick(532.0);
    assert_eq!(animation_events(&dom), ["animationstart a fade 0"]);
    handle.tick(566.0);
    assert!(dom.take_animation_events().animating);
    dom.set_attribute(a, "class", "").unwrap();
    dom.set_attribute(t, "class", "").unwrap();
    handle.tick(582.0);
    let events = animation_events(&dom);
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(events[0].starts_with("animationcancel a fade 0.066"), "{events:?}");
    assert_eq!(events[1], "transitionrun t left 0");

    dom.pin(t).unwrap();
    dom.remove(t).unwrap();
    handle.tick(600.0);
    let events = animation_events(&dom);
    assert_eq!(events, ["transitioncancel t left 0"], "canceled in its delay");
}

#[test]
fn textarea_scroll_height() {
    let (_doc, dom) = page(
        "<head><style>textarea { font: 16px/20px sans-serif; padding: 8px; border: 1px solid; box-sizing: border-box; \
         width: 300px; display: block }</style></head><body><textarea id=a rows=1></textarea></body>",
    );
    let a = by_id(&dom, "a");
    let lines = "one\ntwo\nthree\nfour";
    dom.set_value(a, lines).unwrap();
    let m = dom.metrics(a).unwrap().unwrap();
    assert_eq!((m.client_height, m.scroll_height), (36.0, 96.0), "scrollHeight covers the four lines and the padding");
}

#[test]
fn has_selector_follows_state_attributes_and_tree() {
    let (_doc, dom) = page(
        "<head><style>.card:has(input:checked) { color: rgb(0, 128, 0) } main:has(.deep) { color: rgb(0, 0, 255) } \
         .y:has(+ .z) { color: rgb(255, 0, 0) }</style></head>\
         <body><label class=card id=card><input type=checkbox id=cb></label>\
         <main id=main><div><div><b id=b>x</b></div></div></main><p class=y id=y></p></body>",
    );
    let color = |id: &str| dom.computed_style(by_id(&dom, id), "", "color").unwrap();
    let black = "rgb(0, 0, 0)";
    assert!(dom.matches(by_id(&dom, "card"), ":has(input)").unwrap());
    assert_eq!(color("card"), black);
    dom.set_checked(by_id(&dom, "cb"), true).unwrap();
    assert_eq!(color("card"), "rgb(0, 128, 0)", "a descendant's state");
    dom.set_checked(by_id(&dom, "cb"), false).unwrap();
    assert_eq!(color("card"), black);

    dom.set_attribute(by_id(&dom, "b"), "class", "deep").unwrap();
    assert_eq!(color("main"), "rgb(0, 0, 255)", "a class three levels down");
    dom.remove_attribute(by_id(&dom, "b"), "class").unwrap();
    assert_eq!(color("main"), black);
    let deep = dom.create_element(Ns::Html, "i").unwrap();
    dom.set_attribute(deep, "class", "deep").unwrap();
    let inner = dom.first_child(dom.first_child(by_id(&dom, "main")).unwrap()).unwrap();
    dom.insert_before(inner, deep, NONE).unwrap();
    assert_eq!(color("main"), "rgb(0, 0, 255)", "an insertion");
    dom.remove(deep).unwrap();
    assert_eq!(color("main"), black, "a removal");

    let z = dom.create_element(Ns::Html, "p").unwrap();
    dom.set_attribute(z, "class", "z").unwrap();
    dom.insert_before(dom.query_selector(dom.document(), "body").unwrap(), z, NONE).unwrap();
    assert_eq!(color("y"), "rgb(255, 0, 0)", "a next sibling");
    dom.remove(z).unwrap();
    assert_eq!(color("y"), black);
}

#[test]
fn svg_children_have_client_rects() {
    let (_doc, dom) = page(
        "<body style='margin:0'><svg id=s width=200 height=100 viewBox='0 0 100 50' style='padding:5px'>\
         <rect id=r x=10 y=10 width=40 height=20 /><g transform='translate(50,0)'><circle id=c cx=10 cy=10 r=5 /></g>\
         <text id=t x=10 y=45 font-size=10>Hello</text></svg></body>",
    );
    let rect = |id| dom.rect(by_id(&dom, id)).unwrap().map(|v| (v * 1000.0).round() / 1000.0);
    let r = rect("r");
    assert_eq!(r, [25.0, 25.0, 80.0, 40.0], "viewBox scale 2, padding 5");
    let c = rect("c");
    assert_eq!(c, [115.0, 15.0, 20.0, 20.0]);
    let t = rect("t");
    assert!(t[0] > 20.0 && t[2] > 30.0 && t[3] > 10.0, "text drawn with a box: {t:?}");
    assert_eq!(dom.computed_style(by_id(&dom, "t"), "", "font-size").unwrap(), "10px", "font-size=10 is in user units");
}

#[test]
fn popover_and_modal_states() {
    let (_doc, dom) = page("<body><div id=p popover>pop</div><dialog id=d open>dialog</dialog><details id=s open><summary>s</summary></details></body>");
    let (p, d) = (by_id(&dom, "p"), by_id(&dom, "d"));
    assert_eq!(dom.computed_style(p, "", "display").unwrap(), "none");
    dom.set_open_state(p, false, true).unwrap();
    assert!(dom.matches(p, ":popover-open").unwrap());
    assert_eq!(dom.computed_style(p, "", "display").unwrap(), "block");
    assert_eq!(dom.computed_style(p, "", "position").unwrap(), "fixed");
    dom.set_open_state(p, false, false).unwrap();
    assert_eq!(dom.computed_style(p, "", "display").unwrap(), "none");
    assert!(!dom.matches(d, ":modal").unwrap());
    dom.set_open_state(d, true, true).unwrap();
    assert!(dom.matches(d, ":modal").unwrap());
    assert_eq!(dom.computed_style(d, "", "position").unwrap(), "fixed");
    assert!(dom.matches(d, ":open").unwrap() && dom.matches(by_id(&dom, "s"), ":open").unwrap());
}
