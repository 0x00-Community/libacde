use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::host::surface::{DragImage, Event, Surface};

use crate::host::bind;
use crate::host::rt::{self, Msg};
use crate::render::ClipboardEntry;

const CLIPBOARD_WAIT: Duration = Duration::from_secs(2);

#[derive(Default)]
struct State {
    app: Option<String>,
    surface: Option<Surface>,
    waiting: Option<v8::Global<v8::PromiseResolver>>,
    then: Option<v8::Global<v8::Function>>,
    armed: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    imes: bind::Slab<(crate::runtime::Ime, v8::Global<v8::Function>)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>) {
    crate::methods!(scope, natives,
        "claimSurface" => claim_surface,
    );
    #[cfg(target_os = "linux")]
    crate::methods!(scope, natives,
        "Ime" => ime::open,
    );
}

pub fn release_surface() {
    let surface = STATE.with(|s| s.borrow_mut().surface.take());
    drop(surface);
}

pub fn bind_app(app: Option<String>) {
    STATE.with(|s| s.borrow_mut().app = app);
}

pub fn clear() {
    let state = STATE.with(|s| std::mem::take(&mut *s.borrow_mut()));
    drop(state);
}

fn claim_surface(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Ok(token) = v8::Local::<v8::BigInt>::try_from(args.get(0)) else {
        return bind::throw_type_error(scope, "claimSurface: the token is a bigint");
    };
    let (token, lossless) = token.u64_value();
    if !lossless {
        return bind::throw_type_error(scope, "claimSurface: not a token");
    }
    let Some(render) = crate::host::dom::render_handle() else {
        return bind::throw_error(scope, "claimSurface: this web has no document yet");
    };
    let frame = v8::Local::<v8::Object>::try_from(args.get(1))
        .ok()
        .and_then(|options| bind::get(scope, options, "frame"))
        .and_then(|value| value.number_value(scope))
        .filter(|height| height.is_finite() && *height >= 1.0)
        .map_or(0, |height| height.round().min(f64::from(u16::MAX)) as u32);
    let app = STATE.with(|s| s.borrow().app.clone());
    let surface = match Surface::claim(token, render, frame, app) {
        Ok(surface) => surface,
        Err(e) => return bind::throw_error(scope, &e),
    };
    if let Some((width, height, scale)) = surface.size() {
        crate::host::dom::with_dom(|dom| dom.resize(width, height, scale as f32));
    }
    let tx = rt::current().sender();
    let armed = STATE.with(|s| s.borrow().armed.clone());
    surface.set_waker(Some(Arc::new(move || {
        if armed.swap(false, Ordering::SeqCst) {
            _ = tx.send(Msg::Task(Box::new(wake)));
        }
    })));
    let window = surface.window();
    let hosted = surface.hosted();
    let controls = surface.host_controls();
    let minimizable = surface.minimizable();
    let pointer_lock = surface.pointer_lock();
    let (offset_x, offset_y) = surface.content_offset();
    STATE.with(|s| s.borrow_mut().surface = Some(surface));
    let offset = [offset_x, offset_y].map(|v| v8::Number::new(scope, v).into());
    let offset = v8::Array::new_with_elements(scope, &offset);
    let obj = crate::js_object!(scope, {"window" => window, "hosted" => hosted, "hostControls" => controls, "minimizable" => minimizable, "pointerLock" => pointer_lock, "contentOffset" => offset});
    crate::methods!(scope, obj,
        "pump" => pump,
        "wait" => wait,
        "waitThen" => wait_then,
        "requestFrame" => request_frame,
        "requestMode" => request_mode,
        "requestState" => request_state,
        "setGrabHint" => set_grab_hint,
        "setBackdrop" => set_backdrop,
        "requestActivate" => request_activate,
        "requestGrab" => request_grab,
        "releaseGrab" => release_grab,
        "lockPointer" => lock_pointer,
        "openPanel" => open_panel,
        "placePanel" => place_panel,
        "closePanel" => close_panel,
        "reservePanel" => reserve_panel,
        "setDoubleClick" => set_double_click,
        "setFrameTheme" => set_frame_theme,
        "setPageDark" => set_page_dark,
        "setClipboard" => set_clipboard,
        "clipboardTypes" => clipboard_types,
        "readClipboard" => read_clipboard,
        "startDrag" => start_drag,
        "setTitle" => set_title,
        "setImeArea" => set_ime_area,
    );
    rv.set(obj.into());
}

