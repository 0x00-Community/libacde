use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::rc::{Rc, Weak as RcWeak};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::Duration;

use pipewire as pw;
use pw::spa;
use pw::spa::param::ParamType;
use pw::spa::pod::deserialize::PodDeserializer;
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Object, Pod, Property, Value as PodValue, ValueArray};
use pw::types::ObjectType;
use serde_json::{Value, json};

use crate::runtime::sync::Locked;

const MAX_VOLUME: f64 = 1.5;
const SETTLE: Duration = Duration::from_millis(80);
const ANSWER: Duration = Duration::from_secs(5);
const PATIENCE: u32 = 8;

const SINK: &str = "default.audio.sink";
const SOURCE: &str = "default.audio.source";

const LEFT: [u32; 9] = [
    spa::sys::SPA_AUDIO_CHANNEL_FL,
    spa::sys::SPA_AUDIO_CHANNEL_SL,
    spa::sys::SPA_AUDIO_CHANNEL_FLC,
    spa::sys::SPA_AUDIO_CHANNEL_RL,
    spa::sys::SPA_AUDIO_CHANNEL_TFL,
    spa::sys::SPA_AUDIO_CHANNEL_TRL,
    spa::sys::SPA_AUDIO_CHANNEL_RLC,
    spa::sys::SPA_AUDIO_CHANNEL_FLW,
    spa::sys::SPA_AUDIO_CHANNEL_FLH,
];
const RIGHT: [u32; 9] = [
    spa::sys::SPA_AUDIO_CHANNEL_FR,
    spa::sys::SPA_AUDIO_CHANNEL_SR,
    spa::sys::SPA_AUDIO_CHANNEL_FRC,
    spa::sys::SPA_AUDIO_CHANNEL_RR,
    spa::sys::SPA_AUDIO_CHANNEL_TFR,
    spa::sys::SPA_AUDIO_CHANNEL_TRR,
    spa::sys::SPA_AUDIO_CHANNEL_RRC,
    spa::sys::SPA_AUDIO_CHANNEL_FRW,
    spa::sys::SPA_AUDIO_CHANNEL_FRH,
];

pub enum SoundCall {
    State,
    SetDefault { id: String },
    SetVolume { id: String, volume: f64 },
    SetMuted { id: String, muted: bool },
    SetBalance { id: String, balance: f64 },
    SetPort { id: String, port: String },
    SetProfile { card: String, profile: String },
    MoveStream { id: String, device: Option<String> },
}

type Answer = Result<Value, String>;
type Reply = mpsc::Sender<Answer>;
type SoundSink = Box<dyn Fn(Value) -> bool + Send>;

enum Cmd {
    Call(SoundCall, Reply),
}

#[derive(Default)]
struct Hub {
    tx: Option<pw::channel::Sender<Cmd>>,
    sinks: Vec<SoundSink>,
    notify: Option<mpsc::Sender<()>>,
}

#[derive(Clone, Default)]
pub struct Sounds {
    hub: Arc<Mutex<Hub>>,
}

impl Sounds {
    fn sender(&self) -> Result<pw::channel::Sender<Cmd>, String> {
        let mut hub = self.hub.locked();
        if let Some(tx) = &hub.tx {
            return Ok(tx.clone());
        }
        let (ready, started) = mpsc::sync_channel(1);
        let weak = Arc::downgrade(&self.hub);
        std::thread::Builder::new().name("acde-sound".into()).spawn(move || serve(weak, ready)).map_err(|e| format!("cannot start the sound thread: {e}"))?;
        let tx = started.recv_timeout(ANSWER).map_err(|_| "PipeWire did not answer".to_owned())??;
        hub.tx = Some(tx.clone());
        Ok(tx)
    }

    pub fn call(&self, call: SoundCall) -> Answer {
        let (reply, answer) = mpsc::channel();
        let mut cmd = Cmd::Call(call, reply);
        for _ in 0..2 {
            match self.sender()?.send(cmd) {
                Ok(()) => return answer.recv_timeout(ANSWER).map_err(|_| "PipeWire did not answer".to_owned())?,
                Err(back) => {
                    self.hub.locked().tx = None;
                    cmd = back;
                }
            }
        }
        Err("PipeWire is not running".to_owned())
    }

    pub fn listen(&self, sink: impl Fn(Value) -> bool + Send + 'static) -> Result<(), String> {
        self.sender()?;
        let start = {
            let mut hub = self.hub.locked();
            hub.sinks.push(Box::new(sink));
            hub.notify.is_none()
        };
        if !start {
            return Ok(());
        }
        let (tx, rx) = mpsc::channel::<()>();
        let sounds = self.clone();
        std::thread::Builder::new()
            .name("acde-sound-state".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    while rx.recv_timeout(SETTLE).is_ok() {}
                    if let Ok(state) = sounds.call(SoundCall::State) {
                        sounds.hub.locked().sinks.retain(|sink| sink(state.clone()));
                    }
                }
            })
            .map_err(|e| format!("cannot start the sound thread: {e}"))?;
        self.hub.locked().notify = Some(tx);
        Ok(())
    }
}

fn changed(hub: &Weak<Mutex<Hub>>) {
    if let Some(hub) = hub.upgrade()
        && let Some(tx) = &hub.locked().notify
    {
        _ = tx.send(());
    }
}

struct NodeObj {
    proxy: pw::node::Node,
    _listener: pw::node::NodeListener,
    global: HashMap<String, String>,
    info: HashMap<String, String>,
    params: HashMap<u32, Vec<PodValue>>,
}

struct DeviceObj {
    proxy: pw::device::Device,
    _listener: pw::device::DeviceListener,
    info: HashMap<String, String>,
    params: HashMap<u32, Vec<PodValue>>,
}

