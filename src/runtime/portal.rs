use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use napi::bindgen_prelude::Unknown;
use napi::{Env, Result};
use napi_derive::napi;
use serde::Deserialize;
use zbus::blocking::{Connection, MessageIterator, Proxy};

type Options<'a> = HashMap<&'static str, Variant<'a>>;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value as Variant};

use crate::runtime::js::{Watch, notify};
use crate::runtime::{blocking, failure};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const FILE_CHOOSER: &str = "org.freedesktop.portal.FileChooser";
const REQUEST: &str = "org.freedesktop.portal.Request";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";
const COLOR_SCHEME: &str = "color-scheme";
const REDUCED_MOTION: &str = "reduced-motion";
const ACDE_APPEARANCE: &str = "org.acde.appearance";
const GLASS: &str = "glass";
const REDUCED_TRANSPARENCY: &str = "reduced-transparency";

static TOKENS: AtomicU64 = AtomicU64::new(1);

fn bus(error: zbus::Error) -> napi::Error {
    failure("portal_failed", &error.to_string())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Filter {
    pub name: String,
    pub globs: Vec<String>,
    pub mimes: Vec<String>,
}

fn filters(list: &[Filter]) -> Vec<(String, Vec<(u32, String)>)> {
    list.iter()
        .map(|f| {
            let patterns = f.globs.iter().map(|g| (0, g.clone())).chain(f.mimes.iter().map(|m| (1, m.clone()))).collect();
            (f.name.clone(), patterns)
        })
        .filter(|(_, patterns): &(String, Vec<(u32, String)>)| !patterns.is_empty())
        .collect()
}

fn options<'a>(pairs: impl IntoIterator<Item = (&'static str, Variant<'a>)>, list: &[Filter]) -> Options<'a> {
    let mut map: Options<'a> = pairs.into_iter().collect();
    let list = filters(list);
    if !list.is_empty() {
        map.insert("filters", Variant::from(list));
    }
    map
}

fn local_path(uri: &str) -> Option<String> {
    url::Url::parse(uri).ok()?.to_file_path().ok()?.into_os_string().into_string().ok()
}

struct FileChooser {
    connection: Connection,
}

impl FileChooser {
    fn request(&self, method: &str, title: &str, mut options: Options<'_>) -> zbus::Result<Option<Vec<String>>> {
        let token = format!("acde{}_{}", std::process::id(), TOKENS.fetch_add(1, Ordering::Relaxed));
        options.insert("handle_token", Variant::from(token));
        let signals = crate::runtime::bus::signals(&self.connection, |b| b.interface(REQUEST)?.member("Response"))?;
        let proxy = Proxy::new(&self.connection, PORTAL, PORTAL_PATH, FILE_CHOOSER)?;
        let handle: OwnedObjectPath = proxy.call(method, &("", title, options))?;
        for message in signals {
            let message = message?;
            if message.header().path().map(|p| p.as_str()) != Some(handle.as_str()) {
                continue;
            }
            let (response, results): (u32, HashMap<String, OwnedValue>) = message.body().deserialize()?;
            if response != 0 {
                return Ok(None);
            }
            let uris: Vec<String> = results.get("uris").and_then(|v| Vec::<String>::try_from(v.clone()).ok()).unwrap_or_default();
            let paths: Vec<String> = uris.iter().filter_map(|u| local_path(u)).collect();
            return Ok(if paths.is_empty() { None } else { Some(paths) });
        }
        Ok(None)
    }

    fn on(connection: Connection) -> zbus::Result<Self> {
        Proxy::new(&connection, PORTAL, PORTAL_PATH, FILE_CHOOSER)?.get_property::<u32>("version")?;
        Ok(Self { connection })
    }

    fn folder(&self, title: &str) -> zbus::Result<Option<String>> {
        Ok(self.ask_open(title, false, true, &[])?.and_then(|paths| paths.into_iter().next()))
    }

    fn ask_open(&self, title: &str, multiple: bool, directory: bool, filters: &[Filter]) -> zbus::Result<Option<Vec<String>>> {
        let map = options([("multiple", Variant::from(multiple)), ("directory", Variant::from(directory))], filters);
        self.request("OpenFile", title, map)
    }

    fn ask_save(&self, title: &str, name: &str, filters: &[Filter]) -> zbus::Result<Option<String>> {
        let name = (!name.is_empty()).then(|| ("current_name", Variant::from(name)));
        Ok(self.request("SaveFile", title, options(name, filters))?.and_then(|paths| paths.into_iter().next()))
    }

    fn session() -> std::result::Result<Self, String> {
        let connection = Connection::session().map_err(|e| format!("no session bus: {e}"))?;
        Self::on(connection).map_err(|e| format!("no FileChooser portal: {e}"))
    }
}

fn asked<T>(answer: zbus::Result<T>) -> std::result::Result<T, String> {
    answer.map_err(|e| format!("FileChooser portal: {e}"))
}

pub fn choose_folder(title: &str) -> std::result::Result<Option<String>, String> {
    asked(FileChooser::session()?.folder(title))
}

pub fn open_files(title: &str, multiple: bool, directory: bool, filters: &[Filter]) -> std::result::Result<Option<Vec<String>>, String> {
    asked(FileChooser::session()?.ask_open(title, multiple, directory, filters))
}

pub fn save_file(title: &str, name: &str, filters: &[Filter]) -> std::result::Result<Option<String>, String> {
    asked(FileChooser::session()?.ask_save(title, name, filters))
}

fn dark_scheme(value: &Variant<'_>) -> bool {
    number(value) == Some(1)
}

fn number(value: &Variant<'_>) -> Option<u32> {
    match value {
        Variant::Value(inner) => number(inner),
        Variant::U32(n) => Some(*n),
        _ => None,
    }
}

fn appearance_change(namespace: &str, key: &str, value: &Variant<'_>) -> Option<serde_json::Value> {
    match (namespace, key) {
        (APPEARANCE, COLOR_SCHEME) => Some(serde_json::json!({ "dark": dark_scheme(value) })),
        (APPEARANCE, REDUCED_MOTION) => Some(serde_json::json!({ "reducedMotion": number(value) == Some(1) })),
        (ACDE_APPEARANCE, GLASS) => Some(serde_json::json!({ "glass": number(value).unwrap_or(0) })),
        (ACDE_APPEARANCE, REDUCED_TRANSPARENCY) => Some(serde_json::json!({ "reducedTransparency": number(value) == Some(1) })),
        _ => None,
    }
}

#[napi]
pub struct Appearance {
    connection: Connection,
    watch: Watch<serde_json::Value>,
}

impl Appearance {
    fn setting(connection: &Connection, namespace: &str, key: &str) -> zbus::Result<OwnedValue> {
        let proxy = Proxy::new(connection, PORTAL, PORTAL_PATH, SETTINGS)?;
        match proxy.call("ReadOne", &(namespace, key)) {
            Ok(value) => Ok(value),
            Err(_) => proxy.call("Read", &(namespace, key)),
        }
    }

    fn read(connection: &Connection) -> zbus::Result<bool> {
        let value = Self::setting(connection, APPEARANCE, COLOR_SCHEME)?;
        Ok(dark_scheme(&value))
    }

    fn read_glass(connection: &Connection) -> serde_json::Value {
        let read = |namespace, key| Self::setting(connection, namespace, key).ok().and_then(|value| number(&value));
        serde_json::json!({
            "glass": read(ACDE_APPEARANCE, GLASS).unwrap_or(0),
            "reducedTransparency": read(ACDE_APPEARANCE, REDUCED_TRANSPARENCY) == Some(1),
            "reducedMotion": read(APPEARANCE, REDUCED_MOTION) == Some(1),
        })
    }

    fn changes(connection: &Connection) -> zbus::Result<MessageIterator> {
        crate::runtime::bus::signals(connection, |b| b.path(PORTAL_PATH)?.interface(SETTINGS)?.member("SettingChanged"))
    }

    fn on(connection: Connection) -> Self {
        Self { connection, watch: Watch::default() }
    }
}

#[napi]
impl Appearance {
    #[napi(factory)]
    pub async fn open() -> Result<Self> {
        blocking(|| Connection::session().map(Self::on).map_err(|e| failure("portal_unavailable", &format!("no session bus: {e}")))).await
    }

    #[napi]
    pub async fn dark(&self) -> Result<bool> {
        let connection = self.connection.clone();
        blocking(move || Self::read(&connection).map_err(bus)).await
    }

    #[napi(ts_return_type = "Promise<{ glass: number; reducedTransparency: boolean; reducedMotion: boolean }>")]
    pub async fn glass(&self) -> Result<serde_json::Value> {
        let connection = self.connection.clone();
        blocking(move || Ok(Self::read_glass(&connection))).await
    }

    #[napi(ts_args_type = "callback: (change: { dark?: boolean; glass?: number; reducedTransparency?: boolean; reducedMotion?: boolean }) => void")]
    pub fn watch(&self, env: Env, callback: Unknown<'_>) -> Result<()> {
        self.watch.watch(&env, callback, |sink| {
            let signals = Self::changes(&self.connection).map_err(bus)?;
            let listened = crate::runtime::bus::listen("acde-appearance", signals, move |message| {
                if let Ok((namespace, key, value)) = message.body().deserialize::<(String, String, OwnedValue)>()
                    && let Some(change) = appearance_change(&namespace, &key, &value)
                {
                    notify(&sink, change);
                }
                true
            });
            listened.map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::private_bus;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    type Calls = Arc<Mutex<Vec<(String, String, HashMap<String, OwnedValue>)>>>;

    struct Stand {
        calls: Calls,
    }

    impl Stand {
        async fn answer(
            &self,
            method: &str,
            header: zbus::message::Header<'_>,
            connection: &zbus::Connection,
            title: &str,
            mut options: HashMap<String, OwnedValue>,
        ) -> OwnedObjectPath {
            let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
            let token = options.remove("handle_token").and_then(|v| String::try_from(v).ok()).unwrap_or_default();
            let handle =
                OwnedObjectPath::try_from(format!("/org/freedesktop/portal/desktop/request/{}/{token}", sender.trim_start_matches(':').replace('.', "_")))
                    .unwrap();
            let multiple = options.get("multiple").and_then(|v| bool::try_from(v.clone()).ok()).unwrap_or(false);
            let name = options.get("current_name").and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
            self.calls.lock().unwrap().push((method.into(), title.into(), options));
            let (response, uris): (u32, Vec<String>) = if title.contains("cancel") {
                (1, vec![])
            } else if method == "OpenFile" {
                let all = vec!["file:///tmp/picked/a%20b.txt".to_string(), "file:///tmp/picked/%E4%BD%A0.png".to_string()];
                (0, if multiple { all } else { all[..1].to_vec() })
            } else {
                (0, vec![format!("file:///tmp/picked/{name}")])
            };
            let mut results: HashMap<&str, Variant<'_>> = HashMap::new();
            if response == 0 {
                results.insert("uris", Variant::from(uris));
            }
            connection.emit_signal(Some(sender.as_str()), handle.as_str(), REQUEST, "Response", &(response, results)).await.unwrap();
            handle
        }
    }

    #[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
    impl Stand {
        #[zbus(property, name = "version")]
        fn version(&self) -> u32 {
            4
        }

        async fn open_file(
            &self,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
            _parent: &str,
            title: &str,
            options: HashMap<String, OwnedValue>,
        ) -> OwnedObjectPath {
            self.answer("OpenFile", header, connection, title, options).await
        }

        async fn save_file(
            &self,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
            _parent: &str,
            title: &str,
            options: HashMap<String, OwnedValue>,
        ) -> OwnedObjectPath {
            self.answer("SaveFile", header, connection, title, options).await
        }
    }

    #[test]
    fn against_a_stand_in_portal() {
        let Some((mut daemon, address, dir)) = private_bus("portal") else {
            eprintln!("no dbus-daemon: skipped");
            return;
        };
        let connect = || zbus::blocking::connection::Builder::address(address.as_str()).unwrap().build().unwrap();
        assert!(FileChooser::on(connect()).is_err());
        let calls = Calls::default();
        let _portal = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .name(PORTAL)
            .unwrap()
            .serve_at(PORTAL_PATH, Stand { calls: calls.clone() })
            .unwrap()
            .build()
            .unwrap();
        let chooser = FileChooser::on(connect()).unwrap();
        let open =
            |title: &str, multiple: bool, list: &[Filter]| chooser.request("OpenFile", title, options([("multiple", Variant::from(multiple))], list)).unwrap();
        let images = [Filter { name: "Images".into(), globs: vec!["*.png".into()], mimes: vec!["image/jpeg".into()] }];
        assert_eq!(open("Pick", false, &images), Some(vec!["/tmp/picked/a b.txt".to_string()]));
        assert_eq!(open("Pick", true, &[]), Some(vec!["/tmp/picked/a b.txt".to_string(), "/tmp/picked/你.png".to_string()]));
        assert_eq!(open("cancel", true, &[]), None);
        let save = options([("current_name", Variant::from("out.json"))], &[]);
        assert_eq!(chooser.request("SaveFile", "Save", save).unwrap(), Some(vec!["/tmp/picked/out.json".to_string()]));
        assert_eq!(chooser.ask_open("Pick", true, false, &images).unwrap(), Some(vec!["/tmp/picked/a b.txt".to_string(), "/tmp/picked/你.png".to_string()]));
        assert_eq!(chooser.ask_save("Save", "notes.txt", &images).unwrap(), Some("/tmp/picked/notes.txt".to_string()));
        assert_eq!(chooser.ask_save("cancel", "notes.txt", &[]).unwrap(), None);
        assert_eq!(chooser.folder("Folder").unwrap(), Some("/tmp/picked/a b.txt".to_string()));
        assert_eq!(chooser.folder("cancel").unwrap(), None);
        let calls = calls.lock().unwrap();
        let seen: Vec<(&str, &str)> = calls.iter().map(|(m, t, _)| (m.as_str(), t.as_str())).collect();
        assert_eq!(
            seen,
            [
                ("OpenFile", "Pick"),
                ("OpenFile", "Pick"),
                ("OpenFile", "cancel"),
                ("SaveFile", "Save"),
                ("OpenFile", "Pick"),
                ("SaveFile", "Save"),
                ("SaveFile", "cancel"),
                ("OpenFile", "Folder"),
                ("OpenFile", "cancel")
            ]
        );
        assert_eq!(calls[0].2["filters"].value_signature().to_string(), "a(sa(us))");
        assert_eq!(calls[5].2["filters"].value_signature().to_string(), "a(sa(us))");
        assert!(!calls[6].2.contains_key("filters"));
        assert_eq!(bool::try_from(calls[7].2["directory"].clone()).ok(), Some(true));
        _ = daemon.kill();
        _ = daemon.wait();
        _ = std::fs::remove_dir_all(dir);
    }

    struct Settings {
        scheme: Arc<std::sync::atomic::AtomicU32>,
        read_one: Arc<AtomicBool>,
    }

    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl Settings {
        async fn read_one(&self, namespace: &str, key: &str) -> zbus::fdo::Result<OwnedValue> {
            if !self.read_one.load(Ordering::SeqCst) {
                return Err(zbus::fdo::Error::UnknownMethod("ReadOne".into()));
            }
            if namespace != APPEARANCE || key != COLOR_SCHEME {
                return Err(zbus::fdo::Error::Failed("Requested setting not found".into()));
            }
            Ok(OwnedValue::from(self.scheme.load(Ordering::SeqCst)))
        }

        async fn read(&self, _namespace: &str, _key: &str) -> zbus::fdo::Result<OwnedValue> {
            Ok(Variant::Value(Box::new(Variant::U32(self.scheme.load(Ordering::SeqCst)))).try_into().unwrap())
        }
    }

    #[test]
    fn color_scheme_against_a_stand_in_portal() {
        let Some((mut daemon, address, dir)) = private_bus("portal") else {
            eprintln!("no dbus-daemon: skipped");
            return;
        };
        let connect = || zbus::blocking::connection::Builder::address(address.as_str()).unwrap().build().unwrap();
        let client = connect();
        assert!(Appearance::read(&client).is_err());
        let scheme = Arc::new(std::sync::atomic::AtomicU32::new(1));
        let read_one = Arc::new(AtomicBool::new(true));
        let portal = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .name(PORTAL)
            .unwrap()
            .serve_at(PORTAL_PATH, Settings { scheme: scheme.clone(), read_one: read_one.clone() })
            .unwrap()
            .build()
            .unwrap();
        assert!(Appearance::read(&client).unwrap());
        scheme.store(2, Ordering::SeqCst);
        assert!(!Appearance::read(&client).unwrap());
        read_one.store(false, Ordering::SeqCst);
        scheme.store(1, Ordering::SeqCst);
        assert!(Appearance::read(&client).unwrap());
        let mut changes = Appearance::changes(&client).unwrap();
        portal
            .emit_signal(None::<&str>, PORTAL_PATH, SETTINGS, "SettingChanged", &("org.gnome.desktop.interface", "gtk-theme", Variant::from("Adwaita")))
            .unwrap();
        portal.emit_signal(None::<&str>, PORTAL_PATH, SETTINGS, "SettingChanged", &(APPEARANCE, COLOR_SCHEME, Variant::from(1u32))).unwrap();
        let seen: Vec<(String, String, bool)> = changes
            .by_ref()
            .take(2)
            .map(|m| {
                let (namespace, key, value) = m.unwrap().body().deserialize::<(String, String, OwnedValue)>().unwrap();
                (namespace, key, dark_scheme(&value))
            })
            .collect();
        assert_eq!(
            seen,
            [("org.gnome.desktop.interface".to_string(), "gtk-theme".to_string(), false), (APPEARANCE.to_string(), COLOR_SCHEME.to_string(), true)]
        );
        _ = daemon.kill();
        _ = daemon.wait();
        _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn glass_settings_come_from_the_acde_namespace() {
        assert_eq!(appearance_change(ACDE_APPEARANCE, GLASS, &Variant::U32(2)), Some(serde_json::json!({ "glass": 2 })));
        assert_eq!(appearance_change(ACDE_APPEARANCE, REDUCED_TRANSPARENCY, &Variant::U32(1)), Some(serde_json::json!({ "reducedTransparency": true })));
        assert_eq!(appearance_change(APPEARANCE, GLASS, &Variant::U32(2)), None, "not in the freedesktop namespace");
        assert_eq!(appearance_change(APPEARANCE, COLOR_SCHEME, &Variant::U32(1)), Some(serde_json::json!({ "dark": true })));
        assert_eq!(appearance_change(ACDE_APPEARANCE, "other", &Variant::U32(1)), None);
        assert_eq!(appearance_change(APPEARANCE, REDUCED_MOTION, &Variant::U32(1)), Some(serde_json::json!({ "reducedMotion": true })));
    }

    #[test]
    fn color_scheme_one_is_dark() {
        assert!(dark_scheme(&Variant::U32(1)));
        assert!(!dark_scheme(&Variant::U32(2)));
        assert!(!dark_scheme(&Variant::U32(0)));
        assert!(dark_scheme(&Variant::Value(Box::new(Variant::Value(Box::new(Variant::U32(1)))))));
        assert!(!dark_scheme(&Variant::from("dark")));
    }

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(local_path("file:///home/u/a%20b.png").as_deref(), Some("/home/u/a b.png"));
        assert_eq!(local_path("file://localhost/tmp/%E4%BD%A0.txt").as_deref(), Some("/tmp/你.txt"));
        assert_eq!(local_path("file://other/tmp/x"), None);
        assert_eq!(local_path("https://example.com/x"), None);
    }

    #[test]
    fn empty_filters_are_left_out() {
        let list = filters(&[Filter { name: "Images".into(), globs: vec!["*.png".into()], mimes: vec!["image/jpeg".into()] }, Filter::default()]);
        assert_eq!(list, vec![("Images".to_string(), vec![(0, "*.png".to_string()), (1, "image/jpeg".to_string())])]);
    }
}
