use super::*;
use crate::runtime::http::frames;
use std::io::{BufRead, Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream as StdStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(windows)]
type StdStream = socket2::Socket;

struct Listener(#[cfg(unix)] std::os::unix::net::UnixListener, #[cfg(windows)] socket2::Socket);

impl Listener {
    #[cfg(unix)]
    fn bind(path: &Path) -> Self {
        Self(std::os::unix::net::UnixListener::bind(path).expect("bind"))
    }

    #[cfg(windows)]
    fn bind(path: &Path) -> Self {
        let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None).expect("socket");
        socket.bind(&socket2::SockAddr::unix(path).expect("address")).expect("bind");
        socket.listen(16).expect("listen");
        Self(socket)
    }

    fn accept(&self) -> std::io::Result<(StdStream, ())> {
        self.0.accept().map(|(stream, _)| (stream, ()))
    }
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("acde-fetch-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn missing(name: &str) -> String {
    let root = if cfg!(windows) { r"C:\nonexistent\acde" } else { "/nonexistent/acde" };
    Path::new(root).join(name).to_string_lossy().into_owned()
}

fn serve(name: &str, answer: impl FnOnce(StdStream) + Send + 'static) -> (String, std::thread::JoinHandle<()>) {
    let path = scratch().join(name);
    let listener = Listener::bind(&path);
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        answer(stream);
    });
    (path.to_string_lossy().into_owned(), server)
}

fn read_head(stream: &StdStream) -> (Vec<String>, std::io::BufReader<StdStream>) {
    let mut reader = std::io::BufReader::new(stream.try_clone().expect("clone"));
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("request line");
        let line = line.trim_end_matches(['\r', '\n']).to_owned();
        if line.is_empty() {
            return (lines, reader);
        }
        let lowered = line.split_once(':').filter(|_| !lines.is_empty()).map(|(name, value)| format!("{}:{value}", name.to_ascii_lowercase()));
        lines.push(lowered.unwrap_or(line));
    }
}

fn request(socket: &str, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Exchange {
    Exchange {
        to: Dial::Socket(socket.into()),
        method: method.into(),
        path: path.into(),
        host: "localhost".into(),
        headers: headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
        body: body.map(<[u8]>::to_vec),
    }
}

type Received = async_channel::Receiver<Event>;

fn channel() -> (async_channel::Sender<Event>, Received) {
    async_channel::bounded(8)
}

fn recv(events: &Received) -> Option<Event> {
    let events = events.clone();
    let received =
        crate::runtime::rt::block_on(crate::runtime::rt::local(move || async move { ntex::time::timeout(Duration::from_secs(10), events.recv()).await }));
    received.flatten().expect("the executor").expect("an event or the end in time").ok()
}

fn next(events: &Received) -> Event {
    recv(events).expect("an event")
}

#[test]
fn get_with_content_length() {
    let (socket, server) = serve("3000.sock", |mut stream| {
        let (lines, _) = read_head(&stream);
        assert_eq!(lines[0], "GET /api/x?y=1 HTTP/1.1");
        assert!(lines.contains(&"host: localhost".to_owned()), "{lines:?}");
        assert!(lines.contains(&"connection: close".to_owned()), "{lines:?}");
        assert!(lines.contains(&"accept: text/plain".to_owned()), "{lines:?}");
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-Thing: a b\r\n\r\nhello").expect("answer");
    });
    let (sender, events) = channel();
    let client = FetchClient::default();
    client.exchange(request(&socket, "GET", "/api/x?y=1", &[("Accept", "text/plain")], None), sender);
    let Event::Head { status, status_text, headers } = next(&events) else { panic!("head first") };
    assert_eq!((status, status_text.as_str()), (200, "OK"));
    assert!(headers.contains(&("x-thing".into(), "a b".into())));
    assert_eq!(next(&events), Event::Chunk(b"hello".to_vec()));
    assert_eq!(next(&events), Event::End);
    assert!(recv(&events).is_none(), "nothing after end");
    server.join().expect("server");
}

#[test]
fn remote_url_over_tcp() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut reader = std::io::BufReader::new(stream.try_clone().expect("clone"));
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("request line");
            let line = line.trim_end().to_ascii_lowercase();
            if line.is_empty() {
                break;
            }
            lines.push(line);
        }
        let mut body = [0u8; 2];
        reader.read_exact(&mut body).expect("body");
        assert_eq!(lines[0], "post /x?y=1 http/1.1");
        assert!(lines.contains(&format!("host: 127.0.0.1:{port}")), "{lines:?}");
        assert!(lines.contains(&"origin: http://acde.local".to_owned()), "{lines:?}");
        assert_eq!(&body, b"hi");
        stream.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").expect("answer");
    });
    let exchange = Exchange::new(
        Dial::Url(format!("http://127.0.0.1:{port}/x?y=1#frag")),
        "POST".into(),
        String::new(),
        vec![vec!["Origin".into(), "http://acde.local".into()]],
        Some(b"hi".to_vec()),
    )
    .expect("a valid request");
    let (sender, events) = channel();
    let client = FetchClient::default();
    client.exchange(exchange, sender);
    let Event::Head { status, .. } = next(&events) else { panic!("head first") };
    assert_eq!(status, 201);
    assert_eq!(next(&events), Event::Chunk(b"ok".to_vec()));
    assert_eq!(next(&events), Event::End);
    server.join().expect("server");
}

