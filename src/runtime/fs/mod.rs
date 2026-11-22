mod access;
mod beneath;
mod copy;
pub(crate) mod path;
mod picked;
mod ranged;
pub(crate) mod removable;
mod save;
pub(crate) mod workspace;

pub use access::files::{Files, LentRoot};
#[cfg(unix)]
pub(crate) use access::handed::claim as claim_handed;
#[cfg(unix)]
pub use access::handed::hand_fds;
pub use access::lent::LentFile;
use access::{admit, cache::RootCache, folder::valid_owner};
pub(crate) use beneath::escaped;
#[cfg(not(windows))]
use path::NodePath;
#[cfg(windows)]
use path::windows_directory;
pub use picked::{Dropped, PickedFolders};
pub use ranged::{MAX_CHUNK, Opened, Range, Writes};
use ranged::{OpenWrite, RangeOptions};
pub use workspace::EntryKind;
use workspace::{Contents, FinderRoot, MkdirOptions, WorkspaceFs, WriteOptions};

use crate::runtime::blocking;
use crate::runtime::failure::{Failure, Outcome};
use napi::Result;
use napi_derive::napi;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

trait IntoJson {
    fn json(self) -> Outcome<Value>;
}
impl<T: Serialize> IntoJson for Outcome<T> {
    fn json(self) -> Outcome<Value> {
        serde_json::to_value(self?).map_err(Failure::internal)
    }
}

pub(crate) struct Shared {
    pub(crate) fs: WorkspaceFs,
    lent: std::sync::Mutex<std::collections::HashMap<String, Vec<LentRoot>>>,
    files: std::sync::Mutex<std::collections::HashMap<String, Vec<LentFile>>>,
    lendings: AtomicU64,
}

fn fresh_name(stem: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..).map(|n| if n == 1 { stem.to_owned() } else { format!("{stem}-{n}") }).find(|name| !taken(name)).unwrap_or_default()
}

fn forget<T>(known: &mut Vec<T>, gone: impl Fn(&T) -> bool) -> bool {
    let before = known.len();
    known.retain(|item| !gone(item));
    before != known.len()
}

