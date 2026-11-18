pub mod backlight;
pub mod bus;
pub mod settings;
pub mod shared;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zbus::blocking::MessageIterator;

use backlight::Backlight;
pub use backlight::brightness as backlight_brightness;
use settings::{Action, Group, Machine, Settings};
use shared::PolicyMark;

use crate::runtime::sync::Locked;

pub enum PowerCall {
    State,
    SetProfile { profile: String },
    Configure { patch: Value },
    SetScreenOff { seconds: Option<u32> },
    SetSleep { seconds: Option<u32> },
    Suspend,
    Hibernate,
    SetBrightness { value: f64 },
    SetChargeLimit { end: Option<u32> },
}

pub trait PowerHost: Send + 'static {
    fn set_idle(&self, timeouts: &[u32]) -> Result<(), String>;
    fn set_screens(&self, on: bool);
}

type Sink = Box<dyn Fn(Value) -> bool + Send>;

enum Event {
    Bus,
    Idle(u32, bool),
    Button,
    Configured,
    Start(Box<dyn PowerHost>),
    Critical(u64),
}

#[derive(Default)]
struct Inner {
    settings: Mutex<Option<Settings>>,
    machine: OnceLock<Machine>,
    sinks: Mutex<Vec<Sink>>,
    pump: Mutex<Option<Sender<Event>>>,
    policy: AtomicBool,
    system: Mutex<Option<zbus::blocking::Connection>>,
    path: OnceLock<Option<PathBuf>>,
    watching: AtomicBool,
}

#[derive(Clone, Default)]
pub struct Power {
    inner: Arc<Inner>,
}

#[derive(Clone, Copy)]
enum Kind {
    Dim,
    ScreenOff,
    Auto(Action),
}

#[derive(Default)]
struct Policy {
    host: Option<Box<dyn PowerHost>>,
    group: Option<Group>,
    current: Option<settings::Policy>,
    kinds: Vec<Kind>,
    dimmed: Option<f64>,
    lid_closed: Option<bool>,
    critical: u64,
    critical_armed: bool,
    _inhibit: Option<zbus::zvariant::OwnedFd>,
    _mark: Option<PolicyMark>,
}

struct Facts {
    on_battery: bool,
    percent: Option<f64>,
    lid_present: bool,
    lid_closed: bool,
}

impl Power {
    const SETTLE: Duration = Duration::from_millis(250);
    const CRITICAL_WAIT: Duration = Duration::from_secs(60);
    const DIM: f64 = 0.3;

    fn machine(&self) -> Machine {
        *self.inner.machine.get_or_init(|| Machine::probe(self.system().ok().as_ref()))
    }

    fn settings<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> R {
        let machine = self.machine();
        let path = self.settings_path();
        let mut slot = self.inner.settings.locked();
        f(slot.get_or_insert_with(|| Settings::load(path, machine)))
    }

    fn settings_path(&self) -> Option<PathBuf> {
        self.inner.path.get_or_init(Settings::path).clone()
    }

    fn reload(&self) {
        let fresh = Settings::load(self.settings_path(), self.machine());
        *self.inner.settings.locked() = Some(fresh);
    }

    fn watch_settings(&self) {
        let Some(path) = self.settings_path() else { return };
        if self.inner.watching.swap(true, Ordering::Relaxed) {
            return;
        }
        let inner = Arc::downgrade(&self.inner);
        let changed = move || {
            let Some(inner) = inner.upgrade() else { return false };
            let power = Power { inner };
            power.reload();
            power.poke(Event::Configured);
            true
        };
        if let Err(e) = shared::watch(path, changed) {
            self.inner.watching.store(false, Ordering::Relaxed);
            log::warn!(target: "crate::runtime::power", "电源设置文件的变化看不到, 别的进程改的设置要重启才生效: {e}");
        }
    }

    fn policy_elsewhere() -> bool {
        PolicyMark::path().is_some_and(|path| PolicyMark::held(&path))
    }

    fn group(on_battery: bool, percent: Option<f64>, low: u32) -> Group {
        match (on_battery, percent) {
            (false, _) => Group::Ac,
            (true, Some(p)) if p <= f64::from(low) => Group::LowBattery,
            (true, _) => Group::Battery,
        }
    }

