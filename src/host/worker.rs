use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::host::bind;
use crate::host::envelope::Envelope;
use crate::host::rt::{self, Msg};

pub const MAX_LIVE: usize = 64;
pub const MAX_IN_FLIGHT: usize = 1 << 30;
const FIRST_HEAP: usize = 64 << 20;
const SMALLEST_HEAP: usize = 16 << 20;

fn take(budget: &AtomicUsize, want: usize, least: usize) -> Option<usize> {
    let mut got = 0;
    budget
        .try_update(Ordering::AcqRel, Ordering::Acquire, |left| {
            got = want.min(left);
            (got >= least).then(|| left - got)
        })
        .ok()?;
    Some(got)
}

struct Grant {
    budget: Arc<AtomicUsize>,
    granted: AtomicUsize,
    isolate: std::sync::OnceLock<v8::IsolateHandle>,
    reason: Arc<Mutex<Option<String>>>,
    stopping: Arc<AtomicBool>,
}

impl Grant {
    fn reserve(budget: &Arc<AtomicUsize>, reason: Arc<Mutex<Option<String>>>, stopping: Arc<AtomicBool>) -> Option<Box<Grant>> {
        let first = take(budget, FIRST_HEAP, SMALLEST_HEAP)?;
        Some(Box::new(Grant { budget: budget.clone(), granted: AtomicUsize::new(first), isolate: std::sync::OnceLock::new(), reason, stopping }))
    }

    fn mb(&self) -> usize {
        self.granted.load(Ordering::Acquire) >> 20
    }

    fn attach(self: &Box<Self>, runtime: &mut rt::Runtime, isolate: v8::IsolateHandle) {
        _ = self.isolate.set(isolate);
        let data = &**self as *const Self as *mut std::ffi::c_void;
        runtime.isolate.add_near_heap_limit_callback(Self::near_limit, data);
    }

    extern "C" fn near_limit(data: *mut std::ffi::c_void, current: usize, _initial: usize) -> usize {
        let grant = unsafe { &*(data as *const Self) };
        if let Some(more) = take(&grant.budget, current / 2, (current / 4).max(1)) {
            grant.granted.fetch_add(more, Ordering::AcqRel);
            return current + more;
        }
        if let Ok(mut reason) = grant.reason.lock() {
            reason.get_or_insert_with(|| "memory".to_owned());
        }
        grant.stopping.store(true, Ordering::SeqCst);
        if let Some(isolate) = grant.isolate.get() {
            isolate.terminate_execution();
        }
        current + current / 2
    }
}

impl Drop for Grant {
    fn drop(&mut self) {
        self.budget.fetch_add(self.granted.load(Ordering::Acquire), Ordering::AcqRel);
    }
}
const STACK: usize = 8 << 20;

struct InFlight {
    size: usize,
    counter: Arc<AtomicUsize>,
}

impl InFlight {
    fn take(counter: &Arc<AtomicUsize>, size: usize) -> Option<InFlight> {
        counter.try_update(Ordering::AcqRel, Ordering::Acquire, |used| used.checked_add(size).filter(|total| *total <= MAX_IN_FLIGHT)).ok()?;
        Some(InFlight { size, counter: counter.clone() })
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.size, Ordering::AcqRel);
    }
}

struct Letter {
    envelope: Envelope,
    _in_flight: InFlight,
}

fn seal(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, first: i32, in_flight: &Arc<AtomicUsize>) -> Option<Letter> {
    let transfer: Vec<v8::Local<v8::Value>> = match v8::Local::<v8::Array>::try_from(args.get(first + 1)) {
        Ok(list) => (0..list.length()).filter_map(|i| list.get_index(scope, i)).collect(),
        Err(_) => Vec::new(),
    };
    let envelope = Envelope::pack(scope, args.get(first), &transfer)?;
    let Some(in_flight) = InFlight::take(in_flight, envelope.size()) else {
        bind::throw_error(scope, "DataCloneError: Data cannot be cloned, out of memory.");
        return None;
    };
    Some(Letter { envelope, _in_flight: in_flight })
}

