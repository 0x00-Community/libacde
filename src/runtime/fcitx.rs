use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};
use zbus::blocking::{Connection, MessageIterator, Proxy};

type Result<T> = std::result::Result<T, String>;

const SERVICE: &str = "org.fcitx.Fcitx5";
const PATH: &str = "/controller";
const INTERFACE: &str = "org.fcitx.Fcitx.Controller1";
const GROUPS_CHANGED: &str = "InputMethodGroupsChanged";
const POLL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeEntry {
    pub id: String,
    pub name: String,
    pub short: String,
    pub languages: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeGroup {
    pub name: String,
    pub layout: String,
    pub items: Vec<(String, String)>,
}

pub trait ImeController: Send + Sync {
    fn current(&self) -> Result<String>;
    fn set_current(&self, id: &str) -> Result<()>;
    fn group(&self) -> Result<ImeGroup>;
    fn set_group(&self, group: &ImeGroup) -> Result<()>;
    fn available(&self) -> Result<Vec<ImeEntry>>;
}

pub fn ime_state(controller: &dyn ImeController) -> Value {
    let read = || -> Result<Value> {
        let available = controller.available()?;
        let group = controller.group()?;
        let current = controller.current()?;
        let listed: Vec<Value> = group
            .items
            .iter()
            .map(|(id, _)| {
                let entry = available.iter().find(|entry| &entry.id == id);
                json!({ "id": id, "name": entry.map_or(id.as_str(), |e| e.name.as_str()), "short": entry.map_or("", |e| e.short.as_str()) })
            })
            .collect();
        Ok(json!({ "running": true, "current": (!current.is_empty()).then_some(current), "group": listed }))
    };
    read().unwrap_or_else(|_| json!({ "running": false, "current": null, "group": [] }))
}

pub fn ime_catalog(controller: &dyn ImeController) -> Value {
    let entries = controller.available().unwrap_or_default();
    Value::Array(entries.iter().map(|e| json!({ "id": e.id, "name": e.name, "short": e.short, "languages": e.languages })).collect())
}

pub fn ime_apply(controller: &dyn ImeController, patch: &Value) -> Result<()> {
    let patch = patch.as_object().ok_or_else(|| crate::runtime::bad_request("acde.input.set: ime is an object"))?;
    if let Some(group) = patch.get("group") {
        let ids: Vec<String> = group
            .as_array()
            .and_then(|list| list.iter().map(|id| id.as_str().map(str::to_owned)).collect())
            .ok_or_else(|| crate::runtime::bad_request("acde.input.set: ime.group lists input method ids"))?;
        let known = controller.available()?;
        if let Some(unknown) = ids.iter().find(|id| !known.iter().any(|entry| &entry.id == *id)) {
            return Err(crate::runtime::bad_request(format!("acde.input.set: no input method {unknown}")));
        }
        let mut next = controller.group()?;
        let layout = |id: &String| next.items.iter().find(|(had, _)| had == id).map(|(_, layout)| layout.clone()).unwrap_or_default();
        let items = ids.iter().map(|id| (id.clone(), layout(id))).collect();
        next.items = items;
        controller.set_group(&next)?;
    }
    if let Some(current) = patch.get("current") {
        let id = current.as_str().ok_or_else(|| crate::runtime::bad_request("acde.input.set: ime.current is an input method id"))?;
        controller.set_current(id)?;
    }
    Ok(())
}

pub struct Fcitx;

impl Fcitx {
    fn connection() -> Result<Connection> {
        static SESSION: OnceLock<std::result::Result<Connection, String>> = OnceLock::new();
        SESSION.get_or_init(|| Connection::session().map_err(|e| format!("the session bus: {e}"))).clone()
    }

    fn proxy() -> Result<Proxy<'static>> {
        Proxy::new(&Self::connection()?, SERVICE, PATH, INTERFACE).map_err(|e| format!("{SERVICE}: {e}"))
    }

    fn call<B, R>(method: &str, body: &B) -> Result<R>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
    {
        Self::proxy()?.call(method, body).map_err(|e| format!("{SERVICE} {method}: {e}"))
    }

    pub fn watch(changed: impl Fn() -> bool + Send + Sync + 'static) {
        let changed = std::sync::Arc::new(changed);
        let signals = Self::connection().ok().and_then(|connection| {
            let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(INTERFACE).ok()?.member(GROUPS_CHANGED).ok()?.build();
            MessageIterator::for_match_rule(rule, &connection, None).ok()
        });
        if let Some(signals) = signals {
            let changed = changed.clone();
            _ = std::thread::Builder::new().name("acde-fcitx-groups".into()).spawn(move || {
                for message in signals {
                    if message.is_err() || !changed() {
                        break;
                    }
                }
            });
        }
        _ = std::thread::Builder::new().name("acde-fcitx-poll".into()).spawn(move || {
            let mut last = Fcitx.current().ok();
            loop {
                std::thread::sleep(POLL);
                let now = Fcitx.current().ok();
                if now != last {
                    last = now;
                    if !changed() {
                        break;
                    }
                }
            }
        });
    }
}

