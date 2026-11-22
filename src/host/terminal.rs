use std::cell::RefCell;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::terminal::{OpenOptions, Refusal, TerminalEvent, TerminalHost};

use crate::host::{bind, offload};

thread_local! {
    static TERMINALS: RefCell<Option<Arc<dyn TerminalHost>>> = const { RefCell::new(None) };
}

pub fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>, terminals: Arc<dyn TerminalHost>) {
    TERMINALS.with(|t| *t.borrow_mut() = Some(terminals));
    let terminal = v8::Object::new(scope);
    crate::methods!(scope, terminal,
        "open" => open,
        "subscribe" => subscribe,
        "input" => input,
        "resize" => resize,
        "close" => close,
        "shells" => shells,
        "limits" => limits,
    );
    bind::set(scope, natives, "terminal", terminal.into());
}

fn with_terminals(scope: &mut v8::PinScope, rv: &mut v8::ReturnValue, f: impl FnOnce(&dyn TerminalHost) -> Result<Value, Refusal>) {
    let Some(terminals) = TERMINALS.with(|t| t.borrow().clone()) else {
        return bind::throw_error(scope, "acde.terminal: not given to this web");
    };
    let Some(resolver) = v8::PromiseResolver::new(scope) else { return };
    offload::settle_json::<false>(scope, resolver, f(terminals.as_ref()).map_err(String::from));
    rv.set(resolver.get_promise(scope).into());
}

fn given_options(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Result<Option<Value>, Refusal> {
    if value.is_null_or_undefined() {
        return Ok(None);
    }
    let refused = || Refusal::new(400, "invalid_request", "options must be an object of JSON values");
    if !value.is_object() || value.is_function() {
        return Err(refused());
    }
    v8::tc_scope!(let tc, scope);
    let text = v8::json::stringify(tc, value).map(|text| text.to_rust_string_lossy(tc));
    text.and_then(|text| serde_json::from_str(&text).ok()).map(Some).ok_or_else(refused)
}

fn open(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let (cols, rows) = (bind::arg_f64(scope, &args, 0), bind::arg_f64(scope, &args, 1));
    let options = given_options(scope, args.get(2)).and_then(|given| OpenOptions::parse(cols, rows, given));
    with_terminals(scope, &mut rv, |t| Ok(json!({"id": t.open(options?)?})));
}

fn input(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let (id, data) = (bind::arg_string(scope, &args, 0), bind::arg_string(scope, &args, 1));
    with_terminals(scope, &mut rv, |t| t.input(&id, data).map(|()| json!({"ok": true})));
}

fn resize(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    let (cols, rows) = (bind::arg_f64(scope, &args, 1), bind::arg_f64(scope, &args, 2));
    with_terminals(scope, &mut rv, |t| t.resize(&id, cols, rows).map(|(cols, rows)| json!({"cols": cols, "rows": rows})));
}

fn close(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    with_terminals(scope, &mut rv, |t| {
        t.close(&id);
        Ok(json!({"closed": id}))
    });
}

fn shells(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    with_terminals(scope, &mut rv, |t| Ok(json!(t.shells())));
}

fn limits(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    with_terminals(scope, &mut rv, |t| Ok(json!(t.limits())));
}

fn subscribe(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(1)) else {
        return bind::throw_type_error(scope, "acde.terminal.subscribe: the callback is not a function");
    };
    let sink = offload::json_sink(scope, callback);
    let sink = Box::new(move |events: Vec<TerminalEvent>| sink(json!(events)));
    with_terminals(scope, &mut rv, move |t| t.listen(&id, sink).map(|()| Value::Null));
}
