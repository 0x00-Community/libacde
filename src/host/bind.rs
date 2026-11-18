use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};

#[macro_export]
macro_rules! methods {
    ($scope:expr, $obj:expr, data = $data:expr; $($name:literal => $f:expr),* $(,)?) => {
        $( $crate::method_with($scope, $obj, $name, $f, Some($data)); )*
    };
    ($scope:expr, $obj:expr, $($name:literal => $f:expr),* $(,)?) => {
        $( $crate::method($scope, $obj, $name, $f); )*
    };
}

#[macro_export]
macro_rules! js_object {
    ($scope:expr, { $($key:literal => $value:expr),* $(,)? }) => {{
        let fields = [$(($key, $crate::IntoJs::js($value, &mut *$scope))),*];
        $crate::object($scope, &fields)
    }};
}

pub trait IntoJs<'s> {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value>;
}

impl<'s, T> IntoJs<'s> for v8::Local<'s, T>
where
    v8::Local<'s, T>: Into<v8::Local<'s, v8::Value>>,
{
    fn js(self, _: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        self.into()
    }
}

impl<'s> IntoJs<'s> for () {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        v8::undefined(scope).into()
    }
}

impl<'s> IntoJs<'s> for bool {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        v8::Boolean::new(scope, self).into()
    }
}

macro_rules! numbers {
    ($($t:ty),*) => {$(
        impl<'s> IntoJs<'s> for $t {
            fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
                v8::Number::new(scope, self as f64).into()
            }
        }
    )*};
}
numbers!(u16, u32, u64, i32, i64, usize, f64);

impl<'s> IntoJs<'s> for &str {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        string(scope, self).into()
    }
}

impl<'s> IntoJs<'s> for String {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        string(scope, &self).into()
    }
}

impl<'s, T: IntoJs<'s>> IntoJs<'s> for Option<T> {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        match self {
            Some(v) => v.js(scope),
            None => v8::null(scope).into(),
        }
    }
}

impl<'s> IntoJs<'s> for Vec<String> {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        let items: Vec<v8::Local<v8::Value>> = self.iter().map(|t| string(scope, t).into()).collect();
        v8::Array::new_with_elements(scope, &items).into()
    }
}

impl<'s> IntoJs<'s> for Vec<u8> {
    fn js(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        uint8_array(scope, self).into()
    }
}

pub struct Slab<T> {
    next: u32,
    items: HashMap<u32, T>,
}

impl<T> Default for Slab<T> {
    fn default() -> Self {
        Self { next: 0, items: HashMap::new() }
    }
}

impl<T> Slab<T> {
    pub fn next_id(&mut self) -> u32 {
        self.next = self.next.wrapping_add(1).max(1);
        self.next
    }

    pub fn add(&mut self, item: T) -> u32 {
        let id = self.next_id();
        self.items.insert(id, item);
        id
    }
}

impl<T> Deref for Slab<T> {
    type Target = HashMap<u32, T>;
    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl<T> DerefMut for Slab<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.items
    }
}

pub struct Args<'a, 's, 'i, 'v> {
    pub scope: &'a mut v8::PinScope<'s, 'i>,
    pub args: &'a v8::FunctionCallbackArguments<'v>,
}

impl<'v> Args<'_, '_, '_, 'v> {
    fn given(&self, i: usize) -> Option<v8::Local<'v, v8::Value>> {
        let value = self.args.get(i32::try_from(i).ok()?);
        (!value.is_null_or_undefined()).then_some(value)
    }

    pub fn text(&mut self, i: usize) -> Option<String> {
        let value = self.given(i)?;
        Some(value.to_rust_string_lossy(self.scope))
    }

    pub fn string(&mut self, i: usize) -> String {
        self.text(i).unwrap_or_default()
    }

    pub fn flag(&mut self, i: usize) -> Option<bool> {
        let value = self.given(i)?;
        Some(value.boolean_value(self.scope))
    }

    pub fn data(&mut self, i: usize) -> Option<Vec<u8>> {
        let value = self.given(i)?;
        match value.is_string() {
            true => Some(value.to_rust_string_lossy(self.scope).into_bytes()),
            false => with_bytes(value, <[u8]>::to_vec),
        }
    }

    pub fn count(&mut self, i: usize) -> Option<u64> {
        let n = self.given(i)?.number_value(self.scope)?;
        (n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n <= 9_007_199_254_740_991.0).then_some(n as u64)
    }

    pub fn json(&mut self, i: usize) -> Option<serde_json::Value> {
        let value = self.given(i)?;
        let text = v8::json::stringify(self.scope, value)?.to_rust_string_lossy(self.scope);
        Some(serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)))
    }
}

