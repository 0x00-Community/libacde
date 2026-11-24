use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use crate::webgl::{ActiveInfo, Attributes, ClearColor, Context, Param, Setter, TexSource, Value};

use crate::host::bind;
use crate::host::rt::{self, Msg};

struct Entry {
    gl: Context,
    node: u32,
}

#[derive(Default)]
struct State {
    contexts: bind::Slab<Entry>,
    lost: Option<v8::Global<v8::Function>>,
    listener: Option<(std::sync::Arc<crate::gpu::Gpu>, u64)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn clear() {
    crate::host::gpuwatch::clear();
    let state = STATE.with(|s| std::mem::take(&mut *s.borrow_mut()));
    if let Some((gpu, id)) = &state.listener {
        gpu.forget_lost(*id);
    }
    drop(state);
}

enum Ret {
    Undefined,
    Null,
    Num(f64),
    Bool(bool),
    Str(String),
    Strs(Vec<String>),
    Ids(Vec<u32>),
    Param(Param),
    Info(ActiveInfo),
    Pixels(u32, u32, Vec<u8>),
    Uniform(u32, Value),
    Block(String, u32, u32, u32, Vec<u32>, bool, bool),
    Member(i32, u32, u32, u32, u32, u32),
    Attrib(crate::webgl::objects::Attrib, crate::webgl::state::Generic),
}

impl From<()> for Ret {
    fn from(_: ()) -> Self {
        Ret::Undefined
    }
}

struct A<'a, 's, 'i> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    args: &'a v8::FunctionCallbackArguments<'s>,
}

impl<'s> A<'_, 's, '_> {
    fn value(&self, i: i32) -> v8::Local<'s, v8::Value> {
        self.args.get(i)
    }
    fn f(&mut self, i: i32) -> f64 {
        self.args.get(i).number_value(self.scope).unwrap_or(0.0)
    }
    fn u(&mut self, i: i32) -> u32 {
        let n = self.f(i);
        if n.is_finite() { n as i64 as u32 } else { 0 }
    }
    fn i(&mut self, i: i32) -> i32 {
        let n = self.f(i);
        if n.is_finite() { n.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32 } else { 0 }
    }
    fn i64(&mut self, i: i32) -> i64 {
        let n = self.f(i);
        if n.is_finite() { n as i64 } else { 0 }
    }
    fn b(&mut self, i: i32) -> bool {
        self.args.get(i).boolean_value(self.scope)
    }
    fn id(&mut self, i: i32) -> Option<u32> {
        let v = self.u(i);
        (v != 0).then_some(v)
    }
    fn s(&mut self, i: i32) -> String {
        bind::arg_string(self.scope, self.args, i)
    }
    fn bytes(&self, i: i32) -> Option<Vec<u8>> {
        let v = self.args.get(i);
        if v.is_null_or_undefined() {
            return None;
        }
        bind::bytes(v)
    }
}

fn finish(scope: &mut v8::PinScope, rv: &mut v8::ReturnValue, ret: Ret) {
    let value: v8::Local<v8::Value> = match ret {
        Ret::Undefined => return,
        Ret::Null => v8::null(scope).into(),
        Ret::Num(n) => v8::Number::new(scope, n).into(),
        Ret::Bool(b) => v8::Boolean::new(scope, b).into(),
        Ret::Str(s) => bind::string(scope, &s).into(),
        Ret::Strs(list) => {
            let items: Vec<v8::Local<v8::Value>> = list.iter().map(|s| bind::string(scope, s).into()).collect();
            v8::Array::new_with_elements(scope, &items).into()
        }
        Ret::Ids(ids) => {
            let items: Vec<v8::Local<v8::Value>> = ids.iter().map(|i| v8::Number::new(scope, f64::from(*i)).into()).collect();
            v8::Array::new_with_elements(scope, &items).into()
        }
        Ret::Param(p) => param(scope, p),
        Ret::Info(info) => crate::js_object!(scope, { "name" => info.name, "size" => info.size, "type" => info.ty }).into(),
        Ret::Pixels(width, height, rgba) => {
            let rgba = bind::uint8_array(scope, rgba);
            crate::js_object!(scope, { "width" => width, "height" => height, "rgba" => rgba }).into()
        }
        Ret::Uniform(ty, v) => {
            let value = match v {
                Value::Floats(f) => param(scope, Param::Floats(f)),
                Value::Ints(i) => param(scope, Param::Ints(i)),
                Value::Uints(u) => param(scope, Param::Uints(u)),
                Value::Bools(b) => param(scope, Param::Bools(b)),
            };
            crate::js_object!(scope, { "type" => ty, "value" => value }).into()
        }
        Ret::Block(name, size, binding, count, indices, vertex, fragment) => {
            let indices = bind::uint32_array(scope, &indices);
            crate::js_object!(scope, { "name" => name, "size" => size, "binding" => binding, "count" => count, "indices" => indices, "vertex" => vertex, "fragment" => fragment }).into()
        }
        Ret::Member(block, ty, size, offset, array, matrix) => {
            crate::js_object!(scope, { "block" => block, "type" => ty, "size" => size, "offset" => if offset == u32::MAX { -1i32 } else { offset as i32 }, "arrayStride" => array, "matrixStride" => matrix }).into()
        }
        Ret::Attrib(a, g) => {
            let generic = match g {
                crate::webgl::state::Generic::Float(v) => param(scope, Param::Floats(v.to_vec())),
                crate::webgl::state::Generic::Int(v) => param(scope, Param::Ints(v.to_vec())),
                crate::webgl::state::Generic::Uint(v) => param(scope, Param::Uints(v.to_vec())),
            };
            crate::js_object!(scope, {
                "enabled" => a.enabled, "buffer" => a.buffer.unwrap_or(0), "size" => a.size, "type" => a.ty, "normalized" => a.normalized,
                "integer" => a.integer, "stride" => a.stride, "offset" => a.offset as f64, "divisor" => a.divisor, "current" => generic,
            })
            .into()
        }
    };
    rv.set(value);
}