#[derive(Clone, Copy)]
enum View<'a> {
    All(Option<&'a dyn Fn(&str) -> bool>),
    Files(&'a Files),
}

impl Shared {
    fn files_roots(&self, owner: Option<&str>, files: &Files) -> Outcome<Vec<FinderRoot>> {
        let owner = owner.ok_or_else(|| Failure::invalid("private folders belong to an app"))?;
        files.roots(&self.fs, owner, self.lent_to(owner))
    }

    fn changes(&self) -> u64 {
        self.lendings.load(Ordering::Relaxed)
    }

    fn lent_to(&self, owner: &str) -> Vec<LentRoot> {
        self.lent.lock().unwrap_or_else(|e| e.into_inner()).get(owner).cloned().unwrap_or_default()
    }

    fn lend(&self, owner: &str, root: LentRoot) -> Outcome<String> {
        if !valid_owner(owner) {
            return Err(Failure::invalid("folders are lent to an app"));
        }
        let mut lent = self.lent.lock().unwrap_or_else(|e| e.into_inner());
        let roots = lent.entry(owner.to_owned()).or_default();
        let root = match root.name.is_empty() {
            true => LentRoot { name: fresh_name("work", |name| roots.iter().any(|root| root.name == name)), ..root },
            false => root,
        }
        .checked()?;
        let name = root.name.clone();
        roots.retain(|known| known.name != name);
        roots.push(root);
        self.lendings.fetch_add(1, Ordering::Relaxed);
        Ok(name)
    }

    fn lend_file(&self, owner: &str, file: LentFile) -> Outcome<String> {
        if !valid_owner(owner) {
            return Err(Failure::invalid("files are lent to an app"));
        }
        let mut lent = self.files.lock().unwrap_or_else(|e| e.into_inner());
        let files = lent.entry(owner.to_owned()).or_default();
        let file = match file.name.is_empty() {
            true => LentFile { name: fresh_name("file", |name| files.iter().any(|file| file.name == name)), ..file },
            false => file,
        }
        .checked()?;
        let name = file.name.clone();
        files.retain(|known| known.name != name);
        files.push(file);
        Ok(name)
    }

    pub(crate) fn lent_file(&self, owner: &str, name: &str) -> Option<LentFile> {
        let lent = self.files.lock().unwrap_or_else(|e| e.into_inner());
        lent.get(owner)?.iter().find(|file| file.name == name).cloned()
    }

    fn unlend(&self, owner: &str, name: &str) -> bool {
        let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
        let file = files.get_mut(owner).is_some_and(|files| forget(files, |file| file.name == name));
        drop(files);
        let mut lent = self.lent.lock().unwrap_or_else(|e| e.into_inner());
        let root = lent.get_mut(owner).is_some_and(|roots| forget(roots, |root| root.name == name));
        self.lendings.fetch_add(1, Ordering::Relaxed);
        file || root
    }

    fn root_in(roots: &[FinderRoot], view: View, id: &str) -> Outcome<FinderRoot> {
        match view {
            View::All(_) => {
                roots.iter().find(|root| root.on_disk() == Some(id) || root.id == id).cloned().ok_or_else(|| Failure::invalid("not a Runtime workspace"))
            }
            View::Files(_) => Files::find(roots.iter().filter(|root| root.id == id).cloned().collect(), id),
        }
    }

    fn path_calls(view: View, what: &str) -> Outcome<()> {
        match view {
            View::Files(files) if !files.paths() => Err(Failure::http(403, format!("{what} works only in system apps that declare shared places"))),
            _ => Ok(()),
        }
    }
}

pub enum FsCall {
    Roots,
    List { root: String, path: Option<String>, hidden: Option<bool> },
    Preview { root: String, path: String },
    Download { root: String, path: String },
    Locate { path: String },
    OpenPath { root: String, path: String },
    Mkdir { root: String, path: String, options: Option<Value> },
    Stat { root: String, path: String },
    ReadText { root: String, path: String },
    ReadBytes { root: String, path: String },
    WriteFile { root: String, path: String, data: Option<Vec<u8>>, options: Option<Value> },
    Copy { from_root: String, from_path: String, to_root: String, to_dir: String },
    Rename { root: String, from: String, to: String },
    Delete { root: String, path: String },
    ReadRange { root: String, path: String, offset: Option<u64>, length: Option<u64>, options: Option<Value> },
    OpenWrite { root: String, path: String, options: Option<Value> },
    WriteChunk { id: String, offset: Option<u64>, data: Option<Vec<u8>> },
    TruncateWrite { id: String, size: Option<u64> },
    CommitWrite { id: String },
    AbortWrite { id: String },
}

pub enum FsReply {
    Json(Value),
    File { name: String, data: Vec<u8> },
    Text { text: String, modified_ms: i64, size: u64, version: String },
    Bytes { data: Vec<u8>, modified_ms: i64, version: String },
    Range(Range),
    Done,
}

#[derive(Debug)]
pub struct FsError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl From<Failure> for FsError {
    fn from(failure: Failure) -> Self {
        Self { status: failure.status, code: failure.errno.as_str(), message: failure.message }
    }
}

impl FsCall {
    fn uses_roots(&self) -> bool {
        !matches!(self, Self::WriteChunk { .. } | Self::TruncateWrite { .. } | Self::CommitWrite { .. } | Self::AbortWrite { .. })
    }

    fn reads(&self) -> bool {
        matches!(
            self,
            Self::Locate { .. }
                | Self::List { .. }
                | Self::Preview { .. }
                | Self::Download { .. }
                | Self::Stat { .. }
                | Self::ReadText { .. }
                | Self::ReadBytes { .. }
                | Self::ReadRange { .. }
        )
    }

    fn targets(&self) -> impl Iterator<Item = &str> {
        let other = match self {
            Self::Copy { to_root, .. } => Some(to_root.as_str()),
            _ => None,
        };
        self.scope().into_iter().chain(other)
    }

    fn scope(&self) -> Option<&str> {
        match self {
            Self::Roots | Self::Locate { .. } => None,
            Self::WriteChunk { .. } | Self::TruncateWrite { .. } | Self::CommitWrite { .. } | Self::AbortWrite { .. } => None,
            Self::List { root, .. }
            | Self::Preview { root, .. }
            | Self::Download { root, .. }
            | Self::OpenPath { root, .. }
            | Self::Mkdir { root, .. }
            | Self::Stat { root, .. }
            | Self::ReadText { root, .. }
            | Self::ReadBytes { root, .. }
            | Self::WriteFile { root, .. }
            | Self::Copy { from_root: root, .. }
            | Self::Rename { root, .. }
            | Self::Delete { root, .. }
            | Self::ReadRange { root, .. }
            | Self::OpenWrite { root, .. } => Some(root),
        }
    }
}

impl Shared {
    fn on(fs: WorkspaceFs) -> Self {
        Self { fs, lent: Default::default(), files: Default::default(), lendings: AtomicU64::new(0) }
    }

    fn call(&self, call: &FsCall, view: View, roots: &[FinderRoot], writes: &Writes) -> Outcome<FsReply> {
        let json = |value: Outcome<Value>| value.map(FsReply::Json);
        let root = |id: &str| Self::root_in(roots, view, id);
        match call {
            FsCall::Roots => serde_json::to_value(Self::allowed(roots, view)).map(FsReply::Json).map_err(Failure::internal),
            FsCall::List { root: at, path, hidden } => json(self.fs.list_in(root(at)?, path.as_deref().unwrap_or(""), hidden.unwrap_or(false)).json()),
            FsCall::Preview { root: at, path } => json(WorkspaceFs::preview_in(&root(at)?, path).json()),
            FsCall::Download { root: at, path } => WorkspaceFs::raw_in(&root(at)?, path).map(|(name, data)| FsReply::File { name, data }),
            FsCall::Locate { path } => json(Self::locate(roots, path, view)),
            FsCall::OpenPath { root: at, path } => {
                Self::path_calls(view, "fs.openPath()")?;
                json(self.open_path(&root(at)?, path))
            }
            FsCall::Mkdir { root: at, path, options } => {
                let options: MkdirOptions = options_of("mkdir", options)?;
                json(WorkspaceFs::mkdir_in(&root(at)?, path, &options).json())
            }
            FsCall::Stat { root: at, path } => json(WorkspaceFs::stat_in(&root(at)?, path).json()),
            FsCall::ReadText { root: at, path } => {
                let Contents { data, modified_ms, version } = WorkspaceFs::read_bytes_in(&root(at)?, path)?;
                let text = String::from_utf8(data).map_err(|_| Failure::invalid("file is not valid UTF-8 text").with(crate::runtime::failure::Errno::Ilseq))?;
                Ok(FsReply::Text { size: text.len() as u64, text, modified_ms, version })
            }
            FsCall::ReadBytes { root: at, path } => {
                let Contents { data, modified_ms, version } = WorkspaceFs::read_bytes_in(&root(at)?, path)?;
                Ok(FsReply::Bytes { data, modified_ms, version })
            }
            FsCall::WriteFile { root: at, path, data, options } => {
                let options: WriteOptions = options_of("writeFile", options)?;
                let data = bytes(data)?;
                json(WorkspaceFs::write_file_in(&root(at)?, path, data, &options).json())
            }
            FsCall::ReadRange { root: at, path, offset, length, options } => {
                let options: RangeOptions = options_of("readRange", options)?;
                WorkspaceFs::read_range_in(&root(at)?, path, *offset, *length, &options).map(FsReply::Range)
            }
            FsCall::OpenWrite { root: at, path, options } => {
                let options: OpenWrite = options_of("openWrite", options)?;
                json(writes.open_in(&root(at)?, path, &options).json())
            }
            FsCall::WriteChunk { id, offset, data } => writes.chunk(id, *offset, bytes(data)?).map(|()| FsReply::Done),
            FsCall::TruncateWrite { id, size } => writes.truncate(id, *size).map(|()| FsReply::Done),
            FsCall::CommitWrite { id } => json(writes.commit(id).json()),
            FsCall::AbortWrite { id } => writes.abort(id).map(|()| FsReply::Done),
            FsCall::Copy { from_root, from_path, to_root, to_dir } => json(copy::copy(&root(from_root)?, from_path, &root(to_root)?, to_dir)),
            FsCall::Rename { root: at, from, to } => json(WorkspaceFs::rename_in(&root(at)?, from, to).json()),
            FsCall::Delete { root: at, path } => {
                WorkspaceFs::remove_in(&root(at)?, path)?;
                json(Ok(serde_json::json!({"status": "deleted", "path": path})))
            }
        }
    }

    fn allowed(roots: &[FinderRoot], view: View) -> Vec<FinderRoot> {
        match view {
            View::All(Some(allows)) => roots.iter().filter(|root| allows(&root.path)).cloned().collect(),
            _ => roots.to_vec(),
        }
    }

    fn locate(roots: &[FinderRoot], path: &str, view: View) -> Outcome<Value> {
        Self::path_calls(view, "fs.locate()")?;
        let home = std::env::home_dir().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default();
        let expanded = match path.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with('/') || cfg!(windows) && rest.starts_with('\\') => {
                format!("{home}{rest}")
            }
            _ => path.to_owned(),
        };
        #[cfg(windows)]
        let clean = windows_directory(&expanded);
        #[cfg(not(windows))]
        let clean = expanded.is_absolute().then(|| expanded.normalize());
        let clean = clean.ok_or_else(|| Failure::invalid("path must be absolute (or ~/…), or give root and path"))?;
        #[cfg(windows)]
        let relative = |root: &str| path::windows_within(std::path::Path::new(&clean), root);
        #[cfg(not(windows))]
        let relative = |root: &str| clean.within(root).map(|path| path.trim_start_matches('/').to_owned());
        let roots = Self::allowed(roots, view);
        let within = |root: &FinderRoot| root.on_disk().and_then(|path| relative(path).map(|inside| (root.id.clone(), path.len(), inside)));
        let (root, _, relative) = roots.iter().filter_map(within).max_by_key(|(_, length, _)| *length).ok_or_else(|| match view {
            View::All(None) => Failure::http(404, "not inside any workspace the Finder lists"),
            _ => Failure::http(403, "not inside a workspace allowed for this app"),
        })?;
        Ok(serde_json::json!({"root": root, "path": relative}))
    }

    fn open_path(&self, workspace: &FinderRoot, path: &str) -> Outcome<Value> {
        let resolved = WorkspaceFs::resolve(workspace, path)?;
        if resolved.relative.is_empty() {
            return Err(Failure::invalid("a folder cannot be opened in the browser"));
        }
        if resolved.relative.split('/').any(|part| part.starts_with('.')) {
            return Err(Failure::invalid("hidden files and folders are not opened in the browser"));
        }
        WorkspaceFs::raw_in(workspace, &resolved.relative)?;
        Ok(serde_json::json!({"root": workspace.id, "path": resolved.relative, "file": resolved.real}))
    }
}