fn open<'s>(scope: &mut v8::PinScope<'s, '_>, letter: Letter) -> Option<v8::Local<'s, v8::Value>> {
    let value = {
        v8::tc_scope!(let tc, scope);
        letter.envelope.open(tc).map(|v| v8::Global::new(tc, v))
    };
    value.map(|v| v8::Local::new(scope, v))
}

struct Stop {
    stopping: Arc<AtomicBool>,
    isolate: Mutex<Option<v8::IsolateHandle>>,
}

impl Stop {
    fn now(&self, tx: &Sender<Msg>) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Ok(isolate) = self.isolate.lock()
            && let Some(isolate) = isolate.as_ref()
        {
            isolate.terminate_execution();
        }
        _ = tx.send(Msg::Wake);
    }
}

struct Waiting {
    name: String,
    rx: Receiver<Msg>,
}

struct Entry {
    tx: Sender<Msg>,
    stop: Arc<Stop>,
    waiting: Option<Waiting>,
}

#[derive(Default)]
struct Page {
    workers: HashMap<u32, Entry>,
    queue: VecDeque<u32>,
    live: usize,
    on_event: Option<v8::Global<v8::Function>>,
    in_flight: Arc<AtomicUsize>,
    heap: Option<Option<Arc<AtomicUsize>>>,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::default();
    static SELF: RefCell<Option<Inside>> = const { RefCell::new(None) };
}

pub fn install<'s>(scope: &mut v8::PinScope<'s, '_>, natives: v8::Local<'s, v8::Object>) {
    let workers = v8::Object::new(scope);
    crate::methods!(scope, workers,
        "spawn" => spawn,
        "load" => load,
        "post" => post,
        "terminate" => terminate,
        "onEvent" => on_event,
    );
    bind::set(scope, natives, "workers", workers.into());
}

pub fn clear() {
    let page = PAGE.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for entry in page.workers.values() {
        entry.stop.now(&entry.tx);
    }
    drop(page);
}

fn on_event(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let callback = v8::Local::<v8::Function>::try_from(args.get(0)).ok().map(|f| v8::Global::new(scope, f));
    PAGE.with(|p| p.borrow_mut().on_event = callback);
}

fn spawn(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_u32(scope, &args, 0);
    let name = bind::arg_string(scope, &args, 1);
    let (tx, rx) = channel();
    let stop = Arc::new(Stop { stopping: Arc::new(AtomicBool::new(false)), isolate: Mutex::new(None) });
    PAGE.with(|p| {
        let mut p = p.borrow_mut();
        p.workers.insert(id, Entry { tx, stop, waiting: Some(Waiting { name, rx }) });
        p.queue.push_back(id);
    });
    start_waiting();
}

fn heap_budget() -> Option<Arc<AtomicUsize>> {
    PAGE.with(|p| p.borrow_mut().heap.get_or_insert_with(|| crate::host::web::heap_mb().map(|mb| Arc::new(AtomicUsize::new(mb << 20)))).clone())
}

fn next_waiting() -> Option<(u32, Waiting, Sender<Msg>, Arc<Stop>, Arc<AtomicUsize>)> {
    PAGE.with(|p| {
        let mut p = p.borrow_mut();
        if p.live >= MAX_LIVE {
            return None;
        }
        let in_flight = p.in_flight.clone();
        while let Some(id) = p.queue.pop_front() {
            let Some(entry) = p.workers.get_mut(&id) else { continue };
            let Some(waiting) = entry.waiting.take() else { continue };
            let parts = (id, waiting, entry.tx.clone(), entry.stop.clone(), in_flight);
            p.live += 1;
            return Some(parts);
        }
        None
    })
}

fn start_waiting() {
    while let Some((id, waiting, tx, stop, in_flight)) = next_waiting() {
        let label = if waiting.name.is_empty() { String::new() } else { format!(":{}", waiting.name) };
        let start = Start {
            id,
            name: format!("{}/worker-{id}{label}", rt::web_name()),
            page: rt::current().sender(),
            tx,
            rx: waiting.rx,
            stop,
            in_flight,
            heap: heap_budget(),
            threads: crate::host::web::worker_threads(),
            resources: crate::host::dom::resources(),
            backend: crate::host::backend::config(),
            platform: rt::current().platform(),
        };
        if let Err(e) = std::thread::Builder::new().name("acde-worker".into()).stack_size(STACK).spawn(move || start.run()) {
            log::error!("worker: {e}");
            finished(id);
        }
    }
}

