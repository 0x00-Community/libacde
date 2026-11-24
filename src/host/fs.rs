use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use crate::runtime::{FsCall, FsError, FsReply, Workspace};

use crate::host::bind::{self, Args};
use crate::host::offload;

type Method = fn(&mut Args) -> FsCall;

const READ: bool = false;
const CHANGE: bool = true;
const READERS: usize = 4;

const METHODS: [(&str, bool, Method); 20] = [
    ("roots", READ, |_| FsCall::Roots),
    ("list", READ, |a| FsCall::List { root: a.string(0), path: a.text(1), hidden: a.flag(2) }),
    ("preview", READ, |a| FsCall::Preview { root: a.string(0), path: a.string(1) }),
    ("download", READ, |a| FsCall::Download { root: a.string(0), path: a.string(1) }),
    ("locate", READ, |a| FsCall::Locate { path: a.string(0) }),
    ("openPath", READ, |a| FsCall::OpenPath { root: a.string(0), path: a.string(1) }),
    ("mkdir", CHANGE, |a| FsCall::Mkdir { root: a.string(0), path: a.string(1), options: a.json(2) }),
    ("copy", CHANGE, |a| FsCall::Copy { from_root: a.string(0), from_path: a.string(1), to_root: a.string(2), to_dir: a.string(3) }),
    ("rename", CHANGE, |a| FsCall::Rename { root: a.string(0), from: a.string(1), to: a.string(2) }),
    ("delete", CHANGE, |a| FsCall::Delete { root: a.string(0), path: a.string(1) }),
    ("stat", READ, |a| FsCall::Stat { root: a.string(0), path: a.string(1) }),
    ("readText", READ, |a| FsCall::ReadText { root: a.string(0), path: a.string(1) }),
    ("readBytes", READ, |a| FsCall::ReadBytes { root: a.string(0), path: a.string(1) }),
    ("writeFile", CHANGE, |a| FsCall::WriteFile { root: a.string(0), path: a.string(1), data: a.data(2), options: a.json(3) }),
    ("readRange", READ, |a| FsCall::ReadRange { root: a.string(0), path: a.string(1), offset: a.count(2), length: a.count(3), options: a.json(4) }),
    ("openWrite", CHANGE, |a| FsCall::OpenWrite { root: a.string(0), path: a.string(1), options: a.json(2) }),
    ("writeChunk", CHANGE, |a| FsCall::WriteChunk { id: a.string(0), offset: a.count(1), data: a.data(2) }),
    ("truncateWrite", CHANGE, |a| FsCall::TruncateWrite { id: a.string(0), size: a.count(1) }),
    ("commitWrite", CHANGE, |a| FsCall::CommitWrite { id: a.string(0) }),
    ("abortWrite", CHANGE, |a| FsCall::AbortWrite { id: a.string(0) }),
];

struct Changes {
    queue: offload::Queue,
    pending: Arc<AtomicUsize>,
}

#[cfg(target_os = "linux")]
type Watching = (v8::Global<v8::Function>, Option<crate::runtime::fs::watch::Watch>);

thread_local! {
    static WORKSPACE: RefCell<Option<Workspace>> = const { RefCell::new(None) };
    static CHANGES: RefCell<Option<Changes>> = const { RefCell::new(None) };
    #[cfg(target_os = "linux")]
    static WATCHES: RefCell<bind::Slab<Watching>> = RefCell::default();
}

fn readers() -> &'static offload::Pool {
    static POOL: OnceLock<offload::Pool> = OnceLock::new();
    POOL.get_or_init(|| offload::Pool::new("acde-fs", READERS))
}

pub fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>, workspace: Workspace) {
    WORKSPACE.with(|w| *w.borrow_mut() = Some(workspace));
    let fs = v8::Object::new(scope);
    for (index, (name, ..)) in METHODS.iter().enumerate() {
        let data = v8::Integer::new_from_unsigned(scope, index as u32).into();
        bind::method_with(scope, fs, name, call, Some(data));
    }
    crate::methods!(scope, fs, "watch" => watch, "unwatch" => unwatch);
    bind::set(scope, natives, "fs", fs.into());
}

fn call(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some((_, changes, method)) = args.data().uint32_value(scope).and_then(|i| METHODS.get(i as usize)) else { return };
    let Some(workspace) = WORKSPACE.with(|w| w.borrow().clone()) else {
        return bind::throw_error(scope, "acde.fs: not given to this web");
    };
    let request = method(&mut Args { scope: &mut *scope, args: &args });
    let pending = CHANGES.with(|c| c.borrow().as_ref().map_or(0, |c| c.pending.load(Ordering::Acquire)));
    if !changes && pending == 0 {
        return readers().run(scope, &mut rv, move || workspace.call(request), settle);
    }
    CHANGES.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            match offload::Queue::new("acde-fs-changes") {
                Ok(queue) => *c = Some(Changes { queue, pending: Arc::default() }),
                Err(e) => return bind::throw_error(scope, &format!("acde.fs: {e}")),
            }
        }
        let Some(Changes { queue, pending }) = c.as_ref() else { return };
        let pending = pending.clone();
        pending.fetch_add(1, Ordering::AcqRel);
        queue.run(
            scope,
            &mut rv,
            move || {
                let reply = workspace.call(request);
                pending.fetch_sub(1, Ordering::AcqRel);
                reply
            },
            settle,
        );
    });
}