struct Batch {
    object: u32,
    id: u32,
    seq: i32,
    sync: Option<i32>,
    last: Option<u32>,
    items: Vec<PodValue>,
}

struct Waiter {
    sync: i32,
    left: u32,
    reply: Reply,
}

struct Model {
    core: pw::core::CoreRc,
    hub: Weak<Mutex<Hub>>,
    nodes: HashMap<u32, NodeObj>,
    devices: HashMap<u32, DeviceObj>,
    links: HashMap<u32, (u32, u32)>,
    metadata: Option<(u32, pw::metadata::Metadata, pw::metadata::MetadataListener)>,
    defaults: HashMap<String, String>,
    batches: Vec<Batch>,
    inflight: Vec<i32>,
    waiters: Vec<Waiter>,
    seq: i32,
}

type Shared = Rc<RefCell<Model>>;

fn dict(props: Option<&spa::utils::dict::DictRef>) -> HashMap<String, String> {
    props.map(|d| d.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect()).unwrap_or_default()
}

fn serve(hub: Weak<Mutex<Hub>>, ready: mpsc::SyncSender<Result<pw::channel::Sender<Cmd>, String>>) {
    let started = (|| -> Result<_, pw::Error> {
        pw::init();
        let main = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&main, None)?;
        let core = context.connect_rc(None)?;
        let registry = core.get_registry_rc()?;
        Ok((main, context, core, registry))
    })();
    let (main, _context, core, registry) = match started {
        Ok(started) => started,
        Err(error) => {
            log::warn!("声音: 连不上 PipeWire: {error}");
            _ = ready.try_send(Err(format!("PipeWire is not running: {error}")));
            return;
        }
    };
    let model: Shared = Rc::new(RefCell::new(Model {
        core: core.clone(),
        hub: hub.clone(),
        nodes: HashMap::new(),
        devices: HashMap::new(),
        links: HashMap::new(),
        metadata: None,
        defaults: HashMap::new(),
        batches: Vec::new(),
        inflight: Vec::new(),
        waiters: Vec::new(),
        seq: 0,
    }));
    let weak_main = main.downgrade();
    let done_model = Rc::downgrade(&model);
    let _core_listener = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE
                && let Some(model) = done_model.upgrade()
            {
                done(&model, seq.seq());
            }
        })
        .error(move |id, _, res, message| {
            log::warn!("声音: PipeWire 出错 (对象 {id}, {res}): {message}");
            if id == pw::core::PW_ID_CORE
                && let Some(main) = weak_main.upgrade()
            {
                main.quit();
            }
        })
        .register();
    let weak_registry = registry.downgrade();
    let add_model = Rc::downgrade(&model);
    let remove_model = Rc::downgrade(&model);
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            if let (Some(model), Some(registry)) = (add_model.upgrade(), weak_registry.upgrade()) {
                added(&model, &registry, global);
            }
        })
        .global_remove(move |id| {
            if let Some(model) = remove_model.upgrade() {
                removed(&model, id);
            }
        })
        .register();
    let (tx, rx) = pw::channel::channel::<Cmd>();
    let cmd_model = Rc::downgrade(&model);
    let _rx = rx.attach(main.loop_(), move |Cmd::Call(call, reply)| {
        if let Some(model) = cmd_model.upgrade() {
            handle(&model, call, reply);
        }
    });
    if ready.try_send(Ok(tx)).is_ok() {
        main.run();
    }
    log::info!("声音: PipeWire 连接断了");
    if let Some(hub) = hub.upgrade() {
        hub.locked().tx = None;
    }
    let mut m = model.borrow_mut();
    for waiter in m.waiters.drain(..) {
        _ = waiter.reply.send(Err("PipeWire stopped".to_owned()));
    }
}

fn sync(m: &mut Model) -> Option<i32> {
    match m.core.sync(0) {
        Ok(seq) => {
            m.inflight.push(seq.seq());
            Some(seq.seq())
        }
        Err(e) => {
            log::warn!("声音: PipeWire sync: {e}");
            None
        }
    }
}

fn done(model: &Shared, seq: i32) {
    let mut m = model.borrow_mut();
    m.inflight.retain(|s| *s > seq);
    let mut committed = false;
    let batches = std::mem::take(&mut m.batches);
    for batch in batches {
        if batch.sync.is_some_and(|s| s <= seq) {
            committed = true;
            let params = if let Some(node) = m.nodes.get_mut(&batch.object) {
                &mut node.params
            } else if let Some(device) = m.devices.get_mut(&batch.object) {
                &mut device.params
            } else {
                continue;
            };
            params.insert(batch.id, batch.items);
        } else {
            m.batches.push(batch);
        }
    }
    if committed {
        changed(&m.hub);
    }
    let idle = m.inflight.is_empty();
    let waiters = std::mem::take(&mut m.waiters);
    let mut keep = Vec::new();
    for mut waiter in waiters {
        if waiter.sync > seq {
            keep.push(waiter);
        } else if idle || waiter.left == 0 {
            _ = waiter.reply.send(Ok(snapshot(&m)));
        } else {
            waiter.left -= 1;
            waiter.sync = m.inflight.iter().copied().max().unwrap_or(seq);
            keep.push(waiter);
        }
    }
    m.waiters = keep;
}

