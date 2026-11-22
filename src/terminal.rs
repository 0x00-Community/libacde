use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenOptions {
    pub cols: f64,
    pub rows: f64,
    pub command: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub env: Option<Vec<(String, String)>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalEvent {
    Output { data: String },
    Cwd { path: String },
    Exit { code: Option<i32>, signal: Option<i32> },
    Foreground { program: String, busy: bool },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Shell {
    pub path: String,
    pub name: String,
    pub login: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Limits {
    pub max: usize,
    pub open: usize,
}

pub type Sink = Box<dyn Fn(Vec<TerminalEvent>) -> bool + Send + Sync>;

pub trait TerminalHost: Send + Sync {
    fn open(&self, options: OpenOptions) -> Result<String, Refusal>;
    fn input(&self, id: &str, data: String) -> Result<(), Refusal>;
    fn resize(&self, id: &str, cols: f64, rows: f64) -> Result<(u32, u32), Refusal>;
    fn close(&self, id: &str);
    fn listen(&self, id: &str, sink: Sink) -> Result<(), Refusal>;
    fn shells(&self) -> Vec<Shell>;
    fn limits(&self) -> Limits;
    fn close_all(&self);
}

static CHOSEN: OnceLock<Arc<dyn TerminalHost>> = OnceLock::new();

pub fn provide(host: Box<dyn TerminalHost>) -> bool {
    let host: Arc<dyn TerminalHost> = Arc::from(host);
    let mut taken = false;
    CHOSEN.get_or_init(|| {
        taken = true;
        host
    });
    taken
}

pub(crate) fn host() -> Arc<dyn TerminalHost> {
    Arc::clone(CHOSEN.get_or_init(|| Arc::new(crate::runtime::Terminals::new())))
}

pub(crate) fn close_all() {
    if let Some(host) = CHOSEN.get() {
        host.close_all();
    }
}

impl Refusal {
    pub fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { status, code: code.into(), message: message.into() }
    }

    pub(crate) fn from_failure(text: &str) -> Self {
        let value: Value = serde_json::from_str(text).unwrap_or_default();
        let status = value["status"].as_u64().and_then(|s| u16::try_from(s).ok()).unwrap_or(500);
        let code = value["code"].as_str().unwrap_or("internal_error");
        Self::new(status, code, value["message"].as_str().unwrap_or(text))
    }
}

impl From<Refusal> for String {
    fn from(refusal: Refusal) -> Self {
        json!({"status": refusal.status, "code": refusal.code, "message": refusal.message, "retryable": false}).to_string()
    }
}

impl OpenOptions {
    pub(crate) fn parse(cols: f64, rows: f64, given: Option<Value>) -> Result<Self, Refusal> {
        let mut options = Self { cols, rows, ..Self::default() };
        let fields = match given {
            None | Some(Value::Null) => return Ok(options),
            Some(Value::Object(fields)) => fields,
            Some(_) => return Err(Refusal::new(400, "invalid_request", "options must be an object")),
        };
        options.command = field(&fields, "command", |v| v.as_array()?.iter().map(|s| s.as_str().map(str::to_owned)).collect(), "an array of strings")?;
        options.cwd = field(&fields, "cwd", |v| v.as_str().map(str::to_owned), "a string")?;
        options.env =
            field(&fields, "env", |v| v.as_object()?.iter().map(|(k, s)| Some((k.clone(), s.as_str()?.to_owned()))).collect(), "an object of strings")?;
        Ok(options)
    }
}

fn field<T>(fields: &Map<String, Value>, name: &str, read: impl Fn(&Value) -> Option<T>, shape: &str) -> Result<Option<T>, Refusal> {
    match fields.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => read(value).map(Some).ok_or_else(|| Refusal::new(400, name, format!("options.{name} must be {shape}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_read_by_shape() {
        assert_eq!(OpenOptions::parse(80.0, 24.0, None).unwrap(), OpenOptions { cols: 80.0, rows: 24.0, ..OpenOptions::default() });
        let given = json!({"command": ["/bin/sh", "-c", "true"], "cwd": "/tmp", "env": {"LANG": "C.UTF-8"}});
        let options = OpenOptions::parse(80.0, 24.0, Some(given)).unwrap();
        assert_eq!(options.command, Some(vec!["/bin/sh".into(), "-c".into(), "true".into()]));
        assert_eq!(options.cwd.as_deref(), Some("/tmp"));
        assert_eq!(options.env, Some(vec![("LANG".into(), "C.UTF-8".into())]));
        assert_eq!(OpenOptions::parse(80.0, 24.0, Some(json!({"command": ["sh", 1]}))).unwrap_err().code, "command");
        assert_eq!(OpenOptions::parse(80.0, 24.0, Some(json!({"cwd": 3}))).unwrap_err().code, "cwd");
        assert_eq!(OpenOptions::parse(80.0, 24.0, Some(json!({"env": {"A": 1}}))).unwrap_err().code, "env");
        assert_eq!(OpenOptions::parse(80.0, 24.0, Some(json!("x"))).unwrap_err().status, 400);
    }

    #[test]
    fn a_refusal_reads_back_as_a_failure() {
        let text: String = Refusal::new(429, "limit", "at most 32 terminals").into();
        assert_eq!(Refusal::from_failure(&text), Refusal::new(429, "limit", "at most 32 terminals"));
    }

    #[test]
    fn events_have_the_page_shape() {
        let events = vec![
            TerminalEvent::Output { data: "hi".into() },
            TerminalEvent::Cwd { path: "~".into() },
            TerminalEvent::Exit { code: Some(7), signal: None },
            TerminalEvent::Foreground { program: "vim".into(), busy: true },
        ];
        let shaped = json!([
            {"type": "output", "data": "hi"},
            {"type": "cwd", "path": "~"},
            {"type": "exit", "code": 7, "signal": null},
            {"type": "foreground", "program": "vim", "busy": true},
        ]);
        assert_eq!(serde_json::to_value(&events).unwrap(), shaped);
        assert_eq!(serde_json::from_value::<Vec<TerminalEvent>>(shaped).unwrap(), events);
    }
}
