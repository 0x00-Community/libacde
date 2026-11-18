use acde::render::{Document, Dom, NodeType, Ns, Options};

fn page() -> (Document, Dom) {
    let doc = Document::new(800, 600, 1.0, &Options::default()).unwrap();
    let dom = doc.dom();
    dom.parse_document("<!doctype html><html><body></body></html>").unwrap();
    (doc, dom)
}

fn root(dom: &Dom, doc: u32) -> u32 {
    *dom.children(doc).unwrap().first().expect("a document element")
}

fn error_of(dom: &Dom, text: &str) -> String {
    let doc = dom.parse_xml(text, "application/xml", "http://acde.local/").unwrap();
    let r = root(dom, doc);
    assert_eq!(dom.local_name(r).unwrap(), "parsererror", "{text} should not parse");
    assert_eq!(dom.namespace_uri(r).unwrap(), "http://www.mozilla.org/newlayout/xml/parsererror.xml");
    dom.text_content(r).unwrap()
}

#[test]
fn namespaces_prefixes_and_attributes() {
    let (_d, dom) = page();
    let doc = dom
        .parse_xml(
            r##"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0" xml:lang="zh">
  <metadata><dc:title id="t">书名</dc:title><meta refines="#t" property="title-type">main</meta></metadata>
</package>"##,
            "application/xml",
            "http://acde.local/",
        )
        .unwrap();
    assert_eq!(dom.node_type(doc).unwrap(), NodeType::Document);
    assert_eq!(dom.document_info(doc).unwrap(), ["document", "application/xml", "http://acde.local/"]);
    let package = root(&dom, doc);
    assert_eq!(dom.namespace(package).unwrap(), Ns::Other);
    assert_eq!(dom.namespace_uri(package).unwrap(), "http://www.idpf.org/2007/opf");
    assert_eq!(dom.prefix(package).unwrap(), "");
    assert_eq!(dom.get_attribute_ns(package, "http://www.w3.org/XML/1998/namespace", "lang").unwrap().as_deref(), Some("zh"));
    assert_eq!(dom.get_attribute(package, "xml:lang").unwrap().as_deref(), Some("zh"));
    let titles = dom.get_elements_by_tag_name_ns(doc, "http://purl.org/dc/elements/1.1/", "title").unwrap();
    assert_eq!(titles.len(), 1);
    assert_eq!(dom.prefix(titles[0]).unwrap(), "dc");
    assert_eq!(dom.text_content(titles[0]).unwrap(), "书名");
    assert_eq!(dom.get_elements_by_tag_name(doc, "dc:title").unwrap(), titles);
    assert_eq!(dom.owner_document(titles[0]).unwrap(), doc);
    let attrs = dom.attributes_ns(package).unwrap();
    assert!(attrs.chunks(4).any(|a| a == ["http://www.w3.org/2000/xmlns/", "xmlns", "dc", "http://purl.org/dc/elements/1.1/"]));
    assert!(attrs.chunks(4).any(|a| a == ["http://www.w3.org/2000/xmlns/", "", "xmlns", "http://www.idpf.org/2007/opf"]));
}

