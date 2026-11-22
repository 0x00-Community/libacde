use super::access::lent::LentFile;
use super::beneath::{self, Dir, OpenOptions};
use super::path::NodePath;
use super::save;
use super::workspace::{Create, FinderEntry, FinderRoot, ModifiedMs, WorkspaceFs};
use crate::runtime::failure::{Failure, Outcome};
use crate::runtime::sync::Locked;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

pub const MAX_CHUNK: u64 = 16 * 1024 * 1024;
pub const MAX_OPEN: usize = 16;
const IDLE: Duration = if cfg!(test) { Duration::from_secs(1) } else { Duration::from_secs(10 * 60) };
const RESERVE: u64 = 256 * 1024 * 1024;
const PART: &str = ".acde-part";

pub struct Range {
    pub data: Vec<u8>,
    pub offset: u64,
    pub size: u64,
    pub modified_ms: i64,
    pub version: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeOptions {
    pub if_version: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWrite {
    #[serde(default)]
    pub keep_existing_data: bool,
    #[serde(default)]
    pub create: Create,
    pub if_version: Option<String>,
    #[serde(default)]
    pub mkdirs: bool,
}

#[derive(Serialize)]
pub struct Opened {
    pub id: String,
    pub size: u64,
}

impl WorkspaceFs {
    pub fn read_range_in(root: &FinderRoot, relative: &str, offset: Option<u64>, length: Option<u64>, options: &RangeOptions) -> Outcome<Range> {
        let (Some(offset), Some(length)) = (offset, length) else {
            return Err(Failure::invalid("offset and length are non-negative integers"));
        };
        if length > MAX_CHUNK {
            return Err(Failure::http(413, "a range is at most 16 MiB"));
        }
        let (_, mut file, info) = Self::opened(root, relative)?;
        let version = save::version(&file)?;
        if options.if_version.as_ref().is_some_and(|expected| *expected != version) {
            return Err(Failure::conflict("file changed since it was opened; open it again"));
        }
        let size = info.len();
        let offset = offset.min(size);
        let take = length.min(size - offset);
        let mut data = Vec::with_capacity(take as usize);
        file.seek(SeekFrom::Start(offset))?;
        file.take(take).read_to_end(&mut data)?;
        Ok(Range { data, offset, size, modified_ms: info.modified_ms(), version })
    }
}

struct Beneath {
    dir: Dir,
    name: String,
    temp: String,
    create: Create,
    expected: Option<String>,
    original: Option<Metadata>,
}

enum Target {
    Beneath(Beneath),
    Lent { into: LentFile, scratch: PathBuf },
}

struct Session {
    target: Target,
    file: File,
    used: Instant,
}

impl Session {
    fn discard(self) {
        drop(self.file);
        match self.target {
            Target::Beneath(beneath) => _ = beneath.dir.remove_file(&beneath.temp),
            Target::Lent { scratch, .. } => _ = std::fs::remove_file(scratch),
        }
    }

    fn commit(self) -> Outcome<Option<FinderEntry>> {
        match self.target {
            Target::Beneath(beneath) => beneath.commit(self.file).map(Some),
            Target::Lent { into, scratch } => {
                let mut file = self.file;
                let poured = into.pour(&mut file);
                drop(file);
                _ = std::fs::remove_file(scratch);
                Ok(poured.map(|()| None)?)
            }
        }
    }
}

impl Beneath {
    fn commit(self, file: File) -> Outcome<FinderEntry> {
        let Beneath { dir, name, temp, create, expected, original } = self;
        let finished = original.as_ref().map_or(Ok(()), |info| save::keep(&file, info)).and_then(|()| file.sync_all());
        drop(file);
        let placed = finished.map_err(Failure::from).and_then(|()| place(&dir, &name, &temp, create, expected.as_deref()));
        if let Err(failure) = placed {
            _ = dir.remove_file(&temp);
            return Err(failure);
        }
        WorkspaceFs::versioned(&dir, &name)
    }
}

fn place(dir: &Dir, name: &str, temp: &str, create: Create, expected: Option<&str>) -> Outcome<()> {
    match beneath::read(dir, name, true) {
        Ok(current) => {
            if create == Create::New {
                return Err(Failure::exists("file already exists"));
            }
            if expected.is_some_and(|expected| save::version(&current).ok().as_deref() != Some(expected)) {
                return Err(Failure::conflict("file changed since the write was opened; reload before saving"));
            }
            drop(current);
            Ok(save::replace(dir, temp, name)?)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if expected.is_some() {
                return Err(Failure::conflict("file no longer exists"));
            }
            if create == Create::Never {
                return Err(error.into());
            }
            beneath::rename_no_replace((dir, temp), (dir, name)).map_err(|error| match error.kind() {
                ErrorKind::AlreadyExists => Failure::exists("file already exists"),
                _ => error.into(),
            })?;
            Ok(beneath::sync(dir)?)
        }
        Err(error) => Err(error.into()),
    }
}

#[derive(Default)]
struct Open {
    next: u64,
    watched: bool,
    sessions: HashMap<String, Session>,
}

impl Open {
    fn expire(&mut self) {
        let stale: Vec<String> = self.sessions.iter().filter(|(_, session)| session.used.elapsed() > IDLE).map(|(id, _)| id.clone()).collect();
        for id in stale {
            if let Some(session) = self.sessions.remove(&id) {
                session.discard();
            }
        }
    }

    fn session(&mut self, id: &str) -> Outcome<&mut Session> {
        self.expire();
        self.sessions.get_mut(id).ok_or_else(|| Failure::invalid(format!("write {id} is not open (committed, aborted or idle too long)")))
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        for (_, session) in self.sessions.drain() {
            session.discard();
        }
    }
}

#[derive(Clone, Default)]
pub struct Writes {
    open: Arc<Mutex<Open>>,
}

impl Writes {
    pub fn open_in(&self, root: &FinderRoot, relative: &str, options: &OpenWrite) -> Outcome<Opened> {
        root.writable()?;
        if options.create == Create::New && (options.if_version.is_some() || options.keep_existing_data) {
            return Err(Failure::invalid("ifVersion and keepExistingData cannot go with create: 'new'"));
        }
        let mut open = self.admitted()?;
        if options.mkdirs && options.create != Create::Never {
            WorkspaceFs::make_dirs(root, WorkspaceFs::clean(relative)?.dirname())?;
        }
        let (dir, name) = WorkspaceFs::target(root, relative)?;
        let original = match beneath::read(&dir, &name, true) {
            Ok(file) => {
                let info = file.metadata()?;
                if !info.is_file() {
                    return Err(Failure::invalid("not a regular file"));
                }
                if options.create == Create::New {
                    return Err(Failure::exists("file already exists"));
                }
                Some((file, info))
            }
            Err(error) if error.kind() == ErrorKind::NotFound && options.if_version.is_some() => {
                return Err(Failure::conflict("file no longer exists or is not a regular file"));
            }
            Err(error) if error.kind() == ErrorKind::NotFound && options.create != Create::Never => None,
            Err(error) => return Err(error.into()),
        };
        let expected = match (&original, &options.if_version) {
            (Some((file, _)), Some(expected)) => {
                if save::version(file)? != *expected {
                    return Err(Failure::conflict("file changed since it was opened; reload before saving"));
                }
                Some(expected.clone())
            }
            _ => None,
        };
        sweep(&dir);
        let temp = format!(".{name}.{}{PART}", uuid::Uuid::new_v4().simple());
        let mut file = beneath::open_file(&dir, &temp, OpenOptions::new().read(true).write(true).create_new(true), false)?;
        let copied = match (&original, options.keep_existing_data) {
            (Some((source, info)), true) => room(&dir, info.len()).and_then(|()| Ok(std::io::copy(&mut &*source, &mut file)?)),
            _ => Ok(0),
        };
        let size = match copied {
            Ok(size) => size,
            Err(failure) => {
                drop(file);
                _ = dir.remove_file(&temp);
                return Err(failure);
            }
        };
        let original = original.map(|(_, info)| info);
        let target = Target::Beneath(Beneath { dir, name, temp, create: options.create, expected, original });
        Ok(Opened { id: self.insert(&mut open, target, file), size })
    }

    pub fn open_lent(&self, into: &LentFile, keep_existing_data: bool) -> Outcome<Opened> {
        into.writable()?;
        let mut open = self.admitted()?;
        let scratch = std::env::temp_dir().join(format!(".acde-lent-{}{PART}", uuid::Uuid::new_v4().simple()));
        let mut file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&scratch)?;
        let copied = match keep_existing_data {
            true => (&*into.file).seek(SeekFrom::Start(0)).and_then(|_| std::io::copy(&mut &*into.file, &mut file)),
            false => Ok(0),
        };
        let size = match copied {
            Ok(size) => size,
            Err(error) => {
                drop(file);
                _ = std::fs::remove_file(&scratch);
                return Err(error.into());
            }
        };
        Ok(Opened { id: self.insert(&mut open, Target::Lent { into: into.clone(), scratch }, file), size })
    }

    fn admitted(&self) -> Outcome<std::sync::MutexGuard<'_, Open>> {
        let mut open = self.open.locked();
        open.expire();
        if open.sessions.len() >= MAX_OPEN {
            return Err(Failure::invalid(format!("at most {MAX_OPEN} writes may be open at once; commit or abort one first")));
        }
        Ok(open)
    }

