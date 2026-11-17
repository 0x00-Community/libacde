use std::io::Write;
use std::time::{Duration, Instant};

use futures_lite::future::or;
use ntex::time::{Millis, Seconds};
use ntex::util::Bytes;
use ntex::ws::{Frame, Message, WsClient, WsClientConfig};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Ctl;
use super::mic;

const ENDPOINT: &str = "wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws";
const REGISTER: &str = "https://log.snssdk.com/service/2/device_register/";
const APP_KEY: &str = "OrnqKvSSrs";
const AID: u32 = 401734;
const APP_VERSION: &str = "1.5.1";
const FRAME: usize = (mic::RATE as usize / 100) * 2;
const LIMIT: Duration = Duration::from_secs(60);
const ANSWER: Duration = Duration::from_secs(10);
const GAP: Duration = Duration::from_secs(1);
const TT_CONSTANT: &str = "4dd4c2e6b83162090e52b3c7a6733ba41cb2462b829ab58a196b39db57177524f49baf7f08e8d68d26a72e37c1a95a2f1f05a51892aef2949732b62a38aadd58";

pub(super) async fn run(id: u32, ctl: async_channel::Receiver<Ctl>, emit: impl Fn(Value)) {
    let started = Instant::now();
    let mut heard = Heard::default();
    let result = session(id, ctl, &emit, &mut heard).await;
    let took = started.elapsed().as_secs_f64();
    let audio = heard.bytes as f64 / f64::from(mic::RATE * 2);
    match result {
        Ok(Some(text)) => {
            log::info!("voice #{id}: finished, recognized {} chars; took {took:.1} s, received {audio:.1} s of audio", text.chars().count());
            emit(json!({"type": "final", "text": text}));
        }
        Ok(None) => log::info!("voice #{id}: cancelled; took {took:.1} s, received {audio:.1} s of audio"),
        Err(message) => {
            log::warn!("voice #{id}: ended with error: {message}; took {took:.1} s, received {audio:.1} s of audio");
            emit(json!({"type": "error", "message": message}));
        }
    }
}

#[derive(Default)]
struct Heard {
    bytes: usize,
    last: Option<Instant>,
}