fn refresh(model: &Shared, object: u32, ids: &[ParamType]) {
    let mut m = model.borrow_mut();
    let mut fresh = Vec::new();
    for id in ids {
        m.seq = m.seq.wrapping_add(1).max(1);
        let seq = m.seq;
        let raw = id.as_raw();
        m.batches.retain(|b| !(b.object == object && b.id == raw));
        m.batches.push(Batch { object, id: raw, seq, sync: None, last: None, items: Vec::new() });
        if let Some(node) = m.nodes.get(&object) {
            node.proxy.enum_params(seq, Some(*id), 0, u32::MAX);
        } else if let Some(device) = m.devices.get(&object) {
            device.proxy.enum_params(seq, Some(*id), 0, u32::MAX);
        }
        fresh.push(seq);
    }
    let sync = sync(&mut m);
    for batch in m.batches.iter_mut().filter(|b| fresh.contains(&b.seq)) {
        batch.sync = sync;
    }
}

fn param(model: &Shared, object: u32, index: u32, id: ParamType, pod: Option<&Pod>) {
    let Some(pod) = pod else { return };
    let Ok((_, value)) = PodDeserializer::deserialize_any_from(pod.as_bytes()) else { return };
    let mut m = model.borrow_mut();
    if let Some(batch) = m.batches.iter_mut().find(|b| b.object == object && b.id == id.as_raw()) {
        if batch.last.is_some_and(|last| index <= last) {
            batch.items.clear();
        }
        batch.last = Some(index);
        batch.items.push(value);
    }
}

fn wanted(class: &str) -> bool {
    class.starts_with("Audio/Sink") || class.starts_with("Audio/Source") || class == "Stream/Output/Audio" || class == "Stream/Input/Audio"
}

fn added(model: &Shared, registry: &pw::registry::RegistryRc, global: &pw::registry::GlobalObject<&spa::utils::dict::DictRef>) {
    let props = dict(global.props);
    let id = global.id;
    match global.type_ {
        ObjectType::Node if props.get("media.class").is_some_and(|c| wanted(c)) => {
            let Ok(proxy) = registry.bind::<pw::node::Node, _>(global) else { return };
            let (info_model, param_model) = (Rc::downgrade(model), Rc::downgrade(model));
            let listener = proxy
                .add_listener_local()
                .info(move |info| node_info(&info_model, id, info))
                .param(move |_, kind, index, _, pod| {
                    if let Some(model) = param_model.upgrade() {
                        param(&model, id, index, kind, pod);
                    }
                })
                .register();
            let mut m = model.borrow_mut();
            m.nodes.insert(id, NodeObj { proxy, _listener: listener, global: props, info: HashMap::new(), params: HashMap::new() });
            sync(&mut m);
            changed(&m.hub);
        }
        ObjectType::Device if props.get("media.class").is_some_and(|c| c == "Audio/Device") => {
            let Ok(proxy) = registry.bind::<pw::device::Device, _>(global) else { return };
            let (info_model, param_model) = (Rc::downgrade(model), Rc::downgrade(model));
            let listener = proxy
                .add_listener_local()
                .info(move |info| device_info(&info_model, id, info))
                .param(move |_, kind, index, _, pod| {
                    if let Some(model) = param_model.upgrade() {
                        param(&model, id, index, kind, pod);
                    }
                })
                .register();
            let mut m = model.borrow_mut();
            m.devices.insert(id, DeviceObj { proxy, _listener: listener, info: props, params: HashMap::new() });
            sync(&mut m);
            changed(&m.hub);
        }
        ObjectType::Metadata if props.get("metadata.name").is_some_and(|n| n == "default") => {
            let Ok(proxy) = registry.bind::<pw::metadata::Metadata, _>(global) else { return };
            let weak = Rc::downgrade(model);
            let listener = proxy
                .add_listener_local()
                .property(move |subject, key, _, value| {
                    if let Some(model) = weak.upgrade() {
                        metadata(&model, subject, key, value);
                    }
                    0
                })
                .register();
            let mut m = model.borrow_mut();
            m.metadata = Some((id, proxy, listener));
            sync(&mut m);
        }
        ObjectType::Link => {
            let node = |key: &str| props.get(key).and_then(|v| v.parse::<u32>().ok());
            if let (Some(out), Some(input)) = (node("link.output.node"), node("link.input.node")) {
                let mut m = model.borrow_mut();
                m.links.insert(id, (out, input));
                changed(&m.hub);
            }
        }
        _ => {}
    }
}

fn removed(model: &Shared, id: u32) {
    let mut m = model.borrow_mut();
    let hit = m.nodes.remove(&id).is_some() || m.devices.remove(&id).is_some() || m.links.remove(&id).is_some();
    if m.metadata.as_ref().is_some_and(|(meta, _, _)| *meta == id) {
        m.metadata = None;
        m.defaults.clear();
    }
    m.batches.retain(|b| b.object != id);
    if hit {
        changed(&m.hub);
    }
}

fn metadata(model: &Shared, subject: u32, key: Option<&str>, value: Option<&str>) {
    if subject != pw::core::PW_ID_CORE {
        return;
    }
    let mut m = model.borrow_mut();
    match key {
        None => m.defaults.clear(),
        Some(key) if key == SINK || key == SOURCE => {
            let name = value.and_then(|v| serde_json::from_str::<Value>(v).ok()).and_then(|v| v["name"].as_str().map(str::to_owned));
            match name {
                Some(name) => m.defaults.insert(key.to_owned(), name),
                None => m.defaults.remove(key),
            };
        }
        Some(_) => return,
    }
    changed(&m.hub);
}

fn readable(params: &[spa::param::ParamInfo], ids: &[ParamType]) -> Vec<ParamType> {
    ids.iter().copied().filter(|id| params.iter().any(|p| p.id() == *id && p.flags().contains(spa::param::ParamInfoFlags::READ))).collect()
}

