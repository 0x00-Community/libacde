use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::FileExt;

use rustix::io::Errno;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, SocketFlags,
    SocketType, recvmsg, sendmsg, socketpair,
};
use serde_json::{Value, json};

pub const API_LEVEL: u32 = parse_level(include_str!("../../acde-api/api-level"));
pub const MIN_API_LEVEL: u32 = parse_level(include_str!("../../acde-api/min-api-level"));

pub fn speaks(level: u64) -> bool {
    (u64::from(MIN_API_LEVEL)..=u64::from(API_LEVEL)).contains(&level)
}

pub fn levels() -> String {
    if MIN_API_LEVEL == API_LEVEL { format!("level {API_LEVEL}") } else { format!("levels {MIN_API_LEVEL} to {API_LEVEL}") }
}

const fn parse_level(text: &str) -> u32 {
    let bytes = text.as_bytes();
    let mut level = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            level = level * 10 + (bytes[i] - b'0') as u32;
        }
        i += 1;
    }
    level
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

impl Refusal {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self { status: 400, code: "bad_request", message: message.into(), retry_after_ms: None }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self { status: 403, code: "forbidden", message: message.into(), retry_after_ms: None }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self { status: 404, code: "not_found", message: message.into(), retry_after_ms: None }
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self { status: 409, code, message: message.into(), retry_after_ms: None }
    }

    pub fn unsupported(service: &str, method: &str) -> Self {
        Self { status: 501, code: "unsupported", message: format!("{service}.{method} is not supported by the session's manager yet"), retry_after_ms: None }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self { status: 503, code: "unavailable", message: message.into(), retry_after_ms: None }
    }

    pub fn to_json(&self) -> Value {
        let mut refusal = json!({ "status": self.status, "code": self.code, "message": self.message });
        if let Some(ms) = self.retry_after_ms {
            refusal["retryAfterMs"] = json!(ms);
            refusal["retryAfter"] = json!(ms.div_ceil(1000).max(1));
        }
        refusal
    }
}

#[derive(Debug, PartialEq)]
pub enum Frame {
    Hello { api: u32, pid: u32, dev_key: Option<String> },
    Call { id: u64, service: String, method: String, args: Vec<Value> },
    Answer { id: u64, result: Result<Value, Value> },
    Ready { provides: serde_json::Map<String, Value> },
    Invoke { function: u64, args: Value },
    Release { function: u64 },
    DevResponse { id: u64, ok: bool, payload: String },
    DevEvent { text: String },
    DevTools { port: u64 },
    DevInject { event: Value },
    DevWindow { change: Value },
    Panels { user_only: Vec<[i32; 4]> },
}

