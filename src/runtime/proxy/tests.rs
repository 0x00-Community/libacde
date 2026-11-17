use super::rules::{Env, NoProxy, exception, proxy, windows_bypass, windows_list, windows_manual};
use super::*;
use crate::runtime::fetch::{Deliver, Dial, Event, FetchClient};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

fn url(text: &str) -> Url {
    Url::parse(text).expect("a URL")
}

fn at(host: &str, port: u16) -> Endpoint {
    Endpoint { host: host.into(), port }
}

fn env(vars: &[(&str, &str)]) -> (Option<Env>, NoProxy) {
    let vars: HashMap<String, String> = vars.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect();
    Env::read(|name| vars.get(name).cloned())
}

#[test]
fn environment_by_scheme_lower_case_first() {
    let (found, _) = env(&[("http_proxy", "127.0.0.1:7890"), ("HTTP_PROXY", "http://other:1"), ("HTTPS_PROXY", "http://[::1]:8080")]);
    let found = found.expect("proxy variables");
    assert_eq!(found.find(&url("http://example.com/")), [Hop::Http(at("127.0.0.1", 7890))]);
    assert_eq!(found.find(&url("https://example.com/")), [Hop::Http(at("::1", 8080))]);
    let (found, _) = env(&[("http_proxy", " "), ("HTTP_PROXY", "proxy.lan")]);
    assert_eq!(found.expect("upper case").find(&url("http://example.com/")), [Hop::Http(at("proxy.lan", 80))]);
}

#[test]
fn all_proxy_serves_the_rest() {
    let (found, _) = env(&[("ALL_PROXY", "socks5h://127.0.0.1:7891"), ("http_proxy", "http://127.0.0.1:7890")]);
    let found = found.expect("proxy variables");
    assert_eq!(found.find(&url("http://example.com/")), [Hop::Http(at("127.0.0.1", 7890))]);
    assert_eq!(found.find(&url("https://example.com/")), [Hop::Socks5(at("127.0.0.1", 7891))]);
}

#[test]
fn no_variables_leave_it_to_the_system() {
    let (found, no_proxy) = env(&[("no_proxy", "example.com")]);
    assert!(found.is_none());
    assert!(no_proxy.matches(&url("https://example.com/")));
}

#[test]
fn proxy_addresses() {
    assert_eq!(proxy("http://user:pw@proxy:3128", "http"), Some(Hop::Http(at("proxy", 3128))));
    assert_eq!(proxy("socks5://127.0.0.1", "http"), Some(Hop::Socks5(at("127.0.0.1", 1080))));
    assert_eq!(proxy("socks4a://h:9", "http"), Some(Hop::Socks4(at("h", 9))));
    assert_eq!(proxy("127.0.0.1:1080", "socks4"), Some(Hop::Socks4(at("127.0.0.1", 1080))));
    assert_eq!(proxy("https://secure-proxy:443", "http"), None);
    assert_eq!(proxy("::::", "http"), None);
}

#[test]
fn no_proxy_names_addresses_and_ports() {
    let list = NoProxy::parse("example.com, .corp.lan,*.test ,10.0.0.0/8, 192.168.1.5, [fd00::1]:8443, api.local:8080");
    assert!(list.matches(&url("https://example.com/")));
    assert!(list.matches(&url("https://www.Example.com./")));
    assert!(!list.matches(&url("https://notexample.com/")));
    assert!(list.matches(&url("http://corp.lan/")));
    assert!(list.matches(&url("http://a.b.corp.lan/")));
    assert!(list.matches(&url("http://x.test/")));
    assert!(list.matches(&url("http://10.20.30.40/")));
    assert!(!list.matches(&url("http://11.0.0.1/")));
    assert!(list.matches(&url("http://192.168.1.5:9/")));
    assert!(list.matches(&url("https://[fd00::1]:8443/")));
    assert!(!list.matches(&url("https://[fd00::1]/")));
    assert!(list.matches(&url("http://api.local:8080/")));
    assert!(!list.matches(&url("http://api.local/")));
    assert!(NoProxy::parse("*").matches(&url("https://anything/")));
    assert!(!NoProxy::parse("").matches(&url("https://anything/")));
}

