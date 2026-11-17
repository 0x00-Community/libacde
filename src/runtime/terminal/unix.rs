use super::{DRAIN, Registry, Session, Utf8Stream};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{Access, OFlags, access, fcntl_setfl};
use rustix::io::Errno;
use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::json;
use std::{
    ffi::CStr,
    os::fd::OwnedFd,
    os::unix::process::ExitStatusExt,
    process::Child,
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) type Master = OwnedFd;

const CWD_EVERY: Duration = Duration::from_secs(1);
const BATCH_BYTES: usize = 256 * 1024;
const LINGER_BYTES: usize = 64 * 1024;
const LINGER: Duration = Duration::from_millis(4);

pub(super) struct Process {
    child: Child,
    home: String,
}

pub(super) struct Spawned {
    pub(super) master: Master,
    pub(super) pid: u32,
    pub(super) process: Process,
}

pub(super) fn root() -> String {
    "/".into()
}

fn account_shell() -> Option<String> {
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let status = unsafe { libc::getpwuid_r(libc::getuid(), &mut entry, buffer.as_mut_ptr(), buffer.len(), &mut found) };
    if status != 0 || found.is_null() || entry.pw_shell.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(entry.pw_shell) }.to_str().ok().map(str::to_owned)
}

fn login_shell() -> String {
    account_shell()
        .into_iter()
        .chain(std::env::var("SHELL").ok())
        .find(|shell| shell.starts_with('/') && access(shell.as_str(), Access::EXEC_OK).is_ok())
        .unwrap_or_else(|| "/bin/sh".into())
}

pub(super) fn spawn(home: &str, cols: u32, rows: u32) -> Result<Spawned, String> {
    let file = login_shell();
    let mut command = std::process::Command::new(&file);
    command.arg("-l").envs([("TERM", "xterm-256color"), ("COLORTERM", "truecolor")]).current_dir(home);
    let (master, child) = crate::runtime::pty::spawn(&mut command, cols, rows).inspect_err(|reason| log::warn!("could not start {file} -l: {reason}"))?;
    _ = fcntl_setfl(&master, OFlags::NONBLOCK);
    Ok(Spawned { master, pid: child.id(), process: Process { child, home: home.to_owned() } })
}

pub(super) fn resize(master: &Master, cols: u32, rows: u32) -> Result<(), String> {
    crate::runtime::pty::set_size(master, cols, rows)
}

pub(super) fn release(master: Arc<Master>) {
    drop(master);
}

pub(super) fn run(registry: &Registry, session: &Session, process: Process) {
    let pid = process.child.id();
    Io {
        registry,
        session,
        pid,
        pidfd: pidfd(pid),
        child: process.child,
        home: process.home,
        decoder: Utf8Stream::default(),
        exited: None,
        drained: false,
        cwd: String::new(),
        next_cwd: Instant::now() + CWD_EVERY,
    }
    .run()
}

struct Io<'a> {
    registry: &'a Registry,
    session: &'a Session,
    pid: u32,
    child: Child,
    pidfd: Option<OwnedFd>,
    home: String,
    decoder: Utf8Stream,
    exited: Option<(Instant, std::process::ExitStatus)>,
    drained: bool,
    cwd: String,
    next_cwd: Instant,
}

