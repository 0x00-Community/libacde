use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::host::Input;
use crate::host::rt::{self, Msg};
use crate::host::{bind, offload};

const NAME: &str = "acde.input";
const NOT_HERE: &str = "acde.input: this runtime has no compositor to configure (only acdebin-system's session has)";

#[derive(Default)]
struct State {
    input: Option<Arc<dyn Input>>,
    watchers: HashMap<u32, v8::Global<v8::Function>>,
    next: u32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>, input: Option<Arc<dyn Input>>) {
    STATE.with(|s| s.borrow_mut().input = input);
    let object = v8::Object::new(scope);
    crate::methods!(scope, object,
        "state" => state,
        "set" => set,
        "catalog" => catalog,
        "watch" => watch,
    );
    bind::set(scope, natives, "input", object.into());
}

pub fn clear() {
    let state = STATE.with(|s| std::mem::take(&mut *s.borrow_mut()));
    drop(state);
}

fn input() -> Result<Arc<dyn Input>, String> {
    STATE.with(|s| s.borrow().input.clone()).ok_or_else(|| NOT_HERE.to_owned())
}

fn call(scope: &mut v8::PinScope, rv: &mut v8::ReturnValue, work: impl FnOnce(Arc<dyn Input>) -> Result<Value, String> + Send + 'static) {
    match input() {
        Ok(input) => offload::run(scope, rv, "acde-input-call", move || work(input), offload::settle_json::<true>),
        Err(e) => bind::throw_error(scope, &e),
    }
}

fn with_ime(mut state: Value) -> Value {
    cfg_select! {
        target_os = "linux" => {
            if let Some(fields) = state.as_object_mut() {
                fields.insert("ime".into(), crate::runtime::ime_state(&crate::runtime::Fcitx));
            }
        }
        _ => {}
    }
    state
}

fn split(mut patch: Value) -> (Value, Option<Value>) {
    let ime = patch.as_object_mut().and_then(|fields| fields.remove("ime"));
    (patch, ime)
}

fn set_ime(patch: Option<Value>) -> Result<(), String> {
    let Some(patch) = patch else { return Ok(()) };
    cfg_select! {
        target_os = "linux" => crate::runtime::ime_apply(&crate::runtime::Fcitx, &patch),
        _ => {
            let _ = patch;
            Err(format!("{NAME}.set: no input method framework here"))
        }
    }
}

fn state(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    call(scope, &mut rv, |input| input.state().map(with_ime));
}

fn set(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let patch: Value =
        v8::json::stringify(scope, args.get(0)).map(|s| s.to_rust_string_lossy(scope)).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if !patch.is_object() {
        return bind::throw_type_error(scope, &format!("{NAME}.set: the patch is an object"));
    }
    let (patch, ime) = split(patch);
    call(scope, &mut rv, move |input| {
        let rest = patch.as_object().is_some_and(|fields| !fields.is_empty());
        if rest {
            input.set(&patch)?;
        }
        set_ime(ime).map(|()| Value::Null)
    });
}

fn catalog(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let read = || -> Result<Value, String> {
        cfg_select! {
            target_os = "linux" => {
                let mut catalog = crate::runtime::XkbCatalog::read()?;
                if let Some(fields) = catalog.as_object_mut() {
                    fields.insert("imes".into(), crate::runtime::ime_catalog(&crate::runtime::Fcitx));
                }
                Ok(catalog)
            }
            _ => Err(format!("{NAME}.catalog: no xkeyboard-config here")),
        }
    };
    offload::run(scope, &mut rv, "acde-input-catalog", read, offload::settle_json::<true>);
}

fn watch(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        return bind::throw_type_error(scope, &format!("{NAME}.watch: the callback is not a function"));
    };
    let input = match input() {
        Ok(input) => input,
        Err(e) => return bind::throw_error(scope, &e),
    };
    let callback = v8::Global::new(scope, callback);
    let token = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let token = s.next.wrapping_add(1).max(1);
        s.next = token;
        s.watchers.insert(token, callback);
        token
    });
    let tx = rt::current().sender();
    let sent = tx.clone();
    input.listen(Box::new(move |state: Value| {
        let state = with_ime(state);
        sent.send(Msg::Task(Box::new(move |scope| deliver(scope, token, &state)))).is_ok()
    }));
    cfg_select! {
        target_os = "linux" => {
            let source = input.clone();
            crate::runtime::Fcitx::watch(move || match source.state() {
                Ok(state) => {
                    let state = with_ime(state);
                    tx.send(Msg::Task(Box::new(move |scope| deliver(scope, token, &state)))).is_ok()
                }
                Err(_) => true,
            });
        }
        _ => drop(tx),
    }
}

fn deliver(scope: &mut v8::PinScope, token: u32, state: &Value) {
    let Some(callback) = STATE.with(|s| s.borrow().watchers.get(&token).cloned()) else { return };
    let value = offload::json(scope, state);
    bind::call(scope, &callback, &[value]);
}