#[test]
fn loopback_and_no_proxy_go_direct() {
    let system = System::read(|name| match name {
        "https_proxy" => Some("http://127.0.0.1:7890".into()),
        "NO_PROXY" => Some("intranet".into()),
        _ => None,
    });
    for direct in ["https://localhost/", "https://app.localhost/", "https://127.0.0.2/", "https://[::1]/", "https://intranet/"] {
        assert_eq!(system.find(&url(direct)), [Hop::Direct], "{direct}");
    }
    assert_eq!(system.find(&url("https://example.com/")), [Hop::Http(at("127.0.0.1", 7890))]);
    assert!(!system.blocking(), "the environment decides without the platform");
}

#[test]
fn windows_proxy_lists() {
    assert_eq!(windows_list("127.0.0.1:7899", true), [Hop::Http(at("127.0.0.1", 7899))]);
    let per_scheme = "http=127.0.0.1:1;https=http://127.0.0.1:2;socks=127.0.0.1:3";
    assert_eq!(windows_list(per_scheme, false), [Hop::Http(at("127.0.0.1", 1))]);
    assert_eq!(windows_list(per_scheme, true), [Hop::Http(at("127.0.0.1", 2))]);
    assert_eq!(windows_list("ftp=h:21;socks=h:1080", true), [Hop::Socks4(at("h", 1080))]);
    assert_eq!(windows_list("a:1; b:2", false), [Hop::Http(at("a", 1)), Hop::Http(at("b", 2))]);
    assert_eq!(windows_list("ftp=h:21", false), [Hop::Direct]);
}

#[test]
fn windows_bypass_lists() {
    let list = "localhost;127.*;192.168.*;*.corp.lan;http://plain.example;<local>";
    assert!(windows_bypass(list, &url("http://192.168.0.7/")));
    assert!(windows_bypass(list, &url("https://build.corp.lan/")));
    assert!(!windows_bypass(list, &url("https://corp.lan/")));
    assert!(windows_bypass(list, &url("http://printer/")));
    assert!(!windows_bypass(list, &url("http://10.0.0.1/")));
    assert!(windows_bypass(list, &url("http://plain.example/")));
    assert!(!windows_bypass(list, &url("https://plain.example/")));
    assert!(windows_bypass("example.com:8443", &url("https://example.com:8443/")));
    assert!(!windows_bypass("example.com", &url("https://example.com.evil/")));
}

fn backend(vars: &[(&str, &str)], windows: bool, manual: Option<Manual>) -> Vec<(String, String)> {
    let vars: HashMap<String, String> = vars.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect();
    variables(|name| vars.get(name).cloned(), windows, move || manual)
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
}

#[test]
fn backends_get_the_session_variables_as_they_are() {
    let system = || Some(Manual { http: Some(Hop::Http(at("system", 1))), ..Manual::default() });
    let session = [("https_proxy", "http://a:1"), ("HTTPS_PROXY", "http://b:2"), ("no_proxy", "corp"), ("ftp_proxy", "x:1")];
    assert_eq!(backend(&session, false, system()), pairs(&[("https_proxy", "http://a:1"), ("HTTPS_PROXY", "http://b:2"), ("no_proxy", "corp")]));
    assert_eq!(backend(&[("HTTPS_PROXY", "http://b:2")], true, system()), pairs(&[("HTTPS_PROXY", "http://b:2")]));
    assert_eq!(backend(&[("no_proxy", "corp")], false, None), pairs(&[("no_proxy", "corp")]));
    assert!(backend(&[], false, None).is_empty());
}