    fn snapshot(&self) -> Result<(Value, Option<Facts>), String> {
        let bus = self.system()?;
        let facts = bus::facts(&bus).map_err(|e| log::debug!(target: "crate::runtime::power", "{e}")).ok();
        let profiles = bus::profiles(&bus);
        let m = self.machine();
        let (on_battery, percent) = facts.as_ref().map_or((false, None), |f| (f.on_battery, f.percent));
        let (settings, policy, group) = self.settings(|s| {
            let group = Self::group(on_battery, percent, s.general().low);
            (s.js(), s.policy(group), group)
        });
        let state = json!({
            "onBattery": on_battery,
            "battery": facts.as_ref().map_or(Value::Null, |f| f.battery.clone()),
            "batteries": facts.as_ref().map_or(Value::Array(vec![]), |f| Value::from(f.batteries.clone())),
            "lid": facts.as_ref().is_some_and(|f| f.lid_present),
            "profile": profiles.as_ref().map(|p| p.active.clone()),
            "profiles": profiles.as_ref().map_or(vec![], |p| p.list.clone()),
            "degraded": profiles.as_ref().and_then(|p| p.degraded.clone()),
            "screenOffSeconds": policy.screen_off,
            "sleepSeconds": (policy.idle_action != Action::None).then_some(policy.idle_after),
            "current": group.js(),
            "settings": settings,
            "can": { "suspend": m.can_suspend, "hibernate": m.can_hibernate, "hybrid": m.can_hybrid, "suspendThenHibernate": m.can_then_hibernate },
            "brightness": backlight::brightness(),
            "inhibitors": bus::inhibitors(&bus),
            "policy": self.running() || Self::policy_elsewhere(),
        });
        let facts = facts.map(|f| Facts { on_battery: f.on_battery, percent: f.percent, lid_present: f.lid_present, lid_closed: f.lid_closed });
        Ok((state, facts))
    }

    pub fn call(&self, call: PowerCall) -> Result<Value, String> {
        let done = match call {
            PowerCall::State => return self.snapshot().map(|(state, _)| state),
            PowerCall::SetProfile { profile } => {
                let bus = self.system()?;
                let known = bus::profiles(&bus).ok_or("power-profiles-daemon is not running")?;
                if !known.list.contains(&profile) {
                    return Err(format!("acde.power.setProfile: no profile {profile} (there are {})", known.list.join(", ")));
                }
                bus::set_profile(&bus, &profile)
            }
            PowerCall::Configure { patch } => self.configure(&patch),
            PowerCall::SetScreenOff { seconds } => {
                let group = self.current_group();
                self.configure(&json!({ group.js(): { "screenOff": seconds } }))
            }
            PowerCall::SetSleep { seconds } => {
                let group = self.current_group();
                let patch = match seconds {
                    None => json!({ "idleAction": "none" }),
                    Some(s) => {
                        let action = self.settings(|st| st.policy(group).idle_action);
                        let action = if action == Action::None { Action::Sleep } else { action };
                        json!({ "idleAction": action.js(), "idleAfter": s })
                    }
                };
                self.configure(&json!({ group.js(): patch }))
            }
            PowerCall::Suspend => {
                let group = self.current_group();
                let mode = self.settings(|s| s.policy(group).sleep_mode);
                bus::logind(&self.system()?, mode.logind())
            }
            PowerCall::Hibernate => bus::logind(&self.system()?, "Hibernate"),
            PowerCall::SetBrightness { value } => backlight::set_brightness(&self.system()?, value),
            PowerCall::SetChargeLimit { end } => self.set_charge_limit(end),
        };
        done.map(|()| {
            self.poke(Event::Bus);
            Value::Null
        })
    }

    fn configure(&self, patch: &Value) -> Result<(), String> {
        self.settings(|s| s.apply(patch))?;
        self.poke(Event::Configured);
        Ok(())
    }

    fn current_group(&self) -> Group {
        let facts = self.system().ok().and_then(|b| bus::facts(&b).ok());
        let (on_battery, percent) = facts.map_or((false, None), |f| (f.on_battery, f.percent));
        self.settings(|s| Self::group(on_battery, percent, s.general().low))
    }