    fn insert(&self, open: &mut Open, target: Target, file: File) -> String {
        open.next += 1;
        let id = format!("write-{}", open.next);
        open.sessions.insert(id.clone(), Session { target, file, used: Instant::now() });
        if !open.watched {
            open.watched = true;
            watch(&self.open);
        }
        id
    }

    pub fn chunk(&self, id: &str, offset: Option<u64>, data: &[u8]) -> Outcome<()> {
        let Some(offset) = offset else { return Err(Failure::invalid("offset is a non-negative integer")) };
        if data.len() as u64 > MAX_CHUNK {
            return Err(Failure::http(413, "a chunk is at most 16 MiB"));
        }
        let end = offset.checked_add(data.len() as u64).ok_or_else(|| Failure::invalid("offset is too large"))?;
        let mut open = self.open.locked();
        let session = open.session(id)?;
        grow(session, end)?;
        session.file.seek(SeekFrom::Start(offset))?;
        session.file.write_all(data)?;
        session.used = Instant::now();
        Ok(())
    }

    pub fn truncate(&self, id: &str, size: Option<u64>) -> Outcome<()> {
        let Some(size) = size else { return Err(Failure::invalid("size is a non-negative integer")) };
        let mut open = self.open.locked();
        let session = open.session(id)?;
        grow(session, size)?;
        session.file.set_len(size)?;
        session.used = Instant::now();
        Ok(())
    }

