use super::FsError;
#[cfg(not(windows))]
use super::path::NodePath;
use super::path::same_folder;
use super::ranged::{OpenWrite, Opened, Range, RangeOptions, Writes};
use super::workspace::{Contents, EntryKind, FinderRoot, WorkspaceFs, WriteOptions};
use crate::runtime::failure::{Errno, Failure, Outcome};
use crate::runtime::sync::Locked;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct PickedFolders {
    lent: Option<super::Workspace>,
    roots: Arc<Mutex<Vec<FinderRoot>>>,
    files: Arc<Mutex<Vec<(String, bool)>>>,
    writes: Writes,
}

type Picked<T> = std::result::Result<T, FsError>;

const LENT: &str = "lent:";

impl PickedFolders {
    pub fn lent_from(workspace: Option<super::Workspace>) -> Self {
        Self { lent: workspace, ..Self::default() }
    }

    pub fn add(&self, path: &str) -> Picked<String> {
        let path = super::access::folder::existing(path, "picked")?;
        let mut roots = self.roots.locked();
        if let Some(known) = roots.iter().find(|root| same_folder(&root.path, &path)) {
            return Ok(known.path.clone());
        }
        roots.push(WorkspaceFs::folder(&path));
        Ok(path)
    }

    pub fn allow_file(&self, path: &str, writable: bool) {
        let mut files = self.files.locked();
        match files.iter_mut().find(|(known, _)| same_folder(known, path)) {
            Some(known) => known.1 = writable,
            None => files.push((path.to_owned(), writable)),
        }
    }

    fn locate(&self, path: &str) -> Outcome<(FinderRoot, String)> {
        if let Some(key) = path.strip_prefix(LENT) {
            let (name, relative) = key.split_once('/').unwrap_or((key, ""));
            let root = self.lent.as_ref().and_then(|workspace| workspace.lent_root(name));
            return root.map(|root| (root, relative.to_owned())).ok_or_else(|| Failure::http(403, "not a folder lent to this app"));
        }
        let inside = self
            .roots
            .locked()
            .iter()
            .filter_map(|root| within(path, &root.path).map(|relative| (root.clone(), relative)))
            .max_by_key(|(root, _)| root.path.len());
        if let Some(found) = inside {
            return Ok(found);
        }
        let files = self.files.locked();
        let picked = files.iter().find(|(known, _)| same_folder(known, path)).and_then(|(known, writable)| {
            let known = std::path::Path::new(known);
            let (parent, name) = (known.parent()?.to_str()?, known.file_name()?.to_str()?);
            Some((WorkspaceFs::folder(parent).access(*writable), name.to_owned()))
        });
        picked.ok_or_else(|| Failure::http(403, "not inside a folder the user picked"))
    }

    pub fn entries(&self, dir: &str) -> Picked<Vec<(String, EntryKind)>> {
        let (root, relative) = self.locate(dir)?;
        Ok(WorkspaceFs::children_in(&root, &relative)?)
    }

    pub fn child(&self, dir: &str, name: &str, create: Option<EntryKind>) -> Picked<Option<EntryKind>> {
        let (root, relative) = self.locate(dir)?;
        Ok(WorkspaceFs::child_in(&root, &relative, name, create)?)
    }

    pub fn remove(&self, dir: &str, name: &str, recursive: bool) -> Picked<bool> {
        let (root, relative) = self.locate(dir)?;
        Ok(WorkspaceFs::remove_child_in(&root, &relative, name, recursive)?)
    }

    pub fn read(&self, path: &str) -> Picked<Option<Contents>> {
        let (root, relative) = self.locate(path)?;
        match WorkspaceFs::read_bytes_in(&root, &relative) {
            Ok(contents) => Ok(Some(contents)),
            Err(failure) if failure.errno == Errno::Noent => Ok(None),
            Err(failure) => Err(failure.into()),
        }
    }

