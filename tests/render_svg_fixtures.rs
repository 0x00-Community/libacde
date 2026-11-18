use acde::render::Options;
use acde::render::blitz_dom::node::SvgImageData;

mod common;
use common::Doc;

const SIZE: u32 = 96;

struct Page {
    doc: Doc,
}

impl Page {
    fn new(source: &str, force_usvg: bool, text_path: bool) -> Option<Self> {
        let options = Options { svg_text_path: Some(text_path), ..Options::default() };
        let doc = Doc::new(SIZE, SIZE, 1.0, &options).unwrap();
        doc.add_stylesheet("html, body { margin: 0; background: #fff; } svg { display: block; width: 80px; height: 80px; margin: 8px; overflow: hidden; }")
            .unwrap();
        let body = doc.create_element("body").unwrap();
        doc.append_child(0, body).unwrap();
        let page = Page { doc };
        let xml = roxmltree::Document::parse(source).ok()?;
        let svg = page.element(xml.root_element(), body);
        if force_usvg {
            page.doc.set_attribute(svg, "filter", "none").unwrap();
        }
        Some(page)
    }

    fn element(&self, el: roxmltree::Node, parent: u32) -> u32 {
        let id = self.doc.create_element_ns(1, el.tag_name().name()).unwrap();
        for a in el.attributes() {
            if !(el.parent_element().is_none() && matches!(a.name(), "width" | "height" | "style")) && a.namespace().is_none() {
                self.doc.set_attribute(id, a.name(), a.value()).unwrap();
            }
        }
        self.doc.append_child(parent, id).unwrap();
        for child in el.children() {
            if child.is_element() {
                self.element(child, id);
            } else if let Some(text) = child.text().filter(|t| !t.trim().is_empty()) {
                let t = self.doc.create_text(text).unwrap();
                self.doc.append_child(id, t).unwrap();
            }
        }
        id
    }

    fn raster(&self) -> Vec<u8> {
        common::rasterize(self.doc.scene(), (SIZE, SIZE))
    }
}

#[test]
fn simple_path_matches_usvg_on_os_v2_fixtures() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/svg");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("no fixtures at {}", dir.display());
        return;
    };
    let mut files: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "svg")).collect();
    files.sort();
    let (mut simple, mut bad, mut unparsable, mut from_dom) = (0, Vec::new(), 0, Vec::<String>::new());
    for path in &files {
        let source = std::fs::read_to_string(path).unwrap();
        if SvgImageData::simple(&source, |s| SvgImageData::from_data(s, &Default::default())).is_some() {
            simple += 1;
        } else if std::env::var_os("SHOW_NOT_SIMPLE").is_some() {
            eprintln!("not simple: {}", path.file_name().unwrap().to_string_lossy());
        }
        let (Some(a), Some(b), Some(text)) = (Page::new(&source, false, false), Page::new(&source, true, false), Page::new(&source, false, true)) else {
            unparsable += 1;
            continue;
        };
        let (a, b) = (a.raster(), b.raster());
        if a != text.raster() {
            from_dom.push(path.file_name().unwrap().to_string_lossy().to_string());
        }
        let differing = a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 24)).count();
        if differing * 100 > (SIZE * SIZE) as usize / 2 {
            bad.push((path.file_name().unwrap().to_string_lossy().to_string(), differing));
        }
    }
    eprintln!("{} fixtures ({unparsable} not well-formed XML, skipped), {simple} simple as exported; differing (>0.5% px): {bad:?}", files.len());
    assert!(bad.is_empty(), "{bad:?}");
    assert!(from_dom.is_empty(), "built from the DOM differs from the text path: {from_dom:?}");
}