fn node_info(model: &RcWeak<RefCell<Model>>, id: u32, info: &pw::node::NodeInfoRef) {
    let Some(model) = model.upgrade() else { return };
    let mask = info.change_mask();
    if mask.contains(pw::node::NodeChangeMask::PROPS) {
        let mut m = model.borrow_mut();
        if let Some(node) = m.nodes.get_mut(&id) {
            node.info = dict(info.props());
        }
        changed(&m.hub);
    }
    if mask.contains(pw::node::NodeChangeMask::PARAMS) {
        let ids = readable(info.params(), &[ParamType::Props]);
        if !ids.is_empty() {
            refresh(&model, id, &ids);
        }
    }
}

fn device_info(model: &RcWeak<RefCell<Model>>, id: u32, info: &pw::device::DeviceInfoRef) {
    let Some(model) = model.upgrade() else { return };
    let mask = info.change_mask();
    if mask.contains(pw::device::DeviceChangeMask::PROPS) {
        let mut m = model.borrow_mut();
        if let Some(device) = m.devices.get_mut(&id) {
            device.info.extend(dict(info.props()));
        }
        changed(&m.hub);
    }
    if mask.contains(pw::device::DeviceChangeMask::PARAMS) {
        let ids = readable(info.params(), &[ParamType::EnumProfile, ParamType::Profile, ParamType::EnumRoute, ParamType::Route]);
        if !ids.is_empty() {
            refresh(&model, id, &ids);
        }
    }
}

fn field(value: &PodValue, key: u32) -> Option<&PodValue> {
    match value {
        PodValue::Object(o) => o.properties.iter().find(|p| p.key == key).map(|p| &p.value),
        _ => None,
    }
}

fn int(value: Option<&PodValue>) -> Option<i64> {
    match value? {
        PodValue::Int(i) => Some(i64::from(*i)),
        PodValue::Long(i) => Some(*i),
        PodValue::Id(id) => Some(i64::from(id.0)),
        _ => None,
    }
}

fn text(value: Option<&PodValue>) -> Option<&str> {
    match value? {
        PodValue::String(s) => Some(s),
        _ => None,
    }
}

fn ints(value: Option<&PodValue>) -> Vec<i64> {
    match value {
        Some(PodValue::ValueArray(ValueArray::Int(v))) => v.iter().map(|i| i64::from(*i)).collect(),
        Some(PodValue::ValueArray(ValueArray::Id(v))) => v.iter().map(|i| i64::from(i.0)).collect(),
        _ => Vec::new(),
    }
}

fn info_of(value: &PodValue, key: u32) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(PodValue::Struct(items)) = field(value, key) {
        let strings: Vec<&str> = items.iter().filter_map(|v| text(Some(v))).collect();
        for pair in strings.chunks(2) {
            if let [k, v] = pair {
                out.insert((*k).to_owned(), (*v).to_owned());
            }
        }
    }
    out
}

#[derive(Clone)]
struct Vol {
    channels: Vec<f64>,
    map: Vec<u32>,
    mute: bool,
}

impl Vol {
    fn of(props: &PodValue) -> Option<Vol> {
        let channels: Vec<f64> = match field(props, spa::sys::SPA_PROP_channelVolumes) {
            Some(PodValue::ValueArray(ValueArray::Float(v))) => v.iter().map(|c| f64::from(*c).max(0.0).cbrt()).collect(),
            _ => Vec::new(),
        };
        let mute = matches!(field(props, spa::sys::SPA_PROP_mute), Some(PodValue::Bool(true)));
        if channels.is_empty() && field(props, spa::sys::SPA_PROP_mute).is_none() {
            return None;
        }
        let mut map: Vec<u32> = ints(field(props, spa::sys::SPA_PROP_channelMap)).into_iter().map(|c| c as u32).collect();
        if map.len() != channels.len() {
            map = match channels.len() {
                2 => vec![spa::sys::SPA_AUDIO_CHANNEL_FL, spa::sys::SPA_AUDIO_CHANNEL_FR],
                n => vec![spa::sys::SPA_AUDIO_CHANNEL_UNKNOWN; n],
            };
        }
        Some(Vol { channels, map, mute })
    }

    fn volume(&self) -> f64 {
        self.channels.iter().copied().fold(0.0, f64::max)
    }

    fn sides(&self) -> Option<(f64, f64)> {
        let side = |set: &[u32]| {
            let picked: Vec<f64> = self.channels.iter().zip(&self.map).filter(|(_, c)| set.contains(c)).map(|(v, _)| *v).collect();
            (!picked.is_empty()).then(|| picked.iter().sum::<f64>() / picked.len() as f64)
        };
        Some((side(&LEFT)?, side(&RIGHT)?))
    }

    fn balance(&self) -> Option<f64> {
        let (left, right) = self.sides()?;
        Some(if (left - right).abs() < 1e-9 {
            0.0
        } else if right > left {
            1.0 - left / right
        } else {
            right / left - 1.0
        })
    }

    fn scale(&mut self, volume: f64) {
        let max = self.volume();
        for c in &mut self.channels {
            *c = if max <= 0.0 { volume } else { *c * volume / max };
        }
    }

    fn set_balance(&mut self, balance: f64) -> bool {
        let Some((left, right)) = self.sides() else { return false };
        let max = left.max(right);
        let (nl, nr) = if balance <= 0.0 { (max, (balance + 1.0) * max) } else { ((1.0 - balance) * max, max) };
        for (v, c) in self.channels.iter_mut().zip(&self.map) {
            if LEFT.contains(c) {
                *v = if left <= 0.0 { nl } else { *v * nl / left };
            } else if RIGHT.contains(c) {
                *v = if right <= 0.0 { nr } else { *v * nr / right };
            }
        }
        true
    }