fn bytes(data: &Option<Vec<u8>>) -> Outcome<&[u8]> {
    data.as_deref().ok_or_else(|| Failure::invalid("data must be a string or a Uint8Array"))
}

fn options_of<T: Default + serde::de::DeserializeOwned>(method: &str, options: &Option<Value>) -> Outcome<T> {
    match options {
        None | Some(Value::Null) => Ok(T::default()),
        Some(value) => <T as serde::Deserialize>::deserialize(value).map_err(|error| Failure::invalid(format!("{method} options: {error}"))),
    }
}

#[derive(Clone)]
pub struct Workspace {
    shared: Arc<Shared>,
    roots: Option<Arc<[String]>>,
    owner: Option<Arc<str>>,
    files: Option<Arc<Files>>,
    writes: Writes,
    cache: RootCache,
}

impl Workspace {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::on(Arc::new(Shared::on(WorkspaceFs::new())))
    }

    fn on(shared: Arc<Shared>) -> Self {
        Self { shared, roots: None, owner: None, files: None, writes: Writes::default(), cache: RootCache::default() }
    }

    #[cfg(test)]
    pub(crate) fn at(home: &str) -> Self {
        Self::on(Arc::new(Shared::on(WorkspaceFs::at(home))))
    }

    pub fn scoped(&self, roots: Option<Vec<String>>, owner: Option<&str>) -> Self {
        let owner = owner.filter(|owner| valid_owner(owner)).map(Into::into);
        Self { roots: roots.map(Into::into), owner, ..Self::on(Arc::clone(&self.shared)) }
    }

    pub(crate) fn lend(&self, owner: &str, root: LentRoot) -> Outcome<String> {
        self.shared.lend(owner, root)
    }

    pub(crate) fn unlend(&self, owner: &str, name: &str) -> bool {
        self.shared.unlend(owner, name)
    }

    pub(crate) fn lend_file(&self, owner: &str, file: LentFile) -> Outcome<String> {
        self.shared.lend_file(owner, file)
    }

    pub(crate) fn lent_file(&self, name: &str) -> Option<LentFile> {
        self.shared.lent_file(self.owner.as_deref()?, name)
    }

    pub(crate) fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }

    pub(crate) fn lent_root(&self, name: &str) -> Option<FinderRoot> {
        let owner = self.owner.as_deref()?;
        self.shared.lent_to(owner).iter().find(|root| root.name == name).map(LentRoot::root)
    }

    pub fn files(&self, files: Files, owner: &str) -> Self {
        let owner = Some(owner).filter(|owner| valid_owner(owner)).map(Into::into);
        Self { roots: Some(Arc::from([])), owner, files: Some(Arc::new(files)), ..Self::on(Arc::clone(&self.shared)) }
    }

    fn allows(&self, root: &str) -> bool {
        let Some(roots) = &self.roots else { return true };
        roots.iter().any(|r| r == root)
    }

    pub(crate) fn admit(&self, path: &str) -> Outcome<String> {
        self.admit_with(path, &[])
    }

    pub(crate) fn admit_with(&self, path: &str, handed: &[String]) -> Outcome<String> {
        let mut roots = self.finder_roots()?;
        roots.extend(handed.iter().map(|dir| WorkspaceFs::folder(dir)));
        admit::real_inside(&roots, path)
    }

    fn fresh_roots(&self) -> Outcome<Vec<FinderRoot>> {
        let owner = self.owner.as_deref();
        match &self.files {
            Some(files) => self.shared.files_roots(owner, files),
            None => self.shared.fs.roots(),
        }
    }

    pub(crate) fn finder_roots(&self) -> Outcome<Vec<FinderRoot>> {
        let roots = self.fresh_roots()?;
        Ok(if self.files.is_some() { roots } else { roots.into_iter().filter(|root| self.allows(&root.path)).collect() })
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn shared_fs(&self) -> Arc<Shared> {
        Arc::clone(&self.shared)
    }

    pub fn call(&self, call: FsCall) -> std::result::Result<FsReply, FsError> {
        let owner = self.owner.as_deref();
        let allows = |root: &str| self.allows(root);
        let view = match &self.files {
            Some(_) if owner.is_none() => return Err(Failure::http(403, "this web has no app to own its files").into()),
            Some(files) => View::Files(files),
            None => {
                self.in_manifest(&call)?;
                View::All(self.roots.as_ref().map(|_| &allows as &dyn Fn(&str) -> bool))
            }
        };
        if !call.uses_roots() {
            return Ok(self.shared.call(&call, view, &[], &self.writes)?);
        }
        let grants = self.shared.changes();
        let mut roots = match call {
            FsCall::Roots => self.cache.keep(grants, self.fresh_roots()?),
            _ => self.cache.get(grants, || self.fresh_roots())?,
        };
        if call.targets().any(|id| Shared::root_in(&roots, view, id).is_err()) {
            let grants = self.shared.changes();
            roots = self.cache.keep(grants, self.fresh_roots()?);
        }
        match self.shared.call(&call, view, &roots, &self.writes) {
            Err(failure) if call.reads() => {
                let missing = roots.iter().any(|root| root.exists && root.on_disk().is_some_and(|path| !std::path::Path::new(path).is_dir()));
                let grants = self.shared.changes();
                let fresh = self.fresh_roots()?;
                if !missing && fresh == roots {
                    return Err(failure.into());
                }
                let fresh = self.cache.keep(grants, fresh);
                Ok(self.shared.call(&call, view, &fresh, &self.writes)?)
            }
            reply => Ok(reply?),
        }
    }

    fn in_manifest(&self, call: &FsCall) -> Outcome<()> {
        if self.roots.is_none() {
            return Ok(());
        }
        if let FsCall::Copy { to_root, .. } = call
            && !self.allows(to_root)
        {
            return Err(Failure::http(403, format!("this app may not use the files of {to_root} (not in its manifest's capabilities)")));
        }
        match call.scope() {
            Some(root) if self.allows(root) => Ok(()),
            None => Ok(()),
            what => {
                let what = what.unwrap_or("its workspaces");
                Err(Failure::http(403, format!("this app may not use the files of {what} (not in its manifest's capabilities)")))
            }
        }
    }
}

