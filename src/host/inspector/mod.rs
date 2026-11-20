use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use v8::UniquePtr;
use v8::inspector::{
    Channel, ChannelImpl, StringBuffer, StringView, V8Inspector, V8InspectorClient, V8InspectorClientImpl, V8InspectorClientTrustLevel, V8InspectorSession,
};

use crate::host::rt;

mod devtools;
mod ignore;
mod page;

pub use page::{install as page_handler, reply as page_reply};

const GROUP: i32 = 1;

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

struct Client {
    isolate: v8::UnsafeRawIsolatePtr,
    context: v8::Global<v8::Context>,
}

impl V8InspectorClientImpl for Client {
    fn run_message_loop_on_pause(&self, context_group_id: i32) {
        if context_group_id == GROUP {
            paused(self.isolate, &self.context);
        }
    }

    fn quit_message_loop_on_pause(&self) {
        STATE.with(|s| s.borrow().as_ref().map(|s| s.pause.quit.set(true)));
    }

    fn ensure_default_context_in_group(&self, context_group_id: i32) -> Option<v8::Local<'_, v8::Context>> {
        if context_group_id != GROUP {
            return None;
        }
        let mut raw = self.isolate;
        let isolate = unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut raw) };
        v8::callback_scope!(unsafe scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        Some(unsafe { std::mem::transmute::<v8::Local<'_, v8::Context>, v8::Local<'_, v8::Context>>(context) })
    }
}

#[derive(Default)]
struct Inbox {
    reply: RefCell<Option<String>>,
    scripts: RefCell<HashMap<String, String>>,
}

struct Replies(Rc<Inbox>);

impl ChannelImpl for Replies {
    fn send_response(&self, _: i32, mut message: UniquePtr<StringBuffer>) {
        *self.0.reply.borrow_mut() = message.as_mut().map(|m| m.string().to_string());
    }

    fn send_notification(&self, mut message: UniquePtr<StringBuffer>) {
        let Some(text) = message.as_mut().map(|m| m.string().to_string()) else { return };
        if !text.contains("\"Debugger.scriptParsed\"") {
            return;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return };
        let params = &value["params"];
        if let (Some(url), Some(id)) = (params["url"].as_str(), params["scriptId"].as_str())
            && !url.is_empty()
        {
            self.0.scripts.borrow_mut().insert(url.to_owned(), id.to_owned());
        }
    }

    fn flush_protocol_notifications(&self) {}
}

pub type Sink = Arc<dyn Fn(String) + Send + Sync>;

struct Forward(Sink, page::Resolving);

impl ChannelImpl for Forward {
    fn send_response(&self, _: i32, mut message: UniquePtr<StringBuffer>) {
        if let Some(text) = message.as_mut().map(|m| m.string().to_string()) {
            (self.0)(page::resolved(&self.1, text));
        }
    }

    fn send_notification(&self, mut message: UniquePtr<StringBuffer>) {
        if let Some(text) = message.as_mut().map(|m| m.string().to_string()) {
            (self.0)(text);
        }
    }

    fn flush_protocol_notifications(&self) {}
}

enum Posted {
    Connect(u32, Sink),
    Message(u32, String),
    Page(u32, String),
    ExpectHandler,
    Disconnect(u32),
}

#[derive(Default)]
struct Pending {
    queue: Mutex<VecDeque<Posted>>,
    ready: Condvar,
}

impl Pending {
    fn push(&self, posted: Posted) -> bool {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let first = queue.is_empty();
        queue.push_back(posted);
        self.ready.notify_all();
        first
    }

    fn pop(&self) -> Option<Posted> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
    }

    fn wait(&self, timeout: Duration) {
        let queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        drop(self.ready.wait_timeout_while(queue, timeout, |q| q.is_empty()));
    }

    fn take_pauses(&self) -> Vec<(u32, String)> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let mut pauses = Vec::new();
        queue.retain(|posted| match posted {
            Posted::Message(id, text) if method(text) == Some("Debugger.pause") => {
                pauses.push((*id, text.clone()));
                false
            }
            _ => true,
        });
        pauses
    }
}

