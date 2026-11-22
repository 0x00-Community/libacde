use crate::runtime::failure::{Code, Failure, Outcome};
use crate::runtime::sync::Locked;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

use platform::{Master, Wake};

type Result<T> = std::result::Result<T, String>;

const MAX_TERMINALS: usize = 8;
const ATTACH: Duration = Duration::from_secs(15);
const ENDED: Duration = Duration::from_secs(30);
const DRAIN: Duration = Duration::from_millis(300);
const KILL_AFTER: Duration = Duration::from_secs(3);
const BACKLOG_UNITS: usize = 256 * 1024;
const MAX_INPUT_UNITS: usize = 64 * 1024;

type Listener = dyn Fn(Value) -> bool + Send + Sync;

#[derive(Default)]
struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut start = 0;
        let keep = loop {
            match std::str::from_utf8(&self.pending[start..]) {
                Ok(_) => break 0,
                Err(error) => match error.error_len() {
                    None => break self.pending.len() - start - error.valid_up_to(),
                    Some(len) => start += error.valid_up_to() + len,
                },
            }
        };
        let rest = self.pending.split_off(self.pending.len() - keep);
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending = rest;
        text
    }
    fn end(&mut self) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned()
    }
}

#[derive(Default)]
struct Deadlines {
    removal: Option<Instant>,
    kill: Option<Instant>,
}

#[derive(Default)]
struct State {
    listeners: Vec<Arc<Listener>>,
    backlog: VecDeque<Value>,
    backlog_units: usize,
    ended: bool,
    closed: bool,
    deadlines: Deadlines,
    input: Vec<u8>,
}

struct Session {
    id: String,
    master: Mutex<Option<Arc<Master>>>,
    wake: Wake,
    state: Mutex<State>,
}

impl Session {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.locked()
    }
    fn master(&self) -> Option<Arc<Master>> {
        self.master.locked().clone()
    }
    fn wake(&self) {
        self.wake.signal();
    }

    fn output(text: String) -> Value {
        json!({"type": "output", "data": text})
    }

    fn units(event: &Value) -> usize {
        event["data"].as_str().map_or(0, |data| data.encode_utf16().count())
    }

    fn emit(&self, events: Vec<Value>) {
        let events: Vec<Value> = events.into_iter().filter(|e| e["type"] != "output" || e["data"].as_str().is_some_and(|d| !d.is_empty())).collect();
        if events.is_empty() {
            return;
        }
        let listeners = {
            let mut state = self.state();
            if state.listeners.is_empty() {
                for event in events {
                    state.backlog_units += Self::units(&event);
                    state.backlog.push_back(event);
                }
                while state.backlog_units > BACKLOG_UNITS && state.backlog.len() > 1 && state.backlog[0]["type"] == "output" {
                    let dropped = state.backlog.pop_front().unwrap_or_default();
                    state.backlog_units -= Self::units(&dropped);
                }
                return;
            }
            state.listeners.clone()
        };
        let batch = Value::Array(events);
        let gone: Vec<Arc<Listener>> = listeners.into_iter().filter(|listener| !listener(batch.clone())).collect();
        if !gone.is_empty() {
            self.state().listeners.retain(|l| !gone.iter().any(|g| Arc::ptr_eq(l, g)));
        }
    }

    fn hang_up(&self) {
        let master = self.master.locked().take();
        if let Some(master) = master {
            let mut state = self.state();
            if !state.ended {
                state.deadlines.kill = Some(Instant::now() + KILL_AFTER);
            }
            drop(state);
            platform::release(master);
        }
        self.wake();
    }

    fn end(&self, registry: &Registry, rest: String, exit: Value) {
        self.emit(vec![Session::output(rest), exit]);
        let subscribed = {
            let mut state = self.state();
            state.ended = true;
            state.deadlines.kill = None;
            state.deadlines.removal = Some(Instant::now() + if state.listeners.is_empty() { ENDED } else { Duration::ZERO });
            !state.listeners.is_empty()
        };
        self.hang_up();
        if subscribed {
            registry.close(self);
        }
    }
}

struct Registry {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl Registry {
    fn sessions(&self) -> MutexGuard<'_, HashMap<String, Arc<Session>>> {
        self.sessions.locked()
    }
    fn get(&self, id: &str) -> Outcome<Arc<Session>> {
        self.sessions().get(id).cloned().ok_or_else(|| Failure::http(404, "no such terminal"))
    }
    fn close(&self, session: &Session) {
        self.sessions().remove(&session.id);
        let mut state = session.state();
        state.closed = true;
        state.deadlines.removal = None;
        drop(state);
        session.hang_up();
    }
}