#[test]
fn backends_get_the_manual_system_proxy_without_session_variables() {
    let manual = Manual { http: Some(Hop::Http(at("p", 8080))), https: Some(Hop::Http(at("::1", 8443))), bypass: vec![".corp.lan".into()] };
    let loopback = "localhost,127.0.0.1,::1";
    assert_eq!(
        backend(&[("NO_PROXY", "intranet")], true, Some(manual.clone())),
        pairs(&[("HTTP_PROXY", "http://p:8080"), ("HTTPS_PROXY", "http://[::1]:8443"), ("NO_PROXY", &format!("intranet,.corp.lan,{loopback}"))])
    );
    let found = backend(&[], false, Some(manual));
    assert!(found.contains(&("http_proxy".into(), "http://p:8080".into())) && found.contains(&("HTTP_PROXY".into(), "http://p:8080".into())), "{found:?}");
    assert!(found.contains(&("no_proxy".into(), format!(".corp.lan,{loopback}"))), "{found:?}");
    let socks = Manual { http: Some(Hop::Socks4(at("s", 1080))), https: Some(Hop::Socks4(at("s", 1080))), bypass: vec![] };
    assert_eq!(backend(&[], true, Some(socks)), pairs(&[("ALL_PROXY", "socks4a://s:1080"), ("NO_PROXY", loopback)]));
    assert_eq!(backend(&[("NO_PROXY", "x")], true, Some(Manual::default())), pairs(&[("NO_PROXY", "x")]));
}

#[test]
fn system_exceptions_in_no_proxy_form() {
    let manual = windows_manual("http=h:1;https=h:2", "localhost;127.*;192.168.*;*.corp.lan;http://plain.example;<local>;a*b.com");
    assert_eq!((manual.http, manual.https), (Some(Hop::Http(at("h", 1))), Some(Hop::Http(at("h", 2)))));
    assert_eq!(manual.bypass, ["localhost", "127.0.0.0/8", "192.168.0.0/16", ".corp.lan"]);
    assert_eq!(windows_manual("ftp=h:21", "").http, None);
    for (item, form) in
        [("*.local", Some(".local")), ("169.254/16", Some("169.254.0.0/16")), ("10.0.0.0/8", Some("10.0.0.0/8")), ("fd00::/8", Some("fd00::/8"))]
    {
        assert_eq!(exception(item).as_deref(), form, "{item}");
    }
    for (item, form) in [("Example.com:8080", Some("example.com:8080")), ("*", Some("*")), ("1.2.3.4", Some("1.2.3.4")), ("x/abc", None)] {
        assert_eq!(exception(item).as_deref(), form, "{item}");
    }
}

#[test]
fn targets() {
    let target = Target::of(&url("wss://[::1]:9443/a?b=1#c")).expect("a target");
    assert_eq!((target.host.as_str(), target.port, target.secure), ("::1", 9443, true));
    assert_eq!((target.authority.as_str(), target.path.as_str()), ("[::1]:9443", "/a?b=1"));
    assert_eq!(target.endpoint().to_string(), "[::1]:9443");
    let target = Target::of(&url("http://Example.com/")).expect("a target");
    assert_eq!((target.authority.as_str(), target.port), ("example.com", 80));
}

/// a target that answers one request with `body`, and the request head it got
fn target(body: &'static str) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let head = head(&mut BufReader::new(stream.try_clone().expect("clone")));
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("answer");
        head
    });
    (port, server)
}

fn head(reader: &mut impl BufRead) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("a line");
        let line = line.trim_end().to_owned();
        if line.is_empty() {
            return lines;
        }
        lines.push(line);
    }
}

fn pipe(mut from: TcpStream, mut to: TcpStream) {
    std::thread::spawn(move || {
        _ = std::io::copy(&mut from, &mut to);
        _ = to.shutdown(std::net::Shutdown::Write);
    });
}

