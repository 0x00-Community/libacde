use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::Duration;

use futures_channel::oneshot;
use serde_json::{Value, json};
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

use crate::runtime::sync::Locked;

const BLUEZ: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const BATTERY: &str = "org.bluez.Battery1";
const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
const AGENT_PATH: &str = "/org/dopame/acde/bluetooth/agent";
const CAPABILITY: &str = "KeyboardDisplay";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const SETTLE: Duration = Duration::from_millis(250);
const NOISE: [&str; 5] = ["RSSI", "TxPower", "ManufacturerData", "ServiceData", "AdvertisingFlags"];
const LOGIN_MODES: [&str; 3] = ["remember", "on", "off"];

type Props = HashMap<String, OwnedValue>;
type Objects = HashMap<OwnedObjectPath, HashMap<String, Props>>;
type Changed = Box<dyn Fn(Value) -> bool + Send>;
type Asked = Box<dyn Fn(Value) -> Option<bool> + Send>;

pub enum BtCall {
    State,
    SetPowered { on: bool },
    SetDiscoverable { on: bool, timeout: Option<u32> },
    SetName { name: String },
    Scan { on: bool, owner: u64 },
    Pair { id: String },
    Answer { id: String, accept: bool, pin: Option<String> },
    Connect { id: String },
    Disconnect { id: String },
    Forget { id: String },
    SetTrusted { id: String, on: bool },
    SetBlocked { id: String, on: bool },
    Rename { id: String, name: String },
    SetOnLogin { mode: String },
}

fn said(error: zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(_, Some(text), _) => text,
        other => other.to_string(),
    }
}

fn fdo(error: zbus::fdo::Error) -> String {
    match error {
        zbus::fdo::Error::ZBus(error) => said(error),
        other => other.to_string(),
    }
}

fn named(error: &zbus::Error, name: &str) -> bool {
    matches!(error, zbus::Error::MethodError(n, _, _) if n.as_str() == format!("org.bluez.Error.{name}"))
}

fn prop<T: TryFrom<OwnedValue>>(props: &Props, name: &str) -> Option<T> {
    T::try_from(props.get(name)?.try_clone().ok()?).ok()
}

fn kind(icon: &str, class: u32) -> &'static str {
    match icon {
        "audio-headset" => "headset",
        "audio-headphones" | "audio-card" | "audio-speakers" | "multimedia-player" => "audio",
        "input-keyboard" => "keyboard",
        "input-mouse" | "input-tablet" => "mouse",
        "input-gaming" => "gamepad",
        "phone" => "phone",
        "computer" => "computer",
        _ => match (class >> 8) & 0x1f {
            1 => "computer",
            2 => "phone",
            4 => "audio",
            5 => match (class >> 6) & 3 {
                1 => "keyboard",
                2 => "mouse",
                _ if matches!((class >> 2) & 0xf, 1 | 2) => "gamepad",
                _ => "other",
            },
            _ => "other",
        },
    }
}

fn rfkill() -> Option<&'static str> {
    let mut soft = false;
    for entry in std::fs::read_dir("/sys/class/rfkill").ok()?.flatten() {
        let read = |name: &str| std::fs::read_to_string(entry.path().join(name)).unwrap_or_default();
        if read("type").trim() != "bluetooth" {
            continue;
        }
        if read("hard").trim() == "1" {
            return Some("hard");
        }
        soft |= read("soft").trim() == "1";
    }
    soft.then_some("soft")
}

fn unblock() -> Result<(), String> {
    let event = [0u8, 0, 0, 0, 2, 3, 0, 0];
    std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/rfkill")
        .and_then(|mut f| f.write_all(&event))
        .map_err(|e| format!("could not unblock Bluetooth (rfkill): {e}"))
}

fn device_path(id: &str) -> Result<OwnedObjectPath, String> {
    if !id.starts_with("/org/bluez/") {
        return Err(crate::runtime::bad_request(format!("{id} is not a Bluetooth device")));
    }
    OwnedObjectPath::try_from(id.to_owned()).map_err(|e| e.to_string())
}

fn config_file() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("acde").join("bluetooth.json"))
}

struct Login {
    file: Option<PathBuf>,
}

impl Login {
    fn read(&self) -> Value {
        self.file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}))
    }

    fn mode(&self) -> String {
        let mode = self.read()["onLogin"].as_str().unwrap_or_default().to_owned();
        if LOGIN_MODES.contains(&mode.as_str()) { mode } else { "remember".to_owned() }
    }

    fn write(&self, key: &str, value: Value) -> Result<(), String> {
        let Some(file) = &self.file else { return Ok(()) };
        let mut all = self.read();
        if !all.is_object() {
            all = json!({});
        }
        all[key] = value;
        let dir = file.parent().ok_or("no config directory")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, format!("{all:#}\n")).and_then(|()| std::fs::rename(&tmp, file)).map_err(|e| format!("{}: {e}", file.display()))
    }
}

#[derive(Clone)]
struct Bluez {
    connection: Connection,
}