    pub fn write(&self, path: &str, data: &[u8]) -> Picked<()> {
        let (root, relative) = self.locate(path)?;
        WorkspaceFs::write_file_in(&root, &relative, data, &WriteOptions::default())?;
        Ok(())
    }

    pub fn read_range(&self, path: &str, offset: Option<u64>, length: Option<u64>, version: Option<String>) -> Picked<Option<Range>> {
        let (root, relative) = self.locate(path)?;
        match WorkspaceFs::read_range_in(&root, &relative, offset, length, &RangeOptions { if_version: version }) {
            Ok(range) => Ok(Some(range)),
            Err(failure) if failure.errno == Errno::Noent => Ok(None),
            Err(failure) => Err(failure.into()),
        }
    }

    pub fn open_write(&self, path: &str, keep_existing_data: bool) -> Picked<Opened> {
        let (root, relative) = self.locate(path)?;
        Ok(self.writes.open_in(&root, &relative, &OpenWrite { keep_existing_data, ..OpenWrite::default() })?)
    }

    pub fn write_chunk(&self, id: &str, offset: Option<u64>, data: &[u8]) -> Picked<()> {
        Ok(self.writes.chunk(id, offset, data)?)
    }

    pub fn truncate_write(&self, id: &str, size: Option<u64>) -> Picked<()> {
        Ok(self.writes.truncate(id, size)?)
    }

    pub fn commit_write(&self, id: &str) -> Picked<()> {
        self.writes.commit(id)?;
        Ok(())
    }

    pub fn abort_write(&self, id: &str) -> Picked<()> {
        Ok(self.writes.abort(id)?)
    }
}

#[cfg(windows)]
fn within(path: &str, root: &str) -> Option<String> {
    super::path::windows_within(std::path::Path::new(path), root)
}

