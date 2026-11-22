use super::beneath::{self, Dir, OpenOptions};
use super::path::NodePath;
use crate::runtime::failure::{Failure, Outcome};
use icu_collator::{Collator, CollatorBorrowed, options::CollatorOptions};
use icu_locale_core::Locale;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, Metadata},
    io::{self, ErrorKind, Read, Write},
    path::Path,
    time::UNIX_EPOCH,
};

const MAX_ENTRIES: usize = if cfg!(test) { 50 } else { 5000 };
const MAX_PREVIEW_BYTES: u64 = 256 * 1024;
const MAX_RAW_BYTES: u64 = 64 * 1024 * 1024;
const IMAGES: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"];

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

    pub(crate) fn open(&self) -> io::Result<Dir> {
        beneath::open_root(&self.path)
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
    pub(super) fn read(dir: &Dir, name: &str) -> Outcome<Self> {
        let info = dir.symlink_metadata(name)?;
        let link = info.file_type().is_symlink();
        let target = link.then(|| dir.read_link_contents(name).map_or_else(|_| "?".into(), |t| t.to_string_lossy().into_owned()));
        Ok(Self::of(&info, name, target))
    }

    fn of(info: &cap_std::fs::Metadata, name: &str, target: Option<String>) -> Self {
        Self {
            name: name.to_owned(),
            kind: info.file_type().into(),
            size: info.len(),
            modified_ms: info.modified().ok().and_then(|at| at.into_std().duration_since(UNIX_EPOCH).ok()).map_or(0, |since| since.as_millis() as i64),
            target,
            version: None,
            in_place: None,
        }
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

impl From<cap_std::fs::FileType> for EntryKind {
    fn from(kind: cap_std::fs::FileType) -> Self {
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

mod userdirs;

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

    pub(crate) fn inner(relative: &str) -> Outcome<String> {
        let clean = Self::clean(relative)?;
        Ok(if clean == "." { String::new() } else { clean })
    }

    pub fn resolve(root: &FinderRoot, relative: &str) -> Outcome<Resolved> {
        let relative = Self::inner(relative)?;
        let real_root = Self::realpath(&root.path)?;
        let real = Self::realpath(&root.path.join(if relative.is_empty() { "." } else { &relative }))
            .map_err(|error| denied_or(error, || Failure::http(404, "no such file or directory")))?;
        if !Path::new(&real).starts_with(&real_root) {
            return Err(Failure::invalid("path leaves the workspace through a symlink"));
        }
        Ok(Resolved { real, relative })
    }

    pub(super) fn dir(root: &FinderRoot, relative: &str) -> Outcome<Dir> {
        Ok(beneath::open_dir(&root.open()?, relative)?)
    }

    pub(super) fn parent(root: &FinderRoot, relative: &str) -> Outcome<(Dir, String)> {
        Ok((Self::dir(root, relative.dirname())?, relative.basename().to_owned()))
    }

    pub(super) fn target(root: &FinderRoot, relative: &str) -> Outcome<(Dir, String)> {
        let relative = Self::inner(relative)?;
        if relative.is_empty() {
            return Err(Failure::invalid("the workspace root itself cannot be replaced"));
        }
        let dir = beneath::open_dir(&root.open()?, relative.dirname()).map_err(|error| match error.kind() {
            ErrorKind::NotFound | ErrorKind::NotADirectory => Failure::invalid("parent directory is missing or outside the workspace"),
            _ => error.into(),
        })?;
        Ok((dir, relative.basename().to_owned()))
    }

    fn entry(root: &FinderRoot, relative: &str) -> Outcome<FinderEntry> {
        if relative.is_empty() {
            return Ok(FinderEntry::of(&root.open()?.dir_metadata()?, root.path.basename(), None));
        }
        let (dir, name) = Self::parent(root, relative)?;
        FinderEntry::read(&dir, &name)
    }

    pub(super) fn versioned(dir: &Dir, name: &str) -> Outcome<FinderEntry> {
        let mut entry = FinderEntry::read(dir, name)?;
        entry.version = Some(super::save::version(&beneath::read(dir, name, true)?)?);
        Ok(entry)
    }

    pub fn list_in(&self, root: FinderRoot, relative: &str, hidden: bool) -> Outcome<Listing> {
        let relative = Self::inner(relative)?;
        let dir = Self::dir(&root, &relative)?;
        let (mut named, mut skipped) = (Vec::new(), 0);
        for entry in dir.entries()?.filter_map(|entry| entry.ok()).filter(|entry| hidden || !Self::is_hidden(entry)) {
            match entry.file_name().into_string() {
                Ok(name) => named.push((entry.file_type().is_ok_and(|t| t.is_dir()), name)),
                Err(_) => skipped += 1,
            }
        }
        named.sort_by(|(a_dir, a), (b_dir, b)| b_dir.cmp(a_dir).then_with(|| self.collator.compare(a, b)).then_with(|| a.cmp(b)));
        let truncated = named.len() > MAX_ENTRIES;
        let entries = named.iter().take(MAX_ENTRIES).map(|(_, name)| FinderEntry::read(&dir, name)).collect::<Outcome<Vec<_>>>()?;
        Ok(Listing { root, path: relative, entries, truncated, skipped })
    }

    fn is_hidden(entry: &cap_std::fs::DirEntry) -> bool {
        if entry.file_name().to_string_lossy().starts_with('.') {
            return true;
        }
        cfg_select! {
            windows => {
                use cap_std::fs::MetadataExt;
                use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM};
                let hidden = FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM;
                entry.metadata().is_ok_and(|m| m.file_attributes() & hidden != 0)
            }
            _ => false,
        }
    }

    pub fn preview_in(root: &FinderRoot, relative: &str) -> Outcome<FinderPreview> {
        let (relative, mut file, info) = Self::opened(root, relative)?;
        let size = info.len();
        let name = relative.basename().to_owned();
        let image = IMAGES.contains(&name.extname().to_lowercase().as_str());
        let preview = |kind, content, truncated| FinderPreview { path: relative, name, size, modified_ms: info.modified_ms(), kind, content, truncated };
        if image {
            let kind = if size <= MAX_RAW_BYTES { PreviewKind::Image } else { PreviewKind::TooLarge };
            return Ok(preview(kind, None, None));
        }
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
        let (relative, contents) = Self::contents(root, relative)?;
        Ok((relative.basename().to_owned(), contents.data))
    }

    pub(crate) fn opened(root: &FinderRoot, relative: &str) -> Outcome<(String, File, Metadata)> {
        let relative = Self::inner(relative)?;
        if relative.is_empty() {
            return Err(Failure::invalid("not a regular file"));
        }
        let (dir, name) = Self::parent(root, &relative)?;
        let file = beneath::read(&dir, &name, true)?;
        let info = file.metadata()?;
        if !info.is_file() {
            return Err(Failure::invalid("not a regular file"));
        }
        Ok((relative, file, info))
    }

    fn contents(root: &FinderRoot, relative: &str) -> Outcome<(String, Contents)> {
        let (relative, file, info) = Self::opened(root, relative)?;
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
        Ok((relative, Contents { data, modified_ms: info.modified_ms(), version }))
    }

    pub fn stat_in(root: &FinderRoot, relative: &str) -> Outcome<Stat> {
        let relative = Self::inner(relative)?;
        let entry = Self::entry(root, &relative)?;
        let entry = match entry.kind {
            EntryKind::File => Self::parent(root, &relative).and_then(|(dir, name)| Self::versioned(&dir, &name))?,
            _ => entry,
        };
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
            Self::make_dirs(root, Self::clean(relative)?.dirname())?;
        }
        let (dir, name) = Self::target(root, relative)?;
        let fresh = |dir: &Dir| -> Outcome<FinderEntry> {
            let mut file = beneath::create_new(dir, &name).map_err(|error| match error.kind() {
                ErrorKind::AlreadyExists => Failure::exists("file already exists"),
                _ => error.into(),
            })?;
            file.write_all(data)?;
            file.sync_all()?;
            FinderEntry::read(dir, &name)
        };
        if options.create == Create::New {
            return fresh(&dir);
        }
        let file = match beneath::open_file(&dir, &name, OpenOptions::new().write(true), true) {
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
        let mut entry = Self::versioned(&dir, &name)?;
        entry.in_place = matches!(saved, super::save::Saved::InPlace).then_some(true);
        Ok(entry)
    }

    pub fn mkdir_in(root: &FinderRoot, relative: &str, options: &MkdirOptions) -> Outcome<FinderEntry> {
        root.writable()?;
        let relative = Self::inner(relative)?;
        match options.recursive {
            true => Self::make_dirs(root, &relative)?,
            false => {
                let (dir, name) = Self::target(root, &relative)?;
                dir.create_dir(&name)?;
            }
        }
        Self::entry(root, &relative)
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
        let dir = Self::dir(root, &Self::inner(relative)?)?;
        let mut children = Vec::new();
        for entry in dir.entries()? {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else { continue };
            children.push((name, entry.file_type()?.into()));
        }
        Ok(children)
    }

    pub fn child_in(root: &FinderRoot, dir: &str, name: &str, create: Option<EntryKind>) -> Outcome<Option<EntryKind>> {
        let (parent, name) = Self::target(root, &Self::child_path(dir, name)?)?;
        match parent.symlink_metadata(&name) {
            Ok(info) => Ok(Some(info.file_type().into())),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                if matches!(create, Some(EntryKind::File | EntryKind::Directory)) {
                    root.writable()?;
                }
                let made = match create {
                    None | Some(EntryKind::Other | EntryKind::Symlink) => return Ok(None),
                    Some(EntryKind::File) => beneath::create_new(&parent, &name).map(drop),
                    Some(EntryKind::Directory) => parent.create_dir(&name),
                };
                match made {
                    Ok(()) => Ok(create),
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(Some(parent.symlink_metadata(&name)?.file_type().into())),
                    Err(error) => Err(error.into()),
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn remove_child_in(root: &FinderRoot, dir: &str, name: &str, recursive: bool) -> Outcome<bool> {
        root.writable()?;
        let (parent, name) = Self::target(root, &Self::child_path(dir, name)?)?;
        let info = match parent.symlink_metadata(&name) {
            Ok(info) => info,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        match (info.is_dir(), recursive) {
            (true, true) => parent.remove_dir_all(&name)?,
            (true, false) => parent.remove_dir(&name)?,
            (false, _) => parent.remove_file(&name)?,
        }
        Ok(true)
    }

    pub(super) fn make_dirs(root: &FinderRoot, relative: &str) -> Outcome<()> {
        let clean = Self::inner(relative)?;
        let top = root.open()?;
        let parts: Vec<&str> = clean.split('/').filter(|part| !part.is_empty()).collect();
        for (index, part) in parts.iter().enumerate() {
            let prefix = parts[..=index].join("/");
            let blocked = || match index + 1 == parts.len() {
                true => Failure::exists("a file with that name already exists"),
                false => Failure::invalid("not a directory"),
            };
            match beneath::open_dir(&top, &prefix) {
                Ok(_) => continue,
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) if error.kind() == ErrorKind::NotADirectory => return Err(blocked()),
                Err(error) => return Err(error.into()),
            }
            let parent = beneath::open_dir(&top, prefix.dirname())?;
            match parent.symlink_metadata(part) {
                Ok(info) if info.file_type().is_symlink() => match parent.metadata(part) {
                    Ok(_) => return Err(blocked()),
                    Err(_) => return Err(Failure::invalid("a symlink on the path points nowhere")),
                },
                Ok(_) => return Err(blocked()),
                Err(error) if error.kind() == ErrorKind::NotFound => match parent.create_dir(part) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::AlreadyExists && beneath::open_dir(&top, &prefix).is_ok() => {}
                    Err(error) => return Err(error.into()),
                },
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
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
        let pinned = Self::dir(root, &Self::inner(dir)?)?;
        for n in 1..10_000 {
            let candidate = Self::numbered(name, n);
            let mut file = match beneath::create_new(&pinned, &candidate) {
                Ok(file) => file,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            if let Err(error) = io::copy(&mut reader, &mut file).and_then(|_| file.sync_all()) {
                drop(file);
                _ = pinned.remove_file(&candidate);
                return Err(error.into());
            }
            return FinderEntry::read(&pinned, &candidate);
        }
        Err(Failure::exists("no free name for the copy"))
    }

    pub fn rename_in(root: &FinderRoot, from: &str, to: &str) -> Outcome<FinderEntry> {
        root.writable()?;
        let source = Self::inner(from)?;
        if source.is_empty() {
            return Err(Failure::invalid("the workspace root cannot be renamed"));
        }
        let from = Self::parent(root, &source)?;
        let to = Self::target(root, to)?;
        beneath::rename_no_replace((&from.0, &from.1), (&to.0, &to.1)).map_err(|error| match error.kind() {
            ErrorKind::AlreadyExists => Failure::exists("target already exists"),
            _ => error.into(),
        })?;
        FinderEntry::read(&to.0, &to.1)
    }

    pub fn remove_in(root: &FinderRoot, relative: &str) -> Outcome<()> {
        root.writable()?;
        if Self::inner(relative)?.is_empty() {
            return Err(Failure::invalid("the workspace root cannot be deleted"));
        }
        let (dir, name) = Self::target(root, relative)?;
        match dir.symlink_metadata(&name)?.is_dir() {
            true => dir.remove_dir(&name)?,
            false => dir.remove_file(&name)?,
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
