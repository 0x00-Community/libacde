use super::documents::{OfficeFile, Source};
use crate::runtime::sync::Locked;
use base64::Engine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

const IDLE: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SaveState {
    Open,
    Saving,
    Saved,
    Failed,
    Conflict,
    Closed,
}

pub struct Session {
    pub key: String,
    pub id: String,
    pub source: Source,
    pub path: String,
    pub state: SaveState,
    pub saved_at: Option<i64>,
    pub message: Option<String>,
    pub conflict_path: Option<String>,
    pub editors: usize,
    pub touched: Instant,
    pub expected_ms: i64,
    pub target: String,
}

impl Session {
    pub fn status(&self) -> Value {
        json!({"key": self.key, "state": self.state, "savedAt": self.saved_at, "expectedMs": self.expected_ms,
            "message": self.message, "conflictPath": self.conflict_path, "editors": self.editors})
    }
    pub fn touch(&mut self) {
        self.touched = Instant::now();
    }
}

pub type Shared = Arc<Mutex<Session>>;

#[derive(Default)]
struct Sessions {
    by_key: HashMap<String, Shared>,
    live: HashMap<String, String>,
}

#[derive(Default)]
pub struct EditSessions {
    inner: Mutex<Sessions>,
}

impl EditSessions {
    fn key(id: &str, modified_ms: i64, size: u64) -> String {
        let digest = Sha256::digest(format!("{id}\0{modified_ms}\0{size}").as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)[..32].to_owned()
    }

    fn inner(&self) -> MutexGuard<'_, Sessions> {
        self.inner.locked()
    }

    fn fresh(key: String, id: &str, file: &OfficeFile) -> Shared {
        Arc::new(Mutex::new(Session {
            key,
            id: id.to_owned(),
            source: file.source.clone(),
            path: file.path.clone(),
            state: SaveState::Open,
            saved_at: None,
            message: None,
            conflict_path: None,
            editors: 0,
            touched: Instant::now(),
            expected_ms: file.modified_ms,
            target: file.path.clone(),
        }))
    }

    pub fn open(&self, id: &str, file: &OfficeFile) -> Shared {
        let mut inner = self.inner();
        let cutoff = Instant::now() - IDLE;
        let stale: Vec<String> = inner.by_key.iter().filter(|(_, s)| s.locked().touched < cutoff).map(|(k, _)| k.clone()).collect();
        for key in stale {
            if let Some(session) = inner.by_key.remove(&key) {
                let id = session.locked().id.clone();
                if inner.live.get(&id) == Some(&key) {
                    inner.live.remove(&id);
                }
            }
        }
        let current = inner.live.get(id).and_then(|key| inner.by_key.get(key)).cloned();
        if let Some(current) = current.filter(|s| s.locked().state != SaveState::Closed) {
            current.locked().touch();
            return current;
        }
        let key = Self::key(id, file.modified_ms, file.size);
        let session = inner.by_key.get(&key).cloned().unwrap_or_else(|| Self::fresh(key.clone(), id, file));
        {
            let mut s = session.locked();
            s.state = SaveState::Open;
            s.message = None;
            s.touch();
        }
        inner.by_key.insert(key.clone(), Arc::clone(&session));
        inner.live.insert(id.to_owned(), key);
        session
    }

    pub fn get(&self, key: &str) -> Option<Shared> {
        self.inner().by_key.get(key).cloned()
    }

    pub fn close(&self, session: &mut Session) {
        if !matches!(session.state, SaveState::Failed | SaveState::Conflict) {
            session.state = SaveState::Closed;
        }
        session.editors = 0;
        session.touch();
        let mut inner = self.inner();
        if inner.live.get(&session.id) == Some(&session.key) {
            inner.live.remove(&session.id);
        }
    }
}
