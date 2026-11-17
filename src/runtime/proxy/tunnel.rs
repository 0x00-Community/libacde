//! A connection to the target through a hop: `CONNECT` on an HTTP proxy (for every scheme, so the proxy never sees the
//! request), or SOCKS4a / SOCKS5 with the name resolved by the proxy; then TLS with the target for https and wss.

use super::{Endpoint, Hop, Target};
use ntex::connect::rustls::TlsClientFilter;
use ntex::connect::{Connect, ConnectError, Connector};
use ntex::io::{Io, IoBoxed, Layer};
use ntex::time::Millis;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

/// connecting to the hop and opening the tunnel; TLS with the target has `tls_config`'s 30 seconds of its own
const SETUP: Millis = Millis(30_000);
const HEAD: usize = 16 << 10;

pub(crate) enum Tunnel {
    Plain(Io),
    Tls(Io<Layer<TlsClientFilter>>),
}

impl Tunnel {
    pub(crate) fn boxed(self) -> IoBoxed {
        match self {
            Self::Plain(io) => io.boxed(),
            Self::Tls(io) => io.boxed(),
        }
    }
}

/// Tries the hops in order; the error names every hop that failed and why.
pub(crate) async fn open(hops: &[Hop], target: &Target) -> Result<Tunnel, String> {
    open_with(hops, target, crate::runtime::http::tls()?).await
}

pub(crate) async fn open_with(hops: &[Hop], target: &Target, tls: Arc<rustls::ClientConfig>) -> Result<Tunnel, String> {
    let cfg: ntex::SharedCfg = ntex::SharedCfg::new("acde-proxy").add(crate::runtime::http::tls_config()).into();
    let mut failures = Vec::new();
    for hop in hops {
        let opened = ntex::time::timeout(SETUP, through(hop, target, &cfg)).await.unwrap_or_else(|()| Err("timed out".into()));
        match opened {
            Ok(io) => return secure(io, hop, target, tls).await,
            Err(error) => failures.push(match hop {
                Hop::Direct => format!("cannot connect to {}: {error}", target.endpoint()),
                _ => format!("cannot reach {} through the proxy {hop}: {error}", target.endpoint()),
            }),
        }
    }
    Err(failures.join("; "))
}

async fn secure(io: Io, hop: &Hop, target: &Target, tls: Arc<rustls::ClientConfig>) -> Result<Tunnel, String> {
    if !target.secure {
        return Ok(Tunnel::Plain(io));
    }
    let failed = |why: String| match hop {
        Hop::Direct => format!("TLS with {}: {why}", target.endpoint()),
        _ => format!("TLS with {} through the proxy {hop}: {why}", target.endpoint()),
    };
    let name = rustls::pki_types::ServerName::try_from(target.host.clone()).map_err(|error| failed(error.to_string()))?;
    let handshake = crate::runtime::http::tls_config().handshake_timeout();
    match ntex::time::timeout(handshake, TlsClientFilter::create(io, tls, name)).await {
        Ok(Ok(io)) => Ok(Tunnel::Tls(io)),
        Ok(Err(error)) => Err(failed(crate::runtime::http::chain(&error))),
        Err(()) => Err(failed("the handshake timed out".into())),
    }
}

/// ntex shows an I/O failure only as "Tcp connect"; the cause is in the I/O error
fn reason(error: &ntex::Error<ConnectError>) -> String {
    match &**error {
        ConnectError::Io(error) | ConnectError::Resolver(error) => crate::runtime::http::chain(error),
        other => other.to_string(),
    }
}

async fn through(hop: &Hop, target: &Target, cfg: &ntex::SharedCfg) -> Result<Io, String> {
    let at = match hop {
        Hop::Direct => return tcp(&target.endpoint(), cfg).await,
        Hop::Http(at) | Hop::Socks4(at) | Hop::Socks5(at) => at,
    };
    let io = tcp(at, cfg).await.map_err(|error| format!("cannot connect to it: {error}"))?;
    match hop {
        Hop::Direct => {}
        Hop::Http(_) => http_connect(&io, target).await?,
        Hop::Socks4(_) => socks4(&io, target).await?,
        Hop::Socks5(_) => socks5(&io, target).await?,
    }
    Ok(io)
}

async fn tcp(at: &Endpoint, cfg: &ntex::SharedCfg) -> Result<Io, String> {
    let request = match at.host.parse::<IpAddr>() {
        Ok(ip) => Connect::with(at.host.clone(), SocketAddr::new(ip, at.port)),
        Err(_) => Connect::new(at.host.clone()).set_port(at.port),
    };
    Connector::<String>::new().connect(request, cfg).await.map_err(|error| reason(&error))
}