impl Io<'_> {
    fn run(mut self) {
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let (closed, ended, deadlines, pending_input) = {
                let state = self.session.state();
                (state.closed, state.ended, [state.deadlines.removal, state.deadlines.kill], !state.input.is_empty())
            };
            if closed && ended && self.exited.is_some() {
                return;
            }
            let master = self.session.master().filter(|_| !self.drained);
            let now = Instant::now();
            let drain = self.exited.filter(|_| !ended).map(|(at, _)| at + DRAIN);
            let cwd = (!ended).then_some(self.next_cwd);
            let cwd = cwd.or((self.pidfd.is_none() && self.exited.is_none()).then(|| now + Duration::from_millis(100)));
            let next = deadlines.into_iter().flatten().chain(drain).chain(cwd).min();
            let timeout = next.and_then(|at| Timespec::try_from(at.saturating_duration_since(now).min(Duration::from_secs(60))).ok());
            let wake = &self.session.wake.read;
            let idle = PollFd::new(wake, PollFlags::empty());
            let events = PollFlags::IN | if pending_input { PollFlags::OUT } else { PollFlags::empty() };
            let mut fds = [
                PollFd::new(wake, PollFlags::IN),
                self.pidfd.as_ref().map_or(idle.clone(), |pidfd| PollFd::new(pidfd, PollFlags::IN)),
                master.as_deref().map_or(idle, |master| PollFd::new(master, events)),
            ];
            _ = poll(&mut fds, timeout.as_ref());
            let [woken, exited, ready] = fds.map(|fd| fd.revents());
            if !woken.is_empty() {
                self.session.wake.drain();
            }
            if let Some(master) = &master {
                if ready.contains(PollFlags::OUT) {
                    self.write(master);
                }
                if ready.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR) {
                    self.read(master, &mut buffer);
                }
            }
            if !exited.is_empty() {
                self.reap();
            }
            self.tick();
        }
    }

    fn write(&self, master: &OwnedFd) {
        let mut state = self.session.state();
        while !state.input.is_empty() {
            match rustix::io::write(master, &state.input) {
                Ok(0) => return,
                Ok(wrote) => _ = state.input.drain(..wrote),
                Err(error) => {
                    if error != Errno::AGAIN {
                        state.input.clear();
                    }
                    return;
                }
            }
        }
    }

    fn read(&mut self, master: &OwnedFd, buffer: &mut [u8]) {
        let (mut text, mut total, started) = (String::new(), 0, Instant::now());
        'batch: loop {
            while total < BATCH_BYTES {
                match rustix::io::read(master, &mut *buffer) {
                    Ok(read) if read > 0 => {
                        total += read;
                        text.push_str(&self.decoder.push(&buffer[..read]));
                    }
                    Err(Errno::AGAIN) => break,
                    _ => {
                        self.drained = true;
                        break 'batch;
                    }
                }
            }
            if total == 0 || total >= LINGER_BYTES || started.elapsed() >= LINGER {
                break;
            }
            let mut more = [PollFd::new(master, PollFlags::IN)];
            if !matches!(poll(&mut more, Some(&Timespec { tv_sec: 0, tv_nsec: 1_000_000 })), Ok(1..)) {
                break;
            }
        }
        self.session.emit(vec![Session::output(text)]);
    }

    fn reap(&mut self) {
        if let Ok(Some(status)) = self.child.try_wait() {
            self.pidfd = None;
            if self.exited.is_none() {
                self.exited = Some((Instant::now(), status));
            }
        }
    }

    fn display(&self, path: &str) -> String {
        match path.strip_prefix(self.home.as_str()) {
            Some("") => "~".into(),
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => path.into(),
        }
    }

    fn tick(&mut self) {
        if self.exited.is_none() && self.pidfd.is_none() {
            self.reap();
        }
        let now = Instant::now();
        let (ended, removal, kill) = {
            let state = self.session.state();
            (state.ended, state.deadlines.removal, state.deadlines.kill)
        };
        if !ended && now >= self.next_cwd {
            self.next_cwd = now + CWD_EVERY;
            if let Ok(path) = std::fs::read_link(format!("/proc/{}/cwd", self.pid)) {
                let path = self.display(&path.to_string_lossy());
                if path != self.cwd {
                    self.cwd = path.clone();
                    self.session.emit(vec![json!({"type": "cwd", "path": path})]);
                }
            }
        }
        if let Some((at, status)) = self.exited.filter(|_| !ended)
            && (self.drained || now >= at + DRAIN)
        {
            let exit = json!({"type": "exit", "code": status.code(), "signal": status.signal()});
            self.session.end(self.registry, self.decoder.end(), exit);
        }
        if removal.is_some_and(|at| now >= at) {
            self.registry.close(self.session);
        }
        if kill.is_some_and(|at| now >= at) {
            self.session.state().deadlines.kill = None;
            if !self.session.state().ended
                && let Some(group) = Pid::from_raw(self.pid as i32)
            {
                _ = kill_process_group(group, Signal::KILL);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn pidfd(pid: u32) -> Option<OwnedFd> {
    Pid::from_raw(pid as i32).and_then(|pid| rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).ok())
}

#[cfg(not(target_os = "linux"))]
fn pidfd(_: u32) -> Option<OwnedFd> {
    None
}

pub(super) struct Wake {
    read: OwnedFd,
    write: OwnedFd,
}

impl Wake {
    #[cfg(target_os = "linux")]
    pub(super) fn new() -> std::io::Result<Self> {
        let fd = rustix::event::eventfd(0, rustix::event::EventfdFlags::NONBLOCK | rustix::event::EventfdFlags::CLOEXEC)?;
        Ok(Self { write: fd.try_clone()?, read: fd })
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn new() -> std::io::Result<Self> {
        let (read, write) = std::os::unix::net::UnixStream::pair()?;
        read.set_nonblocking(true)?;
        write.set_nonblocking(true)?;
        Ok(Self { read: read.into(), write: write.into() })
    }

    pub(super) fn signal(&self) {
        _ = rustix::io::write(&self.write, &1u64.to_ne_bytes());
    }

    fn drain(&self) {
        let mut buf = [0u8; 64];
        while matches!(rustix::io::read(&self.read, &mut buf), Ok(n) if n > 0) {}
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::super::Terminals;
    use super::super::tests::{Seen, WAIT, failed, invalid, missing, open};
    use super::*;

    #[test]
    fn a_session_on_a_real_pty() {
        let t = Terminals::new();
        let id = open(&t);
        assert_eq!(id.len(), 36);
        let s = Seen::follow(&t, &id);
        assert_eq!(t.input(id.clone(), "echo he\"\"llo; pwd\n".into()).unwrap(), json!({"ok": true}));
        let home = std::env::home_dir().unwrap().to_string_lossy().into_owned();
        s.until("echo", WAIT, |log| log.0.contains("hello") && log.0.contains(&home));
        s.until("cwd ~", WAIT, |log| log.1.iter().any(|e| e["type"] == "cwd" && e["path"] == "~"));
        t.input(id.clone(), "cd /tmp\n".into()).unwrap();
        s.until("cwd /tmp", WAIT, |log| log.1.iter().any(|e| e["type"] == "cwd" && e["path"] == "/tmp"));
        assert_eq!(t.resize(id.clone(), 100.0, 30.0).unwrap(), json!({"cols": 100, "rows": 30}));
        assert_eq!(failed(t.resize(id.clone(), 100.0, 1.0)), invalid("rows must be an integer in [2, 1000]"));
        t.input(id.clone(), "stty size\n".into()).unwrap();
        s.until("stty size", WAIT, |log| log.0.contains("30 100"));
        t.input(id.clone(), "printf '\\xe4\\xb8'; sleep 0.3; printf '\\xad|\\n'\n".into()).unwrap();
        s.until("split UTF-8 joined", WAIT, |log| log.0.contains("中|"));
        assert!(!s.text().contains('\u{fffd}'));
        assert_eq!(failed(t.input(id.clone(), "x".repeat(65537))), invalid("data must be a string of at most 65536 characters"));
        let before = s.sizes().len();
        t.input(id.clone(), "seq 1 200000; echo SEQ-$((1+1))-DONE\n".into()).unwrap();
        s.until("seq output", Duration::from_secs(20), |log| log.0.contains("200000\r\nSEQ-2-DONE\r\n"));
        let sizes = s.sizes()[before..].to_vec();
        assert!(sizes.iter().sum::<usize>() / sizes.len() > 4096, "output is batched: {} callbacks", sizes.len());
        t.input(id.clone(), "exit 7\n".into()).unwrap();
        assert_eq!(s.exited(), json!({"type": "exit", "code": 7, "signal": null}));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(failed(t.input(id, "x".into())), missing());
    }

    #[test]
    fn backlog_signals_and_hang_up() {
        let t = Terminals::new();
        let id = open(&t);
        t.input(id.clone(), "echo early-$((6*7)); exit 3\n".into()).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        let s = Seen::follow(&t, &id);
        assert_eq!(s.exited(), json!({"type": "exit", "code": 3, "signal": null}));
        assert!(s.text().contains("early-42"));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(failed(t.input(id, "x".into())), missing());

        let id = open(&t);
        let s = Seen::follow(&t, &id);
        t.input(id, "kill -9 $$\n".into()).unwrap();
        assert_eq!(s.exited(), json!({"type": "exit", "code": null, "signal": 9}));

        let id = open(&t);
        let s = Seen::follow(&t, &id);
        t.input(id.clone(), "exec /bin/sh -c \"trap '' HUP; echo TRAPPED; while :; do sleep 1; done\"\n".into()).unwrap();
        s.until("trap set", WAIT, |log| log.0.contains("TRAPPED\r\n"));
        let at = Instant::now();
        assert_eq!(t.close(id.clone()), json!({"closed": &id}));
        assert_eq!(failed(t.resize(id, 80.0, 24.0)), missing());
        assert_eq!(s.exited(), json!({"type": "exit", "code": null, "signal": 9}));
        assert!((2500..5000).contains(&at.elapsed().as_millis()), "SIGKILL about 3 s after the hang-up");
    }

    #[test]
    fn the_shell_is_the_account_login_shell() {
        let shell = login_shell();
        assert!(shell.starts_with('/'), "{shell}");
        if let Some(account) = account_shell().filter(|s| access(s.as_str(), Access::EXEC_OK).is_ok()) {
            assert_eq!(shell, account);
        }
    }
}
