use std::cell::{Cell, RefCell};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use crate::napi::{LocalTask, ScopeTask};

pub enum Msg {
    Task(ScopeTask),
    V8(v8::Task),
    Delayed(Instant, v8::Task),
    Wake,
}

enum Job {
    Js { callback: v8::Global<v8::Function>, refed: bool },
    V8(v8::Task),
    Local(LocalTask),
}

#[derive(Default)]
struct Timers {
    queue: BinaryHeap<Reverse<(Instant, u32)>>,
    live: crate::host::bind::Slab<Job>,
}

impl Timers {
    fn add(&mut self, at: Instant, job: Job) -> u32 {
        let id = self.live.add(job);
        self.queue.push(Reverse((at, id)));
        id
    }

    fn js(&mut self, id: u32) -> Option<&mut bool> {
        match self.live.get_mut(&id) {
            Some(Job::Js { refed, .. }) => Some(refed),
            _ => None,
        }
    }
}

type Hook = Rc<dyn Fn(&mut v8::PinScope)>;

type Rejection = (v8::Global<v8::Promise>, v8::Global<v8::Value>, Option<String>);

pub struct Loop {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    local: RefCell<VecDeque<LocalTask>>,
    timers: RefCell<Timers>,
    ops: Arc<AtomicIsize>,
    refs: Cell<isize>,
    rejections: RefCell<Vec<Rejection>>,
    after_task: RefCell<Option<Hook>>,
    hook_owed: Cell<bool>,
    busy: Arc<AtomicU64>,
    reporter: RefCell<Option<v8::Global<v8::Function>>>,
    origins: RefCell<Vec<(v8::Global<v8::Value>, String)>>,
    exit: Cell<Option<i32>>,
    started: Instant,
    name: String,
    platform: crate::host::platform::Shared,
}

thread_local! {
    static LOOP: RefCell<Option<Rc<Loop>>> = const { RefCell::new(None) };
}

pub fn current() -> Rc<Loop> {
    LOOP.with(|l| l.borrow().clone()).expect("libacde: no event loop on this thread")
}

impl Loop {
    pub fn platform(&self) -> crate::host::platform::Shared {
        self.platform.clone()
    }

    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    pub fn spawner(&self) -> crate::napi::V8CrossThreadTaskSpawner {
        let tx = self.tx.clone();
        crate::napi::V8CrossThreadTaskSpawner::new(move |task| _ = tx.send(Msg::Task(task)))
    }

    pub fn ops(&self) -> crate::napi::ExternalOpsTracker {
        let (ops, tx) = (self.ops.clone(), self.tx.clone());
        crate::napi::ExternalOpsTracker::new(move |more| {
            if more {
                ops.fetch_add(1, Ordering::SeqCst);
            } else if ops.fetch_sub(1, Ordering::SeqCst) == 1 {
                _ = tx.send(Msg::Wake);
            }
        })
    }

    pub fn spawn_local(&self, task: impl FnOnce(&mut v8::PinScope) + 'static) {
        self.local.borrow_mut().push_back(Box::new(task));
    }

    pub fn keep_alive(&self, on: bool) {
        self.refs.set(self.refs.get() + if on { 1 } else { -1 });
    }

    pub fn set_after_task(&self, hook: impl Fn(&mut v8::PinScope) + 'static) {
        *self.after_task.borrow_mut() = Some(Rc::new(hook));
    }

    pub fn busy(&self) -> Arc<AtomicU64> {
        self.busy.clone()
    }

    fn mark_busy(&self) {
        self.busy.store(busy_stamp(), Ordering::Relaxed);
    }

    pub fn set_reporter(&self, reporter: Option<v8::Global<v8::Function>>) {
        *self.reporter.borrow_mut() = reporter;
    }

