use super::*;
use std::{path::PathBuf, process::Command};

struct Scratch {
    path: PathBuf,
    root: FinderRoot,
}

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("acde-ws-{}", uuid::Uuid::new_v4()));
        let root = path.join("root");
        fs::create_dir_all(&root).unwrap();
        Self { path, root: WorkspaceFs::granted(root.to_str().unwrap()) }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir(Path::new(&self.root.path).join("out"));
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn invalid<T>(result: Outcome<T>) {
    assert!(matches!(result, Err(Failure { errno: Errno::Inval, .. })));
}

#[test]
fn native_descendants_can_be_listed_read_and_written() {
    let s = Scratch::new();
    let root = Path::new(&s.root.path);
    fs::create_dir(root.join("notes")).unwrap();
    fs::write(root.join("notes/source.txt"), "test").unwrap();
    assert_eq!(WorkspaceFs::read_text_in(&s.root, "notes/./source.txt").unwrap().0, "test");
    let listing = WorkspaceFs::new().list_in(s.root.clone(), "notes", false).unwrap();
    assert_eq!(listing.entries[0].name, "source.txt");
    WorkspaceFs::write_file_in(&s.root, "notes/new.txt", b"created", &WriteOptions::default()).unwrap();
    assert_eq!(fs::read(root.join("notes/new.txt")).unwrap(), b"created");
}

#[test]
fn relative_names_cannot_address_windows_namespaces_or_escape() {
    let s = Scratch::new();
    let rooted_or_traversal = ["../secret", "..\\secret", "C:note.txt", "C:/note.txt", "\\note.txt", "dir/.. /file"];
    let special_names = ["file.txt:stream", "NUL.txt", "COM¹.txt"];
    for path in rooted_or_traversal.into_iter().chain(special_names) {
        invalid(WorkspaceFs::write_file_in(&s.root, path, b"test", &WriteOptions::default()));
    }
    assert_eq!(fs::read_dir(&s.root.path).unwrap().count(), 0);
}

#[test]
fn junction_cannot_escape_on_read_or_recursive_create() {
    let s = Scratch::new();
    let outside = s.path.join("root-sibling");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "outside").unwrap();
    let create_junction = "New-Item -ItemType Junction -Path $env:ACDE_TEST_JUNCTION \
        -Target $env:ACDE_TEST_TARGET -ErrorAction Stop | Out-Null";
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(create_junction)
        .env("ACDE_TEST_JUNCTION", Path::new(&s.root.path).join("out"))
        .env("ACDE_TEST_TARGET", &outside)
        .status()
        .unwrap();
    assert!(status.success());
    invalid(WorkspaceFs::read_text_in(&s.root, "out/secret.txt"));
    invalid(WorkspaceFs::write_file_in(&s.root, "out/secret.txt", b"changed", &WriteOptions::default()));
    invalid(WorkspaceFs::mkdir_in(&s.root, "out/escaped", &MkdirOptions { recursive: true }));
    assert!(!outside.join("escaped").exists());
    assert_eq!(fs::read(outside.join("secret.txt")).unwrap(), b"outside");
}

struct Denied(Vec<PathBuf>);

impl Denied {
    fn add(&mut self, path: PathBuf, rights: &str) {
        let status = Command::new("icacls").arg(&path).arg("/deny").arg(format!("*S-1-1-0:{rights}")).arg("/Q").status().unwrap();
        assert!(status.success(), "icacls /deny {rights} on {}", path.display());
        self.0.push(path);
    }
}

impl Drop for Denied {
    fn drop(&mut self) {
        for path in &self.0 {
            _ = Command::new("icacls").arg(path).args(["/remove:d", "*S-1-1-0", "/Q"]).status();
        }
    }
}

fn answer<T>(outcome: Outcome<T>) -> (u16, &'static str) {
    match outcome {
        Ok(_) => (0, "ok"),
        Err(failure) => (failure.status, failure.errno.as_str()),
    }
}

fn acl_bypass() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, LookupPrivilegeNameW, SE_PRIVILEGE_ENABLED, TOKEN_PRIVILEGES, TOKEN_QUERY, TokenPrivileges};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let mut size = 0u32;
    unsafe { GetTokenInformation(token, TokenPrivileges, std::ptr::null_mut(), 0, &mut size) };
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    let read = unsafe { GetTokenInformation(token, TokenPrivileges, buffer.as_mut_ptr().cast(), size, &mut size) };
    unsafe { CloseHandle(token) };
    if read == 0 {
        return None;
    }
    let privileges = unsafe { &*buffer.as_ptr().cast::<TOKEN_PRIVILEGES>() };
    let all = unsafe { std::slice::from_raw_parts(privileges.Privileges.as_ptr(), privileges.PrivilegeCount as usize) };
    let enabled: Vec<String> = all
        .iter()
        .filter(|p| p.Attributes & SE_PRIVILEGE_ENABLED != 0)
        .filter_map(|p| {
            let mut name = [0u16; 64];
            let mut length = name.len() as u32;
            let found = unsafe { LookupPrivilegeNameW(std::ptr::null(), &p.Luid, name.as_mut_ptr(), &mut length) };
            (found != 0).then(|| String::from_utf16_lossy(&name[..length as usize]))
        })
        .filter(|name| name == "SeBackupPrivilege" || name == "SeRestorePrivilege")
        .collect();
    (!enabled.is_empty()).then(|| enabled.join(", "))
}

