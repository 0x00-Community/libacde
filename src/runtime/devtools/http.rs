use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::time::Instant;

use serde_json::{Value, json};

use super::ws::{Message, Messages};
use super::{Client, Endpoint, VERSION};
use crate::runtime::sync::Locked;

const HEAD: usize = 16 << 10;
const FRONTEND_ORIGIN: &str = "devtools://devtools";

pub(super) fn accept(endpoint: &'static Endpoint, listener: TcpListener) {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                log::warn!("devtools: accept: {e}");
                continue;
            }
        };
        if endpoint.connections.fetch_add(1, Ordering::AcqRel) >= endpoint.limit {
            endpoint.connections.fetch_sub(1, Ordering::AcqRel);
            log::warn!("devtools: more than {} connections; refusing one", endpoint.limit);
            continue;
        }
        let spawned = std::thread::Builder::new().name("acde-devtools-conn".into()).spawn(move || {
            if let Err(e) = serve(endpoint, stream) {
                log::debug!("devtools: connection: {e}");
            }
            endpoint.connections.fetch_sub(1, Ordering::AcqRel);
        });
        if let Err(e) = spawned {
            endpoint.connections.fetch_sub(1, Ordering::AcqRel);
            log::warn!("devtools connection thread: {e}");
        }
    }
}

struct Deadline {
    stream: TcpStream,
    until: Option<Instant>,
}

impl Deadline {
    fn lift(&mut self) -> io::Result<()> {
        self.until = None;
        self.stream.set_read_timeout(None)
    }
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(until) = self.until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "the request head took too long"));
            }
            self.stream.set_read_timeout(Some(left))?;
        }
        self.stream.read(buf)
    }
}

fn local_host(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split_once(']').filter(|(_, port)| port.is_empty() || port.strip_prefix(':').is_some_and(numeric)).map(|(name, _)| name),
        None => match host.rsplit_once(':') {
            Some((name, port)) if numeric(port) => Some(name),
            Some(_) => None,
            None => Some(host),
        },
    };
    name.is_some_and(|name| name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost") || name == "::1")
}

