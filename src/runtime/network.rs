use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use napi::Result;
use serde_json::{Value, json};
use zbus::blocking::{Connection, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value as Variant};

use crate::runtime::failure;
use crate::runtime::sync::Locked;

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const WIRED: &str = "org.freedesktop.NetworkManager.Device.Wired";
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const IP4: &str = "org.freedesktop.NetworkManager.IP4Config";
const SETTINGS: &str = "org.freedesktop.NetworkManager.Settings";
const SETTING: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const ETHERNET: u32 = 1;
const WIFI: u32 = 2;
const SETTLE: Duration = Duration::from_millis(250);

fn bus(error: zbus::Error) -> napi::Error {
    failure("network_failed", &error.to_string())
}

fn device_state(state: u32) -> &'static str {
    match state {
        100 => "connected",
        40..=90 => "connecting",
        110 => "disconnecting",
        120 => "failed",
        30 => "disconnected",
        20 => "unavailable",
        _ => "unmanaged",
    }
}

fn connectivity(state: u32) -> &'static str {
    match state {
        4 => "full",
        3 => "limited",
        2 => "portal",
        1 => "none",
        _ => "unknown",
    }
}

struct Props(HashMap<String, OwnedValue>);

impl Props {
    fn get<T: TryFrom<OwnedValue>>(&self, name: &str) -> zbus::Result<T>
    where
        T::Error: Into<zbus::Error>,
    {
        let value = self.0.get(name).ok_or_else(|| zbus::Error::Failure(format!("no property {name}")))?;
        T::try_from(value.try_clone()?).map_err(Into::into)
    }
}

struct Device {
    path: OwnedObjectPath,
    kind: u32,
    props: Props,
}

struct Seen {
    ssid: String,
    strength: u8,
    secure: bool,
    frequency: u32,
    path: OwnedObjectPath,
}

#[derive(Clone)]
struct Nm {
    connection: Connection,
}