#[test]
fn remote_urls_are_checked() {
    let exchange = |url: &str| Exchange::new(Dial::Url(url.into()), "GET".into(), String::new(), vec![], None).err();
    assert!(exchange("ftp://example.com/x").is_some());
    assert!(exchange("http://user:pw@example.com/").is_some());
    assert!(exchange("not a url").is_some());
    assert!(exchange("https://example.com/a b").is_none());
    let upgrade = |url: &str| Upgrade::new(Dial::Url(url.into()), String::new(), vec![], vec![]).err();
    assert!(upgrade("https://example.com/ws").is_some());
    assert!(upgrade("wss://example.com/ws").is_none());
}

/// a client that reads no proxy setting: everything direct
fn unproxied() -> FetchClient {
    FetchClient::with_proxies(std::sync::Arc::new(crate::runtime::proxy::Fixed(vec![crate::runtime::proxy::Hop::Direct])))
}

/// a local port nothing listens on
fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("address").port()
}

fn remote(url: &str) -> Exchange {
    Exchange::new(Dial::Url(url.into()), "GET".into(), String::new(), vec![], None).expect("a valid request")
}

#[test]
fn a_refused_connection_is_named() {
    let (sender, events) = channel();
    let client = unproxied();
    client.exchange(remote(&format!("http://127.0.0.1:{}/", closed_port())), sender);
    let Event::Error(message) = next(&events) else { panic!("an error") };
    assert!(message.to_ascii_lowercase().contains("refused"), "{message}");

    let (sender, events) = channel();
    client.upgrade(Upgrade::new(Dial::Url(format!("ws://127.0.0.1:{}/", closed_port())), String::new(), vec![], vec![]).expect("valid"), sender);
    let Event::Error(message) = next(&events) else { panic!("an error") };
    assert!(message.to_ascii_lowercase().contains("refused"), "{message}");
    assert!(matches!(next(&events), Event::Close { code: 1006, clean: false, .. }));
}

/// a TLS server with a self-signed certificate for `localhost`, which no client trusts
fn untrusted() -> (u16, std::thread::JoinHandle<()>) {
    let (config, _) = crate::runtime::proxy::tests::localhost_tls();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut tls = rustls::StreamOwned::new(rustls::ServerConnection::new(config).expect("session"), stream);
        _ = tls.read(&mut [0u8; 1]);
    });
    (port, server)
}

#[test]
fn an_untrusted_certificate_is_named() {
    let (port, server) = untrusted();
    let (sender, events) = channel();
    let client = unproxied();
    client.exchange(remote(&format!("https://127.0.0.1:{port}/")), sender);
    let Event::Error(message) = next(&events) else { panic!("an error") };
    assert!(message.contains("certificate"), "{message}");
    server.join().expect("server");
}