    pub fn exit(&self, code: i32) {
        self.exit.set(Some(code));
    }

    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    pub fn from_log_ms(&self, log_ms: f64) -> f64 {
        let origin = crate::log::start();
        let after = self.started.checked_duration_since(origin).map(|d| d.as_secs_f64() * 1000.0);
        log_ms - after.unwrap_or_else(|| -origin.duration_since(self.started).as_secs_f64() * 1000.0)
    }

    pub fn set_timer(&self, ms: f64, callback: v8::Global<v8::Function>, refed: bool) -> u32 {
        let at = Instant::now() + Duration::from_secs_f64(ms.max(0.0) / 1000.0);
        self.timers.borrow_mut().add(at, Job::Js { callback, refed })
    }

    pub fn delay(&self, after: Duration, task: impl FnOnce(&mut v8::PinScope) + 'static) {
        self.timers.borrow_mut().add(Instant::now() + after, Job::Local(Box::new(task)));
    }

    pub fn clear_timer(&self, id: u32) {
        let mut timers = self.timers.borrow_mut();
        if timers.js(id).is_some() {
            timers.live.remove(&id);
        }
    }

    pub fn ref_timer(&self, id: u32, refed: bool) {
        if let Some(on) = self.timers.borrow_mut().js(id) {
            *on = refed;
        }
    }

    fn due(&self, now: Instant) -> Option<Job> {
        let mut timers = self.timers.borrow_mut();
        while let Some(&Reverse((at, id))) = timers.queue.peek() {
            if at > now {
                return None;
            }
            timers.queue.pop();
            if let Some(job) = timers.live.remove(&id) {
                return Some(job);
            }
        }
        None
    }

    fn next_deadline(&self) -> Option<Instant> {
        let mut timers = self.timers.borrow_mut();
        while let Some(&Reverse((at, id))) = timers.queue.peek() {
            if timers.live.contains_key(&id) {
                return Some(at);
            }
            timers.queue.pop();
        }
        None
    }

    fn alive(&self) -> bool {
        self.refs.get() > 0
            || self.ops.load(Ordering::SeqCst) > 0
            || !self.local.borrow().is_empty()
            || self.timers.borrow().live.values().any(|job| matches!(job, Job::Js { refed: true, .. }))
    }
}

pub struct Runtime {
    pub context: v8::Global<v8::Context>,
    pub lp: Rc<Loop>,
    pub isolate: v8::OwnedIsolate,
}

impl Runtime {
    pub fn new(heap_mb: Option<usize>, name: &str, platform: crate::host::platform::Shared) -> Self {
        Self::with_channel(heap_mb, name, platform, channel(), true)
    }

    pub fn with_channel(
        heap_mb: Option<usize>,
        name: &str,
        platform: crate::host::platform::Shared,
        (tx, rx): (Sender<Msg>, Receiver<Msg>),
        inspect: bool,
    ) -> Self {
        crate::host::engine::init();
        let mut params = v8::CreateParams::default();
        if let Some(mb) = heap_mb {
            params = params.heap_limits(0, mb * 1024 * 1024);
        }
        let mut isolate = v8::Isolate::new(params);
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
        isolate.set_promise_reject_callback(promise_reject);
        isolate.set_host_import_module_dynamically_callback(crate::host::modules::dynamic_import);
        isolate.set_host_initialize_import_meta_object_callback(crate::host::modules::import_meta);
        crate::host::engine::register(isolate_ptr(&mut isolate), tx.clone());
        let lp = Rc::new(Loop {
            tx,
            rx,
            local: Default::default(),
            timers: Default::default(),
            ops: Default::default(),
            refs: Cell::new(0),
            rejections: Default::default(),
            after_task: Default::default(),
            hook_owed: Cell::new(false),
            busy: Default::default(),
            reporter: Default::default(),
            origins: Default::default(),
            exit: Cell::new(None),
            started: Instant::now(),
            name: name.to_owned(),
            platform,
        });
        LOOP.with(|l| *l.borrow_mut() = Some(lp.clone()));
        let context = {
            v8::scope!(let scope, &mut isolate);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            if inspect {
                crate::host::inspector::start(scope, name);
            }
            crate::host::profile::start(name);
            v8::Global::new(scope, context)
        };
        Runtime { context, lp, isolate }
    }