fn param<'s>(scope: &mut v8::PinScope<'s, '_>, p: Param) -> v8::Local<'s, v8::Value> {
    match p {
        Param::Null => v8::null(scope).into(),
        Param::Bool(b) => v8::Boolean::new(scope, b).into(),
        Param::Int(i) => v8::Number::new(scope, i as f64).into(),
        Param::Float(f) => v8::Number::new(scope, f).into(),
        Param::Str(s) => bind::string(scope, &s).into(),
        Param::Ints(v) => {
            let u: Vec<u32> = v.iter().map(|x| *x as u32).collect();
            let array = bind::uint32_array(scope, &u);
            let buffer = array.buffer(scope).unwrap_or_else(|| v8::ArrayBuffer::new(scope, 0));
            v8::Int32Array::new(scope, buffer, 0, v.len()).map_or_else(|| v8::null(scope).into(), Into::into)
        }
        Param::Uints(v) => bind::uint32_array(scope, &v).into(),
        Param::Floats(v) => bind::float32_array(scope, &v).into(),
        Param::Bools(v) => {
            let items: Vec<v8::Local<v8::Value>> = v.iter().map(|b| v8::Boolean::new(scope, *b).into()).collect();
            v8::Array::new_with_elements(scope, &items).into()
        }
        Param::Object(kind, id) => crate::js_object!(scope, { "kind" => kind, "id" => id }).into(),
    }
}

fn with<R>(id: u32, f: impl FnOnce(&mut Entry) -> R) -> Option<R> {
    STATE.with(|s| s.borrow_mut().contexts.get_mut(&id).map(f))
}

macro_rules! gl {
    ($($js:literal => |$a:ident, $c:ident| $body:expr;)*) => {
        pub(crate) fn methods(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>) {
            $(
                {
                    fn native<'s>(scope: &mut v8::PinScope<'s, '_>, args: v8::FunctionCallbackArguments<'s>, mut rv: v8::ReturnValue) {
                        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                            #[allow(unused_mut)]
                            let mut $a = A { scope: &mut *scope, args: &args };
                            let context = $a.u(0);
                            STATE.with(|s| {
                                let mut state = s.borrow_mut();
                                let Some(entry) = state.contexts.get_mut(&context) else { return Ret::Null };
                                if entry.gl.lost {
                                    return Ret::Null;
                                }
                                #[allow(unused_variables)]
                                let $c = &mut entry.gl;
                                Ret::from($body)
                            })
                        }));
                        match result {
                            Ok(ret) => finish(scope, &mut rv, ret),
                            Err(panic) => {
                                let message = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                                log::error!("webgl {}: {message}", $js);
                            }
                        }
                    }
                    crate::host::bind::method(scope, obj, $js, native);
                }
            )*
        }
    };
}

impl From<u32> for Ret {
    fn from(n: u32) -> Self {
        Ret::Num(f64::from(n))
    }
}
impl From<i32> for Ret {
    fn from(n: i32) -> Self {
        Ret::Num(f64::from(n))
    }
}
impl From<f64> for Ret {
    fn from(n: f64) -> Self {
        Ret::Num(n)
    }
}
impl From<bool> for Ret {
    fn from(b: bool) -> Self {
        Ret::Bool(b)
    }
}
impl From<Option<u32>> for Ret {
    fn from(n: Option<u32>) -> Self {
        n.map_or(Ret::Null, Ret::from)
    }
}
impl From<Option<String>> for Ret {
    fn from(s: Option<String>) -> Self {
        s.map_or(Ret::Null, Ret::Str)
    }
}
impl From<Option<f32>> for Ret {
    fn from(n: Option<f32>) -> Self {
        n.map_or(Ret::Null, |v| Ret::Num(f64::from(v)))
    }
}
impl From<Param> for Ret {
    fn from(p: Param) -> Self {
        Ret::Param(p)
    }
}
impl From<Option<ActiveInfo>> for Ret {
    fn from(i: Option<ActiveInfo>) -> Self {
        i.map_or(Ret::Null, Ret::Info)
    }
}
impl From<Vec<u32>> for Ret {
    fn from(v: Vec<u32>) -> Self {
        Ret::Ids(v)
    }
}