#[napi]
pub struct Finder {
    shared: Arc<Shared>,
}

#[napi]
impl Finder {
    #[napi(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { shared: Arc::new(Shared::on(WorkspaceFs::new())) }
    }

    #[napi]
    pub async fn locate(&self, path: String) -> Result<Value> {
        let shared = Arc::clone(&self.shared);
        blocking(move || Ok(Shared::locate(&shared.fs.roots()?, &path, View::All(None))?)).await
    }

    #[napi]
    pub async fn admit(&self, path: String, roots: Option<Vec<String>>, owner: Option<String>, places: Option<Vec<String>>) -> Result<String> {
        blocking(move || {
            let all = Workspace::new();
            Ok(match (places, owner.as_deref()) {
                (Some(places), Some(owner)) => all.files(Files::new(&places, false), owner).admit_with(&path, &roots.unwrap_or_default())?,
                _ => all.scoped(roots, owner.as_deref()).admit(&path)?,
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_outside_the_manifest_is_refused_with_eacces() {
        let workspace = Workspace::new().scoped(Some(vec!["/allowed".into()]), Some("app"));
        let Err(error) = workspace.call(FsCall::List { root: "/elsewhere".into(), path: None, hidden: None }) else { panic!("not refused") };
        assert_eq!((error.status, error.code), (403, "EACCES"));
        assert_eq!(error.message, "this app may not use the files of /elsewhere (not in its manifest's capabilities)");
    }

    fn json(reply: std::result::Result<FsReply, FsError>) -> Value {
        match reply {
            Ok(FsReply::Json(value)) => value,
            Ok(_) => Value::String("not json".into()),
            Err(error) => serde_json::json!({"status": error.status, "code": error.code}),
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("acde-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("Desktop")).unwrap();
        base
    }

    #[test]
    fn locate_returns_a_path_relative_to_an_allowed_root() {
        let base = scratch("locate");
        let root = base.join("Desktop");
        let file = root.join("folder").join("entry.txt");
        let sibling = base.join("Desktop Other").join("entry.txt");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::create_dir_all(sibling.parent().unwrap()).unwrap();
        std::fs::write(&file, "inside").unwrap();
        std::fs::write(&sibling, "outside").unwrap();
        let all = Workspace::at(&base.to_string_lossy());
        let root = root.to_string_lossy().into_owned();
        let scoped = all.scoped(Some(vec![root.clone()]), Some("locate-probe"));
        let other = all.scoped(Some(vec![]), Some("other"));
        let locate = |workspace: &Workspace, path: &str| json(workspace.call(FsCall::Locate { path: path.to_owned() }));
        let expected = serde_json::json!({"root": root, "path": "folder/entry.txt"});
        assert_eq!(locate(&scoped, file.to_str().unwrap()), expected);
        assert_eq!(locate(&scoped, &format!("{root}/folder/entry.txt")), expected);
        assert_eq!(locate(&scoped, &root), serde_json::json!({"root": root, "path": ""}));
        assert_eq!(locate(&scoped, sibling.to_str().unwrap())["code"], "EACCES");
        assert_eq!(locate(&scoped, &format!("{root}/../Desktop Other/entry.txt"))["code"], "EACCES");
        assert_eq!(locate(&other, file.to_str().unwrap())["code"], "EACCES");
        assert_eq!(locate(&scoped, "relative/entry.txt")["code"], "EINVAL");
        _ = std::fs::remove_dir_all(base);
    }

    #[cfg(windows)]
    #[test]
    fn locate_matches_windows_roots_ignoring_case_and_separators() {
        let base = scratch("locate-case");
        let root = base.join("Desktop");
        let file = root.join("Folder").join("Entry.txt");
        let sibling = base.join("Desktop2").join("entry.txt");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::create_dir_all(sibling.parent().unwrap()).unwrap();
        std::fs::write(&file, "inside").unwrap();
        std::fs::write(&sibling, "outside").unwrap();
        let root = root.to_string_lossy().into_owned();
        let scoped = Workspace::at(&base.to_string_lossy()).scoped(Some(vec![root.clone()]), Some("locate-case"));
        let locate = |path: &str| json(scoped.call(FsCall::Locate { path: path.to_owned() }));
        let found = |path: &str| serde_json::json!({"root": root, "path": path});
        let file = file.to_str().unwrap();
        assert_eq!(locate(&file.to_lowercase()), found("folder/entry.txt"));
        assert_eq!(locate(&file.to_uppercase()), found("FOLDER/ENTRY.TXT"));
        assert_eq!(locate(&root.to_lowercase()), found(""));
        assert_eq!(locate(&format!("{}/Folder\\Entry.txt", root.replace('\\', "/").to_uppercase())), found("Folder/Entry.txt"));
        assert_eq!(locate(&sibling.to_str().unwrap().to_lowercase())["code"], "EACCES");
        assert_eq!(locate(&format!("{}2\\entry.txt", root.to_lowercase()))["code"], "EACCES");
        _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_hand_off_stays_inside_the_callers_roots() {
        let base = scratch("admit");
        let root = base.join("Desktop");
        let outside = base.join("Outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("in.pdf"), "in").unwrap();
        std::fs::write(outside.join("out.pdf"), "out").unwrap();
        let all = Workspace::at(&base.to_string_lossy());
        let with = all.scoped(Some(vec![root.to_string_lossy().into_owned()]), Some("admit-with"));
        let without = all.scoped(Some(vec![]), Some("admit-without"));
        let every_root = all.scoped(None, Some("admit-with"));
        let inside = root.join("in.pdf");
        let real = std::fs::canonicalize(&inside).unwrap();
        let expected = real.to_string_lossy().trim_start_matches(r"\\?\").to_owned();
        assert_eq!(with.admit(inside.to_str().unwrap()).unwrap(), expected);
        assert!(every_root.admit(inside.to_str().unwrap()).is_ok());
        assert_eq!(without.admit(inside.to_str().unwrap()).unwrap_err().status, 403);
        assert_eq!(with.admit(outside.join("out.pdf").to_str().unwrap()).unwrap_err().status, 403);
        assert_eq!(with.admit(root.join("missing.pdf").to_str().unwrap()).unwrap_err().status, 404);
        #[cfg(windows)]
        {
            let link = root.join("Linked");
            let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&outside).output().unwrap();
            assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
            assert_eq!(with.admit(link.join("out.pdf").to_str().unwrap()).unwrap_err().status, 403);
            std::fs::remove_dir(&link).unwrap();
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
            assert_eq!(with.admit(root.join("linked/out.pdf").to_str().unwrap()).unwrap_err().status, 403);
        }
        _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn listings_leave_hidden_entries_out_unless_asked() {
        let base = scratch("hidden");
        let desktop = base.join("Desktop");
        std::fs::write(desktop.join("seen.txt"), "a").unwrap();
        std::fs::write(desktop.join(".dot"), "b").unwrap();
        let root = desktop.to_string_lossy().into_owned();
        let app = Workspace::at(&base.to_string_lossy()).scoped(Some(vec![root.clone()]), Some("lister"));
        let names = |hidden: Option<bool>| {
            let listed = json(app.call(FsCall::List { root: root.clone(), path: None, hidden }));
            listed["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect::<Vec<_>>()
        };
        assert_eq!(names(None), ["seen.txt"]);
        assert_eq!(names(Some(false)), ["seen.txt"]);
        assert_eq!(names(Some(true)).len(), 2);
        _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_root_that_appears_or_goes_away_is_seen_by_the_next_call() {
        use crate::runtime::fs::path::NodePath;
        use std::path::Path;
        let base = scratch("root-cache");
        let home = base.to_string_lossy().into_owned();
        let web = Workspace::at(&home).scoped(None, Some("cached"));
        let list = |root: &str| json(web.call(FsCall::List { root: root.to_owned(), path: None, hidden: None }));
        let (desktop, downloads) = (home.join("Desktop"), home.join("Downloads"));
        assert_eq!(list(&desktop)["entries"], serde_json::json!([]));
        assert_eq!(list(&downloads)["code"], "EINVAL");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::write(Path::new(&downloads).join("got.txt"), "got").unwrap();
        assert_eq!(list(&downloads)["entries"][0]["name"], "got.txt");
        std::fs::remove_dir_all(&desktop).unwrap();
        assert_eq!(list(&desktop)["entries"], serde_json::json!([]));
        assert!(Path::new(&desktop).is_dir());
        std::fs::remove_dir_all(&downloads).unwrap();
        assert_eq!(list(&downloads)["code"], "EINVAL");
        _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_lent_root_is_seen_and_gone_at_once() {
        let base = scratch("lent-cache");
        let work = base.join("Work");
        std::fs::create_dir_all(&work).unwrap();
        let all = Workspace::at(&base.to_string_lossy());
        let app = all.files(Files::new(&[], false), "cached");
        let list = || json(app.call(FsCall::List { root: "work".into(), path: None, hidden: None }));
        assert_eq!(list()["code"], "EACCES");
        all.lend("cached", LentRoot::open("Work".into(), true, &work.to_string_lossy()).unwrap()).unwrap();
        assert_eq!(list()["entries"], serde_json::json!([]));
        assert!(all.unlend("cached", "work"));
        assert_eq!(list()["code"], "EACCES");
        _ = std::fs::remove_dir_all(base);
    }
}