    pub fn commit(&self, id: &str) -> Outcome<Option<FinderEntry>> {
        let session = self.take(id)?;
        session.commit()
    }

    pub fn abort(&self, id: &str) -> Outcome<()> {
        self.take(id)?.discard();
        Ok(())
    }

    fn take(&self, id: &str) -> Outcome<Session> {
        let mut open = self.open.locked();
        open.session(id)?;
        open.sessions.remove(id).ok_or_else(|| Failure::invalid(format!("write {id} is not open")))
    }

    #[cfg(test)]
    pub(crate) fn open_count(&self) -> usize {
        self.open.locked().sessions.len()
    }
}

fn grow(session: &Session, end: u64) -> Outcome<()> {
    let size = session.file.metadata()?.len();
    if end > size {
        if let Target::Beneath(beneath) = &session.target {
            room(&beneath.dir, end - size)?;
        }
    }
    Ok(())
}

fn room(dir: &Dir, more: u64) -> Outcome<()> {
    match free(dir) {
        Some(free) if free < more.saturating_add(RESERVE) => Err(Failure::http(507, "not enough disk space for this write")),
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn free(dir: &Dir) -> Option<u64> {
    let info = rustix::fs::fstatvfs(dir).ok()?;
    Some(info.f_bavail.saturating_mul(info.f_frsize))
}

#[cfg(windows)]
fn free(dir: &Dir) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = beneath::final_path(dir).ok()?.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0u64;
    match unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) } {
        0 => None,
        _ => Some(available),
    }
}

fn sweep(dir: &Dir) {
    let Ok(entries) = dir.entries() else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with('.') || !name.ends_with(PART) {
            continue;
        }
        let age = entry.metadata().ok().and_then(|info| info.modified().ok()).and_then(|time| time.into_std().elapsed().ok());
        if age.is_some_and(|age| age > IDLE * 2) {
            _ = dir.remove_file(name);
        }
    }
}