#[test]
fn writes_the_system_denies_answer_403() {
    if let Some(privileges) = acl_bypass() {
        eprintln!("skipped: this process has {privileges} enabled, which bypasses deny ACEs");
        return;
    }
    let s = Scratch::new();
    let root = Path::new(&s.root.path);
    for dir in ["no-add", "sealed", "kept"] {
        fs::create_dir(root.join(dir)).unwrap();
    }
    fs::write(root.join("a.txt"), "a").unwrap();
    fs::write(root.join("kept/keep.txt"), "keep").unwrap();
    let mut denied = Denied(Vec::new());
    denied.add(root.join("no-add"), "(WD,AD)");
    denied.add(root.join("sealed"), "(W)");
    denied.add(root.join("kept"), "(DC)");
    denied.add(root.join("kept/keep.txt"), "(DE)");
    let recursive = MkdirOptions { recursive: true };
    for dir in ["no-add", "sealed"] {
        let at = |name: &str| format!("{dir}/{name}");
        assert_eq!(answer(WorkspaceFs::write_file_in(&s.root, &at("new.txt"), b"x", &WriteOptions::default())), (403, "EACCES"), "{dir}");
        assert_eq!(answer(WorkspaceFs::mkdir_in(&s.root, &at("sub"), &MkdirOptions::default())), (403, "EACCES"), "{dir}");
        assert_eq!(answer(WorkspaceFs::mkdir_in(&s.root, &at("sub/deeper"), &recursive)), (403, "EACCES"), "{dir}");
        assert_eq!(answer(WorkspaceFs::child_in(&s.root, dir, "made", Some(EntryKind::File))), (403, "EACCES"), "{dir}");
        assert_eq!(answer(WorkspaceFs::rename_in(&s.root, "a.txt", &at("a.txt"))), (403, "EACCES"), "{dir}");
        assert_eq!(answer(crate::runtime::fs::copy::copy(&s.root, "a.txt", &s.root, dir)), (403, "EACCES"), "{dir}");
    }
    assert_eq!(answer(WorkspaceFs::remove_in(&s.root, "kept/keep.txt")), (403, "EACCES"));
    assert_eq!(answer(WorkspaceFs::remove_child_in(&s.root, "kept", "keep.txt", false)), (403, "EACCES"));
    assert_eq!(answer(WorkspaceFs::new().list_in(s.root.clone(), "sealed", true)), (403, "EACCES"));
    drop(denied);
    assert_eq!(fs::read(root.join("kept/keep.txt")).unwrap(), b"keep");
    assert_eq!(fs::read(root.join("a.txt")).unwrap(), b"a");
    for dir in ["no-add", "sealed"] {
        assert_eq!(fs::read_dir(root.join(dir)).unwrap().count(), 0, "{dir}");
    }
}

#[test]
fn a_missing_parent_still_answers_einval() {
    let s = Scratch::new();
    assert_eq!(answer(WorkspaceFs::write_file_in(&s.root, "missing/new.txt", b"x", &WriteOptions::default())), (400, "EINVAL"));
    assert_eq!(answer(WorkspaceFs::mkdir_in(&s.root, "missing/sub", &MkdirOptions::default())), (400, "EINVAL"));
    assert_eq!(answer(WorkspaceFs::remove_in(&s.root, "missing.txt")), (404, "ENOENT"));
}