    fn linear(&self) -> Vec<f32> {
        self.channels.iter().map(|c| c.clamp(0.0, MAX_VOLUME).powi(3) as f32).collect()
    }
}

impl Model {
    fn node(&self, id: &str) -> Result<(u32, &NodeObj), String> {
        let n: u32 = id.parse().map_err(|_| format!("no sound device or stream {id}"))?;
        self.nodes.get(&n).map(|node| (n, node)).ok_or_else(|| format!("no sound device or stream {id}"))
    }

    fn target(&self, node: &'_ NodeObj) -> Option<(&DeviceObj, i64)> {
        let device: u32 = node.info.get("device.id").and_then(|v| v.parse().ok())?;
        let profile_device: i64 = node.info.get("card.profile.device").and_then(|v| v.parse().ok())?;
        Some((self.devices.get(&device)?, profile_device))
    }

    fn route<'a>(&self, device: &'a DeviceObj, profile_device: i64) -> Option<&'a PodValue> {
        device.params.get(&ParamType::Route.as_raw())?.iter().find(|r| int(field(r, spa::sys::SPA_PARAM_ROUTE_device)) == Some(profile_device))
    }

    fn volume_of(&self, node: &NodeObj) -> Option<Vol> {
        let routed = self.target(node).and_then(|(d, pd)| self.route(d, pd)).and_then(|r| field(r, spa::sys::SPA_PARAM_ROUTE_props)).and_then(Vol::of);
        routed.or_else(|| node.params.get(&ParamType::Props.as_raw()).and_then(|p| p.iter().find_map(Vol::of)))
    }

    fn name_of(node: &NodeObj, keys: &[&str]) -> String {
        keys.iter().find_map(|k| node.info.get(*k).or_else(|| node.global.get(*k)).filter(|v| !v.is_empty())).cloned().unwrap_or_default()
    }

    fn class(node: &NodeObj) -> &str {
        node.global.get("media.class").map(String::as_str).unwrap_or_default()
    }

    fn active_profile(device: &DeviceObj) -> Option<i64> {
        device.params.get(&ParamType::Profile.as_raw())?.first().and_then(|p| int(field(p, spa::sys::SPA_PARAM_PROFILE_index)))
    }

    fn ports(&self, node: &NodeObj, output: bool) -> (Vec<Value>, Option<String>, Option<String>) {
        let Some((device, profile_device)) = self.target(node) else { return (Vec::new(), None, None) };
        let direction = i64::from(if output { spa::sys::SPA_DIRECTION_OUTPUT } else { spa::sys::SPA_DIRECTION_INPUT });
        let profile = Self::active_profile(device);
        let mut ports: Vec<(i64, Value)> = device
            .params
            .get(&ParamType::EnumRoute.as_raw())
            .into_iter()
            .flatten()
            .filter(|r| int(field(r, spa::sys::SPA_PARAM_ROUTE_direction)) == Some(direction))
            .filter(|r| ints(field(r, spa::sys::SPA_PARAM_ROUTE_devices)).contains(&profile_device))
            .filter(|r| {
                profile.is_none_or(|p| {
                    let profiles = ints(field(r, spa::sys::SPA_PARAM_ROUTE_profiles));
                    profiles.is_empty() || profiles.contains(&p)
                })
            })
            .filter_map(|r| {
                let index = int(field(r, spa::sys::SPA_PARAM_ROUTE_index))?;
                let priority = int(field(r, spa::sys::SPA_PARAM_ROUTE_priority)).unwrap_or(0);
                let name = text(field(r, spa::sys::SPA_PARAM_ROUTE_name)).unwrap_or_default();
                let label = text(field(r, spa::sys::SPA_PARAM_ROUTE_description)).filter(|d| !d.is_empty()).unwrap_or(name);
                Some((priority, json!({ "id": index.to_string(), "name": label, "available": available(field(r, spa::sys::SPA_PARAM_ROUTE_available)) })))
            })
            .collect();
        ports.sort_by_key(|p| std::cmp::Reverse(p.0));
        let active = self.route(device, profile_device);
        let kind = active.and_then(|r| {
            let info = info_of(r, spa::sys::SPA_PARAM_ROUTE_info);
            let name = text(field(r, spa::sys::SPA_PARAM_ROUTE_name)).unwrap_or_default().to_owned();
            port_kind(info.get("port.type").map(String::as_str).unwrap_or_default(), &name)
        });
        let active = active.and_then(|r| int(field(r, spa::sys::SPA_PARAM_ROUTE_index))).map(|i| i.to_string());
        (ports.into_iter().map(|(_, p)| p).collect(), active, kind)
    }

    fn device_json(&self, id: u32, node: &NodeObj, output: bool) -> Value {
        let default = self.defaults.get(if output { SINK } else { SOURCE });
        let name = node.global.get("node.name").or_else(|| node.info.get("node.name"));
        let vol = self.volume_of(node);
        let (ports, active, kind) = self.ports(node, output);
        let card = self.target(node).map(|_| node.info.get("device.id").cloned().unwrap_or_default());
        let bus = self.target(node).and_then(|(d, _)| d.info.get("device.bus").cloned()).unwrap_or_default();
        let port = match bus.as_str() {
            "bluetooth" => "bluetooth".to_owned(),
            _ => kind.unwrap_or_else(|| {
                if bus == "usb" {
                    "usb".to_owned()
                } else if output {
                    "speaker".to_owned()
                } else {
                    "mic".to_owned()
                }
            }),
        };
        json!({
            "id": id.to_string(),
            "name": Self::name_of(node, &["node.description", "node.nick", "node.name"]),
            "port": port,
            "isDefault": default.is_some() && default == name,
            "volume": vol.as_ref().map(Vol::volume).unwrap_or(0.0),
            "muted": vol.as_ref().is_some_and(|v| v.mute),
            "balance": vol.as_ref().and_then(Vol::balance),
            "ports": ports,
            "activePort": active,
            "card": card,
            "virtual": card.is_none(),
        })
    }

    fn stream_json(&self, id: u32, node: &NodeObj, playback: bool) -> Value {
        let vol = self.volume_of(node);
        let device = self.links.values().find_map(|(out, input)| {
            let other = if playback && *out == id {
                *input
            } else if !playback && *input == id {
                *out
            } else {
                return None;
            };
            self.nodes.get(&other).filter(|n| Self::class(n).starts_with("Audio/")).map(|_| other.to_string())
        });
        json!({
            "id": id.to_string(),
            "kind": if playback { "playback" } else { "capture" },
            "app": Self::name_of(node, &["application.name", "node.description", "application.process.binary", "node.name"]),
            "title": Self::name_of(node, &["media.name"]),
            "icon": Self::name_of(node, &["application.icon-name", "media.icon-name"]),
            "volume": vol.as_ref().map(Vol::volume).unwrap_or(0.0),
            "muted": vol.as_ref().is_some_and(|v| v.mute),
            "balance": vol.as_ref().and_then(Vol::balance),
            "device": device,
        })
    }

    fn card_json(id: u32, device: &DeviceObj) -> Value {
        let active = Self::active_profile(device);
        let mut profiles: Vec<(i64, bool, Value)> = device
            .params
            .get(&ParamType::EnumProfile.as_raw())
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let name = text(field(p, spa::sys::SPA_PARAM_PROFILE_name))?.to_owned();
                let label = text(field(p, spa::sys::SPA_PARAM_PROFILE_description)).filter(|d| !d.is_empty()).unwrap_or(&name).to_owned();
                let priority = int(field(p, spa::sys::SPA_PARAM_PROFILE_priority)).unwrap_or(0);
                let available = available(field(p, spa::sys::SPA_PARAM_PROFILE_available));
                let index = int(field(p, spa::sys::SPA_PARAM_PROFILE_index));
                let off = name == "off";
                Some((priority, off, json!({ "id": name, "name": label, "available": available, "active": index.is_some() && index == active })))
            })
            .collect();
        profiles.sort_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
        let profile = profiles.iter().find(|(_, _, p)| p["active"] == true).map(|(_, _, p)| p["id"].clone()).unwrap_or(Value::Null);
        let label = ["device.description", "device.nick", "device.name"]
            .iter()
            .find_map(|k| device.info.get(*k).filter(|v| !v.is_empty()))
            .cloned()
            .unwrap_or_default();
        json!({
            "id": id.to_string(),
            "name": label,
            "icon": device.info.get("device.icon-name").cloned().unwrap_or_default(),
            "bus": device.info.get("device.bus").cloned().unwrap_or_default(),
            "profile": profile,
            "profiles": profiles.into_iter().map(|(_, _, mut p)| {
                if let Some(o) = p.as_object_mut() {
                    o.remove("active");
                }
                p
            }).collect::<Vec<_>>(),
        })
    }
}