impl Bluez {
    fn proxy(&self, path: &str, interface: &'static str) -> zbus::Result<Proxy<'static>> {
        zbus::blocking::proxy::Builder::new(&self.connection)
            .destination(BLUEZ)?
            .path(path.to_owned())?
            .interface(interface)?
            .cache_properties(CacheProperties::No)
            .build()
    }

    fn objects(&self) -> zbus::Result<Objects> {
        self.proxy("/", OBJECT_MANAGER)?.call("GetManagedObjects", &())
    }

    fn adapters(objects: &Objects) -> Vec<(&OwnedObjectPath, &Props)> {
        let mut out: Vec<_> = objects.iter().filter_map(|(path, ifaces)| Some((path, ifaces.get(ADAPTER)?))).collect();
        out.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        out
    }

    fn adapter(&self) -> Result<OwnedObjectPath, String> {
        let objects = self.objects().map_err(|_| "Bluetooth is not running on this computer".to_owned())?;
        let adapters = Self::adapters(&objects);
        adapters
            .iter()
            .find(|(_, p)| prop::<bool>(p, "Powered").unwrap_or(false))
            .or_else(|| adapters.first())
            .map(|(path, _)| (*path).clone())
            .ok_or_else(|| "this computer has no Bluetooth adapter".to_owned())
    }

    fn register(&self) -> zbus::Result<()> {
        let manager = self.proxy("/org/bluez", AGENT_MANAGER)?;
        let path = ObjectPath::try_from(AGENT_PATH)?;
        if let Err(e) = manager.call::<_, _, ()>("RegisterAgent", &(&path, CAPABILITY))
            && !named(&e, "AlreadyExists")
        {
            return Err(e);
        }
        manager.call("RequestDefaultAgent", &(&path,))
    }

    fn state(&self, login: &Login) -> Value {
        let blocked = rfkill();
        let on_login = login.mode();
        let Ok(objects) = self.objects() else {
            return json!({
                "available": false, "powered": false, "discoverable": false, "discoverableTimeout": 0, "pairable": false,
                "discovering": false, "name": "", "address": "", "blocked": blocked, "onLogin": on_login, "devices": [],
            });
        };
        let adapters = Self::adapters(&objects);
        let main = adapters.iter().find(|(_, p)| prop::<bool>(p, "Powered").unwrap_or(false)).or_else(|| adapters.first());
        let mut devices: Vec<Value> = objects
            .iter()
            .filter_map(|(path, ifaces)| {
                let d = ifaces.get(DEVICE)?;
                let address: String = prop(d, "Address").unwrap_or_default();
                let icon: String = prop(d, "Icon").unwrap_or_default();
                let class: u32 = prop(d, "Class").unwrap_or(0);
                let name: Option<String> = prop(d, "Name");
                let battery = ifaces.get(BATTERY).and_then(|b| prop::<u8>(b, "Percentage"));
                Some(json!({
                    "id": path.as_str(),
                    "address": address,
                    "name": prop::<String>(d, "Alias").or(name.clone()).unwrap_or_else(|| address.clone()),
                    "named": name.is_some(),
                    "kind": kind(&icon, class),
                    "icon": icon,
                    "paired": prop::<bool>(d, "Paired").unwrap_or(false),
                    "connected": prop::<bool>(d, "Connected").unwrap_or(false),
                    "trusted": prop::<bool>(d, "Trusted").unwrap_or(false),
                    "blocked": prop::<bool>(d, "Blocked").unwrap_or(false),
                    "battery": battery,
                    "rssi": prop::<i16>(d, "RSSI"),
                }))
            })
            .collect();
        let rank = |d: &Value| {
            (
                !d["connected"].as_bool().unwrap_or(false),
                !d["paired"].as_bool().unwrap_or(false),
                !d["named"].as_bool().unwrap_or(false),
                -d["rssi"].as_i64().unwrap_or(-200),
                d["name"].as_str().unwrap_or_default().to_lowercase(),
            )
        };
        devices.sort_by_key(rank);
        let a = main.map(|(_, p)| *p);
        let flag = |name: &str| a.and_then(|p| prop::<bool>(p, name)).unwrap_or(false);
        json!({
            "available": a.is_some(),
            "powered": adapters.iter().any(|(_, p)| prop::<bool>(p, "Powered").unwrap_or(false)),
            "discoverable": flag("Discoverable"),
            "discoverableTimeout": a.and_then(|p| prop::<u32>(p, "DiscoverableTimeout")).unwrap_or(0),
            "pairable": flag("Pairable"),
            "discovering": flag("Discovering"),
            "name": a.and_then(|p| prop::<String>(p, "Alias")).unwrap_or_default(),
            "address": a.and_then(|p| prop::<String>(p, "Address")).unwrap_or_default(),
            "blocked": blocked,
            "onLogin": on_login,
            "devices": devices,
        })
    }

    fn set(&self, path: &str, interface: &'static str, name: &str, value: impl Into<zbus::zvariant::Value<'static>> + Send) -> Result<(), String> {
        let proxy = self.proxy(path, interface).map_err(said)?;
        proxy.set_property(name, value.into()).map_err(fdo)
    }

    fn power(&self, on: bool) -> Result<(), String> {
        match rfkill() {
            Some("hard") if on => return Err("Bluetooth is switched off by a hardware switch".to_owned()),
            Some("soft") if on => unblock()?,
            _ => {}
        }
        let mut last = String::new();
        for attempt in 0..20 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(100));
            }
            let objects = match self.objects() {
                Ok(objects) => objects,
                Err(_) => {
                    last = "Bluetooth is not running on this computer".to_owned();
                    continue;
                }
            };
            let adapters: Vec<String> = Self::adapters(&objects).iter().map(|(p, _)| p.to_string()).collect();
            if adapters.is_empty() {
                last = "this computer has no Bluetooth adapter".to_owned();
                if !on {
                    return Ok(());
                }
                continue;
            }
            match adapters.iter().try_for_each(|path| self.set(path, ADAPTER, "Powered", on)) {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
            if !on {
                break;
            }
        }
        Err(last)
    }

    fn device(&self, id: &str) -> Result<Proxy<'static>, String> {
        let path = device_path(id)?;
        self.proxy(path.as_str(), DEVICE).map_err(said)
    }

    fn pair(&self, id: &str) -> Result<(), String> {
        let device = self.device(id)?;
        if let Err(e) = device.call::<_, _, ()>("Pair", &())
            && !named(&e, "AlreadyExists")
        {
            return Err(said(e));
        }
        self.set(id, DEVICE, "Trusted", true)?;
        if let Err(e) = device.call::<_, _, ()>("Connect", &()) {
            log::info!("bluetooth: paired {id}, not connected: {}", said(e));
        }
        Ok(())
    }

    fn forget(&self, id: &str) -> Result<(), String> {
        let path = device_path(id)?;
        let adapter = path.as_str().rsplit_once('/').map(|(a, _)| a.to_owned()).ok_or("no adapter")?;
        self.proxy(&adapter, ADAPTER).map_err(said)?.call::<_, _, ()>("RemoveDevice", &(&path,)).map_err(said)
    }

    fn discoverable(&self, on: bool, timeout: Option<u32>) -> Result<(), String> {
        let adapter = self.adapter()?;
        if let Some(timeout) = timeout {
            self.set(adapter.as_str(), ADAPTER, "DiscoverableTimeout", timeout)?;
        }
        self.set(adapter.as_str(), ADAPTER, "Discoverable", on)?;
        self.set(adapter.as_str(), ADAPTER, "Pairable", on)
    }

    fn discovery(&self, on: bool) -> Result<(), String> {
        let adapter = self.adapter()?;
        let proxy = self.proxy(adapter.as_str(), ADAPTER).map_err(said)?;
        let harmless = |e: &zbus::Error| if on { named(e, "InProgress") } else { named(e, "NotReady") || named(e, "Failed") };
        match proxy.call::<_, _, ()>(if on { "StartDiscovery" } else { "StopDiscovery" }, &()) {
            Err(e) if !harmless(&e) => Err(said(e)),
            _ => Ok(()),
        }
    }
}