    fn set_charge_limit(&self, end: Option<u32>) -> Result<(), String> {
        if end.is_some_and(|e| !(50..=100).contains(&e)) {
            return Err("acde.power.setChargeLimit: the limit is 50..100 % or null".into());
        }
        let bus = self.system()?;
        let facts = bus::facts(&bus)?;
        let battery = facts.batteries.iter().find(|b| !b["chargeLimit"].is_null()).ok_or("this battery's charge cannot be limited")?;
        let native = battery["id"].as_str().unwrap_or_default().to_owned();
        let limit = &battery["chargeLimit"];
        if limit["writable"].as_bool() == Some(true) {
            let dir = std::path::Path::new("/sys/class/power_supply").join(&native);
            let target = end.unwrap_or(100);
            if let Some(start) = limit["start"].as_u64().and_then(|s| u32::try_from(s).ok())
                && start >= target
            {
                std::fs::write(dir.join("charge_control_start_threshold"), target.saturating_sub(5).to_string()).map_err(|e| format!("{native}: {e}"))?;
            }
            return std::fs::write(dir.join("charge_control_end_threshold"), target.to_string()).map_err(|e| format!("{native}: {e}"));
        }
        if limit["upower"].as_bool() == Some(true) {
            return bus::enable_charge_threshold(&bus, &native, end.is_some());
        }
        Err(format!("{native}: only root may change the charge limit (charge_control_end_threshold is not writable)"))
    }

    pub fn listen(&self, sink: impl Fn(Value) -> bool + Send + 'static) -> Result<(), String> {
        self.inner.sinks.locked().push(Box::new(sink));
        self.pump()?;
        Ok(())
    }

    pub fn start_policy(&self, host: impl PowerHost) -> Result<(), String> {
        self.pump()?.send(Event::Start(Box::new(host))).map_err(|e| e.to_string())
    }

    pub fn running(&self) -> bool {
        self.inner.policy.load(Ordering::Relaxed)
    }

    pub fn idle(&self, index: u32, idle: bool) {
        self.poke(Event::Idle(index, idle));
    }

    pub fn power_button(&self) {
        self.poke(Event::Button);
    }

    fn poke(&self, event: Event) {
        if let Some(tx) = self.inner.pump.locked().as_ref() {
            _ = tx.send(event);
        }
    }

    fn emit(&self, event: &Value) {
        self.inner.sinks.locked().retain(|sink| sink(event.clone()));
    }

    fn system(&self) -> Result<zbus::blocking::Connection, String> {
        let mut slot = self.inner.system.locked();
        if let Some(connection) = slot.as_ref() {
            return Ok(connection.clone());
        }
        let connection = zbus::blocking::Connection::system().map_err(|e| format!("system bus: {e}"))?;
        *slot = Some(connection.clone());
        Ok(connection)
    }

    fn pump(&self) -> Result<Sender<Event>, String> {
        let mut slot = self.inner.pump.locked();
        if let Some(tx) = slot.as_ref() {
            return Ok(tx.clone());
        }
        let (tx, rx) = mpsc::channel();
        let power = self.clone();
        let back = tx.clone();
        std::thread::Builder::new().name("acde-power".into()).spawn(move || power.run(&rx, &back)).map_err(|e| e.to_string())?;
        self.watch_bus(tx.clone());
        self.watch_settings();
        *slot = Some(tx.clone());
        Ok(tx)
    }