fn with_surface<R>(scope: &mut v8::PinScope, f: impl FnOnce(&Surface) -> R) -> Option<R> {
    let result = STATE.with(|s| s.borrow().surface.as_ref().map(f));
    if result.is_none() {
        bind::throw_error(scope, "surface: released");
    }
    result
}

fn pump(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(events) = with_surface(scope, Surface::pump) else { return };
    traced(&events);
    let list = v8::Array::new(scope, events.len() as i32);
    for (i, event) in events.into_iter().enumerate() {
        let event = event_object(scope, event);
        list.set_index(scope, i as u32, event.into());
    }
    rv.set(list.into());
}

fn traced(events: &[Event]) {
    if !log::log_enabled!(target: "perf::latency", log::Level::Debug) {
        return;
    }
    let pumped = crate::log::now_ms();
    let trace = events
        .iter()
        .filter_map(|e| {
            let kind = match e.kind.as_str() {
                "pointerdown" => "pointerdown",
                "pointerup" => "pointerup",
                "keydown" => "keydown",
                "wheel" => "wheel",
                _ => return None,
            };
            Some(crate::log::Trace { pumped, ..crate::log::Trace::input(kind, e.stamp?) })
        })
        .fold(None, |a, b| crate::log::Trace::earliest(a, Some(b)));
    if let Some(trace) = trace {
        crate::host::dom::with_dom(|dom| dom.input(trace));
    }
}

pub fn event_object<'s>(scope: &mut v8::PinScope<'s, '_>, event: Event) -> v8::Local<'s, v8::Object> {
    let mut fields: Vec<(&str, v8::Local<v8::Value>)> = vec![("type", bind::string(scope, &event.kind).into())];
    let numbers = [
        ("window", event.window.map(f64::from)),
        ("x", event.x),
        ("y", event.y),
        ("button", event.button.map(f64::from)),
        ("deltaX", event.delta_x),
        ("deltaY", event.delta_y),
        ("code", event.code.map(f64::from)),
        ("width", event.width),
        ("height", event.height),
        ("time", event.time),
        ("scale", event.scale),
        ("parent", event.parent.map(f64::from)),
    ];
    for (name, value) in numbers {
        if let Some(value) = value {
            fields.push((name, v8::Number::new(scope, value).into()));
        }
    }
    let strings = [("key", &event.key), ("title", &event.title), ("appId", &event.app_id), ("state", &event.state), ("previous", &event.previous)];
    for (name, value) in strings {
        if let Some(value) = value {
            fields.push((name, bind::string(scope, value).into()));
        }
    }
    if let Some(mime) = &event.mime {
        let kinds: Vec<v8::Local<v8::Value>> = mime.iter().map(|kind| bind::string(scope, kind).into()).collect();
        fields.push(("mime", v8::Array::new_with_elements(scope, &kinds).into()));
    }
    if let Some(data) = &event.data {
        let texts: Vec<(&str, v8::Local<v8::Value>)> = data.iter().map(|(kind, text)| (kind.as_str(), bind::string(scope, text).into())).collect();
        fields.push(("data", bind::object(scope, &texts).into()));
    }
    if let Some(files) = &event.files {
        let paths: Vec<v8::Local<v8::Value>> = files.iter().map(|path| bind::string(scope, path).into()).collect();
        fields.push(("files", v8::Array::new_with_elements(scope, &paths).into()));
    }
    bind::object(scope, &fields)
}