fn method(text: &str) -> Option<&str> {
    let start = text.find("\"method\"")? + 8;
    let rest = text[start..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    rest.split('"').next()
}

#[derive(Clone)]
pub struct Remote {
    pending: Arc<Pending>,
    next: Arc<AtomicU32>,
    wake: Sender<rt::Msg>,
    isolate: v8::IsolateHandle,
}

impl Remote {
    pub fn connect(&self, sink: Sink) -> u32 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.post(Posted::Connect(id, sink));
        id
    }

    pub fn send(&self, session: u32, message: String) {
        let pause = method(&message) == Some("Debugger.pause");
        self.post(Posted::Message(session, message));
        if pause {
            self.isolate.request_interrupt(interrupted, std::ptr::null_mut());
        }
    }

    pub fn page(&self, session: u32, message: String) {
        self.post(Posted::Page(session, message));
    }

    pub fn disconnect(&self, session: u32) {
        self.post(Posted::Disconnect(session));
    }

    pub fn expect_handler(&self) {
        self.post(Posted::ExpectHandler);
    }

    fn post(&self, posted: Posted) {
        if self.pending.push(posted) {
            _ = self.wake.send(rt::Msg::Task(Box::new(|scope: &mut v8::PinScope| dispatch_posted(scope))));
        }
    }
}

struct Session {
    session: V8InspectorSession,
    inbox: Rc<Inbox>,
    calls: Cell<i32>,
}

#[derive(Default)]
struct Pause {
    quit: Cell<bool>,
    depth: Cell<u32>,
    stopping: RefCell<Option<Arc<AtomicBool>>>,
}

struct State {
    main: Session,
    pause: Pause,
    isolate: v8::IsolateHandle,
    extra: RefCell<HashMap<u32, Rc<V8InspectorSession>>>,
    pending: Arc<Pending>,
    next: Arc<AtomicU32>,
    wake: Sender<rt::Msg>,
    name: String,
    listed: bool,
    page: page::PageSide,
    inspector: V8Inspector,
}

pub fn wanted() -> bool {
    std::env::var_os("ACDE_PROFILE_DIR").is_some() || dev()
}

fn dev() -> bool {
    crate::runtime::dev_mode()
}

const TAIL: &str = "\n;";

pub fn module_source<'a>(url: &str, source: &'a str) -> std::borrow::Cow<'a, str> {
    if !dev() {
        return source.into();
    }
    let mut text = format!("{source}{TAIL}");
    if let Some(map) = ignore::map_for(url, source, text.lines().count()) {
        text.push_str(&map);
    }
    text.into()
}