    pub fn task<R>(&mut self, f: impl FnOnce(&mut v8::PinScope) -> R) -> R {
        self.lp.mark_busy();
        v8::scope_with_context!(let scope, &mut self.isolate, &self.context);
        run_task(scope, f)
    }

    fn v8_task(&mut self, task: v8::Task) {
        self.lp.mark_busy();
        v8::scope_with_context!(let scope, &mut self.isolate, &self.context);
        let started = Instant::now();
        task.run();
        let ms = crate::log::since_ms(started);
        if ms >= LONG_TASK_MS {
            log::debug!(target: "perf::task", "{} v8 ms={ms:.3}", web_name());
        }
        if scope.is_execution_terminating() {
            return;
        }
        scope.perform_microtask_checkpoint();
        report_rejections(scope);
        self.lp.hook_owed.set(true);
    }

    fn before_wait(&mut self) {
        if !self.lp.hook_owed.get() {
            return;
        }
        v8::scope_with_context!(let scope, &mut self.isolate, &self.context);
        if scope.is_execution_terminating() {
            return;
        }
        after_task(scope);
    }

    pub fn run_until(&mut self, stop: impl Fn() -> bool) -> i32 {
        let lp = self.lp.clone();
        loop {
            if let Some(code) = lp.exit.get() {
                return code;
            }
            if stop() {
                return 1;
            }
            let now = Instant::now();
            while let Some(job) = lp.due(now) {
                match job {
                    Job::Js { callback, .. } => self.task(|scope| _ = crate::host::bind::call(scope, &callback, &[])),
                    Job::V8(task) => self.v8_task(task),
                    Job::Local(task) => self.task(task),
                }
                if lp.exit.get().is_some() {
                    break;
                }
            }
            let local: Vec<_> = lp.local.borrow_mut().drain(..).collect();
            for task in local {
                if lp.exit.get().is_some() {
                    break;
                }
                self.task(task);
            }
            // only what is queued now: a task that posts another (a busy page's `waitThen` finding events ready again) would keep
            // this drain going forever, and timers that came due meanwhile never ran
            let queued: Vec<Msg> = std::iter::from_fn(|| lp.rx.try_recv().ok()).collect();
            for msg in queued {
                if lp.exit.get().is_some() {
                    break;
                }
                self.message(msg);
            }
            if lp.exit.get().is_some() || !lp.local.borrow().is_empty() {
                continue;
            }
            if stop() {
                return 1;
            }
            if !lp.alive() {
                // a task handled above may have posted more: the loop ends only once nothing is queued either
                match lp.rx.try_recv() {
                    Ok(msg) => {
                        self.message(msg);
                        continue;
                    }
                    Err(_) => return 0,
                }
            }
            self.before_wait();
            if !lp.local.borrow().is_empty() || lp.exit.get().is_some() {
                continue;
            }
            lp.busy.store(0, Ordering::Relaxed);
            let wait = lp.next_deadline().map_or(Duration::MAX, |at| at.saturating_duration_since(Instant::now()));
            let msg = match lp.rx.recv_timeout(wait) {
                Ok(msg) => msg,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return 0,
            };
            self.message(msg);
        }
    }