fn setter(kind: u32, n: u32, m: u32) -> Setter {
    match kind {
        0 => Setter::Float(n),
        1 => Setter::Int(n),
        2 => Setter::Uint(n),
        _ => Setter::Matrix(n, m),
    }
}

fn clear_color(kind: u32, v: &[u8]) -> ClearColor {
    let word = |i: usize| v.get(i * 4..i * 4 + 4).map_or([0; 4], |b| [b[0], b[1], b[2], b[3]]);
    match kind {
        1 => ClearColor::Int([0, 1, 2, 3].map(|i| i32::from_le_bytes(word(i)))),
        2 => ClearColor::Uint([0, 1, 2, 3].map(|i| u32::from_le_bytes(word(i)))),
        _ => ClearColor::Float([0, 1, 2, 3].map(|i| f32::from_le_bytes(word(i)))),
    }
}

fn source_of(node: u32, contexts: &mut bind::Slab<Entry>) -> Option<TexSource> {
    if let Some(entry) = contexts.values_mut().find(|e| e.node == node) {
        if entry.gl.drawing_dirty() {
            present_entry(entry);
        }
        return entry.gl.front().map(TexSource::Gpu);
    }
    match crate::host::dom::with_dom(|dom| dom.image_pixels(node)).and_then(Result::ok).flatten()? {
        crate::render::Pixels::Cpu { width, height, rgba } => Some(TexSource::Cpu { width, height, rgba }),
        crate::render::Pixels::Gpu(image) => Some(TexSource::Gpu(image)),
    }
}

fn present_entry(entry: &mut Entry) -> bool {
    let Some(image) = entry.gl.present() else { return false };
    let (node, data, tx) = (entry.node, image.image(), rt::current().sender());
    entry.gl.after_submitted(Box::new(move || {
        let show = move |_: &mut v8::PinScope| {
            if let Some(Err(e)) = crate::host::dom::with_dom(|dom| dom.canvas_image(node, Some(data))) {
                log::warn!("webgl: showing the canvas: {e}");
            }
        };
        _ = tx.send(Msg::Task(Box::new(show)));
    }));
    true
}

fn create(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let node = bind::arg_u32(scope, &args, 0);
    let webgl2 = bind::arg_bool(scope, &args, 1);
    let options = v8::Local::<v8::Object>::try_from(args.get(2)).ok();
    let flag = |scope: &mut v8::PinScope, name: &str, default: bool| {
        options.and_then(|o| bind::get(scope, o, name)).filter(|v| !v.is_undefined()).map_or(default, |v| v.boolean_value(scope))
    };
    let attrs = Attributes {
        alpha: flag(scope, "alpha", true),
        depth: flag(scope, "depth", true),
        stencil: flag(scope, "stencil", false),
        antialias: flag(scope, "antialias", true),
        premultiplied_alpha: flag(scope, "premultipliedAlpha", true),
        preserve_drawing_buffer: flag(scope, "preserveDrawingBuffer", false),
    };
    let (width, height) = (bind::arg_u32(scope, &args, 3), bind::arg_u32(scope, &args, 4));
    let Some(gpu) = crate::gpu::Gpu::shared() else {
        rv.set_null();
        return;
    };
    if gpu.gone() {
        rv.set_null();
        return;
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| Context::new(gpu.clone(), webgl2, attrs, width, height)));
    let Ok(gl) = result else {
        rv.set_null();
        return;
    };
    let id = STATE.with(|s| {
        let mut s = s.borrow_mut();
        // contexts made after a device was replaced listen to the new device
        if s.listener.as_ref().is_some_and(|(held, _)| !Arc::ptr_eq(held, &gpu)) {
            if let Some((held, id)) = s.listener.take() {
                held.forget_lost(id);
            }
        }
        if s.listener.is_none() {
            let tx = rt::current().sender();
            let listener = gpu.on_lost(move |_| {
                _ = tx.send(Msg::Task(Box::new(all_lost)));
            });
            s.listener = Some((gpu.clone(), listener));
        }
        s.contexts.add(Entry { gl, node })
    });
    rv.set_double(f64::from(id));
}

fn all_lost(scope: &mut v8::PinScope) {
    let (lost, ids) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let ids: Vec<u32> = s
            .contexts
            .iter_mut()
            .filter(|(_, e)| !e.gl.lost)
            .map(|(id, e)| {
                e.gl.lose();
                *id
            })
            .collect();
        (s.lost.clone(), ids)
    });
    let Some(lost) = lost else { return };
    for id in ids {
        let id = v8::Number::new(scope, f64::from(id));
        bind::call(scope, lost.clone(), &[id.into()]);
    }
}

