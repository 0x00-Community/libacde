use v8::{ValueDeserializerHelper, ValueSerializerHelper};

use crate::host::bind;
use crate::host::rt;

pub fn object<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Object> {
    let obj = v8::Object::new(scope);
    crate::methods!(scope, obj,
        "now" => now,
        "timeOrigin" => time_origin,
        "setTimer" => set_timer,
        "clearTimer" => clear_timer,
        "refTimer" => ref_timer,
        "reportError" => report_error,
        "print" => print,
        "log" => log,
        "logEnabled" => log_enabled,
        "clock" => clock,
        "importModule" => import_module,
        "defineModule" => define_module,
        "refuseModule" => refuse_module,
        "setModuleSource" => set_module_source,
        "runModule" => run_module,
        "runScript" => run_script,
        "readAsset" => read_asset,
        "readSourceMap" => read_source_map,
        "liveEdit" => live_edit,
        "inspected" => inspected,
        "devtoolsHandler" => crate::host::inspector::page_handler,
        "devtoolsSend" => crate::host::inspector::page_reply,
        "addImportMap" => add_import_map,
        "encodeUtf8" => encode_utf8,
        "decode" => crate::host::encoding::decode,
        "encodingName" => crate::host::encoding::name,
        "openDecoder" => crate::host::encoding::open,
        "decodeChunk" => crate::host::encoding::chunk,
        "closeDecoder" => crate::host::encoding::close,
        "parseUrl" => parse_url,
        "serialize" => serialize,
        "deserialize" => deserialize,
        "cloneHooks" => clone_hooks,
        "isProxy" => is_proxy,
        "runMicrotasks" => run_microtasks,
        "randomBytes" => random_bytes,
        "randomUUID" => random_uuid,
        "queueMicrotask" => queue_microtask,
        "onTaskEnd" => on_task_end,
        "setReporter" => set_reporter,
    );
    obj
}

fn now(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    rv.set_double(rt::current().now());
}

fn time_origin(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let lp = rt::current();
    let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0);
    rv.set_double(wall - lp.now());
}

fn set_timer(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let ms = bind::arg_f64(scope, &args, 0);
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(1)) else {
        return bind::throw_type_error(scope, "setTimer: callback is not a function");
    };
    let refed = args.get(2).is_undefined() || args.get(2).boolean_value(scope);
    let callback = v8::Global::new(scope, callback);
    let id = rt::current().set_timer(if ms.is_finite() { ms } else { 0.0 }, callback, refed);
    rv.set_uint32(id);
}

fn clear_timer(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_f64(scope, &args, 0);
    if id.is_finite() {
        rt::current().clear_timer(id as u32);
    }
}

fn ref_timer(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_f64(scope, &args, 0);
    let refed = bind::arg_bool(scope, &args, 1);
    if id.is_finite() {
        rt::current().ref_timer(id as u32, refed);
    }
}

fn report_error(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let value = args.get(0);
    let thrown = Some(args.get(1)).filter(|thrown| !thrown.is_undefined()).unwrap_or(value);
    crate::host::inspector::thrown(scope, thrown);
    let origin = rt::origin_of(scope, value);
    let text = rt::describe_at(scope, value, origin.as_deref());
    let lines: Vec<&str> = text.lines().take(10).collect();
    log::error!(target: "js", "[{}] page error: {}", rt::web_name(), lines.join("\n  "));
}

fn print(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let level = bind::arg_f64(scope, &args, 0) as i32;
    let text = bind::arg_string(scope, &args, 1);
    let level = match level {
        1 => log::Level::Warn,
        2 => log::Level::Error,
        3 => log::Level::Debug,
        _ => log::Level::Info,
    };
    log::log!(target: "js", level, "{text}");
}

fn level(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments) -> Option<log::Level> {
    match bind::arg_f64(scope, args, 0) as usize {
        1 => Some(log::Level::Error),
        2 => Some(log::Level::Warn),
        3 => Some(log::Level::Info),
        4 => Some(log::Level::Debug),
        5 => Some(log::Level::Trace),
        _ => None,
    }
}

fn log(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Some(level) = level(scope, &args).filter(|l| *l <= log::max_level()) else { return };
    let target = bind::arg_string(scope, &args, 1);
    let text = bind::arg_string(scope, &args, 2);
    log::log!(target: &target, level, "{text}");
}

fn log_enabled(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let enabled = level(scope, &args).filter(|l| *l <= log::max_level()).is_some_and(|level| {
        let target = bind::arg_string(scope, &args, 1);
        log::log_enabled!(target: &target, level)
    });
    rv.set_bool(enabled);
}