#[test]
fn xhtml_elements_are_html_elements() {
    let (_d, dom) = page();
    let doc = dom
        .parse_xml(
            r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>T</title></head>
<body><p epub:type="footnote" CLASS="a">x&nbsp;&#x4e2d;&amp;<![CDATA[<raw>]]><?pi data?></p></body></html>"#,
            "application/xhtml+xml",
            "http://acde.local/",
        )
        .unwrap();
    let kids = dom.child_nodes(doc).unwrap();
    assert_eq!(dom.node_type(kids[0]).unwrap(), NodeType::DocumentType);
    assert_eq!(dom.document_type(kids[0]).unwrap().1, "-//W3C//DTD XHTML 1.1//EN");
    let p = dom.get_elements_by_tag_name(doc, "p").unwrap()[0];
    assert_eq!(dom.namespace(p).unwrap(), Ns::Html);
    assert_eq!(dom.get_attribute_ns(p, "http://www.idpf.org/2007/ops", "type").unwrap().as_deref(), Some("footnote"));
    assert_eq!(dom.get_attribute(p, "class").unwrap(), None, "attribute names keep their case in XML documents");
    assert_eq!(dom.get_attribute(p, "CLASS").unwrap().as_deref(), Some("a"));
    assert!(dom.get_elements_by_tag_name(doc, "P").unwrap().is_empty());
    let parts = dom.child_nodes(p).unwrap();
    let types: Vec<NodeType> = parts.iter().map(|&n| dom.node_type(n).unwrap()).collect();
    assert_eq!(types, [NodeType::Text, NodeType::CDataSection, NodeType::ProcessingInstruction]);
    assert_eq!(dom.data(parts[0]).unwrap(), "x\u{a0}中&");
    assert_eq!(dom.data(parts[1]).unwrap(), "<raw>");
    assert_eq!(dom.processing_instruction_target(parts[2]).unwrap(), "pi");
    assert_eq!(dom.data(parts[2]).unwrap(), "data");
    assert_eq!(
        dom.outer_html(p).unwrap(),
        r#"<p xmlns="http://www.w3.org/1999/xhtml" xmlns:ns1="http://www.idpf.org/2007/ops" ns1:type="footnote" CLASS="a">x&#160;中&amp;<![CDATA[<raw>]]><?pi data?></p>"#
            .replace("&#160;", "\u{a0}")
    );
}

