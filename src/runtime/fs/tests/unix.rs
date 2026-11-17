use super::*;
use std::os::unix::fs::{MetadataExt, symlink};
use std::sync::atomic::{AtomicU32, Ordering};

struct Scratch {
    dir: String,
    root: FinderRoot,
}

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let base = std::env::temp_dir().join(format!("acde-ws-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let dir = base.to_string_lossy().into_owned();
        fs::create_dir_all(dir.join("root")).unwrap();
        let root = FinderRoot::new(dir.join("root"), "root", RootSource::Default);
        Self { dir, root }
    }

    fn at(&self, relative: &str) -> String {
        self.root.path.join(relative)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        _ = fs::remove_dir_all(&self.dir);
    }
}

fn code<T>(outcome: Outcome<T>) -> (u16, &'static str) {
    match outcome {
        Ok(_) => (0, "ok"),
        Err(failure) => (failure.status, failure.errno.as_str()),
    }
}

fn write(options: &str) -> WriteOptions {
    serde_json::from_str(options).unwrap()
}

#[test]
fn stat_reports_the_entry_and_its_clean_path() {
    let s = Scratch::new();
    fs::create_dir(s.at("notes")).unwrap();
    fs::write(s.at("notes/a.md"), "hello").unwrap();
    symlink("a.md", s.at("notes/link.md")).unwrap();
    let stat = WorkspaceFs::stat_in(&s.root, "notes/./a.md").unwrap();
    assert_eq!((stat.path.as_str(), stat.entry.name.as_str(), stat.entry.size), ("notes/a.md", "a.md", 5));
    assert!(stat.entry.kind == EntryKind::File);
    let link = WorkspaceFs::stat_in(&s.root, "notes/link.md").unwrap();
    assert!(link.entry.kind == EntryKind::Symlink && link.entry.target.as_deref() == Some("a.md"));
    let root = WorkspaceFs::stat_in(&s.root, "").unwrap();
    assert!(root.path.is_empty() && root.entry.kind == EntryKind::Directory && root.entry.name == "root");
    assert_eq!(code(WorkspaceFs::stat_in(&s.root, "nope")), (404, "ENOENT"));
    assert_eq!(code(WorkspaceFs::stat_in(&s.root, "../")), (400, "EINVAL"));
}

#[test]
fn reads_follow_the_root_rules() {
    let s = Scratch::new();
    fs::write(s.dir.join("secret"), "outside").unwrap();
    symlink(s.dir.join("secret"), s.at("out")).unwrap();
    fs::create_dir(s.at("dir")).unwrap();
    assert_eq!(code(WorkspaceFs::read_text_in(&s.root, "out")), (400, "EINVAL"));
    assert_eq!(code(WorkspaceFs::read_bytes_in(&s.root, "/etc/hosts")), (400, "EINVAL"));
    assert_eq!(code(WorkspaceFs::read_text_in(&s.root, "dir")), (400, "EINVAL"));
    assert_eq!(code(WorkspaceFs::read_text_in(&s.root, "missing")), (404, "ENOENT"));
}

#[test]
fn read_text_is_the_whole_file_beyond_the_preview() {
    let s = Scratch::new();
    let big = format!("\u{feff}{}", "é".repeat(200 * 1024));
    fs::write(s.at("big.txt"), &big).unwrap();
    let (text, modified) = WorkspaceFs::read_text_in(&s.root, "big.txt").unwrap();
    assert_eq!(text.len(), big.len());
    assert!(text == big && text.len() as u64 > MAX_PREVIEW_BYTES);
    assert_eq!(modified, fs::metadata(s.at("big.txt")).unwrap().modified_ms());
    fs::write(s.at("bad.txt"), [b'a', 0xff, b'b']).unwrap();
    assert_eq!(code(WorkspaceFs::read_text_in(&s.root, "bad.txt")), (400, "EILSEQ"));
    let bytes = WorkspaceFs::read_bytes_in(&s.root, "bad.txt").unwrap();
    assert_eq!(bytes.data, [b'a', 0xff, b'b']);
}

#[test]
fn reads_stop_at_the_raw_limit() {
    let s = Scratch::new();
    let file = File::create(s.at("huge.bin")).unwrap();
    file.set_len(MAX_RAW_BYTES + 1).unwrap();
    assert_eq!(code(WorkspaceFs::read_bytes_in(&s.root, "huge.bin")), (413, "EFBIG"));
    let data = vec![0u8; MAX_RAW_BYTES as usize + 1];
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "x.bin", &data, &WriteOptions::default())), (413, "EFBIG"));
    assert!(!Path::new(&s.at("x.bin")).exists());
}