struct Reply {
    accept: bool,
    pin: Option<String>,
}

#[derive(Default)]
struct Hub {
    bluez: Option<Bluez>,
    changed: Vec<Changed>,
    asked: Vec<Asked>,
    pending: HashMap<String, oneshot::Sender<Reply>>,
    current: Option<String>,
    scanners: HashSet<u64>,
    watching: bool,
    login: Option<PathBuf>,
}

impl Hub {
    fn ask(&mut self, request: &Value) -> bool {
        let mut any = false;
        self.asked.retain(|sink| match sink(request.clone()) {
            None => false,
            Some(taken) => {
                any |= taken;
                true
            }
        });
        any
    }

    fn tell(&mut self, state: &Value) {
        self.changed.retain(|sink| sink(state.clone()));
    }
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

struct Agent {
    hub: Weak<Mutex<Hub>>,
}

impl Agent {
    async fn alias(connection: &zbus::Connection, device: &OwnedObjectPath) -> String {
        let proxy = zbus::proxy::Builder::<zbus::Proxy>::new(connection)
            .destination(BLUEZ)
            .and_then(|b| b.path(device.clone()))
            .and_then(|b| b.interface(DEVICE))
            .map(|b| b.cache_properties(CacheProperties::No));
        let Ok(proxy) = proxy else { return String::new() };
        match proxy.build().await {
            Ok(proxy) => proxy.get_property::<String>("Alias").await.unwrap_or_default(),
            Err(_) => String::new(),
        }
    }

    async fn request(connection: &zbus::Connection, device: &OwnedObjectPath, kind: &str, extra: Value) -> Value {
        let mut request = json!({ "device": device.as_str(), "name": Self::alias(connection, device).await, "kind": kind });
        if let (Some(request), Value::Object(extra)) = (request.as_object_mut(), extra) {
            request.extend(extra);
        }
        request
    }