fn watch(open: &Arc<Mutex<Open>>) {
    static WATCHED: OnceLock<Arc<Mutex<Vec<Weak<Mutex<Open>>>>>> = OnceLock::new();
    let mut first = false;
    let watched = WATCHED.get_or_init(|| {
        first = true;
        Arc::default()
    });
    watched.locked().push(Arc::downgrade(open));
    if !first {
        return;
    }
    let watched = Arc::clone(watched);
    let spawned = std::thread::Builder::new().name("acde-fs-writes".into()).spawn(move || {
        loop {
            std::thread::sleep(IDLE.min(Duration::from_secs(30)));
            let alive: Vec<Arc<Mutex<Open>>> = {
                let mut list = watched.locked();
                list.retain(|open| open.strong_count() > 0);
                list.iter().filter_map(Weak::upgrade).collect()
            };
            for open in alive {
                open.locked().expire();
            }
        }
    });
    if let Err(error) = spawned {
        log::error!("acde-fs-writes: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch() -> (std::path::PathBuf, FinderRoot) {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("acde-ranged-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        let root = WorkspaceFs::folder(dir.to_str().unwrap());
        (dir, root)
    }

    fn parts(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(PART)).count()
    }

    fn code<T>(result: Outcome<T>) -> &'static str {
        result.err().map_or("ok", |failure| failure.errno.as_str())
    }

    #[test]
    fn ranges_read_part_of_a_file_and_notice_changes() {
        let (dir, root) = scratch();
        std::fs::write(dir.join("a.bin"), b"0123456789").unwrap();
        let none = RangeOptions::default();
        let range = WorkspaceFs::read_range_in(&root, "a.bin", Some(3), Some(4), &none).unwrap();
        assert_eq!((range.data.as_slice(), range.offset, range.size), (&b"3456"[..], 3, 10));
        let tail = WorkspaceFs::read_range_in(&root, "a.bin", Some(8), Some(100), &none).unwrap();
        assert_eq!(tail.data, b"89");
        assert!(WorkspaceFs::read_range_in(&root, "a.bin", Some(20), Some(4), &none).unwrap().data.is_empty());
        assert_eq!(code(WorkspaceFs::read_range_in(&root, "a.bin", Some(0), Some(MAX_CHUNK + 1), &none)), "EFBIG");
        assert_eq!(code(WorkspaceFs::read_range_in(&root, "a.bin", None, Some(1), &none)), "EINVAL");
        let same = RangeOptions { if_version: Some(range.version.clone()) };
        assert_eq!(WorkspaceFs::read_range_in(&root, "a.bin", Some(0), Some(1), &same).unwrap().data, b"0");
        std::fs::write(dir.join("a.bin"), b"changed!!!!").unwrap();
        assert_eq!(code(WorkspaceFs::read_range_in(&root, "a.bin", Some(0), Some(1), &same)), "ECONFLICT");
        _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn chunks_land_only_on_commit() {
        let (dir, root) = scratch();
        std::fs::write(dir.join("a.txt"), b"old contents").unwrap();
        let writes = Writes::default();
        let opened = writes.open_in(&root, "a.txt", &OpenWrite::default()).unwrap();
        assert_eq!(opened.size, 0);
        writes.chunk(&opened.id, Some(4), b"tail").unwrap();
        writes.chunk(&opened.id, Some(0), b"he").unwrap();
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"old contents");
        assert_eq!(parts(&dir), 1);
        let entry = writes.commit(&opened.id).unwrap().unwrap();
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"he\0\0tail");
        assert!(entry.version.is_some());
        assert_eq!(parts(&dir), 0);
        assert_eq!(code(writes.commit(&opened.id)), "EINVAL");
        _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn existing_data_is_kept_truncated_and_extended() {
        let (dir, root) = scratch();
        std::fs::write(dir.join("a.txt"), b"0123456789").unwrap();
        let writes = Writes::default();
        let keep = OpenWrite { keep_existing_data: true, ..OpenWrite::default() };
        let opened = writes.open_in(&root, "a.txt", &keep).unwrap();
        assert_eq!(opened.size, 10);
        writes.truncate(&opened.id, Some(4)).unwrap();
        writes.chunk(&opened.id, Some(6), b"x").unwrap();
        writes.commit(&opened.id).unwrap();
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"0123\0\0x");
        _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn aborted_dropped_and_idle_writes_leave_nothing() {
        let (dir, root) = scratch();
        let writes = Writes::default();
        let opened = writes.open_in(&root, "new.txt", &OpenWrite::default()).unwrap();
        writes.chunk(&opened.id, Some(0), b"x").unwrap();
        writes.abort(&opened.id).unwrap();
        assert!(!dir.join("new.txt").exists());
        assert_eq!(parts(&dir), 0);
        writes.open_in(&root, "new.txt", &OpenWrite::default()).unwrap();
        assert_eq!(parts(&dir), 1);
        drop(writes);
        assert_eq!(parts(&dir), 0);
        let writes = Writes::default();
        let opened = writes.open_in(&root, "new.txt", &OpenWrite::default()).unwrap();
        std::thread::sleep(IDLE + Duration::from_millis(100));
        assert_eq!(code(writes.chunk(&opened.id, Some(0), b"x")), "EINVAL");
        assert_eq!(parts(&dir), 0);
        _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn idle_writes_are_given_up_without_further_calls() {
        let (dir, root) = scratch();
        let writes = Writes::default();
        writes.open_in(&root, "idle.txt", &OpenWrite::default()).unwrap();
        std::thread::sleep(IDLE * 3);
        assert_eq!(parts(&dir), 0);
        assert_eq!(writes.open_count(), 0);
        _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn writes_are_limited_and_checked() {
        let (dir, root) = scratch();
        std::fs::write(dir.join("a.txt"), b"a").unwrap();
        let writes = Writes::default();
        let opened: Vec<Opened> = (0..MAX_OPEN).map(|_| writes.open_in(&root, "a.txt", &OpenWrite::default()).unwrap()).collect();
        assert_eq!(code(writes.open_in(&root, "a.txt", &OpenWrite::default())), "EINVAL");
        writes.abort(&opened[0].id).unwrap();
        let version = save::version(&File::open(dir.join("a.txt")).unwrap()).unwrap();
        let guarded = OpenWrite { if_version: Some(version), ..OpenWrite::default() };
        let id = writes.open_in(&root, "a.txt", &guarded).unwrap().id;
        std::fs::write(dir.join("a.txt"), b"someone else").unwrap();
        writes.chunk(&id, Some(0), b"mine").unwrap();
        assert_eq!(code(writes.commit(&id)), "ECONFLICT");
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"someone else");
        let stale = OpenWrite { if_version: Some("stale".into()), ..OpenWrite::default() };
        assert_eq!(code(writes.open_in(&root, "a.txt", &stale)), "ECONFLICT");
        let fresh = OpenWrite { create: Create::New, ..OpenWrite::default() };
        assert_eq!(code(writes.open_in(&root, "a.txt", &fresh)), "EEXIST");
        let never = OpenWrite { create: Create::Never, ..OpenWrite::default() };
        assert_eq!(code(writes.open_in(&root, "missing.txt", &never)), "ENOENT");
        assert_eq!(code(writes.chunk(&opened[1].id, Some(0), &vec![0; MAX_CHUNK as usize + 1])), "EFBIG");
        assert_eq!(code(writes.open_in(&root, "../out.txt", &OpenWrite::default())), "EINVAL");
        let read_only = WorkspaceFs::folder(dir.to_str().unwrap()).access(false);
        assert_eq!(code(writes.open_in(&read_only, "a.txt", &OpenWrite::default())), "EACCES");
        drop(writes);
        assert_eq!(parts(&dir), 0);
        _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_picked_file_alone_is_reachable() {
        let (dir, _) = scratch();
        std::fs::write(dir.join("one.bin"), b"one").unwrap();
        std::fs::write(dir.join("other.bin"), b"other").unwrap();
        let folders = super::super::PickedFolders::default();
        let one = dir.join("one.bin").to_string_lossy().into_owned();
        let other = dir.join("other.bin").to_string_lossy().into_owned();
        assert_eq!(folders.read_range(&one, Some(0), Some(3), None).err().map(|e| e.code), Some("EACCES"));
        folders.allow_file(&one, false);
        assert_eq!(folders.read_range(&one, Some(1), Some(5), None).unwrap().unwrap().data, b"ne");
        assert_eq!(folders.read_range(&other, Some(0), Some(3), None).err().map(|e| e.code), Some("EACCES"));
        assert_eq!(folders.open_write(&one, false).err().map(|e| e.code), Some("EACCES"));
        folders.allow_file(&one, true);
        let opened = folders.open_write(&one, true).unwrap();
        folders.write_chunk(&opened.id, Some(3), b"!").unwrap();
        folders.commit_write(&opened.id).unwrap();
        assert_eq!(std::fs::read(dir.join("one.bin")).unwrap(), b"one!");
        _ = std::fs::remove_dir_all(dir);
    }
}
