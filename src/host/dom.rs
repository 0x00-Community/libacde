use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::render::{Dom, NONE};

use crate::host::bind::{self, IntoJs};
use crate::host::modules::Loader;

const ORIGIN: &str = "http://acde.local";

thread_local! {
    static DOC: RefCell<Option<(crate::render::Document, Dom)>> = const { RefCell::new(None) };
}

pub fn with_dom<R>(f: impl FnOnce(&Dom) -> R) -> Option<R> {
    DOC.with(|d| d.borrow().as_ref().map(|(_, dom)| f(dom)))
}

pub fn render_handle() -> Option<crate::render::RenderHandle> {
    DOC.with(|d| d.borrow().as_ref().map(|(document, _)| document.handle()))
}

#[derive(Default)]
struct Assets(Vec<crate::render::Resource>);

impl Assets {
    fn file(&self, url: &str, map: bool) -> Option<PathBuf> {
        let parsed = url::Url::parse(url).ok()?;
        if parsed.origin().ascii_serialization() != ORIGIN {
            return None;
        }
        let path = parsed.path();
        let (name, dirs) = self.0.iter().filter_map(|r| Some((path.strip_prefix(r.prefix.as_str())?, &r.dirs))).min_by_key(|(name, _)| name.len())?;
        let name = percent(name);
        if name.is_empty() || name.ends_with(".map") != map || name.split('/').any(|seg| seg.is_empty() || seg.starts_with('.')) {
            return None;
        }
        dirs.iter().map(|dir| Path::new(dir).join(&name)).find(|file| file.is_file())
    }
}

thread_local! {
    static ASSETS: RefCell<Assets> = RefCell::default();
}

fn asset_file(url: &str) -> Option<PathBuf> {
    ASSETS.with(|a| a.borrow().file(url, false))
}

pub fn read_asset(url: &str) -> Option<Vec<u8>> {
    std::fs::read(asset_file(url)?).ok()
}

pub fn read_source_map(url: &str) -> Option<Vec<u8>> {
    std::fs::read(ASSETS.with(|a| a.borrow().file(url, true))?).ok()
}

fn percent(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(byte) = text.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct WebLoader;

impl Loader for WebLoader {
    fn resolve(&self, specifier: &str, referrer: &str) -> Result<String, String> {
        if referrer.starts_with("file:") {
            return crate::host::sys::FileLoader.resolve(specifier, referrer);
        }
        let base = url::Url::parse(referrer).or_else(|_| url::Url::parse(&format!("{ORIGIN}/"))).map_err(|e| e.to_string())?;
        if !crate::host::modules::url_like(specifier) {
            return Err(format!("acdert: bare module specifier '{specifier}' in {referrer} (pages import URLs)"));
        }
        base.join(specifier).map(String::from).map_err(|e| format!("acdert: {specifier}: {e}"))
    }

    fn load(&self, url: &str) -> Result<String, String> {
        if let Some(source) = crate::host::bundle::source(url) {
            return Ok(source.into_owned());
        }
        if url.starts_with("file:") {
            return crate::host::sys::FileLoader.load(url);
        }
        let file = asset_file(url).ok_or_else(|| format!("acdert: module {url} is not a local file"))?;
        std::fs::read_to_string(&file).map_err(|e| format!("acdert: {}: {e}", file.display()))
    }
}

pub fn create_document<'s>(scope: &mut v8::PinScope<'s, '_>, args: v8::FunctionCallbackArguments<'s>, mut rv: v8::ReturnValue) {
    let width = bind::arg_f64(scope, &args, 0).max(1.0) as u32;
    let height = bind::arg_f64(scope, &args, 1).max(1.0) as u32;
    let scale = bind::arg_f64(scope, &args, 2) as f32;
    let mut resources = Vec::new();
    let mut dark = false;
    if let Ok(options) = v8::Local::<v8::Object>::try_from(args.get(3)) {
        if let Some(Ok(list)) = bind::get(scope, options, "resources").map(v8::Local::<v8::Array>::try_from) {
            for i in 0..list.length() {
                let Some(Ok(entry)) = list.get_index(scope, i).map(v8::Local::<v8::Object>::try_from) else { continue };
                let prefix = bind::get(scope, entry, "prefix").map(|p| p.to_rust_string_lossy(scope)).unwrap_or_default();
                let mut dirs = Vec::new();
                if let Some(Ok(list)) = bind::get(scope, entry, "dirs").map(v8::Local::<v8::Array>::try_from) {
                    for j in 0..list.length() {
                        if let Some(dir) = list.get_index(scope, j) {
                            dirs.push(dir.to_rust_string_lossy(scope));
                        }
                    }
                }
                resources.push(crate::render::Resource { prefix, dirs });
            }
        }
        dark = bind::get(scope, options, "dark").is_some_and(|v| v.is_true());
    }
    ASSETS.with(|a| *a.borrow_mut() = Assets(resources.clone()));
    let name = Some(crate::host::rt::web_name());
    let tasks = crate::host::rt::current().sender();
    let reported = std::env::var_os("ACDE_DEV").is_some();
    let loader = crate::render::Loader::new(move |load| {
        if reported || !load.served {
            _ = tasks.send(crate::host::rt::Msg::Task(Box::new(move |scope| start_load(scope, load))));
        }
    });
    let options =
        crate::render::Options { base_url: Some(format!("{ORIGIN}/")), resources: Some(resources), dark, name, loader: Some(loader), ..Default::default() };
    let document = match crate::render::Document::new(width, height, scale, &options) {
        Ok(document) => document,
        Err(e) => return bind::throw_error(scope, &format!("createDocument: {e}")),
    };
    let dom = document.dom();
    DOC.with(|d| *d.borrow_mut() = Some((document, dom)));
    rv.set(object(scope).into());
}

