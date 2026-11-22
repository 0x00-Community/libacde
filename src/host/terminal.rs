use std::cell::RefCell;
use std::sync::Arc;

use crate::terminal::{OpenOptions, Refusal, TerminalHost};

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

fn with_terminals(scope: &mut v8::PinScope, rv: &mut v8::ReturnValue, f: impl FnOnce(&dyn TerminalHost) -> Result<serde_json::Value, Refusal>) {
    let Some(terminals) = TERMINALS.with(|t| t.borrow().clone()) else {
        return bind::throw_error(scope, "acde.terminal: not given to this web");
    };
    let Some(resolver) = v8::PromiseResolver::new(scope) else { return };
    offload::settle_json::<false>(scope, resolver, f(terminals.as_ref()).map_err(String::from));
    rv.set(resolver.get_promise(scope).into());
}

fn open(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let (cols, rows) = (bind::arg_f64(scope, &args, 0), bind::arg_f64(scope, &args, 1));
    let given = bind::Args { scope: &mut *scope, args: &args }.json(2);
    let options = OpenOptions::parse(cols, rows, given);
    with_terminals(scope, &mut rv, |t| t.open(options?));
}

fn input(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let (id, data) = (bind::arg_string(scope, &args, 0), bind::arg_string(scope, &args, 1));
    with_terminals(scope, &mut rv, |t| t.input(&id, data));
}

fn resize(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    let (cols, rows) = (bind::arg_f64(scope, &args, 1), bind::arg_f64(scope, &args, 2));
    with_terminals(scope, &mut rv, |t| t.resize(&id, cols, rows));
}

fn close(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    with_terminals(scope, &mut rv, |t| Ok(t.close(&id)));
}

fn shells(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    with_terminals(scope, &mut rv, |t| Ok(t.shells()));
}

fn limits(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    with_terminals(scope, &mut rv, |t| Ok(t.limits()));
}

fn subscribe(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let id = bind::arg_string(scope, &args, 0);
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(1)) else {
        return bind::throw_type_error(scope, "acde.terminal.subscribe: the callback is not a function");
    };
    let sink = Box::new(offload::json_sink(scope, callback));
    with_terminals(scope, &mut rv, move |t| t.listen(&id, sink).map(|()| serde_json::Value::Null));
}
