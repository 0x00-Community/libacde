use crate::runtime::failure::{Failure, Outcome};
use crate::runtime::sync::Locked;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Map, Value, json};
use sha2::Sha256;
use std::{collections::HashMap, sync::Mutex};

fn mac(key: &[u8], data: &[u8]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key");
    mac.update(data);
    mac
}

fn signature(key: &[u8], data: &str) -> String {
    B64.encode(mac(key, data.as_bytes()).finalize().into_bytes())
}

fn verified(key: &[u8], data: &str, signature: &str) -> bool {
    B64.decode(signature).is_ok_and(|signature| mac(key, data.as_bytes()).verify_slice(&signature).is_ok())
}

fn object(part: &str) -> Outcome<Map<String, Value>> {
    let bytes = B64.decode(part.trim_end_matches('=')).map_err(|_| Failure::token_refused("malformed token"))?;
    match serde_json::from_slice(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(Failure::token_refused("malformed token")),
    }
}

fn now_ms() -> f64 {
    crate::runtime::unix_now().as_millis() as f64
}

pub struct Jwt {
    secret: Vec<u8>,
}

impl Jwt {
    pub fn new(secret: &str) -> Self {
        Self { secret: secret.as_bytes().to_vec() }
    }

    pub fn sign(&self, claims: &Value, ttl_seconds: Option<u64>) -> String {
        let now = (now_ms() / 1000.0).floor() as u64;
        let mut body = claims.as_object().cloned().unwrap_or_default();
        body.insert("iat".into(), now.into());
        if let Some(ttl) = ttl_seconds {
            body.insert("exp".into(), (now + ttl).into());
        }
        let unsigned = format!("{}.{}", B64.encode(json!({"alg": "HS256", "typ": "JWT"}).to_string()), B64.encode(Value::Object(body).to_string()));
        format!("{unsigned}.{}", signature(&self.secret, &unsigned))
    }

    pub fn verify(&self, token: &str) -> Outcome<Map<String, Value>> {
        let parts: Vec<&str> = token.split('.').collect();
        let [header_part, claims_part, signature] = parts.as_slice() else { return Err(Failure::token_refused("malformed token")) };
        let (header, claims) = (object(header_part)?, object(claims_part)?);
        if header.get("alg").and_then(Value::as_str) != Some("HS256") {
            return Err(Failure::token_refused("unexpected token algorithm"));
        }
        let signed = format!("{header_part}.{claims_part}");
        if !verified(&self.secret, &signed, signature) {
            return Err(Failure::token_refused("bad token signature"));
        }
        let now = now_ms() / 1000.0;
        if claims.get("exp").and_then(Value::as_f64).is_some_and(|exp| exp + 60.0 < now) {
            return Err(Failure::token_refused("token expired"));
        }
        if claims.get("nbf").and_then(Value::as_f64).is_some_and(|nbf| nbf - 60.0 > now) {
            return Err(Failure::token_refused("token not yet valid"));
        }
        Ok(claims)
    }