/// An HTTP proxy that answers one `CONNECT` with `answer` and, on 200, joins the client to 127.0.0.1:`port` whatever the
/// host; it gives back the request head.
fn http_proxy(answer: &'static str, port: u16) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let proxy = listener.local_addr().expect("address").port();
    let server = std::thread::spawn(move || {
        let (mut client, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(client.try_clone().expect("clone"));
        let head = head(&mut reader);
        client.write_all(answer.as_bytes()).expect("answer");
        if answer.starts_with("HTTP/1.1 200") {
            assert!(reader.buffer().is_empty(), "nothing before the tunnel is open");
            let target = TcpStream::connect(("127.0.0.1", port)).expect("the target");
            pipe(client.try_clone().expect("clone"), target.try_clone().expect("clone"));
            pipe(target, client);
        }
        head
    });
    (proxy, server)
}

fn deliver() -> (Deliver, async_channel::Receiver<Event>) {
    let (sender, events) = async_channel::unbounded();
    let deliver: Deliver = Arc::new(move |event: Event, taken: futures_channel::oneshot::Sender<()>| {
        let delivered = sender.try_send(event).is_ok();
        _ = taken.send(());
        delivered
    });
    (deliver, events)
}

fn next(events: &async_channel::Receiver<Event>) -> Event {
    let events = events.clone();
    let received =
        crate::runtime::rt::block_on(crate::runtime::rt::local(move || async move { ntex::time::timeout(Duration::from_secs(10), events.recv()).await }));
    received.flatten().expect("the executor").expect("an event in time").expect("an event")
}

fn get(client: &FetchClient, url: &str) -> async_channel::Receiver<Event> {
    let (deliver, events) = deliver();
    client.request(Dial::Url(url.into()), "GET".into(), String::new(), vec![], None, deliver).expect("a request");
    events
}

#[test]
fn fetch_through_an_http_proxy_by_name() {
    let (port, target) = target("hello");
    let (proxy, proxied) = http_proxy("HTTP/1.1 200 Connection established\r\n\r\n", port);
    let client = FetchClient::with_proxies(Arc::new(Fixed(vec![Hop::Http(at("127.0.0.1", proxy))])));
    let events = get(&client, &format!("http://only.the.proxy.resolves.invalid:{port}/x?y=1"));
    let Event::Head { status, .. } = next(&events) else { panic!("head first") };
    assert_eq!(status, 200);
    assert_eq!(next(&events), Event::Chunk(b"hello".to_vec()));
    assert_eq!(next(&events), Event::End);
    let proxied = proxied.join().expect("proxy");
    assert_eq!(proxied[0], format!("CONNECT only.the.proxy.resolves.invalid:{port} HTTP/1.1"));
    let got = target.join().expect("target");
    assert_eq!(got[0], "GET /x?y=1 HTTP/1.1");
    assert!(got.iter().any(|line| line.eq_ignore_ascii_case(&format!("host: only.the.proxy.resolves.invalid:{port}"))), "{got:?}");
}

#[test]
fn a_refusing_proxy_is_named() {
    let (proxy, proxied) = http_proxy("HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n", 0);
    let client = FetchClient::with_proxies(Arc::new(Fixed(vec![Hop::Http(at("127.0.0.1", proxy))])));
    let events = get(&client, "https://example.com/");
    let Event::Error(message) = next(&events) else { panic!("an error") };
    assert!(message.contains(&format!("http://127.0.0.1:{proxy}")), "{message}");
    assert!(message.contains("407"), "{message}");
    proxied.join().expect("proxy");
}

#[test]
fn the_next_hop_after_a_dead_proxy() {
    let dead = TcpListener::bind("127.0.0.1:0").expect("bind");
    let dead_port = dead.local_addr().expect("address").port();
    drop(dead);
    let (port, target) = target("ok");
    let client = FetchClient::with_proxies(Arc::new(Fixed(vec![Hop::Http(at("127.0.0.1", dead_port)), Hop::Direct])));
    let events = get(&client, &format!("http://127.0.0.1:{port}/"));
    let Event::Head { status, .. } = next(&events) else { panic!("head first") };
    assert_eq!(status, 200);
    target.join().expect("target");
}

#[test]
fn fetch_through_socks5() {
    let (port, target) = target("socks");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let proxy = listener.local_addr().expect("address").port();
    let server = std::thread::spawn(move || {
        let (mut client, _) = listener.accept().expect("accept");
        let mut greeting = [0u8; 3];
        client.read_exact(&mut greeting).expect("greeting");
        assert_eq!(greeting, [5, 1, 0]);
        client.write_all(&[5, 0]).expect("method");
        let mut request = [0u8; 5];
        client.read_exact(&mut request).expect("request");
        assert_eq!(request[..4], [5, 1, 0, 3]);
        let mut name = vec![0u8; usize::from(request[4]) + 2];
        client.read_exact(&mut name).expect("name");
        client.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).expect("reply");
        let target = TcpStream::connect(("127.0.0.1", port)).expect("the target");
        pipe(client.try_clone().expect("clone"), target.try_clone().expect("clone"));
        pipe(target, client);
        String::from_utf8_lossy(&name[..name.len() - 2]).into_owned()
    });
    let client = FetchClient::with_proxies(Arc::new(Fixed(vec![Hop::Socks5(at("127.0.0.1", proxy))])));
    let events = get(&client, &format!("http://named.invalid:{port}/"));
    let Event::Head { status, .. } = next(&events) else { panic!("head first") };
    assert_eq!(status, 200);
    assert_eq!(next(&events), Event::Chunk(b"socks".to_vec()));
    assert_eq!(server.join().expect("proxy"), "named.invalid");
    target.join().expect("target");
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

pub(crate) fn localhost_tls() -> (Arc<rustls::ServerConfig>, rustls::pki_types::CertificateDer<'static>) {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("a certificate");
    let cert = certified.cert.der().clone();
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(certified.signing_key.serialize_der().into());
    let config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key)
        .expect("a server configuration");
    (Arc::new(config), cert)
}