fn available(value: Option<&PodValue>) -> Value {
    match int(value).map(|v| v as u32) {
        Some(spa::sys::SPA_PARAM_AVAILABILITY_yes) => Value::Bool(true),
        Some(spa::sys::SPA_PARAM_AVAILABILITY_no) => Value::Bool(false),
        _ => Value::Null,
    }
}

fn port_kind(port_type: &str, name: &str) -> Option<String> {
    let name = name.to_ascii_lowercase();
    let kind = match port_type {
        "headphones" | "headset" | "handset" => "headphones",
        "hdmi" | "displayport" => "hdmi",
        "speaker" => "speaker",
        "mic" => "mic",
        "usb" => "usb",
        "bluetooth" => "bluetooth",
        _ if name.contains("headphone") || name.contains("headset") => "headphones",
        _ if name.contains("hdmi") || name.contains("iec958") => "hdmi",
        _ if name.contains("speaker") => "speaker",
        _ if name.contains("mic") => "mic",
        _ => return None,
    };
    Some(kind.to_owned())
}

fn snapshot(m: &Model) -> Value {
    let mut ids: Vec<&u32> = m.nodes.keys().collect();
    ids.sort();
    let (mut outputs, mut inputs, mut streams) = (Vec::new(), Vec::new(), Vec::new());
    for id in ids {
        let node = &m.nodes[id];
        let class = Model::class(node);
        if class.starts_with("Audio/Sink") {
            outputs.push(m.device_json(*id, node, true));
        } else if class.starts_with("Audio/Source") {
            inputs.push(m.device_json(*id, node, false));
        } else if node.info.get("stream.monitor").is_none_or(|v| v != "true") {
            streams.push(m.stream_json(*id, node, class == "Stream/Output/Audio"));
        }
    }
    let mut cards: Vec<(&u32, &DeviceObj)> = m.devices.iter().collect();
    cards.sort_by_key(|(id, _)| **id);
    json!({
        "outputs": outputs,
        "inputs": inputs,
        "streams": streams,
        "cards": cards.into_iter().map(|(id, d)| Model::card_json(*id, d)).collect::<Vec<_>>(),
        "maxVolume": MAX_VOLUME,
    })
}

fn pod(value: &PodValue) -> Result<Vec<u8>, String> {
    PodSerializer::serialize(Cursor::new(Vec::new()), value).map(|(c, _)| c.into_inner()).map_err(|e| format!("cannot build a PipeWire param: {e:?}"))
}