fn clock(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    rv.set_double(crate::log::now_ms());
}

fn import_module(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let url = bind::arg_string(scope, &args, 0);
    if let Some(promise) = crate::host::modules::import(scope, &url) {
        rv.set(promise.into());
    }
}

fn define_module(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let url = bind::arg_string(scope, &args, 0);
    let Ok(exports) = v8::Local::<v8::Object>::try_from(args.get(1)) else {
        return bind::throw_type_error(scope, "defineModule(url, exports): exports is an object");
    };
    if !url.starts_with(crate::host::modules::SCHEME) {
        return bind::throw_type_error(scope, &format!("defineModule: {url} is not an {} module", crate::host::modules::SCHEME));
    }
    crate::host::modules::define(scope, &url, exports);
}

fn set_module_source(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let function = v8::Local::<v8::Function>::try_from(args.get(0)).ok().map(|f| v8::Global::new(scope, f));
    crate::host::modules::set_inline_source(function);
}

fn refuse_module(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let url = bind::arg_string(scope, &args, 0);
    if !url.starts_with(crate::host::modules::SCHEME) {
        return bind::throw_type_error(scope, &format!("refuseModule: {url} is not an {} module", crate::host::modules::SCHEME));
    }
    let reason = bind::arg_string(scope, &args, 1);
    crate::host::modules::refuse(&url, &reason);
}

fn run_module(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let source = bind::arg_string(scope, &args, 0);
    let url = bind::arg_string(scope, &args, 1);
    if let Some(promise) = crate::host::modules::run(scope, &url, &source) {
        rv.set(promise.into());
    }
}

fn run_script(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let source = bind::arg_string(scope, &args, 0);
    let url = bind::arg_string(scope, &args, 1);
    if let Some(value) = bind::run_script(scope, &source, &url) {
        rv.set(value);
    }
}

fn read_asset(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let url = bind::arg_string(scope, &args, 0);
    match crate::host::dom::read_asset(&url) {
        Some(bytes) => rv.set(bind::uint8_array(scope, bytes).into()),
        None => rv.set_null(),
    }
}

fn read_source_map(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let url = bind::arg_string(scope, &args, 0);
    match crate::host::dom::read_source_map(&url) {
        Some(bytes) => rv.set(bind::uint8_array(scope, bytes).into()),
        None => rv.set_null(),
    }
}

fn pairs<'s>(scope: &mut v8::PinScope<'s, '_>, value: v8::Local<'s, v8::Value>) -> Vec<(String, v8::Local<'s, v8::Value>)> {
    let Ok(list) = v8::Local::<v8::Array>::try_from(value) else { return Vec::new() };
    (0..list.length())
        .filter_map(|i| {
            let pair = v8::Local::<v8::Array>::try_from(list.get_index(scope, i)?).ok()?;
            let key = pair.get_index(scope, 0)?.to_rust_string_lossy(scope);
            Some((key, pair.get_index(scope, 1)?))
        })
        .collect()
}

fn strings<'s>(scope: &mut v8::PinScope<'s, '_>, value: v8::Local<'s, v8::Value>) -> Vec<(String, String)> {
    pairs(scope, value).into_iter().map(|(k, v)| (k, v.to_rust_string_lossy(scope))).collect()
}

fn add_import_map<'s>(scope: &mut v8::PinScope<'s, '_>, args: v8::FunctionCallbackArguments<'s>, _: v8::ReturnValue) {
    let imports = strings(scope, args.get(0));
    let scopes = pairs(scope, args.get(1)).into_iter().map(|(prefix, map)| (prefix, strings(scope, map))).collect();
    crate::host::modules::add_import_map(imports, scopes);
}

fn encode_utf8(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let text = bind::arg_string(scope, &args, 0);
    rv.set(bind::uint8_array(scope, text.into_bytes()).into());
}

fn parse_url(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let input = bind::arg_string(scope, &args, 0);
    let base = args.get(1);
    let parsed = match base.is_null_or_undefined() {
        true => url::Url::parse(&input),
        false => {
            let base = base.to_rust_string_lossy(scope);
            url::Url::parse(&base).and_then(|base| base.join(&input))
        }
    };
    let Ok(url) = parsed else {
        return rv.set_null();
    };
    let port = url.port().map(|p| p.to_string()).unwrap_or_default();
    let host = match url.port() {
        Some(port) => format!("{}:{port}", url.host_str().unwrap_or("")),
        None => url.host_str().unwrap_or("").to_owned(),
    };
    let search = url.query().filter(|q| !q.is_empty()).map(|q| format!("?{q}")).unwrap_or_default();
    let hash = url.fragment().filter(|f| !f.is_empty()).map(|f| format!("#{f}")).unwrap_or_default();
    let fields = [
        ("href", url.as_str().to_owned()),
        ("origin", url.origin().ascii_serialization()),
        ("protocol", format!("{}:", url.scheme())),
        ("username", url.username().to_owned()),
        ("password", url.password().unwrap_or("").to_owned()),
        ("host", host),
        ("hostname", url.host_str().unwrap_or("").to_owned()),
        ("port", port),
        ("pathname", url.path().to_owned()),
        ("search", search),
        ("hash", hash),
    ];
    let fields: Vec<(&str, v8::Local<v8::Value>)> = fields.iter().map(|(name, value)| (*name, bind::string(scope, value).into())).collect();
    rv.set(bind::object(scope, &fields).into());
}