    fn show(&self, request: &Value) {
        let Some(hub) = self.hub.upgrade() else { return };
        let mut hub = hub.locked();
        hub.current = request["device"].as_str().map(str::to_owned);
        hub.ask(request);
    }

    async fn ask(&self, connection: &zbus::Connection, device: &OwnedObjectPath, kind: &str, extra: Value) -> Result<Option<String>, AgentError> {
        let request = Self::request(connection, device, kind, extra).await;
        let id = device.to_string();
        let (tx, rx) = oneshot::channel();
        {
            let hub = self.hub.upgrade().ok_or_else(|| AgentError::Rejected("acde has stopped".into()))?;
            let mut hub = hub.locked();
            hub.pending.insert(id.clone(), tx);
            hub.current = Some(id.clone());
            if !hub.ask(&request) {
                hub.pending.remove(&id);
                return Err(AgentError::Rejected("nobody is there to answer".into()));
            }
        }
        match rx.await {
            Ok(Reply { accept: true, pin }) => Ok(pin),
            Ok(_) => Err(AgentError::Rejected("declined".into())),
            Err(_) => Err(AgentError::Canceled("canceled".into())),
        }
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    async fn release(&self) {}

    async fn request_pin_code(&self, #[zbus(connection)] connection: &zbus::Connection, device: OwnedObjectPath) -> Result<String, AgentError> {
        let pin = self.ask(connection, &device, "pin", json!({})).await?.unwrap_or_default();
        if pin.is_empty() || pin.len() > 16 {
            return Err(AgentError::Rejected("a PIN code has 1 to 16 characters".into()));
        }
        Ok(pin)
    }

    async fn display_pin_code(&self, #[zbus(connection)] connection: &zbus::Connection, device: OwnedObjectPath, pincode: String) {
        let request = Self::request(connection, &device, "display", json!({ "passkey": pincode })).await;
        self.show(&request);
    }

    async fn request_passkey(&self, #[zbus(connection)] connection: &zbus::Connection, device: OwnedObjectPath) -> Result<u32, AgentError> {
        let pin = self.ask(connection, &device, "passkey", json!({})).await?.unwrap_or_default();
        pin.trim().parse::<u32>().ok().filter(|n| *n <= 999_999).ok_or_else(|| AgentError::Rejected("a passkey is a number up to 999999".into()))
    }

    async fn display_passkey(&self, #[zbus(connection)] connection: &zbus::Connection, device: OwnedObjectPath, passkey: u32, entered: u16) {
        let request = Self::request(connection, &device, "display", json!({ "passkey": format!("{passkey:06}"), "entered": entered })).await;
        self.show(&request);
    }

    async fn request_confirmation(&self, #[zbus(connection)] connection: &zbus::Connection, device: OwnedObjectPath, passkey: u32) -> Result<(), AgentError> {
        self.ask(connection, &device, "confirm", json!({ "passkey": format!("{passkey:06}") })).await.map(drop)
    }

    async fn request_authorization(&self, #[zbus(connection)] connection: &zbus::Connection, device: OwnedObjectPath) -> Result<(), AgentError> {
        self.ask(connection, &device, "authorize", json!({})).await.map(drop)
    }

    async fn authorize_service(&self, #[zbus(connection)] connection: &zbus::Connection, device: OwnedObjectPath, uuid: String) -> Result<(), AgentError> {
        self.ask(connection, &device, "service", json!({ "uuid": uuid })).await.map(drop)
    }

    async fn cancel(&self) {
        let Some(hub) = self.hub.upgrade() else { return };
        let mut hub = hub.locked();
        let Some(id) = hub.current.take() else { return };
        hub.pending.remove(&id);
        hub.ask(&json!({ "device": id, "name": "", "kind": "cancel" }));
    }
}

#[derive(Clone, Default)]
pub struct Bluetooths {
    hub: Arc<Mutex<Hub>>,
    opening: Arc<Mutex<()>>,
}

impl Bluetooths {
    #[cfg(test)]
    fn on(connection: Connection, login: PathBuf) -> std::result::Result<Self, String> {
        let this = Self::default();
        this.hub.locked().login = Some(login);
        this.start(Bluez { connection })?;
        Ok(this)
    }

    fn login(&self) -> Login {
        Login { file: self.hub.locked().login.clone().or_else(config_file) }
    }

    fn start(&self, bluez: Bluez) -> std::result::Result<(), String> {
        let agent = Agent { hub: Arc::downgrade(&self.hub) };
        bluez.connection.object_server().at(AGENT_PATH, agent).map_err(said)?;
        if let Err(e) = bluez.register() {
            log::warn!("bluetooth: no pairing agent yet: {}", said(e));
        }
        {
            let mut hub = self.hub.locked();
            if hub.login.is_none() {
                hub.login = config_file();
            }
            hub.bluez = Some(bluez.clone());
        }
        self.follow_owner(&bluez);
        Ok(())
    }

