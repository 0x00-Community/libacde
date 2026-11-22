use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
pub(crate) use cap_std::fs::{Dir, OpenOptions};
use std::io;
use std::path::Path;

const ESCAPE: &str = "a path led outside of the filesystem";

pub(crate) fn escaped(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied && error.get_ref().is_some_and(|inner| inner.to_string() == ESCAPE)
}

pub(crate) fn open_root(path: &str) -> io::Result<Dir> {
    Dir::open_ambient_dir(path, cap_std::ambient_authority())
}

pub(crate) fn open_dir(root: &Dir, relative: &str) -> io::Result<Dir> {
    #[cfg(test)]
    hook::run();
    match relative {
        "" | "." => root.try_clone(),
        relative => root.open_dir(relative),
    }
}

pub(crate) fn open_file(dir: &Dir, name: impl AsRef<Path>, options: &mut OpenOptions, nonblock: bool) -> io::Result<std::fs::File> {
    options.follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o644);
        if nonblock {
            options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
        }
    }
    #[cfg(not(unix))]
    let _ = nonblock;
    let file = dir.open_with(name, options)?.into_std();
    #[cfg(windows)]
    if file.metadata()?.file_type().is_symlink() {
        return Err(io::Error::other("refusing to follow a symlink"));
    }
    Ok(file)
}

pub(crate) fn read(dir: &Dir, name: impl AsRef<Path>, nonblock: bool) -> io::Result<std::fs::File> {
    open_file(dir, name, OpenOptions::new().read(true), nonblock)
}

pub(crate) fn create_new(dir: &Dir, name: impl AsRef<Path>) -> io::Result<std::fs::File> {
    open_file(dir, name, OpenOptions::new().write(true).create_new(true), false)
}

pub(crate) fn sync(dir: &Dir) -> io::Result<()> {
    cfg_select! {
        unix => dir.open(".")?.sync_all(),
        _ => {
            _ = dir;
            Ok(())
        }
    }
}

pub(crate) fn identity(dir: &Dir) -> io::Result<(u64, u64)> {
    super::save::identity(&dir.try_clone()?.into_std_file())
}

#[cfg(windows)]
pub(crate) fn final_path(dir: &Dir) -> io::Result<std::path::PathBuf> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_DOS};
    let mut buffer = vec![0u16; 1024];
    loop {
        let n = unsafe { GetFinalPathNameByHandleW(dir.as_raw_handle() as _, buffer.as_mut_ptr(), buffer.len() as u32, VOLUME_NAME_DOS) } as usize;
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n < buffer.len() {
            return Ok(String::from_utf16_lossy(&buffer[..n]).into());
        }
        buffer.resize(n + 1, 0);
    }
}

pub(crate) fn rename_no_replace(from: (&Dir, &str), to: (&Dir, &str)) -> io::Result<()> {
    cfg_select! {
        any(target_os = "linux", target_os = "macos") => {
            use rustix::fs::{RenameFlags, renameat_with};
            renameat_with(from.0, from.1, to.0, to.1, RenameFlags::NOREPLACE).map_err(io::Error::from)
        }
        windows => {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
            let wide = |(dir, name): (&Dir, &str)| -> io::Result<Vec<u16>> {
                Ok(final_path(dir)?.join(name).as_os_str().encode_wide().chain(Some(0)).collect())
            };
            let (a, b) = (wide(from)?, wide(to)?);
            match unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), MOVEFILE_WRITE_THROUGH) } {
                0 => Err(io::Error::last_os_error()),
                _ => Ok(()),
            }
        }
        _ => {
            if to.0.symlink_metadata(to.1).is_ok() {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            from.0.rename(from.1, to.0, to.1)
        }
    }
}

#[cfg(test)]
pub(crate) mod hook {
    use std::cell::RefCell;

    thread_local! {
        static BEFORE_OPEN: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    }

    pub(crate) fn before_open(f: impl FnOnce() + 'static) {
        BEFORE_OPEN.with(|h| *h.borrow_mut() = Some(Box::new(f)));
    }

    pub(super) fn run() {
        if let Some(f) = BEFORE_OPEN.with(|h| h.borrow_mut().take()) {
            f();
        }
    }
}
