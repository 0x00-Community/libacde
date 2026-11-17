use ntex::io::Io;
use std::sync::{Arc, OnceLock};

pub(crate) fn tls() -> Result<Arc<rustls::ClientConfig>, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    CONFIG.get_or_init(client_config).clone()
}

fn client_config() -> Result<Arc<rustls::ClientConfig>, String> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .and_then(|builder| builder.with_platform_verifier())
        .map_err(|error| format!("TLS: {error}"))?
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// An error with its causes, `outer: cause: root`: ntex's outer errors ("Failed to connect to host: Connector has been
/// disconnected") hide why (a refused connection, a certificate that does not verify). A cause whose text is already in
/// the message is left out. An OS error also gets its kind in English (`connection refused`): Windows words the OS message
/// in the user's language.
pub(crate) fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = String::new();
    let mut next = Some(error);
    while let Some(cause) = next {
        let mut part = cause.to_string();
        if let Some(io) = cause.downcast_ref::<std::io::Error>().filter(|io| io.raw_os_error().is_some()) {
            let kind = io.kind().to_string();
            if !part.to_ascii_lowercase().contains(&kind) {
                part = format!("{kind} ({part})");
            }
        }
        if !part.is_empty() && !text.contains(&part) {
            if !text.is_empty() {
                text.push_str(": ");
            }
            text.push_str(&part);
        }
        next = cause.source();
    }
    text
}

pub(crate) fn tls_config() -> ntex::tls::TlsConfig {
    ntex::tls::TlsConfig::new().set_handshake_timeout(ntex::time::Seconds(30))
}

pub(crate) fn client() -> ntex::client::Client {
    thread_local! {
        static CLIENT: ntex::client::Client = build_client();
    }
    CLIENT.with(Clone::clone)
}

fn build_client() -> ntex::client::Client {
    let tls = tls().expect("acde runtime: TLS");
    ntex::client::Client::builder().rustls((*tls).clone()).build(ntex::SharedCfg::new("acde-http").add(tls_config()))
}

#[cfg(target_os = "linux")]
pub(crate) async fn take(io: &Io, raw: &mut Vec<u8>) -> std::io::Result<bool> {
    loop {
        let moved = io.with_read_dst(|buf| {
            raw.extend_from_slice(buf);
            let moved = buf.len();
            buf.clear();
            moved
        });
        if moved > 0 {
            return Ok(true);
        }
        if io.read_more().await?.is_none() {
            return Ok(false);
        }
    }
}

#[derive(Default)]
pub(crate) struct Messages {
    partial: Option<(bool, Vec<u8>)>,
}

pub(crate) enum Incoming {
    Text(String),
    Binary(Vec<u8>),
    Ping(ntex::util::Bytes),
    Close(Option<ntex::ws::CloseReason>),
    Nothing,
}

impl Messages {
    pub(crate) const FRAME: usize = 16 << 20;
    const MESSAGE: usize = 64 << 20;

    pub(crate) async fn next<F>(&mut self, io: &Io<F>, codec: &ntex::ws::Codec) -> Result<Option<Incoming>, String> {
        use ntex::ws::{Frame, Item};
        let frame = loop {
            if let Some(frame) = io.decode(codec).map_err(|e| e.to_string())? {
                break frame;
            }
            if io.read_more().await.map_err(|e| e.to_string())?.is_none() {
                return Ok(None);
            }
        };
        let (text, data) = match frame {
            Frame::Text(data) => (true, data.to_vec()),
            Frame::Binary(data) => (false, data.to_vec()),
            Frame::Ping(data) => return Ok(Some(Incoming::Ping(data))),
            Frame::Pong(_) => return Ok(Some(Incoming::Nothing)),
            Frame::Close(reason) => return Ok(Some(Incoming::Close(reason))),
            Frame::Continuation(Item::FirstText(data)) => {
                self.partial = Some((true, data.to_vec()));
                return Ok(Some(Incoming::Nothing));
            }
            Frame::Continuation(Item::FirstBinary(data)) => {
                self.partial = Some((false, data.to_vec()));
                return Ok(Some(Incoming::Nothing));
            }
            Frame::Continuation(Item::Continue(data)) => {
                self.join(&data)?;
                return Ok(Some(Incoming::Nothing));
            }
            Frame::Continuation(Item::Last(data)) => {
                self.join(&data)?;
                self.partial.take().ok_or("a continuation without a first frame")?
            }
        };
        Ok(Some(if text {
            Incoming::Text(String::from_utf8(data).map_err(|_| "a text message that is not UTF-8".to_owned())?)
        } else {
            Incoming::Binary(data)
        }))
    }

    fn join(&mut self, data: &[u8]) -> Result<(), String> {
        let (_, joined) = self.partial.as_mut().ok_or("a continuation without a first frame")?;
        joined.extend_from_slice(data);
        if joined.len() > Self::MESSAGE {
            return Err("the message is too big".into());
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod frames {
    use std::io::Read;

    pub(crate) fn write(fin: bool, opcode: u8, payload: &[u8], mask: bool) -> Vec<u8> {
        let mut frame = vec![if fin { 0x80 } else { 0 } | opcode];
        let bit = if mask { 0x80 } else { 0 };
        match payload.len() {
            n if n < 126 => frame.push(bit | n as u8),
            n if n <= 0xffff => {
                frame.push(bit | 126);
                frame.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                frame.push(bit | 127);
                frame.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        if mask {
            let key = [0x12, 0x34, 0x56, 0x78];
            frame.extend_from_slice(&key);
            frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        } else {
            frame.extend_from_slice(payload);
        }
        frame
    }

    pub(crate) fn read(reader: &mut impl Read, masked: bool) -> (bool, u8, Vec<u8>) {
        let mut head = [0u8; 2];
        reader.read_exact(&mut head).expect("frame head");
        assert_eq!(head[1] & 0x80 != 0, masked, "masking");
        let length = match head[1] & 0x7f {
            126 => {
                let mut n = [0u8; 2];
                reader.read_exact(&mut n).expect("length");
                u16::from_be_bytes(n) as usize
            }
            127 => {
                let mut n = [0u8; 8];
                reader.read_exact(&mut n).expect("length");
                u64::from_be_bytes(n) as usize
            }
            n => n as usize,
        };
        let mut key = [0u8; 4];
        if masked {
            reader.read_exact(&mut key).expect("mask");
        }
        let mut payload = vec![0u8; length];
        reader.read_exact(&mut payload).expect("payload");
        payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= key[i % 4]);
        (head[0] & 0x80 != 0, head[0] & 0x0f, payload)
    }
}