    fn bluez(&self) -> std::result::Result<Bluez, String> {
        if let Some(bluez) = self.hub.locked().bluez.clone() {
            return Ok(bluez);
        }
        let _opening = self.opening.locked();
        if let Some(bluez) = self.hub.locked().bluez.clone() {
            return Ok(bluez);
        }
        let connection = Connection::system().map_err(|e| format!("no system bus: {e}"))?;
        let bluez = Bluez { connection };
        self.start(bluez.clone())?;
        Ok(bluez)
    }

    fn follow_owner(&self, bluez: &Bluez) {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|b| b.interface("org.freedesktop.DBus"))
            .and_then(|b| b.member("NameOwnerChanged"))
            .and_then(|b| b.add_arg(BLUEZ))
            .map(|b| b.build());
        let signals = match rule.and_then(|rule| MessageIterator::for_match_rule(rule, &bluez.connection, None)) {
            Ok(signals) => signals,
            Err(e) => return log::warn!("bluetooth: not following bluetoothd: {e}"),
        };
        let hub = Arc::downgrade(&self.hub);
        let bluez = bluez.clone();
        let started = std::thread::Builder::new().name("acde-bluetooth-owner".into()).spawn(move || {
            for message in signals {
                let Ok(message) = message else { break };
                let Ok((_, _, owner)) = message.body().deserialize::<(String, String, String)>() else { continue };
                if !owner.is_empty()
                    && let Err(e) = bluez.register()
                {
                    log::warn!("bluetooth: no pairing agent: {}", said(e));
                }
                let Some(hub) = hub.upgrade() else { break };
                let login = Login { file: hub.locked().login.clone() };
                let state = bluez.state(&login);
                hub.locked().tell(&state);
            }
        });
        if let Err(e) = started {
            log::warn!("bluetooth: {e}");
        }
    }

    pub fn call(&self, call: BtCall) -> std::result::Result<Value, String> {
        let bluez = self.bluez()?;
        let login = self.login();
        let done = |r: std::result::Result<(), String>| r.map(|()| Value::Null);
        match call {
            BtCall::State => Ok(bluez.state(&login)),
            BtCall::SetPowered { on } => {
                bluez.power(on)?;
                login.write("powered", json!(on)).map(|()| Value::Null)
            }
            BtCall::SetDiscoverable { on, timeout } => done(bluez.discoverable(on, timeout)),
            BtCall::SetName { name } => {
                let name = name.trim().to_owned();
                if name.is_empty() || name.len() > 248 {
                    return Err("a Bluetooth name has 1 to 248 bytes".to_owned());
                }
                let adapter = bluez.adapter()?;
                done(bluez.set(adapter.as_str(), ADAPTER, "Alias", name))
            }
            BtCall::Scan { on, owner } => done(self.scan(&bluez, on, owner)),
            BtCall::Pair { id } => done(bluez.pair(&id)),
            BtCall::Answer { id, accept, pin } => {
                let pending = self.hub.locked().pending.remove(&id);
                match pending {
                    Some(tx) => {
                        _ = tx.send(Reply { accept, pin });
                        Ok(Value::Null)
                    }
                    None if !accept => {
                        _ = bluez.device(&id)?.call::<_, _, ()>("CancelPairing", &());
                        Ok(Value::Null)
                    }
                    None => Err("nothing is waiting for this answer".to_owned()),
                }
            }
            BtCall::Connect { id } => bluez.device(&id)?.call::<_, _, ()>("Connect", &()).map(|()| Value::Null).map_err(said),
            BtCall::Disconnect { id } => bluez.device(&id)?.call::<_, _, ()>("Disconnect", &()).map(|()| Value::Null).map_err(said),
            BtCall::Forget { id } => done(bluez.forget(&id)),
            BtCall::SetTrusted { id, on } => done(device_path(&id).and_then(|p| bluez.set(p.as_str(), DEVICE, "Trusted", on))),
            BtCall::SetBlocked { id, on } => done(device_path(&id).and_then(|p| bluez.set(p.as_str(), DEVICE, "Blocked", on))),
            BtCall::Rename { id, name } => done(device_path(&id).and_then(|p| bluez.set(p.as_str(), DEVICE, "Alias", name.trim().to_owned()))),
            BtCall::SetOnLogin { mode } => {
                if !LOGIN_MODES.contains(&mode.as_str()) {
                    return Err(format!("{mode}: not remember, on or off"));
                }
                login.write("onLogin", json!(mode)).map(|()| Value::Null)
            }
        }
    }

    fn scan(&self, bluez: &Bluez, on: bool, owner: u64) -> std::result::Result<(), String> {
        let start = {
            let mut hub = self.hub.locked();
            let was = hub.scanners.len();
            if on {
                hub.scanners.insert(owner);
            } else {
                hub.scanners.remove(&owner);
            }
            match (was, hub.scanners.len()) {
                (0, 1) => Some(true),
                (1, 0) => Some(false),
                _ => None,
            }
        };
        let Some(start) = start else { return Ok(()) };
        let result = bluez.discovery(start);
        if result.is_err() && start {
            self.hub.locked().scanners.remove(&owner);
        }
        result
    }

