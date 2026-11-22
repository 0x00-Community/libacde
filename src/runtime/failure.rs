use napi::Status;
use serde::Serialize;
use std::io;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Code {
    HttpError,
    InvalidRequest,
    InternalError,
    #[serde(untagged)]
    Service(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    Inval,
    Acces,
    Noent,
    Exist,
    Conflict,
    NotEmpty,
    Fbig,
    Ilseq,
    NoSpc,
    Io,
}

impl Errno {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inval => "EINVAL",
            Self::Acces => "EACCES",
            Self::Noent => "ENOENT",
            Self::Exist => "EEXIST",
            Self::Conflict => "ECONFLICT",
            Self::NotEmpty => "ENOTEMPTY",
            Self::Fbig => "EFBIG",
            Self::Ilseq => "EILSEQ",
            Self::NoSpc => "ENOSPC",
            Self::Io => "EIO",
        }
    }

    fn of_status(status: u16) -> Self {
        match status {
            400 => Self::Inval,
            403 => Self::Acces,
            404 => Self::Noent,
            409 => Self::Exist,
            413 => Self::Fbig,
            507 => Self::NoSpc,
            _ => Self::Io,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Failure {
    pub status: u16,
    pub code: Code,
    pub message: String,
    retryable: bool,
    #[serde(skip)]
    pub errno: Errno,
}

pub(crate) type Outcome<T> = std::result::Result<T, Failure>;

impl Failure {
    pub fn http(status: u16, message: impl Into<String>) -> Self {
        Self::new(status, Code::HttpError, message)
    }
    pub fn new(status: u16, code: Code, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into(), retryable: false, errno: Errno::of_status(status) }
    }
    pub fn exists(message: impl Into<String>) -> Self {
        Self::http(409, message).with(Errno::Exist)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::http(409, message).with(Errno::Conflict)
    }
    pub fn with(mut self, errno: Errno) -> Self {
        self.errno = errno;
        self
    }
    #[cfg(target_os = "linux")]
    pub fn token_refused(message: &str) -> Self {
        Self::new(403, Code::Service("token_refused".into()), message)
    }
    #[cfg(target_os = "linux")]
    pub fn office_config(message: impl Into<String>) -> Self {
        Self::new(503, Code::Service("office_config".into()), message)
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(400, Code::InvalidRequest, message)
    }
    pub fn internal(detail: impl std::fmt::Display) -> Self {
        log::error!("operation failed: {detail}");
        Self::new(500, Code::InternalError, "Runtime server operation failed")
    }
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        use io::ErrorKind as Kind;
        let mapped = match error.kind() {
            _ if crate::runtime::fs::escaped(&error) => (400, "path leaves the folder through a link"),
            Kind::NotFound => (404, "no such file or directory"),
            Kind::PermissionDenied => (403, "permission denied"),
            Kind::AlreadyExists => (409, "already exists"),
            Kind::DirectoryNotEmpty => return Self::http(409, "directory is not empty").with(Errno::NotEmpty),
            Kind::NotADirectory => (400, "not a directory"),
            Kind::IsADirectory => (400, "is a directory"),
            Kind::InvalidFilename => (400, "invalid file name"),
            Kind::CrossesDevices => (400, "cannot move across filesystems"),
            Kind::StorageFull => (507, "no space left on device"),
            Kind::ReadOnlyFilesystem => (403, "read-only filesystem"),
            _ if symlink_loop(&error) => (400, "refusing to follow a symlink"),
            _ => return Self::internal(error),
        };
        Self::http(mapped.0, mapped.1)
    }
}

fn symlink_loop(error: &io::Error) -> bool {
    cfg_select! {
        unix => rustix::io::Errno::from_io_error(error) == Some(rustix::io::Errno::LOOP),
        _ => {
            _ = error;
            false
        }
    }
}

impl From<Failure> for String {
    fn from(failure: Failure) -> Self {
        serde_json::to_string(&failure).unwrap_or(failure.message)
    }
}

impl From<Failure> for napi::Error {
    fn from(failure: Failure) -> Self {
        napi::Error::new(Status::GenericFailure, serde_json::to_string(&failure).unwrap_or(failure.message))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use rustix::io::Errno as OsErrno;

    fn errno(code: i32) -> (u16, &'static str, String) {
        let failure = Failure::from(io::Error::from_raw_os_error(code));
        (failure.status, failure.errno.as_str(), failure.message)
    }

    #[test]
    fn io_errors_get_a_status_and_a_node_code() {
        assert_eq!(errno(OsErrno::NOENT.raw_os_error()), (404, "ENOENT", "no such file or directory".into()));
        assert_eq!(errno(OsErrno::ACCESS.raw_os_error()), (403, "EACCES", "permission denied".into()));
        assert_eq!(errno(OsErrno::EXIST.raw_os_error()), (409, "EEXIST", "already exists".into()));
        assert_eq!(errno(OsErrno::NOTEMPTY.raw_os_error()), (409, "ENOTEMPTY", "directory is not empty".into()));
        assert_eq!(errno(OsErrno::ISDIR.raw_os_error()), (400, "EINVAL", "is a directory".into()));
        assert_eq!(errno(OsErrno::NOSPC.raw_os_error()), (507, "ENOSPC", "no space left on device".into()));
        assert_eq!(errno(OsErrno::IO.raw_os_error()), (500, "EIO", "Runtime server operation failed".into()));
    }

    #[test]
    fn the_maker_picks_the_code_of_a_409() {
        assert_eq!(Failure::exists("file already exists").errno, Errno::Exist);
        assert_eq!(Failure::conflict("file changed since it was opened").errno, Errno::Conflict);
        assert_eq!(Failure::http(413, "too big").errno, Errno::Fbig);
        assert_eq!(Failure::invalid("bad").errno, Errno::Inval);
        assert_eq!(Failure::http(404, "gone").with(Errno::Conflict).errno, Errno::Conflict);
    }

    #[test]
    fn the_json_of_a_failure_does_not_change() {
        let json = serde_json::to_value(Failure::conflict("changed")).unwrap();
        assert_eq!(json, serde_json::json!({"status": 409, "code": "http_error", "message": "changed", "retryable": false}));
    }
}