fn wait(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(resolver) = v8::PromiseResolver::new(scope) else { return };
    let promise = resolver.get_promise(scope);
    let global = v8::Global::new(scope, resolver);
    let (ready, replaced) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.armed.store(true, Ordering::SeqCst);
        let ready = s.surface.as_ref().is_none_or(Surface::has_events);
        if ready {
            s.armed.store(false, Ordering::SeqCst);
            return (true, None);
        }
        (false, s.waiting.replace(global))
    });
    let undefined = v8::undefined(scope);
    if let Some(replaced) = replaced {
        v8::Local::new(scope, replaced).resolve(scope, undefined.into());
    }
    if ready {
        resolver.resolve(scope, undefined.into());
    }
    rv.set(promise.into());
}

fn wait_then(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        return bind::throw_type_error(scope, "waitThen: a function");
    };
    let callback = v8::Global::new(scope, callback);
    let ready = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.then = Some(callback);
        s.armed.store(true, Ordering::SeqCst);
        let ready = s.surface.as_ref().is_none_or(Surface::has_events);
        ready && s.armed.swap(false, Ordering::SeqCst)
    });
    if ready {
        _ = rt::current().sender().send(Msg::Task(Box::new(wake)));
    }
}

fn wake(scope: &mut v8::PinScope) {
    if let Some(waiting) = STATE.with(|s| s.borrow_mut().waiting.take()) {
        let undefined = v8::undefined(scope);
        v8::Local::new(scope, waiting).resolve(scope, undefined.into());
    }
    if let Some(then) = STATE.with(|s| s.borrow_mut().then.take()) {
        bind::call(scope, then, &[]);
    }
}

fn request_frame(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    with_surface(scope, Surface::request_frame);
}

fn request_mode(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let mode = match bind::arg_string(scope, &args, 0).as_str() {
        "free" => 1,
        "fixed" => 2,
        "pinned" => 3,
        "fullscreen" => return bind::throw_type_error(scope, "requestMode: full screen is a window state, use requestState('fullscreen')"),
        other => return bind::throw_type_error(scope, &format!("requestMode: no mode {other}")),
    };
    with_surface(scope, |surface| surface.request_mode(mode));
}

fn request_state(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let name = bind::arg_string(scope, &args, 0);
    match name.parse::<crate::model::WindowState>() {
        Ok(state) => {
            with_surface(scope, |surface| surface.request_state(state as u32));
        }
        Err(e) => bind::throw_type_error(scope, &format!("requestState: {e}")),
    }
}

fn rect(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments) -> [i32; 4] {
    [0, 1, 2, 3].map(|i| bind::arg_f64(scope, args, i).round() as i32)
}

fn set_grab_hint(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let rect = rect(scope, &args);
    let edges = bind::arg_f64(scope, &args, 4) as u32;
    with_surface(scope, |surface| surface.set_grab_hint(rect, edges));
}

fn set_backdrop(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let rect = rect(scope, &args);
    let effect = [4, 5, 6, 7].map(|i| bind::arg_f64(scope, &args, i));
    with_surface(scope, |surface| surface.set_backdrop(rect, effect));
}

fn open_panel(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_f64(scope, &args, 0) as u32;
    let Ok(token) = v8::Local::<v8::BigInt>::try_from(args.get(1)) else {
        return bind::throw_type_error(scope, "openPanel: the token is a bigint");
    };
    let (token, _) = token.u64_value();
    let rect = [2, 3, 4, 5].map(|i| bind::arg_f64(scope, &args, i).round() as i32);
    let exclusive = args.get(6).is_true();
    with_surface(scope, |surface| surface.open_panel(id, token, rect, exclusive));
}

fn place_panel(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_f64(scope, &args, 0) as u32;
    let rect = [1, 2, 3, 4].map(|i| bind::arg_f64(scope, &args, i).round() as i32);
    let input = [5, 6, 7, 8].map(|i| bind::arg_f64(scope, &args, i).round() as i32);
    with_surface(scope, |surface| surface.place_panel(id, rect, input));
}

fn close_panel(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_f64(scope, &args, 0) as u32;
    with_surface(scope, |surface| surface.close_panel(id));
}

fn reserve_panel(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_f64(scope, &args, 0) as u32;
    let edge = bind::arg_f64(scope, &args, 1) as u32;
    let thickness = bind::arg_f64(scope, &args, 2).max(0.0);
    with_surface(scope, |surface| surface.reserve_panel(id, edge, thickness));
}

