use std::path::PathBuf;

use serde_json::{Map, Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Ac,
    Battery,
    LowBattery,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Ac, Group::Battery, Group::LowBattery];

    pub fn kde(self) -> &'static str {
        match self {
            Group::Ac => "AC",
            Group::Battery => "Battery",
            Group::LowBattery => "LowBattery",
        }
    }

    pub fn js(self) -> &'static str {
        match self {
            Group::Ac => "ac",
            Group::Battery => "battery",
            Group::LowBattery => "lowBattery",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Sleep,
    Hibernate,
    Shutdown,
    Prompt,
    Lock,
    ScreenOff,
}

impl Action {
    const ALL: [Action; 7] = [Action::None, Action::Sleep, Action::Hibernate, Action::Shutdown, Action::Prompt, Action::Lock, Action::ScreenOff];

    fn kde(self) -> u64 {
        match self {
            Action::None => 0,
            Action::Sleep => 1,
            Action::Hibernate => 2,
            Action::Shutdown => 8,
            Action::Prompt => 16,
            Action::Lock => 32,
            Action::ScreenOff => 64,
        }
    }

    pub fn js(self) -> &'static str {
        match self {
            Action::None => "none",
            Action::Sleep => "sleep",
            Action::Hibernate => "hibernate",
            Action::Shutdown => "shutdown",
            Action::Prompt => "prompt",
            Action::Lock => "lock",
            Action::ScreenOff => "screen-off",
        }
    }

    fn from_kde(n: u64) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.kde() == n)
    }

    fn from_js(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.js() == name)
    }

    fn idle(self) -> bool {
        matches!(self, Action::None | Action::Sleep | Action::Hibernate | Action::Shutdown)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepMode {
    Suspend,
    Hybrid,
    ThenHibernate,
}

impl SleepMode {
    const ALL: [SleepMode; 3] = [SleepMode::Suspend, SleepMode::Hybrid, SleepMode::ThenHibernate];

    fn kde(self) -> u64 {
        match self {
            SleepMode::Suspend => 1,
            SleepMode::Hybrid => 2,
            SleepMode::ThenHibernate => 3,
        }
    }

    fn js(self) -> &'static str {
        match self {
            SleepMode::Suspend => "suspend",
            SleepMode::Hybrid => "hybrid",
            SleepMode::ThenHibernate => "suspend-then-hibernate",
        }
    }

    pub fn logind(self) -> &'static str {
        match self {
            SleepMode::Suspend => "Suspend",
            SleepMode::Hybrid => "HybridSleep",
            SleepMode::ThenHibernate => "SuspendThenHibernate",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    pub dim: Option<u32>,
    pub screen_off: Option<u32>,
    pub lock_on_screen_off: bool,
    pub idle_action: Action,
    pub idle_after: u32,
    pub lid: Action,
    pub lid_external: bool,
    pub button: Action,
    pub sleep_mode: SleepMode,
    pub profile: Option<String>,
    pub brightness: Option<u32>,
}

impl Policy {
    pub fn js(&self) -> Value {
        json!({
            "dim": self.dim,
            "screenOff": self.screen_off,
            "lockOnScreenOff": self.lock_on_screen_off,
            "idleAction": self.idle_action.js(),
            "idleAfter": self.idle_after,
            "lidAction": self.lid.js(),
            "lidExternal": self.lid_external,
            "powerButton": self.button.js(),
            "sleepMode": self.sleep_mode.js(),
            "profile": self.profile,
            "brightness": self.brightness.map(|p| f64::from(p) / 100.0),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct General {
    pub low: u32,
    pub critical: u32,
    pub critical_action: Action,
}

#[derive(Clone, Copy, Default)]
pub struct Machine {
    pub can_suspend: bool,
    pub can_hibernate: bool,
    pub can_hybrid: bool,
    pub can_then_hibernate: bool,
    pub virtual_machine: bool,
}

impl Machine {
    pub fn probe(bus: Option<&zbus::blocking::Connection>) -> Self {
        let virtual_machine = std::fs::read_to_string("/proc/cpuinfo")
            .is_ok_and(|t| t.lines().any(|l| l.starts_with("flags") && l.split_whitespace().any(|f| f == "hypervisor")));
        match bus {
            Some(b) => Machine {
                can_suspend: super::bus::can(b, "CanSuspend"),
                can_hibernate: super::bus::can(b, "CanHibernate"),
                can_hybrid: super::bus::can(b, "CanHybridSleep"),
                can_then_hibernate: super::bus::can(b, "CanSuspendThenHibernate"),
                virtual_machine,
            },
            None => Machine { virtual_machine, ..Machine::default() },
        }
    }
}

pub struct Settings {
    file: Value,
    machine: Machine,
    path: Option<PathBuf>,
}

impl Settings {
    pub fn load(path: Option<PathBuf>, machine: Machine) -> Self {
        let file = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        Self { file, machine, path }
    }

    pub fn path() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(config.join("acde").join("power.json"))
    }

    fn get(&self, group: &str, section: Option<&str>, key: &str) -> Option<&Value> {
        let mut at = self.file.get(group)?;
        if let Some(section) = section {
            at = at.get(section)?;
        }
        at.get(key)
    }

    fn uint(&self, group: &str, section: Option<&str>, key: &str, default: u32) -> u32 {
        self.get(group, section, key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(default)
    }

    fn flag(&self, group: &str, section: Option<&str>, key: &str, default: bool) -> bool {
        self.get(group, section, key).and_then(Value::as_bool).unwrap_or(default)
    }

    fn set(&mut self, group: &str, section: Option<&str>, key: &str, value: Value) {
        let at = Self::child(&mut self.file, group);
        let at = match section {
            Some(section) => at.and_then(|at| Self::child(at, section)),
            None => at,
        };
        if let Some(object) = at.and_then(Value::as_object_mut) {
            object.insert(key.to_owned(), value);
        }
    }

    fn child<'a>(at: &'a mut Value, key: &str) -> Option<&'a mut Value> {
        if !at.is_object() {
            *at = Value::Object(Map::new());
        }
        let child = at.as_object_mut()?.entry(key).or_insert_with(|| Value::Object(Map::new()));
        if !child.is_object() {
            *child = Value::Object(Map::new());
        }
        Some(child)
    }

    pub fn policy(&self, group: Group) -> Policy {
        let g = group.kde();
        let m = self.machine;
        let (dim, off, auto) = match group {
            Group::Ac => (300, 600, 900),
            Group::Battery => (120, 300, 600),
            Group::LowBattery => (60, 120, 300),
        };
        let display = Some("Display");
        let suspend = Some("SuspendAndShutdown");
        let auto_default = if m.virtual_machine || !m.can_suspend { Action::None } else { Action::Sleep };
        let action = |key: &str, default: Action, idle: bool| {
            self.get(g, suspend, key).and_then(Value::as_u64).and_then(Action::from_kde).filter(|a| !idle || a.idle()).unwrap_or(default)
        };
        let profile = self
            .get(g, Some("Performance"), "PowerProfile")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| if group == Group::LowBattery { "power-saver".to_owned() } else { String::new() });
        let brightness_default = match group {
            Group::Ac => 70,
            Group::Battery => 50,
            Group::LowBattery => 30,
        };
        Policy {
            dim: self.flag(g, display, "DimDisplayWhenIdle", true).then(|| self.uint(g, display, "DimDisplayIdleTimeoutSec", dim)),
            screen_off: self.flag(g, display, "TurnOffDisplayWhenIdle", true).then(|| self.uint(g, display, "TurnOffDisplayIdleTimeoutSec", off)),
            lock_on_screen_off: self.flag(g, display, "LockBeforeTurnOffDisplay", true),
            idle_action: action("AutoSuspendAction", auto_default, true),
            idle_after: self.uint(g, suspend, "AutoSuspendIdleTimeoutSec", auto),
            lid: action("LidAction", if m.can_suspend { Action::Sleep } else { Action::ScreenOff }, false),
            lid_external: !self.flag(g, suspend, "InhibitLidActionWhenExternalMonitorPresent", true),
            button: action("PowerButtonAction", Action::Prompt, false),
            sleep_mode: self
                .get(g, suspend, "SleepMode")
                .and_then(Value::as_u64)
                .and_then(|n| SleepMode::ALL.into_iter().find(|s| s.kde() == n))
                .unwrap_or(SleepMode::Suspend),
            profile: Some(profile).filter(|p| !p.is_empty()),
            brightness: self
                .flag(g, display, "UseProfileSpecificDisplayBrightness", false)
                .then(|| self.uint(g, display, "DisplayBrightness", brightness_default).clamp(1, 100)),
        }
    }

    pub fn general(&self) -> General {
        let low = self.uint("General", None, "BatteryLowLevel", 10).min(100);
        let fallback = if self.machine.can_hibernate { Action::Hibernate } else { Action::Shutdown };
        General {
            low,
            critical: self.uint("General", None, "BatteryCriticalLevel", 5).min(low),
            critical_action: self
                .get("General", None, "BatteryCriticalAction")
                .and_then(Value::as_u64)
                .and_then(Action::from_kde)
                .filter(|a| a.idle())
                .unwrap_or(fallback),
        }
    }

    pub fn js(&self) -> Value {
        let general = self.general();
        let mut out = json!({
            "lowLevel": general.low,
            "criticalLevel": general.critical,
            "criticalAction": general.critical_action.js(),
        });
        for group in Group::ALL {
            out[group.js()] = self.policy(group).js();
        }
        out
    }

    pub fn apply(&mut self, patch: &Value) -> Result<(), String> {
        let next = self.checked(patch).map_err(crate::runtime::bad_request)?;
        self.file = next.file;
        self.save()
    }

    fn checked(&self, patch: &Value) -> Result<Settings, String> {
        let patch = patch.as_object().ok_or("acde.power.configure: the patch is an object")?;
        let mut next = Settings { file: self.file.clone(), machine: self.machine, path: None };
        for (key, value) in patch {
            match key.as_str() {
                "lowLevel" | "criticalLevel" => {
                    let n = value.as_u64().filter(|n| *n <= 100).ok_or_else(|| format!("acde.power.configure: {key} is 0..100"))?;
                    next.set("General", None, if key == "lowLevel" { "BatteryLowLevel" } else { "BatteryCriticalLevel" }, n.into());
                }
                "criticalAction" => {
                    let action = value
                        .as_str()
                        .and_then(Action::from_js)
                        .filter(|a| a.idle())
                        .ok_or("acde.power.configure: criticalAction is none, sleep, hibernate or shutdown")?;
                    next.set("General", None, "BatteryCriticalAction", action.kde().into());
                }
                name => {
                    let group = Group::ALL.into_iter().find(|g| g.js() == name).ok_or_else(|| format!("acde.power.configure: no setting {name}"))?;
                    let fields = value.as_object().ok_or_else(|| format!("acde.power.configure: {name} is an object"))?;
                    for (field, v) in fields {
                        next.apply_field(group, field, v)?;
                    }
                }
            }
        }
        let general = next.general();
        let critical = next.uint("General", None, "BatteryCriticalLevel", 5);
        if critical > general.low {
            return Err("acde.power.configure: the critical level is above the low level".into());
        }
        Ok(next)
    }

    fn apply_field(&mut self, group: Group, field: &str, v: &Value) -> Result<(), String> {
        let g = group.kde();
        let seconds =
            |v: &Value| v.as_u64().filter(|n| (1..=86_400).contains(n)).ok_or_else(|| format!("acde.power.configure: {field} is seconds (1..86400) or null"));
        let action = |v: &Value, idle: bool| {
            v.as_str().and_then(Action::from_js).filter(|a| !idle || a.idle()).ok_or_else(|| format!("acde.power.configure: {field} is not an action it takes"))
        };
        let display = Some("Display");
        let suspend = Some("SuspendAndShutdown");
        match field {
            "dim" | "screenOff" => {
                let (on, after) = if field == "dim" {
                    ("DimDisplayWhenIdle", "DimDisplayIdleTimeoutSec")
                } else {
                    ("TurnOffDisplayWhenIdle", "TurnOffDisplayIdleTimeoutSec")
                };
                if v.is_null() {
                    self.set(g, display, on, false.into());
                } else {
                    let n = seconds(v)?;
                    self.set(g, display, on, true.into());
                    self.set(g, display, after, n.into());
                }
            }
            "lockOnScreenOff" => {
                let on = v.as_bool().ok_or("acde.power.configure: lockOnScreenOff is a boolean")?;
                self.set(g, display, "LockBeforeTurnOffDisplay", on.into());
            }
            "idleAction" => self.set(g, suspend, "AutoSuspendAction", action(v, true)?.kde().into()),
            "idleAfter" => self.set(g, suspend, "AutoSuspendIdleTimeoutSec", seconds(v)?.into()),
            "lidAction" => self.set(g, suspend, "LidAction", action(v, false)?.kde().into()),
            "powerButton" => self.set(g, suspend, "PowerButtonAction", action(v, false)?.kde().into()),
            "lidExternal" => self.set(
                g,
                suspend,
                "InhibitLidActionWhenExternalMonitorPresent",
                (!v.as_bool().ok_or("acde.power.configure: lidExternal is a boolean")?).into(),
            ),
            "sleepMode" => {
                let mode = v
                    .as_str()
                    .and_then(|s| SleepMode::ALL.into_iter().find(|m| m.js() == s))
                    .ok_or("acde.power.configure: sleepMode is suspend, hybrid or suspend-then-hibernate")?;
                self.set(g, suspend, "SleepMode", mode.kde().into());
            }
            "profile" => {
                let profile = match v {
                    Value::Null => "",
                    Value::String(p) if matches!(p.as_str(), "power-saver" | "balanced" | "performance") => p,
                    _ => return Err("acde.power.configure: profile is power-saver, balanced, performance or null".into()),
                };
                self.set(g, Some("Performance"), "PowerProfile", profile.into());
            }
            "brightness" => {
                if v.is_null() {
                    self.set(g, display, "UseProfileSpecificDisplayBrightness", false.into());
                } else {
                    let f = v.as_f64().filter(|f| (0.0..=1.0).contains(f)).ok_or("acde.power.configure: brightness is 0..1 or null")?;
                    self.set(g, display, "UseProfileSpecificDisplayBrightness", true.into());
                    self.set(g, display, "DisplayBrightness", ((f * 100.0).round() as u64).max(1).into());
                }
            }
            other => return Err(format!("acde.power.configure: no setting {}.{other}", group.js())),
        }
        Ok(())
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(&self.file).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Settings {
        Settings { file: json!({}), machine: Machine { can_suspend: true, ..Machine::default() }, path: None }
    }

    #[test]
    fn defaults_are_plasmas() {
        let s = fresh();
        let ac = s.policy(Group::Ac);
        assert_eq!((ac.dim, ac.screen_off, ac.idle_action, ac.idle_after), (Some(300), Some(600), Action::Sleep, 900));
        assert_eq!((ac.lid, ac.button, ac.profile.as_deref()), (Action::Sleep, Action::Prompt, None));
        let low = s.policy(Group::LowBattery);
        assert_eq!((low.dim, low.screen_off, low.idle_after, low.profile.as_deref()), (Some(60), Some(120), 300, Some("power-saver")));
        let g = s.general();
        assert_eq!((g.low, g.critical, g.critical_action), (10, 5, Action::Shutdown));
    }

    #[test]
    fn patches_write_kde_keys() {
        let mut s = fresh();
        s.apply(&json!({ "battery": { "screenOff": null, "idleAction": "hibernate", "brightness": 0.4 }, "lowLevel": 20 })).unwrap();
        assert_eq!(s.file["Battery"]["Display"]["TurnOffDisplayWhenIdle"], json!(false));
        assert_eq!(s.file["Battery"]["SuspendAndShutdown"]["AutoSuspendAction"], json!(2));
        assert_eq!(s.file["Battery"]["Display"]["DisplayBrightness"], json!(40));
        assert_eq!(s.policy(Group::Battery).screen_off, None);
        assert_eq!(s.general().low, 20);
        assert!(s.apply(&json!({ "ac": { "idleAction": "prompt" } })).is_err());
        assert!(s.apply(&json!({ "criticalLevel": 50 })).is_err());
        assert_eq!(s.general().critical, 5);
    }

    #[test]
    fn locks_on_screen_off_unless_told_not_to() {
        let mut s = fresh();
        assert!(Group::ALL.into_iter().all(|g| s.policy(g).lock_on_screen_off), "an old power.json without the key locks");
        s.apply(&json!({ "battery": { "lockOnScreenOff": false } })).unwrap();
        assert_eq!(s.file["Battery"]["Display"]["LockBeforeTurnOffDisplay"], json!(false));
        assert!(!s.policy(Group::Battery).lock_on_screen_off);
        assert!(s.policy(Group::Ac).lock_on_screen_off);
        assert_eq!(s.policy(Group::Battery).js()["lockOnScreenOff"], json!(false));
        assert!(s.apply(&json!({ "ac": { "lockOnScreenOff": "yes" } })).is_err());
        assert!(s.policy(Group::Ac).lock_on_screen_off);
    }
}