fn watch(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    if let Ok(f) = v8::Local::<v8::Function>::try_from(args.get(0)) {
        crate::host::gpuwatch::watch(v8::Global::new(scope, f));
    }
}

fn on_lost(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    if let Ok(f) = v8::Local::<v8::Function>::try_from(args.get(0)) {
        let f = v8::Global::new(scope, f);
        STATE.with(|s| s.borrow_mut().lost = Some(f));
    }
}

fn release(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_u32(scope, &args, 0);
    let entry = STATE.with(|s| s.borrow_mut().contexts.remove(&id));
    drop(entry);
}

fn present(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_u32(scope, &args, 0);
    let shown = std::panic::catch_unwind(AssertUnwindSafe(|| with(id, |e| !e.gl.lost && present_entry(e)).unwrap_or(false))).unwrap_or(false);
    rv.set_bool(shown);
}

fn done(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Some(gpu) = crate::gpu::Gpu::shared().filter(|gpu| !gpu.gone()) else { return };
    let frame = gpu.frame();
    crate::webgl::submit::when_done(gpu, Box::new(move || frame.store(true, std::sync::atomic::Ordering::Release)));
}

fn in_flight(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    rv.set_uint32(crate::gpu::Gpu::frames_in_flight(bind::arg_u32(scope, &args, 0) as usize) as u32);
}

fn pixels(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_u32(scope, &args, 0);
    let read = std::panic::catch_unwind(AssertUnwindSafe(|| {
        with(id, |e| {
            if e.gl.drawing_dirty() {
                present_entry(e);
            }
            e.gl.front_pixels()
        })
        .flatten()
    }))
    .ok()
    .flatten();
    match read {
        Some((w, h, rgba)) => finish(scope, &mut rv, Ret::Pixels(w, h, rgba)),
        None => rv.set_null(),
    }
}

fn png(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_u32(scope, &args, 0);
    let read = std::panic::catch_unwind(AssertUnwindSafe(|| {
        with(id, |e| {
            if e.gl.drawing_dirty() {
                present_entry(e);
            }
            e.gl.front_pixels()
        })
        .flatten()
    }))
    .ok()
    .flatten();
    match read.and_then(|(w, h, rgba)| crate::render::encode_png(w, h, &rgba).map(|png| (w, h, png))) {
        Some((w, h, png)) => finish(scope, &mut rv, Ret::Pixels(w, h, png)),
        None => rv.set_null(),
    }
}

fn tex_image_source<'s>(scope: &mut v8::PinScope<'s, '_>, args: v8::FunctionCallbackArguments<'s>, _: v8::ReturnValue) {
    let mut a = A { scope, args: &args };
    let (context, target, level, internal, format, ty) = (a.u(0), a.u(1), a.i(2), a.u(3), a.u(4), a.u(5));
    let kind = a.u(6);
    let source = match kind {
        0 | 2 | 3 => Some(a.u(7)),
        _ => None,
    };
    let bitmap = matches!(kind, 2 | 3).then_some(kind == 3);
    let pixels = (kind == 1).then(|| (a.u(7), a.u(8), a.bytes(9).unwrap_or_default()));
    let sub = (!a.value(10).is_null_or_undefined()).then(|| [a.i(10), a.i(11), a.i(12)]);
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            let source = match (source, pixels) {
                (Some(node), _) => source_of(node, &mut s.contexts),
                (_, Some((w, h, rgba))) => Some(TexSource::Cpu { width: w, height: h, rgba }),
                _ => None,
            };
            let Some(entry) = s.contexts.get_mut(&context) else { return };
            match source {
                Some(src) => entry.gl.tex_image_source(target, level, internal, format, ty, src, sub, bitmap),
                None => entry.gl.error(crate::webgl::consts::INVALID_VALUE),
            }
        })
    }));
}

pub fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>) {
    let webgl = v8::Object::new(scope);
    crate::methods!(scope, webgl,
        "create" => create,
        "release" => release,
        "present" => present,
        "done" => done,
        "inFlight" => in_flight,
        "pixels" => pixels,
        "png" => png,
        "onLost" => on_lost,
        "watch" => watch,
        "texImageSource" => tex_image_source,
    );
    methods(scope, webgl);
    bind::set(scope, natives, "webgl", webgl.into());
}