#[cfg(not(windows))]
fn within(path: &str, root: &str) -> Option<String> {
    path.within(root).map(str::to_owned)
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn picked_folders_match_ignoring_case_and_separators() {
        let base = std::env::temp_dir().join(format!("acde-picked-case-{}", std::process::id()));
        let picked = base.join("Picked");
        std::fs::create_dir_all(picked.join("Sub")).unwrap();
        std::fs::create_dir_all(base.join("Picked2")).unwrap();
        std::fs::write(picked.join("Sub").join("Note.md"), b"n").unwrap();
        let folders = PickedFolders::default();
        let root = folders.add(picked.to_str().unwrap()).unwrap();
        let lower = root.to_lowercase();
        let upper_slashes = root.replace('\\', "/").to_uppercase();
        assert_eq!(folders.read(&format!("{lower}\\sub\\note.md")).unwrap().unwrap().data, b"n");
        assert_eq!(folders.read(&format!("{upper_slashes}/Sub/Note.md")).unwrap().unwrap().data, b"n");
        assert_eq!(folders.entries(&root.to_uppercase()).unwrap().len(), 1);
        assert_eq!(folders.entries(&format!("{lower}2")).err().map(|error| error.code), Some("EACCES"));
        assert_eq!(folders.add(&root.to_uppercase()).unwrap(), root);
        assert_eq!(folders.roots.locked().len(), 1);
        _ = std::fs::remove_dir_all(base);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch() -> String {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("acde-picked-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(dir.join("picked")).unwrap();
        dir.to_string_lossy().into_owned()
    }

    fn code<T>(result: Picked<T>) -> &'static str {
        result.err().map_or("ok", |error| error.code)
    }

    #[test]
    fn only_inside_a_picked_folder() {
        let base = scratch();
        let picked = base.join("picked");
        let folders = PickedFolders::default();
        assert_eq!(code(folders.entries(&picked)), "EACCES");
        assert_eq!(folders.add(&format!("{picked}/")).unwrap(), picked);
        assert!(folders.entries(&picked).unwrap().is_empty());
        assert_eq!(code(folders.entries(&base)), "EACCES");
        assert_eq!(code(folders.read(&format!("{picked}x/a"))), "EACCES");
        assert_eq!(code(folders.add("relative")), "EINVAL");
        _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn children_are_created_listed_read_and_removed() {
        let base = scratch();
        let picked = base.join("picked");
        let folders = PickedFolders::default();
        folders.add(&picked).unwrap();
        assert_eq!(folders.child(&picked, "a.txt", None).unwrap(), None);
        assert_eq!(folders.child(&picked, "a.txt", Some(EntryKind::File)).unwrap(), Some(EntryKind::File));
        assert_eq!(folders.child(&picked, "sub", Some(EntryKind::Directory)).unwrap(), Some(EntryKind::Directory));
        assert_eq!(folders.child(&picked, "sub", Some(EntryKind::File)).unwrap(), Some(EntryKind::Directory));
        let sub = picked.join("sub");
        assert_eq!(folders.child(&sub, "deep", Some(EntryKind::Directory)).unwrap(), Some(EntryKind::Directory));
        folders.write(&sub.join("deep/n.md"), b"# n").unwrap();
        let read = folders.read(&sub.join("deep/n.md")).unwrap().unwrap();
        assert_eq!(read.data, b"# n");
        assert!(folders.read(&sub.join("gone.md")).unwrap().is_none());
        let mut names = folders.entries(&picked).unwrap();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(names, [("a.txt".to_string(), EntryKind::File), ("sub".to_string(), EntryKind::Directory)]);
        assert_eq!(code(folders.remove(&picked, "sub", false)), "ENOTEMPTY");
        assert!(folders.remove(&picked, "sub", true).unwrap());
        assert!(!folders.remove(&picked, "sub", true).unwrap());
        assert_eq!(code(folders.child(&picked, "..", None)), "EINVAL");
        assert_eq!(code(folders.child(&picked, "a/b", None)), "EINVAL");
        _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_lent_key_reaches_the_folder_lent_to_this_app_only() {
        let base = scratch();
        let picked = base.join("picked");
        std::fs::write(Path::new(&picked).join("a.txt"), b"a").unwrap();
        let all = crate::runtime::fs::Workspace::at(&base);
        let root = crate::runtime::LentRoot::open("Picked".into(), false, &picked).unwrap();
        assert_eq!(all.lend("viewer", root).unwrap(), "work");
        let folders = PickedFolders::lent_from(Some(all.files(crate::runtime::Files::new(&[], false), "viewer")));
        assert_eq!(folders.read("lent:work/a.txt").unwrap().unwrap().data, b"a");
        assert_eq!(folders.entries("lent:work").unwrap(), [("a.txt".to_string(), EntryKind::File)]);
        assert_eq!(code(folders.write("lent:work/b.txt", b"b")), "EACCES");
        assert_eq!(code(folders.read("lent:work/../x")), "EINVAL");
        assert_eq!(code(folders.read("lent:other/a.txt")), "EACCES");
        let stranger = PickedFolders::lent_from(Some(all.files(crate::runtime::Files::new(&[], false), "stranger")));
        assert_eq!(code(stranger.read("lent:work/a.txt")), "EACCES");
        assert_eq!(code(PickedFolders::default().read("lent:work/a.txt")), "EACCES");
        _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn links_do_not_lead_out() {
        let base = scratch();
        let picked = base.join("picked");
        let folders = PickedFolders::default();
        folders.add(&picked).unwrap();
        symlink(base.join("nowhere"), picked.join("dangling")).unwrap();
        symlink(&base, picked.join("out")).unwrap();
        assert_eq!(folders.child(&picked, "dangling", Some(EntryKind::File)).unwrap(), Some(EntryKind::Symlink));
        assert!(!Path::new(&base.join("nowhere")).exists());
        assert_eq!(folders.child(&picked, "out", None).unwrap(), Some(EntryKind::Symlink));
        assert_eq!(code(folders.entries(&picked.join("out"))), "EINVAL");
        assert_eq!(code(folders.write(&picked.join("out/x"), b"x")), "EINVAL");
        assert!(!Path::new(&base.join("x")).exists());
        _ = std::fs::remove_dir_all(base);
    }
}