async fn write(io: &Io, bytes: &[u8]) -> Result<(), String> {
    io.encode_slice(bytes).map_err(|error| error.to_string())?;
    io.flush(true).await.map_err(|error| error.to_string())
}

async fn read(io: &Io, into: &mut [u8]) -> Result<(), String> {
    io.read_exact(into).await.map_err(|error| error.to_string())
}

/// `CONNECT host:port`; a 2xx answer opens the tunnel. Only the answer's head is taken from the connection.
async fn http_connect(io: &Io, target: &Target) -> Result<(), String> {
    let endpoint = target.endpoint();
    write(io, format!("CONNECT {endpoint} HTTP/1.1\r\nHost: {endpoint}\r\n\r\n").as_bytes()).await?;
    let head = loop {
        let head = io.with_read_dst(|buf| {
            let end = buf.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
            let head = buf[..end].to_vec();
            buf.advance_to(end);
            Some(head)
        });
        if let Some(head) = head {
            break head;
        }
        if io.with_read_dst(|buf| buf.len()) > HEAD {
            return Err("the answer to CONNECT is too long".into());
        }
        if io.read_more().await.map_err(|error| error.to_string())?.is_none() {
            return Err("it closed the connection before answering CONNECT".into());
        }
    };
    let head = String::from_utf8_lossy(&head);
    let status = head.lines().next().unwrap_or_default().trim();
    let mut parts = status.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    let code = parts.next().and_then(|code| code.parse::<u16>().ok());
    match code {
        Some(200..=299) if version.starts_with("HTTP/") => Ok(()),
        _ => Err(format!("it answered {status:?} to CONNECT {endpoint}")),
    }
}

async fn socks5(io: &Io, target: &Target) -> Result<(), String> {
    write(io, &[5, 1, 0]).await?;
    let mut chosen = [0u8; 2];
    read(io, &mut chosen).await?;
    match chosen {
        [5, 0] => {}
        [5, 0xff] => return Err("the SOCKS5 proxy wants a user name and password".into()),
        _ => return Err("not a SOCKS5 proxy".into()),
    }
    let mut request = vec![5, 1, 0];
    match target.host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            request.push(1);
            request.extend_from_slice(&ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            request.push(4);
            request.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            let name = u8::try_from(target.host.len()).map_err(|_| "the host name is too long for SOCKS5")?;
            request.extend_from_slice(&[3, name]);
            request.extend_from_slice(target.host.as_bytes());
        }
    }
    request.extend_from_slice(&target.port.to_be_bytes());
    write(io, &request).await?;
    let mut answer = [0u8; 4];
    read(io, &mut answer).await?;
    if answer[0] != 5 {
        return Err("not a SOCKS5 proxy".into());
    }
    if answer[1] != 0 {
        const REPLIES: [&str; 9] = [
            "succeeded",
            "general failure",
            "not allowed by its rules",
            "network unreachable",
            "host unreachable",
            "connection refused",
            "TTL expired",
            "command not supported",
            "address type not supported",
        ];
        let reply = REPLIES.get(usize::from(answer[1])).copied().unwrap_or("an unknown reply");
        return Err(format!("the SOCKS5 proxy could not connect to {}: {reply}", target.endpoint()));
    }
    let bound = match answer[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut length = [0u8; 1];
            read(io, &mut length).await?;
            usize::from(length[0])
        }
        _ => return Err("not a SOCKS5 proxy".into()),
    };
    read(io, &mut vec![0u8; bound + 2]).await
}

async fn socks4(io: &Io, target: &Target) -> Result<(), String> {
    let mut request = vec![4, 1];
    request.extend_from_slice(&target.port.to_be_bytes());
    match target.host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            request.extend_from_slice(&ip.octets());
            request.push(0);
        }
        Ok(IpAddr::V6(_)) => return Err("SOCKS4 cannot reach an IPv6 address".into()),
        Err(_) => {
            request.extend_from_slice(&[0, 0, 0, 1, 0]);
            request.extend_from_slice(target.host.as_bytes());
            request.push(0);
        }
    }
    write(io, &request).await?;
    let mut answer = [0u8; 8];
    read(io, &mut answer).await?;
    match answer[1] {
        0x5a => Ok(()),
        reply => Err(format!("the SOCKS4 proxy could not connect to {} (reply {reply:#x})", target.endpoint())),
    }
}