fn settle(scope: &mut v8::PinScope, resolver: v8::Local<v8::PromiseResolver>, reply: Result<FsReply, FsError>) {
    match reply {
        Ok(FsReply::Json(value)) => {
            let value = offload::json(scope, &value);
            resolver.resolve(scope, value);
        }
        Ok(FsReply::File { name, data }) => {
            let file = crate::js_object!(scope, {"name" => name, "data" => data});
            resolver.resolve(scope, file.into());
        }
        Ok(FsReply::Text { text, modified_ms, size, version }) => {
            let read = crate::js_object!(scope, {"text" => text, "modifiedMs" => modified_ms, "size" => size, "version" => version});
            resolver.resolve(scope, read.into());
        }
        Ok(FsReply::Done | FsReply::Dir(_)) => {
            let done = v8::undefined(scope).into();
            resolver.resolve(scope, done);
        }
        Ok(FsReply::Bytes { data, modified_ms, version }) => {
            let size = data.len();
            let read = crate::js_object!(scope, {"data" => data, "modifiedMs" => modified_ms, "size" => size, "version" => version});
            resolver.resolve(scope, read.into());
        }
        Ok(FsReply::Range(range)) => {
            let read = crate::js_object!(scope, {
                "data" => range.data, "offset" => range.offset, "size" => range.size, "modifiedMs" => range.modified_ms, "version" => range.version
            });
            resolver.resolve(scope, read.into());
        }
        Err(FsError { status, code, message }) => {
            let thrown = offload::failure(scope, &message, Some(i32::from(status)), code);
            resolver.reject(scope, thrown);
        }
    }
}

#[cfg(target_os = "linux")]
fn watch(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(workspace) = WORKSPACE.with(|w| w.borrow().clone()) else {
        return bind::throw_error(scope, "acde.fs: not given to this web");
    };
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(2)) else {
        return bind::throw_type_error(scope, "acde.fs.watch: the handler is not a function");
    };
    let (root, path) = (bind::arg_string(scope, &args, 0), bind::arg_string(scope, &args, 1));
    let id = WATCHES.with(|w| w.borrow_mut().add((v8::Global::new(scope, callback), None)));
    readers().run(scope, &mut rv, move || (id, workspace.call(crate::runtime::FsCall::Watch { root, path })), watched);
}

#[cfg(target_os = "linux")]
fn watched(scope: &mut v8::PinScope, resolver: v8::Local<v8::PromiseResolver>, (id, reply): (u32, Result<FsReply, FsError>)) {
    let started = match reply {
        Ok(FsReply::Dir(dir)) => {
            let tx = crate::host::rt::current().sender();
            let sink = move |changes: Vec<crate::runtime::fs::watch::Change>| {
                let deliver = move |scope: &mut v8::PinScope| {
                    let Some(callback) = WATCHES.with(|w| w.borrow().get(&id).map(|(callback, _)| callback.clone())) else { return };
                    let value = serde_json::to_value(&changes).unwrap_or_default();
                    let value = offload::json(scope, &value);
                    bind::call(scope, &callback, &[value]);
                };
                tx.send(crate::host::rt::Msg::Task(Box::new(deliver))).is_ok()
            };
            crate::runtime::fs::watch::watch(&dir, Box::new(sink)).map_err(|e| FsError { status: 500, code: "EIO", message: format!("watching: {e}") })
        }
        Ok(_) => Err(FsError { status: 500, code: "EIO", message: "watching: not a directory handle".into() }),
        Err(e) => Err(e),
    };
    match started {
        Ok(handle) if WATCHES.with(|w| w.borrow().get(&id).is_some()) => {
            WATCHES.with(|w| {
                if let Some(entry) = w.borrow_mut().get_mut(&id) {
                    entry.1 = Some(handle);
                }
            });
            let id = v8::Integer::new_from_unsigned(scope, id).into();
            resolver.resolve(scope, id);
        }
        Ok(_) => {
            let id = v8::Integer::new_from_unsigned(scope, id).into();
            resolver.resolve(scope, id);
        }
        Err(FsError { status, code, message }) => {
            WATCHES.with(|w| w.borrow_mut().remove(&id));
            let thrown = offload::failure(scope, &message, Some(i32::from(status)), code);
            resolver.reject(scope, thrown);
        }
    }
}

#[cfg(target_os = "linux")]
fn unwatch(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_u32(scope, &args, 0);
    let entry = WATCHES.with(|w| w.borrow_mut().remove(&id));
    drop(entry);
}

#[cfg(not(target_os = "linux"))]
fn watch(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some((token, promise)) = bind::promise(scope) else { return };
    rv.set(promise.into());
    if let Some(resolver) = bind::resolver(scope, token) {
        let thrown = offload::failure(scope, "acde.fs.watch: not on this platform", Some(501), "ENOSYS");
        resolver.reject(scope, thrown);
    }
}

#[cfg(not(target_os = "linux"))]
fn unwatch(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {}