fn desktop_child(home: &Path, known: &Path, desktop: Option<&str>, expected: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "runtime::fs::workspace::windows_tests::desktop_override_child"])
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_STATE_HOME", home)
        .env("ACDE_TEST_KNOWN_DESKTOP", known)
        .env_remove("XDG_DOWNLOAD_DIR")
        .env("ACDE_TEST_DESKTOP", expected);
    match desktop {
        Some(input) => child.env("XDG_DESKTOP_DIR", input),
        None => child.env_remove("XDG_DESKTOP_DIR"),
    };
    let output = child.output().unwrap();
    assert!(output.status.success(), "override {desktop:?}: {}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn windows_desktop_override_uses_native_paths_and_safe_fallback() {
    let s = Scratch::new();
    let home = s.path.join("profile");
    let fallback = home.join("Desktop");
    let selected = home.join("override");
    let known = s.path.join("known");
    for path in [&fallback, &selected, &known] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("fixture.txt"), "isolated desktop").unwrap();
    }
    let selected = selected.to_str().unwrap();
    let forward_slash = selected.replace('\\', "/");
    let fallback = fallback.to_str().unwrap();
    let paths = [
        (selected, selected),
        (&forward_slash, selected),
        ("$HOME/override", selected),
        (r"$HOME\override", selected),
        ("", known.to_str().unwrap()),
        ("D:relative", fallback),
        (r"\root-relative", fallback),
        ("/", fallback),
        (r"\\server", fallback),
        (r"\\.\pipe\anything", fallback),
        (r"\\?\C:\", fallback),
        ("$HOME", fallback),
    ];
    for (input, expected) in paths {
        desktop_child(&home, &known, Some(input), expected);
    }
}

#[test]
fn level_0_desktop_uses_the_known_folder_without_creating_the_profile_one() {
    let s = Scratch::new();
    let home = s.path.join("profile");
    let known = s.path.join("OneDrive").join("Desktop");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&known).unwrap();
    fs::write(known.join("fixture.txt"), "isolated desktop").unwrap();
    desktop_child(&home, &known, None, known.to_str().unwrap());
    assert!(!home.join("Desktop").exists());
}

#[test]
fn desktop_override_child() {
    use crate::runtime::fs::{FsCall, FsReply, Workspace};

    let Some(expected) = std::env::var_os("ACDE_TEST_DESKTOP") else { return };
    let workspace = Workspace::new();
    let FsReply::Json(roots) = workspace.call(FsCall::Roots).unwrap() else { panic!("expected roots") };
    let root = roots[0]["path"].as_str().unwrap();
    assert_eq!(Path::new(root), Path::new(&expected));
    let read = FsCall::ReadText { root: root.to_owned(), path: "fixture.txt".into() };
    let FsReply::Text { text, .. } = workspace.call(read).unwrap() else { panic!("expected desktop file") };
    assert_eq!(text, "isolated desktop");
}

#[test]
fn windows_desktop_override_distinguishes_disk_unc_and_device_paths() {
    use super::super::path::windows_directory;

    let accepted = [r"D:\", r"D:\desktop", "D:/desktop", r"\\server\share", "//server/share/desktop"];
    for input in accepted {
        assert_eq!(windows_directory(input).as_deref().map(Path::new), Some(Path::new(input)));
    }
    let rejected = [r"\\server", r"\\server\IPC$", r"\\server\pipe\desk", r"\\?\UNC\server\share"];
    for input in rejected {
        assert!(windows_directory(input).is_none(), "accepted {input:?}");
    }
}

fn junction(at: &Path, target: &Path) {
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "New-Item -ItemType Junction -Path $env:ACDE_TEST_JUNCTION -Target $env:ACDE_TEST_TARGET -ErrorAction Stop | Out-Null",
        ])
        .env("ACDE_TEST_JUNCTION", at)
        .env("ACDE_TEST_TARGET", target)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn a_folder_swapped_for_a_junction_after_the_check_is_refused() {
    let s = Scratch::new();
    let root = PathBuf::from(&s.root.path);
    let outside = s.path.join("outside");
    fs::create_dir_all(outside.join("b")).unwrap();
    fs::write(outside.join("b/file.txt"), "outside").unwrap();
    let setup = |n: usize| {
        let (a, old) = (root.join("a"), root.join(format!("a-old-{n}")));
        if a.exists() {
            fs::remove_dir(&a).unwrap();
        }
        fs::create_dir_all(a.join("b")).unwrap();
        fs::write(a.join("b/file.txt"), "inside").unwrap();
        let outside = outside.clone();
        beneath::hook::before_open(move || {
            fs::rename(&a, &old).unwrap();
            junction(&a, &outside);
        });
    };
    let cases: [(&str, &dyn Fn() -> Outcome<()>); 5] = [
        ("read", &|| WorkspaceFs::read_text_in(&s.root, "a/b/file.txt").map(drop)),
        ("write", &|| WorkspaceFs::write_file_in(&s.root, "a/b/file.txt", b"changed", &WriteOptions::default()).map(drop)),
        ("mkdir", &|| WorkspaceFs::mkdir_in(&s.root, "a/b/made", &MkdirOptions::default()).map(drop)),
        ("rename", &|| WorkspaceFs::rename_in(&s.root, "a/b/file.txt", "a/b/moved.txt").map(drop)),
        ("remove", &|| WorkspaceFs::remove_in(&s.root, "a/b/file.txt")),
    ];
    for (n, (name, run)) in cases.into_iter().enumerate() {
        setup(n);
        assert_eq!(answer(run()), (400, "EINVAL"), "{name}");
    }
    let _ = fs::remove_dir(root.join("a"));
    assert_eq!(fs::read_to_string(outside.join("b/file.txt")).unwrap(), "outside");
    assert_eq!(fs::read_dir(outside.join("b")).unwrap().count(), 1);
}