#[derive(Default)]
struct Loads {
    callback: Option<v8::Global<v8::Function>>,
    pending: bind::Slab<crate::render::Load>,
}

thread_local! {
    static LOADS: RefCell<Loads> = RefCell::default();
}

fn start_load(scope: &mut v8::PinScope, load: crate::render::Load) {
    let Some(callback) = LOADS.with(|l| l.borrow().callback.clone()) else { return };
    let (url, destination, cors) = (load.url.clone(), load.destination, load.cors);
    if load.served {
        let args = [(-1.0_f64).js(scope), url.js(scope), destination.js(scope), cors.js(scope), true.js(scope), load.cached.js(scope)];
        bind::call(scope, callback, &args);
        return;
    }
    let id = LOADS.with(|l| l.borrow_mut().pending.add(load));
    let args = [id.js(scope), url.js(scope), destination.js(scope), cors.js(scope)];
    if bind::call(scope, callback, &args).is_none() {
        let failed = LOADS.with(|l| l.borrow_mut().pending.remove(&id));
        drop(failed);
    }
}

fn finish_load(id: u32, bytes: Option<Vec<u8>>, opaque: bool) {
    let load = LOADS.with(|l| l.borrow_mut().pending.remove(&id));
    match (load, bytes) {
        (Some(load), Some(bytes)) if opaque => load.finish_opaque(bytes),
        (Some(load), bytes) => load.finish(bytes),
        (None, _) => {}
    }
}

pub fn clear_loads() {
    let loads = LOADS.with(|l| std::mem::take(&mut *l.borrow_mut()));
    drop(loads);
}

fn id(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> u32 {
    let n = bind::arg_f64(scope, args, i);
    if n >= 0.0 && n < NONE as f64 { n as u32 } else { NONE }
}

macro_rules! text {
    ($name:ident = $scope:ident, $args:ident, $i:expr) => {
        let mut buffer = [std::mem::MaybeUninit::<u8>::uninit(); 256];
        let $name = bind::arg_str($scope, &$args, $i, &mut buffer);
    };
}

fn f(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> f64 {
    bind::arg_f64(scope, args, i)
}

struct Id(u32);

impl<'s> IntoJs<'s> for Id {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        match self.0 {
            NONE => v8::Integer::new(scope, -1).into(),
            id => v8::Integer::new_from_unsigned(scope, id).into(),
        }
    }
}

struct Ids(Vec<u32>);

impl<'s> IntoJs<'s> for Ids {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        bind::uint32_array(scope, &self.0).into()
    }
}

impl<'s> IntoJs<'s> for (crate::render::NodeType, crate::render::Ns, bool, String) {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        let items = [
            v8::Integer::new(scope, self.0 as i32).into(),
            v8::Integer::new(scope, self.1 as i32).into(),
            v8::Boolean::new(scope, self.2).into(),
            bind::string(scope, &self.3).into(),
        ];
        v8::Array::new_with_elements(scope, &items).into()
    }
}

impl<'s> IntoJs<'s> for crate::render::AnimationEvents {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        let events: Vec<v8::Local<v8::Value>> = self
            .events
            .into_iter()
            .map(|e| {
                crate::js_object!(scope, {
                    "type" => e.kind,
                    "transition" => e.transition,
                    "target" => e.target,
                    "name" => e.name,
                    "elapsedTime" => e.elapsed,
                    "pseudoElement" => e.pseudo,
                })
                .into()
            })
            .collect();
        let events = v8::Array::new_with_elements(scope, &events);
        crate::js_object!(scope, {"animating" => self.animating, "events" => events}).into()
    }
}

struct Floats(Vec<f64>);

impl<'s> IntoJs<'s> for Floats {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        bind::float64_array(scope, &self.0).into()
    }
}

