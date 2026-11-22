mod blank;
mod bridge;
mod docserver;
mod documents;
mod sessions;
mod settings;
mod tokens;

use crate::runtime::failure::{Failure, Outcome};
use crate::runtime::fs::Workspace;
use crate::runtime::fs::path::NodePath;
use crate::runtime::fs::workspace::FinderRoot;
use crate::runtime::js::JsText;
use crate::runtime::sync::Locked;
use blank::BlankKind;
use bridge::Handlers;
use docserver::DocumentServer;
use documents::{MAX_DOCUMENT_BYTES, OfficeFile, OfficeFiles, SaveResult, Source};
use serde_json::{Value, json};
use sessions::{EditSessions, SaveState, Session, Shared as SessionRef};
use settings::OfficeSettings;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokens::{Jwt, Ticket, TicketKind, Tickets};

struct Credentials {
    jwt: Jwt,
    tickets: Tickets,
}

struct Service {
    settings: OfficeSettings,
    files: OfficeFiles,
    sessions: EditSessions,
    workspaces: Mutex<HashMap<String, Workspace>>,
    secrets: Mutex<Option<Arc<Credentials>>>,
    cached: Mutex<Option<(Instant, Value)>>,
    bridge_error: Mutex<Option<String>>,
    bridge: Mutex<Option<futures_util::future::AbortHandle>>,
    user: Value,
}