async fn session(id: u32, ctl: async_channel::Receiver<Ctl>, emit: &impl Fn(Value), heard: &mut Heard) -> Result<Option<String>, String> {
    let opening = Instant::now();
    let (mic, pcm) = mic::open(id).await?;
    let mut mic = Some(mic);
    let mut open = std::pin::pin!(Asr::open());
    let mut stop = false;
    let mut asr = loop {
        match or(async { Ok(open.as_mut().await) }, async { Err(ctl.recv().await.ok()) }).await {
            Ok(asr) => {
                let asr = asr?;
                log::info!("voice #{id}: recognition service connected ({} ms)", opening.elapsed().as_millis());
                break asr;
            }
            Err(Some(Ctl::Finish)) => stop = true,
            Err(_) => return Ok(None),
        }
    };
    let started = Instant::now();
    let mut pending = Vec::new();
    let mut finishing: Option<Instant> = None;
    let mut transcript = Transcript::default();
    loop {
        let deadline = finishing.unwrap_or(started + LIMIT);
        let wait = Millis::from(deadline.saturating_duration_since(Instant::now()));
        let step = if std::mem::take(&mut stop) {
            Step::Ctl(Some(Ctl::Finish))
        } else {
            or(
                or(async { Step::Frame(asr.frames.recv().await) }, async {
                    if finishing.is_some() { std::future::pending().await } else { Step::Pcm(pcm.recv().await.ok()) }
                }),
                or(async { Step::Ctl(ctl.recv().await.ok()) }, async {
                    ntex::time::sleep(wait).await;
                    Step::Timeout
                }),
            )
            .await
        };
        match step {
            Step::Pcm(Some(chunk)) => {
                let now = Instant::now();
                if let Some(gap) = heard.last.map(|last| now - last)
                    && gap > GAP
                {
                    log::warn!("voice #{id}: no audio from the microphone for {:.1} s", gap.as_secs_f64());
                }
                heard.last = Some(now);
                heard.bytes += chunk.len();
                pending.extend_from_slice(&chunk);
                while pending.len() >= FRAME {
                    let frame: Vec<u8> = pending.drain(..FRAME).collect();
                    asr.audio(&frame).await?;
                }
            }
            Step::Pcm(None) => return Err("the microphone stopped".into()),
            Step::Ctl(Some(Ctl::Finish)) | Step::Timeout if finishing.is_none() => {
                if matches!(step, Step::Timeout) {
                    log::info!("voice #{id}: reached {} s, stopping", LIMIT.as_secs());
                }
                if heard.last.is_none() {
                    log::warn!("voice #{id}: the microphone never sent audio");
                }
                mic = None;
                while let Ok(chunk) = pcm.try_recv() {
                    pending.extend_from_slice(&chunk);
                }
                for frame in pending.chunks(FRAME) {
                    asr.audio(frame).await?;
                }
                pending.clear();
                if asr.sequence == 0 {
                    return Ok(Some(String::new()));
                }
                asr.send("FinishSession", None, &[]).await?;
                finishing = Some(Instant::now() + ANSWER);
            }
            Step::Timeout => return Err("speech recognition timed out".into()),
            Step::Ctl(_) => return Ok(None),
            Step::Frame(None) => return Err("the speech recognition connection closed".into()),
            Step::Frame(Some(Err(error))) => return Err(format!("speech recognition: {error}")),
            Step::Frame(Some(Ok(Frame::Binary(data)))) => {
                let Some(response) = asr.response(&data).await? else { continue };
                if let Some((text, finished)) = response.update()
                    && let Some(text) = transcript.apply(text, finished)
                {
                    emit(json!({"type": "partial", "text": text}));
                }
                if matches!(response.event.as_str(), "SessionFinished" | "TaskFinished") {
                    if response.event != "TaskFinished" {
                        _ = asr.send("FinishTask", None, &[]).await;
                    }
                    drop(mic);
                    return Ok(Some(transcript.text));
                }
            }
            Step::Frame(Some(Ok(Frame::Close(_)))) => return Err("the speech recognition server closed the connection".into()),
            Step::Frame(Some(Ok(_))) => {}
        }
    }
}

enum Step {
    Frame(Option<Result<Frame, ntex::ws::error::WsError<()>>>),
    Pcm(Option<Vec<u8>>),
    Ctl(Option<Ctl>),
    Timeout,
}

#[derive(Default)]
struct Transcript {
    committed: String,
    text: String,
    current: String,
    finished: bool,
}

impl Transcript {
    fn apply(&mut self, text: String, finished: bool) -> Option<String> {
        if (text.is_empty() && !finished) || (finished && self.finished && text == self.current) {
            return None;
        }
        let joined = format!("{}{text}", self.committed);
        self.current = text;
        self.finished = finished;
        if finished {
            self.committed.clone_from(&joined);
        }
        (joined != std::mem::replace(&mut self.text, joined.clone())).then_some(joined)
    }
}

struct Asr {
    sink: ntex::ws::WsSink,
    frames: ntex::channel::mpsc::Receiver<Result<Frame, ntex::ws::error::WsError<()>>>,
    task: String,
    sequence: u64,
}