fn numeric(port: &str) -> bool {
    !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    fn upgrade(&self) -> Option<&str> {
        let websocket = self.header("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
        let version = self.header("sec-websocket-version") == Some("13");
        self.header("sec-websocket-key").filter(|_| websocket && version)
    }

    fn origin_allowed(&self) -> bool {
        self.header("origin").is_none_or(|origin| origin == FRONTEND_ORIGIN)
    }
}

fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn encode(text: &str) -> String {
    text.bytes().map(|b| if unreserved(b) { char::from(b).to_string() } else { format!("%{b:02X}") }).collect()
}

fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn read_head(reader: &mut impl BufRead) -> io::Result<Option<Request>> {
    let mut lines = Vec::new();
    let mut size = 0;
    loop {
        let mut line = String::new();
        let n = (&mut *reader).take((HEAD - size + 1) as u64).read_line(&mut line)?;
        size += n;
        if n == 0 || size > HEAD {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']).to_owned();
        if line.is_empty() {
            break;
        }
        lines.push(line);
    }
    let mut first = lines.first().map(|l| l.split(' ')).into_iter().flatten();
    let (Some(method), Some(target)) = (first.next(), first.next()) else { return Ok(None) };
    let target = target.split('#').next().unwrap_or_default();
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    let headers =
        lines[1..].iter().filter_map(|l| l.split_once(':')).map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned())).collect();
    Ok(Some(Request { method: method.to_owned(), path: path.to_owned(), headers }))
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=UTF-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

#[derive(Debug, PartialEq)]
enum Route {
    Version,
    List,
    Page(String),
    Browser(String),
    Missing,
}

fn route(path: &str) -> Route {
    let one = |id: &str| (!id.is_empty() && !id.contains('/')).then(|| decode(id)).flatten().filter(|id| !id.is_empty());
    match path {
        "/json/version" => Route::Version,
        "/json" | "/json/list" => Route::List,
        _ => match (path.strip_prefix("/devtools/page/").and_then(one), path.strip_prefix("/devtools/browser/").and_then(one)) {
            (Some(id), _) => Route::Page(id),
            (_, Some(id)) => Route::Browser(id),
            _ => Route::Missing,
        },
    }
}

impl Endpoint {
    fn socket(&self, path: &str) -> String {
        format!("127.0.0.1:{}{path}", self.port)
    }

    fn version(&self) -> Value {
        json!({
            "Browser": format!("acde/{VERSION}"),
            "Protocol-Version": "1.3",
            "User-Agent": format!("acde/{VERSION}"),
            "webSocketDebuggerUrl": format!("ws://{}", self.socket(&format!("/devtools/browser/{}", self.browser))),
        })
    }

    fn list(&self) -> Value {
        let targets = self.targets.locked().clone();
        targets
            .iter()
            .map(|t| {
                let socket = self.socket(&format!("/devtools/page/{}", encode(&t.id)));
                json!({
                    "description": "",
                    "devtoolsFrontendUrl": format!("devtools://devtools/bundled/devtools_app.html?ws={}", encode(&socket)),
                    "id": t.id,
                    "title": t.title,
                    "type": t.kind,
                    "url": t.url,
                    "webSocketDebuggerUrl": format!("ws://{socket}"),
                })
            })
            .collect()
    }
}

fn serve(endpoint: &'static Endpoint, mut stream: TcpStream) -> io::Result<()> {
    let mut reader = BufReader::new(Deadline { stream: stream.try_clone()?, until: Some(Instant::now() + endpoint.head_wait) });
    let Some(request) = read_head(&mut reader)? else {
        return respond(&mut stream, "400 Bad Request", r#"{"error":"bad request"}"#);
    };
    if !request.header("host").is_some_and(local_host) {
        return respond(&mut stream, "400 Bad Request", r#"{"error":"the Host is not this machine"}"#);
    }
    if !request.origin_allowed() {
        return respond(&mut stream, "403 Forbidden", r#"{"error":"requests from web pages may not use this endpoint"}"#);
    }
    if request.method != "GET" {
        return respond(&mut stream, "405 Method Not Allowed", r#"{"error":"only GET"}"#);
    }
    let target = match route(&request.path) {
        Route::Version => return respond(&mut stream, "200 OK", &endpoint.version().to_string()),
        Route::List => return respond(&mut stream, "200 OK", &endpoint.list().to_string()),
        Route::Missing => return respond(&mut stream, "404 Not Found", r#"{"error":"not found"}"#),
        Route::Browser(id) if id == endpoint.browser => None,
        Route::Browser(_) => return respond(&mut stream, "404 Not Found", r#"{"error":"no such browser"}"#),
        Route::Page(id) => match endpoint.target(&id) {
            Some(target) => Some(target),
            None => return respond(&mut stream, "404 Not Found", &json!({"error": format!("no target {id}")}).to_string()),
        },
    };
    let Some(key) = request.upgrade() else {
        return respond(&mut stream, "400 Bad Request", r#"{"error":"a WebSocket upgrade is required"}"#);
    };
    let accept = ntex::ws::hash_key(key.as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let answer = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n");
    stream.write_all(answer.as_bytes())?;
    reader.get_mut().lift()?;
    stream.set_read_timeout(None)?;
    session(endpoint, Client::new(target, stream)?, reader)
}

fn session(endpoint: &'static Endpoint, client: Client, mut reader: impl Read) -> io::Result<()> {
    endpoint.attach(&client);
    let mut messages = Messages::default();
    let result = loop {
        match messages.next(&mut reader) {
            Ok(Message::Text(text)) => handle(endpoint, &client, &text),
            Ok(Message::Ping(payload)) => client.pong(&payload),
            Ok(Message::Binary) => break Ok(()),
            Ok(Message::Close(payload)) => {
                client.answer_close(&payload);
                break Ok(());
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break Ok(()),
            Err(e) => break Err(e),
        }
    };
    endpoint.detach(&client);
    client.close();
    result
}

fn handle(endpoint: &Endpoint, client: &Client, text: &str) {
    let Ok(Value::Object(message)) = serde_json::from_str::<Value>(text) else {
        client.send(&json!({"id": null, "error": {"code": -32700, "message": "Message must be a valid JSON"}}).to_string());
        return;
    };
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let mut answer = match message.get("method").and_then(Value::as_str) {
        Some("Browser.getVersion") => json!({"id": id, "result": {
            "protocolVersion": "1.3",
            "product": format!("acde/{VERSION}"),
            "revision": "",
            "userAgent": format!("acde/{VERSION}"),
            "jsVersion": "",
        }}),
        Some(_) if endpoint.forwarded(client, text) => return,
        Some(method) => json!({"id": id, "error": {"code": -32601, "message": format!("'{method}' wasn't found")}}),
        None => json!({"id": id, "error": {"code": -32600, "message": "Message must have string 'method' property"}}),
    };
    if let Some(session) = message.get("sessionId") {
        answer["sessionId"] = session.clone();
    }
    client.send(&answer.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_follow_chrome() {
        assert_eq!(route("/json/version"), Route::Version);
        assert_eq!(route("/json"), Route::List);
        assert_eq!(route("/json/list"), Route::List);
        assert_eq!(route("/devtools/page/alpha%237"), Route::Page("alpha#7".into()));
        assert_eq!(route("/devtools/page/desktop"), Route::Page("desktop".into()));
        assert_eq!(route("/devtools/browser/b"), Route::Browser("b".into()));
        assert_eq!(route("/devtools/page/"), Route::Missing);
        assert_eq!(route("/devtools/page/a/b"), Route::Missing);
        assert_eq!(route("/devtools/page/%2"), Route::Missing);
        assert_eq!(route("/json/protocol"), Route::Missing);
    }

    #[test]
    fn names_round_trip_through_a_url() {
        assert_eq!(encode("alpha#7"), "alpha%237");
        assert_eq!(decode(&encode("a b/c?d=e#f")).as_deref(), Some("a b/c?d=e#f"));
        assert_eq!(decode(&encode(&encode("x#1"))).as_deref(), Some("x%231"));
    }

    #[test]
    fn only_requests_without_an_origin_or_from_the_devtools_front_end_pass() {
        let request = |headers: &[(&str, &str)]| {
            let head: String = headers.iter().map(|(n, v)| format!("{n}: {v}\r\n")).collect();
            read_head(&mut BufReader::new(format!("GET /json/list HTTP/1.1\r\n{head}\r\n").as_bytes())).unwrap().unwrap()
        };
        assert!(request(&[]).origin_allowed());
        assert!(request(&[("Origin", "devtools://devtools")]).origin_allowed());
        for origin in ["https://example.com", "http://127.0.0.1:8080", "null", "devtools://devtools.evil", "DEVTOOLS://DEVTOOLS", ""] {
            assert!(!request(&[("Origin", origin)]).origin_allowed(), "{origin}");
        }
    }

    #[test]
    fn a_head_is_read_to_its_blank_line() {
        let raw = "GET /json/list?x=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: WebSocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: k\r\n\r\nrest";
        let mut reader = BufReader::new(raw.as_bytes());
        let request = read_head(&mut reader).unwrap().unwrap();
        assert_eq!((request.method.as_str(), request.path.as_str()), ("GET", "/json/list"));
        assert_eq!(request.upgrade(), Some("k"));
        let mut rest = String::new();
        reader.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "rest");
        assert!(read_head(&mut BufReader::new(format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(HEAD)).as_bytes())).unwrap().is_none());
    }

    #[test]
    fn only_this_machine_may_be_the_host() {
        for host in ["127.0.0.1", "127.0.0.1:9222", "localhost", "LOCALHOST:1", "[::1]", "[::1]:9222"] {
            assert!(local_host(host), "{host}");
        }
        for host in ["", "evil.example", "evil.example:9222", "127.0.0.1.evil.example", "localhost:", "localhost:x", "[::1]x", "::1", "[::2]:1"] {
            assert!(!local_host(host), "{host}");
        }
    }

    fn served(limit: usize, head_wait: std::time::Duration) -> (&'static Endpoint, std::net::SocketAddr) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let endpoint: &'static Endpoint = Box::leak(Box::new(Endpoint::new(listener, limit, head_wait).unwrap()));
        endpoint.serve();
        (endpoint, address)
    }

    fn answer(address: std::net::SocketAddr, head: &str) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(head.as_bytes()).unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn a_foreign_host_gets_400_before_the_origin_is_looked_at() {
        let (_, address) = served(4, std::time::Duration::from_secs(5));
        let port = address.port();
        let foreign = answer(address, &format!("GET /json/list HTTP/1.1\r\nHost: evil.example:{port}\r\nOrigin: devtools://devtools\r\n\r\n"));
        assert!(foreign.starts_with("HTTP/1.1 400 "), "{foreign}");
        let missing = answer(address, "GET /json/list HTTP/1.1\r\n\r\n");
        assert!(missing.starts_with("HTTP/1.1 400 "), "{missing}");
        let local = answer(address, &format!("GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"));
        assert!(local.starts_with("HTTP/1.1 200 "), "{local}");
    }

    #[test]
    fn a_web_page_origin_gets_403_on_http_and_websocket_requests() {
        let (endpoint, address) = served(4, std::time::Duration::from_secs(5));
        let target = crate::runtime::devtools::DevToolsTarget { id: "alpha#7".into(), kind: "page".into(), title: "Alpha".into(), url: "acde://alpha/".into() };
        endpoint.set_targets(vec![target]);
        let port = address.port();
        let host = format!("Host: 127.0.0.1:{port}\r\n");
        let upgrade = "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n";
        let page = answer(address, &format!("GET /json/list HTTP/1.1\r\n{host}Origin: https://example.com\r\n\r\n"));
        assert!(page.starts_with("HTTP/1.1 403 "), "{page}");
        let socket = answer(address, &format!("GET /devtools/page/alpha%237 HTTP/1.1\r\n{host}{upgrade}Origin: https://example.com\r\n\r\n"));
        assert!(socket.starts_with("HTTP/1.1 403 "), "{socket}");
        let front_end = answer(address, &format!("GET /json/version HTTP/1.1\r\n{host}Origin: devtools://devtools\r\n\r\n"));
        assert!(front_end.starts_with("HTTP/1.1 200 "), "{front_end}");
        let tool = answer(address, &format!("GET /json/list HTTP/1.1\r\n{host}\r\n"));
        assert!(tool.starts_with("HTTP/1.1 200 "), "{tool}");
        let list: Value = serde_json::from_str(tool.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        let socket = format!("127.0.0.1:{port}/devtools/page/alpha%237");
        assert_eq!(list[0]["webSocketDebuggerUrl"], format!("ws://{socket}"));
        let frontend = list[0]["devtoolsFrontendUrl"].as_str().unwrap();
        assert_eq!(frontend, format!("devtools://devtools/bundled/devtools_app.html?ws={}", encode(&socket)));
        let ws = decode(frontend.split_once("?ws=").unwrap().1).unwrap();
        assert_eq!(ws, socket);
        assert!(ws.ends_with("/devtools/page/alpha%237"), "the page id stays encoded after one decoding: {ws}");
        let mut opened = TcpStream::connect(address).unwrap();
        let head = format!("GET /devtools/page/alpha%237 HTTP/1.1\r\n{host}{upgrade}Origin: devtools://devtools\r\n\r\n");
        opened.write_all(head.as_bytes()).unwrap();
        opened.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut status = [0u8; 12];
        opened.read_exact(&mut status).unwrap();
        assert_eq!(&status, b"HTTP/1.1 101");
    }

    #[test]
    fn an_open_socket_idle_past_the_head_deadline_still_answers() {
        let (endpoint, address) = served(4, std::time::Duration::from_millis(200));
        let target = crate::runtime::devtools::DevToolsTarget { id: "alpha#7".into(), kind: "page".into(), title: "Alpha".into(), url: "acde://alpha/".into() };
        endpoint.set_targets(vec![target]);
        let port = address.port();
        let mut socket = TcpStream::connect(address).unwrap();
        let head = format!(
            "GET /devtools/page/alpha%237 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        );
        socket.write_all(head.as_bytes()).unwrap();
        socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("HTTP/1.1 101"), "{line}");
        while line != "\r\n" {
            line.clear();
            reader.read_line(&mut line).unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(600));
        let request = br#"{"id":1,"method":"Browser.getVersion"}"#;
        let mut frame = vec![0x81, 0x80 | request.len() as u8, 0, 0, 0, 0];
        frame.extend_from_slice(request);
        socket.write_all(&frame).unwrap();
        let mut head = [0u8; 2];
        reader.read_exact(&mut head).expect("an answer after idling past the head deadline");
        let length = match head[1] & 0x7f {
            126 => {
                let mut n = [0u8; 2];
                reader.read_exact(&mut n).unwrap();
                usize::from(u16::from_be_bytes(n))
            }
            n => usize::from(n),
        };
        let mut payload = vec![0u8; length];
        reader.read_exact(&mut payload).unwrap();
        assert_eq!(head[0], 0x81, "a text frame, not a close");
        let answer: Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(answer["id"], 1);
        assert!(answer["result"]["product"].is_string(), "{answer}");
    }

    #[test]
    fn connections_over_the_limit_are_closed() {
        let (_, address) = served(2, std::time::Duration::from_secs(5));
        let held: Vec<TcpStream> = (0..2).map(|_| TcpStream::connect(address).unwrap()).collect();
        let mut over = TcpStream::connect(address).unwrap();
        over.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        let started = Instant::now();
        let mut byte = [0u8; 1];
        let read = over.read(&mut byte);
        assert!(matches!(read, Ok(0)) || read.as_ref().is_err_and(|e| e.kind() != io::ErrorKind::WouldBlock && e.kind() != io::ErrorKind::TimedOut));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        drop(held);
    }

    #[test]
    fn a_slow_head_is_cut_at_the_deadline() {
        let (_, address) = served(4, std::time::Duration::from_millis(300));
        let mut stream = TcpStream::connect(address).unwrap();
        let mut trickle = stream.try_clone().unwrap();
        std::thread::spawn(move || {
            _ = trickle.write_all(b"GET /json/list HTTP/1.1\r\n");
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if trickle.write_all(b"X: y\r\n").is_err() {
                    break;
                }
            }
        });
        stream.set_read_timeout(Some(std::time::Duration::from_secs(4))).unwrap();
        let started = Instant::now();
        let mut rest = Vec::new();
        let read = stream.read_to_end(&mut rest);
        assert!(read.is_ok() || read.as_ref().is_err_and(|e| e.kind() != io::ErrorKind::WouldBlock && e.kind() != io::ErrorKind::TimedOut));
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "cut after {:?}", started.elapsed());
    }
}