pub struct SendFunction(pub v8::Global<v8::Function>);

unsafe impl Send for SendFunction {}
unsafe impl Sync for SendFunction {}

pub fn string<'s>(scope: &mut v8::PinScope<'s, '_>, text: &str) -> v8::Local<'s, v8::String> {
    v8::String::new(scope, text).unwrap_or_else(|| v8::String::empty(scope))
}

pub fn key<'s>(scope: &mut v8::PinScope<'s, '_>, name: &str) -> v8::Local<'s, v8::String> {
    v8::String::new_from_utf8(scope, name.as_bytes(), v8::NewStringType::Internalized).unwrap_or_else(|| v8::String::empty(scope))
}

pub fn method(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>, name: &str, callback: impl v8::MapFnTo<v8::FunctionCallback>) {
    method_with(scope, obj, name, callback, None);
}

pub fn method_with(
    scope: &mut v8::PinScope,
    obj: v8::Local<v8::Object>,
    name: &str,
    callback: impl v8::MapFnTo<v8::FunctionCallback>,
    data: Option<v8::Local<v8::Value>>,
) {
    let key = key(scope, name);
    let mut builder = v8::Function::builder(callback);
    if let Some(data) = data {
        builder = builder.data(data);
    }
    let function = builder.build(scope).expect("libacde: function");
    function.set_name(key);
    obj.set(scope, key.into(), function.into());
}

pub fn call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    function: impl v8::Handle<Data = v8::Function>,
    args: &[v8::Local<v8::Value>],
) -> Option<v8::Local<'s, v8::Value>> {
    let function = v8::Local::new(scope, function);
    let recv = v8::undefined(scope);
    function.call(scope, recv.into(), args)
}

pub fn object<'s>(scope: &mut v8::PinScope<'s, '_>, fields: &[(&str, v8::Local<'s, v8::Value>)]) -> v8::Local<'s, v8::Object> {
    let obj = v8::Object::new(scope);
    for &(name, value) in fields {
        set(scope, obj, name, value);
    }
    obj
}