    fn message(&mut self, msg: Msg) {
        match msg {
            Msg::Task(task) => self.task(task),
            Msg::V8(task) => self.v8_task(task),
            Msg::Delayed(at, task) => _ = self.lp.timers.borrow_mut().add(at, Job::V8(task)),
            Msg::Wake => {}
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        crate::host::profile::stop();
        crate::host::inspector::stop();
        crate::host::engine::unregister(isolate_ptr(&mut self.isolate));
        LOOP.with(|l| *l.borrow_mut() = None);
    }
}

fn isolate_ptr(isolate: &mut v8::Isolate) -> *mut std::ffi::c_void {
    unsafe { std::mem::transmute::<v8::UnsafeRawIsolatePtr, *mut std::ffi::c_void>(isolate.as_raw_isolate_ptr()) }
}

const LONG_TASK_MS: f64 = 8.0;

pub fn busy_stamp() -> u64 {
    crate::log::start().elapsed().as_millis() as u64 + 1
}

pub fn web_name() -> String {
    LOOP.with(|l| l.borrow().as_ref().map_or_else(|| "broker".to_owned(), |l| l.name.clone()))
}

fn run_task<R>(scope: &mut v8::PinScope, f: impl FnOnce(&mut v8::PinScope) -> R) -> R {
    let started = log::log_enabled!(target: "perf::task", log::Level::Debug).then(Instant::now);
    let result = {
        v8::tc_scope!(let tc, scope);
        let result = f(tc);
        if let Some(exception) = tc.exception().filter(|_| !tc.has_terminated()) {
            let message = tc.message();
            let at = located(tc, message);
            let origin = thrown_at(tc, message, exception);
            report_to(tc, exception, None, origin, at);
        }
        result
    };
    if scope.is_execution_terminating() {
        return result;
    }
    scope.perform_microtask_checkpoint();
    report_rejections(scope);
    let hook = started.map(|_| Instant::now());
    after_task(scope);
    if let (Some(started), Some(hook)) = (started, hook) {
        let ms = crate::log::since_ms(started);
        if ms >= LONG_TASK_MS {
            log::debug!(target: "perf::task", "{} ms={ms:.3} hook={:.3}", web_name(), crate::log::since_ms(hook));
        }
    }
    result
}

fn report_rejections(scope: &mut v8::PinScope) {
    let lp = current();
    let rejections: Vec<_> = lp.rejections.borrow_mut().drain(..).collect();
    for (promise, reason, origin) in rejections {
        let (promise, reason) = (v8::Local::new(scope, promise), v8::Local::new(scope, reason));
        report_to(scope, reason, Some(promise), origin, None);
    }
}

fn after_task(scope: &mut v8::PinScope) {
    let lp = current();
    lp.hook_owed.set(false);
    let hook = lp.after_task.borrow().clone();
    if let Some(hook) = hook {
        let thrown = {
            v8::tc_scope!(let tc, scope);
            hook(tc);
            let message = tc.message();
            tc.exception().filter(|_| !tc.has_terminated()).map(|e| (v8::Global::new(tc, e), thrown_at(tc, message, e)))
        };
        if let Some((exception, origin)) = thrown {
            let exception = v8::Local::new(scope, exception);
            report_to(scope, exception, None, origin, None);
        }
        scope.perform_microtask_checkpoint();
    }
}

pub fn report(scope: &mut v8::PinScope, error: v8::Local<v8::Value>) {
    report_to(scope, error, None, None, None);
}

struct Located {
    file: String,
    line: i32,
    column: i32,
}

fn located(scope: &mut v8::PinScope, message: Option<v8::Local<v8::Message>>) -> Option<Located> {
    let message = message?;
    let file = message.get_script_resource_name(scope).map(|r| r.to_rust_string_lossy(scope)).unwrap_or_default();
    Some(Located { file, line: message.get_line_number(scope)? as i32, column: message.get_start_column() as i32 + 1 })
}

fn report_to(scope: &mut v8::PinScope, error: v8::Local<v8::Value>, promise: Option<v8::Local<v8::Promise>>, origin: Option<String>, at: Option<Located>) {
    if scope.is_execution_terminating() {
        return;
    }
    let lp = current();
    let reporter = lp.reporter.borrow().clone();
    if let Some(reporter) = reporter {
        if let Some(origin) = &origin {
            lp.origins.borrow_mut().push((v8::Global::new(scope, error), origin.clone()));
        }
        let handled = {
            v8::tc_scope!(let tc, scope);
            let args: Vec<v8::Local<v8::Value>> = match (promise, at) {
                (Some(promise), _) => vec![error, promise.into()],
                (None, Some(at)) => {
                    let fields: [(&str, v8::Local<v8::Value>); 3] = [
                        ("filename", crate::host::bind::string(tc, &at.file).into()),
                        ("lineno", v8::Integer::new(tc, at.line).into()),
                        ("colno", v8::Integer::new(tc, at.column).into()),
                    ];
                    let at = crate::host::bind::object(tc, &fields);
                    vec![error, v8::undefined(tc).into(), at.into()]
                }
                (None, None) => vec![error],
            };
            crate::host::bind::call(tc, &reporter, &args).is_some()
        };
        if origin.is_some() {
            lp.origins.borrow_mut().pop();
        }
        if handled {
            return;
        }
    }
    let kind = if promise.is_some() { "unhandled rejection" } else { "uncaught" };
    log::error!(target: "js", "[{}] {kind}: {}", lp.name, describe_at(scope, error, origin.as_deref()));
}

pub fn origin_of(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<String> {
    let lp = current();
    let origins = lp.origins.borrow();
    origins.iter().rev().find(|(v, _)| v8::Local::new(scope, v).strict_equals(value)).map(|(_, o)| o.clone())
}

pub fn describe_at(scope: &mut v8::PinScope, value: v8::Local<v8::Value>, origin: Option<&str>) -> String {
    let text = describe(scope, value);
    match origin {
        Some(origin) => format!("{text}\n{origin}"),
        None => text,
    }
}

const SHOWN: usize = 400;

pub fn describe(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> String {
    v8::tc_scope!(let tc, scope);
    if let Ok(object) = v8::Local::<v8::Object>::try_from(value) {
        let key = v8::String::new(tc, "stack").unwrap();
        if let Some(stack) = object.get(tc, key.into())
            && stack.is_string()
        {
            return stack.to_rust_string_lossy(tc);
        }
        tc.reset();
    }
    let shown = inspect(tc, value);
    tc.reset();
    format!("{shown} (not an Error, so no stack)")
}

fn clip(mut text: String) -> String {
    if text.len() > SHOWN {
        let mut end = SHOWN;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

fn json_text(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<String> {
    v8::tc_scope!(let tc, scope);
    let text = v8::json::stringify(tc, value).map(|s| s.to_rust_string_lossy(tc));
    tc.reset();
    text.filter(|t| t != "undefined")
}

fn inspect(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> String {
    if value.is_null() || value.is_undefined() {
        return value.to_rust_string_lossy(scope);
    }
    if let Ok(symbol) = v8::Local::<v8::Symbol>::try_from(value) {
        let description = symbol.description(scope);
        let description = if description.is_undefined() { String::new() } else { description.to_rust_string_lossy(scope) };
        return format!("symbol Symbol({description})");
    }
    if value.is_string() {
        return format!("string {}", clip(json_text(scope, value).unwrap_or_else(|| value.to_rust_string_lossy(scope))));
    }
    let kind = value.type_of(scope).to_rust_string_lossy(scope);
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        return format!("{kind} {}", value.to_rust_string_lossy(scope));
    };
    if let Ok(function) = v8::Local::<v8::Function>::try_from(value) {
        let name = function.get_name(scope).to_rust_string_lossy(scope);
        return format!("function {}", if name.is_empty() { "(anonymous)" } else { &name });
    }
    let class = object.get_constructor_name().to_rust_string_lossy(scope);
    let json = json_text(scope, value);
    let mut extra = Vec::new();
    for name in ["name", "message", "type", "code", "reason"] {
        if json.as_deref().is_some_and(|j| j.contains(&format!("\"{name}\":"))) {
            continue;
        }
        let key = v8::String::new(scope, name).unwrap();
        let Some(field) = object.get(scope, key.into()) else { continue };
        if field.is_string() || field.is_number() || field.is_boolean() {
            let shown = json_text(scope, field).unwrap_or_else(|| field.to_rust_string_lossy(scope));
            extra.push(format!("{name}: {shown}"));
        }
    }
    let body = match json {
        Some(json) if json != "{}" || extra.is_empty() => json,
        _ => format!("{{{}}}", std::mem::take(&mut extra).join(", ")),
    };
    let tail = if extra.is_empty() { String::new() } else { format!(" ({})", extra.join(", ")) };
    clip(format!("{class} {body}{tail}"))
}

fn thrown_at(scope: &mut v8::PinScope, message: Option<v8::Local<v8::Message>>, exception: v8::Local<v8::Value>) -> Option<String> {
    if exception.is_native_error() {
        return None;
    }
    let message = message?;
    let resource = message.get_script_resource_name(scope).map(|r| r.to_rust_string_lossy(scope)).unwrap_or_default();
    let line = message.get_line_number(scope)?;
    let column = message.get_start_column() + 1;
    Some(format!("  thrown at {resource}:{line}:{column}"))
}

fn stack_here(scope: &mut v8::PinScope) -> Option<String> {
    let trace = v8::StackTrace::current_stack_trace(scope, 8)?;
    let frames: Vec<String> = (0..trace.get_frame_count())
        .filter_map(|i| trace.get_frame(scope, i))
        .map(|frame| {
            let function = frame.get_function_name(scope).map(|f| f.to_rust_string_lossy(scope)).filter(|f| !f.is_empty());
            let script = frame.get_script_name_or_source_url(scope).map(|s| s.to_rust_string_lossy(scope)).unwrap_or_default();
            let at = format!("{script}:{}:{}", frame.get_line_number(), frame.get_column());
            match function {
                Some(function) => format!("    at {function} ({at})"),
                None => format!("    at {at}"),
            }
        })
        .collect();
    (!frames.is_empty()).then(|| format!("  rejected at:\n{}", frames.join("\n")))
}

extern "C" fn promise_reject(message: v8::PromiseRejectMessage) {
    v8::callback_scope!(unsafe scope, &message);
    let promise = message.get_promise();
    let lp = current();
    match message.get_event() {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
            let reason = message.get_value().unwrap_or_else(|| v8::undefined(scope).into());
            let origin = if reason.is_native_error() { None } else { stack_here(scope) };
            let entry = (v8::Global::new(scope, promise), v8::Global::new(scope, reason), origin);
            lp.rejections.borrow_mut().push(entry);
        }
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
            let promise = v8::Global::new(scope, promise);
            lp.rejections.borrow_mut().retain(|(p, _, _)| *p != promise);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(scope: &mut v8::PinScope, code: &str) -> Option<()> {
        let code = v8::String::new(scope, code)?;
        v8::Script::compile(scope, code, None)?.run(scope).map(|_| ())
    }

    #[test]
    fn terminate_execution_stops_a_wasm_loop() {
        let mut rt = Runtime::new(None, "wasm-loop", std::sync::Arc::new(crate::host::platform::Headless));
        let handle = rt.isolate.thread_safe_handle();
        let started = Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            handle.terminate_execution();
        });
        let spin = "new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0, \
                    7, 8, 1, 4, 115, 112, 105, 110, 0, 0, 10, 9, 1, 7, 0, 3, 64, 12, 0, 11, 11]))).exports.spin()";
        let finished = rt.task(|scope| run(scope, spin).is_some());
        assert!(!finished, "the loop never returns on its own");
        assert!(started.elapsed() < Duration::from_secs(5), "and termination ends it");
        rt.isolate.cancel_terminate_execution();
        assert!(rt.task(|scope| run(scope, "1").is_some()), "the isolate runs again afterwards");
    }

    #[test]
    fn termination_in_the_end_of_task_hook_is_not_a_page_error() {
        let mut rt = Runtime::new(None, "termination", std::sync::Arc::new(crate::host::platform::Headless));
        rt.task(|scope| {
            let code = v8::String::new(scope, "globalThis.reports = []; (e) => { globalThis.reports.push(String(e)) }").expect("source");
            let reporter = v8::Script::compile(scope, code, None).and_then(|s| s.run(scope)).expect("reporter");
            let reporter = v8::Local::<v8::Function>::try_from(reporter).expect("function");
            current().set_reporter(Some(v8::Global::new(scope, reporter)));
        });
        rt.lp.set_after_task(|scope| {
            let handle = scope.thread_safe_handle();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                handle.terminate_execution();
            });
            let _ = run(scope, "for (;;) {}");
        });
        rt.task(|scope| _ = run(scope, "1"));
        rt.lp.set_after_task(|_| {});
        rt.task(|scope| _ = run(scope, "Promise.reject(new Error('real'))"));
        let reports = rt.task(|scope| {
            let code = v8::String::new(scope, "globalThis.reports.join('|')").expect("source");
            let value = v8::Script::compile(scope, code, None).and_then(|s| s.run(scope)).expect("reports");
            value.to_rust_string_lossy(scope)
        });
        assert_eq!(reports, "Error: real", "only the real rejection is reported, not the termination");
    }

    /// A busy page re-posts its frame task while it handles one (`waitThen` finds events ready and sends `wake` again); timers
    /// that came due meanwhile still run between those tasks instead of waiting until the page goes quiet.
    #[test]
    fn timers_run_between_tasks_that_post_more_tasks() {
        use std::sync::atomic::AtomicU32;
        const LIMIT: u32 = 200;
        let mut rt = Runtime::new(None, "busy", std::sync::Arc::new(crate::host::platform::Headless));
        rt.lp.keep_alive(true);
        let posted = Arc::new(AtomicU32::new(0));
        let fired_after = Rc::new(Cell::new(None::<u32>));
        fn frame(posted: Arc<AtomicU32>) -> ScopeTask {
            Box::new(move |_| {
                std::thread::sleep(Duration::from_millis(2));
                if posted.fetch_add(1, Ordering::SeqCst) + 1 < LIMIT {
                    _ = current().sender().send(Msg::Task(frame(posted.clone())));
                }
            })
        }
        let (seen, at) = (posted.clone(), fired_after.clone());
        rt.lp.delay(Duration::from_millis(1), move |_| at.set(Some(seen.load(Ordering::SeqCst))));
        _ = rt.lp.sender().send(Msg::Task(frame(posted.clone())));
        let (done, stop_at) = (posted.clone(), fired_after.clone());
        rt.run_until(move || stop_at.get().is_some() || done.load(Ordering::SeqCst) >= LIMIT);
        let fired_after = fired_after.get().expect("the timer ran before the busy tasks ended");
        assert!(fired_after <= 2, "the 1 ms timer waited for {fired_after} busy tasks of 2 ms");
    }

    /// Tasks that keep posting tasks hold the loop open even with nothing else alive: it ends once nothing is queued.
    #[test]
    fn the_loop_ends_only_when_nothing_is_queued() {
        use std::sync::atomic::AtomicU32;
        const LIMIT: u32 = 50;
        let mut rt = Runtime::new(None, "chain", std::sync::Arc::new(crate::host::platform::Headless));
        let posted = Arc::new(AtomicU32::new(0));
        fn link(posted: Arc<AtomicU32>) -> ScopeTask {
            Box::new(move |_| {
                if posted.fetch_add(1, Ordering::SeqCst) + 1 < LIMIT {
                    _ = current().sender().send(Msg::Task(link(posted.clone())));
                }
            })
        }
        _ = rt.lp.sender().send(Msg::Task(link(posted.clone())));
        assert_eq!(rt.run_until(|| false), 0);
        assert_eq!(posted.load(Ordering::SeqCst), LIMIT, "every task in the chain ran before the loop ended");
    }
}