fn with_entry<R>(id: u32, f: impl FnOnce(&Entry) -> R) -> Option<R> {
    PAGE.with(|p| p.borrow().workers.get(&id).map(f))
}

fn load(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_u32(scope, &args, 0);
    let url = bind::arg_string(scope, &args, 1);
    let source = bind::arg_string(scope, &args, 2);
    let module = bind::arg_bool(scope, &args, 3);
    let env = bind::arg_string(scope, &args, 4);
    with_entry(id, |entry| {
        _ = entry.tx.send(Msg::Task(Box::new(move |scope| inside(scope, Event::Load { url, source, module, env }))));
    });
}

fn post(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_u32(scope, &args, 0);
    let Some(tx) = with_entry(id, |entry| entry.tx.clone()) else { return };
    let in_flight = PAGE.with(|p| p.borrow().in_flight.clone());
    let Some(letter) = seal(scope, &args, 1, &in_flight) else { return };
    _ = tx.send(Msg::Task(Box::new(move |scope| inside(scope, Event::Message(letter)))));
}

fn terminate(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let id = bind::arg_u32(scope, &args, 0);
    let entry = PAGE.with(|p| {
        let mut p = p.borrow_mut();
        p.queue.retain(|q| *q != id);
        p.workers.remove(&id)
    });
    if let Some(entry) = entry {
        entry.stop.now(&entry.tx);
    }
}

fn finished(id: u32) {
    PAGE.with(|p| {
        let mut p = p.borrow_mut();
        p.live = p.live.saturating_sub(1);
        p.workers.remove(&id);
    });
    start_waiting();
}

enum Report {
    Message(Letter),
    Error { message: String, filename: String, line: u32, column: u32 },
    Exit,
}

fn outside(scope: &mut v8::PinScope, id: u32, report: Report) {
    if let Report::Exit = report {
        let known = PAGE.with(|p| p.borrow().workers.contains_key(&id));
        finished(id);
        if !known {
            return;
        }
    } else if !PAGE.with(|p| p.borrow().workers.contains_key(&id)) {
        return;
    }
    let Some(callback) = PAGE.with(|p| p.borrow().on_event.clone()) else { return };
    let id_value = v8::Integer::new_from_unsigned(scope, id).into();
    let (kind, payload): (&str, v8::Local<v8::Value>) = match report {
        Report::Message(letter) => match open(scope, letter) {
            Some(value) => ("message", value),
            None => ("messageerror", v8::undefined(scope).into()),
        },
        Report::Error { message, filename, line, column } => {
            let fields: [(&str, v8::Local<v8::Value>); 4] = [
                ("message", bind::string(scope, &message).into()),
                ("filename", bind::string(scope, &filename).into()),
                ("lineno", v8::Integer::new_from_unsigned(scope, line).into()),
                ("colno", v8::Integer::new_from_unsigned(scope, column).into()),
            ];
            ("error", bind::object(scope, &fields).into())
        }
        Report::Exit => ("exit", v8::undefined(scope).into()),
    };
    let kind = bind::string(scope, kind).into();
    bind::call(scope, &callback, &[id_value, kind, payload]);
}

struct Start {
    id: u32,
    name: String,
    page: Sender<Msg>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    stop: Arc<Stop>,
    in_flight: Arc<AtomicUsize>,
    heap: Option<Arc<AtomicUsize>>,
    threads: Option<Arc<crate::host::web::Threads>>,
    resources: Vec<crate::render::Resource>,
    backend: crate::host::backend::Config,
    platform: crate::host::platform::Shared,
}

enum Event {
    Load { url: String, source: String, module: bool, env: String },
    Message(Letter),
}

struct Inside {
    id: u32,
    page: Sender<Msg>,
    stopping: Arc<AtomicBool>,
    in_flight: Arc<AtomicUsize>,
    on_event: Option<v8::Global<v8::Function>>,
    early: Vec<Event>,
}