#[test]
fn a_rename_never_replaces_a_name_that_appears_meanwhile() {
    let s = Scratch::new();
    let root = PathBuf::from(&s.root.path);
    fs::write(root.join("a.txt"), "a").unwrap();
    let late = root.join("b.txt");
    beneath::hook::before_open(move || fs::write(&late, "late").unwrap());
    assert_eq!(answer(WorkspaceFs::rename_in(&s.root, "a.txt", "b.txt")), (409, "EEXIST"));
    assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "late");
}

#[test]
fn a_save_replaces_the_file_keeps_its_attributes_and_survives_an_interruption() {
    use std::os::windows::fs::MetadataExt;
    let s = Scratch::new();
    let root = PathBuf::from(&s.root.path);
    fs::write(root.join("doc.txt"), "original").unwrap();
    assert!(Command::new("attrib").arg("+H").arg(root.join("doc.txt")).status().unwrap().success());
    crate::runtime::fs::save::hook::interrupt_next_save();
    assert!(WorkspaceFs::write_file_in(&s.root, "doc.txt", b"half", &WriteOptions::default()).is_err());
    assert_eq!(fs::read_to_string(root.join("doc.txt")).unwrap(), "original");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1, "no temporary file is left");
    let saved = WorkspaceFs::write_file_in(&s.root, "doc.txt", b"new", &WriteOptions::default()).unwrap();
    assert_eq!(fs::read_to_string(root.join("doc.txt")).unwrap(), "new");
    assert!(saved.in_place.is_none());
    assert_ne!(fs::metadata(root.join("doc.txt")).unwrap().file_attributes() & 0x2, 0, "still hidden");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn a_stale_version_conflicts_even_within_one_millisecond() {
    let s = Scratch::new();
    let root = PathBuf::from(&s.root.path);
    fs::write(root.join("doc.txt"), "one").unwrap();
    let version = WorkspaceFs::read_bytes_in(&s.root, "doc.txt").unwrap().version;
    assert_eq!(WorkspaceFs::stat_in(&s.root, "doc.txt").unwrap().entry.version.as_ref(), Some(&version));
    let with = |v: &str| WriteOptions { if_version: Some(v.to_owned()), ..WriteOptions::default() };
    let saved = WorkspaceFs::write_file_in(&s.root, "doc.txt", b"two", &with(&version)).unwrap();
    assert_ne!(saved.version.as_ref(), Some(&version));
    assert_eq!(answer(WorkspaceFs::write_file_in(&s.root, "doc.txt", b"three", &with(&version))), (409, "ECONFLICT"));
    assert_eq!(answer(WorkspaceFs::write_file_in(&s.root, "doc.txt", b"four", &with(saved.version.as_ref().unwrap()))), (0, "ok"));
    assert_eq!(fs::read_to_string(root.join("doc.txt")).unwrap(), "four");
}

#[test]
fn a_folder_that_takes_no_new_files_is_saved_in_place() {
    let s = Scratch::new();
    let root = PathBuf::from(&s.root.path);
    fs::create_dir(root.join("locked")).unwrap();
    fs::write(root.join("locked/doc.txt"), "old").unwrap();
    let mut denied = Denied(Vec::new());
    denied.add(root.join("locked"), "(WD)");
    let saved = WorkspaceFs::write_file_in(&s.root, "locked/doc.txt", b"new", &WriteOptions::default());
    drop(denied);
    assert_eq!(saved.unwrap().in_place, Some(true));
    assert_eq!(fs::read_to_string(root.join("locked/doc.txt")).unwrap(), "new");
}

#[test]
fn a_long_listing_keeps_folders_first() {
    let s = Scratch::new();
    let root = PathBuf::from(&s.root.path);
    for n in 0..MAX_ENTRIES + 10 {
        fs::write(root.join(format!("a-{n:04}.txt")), "").unwrap();
    }
    fs::create_dir(root.join("zz folder")).unwrap();
    let listing = WorkspaceFs::new().list_in(s.root.clone(), "", false).unwrap();
    assert!(listing.truncated);
    assert_eq!(listing.entries.len(), MAX_ENTRIES);
    assert_eq!(listing.entries[0].name, "zz folder");
    assert_eq!((listing.entries[1].name.as_str(), listing.skipped), ("a-0000.txt", 0));
}