pub struct Terminals {
    registry: Arc<Registry>,
    owner: bool,
}

impl Terminals {
    fn size(cols: f64, rows: f64) -> Outcome<(u32, u32)> {
        for (name, value) in [("cols", cols), ("rows", rows)] {
            if value.fract() != 0.0 || !(2.0..=1000.0).contains(&value) {
                return Err(Failure::invalid(format!("{name} must be an integer in [2, 1000]")));
            }
        }
        Ok((cols as u32, rows as u32))
    }

    fn start(&self, cols: u32, rows: u32) -> Outcome<String> {
        if self.registry.sessions().len() >= MAX_TERMINALS {
            return Err(Failure::http(429, format!("at most {MAX_TERMINALS} terminals")));
        }
        let home = std::env::home_dir().map_or_else(platform::root, |h| h.to_string_lossy().into_owned());
        let spawned = platform::spawn(&home, cols, rows).map_err(|reason| Failure::http(500, format!("could not start the terminal: {reason}")))?;
        let wake = Wake::new().map_err(Failure::internal)?;
        let pid = spawned.pid;
        let session = Arc::new(Session {
            id: uuid::Uuid::new_v4().to_string(),
            master: Mutex::new(Some(Arc::new(spawned.master))),
            wake,
            state: Mutex::new(State { deadlines: Deadlines { removal: Some(Instant::now() + ATTACH), kill: None }, ..State::default() }),
        });
        self.registry.sessions().insert(session.id.clone(), Arc::clone(&session));
        let (registry, id) = (Arc::clone(&self.registry), session.id.clone());
        let process = spawned.process;
        std::thread::Builder::new().name(format!("terminal-{pid}")).spawn(move || platform::run(&registry, &session, process)).map_err(Failure::internal)?;
        Ok(id)
    }
}

