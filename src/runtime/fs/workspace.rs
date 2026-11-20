use super::beneath::{self, Dir};
use super::path::NodePath;
use crate::runtime::failure::{Failure, Outcome};
use icu_collator::{Collator, CollatorBorrowed, options::CollatorOptions};
use icu_locale_core::Locale;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, DirBuilder, File, Metadata, OpenOptions},
    io::{self, ErrorKind, Read, Write},
    path::Path,
    time::UNIX_EPOCH,
};

const MAX_ENTRIES: usize = if cfg!(test) { 50 } else { 5000 };
const MAX_PREVIEW_BYTES: u64 = 256 * 1024;
const MAX_RAW_BYTES: u64 = 64 * 1024 * 1024;
const IMAGES: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"];

#[cfg(all(windows, not(test)))]
use super::path::known_folder;

/// Tests never ask the shell for the real folders: `ACDE_TEST_KNOWN_<NAME>` stands in for them.
#[cfg(all(windows, test))]
fn known_folder(name: &str) -> Option<String> {
    std::env::var(format!("ACDE_TEST_KNOWN_{name}")).ok().filter(|dir| !dir.is_empty())
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RootMode {
    Writable,
    ReadOnly,
    Unavailable,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RootSource {
    Desktop,
    Downloads,
    Default,
    Granted,
    Private,
    Place,
    Lent,
    Removable,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct FinderRoot {
    pub id: String,
    #[serde(skip)]
    pub path: String,
    #[serde(rename = "path", skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    pub label: String,
    pub source: RootSource,
    pub mode: RootMode,
    pub reason: Option<String>,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drive: Option<String>,
}

impl FinderRoot {
    pub fn new(path: String, label: &str, source: RootSource) -> Self {
        Self {
            id: path.clone(),
            shown: Some(path.clone()),
            path,
            label: label.into(),
            source,
            mode: RootMode::Writable,
            reason: None,
            exists: true,
            place: None,
            drive: None,
        }
    }

    pub fn access(self, writable: bool) -> Self {
        match self.mode {
            RootMode::Writable if !writable => Self { mode: RootMode::ReadOnly, ..self },
            _ => self,
        }
    }

    pub(crate) fn writable(&self) -> Outcome<()> {
        match self.mode {
            RootMode::ReadOnly => Err(Failure::http(403, format!("{} was picked read-only", self.label))),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinderEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub modified_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_place: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewKind {
    Text,
    Image,
    Binary,
    TooLarge,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinderPreview {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified_ms: i64,
    pub kind: PreviewKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

#[derive(Serialize)]
pub struct Listing {
    pub root: FinderRoot,
    pub path: String,
    pub entries: Vec<FinderEntry>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Serialize)]
pub struct Stat {
    #[serde(flatten)]
    pub entry: FinderEntry,
    pub path: String,
}

pub struct Contents {
    pub data: Vec<u8>,
    pub modified_ms: i64,
    pub version: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Create {
    #[default]
    Always,
    New,
    Never,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOptions {
    #[serde(default)]
    pub create: Create,
    pub if_modified_ms: Option<f64>,
    pub if_version: Option<String>,
    #[serde(default)]
    pub mkdirs: bool,
}

#[derive(Default, Deserialize)]
pub struct MkdirOptions {
    #[serde(default)]
    pub recursive: bool,
}

pub struct Resolved {
    pub absolute: String,
    pub real: String,
    pub relative: String,
}

pub trait ModifiedMs {
    fn modified_ms(&self) -> i64;
}
impl ModifiedMs for Metadata {
    fn modified_ms(&self) -> i64 {
        self.modified().ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map_or(0, |since| since.as_millis() as i64)
    }
}

impl FinderEntry {
    pub fn read(absolute: &str, name: &str) -> Outcome<Self> {
        let info = fs::symlink_metadata(absolute)?;
        let kind = info.file_type().into();
        Ok(Self {
            name: name.to_owned(),
            kind,
            size: info.len(),
            modified_ms: info.modified_ms(),
            target: (kind == EntryKind::Symlink).then(|| fs::read_link(absolute).map_or_else(|_| "?".into(), |t| t.to_string_lossy().into_owned())),
            version: None,
            in_place: None,
        })
    }
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Symlink => "symlink",
            Self::Other => "other",
        }
    }
}

impl From<fs::FileType> for EntryKind {
    fn from(kind: fs::FileType) -> Self {
        match () {
            _ if kind.is_dir() => Self::Directory,
            _ if kind.is_file() => Self::File,
            _ if kind.is_symlink() => Self::Symlink,
            _ => Self::Other,
        }
    }
}

pub struct WorkspaceFs {
    collator: CollatorBorrowed<'static>,
    home: String,
    data: String,
    system: bool,
}

impl WorkspaceFs {
    pub fn new() -> Self {
        let home = std::env::home_dir().map_or_else(|| "/".into(), |h| h.to_string_lossy().into_owned());
        let data = std::env::var("XDG_DATA_HOME").ok().filter(|dir| Path::new(dir).is_absolute()).unwrap_or_else(|| {
            let base = std::env::var("HOME").ok().filter(|dir| !dir.is_empty()).unwrap_or_else(|| home.clone());
            base.join(".local").join("share")
        });
        Self { collator: Self::collator(), home, data, system: true }
    }

    #[cfg(test)]
    pub(crate) fn at(home: &str) -> Self {
        Self { collator: Self::collator(), home: home.to_owned(), data: home.join("data"), system: false }
    }

    pub fn place_dir(&self, name: &str) -> Option<String> {
        let (key, fallback) = match name {
            "desktop" => ("DESKTOP", "Desktop"),
            "documents" => ("DOCUMENTS", "Documents"),
            "downloads" => ("DOWNLOAD", "Downloads"),
            "music" => ("MUSIC", "Music"),
            "pictures" => ("PICTURES", "Pictures"),
            "videos" => ("VIDEOS", "Videos"),
            _ => return None,
        };
        Some(self.user_dir(key, fallback))
    }

    pub fn private_dir(&self, owner: &str) -> String {
        self.data.join("acde").join("files").join(owner)
    }

    fn collator() -> CollatorBorrowed<'static> {
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
            .and_then(|value| {
                let tag = value.split(['.', '@']).next().unwrap_or("").replace('_', "-");
                (tag != "C" && tag != "POSIX").then(|| tag.parse::<Locale>().ok()).flatten()
            })
            .unwrap_or(Locale::UNKNOWN);
        Collator::try_new((&locale).into(), CollatorOptions::default())
            .or_else(|_| Collator::try_new(Default::default(), CollatorOptions::default()))
            .expect("compiled root collation data")
    }

    fn user_dir(&self, name: &str, fallback: &str) -> String {
        let home = self.home.as_str();
        let key = format!("XDG_{name}_DIR");
        let dir = std::env::var(&key).ok().filter(|dir| self.system && !dir.is_empty()).or_else(|| {
            let text = fs::read(home.join(".config").join("user-dirs.dirs")).ok()?;
            let prefix = format!("{key}=\"");
            String::from_utf8_lossy(&text).split(['\n', '\r']).find_map(|line| {
                let rest = line.strip_prefix(prefix.as_str())?;
                let (value, _) = rest.split_once('"')?;
                (!value.is_empty()).then(|| value.to_owned())
            })
        });
        #[cfg(windows)]
        let dir = dir.or_else(|| self.system.then(|| known_folder(name)).flatten());
        let Some(dir) = dir else { return home.join(fallback) };
        let path = match dir.strip_prefix("$HOME") {
            Some(rest) if rest.is_empty() || rest.starts_with('/') || (cfg!(windows) && rest.starts_with('\\')) => {
                format!("{home}{rest}")
            }
            _ => dir,
        };
        let selected = cfg_select! {
            windows => super::path::windows_directory(&path).filter(|dir| {
                let same_spelling = super::path::windows_directory(home)
                    .is_some_and(|home| dir.eq_ignore_ascii_case(&home));
                let same_location = || fs::canonicalize(home).ok().zip(fs::canonicalize(dir).ok())
                    .is_some_and(|(home, dir)| home == dir);
                !same_spelling && !same_location()
            }),
            _ => path.is_absolute().then(|| Self::directory(&path)).filter(|dir| !Self::same_directory(home, dir)),
        };
        selected.unwrap_or_else(|| home.join(fallback))
    }

    #[cfg(not(windows))]
    fn directory(path: &str) -> String {
        match path.normalize().trim_end_matches('/') {
            "" => "/".into(),
            clean => clean.into(),
        }
    }

    #[cfg(not(windows))]
    fn same_directory(home: &str, dir: &str) -> bool {
        Self::directory(home) == dir || fs::canonicalize(home).ok().zip(fs::canonicalize(dir).ok()).is_some_and(|(home, dir)| home == dir)
    }

    pub fn desktop(&self) -> Outcome<String> {
        let desktop = self.user_dir("DESKTOP", "Desktop");
        if !Path::new(&desktop).exists() {
            DirBuilder::new().recursive(true).create(&desktop)?;
        }
        Ok(desktop)
    }

    pub fn downloads(&self) -> Option<String> {
        Some(self.user_dir("DOWNLOAD", "Downloads")).filter(|dir| Path::new(dir).exists())
    }

    pub fn roots(&self) -> Outcome<Vec<FinderRoot>> {
        let fallback = self.home.join("agentos-work");
        let work = Some(fallback).filter(|path| Path::new(path).exists());
        let desktop = self.desktop()?;
        let root = |path: String, label: &str, source: RootSource| FinderRoot::new(path, label, source);
        let downloads = self.downloads().filter(|dir| *dir != desktop && work.as_ref() != Some(dir));
        let down = downloads.map(|path| root(path, "Downloads", RootSource::Downloads));
        let work = work.filter(|path| *path != desktop).map(|path| root(path, "~/agentos-work", RootSource::Default));
        let drives = super::removable::mounted().into_iter().map(|mount| mount.root(true));
        Ok([root(desktop, "Desktop", RootSource::Desktop)].into_iter().chain(down).chain(work).chain(drives).collect())
    }

    pub fn granted(path: &str) -> FinderRoot {
        let exists = Path::new(path).is_dir();
        let label = Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path);
        FinderRoot {
            mode: if exists { RootMode::Writable } else { RootMode::Unavailable },
            reason: (!exists).then(|| "the folder no longer exists".to_owned()),
            exists,
            ..FinderRoot::new(path.to_owned(), label, RootSource::Granted)
        }
    }

    fn realpath(path: &str) -> std::io::Result<String> {
        fs::canonicalize(path).map(|real| real.to_string_lossy().into_owned())
    }

    pub(super) fn clean(relative: &str) -> Outcome<String> {
        if relative.contains('\0') || relative.is_absolute() {
            return Err(Failure::invalid("path must be relative to the workspace"));
        }
        #[cfg(windows)]
        if relative.split('/').any(|name| !matches!(name, "" | "." | "..") && !super::path::windows_file_name(name)) {
            return Err(Failure::invalid("not a valid workspace-relative Windows path"));
        }
        let normal = if relative.is_empty() { "." } else { relative }.normalize();
        let clean = normal.trim_end_matches('/');
        let clean = if clean.is_empty() { "." } else { clean };
        if clean == ".." || clean.starts_with("../") {
            return Err(Failure::invalid("path escapes the workspace"));
        }
        Ok(clean.to_owned())
    }

    fn inside(real_root: &str, path: &str) -> bool {
        Path::new(path).starts_with(real_root)
    }

    pub fn resolve(root: &FinderRoot, relative: &str, must_exist: bool) -> Outcome<Resolved> {
        let clean = Self::clean(relative)?;
        let clean = clean.as_str();
        let absolute = if clean == "." { root.path.clone() } else { root.path.join(clean) };
        let real_root = Self::realpath(&root.path)?;
        let inside = |path: &str| Self::inside(&real_root, path);
        let mut real = absolute.clone();
        if must_exist {
            real = Self::realpath(&absolute).map_err(|error| denied_or(error, || Failure::http(404, "no such file or directory")))?;
            if !inside(&real) {
                return Err(Failure::invalid("path leaves the workspace through a symlink"));
            }
        } else {
            if clean == "." {
                return Err(Failure::invalid("the workspace root itself cannot be replaced"));
            }
            let missing = || Failure::invalid("parent directory is missing or outside the workspace");
            let parent = Self::realpath(absolute.dirname()).map_err(|error| denied_or(error, missing))?;
            if !inside(&parent) {
                return Err(missing());
            }
        }
        let relative = if clean == "." { String::new() } else { clean.to_owned() };
        Ok(Resolved { absolute, real, relative })
    }

    fn walk_error(error: io::Error) -> Failure {
        match beneath::escaped(&error) {
            true => Failure::invalid("path leaves the workspace through a symlink"),
            false => denied_or(error, || Failure::http(404, "no such file or directory")),
        }
    }

    fn dir(root: &FinderRoot, relative: &str) -> Outcome<Dir> {
        Dir::open(&Self::realpath(&root.path)?, relative).map_err(Self::walk_error)
    }

    pub(super) fn parent(root: &FinderRoot, relative: &str) -> Outcome<(Dir, String)> {
        let dir = Self::dir(root, relative.dirname())?;
        Ok((dir, relative.basename().to_owned()))
    }

    pub fn list_in(&self, root: FinderRoot, relative: &str, hidden: bool) -> Outcome<Listing> {
        let Resolved { absolute, real, relative } = Self::resolve(&root, relative, true)?;
        if !fs::metadata(&real)?.is_dir() {
            return Err(Failure::invalid("not a directory"));
        }
        let dir = Self::dir(&root, &relative)?;
        let (mut named, mut skipped) = (Vec::new(), 0);
        for entry in fs::read_dir(dir.path())?.filter_map(|entry| entry.ok()).filter(|entry| hidden || !Self::is_hidden(entry)) {
            match entry.file_name().into_string() {
                Ok(name) => named.push((entry.file_type().is_ok_and(|t| t.is_dir()), name)),
                Err(_) => skipped += 1,
            }
        }
        named.sort_by(|(a_dir, a), (b_dir, b)| b_dir.cmp(a_dir).then_with(|| self.collator.compare(a, b)).then_with(|| a.cmp(b)));
        let truncated = named.len() > MAX_ENTRIES;
        let entries = named.iter().take(MAX_ENTRIES).map(|(_, name)| FinderEntry::read(&absolute.join(name), name)).collect::<Outcome<Vec<_>>>()?;
        Ok(Listing { root, path: relative, entries, truncated, skipped })
    }

    fn is_hidden(entry: &fs::DirEntry) -> bool {
        if entry.file_name().to_string_lossy().starts_with('.') {
            return true;
        }
        cfg_select! {
            windows => {
                use std::os::windows::fs::MetadataExt;
                use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM};
                let hidden = FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM;
                entry.metadata().is_ok_and(|m| m.file_attributes() & hidden != 0)
            }
            _ => false,
        }
    }

    fn open_nofollow(real: &Path) -> std::io::Result<File> {
        Self::no_follow(OpenOptions::new().read(true), real, false)
    }

    pub(super) fn no_follow(options: &mut OpenOptions, path: impl AsRef<Path>, nonblock: bool) -> io::Result<File> {
        cfg_select! {
            unix => {
                use rustix::fs::OFlags;
                use std::os::unix::fs::OpenOptionsExt;
                let flags = if nonblock { OFlags::NOFOLLOW | OFlags::NONBLOCK } else { OFlags::NOFOLLOW };
                options.custom_flags(flags.bits() as i32).mode(0o644).open(path)
            }
            windows => {
                use std::os::windows::fs::OpenOptionsExt;
                _ = nonblock;
                use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
                let file = options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path)?;
                if file.metadata()?.file_type().is_symlink() {
                    return Err(std::io::Error::other("refusing to follow a symlink"));
                }
                Ok(file)
            }
        }
    }

    pub fn preview_in(root: &FinderRoot, relative: &str) -> Outcome<FinderPreview> {
        let Resolved { absolute, real, relative } = Self::resolve(root, relative, true)?;
        let info = fs::metadata(&real)?;
        if !info.is_file() {
            return Err(Failure::invalid("not a regular file"));
        }
        let size = info.len();
        let preview = |kind, content, truncated| FinderPreview {
            path: relative.clone(),
            name: absolute.basename().to_owned(),
            size,
            modified_ms: info.modified_ms(),
            kind,
            content,
            truncated,
        };
        if IMAGES.contains(&absolute.extname().to_lowercase().as_str()) {
            let kind = if size <= MAX_RAW_BYTES { PreviewKind::Image } else { PreviewKind::TooLarge };
            return Ok(preview(kind, None, None));
        }
        let (dir, name) = Self::parent(root, &relative)?;
        let mut file = Self::open_nofollow(&dir.child(&name))?;
        let mut buffer = vec![0; size.min(MAX_PREVIEW_BYTES) as usize];
        let read = file.read(&mut buffer)?;
        buffer.truncate(read);
        if buffer.contains(&0) {
            return Ok(preview(PreviewKind::Binary, None, None));
        }
        let content = match String::from_utf8(buffer) {
            Ok(text) => text,
            Err(_) if size <= MAX_PREVIEW_BYTES => return Ok(preview(PreviewKind::Binary, None, None)),
            Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
        };
        let content = content.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(content);
        Ok(preview(PreviewKind::Text, Some(content), Some(size > MAX_PREVIEW_BYTES)))
    }

    pub fn raw_in(root: &FinderRoot, relative: &str) -> Outcome<(String, Vec<u8>)> {
        let (absolute, contents) = Self::contents(root, relative)?;
        Ok((absolute.basename().to_owned(), contents.data))
    }

    pub(super) fn opened(root: &FinderRoot, relative: &str) -> Outcome<(String, File, Metadata)> {
        let Resolved { absolute, real, relative } = Self::resolve(root, relative, true)?;
        if !fs::metadata(&real)?.is_file() {
            return Err(Failure::invalid("not a regular file"));
        }
        let (dir, name) = Self::parent(root, &relative)?;
        let file = Self::no_follow(OpenOptions::new().read(true), dir.child(&name), true)?;
        let info = file.metadata()?;
        if !info.is_file() {
            return Err(Failure::invalid("not a regular file"));
        }
        Ok((absolute, file, info))
    }

    fn contents(root: &FinderRoot, relative: &str) -> Outcome<(String, Contents)> {
        let (absolute, file, info) = Self::opened(root, relative)?;
        let too_large = || Failure::http(413, "file exceeds 64 MiB");
        if info.len() > MAX_RAW_BYTES {
            return Err(too_large());
        }
        let version = super::save::version(&file)?;
        let mut data = Vec::with_capacity(info.len() as usize);
        file.take(MAX_RAW_BYTES + 1).read_to_end(&mut data)?;
        if data.len() as u64 > MAX_RAW_BYTES {
            return Err(too_large());
        }
        Ok((absolute, Contents { data, modified_ms: info.modified_ms(), version }))
    }

    pub fn stat_in(root: &FinderRoot, relative: &str) -> Outcome<Stat> {
        let Resolved { absolute, relative, .. } = Self::resolve(root, relative, true)?;
        let mut entry = FinderEntry::read(&absolute, absolute.basename())?;
        if entry.kind == EntryKind::File && !relative.is_empty() {
            let (dir, name) = Self::parent(root, &relative)?;
            let file = Self::no_follow(OpenOptions::new().read(true), dir.child(&name), true)?;
            entry.version = Some(super::save::version(&file)?);
        }
        Ok(Stat { entry, path: relative })
    }

    pub fn read_bytes_in(root: &FinderRoot, relative: &str) -> Outcome<Contents> {
        Self::contents(root, relative).map(|(_, contents)| contents)
    }

    #[cfg(test)]
    pub fn read_text_in(root: &FinderRoot, relative: &str) -> Outcome<(String, i64)> {
        let Contents { data, modified_ms, .. } = Self::read_bytes_in(root, relative)?;
        let text = String::from_utf8(data).map_err(|_| Failure::invalid("file is not valid UTF-8 text").with(crate::runtime::failure::Errno::Ilseq))?;
        Ok((text, modified_ms))
    }

    pub fn write_file_in(root: &FinderRoot, relative: &str, data: &[u8], options: &WriteOptions) -> Outcome<FinderEntry> {
        root.writable()?;
        if data.len() as u64 > MAX_RAW_BYTES {
            return Err(Failure::http(413, "content exceeds 64 MiB"));
        }
        let expects = options.if_modified_ms.is_some() || options.if_version.is_some();
        if options.create == Create::New && expects {
            return Err(Failure::invalid("ifModifiedMs and ifVersion cannot go with create: 'new'"));
        }
        if options.mkdirs && options.create != Create::Never {
            let parent = Self::clean(relative)?.dirname().to_owned();
            if parent != "." {
                Self::make_dirs(root, &parent)?;
            }
        }
        let Resolved { absolute, relative, .. } = Self::resolve(root, relative, false)?;
        let (dir, name) = Self::parent(root, &relative)?;
        let fresh = |dir: &Dir| -> Outcome<FinderEntry> {
            let mut file = Self::no_follow(OpenOptions::new().write(true).create_new(true), dir.child(&name), true).map_err(|error| match error.kind() {
                ErrorKind::AlreadyExists => Failure::exists("file already exists"),
                _ => error.into(),
            })?;
            file.write_all(data)?;
            file.sync_all()?;
            FinderEntry::read(&absolute, absolute.basename())
        };
        if options.create == Create::New {
            return fresh(&dir);
        }
        let file = match Self::no_follow(OpenOptions::new().write(true), dir.child(&name), true) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound && expects => {
                return Err(Failure::conflict("file no longer exists or is not a regular file"));
            }
            Err(error) if error.kind() == ErrorKind::NotFound && options.create == Create::Always => return fresh(&dir),
            Err(error) => return Err(error.into()),
        };
        let info = file.metadata()?;
        if !info.is_file() {
            return Err(Failure::invalid("not a regular file"));
        }
        let stale = options.if_modified_ms.is_some_and(|expected| info.modified_ms() as f64 != expected)
            || options.if_version.as_ref().is_some_and(|expected| super::save::version(&file).ok().as_ref() != Some(expected));
        if stale {
            return Err(Failure::conflict("file changed since it was opened; reload before saving"));
        }
        let saved = super::save::save(&dir, &name, data, file, &info)?;
        let mut entry = FinderEntry::read(&absolute, absolute.basename())?;
        let file = Self::no_follow(OpenOptions::new().read(true), dir.child(&name), true)?;
        entry.version = Some(super::save::version(&file)?);
        entry.in_place = matches!(saved, super::save::Saved::InPlace).then_some(true);
        Ok(entry)
    }

    pub fn mkdir_in(root: &FinderRoot, relative: &str, options: &MkdirOptions) -> Outcome<FinderEntry> {
        root.writable()?;
        let absolute = match options.recursive {
            true => Self::make_dirs(root, relative)?,
            false => {
                let Resolved { absolute, relative, .. } = Self::resolve(root, relative, false)?;
                let (dir, name) = Self::parent(root, &relative)?;
                DirBuilder::new().create(dir.child(&name))?;
                absolute
            }
        };
        FinderEntry::read(&absolute, absolute.basename())
    }

    fn child_path(dir: &str, name: &str) -> Outcome<String> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
            return Err(Failure::invalid("not a valid file name"));
        }
        Ok(match Self::clean(dir)?.as_str() {
            "." => name.to_owned(),
            dir => format!("{dir}/{name}"),
        })
    }

    pub fn children_in(root: &FinderRoot, relative: &str) -> Outcome<Vec<(String, EntryKind)>> {
        let Resolved { real, relative, .. } = Self::resolve(root, relative, true)?;
        if !fs::metadata(&real)?.is_dir() {
            return Err(Failure::invalid("not a directory"));
        }
        let dir = Self::dir(root, &relative)?;
        let mut children = Vec::new();
        for entry in fs::read_dir(dir.path())? {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else { continue };
            children.push((name, entry.file_type()?.into()));
        }
        Ok(children)
    }

    pub fn child_in(root: &FinderRoot, dir: &str, name: &str, create: Option<EntryKind>) -> Outcome<Option<EntryKind>> {
        let Resolved { relative, .. } = Self::resolve(root, &Self::child_path(dir, name)?, false)?;
        let (parent, name) = Self::parent(root, &relative)?;
        let absolute = parent.child(&name);
        match fs::symlink_metadata(&absolute) {
            Ok(info) => Ok(Some(info.file_type().into())),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                if matches!(create, Some(EntryKind::File | EntryKind::Directory)) {
                    root.writable()?;
                }
                let made = match create {
                    None | Some(EntryKind::Other | EntryKind::Symlink) => return Ok(None),
                    Some(EntryKind::File) => Self::no_follow(OpenOptions::new().write(true).create_new(true), &absolute, false).map(drop),
                    Some(EntryKind::Directory) => DirBuilder::new().create(&absolute),
                };
                match made {
                    Ok(()) => Ok(create),
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(Some(fs::symlink_metadata(&absolute)?.file_type().into())),
                    Err(error) => Err(error.into()),
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn remove_child_in(root: &FinderRoot, dir: &str, name: &str, recursive: bool) -> Outcome<bool> {
        root.writable()?;
        let Resolved { relative, .. } = Self::resolve(root, &Self::child_path(dir, name)?, false)?;
        let (parent, name) = Self::parent(root, &relative)?;
        let absolute = parent.child(&name);
        let info = match fs::symlink_metadata(&absolute) {
            Ok(info) => info,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        match (info.is_dir(), recursive) {
            (true, true) => fs::remove_dir_all(&absolute)?,
            (true, false) => fs::remove_dir(&absolute)?,
            (false, _) => fs::remove_file(&absolute)?,
        }
        Ok(true)
    }

    pub(super) fn make_dirs(root: &FinderRoot, relative: &str) -> Outcome<String> {
        let clean = Self::clean(relative)?;
        let real_root = Self::realpath(&root.path)?;
        let mut at = root.path.clone();
        if clean == "." {
            return Ok(at);
        }
        let parts: Vec<&str> = clean.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            at = at.join(part);
            let prefix = parts[..=index].join("/");
            let blocked = || match index + 1 == parts.len() {
                true => Failure::exists("a file with that name already exists"),
                false => Failure::invalid("not a directory"),
            };
            match Dir::open(&real_root, &prefix) {
                Ok(_) => continue,
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) if error.kind() == ErrorKind::NotADirectory => return Err(blocked()),
                Err(error) => return Err(Self::walk_error(error)),
            }
            let parent = Self::dir(root, prefix.dirname())?;
            let child = parent.child(part);
            match fs::symlink_metadata(&child) {
                Ok(info) if info.file_type().is_symlink() => match fs::metadata(&child) {
                    Ok(_) => return Err(blocked()),
                    Err(_) => return Err(Failure::invalid("a symlink on the path points nowhere")),
                },
                Ok(_) => return Err(blocked()),
                Err(error) if error.kind() == ErrorKind::NotFound => match DirBuilder::new().create(&child) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::AlreadyExists && Dir::open(&real_root, &prefix).is_ok() => {}
                    Err(error) => return Err(error.into()),
                },
                Err(error) => return Err(error.into()),
            }
        }
        Ok(at)
    }

    pub fn numbered(name: &str, n: u32) -> String {
        if n <= 1 {
            return name.to_owned();
        }
        let ext = name.extname();
        format!("{} {n}{ext}", &name[..name.len() - ext.len()])
    }

    pub fn copy_into(root: &FinderRoot, dir: &str, name: &str, mut reader: File) -> Outcome<FinderEntry> {
        root.writable()?;
        let Resolved { absolute, real, relative } = Self::resolve(root, dir, true)?;
        if !fs::metadata(&real)?.is_dir() {
            return Err(Failure::invalid("not a directory"));
        }
        let pinned = Self::dir(root, &relative)?;
        for n in 1..10_000 {
            let candidate = Self::numbered(name, n);
            let target = absolute.join(&candidate);
            let created = Self::no_follow(OpenOptions::new().write(true).create_new(true), pinned.child(&candidate), false);
            let mut file = match created {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            if let Err(error) = std::io::copy(&mut reader, &mut file).and_then(|_| file.sync_all()) {
                drop(file);
                let _ = fs::remove_file(pinned.child(&candidate));
                return Err(error.into());
            }
            return FinderEntry::read(&target, &candidate);
        }
        Err(Failure::exists("no free name for the copy"))
    }

    pub fn rename_in(root: &FinderRoot, from: &str, to: &str) -> Outcome<FinderEntry> {
        root.writable()?;
        let source = Self::resolve(root, from, true)?;
        let target = Self::resolve(root, to, false)?;
        if source.relative.is_empty() {
            return Err(Failure::invalid("the workspace root cannot be renamed"));
        }
        let from = Self::parent(root, &source.relative)?;
        let to = Self::parent(root, &target.relative)?;
        beneath::rename_no_replace((&from.0, &from.1), (&to.0, &to.1)).map_err(|error| match error.kind() {
            ErrorKind::AlreadyExists => Failure::exists("target already exists"),
            _ => error.into(),
        })?;
        FinderEntry::read(&target.absolute, target.absolute.basename())
    }

    pub fn remove_in(root: &FinderRoot, relative: &str) -> Outcome<()> {
        root.writable()?;
        let Resolved { relative, .. } = Self::resolve(root, relative, false)?;
        if relative.is_empty() {
            return Err(Failure::invalid("the workspace root cannot be deleted"));
        }
        let (dir, name) = Self::parent(root, &relative)?;
        let target = dir.child(&name);
        let info = fs::symlink_metadata(&target).map_err(|error| denied_or(error, || Failure::http(404, "no such file or directory")))?;
        match info.is_dir() {
            true => fs::remove_dir(&target)?,
            false => fs::remove_file(&target)?,
        }
        Ok(())
    }
}

fn denied_or(error: io::Error, otherwise: impl FnOnce() -> Failure) -> Failure {
    match error.kind() {
        ErrorKind::PermissionDenied => error.into(),
        _ => otherwise(),
    }
}

#[cfg(all(test, windows))]
#[path = "tests/windows.rs"]
mod windows_tests;

#[cfg(all(test, unix))]
#[path = "tests/unix.rs"]
mod tests;