#[test]
fn write_file_creates_overwrites_and_detects_conflicts() {
    let s = Scratch::new();
    let entry = WorkspaceFs::write_file_in(&s.root, "a.bin", &[0, 1, 2, 255], &WriteOptions::default()).unwrap();
    assert_eq!((entry.name.as_str(), entry.size), ("a.bin", 4));
    assert_eq!(fs::read(s.at("a.bin")).unwrap(), [0, 1, 2, 255]);
    let entry = WorkspaceFs::write_file_in(&s.root, "a.bin", b"xy", &WriteOptions::default()).unwrap();
    assert_eq!((entry.size, fs::read(s.at("a.bin")).unwrap()), (2, b"xy".to_vec()));
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "a.bin", b"z", &write(r#"{"create":"new"}"#))), (409, "EEXIST"));
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "b.bin", b"z", &write(r#"{"create":"never"}"#))), (404, "ENOENT"));
    assert!(!Path::new(&s.at("b.bin")).exists());
    let now = entry.modified_ms;
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "a.bin", b"z", &write(&format!(r#"{{"ifModifiedMs":{}}}"#, now - 1000)))), (409, "ECONFLICT"));
    assert_eq!(fs::read(s.at("a.bin")).unwrap(), b"xy");
    let ok = WorkspaceFs::write_file_in(&s.root, "a.bin", b"saved", &write(&format!(r#"{{"ifModifiedMs":{now},"create":"never"}}"#))).unwrap();
    assert_eq!(ok.size, 5);
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "gone.bin", b"z", &write(r#"{"ifModifiedMs":1}"#))), (409, "ECONFLICT"));
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "c.bin", b"z", &write(r#"{"ifModifiedMs":1,"create":"new"}"#))), (400, "EINVAL"));
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "", b"z", &WriteOptions::default())), (400, "EINVAL"));
}

#[test]
fn write_file_replaces_the_file_keeps_its_mode_and_refuses_links_and_folders() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new();
    fs::write(s.at("keep.txt"), "old").unwrap();
    fs::set_permissions(s.at("keep.txt"), fs::Permissions::from_mode(0o640)).unwrap();
    let inode = fs::metadata(s.at("keep.txt")).unwrap().ino();
    let saved = WorkspaceFs::write_file_in(&s.root, "keep.txt", b"new", &WriteOptions::default()).unwrap();
    assert_eq!(fs::read_to_string(s.at("keep.txt")).unwrap(), "new");
    assert_ne!(fs::metadata(s.at("keep.txt")).unwrap().ino(), inode, "replaced, not truncated");
    assert_eq!(fs::metadata(s.at("keep.txt")).unwrap().permissions().mode() & 0o777, 0o640);
    assert!(saved.in_place.is_none() && saved.version.is_some());
    assert_eq!(fs::read_dir(&s.root.path).unwrap().count(), 1, "no temporary file is left");
    fs::write(s.dir.join("secret"), "outside").unwrap();
    symlink(s.dir.join("secret"), s.at("link")).unwrap();
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "link", b"x", &WriteOptions::default())), (400, "EINVAL"));
    assert_eq!(fs::read_to_string(s.dir.join("secret")).unwrap(), "outside");
    fs::create_dir(s.at("folder")).unwrap();
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "folder", b"x", &WriteOptions::default())), (400, "EINVAL"));
    symlink(&s.dir, s.at("away")).unwrap();
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "away/x", b"x", &WriteOptions::default())), (400, "EINVAL"));
    assert!(!Path::new(&s.dir.join("x")).exists());
}