#[test]
fn chunked_body_arrives_in_pieces() {
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let (socket, server) = serve("0.sock", move |mut stream| {
        read_head(&stream);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").expect("head");
        stream.write_all(b"d\r\ndata: first\n\n\r\n").expect("first");
        wait.recv().expect("go");
        stream.write_all(b"e;ext=1\r\ndata: second\n\n\r\n0\r\nX-Trailer: t\r\n\r\n").expect("second");
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).ok();
    });
    let (sender, events) = channel();
    let client = FetchClient::default();
    client.exchange(request(&socket, "GET", "/events", &[], None), sender);
    assert!(matches!(next(&events), Event::Head { status: 200, .. }));
    assert_eq!(next(&events), Event::Chunk(b"data: first\n\n".to_vec()));
    go.send(()).expect("go");
    assert_eq!(next(&events), Event::Chunk(b"data: second\n\n".to_vec()));
    assert_eq!(next(&events), Event::End);
    server.join().expect("server");
}

#[test]
fn post_body_and_read_to_close() {
    let (socket, server) = serve("8080.sock", |mut stream| {
        let (lines, mut reader) = read_head(&stream);
        assert_eq!(lines[0], "POST /api/echo HTTP/1.1");
        assert!(lines.contains(&"host: localhost:8080".to_owned()), "{lines:?}");
        let length: usize = lines.iter().find_map(|l| l.strip_prefix("content-length: ")).expect("length").parse().expect("number");
        let mut body = vec![0; length];
        reader.read_exact(&mut body).expect("body");
        stream.write_all(b"HTTP/1.1 201 Created\r\n\r\n").expect("head");
        stream.write_all(&body).expect("echo");
    });
    let (sender, events) = channel();
    let mut exchange = request(&socket, "POST", "/api/echo", &[("Content-Type", "application/json")], Some(br#"{"a":1}"#));
    exchange.host = "localhost:8080".into();
    let client = FetchClient::default();
    client.exchange(exchange, sender);
    assert!(matches!(next(&events), Event::Head { status: 201, .. }));
    let mut body = Vec::new();
    loop {
        match next(&events) {
            Event::Chunk(data) => body.extend(data),
            Event::End => break,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(body, br#"{"a":1}"#);
    server.join().expect("server");
}

#[test]
fn abort_drops_the_connection() {
    let (closed, seen) = std::sync::mpsc::channel();
    let (socket, server) = serve("1.sock", move |mut stream| {
        read_head(&stream);
        stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").expect("head");
        let mut rest = [0u8; 16];
        closed.send(stream.read(&mut rest).map(|n| n == 0).unwrap_or(true)).ok();
    });
    let connections = FetchClient::default();
    let (sender, events) = channel();
    let id = connections.exchange(request(&socket, "GET", "/slow", &[], None), sender);
    assert!(matches!(next(&events), Event::Head { status: 200, .. }));
    connections.abort(id);
    assert!(seen.recv_timeout(Duration::from_secs(10)).expect("server saw the end"));
    assert!(recv(&events).is_none(), "no event after abort");
    assert!(connections.open.locked().is_empty());
    server.join().expect("server");
}

#[test]
fn missing_socket_is_an_error_event() {
    let (sender, events) = channel();
    let client = FetchClient::default();
    let socket = missing("3000.sock");
    client.exchange(request(&socket, "GET", "/", &[], None), sender);
    let Event::Error(message) = next(&events) else { panic!("an error") };
    assert!(message.contains(&socket), "{message}");
    assert!(recv(&events).is_none());
}

#[test]
fn socket_bound_later_is_waited_for() {
    let path = scratch().join("3000.sock");
    let (sender, events) = channel();
    let client = FetchClient::default();
    client.exchange(request(&path.to_string_lossy(), "GET", "/", &[], None), sender);
    std::thread::sleep(Duration::from_millis(200));
    let listener = Listener::bind(&path);
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        read_head(&stream);
        stream.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").expect("answer");
    });
    assert!(matches!(next(&events), Event::Head { status: 204, .. }));
    assert_eq!(next(&events), Event::End);
    server.join().expect("server");
}

#[test]
fn many_streams_at_once() {
    let path = scratch().join("3000.sock");
    let listener = Listener::bind(&path);
    let server = std::thread::spawn(move || {
        let accept = |_| {
            let (mut stream, _) = listener.accept().expect("accept");
            read_head(&stream);
            stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").expect("head");
            stream
        };
        (0..12).map(accept).collect::<Vec<_>>()
    });
    let client = FetchClient::default();
    let streams: Vec<Received> = (0..12)
        .map(|_| {
            let (sender, events) = channel();
            client.exchange(request(&path.to_string_lossy(), "GET", "/events", &[], None), sender);
            events
        })
        .collect();
    for events in &streams {
        assert!(matches!(next(events), Event::Head { status: 200, .. }));
    }
    drop(server.join().expect("server"));
}

#[test]
fn early_end_is_an_error() {
    let (socket, server) = serve("2.sock", |mut stream| {
        read_head(&stream);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc").expect("short");
    });
    let (sender, events) = channel();
    let client = FetchClient::default();
    client.exchange(request(&socket, "GET", "/", &[], None), sender);
    assert!(matches!(next(&events), Event::Head { status: 200, .. }));
    assert_eq!(next(&events), Event::Chunk(b"abc".to_vec()));
    assert!(matches!(next(&events), Event::Error(_)));
    server.join().expect("server");
}

#[test]
fn options_are_checked() {
    let options = |headers: Vec<Vec<String>>, path: &str| Exchange::new(Dial::Socket(missing("x.sock")), "GET".into(), path.into(), headers, None);
    let pair = |n: &str, v: &str| vec![n.to_owned(), v.to_owned()];
    assert!(options(vec![pair("Bad Name", "x")], "/").is_err());
    assert!(options(vec![pair("X", "a\r\nInjected: 1")], "/").is_err());
    assert!(Exchange::new(Dial::Socket("acde/x.sock".into()), "GET".into(), "/".into(), vec![], None).is_err());
    assert!(options(vec![], "no-slash").is_err());
    assert!(options(vec![], "/a b").is_err());
    let exchange = options(vec![pair("Host", "localhost:3000"), pair("Content-Length", "9"), pair("X-A", "b")], "/").expect("valid");
    assert_eq!(exchange.host, "localhost:3000");
    assert_eq!(exchange.headers, vec![("X-A".to_owned(), "b".to_owned())]);
    assert!(close_frame(Some(1001), None).is_err());
    assert!(close_frame(Some(4000), Some("x".repeat(124))).is_err());
    assert!(close_frame(None, None).expect("no code").is_none());
    assert_eq!(close_frame(None, Some("bye".into())).expect("1000").map(|f| u16::from(f.code)), Some(1000));
}

fn upgrade(socket: &str, protocols: &[&str]) -> Upgrade {
    Upgrade {
        to: Dial::Socket(socket.into()),
        path: "/ws".into(),
        host: "localhost".into(),
        headers: vec![("Origin".into(), "http://acde.local".into())],
        protocols: protocols.iter().map(|p| p.to_string()).collect(),
    }
}

fn accept(stream: &mut StdStream, protocol: Option<&str>) -> (Vec<String>, std::io::BufReader<StdStream>) {
    let (lines, reader) = read_head(stream);
    assert_eq!(lines[0], "GET /ws HTTP/1.1");
    assert!(lines.contains(&"upgrade: websocket".to_owned()), "{lines:?}");
    let key = lines.iter().find_map(|l| l.strip_prefix("sec-websocket-key: ")).expect("key");
    let accept = ntex::ws::hash_key(key.as_bytes()).expect("key");
    let protocol = protocol.map(|p| format!("Sec-WebSocket-Protocol: {p}\r\n")).unwrap_or_default();
    let answer = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n{protocol}\r\n");
    stream.write_all(answer.as_bytes()).expect("handshake");
    (lines, reader)
}

#[test]
fn websocket_echo_and_client_close() {
    let (socket, server) = serve("3000.sock", |mut stream| {
        let (lines, mut reader) = accept(&mut stream, Some("v2"));
        assert!(lines.contains(&"origin: http://acde.local".to_owned()), "{lines:?}");
        assert!(lines.contains(&"sec-websocket-protocol: chat,v2".to_owned()), "{lines:?}");
        loop {
            match frames::read(&mut reader, true) {
                (true, opcode @ (1 | 2), data) => stream.write_all(&frames::write(true, opcode, &data, false)).expect("echo"),
                (true, 8, close) => {
                    stream.write_all(&frames::write(true, 8, &close, false)).expect("close");
                    return;
                }
                other => panic!("{other:?}"),
            }
        }
    });
    let connections = FetchClient::default();
    let (sender, events) = channel();
    let id = connections.upgrade(upgrade(&socket, &["chat", "v2"]), sender);
    connections.command(id, Command::Send(Message::Text("hello 你好".into())));
    assert_eq!(next(&events), Event::Open { protocol: "v2".into(), extensions: String::new() });
    assert_eq!(next(&events), Event::Text("hello 你好".into()));
    let big: Vec<u8> = (0..300_000u32).map(|i| i as u8).collect();
    connections.command(id, Command::Send(Message::Binary(big.clone().into())));
    assert_eq!(next(&events), Event::Binary(big));
    connections.command(id, Command::Close(close_frame(Some(4000), Some("done".into())).expect("frame")));
    assert_eq!(next(&events), Event::Close { code: 4000, reason: "done".into(), clean: true });
    assert!(recv(&events).is_none());
    server.join().expect("server");
}

#[test]
fn websocket_fragments_ping_and_server_close() {
    let (socket, server) = serve("4000.sock", |mut stream| {
        let mut answer = Vec::new();
        let (lines, mut reader) = {
            let (lines, reader) = read_head(&stream);
            assert!(!lines.iter().any(|l| l.starts_with("sec-websocket-protocol")), "no protocol offered");
            let key = lines.iter().find_map(|l| l.strip_prefix("sec-websocket-key: ")).expect("key");
            let accept = ntex::ws::hash_key(key.as_bytes()).expect("key");
            answer.extend(
                format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").into_bytes(),
            );
            (lines, reader)
        };
        assert_eq!(lines[0], "GET /ws HTTP/1.1");
        answer.extend(frames::write(false, 1, "frag".as_bytes(), false));
        stream.write_all(&answer).expect("handshake");
        stream.write_all(&frames::write(false, 0, "men".as_bytes(), false)).expect("continuation");
        stream.write_all(&frames::write(true, 0, "ted ✓".as_bytes(), false)).expect("last");
        stream.write_all(&frames::write(true, 9, b"ping!", false)).expect("ping");
        assert_eq!(frames::read(&mut reader, true), (true, 10, b"ping!".to_vec()), "pong with the ping's payload");
        let mut close = 4001u16.to_be_bytes().to_vec();
        close.extend_from_slice(b"bye");
        stream.write_all(&frames::write(true, 8, &close, false)).expect("close");
        assert_eq!(frames::read(&mut reader, true), (true, 8, close), "the close answered with its code");
        stream.shutdown(std::net::Shutdown::Both).ok();
    });
    let (sender, events) = channel();
    let client = FetchClient::default();
    client.upgrade(upgrade(&socket, &[]), sender);
    assert_eq!(next(&events), Event::Open { protocol: String::new(), extensions: String::new() });
    assert_eq!(next(&events), Event::Text("fragmented ✓".into()));
    assert_eq!(next(&events), Event::Close { code: 4001, reason: "bye".into(), clean: true });
    server.join().expect("server");
}

#[test]
fn websocket_refused_and_missing() {
    let (socket, server) = serve("5000.sock", |mut stream| {
        read_head(&stream);
        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").expect("answer");
    });
    let (sender, events) = channel();
    let client = FetchClient::default();
    client.upgrade(upgrade(&socket, &[]), sender);
    let Event::Error(message) = next(&events) else { panic!("an error") };
    assert!(message.contains("404"), "{message}");
    assert_eq!(next(&events), Event::Close { code: 1006, reason: String::new(), clean: false });
    server.join().expect("server");

    let (sender, events) = channel();
    let client = FetchClient::default();
    let socket = missing("1.sock");
    client.upgrade(upgrade(&socket, &[]), sender);
    assert!(matches!(next(&events), Event::Error(message) if message.contains(&socket)));
    assert!(matches!(next(&events), Event::Close { code: 1006, clean: false, .. }));
}

#[test]
fn channel_exchange_past_the_initial_credit() {
    use crate::runtime::channel::{CREDIT, DATA, FIN, Frame, Mux, OPEN, WINDOW};
    let size = CREDIT as usize + 50_000;
    let (from_mux, to_peer) = std::io::pipe().expect("pipe");
    let (from_peer, mut to_mux) = std::io::pipe().expect("pipe");
    let mux = Mux::start(to_peer, from_peer).expect("mux");
    let (sender, events) = channel();
    let client = FetchClient::default();
    let body: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    let to = Dial::App { app: "fetch.channel.test".into(), port: 3000 };
    let mut exchange = Exchange::new(to, "POST".into(), "/echo".into(), vec![], Some(body.clone())).expect("valid");
    exchange.host = "localhost:3000".into();
    client.exchange(exchange, sender);
    std::thread::sleep(Duration::from_millis(200));
    let generation = crate::runtime::channel::register("fetch.channel.test", crate::runtime::channel::Route::Channel(mux.clone()));
    let server = std::thread::spawn(move || {
        let mut reader = from_mux;
        let mut frame = || Frame::read(&mut reader).expect("a frame").expect("not the end");
        let open = frame();
        assert_eq!((open.kind, open.payload.as_slice()), (OPEN, 3000u16.to_be_bytes().as_slice()));
        let id = open.stream;
        let mut request = Vec::new();
        let body_at = loop {
            let data = frame();
            assert_eq!((data.kind, data.stream), (DATA, id));
            to_mux.write_all(&Frame::new(WINDOW, id, (data.payload.len() as u32).to_be_bytes()).encode()).expect("window");
            request.extend(data.payload);
            if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n").map(|at| at + 4)
                && request.len() >= at + size
            {
                break at;
            }
        };
        let head = String::from_utf8_lossy(&request[..body_at]).to_ascii_lowercase();
        assert!(head.starts_with("post /echo http/1.1\r\n") && head.contains("host: localhost:3000"), "{head}");
        let mut answer = format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\n\r\n").into_bytes();
        answer.extend_from_slice(&request[body_at..]);
        let mut credit = CREDIT as usize;
        for piece in answer.chunks(16 * 1024) {
            while credit < piece.len() {
                let grant = frame();
                if grant.kind == WINDOW {
                    credit += u32::from_be_bytes(grant.payload.try_into().expect("u32")) as usize;
                }
            }
            credit -= piece.len();
            to_mux.write_all(&Frame::new(DATA, id, piece.to_vec()).encode()).expect("answer");
        }
        to_mux.write_all(&Frame::new(FIN, id, []).encode()).expect("fin");
        loop {
            let last = frame();
            if last.kind == FIN {
                return;
            }
            assert_eq!(last.kind, WINDOW, "{last:?}");
        }
    });
    assert!(matches!(next(&events), Event::Head { status: 200, .. }));
    let mut echoed = Vec::new();
    loop {
        match next(&events) {
            Event::Chunk(data) => echoed.extend(data),
            Event::End => break,
            other => panic!("{other:?}"),
        }
    }
    assert!(echoed == body, "the body came back whole ({} of {size} bytes)", echoed.len());
    server.join().expect("server");
    crate::runtime::channel::unregister("fetch.channel.test", generation);
}

#[test]
fn app_sockets_follow_the_current_generation() {
    let old = scratch();
    let generation = crate::runtime::channel::register("fetch.sockets.test", crate::runtime::channel::Route::Sockets(old.clone()));
    let (sender, events) = channel();
    let client = FetchClient::default();
    let exchange = Exchange::new(Dial::App { app: "fetch.sockets.test".into(), port: 3000 }, "GET".into(), "/".into(), vec![], None).expect("valid");
    client.exchange(exchange, sender);
    std::thread::sleep(Duration::from_millis(200));
    crate::runtime::channel::unregister("fetch.sockets.test", generation);
    let new = scratch();
    let listener = Listener::bind(&new.join("3000.sock"));
    let generation = crate::runtime::channel::register("fetch.sockets.test", crate::runtime::channel::Route::Sockets(new));
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        read_head(&stream);
        stream.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").expect("answer");
    });
    assert!(matches!(next(&events), Event::Head { status: 204, .. }));
    assert_eq!(next(&events), Event::End);
    server.join().expect("server");
    crate::runtime::channel::unregister("fetch.sockets.test", generation);
}

#[test]
fn app_dials_are_checked() {
    let to = |app: &str| Dial::App { app: app.into(), port: 1 };
    assert!(Exchange::new(to(""), "GET".into(), "/".into(), vec![], None).is_err());
    assert!(Exchange::new(to("x"), "GET".into(), "no-slash".into(), vec![], None).is_err());
    assert!(Exchange::new(to("x"), "GET".into(), "/".into(), vec![], None).is_ok());
}
