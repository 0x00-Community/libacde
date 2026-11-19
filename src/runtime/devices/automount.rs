use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Known {
    pub name: String,
    pub ever_mounted: bool,
    pub login: bool,
    pub attach: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub on_login: bool,
    pub on_attach: bool,
    pub unknown: bool,
    pub devices: BTreeMap<String, Known>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { on_login: false, on_attach: true, unknown: true, devices: BTreeMap::new() }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    Login,
    Attach,
}

impl Settings {
    fn file() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").filter(|h| !h.is_empty()).map(|home| PathBuf::from(home).join(".config")))?;
        Some(config.join("acde/automount.json"))
    }

    pub fn load() -> Self {
        Self::file().and_then(|f| std::fs::read(f).ok()).and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let file = Self::file().ok_or("no config directory (HOME is not set)")?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let text = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &file).map_err(|e| format!("{}: {e}", file.display()))
    }

    pub fn should(&self, key: &str, when: When) -> bool {
        let known = self.devices.get(key);
        let (forced, global) = match when {
            When::Login => (known.is_some_and(|k| k.login), self.on_login),
            When::Attach => (known.is_some_and(|k| k.attach), self.on_attach),
        };
        forced || (global && (known.is_some_and(|k| k.ever_mounted) || self.unknown))
    }

    pub fn seen(&mut self, key: &str, name: &str, mounted: bool) -> bool {
        let entry = self.devices.entry(key.to_owned()).or_default();
        let before = entry.clone();
        if !name.is_empty() {
            entry.name = name.to_owned();
        }
        entry.ever_mounted |= mounted;
        *entry != before
    }

    pub fn view(&self, attached: &[String]) -> Value {
        let devices: Vec<Value> = self
            .devices
            .iter()
            .map(|(id, k)| json!({ "id": id, "name": k.name, "attached": attached.contains(id), "login": k.login, "attach": k.attach, "everMounted": k.ever_mounted }))
            .collect();
        json!({ "onLogin": self.on_login, "onAttach": self.on_attach, "unknown": self.unknown, "devices": devices })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugged_drive_mounts_unless_turned_off() {
        let s = Settings::default();
        assert!(s.should("new", When::Attach), "on attach, unknown drives too, by default");
        assert!(!s.should("new", When::Login));
        let read: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(read, s, "a file without the keys reads as the defaults");
        let off: Settings = serde_json::from_str(r#"{"onAttach": false}"#).unwrap();
        assert!(!off.should("new", When::Attach), "turned off in the settings");
    }

    #[test]
    fn decides_like_kde() {
        let mut s = Settings { on_attach: false, unknown: false, ..Settings::default() };
        assert!(!s.should("a", When::Attach));
        s.on_attach = true;
        assert!(!s.should("a", When::Attach), "never mounted, unknown devices off");
        s.seen("a", "Stick", false);
        assert!(!s.should("a", When::Attach));
        assert!(s.seen("a", "Stick", true));
        assert!(!s.seen("a", "Stick", false), "ever mounted stays");
        assert!(s.should("a", When::Attach));
        assert!(!s.should("a", When::Login));
        s.unknown = true;
        assert!(s.should("b", When::Attach));
        s.on_attach = false;
        assert!(!s.should("b", When::Attach));
        s.devices.get_mut("a").unwrap().login = true;
        assert!(s.should("a", When::Login), "forced per device");
        let view = s.view(&["a".into()]);
        assert_eq!(view["devices"][0]["attached"], true);
        assert_eq!(view["devices"][0]["name"], "Stick");
    }
}