fn set_double_click(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let name = bind::arg_string(scope, &args, 0);
    match name.parse::<crate::model::DoubleClick>() {
        Ok(action) => {
            with_surface(scope, |surface| surface.set_double_click(action as u32));
        }
        Err(e) => bind::throw_type_error(scope, &format!("setDoubleClick: {e}")),
    }
}

fn set_frame_theme(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    use crate::render::{ButtonSide, FrameColors, FrameStyle, FrameTheme, TitleAlign};
    let color = |scope: &mut v8::PinScope, i: i32| {
        let v = bind::arg_f64(scope, &args, i);
        (0.0..=f64::from(u32::MAX)).contains(&v).then(|| (v as u32).to_be_bytes())
    };
    let mut colors = [FrameColors::default(); 4];
    for (i, c) in colors.iter_mut().enumerate() {
        let at = i as i32 * 4;
        *c = FrameColors { background: color(scope, at), title: color(scope, at + 1), line: color(scope, at + 2), buttons: color(scope, at + 3) };
    }
    let pick = |scope: &mut v8::PinScope, i: i32| bind::arg_f64(scope, &args, i) as u32;
    let theme = FrameTheme {
        active: colors[0],
        inactive: colors[1],
        light: [colors[2], colors[3]],
        style: [None, Some(FrameStyle::Mac), Some(FrameStyle::Windows)].get(pick(scope, 16) as usize).copied().flatten(),
        side: [None, Some(ButtonSide::Left), Some(ButtonSide::Right)].get(pick(scope, 17) as usize).copied().flatten(),
        align: [None, Some(TitleAlign::Center), Some(TitleAlign::Start)].get(pick(scope, 18) as usize).copied().flatten(),
    };
    with_surface(scope, |surface| surface.set_frame_theme(theme));
}

fn set_page_dark(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let dark = bind::arg_bool(scope, &args, 0);
    with_surface(scope, |surface| surface.set_page_dark(dark));
}

fn request_activate(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    with_surface(scope, Surface::request_activate);
}

fn grab_kind(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments) -> Option<u32> {
    match bind::arg_string(scope, args, 0).as_str() {
        "pointer" => Some(1),
        "keyboard" => Some(2),
        other => {
            bind::throw_type_error(scope, &format!("grab: no kind {other}"));
            None
        }
    }
}

fn request_grab(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Some(kind) = grab_kind(scope, &args) else { return };
    with_surface(scope, |surface| surface.request_grab(kind, true));
}

fn release_grab(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Some(kind) = grab_kind(scope, &args) else { return };
    with_surface(scope, |surface| surface.request_grab(kind, false));
}

fn lock_pointer(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let on = bind::arg_bool(scope, &args, 0);
    with_surface(scope, |surface| surface.lock_pointer(on));
}

fn set_title(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let title = bind::arg_string(scope, &args, 0);
    with_surface(scope, |surface| surface.set_title(&title));
}

fn set_ime_area(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let rect = [0, 1, 2, 3].map(|i| bind::arg_f64(scope, &args, i));
    with_surface(scope, |surface| surface.set_ime_area(rect));
}

fn set_clipboard(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let entries = clipboard_entries(scope, args.get(0));
    with_surface(scope, |surface| surface.set_clipboard(entries));
}