#[test]
fn write_file_mkdirs_creates_the_parents() {
    let s = Scratch::new();
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "a/b/c.md", b"x", &WriteOptions::default())), (400, "EINVAL"));
    WorkspaceFs::write_file_in(&s.root, "a/b/c.md", b"x", &write(r#"{"mkdirs":true}"#)).unwrap();
    assert_eq!(fs::read(s.at("a/b/c.md")).unwrap(), b"x");
    assert!(serde_json::from_str::<WriteOptions>(r#"{"create":"sometimes"}"#).is_err());
    assert!(serde_json::from_str::<WriteOptions>(r#"{"ifModifiedMs":"12"}"#).is_err());
    assert!(serde_json::from_str::<WriteOptions>(r#"{"other":1}"#).is_ok());
}

#[test]
fn mkdir_recursive() {
    let s = Scratch::new();
    let recursive = MkdirOptions { recursive: true };
    assert_eq!(code(WorkspaceFs::mkdir_in(&s.root, "x/y", &MkdirOptions::default())), (400, "EINVAL"));
    let entry = WorkspaceFs::mkdir_in(&s.root, "x/y/z", &recursive).unwrap();
    assert!(entry.name == "z" && entry.kind == EntryKind::Directory);
    assert!(Path::new(&s.at("x/y/z")).is_dir());
    assert_eq!(code(WorkspaceFs::mkdir_in(&s.root, "x/y/z", &recursive)), (0, "ok"));
    assert_eq!(code(WorkspaceFs::mkdir_in(&s.root, "x/y/z", &MkdirOptions::default())), (409, "EEXIST"));
    assert_eq!(code(WorkspaceFs::mkdir_in(&s.root, "", &recursive)), (0, "ok"));
    fs::write(s.at("x/file"), "").unwrap();
    assert_eq!(code(WorkspaceFs::mkdir_in(&s.root, "x/file", &recursive)), (409, "EEXIST"));
    assert_eq!(code(WorkspaceFs::mkdir_in(&s.root, "x/file/deeper", &recursive)), (400, "EINVAL"));
    symlink(s.at("x"), s.at("inside")).unwrap();
    WorkspaceFs::mkdir_in(&s.root, "inside/w", &recursive).unwrap();
    assert!(Path::new(&s.at("x/w")).is_dir());
    symlink(&s.dir, s.at("away")).unwrap();
    assert_eq!(code(WorkspaceFs::mkdir_in(&s.root, "away/w", &recursive)), (400, "EINVAL"));
    assert!(!Path::new(&s.dir.join("w")).exists());
    assert_eq!(code(WorkspaceFs::mkdir_in(&s.root, "../w", &recursive)), (400, "EINVAL"));
}

#[test]
fn copy_into_keeps_or_numbers_the_name() {
    let s = Scratch::new();
    let src = s.dir.join("src dir");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir(s.at("docs")).unwrap();
    fs::write(s.at("docs/plain.txt"), "x").unwrap();
    let image = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 1, 2, 255];
    fs::write(src.join("截图 1.png"), image).unwrap();
    fs::write(src.join(".env"), "A=1\n").unwrap();
    fs::write(src.join("README"), "readme\n").unwrap();
    let copy = |dir: &str, name: &str| {
        let file = File::open(src.join(name)).unwrap();
        WorkspaceFs::copy_into(&s.root, dir, name, file)
    };
    let entry = copy("docs", "截图 1.png").unwrap();
    assert_eq!((entry.name.as_str(), entry.kind, entry.size), ("截图 1.png", EntryKind::File, image.len() as u64));
    assert_eq!(fs::read(s.at("docs/截图 1.png")).unwrap(), image);
    assert_eq!(copy("docs", "截图 1.png").unwrap().name, "截图 1 2.png");
    assert_eq!(copy("docs", "截图 1.png").unwrap().name, "截图 1 3.png");
    for name in [".env", "README"] {
        assert_eq!(copy("", name).unwrap().name, name);
        assert_eq!(copy("", name).unwrap().name, format!("{name} 2"));
    }
    assert_eq!(fs::read_to_string(s.at("docs/plain.txt")).unwrap(), "x");
    File::create(src.join("big.bin")).unwrap().set_len(70 << 20).unwrap();
    assert_eq!(copy("", "big.bin").unwrap().size, 70 << 20);
    let failed = |dir: &str, name: &str| copy(dir, name).err().map(|f| (f.status, f.message));
    assert_eq!(failed("docs/plain.txt", "README"), Some((400, "not a directory".into())));
    assert_eq!(failed("../", "README"), Some((400, "path escapes the workspace".into())));
    assert_eq!(failed("nope", "README"), Some((404, "no such file or directory".into())));
    assert!(!Path::new(&s.at("nope")).exists());
}

#[test]
fn desktop_override_is_a_clean_absolute_folder_other_than_home() {
    let s = Scratch::new();
    let home = s.dir.join("home");
    let chosen = s.dir.join("chosen");
    let listed = s.dir.join("listed");
    let link = s.dir.join("home link");
    for dir in [&home, &chosen, &listed.join(".config")] {
        fs::create_dir_all(dir).unwrap();
    }
    symlink(&home, &link).unwrap();
    fs::write(listed.join(".config/user-dirs.dirs"), "XDG_DESKTOP_DIR=\"$HOME/\"\n").unwrap();
    let desktop = |home: &str, value: Option<&str>| {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "runtime::fs::workspace::tests::desktop_override_child", "--nocapture"]).env("HOME", home).env("ACDE_TEST_DESKTOP", "1");
        match value {
            Some(value) => child.env("XDG_DESKTOP_DIR", value),
            None => child.env_remove("XDG_DESKTOP_DIR"),
        };
        let output = child.output().unwrap();
        assert!(output.status.success(), "override {value:?}: {}", String::from_utf8_lossy(&output.stderr));
        let printed = String::from_utf8_lossy(&output.stdout);
        printed.lines().find_map(|line| line.strip_prefix("desktop=")).unwrap_or_else(|| panic!("override {value:?}: {printed}")).to_owned()
    };
    let fallback = home.join("Desktop");
    let cases = [
        (chosen.clone(), &chosen),
        (format!("{chosen}/"), &chosen),
        (format!("{chosen}//sub/../"), &chosen),
        ("$HOME/picked".into(), &home.join("picked")),
        (String::new(), &fallback),
        ("relative/desktop".into(), &fallback),
        ("$HOMEX/desktop".into(), &fallback),
        ("$HOME".into(), &fallback),
        ("$HOME/".into(), &fallback),
        ("$HOME/.".into(), &fallback),
        (home.clone(), &fallback),
        (format!("{home}/"), &fallback),
        (link, &fallback),
    ];
    for (value, expected) in cases {
        assert_eq!(&desktop(&home, Some(&value)), expected, "override {value:?}");
    }
    assert_eq!(desktop(&listed, None), listed.join("Desktop"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_folder_swapped_for_a_link_after_the_check_is_refused() {
    let s = Scratch::new();
    let outside = s.dir.join("outside");
    fs::create_dir_all(outside.join("b")).unwrap();
    fs::write(outside.join("b/file.txt"), "outside").unwrap();
    let setup = |s: &Scratch| {
        _ = fs::remove_file(s.at("a"));
        _ = fs::remove_dir_all(s.at("a"));
        _ = fs::remove_dir_all(s.at("a-old"));
        fs::create_dir_all(s.at("a/b")).unwrap();
        fs::write(s.at("a/b/file.txt"), "inside").unwrap();
        let (a, old, outside) = (s.at("a"), s.at("a-old"), outside.clone());
        beneath::hook::before_open(move || {
            fs::rename(&a, &old).unwrap();
            symlink(&outside, &a).unwrap();
        });
    };
    type Case<'a> = (&'a str, &'a dyn Fn(&Scratch) -> (u16, &'static str));
    let cases: [Case; 5] = [
        ("read", &|s| code(WorkspaceFs::read_text_in(&s.root, "a/b/file.txt"))),
        ("write", &|s| code(WorkspaceFs::write_file_in(&s.root, "a/b/file.txt", b"changed", &WriteOptions::default()))),
        ("mkdir", &|s| code(WorkspaceFs::mkdir_in(&s.root, "a/b/made", &MkdirOptions::default()))),
        ("rename", &|s| code(WorkspaceFs::rename_in(&s.root, "a/b/file.txt", "a/b/moved.txt"))),
        ("remove", &|s| code(WorkspaceFs::remove_in(&s.root, "a/b/file.txt"))),
    ];
    for (name, run) in cases {
        setup(&s);
        assert_eq!(run(&s), (400, "EINVAL"), "{name}");
    }
    assert_eq!(fs::read_to_string(outside.join("b/file.txt")).unwrap(), "outside");
    assert_eq!(fs::read_dir(outside.join("b")).unwrap().count(), 1);
}

#[test]
fn a_rename_never_replaces_a_name_that_appears_meanwhile() {
    let s = Scratch::new();
    fs::write(s.at("a.txt"), "a").unwrap();
    let late = s.at("b.txt");
    beneath::hook::before_open(move || fs::write(&late, "late").unwrap());
    assert_eq!(code(WorkspaceFs::rename_in(&s.root, "a.txt", "b.txt")), (409, "EEXIST"));
    assert_eq!(fs::read_to_string(s.at("b.txt")).unwrap(), "late");
    assert_eq!(fs::read_to_string(s.at("a.txt")).unwrap(), "a");
}

#[test]
fn desktop_override_child() {
    if std::env::var_os("ACDE_TEST_DESKTOP").is_some() {
        println!("desktop={}", WorkspaceFs::new().desktop().unwrap());
    }
}

#[test]
fn an_interrupted_save_leaves_the_original_and_no_temporary_file() {
    let s = Scratch::new();
    fs::write(s.at("doc.txt"), "original").unwrap();
    crate::runtime::fs::save::hook::interrupt_next_save();
    assert!(WorkspaceFs::write_file_in(&s.root, "doc.txt", b"half", &WriteOptions::default()).is_err());
    assert_eq!(fs::read_to_string(s.at("doc.txt")).unwrap(), "original");
    assert_eq!(fs::read_dir(&s.root.path).unwrap().count(), 1);
}

#[test]
fn a_stale_version_conflicts_even_within_one_millisecond() {
    let s = Scratch::new();
    fs::write(s.at("doc.txt"), "one").unwrap();
    let version = WorkspaceFs::read_bytes_in(&s.root, "doc.txt").unwrap().version;
    assert_eq!(WorkspaceFs::stat_in(&s.root, "doc.txt").unwrap().entry.version.as_ref(), Some(&version));
    let with = |v: &str| write(&format!(r#"{{"ifVersion":"{v}"}}"#));
    let saved = WorkspaceFs::write_file_in(&s.root, "doc.txt", b"two", &with(&version)).unwrap();
    assert_ne!(saved.version.as_ref(), Some(&version));
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "doc.txt", b"three", &with(&version))), (409, "ECONFLICT"));
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "doc.txt", b"four", &with(saved.version.as_ref().unwrap()))), (0, "ok"));
    assert_eq!(fs::read_to_string(s.at("doc.txt")).unwrap(), "four");
    assert_eq!(code(WorkspaceFs::write_file_in(&s.root, "new.txt", b"x", &write(r#"{"ifVersion":"1","create":"new"}"#))), (400, "EINVAL"));
}

#[test]
fn a_folder_that_takes_no_new_files_is_saved_in_place() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new();
    fs::create_dir(s.at("locked")).unwrap();
    fs::write(s.at("locked/doc.txt"), "old").unwrap();
    fs::set_permissions(s.at("locked"), fs::Permissions::from_mode(0o555)).unwrap();
    let saved = WorkspaceFs::write_file_in(&s.root, "locked/doc.txt", b"new", &WriteOptions::default());
    fs::set_permissions(s.at("locked"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(saved.unwrap().in_place, Some(true));
    assert_eq!(fs::read_to_string(s.at("locked/doc.txt")).unwrap(), "new");
}

#[test]
fn a_long_listing_keeps_folders_first_and_skips_names_it_cannot_show() {
    use std::os::unix::ffi::OsStrExt;
    let s = Scratch::new();
    for n in 0..MAX_ENTRIES + 10 {
        fs::write(s.at(&format!("a-{n:04}.txt")), "").unwrap();
    }
    fs::create_dir(s.at("zz folder")).unwrap();
    let bad = match fs::write(Path::new(&s.root.path).join(std::ffi::OsStr::from_bytes(b"bad\xff.txt")), "") {
        Ok(()) => 1,
        Err(e) if e.raw_os_error() == Some(rustix::io::Errno::ILSEQ.raw_os_error()) => 0,
        Err(e) => panic!("{e}"),
    };
    let listing = WorkspaceFs::new().list_in(s.root.clone(), "", false).unwrap();
    assert!(listing.truncated);
    assert_eq!(listing.entries.len(), MAX_ENTRIES);
    assert_eq!(listing.entries[0].name, "zz folder", "folders come before the cut");
    assert_eq!((listing.entries[1].name.as_str(), listing.skipped), ("a-0000.txt", bad));
    let children = WorkspaceFs::children_in(&s.root, "").unwrap();
    assert_eq!(children.len(), MAX_ENTRIES + 11, "the handle listing skips the same name");
    assert!(children.iter().all(|(name, _)| !name.contains('\u{fffd}')));
}