impl<'s> IntoJs<'s> for crate::render::Metrics {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        crate::js_object!(scope, {
            "offsetLeft" => self.offset_left,
            "offsetTop" => self.offset_top,
            "offsetWidth" => self.offset_width,
            "offsetHeight" => self.offset_height,
            "offsetParent" => Id(self.offset_parent.unwrap_or(NONE)),
            "clientLeft" => self.client_left,
            "clientTop" => self.client_top,
            "clientWidth" => self.client_width,
            "clientHeight" => self.client_height,
            "scrollLeft" => self.scroll_left,
            "scrollTop" => self.scroll_top,
            "scrollWidth" => self.scroll_width,
            "scrollHeight" => self.scroll_height,
        })
        .into()
    }
}

impl<'s> IntoJs<'s> for crate::render::Viewport {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        crate::js_object!(scope, {"width" => self.width, "height" => self.height, "scale" => self.scale}).into()
    }
}

thread_local! {
    static PICTURE: RefCell<(u32, Option<(u32, crate::gpu::Scene)>)> = const { RefCell::new((0, None)) };
}

fn keep_picture(scene: crate::gpu::Scene) -> u32 {
    PICTURE.with(|p| {
        let mut p = p.borrow_mut();
        p.0 = p.0.wrapping_add(1).max(1);
        let id = p.0;
        p.1 = Some((id, scene));
        id
    })
}

pub fn take_picture(id: u32) -> Option<crate::gpu::Scene> {
    PICTURE.with(|p| {
        let mut p = p.borrow_mut();
        match p.1.take() {
            Some((kept, scene)) if kept == id => Some(scene),
            other => {
                p.1 = other;
                None
            }
        }
    })
}

impl<'s> IntoJs<'s> for crate::render::Capture {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        crate::js_object!(scope, {
            "x" => self.x,
            "y" => self.y,
            "width" => self.width,
            "height" => self.height,
            "pixelWidth" => self.pixel_width as f64,
            "pixelHeight" => self.pixel_height as f64,
            "scale" => self.scale,
            "picture" => keep_picture(self.scene),
        })
        .into()
    }
}

impl<'s> IntoJs<'s> for crate::render::Timing {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        crate::js_object!(scope, {"count" => self.count as f64, "totalMs" => self.total_ms, "maxMs" => self.max_ms}).into()
    }
}

impl<'s> IntoJs<'s> for crate::render::Stats {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        crate::js_object!(scope, {
            "resolve" => self.resolve,
            "frame" => self.frame,
            "frameLocked" => self.frame_locked,
            "resolveMs" => self.resolve_ms,
            "paintMs" => self.paint_ms,
            "damageMs" => self.damage_ms,
            "layersMs" => self.layers_ms,
            "fragmentsRecorded" => self.fragments_recorded as f64,
            "fragmentsReused" => self.fragments_reused as f64,
            "animating" => self.animating,
            "nodes" => self.nodes as f64,
            "images" => self.images as f64,
            "imageBytes" => self.image_bytes,
            "mallocBytes" => self.malloc_bytes,
        })
        .into()
    }
}

impl<'s> IntoJs<'s> for crate::render::CaretPosition {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        crate::js_object!(scope, {"node" => Id(self.node), "offset" => self.offset as f64}).into()
    }
}

macro_rules! natives {
    ($($name:literal => fn $f:ident($scope:ident, $args:ident, $dom:ident) $body:block)*) => {
        $(
            #[allow(unused_variables, clippy::redundant_closure_call)]
            fn $f<'s>($scope: &mut v8::PinScope<'s, '_>, $args: v8::FunctionCallbackArguments<'s>, mut rv: v8::ReturnValue) {
                let result = DOC.with(|d| {
                    let d = d.borrow();
                    let (_, $dom) = d.as_ref()?;
                    let result: Result<_, crate::render::Error> = (|| $body)();
                    Some(result)
                });
                match result {
                    Some(Ok(value)) => rv.set(value.js($scope)),
                    Some(Err(e)) => bind::throw_error($scope, &e.to_string()),
                    None => bind::throw_error($scope, "InvalidStateError: no document"),
                }
            }
        )*

        fn object<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Object> {
            let obj = v8::Object::new(scope);
            $( bind::method(scope, obj, $name, $f); )*
            obj
        }
    };
}

fn pinned(dom: &Dom, id: u32) -> Result<Id, crate::render::Error> {
    dom.pin(id)?;
    Ok(Id(id))
}

fn quads(list: Vec<[f64; 4]>) -> Floats {
    Floats(list.into_iter().flatten().collect())
}

fn ns(code: f64) -> crate::render::Ns {
    match code as i32 {
        1 => crate::render::Ns::Svg,
        2 => crate::render::Ns::MathMl,
        _ => crate::render::Ns::Html,
    }
}