    pub fn verify_header(&self, header: Option<&str>) -> Outcome<Map<String, Value>> {
        let token = header
            .and_then(|h| {
                let (scheme, rest) = h.split_once(char::is_whitespace)?;
                let token = rest.trim_start();
                (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() && !token.contains(char::is_whitespace)).then_some(token)
            })
            .ok_or_else(|| Failure::token_refused("missing Authorization token"))?;
        let claims = self.verify(token)?;
        match claims.get("payload") {
            Some(Value::Object(payload)) => Ok(payload.clone()),
            _ => Ok(claims),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TicketKind {
    File,
    Callback,
}

#[derive(Clone)]
pub struct Ticket {
    pub key: String,
    nonce: String,
    exp: f64,
}

pub struct Tickets {
    seal: Vec<u8>,
    key: Vec<u8>,
    used: Mutex<HashMap<String, f64>>,
}

const FILE_TTL_MS: f64 = 10.0 * 60_000.0;
const CALLBACK_TTL_MS: f64 = 24.0 * 3_600_000.0;
const SEAL_NONCE: usize = 16;

fn keystream(key: &[u8], nonce: &[u8], data: &mut [u8]) {
    for (block, chunk) in data.chunks_mut(32).enumerate() {
        let mut stream = mac(key, nonce);
        stream.update(&(block as u32).to_be_bytes());
        for (byte, pad) in chunk.iter_mut().zip(stream.finalize().into_bytes()) {
            *byte ^= pad;
        }
    }
}

impl Tickets {
    pub fn new(secret: &str) -> Self {
        let derive = |label: &[u8]| mac(secret.as_bytes(), label).finalize().into_bytes().to_vec();
        Self { seal: derive(b"agentrt-office/ticket/v2/seal"), key: derive(b"agentrt-office/ticket/v2/sign"), used: Mutex::default() }
    }

    fn sealed(&self, body: &Value) -> String {
        let nonce = rand::random::<[u8; SEAL_NONCE]>();
        let mut data = body.to_string().into_bytes();
        keystream(&self.seal, &nonce, &mut data);
        B64.encode([nonce.as_slice(), &data].concat())
    }

    fn opened(&self, body: &str) -> Option<Map<String, Value>> {
        let bytes = B64.decode(body).ok()?;
        let (nonce, data) = bytes.split_at_checked(SEAL_NONCE)?;
        let mut data = data.to_vec();
        keystream(&self.seal, nonce, &mut data);
        match serde_json::from_slice(&data) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        }
    }

    pub fn issue(&self, kind: TicketKind, key: &str) -> String {
        let ttl = if kind == TicketKind::File { FILE_TTL_MS } else { CALLBACK_TTL_MS };
        let nonce = B64.encode(rand::random::<[u8; 9]>());
        let body = json!({"key": key, "kind": kind, "exp": now_ms() + ttl, "nonce": nonce});
        let body = self.sealed(&body);
        format!("{body}.{}", signature(&self.key, &body))
    }

    pub fn read(&self, token: &str, kind: TicketKind) -> Outcome<Ticket> {
        let mut parts = token.split('.');
        let (Some(body), Some(signature), None) = (parts.next(), parts.next(), parts.next()) else { return Err(Failure::token_refused("bad ticket")) };
        if body.is_empty() || signature.is_empty() || !verified(&self.key, body, signature) {
            return Err(Failure::token_refused("bad ticket"));
        }
        let value = self.opened(body).ok_or_else(|| Failure::token_refused("bad ticket"))?;
        if value.get("kind") != Some(&json!(kind)) {
            return Err(Failure::token_refused("ticket is for another purpose"));
        }
        let exp = value.get("exp").and_then(Value::as_f64).ok_or_else(|| Failure::token_refused("ticket expired"))?;
        if exp < now_ms() {
            return Err(Failure::token_refused("ticket expired"));
        }
        let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
        let (Some(key), Some(nonce)) = (text("key"), text("nonce")) else {
            return Err(Failure::token_refused("bad ticket"));
        };
        Ok(Ticket { key, nonce, exp })
    }

    pub fn consume(&self, token: &str) -> Outcome<Ticket> {
        let ticket = self.read(token, TicketKind::File)?;
        let now = now_ms();
        let mut used = self.used.locked();
        used.retain(|_, exp| *exp >= now);
        if used.contains_key(&ticket.nonce) {
            return Err(Failure::token_refused("ticket already used"));
        }
        used.insert(ticket.nonce.clone(), ticket.exp);
        Ok(ticket)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_cannot_read_the_key_in_a_ticket() {
        let tickets = Tickets::new("shared with Document Server");
        for kind in [TicketKind::File, TicketKind::Callback] {
            let token = tickets.issue(kind, "doc-key");
            let url = format!("http://127.0.0.1:1/office/file/{token}");
            for text in [token.clone(), url] {
                let decoded: Vec<u8> = text.split(['.', '/']).filter_map(|part| B64.decode(part).ok()).flatten().collect();
                assert!(!text.contains("doc-key"), "{text}");
                assert!(!decoded.windows(7).any(|w| w == b"doc-key"), "the key decodes out of {text}");
            }
            assert_eq!(tickets.read(&token, kind).unwrap().key, "doc-key");
        }
    }

    #[test]
    fn a_ticket_only_opens_with_its_secret_and_untouched() {
        let tickets = Tickets::new("one");
        let token = tickets.issue(TicketKind::File, "k");
        assert!(Tickets::new("two").read(&token, TicketKind::File).is_err());
        assert!(tickets.read(&token, TicketKind::Callback).is_err());
        let (body, signature) = token.split_once('.').unwrap();
        let mut bytes = B64.decode(body).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(tickets.read(&format!("{}.{signature}", B64.encode(bytes)), TicketKind::File).is_err());
        assert!(tickets.consume(&token).is_ok());
        assert!(tickets.consume(&token).is_err(), "a file ticket is good once");
    }
}
