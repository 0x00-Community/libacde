use std::sync::{Arc, Mutex};

use acde::render::{Document, Dom, Load, Loader, NONE, Ns, Options};

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAADCAYAAAC56t6BAAAAEUlEQVR4nGP4z8DwH4QZMBgAoXkL9U3EmgcAAAAASUVORK5CYII=";

type Loads = Arc<Mutex<Vec<Load>>>;

fn page(html: &str) -> (Document, Dom, Loads) {
    let loads = Loads::default();
    let sink = loads.clone();
    let options = Options { loader: Some(Loader::new(move |load| sink.lock().unwrap().push(load))), ..Options::default() };
    let doc = Document::new(800, 600, 1.0, &options).unwrap();
    let dom = doc.dom();
    dom.parse_document(html).unwrap();
    (doc, dom, loads)
}

fn by_id(dom: &Dom, id: &str) -> u32 {
    let found = dom.get_element_by_id(id);
    assert_ne!(found, NONE, "#{id}");
    found
}

fn take(loads: &Loads, url: &str) -> Load {
    let mut loads = loads.lock().unwrap();
    let i = loads.iter().position(|l| l.url == url).unwrap_or_else(|| panic!("{url} was not asked for"));
    loads.remove(i)
}

fn png() -> Vec<u8> {
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, PNG).unwrap()
}

#[test]
fn remote_images_load_through_the_loader() {
    let (_doc, dom, loads) = page("<body><img id=a src='https://img.example.com/a.png'><img id=b src='https://img.example.com/b.png'></body>");
    let (a, b) = (by_id(&dom, "a"), by_id(&dom, "b"));
    assert_eq!(dom.take_loaded(), vec![a, b], "both reported as loading, so the page keeps asking");
    assert!(dom.take_loaded().is_empty(), "loading is reported once");
    assert_eq!(dom.image_state(a).unwrap()[2], 0.0, "loading");

    let load = take(&loads, "https://img.example.com/a.png");
    assert_eq!(load.destination, "image");
    load.finish(Some(png()));
    assert_eq!(dom.take_loaded(), vec![a]);
    assert_eq!(dom.image_state(a).unwrap(), [2.0, 3.0, 1.0]);

    drop(take(&loads, "https://img.example.com/b.png"));
    assert_eq!(dom.take_loaded(), vec![b], "a load dropped unanswered fails");
    assert_eq!(dom.image_state(b).unwrap(), [0.0, 0.0, 2.0]);
}

#[test]
fn remote_stylesheets_apply_when_they_arrive_and_ask_for_their_fonts() {
    let (_doc, dom, loads) = page("<head><link rel=stylesheet href='https://cdn.example.com/css/site.css'></head><body><p id=p>x</p></body>");
    let p = by_id(&dom, "p");
    let load = take(&loads, "https://cdn.example.com/css/site.css");
    assert_eq!(load.destination, "style");
    load.finish(Some(b"@font-face { font-family: Remote; src: url(../fonts/r.woff2) } p { color: rgb(255, 0, 0) }".to_vec()));
    assert_eq!(dom.computed_style(p, "", "color").unwrap(), "rgb(255, 0, 0)");
    assert_eq!(take(&loads, "https://cdn.example.com/fonts/r.woff2").destination, "font");
}

