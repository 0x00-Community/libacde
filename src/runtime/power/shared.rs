use std::ffi::OsStr;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};

use rustix::fs::inotify::{self, CreateFlags, WatchFlags};

pub struct PolicyMark {
    path: PathBuf,
}

impl PolicyMark {
    pub fn path() -> Option<PathBuf> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_absolute())?;
        Some(runtime.join("acde").join("power-policy"))
    }

    pub fn claim(path: PathBuf) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, std::process::id().to_string()).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self { path })
    }

    pub fn held(path: &Path) -> bool {
        let Some(pid) = std::fs::read_to_string(path).ok().and_then(|text| text.trim().parse::<u32>().ok()) else { return false };
        pid == std::process::id() || Path::new("/proc").join(pid.to_string()).exists()
    }
}

impl Drop for PolicyMark {
    fn drop(&mut self) {
        let ours = std::fs::read_to_string(&self.path).is_ok_and(|text| text.trim() == std::process::id().to_string());
        if ours {
            _ = std::fs::remove_file(&self.path);
        }
    }
}

pub fn watch(file: PathBuf, changed: impl Fn() -> bool + Send + 'static) -> Result<(), String> {
    let (Some(dir), Some(name)) = (file.parent().map(Path::to_path_buf), file.file_name().map(OsStr::to_os_string)) else {
        return Err(format!("{}: not a file path", file.display()));
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let fd = inotify::init(CreateFlags::CLOEXEC).map_err(|e| format!("inotify: {e}"))?;
    inotify::add_watch(&fd, &dir, WatchFlags::CLOSE_WRITE | WatchFlags::MOVED_TO).map_err(|e| format!("{}: {e}", dir.display()))?;
    let run = move || {
        let mut buf = [MaybeUninit::<u8>::uninit(); 4096];
        let mut events = inotify::Reader::new(&fd, &mut buf);
        loop {
            let ours = match events.next() {
                Ok(event) => event.file_name().is_some_and(|n| n.to_bytes() == name.as_encoded_bytes()),
                Err(rustix::io::Errno::INTR) => continue,
                Err(e) => return log::warn!(target: "crate::runtime::power", "{}: {e}", dir.display()),
            };
            if ours && !changed() {
                return;
            }
        }
    };
    std::thread::Builder::new().name("acde-power-settings".into()).spawn(run).map(drop).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_names_a_live_process_and_goes_with_it() {
        let dir = std::env::temp_dir().join(format!("acde-power-mark-{}", std::process::id()));
        let path = dir.join("power-policy");
        {
            let _mark = PolicyMark::claim(path.clone()).unwrap();
            assert!(PolicyMark::held(&path));
        }
        assert!(!path.exists());
        std::fs::write(&path, "4294967294").unwrap();
        assert!(!PolicyMark::held(&path), "a mark left by a process that is gone is not a running policy");
        _ = std::fs::remove_dir_all(&dir);
    }
}
