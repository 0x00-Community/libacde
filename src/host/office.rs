use std::cell::RefCell;

use crate::runtime::{OfficeCall, OfficeService};

use crate::host::{bind, offload};

thread_local! {
    static OFFICE: RefCell<Option<OfficeService>> = const { RefCell::new(None) };
}

pub fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>, office: OfficeService) {
    OFFICE.with(|o| *o.borrow_mut() = Some(office));
    let object = v8::Object::new(scope);
    crate::methods!(scope, object,
        "server" => server,
        "state" => state,
        "openLent" => open_lent,
        "status" => status,
        "save" => save,
        "new" => create,
    );
    bind::set(scope, natives, "office", object.into());
}

fn send(scope: &mut v8::PinScope, rv: &mut v8::ReturnValue, call: OfficeCall) {
    let Some(office) = OFFICE.with(|o| o.borrow().clone()) else {
        return bind::throw_error(scope, "acde.office: not given to this web");
    };
    offload::run(scope, rv, "acde-office", move || office.call(call), offload::settle_json::<false>);
}

fn server(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    send(scope, &mut rv, OfficeCall::Server);
}

fn state(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let mut a = bind::Args { scope: &mut *scope, args: &args };
    let (root, dir, path) = (a.text(0), a.text(1), a.text(2));
    send(scope, &mut rv, OfficeCall::State { root, dir, path });
}

fn open_lent(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let name = bind::arg_string(scope, &args, 0);
    send(scope, &mut rv, OfficeCall::Lent { name });
}

fn status(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let key = bind::arg_string(scope, &args, 0);
    send(scope, &mut rv, OfficeCall::Status { key });
}

fn save(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let body = bind::Args { scope: &mut *scope, args: &args }.json(0);
    send(scope, &mut rv, OfficeCall::Save { body });
}

fn create(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let body = bind::Args { scope: &mut *scope, args: &args }.json(0);
    send(scope, &mut rv, OfficeCall::New { body });
}