struct Clone;

struct Hooks {
    brand: v8::Global<v8::Symbol>,
    revive: v8::Global<v8::Function>,
}

type Placeholder = (v8::Global<v8::Object>, v8::Global<v8::Value>);

thread_local! {
    static HOOKS: std::cell::RefCell<Option<Hooks>> = const { std::cell::RefCell::new(None) };
    static PLACEHOLDERS: std::cell::RefCell<Vec<Placeholder>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn hooks<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<(v8::Local<'s, v8::Symbol>, v8::Local<'s, v8::Function>)> {
    let (brand, revive) = HOOKS.with(|h| h.borrow().as_ref().map(|h| (h.brand.clone(), h.revive.clone())))?;
    Some((v8::Local::new(scope, brand), v8::Local::new(scope, revive)))
}

fn is_proxy(_: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    rv.set_bool(args.get(0).is_proxy());
}

fn run_microtasks(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    scope.perform_microtask_checkpoint();
}

pub fn clear() {
    let hooks = HOOKS.with(|h| h.borrow_mut().take());
    let placeholders = PLACEHOLDERS.with(|p| std::mem::take(&mut *p.borrow_mut()));
    drop((hooks, placeholders));
    crate::host::encoding::clear();
}

fn clone_hooks(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let (Ok(brand), Ok(revive)) = (v8::Local::<v8::Symbol>::try_from(args.get(0)), v8::Local::<v8::Function>::try_from(args.get(1))) else {
        return bind::throw_type_error(scope, "cloneHooks: a symbol and a function");
    };
    let hooks = Hooks { brand: v8::Global::new(scope, brand), revive: v8::Global::new(scope, revive) };
    HOOKS.with(|h| *h.borrow_mut() = Some(hooks));
}

fn brand_writer<'s>(scope: &mut v8::PinScope<'s, '_>, object: v8::Local<'s, v8::Object>) -> Option<v8::Local<'s, v8::Function>> {
    let (brand, _) = hooks(scope)?;
    let write = object.get(scope, brand.into())?;
    v8::Local::<v8::Function>::try_from(write).ok()
}

impl v8::ValueSerializerImpl for Clone {
    fn throw_data_clone_error<'s>(&self, scope: &mut v8::PinScope<'s, '_>, message: v8::Local<'s, v8::String>) {
        let text = format!("DataCloneError: {}", message.to_rust_string_lossy(scope));
        bind::throw_error(scope, &text);
    }

    fn has_custom_host_object(&self, _: &v8::Isolate) -> bool {
        HOOKS.with(|h| h.borrow().is_some())
    }

    fn is_host_object<'s>(&self, scope: &mut v8::PinScope<'s, '_>, object: v8::Local<'s, v8::Object>) -> Option<bool> {
        if object.is_proxy() {
            return Some(false);
        }
        Some(brand_writer(scope, object).is_some())
    }

    fn write_host_object<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        object: v8::Local<'s, v8::Object>,
        serializer: &dyn ValueSerializerHelper,
    ) -> Option<bool> {
        let write = brand_writer(scope, object)?;
        let payload = write.call(scope, object.into(), &[])?;
        let context = scope.get_current_context();
        serializer.write_value(context, payload)
    }
}

impl v8::ValueDeserializerImpl for Clone {
    fn read_host_object<'s>(&self, scope: &mut v8::PinScope<'s, '_>, deserializer: &dyn ValueDeserializerHelper) -> Option<v8::Local<'s, v8::Object>> {
        let context = scope.get_current_context();
        let payload = deserializer.read_value(context)?;
        let placeholder = v8::Object::new(scope);
        let entry = (v8::Global::new(scope, placeholder), v8::Global::new(scope, payload));
        PLACEHOLDERS.with(|p| p.borrow_mut().push(entry));
        Some(placeholder)
    }
}

