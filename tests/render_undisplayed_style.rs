use acde::render::{Document, Options};

fn page(html: &str) -> (Document, acde::render::Dom) {
    let doc = Document::new(800, 600, 1.0, &Options::default()).unwrap();
    let dom = doc.dom();
    dom.parse_document(html).unwrap();
    (doc, dom)
}

#[test]
fn computed_style_of_a_display_none_subtree_resolves_outside_layout() {
    let (_doc, dom) = page(
        "<style>.hidden { display: none; color: rgb(255, 0, 0); } .child { width: 40px; }</style>
        <div id=hidden class=hidden><span id=child class=child>x</span></div>",
    );
    let hidden = dom.get_element_by_id("hidden");
    let child = dom.get_element_by_id("child");
    assert_eq!(dom.computed_style(hidden, "", "color").unwrap(), "rgb(255, 0, 0)");
    assert_eq!(dom.computed_style(child, "", "width").unwrap(), "40px");
    assert_eq!(dom.computed_style(hidden, "", "display").unwrap(), "none");
}