impl Start {
    fn run(self) {
        let (id, page) = (self.id, self.page.clone());
        let threads = self.threads.clone();
        let tid = threads.as_ref().map(|t| t.enter());
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.work()));
        if let Err(panic) = ran {
            let message = panic.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
            log::error!("[worker {id}] panic: {message}");
            let report = Report::Error { message: format!("the worker stopped: {message}"), filename: String::new(), line: 0, column: 0 };
            _ = page.send(Msg::Task(Box::new(move |scope| outside(scope, id, report))));
        }
        if let (Some(threads), Some(tid)) = (threads, tid) {
            threads.leave(tid);
        }
        _ = page.send(Msg::Task(Box::new(move |scope| outside(scope, id, Report::Exit))));
    }

    fn work(self) {
        let Start { id, name, page, tx, rx, stop, in_flight, heap, threads: _, resources, backend, platform } = self;
        if stop.stopping.load(Ordering::SeqCst) {
            return;
        }
        let reason = Arc::new(Mutex::new(None::<String>));
        let grant = match &heap {
            Some(budget) => match Grant::reserve(budget, reason.clone(), stop.stopping.clone()) {
                Some(grant) => Some(grant),
                None => {
                    let report =
                        Report::Error { message: "the page's workers already use all of its memory".into(), filename: String::new(), line: 0, column: 0 };
                    _ = page.send(Msg::Task(Box::new(move |scope| outside(scope, id, report))));
                    return;
                }
            },
            None => None,
        };
        let mut runtime = rt::Runtime::with_channel(grant.as_ref().map(|g| g.mb()), &name, platform, (tx, rx), false);
        let handle = runtime.isolate.thread_safe_handle();
        if let Ok(mut isolate) = stop.isolate.lock() {
            *isolate = Some(handle.clone());
        }
        if stop.stopping.load(Ordering::SeqCst) {
            return;
        }
        let guard = match &grant {
            Some(grant) => {
                grant.attach(&mut runtime, handle);
                None
            }
            None => Some(crate::host::web::HeapGuard::install(&mut runtime, handle, reason.clone(), stop.stopping.clone())),
        };
        crate::host::modules::set_loader(std::rc::Rc::new(crate::host::dom::WebLoader));
        crate::host::dom::adopt_resources(resources);
        SELF.with(|s| *s.borrow_mut() = Some(Inside { id, page: page.clone(), stopping: stop.stopping.clone(), in_flight, on_event: None, early: Vec::new() }));
        runtime.task(|scope| {
            let natives = v8::Object::new(scope);
            let host = crate::host::host::object(scope);
            bind::set(scope, natives, "host", host.into());
            crate::host::backend::install_config(scope, natives, backend);
            let worker = v8::Object::new(scope);
            crate::methods!(scope, worker,
                "post" => post_out,
                "onEvent" => on_inside_event,
                "close" => close,
                "error" => error_out,
            );
            bind::set(scope, natives, "worker", worker.into());
            crate::host::sys::boot_module(scope, crate::host::bundle::WORKER_URL, natives.into());
        });
        runtime.lp.keep_alive(true);
        let stopping = stop.stopping.clone();
        runtime.run_until(|| stopping.load(Ordering::SeqCst));
        if let Some(reason) = reason.lock().ok().and_then(|r| r.clone()) {
            log::warn!("[{name}] stopped: {reason}");
            let report = Report::Error { message: format!("the worker ran out of {reason}"), filename: String::new(), line: 0, column: 0 };
            _ = page.send(Msg::Task(Box::new(move |scope| outside(scope, id, report))));
        }
        SELF.with(|s| s.borrow_mut().take());
        crate::host::backend::clear();
        crate::host::offload::clear();
        crate::host::host::clear();
        bind::clear_promises();
        drop(runtime);
        drop((guard, grant));
    }
}

fn inside(scope: &mut v8::PinScope, event: Event) {
    let callback = SELF.with(|s| {
        let mut s = s.borrow_mut();
        let inside = s.as_mut()?;
        if inside.stopping.load(Ordering::SeqCst) {
            return None;
        }
        match &inside.on_event {
            Some(callback) => Some((callback.clone(), event)),
            None => {
                inside.early.push(event);
                None
            }
        }
    });
    if let Some((callback, event)) = callback {
        dispatch(scope, &callback, event);
    }
}