fn serialize(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    if let Some(bytes) = serialize_value(scope, args.get(0)) {
        rv.set(bind::uint8_array(scope, bytes).into());
    }
}

fn deserialize(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(bytes) = bind::bytes(args.get(0)) else {
        return bind::throw_type_error(scope, "deserialize: not bytes");
    };
    let value = deserialize_bytes(scope, &bytes);
    if let Some(value) = value {
        rv.set(value);
    }
}

pub fn deserialize_bytes<'s>(scope: &mut v8::PinScope<'s, '_>, bytes: &[u8]) -> Option<v8::Local<'s, v8::Value>> {
    deserialize_with(scope, bytes, &[])
}

pub fn deserialize_with<'s>(scope: &mut v8::PinScope<'s, '_>, bytes: &[u8], buffers: &[v8::SharedRef<v8::BackingStore>]) -> Option<v8::Local<'s, v8::Value>> {
    let outer = PLACEHOLDERS.with(|p| std::mem::take(&mut *p.borrow_mut()));
    let value = {
        let deserializer = v8::ValueDeserializer::new(scope, Box::new(Clone), bytes);
        for (i, store) in buffers.iter().enumerate() {
            let buffer = v8::ArrayBuffer::with_backing_store(scope, store);
            deserializer.transfer_array_buffer(i as u32, buffer);
        }
        let context = scope.get_current_context();
        deserializer.read_header(context).and_then(|_| deserializer.read_value(context))
    };
    let placeholders = PLACEHOLDERS.with(|p| std::mem::replace(&mut *p.borrow_mut(), outer));
    let value = value?;
    if placeholders.is_empty() {
        return Some(value);
    }
    let Some((_, revive)) = hooks(scope) else {
        bind::throw_error(scope, "DataCloneError: no platform objects in this realm");
        return None;
    };
    let pairs: Vec<v8::Local<v8::Value>> = placeholders
        .iter()
        .map(|(object, payload)| {
            let pair = [v8::Local::new(scope, object).into(), v8::Local::new(scope, payload)];
            v8::Array::new_with_elements(scope, &pair).into()
        })
        .collect();
    let pairs = v8::Array::new_with_elements(scope, &pairs);
    let undefined = v8::undefined(scope).into();
    revive.call(scope, undefined, &[value, pairs.into()])
}

pub fn serialize_value(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<Vec<u8>> {
    serialize_with(scope, value, &[])
}

pub fn serialize_with(scope: &mut v8::PinScope, value: v8::Local<v8::Value>, transfer: &[v8::Local<v8::ArrayBuffer>]) -> Option<Vec<u8>> {
    let serializer = v8::ValueSerializer::new(scope, Box::new(Clone));
    for (i, buffer) in transfer.iter().enumerate() {
        serializer.transfer_array_buffer(i as u32, *buffer);
    }
    serializer.write_header();
    let context = scope.get_current_context();
    match serializer.write_value(context, value) {
        Some(true) => Some(serializer.release()),
        _ => None,
    }
}

fn random_bytes(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(args.get(0)) else {
        return bind::throw_type_error(scope, "randomBytes: not a typed array");
    };
    let mut bytes = vec![0u8; view.byte_length()];
    if getrandom::fill(&mut bytes).is_err() {
        return bind::throw_error(scope, "randomBytes: no entropy");
    }
    let Some(buffer) = view.buffer(scope) else { return };
    if let Some(data) = buffer.data() {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), (data.as_ptr() as *mut u8).add(view.byte_offset()), bytes.len());
        }
    }
}

fn random_uuid(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let mut b = [0u8; 16];
    _ = getrandom::fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let uuid = format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32]);
    rv.set(bind::string(scope, &uuid).into());
}

fn queue_microtask(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        return bind::throw_type_error(scope, "queueMicrotask: callback is not a function");
    };
    scope.enqueue_microtask(callback);
}

fn on_task_end(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        return bind::throw_type_error(scope, "onTaskEnd: not a function");
    };
    let callback = v8::Global::new(scope, callback);
    rt::current().set_after_task(move |scope| {
        bind::call(scope, &callback, &[]);
    });
}

fn set_reporter(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let reporter = v8::Local::<v8::Function>::try_from(args.get(0)).ok().map(|f| v8::Global::new(scope, f));
    rt::current().set_reporter(reporter);
}

fn inspected(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    rv.set_bool(crate::host::inspector::inspected());
}

fn live_edit(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let url = bind::arg_string(scope, &args, 0);
    let source = bind::arg_string(scope, &args, 1);
    if let Err(e) = crate::host::inspector::live_edit(&url, &source) {
        rv.set(bind::string(scope, &e).into());
    }
}
