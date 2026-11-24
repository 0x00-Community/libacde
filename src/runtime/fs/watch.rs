use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Change {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub directory: bool,
}

type Raw = (i32, ReadFlags, u32, Option<String>);

pub type Sink = Box<dyn Fn(Vec<Change>) -> bool + Send>;

pub struct Watch {
    wd: i32,
    token: u64,
}

struct Watcher {
    fd: OwnedFd,
    subscribers: Mutex<HashMap<i32, Vec<(u64, Sink)>>>,
    next: AtomicU64,
}

const BUFFER: usize = 64 * 1024;

fn mask() -> WatchFlags {
    WatchFlags::CREATE
        | WatchFlags::DELETE
        | WatchFlags::CLOSE_WRITE
        | WatchFlags::ATTRIB
        | WatchFlags::MOVED_FROM
        | WatchFlags::MOVED_TO
        | WatchFlags::DELETE_SELF
        | WatchFlags::MOVE_SELF
        | WatchFlags::ONLYDIR
        | WatchFlags::EXCL_UNLINK
}

fn watcher() -> Option<&'static Watcher> {
    static WATCHER: OnceLock<Option<Watcher>> = OnceLock::new();
    WATCHER
        .get_or_init(|| {
            let fd = inotify::init(CreateFlags::CLOEXEC).inspect_err(|e| log::warn!("acde:fs watch: inotify: {e}")).ok()?;
            let spawned = std::thread::Builder::new().name("acde-fs-watch".into()).spawn(|| {
                if let Some(watcher) = watcher() {
                    watcher.read_forever();
                }
            });
            if let Err(e) = spawned {
                log::warn!("acde:fs watch: {e}");
                return None;
            }
            Some(Watcher { fd, subscribers: Mutex::default(), next: AtomicU64::new(1) })
        })
        .as_ref()
}

impl Watcher {
    fn add(&self, dir: &impl AsFd, sink: Sink) -> std::io::Result<Watch> {
        let path = format!("/proc/self/fd/{}", dir.as_fd().as_raw_fd());
        let mut subscribers = self.subscribers();
        let wd = inotify::add_watch(&self.fd, path, mask())?;
        let token = self.next.fetch_add(1, Ordering::Relaxed);
        subscribers.entry(wd).or_default().push((token, sink));
        Ok(Watch { wd, token })
    }

    fn remove(&self, wd: i32, token: u64) {
        let mut subscribers = self.subscribers();
        let Some(list) = subscribers.get_mut(&wd) else { return };
        let before = list.len();
        list.retain(|(t, _)| *t != token);
        if list.len() == before || !list.is_empty() {
            return;
        }
        subscribers.remove(&wd);
        _ = inotify::remove_watch(&self.fd, wd);
    }

    fn read_forever(&self) {
        let mut buf = vec![MaybeUninit::<u8>::uninit(); BUFFER];
        let mut events = inotify::Reader::new(&self.fd, &mut buf);
        let mut batch: Vec<Raw> = Vec::new();
        loop {
            match events.next() {
                Ok(event) => {
                    let name = event.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty());
                    batch.push((event.wd(), event.events(), event.cookie(), name));
                }
                Err(rustix::io::Errno::INTR) => continue,
                Err(e) => {
                    log::warn!("acde:fs watch: reading inotify: {e}");
                    return;
                }
            }
            if events.is_buffer_empty() {
                self.deliver(std::mem::take(&mut batch));
            }
        }
    }

    fn deliver(&self, batch: Vec<Raw>) {
        let mut subscribers = self.subscribers();
        if batch.iter().any(|(_, flags, ..)| flags.contains(ReadFlags::QUEUE_OVERFLOW)) {
            let unknown = Change { kind: "unknown", name: String::new(), from: None, directory: false };
            for list in subscribers.values_mut() {
                list.retain(|(_, sink)| sink(vec![unknown.clone()]));
            }
        }
        let mut ignored = Vec::new();
        for (wd, changes) in changes(&batch, &mut ignored) {
            if let Some(list) = subscribers.get_mut(&wd) {
                list.retain(|(_, sink)| sink(changes.clone()));
            }
        }
        for wd in ignored {
            subscribers.remove(&wd);
        }
        let dead: Vec<i32> = subscribers.iter().filter(|(_, list)| list.is_empty()).map(|(wd, _)| *wd).collect();
        for wd in dead {
            subscribers.remove(&wd);
            _ = inotify::remove_watch(&self.fd, wd);
        }
    }
}

