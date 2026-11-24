use super::beneath::{self, Dir};
use super::workspace::{FinderEntry, FinderRoot, WorkspaceFs};
use crate::runtime::failure::{Failure, Outcome};
use serde_json::Value;
use std::io::ErrorKind;

pub(super) fn copy(from: &FinderRoot, path: &str, to: &FinderRoot, dir: &str) -> Outcome<Value> {
    to.writable()?;
    let source = WorkspaceFs::inner(path)?;
    if source.is_empty() {
        return Err(Failure::invalid("a root itself cannot be copied; copy what is in it"));
    }
    let (parent, name) = WorkspaceFs::parent(from, &source)?;
    let relative = WorkspaceFs::inner(dir)?;
    let target = WorkspaceFs::dir(to, &relative)?;
    let kind = parent.symlink_metadata(&name)?.file_type();
    let entry = if kind.is_symlink() {
        return Err(Failure::invalid("links are not copied"));
    } else if kind.is_dir() {
        let folder_dir = beneath::open_dir(&parent, &name)?;
        scan(&folder_dir, &source)?;
        folder(&folder_dir, &target, &name)?
    } else {
        WorkspaceFs::copy_into(to, &relative, &name, beneath::read(&parent, &name, true)?)?
    };
    let relative = if relative.is_empty() { entry.name.clone() } else { format!("{relative}/{}", entry.name) };
    let mut value = serde_json::to_value(entry).map_err(Failure::internal)?;
    value["path"] = Value::String(relative);
    Ok(value)
}

fn scan(dir: &Dir, at: &str) -> Outcome<()> {
    for entry in dir.entries()? {
        let entry = entry?;
        let path = format!("{at}/{}", entry.file_name().to_string_lossy());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(Failure::invalid(format!("{path} is a link; folders with links are not copied")));
        }
        if kind.is_dir() {
            scan(&entry.open_dir()?, &path)?;
        } else if !kind.is_file() {
            return Err(Failure::invalid(format!("{path} is not a regular file or folder")));
        }
    }
    Ok(())
}