impl Terminals {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { registry: Arc::new(Registry { sessions: Mutex::default() }), owner: true }
    }

    pub fn open(&self, cols: f64, rows: f64) -> Result<Value> {
        let (cols, rows) = Self::size(cols, rows)?;
        Ok(json!({"id": self.start(cols, rows)?}))
    }

    pub fn input(&self, id: String, data: String) -> Result<Value> {
        if data.encode_utf16().count() > MAX_INPUT_UNITS {
            return Err(Failure::invalid("data must be a string of at most 65536 characters").into());
        }
        let session = self.registry.get(&id)?;
        if session.master().is_some() {
            session.state().input.extend_from_slice(data.as_bytes());
            session.wake();
        }
        Ok(json!({"ok": true}))
    }

    pub fn resize(&self, id: String, cols: f64, rows: f64) -> Result<Value> {
        let (c, r) = Self::size(cols, rows)?;
        let session = self.registry.get(&id)?;
        if let Some(master) = session.master() {
            platform::resize(&master, c, r).map_err(|e| Failure::new(500, Code::Service("pty_failed".into()), e))?;
        }
        Ok(json!({"cols": c, "rows": r}))
    }

    pub fn close(&self, id: String) -> Value {
        if let Ok(session) = self.registry.get(&id) {
            self.registry.close(&session);
        }
        json!({"closed": id})
    }

    pub fn close_all(&self) {
        let sessions: Vec<_> = self.registry.sessions().values().cloned().collect();
        for session in sessions {
            self.registry.close(&session);
        }
    }

    pub fn shared(&self) -> Self {
        Self { registry: Arc::clone(&self.registry), owner: false }
    }

    pub fn listen(&self, id: &str, listener: impl Fn(Value) -> bool + Send + Sync + 'static) -> Result<()> {
        let session = self.registry.get(id)?;
        let ended = {
            let mut state = session.state();
            state.deadlines.removal = None;
            let backlog: Vec<Value> = state.backlog.drain(..).collect();
            state.backlog_units = 0;
            if !backlog.is_empty() {
                listener(Value::Array(backlog));
            }
            state.listeners.push(Arc::new(listener));
            state.ended
        };
        if ended {
            self.registry.close(&session);
        }
        Ok(())
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        if self.owner {
            self.close_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Log = (String, Vec<Value>, Vec<usize>);

    #[derive(Clone, Default)]
    pub(super) struct Seen(Arc<Mutex<Log>>);

    impl Seen {
        pub(super) fn follow(terminals: &Terminals, id: &str) -> Self {
            let seen = Self::default();
            let sink = seen.clone();
            let listened = terminals.listen(id, move |events| {
                let mut log = sink.0.locked();
                let mut size = 0;
                for event in events.as_array().into_iter().flatten() {
                    if event["type"] == "output" {
                        let data = event["data"].as_str().unwrap_or_default();
                        log.0.push_str(data);
                        size += data.encode_utf16().count();
                    }
                    log.1.push(event.clone());
                }
                log.2.push(size);
                true
            });
            listened.unwrap();
            seen
        }

        pub(super) fn until(&self, what: &str, within: Duration, done: impl Fn(&Log) -> bool) {
            let end = Instant::now() + within;
            while !done(&self.0.locked()) {
                assert!(Instant::now() < end, "timed out waiting for {what}: {:?}", self.0.locked().0);
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        pub(super) fn exited(&self) -> Value {
            self.until("exit", Duration::from_secs(6), |log| log.1.iter().any(|e| e["type"] == "exit"));
            self.0.locked().1.last().cloned().unwrap_or_default()
        }

        pub(super) fn text(&self) -> String {
            self.0.locked().0.clone()
        }

        #[cfg(target_os = "linux")]
        pub(super) fn sizes(&self) -> Vec<usize> {
            self.0.locked().2.clone()
        }
    }

    pub(super) fn failed<T: std::fmt::Debug>(result: Result<T>) -> Value {
        let mut failure: Value = serde_json::from_str(&result.unwrap_err()).unwrap();
        failure.as_object_mut().unwrap().remove("retryable");
        failure
    }

    pub(super) fn invalid(message: &str) -> Value {
        json!({"status": 400, "code": "invalid_request", "message": message})
    }

    pub(super) fn missing() -> Value {
        json!({"status": 404, "code": "http_error", "message": "no such terminal"})
    }

    pub(super) fn open(t: &Terminals) -> String {
        t.open(80.0, 24.0).unwrap()["id"].as_str().unwrap().to_owned()
    }

    pub(super) const WAIT: Duration = Duration::from_secs(5);
    pub(super) const STARTUP: Duration = Duration::from_secs(30);
    pub(super) const READY: &str = "echo SHELL-READY-$((40+2))\n";

    pub(super) fn is_ready(text: &str) -> bool {
        text.contains("SHELL-READY-42")
    }

    pub(super) fn started(t: &Terminals) -> String {
        let id = open(t);
        let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let text = Arc::new(Mutex::new(String::new()));
        let (seen, sink) = (ready.clone(), text.clone());
        let listened = t.listen(&id, move |events| {
            let mut text = sink.locked();
            for event in events.as_array().into_iter().flatten() {
                text.push_str(event["data"].as_str().unwrap_or_default());
            }
            let done = is_ready(&text);
            seen.store(done, std::sync::atomic::Ordering::SeqCst);
            !done
        });
        listened.unwrap();
        t.input(id.clone(), READY.into()).unwrap();
        let end = Instant::now() + STARTUP;
        while !ready.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(Instant::now() < end, "the shell did not start: {:?}", text.locked());
            std::thread::sleep(Duration::from_millis(20));
        }
        id
    }

    #[test]
    fn requests_are_checked() {
        let t = Terminals::new();
        assert_eq!(failed(t.open(1.0, 24.0)), invalid("cols must be an integer in [2, 1000]"));
        assert_eq!(failed(t.open(80.0, 24.5)), invalid("rows must be an integer in [2, 1000]"));
        assert_eq!(failed(t.open(80.0, 1001.0)), invalid("rows must be an integer in [2, 1000]"));
        assert_eq!(failed(t.input("nope".into(), "x".into())), missing());
        assert_eq!(failed(t.resize("nope".into(), 80.0, 24.0)), missing());
        assert_eq!(failed(t.listen("nope", |_| true)), missing());
        assert_eq!(t.close("nope".into()), json!({"closed": "nope"}));
    }

    #[test]
    fn at_most_eight_terminals() {
        let t = Terminals::new();
        let ids: Vec<String> = (0..8).map(|_| open(&t)).collect();
        assert_eq!(failed(t.open(80.0, 24.0)), json!({"status": 429, "code": "http_error", "message": "at most 8 terminals"}));
        for id in ids {
            t.close(id);
        }
        open(&t);
        t.close_all();
    }

    #[test]
    fn a_terminal_nobody_subscribes_to_goes_after_15_s() {
        let t = Terminals::new();
        let id = open(&t);
        std::thread::sleep(Duration::from_secs(14));
        assert_eq!(t.input(id.clone(), "\n".into()).unwrap(), json!({"ok": true}));
        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(failed(t.input(id, "x".into())), missing());
    }
}