fn dispatch(scope: &mut v8::PinScope, callback: &v8::Global<v8::Function>, event: Event) {
    let (kind, payload): (&str, v8::Local<v8::Value>) = match event {
        Event::Load { url, source, module, env } => {
            let fields: [(&str, v8::Local<v8::Value>); 4] = [
                ("url", bind::string(scope, &url).into()),
                ("source", bind::string(scope, &source).into()),
                ("module", v8::Boolean::new(scope, module).into()),
                ("env", bind::string(scope, &env).into()),
            ];
            ("load", bind::object(scope, &fields).into())
        }
        Event::Message(letter) => match open(scope, letter) {
            Some(value) => ("message", value),
            None => ("messageerror", v8::undefined(scope).into()),
        },
    };
    let kind = bind::string(scope, kind).into();
    bind::call(scope, callback, &[kind, payload]);
}

fn on_inside_event(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        return bind::throw_type_error(scope, "worker.onEvent: not a function");
    };
    let callback = v8::Global::new(scope, callback);
    let early = SELF.with(|s| {
        let mut s = s.borrow_mut();
        let inside = s.as_mut()?;
        inside.on_event = Some(callback.clone());
        Some(std::mem::take(&mut inside.early))
    });
    for event in early.unwrap_or_default() {
        dispatch(scope, &callback, event);
    }
}

fn post_out(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Some((id, page, in_flight)) = SELF.with(|s| s.borrow().as_ref().map(|i| (i.id, i.page.clone(), i.in_flight.clone()))) else { return };
    let Some(letter) = seal(scope, &args, 0, &in_flight) else { return };
    _ = page.send(Msg::Task(Box::new(move |scope| outside(scope, id, Report::Message(letter)))));
}

fn error_out(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Some((id, page)) = SELF.with(|s| s.borrow().as_ref().map(|i| (i.id, i.page.clone()))) else { return };
    let report = Report::Error {
        message: bind::arg_string(scope, &args, 0),
        filename: bind::arg_string(scope, &args, 1),
        line: bind::arg_u32(scope, &args, 2),
        column: bind::arg_u32(scope, &args, 3),
    };
    _ = page.send(Msg::Task(Box::new(move |scope| outside(scope, id, report))));
}

fn close(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    SELF.with(|s| {
        if let Some(inside) = s.borrow().as_ref() {
            inside.stopping.store(true, Ordering::SeqCst);
        }
    });
    rt::current().exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workers_share_the_page_heap_budget() {
        let budget = Arc::new(AtomicUsize::new(100 << 20));
        let stop = || Arc::new(AtomicBool::new(false));
        let reason = || Arc::new(Mutex::new(None));
        let a = Grant::reserve(&budget, reason(), stop()).unwrap();
        assert_eq!(a.mb(), 64);
        let b = Grant::reserve(&budget, reason(), stop()).unwrap();
        assert_eq!(b.mb(), 36, "the second gets what is left");
        assert!(Grant::reserve(&budget, reason(), stop()).is_none(), "nothing left for a third");
        assert_eq!(take(&budget, 1, 1), None);
        drop(a);
        assert_eq!(budget.load(Ordering::Acquire), 64 << 20, "a worker's heap comes back when it ends");
        drop(b);
        assert_eq!(budget.load(Ordering::Acquire), 100 << 20);
    }

    #[test]
    fn messages_in_flight_share_one_cap_and_give_back_when_delivered_or_dropped() {
        let counter = Arc::new(AtomicUsize::new(0));
        let big = InFlight::take(&counter, MAX_IN_FLIGHT - 100).unwrap();
        assert!(InFlight::take(&counter, 101).is_none(), "over the cap while the first is queued");
        let small = InFlight::take(&counter, 100).unwrap();
        drop(big);
        assert_eq!(counter.load(Ordering::Acquire), 100);
        drop(small);
        assert_eq!(counter.load(Ordering::Acquire), 0);
    }
}
