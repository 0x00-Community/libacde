use crate::runtime::failure::{Failure, Outcome};
use crate::runtime::fs::ranged::{MAX_CHUNK, Range, RangeOptions};
use crate::runtime::fs::save;
use crate::runtime::fs::workspace::{Contents, ModifiedMs};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::Arc;

const BUFFER: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct LentFile {
    pub name: String,
    pub label: String,
    pub writable: bool,
    pub file: Arc<File>,
}

impl LentFile {
    #[cfg(unix)]
    pub fn from_fd(name: String, label: String, writable: bool, fd: std::os::fd::OwnedFd) -> Self {
        Self { name, label, writable, file: Arc::new(File::from(fd)) }
    }

    pub fn open(path: &str, writable: bool) -> io::Result<Self> {
        let file = std::fs::OpenOptions::new().read(true).write(writable).create(writable).truncate(false).open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"));
        }
        let label = std::path::Path::new(path).file_name().map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned());
        Ok(Self { name: String::new(), label, writable, file: Arc::new(file) })
    }

    pub(crate) fn checked(self) -> Outcome<Self> {
        if self.name.is_empty() {
            return Err(Failure::invalid("a lent file has a name"));
        }
        let label = if self.label.is_empty() { self.name.clone() } else { self.label };
        Ok(Self { label, ..self })
    }

    pub(crate) fn identity(&self) -> io::Result<(u64, u64)> {
        save::identity(&self.file)
    }

    pub(crate) fn modified_ms(&self) -> io::Result<i64> {
        Ok(self.file.metadata()?.modified_ms())
    }

    pub(crate) fn writable(&self) -> Outcome<()> {
        match self.writable {
            true => Ok(()),
            false => Err(Failure::http(403, format!("{} was picked read-only", self.label))),
        }
    }

    pub(crate) fn read(&self) -> Outcome<Contents> {
        let info = self.file.metadata()?;
        let version = save::version(&self.file)?;
        Ok(Contents { data: span(&self.file, 0, info.len())?, modified_ms: info.modified_ms(), version })
    }

    pub(crate) fn write(&self, data: &[u8]) -> Outcome<()> {
        self.writable()?;
        write_all_at(&self.file, data, 0)?;
        self.file.set_len(data.len() as u64)?;
        Ok(self.file.sync_data()?)
    }

    pub(crate) fn read_range(&self, offset: Option<u64>, length: Option<u64>, options: &RangeOptions) -> Outcome<Range> {
        let (Some(offset), Some(length)) = (offset, length) else {
            return Err(Failure::invalid("offset and length are non-negative integers"));
        };
        if length > MAX_CHUNK {
            return Err(Failure::http(413, "a range is at most 16 MiB"));
        }
        let version = save::version(&self.file)?;
        if options.if_version.as_ref().is_some_and(|expected| *expected != version) {
            return Err(Failure::conflict("file changed since it was opened; open it again"));
        }
        let info = self.file.metadata()?;
        let size = info.len();
        let offset = offset.min(size);
        let data = span(&self.file, offset, length.min(size - offset))?;
        Ok(Range { data, offset, size, modified_ms: info.modified_ms(), version })
    }

    pub(crate) fn pour(&self, scratch: &mut File) -> io::Result<()> {
        scratch.seek(SeekFrom::Start(0))?;
        let mut buffer = vec![0; BUFFER];
        let mut offset = 0;
        loop {
            let read = match scratch.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            write_all_at(&self.file, &buffer[..read], offset)?;
            offset += read as u64;
        }
        self.file.set_len(offset)?;
        self.file.sync_data()
    }

    pub(crate) fn copy_into(&self, scratch: &mut File) -> io::Result<u64> {
        let mut buffer = vec![0; BUFFER];
        let mut offset = 0;
        loop {
            let read = match read_at(&self.file, &mut buffer, offset) {
                Ok(0) => return Ok(offset),
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            scratch.write_all(&buffer[..read])?;
            offset += read as u64;
        }
    }
}

fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    cfg_select! {
        unix => std::os::unix::fs::FileExt::read_at(file, buf, offset),
        _ => std::os::windows::fs::FileExt::seek_read(file, buf, offset),
    }
}

fn write_at(file: &File, data: &[u8], offset: u64) -> io::Result<usize> {
    cfg_select! {
        unix => std::os::unix::fs::FileExt::write_at(file, data, offset),
        _ => std::os::windows::fs::FileExt::seek_write(file, data, offset),
    }
}

fn span(file: &File, offset: u64, length: u64) -> io::Result<Vec<u8>> {
    let mut data = vec![0; usize::try_from(length).map_err(io::Error::other)?];
    let mut filled = 0;
    while filled < data.len() {
        match read_at(file, &mut data[filled..], offset + filled as u64) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    data.truncate(filled);
    Ok(data)
}

fn write_all_at(file: &File, mut data: &[u8], mut offset: u64) -> io::Result<()> {
    while !data.is_empty() {
        match write_at(file, data, offset) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => {
                data = &data[written..];
                offset += written as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("acde-lent-file-{name}-{}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn a_lent_file_reads_ranges_and_writes_in_place() {
        let path = scratch("rw", b"hello world");
        let lent = LentFile::open(&path.to_string_lossy(), true).unwrap();
        assert_eq!(lent.read().unwrap().data, b"hello world");
        let range = lent.read_range(Some(6), Some(100), &RangeOptions::default()).unwrap();
        assert_eq!((range.data.as_slice(), range.size), (&b"world"[..], 11));
        lent.write(b"bye").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"bye");
        _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_read_only_lent_file_refuses_writes() {
        let path = scratch("ro", b"keep");
        let lent = LentFile::open(&path.to_string_lossy(), false).unwrap();
        assert_eq!(lent.write(b"x").err().map(|failure| failure.status), Some(403));
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        _ = std::fs::remove_file(path);
    }
}