impl Service {
    fn wait<F, T>(work: impl FnOnce() -> F + Send + 'static) -> Outcome<T>
    where
        F: std::future::Future<Output = T> + 'static,
        T: Send + 'static,
    {
        crate::runtime::rt::block_on(crate::runtime::rt::local(work)).flatten().ok_or_else(|| Failure::internal("the runtime's executor stopped"))
    }

    fn credentials(&self) -> Outcome<Arc<Credentials>> {
        let mut secrets = self.secrets.locked();
        if let Some(found) = secrets.as_ref() {
            return Ok(Arc::clone(found));
        }
        let secret = self.settings.jwt_secret()?;
        let fresh = Arc::new(Credentials { jwt: Jwt::new(&secret), tickets: Tickets::new(&secret) });
        *secrets = Some(Arc::clone(&fresh));
        Ok(fresh)
    }

    fn status(&self, fresh: bool) -> Value {
        if let Some((at, value)) = self.cached.locked().as_ref().filter(|_| !fresh)
            && at.elapsed() < Duration::from_secs(5)
        {
            return value.clone();
        }
        let (credentials, problem) = match self.credentials() {
            Ok(c) => (Some(c), None),
            Err(failure) => (None, Some(failure.message)),
        };
        let server = DocumentServer::new(&self.settings.document_server, credentials, problem);
        let url = server.url.clone();
        let mut value = Service::wait(move || async move { server.status().await })
            .unwrap_or_else(|failure| json!({"ok": false, "url": url, "version": null, "problem": "unreachable", "message": failure.message}));
        if value["ok"] == true
            && let Some(error) = self.bridge_error.locked().clone()
        {
            value = json!({"ok": false, "url": value["url"], "version": value["version"], "problem": "bridge", "message": error});
        }
        *self.cached.locked() = Some((Instant::now(), value.clone()));
        value
    }

    fn found(roots: &[FinderRoot], id: Option<&str>) -> Option<FinderRoot> {
        match id {
            Some(wanted) => roots.iter().find(|r| r.id == wanted).cloned(),
            None => roots.first().cloned(),
        }
    }

    fn blank(&self, roots: &[FinderRoot], root: Option<&FinderRoot>) -> Value {
        json!({"base": "", "apps": [], "roots": roots, "root": root.map(|r| r.id.clone()), "dir": "", "entries": [], "truncated": false,
            "file": null, "editor": null, "status": null, "server": self.status(false), "error": null})
    }

    fn show(&self, props: &mut Value, file: OfficeFile) -> Outcome<()> {
        let folder = file.path.dirname().to_owned();
        props["dir"] = if folder == "." { String::new() } else { folder }.into();
        props["file"] = file.view();
        if props["server"]["ok"] == true {
            let session = self.sessions.open(&file.source.identity(&file.path)?, &file);
            props["editor"] = self.editor(&file, &session)?;
            props["status"] = session.locked().status();
        }
        Ok(())
    }

    fn lent(&self, scope: Option<&Workspace>, name: &str) -> Outcome<Value> {
        let mut props = self.blank(&Self::roots(scope)?, None);
        let opened = scope
            .and_then(|workspace| workspace.lent_file(name))
            .ok_or_else(|| Failure::http(404, "not a file lent to this app"))
            .and_then(|file| OfficeFiles::open(Source::Lent { owner: Self::owner(scope), file }, ""))
            .and_then(|file| self.show(&mut props, file));
        match opened {
            Err(failure) if failure.status == 500 => Err(failure),
            Err(failure) => {
                props["error"] = failure.message.into();
                Ok(props)
            }
            Ok(()) => Ok(props),
        }
    }

    fn roots(scope: Option<&Workspace>) -> Outcome<Vec<FinderRoot>> {
        scope.map_or_else(|| Ok(Vec::new()), Workspace::finder_roots)
    }

    fn owner(scope: Option<&Workspace>) -> String {
        scope.and_then(Workspace::owner).unwrap_or_default().to_owned()
    }

    fn props(&self, scope: Option<&Workspace>, root: Option<&str>, dir: Option<&str>, path: Option<&str>) -> Outcome<Value> {
        let roots = Self::roots(scope)?;
        let found = Self::found(&roots, root);
        let mut props = self.blank(&roots, found.as_ref());
        if root.is_some() && found.is_none() {
            props["error"] = "not a Runtime workspace".into();
            return Ok(props);
        }
        let Some(found) = found else {
            return Ok(props);
        };
        let loaded = (|| -> Outcome<()> {
            match path {
                Some(path) => self.show(&mut props, OfficeFiles::open(Source::Root { owner: Self::owner(scope), root: found.clone() }, path)?)?,
                None => {
                    let (path, entries, truncated) = self.files.list_in(found.clone(), dir.unwrap_or(""))?;
                    props["dir"] = path.into();
                    props["entries"] = serde_json::to_value(entries).map_err(Failure::internal)?;
                    props["truncated"] = truncated.into();
                }
            }
            Ok(())
        })();
        if let Err(failure) = loaded {
            if failure.status == 500 {
                return Err(failure);
            }
            props["error"] = failure.message.into();
        }
        Ok(props)
    }

    fn editor(&self, file: &OfficeFile, session: &SessionRef) -> Outcome<Value> {
        let credentials = self.credentials()?;
        if let Some(error) = self.bridge_error.locked().clone() {
            return Err(Failure::http(503, error));
        }
        let base = &self.settings.bridge_url;
        let (key, origin) = {
            let s = session.locked();
            (s.key.clone(), s.origin())
        };
        let issue = |kind| credentials.tickets.issue(kind, &key, origin.as_ref());
        let edit = file.editable;
        let mut editor = json!({
            "mode": if edit { "edit" } else { "view" },
            "lang": "zh",
            "region": "zh-CN",
            "user": self.user,
        });
        if edit {
            editor["callbackUrl"] = format!("{base}/office/callback/{}", issue(TicketKind::Callback)).into();
        }
        editor["coEditing"] = json!({"mode": "fast", "change": false});
        editor["customization"] = json!({
            "autosave": true,
            "forcesave": true,
            "compactHeader": true,
            "toolbarHideFileName": true,
            "hideRightMenu": true,
            "integrationMode": "embed",
            "features": {"spellcheck": {"mode": false, "change": true}, "featuresTips": false, "tabBackground": {"mode": "toolbar", "change": true}},
            "feedback": false,
            "help": false,
            "plugins": false,
            "macros": false,
            "mentionShare": false,
            "close": {"visible": true, "text": "关闭"},
        });
        let mut config = json!({
            "type": "desktop",
            "width": "100%",
            "height": "100%",
            "documentType": file.document_type,
            "document": {
                "fileType": file.ext,
                "key": key,
                "title": file.name,
                "url": format!("{base}/office/file/{}", issue(TicketKind::File)),
                "permissions": {"edit": edit, "comment": edit, "review": edit, "fillForms": edit, "modifyFilter": edit,
                    "modifyContentControl": edit, "download": true, "print": true, "copy": true, "chat": false, "protect": false},
            },
            "editorConfig": editor,
        });
        config["token"] = credentials.jwt.sign(&config, None).into();
        Ok(json!({"api": format!("{}/web-apps/apps/api/documents/api.js", self.settings.document_server), "key": key, "config": config}))
    }

    fn session(&self, key: &str) -> Outcome<SessionRef> {
        self.sessions.get(key).ok_or_else(|| Failure::http(404, "no open document with this key"))
    }

    fn reopened(&self, ticket: &Ticket) -> Outcome<SessionRef> {
        let Some(origin) = ticket.origin.as_ref().filter(|_| self.sessions.get(&ticket.key).is_none()) else {
            return self.session(&ticket.key);
        };
        let gone = || Failure::http(404, "the document's folder is no longer one of this app's roots");
        let workspace = self.workspaces.locked().get(&origin.owner).cloned().ok_or_else(gone)?;
        let root = workspace.finder_roots()?.into_iter().find(|root| root.id == origin.root).ok_or_else(gone)?;
        let source = Source::Root { owner: origin.owner.clone(), root };
        Ok(self.sessions.resume(&ticket.key, &source.identity(&origin.path)?, source, origin))
    }

    fn save_failure(failure: &Failure) -> String {
        if failure.status == 403 {
            return format!("这个工作区不能写：{}", failure.message);
        }
        failure.message.clone()
    }

    fn saved(session: &mut Session, result: SaveResult) {
        session.state = if result.conflict { SaveState::Conflict } else { SaveState::Saved };
        session.saved_at = Some(crate::runtime::unix_now().as_millis() as i64);
        session.expected_ms = result.modified_ms;
        if result.conflict {
            session.conflict_path = Some(result.path.clone());
            session.message = Some(format!("文件在打开后被别处改过，没有覆盖它；修改另存为 {}", result.path));
        } else {
            session.message = None;
        }
        session.target = result.path;
        session.touch();
    }

    fn store(self: &Arc<Self>, session: &SessionRef, payload: &serde_json::Map<String, Value>, last: bool) -> Outcome<()> {
        let url = payload.get("url").and_then(Value::as_str).ok_or_else(|| Failure::http(400, "save callback without url"))?;
        let mut s = session.locked();
        if let Some(kind) = payload.get("filetype").and_then(Value::as_str)
            && kind.to_lowercase() != OfficeFiles::extension(&s.path)
        {
            return Err(Failure::http(409, format!("Document Server saved .{kind}, the file is {}", s.path)));
        }
        s.state = SaveState::Saving;
        s.message = None;
        s.touch();
        let credentials = self.credentials()?;
        let attempt = (|| -> Outcome<()> {
            let server = DocumentServer::new(&self.settings.document_server, Some(Arc::clone(&credentials)), None);
            let url = url.to_owned();
            let data = Service::wait(move || async move { server.download(&url, MAX_DOCUMENT_BYTES as usize).await })??;
            if !data.starts_with(b"PK\x03\x04") {
                return Err(Failure::http(502, "Document Server returned something that is not an Office file"));
            }
            let result = self.files.save(&s.source, &s.target, &data, s.expected_ms)?;
            Self::saved(&mut s, result);
            Ok(())
        })();
        match attempt {
            Ok(()) => {
                if last {
                    self.sessions.close(&mut s);
                }
                Ok(())
            }
            Err(failure) => {
                s.state = SaveState::Failed;
                s.message = Some(Self::save_failure(&failure));
                s.touch();
                Err(failure)
            }
        }
    }
}

struct Bridged(Arc<Service>);

impl Handlers for Bridged {
    fn file(&self, ticket: &str, authorization: Option<&str>) -> Outcome<(String, Vec<u8>)> {
        let service = &self.0;
        let credentials = service.credentials()?;
        credentials.jwt.verify_header(authorization)?;
        let ticket = credentials.tickets.consume(ticket)?;
        let (source, path, expected_ms) = {
            let s = service.reopened(&ticket)?;
            let s = s.locked();
            (s.source.clone(), s.target.clone(), s.expected_ms)
        };
        let file = OfficeFiles::open(source, &path)?;
        if file.modified_ms != expected_ms {
            return Err(Failure::http(409, "the file changed after the editor was opened; reload the page"));
        }
        Ok((file.name.clone(), service.files.read(&file)?))
    }