pub fn strings(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<Vec<String>> {
    let list = v8::Local::<v8::Array>::try_from(value).ok()?;
    (0..list.length()).map(|i| list.get_index(scope, i).filter(|v| v.is_string()).map(|v| v.to_rust_string_lossy(scope))).collect()
}

pub fn own_string_pairs(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Vec<(String, String)> {
    let Ok(obj) = v8::Local::<v8::Object>::try_from(value) else { return Vec::new() };
    let Some(names) = obj.get_own_property_names(scope, Default::default()) else { return Vec::new() };
    (0..names.length())
        .filter_map(|i| {
            let name = names.get_index(scope, i)?;
            let value = obj.get(scope, name).filter(|v| v.is_string())?;
            Some((name.to_rust_string_lossy(scope), value.to_rust_string_lossy(scope)))
        })
        .collect()
}

thread_local! {
    static PROMISES: RefCell<Slab<v8::Global<v8::PromiseResolver>>> = RefCell::default();
}

pub fn promise<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<(u32, v8::Local<'s, v8::Promise>)> {
    let resolver = v8::PromiseResolver::new(scope)?;
    let promise = resolver.get_promise(scope);
    let resolver = v8::Global::new(scope, resolver);
    Some((PROMISES.with(|p| p.borrow_mut().add(resolver)), promise))
}

pub fn resolver<'s>(scope: &mut v8::PinScope<'s, '_>, token: u32) -> Option<v8::Local<'s, v8::PromiseResolver>> {
    let resolver = PROMISES.with(|p| p.borrow_mut().remove(&token))?;
    Some(v8::Local::new(scope, resolver))
}

pub fn forget_promise(token: u32) {
    PROMISES.with(|p| p.borrow_mut().remove(&token));
}

pub fn clear_promises() {
    let promises = PROMISES.with(|p| std::mem::take(&mut *p.borrow_mut()));
    drop(promises);
}

pub fn set(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>, name: &str, value: v8::Local<v8::Value>) {
    let key = key(scope, name);
    obj.set(scope, key.into(), value);
}

pub fn get<'s>(scope: &mut v8::PinScope<'s, '_>, obj: v8::Local<v8::Object>, name: &str) -> Option<v8::Local<'s, v8::Value>> {
    let key = key(scope, name);
    obj.get(scope, key.into())
}

pub fn get_string(scope: &mut v8::PinScope, obj: v8::Local<v8::Object>, name: &str) -> Option<String> {
    get(scope, obj, name).filter(|v| v.is_string()).map(|v| v.to_rust_string_lossy(scope))
}

macro_rules! throwers {
    ($($name:ident => $make:ident),*) => {$(
        pub fn $name(scope: &mut v8::PinScope, message: &str) {
            let message = string(scope, message);
            let error = v8::Exception::$make(scope, message);
            scope.throw_exception(error);
        }
    )*};
}
throwers!(throw_error => error, throw_type_error => type_error, throw_range_error => range_error);

pub fn arg_string(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> String {
    let value = args.get(i);
    if value.is_undefined() {
        return String::new();
    }
    value.to_rust_string_lossy(scope)
}

pub fn arg_str<'a, const N: usize>(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
    i: i32,
    buffer: &'a mut [MaybeUninit<u8>; N],
) -> Cow<'a, str> {
    let value = args.get(i);
    if value.is_undefined() {
        return Cow::Borrowed("");
    }
    let string = match v8::Local::<v8::String>::try_from(value) {
        Ok(string) => string,
        Err(_) => match value.to_string(scope) {
            Some(string) => string,
            None => return Cow::Borrowed(""),
        },
    };
    string.to_rust_cow_lossy(scope, buffer)
}

pub fn arg_f64(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> f64 {
    args.get(i).number_value(scope).unwrap_or(f64::NAN)
}

pub fn arg_u32(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> u32 {
    let n = arg_f64(scope, args, i);
    if n.is_finite() && n >= 0.0 { n as u32 } else { 0 }
}

#[cfg(target_os = "linux")]
pub fn arg_u64(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> u64 {
    let n = arg_f64(scope, args, i);
    if n.is_finite() && n >= 0.0 { n as u64 } else { 0 }
}

pub fn arg_bool(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, i: i32) -> bool {
    args.get(i).boolean_value(scope)
}

pub fn with_bytes<R>(value: v8::Local<v8::Value>, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    if let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(value) {
        let mut storage = [0u8; v8::TYPED_ARRAY_MAX_SIZE_IN_HEAP];
        return Some(f(view.get_contents(&mut storage)));
    }
    if let Ok(buffer) = v8::Local::<v8::ArrayBuffer>::try_from(value) {
        let len = buffer.byte_length();
        return Some(match buffer.data() {
            Some(data) => f(unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, len) }),
            None => f(&[]),
        });
    }
    None
}

pub fn bytes(value: v8::Local<v8::Value>) -> Option<Vec<u8>> {
    with_bytes(value, <[u8]>::to_vec)
}

pub fn uint8_array<'s>(scope: &mut v8::PinScope<'s, '_>, data: Vec<u8>) -> v8::Local<'s, v8::Uint8Array> {
    let len = data.len();
    let store = v8::ArrayBuffer::new_backing_store_from_vec(data).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
    v8::Uint8Array::new(scope, buffer, 0, len).expect("libacde: Uint8Array")
}

fn array_buffer<'s, T: Copy>(scope: &mut v8::PinScope<'s, '_>, data: &[T]) -> v8::Local<'s, v8::ArrayBuffer> {
    let len = std::mem::size_of_val(data);
    let buffer = v8::ArrayBuffer::new(scope, len);
    if let Some(target) = buffer.data()
        && len > 0
    {
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, target.as_ptr() as *mut u8, len) };
    }
    buffer
}

macro_rules! typed_arrays {
    ($($name:ident: $t:ty => $array:ident),*) => {$(
        pub fn $name<'s>(scope: &mut v8::PinScope<'s, '_>, data: &[$t]) -> v8::Local<'s, v8::$array> {
            let buffer = array_buffer(scope, data);
            v8::$array::new(scope, buffer, 0, data.len()).expect(concat!("libacde: ", stringify!($array)))
        }
    )*};
}
typed_arrays!(uint32_array: u32 => Uint32Array, float32_array: f32 => Float32Array, float64_array: f64 => Float64Array);

pub fn run_script<'s>(scope: &mut v8::PinScope<'s, '_>, source: &str, url: &str) -> Option<v8::Local<'s, v8::Value>> {
    let name = string(scope, url);
    let text = string(scope, source);
    let origin = v8::ScriptOrigin::new(scope, name.into(), 0, 0, false, -1, None, false, false, false, None);
    let script = v8::Script::compile(scope, text, Some(&origin))?;
    script.run(scope)
}