gl! {
    "resize" => |a, c| { let (w, h) = (a.u(1), a.u(2)); c.resize(w, h) };
    "drawingSize" => |a, c| { let (w, h) = c.drawing_size(); Ret::Ids(vec![w, h]) };
    "supported" => |a, c| Ret::Strs(c.supported_extensions().into_iter().map(String::from).collect());
    "enableExtension" => |a, c| { let name = a.s(1); c.enable_extension(&name) };
    "loseContext" => |a, c| c.lose();
    "getError" => |a, c| c.get_error();
    "enable" => |a, c| { let (cap, on) = (a.u(1), a.b(2)); c.enable(cap, on) };
    "isEnabled" => |a, c| { let cap = a.u(1); c.is_enabled(cap) };
    "viewport" => |a, c| { let v = [a.i(1), a.i(2), a.i(3), a.i(4)]; c.viewport(v[0], v[1], v[2], v[3]) };
    "scissor" => |a, c| { let v = [a.i(1), a.i(2), a.i(3), a.i(4)]; c.scissor(v[0], v[1], v[2], v[3]) };
    "clearColor" => |a, c| { let v = [a.f(1), a.f(2), a.f(3), a.f(4)].map(|x| x as f32); c.clear_color(v[0], v[1], v[2], v[3]) };
    "clearDepth" => |a, c| { let d = a.f(1) as f32; c.clear_depth(d) };
    "clearStencil" => |a, c| { let s = a.i(1); c.clear_stencil(s) };
    "colorMask" => |a, c| { let v = [a.b(1), a.b(2), a.b(3), a.b(4)]; c.color_mask(v[0], v[1], v[2], v[3]) };
    "depthMask" => |a, c| { let v = a.b(1); c.depth_mask(v) };
    "depthFunc" => |a, c| { let v = a.u(1); c.depth_func(v) };
    "depthRange" => |a, c| { let (n, f) = (a.f(1) as f32, a.f(2) as f32); c.depth_range(n, f) };
    "blendColor" => |a, c| { let v = [a.f(1), a.f(2), a.f(3), a.f(4)].map(|x| x as f32); c.blend_color(v[0], v[1], v[2], v[3]) };
    "blendEquation" => |a, c| { let (rgb, alpha) = (a.u(1), a.u(2)); c.blend_equation(rgb, alpha) };
    "blendFunc" => |a, c| { let v = [a.u(1), a.u(2), a.u(3), a.u(4)]; c.blend_func(v[0], v[1], v[2], v[3]) };
    "stencilFunc" => |a, c| { let (face, func, r, m) = (a.u(1), a.u(2), a.i(3), a.u(4)); c.stencil_func(face, func, r, m) };
    "stencilMask" => |a, c| { let (face, m) = (a.u(1), a.u(2)); c.stencil_mask(face, m) };
    "stencilOp" => |a, c| { let v = [a.u(1), a.u(2), a.u(3), a.u(4)]; c.stencil_op(v[0], v[1], v[2], v[3]) };
    "cullFace" => |a, c| { let v = a.u(1); c.cull_face(v) };
    "frontFace" => |a, c| { let v = a.u(1); c.front_face(v) };
    "lineWidth" => |a, c| { let v = a.f(1) as f32; c.line_width(v) };
    "polygonOffset" => |a, c| { let (f, u) = (a.f(1) as f32, a.f(2) as f32); c.polygon_offset(f, u) };
    "sampleCoverage" => |a, c| { let (v, i) = (a.f(1) as f32, a.b(2)); c.sample_coverage(v, i) };
    "hint" => |a, c| { let (t, m) = (a.u(1), a.u(2)); c.hint(t, m) };
    "pixelStorei" => |a, c| { let (p, v) = (a.u(1), a.i(2)); c.pixel_store(p, v) };
    "activeTexture" => |a, c| { let u = a.u(1); c.active_texture(u) };
    "exists" => |a, c| { let id = a.u(1); c.exists(id) };
    "createBuffer" => |a, c| c.create_buffer();
    "deleteBuffer" => |a, c| { let id = a.u(1); c.delete_buffer(id) };
    "bindBuffer" => |a, c| { let (t, id) = (a.u(1), a.id(2)); c.bind_buffer(t, id) };
    "bindBufferRange" => |a, c| { let (t, i, id, off, size) = (a.u(1), a.u(2), a.id(3), a.i64(4), a.i64(5)); c.bind_buffer_range(t, i, id, off.max(0) as u64, (size >= 0).then_some(size as u64)) };
    "bufferData" => |a, c| { let (t, size, usage) = (a.u(1), a.i64(3), a.u(4)); let data = a.bytes(2); c.buffer_data(t, data.as_deref(), size.max(0) as u64, usage) };
    "bufferSubData" => |a, c| { let (t, off) = (a.u(1), a.i64(2)); match a.bytes(3) { Some(d) if off >= 0 => c.buffer_sub_data(t, off as u64, &d), _ => c.error(crate::webgl::consts::INVALID_VALUE) } };
    "copyBufferSubData" => |a, c| { let v = [a.i64(3), a.i64(4), a.i64(5)]; let (r, w) = (a.u(1), a.u(2)); if v.iter().any(|x| *x < 0) { c.error(crate::webgl::consts::INVALID_VALUE) } else { c.copy_buffer_sub_data(r, w, v[0] as u64, v[1] as u64, v[2] as u64) } };
    "getBufferSubData" => |a, c| {
        let (t, off) = (a.u(1), a.i64(2));
        let view = v8::Local::<v8::ArrayBufferView>::try_from(a.value(3)).ok();
        match view {
            Some(view) if off >= 0 => {
                let len = view.byte_length();
                let mut out = vec![0u8; len];
                c.get_buffer_sub_data(t, off as u64, &mut out);
                if let Some(buffer) = view.buffer(a.scope) { if let Some(data) = buffer.data() { unsafe { std::ptr::copy_nonoverlapping(out.as_ptr(), (data.as_ptr() as *mut u8).add(view.byte_offset()), len) } } }
            }
            _ => c.error(crate::webgl::consts::INVALID_VALUE),
        }
    };
    "createTexture" => |a, c| c.create_texture();
    "deleteTexture" => |a, c| { let id = a.u(1); c.delete_texture(id) };
    "bindTexture" => |a, c| { let (t, id) = (a.u(1), a.id(2)); c.bind_texture(t, id) };
    "texParameter" => |a, c| { let (t, p, v) = (a.u(1), a.u(2), a.f(3) as f32); c.tex_parameter(t, p, v) };
    "getTexParameter" => |a, c| { let (t, p) = (a.u(1), a.u(2)); c.get_tex_parameter(t, p) };
    "texImage" => |a, c| { let v = [a.i(2), a.i(4), a.i(5), a.i(6), a.i(7)]; let (t, internal, f, ty) = (a.u(1), a.u(3), a.u(8), a.u(9)); let data = a.bytes(10); c.tex_image(t, v[0], internal, v[1], v[2], v[3], v[4], f, ty, data.as_deref()) };
    "texSubImage" => |a, c| { let v = [a.i(2), a.i(3), a.i(4), a.i(5), a.i(6), a.i(7), a.i(8)]; let (t, f, ty) = (a.u(1), a.u(9), a.u(10)); let data = a.bytes(11); c.tex_sub_image(t, v[0], v[1], v[2], v[3], v[4], v[5], v[6], f, ty, data.as_deref()) };
    "texStorage" => |a, c| { let (t, l, i, w, h, d) = (a.u(1), a.i(2), a.u(3), a.i(4), a.i(5), a.i(6)); c.tex_storage(t, l, i, w, h, d) };
    "compressedTexImage" => |a, c| { let (t, l, i, w, h, d) = (a.u(1), a.i(2), a.u(3), a.i(4), a.i(5), a.i(6)); let data = a.bytes(7).unwrap_or_default(); let sub = (!a.value(8).is_null_or_undefined()).then(|| [a.i(8), a.i(9), a.i(10)]); c.compressed_tex_image(t, l, i, w, h, d, &data, sub) };
    "copyTexImage" => |a, c| { let (t, l, i) = (a.u(1), a.i(2), a.u(3)); let d = [a.i(4), a.i(5), a.i(6)]; let s = [a.i(7), a.i(8), a.i(9), a.i(10)]; c.copy_tex_image(t, l, (i != 0).then_some(i), d, s) };
    "generateMipmap" => |a, c| { let t = a.u(1); c.generate_mipmap(t) };
    "createSampler" => |a, c| c.create_sampler();
    "deleteSampler" => |a, c| { let id = a.u(1); c.delete_sampler(id) };
    "bindSampler" => |a, c| { let (u, id) = (a.u(1), a.id(2)); c.bind_sampler(u, id) };
    "samplerParameter" => |a, c| { let (s, p, v) = (a.u(1), a.u(2), a.f(3) as f32); c.sampler_parameter(s, p, v) };
    "getSamplerParameter" => |a, c| { let (s, p) = (a.u(1), a.u(2)); c.get_sampler_parameter(s, p) };
    "createFramebuffer" => |a, c| c.create_framebuffer();
    "deleteFramebuffer" => |a, c| { let id = a.u(1); c.delete_framebuffer(id) };
    "bindFramebuffer" => |a, c| { let (t, id) = (a.u(1), a.id(2)); c.bind_framebuffer(t, id) };
    "createRenderbuffer" => |a, c| c.create_renderbuffer();
    "deleteRenderbuffer" => |a, c| { let id = a.u(1); c.delete_renderbuffer(id) };
    "bindRenderbuffer" => |a, c| { let (t, id) = (a.u(1), a.id(2)); c.bind_renderbuffer(t, id) };
    "renderbufferStorage" => |a, c| { let (t, s, i, w, h) = (a.u(1), a.i(2), a.u(3), a.i(4), a.i(5)); c.renderbuffer_storage(t, s, i, w, h) };
    "framebufferRenderbuffer" => |a, c| { let (t, at, rt, id) = (a.u(1), a.u(2), a.u(3), a.id(4)); c.framebuffer_renderbuffer(t, at, rt, id) };
    "framebufferTexture" => |a, c| { let (t, at, tt, id, l, layer) = (a.u(1), a.u(2), a.u(3), a.id(4), a.i(5), a.i(6)); c.framebuffer_texture(t, at, tt, id, l, layer) };
    "checkFramebufferStatus" => |a, c| { let t = a.u(1); c.check_framebuffer_status(t) };
    "drawBuffers" => |a, c| { let list = a.bytes(1).unwrap_or_default(); let v: Vec<u32> = list.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(); c.draw_buffers(&v) };
    "readBuffer" => |a, c| { let s = a.u(1); c.read_buffer(s) };
    "blitFramebuffer" => |a, c| { let s = [a.i(1), a.i(2), a.i(3), a.i(4)]; let d = [a.i(5), a.i(6), a.i(7), a.i(8)]; let (m, f) = (a.u(9), a.u(10)); c.blit_framebuffer(s, d, m, f) };
    "createShader" => |a, c| { let t = a.u(1); c.create_shader(t) };
    "shaderSource" => |a, c| { let (id, src) = (a.u(1), a.s(2)); c.shader_source(id, &src) };
    "getShaderSource" => |a, c| { let id = a.u(1); c.get_shader_source(id) };
    "compileShader" => |a, c| { let id = a.u(1); c.compile_shader(id) };
    "shaderInfoLog" => |a, c| { let id = a.u(1); c.shader_info_log(id) };
    "deleteShader" => |a, c| { let id = a.u(1); c.delete_shader(id) };
    "createProgram" => |a, c| c.create_program();
    "attachShader" => |a, c| { let (p, s) = (a.u(1), a.u(2)); c.attach_shader(p, s) };
    "detachShader" => |a, c| { let (p, s) = (a.u(1), a.u(2)); c.detach_shader(p, s) };
    "attachedShaders" => |a, c| { let p = a.u(1); c.attached_shaders(p) };
    "bindAttribLocation" => |a, c| { let (p, i, n) = (a.u(1), a.u(2), a.s(3)); c.bind_attrib_location(p, i, &n) };
    "linkProgram" => |a, c| { let p = a.u(1); c.link_program(p) };
    "programInfoLog" => |a, c| { let p = a.u(1); c.program_info_log(p) };
    "useProgram" => |a, c| { let p = a.id(1); c.use_program(p); c.current_program() == p };
    "activeAttribCount" => |a, c| { let p = a.u(1); c.active_attrib_count(p) };
    "error" => |a, c| { let e = a.u(1); c.error(e) };
    "deleteProgram" => |a, c| { let p = a.u(1); c.delete_program(p) };
    "validateProgram" => |a, c| { let p = a.u(1); c.validate_program(p) };
    "programSerial" => |a, c| { let p = a.u(1); c.program_serial(p) as f64 };
    "attribLocation" => |a, c| { let (p, n) = (a.u(1), a.s(2)); c.attrib_location(p, &n) };
    "activeAttrib" => |a, c| { let (p, i) = (a.u(1), a.u(2)); c.active_attrib(p, i) };
    "activeUniform" => |a, c| { let (p, i) = (a.u(1), a.u(2)); c.active_uniform(p, i) };
    "activeUniformCount" => |a, c| { let p = a.u(1); c.active_uniform_count(p) };
    "uniformLocation" => |a, c| { let (p, n) = (a.u(1), a.s(2)); c.uniform_location(p, &n).map_or(-1, |l| l as i32) };
    "uniform" => |a, c| {
        let (program, location, serial, kind, n, m, transpose) = (a.id(1), a.u(2), a.f(3) as u64, a.u(4), a.u(5), a.u(6), a.b(7));
        let data = a.bytes(8).unwrap_or_default();
        c.uniform(program, location, serial, setter(kind, n, m), transpose, &data)
    };
    "getUniform" => |a, c| { let (p, l, s) = (a.u(1), a.u(2), a.f(3) as u64); match c.get_uniform(p, l, s) { Some((t, v)) => Ret::Uniform(t, v), None => Ret::Null } };
    "uniformBlockIndex" => |a, c| { let (p, n) = (a.u(1), a.s(2)); c.uniform_block_index(p, &n) };
    "uniformBlockBinding" => |a, c| { let (p, i, b) = (a.u(1), a.u(2), a.u(3)); c.uniform_block_binding(p, i, b) };
    "blockInfo" => |a, c| { let (p, i) = (a.u(1), a.u(2)); match c.block_info(p, i) { Some(b) => Ret::Block(b.0, b.1, b.2, b.3, b.4, b.5, b.6), None => Ret::Null } };
    "blockMember" => |a, c| { let (p, i) = (a.u(1), a.u(2)); match c.block_member(p, i) { Some(m) => Ret::Member(m.0, m.1, m.2, m.3, m.4, m.5), None => Ret::Null } };
    "uniformIndices" => |a, c| { let p = a.u(1); let names = bind::strings(a.scope, a.value(2)).unwrap_or_default(); c.uniform_indices(p, &names).map_or(Ret::Null, Ret::Ids) };
    "fragDataLocation" => |a, c| { let (p, n) = (a.u(1), a.s(2)); c.frag_data_location(p, &n) };
    "transformFeedbackVaryings" => |a, c| { let p = a.u(1); let names = bind::strings(a.scope, a.value(2)).unwrap_or_default(); let m = a.u(3); c.transform_feedback_varyings(p, names, m) };
    "createVertexArray" => |a, c| c.create_vertex_array();
    "deleteVertexArray" => |a, c| { let id = a.u(1); c.delete_vertex_array(id) };
    "bindVertexArray" => |a, c| { let id = a.id(1); c.bind_vertex_array(id) };
    "isVertexArray" => |a, c| { let id = a.u(1); c.is_vertex_array(id) };
    "vertexAttribPointer" => |a, c| { let (i, s, t, n, st, o, int) = (a.u(1), a.i(2), a.u(3), a.b(4), a.i(5), a.i64(6), a.b(7)); c.vertex_attrib_pointer(i, s, t, n, st, o, int) };
    "enableVertexAttrib" => |a, c| { let (i, on) = (a.u(1), a.b(2)); c.enable_vertex_attrib(i, on) };
    "vertexAttribDivisor" => |a, c| { let (i, d) = (a.u(1), a.u(2)); c.vertex_attrib_divisor(i, d) };
    "vertexAttrib" => |a, c| {
        let (i, kind) = (a.u(1), a.u(2));
        let v = [a.f(3), a.f(4), a.f(5), a.f(6)];
        let g = match kind {
            1 => crate::webgl::state::Generic::Int(v.map(|x| x as i32)),
            2 => crate::webgl::state::Generic::Uint(v.map(|x| x as u32)),
            _ => crate::webgl::state::Generic::Float(v.map(|x| x as f32)),
        };
        c.vertex_attrib(i, g)
    };
    "vertexAttribState" => |a, c| { let i = a.u(1); match c.vertex_attrib_state(i) { Some((at, g)) => Ret::Attrib(at, g), None => Ret::Null } };
    "draw" => |a, c| { let (m, f, n, inst, ty, off) = (a.u(1), a.i(2), a.i(3), a.i(4), a.u(5), a.i64(6)); c.draw(m, f, n, inst, (ty != 0).then_some((ty, off))) };
    "clear" => |a, c| { let m = a.u(1); c.clear(m) };
    "clearBuffer" => |a, c| { let (b, d, kind) = (a.u(1), a.i(2), a.u(3)); let v = a.bytes(4).unwrap_or_default(); let (depth, stencil) = (a.f(5) as f32, a.i(6)); c.clear_buffer(b, d, clear_color(kind, &v), depth, stencil) };
    "readPixels" => |a, c| {
        let (x, y, w, h, f, t) = (a.i(1), a.i(2), a.i(3), a.i(4), a.u(5), a.u(6));
        match v8::Local::<v8::ArrayBufferView>::try_from(a.value(7)) {
            Ok(view) => {
                let len = view.byte_length();
                let offset = view.byte_offset();
                if let Some(buffer) = view.buffer(a.scope) {
                    if let Some(data) = buffer.data() {
                        let out = unsafe { std::slice::from_raw_parts_mut((data.as_ptr() as *mut u8).add(offset), len) };
                        c.read_pixels(x, y, w, h, f, t, out);
                    }
                }
            }
            Err(_) => c.error(crate::webgl::consts::INVALID_VALUE),
        }
    };
    "parameter" => |a, c| { let p = a.u(1); c.parameter(p) };
    "indexedParameter" => |a, c| { let (p, i) = (a.u(1), a.u(2)); c.indexed_parameter(p, i) };
    "bufferParameter" => |a, c| { let (t, p) = (a.u(1), a.u(2)); c.buffer_parameter(t, p) };
    "renderbufferParameter" => |a, c| { let p = a.u(2); c.renderbuffer_parameter(p) };
    "attachmentParameter" => |a, c| { let (t, at, p) = (a.u(1), a.u(2), a.u(3)); c.attachment_parameter(t, at, p) };
    "createQuery" => |a, c| c.create_query();
    "deleteQuery" => |a, c| { let id = a.u(1); c.delete_query(id) };
    "beginQuery" => |a, c| { let (t, id) = (a.u(1), a.u(2)); c.begin_query(t, id) };
    "endQuery" => |a, c| { let t = a.u(1); c.end_query(t) };
    "queryResult" => |a, c| { let (id, p) = (a.u(1), a.u(2)); c.query_result(id, p).map_or(Ret::Null, |v| Ret::Num(v as f64)) };
    "fenceSync" => |a, c| c.fence_sync();
    "deleteSync" => |a, c| { let id = a.u(1); c.delete_sync(id) };
    "syncStatus" => |a, c| { let id = a.u(1); c.sync_status(id).map_or(Ret::Null, Ret::Bool) };
    "createTransformFeedback" => |a, c| c.create_transform_feedback();
    "deleteTransformFeedback" => |a, c| { let id = a.u(1); c.delete_transform_feedback(id) };
    "bindTransformFeedback" => |a, c| { let id = a.id(1); c.bind_transform_feedback(id) };
    "beginTransformFeedback" => |a, c| c.begin_transform_feedback();
    "flush" => |a, c| { c.submit(); };
}