natives! {
    "document" => fn document(scope, args, dom) { Ok(Id(dom.document())) }
    "parseDocument" => fn parse_document(scope, args, dom) { text!(html = scope, args, 0); dom.parse_document(&html) }
    "viewport" => fn viewport(scope, args, dom) { Ok(dom.viewport()) }
    "nodeType" => fn node_type(scope, args, dom) { let id = id(scope, &args, 0); dom.node_type(id).map(|t| t as u8 as u32) }
    "parent" => fn parent(scope, args, dom) { let id = id(scope, &args, 0); dom.parent(id).map(Id) }
    "ancestors" => fn ancestors(scope, args, dom) { let id = id(scope, &args, 0); dom.ancestors(id).map(Ids) }
    "describe" => fn describe(scope, args, dom) { let id = id(scope, &args, 0); dom.describe(id) }
    "firstChild" => fn first_child(scope, args, dom) { let id = id(scope, &args, 0); dom.first_child(id).map(Id) }
    "lastChild" => fn last_child(scope, args, dom) { let id = id(scope, &args, 0); dom.last_child(id).map(Id) }
    "nextSibling" => fn next_sibling(scope, args, dom) { let id = id(scope, &args, 0); dom.next_sibling(id).map(Id) }
    "previousSibling" => fn previous_sibling(scope, args, dom) { let id = id(scope, &args, 0); dom.previous_sibling(id).map(Id) }
    "childNodes" => fn child_nodes(scope, args, dom) { let id = id(scope, &args, 0); dom.child_nodes(id).map(Ids) }
    "children" => fn children(scope, args, dom) { let id = id(scope, &args, 0); dom.children(id).map(Ids) }
    "childCount" => fn child_count(scope, args, dom) { let id = id(scope, &args, 0); dom.child_count(id) }
    "isConnected" => fn is_connected(scope, args, dom) { let id = id(scope, &args, 0); dom.is_connected(id) }
    "compareDocumentPosition" => fn compare_document_position(scope, args, dom) {
        let (a, b) = (id(scope, &args, 0), id(scope, &args, 1));
        dom.compare_document_position(a, b)
    }
    "createElement" => fn create_element(scope, args, dom) {
        let n = ns(f(scope, &args, 0)); text!(name = scope, args, 1);
        dom.create_element(n, &name).and_then(|id| pinned(dom, id))
    }
    "createText" => fn create_text(scope, args, dom) { text!(data = scope, args, 0); dom.create_text(&data).and_then(|id| pinned(dom, id)) }
    "createComment" => fn create_comment(scope, args, dom) { text!(data = scope, args, 0); dom.create_comment(&data).and_then(|id| pinned(dom, id)) }
    "createFragment" => fn create_fragment(scope, args, dom) { dom.create_fragment().and_then(|id| pinned(dom, id)) }
    "createDocumentType" => fn create_document_type(scope, args, dom) {
        text!(name = scope, args, 0); text!(public = scope, args, 1); text!(system = scope, args, 2);
        dom.create_document_type(&name, &public, &system).map(Id)
    }
    "documentType" => fn document_type(scope, args, dom) {
        let id = id(scope, &args, 0);
        dom.document_type(id).map(|(a, b, c)| vec![a, b, c])
    }
    "clone" => fn clone_node(scope, args, dom) {
        let (id, deep) = (id(scope, &args, 0), bind::arg_bool(scope, &args, 1));
        dom.clone(id, deep).and_then(|id| pinned(dom, id))
    }
    "insertBefore" => fn insert_before(scope, args, dom) {
        let (parent, child, reference) = (id(scope, &args, 0), id(scope, &args, 1), id(scope, &args, 2));
        dom.insert_before(parent, child, reference)
    }
    "remove" => fn remove(scope, args, dom) { let id = id(scope, &args, 0); dom.remove(id) }
    "replaceChildren" => fn replace_children(scope, args, dom) {
        let parent = id(scope, &args, 0);
        let list: Vec<u32> = bind::with_bytes(args.get(1), |bytes| bytes.as_chunks::<4>().0.iter().map(|c| u32::from_ne_bytes(*c)).collect())
            .unwrap_or_default();
        dom.replace_children(parent, &list)
    }
    "localName" => fn local_name(scope, args, dom) { let id = id(scope, &args, 0); dom.local_name(id) }
    "namespace" => fn namespace(scope, args, dom) { let id = id(scope, &args, 0); dom.namespace(id).map(|n| n as u8 as u32) }
    "getAttribute" => fn get_attribute(scope, args, dom) {
        let id = id(scope, &args, 0); text!(name = scope, args, 1);
        dom.get_attribute(id, &name)
    }
    "setAttribute" => fn set_attribute(scope, args, dom) {
        let id = id(scope, &args, 0); text!(name = scope, args, 1); text!(value = scope, args, 2);
        dom.set_attribute(id, &name, &value)
    }
    "removeAttribute" => fn remove_attribute(scope, args, dom) {
        let id = id(scope, &args, 0); text!(name = scope, args, 1);
        dom.remove_attribute(id, &name)
    }
    "hasAttribute" => fn has_attribute(scope, args, dom) {
        let id = id(scope, &args, 0); text!(name = scope, args, 1);
        dom.has_attribute(id, &name)
    }
    "attributes" => fn attributes(scope, args, dom) { let id = id(scope, &args, 0); dom.attributes(id) }
    "id" => fn element_id(scope, args, dom) { let id = id(scope, &args, 0); dom.id(id) }
    "className" => fn class_name(scope, args, dom) { let id = id(scope, &args, 0); dom.class_name(id) }
    "data" => fn data(scope, args, dom) { let id = id(scope, &args, 0); dom.data(id) }
    "setData" => fn set_data(scope, args, dom) {
        let id = id(scope, &args, 0); text!(data = scope, args, 1);
        dom.set_data(id, &data)
    }
    "textContent" => fn text_content(scope, args, dom) { let id = id(scope, &args, 0); dom.text_content(id) }
    "setTextContent" => fn set_text_content(scope, args, dom) {
        let id = id(scope, &args, 0); text!(text = scope, args, 1);
        dom.set_text_content(id, &text)
    }
    "innerText" => fn inner_text(scope, args, dom) { let id = id(scope, &args, 0); dom.inner_text(id) }
    "innerHTML" => fn inner_html(scope, args, dom) { let id = id(scope, &args, 0); dom.inner_html(id) }
    "outerHTML" => fn outer_html(scope, args, dom) { let id = id(scope, &args, 0); dom.outer_html(id) }
    "parseFragment" => fn parse_fragment(scope, args, dom) {
        let context = id(scope, &args, 0); text!(html = scope, args, 1);
        dom.parse_fragment(context, &html).map(Id)
    }
    "templateContent" => fn template_content(scope, args, dom) { let id = id(scope, &args, 0); dom.template_content(id).map(Id) }
    "parseInert" => fn parse_inert(scope, args, dom) {
        text!(html = scope, args, 0); text!(url = scope, args, 1);
        dom.parse_inert(&html, &url).and_then(|id| pinned(dom, id))
    }
    "querySelector" => fn query_selector(scope, args, dom) {
        let root = id(scope, &args, 0); text!(selector = scope, args, 1);
        dom.query_selector(root, &selector).map(Id)
    }
    "querySelectorAll" => fn query_selector_all(scope, args, dom) {
        let root = id(scope, &args, 0); text!(selector = scope, args, 1);
        dom.query_selector_all(root, &selector).map(Ids)
    }
    "matches" => fn matches(scope, args, dom) {
        let id = id(scope, &args, 0); text!(selector = scope, args, 1);
        dom.matches(id, &selector)
    }
    "closest" => fn closest(scope, args, dom) {
        let id = id(scope, &args, 0); text!(selector = scope, args, 1);
        dom.closest(id, &selector).map(Id)
    }
    "getElementById" => fn get_element_by_id(scope, args, dom) { text!(name = scope, args, 0); Ok(Id(dom.get_element_by_id(&name))) }
    "getElementsByClassName" => fn get_elements_by_class_name(scope, args, dom) {
        let root = id(scope, &args, 0); text!(names = scope, args, 1);
        dom.get_elements_by_class_name(root, &names).map(Ids)
    }
    "getElementsByTagNameNS" => fn get_elements_by_tag_name_ns(scope, args, dom) {
        let root = id(scope, &args, 0); text!(namespace = scope, args, 1); text!(local = scope, args, 2);
        dom.get_elements_by_tag_name_ns(root, &namespace, &local).map(Ids)
    }
    "createElementNS" => fn create_element_ns(scope, args, dom) {
        text!(namespace = scope, args, 0); text!(prefix = scope, args, 1); text!(local = scope, args, 2);
        dom.create_element_ns(&namespace, &prefix, &local).and_then(|id| pinned(dom, id))
    }
    "createCDATASection" => fn create_cdata(scope, args, dom) { text!(data = scope, args, 0); dom.create_cdata(&data).and_then(|id| pinned(dom, id)) }
    "createProcessingInstruction" => fn create_processing_instruction(scope, args, dom) {
        text!(target = scope, args, 0); text!(data = scope, args, 1);
        dom.create_processing_instruction(&target, &data).and_then(|id| pinned(dom, id))
    }
    "processingInstructionTarget" => fn processing_instruction_target(scope, args, dom) { let id = id(scope, &args, 0); dom.processing_instruction_target(id) }
    "createDocument" => fn create_document_node(scope, args, dom) {
        text!(interface = scope, args, 0); text!(content_type = scope, args, 1); text!(url = scope, args, 2);
        dom.create_document(&interface, &content_type, &url).and_then(|id| pinned(dom, id))
    }
    "documentInfo" => fn document_info(scope, args, dom) { let id = id(scope, &args, 0); dom.document_info(id).map(Vec::from) }
    "parseXml" => fn parse_xml(scope, args, dom) {
        text!(text = scope, args, 0); text!(content_type = scope, args, 1); text!(url = scope, args, 2);
        dom.parse_xml(&text, &content_type, &url).and_then(|id| pinned(dom, id))
    }
    "ownerDocument" => fn owner_document(scope, args, dom) { let id = id(scope, &args, 0); dom.owner_document(id).map(Id) }
    "adopt" => fn adopt(scope, args, dom) { let (id, doc) = (id(scope, &args, 0), id(scope, &args, 1)); dom.adopt(id, doc) }
    "inHtmlDocument" => fn in_html_document(scope, args, dom) { let id = id(scope, &args, 0); dom.in_html_document(id) }
    "namespaceURI" => fn namespace_uri(scope, args, dom) { let id = id(scope, &args, 0); dom.namespace_uri(id) }
    "prefix" => fn prefix(scope, args, dom) { let id = id(scope, &args, 0); dom.prefix(id) }
    "attributesNS" => fn attributes_ns(scope, args, dom) { let id = id(scope, &args, 0); dom.attributes_ns(id) }
    "getAttributeNS" => fn get_attribute_ns(scope, args, dom) {
        let id = id(scope, &args, 0); text!(namespace = scope, args, 1); text!(local = scope, args, 2);
        dom.get_attribute_ns(id, &namespace, &local)
    }
    "setAttributeNS" => fn set_attribute_ns(scope, args, dom) {
        let id = id(scope, &args, 0); text!(namespace = scope, args, 1); text!(prefix = scope, args, 2); text!(local = scope, args, 3); text!(value = scope, args, 4);
        dom.set_attribute_ns(id, &namespace, &prefix, &local, &value)
    }
    "removeAttributeNS" => fn remove_attribute_ns(scope, args, dom) {
        let id = id(scope, &args, 0); text!(namespace = scope, args, 1); text!(local = scope, args, 2);
        dom.remove_attribute_ns(id, &namespace, &local)
    }
    "serializeXml" => fn serialize_xml(scope, args, dom) { let id = id(scope, &args, 0); dom.serialize_xml(id) }
    "getElementsByTagName" => fn get_elements_by_tag_name(scope, args, dom) {
        let root = id(scope, &args, 0); text!(name = scope, args, 1);
        dom.get_elements_by_tag_name(root, &name).map(Ids)
    }
    "value" => fn value(scope, args, dom) { let id = id(scope, &args, 0); dom.value(id) }
    "setValue" => fn set_value(scope, args, dom) {
        let id = id(scope, &args, 0); text!(value = scope, args, 1);
        dom.set_value(id, &value)
    }
    "checked" => fn checked(scope, args, dom) { let id = id(scope, &args, 0); dom.checked(id) }
    "setChecked" => fn set_checked(scope, args, dom) {
        let (id, on) = (id(scope, &args, 0), bind::arg_bool(scope, &args, 1));
        dom.set_checked(id, on)
    }
    "selectedIndex" => fn selected_index(scope, args, dom) { let id = id(scope, &args, 0); dom.selected_index(id) }
    "setSelectedIndex" => fn set_selected_index(scope, args, dom) {
        let (id, index) = (id(scope, &args, 0), f(scope, &args, 1) as i32);
        dom.set_selected_index(id, index)
    }
    "options" => fn options(scope, args, dom) { let id = id(scope, &args, 0); dom.options(id).map(Ids) }
    "optionSelected" => fn option_selected(scope, args, dom) { let id = id(scope, &args, 0); dom.option_selected(id) }
    "setOptionSelected" => fn set_option_selected(scope, args, dom) {
        let (id, on) = (id(scope, &args, 0), bind::arg_bool(scope, &args, 1));
        dom.set_option_selected(id, on)
    }
    "resetSelect" => fn reset_select(scope, args, dom) { let id = id(scope, &args, 0); dom.reset_select(id) }
    "listBoxOptionAt" => fn list_box_option_at(scope, args, dom) {
        let (id, x, y) = (id(scope, &args, 0), f(scope, &args, 1), f(scope, &args, 2));
        dom.list_box_option_at(id, x, y)
    }
    "openPicker" => fn open_picker(scope, args, dom) { let id = id(scope, &args, 0); dom.open_picker(id) }
    "closePicker" => fn close_picker(scope, args, dom) { dom.close_picker(); Ok(()) }
    "pickerState" => fn picker_state(scope, args, dom) { Ok(Floats(dom.picker_state().to_vec())) }
    "pickerOptionAt" => fn picker_option_at(scope, args, dom) {
        let (x, y) = (f(scope, &args, 0), f(scope, &args, 1));
        Ok(dom.picker_option_at(x, y))
    }
    "pickerOptionRect" => fn picker_option_rect(scope, args, dom) {
        let id = id(scope, &args, 0);
        dom.picker_option_rect(id).map(|q| q.map(|q| Floats(q.to_vec())))
    }
    "setPickerActive" => fn set_picker_active(scope, args, dom) { let index = f(scope, &args, 0) as i32; dom.set_picker_active(index); Ok(()) }
    "setSelectionRange" => fn set_selection_range(scope, args, dom) {
        let (id, start, end) = (id(scope, &args, 0), f(scope, &args, 1) as u32, f(scope, &args, 2) as u32);
        dom.set_selection_range(id, start, end)
    }
    "setFocus" => fn set_focus(scope, args, dom) {
        let (id, visible) = (id(scope, &args, 0), bind::arg_bool(scope, &args, 1));
        dom.set_focus(id, visible)
    }
    "setActive" => fn set_active(scope, args, dom) { let id = id(scope, &args, 0); dom.set_active(id) }
    "setOpenState" => fn set_open_state(scope, args, dom) {
        let (id, modal, open) = (id(scope, &args, 0), bind::arg_bool(scope, &args, 1), bind::arg_bool(scope, &args, 2));
        dom.set_open_state(id, modal, open)
    }
    "setSelection" => fn set_selection(scope, args, dom) {
        let (a, ao, b, bo) = (id(scope, &args, 0), f(scope, &args, 1) as u32, id(scope, &args, 2), f(scope, &args, 3) as u32);
        dom.set_selection(a, ao, b, bo)
    }
    "rect" => fn rect(scope, args, dom) { let id = id(scope, &args, 0); dom.rect(id).map(|q| Floats(q.to_vec())) }
    "clientRects" => fn client_rects(scope, args, dom) { let id = id(scope, &args, 0); dom.client_rects(id).map(quads) }
    "metrics" => fn metrics(scope, args, dom) { let id = id(scope, &args, 0); dom.metrics(id) }
    "hitTest" => fn hit_test(scope, args, dom) {
        let (x, y) = (f(scope, &args, 0), f(scope, &args, 1));
        Ok(dom.hit_test(x, y).map_or(-1.0, f64::from))
    }
    "hitTestAll" => fn hit_test_all(scope, args, dom) {
        let (x, y) = (f(scope, &args, 0), f(scope, &args, 1));
        Ok(Ids(dom.hit_test_all(x, y)))
    }
    "computedStyle" => fn computed_style(scope, args, dom) {
        let id = id(scope, &args, 0); text!(pseudo = scope, args, 1); text!(property = scope, args, 2);
        dom.computed_style(id, &pseudo, &property)
    }
    "computedStyleAll" => fn computed_style_all(scope, args, dom) {
        let id = id(scope, &args, 0); text!(pseudo = scope, args, 1);
        dom.computed_style_all(id, &pseudo)
    }
    "cssSupports" => fn css_supports(scope, args, dom) {
        text!(condition = scope, args, 0);
        dom.css_supports(&condition)
    }
    "cssSupportsDeclaration" => fn css_supports_declaration(scope, args, dom) {
        text!(property = scope, args, 0); text!(value = scope, args, 1);
        dom.css_supports_declaration(&property, &value)
    }
    "caretRect" => fn caret_rect(scope, args, dom) {
        let (id, offset) = (id(scope, &args, 0), f(scope, &args, 1) as u32);
        dom.caret_rect(id, offset).map(|q| Floats(q.to_vec()))
    }
    "caretFromPoint" => fn caret_from_point(scope, args, dom) {
        let (x, y) = (f(scope, &args, 0), f(scope, &args, 1));
        Ok(dom.caret_from_point(x, y))
    }
    "textRects" => fn text_rects(scope, args, dom) {
        let (id, start, end) = (id(scope, &args, 0), f(scope, &args, 1) as u32, f(scope, &args, 2) as u32);
        dom.text_rects(id, start, end).map(quads)
    }
    "matchMedia" => fn match_media(scope, args, dom) { text!(query = scope, args, 0); Ok(dom.match_media(&query)) }
    "parseColor" => fn parse_color(scope, args, dom) { text!(color = scope, args, 0); Ok(dom.parse_color(&color).map(|c| Floats(c.to_vec()))) }
    "sheetRuleCount" => fn sheet_rule_count(scope, args, dom) {
        let id = id(scope, &args, 0);
        dom.sheet_rule_count(id).map(|n| n.map_or(-1.0, f64::from))
    }
    "sheetRule" => fn sheet_rule(scope, args, dom) {
        let (id, index) = (id(scope, &args, 0), f(scope, &args, 1) as u32);
        dom.sheet_rule(id, index)
    }
    "sheetInsertRule" => fn sheet_insert_rule(scope, args, dom) {
        let id = id(scope, &args, 0); text!(rule = scope, args, 1); let index = f(scope, &args, 2) as u32;
        dom.sheet_insert_rule(id, &rule, index)
    }
    "sheetDeleteRule" => fn sheet_delete_rule(scope, args, dom) {
        let (id, index) = (id(scope, &args, 0), f(scope, &args, 1) as u32);
        dom.sheet_delete_rule(id, index)
    }
    "scrollTo" => fn scroll_to(scope, args, dom) {
        let (id, left, top, smooth) = (id(scope, &args, 0), f(scope, &args, 1), f(scope, &args, 2), bind::arg_bool(scope, &args, 3));
        dom.scroll_to(id, left, top, smooth)
    }
    "takeScrolled" => fn take_scrolled(scope, args, dom) { Ok(Ids(dom.take_scrolled())) }
    "imageState" => fn image_state(scope, args, dom) { let id = id(scope, &args, 0); dom.image_state(id).map(|s| Floats(s.to_vec())) }
    "takeLoaded" => fn take_loaded(scope, args, dom) { Ok(Ids(dom.take_loaded())) }
    "takeAnimationEvents" => fn take_animation_events(scope, args, dom) { Ok(dom.take_animation_events()) }
    "sheetState" => fn sheet_state(scope, args, dom) { let id = id(scope, &args, 0); dom.sheet_state(id).map(f64::from) }
    "sheetUrl" => fn sheet_url(scope, args, dom) { let id = id(scope, &args, 0); dom.sheet_url(id) }
    "sheetOpaque" => fn sheet_opaque(scope, args, dom) { let id = id(scope, &args, 0); dom.sheet_opaque(id) }
    "sheetSerial" => fn sheet_serial(scope, args, dom) { let id = id(scope, &args, 0); dom.sheet_serial(id).map(|n| n.map_or(-1.0, f64::from)) }
    "setLoader" => fn set_loader(scope, args, dom) {
        let callback = v8::Local::<v8::Function>::try_from(args.get(0)).ok().map(|f| v8::Global::new(scope, f));
        LOADS.with(|l| l.borrow_mut().callback = callback);
        Ok(())
    }
    "loaded" => fn loaded(scope, args, dom) {
        let id = f(scope, &args, 0) as u32;
        finish_load(id, bind::bytes(args.get(1)), bind::arg_bool(scope, &args, 2));
        Ok(())
    }
    "canvas" => fn canvas(scope, args, dom) {
        let id = id(scope, &args, 0);
        bind::with_bytes(args.get(1), |commands| dom.canvas(id, commands)).unwrap_or_else(|| dom.canvas(id, &[]))
    }
    "measureText" => fn measure_text(scope, args, dom) {
        let id = id(scope, &args, 0); text!(font = scope, args, 1); text!(text = scope, args, 2);
        dom.measure_text(id, &font, &text).map(|m| Floats(vec![
            m.width,
            m.actual_bounding_box_left,
            m.actual_bounding_box_right,
            m.actual_bounding_box_ascent,
            m.actual_bounding_box_descent,
            m.font_bounding_box_ascent,
            m.font_bounding_box_descent,
        ]))
    }
    "toDataURL" => fn to_data_url(scope, args, dom) {
        let id = id(scope, &args, 0); text!(kind = scope, args, 1);
        dom.to_data_url(id, &kind)
    }
    "getImageData" => fn get_image_data(scope, args, dom) {
        let (id, x, y, w, h) = (id(scope, &args, 0), f(scope, &args, 1), f(scope, &args, 2), f(scope, &args, 3), f(scope, &args, 4));
        dom.get_image_data(id, x, y, w, h)
    }
    "capture" => fn capture(scope, args, dom) {
        let id = id(scope, &args, 0);
        let scale = f(scope, &args, 1);
        dom.capture(id, if scale.is_finite() && scale > 0.0 { scale } else { 1.0 })
    }
    "setPanel" => fn set_panel(scope, args, dom) {
        let (id, x, y, w, h) = (id(scope, &args, 0), f(scope, &args, 1), f(scope, &args, 2), f(scope, &args, 3), f(scope, &args, 4));
        dom.set_panel(id, (w > 0.0 && h > 0.0).then_some([x, y, w, h]))
    }
    "highlight" => fn highlight(scope, args, dom) {
        let id = id(scope, &args, 0);
        if std::env::var_os("ACDE_DEV").is_some() { dom.highlight(id) } else { Ok(()) }
    }
    "unpin" => fn unpin(scope, args, dom) { let id = id(scope, &args, 0); dom.unpin(id) }
    "pinnedIn" => fn pinned_in(scope, args, dom) { let id = id(scope, &args, 0); dom.pinned_in(id).map(Ids) }
    "commit" => fn commit(scope, args, dom) { dom.commit(); Ok(()) }
    "semantics" => fn semantics(scope, args, dom) {
        let (id, limit, offscreen, invisible) = (id(scope, &args, 0), f(scope, &args, 1), bind::arg_bool(scope, &args, 2), bind::arg_bool(scope, &args, 3));
        dom.semantics(id, if limit.is_finite() && limit >= 1.0 { limit as u32 } else { 600 }, offscreen, invisible)
    }
    "stats" => fn stats(scope, args, dom) {
        let stats = dom.stats();
        if bind::arg_bool(scope, &args, 0) {
            dom.reset_timings();
        }
        Ok(stats)
    }
    "resize" => fn resize(scope, args, dom) {
        let (w, h, scale) = (f(scope, &args, 0) as u32, f(scope, &args, 1) as u32, f(scope, &args, 2) as f32);
        dom.resize(w, h, scale);
        Ok(())
    }
    "setDark" => fn set_dark(scope, args, dom) {
        dom.set_dark(bind::arg_bool(scope, &args, 0));
        Ok(())
    }
}