fn clipboard_entries(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Vec<ClipboardEntry> {
    let Ok(list) = v8::Local::<v8::Array>::try_from(value) else { return Vec::new() };
    let mut entries = Vec::new();
    for i in 0..list.length() {
        let Some(entry) = list.get_index(scope, i).and_then(|v| v8::Local::<v8::Object>::try_from(v).ok()) else { continue };
        let Some(mime) = bind::get_string(scope, entry, "type") else { continue };
        let Some(data) = bind::get(scope, entry, "data") else { continue };
        let data = if data.is_string() { Some(data.to_rust_string_lossy(scope).into_bytes()) } else { bind::bytes(data) };
        if let Some(data) = data {
            entries.push(ClipboardEntry::new(mime, data));
        }
    }
    entries
}

fn clipboard_types(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some((token, promise)) = bind::promise(scope) else { return };
    let tx = rt::current().sender();
    let reply = move |types: Vec<String>| _ = tx.send(Msg::Task(Box::new(move |scope| clipboard_types_answer(scope, token, &types))));
    if with_surface(scope, |surface| surface.clipboard_types(Box::new(reply))).is_none() {
        return bind::forget_promise(token);
    }
    rt::current().delay(CLIPBOARD_WAIT, move |scope| clipboard_types_answer(scope, token, &[]));
    rv.set(promise.into());
}

fn clipboard_types_answer(scope: &mut v8::PinScope, token: u32, types: &[String]) {
    if let Some(resolver) = bind::resolver(scope, token) {
        let names: Vec<v8::Local<v8::Value>> = types.iter().map(|t| bind::string(scope, t).into()).collect();
        let list = v8::Array::new_with_elements(scope, &names);
        resolver.resolve(scope, list.into());
    }
}

fn read_clipboard(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let mime = bind::arg_string(scope, &args, 0);
    let Some((token, promise)) = bind::promise(scope) else { return };
    let tx = rt::current().sender();
    let reply = move |data: Option<Vec<u8>>| _ = tx.send(Msg::Task(Box::new(move |scope| clipboard_read(scope, token, data))));
    if with_surface(scope, |surface| surface.read_clipboard(mime, Box::new(reply))).is_none() {
        return bind::forget_promise(token);
    }
    rt::current().delay(CLIPBOARD_WAIT, move |scope| clipboard_read(scope, token, None));
    rv.set(promise.into());
}

fn clipboard_read(scope: &mut v8::PinScope, token: u32, data: Option<Vec<u8>>) {
    if let Some(resolver) = bind::resolver(scope, token) {
        let value: v8::Local<v8::Value> = match data {
            Some(data) => bind::uint8_array(scope, data).into(),
            None => v8::null(scope).into(),
        };
        resolver.resolve(scope, value);
    }
}

fn start_drag<'s>(scope: &mut v8::PinScope<'s, '_>, args: v8::FunctionCallbackArguments<'s>, _: v8::ReturnValue) {
    let data: HashMap<String, String> = bind::own_string_pairs(scope, args.get(0)).into_iter().collect();
    let icon = v8::Local::<v8::Object>::try_from(args.get(1)).ok().and_then(|icon| {
        let mut number = |name: &str| bind::get(scope, icon, name).and_then(|v| v.number_value(scope)).filter(|n| n.is_finite());
        let (width, height) = (number("width")? as u32, number("height")? as u32);
        let hotspot = (number("hotX").unwrap_or(0.0) as i32, number("hotY").unwrap_or(0.0) as i32);
        let picture = number("picture").and_then(|id| crate::host::dom::take_picture(id as u32));
        let scene = match picture {
            Some(scene) => scene,
            None => crate::render::Capture::bitmap(
                width,
                height,
                bind::get(scope, icon, "rgba").and_then(bind::bytes).filter(|b| b.len() == (width * height * 4) as usize)?,
            ),
        };
        Some(DragImage { width, height, scene, hotspot })
    });
    with_surface(scope, |surface| surface.start_drag(data, icon));
}

#[cfg(target_os = "linux")]
mod ime {
    use super::*;