    fn callback(&self, ticket: &str, authorization: Option<&str>, body: &[u8]) -> Outcome<()> {
        let service = &self.0;
        let credentials = service.credentials()?;
        let ticket = credentials.tickets.read(ticket, TicketKind::Callback)?;
        let payload = match authorization {
            Some(header) => credentials.jwt.verify_header(Some(header))?,
            None => {
                let text = std::str::from_utf8(body).map_err(Failure::internal)?;
                let parsed: Value = serde_json::from_str(if text.is_empty() { "{}" } else { text }).map_err(Failure::internal)?;
                let token = parsed["token"].as_str().ok_or_else(|| Failure::token_refused("callback without token"))?;
                credentials.jwt.verify(token)?
            }
        };
        if payload.get("key").and_then(Value::as_str) != Some(ticket.key.as_str()) {
            return Err(Failure::token_refused("callback for another document"));
        }
        let session = service.reopened(&ticket)?;
        let status = payload.get("status").and_then(|s| s.as_f64().or_else(|| s.as_str()?.parse().ok())).unwrap_or(f64::NAN);
        let editors = payload.get("users").and_then(Value::as_array).map_or(0, Vec::len);
        log::info!("callback status {status} for {} ({editors} editor(s))", session.locked().target);
        match status as i64 {
            1 => {
                let mut s = session.locked();
                s.editors = editors;
                s.touch();
                Ok(())
            }
            2 | 6 => {
                let service = Arc::clone(service);
                service.store(&session, &payload, status as i64 == 2)
            }
            3 | 7 => {
                let mut s = session.locked();
                s.state = SaveState::Failed;
                s.message = Some("Document Server 没能生成要保存的文件，修改仍留在编辑器里。".into());
                s.touch();
                if status as i64 == 3 {
                    service.sessions.close(&mut s);
                }
                Ok(())
            }
            4 => {
                service.sessions.close(&mut session.locked());
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveBody {
    key: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NewBody {
    root: String,
    kind: BlankKind,
    #[serde(default)]
    dir: String,
    name: Option<String>,
}

fn fields<T: serde::de::DeserializeOwned>(body: Option<Value>) -> Outcome<T> {
    serde_json::from_value(body.unwrap_or_else(|| json!({}))).map_err(|error| Failure::invalid(error.to_string()))
}

pub enum OfficeCall {
    Server,
    State { root: Option<String>, dir: Option<String>, path: Option<String> },
    Lent { name: String },
    Status { key: String },
    Save { body: Option<Value> },
    New { body: Option<Value> },
}

impl Service {
    fn start(finder: Arc<crate::runtime::fs::Shared>) -> Outcome<Arc<Service>> {
        let settings = OfficeSettings::from_env()?;
        let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
        let service = Arc::new(Service {
            files: OfficeFiles(finder),
            bridge: Mutex::default(),
            sessions: EditSessions::default(),
            workspaces: Mutex::default(),
            secrets: Mutex::default(),
            cached: Mutex::default(),
            bridge_error: Mutex::new(Some("not started".into())),
            user: json!({"id": format!("agentrt-{user}"), "name": user}),
            settings,
        });
        let handlers: Arc<dyn Handlers> = Arc::new(Bridged(Arc::clone(&service)));
        let starting = Arc::clone(&service);
        let task = crate::runtime::rt::spawn_abortable(async move {
            match bridge::listen(&starting.settings.socket) {
                Ok(listener) => {
                    *starting.bridge_error.locked() = None;
                    bridge::serve(listener, handlers).await;
                }
                Err(error) => {
                    let message = format!("office bridge could not listen on {}: {error}", starting.settings.socket);
                    log::error!("{message}");
                    *starting.bridge_error.locked() = Some(message);
                }
            }
        });
        *service.bridge.locked() = Some(task);
        Ok(service)
    }

    fn call(&self, call: OfficeCall, scope: Option<&Workspace>) -> Outcome<Value> {
        match call {
            OfficeCall::Server => Ok(self.status(true)),
            OfficeCall::State { root, dir, path } => self.props(scope, root.as_deref(), dir.as_deref(), path.as_deref()),
            OfficeCall::Lent { name } => self.lent(scope, &name),
            OfficeCall::Status { key } => Ok(self.session(&key)?.locked().status()),
            OfficeCall::Save { body } => {
                let SaveBody { key } = fields(body)?;
                if key.js_trim().is_empty() {
                    return Err(Failure::invalid("all fields must be non-empty strings"));
                }
                let session = self.session(&key)?;
                let credentials = self.credentials().map_err(|f| Failure::http(503, f.message))?;
                let server = DocumentServer::new(&self.settings.document_server, Some(credentials), None);
                let forced = key.clone();
                let result = Service::wait(move || async move { server.forcesave(&forced).await })??;
                Ok(json!({"result": result, "status": session.locked().status()}))
            }
            OfficeCall::New { body } => {
                let NewBody { root, kind, dir, name } = fields(body)?;
                if root.js_trim().is_empty() {
                    return Err(Failure::invalid("all fields must be non-empty strings"));
                }
                let name = name.map(|n| n.js_trim().to_owned()).filter(|n| !n.is_empty());
                let found =
                    Self::found(&Self::roots(scope)?, Some(&root)).ok_or_else(|| Failure::http(403, format!("{root} is not one of this app's roots")))?;
                let path = OfficeFiles::create(&found, &dir, kind, name.as_deref())?;
                Ok(json!({"path": path}))
            }
        }
    }
}

#[derive(Clone)]
pub struct OfficeService(Arc<Service>, Option<Workspace>);

impl OfficeService {
    pub fn open(workspace: &Workspace) -> std::result::Result<Self, String> {
        Service::start(workspace.shared_fs()).map(|service| Self(service, None)).map_err(|f| f.message)
    }

    pub fn for_web(&self, files: Option<Workspace>) -> Self {
        if let Some(workspace) = &files {
            self.0.workspaces.locked().insert(workspace.owner().unwrap_or_default().to_owned(), workspace.clone());
        }
        Self(Arc::clone(&self.0), files)
    }

    pub fn call(&self, call: OfficeCall) -> std::result::Result<Value, String> {
        self.0.call(call, self.1.as_ref()).map_err(String::from)
    }

    pub fn close(&self) {
        if let Some(task) = self.0.bridge.locked().take() {
            task.abort();
        }
        *self.0.bridge_error.locked() = Some("office bridge is not listening".into());
    }
}