pub fn start(scope: &mut v8::PinScope, name: &str) {
    if !wanted() {
        return;
    }
    attach(scope, name);
    if dev() {
        send("Debugger.enable", "{}");
        send("Debugger.setSkipAllPauses", r#"{"skip":true}"#);
        if let Some(remote) = remote() {
            devtools::opened(name, remote);
            STATE.with(|s| s.borrow_mut().as_mut().map(|s| s.listed = true));
        }
    }
}

pub(crate) fn attach(scope: &mut v8::PinScope, name: &str) {
    let context = scope.get_current_context();
    let client = Client { isolate: unsafe { scope.as_raw_isolate_ptr() }, context: v8::Global::new(scope, context) };
    let inspector = V8Inspector::create(scope, V8InspectorClient::new(Box::new(client)));
    let aux = serde_json::json!({ "isDefault": true, "type": "default", "frameId": name }).to_string();
    inspector.context_created(context, GROUP, StringView::from(name.as_bytes()), StringView::from(aux.as_bytes()));
    let inbox = Rc::new(Inbox::default());
    let channel = Channel::new(Box::new(Replies(inbox.clone())));
    let session = inspector.connect(GROUP, channel, StringView::empty(), V8InspectorClientTrustLevel::FullyTrusted);
    let main = Session { session, inbox, calls: Cell::new(0) };
    let wake = rt::current().sender();
    let (pending, next, name, listed) = (Arc::default(), Arc::new(AtomicU32::new(1)), name.to_owned(), false);
    let isolate = scope.thread_safe_handle();
    let pause = Pause::default();
    let state = State { main, pause, isolate, extra: Default::default(), pending, next, wake, name, listed, page: Default::default(), inspector };
    STATE.with(|s| *s.borrow_mut() = Some(state));
}

pub fn remote() -> Option<Remote> {
    STATE.with(|s| s.borrow().as_ref().map(|s| Remote { pending: s.pending.clone(), next: s.next.clone(), wake: s.wake.clone(), isolate: s.isolate.clone() }))
}

pub fn dispatch_posted(scope: &mut v8::PinScope) {
    let Some(pending) = STATE.with(|s| s.borrow().as_ref().map(|s| s.pending.clone())) else { return };
    while let Some(posted) = pending.pop() {
        match posted {
            Posted::Connect(id, sink) => STATE.with(|s| {
                let s = s.borrow();
                let Some(s) = s.as_ref() else { return };
                s.page.connected(id, sink.clone());
                let channel = Channel::new(Box::new(Forward(sink, s.page.resolving(id))));
                let session = s.inspector.connect(GROUP, channel, StringView::empty(), V8InspectorClientTrustLevel::FullyTrusted);
                if s.extra.borrow_mut().insert(id, Rc::new(session)).is_none() {
                    rt::current().keep_alive(true);
                }
            }),
            Posted::Message(id, message) => {
                let session = STATE.with(|s| s.borrow().as_ref().and_then(|s| s.extra.borrow().get(&id).cloned()));
                if let Some(session) = session {
                    session.dispatch_protocol_message(StringView::from(message.as_bytes()));
                }
            }
            Posted::Page(id, message) if page::set_inspected(scope, id, &message) => {}
            Posted::Page(id, message) => match page::rewrite(id, &message) {
                Some(evaluate) => {
                    let session = STATE.with(|s| s.borrow().as_ref().and_then(|s| s.extra.borrow().get(&id).cloned()));
                    if let Some(session) = session {
                        session.dispatch_protocol_message(StringView::from(evaluate.as_bytes()));
                    }
                }
                None => page::message(scope, id, &message),
            },
            Posted::ExpectHandler => page::expect_handler(),
            Posted::Disconnect(id) => {
                page::disconnected(scope, id);
                let session = STATE.with(|s| s.borrow().as_ref().and_then(|s| s.extra.borrow_mut().remove(&id)));
                if session.is_some() {
                    rt::current().keep_alive(false);
                }
                drop(session);
            }
        }
    }
}

pub fn stop_when(stopping: Arc<AtomicBool>) {
    STATE.with(|s| s.borrow().as_ref().map(|s| *s.pause.stopping.borrow_mut() = Some(stopping)));
}

const PAUSE_POLL: Duration = Duration::from_millis(100);

static RELEASED: AtomicBool = AtomicBool::new(false);

pub fn release_pauses() {
    RELEASED.store(true, Ordering::SeqCst);
}

fn paused(isolate: v8::UnsafeRawIsolatePtr, context: &v8::Global<v8::Context>) {
    let Some(pending) = STATE.with(|s| {
        let s = s.borrow();
        let s = s.as_ref()?;
        if s.extra.borrow().is_empty() || RELEASED.load(Ordering::SeqCst) {
            return None;
        }
        s.pause.quit.set(false);
        s.pause.depth.set(s.pause.depth.get() + 1);
        Some(s.pending.clone())
    }) else {
        return;
    };
    let busy = rt::current().busy();
    let was = busy.swap(0, Ordering::Relaxed);
    let resume = || {
        STATE.with(|s| {
            s.borrow().as_ref().is_none_or(|s| {
                let stopping = s.pause.stopping.borrow().as_ref().is_some_and(|f| f.load(Ordering::SeqCst));
                s.pause.quit.get() || stopping || s.extra.borrow().is_empty() || RELEASED.load(Ordering::SeqCst)
            })
        })
    };
    {
        let mut raw = isolate;
        let isolate = unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut raw) };
        v8::scope_with_context!(let scope, isolate, context);
        v8::tc_scope!(let tc, scope);
        while !resume() {
            pending.wait(PAUSE_POLL);
            dispatch_posted(tc);
            if tc.has_terminated() {
                break;
            }
            if let Some(exception) = tc.exception() {
                let text = exception.to_rust_string_lossy(tc);
                log::warn!(target: "acde_host", "[{}] devtools: while paused: {text}", rt::web_name());
                tc.reset();
            }
        }
    }
    STATE.with(|s| s.borrow().as_ref().map(|s| s.pause.depth.set(s.pause.depth.get().saturating_sub(1))));
    if was != 0 {
        busy.store(rt::busy_stamp(), Ordering::Relaxed);
    }
}