fn object(type_: u32, id: u32, properties: Vec<Property>) -> PodValue {
    PodValue::Object(Object { type_, id, properties })
}

fn props(vol: Option<&Vol>, mute: Option<bool>) -> PodValue {
    let mut properties = Vec::new();
    if let Some(vol) = vol {
        properties.push(Property::new(spa::sys::SPA_PROP_channelVolumes, PodValue::ValueArray(ValueArray::Float(vol.linear()))));
    }
    if let Some(mute) = mute {
        properties.push(Property::new(spa::sys::SPA_PROP_mute, PodValue::Bool(mute)));
    }
    object(spa::sys::SPA_TYPE_OBJECT_Props, spa::sys::SPA_PARAM_Props, properties)
}

fn apply(m: &Model, id: &str, vol: Option<&Vol>, mute: Option<bool>) -> Result<(), String> {
    let (_, node) = m.node(id)?;
    let routed = m.target(node).and_then(|(d, pd)| m.route(d, pd).map(|r| (d, pd, r)));
    if let Some((device, profile_device, route)) = routed {
        let index = int(field(route, spa::sys::SPA_PARAM_ROUTE_index)).unwrap_or(0) as i32;
        let value = object(
            spa::sys::SPA_TYPE_OBJECT_ParamRoute,
            spa::sys::SPA_PARAM_Route,
            vec![
                Property::new(spa::sys::SPA_PARAM_ROUTE_index, PodValue::Int(index)),
                Property::new(spa::sys::SPA_PARAM_ROUTE_device, PodValue::Int(profile_device as i32)),
                Property::new(spa::sys::SPA_PARAM_ROUTE_props, props(vol, mute)),
                Property::new(spa::sys::SPA_PARAM_ROUTE_save, PodValue::Bool(true)),
            ],
        );
        let bytes = pod(&value)?;
        let pod = Pod::from_bytes(&bytes).ok_or("cannot build a PipeWire param")?;
        device.proxy.set_param(ParamType::Route, 0, pod);
    } else {
        let bytes = pod(&props(vol, mute))?;
        let pod = Pod::from_bytes(&bytes).ok_or("cannot build a PipeWire param")?;
        node.proxy.set_param(ParamType::Props, 0, pod);
    }
    Ok(())
}

fn current(m: &Model, id: &str) -> Result<Vol, String> {
    let (_, node) = m.node(id)?;
    m.volume_of(node).filter(|v| !v.channels.is_empty()).ok_or_else(|| format!("{id} has no volume"))
}

fn handle(model: &Shared, call: SoundCall, reply: Reply) {
    let mut m = model.borrow_mut();
    let result = match call {
        SoundCall::State => {
            if let Some(sync) = sync(&mut m) {
                m.waiters.push(Waiter { sync, left: PATIENCE, reply });
            } else {
                _ = reply.send(Ok(snapshot(&m)));
            }
            return;
        }
        SoundCall::SetDefault { id } => set_default(&m, &id),
        SoundCall::SetVolume { id, volume } => current(&m, &id).and_then(|mut vol| {
            if !volume.is_finite() {
                return Err(format!("the volume is a number from 0 to {MAX_VOLUME}"));
            }
            vol.scale(volume.clamp(0.0, MAX_VOLUME));
            apply(&m, &id, Some(&vol), None)
        }),
        SoundCall::SetMuted { id, muted } => m.node(&id).and_then(|_| apply(&m, &id, None, Some(muted))),
        SoundCall::SetBalance { id, balance } => current(&m, &id).and_then(|mut vol| {
            if !balance.is_finite() {
                return Err("the balance is a number from -1 to 1".to_owned());
            }
            if !vol.set_balance(balance.clamp(-1.0, 1.0)) {
                return Err(format!("{id} has no left and right channels"));
            }
            apply(&m, &id, Some(&vol), None)
        }),
        SoundCall::SetPort { id, port } => set_port(&m, &id, &port),
        SoundCall::SetProfile { card, profile } => set_profile(&m, &card, &profile),
        SoundCall::MoveStream { id, device } => move_stream(&m, &id, device.as_deref()),
    };
    _ = reply.send(result.map(|()| Value::Null));
}

fn set_default(m: &Model, id: &str) -> Result<(), String> {
    let (_, node) = m.node(id)?;
    let class = Model::class(node);
    let key = if class.starts_with("Audio/Sink") {
        "default.configured.audio.sink"
    } else if class.starts_with("Audio/Source") {
        "default.configured.audio.source"
    } else {
        return Err(crate::runtime::bad_request(format!("{id} is not a sound device")));
    };
    let name = node.global.get("node.name").or_else(|| node.info.get("node.name")).ok_or_else(|| format!("{id} has no name"))?;
    let (_, meta, _) = m.metadata.as_ref().ok_or("no session manager (WirePlumber) is running")?;
    meta.set_property(pw::core::PW_ID_CORE, key, Some("Spa:String:JSON"), Some(&json!({ "name": name }).to_string()));
    Ok(())
}