    fn watch_bus(&self, tx: Sender<Event>) {
        let rules = [bus::UPOWER_PATH, "/net/hadess"]
            .map(|path| zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).path_namespace(path).map(|b| b.build()));
        let Ok(connection) = self.system() else { return };
        for rule in rules.into_iter().flatten() {
            let Ok(signals) = MessageIterator::for_match_rule(rule, &connection, None) else { continue };
            let tx = tx.clone();
            _ = std::thread::Builder::new().name("acde-power-bus".into()).spawn(move || {
                for message in signals {
                    if message.is_err() || tx.send(Event::Bus).is_err() {
                        break;
                    }
                }
            });
        }
    }

    fn run(&self, rx: &Receiver<Event>, tx: &Sender<Event>) {
        let mut policy = Policy::default();
        let mut due = Some(Instant::now());
        loop {
            let event = match due {
                Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match event {
                Ok(Event::Bus | Event::Configured) => {
                    due.get_or_insert_with(|| Instant::now() + Self::SETTLE);
                }
                Ok(Event::Start(host)) => {
                    self.start(&mut policy, host);
                    due = Some(Instant::now());
                }
                Ok(Event::Idle(index, idle)) => self.on_idle(&mut policy, index, idle),
                Ok(Event::Button) => {
                    if let Some(action) = policy.current.as_ref().map(|p| p.button) {
                        log::info!(target: "crate::runtime::power", "电源键: {}", action.js());
                        self.perform(&policy, action);
                    }
                }
                Ok(Event::Critical(serial)) => self.on_critical(&policy, serial),
                Err(RecvTimeoutError::Timeout) => {
                    due = None;
                    self.refresh(&mut policy, tx);
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn start(&self, policy: &mut Policy, host: Box<dyn PowerHost>) {
        policy.host = Some(host);
        policy.group = None;
        policy.current = None;
        self.inner.policy.store(true, Ordering::Relaxed);
        policy._mark = PolicyMark::path().and_then(|path| PolicyMark::claim(path).map_err(|e| log::warn!(target: "crate::runtime::power", "{e}")).ok());
        match self
            .system()
            .and_then(|b| bus::inhibit(&b, "handle-power-key:handle-lid-switch", "acde handles the power button and the lid (Power and battery settings)"))
        {
            Ok(fd) => policy._inhibit = Some(fd),
            Err(e) => log::warn!(target: "crate::runtime::power", "logind 的电源键 / 合盖 inhibitor 拿不到, 由 logind 处理: {e}"),
        }
        log::info!(target: "crate::runtime::power", "电源策略开始 ({:?})", self.settings_path());
    }

    fn refresh(&self, policy: &mut Policy, tx: &Sender<Event>) {
        let (state, facts) = match self.snapshot() {
            Ok(s) => s,
            Err(e) => return log::warn!(target: "crate::runtime::power", "{e}"),
        };
        self.emit(&json!({ "type": "changed", "state": state }));
        if policy.host.is_some() {
            self.apply(policy, facts.as_ref(), tx);
        }
    }

    fn apply(&self, p: &mut Policy, facts: Option<&Facts>, tx: &Sender<Event>) {
        let (on_battery, percent) = facts.map_or((false, None), |f| (f.on_battery, f.percent));
        let general = self.settings(|s| s.general());
        let group = Self::group(on_battery, percent, general.low);
        let next = self.settings(|s| s.policy(group));
        let fresh = p.group != Some(group);
        let was = p.current.clone();
        if fresh {
            log::info!(target: "crate::runtime::power", "电源方案: {}", group.kde());
        }
        if (fresh || was.as_ref().is_none_or(|w| w.profile != next.profile))
            && let Some(profile) = &next.profile
            && let Err(e) = self.system().and_then(|b| bus::set_profile(&b, profile))
        {
            log::warn!(target: "crate::runtime::power", "电源模式 {profile}: {e}");
        }
        if (fresh || was.as_ref().is_none_or(|w| w.brightness != next.brightness))
            && let Some(percent) = next.brightness
            && let Some(light) = Backlight::find()
            && let Err(e) = self.system().and_then(|bus| light.set(&bus, f64::from(percent) / 100.0))
        {
            log::warn!(target: "crate::runtime::power", "亮度: {e}");
        }
        if fresh || was.as_ref() != Some(&next) {
            self.arm(p, &next);
        }
        p.group = Some(group);
        p.current = Some(next.clone());
        if let Some(f) = facts.filter(|f| f.lid_present) {
            let before = p.lid_closed.replace(f.lid_closed);
            if f.lid_closed && before == Some(false) {
                let docked = self.system().is_ok_and(|b| bus::docked(&b));
                if docked && !next.lid_external {
                    log::info!(target: "crate::runtime::power", "合上了盖子, 接着外接显示器: 不动");
                } else {
                    log::info!(target: "crate::runtime::power", "合上了盖子: {}", next.lid.js());
                    self.perform(p, next.lid);
                }
            }
        }
        let critical = on_battery && percent.is_some_and(|x| x <= f64::from(general.critical));
        if critical != p.critical_armed {
            p.critical_armed = critical;
            p.critical += 1;
            if critical {
                let serial = p.critical;
                log::warn!(target: "crate::runtime::power", "电量 {:.0}% 到了严重低电量, {} 秒后{}", percent.unwrap_or(0.0), Self::CRITICAL_WAIT.as_secs(), general.critical_action.js());
                self.emit(&json!({ "type": "critical", "action": general.critical_action.js(), "seconds": Self::CRITICAL_WAIT.as_secs() }));
                let tx = tx.clone();
                _ = std::thread::Builder::new().name("acde-power-critical".into()).spawn(move || {
                    std::thread::sleep(Self::CRITICAL_WAIT);
                    _ = tx.send(Event::Critical(serial));
                });
            }
        }
    }

    fn arm(&self, p: &mut Policy, policy: &settings::Policy) {
        let mut kinds = Vec::new();
        let mut timeouts = Vec::new();
        let mut push = |kind: Kind, seconds: u32| {
            kinds.push(kind);
            timeouts.push(seconds.saturating_mul(1000));
        };
        if let Some(s) = policy.dim {
            push(Kind::Dim, s);
        }
        if let Some(s) = policy.screen_off {
            push(Kind::ScreenOff, s);
        }
        if policy.idle_action != Action::None {
            push(Kind::Auto(policy.idle_action), policy.idle_after);
        }
        self.undim(p);
        p.kinds = kinds;
        if let Some(host) = &p.host
            && let Err(e) = host.set_idle(&timeouts)
        {
            log::warn!(target: "crate::runtime::power", "空闲计时: {e}");
        }
    }

    fn undim(&self, p: &mut Policy) {
        if let Some(value) = p.dimmed.take()
            && let Some(light) = Backlight::find()
            && let Err(e) = self.system().and_then(|bus| light.set(&bus, value))
        {
            log::warn!(target: "crate::runtime::power", "亮度恢复: {e}");
        }
    }

    fn on_idle(&self, p: &mut Policy, index: u32, idle: bool) {
        let Some(&kind) = p.kinds.get(index as usize) else { return };
        if !idle {
            return self.undim(p);
        }
        if self.system().is_ok_and(|b| bus::blocked(&b, "idle")) {
            return log::info!(target: "crate::runtime::power", "空闲了, 但有程序挡着 (logind idle inhibitor)");
        }
        match kind {
            Kind::Dim => {
                if p.dimmed.is_none()
                    && let Some(light) = Backlight::find()
                    && let Some(now) = light.get()
                {
                    p.dimmed = Some(now);
                    if let Err(e) = self.system().and_then(|bus| light.set(&bus, now * Self::DIM)) {
                        log::warn!(target: "crate::runtime::power", "调暗: {e}");
                    }
                }
            }
            Kind::ScreenOff => {
                log::info!(target: "crate::runtime::power", "空闲: 关屏");
                if let Some(host) = &p.host {
                    host.set_screens(false);
                }
            }
            Kind::Auto(action) => {
                log::info!(target: "crate::runtime::power", "空闲: {}", action.js());
                self.perform(p, action);
            }
        }
    }

    fn on_critical(&self, p: &Policy, serial: u64) {
        if serial != p.critical || !p.critical_armed {
            return;
        }
        let action = self.settings(|s| s.general().critical_action);
        log::warn!(target: "crate::runtime::power", "严重低电量: {}", action.js());
        self.perform(p, action);
    }

    fn perform(&self, p: &Policy, action: Action) {
        let sleep = p.current.as_ref().map_or(settings::SleepMode::Suspend, |c| c.sleep_mode);
        let done = match action {
            Action::None => Ok(()),
            Action::Sleep => self.system().and_then(|b| bus::logind(&b, sleep.logind())),
            Action::Hibernate => self.system().and_then(|b| bus::logind(&b, "Hibernate")),
            Action::Shutdown => self.system().and_then(|b| bus::logind(&b, "PowerOff")),
            Action::Lock => self.system().and_then(|b| bus::lock(&b)),
            Action::ScreenOff => {
                if let Some(host) = &p.host {
                    host.set_screens(false);
                }
                Ok(())
            }
            Action::Prompt => {
                self.emit(&json!({ "type": "prompt" }));
                Ok(())
            }
        };
        if let Err(e) = done {
            log::warn!(target: "crate::runtime::power", "{}: {e}", action.js());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(file: &std::path::Path) -> Power {
        let power = Power::default();
        _ = power.inner.path.set(Some(file.to_path_buf()));
        _ = power.inner.machine.set(Machine { can_suspend: true, ..Machine::default() });
        power
    }

    #[test]
    fn rereads_settings_another_process_saved() {
        let dir = std::env::temp_dir().join(format!("acde-power-watch-{}", std::process::id()));
        let file = dir.join("power.json");
        let watcher = at(&file);
        let writer = at(&file);
        assert_eq!(watcher.settings(|s| s.general().low), 10);
        watcher.watch_settings();
        writer.configure(&json!({ "lowLevel": 20 })).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while watcher.settings(|s| s.general().low) != 20 {
            assert!(Instant::now() < deadline, "the watching Power never reread {}", file.display());
            std::thread::sleep(Duration::from_millis(20));
        }
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "reads this machine's UPower, power-profiles-daemon and logind"]
    fn reads_this_machine() {
        let state = Power::default().call(PowerCall::State).unwrap();
        println!("{}", serde_json::to_string_pretty(&state).unwrap());
        assert!(state["settings"]["ac"]["idleAfter"].is_u64());
    }
}