#[test]
fn entities_from_the_internal_subset() {
    let (_d, dom) = page();
    let doc = dom
        .parse_xml(
            r#"<!DOCTYPE r [<!ENTITY who "World"><!ENTITY greet "Hello, &who;!"><!ENTITY b "<b>bold</b>">]><r a="&greet;">&greet; &b;</r>"#,
            "text/xml",
            "http://acde.local/",
        )
        .unwrap();
    let r = root(&dom, doc);
    assert_eq!(dom.get_attribute(r, "a").unwrap().as_deref(), Some("Hello, World!"));
    assert_eq!(dom.inner_html(r).unwrap(), "Hello, World! <b>bold</b>");
    assert!(error_of(&dom, r#"<!DOCTYPE r [<!ENTITY a "&b;"><!ENTITY b "&a;">]><r>&a;</r>"#).contains("refers to itself"));
    assert!(error_of(&dom, r#"<!DOCTYPE r [<!ENTITY b "<b>">]><r a="&b;"/>"#).contains("'<'"));
    let skipped = dom.parse_xml(r#"<!DOCTYPE r SYSTEM "r.dtd"><r>a&unknown;b</r>"#, "application/xml", "http://acde.local/").unwrap();
    assert_eq!(dom.text_content(root(&dom, skipped)).unwrap(), "ab", "undeclared entities are skipped when an external subset exists");
    assert!(error_of(&dom, "<r>&nbsp;</r>").contains("'nbsp' is not defined"));
}

#[test]
fn not_well_formed_is_a_parsererror() {
    let (_d, dom) = page();
    for bad in [
        "",
        "<a><b></a>",
        "<a></a><b/>",
        "<a x='1' x='2'/>",
        "<a xmlns:p='u' xmlns:q='u' p:x='1' q:x='2'/>",
        "<p:a/>",
        "<a>]]></a>",
        "<a><!-- -- --></a>",
        "text<a/>",
        "<a/>text",
        "<a b=c/>",
        "<a>&#0;</a>",
        "<a xmlns:p=''/>",
        "<?xml version='1.0'?><?xml version='1.0'?><a/>",
        "<a><?xml x?></a>",
        "<a:b:c/>",
        "<a>\u{1}</a>",
    ] {
        let message = error_of(&dom, bad);
        assert!(message.starts_with("XML Parsing Error: "), "{bad}: {message}");
        assert!(message.contains("Line Number 1"), "{bad}: {message}");
    }
    let doc = dom.parse_xml("<a>\n\n  <b>\n</a>", "application/xml", "http://acde.local/").unwrap();
    assert!(dom.text_content(root(&dom, doc)).unwrap().contains("Line Number 4, Column 1"));
}

#[test]
fn xml_documents_serialize_as_xml() {
    let (_d, dom) = page();
    let doc = dom.parse_xml("<r xmlns='urn:a'><e/><x:e xmlns:x='urn:b' x:at='1&#10;'/></r>", "application/xml", "http://acde.local/").unwrap();
    assert_eq!(dom.serialize_xml(doc).unwrap(), r#"<r xmlns="urn:a"><e/><x:e xmlns:x="urn:b" x:at="1&#xA;"/></r>"#);
    let r = root(&dom, doc);
    assert_eq!(dom.inner_html(r).unwrap(), r#"<e xmlns="urn:a"/><x:e xmlns:x="urn:b" x:at="1&#xA;"/>"#);
    let fragment = dom.parse_fragment(r, "<f>text<x:g xmlns:x='urn:c'/></f>").unwrap();
    let f = dom.children(fragment).unwrap()[0];
    assert_eq!(dom.namespace_uri(f).unwrap(), "urn:a", "fragments inherit the context's default namespace");
    assert_eq!(dom.owner_document(f).unwrap(), doc);
    assert_eq!(error_name(dom.parse_fragment(r, "<unclosed>")), "SyntaxError");
}

#[test]
fn insertion_adopts_into_the_parents_document() {
    let (_d, dom) = page();
    let doc = dom.parse_xml("<r/>", "application/xml", "http://acde.local/").unwrap();
    let live = dom.document();
    let el = dom.create_element(Ns::Html, "div").unwrap();
    assert_eq!(dom.owner_document(el).unwrap(), live);
    dom.insert_before(root(&dom, doc), el, acde::render::NONE).unwrap();
    assert_eq!(dom.owner_document(el).unwrap(), doc);
    assert!(!dom.in_html_document(el).unwrap());
    let body = dom.get_elements_by_tag_name(live, "body").unwrap()[0];
    dom.insert_before(body, el, acde::render::NONE).unwrap();
    assert_eq!(dom.owner_document(el).unwrap(), live);
    assert!(dom.in_html_document(el).unwrap());
}

fn error_name<T: std::fmt::Debug>(result: acde::render::Result<T>) -> String {
    result.unwrap_err().0.split(':').next().unwrap().to_owned()
}

#[test]
fn selectors_match_in_xml_documents() {
    let (_d, dom) = page();
    let doc = dom.parse_xml("<Foo xmlns:x='urn:x'><bar>baz</bar><x:q/><aB><c/></aB></Foo>", "application/xml", "http://acde.local/").unwrap();
    let root = root(&dom, doc);
    assert_eq!(dom.query_selector(doc, "Foo").unwrap(), root);
    assert_eq!(dom.query_selector_all(doc, "bar").unwrap().len(), 1);
    assert_eq!(dom.query_selector_all(doc, "Foo bar").unwrap().len(), 1);
    assert_eq!(dom.query_selector_all(doc, "aB *").unwrap().len(), 1);
    assert_eq!(dom.query_selector_all(doc, "foo").unwrap().len(), 0, "type selectors are case-sensitive in XML documents");
    assert_eq!(dom.query_selector_all(doc, "q").unwrap().len(), 1);
    let copy = dom.clone(root, true).unwrap();
    assert_eq!(dom.query_selector_all(copy, "aB").unwrap().len(), 1, "clones stay in the XML document");
    let (_d2, page2) = (0, &dom);
    page2.parse_document("<body><svg><linearGradient id=g /></svg><DIV id=d></DIV></body>").unwrap();
    assert_eq!(page2.query_selector_all(page2.document(), "linearGradient").unwrap().len(), 1, "SVG names keep their case in HTML documents");
    assert_eq!(page2.query_selector_all(page2.document(), "lineargradient").unwrap().len(), 0);
    assert_eq!(page2.query_selector_all(page2.document(), "div").unwrap().len(), 1);
    assert_eq!(page2.query_selector_all(page2.document(), "DIV").unwrap().len(), 1, "HTML names are case-insensitive in HTML documents");
}