    pub fn open(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
        let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
            return bind::throw_type_error(scope, "Ime: the callback is not a function");
        };
        let id = STATE.with(|s| s.borrow_mut().imes.next_id());
        let tx = rt::current().sender();
        let opened = crate::runtime::Ime::open(move |event| {
            let json = event.to_string();
            _ = tx.send(Msg::Task(Box::new(move |scope| deliver(scope, id, &json))));
        });
        let ime = match opened {
            Ok(ime) => ime,
            Err(e) => return bind::throw_error(scope, &e),
        };
        let backend = ime.backend();
        let callback = v8::Global::new(scope, callback);
        STATE.with(|s| s.borrow_mut().imes.insert(id, (ime, callback)));
        let obj = crate::js_object!(scope, {"backend" => backend});
        let data: v8::Local<v8::Value> = v8::Integer::new_from_unsigned(scope, id).into();
        methods(scope, obj, data);
        rv.set(obj.into());
    }

    fn deliver(scope: &mut v8::PinScope, id: u32, json: &str) {
        let Some(callback) = STATE.with(|s| s.borrow().imes.get(&id).map(|(_, f)| f.clone())) else { return };
        let json = bind::string(scope, json);
        let Some(event) = v8::json::parse(scope, json) else { return };
        bind::call(scope, callback, &[event]);
    }

    type Answer = Result<Option<bool>, String>;

    macro_rules! done {
        ($result:expr) => {
            $result.map(|()| None)
        };
    }

    macro_rules! natives {
        ($($name:literal => fn $f:ident($s:pat, $a:pat, $ime:ident) $body:expr;)*) => {
            $(
                fn $f(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, rv: v8::ReturnValue) {
                    call(scope, &args, rv, |$s, $a, $ime| $body);
                }
            )*

            fn methods(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>, data: v8::Local<v8::Value>) {
                crate::methods!(scope, obj, data = data; $($name => $f,)* "close" => close);
            }
        };
    }

    natives! {
        "focusIn" => fn focus_in(_, _, ime) done!(ime.focus_in());
        "focusOut" => fn focus_out(_, _, ime) done!(ime.focus_out());
        "reset" => fn reset(_, _, ime) done!(ime.reset());
        "setCursorRect" => fn set_cursor_rect(s, a, ime) {
            let [x, y, w, h] = [0, 1, 2, 3].map(|i| bind::arg_f64(s, a, i) as i32);
            done!(ime.set_cursor_rect(x, y, w, h))
        };
        "setSurroundingText" => fn set_surrounding_text(s, a, ime) {
            let text = bind::arg_string(s, a, 0);
            done!(ime.set_surrounding_text(text, bind::arg_f64(s, a, 1) as u32, bind::arg_f64(s, a, 2) as u32))
        };
        "setCapability" => fn set_capability(s, a, ime) {
            let obj = v8::Local::<v8::Object>::try_from(a.get(0)).ok();
            let mut flag = |name: &str| obj.and_then(|o| bind::get(s, o, name)).is_some_and(|v| v.boolean_value(s));
            let capability = crate::runtime::ImeCapability {
                preedit: flag("preedit"),
                surrounding_text: flag("surroundingText"),
                password: flag("password"),
                client_panel: flag("clientPanel"),
            };
            done!(ime.set_capability(capability))
        };
        "processKey" => fn process_key(s, a, ime) {
            let [keysym, keycode, state] = [0, 1, 2].map(|i| bind::arg_f64(s, a, i) as u32);
            ime.process_key(keysym, keycode, state, bind::arg_bool(s, a, 3)).map(Some)
        };
        "selectCandidate" => fn select_candidate(s, a, ime) done!(ime.select_candidate(bind::arg_f64(s, a, 0) as i32));
        "prevPage" => fn prev_page(_, _, ime) done!(ime.prev_page());
        "nextPage" => fn next_page(_, _, ime) done!(ime.next_page());
    }

    fn call(
        scope: &mut v8::PinScope,
        args: &v8::FunctionCallbackArguments,
        mut rv: v8::ReturnValue,
        f: impl FnOnce(&mut v8::PinScope, &v8::FunctionCallbackArguments, &crate::runtime::Ime) -> Answer,
    ) {
        let id = args.data().uint32_value(scope).unwrap_or(0);
        let result = STATE.with(|s| s.borrow().imes.get(&id).map(|(ime, _)| f(scope, args, ime)));
        match result {
            None => bind::throw_error(scope, "Ime: closed"),
            Some(Err(e)) => bind::throw_error(scope, &e),
            Some(Ok(Some(value))) => rv.set_bool(value),
            Some(Ok(None)) => {}
        }
    }

    fn close(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
        let id = args.data().uint32_value(scope).unwrap_or(0);
        let Some((ime, callback)) = STATE.with(|s| s.borrow_mut().imes.remove(&id)) else { return };
        let closed: Answer = done!(ime.close());
        drop((ime, callback));
        if let Err(e) = closed {
            bind::throw_error(scope, &e);
        }
    }
}