fn changes(batch: &[Raw], ignored: &mut Vec<i32>) -> Vec<(i32, Vec<Change>)> {
    let mut out: Vec<(i32, Vec<Change>)> = Vec::new();
    let mut push = |wd: i32, change: Change| {
        let at = match out.iter().position(|(w, _)| *w == wd) {
            Some(at) => at,
            None => {
                out.push((wd, Vec::new()));
                out.len() - 1
            }
        };
        let list = &mut out[at].1;
        if !list.contains(&change) {
            list.push(change);
        }
    };
    let mut paired = vec![false; batch.len()];
    for (i, (wd, flags, cookie, name)) in batch.iter().enumerate() {
        if *wd < 0 {
            continue;
        }
        if flags.contains(ReadFlags::IGNORED) {
            ignored.push(*wd);
            continue;
        }
        let directory = flags.contains(ReadFlags::ISDIR);
        let name = name.clone().unwrap_or_default();
        let change = |kind: &'static str, name: String, from: Option<String>| Change { kind, name, from, directory };
        if flags.intersects(ReadFlags::DELETE_SELF | ReadFlags::MOVE_SELF) {
            push(*wd, change("disappeared", String::new(), None));
        } else if flags.contains(ReadFlags::CREATE) {
            push(*wd, change("appeared", name, None));
        } else if flags.contains(ReadFlags::DELETE) {
            push(*wd, change("disappeared", name, None));
        } else if flags.intersects(ReadFlags::CLOSE_WRITE | ReadFlags::ATTRIB) {
            push(*wd, change("modified", name, None));
        } else if flags.contains(ReadFlags::MOVED_FROM) {
            let to = batch.iter().enumerate().skip(i + 1).find(|(_, (w, f, c, _))| *w == *wd && *c == *cookie && f.contains(ReadFlags::MOVED_TO));
            match to {
                Some((j, (_, _, _, to))) => {
                    paired[j] = true;
                    push(*wd, change("moved", to.clone().unwrap_or_default(), Some(name)));
                }
                None => push(*wd, change("disappeared", name, None)),
            }
        } else if flags.contains(ReadFlags::MOVED_TO) && !paired[i] {
            push(*wd, change("appeared", name, None));
        }
    }
    out
}

impl Watcher {
    fn subscribers(&self) -> MutexGuard<'_, HashMap<i32, Vec<(u64, Sink)>>> {
        self.subscribers.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn watch(dir: &impl AsFd, sink: Sink) -> std::io::Result<Watch> {
    watcher().ok_or_else(|| std::io::Error::other("no inotify"))?.add(dir, sink)
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some(watcher) = watcher() {
            watcher.remove(self.wd, self.token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_watched_folder_reports_what_appears_moves_changes_and_goes() {
        let place = std::env::temp_dir().join(format!("acde-watch-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&place);
        std::fs::create_dir_all(&place).unwrap();
        let dir = cap_std::fs::Dir::open_ambient_dir(&place, cap_std::ambient_authority()).unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = watch(&dir, Box::new(move |changes| tx.send(changes).is_ok())).unwrap();
        drop(dir);
        let mut seen = Vec::new();
        let mut until = |want: &Change| {
            while !seen.contains(want) {
                seen.extend(rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| panic!("no {want:?} in {seen:?}")));
            }
        };
        let change = |kind: &'static str, name: &str, from: Option<&str>| Change { kind, name: name.into(), from: from.map(Into::into), directory: false };
        std::fs::write(place.join("a.txt"), "x").unwrap();
        until(&change("appeared", "a.txt", None));
        until(&change("modified", "a.txt", None));
        std::fs::rename(place.join("a.txt"), place.join("b.txt")).unwrap();
        until(&change("moved", "b.txt", Some("a.txt")));
        std::fs::remove_file(place.join("b.txt")).unwrap();
        until(&change("disappeared", "b.txt", None));
        std::fs::remove_dir(&place).unwrap();
        until(&Change { kind: "disappeared", name: String::new(), from: None, directory: false });
        drop(handle);
    }
}