impl Asr {
    async fn open() -> Result<Self, String> {
        let identity = Identity::ensure().await?;
        let url = format!("{ENDPOINT}?device_id={}&aid={AID}&iid={}", identity.device_id, identity.install_id);
        let config = WsClientConfig::new()
            .set_header("proto-version", "v2")
            .and_then(|c| c.set_header("sdk-version", "2"))
            .and_then(|c| c.set_header("x-custom-keepalive", "true"))
            .map_err(|error| error.to_string())?
            .set_handshake_timeout(Seconds(5));
        let tls = crate::runtime::http::tls()?;
        let connection = WsClient::new(url.as_str(), ntex::SharedCfg::new("acde-voice").add(config))
            .rustls(tls)
            .connect()
            .await
            .map_err(|error| format!("cannot reach the speech recognition server: {error}"))?;
        let sink = connection.sink();
        let frames = connection.seal().receiver();
        let mut asr = Self { sink, frames, task: uuid::Uuid::new_v4().to_string(), sequence: 0 };
        asr.send("StartTask", None, &[]).await?;
        asr.expect("TaskStarted").await?;
        let session = json!({
            "audio_info": {"sample_rate": mic::RATE, "channel": 1, "format": "pcm"},
            "extra": {
                "enable_punctuation": true,
                "enable_asr_twopass": true,
                "enable_asr_threepass": true,
                "end_smooth_window_ms": 800,
                "did": identity.device_id,
                "app_name": "oime",
                "app_version": APP_VERSION,
            },
        });
        asr.send("StartSession", Some(&session), &[]).await?;
        asr.expect("SessionStarted").await?;
        Ok(asr)
    }

    async fn expect(&mut self, event: &str) -> Result<(), String> {
        let waited = ntex::time::timeout(Millis::from(ANSWER), async {
            loop {
                match self.frames.recv().await {
                    Some(Ok(Frame::Binary(data))) => {
                        if self.response(&data).await?.is_some_and(|response| response.event == event) {
                            return Ok(());
                        }
                    }
                    Some(Ok(Frame::Close(_))) | None => return Err(format!("the speech recognition server closed before {event}")),
                    Some(Err(error)) => return Err(format!("speech recognition: {error}")),
                    Some(Ok(_)) => {}
                }
            }
        });
        waited.await.unwrap_or_else(|()| Err(format!("the speech recognition server did not answer {event}")))
    }

    async fn response(&mut self, data: &[u8]) -> Result<Option<Response>, String> {
        let response = Response::decode(data).ok_or("a malformed speech recognition message")?;
        if !response.task.is_empty() && response.task != self.task {
            return Ok(None);
        }
        match response.event.as_str() {
            "SessionFailed" | "TaskFailed" => Err(format!("speech recognition failed: {} {}", response.status, response.status_text)),
            "Ping" => {
                self.send("Pong", None, &[]).await?;
                Ok(None)
            }
            "Pong" => Ok(None),
            _ => Ok(Some(response)),
        }
    }

    async fn audio(&mut self, frame: &[u8]) -> Result<(), String> {
        self.sequence += 1;
        let millis = crate::runtime::unix_now().as_millis() as u64;
        self.send("TaskRequest", Some(&json!({"extra": {}, "timestamp_ms": millis})), frame).await
    }