fn folder(source: &Dir, target: &Dir, name: &str) -> Outcome<FinderEntry> {
    for n in 1..10_000 {
        let candidate = WorkspaceFs::numbered(name, n);
        match target.create_dir(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
        let copied = target.open_dir(&candidate).map_err(Failure::from).and_then(|made| tree(source, &made, beneath::identity(&made)?));
        if let Err(failure) = copied {
            _ = target.remove_dir_all(&candidate);
            return Err(failure);
        }
        return FinderEntry::read(target, &candidate);
    }
    Err(Failure::exists("no free name for the copy"))
}

fn tree(from: &Dir, to: &Dir, made: (u64, u64)) -> Outcome<()> {
    for entry in from.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(Failure::invalid(format!("{} became a link while it was copied", name.to_string_lossy())));
        }
        if kind.is_dir() {
            let inner = entry.open_dir()?;
            if beneath::identity(&inner)? == made {
                return Err(Failure::invalid("a folder cannot be copied into itself"));
            }
            to.create_dir(&name)?;
            tree(&inner, &to.open_dir(&name)?, made)?;
            continue;
        }
        let mut reader = beneath::read(from, &name, true)?;
        let mut writer = beneath::create_new(to, &name)?;
        std::io::copy(&mut reader, &mut writer)?;
        writer.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::runtime::fs::{Files, FsCall, FsError, FsReply, Workspace};
    use serde_json::{Value, json};
    use std::path::PathBuf;

    struct Home(PathBuf);

    impl Home {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("acde-copy-{name}-{}", std::process::id()));
            _ = std::fs::remove_dir_all(&dir);
            for place in ["Desktop", "Documents", "Downloads"] {
                std::fs::create_dir_all(dir.join(place)).unwrap();
            }
            Self(dir)
        }

        fn workspace(&self, places: &[&str]) -> Workspace {
            let places: Vec<String> = places.iter().map(|place| (*place).to_owned()).collect();
            Workspace::at(&self.0.to_string_lossy()).files(Files::new(&places, false), "mover")
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn json(reply: Result<FsReply, FsError>) -> Value {
        match reply {
            Ok(FsReply::Json(value)) => value,
            Ok(_) => Value::String("other".into()),
            Err(error) => json!({"status": error.status, "code": error.code, "message": error.message}),
        }
    }

    fn copy(workspace: &Workspace, from: (&str, &str), to: (&str, &str)) -> Value {
        let call = FsCall::Copy { from_root: from.0.into(), from_path: from.1.into(), to_root: to.0.into(), to_dir: to.1.into() };
        json(workspace.call(call))
    }

    #[test]
    fn copies_within_a_root_and_across_two_granted_roots() {
        let home = Home::new("across");
        std::fs::write(home.0.join("Documents").join("report.txt"), "report").unwrap();
        let app = home.workspace(&["documents", "downloads"]);
        std::fs::create_dir_all(home.0.join("Documents").join("archive")).unwrap();
        let inside = copy(&app, ("documents", "report.txt"), ("documents", "archive"));
        assert_eq!((inside["name"].as_str(), inside["path"].as_str()), (Some("report.txt"), Some("archive/report.txt")));
        let across = copy(&app, ("documents", "report.txt"), ("downloads", ""));
        assert_eq!((across["name"].as_str(), across["path"].as_str(), across["size"].as_u64()), (Some("report.txt"), Some("report.txt"), Some(6)));
        assert_eq!(std::fs::read_to_string(home.0.join("Downloads").join("report.txt")).unwrap(), "report");
        let private = copy(&app, ("downloads", "report.txt"), ("private", ""));
        assert_eq!(private["name"], "report.txt");
    }

    #[test]
    fn copies_a_folder_and_numbers_a_name_that_is_taken() {
        let home = Home::new("folder");
        let project = home.0.join("Documents").join("project");
        std::fs::create_dir_all(project.join("src").join("deep")).unwrap();
        std::fs::write(project.join("readme.md"), "# project").unwrap();
        std::fs::write(project.join("src").join("deep").join("main.rs"), "fn main() {}").unwrap();
        let app = home.workspace(&["documents", "downloads"]);
        let first = copy(&app, ("documents", "project"), ("downloads", ""));
        assert_eq!((first["name"].as_str(), first["kind"].as_str()), (Some("project"), Some("directory")));
        let copied = home.0.join("Downloads").join("project");
        assert_eq!(std::fs::read_to_string(copied.join("src").join("deep").join("main.rs")).unwrap(), "fn main() {}");
        let second = copy(&app, ("documents", "project"), ("downloads", ""));
        assert_eq!(second["name"], "project 2");
        std::fs::write(home.0.join("Documents").join("a.txt"), "a").unwrap();
        copy(&app, ("documents", "a.txt"), ("downloads", ""));
        assert_eq!(copy(&app, ("documents", "a.txt"), ("downloads", ""))["name"], "a 2.txt");
        assert_eq!(copy(&app, ("documents", "project"), ("documents", "project/src"))["code"], "EINVAL");
    }

    #[test]
    fn a_source_or_target_outside_the_roots_is_refused() {
        let home = Home::new("outside");
        std::fs::write(home.0.join("Desktop").join("secret.txt"), "secret").unwrap();
        std::fs::write(home.0.join("Documents").join("report.txt"), "report").unwrap();
        let app = home.workspace(&["documents"]);
        assert_eq!(copy(&app, ("documents", "../Desktop/secret.txt"), ("private", ""))["code"], "EINVAL");
        assert_eq!(copy(&app, ("desktop", "secret.txt"), ("private", ""))["code"], "EACCES");
        assert_eq!(copy(&app, ("documents", "report.txt"), ("desktop", ""))["code"], "EACCES");
        let desktop = home.0.join("Desktop").to_string_lossy().into_owned();
        assert_eq!(copy(&app, ("documents", "report.txt"), (&desktop, ""))["code"], "EACCES");
        assert_eq!(copy(&app, ("documents", "report.txt"), ("documents", "../Desktop"))["code"], "EINVAL");
        assert_eq!(copy(&app, ("documents", ""), ("private", ""))["code"], "EINVAL");
        assert!(!home.0.join("Desktop").join("report.txt").exists());
    }

    #[test]
    fn a_link_leaving_the_root_is_refused() {
        let home = Home::new("link");
        std::fs::write(home.0.join("Desktop").join("secret.txt"), "secret").unwrap();
        let documents = home.0.join("Documents");
        let folder = documents.join("folder");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("plain.txt"), "plain").unwrap();
        #[cfg(windows)]
        let link = |at: &PathBuf| {
            let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(at).arg(home.0.join("Desktop")).output().unwrap();
            assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
        };
        #[cfg(unix)]
        let link = |at: &PathBuf| std::os::unix::fs::symlink(home.0.join("Desktop"), at).unwrap();
        link(&documents.join("leak"));
        link(&folder.join("inner"));
        let app = home.workspace(&["documents", "downloads"]);
        assert_eq!(copy(&app, ("documents", "leak"), ("downloads", ""))["code"], "EINVAL");
        assert_eq!(copy(&app, ("documents", "leak/secret.txt"), ("downloads", ""))["code"], "EINVAL");
        assert_eq!(copy(&app, ("documents", "folder"), ("downloads", ""))["code"], "EINVAL");
        assert_eq!(copy(&app, ("documents", "folder/plain.txt"), ("documents", "leak"))["code"], "EINVAL");
        assert!(std::fs::read_dir(home.0.join("Downloads")).unwrap().next().is_none());
        for at in [documents.join("leak"), folder.join("inner")] {
            #[cfg(windows)]
            std::fs::remove_dir(&at).unwrap();
            #[cfg(unix)]
            std::fs::remove_file(&at).unwrap();
        }
    }

    #[test]
    fn a_copy_larger_than_the_page_write_limit_succeeds() {
        let home = Home::new("large");
        let size = 65 * 1024 * 1024 + 7;
        let file = std::fs::File::create(home.0.join("Documents").join("large.bin")).unwrap();
        file.set_len(size).unwrap();
        drop(file);
        let app = home.workspace(&["documents", "downloads"]);
        let copied = copy(&app, ("documents", "large.bin"), ("downloads", ""));
        assert_eq!(copied["size"].as_u64(), Some(size), "{copied}");
        assert_eq!(std::fs::metadata(home.0.join("Downloads").join("large.bin")).unwrap().len(), size);
    }

    #[test]
    fn a_path_workspace_needs_both_roots_in_its_manifest() {
        let home = Home::new("legacy");
        std::fs::write(home.0.join("Downloads").join("a.txt"), "a").unwrap();
        let all = Workspace::at(&home.0.to_string_lossy());
        let downloads = home.0.join("Downloads").to_string_lossy().into_owned();
        let root = downloads.clone();
        let app = all.scoped(Some(vec![downloads]), Some("legacy"));
        assert_eq!(copy(&app, (&root, "a.txt"), (&root, ""))["name"], "a 2.txt");
        let desktop = home.0.join("Desktop").to_string_lossy().into_owned();
        assert_eq!(copy(&app, (&root, "a.txt"), (&desktop, ""))["code"], "EACCES");
    }
}
