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

thread_local! {
    static WORKSPACE: RefCell<Option<Workspace>> = const { RefCell::new(None) };
    static CHANGES: RefCell<Option<Changes>> = const { RefCell::new(None) };
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
        Ok(FsReply::Done) => {
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