    pub fn release(&self, owner: u64) {
        if self.hub.locked().scanners.contains(&owner) {
            let this = self.clone();
            _ = std::thread::Builder::new().name("acde-bluetooth".into()).spawn(move || {
                if let Err(e) = this.call(BtCall::Scan { on: false, owner }) {
                    log::debug!("bluetooth: stopping discovery: {e}");
                }
            });
        }
    }

    pub fn listen(&self, sink: impl Fn(Value) -> bool + Send + 'static) -> std::result::Result<(), String> {
        let bluez = self.bluez()?;
        let start = {
            let mut hub = self.hub.locked();
            hub.changed.push(Box::new(sink));
            !std::mem::replace(&mut hub.watching, true)
        };
        if !start {
            return Ok(());
        }
        self.watch(&bluez).inspect_err(|_| self.hub.locked().watching = false)
    }

    pub fn pairing(&self, sink: impl Fn(Value) -> Option<bool> + Send + 'static) -> std::result::Result<(), String> {
        self.bluez()?;
        self.hub.locked().asked.push(Box::new(sink));
        Ok(())
    }

    fn watch(&self, bluez: &Bluez) -> std::result::Result<(), String> {
        let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(BLUEZ).map_err(said)?.build();
        let signals = MessageIterator::for_match_rule(rule, &bluez.connection, None).map_err(said)?;
        let (tx, rx) = mpsc::channel::<()>();
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| std::thread::Builder::new().name(name.into()).spawn(f).map(drop).map_err(|e| e.to_string());
        spawn(
            "acde-bluetooth",
            Box::new(move || {
                for message in signals {
                    let Ok(message) = message else { break };
                    if Self::noise(&message) {
                        continue;
                    }
                    if tx.send(()).is_err() {
                        break;
                    }
                }
            }),
        )?;
        let hub = Arc::downgrade(&self.hub);
        let bluez = bluez.clone();
        spawn(
            "acde-bluetooth-state",
            Box::new(move || {
                while rx.recv().is_ok() {
                    while rx.recv_timeout(SETTLE).is_ok() {}
                    let Some(hub) = hub.upgrade() else { break };
                    let login = Login { file: hub.locked().login.clone() };
                    let state = bluez.state(&login);
                    hub.locked().tell(&state);
                }
            }),
        )
    }

    fn noise(message: &zbus::Message) -> bool {
        let header = message.header();
        if header.member().is_none_or(|m| m.as_str() != "PropertiesChanged") || header.interface().is_none_or(|i| i.as_str() != PROPERTIES) {
            return false;
        }
        let Ok((interface, changed, invalidated)) = message.body().deserialize::<(String, Props, Vec<String>)>() else { return false };
        interface == DEVICE && invalidated.is_empty() && changed.keys().all(|k| NOISE.contains(&k.as_str()))
    }