fn icon(entries: &[(u8, u8, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * entries.len();
    for (width, height, bits, data) in entries {
        out.extend_from_slice(&[*width, *height, 0, 0, 1, 0]);
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
    }
    for (.., data) in entries {
        out.extend_from_slice(data);
    }
    out
}

fn bmp_entry(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for value in [40, width, height * 2] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for _ in 0..width * height {
        out.extend_from_slice(&[0, 0, 255, 255]);
    }
    let mask_row = (width.div_ceil(32) * 4) as usize;
    out.extend(std::iter::repeat_n(0, mask_row * height as usize));
    out
}

#[test]
fn icons_load_from_their_largest_entry_in_png_or_bmp() {
    let (_doc, dom, loads) = page("<body><img id=png src='https://site.example.com/favicon.ico'><img id=bmp src='https://site.example.com/bmp.ico'></body>");
    let (png_icon, bmp_icon) = (by_id(&dom, "png"), by_id(&dom, "bmp"));
    take(&loads, "https://site.example.com/favicon.ico").finish(Some(icon(&[(1, 1, 32, bmp_entry(1, 1)), (2, 3, 32, png())])));
    take(&loads, "https://site.example.com/bmp.ico").finish(Some(icon(&[(4, 2, 32, bmp_entry(4, 2)), (1, 1, 32, png())])));
    let mut loaded = dom.take_loaded();
    loaded.sort();
    let mut both = vec![png_icon, bmp_icon];
    both.sort();
    assert_eq!(loaded, both);
    assert_eq!(dom.image_state(png_icon).unwrap(), [2.0, 3.0, 1.0], "the 2x3 PNG entry, not the 1x1 bitmap");
    assert_eq!(dom.image_state(bmp_icon).unwrap(), [4.0, 2.0, 1.0], "the 4x2 bitmap entry, not the 1x1 PNG");
}

#[test]
fn a_cursor_file_loads_from_its_largest_entry() {
    let (_doc, dom, loads) = page("<body><img id=cur src='https://site.example.com/pointer.cur'></body>");
    let cur = by_id(&dom, "cur");
    let mut file = icon(&[(4, 2, 32, bmp_entry(4, 2)), (1, 1, 32, bmp_entry(1, 1))]);
    file[2] = 2;
    for (entry, (x, y)) in [(0, (300u16, 290u16)), (1, (0, 0))] {
        let at = 6 + 16 * entry + 4;
        file[at..at + 2].copy_from_slice(&x.to_le_bytes());
        file[at + 2..at + 4].copy_from_slice(&y.to_le_bytes());
    }
    take(&loads, "https://site.example.com/pointer.cur").finish(Some(file));
    assert_eq!(dom.take_loaded(), vec![cur]);
    assert_eq!(dom.image_state(cur).unwrap(), [4.0, 2.0, 1.0], "a hotspot over 256 does not stop the decoder");
}

#[test]
fn local_and_data_urls_never_reach_the_loader() {
    let (_doc, dom, loads) = page(&format!("<body><img id=a src='data:image/png;base64,{PNG}'><img src='file:///nowhere/x.png'></body>"));
    assert_eq!(dom.image_state(by_id(&dom, "a")).unwrap()[2], 1.0);
    assert!(loads.lock().unwrap().is_empty());
}

const SITE: &str = "https://cdn.example.com/site.css";

fn linked(body: &str) -> (Document, Dom, Loads, u32, u32) {
    let (doc, dom, loads) = page(&format!("<head><link id=l rel=stylesheet href='{SITE}'></head><body><p id=p>x</p>{body}</body>"));
    let (link, p) = (by_id(&dom, "l"), by_id(&dom, "p"));
    (doc, dom, loads, link, p)
}

fn color(dom: &Dom, id: u32) -> String {
    dom.computed_style(id, "", "color").unwrap()
}

#[test]
fn a_link_reports_its_stylesheet_once_it_is_fetched_and_applied() {
    let (_doc, dom, loads, link, p) = linked("");
    assert_eq!(dom.sheet_state(link).unwrap(), 0, "loading");
    assert_eq!(dom.sheet_rule_count(link).unwrap(), None, "no sheet yet");
    assert!(!dom.take_loaded().contains(&link));
    take(&loads, SITE).finish(Some(b"p { color: rgb(255, 0, 0) }".to_vec()));
    assert_eq!(dom.take_loaded(), vec![link]);
    assert_eq!(dom.sheet_state(link).unwrap(), 1, "loaded");
    assert_eq!(dom.sheet_rule_count(link).unwrap(), Some(1));
    assert_eq!(color(&dom, p), "rgb(255, 0, 0)");
    assert!(dom.take_loaded().is_empty(), "reported once");
    assert_eq!(dom.sheet_state(p).unwrap(), 3, "not a link");
}

#[test]
fn an_empty_stylesheet_loads_and_an_unanswered_one_fails() {
    let (_doc, dom, loads, link, p) = linked("");
    take(&loads, SITE).finish(Some(Vec::new()));
    assert_eq!(dom.take_loaded(), vec![link]);
    assert_eq!((dom.sheet_state(link).unwrap(), dom.sheet_rule_count(link).unwrap()), (1, Some(0)));

    dom.set_attribute(link, "href", "https://cdn.example.com/b.css").unwrap();
    assert_eq!(dom.sheet_state(link).unwrap(), 0);
    assert_eq!(dom.sheet_rule_count(link).unwrap(), Some(0), "the old sheet stays until the new answer");
    drop(take(&loads, "https://cdn.example.com/b.css"));
    assert_eq!(dom.take_loaded(), vec![link]);
    assert_eq!((dom.sheet_state(link).unwrap(), dom.sheet_rule_count(link).unwrap()), (2, None), "failed, and the old sheet is gone");
    assert_eq!(color(&dom, p), "rgb(0, 0, 0)");
}

#[test]
fn only_the_current_fetch_of_a_link_counts() {
    let (_doc, dom, loads, link, p) = linked("");
    let first = take(&loads, SITE);
    dom.set_attribute(link, "href", "https://cdn.example.com/b.css").unwrap();
    take(&loads, "https://cdn.example.com/b.css").finish(Some(b"p { color: rgb(0, 0, 255) }".to_vec()));
    assert_eq!(dom.take_loaded(), vec![link]);
    first.finish(Some(b"p { color: rgb(255, 0, 0) }".to_vec()));
    assert!(dom.take_loaded().is_empty(), "the answer to the earlier href is dropped");
    assert_eq!((dom.sheet_state(link).unwrap(), color(&dom, p)), (1, "rgb(0, 0, 255)".to_owned()));

    dom.set_attribute(link, "href", "https://cdn.example.com/c.css").unwrap();
    let pending = take(&loads, "https://cdn.example.com/c.css");
    let holder = dom.create_element(acde::render::Ns::Html, "div").unwrap();
    dom.insert_before(holder, link, NONE).unwrap();
    assert!(dom.sheet_state(link).is_ok_and(|state| state == 3), "a link out of the document has no fetch");
    assert_eq!(color(&dom, p), "rgb(0, 0, 0)");
    pending.finish(Some(b"p { color: rgb(255, 0, 0) }".to_vec()));
    assert!(dom.take_loaded().is_empty(), "nothing is reported for a link that left");
    assert_eq!(color(&dom, p), "rgb(0, 0, 0)", "and its sheet does not apply");
}

#[test]
fn a_link_without_a_sheet_can_lose_its_href_and_rel() {
    let (_doc, dom, loads, link, p) = linked("");
    dom.remove_attribute(link, "href").unwrap();
    assert_eq!(dom.sheet_state(link).unwrap(), 3);
    take(&loads, SITE).finish(Some(b"p { color: rgb(255, 0, 0) }".to_vec()));
    assert!(dom.take_loaded().is_empty());
    assert_eq!(color(&dom, p), "rgb(0, 0, 0)");

    dom.set_attribute(link, "href", SITE).unwrap();
    take(&loads, SITE).finish(Some(b"p { color: rgb(255, 0, 0) }".to_vec()));
    assert_eq!((dom.take_loaded(), color(&dom, p)), (vec![link], "rgb(255, 0, 0)".to_owned()));
    dom.set_attribute(link, "rel", "alternate").unwrap();
    assert_eq!((dom.sheet_state(link).unwrap(), dom.sheet_rule_count(link).unwrap(), color(&dom, p)), (3, None, "rgb(0, 0, 0)".to_owned()));
}

#[test]
fn a_link_waits_for_the_sheets_its_stylesheet_imports() {
    let import = "https://cdn.example.com/more.css";
    let (_doc, dom, loads) = page(&format!(
        "<head><link id=l rel=stylesheet href='data:text/css,@import url({import}); p {{ margin-left: 3px }}'></head><body><p id=p>x</p></body>"
    ));
    let (link, p) = (by_id(&dom, "l"), by_id(&dom, "p"));
    assert_eq!(dom.sheet_state(link).unwrap(), 0, "the import is still out");
    assert!(dom.take_loaded().is_empty());
    assert_eq!(dom.computed_style(p, "", "margin-left").unwrap(), "3px", "its own rules apply already");
    let load = take(&loads, import);
    assert_eq!(load.destination, "style");
    load.finish(Some(b"p { color: rgb(255, 0, 0) }".to_vec()));
    assert_eq!(dom.take_loaded(), vec![link]);
    assert_eq!((dom.sheet_state(link).unwrap(), color(&dom, p)), (1, "rgb(255, 0, 0)".to_owned()));

    dom.set_attribute(link, "href", &format!("data:text/css,@import url({import}); p {{ margin-left: 5px }}")).unwrap();
    assert_eq!(dom.sheet_state(link).unwrap(), 0, "the new sheet is in and its import is out");
    drop(take(&loads, import));
    assert_eq!(dom.take_loaded(), vec![link]);
    assert_eq!(dom.sheet_state(link).unwrap(), 2, "a failed import fails the link");
    assert_eq!(dom.computed_style(p, "", "margin-left").unwrap(), "5px", "what did load applies");
}

#[test]
fn the_media_attribute_decides_whether_a_sheet_applies() {
    let (_doc, dom, loads) = page(&format!(
        "<head><link id=l rel=stylesheet media=print href='{SITE}'><style id=s media=print>p {{ margin-left: 7px }}</style></head><body><p id=p>x</p></body>"
    ));
    let (link, style, p) = (by_id(&dom, "l"), by_id(&dom, "s"), by_id(&dom, "p"));
    take(&loads, SITE).finish(Some(b"p { color: rgb(255, 0, 0) }".to_vec()));
    assert_eq!(dom.take_loaded(), vec![link], "fetched and reported whatever its media");
    assert_eq!((dom.sheet_state(link).unwrap(), dom.sheet_rule_count(link).unwrap()), (1, Some(1)));
    assert_eq!(
        (color(&dom, p), dom.computed_style(p, "", "margin-left").unwrap()),
        ("rgb(0, 0, 0)".to_owned(), "0px".to_owned()),
        "print sheets do not apply on a screen"
    );
    dom.set_attribute(link, "media", "all").unwrap();
    dom.remove_attribute(style, "media").unwrap();
    assert_eq!((color(&dom, p), dom.computed_style(p, "", "margin-left").unwrap()), ("rgb(255, 0, 0)".to_owned(), "7px".to_owned()));
    assert!(loads.lock().unwrap().is_empty(), "a media change does not fetch again");
    dom.set_attribute(link, "media", "(min-width: 5000px)").unwrap();
    assert_eq!(color(&dom, p), "rgb(0, 0, 0)");
}

#[test]
fn the_crossorigin_attribute_makes_the_fetch_a_cors_one() {
    let (_doc, dom, loads, link, p) = linked("");
    let first = take(&loads, SITE);
    assert_eq!(first.cors, "", "no attribute: not a CORS request");
    dom.set_attribute(link, "crossorigin", "").unwrap();
    let anonymous = take(&loads, SITE);
    assert_eq!((anonymous.destination, anonymous.cors), ("style", "anonymous"), "fetched again under the new settings");
    dom.set_attribute(link, "crossorigin", "Use-Credentials").unwrap();
    let credentialed = take(&loads, SITE);
    assert_eq!(credentialed.cors, "use-credentials");
    first.finish(Some(b"p { color: rgb(255, 0, 0) }".to_vec()));
    anonymous.finish(Some(b"p { color: rgb(255, 0, 0) }".to_vec()));
    assert!(dom.take_loaded().is_empty(), "the answers under the earlier settings are dropped");
    assert_eq!(color(&dom, p), "rgb(0, 0, 0)");
    dom.remove_attribute(link, "crossorigin").unwrap();
    drop(credentialed);
    let plain = take(&loads, SITE);
    assert_eq!(plain.cors, "");
    plain.finish(Some(b"p { color: rgb(0, 0, 255) }".to_vec()));
    assert_eq!((dom.take_loaded(), color(&dom, p)), (vec![link], "rgb(0, 0, 255)".to_owned()));
}

#[test]
fn a_sheet_is_closed_to_the_page_by_the_response_it_was_made_from() {
    let (_doc, dom, loads, link, _p) = linked("");
    assert_eq!((dom.sheet_url(link).unwrap(), dom.sheet_opaque(link).unwrap()), (None, false), "no sheet yet");
    take(&loads, SITE).finish_opaque(b"p { color: rgb(255, 0, 0) }".to_vec());
    assert_eq!(dom.take_loaded(), vec![link]);
    assert_eq!((dom.sheet_url(link).unwrap().as_deref(), dom.sheet_opaque(link).unwrap()), (Some(SITE), true));

    let own = "https://cdn.example.com/own.css";
    dom.set_attribute(link, "crossorigin", "").unwrap();
    let again = take(&loads, SITE);
    dom.set_attribute(link, "href", own).unwrap();
    drop(again);
    assert_eq!(
        (dom.sheet_url(link).unwrap().as_deref(), dom.sheet_opaque(link).unwrap()),
        (Some(SITE), true),
        "the sheet it still has, not what its attributes say now"
    );
    let first = dom.sheet_serial(link).unwrap();
    assert!(first.is_some());
    take(&loads, own).finish(Some(b"p { color: rgb(0, 0, 255) }".to_vec()));
    assert_eq!(dom.take_loaded(), vec![link]);
    assert_ne!(dom.sheet_serial(link).unwrap(), first, "another sheet");
    assert_eq!((dom.sheet_url(link).unwrap().as_deref(), dom.sheet_opaque(link).unwrap()), (Some(own), false));

    dom.set_attribute(link, "href", SITE).unwrap();
    drop(take(&loads, SITE));
    assert_eq!(dom.take_loaded(), vec![link]);
    assert_eq!((dom.sheet_url(link).unwrap(), dom.sheet_opaque(link).unwrap()), (None, false), "a failed fetch leaves no sheet");
    assert_eq!(dom.sheet_serial(link).unwrap(), None);
}

#[test]
fn an_image_taken_from_the_cache_is_still_reported_to_the_loader() {
    let (_doc, dom, loads) = page("<body id=body><img id=a src='https://img.example.com/a.png'></body>");
    take(&loads, "https://img.example.com/a.png").finish(Some(png()));
    dom.take_loaded();
    let b = dom.create_element(Ns::Html, "img").unwrap();
    dom.set_attribute(b, "src", "https://img.example.com/a.png").unwrap();
    dom.insert_before(by_id(&dom, "body"), b, NONE).unwrap();
    dom.take_loaded();
    let again = take(&loads, "https://img.example.com/a.png");
    assert!(again.served && again.cached, "taken from the cache, so only reported, as a cache hit");
    assert_eq!(again.destination, "image");
    assert!(loads.lock().unwrap().is_empty(), "inserting the image that has its picture asks for nothing more");
}

#[test]
fn a_new_image_out_of_the_document_loads_through_the_loader() {
    let (_doc, dom, loads) = page("<body></body>");
    let img = dom.create_element(Ns::Html, "img").unwrap();
    dom.set_attribute(img, "src", "https://img.example.com/new.png").unwrap();
    let load = take(&loads, "https://img.example.com/new.png");
    assert!(!load.served);
    assert_eq!(load.destination, "image");
    load.finish(Some(png()));
    assert_eq!(dom.take_loaded(), vec![img]);
    assert_eq!(dom.image_state(img).unwrap(), [2.0, 3.0, 1.0]);
}

#[test]
fn an_image_that_failed_out_of_the_document_is_not_loaded_again_when_inserted() {
    let (_doc, dom, loads) = page("<body id=body></body>");
    let img = dom.create_element(Ns::Html, "img").unwrap();
    dom.set_attribute(img, "src", "https://img.example.com/gone.png").unwrap();
    drop(take(&loads, "https://img.example.com/gone.png"));
    assert_eq!(dom.take_loaded(), vec![img], "the error is reported");
    assert_eq!(dom.image_state(img).unwrap(), [0.0, 0.0, 2.0]);
    dom.insert_before(by_id(&dom, "body"), img, NONE).unwrap();
    assert!(dom.take_loaded().is_empty(), "no second error");
    assert!(loads.lock().unwrap().is_empty(), "no second request");
    assert_eq!(dom.image_state(img).unwrap(), [0.0, 0.0, 2.0]);
    dom.set_attribute(img, "src", "https://img.example.com/other.png").unwrap();
    drop(take(&loads, "https://img.example.com/other.png"));
}

#[test]
fn setting_the_same_src_again_after_a_failure_loads_the_image_again() {
    let (_doc, dom, loads) = page("<body><img id=a src='https://img.example.com/flaky.png'></body>");
    let a = by_id(&dom, "a");
    drop(take(&loads, "https://img.example.com/flaky.png"));
    dom.take_loaded();
    assert_eq!(dom.image_state(a).unwrap(), [0.0, 0.0, 2.0]);
    dom.set_attribute(a, "src", "https://img.example.com/flaky.png").unwrap();
    take(&loads, "https://img.example.com/flaky.png").finish(Some(png()));
    assert_eq!(dom.take_loaded(), vec![a]);
    assert_eq!(dom.image_state(a).unwrap(), [2.0, 3.0, 1.0]);
    dom.set_attribute(a, "alt", "x").unwrap();
    dom.set_attribute(a, "alt", "x").unwrap();
    assert!(loads.lock().unwrap().is_empty(), "other attributes keep the same-value shortcut and load nothing");
}

#[test]
fn every_load_attempt_is_reported_once_even_when_its_state_does_not_change() {
    let (_doc, dom, loads) = page("<body id=body><img id=a src='https://img.example.com/flaky.png'><img id=b src='https://img.example.com/ok.png'></body>");
    let (a, b) = (by_id(&dom, "a"), by_id(&dom, "b"));
    dom.take_loaded();
    drop(take(&loads, "https://img.example.com/flaky.png"));
    take(&loads, "https://img.example.com/ok.png").finish(Some(png()));
    assert_eq!(dom.take_loaded(), vec![a, b]);

    dom.set_attribute(a, "src", "https://img.example.com/flaky.png").unwrap();
    drop(take(&loads, "https://img.example.com/flaky.png"));
    assert_eq!(dom.take_loaded(), vec![a], "a second failure fires a second error");
    assert_eq!(dom.image_state(a).unwrap(), [0.0, 0.0, 2.0]);

    dom.set_attribute(b, "src", "https://img.example.com/ok.png").unwrap();
    assert_eq!(dom.take_loaded(), vec![b], "the same src again, from the cache, fires a second load");
    assert_eq!(dom.image_state(b).unwrap(), [2.0, 3.0, 1.0]);

    let copy = dom.clone(b, false).unwrap();
    assert!(dom.take_loaded().is_empty(), "a clone out of the document has not tried yet");
    dom.insert_before(by_id(&dom, "body"), copy, NONE).unwrap();
    assert_eq!(dom.take_loaded(), vec![copy], "the clone loads when it is inserted");
    assert_eq!(dom.image_state(copy).unwrap(), [2.0, 3.0, 1.0]);
    assert!(dom.take_loaded().is_empty(), "and only once");
}