unsafe extern "C" fn interrupted(_: v8::UnsafeRawIsolatePtr, _: *mut std::ffi::c_void) {
    let Some(pending) = STATE.with(|s| s.borrow().as_ref().map(|s| s.pending.clone())) else { return };
    for (id, text) in pending.take_pauses() {
        let session = STATE.with(|s| s.borrow().as_ref().and_then(|s| s.extra.borrow().get(&id).cloned()));
        if let Some(session) = session {
            session.dispatch_protocol_message(StringView::from(text.as_bytes()));
        }
    }
}

pub fn send(method: &str, params: &str) -> Option<String> {
    STATE.with(|s| {
        let s = s.borrow();
        let s = &s.as_ref()?.main;
        let id = s.calls.get() + 1;
        s.calls.set(id);
        let message = format!(r#"{{"id":{id},"method":"{method}","params":{params}}}"#);
        s.session.dispatch_protocol_message(StringView::from(message.as_bytes()));
        s.inbox.reply.borrow_mut().take()
    })
}

pub fn stop() {
    let state = STATE.with(|s| s.borrow_mut().take());
    if let Some(state) = state {
        if state.listed {
            devtools::closed(&state.name);
        }
        state.extra.borrow_mut().clear();
        drop(state);
    }
}

pub fn inspected() -> bool {
    dev() && STATE.with(|s| s.borrow().is_some())
}

pub fn thrown(scope: &mut v8::PinScope, exception: v8::Local<v8::Value>) {
    if !dev() {
        return;
    }
    let stack = v8::Exception::get_stack_trace(scope, exception);
    let context = scope.get_current_context();
    STATE.with(|s| {
        let s = s.borrow();
        let Some(s) = s.as_ref() else { return };
        let message = "Uncaught";
        let trace = s.inspector.create_stack_trace(stack);
        s.inspector.exception_thrown(context, StringView::from(message.as_bytes()), exception, StringView::empty(), StringView::empty(), 0, 0, trace, 0);
    });
}

pub fn live_edit(url: &str, source: &str) -> Result<(), String> {
    let id = STATE.with(|s| s.borrow().as_ref().and_then(|s| s.main.inbox.scripts.borrow().get(url).cloned()));
    let Some(id) = id else {
        return Err(if dev() { format!("no script {url}") } else { "not under acde dev".into() });
    };
    let params = serde_json::json!({ "scriptId": id, "scriptSource": format!("{source}{TAIL}"), "allowTopFrameEditing": false }).to_string();
    let reply = send("Debugger.setScriptSource", &params).ok_or("no reply")?;
    let value: serde_json::Value = serde_json::from_str(&reply).map_err(|e| e.to_string())?;
    if let Some(error) = value.get("error") {
        return Err(error["message"].as_str().unwrap_or("error").to_owned());
    }
    match value["result"]["status"].as_str() {
        Some("Ok") => Ok(()),
        Some("CompileError") => Err(format!("compile error: {}", value["result"]["exceptionDetails"]["text"].as_str().unwrap_or(""))),
        Some(status) => Err(status.to_owned()),
        None => Err("no status".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    #[test]
    fn a_second_session_evaluates_and_hears_the_console_from_another_thread() {
        let mut runtime = rt::Runtime::new(None, "inspected", std::sync::Arc::new(crate::host::platform::Headless));
        runtime.task(|scope| attach(scope, "inspected"));
        let remote = remote().expect("an inspector");
        let (tx, rx) = channel::<String>();
        let tx = Mutex::new(tx);
        let session = remote.connect(Arc::new(move |text| _ = tx.lock().unwrap().send(text)));
        let posting = remote.clone();
        let started = Instant::now();
        let poster = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            posting.send(session, r#"{"id":1,"method":"Runtime.enable","params":{}}"#.into());
            let evaluate = r#"{"id":2,"method":"Runtime.evaluate","params":{"expression":"console.log('from devtools'); 6 * 7"}}"#;
            posting.send(session, evaluate.into());
            Instant::now()
        });
        let messages = RefCell::new(Vec::<serde_json::Value>::new());
        let heard = |messages: &[serde_json::Value]| {
            let console = messages.iter().any(|m| m["method"] == "Runtime.consoleAPICalled" && m["params"]["args"][0]["value"] == "from devtools");
            (console, messages.iter().find(|m| m["id"] == 2).cloned())
        };
        runtime.lp.keep_alive(true);
        runtime.lp.delay(Duration::from_secs(10), |_: &mut v8::PinScope| {});
        let deadline = started + Duration::from_secs(10);
        runtime.run_until(|| {
            let mut messages = messages.borrow_mut();
            messages.extend(rx.try_iter().map(|text| serde_json::from_str::<serde_json::Value>(&text).expect("json")));
            let (console, answer) = heard(&messages);
            (console && answer.is_some()) || Instant::now() > deadline
        });
        let messages = messages.into_inner();
        let (console, answer) = heard(&messages);
        let posted = poster.join().expect("poster");
        assert!(console, "console event delivered: {messages:?}");
        let created = messages.iter().find(|m| m["method"] == "Runtime.executionContextCreated").expect("a context");
        assert_eq!(created["params"]["context"]["auxData"], serde_json::json!({"isDefault": true, "type": "default", "frameId": "inspected"}));
        assert_eq!(answer.expect("evaluate answered")["result"]["result"]["value"], 42);
        assert!(posted.elapsed() < Duration::from_secs(2), "an idle page answers promptly");
        let main = runtime.task(|_| send("Runtime.evaluate", r#"{"expression":"1 + 1"}"#));
        assert!(main.is_some_and(|reply| reply.contains("\"value\":2")), "the main session still works");
        remote.disconnect(session);
        runtime.task(|scope| dispatch_posted(scope));
        assert!(STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.extra.borrow().is_empty())), "the second session is gone");
        runtime.lp.keep_alive(false);
        stop();
    }

    #[test]
    fn set_inspected_node_makes_the_node_the_console_dollar_zero() {
        let mut runtime = rt::Runtime::new(None, "dollar", std::sync::Arc::new(crate::host::platform::Headless));
        runtime.task(|scope| {
            attach(scope, "dollar");
            let source = "globalThis.marker = {tag: 'node 5'}; globalThis[Symbol.for('acde.devtools.node')] = (id) => (id === 5 ? marker : undefined)";
            let code = v8::String::new(scope, source).unwrap();
            v8::Script::compile(scope, code, None).unwrap().run(scope).unwrap();
        });
        let remote = remote().expect("an inspector");
        let (tx, rx) = channel::<String>();
        let tx = Mutex::new(tx);
        let session = remote.connect(Arc::new(move |text| _ = tx.lock().unwrap().send(text)));
        remote.page(session, r#"{"id":1,"method":"DOM.setInspectedNode","params":{"nodeId":5},"sessionId":"s"}"#.into());
        remote.page(session, r#"{"id":2,"method":"DOM.setInspectedNode","params":{"nodeId":6}}"#.into());
        remote.send(session, r#"{"id":4,"method":"Runtime.enable","params":{}}"#.into());
        let evaluate = r#"{"id":3,"method":"Runtime.evaluate","params":{"expression":"$0 === marker","includeCommandLineAPI":true}}"#;
        remote.send(session, evaluate.into());
        runtime.task(|scope| dispatch_posted(scope));
        let replies: Vec<serde_json::Value> = rx.try_iter().map(|text| serde_json::from_str(&text).expect("json")).collect();
        let by_id = |id: i64| replies.iter().find(|m| m["id"] == id).cloned().unwrap_or_else(|| panic!("reply {id}: {replies:?}"));
        assert_eq!(by_id(1), serde_json::json!({"id": 1, "result": {}, "sessionId": "s"}));
        assert_eq!(by_id(2)["error"]["code"], -32000);
        assert_eq!(by_id(3)["result"]["result"]["value"], true, "$0 is the inspected node");
        remote.disconnect(session);
        runtime.task(|scope| dispatch_posted(scope));
        stop();
    }

    fn run(scope: &mut v8::PinScope, code: &str) -> Option<String> {
        let code = v8::String::new(scope, code)?;
        let value = v8::Script::compile(scope, code, None)?.run(scope)?;
        Some(value.to_rust_string_lossy(scope))
    }

    fn client(remote: &Remote) -> (u32, std::sync::mpsc::Receiver<serde_json::Value>) {
        let (tx, rx) = channel::<serde_json::Value>();
        let tx = Mutex::new(tx);
        let session = remote.connect(Arc::new(move |text| _ = tx.lock().unwrap().send(serde_json::from_str(&text).expect("json"))));
        (session, rx)
    }

    fn next(rx: &std::sync::mpsc::Receiver<serde_json::Value>, wanted: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let message = rx.recv_timeout(left).expect("the message in time");
            if wanted(&message) {
                return message;
            }
        }
    }

    #[test]
    fn a_debugger_statement_pauses_until_devtools_resumes_and_answers_while_paused() {
        let mut runtime = rt::Runtime::new(None, "pausing", std::sync::Arc::new(crate::host::platform::Headless));
        runtime.task(|scope| attach(scope, "pausing"));
        send("Debugger.enable", "{}");
        send("Debugger.setSkipAllPauses", r#"{"skip":true}"#);
        let remote = remote().expect("an inspector");
        let (session, rx) = client(&remote);
        remote.send(session, r#"{"id":1,"method":"Debugger.enable","params":{}}"#.into());
        runtime.task(|scope| dispatch_posted(scope));
        let driving = remote.clone();
        let driver = std::thread::spawn(move || {
            let paused = next(&rx, |m| m["method"] == "Debugger.paused");
            let frame = paused["params"]["callFrames"][0]["callFrameId"].as_str().expect("a frame").to_owned();
            let evaluate =
                serde_json::json!({"id": 2, "method": "Debugger.evaluateOnCallFrame", "params": {"callFrameId": frame, "expression": "local + ' 中文'"}});
            driving.send(session, evaluate.to_string());
            let local = next(&rx, |m| m["id"] == 2)["result"]["result"]["value"].clone();
            driving.send(session, r#"{"id":3,"method":"Debugger.stepOver","params":{}}"#.into());
            let stepped = next(&rx, |m| m["method"] == "Debugger.paused")["params"]["callFrames"][0]["location"]["lineNumber"].clone();
            driving.send(session, r#"{"id":4,"method":"Debugger.resume","params":{}}"#.into());
            next(&rx, |m| m["method"] == "Debugger.resumed");
            (local, stepped)
        });
        let after = runtime
            .task(|scope| run(scope, "(function () {\n  const local = 'here'\n  debugger\n  globalThis.after = local + '!'\n  return globalThis.after\n})()"));
        let (local, stepped) = driver.join().expect("driver");
        assert_eq!(local, "here 中文", "evaluated in the paused frame");
        assert_eq!(stepped, 3, "step over stopped on the next line");
        assert_eq!(after.as_deref(), Some("here!"), "the page ran on after resume");
        remote.disconnect(session);
        runtime.task(|scope| dispatch_posted(scope));
        stop();
    }

    #[test]
    fn a_debugger_statement_without_a_client_does_not_pause() {
        let mut runtime = rt::Runtime::new(None, "unwatched", std::sync::Arc::new(crate::host::platform::Headless));
        runtime.task(|scope| attach(scope, "unwatched"));
        send("Debugger.enable", "{}");
        send("Debugger.setSkipAllPauses", r#"{"skip":true}"#);
        let started = Instant::now();
        let value = runtime.task(|scope| run(scope, "debugger; 'ran'"));
        assert_eq!(value.as_deref(), Some("ran"));
        assert!(started.elapsed() < Duration::from_secs(1));
        stop();
    }

    #[test]
    fn stopping_the_page_ends_a_pause() {
        let mut runtime = rt::Runtime::new(None, "closing", std::sync::Arc::new(crate::host::platform::Headless));
        runtime.task(|scope| attach(scope, "closing"));
        let stopping = Arc::new(AtomicBool::new(false));
        stop_when(stopping.clone());
        let remote = remote().expect("an inspector");
        let (session, rx) = client(&remote);
        remote.send(session, r#"{"id":1,"method":"Debugger.enable","params":{}}"#.into());
        runtime.task(|scope| dispatch_posted(scope));
        let isolate = runtime.isolate.thread_safe_handle();
        let closer = std::thread::spawn(move || {
            next(&rx, |m| m["method"] == "Debugger.paused");
            stopping.store(true, Ordering::SeqCst);
            isolate.terminate_execution();
        });
        let started = Instant::now();
        let value = runtime.task(|scope| run(scope, "debugger; 'not reached'"));
        closer.join().expect("closer");
        assert_eq!(value, None, "the page script was terminated");
        assert!(started.elapsed() < Duration::from_secs(5), "the pause ended");
        remote.disconnect(session);
        stop();
    }

    #[test]
    fn detaching_the_last_client_ends_a_pause() {
        let mut runtime = rt::Runtime::new(None, "detaching", std::sync::Arc::new(crate::host::platform::Headless));
        runtime.task(|scope| attach(scope, "detaching"));
        let remote = remote().expect("an inspector");
        let (session, rx) = client(&remote);
        remote.send(session, r#"{"id":1,"method":"Debugger.enable","params":{}}"#.into());
        runtime.task(|scope| dispatch_posted(scope));
        let leaving = remote.clone();
        let leaver = std::thread::spawn(move || {
            next(&rx, |m| m["method"] == "Debugger.paused");
            leaving.disconnect(session);
        });
        let value = runtime.task(|scope| run(scope, "debugger; 'went on'"));
        leaver.join().expect("leaver");
        assert_eq!(value.as_deref(), Some("went on"));
        stop();
    }

    #[test]
    fn debugger_pause_interrupts_a_busy_loop() {
        let mut runtime = rt::Runtime::new(None, "spinning", std::sync::Arc::new(crate::host::platform::Headless));
        runtime.task(|scope| attach(scope, "spinning"));
        let remote = remote().expect("an inspector");
        let (session, rx) = client(&remote);
        remote.send(session, r#"{"id":1,"method":"Debugger.enable","params":{}}"#.into());
        runtime.task(|scope| dispatch_posted(scope));
        let driving = remote.clone();
        let driver = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            driving.send(session, r#"{"id":2,"method":"Debugger.pause","params":{}}"#.into());
            let paused = next(&rx, |m| m["method"] == "Debugger.paused");
            let frame = paused["params"]["callFrames"][0]["callFrameId"].as_str().expect("a frame").to_owned();
            let stop = serde_json::json!({"id": 3, "method": "Debugger.evaluateOnCallFrame", "params": {"callFrameId": frame, "expression": "spin = false"}});
            driving.send(session, stop.to_string());
            next(&rx, |m| m["id"] == 3);
            driving.send(session, r#"{"id":4,"method":"Debugger.resume","params":{}}"#.into());
        });
        let value = runtime.task(|scope| run(scope, "var spin = true; let turns = 0; while (spin) turns++; 'stopped'"));
        driver.join().expect("driver");
        assert_eq!(value.as_deref(), Some("stopped"), "the loop was paused, changed and resumed");
        remote.disconnect(session);
        runtime.task(|scope| dispatch_posted(scope));
        stop();
    }

    #[test]
    fn a_console_evaluation_that_hits_debugger_pauses_and_a_disconnect_ends_it() {
        let mut runtime = rt::Runtime::new(None, "console", std::sync::Arc::new(crate::host::platform::Headless));
        runtime.task(|scope| attach(scope, "console"));
        let remote = remote().expect("an inspector");
        let (session, rx) = client(&remote);
        remote.send(session, r#"{"id":1,"method":"Debugger.enable","params":{}}"#.into());
        remote.send(session, r#"{"id":2,"method":"Runtime.evaluate","params":{"expression":"debugger; 1"}}"#.into());
        let (other, other_rx) = client(&remote);
        remote.send(other, r#"{"id":1,"method":"Runtime.enable","params":{}}"#.into());
        let leaving = remote.clone();
        let leaver = std::thread::spawn(move || {
            next(&rx, |m| m["method"] == "Debugger.paused");
            leaving.send(other, r#"{"id":2,"method":"Runtime.evaluate","params":{"expression":"'answered while paused'"}}"#.into());
            let answered = next(&other_rx, |m| m["id"] == 2)["result"]["result"]["value"].clone();
            leaving.disconnect(session);
            leaving.disconnect(other);
            answered
        });
        let started = Instant::now();
        runtime.task(|scope| dispatch_posted(scope));
        let answered = leaver.join().expect("leaver");
        assert_eq!(answered, "answered while paused");
        assert!(started.elapsed() < Duration::from_secs(5), "the pause ended when its client left");
        runtime.task(|scope| dispatch_posted(scope));
        assert!(STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.extra.borrow().is_empty())), "both sessions are gone");
        stop();
    }
}