impl ImeController for Fcitx {
    fn current(&self) -> Result<String> {
        Self::call("CurrentInputMethod", &())
    }

    fn set_current(&self, id: &str) -> Result<()> {
        Self::call("SetCurrentIM", &(id,))
    }

    fn group(&self) -> Result<ImeGroup> {
        let name: String = Self::call("CurrentInputMethodGroup", &())?;
        let (layout, items): (String, Vec<(String, String)>) = Self::call("InputMethodGroupInfo", &(name.as_str(),))?;
        Ok(ImeGroup { name, layout, items })
    }

    fn set_group(&self, group: &ImeGroup) -> Result<()> {
        Self::call("SetInputMethodGroupInfo", &(group.name.as_str(), group.layout.as_str(), &group.items))
    }

    fn available(&self) -> Result<Vec<ImeEntry>> {
        let listed: Vec<(String, String, String, String, String, String, bool)> = Self::call("AvailableInputMethods", &())?;
        Ok(listed
            .into_iter()
            .map(|(id, name, native, _icon, label, language, _configurable)| ImeEntry {
                id,
                name: if native.is_empty() { name } else { native },
                short: label,
                languages: (!language.is_empty()).then_some(language).into_iter().collect(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Stand {
        running: bool,
        current: Mutex<String>,
        group: Mutex<ImeGroup>,
    }

    impl Stand {
        fn new(running: bool) -> Self {
            let group =
                ImeGroup { name: "Default".into(), layout: "us".into(), items: vec![("keyboard-us".into(), String::new()), ("pinyin".into(), "us".into())] };
            Self { running, current: Mutex::new("pinyin".into()), group: Mutex::new(group) }
        }

        fn up(&self) -> Result<()> {
            if self.running { Ok(()) } else { Err("org.freedesktop.DBus.Error.ServiceUnknown".into()) }
        }
    }

    impl ImeController for Stand {
        fn current(&self) -> Result<String> {
            self.up()?;
            Ok(self.current.lock().unwrap().clone())
        }

        fn set_current(&self, id: &str) -> Result<()> {
            self.up()?;
            *self.current.lock().unwrap() = id.into();
            Ok(())
        }

        fn group(&self) -> Result<ImeGroup> {
            self.up()?;
            Ok(self.group.lock().unwrap().clone())
        }

        fn set_group(&self, group: &ImeGroup) -> Result<()> {
            self.up()?;
            *self.group.lock().unwrap() = group.clone();
            Ok(())
        }

        fn available(&self) -> Result<Vec<ImeEntry>> {
            self.up()?;
            let entry = |id: &str, name: &str, short: &str, language: &str| ImeEntry {
                id: id.into(),
                name: name.into(),
                short: short.into(),
                languages: vec![language.into()],
            };
            Ok(vec![entry("keyboard-us", "English (US)", "en", "en"), entry("pinyin", "拼音", "拼", "zh_CN"), entry("rime", "中州韻", "ㄓ", "zh")])
        }
    }

    #[test]
    fn the_state_names_the_group_and_the_current_input_method() {
        let state = ime_state(&Stand::new(true));
        assert_eq!(state["running"], json!(true));
        assert_eq!(state["current"], json!("pinyin"));
        assert_eq!(state["group"], json!([{ "id": "keyboard-us", "name": "English (US)", "short": "en" }, { "id": "pinyin", "name": "拼音", "short": "拼" }]));
        assert_eq!(ime_catalog(&Stand::new(true))[2], json!({ "id": "rime", "name": "中州韻", "short": "ㄓ", "languages": ["zh"] }));
    }

    #[test]
    fn without_fcitx5_the_state_is_empty_and_writes_fail() {
        let stand = Stand::new(false);
        assert_eq!(ime_state(&stand), json!({ "running": false, "current": null, "group": [] }));
        assert_eq!(ime_catalog(&stand), json!([]));
        assert!(ime_apply(&stand, &json!({ "current": "pinyin" })).is_err());
    }

    #[test]
    fn a_patch_sets_the_group_keeping_layouts_then_the_current_one() {
        let stand = Stand::new(true);
        ime_apply(&stand, &json!({ "group": ["pinyin", "rime"], "current": "rime" })).unwrap();
        let group = stand.group.lock().unwrap().clone();
        assert_eq!(group.items, vec![("pinyin".into(), "us".into()), ("rime".into(), String::new())]);
        assert_eq!((group.name.as_str(), group.layout.as_str()), ("Default", "us"));
        assert_eq!(*stand.current.lock().unwrap(), "rime");
        assert!(ime_apply(&stand, &json!({ "group": ["nope"] })).unwrap_err().contains("no input method nope"));
        assert!(ime_apply(&stand, &json!({ "current": 3 })).is_err());
    }
}