impl Frame {
    pub fn parse(value: Value) -> Result<Self, String> {
        if let Some(hello) = value.get("hello") {
            let api = hello.get("api").and_then(Value::as_u64).ok_or("hello needs api")?;
            let pid = hello.get("pid").and_then(Value::as_u64).unwrap_or(0);
            let dev_key = hello.get("dev_key").and_then(Value::as_str).map(String::from);
            return Ok(Frame::Hello { api: api as u32, pid: pid as u32, dev_key });
        }
        if let Some(ready) = value.get("ready") {
            let provides = ready.get("provides").and_then(Value::as_object).cloned().ok_or("ready needs provides")?;
            return Ok(Frame::Ready { provides });
        }
        let function = |frame: &Value| frame.get("fn").and_then(Value::as_u64).ok_or("needs a numeric fn");
        if let Some(invoke) = value.get("invoke") {
            return Ok(Frame::Invoke { function: function(invoke)?, args: invoke.get("args").cloned().unwrap_or(Value::Array(Vec::new())) });
        }
        if let Some(release) = value.get("release") {
            return Ok(Frame::Release { function: function(release)? });
        }
        if let Some(response) = value.get("devresponse") {
            let id = response.get("id").and_then(Value::as_u64).ok_or("devresponse needs a numeric id")?;
            let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let payload = response.get("payload").and_then(Value::as_str).unwrap_or_default().to_string();
            return Ok(Frame::DevResponse { id, ok, payload });
        }
        if let Some(text) = value.get("devevent") {
            return Ok(Frame::DevEvent { text: text.as_str().ok_or("devevent is JSON text")?.to_string() });
        }
        if let Some(change) = value.get("devwindow") {
            return Ok(Frame::DevWindow { change: change.clone() });
        }
        if let Some(event) = value.get("devinject") {
            return Ok(Frame::DevInject { event: event.clone() });
        }
        if let Some(panels) = value.get("panels") {
            let rects = panels.get("user_only").and_then(Value::as_array).ok_or("panels needs user_only")?;
            let rect = |r: &Value| -> Option<[i32; 4]> {
                let r = r.as_array().filter(|r| r.len() == 4)?;
                let n = |i: usize| r[i].as_i64().and_then(|v| i32::try_from(v).ok());
                Some([n(0)?, n(1)?, n(2)?, n(3)?])
            };
            let user_only = rects.iter().map(rect).collect::<Option<Vec<_>>>().ok_or("panels: each rect is [x, y, width, height]")?;
            return Ok(Frame::Panels { user_only });
        }
        if let Some(devtools) = value.get("devtools") {
            return Ok(Frame::DevTools { port: devtools.get("port").and_then(Value::as_u64).ok_or("devtools needs a port")? });
        }
        let id = value.get("id").and_then(Value::as_u64).ok_or("a call needs a numeric id")?;
        if value.get("service").is_none() {
            if let Some(error) = value.get("error") {
                return Ok(Frame::Answer { id, result: Err(error.clone()) });
            }
            if let Some(ok) = value.get("ok") {
                return Ok(Frame::Answer { id, result: Ok(ok.clone()) });
            }
        }
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(String::from).ok_or_else(|| format!("call {id} needs {key}"));
        let args = match value.get("args") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(args)) => args.clone(),
            Some(_) => return Err(format!("call {id}: args is an array")),
        };
        Ok(Frame::Call { id, service: text("service")?, method: text("method")?, args })
    }
}

pub fn answer(id: u64, result: Result<Value, Refusal>) -> Value {
    match result {
        Ok(ok) => json!({ "id": id, "ok": ok }),
        Err(refusal) => json!({ "id": id, "error": refusal.to_json() }),
    }
}

pub fn event(event: Value) -> Value {
    json!({ "event": event })
}

pub fn pair() -> Result<(OwnedFd, OwnedFd), String> {
    socketpair(AddressFamily::UNIX, SocketType::SEQPACKET, SocketFlags::CLOEXEC, None).map_err(|e| format!("the manager's channel: {e}"))
}

pub struct Packet {
    bytes: Vec<u8>,
    fds: Vec<OwnedFd>,
    size: usize,
}