fn set_port(m: &Model, id: &str, port: &str) -> Result<(), String> {
    let (_, node) = m.node(id)?;
    let (device, profile_device) = m.target(node).ok_or_else(|| format!("{id} has no ports"))?;
    let index: i32 = port.parse().map_err(|_| format!("no port {port}"))?;
    let known = device
        .params
        .get(&ParamType::EnumRoute.as_raw())
        .into_iter()
        .flatten()
        .any(|r| int(field(r, spa::sys::SPA_PARAM_ROUTE_index)) == Some(i64::from(index)));
    if !known {
        return Err(format!("no port {port}"));
    }
    let value = object(
        spa::sys::SPA_TYPE_OBJECT_ParamRoute,
        spa::sys::SPA_PARAM_Route,
        vec![
            Property::new(spa::sys::SPA_PARAM_ROUTE_index, PodValue::Int(index)),
            Property::new(spa::sys::SPA_PARAM_ROUTE_device, PodValue::Int(profile_device as i32)),
            Property::new(spa::sys::SPA_PARAM_ROUTE_save, PodValue::Bool(true)),
        ],
    );
    let bytes = pod(&value)?;
    device.proxy.set_param(ParamType::Route, 0, Pod::from_bytes(&bytes).ok_or("cannot build a PipeWire param")?);
    Ok(())
}

fn set_profile(m: &Model, card: &str, profile: &str) -> Result<(), String> {
    let device = card.parse::<u32>().ok().and_then(|id| m.devices.get(&id)).ok_or_else(|| format!("no sound card {card}"))?;
    let index = device
        .params
        .get(&ParamType::EnumProfile.as_raw())
        .into_iter()
        .flatten()
        .find(|p| text(field(p, spa::sys::SPA_PARAM_PROFILE_name)) == Some(profile))
        .and_then(|p| int(field(p, spa::sys::SPA_PARAM_PROFILE_index)))
        .ok_or_else(|| format!("{card} has no profile {profile}"))?;
    let value = object(
        spa::sys::SPA_TYPE_OBJECT_ParamProfile,
        spa::sys::SPA_PARAM_Profile,
        vec![
            Property::new(spa::sys::SPA_PARAM_PROFILE_index, PodValue::Int(index as i32)),
            Property::new(spa::sys::SPA_PARAM_PROFILE_save, PodValue::Bool(true)),
        ],
    );
    let bytes = pod(&value)?;
    device.proxy.set_param(ParamType::Profile, 0, Pod::from_bytes(&bytes).ok_or("cannot build a PipeWire param")?);
    Ok(())
}

fn move_stream(m: &Model, id: &str, device: Option<&str>) -> Result<(), String> {
    let (stream, node) = m.node(id)?;
    let class = Model::class(node);
    if !class.starts_with("Stream/") {
        return Err(crate::runtime::bad_request(format!("{id} is not a stream")));
    }
    let serial = match device {
        Some(device) => {
            let (_, target) = m.node(device)?;
            let fits =
                if class == "Stream/Output/Audio" { Model::class(target).starts_with("Audio/Sink") } else { Model::class(target).starts_with("Audio/Source") };
            if !fits {
                return Err(format!("{id} cannot go to {device}"));
            }
            Some(target.global.get("object.serial").cloned().unwrap_or_else(|| device.to_owned()))
        }
        None => None,
    };
    let (_, meta, _) = m.metadata.as_ref().ok_or("no session manager (WirePlumber) is running")?;
    meta.set_property(stream, "target.node", None, None);
    meta.set_property(stream, "target.object", serial.as_ref().map(|_| "Spa:Id"), serial.as_deref());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_output(sounds: &Sounds) -> Value {
        let state = sounds.call(SoundCall::State).expect("state");
        state["outputs"].as_array().and_then(|o| o.iter().find(|d| d["isDefault"] == true).cloned()).expect("a default output")
    }

    #[test]
    #[ignore]
    fn state_of_the_session() {
        let sounds = Sounds::default();
        let state = sounds.call(SoundCall::State).expect("state");
        println!("{}", serde_json::to_string_pretty(&state).unwrap_or_default());
        let (tx, rx) = mpsc::channel();
        sounds.listen(move |state| tx.send(state).is_ok()).expect("listen");
        let out = default_output(&sounds);
        let id = out["id"].as_str().unwrap_or_default().to_owned();
        let volume = out["volume"].as_f64().unwrap_or(0.0);
        let muted = out["muted"] == true;
        let nudged = if volume > 0.5 { volume - 0.01 } else { volume + 0.01 };
        sounds.call(SoundCall::SetVolume { id: id.clone(), volume: nudged }).expect("nudge");
        let event = rx.recv_timeout(Duration::from_secs(3)).expect("a changed event");
        let seen = event["outputs"].as_array().and_then(|o| o.iter().find(|d| d["id"] == id.as_str())).and_then(|d| d["volume"].as_f64()).unwrap_or(-1.0);
        println!("nudged to {nudged}, event says {seen}");
        assert!((seen - nudged).abs() < 0.005);
        sounds.call(SoundCall::SetBalance { id: id.clone(), balance: 0.25 }).expect("balance");
        std::thread::sleep(Duration::from_millis(300));
        let balanced = default_output(&sounds);
        println!("balance {} volume {}", balanced["balance"], balanced["volume"]);
        assert!((balanced["balance"].as_f64().unwrap_or(0.0) - 0.25).abs() < 0.01);
        sounds.call(SoundCall::SetBalance { id: id.clone(), balance: 0.0 }).expect("balance back");
        sounds.call(SoundCall::SetMuted { id: id.clone(), muted: !muted }).expect("mute");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(default_output(&sounds)["muted"] == true, !muted);
        sounds.call(SoundCall::SetMuted { id: id.clone(), muted }).expect("unmute");
        sounds.call(SoundCall::SetVolume { id: id.clone(), volume }).expect("restore");
        std::thread::sleep(Duration::from_millis(300));
        let back = default_output(&sounds);
        println!("restored: volume {} muted {} balance {}", back["volume"], back["muted"], back["balance"]);
        assert!((back["volume"].as_f64().unwrap_or(0.0) - volume).abs() < 0.005);
    }
}
