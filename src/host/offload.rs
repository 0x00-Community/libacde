use std::cell::RefCell;

use crate::host::bind;
use crate::host::rt::{self, Msg};

thread_local! {
    static CALLBACKS: RefCell<bind::Slab<v8::Global<v8::Function>>> = RefCell::default();
}

pub type Settle<T> = fn(&mut v8::PinScope, v8::Local<v8::PromiseResolver>, T);

type Job = Box<dyn FnOnce() + Send>;

fn settled<T: Send + 'static>(
    scope: &mut v8::PinScope,
    rv: &mut v8::ReturnValue,
    work: impl FnOnce() -> T + Send + 'static,
    settle: Settle<T>,
) -> Option<(u32, Job)> {
    let (token, promise) = bind::promise(scope)?;
    let lp = rt::current();
    let tx = lp.sender();
    lp.keep_alive(true);
    rv.set(promise.into());
    Some((
        token,
        Box::new(move || {
            let value = work();
            _ = tx.send(Msg::Task(Box::new(move |scope| {
                rt::current().keep_alive(false);
                if let Some(resolver) = bind::resolver(scope, token) {
                    settle(scope, resolver, value);
                }
            })));
        }),
    ))
}

fn unsettle(rv: &mut v8::ReturnValue, token: u32) {
    rt::current().keep_alive(false);
    bind::forget_promise(token);
    rv.set_undefined();
}

pub fn run<T: Send + 'static>(scope: &mut v8::PinScope, rv: &mut v8::ReturnValue, thread: &str, work: impl FnOnce() -> T + Send + 'static, settle: Settle<T>) {
    let Some((token, job)) = settled(scope, rv, work, settle) else { return };
    if let Err(e) = std::thread::Builder::new().name(thread.into()).spawn(job) {
        unsettle(rv, token);
        bind::throw_error(scope, &format!("{thread}: {e}"));
    }
}

pub struct Pool {
    tx: std::sync::Mutex<std::sync::mpsc::Sender<Job>>,
}

impl Pool {
    pub fn new(thread: &str, size: usize) -> Pool {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
        for n in 0..size {
            let rx = rx.clone();
            let spawned = std::thread::Builder::new().name(format!("{thread}-{n}")).spawn(move || {
                loop {
                    let job = match rx.lock() {
                        Ok(rx) => rx.recv(),
                        Err(_) => return,
                    };
                    match job {
                        Ok(job) => job(),
                        Err(_) => return,
                    }
                }
            });
            if let Err(e) = spawned {
                log::error!("{thread}: {e}");
            }
        }
        Pool { tx: std::sync::Mutex::new(tx) }
    }

    pub fn run<T: Send + 'static>(&self, scope: &mut v8::PinScope, rv: &mut v8::ReturnValue, work: impl FnOnce() -> T + Send + 'static, settle: Settle<T>) {
        let Some((token, job)) = settled(scope, rv, work, settle) else { return };
        let sent = self.tx.lock().map_err(|_| ()).and_then(|tx| tx.send(job).map_err(|_| ()));
        if sent.is_err() {
            unsettle(rv, token);
            bind::throw_error(scope, "the pool's threads are gone");
        }
    }
}

pub struct Queue {
    tx: std::sync::mpsc::Sender<Job>,
}

impl Queue {
    pub fn new(thread: &str) -> std::io::Result<Queue> {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new().name(thread.into()).spawn(move || {
            for job in rx {
                job();
            }
        })?;
        Ok(Queue { tx })
    }

    pub fn run<T: Send + 'static>(&self, scope: &mut v8::PinScope, rv: &mut v8::ReturnValue, work: impl FnOnce() -> T + Send + 'static, settle: Settle<T>) {
        let Some((token, job)) = settled(scope, rv, work, settle) else { return };
        if self.tx.send(job).is_err() {
            unsettle(rv, token);
            bind::throw_error(scope, "the queue's thread is gone");
        }
    }
}

pub fn settle_json<const NULL_IS_UNDEFINED: bool>(
    scope: &mut v8::PinScope,
    resolver: v8::Local<v8::PromiseResolver>,
    reply: Result<serde_json::Value, String>,
) {
    match reply {
        Ok(serde_json::Value::Null) if NULL_IS_UNDEFINED => {
            let undefined = v8::undefined(scope).into();
            resolver.resolve(scope, undefined);
        }
        Ok(value) => {
            let value = json(scope, &value);
            resolver.resolve(scope, value);
        }
        Err(message) => {
            let thrown = match coded(&message) {
                Some((code, text)) => failure(scope, &text, None, &code),
                None => failure(scope, &message, None, "GenericFailure"),
            };
            resolver.reject(scope, thrown);
        }
    };
}

fn coded(message: &str) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(message).ok()?;
    Some((value.get("code")?.as_str()?.to_owned(), value.get("message")?.as_str()?.to_owned()))
}

pub fn json_sink(scope: &mut v8::PinScope, callback: v8::Local<v8::Function>) -> impl Fn(serde_json::Value) -> bool + Send + Sync + 'static {
    let token = CALLBACKS.with(|c| c.borrow_mut().add(v8::Global::new(scope, callback)));
    let tx = rt::current().sender();
    move |value| {
        let deliver = move |scope: &mut v8::PinScope| {
            let Some(callback) = CALLBACKS.with(|c| c.borrow().get(&token).cloned()) else { return };
            let value = json(scope, &value);
            bind::call(scope, &callback, &[value]);
        };
        tx.send(Msg::Task(Box::new(deliver))).is_ok()
    }
}

pub fn clear() {
    let callbacks = CALLBACKS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    drop(callbacks);
}

pub fn json<'s>(scope: &mut v8::PinScope<'s, '_>, value: &serde_json::Value) -> v8::Local<'s, v8::Value> {
    let text = bind::string(scope, &value.to_string());
    v8::json::parse(scope, text).unwrap_or_else(|| v8::null(scope).into())
}

pub fn failure<'s>(scope: &mut v8::PinScope<'s, '_>, message: &str, status: Option<i32>, code: &str) -> v8::Local<'s, v8::Value> {
    let text = bind::string(scope, message);
    let thrown = v8::Exception::error(scope, text);
    if let Ok(object) = v8::Local::<v8::Object>::try_from(thrown) {
        if let Some(status) = status {
            let status = v8::Integer::new(scope, status).into();
            bind::set(scope, object, "status", status);
        }
        let code = bind::string(scope, code).into();
        bind::set(scope, object, "code", code);
    }
    thrown
}