impl Packet {
    pub fn size(&self) -> usize {
        self.size
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Received {
    Frame(Value),
    Nothing,
    Closed,
}

pub struct Channel {
    fd: OwnedFd,
}

impl Channel {
    pub const FRAME_MAX: usize = 256 * 1024;
    pub const FDS_MAX: usize = 64;
    pub const BIG_MAX: usize = 64 << 20;
    const INLINE_MAX: usize = 128 * 1024;
    const BIG: &str = "@memfd";

    pub fn new(fd: OwnedFd) -> Self {
        Self { fd }
    }

    pub fn try_clone(&self) -> Result<Self, String> {
        self.fd.try_clone().map(Self::new).map_err(|e| format!("the manager's channel: {e}"))
    }

    pub fn send(&self, frame: &Value, wait: bool) -> Result<(), String> {
        let packet = Self::pack(frame)?;
        self.send_bytes(&packet.bytes, &borrowed(&packet.fds), wait)
    }

    pub fn pack(frame: &Value) -> Result<Packet, String> {
        let bytes = serde_json::to_vec(frame).map_err(|e| e.to_string())?;
        if bytes.len() <= Self::INLINE_MAX {
            return Ok(Packet { size: bytes.len(), bytes, fds: Vec::new() });
        }
        if bytes.len() > Self::BIG_MAX {
            return Err(format!("a frame of {} bytes is over {}", bytes.len(), Self::BIG_MAX));
        }
        let memfd = rustix::fs::memfd_create("acde-frame", rustix::fs::MemfdFlags::CLOEXEC).map_err(|e| format!("a big frame: {e}"))?;
        let file = std::fs::File::from(memfd);
        file.write_all_at(&bytes, 0).map_err(|e| format!("a big frame: {e}"))?;
        let head = serde_json::to_vec(&json!({ Self::BIG: bytes.len() })).map_err(|e| e.to_string())?;
        Ok(Packet { bytes: head, fds: vec![OwnedFd::from(file)], size: bytes.len() })
    }

    pub fn pack_with_fd(frame: &Value, fd: OwnedFd) -> Result<Packet, String> {
        Self::pack_with_fds(frame, vec![fd])
    }

    pub fn pack_with_fds(frame: &Value, fds: Vec<OwnedFd>) -> Result<Packet, String> {
        let bytes = Self::inline_with_fds(frame, fds.len())?;
        Ok(Packet { size: bytes.len(), bytes, fds })
    }

    fn inline_with_fds(frame: &Value, fds: usize) -> Result<Vec<u8>, String> {
        if fds > Self::FDS_MAX {
            return Err(format!("a frame carries at most {} fds, not {fds}", Self::FDS_MAX));
        }
        let bytes = serde_json::to_vec(frame).map_err(|e| e.to_string())?;
        if bytes.len() > Self::INLINE_MAX {
            return Err(format!("a frame with fds fits inline ({} bytes is over {})", bytes.len(), Self::INLINE_MAX));
        }
        Ok(bytes)
    }

    pub fn offer(&self, packet: &Packet) -> Result<bool, String> {
        match self.send_raw(&packet.bytes, &borrowed(&packet.fds), false) {
            Ok(()) => Ok(true),
            Err(Errno::AGAIN) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn send_with_fd(&self, frame: &Value, fd: BorrowedFd<'_>, wait: bool) -> Result<(), String> {
        self.send_with_fds(frame, &[fd], wait)
    }

    pub fn send_with_fds(&self, frame: &Value, fds: &[BorrowedFd<'_>], wait: bool) -> Result<(), String> {
        let bytes = Self::inline_with_fds(frame, fds.len())?;
        self.send_bytes(&bytes, fds, wait)
    }

    fn send_bytes(&self, bytes: &[u8], fds: &[BorrowedFd<'_>], wait: bool) -> Result<(), String> {
        self.send_raw(bytes, fds, wait).map_err(|e| e.to_string())
    }

    fn send_raw(&self, bytes: &[u8], fds: &[BorrowedFd<'_>], wait: bool) -> Result<(), Errno> {
        let flags = if wait { SendFlags::NOSIGNAL } else { SendFlags::NOSIGNAL | SendFlags::DONTWAIT };
        let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(Channel::FDS_MAX))];
        let mut ancillary = SendAncillaryBuffer::new(&mut space);
        if !fds.is_empty() && !ancillary.push(SendAncillaryMessage::ScmRights(fds)) {
            return Err(Errno::INVAL);
        }
        loop {
            match sendmsg(&self.fd, &[IoSlice::new(bytes)], &mut ancillary, flags) {
                Ok(_) => return Ok(()),
                Err(Errno::INTR) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn unpack(frame: Value, mut fds: Vec<OwnedFd>) -> Result<(Value, Vec<OwnedFd>), String> {
        let Some(length) = frame.get(Self::BIG).and_then(Value::as_u64).filter(|_| frame.as_object().is_some_and(|o| o.len() == 1)) else {
            return Ok((frame, fds));
        };
        let fd = (fds.len() == 1).then(|| fds.remove(0));
        let length =
            usize::try_from(length).ok().filter(|n| *n <= Self::BIG_MAX).ok_or_else(|| format!("a frame of {length} bytes is over {}", Self::BIG_MAX))?;
        let file = std::fs::File::from(fd.ok_or("a big frame came without its memfd")?);
        let mut bytes = vec![0u8; length];
        file.read_exact_at(&mut bytes, 0).map_err(|e| format!("a big frame: {e}"))?;
        serde_json::from_slice(&bytes).map(|frame| (frame, Vec::new())).map_err(|e| format!("not a JSON frame: {e}"))
    }

    pub fn receive(&self, wait: bool) -> Result<Received, String> {
        self.receive_with_fd(wait).map(|(received, _)| received)
    }

    pub fn receive_with_fd(&self, wait: bool) -> Result<(Received, Option<OwnedFd>), String> {
        self.receive_with_fds(wait).map(|(received, fds)| (received, fds.into_iter().next()))
    }

    pub fn receive_with_fds(&self, wait: bool) -> Result<(Received, Vec<OwnedFd>), String> {
        let mut buf = vec![0u8; Self::FRAME_MAX];
        let flags = if wait { RecvFlags::CMSG_CLOEXEC } else { RecvFlags::CMSG_CLOEXEC | RecvFlags::DONTWAIT };
        loop {
            let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(Channel::FDS_MAX))];
            let mut ancillary = RecvAncillaryBuffer::new(&mut space);
            let got = recvmsg(&self.fd, &mut [IoSliceMut::new(&mut buf)], &mut ancillary, flags);
            let fds: Vec<OwnedFd> = ancillary
                .drain()
                .flat_map(|message| match message {
                    RecvAncillaryMessage::ScmRights(fds) => fds.collect(),
                    _ => Vec::new(),
                })
                .collect();
            return match got {
                Ok(msg) if msg.bytes == 0 => Ok((Received::Closed, Vec::new())),
                Ok(msg) if msg.flags.contains(ReturnFlags::CTRUNC) => Err(format!("a frame came with more than {} fds", Self::FDS_MAX)),
                Ok(msg) if msg.flags.contains(ReturnFlags::TRUNC) => Err(format!("a frame over {} bytes came inline", Self::FRAME_MAX)),
                Ok(msg) => serde_json::from_slice(&buf[..msg.bytes])
                    .map_err(|e| format!("not a JSON frame: {e}"))
                    .and_then(|frame| Self::unpack(frame, fds))
                    .map(|(frame, fds)| (Received::Frame(frame), fds)),
                Err(Errno::INTR) => continue,
                Err(Errno::AGAIN) => Ok((Received::Nothing, Vec::new())),
                Err(Errno::CONNRESET) => Ok((Received::Closed, Vec::new())),
                Err(e) => Err(e.to_string()),
            };
        }
    }
}

impl AsFd for Channel {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

pub type FdKeeper = Box<dyn Fn(Vec<OwnedFd>) -> Vec<String> + Send + Sync>;

pub fn handed(keeper: Option<&FdKeeper>, mut value: Value, fds: Vec<OwnedFd>) -> Value {
    if let (false, Some(keeper), Some(object)) = (fds.is_empty(), keeper, value.as_object_mut()) {
        object.insert("fds".into(), json!(keeper(fds)));
    }
    value
}

fn borrowed(fds: &[OwnedFd]) -> Vec<BorrowedFd<'_>> {
    fds.iter().map(AsFd::as_fd).collect()
}

pub fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { DIGITS[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads_like_rfc_4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_frame_over_the_inline_size_goes_through_a_memfd() {
        let (a, b) = pair().unwrap();
        let (a, b) = (Channel::new(a), Channel::new(b));
        let catalog: Vec<Value> = (0..20_000).map(|i| json!({ "id": i, "name": format!("package {i}") })).collect();
        let big = json!({ "id": 1, "ok": catalog });
        a.send(&big, true).unwrap();
        a.send(&json!({ "id": 2, "ok": null }), true).unwrap();
        assert_eq!(b.receive(true).unwrap(), Received::Frame(big));
        assert_eq!(b.receive(true).unwrap(), Received::Frame(json!({ "id": 2, "ok": null })));
    }

    #[test]
    fn a_small_frame_carries_an_fd_across() {
        let (a, b) = pair().unwrap();
        let (a, b) = (Channel::new(a), Channel::new(b));
        let (ours, theirs) = rustix::net::socketpair(AddressFamily::UNIX, SocketType::STREAM, SocketFlags::CLOEXEC, None).unwrap();
        a.send_with_fd(&json!({ "id": 1, "ok": { "role": "desktop" } }), theirs.as_fd(), true).unwrap();
        drop(theirs);
        let (frame, fd) = b.receive_with_fd(true).unwrap();
        assert_eq!(frame, Received::Frame(json!({ "id": 1, "ok": { "role": "desktop" } })));
        let fd = fd.expect("the fd came with the frame");
        rustix::io::write(&fd, b"hi").unwrap();
        let mut got = [0u8; 2];
        rustix::io::read(&ours, &mut got).unwrap();
        assert_eq!(&got, b"hi");
        a.send(&json!({ "id": 2, "ok": null }), true).unwrap();
        assert!(b.receive_with_fd(true).unwrap().1.is_none(), "a frame without an fd brings none");
    }

    #[test]
    fn a_frame_carries_several_fds_in_order() {
        let (a, b) = pair().unwrap();
        let (a, b) = (Channel::new(a), Channel::new(b));
        let ends: Vec<(OwnedFd, OwnedFd)> =
            (0..3).map(|_| rustix::net::socketpair(AddressFamily::UNIX, SocketType::STREAM, SocketFlags::CLOEXEC, None).unwrap()).collect();
        let theirs: Vec<BorrowedFd<'_>> = ends.iter().map(|(_, theirs)| theirs.as_fd()).collect();
        a.send_with_fds(&json!({ "id": 1, "ok": { "files": 3 } }), &theirs, true).unwrap();
        let (frame, fds) = b.receive_with_fds(true).unwrap();
        assert_eq!(frame, Received::Frame(json!({ "id": 1, "ok": { "files": 3 } })));
        assert_eq!(fds.len(), 3);
        for (i, (fd, (ours, _))) in fds.iter().zip(&ends).enumerate() {
            rustix::io::write(fd, &[i as u8]).unwrap();
            let mut got = [0u8; 1];
            rustix::io::read(ours, &mut got).unwrap();
            assert_eq!(got[0], i as u8, "fd {i} keeps its place");
        }
        let many: Vec<BorrowedFd<'_>> = (0..=Channel::FDS_MAX).map(|_| theirs[0]).collect();
        assert!(a.send_with_fds(&json!({ "id": 2 }), &many, true).is_err());
    }

    #[test]
    fn fds_become_the_keepers_tokens_or_are_closed() {
        let keeper: FdKeeper = Box::new(|fds| fds.iter().enumerate().map(|(i, _)| format!("token-{i}")).collect());
        let (_, theirs) = rustix::net::socketpair(AddressFamily::UNIX, SocketType::STREAM, SocketFlags::CLOEXEC, None).unwrap();
        let kept = handed(Some(&keeper), json!({ "name": "work", "fd": 0 }), vec![theirs]);
        assert_eq!(kept, json!({ "name": "work", "fd": 0, "fds": ["token-0"] }));
        let (ours, theirs) = rustix::net::socketpair(AddressFamily::UNIX, SocketType::STREAM, SocketFlags::CLOEXEC, None).unwrap();
        let dropped = handed(None, json!({ "name": "work", "fd": 0 }), vec![theirs]);
        assert_eq!(dropped, json!({ "name": "work", "fd": 0 }), "no keeper, no fds");
        let mut got = [0u8; 1];
        assert_eq!(rustix::io::read(&ours, &mut got).unwrap(), 0, "the fd was closed");
    }
}