    async fn send(&mut self, event: &str, payload: Option<&Value>, audio: &[u8]) -> Result<(), String> {
        let mut message = Vec::with_capacity(audio.len() + 128);
        if event != "TaskRequest" {
            field(&mut message, 2, APP_KEY.as_bytes());
        }
        field(&mut message, 3, b"ASR");
        field(&mut message, 5, event.as_bytes());
        if let Some(payload) = payload {
            field(&mut message, 6, payload.to_string().as_bytes());
        }
        field(&mut message, 8, self.task.as_bytes());
        if !audio.is_empty() {
            field(&mut message, 7, audio);
            varint(&mut message, 9 << 3);
            varint(&mut message, self.sequence);
        }
        self.sink.send(Message::Binary(Bytes::from(message))).await.map_err(|error| format!("speech recognition: {error}"))
    }
}

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn field(out: &mut Vec<u8>, number: u64, value: &[u8]) {
    varint(out, (number << 3) | 2);
    varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

#[derive(Default)]
struct Response {
    task: String,
    event: String,
    status: u64,
    status_text: String,
    payload: String,
}

impl Response {
    fn decode(data: &[u8]) -> Option<Self> {
        let mut response = Self::default();
        let mut at = 0;
        let read = |at: &mut usize| -> Option<u64> {
            let mut value = 0u64;
            for shift in (0..64).step_by(7) {
                let byte = *data.get(*at)?;
                *at += 1;
                value |= u64::from(byte & 0x7f) << shift;
                if byte & 0x80 == 0 {
                    return Some(value);
                }
            }
            None
        };
        while at < data.len() {
            let key = read(&mut at)?;
            match key & 7 {
                0 => {
                    let value = read(&mut at)?;
                    if key >> 3 == 5 {
                        response.status = value;
                    }
                }
                1 => at += 8,
                5 => at += 4,
                2 => {
                    let length = usize::try_from(read(&mut at)?).ok()?;
                    let bytes = data.get(at..at.checked_add(length)?)?;
                    at += length;
                    let text = String::from_utf8_lossy(bytes).into_owned();
                    match key >> 3 {
                        1 => response.task = text,
                        4 => response.event = text,
                        6 => response.status_text = text,
                        7 => response.payload = text,
                        _ => {}
                    }
                }
                _ => return None,
            }
        }
        (at == data.len()).then_some(response)
    }

    fn update(&self) -> Option<(String, bool)> {
        let body: Value = serde_json::from_str(&self.payload).ok()?;
        let result = body.get("results")?.get(0)?;
        let text = result.get("text")?.as_str()?.to_owned();
        Some((text, result.get("is_vad_finished").and_then(Value::as_bool).unwrap_or(false)))
    }
}

#[derive(Serialize, Deserialize)]
struct Identity {
    device_id: String,
    install_id: String,
    cdid: String,
    openudid: String,
    clientudid: String,
}

impl Identity {
    async fn ensure() -> Result<Self, String> {
        let file = crate::runtime::state_file("doubao-ime.json").ok_or("no HOME")?;
        if let Some(identity) = std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok()) {
            return Ok(identity);
        }
        let identity = Self::register().await?;
        Self::save(&file, &identity).map_err(|error| format!("cannot save {}: {error}", file.display()))?;
        Ok(identity)
    }

    fn save(file: &std::path::Path, identity: &Self) -> std::io::Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temporary = file.with_extension(format!("new-{}", std::process::id()));
        let mut out = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temporary)?;
        out.write_all(&serde_json::to_vec(identity).map_err(std::io::Error::other)?)?;
        out.sync_all()?;
        std::fs::rename(temporary, file)
    }

    async fn register() -> Result<Self, String> {
        let mut random = [0u8; 10];
        rand::rng().fill_bytes(&mut random);
        let openudid: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let (clientudid, cdid, request) = (uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string());
        let now = crate::runtime::unix_now().as_millis() as u64;
        let body = json!({
            "magic_tag": "ss_app_log",
            "_gen_time": now,
            "header": {
                "aid": AID, "app_name": "oime", "channel": "official", "package": "com.bytedance.android.doubaoime",
                "app_version": APP_VERSION, "app_version_minor": "6", "version_code": 100501006, "update_version_code": 100501006,
                "manifest_version_code": 100501006, "not_request_sender": 1, "release_build": "", "custom": {},
                "device_platform": "android", "os": "Android", "os_version": "14", "os_api": 34,
                "device_model": "generic", "device_brand": "generic", "device_manufacturer": "generic", "cpu_abi": "arm64-v8a",
                "language": "zh", "timezone": 8, "tz_name": "Asia/Shanghai", "tz_offset": 28800, "access": "wifi",
                "openudid": openudid, "clientudid": clientudid, "cdid": cdid,
                "git_hash": "b416399", "sdk_version_code": 4030263, "sdk_target_version": 30, "sdk_version": "4.3.2-rc.13",
                "sdk_flavor": "cnInner", "guest_mode": 0, "req_id": request, "apk_first_install_time": now, "is_system_app": 0,
            },
        });
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(body.to_string().as_bytes()).map_err(|error| error.to_string())?;
        let content = tt_encrypt(&gzip.finish().map_err(|error| error.to_string())?)?;
        let url = format!("{REGISTER}?req_id={request}&tt_data=a");
        let exchange = async {
            let response = crate::runtime::http::client()
                .post(url.as_str())
                .timeout(Seconds(20))
                .header("Content-Type", "application/octet-stream;tt-data=a")
                .send_body(content)
                .await
                .map_err(|error| error.to_string())?;
            let bytes = response.body().limit(1 << 20).await.map_err(|error| error.to_string())?;
            serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
        };
        let answered = exchange.await.map_err(|error| format!("cannot register with the speech recognition service: {error}"))?;
        let id = |name: &str| {
            answered
                .get(format!("{name}_str"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| answered.get(name).and_then(Value::as_u64).map(|n| n.to_string()))
                .filter(|id| !id.is_empty() && id != "0")
        };
        let (Some(device_id), Some(install_id)) = (id("device_id"), id("install_id")) else {
            return Err("the speech recognition service did not register this device".into());
        };
        Ok(Self { device_id, install_id, cdid, openudid, clientudid })
    }
}