fn tls_echo(reply: &'static [u8]) -> (u16, rustls::pki_types::CertificateDer<'static>, std::thread::JoinHandle<Vec<u8>>) {
    let (config, cert) = localhost_tls();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut tls = rustls::StreamOwned::new(rustls::ServerConnection::new(config).expect("session"), stream);
        let mut asked = [0u8; 4];
        tls.read_exact(&mut asked).expect("the request");
        tls.write_all(reply).expect("the answer");
        tls.flush().expect("flush");
        asked.to_vec()
    });
    (port, cert, server)
}

fn through_proxy(proxy: u16, port: u16, tls: Arc<rustls::ClientConfig>) -> Result<Vec<u8>, String> {
    let target = Target { host: "localhost".into(), port, secure: true, authority: format!("localhost:{port}"), path: "/".into() };
    let hops = vec![Hop::Http(at("127.0.0.1", proxy))];
    let opened = crate::runtime::rt::block_on(crate::runtime::rt::local(move || async move {
        let Tunnel::Tls(io) = tunnel::open_with(&hops, &target, tls).await? else { return Err("not TLS".to_owned()) };
        io.encode_slice(b"ping").map_err(|error| error.to_string())?;
        io.flush(true).await.map_err(|error| error.to_string())?;
        let mut answer = [0u8; 4];
        io.read_exact(&mut answer).await.map_err(|error| error.to_string())?;
        Ok(answer.to_vec())
    }));
    opened.flatten().expect("the executor")
}

#[test]
fn tls_runs_inside_a_connect_tunnel() {
    let (port, cert, target) = tls_echo(b"pong");
    let (proxy, proxied) = http_proxy("HTTP/1.1 200 Connection established\r\n\r\n", port);
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).expect("a root");
    let tls = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    assert_eq!(through_proxy(proxy, port, Arc::new(tls)).expect("a TLS tunnel"), b"pong");
    assert_eq!(target.join().expect("the target"), b"ping");
    assert!(proxied.join().expect("the proxy")[0].starts_with("CONNECT localhost:"), "a CONNECT for the target's name");
}

#[test]
fn an_untrusted_target_inside_a_tunnel_is_named() {
    let (port, _, _target) = tls_echo(b"pong");
    let (proxy, _) = http_proxy("HTTP/1.1 200 Connection established\r\n\r\n", port);
    let failed = through_proxy(proxy, port, crate::runtime::http::tls().expect("the system's TLS")).expect_err("no trust");
    assert!(failed.contains("through the proxy") && failed.contains("certificate"), "{failed}");
}