impl Nm {
    fn proxy(&self, path: &str, interface: &'static str) -> zbus::Result<Proxy<'static>> {
        zbus::blocking::proxy::Builder::new(&self.connection)
            .destination(NM)?
            .path(path.to_owned())?
            .interface(interface)?
            .cache_properties(CacheProperties::No)
            .build()
    }

    fn get<T: TryFrom<OwnedValue>>(&self, path: &str, interface: &'static str, name: &str) -> zbus::Result<T>
    where
        T::Error: Into<zbus::Error>,
    {
        self.proxy(path, interface)?.get_property(name)
    }

    fn all(&self, path: &str, interface: &'static str) -> zbus::Result<Props> {
        Ok(Props(self.proxy(path, PROPERTIES)?.call("GetAll", &(interface,))?))
    }

    fn devices(&self) -> zbus::Result<Vec<Device>> {
        let paths: Vec<OwnedObjectPath> = self.proxy(NM_PATH, NM)?.call("GetDevices", &())?;
        let mut out = Vec::new();
        for path in paths {
            let props = self.all(path.as_str(), DEVICE)?;
            let kind: u32 = props.get("DeviceType")?;
            if kind == ETHERNET || kind == WIFI {
                out.push(Device { path, kind, props });
            }
        }
        Ok(out)
    }

    fn wifi_device(&self) -> zbus::Result<Option<OwnedObjectPath>> {
        let managed = |d: &Device| d.props.get::<u32>("State").is_ok_and(|state| device_state(state) != "unmanaged");
        Ok(self.devices()?.into_iter().find(|d| d.kind == WIFI && managed(d)).map(|d| d.path))
    }

    fn device_by_iface(&self, iface: &str) -> Result<OwnedObjectPath> {
        for device in self.devices().map_err(bus)? {
            if device.props.get::<String>("Interface").map_err(bus)? == iface {
                return Ok(device.path);
            }
        }
        Err(failure("no_device", &format!("no device {iface}")))
    }

    fn ip4(&self, config: &OwnedObjectPath) -> Option<String> {
        if config.as_str() == "/" {
            return None;
        }
        let data: Vec<HashMap<String, OwnedValue>> = self.get(config.as_str(), IP4, "AddressData").ok()?;
        let first = data.into_iter().next()?;
        let address = String::try_from(first.get("address")?.try_clone().ok()?).ok()?;
        let prefix = u32::try_from(first.get("prefix")?.try_clone().ok()?).ok()?;
        Some(format!("{address}/{prefix}"))
    }

    fn access_point(&self, path: &OwnedObjectPath) -> zbus::Result<Seen> {
        let p = self.all(path.as_str(), ACCESS_POINT)?;
        let ssid: Vec<u8> = p.get("Ssid")?;
        let flags: u32 = p.get("Flags")?;
        let wpa: u32 = p.get("WpaFlags")?;
        let rsn: u32 = p.get("RsnFlags")?;
        Ok(Seen {
            ssid: String::from_utf8_lossy(&ssid).into_owned(),
            strength: p.get("Strength")?,
            secure: flags & 1 != 0 || wpa != 0 || rsn != 0,
            frequency: p.get("Frequency")?,
            path: path.clone(),
        })
    }

    fn seen(&self, device: &OwnedObjectPath) -> zbus::Result<Vec<Seen>> {
        let paths: Vec<OwnedObjectPath> = self.proxy(device.as_str(), WIRELESS)?.call("GetAllAccessPoints", &())?;
        let mut best: Vec<Seen> = Vec::new();
        for path in paths {
            let Ok(ap) = self.access_point(&path) else { continue };
            if ap.ssid.is_empty() {
                continue;
            }
            match best.iter_mut().find(|b| b.ssid == ap.ssid) {
                Some(b) if b.strength >= ap.strength => {}
                Some(b) => *b = ap,
                None => best.push(ap),
            }
        }
        best.sort_by_key(|ap| std::cmp::Reverse(ap.strength));
        Ok(best)
    }

    fn saved(&self) -> zbus::Result<HashMap<String, OwnedObjectPath>> {
        let paths: Vec<OwnedObjectPath> = self.proxy(SETTINGS_PATH, SETTINGS)?.call("ListConnections", &())?;
        let mut out = HashMap::new();
        for path in paths {
            let settings: HashMap<String, HashMap<String, OwnedValue>> = self.proxy(path.as_str(), SETTING)?.call("GetSettings", &())?;
            let ssid = settings.get("802-11-wireless").and_then(|w| w.get("ssid")).and_then(|v| Vec::<u8>::try_from(v.try_clone().ok()?).ok());
            if let Some(ssid) = ssid {
                out.insert(String::from_utf8_lossy(&ssid).into_owned(), path);
            }
        }
        Ok(out)
    }

    fn state(&self) -> zbus::Result<Value> {
        let nm = self.all(NM_PATH, NM)?;
        let wifi_enabled: bool = nm.get("WirelessEnabled")?;
        let connectivity_state: u32 = nm.get("Connectivity")?;
        let primary: OwnedObjectPath = nm.get("PrimaryConnection")?;
        let primary = if primary.as_str() == "/" {
            Value::Null
        } else {
            match self.get::<String>(primary.as_str(), ACTIVE, "Type")?.as_str() {
                "802-11-wireless" => json!("wifi"),
                "802-3-ethernet" => json!("ethernet"),
                other => json!(other),
            }
        };
        let mut devices = Vec::new();
        for Device { path, kind, props: d } in self.devices()? {
            let iface: String = d.get("Interface")?;
            let state: u32 = d.get("State")?;
            let active: OwnedObjectPath = d.get("ActiveConnection")?;
            let config: OwnedObjectPath = d.get("Ip4Config")?;
            let name = if active.as_str() == "/" { None } else { self.get::<String>(active.as_str(), ACTIVE, "Id").ok() };
            let mut device = json!({
                "iface": iface,
                "type": if kind == WIFI { "wifi" } else { "ethernet" },
                "state": device_state(state),
                "connection": name,
                "ip4": self.ip4(&config),
            });
            if kind == WIFI {
                let ap: OwnedObjectPath = self.get(path.as_str(), WIRELESS, "ActiveAccessPoint")?;
                if ap.as_str() != "/"
                    && let Ok(ap) = self.access_point(&ap)
                {
                    device["ssid"] = json!(ap.ssid);
                    device["strength"] = json!(ap.strength);
                }
            } else {
                let wired = self.all(path.as_str(), WIRED)?;
                device["carrier"] = json!(wired.get::<bool>("Carrier").unwrap_or(false));
                device["speed"] = json!(wired.get::<u32>("Speed").unwrap_or(0));
            }
            devices.push(device);
        }
        Ok(json!({
            "wifiEnabled": wifi_enabled,
            "connectivity": connectivity(connectivity_state),
            "primary": primary,
            "devices": devices,
        }))
    }

    fn networks(&self) -> zbus::Result<Value> {
        let Some(device) = self.wifi_device()? else { return Ok(json!([])) };
        let active: OwnedObjectPath = self.get(device.as_str(), WIRELESS, "ActiveAccessPoint")?;
        let active = if active.as_str() == "/" { None } else { self.access_point(&active).ok().map(|ap| ap.ssid) };
        let saved = self.saved()?;
        Ok(self
            .seen(&device)?
            .into_iter()
            .map(|ap| {
                json!({
                    "ssid": ap.ssid,
                    "strength": ap.strength,
                    "secure": ap.secure,
                    "band": if ap.frequency >= 4900 { "5GHz" } else { "2.4GHz" },
                    "active": active.as_deref() == Some(ap.ssid.as_str()),
                    "saved": saved.contains_key(&ap.ssid),
                })
            })
            .collect())
    }

    fn scan(&self) -> zbus::Result<()> {
        if let Some(device) = self.wifi_device()? {
            let options: HashMap<&str, Variant> = HashMap::new();
            _ = self.proxy(device.as_str(), WIRELESS)?.call::<_, _, ()>("RequestScan", &(options,));
        }
        Ok(())
    }

    fn join(&self, ssid: &str, password: Option<&str>) -> Result<()> {
        let device = self.wifi_device().map_err(bus)?.ok_or_else(|| failure("no_wifi", "this computer has no Wi-Fi"))?;
        let ap = self
            .seen(&device)
            .map_err(bus)?
            .into_iter()
            .find(|ap| ap.ssid == ssid)
            .ok_or_else(|| failure("not_in_range", &format!("{ssid} is not in range")))?;
        let saved = self.saved().map_err(bus)?.remove(ssid);
        let nm = self.proxy(NM_PATH, NM).map_err(bus)?;
        match (saved, password) {
            (Some(connection), None) => {
                nm.call::<_, _, OwnedObjectPath>("ActivateConnection", &(&connection, &device, &ap.path)).map_err(bus)?;
            }
            (Some(connection), Some(password)) => {
                let setting = self.proxy(connection.as_str(), SETTING).map_err(bus)?;
                let mut settings: HashMap<String, HashMap<String, OwnedValue>> = setting.call("GetSettings", &()).map_err(bus)?;
                let security = settings.entry("802-11-wireless-security".into()).or_default();
                security.insert("key-mgmt".into(), Variant::from("wpa-psk").try_into().map_err(|e: zbus::zvariant::Error| bus(e.into()))?);
                security.insert("psk".into(), Variant::from(password).try_into().map_err(|e: zbus::zvariant::Error| bus(e.into()))?);
                setting.call::<_, _, ()>("Update", &(settings,)).map_err(bus)?;
                nm.call::<_, _, OwnedObjectPath>("ActivateConnection", &(&connection, &device, &ap.path)).map_err(bus)?;
            }
            (None, password) => {
                if ap.secure && password.is_none() {
                    return Err(failure("password_required", &format!("{ssid} needs a password")));
                }
                let mut settings: HashMap<&str, HashMap<&str, Variant>> = HashMap::new();
                if let Some(password) = password {
                    settings.insert("802-11-wireless-security", HashMap::from([("key-mgmt", Variant::from("wpa-psk")), ("psk", Variant::from(password))]));
                }
                nm.call::<_, _, (OwnedObjectPath, OwnedObjectPath)>("AddAndActivateConnection", &(settings, &device, &ap.path)).map_err(bus)?;
            }
        }
        Ok(())
    }

    fn activate(&self, iface: &str) -> Result<()> {
        let device = self.device_by_iface(iface)?;
        let none = ObjectPath::try_from("/").map_err(|e| bus(e.into()))?;
        self.proxy(NM_PATH, NM).map_err(bus)?.call::<_, _, OwnedObjectPath>("ActivateConnection", &(&none, &device, &none)).map_err(bus)?;
        Ok(())
    }

    fn disconnect(&self, iface: &str) -> Result<()> {
        let device = self.device_by_iface(iface)?;
        self.proxy(device.as_str(), DEVICE).map_err(bus)?.call::<_, _, ()>("Disconnect", &()).map_err(bus)
    }

    fn set_wifi_enabled(&self, on: bool) -> Result<()> {
        self.proxy(NM_PATH, NM).map_err(bus)?.set_property("WirelessEnabled", on).map_err(|e| bus(e.into()))
    }
}

impl Nm {
    fn open() -> Result<Self> {
        let connection = Connection::system().map_err(|e| failure("network_unavailable", &e.to_string()))?;
        let nm = Nm { connection };
        nm.proxy(NM_PATH, NM)
            .and_then(|p| p.get_property::<String>("Version"))
            .map_err(|e| failure("network_unavailable", &format!("NetworkManager is not running: {e}")))?;
        Ok(nm)
    }

    fn call(&self, call: NetCall) -> Result<Value> {
        match call {
            NetCall::State => self.state().map_err(bus),
            NetCall::Networks { scan } => {
                if scan {
                    self.scan().map_err(bus)?;
                }
                self.networks().map_err(bus)
            }
            NetCall::Connect { ssid, password } => self.join(&ssid, password.as_deref()).map(|()| Value::Null),
            NetCall::Activate { iface } => self.activate(&iface).map(|()| Value::Null),
            NetCall::Disconnect { iface } => self.disconnect(&iface).map(|()| Value::Null),
            NetCall::SetWifiEnabled { on } => self.set_wifi_enabled(on).map(|()| Value::Null),
        }
    }

    fn start_watching(&self, notify: impl Fn(Value) + Send + 'static) -> Result<()> {
        let signals =
            crate::runtime::bus::signals(&self.connection, |b| b.sender(NM)?.path_namespace(NM_PATH)?.interface(PROPERTIES)?.member("PropertiesChanged"))
                .map_err(bus)?;
        let (tx, rx) = mpsc::channel::<()>();
        crate::runtime::bus::listen("acde-network", signals, move |_| tx.send(()).is_ok()).map_err(|e| napi::Error::from_reason(e.to_string()))?;
        let nm = self.clone();
        let watching = std::thread::Builder::new().name("acde-network-state".into()).spawn(move || {
            while rx.recv().is_ok() {
                while rx.recv_timeout(SETTLE).is_ok() {}
                if let Ok(state) = nm.state() {
                    notify(state);
                }
            }
        });
        watching.map(drop).map_err(|e| napi::Error::from_reason(e.to_string()))
    }
}

pub enum NetCall {
    State,
    Networks { scan: bool },
    Connect { ssid: String, password: Option<String> },
    Activate { iface: String },
    Disconnect { iface: String },
    SetWifiEnabled { on: bool },
}

type NetSink = Box<dyn Fn(Value) -> bool + Send>;

#[derive(Default)]
struct Hub {
    nm: Option<Nm>,
    sinks: Vec<NetSink>,
    watching: bool,
}

#[derive(Clone, Default)]
pub struct Networks {
    hub: Arc<Mutex<Hub>>,
}

impl Networks {
    fn nm(&self) -> std::result::Result<Nm, String> {
        let mut hub = self.hub.locked();
        if let Some(nm) = &hub.nm {
            return Ok(nm.clone());
        }
        let nm = Nm::open().map_err(|e| e.reason)?;
        hub.nm = Some(nm.clone());
        Ok(nm)
    }

    pub fn call(&self, call: NetCall) -> std::result::Result<Value, String> {
        self.nm()?.call(call).map_err(|e| e.reason)
    }

    pub fn listen(&self, sink: impl Fn(Value) -> bool + Send + 'static) -> std::result::Result<(), String> {
        let nm = self.nm()?;
        let start = {
            let mut hub = self.hub.locked();
            hub.sinks.push(Box::new(sink));
            !std::mem::replace(&mut hub.watching, true)
        };
        if !start {
            return Ok(());
        }
        let hub = Arc::downgrade(&self.hub);
        nm.start_watching(move |state| {
            if let Some(hub) = hub.upgrade() {
                hub.locked().sinks.retain(|sink| sink(state.clone()));
            }
        })
        .map_err(|e| {
            self.hub.locked().watching = false;
            e.reason
        })
    }
}

#[cfg(test)]
mod scanning_tests {
    use super::device_state;

    #[test]
    fn a_device_networkmanager_does_not_manage_is_never_the_wifi_device() {
        assert_eq!(device_state(10), "unmanaged");
        assert_eq!(device_state(0), "unmanaged");
        assert_ne!(device_state(30), "unmanaged");
        assert_ne!(device_state(20), "unmanaged");
    }
}