fn tt_encrypt(gzip: &[u8]) -> Result<Vec<u8>, String> {
    let mut salt = [0u8; 32];
    rand::rng().fill_bytes(&mut salt);
    Ok(tt_seal(gzip, &salt))
}

fn tt_seal(gzip: &[u8], salt: &[u8; 32]) -> Vec<u8> {
    use aes::cipher::{BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7};
    use sha2::{Digest, Sha512};
    let constant: Vec<u8> = (0..TT_CONSTANT.len()).step_by(2).filter_map(|i| u8::from_str_radix(&TT_CONSTANT[i..i + 2], 16).ok()).collect();
    let digest = Sha512::new().chain_update(Sha512::digest(salt)).chain_update(&constant).finalize();
    let payload = [Sha512::digest(gzip).as_slice(), gzip].concat();
    let key: [u8; 16] = digest[..16].try_into().expect("16 bytes");
    let iv: [u8; 16] = digest[16..32].try_into().expect("16 bytes");
    let sealed = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into()).encrypt_padded_vec::<Pkcs7>(&payload);
    [&[0x74, 0x63, 0x05, 0x10, 0x00, 0x00][..], salt, &sealed].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sealed_registration_matches_openssl_byte_for_byte() {
        let salt: [u8; 32] = std::array::from_fn(|i| i as u8);
        let gzip: Vec<u8> = [0x1f, 0x8b].into_iter().chain((0..45u32).map(|i| (i * 7 % 256) as u8)).collect();
        let expected = concat!(
            "746305100000000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f804e02d4ca27f3c35f9a9ea98ba3abfa1e86ca48b61c1170d9a9",
            "3913f1f96c1f7390ac858fc2d6ca0d04d34b2d39cb3efe235e90de7454b76ec43b91494477358fa750ab0b39330a8ea95d7c41bbbeb62ec473027b586f6e66e4",
            "5794043912ce437d51cb1dcfbea7e7f0de3d2c88b431",
        );
        let hex: String = tt_seal(&gzip, &salt).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, expected);
    }

    #[test]
    fn transcript_appends_finished_segments() {
        let mut t = Transcript::default();
        assert_eq!(t.apply("你好".into(), false).as_deref(), Some("你好"));
        assert_eq!(t.apply("你好".into(), false), None);
        assert_eq!(t.apply("你好。".into(), true).as_deref(), Some("你好。"));
        assert_eq!(t.apply("你好。".into(), true), None);
        assert_eq!(t.apply("".into(), false), None);
        assert_eq!(t.apply("再见".into(), false).as_deref(), Some("你好。再见"));
    }

    #[test]
    fn response_round_trip() {
        let mut data = Vec::new();
        field(&mut data, 1, b"task");
        field(&mut data, 4, b"SessionFinished");
        varint(&mut data, 5 << 3);
        varint(&mut data, 20000000);
        field(&mut data, 7, br#"{"results":[{"text":"hi","is_vad_finished":true}]}"#);
        let response = Response::decode(&data).expect("decodes");
        assert_eq!((response.task.as_str(), response.event.as_str(), response.status), ("task", "SessionFinished", 20000000));
        assert_eq!(response.update(), Some(("hi".into(), true)));
        assert!(Response::decode(&data[..data.len() - 1]).is_none());
    }
}