    pub fn restore(&self) {
        let saved = self.login().read();
        let mode = saved["onLogin"].as_str().unwrap_or("remember");
        let target = match mode {
            "on" => Some(true),
            "off" => Some(false),
            _ => saved["powered"].as_bool(),
        };
        let Some(on) = target else { return };
        let mut last = String::new();
        for attempt in 0..10 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(1));
            }
            match self.bluez().and_then(|bluez| bluez.power(on)) {
                Ok(()) => return log::info!("bluetooth: {} at login ({mode})", if on { "on" } else { "off" }),
                Err(e) => last = e,
            }
        }
        log::warn!("bluetooth: not restored at login: {last}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::private_bus;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn kinds() {
        assert_eq!(kind("audio-headset", 0), "headset");
        assert_eq!(kind("input-gaming", 0), "gamepad");
        assert_eq!(kind("", 0x240404), "audio");
        assert_eq!(kind("", 0x002540), "keyboard");
        assert_eq!(kind("", 0x002580), "mouse");
        assert_eq!(kind("", 0x5a020c), "phone");
        assert_eq!(kind("weird", 0), "other");
    }

    #[test]
    fn device_ids() {
        assert!(device_path("/org/bluez/hci0/dev_00_11_22_33_44_55").is_ok());
        assert!(device_path("/org/freedesktop/NetworkManager").is_err());
    }

    struct StandAdapter {
        powered: bool,
        discoverable: bool,
        pairable: bool,
        discovering: bool,
        alias: String,
    }

    #[zbus::interface(name = "org.bluez.Adapter1")]
    impl StandAdapter {
        #[zbus(property)]
        fn powered(&self) -> bool {
            self.powered
        }
        #[zbus(property)]
        fn set_powered(&mut self, on: bool) {
            self.powered = on;
        }
        #[zbus(property)]
        fn discoverable(&self) -> bool {
            self.discoverable
        }
        #[zbus(property)]
        fn set_discoverable(&mut self, on: bool) {
            self.discoverable = on;
        }
        #[zbus(property)]
        fn pairable(&self) -> bool {
            self.pairable
        }
        #[zbus(property)]
        fn set_pairable(&mut self, on: bool) {
            self.pairable = on;
        }
        #[zbus(property)]
        fn discovering(&self) -> bool {
            self.discovering
        }
        #[zbus(property)]
        fn alias(&self) -> String {
            self.alias.clone()
        }
        #[zbus(property)]
        fn set_alias(&mut self, alias: String) {
            self.alias = alias;
        }
        #[zbus(property)]
        fn address(&self) -> String {
            "44:0F:B4:00:00:01".into()
        }
        async fn start_discovery(&mut self) {
            self.discovering = true;
        }
        async fn stop_discovery(&mut self) {
            self.discovering = false;
        }
        async fn remove_device(&self, #[zbus(object_server)] server: &zbus::ObjectServer, device: OwnedObjectPath) -> zbus::fdo::Result<()> {
            server.remove::<StandDevice, _>(device.as_str()).await?;
            Ok(())
        }
    }

    struct StandDevice {
        alias: String,
        paired: AtomicBool,
        trusted: bool,
        connected: bool,
    }

    #[zbus::interface(name = "org.bluez.Device1")]
    impl StandDevice {
        #[zbus(property)]
        fn alias(&self) -> String {
            self.alias.clone()
        }
        #[zbus(property)]
        fn name(&self) -> String {
            self.alias.clone()
        }
        #[zbus(property)]
        fn address(&self) -> String {
            "00:11:22:33:44:55".into()
        }
        #[zbus(property)]
        fn icon(&self) -> String {
            "audio-headset".into()
        }
        #[zbus(property)]
        fn paired(&self) -> bool {
            self.paired.load(Ordering::SeqCst)
        }
        #[zbus(property)]
        fn trusted(&self) -> bool {
            self.trusted
        }
        #[zbus(property)]
        fn set_trusted(&mut self, on: bool) {
            self.trusted = on;
        }
        #[zbus(property)]
        fn connected(&self) -> bool {
            self.connected
        }
        async fn pair(&self, #[zbus(connection)] connection: &zbus::Connection, #[zbus(object_server)] server: &zbus::ObjectServer) -> zbus::fdo::Result<()> {
            let agent = server.interface::<_, StandManager>("/org/bluez").await?.get().await.agent.clone();
            let Some((owner, path)) = agent else { return Err(zbus::fdo::Error::Failed("no agent".into())) };
            let device = ObjectPath::try_from("/org/bluez/hci0/dev_00_11_22_33_44_55").unwrap();
            connection
                .call_method(Some(owner.as_str()), path.as_str(), Some("org.bluez.Agent1"), "RequestConfirmation", &(device, 123_456u32))
                .await
                .map_err(|e| zbus::fdo::Error::Failed(said(e)))?;
            self.paired.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn connect(&mut self) {
            self.connected = true;
        }
    }

    struct StandBattery;

    #[zbus::interface(name = "org.bluez.Battery1")]
    impl StandBattery {
        #[zbus(property)]
        fn percentage(&self) -> u8 {
            73
        }
    }

    #[derive(Default)]
    struct StandManager {
        agent: Option<(String, String)>,
    }

    #[zbus::interface(name = "org.bluez.AgentManager1")]
    impl StandManager {
        async fn register_agent(&mut self, #[zbus(header)] header: zbus::message::Header<'_>, agent: OwnedObjectPath, capability: String) {
            assert_eq!(capability, CAPABILITY);
            self.agent = Some((header.sender().map(|s| s.to_string()).unwrap_or_default(), agent.to_string()));
        }
        async fn request_default_agent(&self, _agent: OwnedObjectPath) {}
    }

    #[test]
    #[ignore = "needs bluetoothd and an adapter; changes its name and visibility for a moment"]
    fn against_this_computers_bluez() {
        let bt = Bluetooths::default();
        let settled = |key: &str, want: Value| {
            (0..40).any(|_| {
                std::thread::sleep(Duration::from_millis(50));
                bt.call(BtCall::State).unwrap()[key] == want
            })
        };
        let state = bt.call(BtCall::State).unwrap();
        println!("{state:#}");
        assert_eq!(state["available"], true);
        let name = state["name"].as_str().unwrap().to_owned();
        bt.call(BtCall::SetName { name: format!("{name}-t") }).unwrap();
        assert!(settled("name", json!(format!("{name}-t"))));
        std::thread::sleep(Duration::from_secs(1));
        bt.call(BtCall::SetName { name: name.clone() }).unwrap();
        std::thread::sleep(Duration::from_secs(1));
        assert!(settled("name", json!(name)));
        let visible = state["discoverable"].as_bool().unwrap();
        bt.call(BtCall::SetDiscoverable { on: true, timeout: None }).unwrap();
        assert!(settled("discoverable", json!(true)));
        bt.call(BtCall::SetDiscoverable { on: visible, timeout: None }).unwrap();
        if state["powered"] == true {
            bt.call(BtCall::Scan { on: true, owner: 1 }).unwrap();
            std::thread::sleep(Duration::from_secs(6));
            let seen = bt.call(BtCall::State).unwrap();
            assert_eq!(seen["discovering"], true);
            for d in seen["devices"].as_array().unwrap() {
                println!("{} {} {} paired={} battery={} rssi={}", d["kind"], d["name"], d["icon"], d["paired"], d["battery"], d["rssi"]);
            }
            bt.call(BtCall::Scan { on: false, owner: 1 }).unwrap();
        }
    }

    #[test]
    fn against_a_stand_in_bluez() {
        let Some((mut daemon, address, dir)) = private_bus("bluetooth") else {
            eprintln!("no dbus-daemon: skipped");
            return;
        };
        let device = "/org/bluez/hci0/dev_00_11_22_33_44_55";
        let _bluez = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .name(BLUEZ)
            .unwrap()
            .serve_at("/", zbus::fdo::ObjectManager)
            .unwrap()
            .serve_at("/org/bluez", StandManager::default())
            .unwrap()
            .serve_at("/org/bluez/hci0", StandAdapter { powered: true, discoverable: false, pairable: false, discovering: false, alias: "box".into() })
            .unwrap()
            .serve_at(device, StandDevice { alias: "Buds".into(), paired: AtomicBool::new(false), trusted: false, connected: false })
            .unwrap()
            .serve_at(device, StandBattery)
            .unwrap()
            .build()
            .unwrap();
        let client = zbus::blocking::connection::Builder::address(address.as_str()).unwrap().build().unwrap();
        let bt = Bluetooths::on(client, dir.join("bluetooth.json")).unwrap();

        let state = bt.call(BtCall::State).unwrap();
        assert_eq!(state["available"], true);
        assert_eq!(state["powered"], true);
        assert_eq!(state["name"], "box");
        assert_eq!(state["onLogin"], "remember");
        let d = &state["devices"][0];
        assert_eq!((d["id"].as_str(), d["name"].as_str(), d["kind"].as_str()), (Some(device), Some("Buds"), Some("headset")));
        assert_eq!(d["battery"], 73);
        assert_eq!(d["paired"], false);

        let (changes, changed) = mpsc::channel();
        bt.listen(move |state| changes.send(state).is_ok()).unwrap();
        bt.call(BtCall::SetPowered { on: false }).unwrap();
        let seen = changed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(seen["powered"], false);
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("bluetooth.json")).unwrap()).unwrap();
        assert_eq!(saved["powered"], false);
        bt.call(BtCall::SetPowered { on: true }).unwrap();
        bt.call(BtCall::SetOnLogin { mode: "off".into() }).unwrap();
        assert!(bt.call(BtCall::SetOnLogin { mode: "maybe".into() }).is_err());
        bt.call(BtCall::SetName { name: " desk ".into() }).unwrap();
        bt.call(BtCall::SetDiscoverable { on: true, timeout: None }).unwrap();
        bt.call(BtCall::Scan { on: true, owner: 1 }).unwrap();
        bt.call(BtCall::Scan { on: true, owner: 2 }).unwrap();
        bt.call(BtCall::Scan { on: false, owner: 1 }).unwrap();
        let state = bt.call(BtCall::State).unwrap();
        assert_eq!((state["name"].as_str(), state["discoverable"].as_bool(), state["pairable"].as_bool()), (Some("desk"), Some(true), Some(true)));
        assert_eq!((state["onLogin"].as_str(), state["discovering"].as_bool()), (Some("off"), Some(true)));
        bt.call(BtCall::Scan { on: false, owner: 2 }).unwrap();
        assert_eq!(bt.call(BtCall::State).unwrap()["discovering"], false);

        let wanted = Arc::new(AtomicBool::new(false));
        let (asks, asked) = mpsc::channel();
        let flag = wanted.clone();
        bt.pairing(move |request| Some(flag.load(Ordering::SeqCst) && asks.send(request).is_ok())).unwrap();
        let refused = bt.call(BtCall::Pair { id: device.into() });
        assert!(refused.is_err(), "{refused:?}");

        wanted.store(true, Ordering::SeqCst);
        let pairer = bt.clone();
        let pairing = std::thread::spawn(move || pairer.call(BtCall::Pair { id: device.into() }));
        let request = asked.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!((request["kind"].as_str(), request["passkey"].as_str(), request["name"].as_str()), (Some("confirm"), Some("123456"), Some("Buds")));
        bt.call(BtCall::Answer { id: device.into(), accept: true, pin: None }).unwrap();
        pairing.join().unwrap().unwrap();
        let d = &bt.call(BtCall::State).unwrap()["devices"][0];
        assert_eq!((d["paired"].as_bool(), d["trusted"].as_bool(), d["connected"].as_bool()), (Some(true), Some(true), Some(true)));
        assert!(bt.call(BtCall::Answer { id: device.into(), accept: true, pin: None }).is_err());

        bt.call(BtCall::Forget { id: device.into() }).unwrap();
        assert_eq!(bt.call(BtCall::State).unwrap()["devices"].as_array().map(Vec::len), Some(0));
        assert!(bt.call(BtCall::Connect { id: "/etc/passwd".into() }).is_err());

        _ = daemon.kill();
        _ = daemon.wait();
        _ = std::fs::remove_dir_all(dir);
    }
}
