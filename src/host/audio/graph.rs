#![allow(clippy::needless_range_loop)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use super::analyser::Tap;
use super::biquad::{Biquad, FilterType};
use super::compressor::Compressor;
use super::convolver::Convolver;
use super::delay::Delay;
use super::shaper::{Oversample, Shaper};
use super::spatial::{self, Hrtf, HrtfModel, Listener, PannerConfig, V3};

pub const QUANTUM: usize = 128;

pub type Bus = [[f32; QUANTUM]; 2];

pub const MAX_PARAMS: usize = 9;

pub struct AudioData {
    pub rate: f64,
    pub channels: Vec<Vec<f32>>,
}

impl AudioData {
    pub fn len(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }
}

pub struct Shared {
    params: Box<[AtomicU32]>,
    reduction: AtomicU32,
    pub tap: Option<Mutex<Tap>>,
}

impl Shared {
    pub fn new(kind: Kind) -> Shared {
        Shared {
            params: (0..kind.param_count()).map(|_| AtomicU32::new(0)).collect(),
            reduction: AtomicU32::new(0),
            tap: (kind == Kind::Analyser).then(|| Mutex::new(Tap::default())),
        }
    }

    pub fn reduction(&self) -> f32 {
        f32::from_bits(self.reduction.load(Ordering::Relaxed))
    }

    pub fn param(&self, i: usize) -> f32 {
        self.params.get(i).map_or(0.0, |p| f32::from_bits(p.load(Ordering::Relaxed)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Destination,
    Gain,
    Oscillator,
    BufferSource,
    StereoPanner,
    ConstantSource,
    Through,
    Stream,
    Panner,
    Listener,
    Biquad,
    Delay,
    WaveShaper,
    Compressor,
    Convolver,
    Analyser,
}

const LISTENER_DEFAULTS: [f32; 9] = [0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0];

impl Kind {
    pub fn from_code(code: u32) -> Option<Kind> {
        Some(match code {
            1 => Kind::Gain,
            2 => Kind::Oscillator,
            3 => Kind::BufferSource,
            4 => Kind::StereoPanner,
            5 => Kind::ConstantSource,
            6 => Kind::Through,
            7 => Kind::Stream,
            8 => Kind::Panner,
            9 => Kind::Listener,
            10 => Kind::Biquad,
            11 => Kind::Delay,
            12 => Kind::WaveShaper,
            13 => Kind::Compressor,
            14 => Kind::Convolver,
            15 => Kind::Analyser,
            _ => return None,
        })
    }

    fn is_source(self) -> bool {
        matches!(self, Kind::Oscillator | Kind::BufferSource | Kind::ConstantSource)
    }

    pub fn param_count(self) -> usize {
        match self {
            Kind::Oscillator | Kind::BufferSource => 2,
            Kind::Gain | Kind::StereoPanner | Kind::ConstantSource => 1,
            Kind::Panner => 6,
            Kind::Listener => 9,
            Kind::Biquad => 4,
            Kind::Delay => 1,
            Kind::Compressor => 5,
            Kind::Destination | Kind::Through | Kind::Stream | Kind::WaveShaper | Kind::Convolver | Kind::Analyser => 0,
        }
    }

    fn params(self, rate: f64) -> Vec<(f32, f32, f32)> {
        let nyquist = (rate / 2.0) as f32;
        let detune = 1200.0 * f32::MAX.log2();
        let any = |v: f32| (v, f32::MIN, f32::MAX);
        let params = match self {
            Kind::Gain => vec![any(1.0)],
            Kind::Oscillator => vec![(440.0, -nyquist, nyquist), (0.0, -detune, detune)],
            Kind::BufferSource => vec![any(1.0), any(0.0)],
            Kind::StereoPanner => vec![(0.0, -1.0, 1.0)],
            Kind::ConstantSource => vec![any(1.0)],
            Kind::Panner => [0.0, 0.0, 0.0, 1.0, 0.0, 0.0].map(any).to_vec(),
            Kind::Listener => LISTENER_DEFAULTS.map(any).to_vec(),
            Kind::Biquad => vec![(350.0, 0.0, nyquist), (0.0, -detune, detune), any(1.0), (0.0, f32::MIN, 40.0 * f32::MAX.log10())],
            Kind::Delay => vec![(0.0, 0.0, 180.0)],
            Kind::Compressor => vec![(-24.0, -100.0, 0.0), (30.0, 0.0, 40.0), (12.0, 1.0, 20.0), (0.003, 0.0, 1.0), (0.25, 0.0, 1.0)],
            Kind::Destination | Kind::Through | Kind::Stream | Kind::WaveShaper | Kind::Convolver | Kind::Analyser => vec![],
        };
        debug_assert_eq!(params.len(), self.param_count());
        params
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

impl Wave {
    pub fn from_code(code: u32) -> Wave {
        match code {
            1 => Wave::Square,
            2 => Wave::Sawtooth,
            3 => Wave::Triangle,
            _ => Wave::Sine,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ParamOp {
    Value(f32),
    SetAt(f32, f64),
    Linear(f32, f64),
    Exponential(f32, f64),
    Target(f32, f64, f64),
    Curve(Arc<[f32]>, f64, f64),
    Cancel(f64),
    CancelAndHold(f64),
}

pub enum NodeOp {
    Param(usize, ParamOp),
    Wave(Wave),
    Buffer(Option<Arc<AudioData>>),
    Loop { looping: bool, start: f64, end: f64 },
    Start { when: f64, offset: f64, duration: Option<f64> },
    Stop(f64),
    Release,
    Stream(Option<Box<super::live::LiveTap>>),
    Panner(PannerConfig),
    Filter(FilterType),
    Delay(Box<Delay>),
    Curve(Option<Arc<[f32]>>),
    Oversample(Oversample),
    Impulse(Option<Box<Convolver>>),
}

#[derive(Clone, Debug)]
enum EventKind {
    Set,
    Ramp(bool),
    Target(f64),
    Curve(Arc<[f32]>, f64),
}

#[derive(Clone, Debug)]
struct Event {
    time: f64,
    value: f32,
    kind: EventKind,
}

impl Event {
    fn end(&self) -> f64 {
        match &self.kind {
            EventKind::Curve(_, duration) => self.time + duration,
            _ => self.time,
        }
    }
}

#[derive(Clone, Debug)]
enum Curve {
    Const(f32),
    Ramp { t0: f64, v0: f32, t1: f64, v1: f32, exponential: bool },
    Target { t0: f64, v0: f32, target: f32, tc: f64 },
    Values { t0: f64, duration: f64, values: Arc<[f32]> },
}

impl Curve {
    fn at(&self, t: f64) -> f32 {
        match self {
            Curve::Const(v) => *v,
            Curve::Ramp { t0, v0, t1, v1, exponential } => {
                if t1 <= t0 {
                    return *v1;
                }
                if *exponential && (*v0 == 0.0 || (*v0 > 0.0) != (*v1 > 0.0)) {
                    return *v0;
                }
                let k = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
                let (v0, v1) = (*v0 as f64, *v1 as f64);
                (if *exponential { v0 * (v1 / v0).powf(k) } else { v0 + (v1 - v0) * k }) as f32
            }
            Curve::Target { t0, v0, target, tc } => {
                if *tc <= 0.0 {
                    return *target;
                }
                (*target as f64 + (*v0 as f64 - *target as f64) * (-(t - t0).max(0.0) / tc).exp()) as f32
            }
            Curve::Values { t0, duration, values } => {
                let n = values.len();
                if n == 0 {
                    return 0.0;
                }
                if n == 1 || *duration <= 0.0 {
                    return values[n - 1];
                }
                let x = ((t - t0) / duration * (n - 1) as f64).max(0.0);
                let k = x.floor() as usize;
                if k >= n - 1 {
                    return values[n - 1];
                }
                let f = (x - k as f64) as f32;
                values[k] + (values[k + 1] - values[k]) * f
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Seg {
    end: f64,
    curve: Curve,
}

struct Param {
    base: f32,
    min: f32,
    max: f32,
    events: Vec<Event>,
    segs: Vec<Seg>,
}

impl Param {
    fn new(value: f32, min: f32, max: f32) -> Param {
        Param { base: value, min, max, events: Vec::new(), segs: vec![Seg { end: f64::INFINITY, curve: Curve::Const(value) }] }
    }

    fn rebuild(&mut self) {
        self.segs.clear();
        let mut hold = Curve::Const(self.base);
        let mut prev: Option<(f64, f32)> = None;
        for e in &self.events {
            match &e.kind {
                EventKind::Ramp(exponential) => {
                    let curve = match prev {
                        Some((t0, v0)) => Curve::Ramp { t0, v0, t1: e.time, v1: e.value, exponential: *exponential },
                        None => hold.clone(),
                    };
                    self.segs.push(Seg { end: e.time, curve });
                    hold = Curve::Const(e.value);
                    prev = Some((e.time, e.value));
                }
                EventKind::Set => {
                    self.segs.push(Seg { end: e.time, curve: hold.clone() });
                    hold = Curve::Const(e.value);
                    prev = Some((e.time, e.value));
                }
                EventKind::Target(tc) => {
                    let v0 = hold.at(e.time);
                    self.segs.push(Seg { end: e.time, curve: hold.clone() });
                    hold = Curve::Target { t0: e.time, v0, target: e.value, tc: *tc };
                    prev = Some((e.time, v0));
                }
                EventKind::Curve(values, duration) => {
                    self.segs.push(Seg { end: e.time, curve: hold.clone() });
                    self.segs.push(Seg { end: e.time + duration, curve: Curve::Values { t0: e.time, duration: *duration, values: values.clone() } });
                    let last = values.last().copied().unwrap_or(0.0);
                    hold = Curve::Const(last);
                    prev = Some((e.time + duration, last));
                }
            }
        }
        self.segs.push(Seg { end: f64::INFINITY, curve: hold });
    }

    fn at(&self, t: f64) -> f32 {
        let i = self.segs.partition_point(|s| s.end <= t);
        self.segs.get(i).map_or(self.base, |s| s.curve.at(t))
    }

    fn insert(&mut self, event: Event) {
        let i = self.events.partition_point(|e| e.time <= event.time);
        self.events.insert(i, event);
    }

    fn apply(&mut self, op: ParamOp, now: f64) {
        match op {
            ParamOp::Value(v) => {
                if self.events.is_empty() {
                    self.base = v;
                } else {
                    self.insert(Event { time: now, value: v, kind: EventKind::Set });
                }
            }
            ParamOp::SetAt(v, t) => self.insert(Event { time: t, value: v, kind: EventKind::Set }),
            ParamOp::Linear(v, t) | ParamOp::Exponential(v, t) => {
                if !self.events.iter().any(|e| e.time < t) {
                    let current = self.at(now);
                    self.insert(Event { time: now.min(t), value: current, kind: EventKind::Set });
                }
                let kind = EventKind::Ramp(matches!(op, ParamOp::Exponential(..)));
                self.insert(Event { time: t, value: v, kind });
            }
            ParamOp::Target(v, t, tc) => self.insert(Event { time: t, value: v, kind: EventKind::Target(tc) }),
            ParamOp::Curve(values, t, duration) => {
                let value = values.first().copied().unwrap_or(0.0);
                self.insert(Event { time: t, value, kind: EventKind::Curve(values, duration) });
            }
            ParamOp::Cancel(t) => self.events.retain(|e| e.time < t),
            ParamOp::CancelAndHold(t) => {
                let held = self.at(t);
                let cut = self.events.iter().find(|e| e.time >= t).map(|e| e.kind.clone());
                self.events.retain(|e| e.time < t);
                if let Some(EventKind::Curve(values, duration)) = self.events.last().map(|e| e.kind.clone())
                    && let Some(last) = self.events.last_mut()
                    && last.time + duration > t
                {
                    let played = ((t - last.time) / duration).clamp(0.0, 1.0);
                    let n = ((values.len() as f64 - 1.0) * played).floor() as usize + 1;
                    let mut kept: Vec<f32> = values[..n.min(values.len())].to_vec();
                    kept.push(held);
                    last.kind = EventKind::Curve(kept.into(), t - last.time);
                } else {
                    let kind = match cut {
                        Some(EventKind::Ramp(exponential)) => EventKind::Ramp(exponential),
                        _ => EventKind::Set,
                    };
                    self.insert(Event { time: t, value: held, kind });
                }
            }
        }
        self.rebuild();
    }

    fn prune(&mut self, now: f64) {
        let mut changed = false;
        while self.events.len() >= 2 && self.events[1].end() <= now {
            self.base = self.at(self.events[1].time);
            self.events.remove(0);
            changed = true;
        }
        if let [only] = self.events.as_slice()
            && only.end() <= now
        {
            let settled = match only.kind {
                EventKind::Target(_) => {
                    let v = self.at(now);
                    ((v - only.value).abs() <= 1e-6 * only.value.abs().max(1.0)).then_some(only.value)
                }
                _ => Some(self.at(now)),
            };
            if let Some(v) = settled {
                self.base = v;
                self.events.clear();
                changed = true;
            }
        }
        if changed {
            self.rebuild();
        }
    }

    fn fill(&mut self, out: &mut [f32; QUANTUM], frame: u64, rate: f64) -> bool {
        let t0 = frame as f64 / rate;
        self.prune(t0);
        if self.events.is_empty() {
            out.fill(self.base);
            return true;
        }
        let mut i = self.segs.partition_point(|s| s.end <= t0);
        for (k, value) in out.iter_mut().enumerate() {
            let t = (frame + k as u64) as f64 / rate;
            while i + 1 < self.segs.len() && self.segs[i].end <= t {
                i += 1;
            }
            *value = self.segs[i].curve.at(t);
        }
        false
    }
}

struct Source {
    start: Option<u64>,
    stop: Option<u64>,
    ended: bool,
}

struct Buffer {
    data: Option<Arc<AudioData>>,
    pos: Option<f64>,
    offset: f64,
    duration: Option<f64>,
    played: f64,
    looping: bool,
    loop_start: f64,
    loop_end: f64,
}

struct Node {
    kind: Kind,
    inputs: Vec<u32>,
    param_inputs: Vec<Vec<u32>>,
    params: Vec<Param>,
    out: Bus,
    mono: bool,
    silent: bool,
    shared: Arc<Shared>,
    released: bool,
    src: Source,
    wave: Wave,
    phase: f64,
    buf: Buffer,
    stream: Option<Box<super::live::LiveTap>>,
    panner: Option<Box<Panner>>,
    effect: Effect,
    tail: u64,
    in_cycle: bool,
    muted: bool,
}

enum Effect {
    None,
    Biquad(Box<Biquad>),
    Delay(Option<Box<Delay>>, bool),
    Shaper(Box<Shaper>),
    Compressor(Box<Compressor>),
    Convolver(Option<Box<Convolver>>),
}

const QUIET: f32 = 1e-7;

struct Panner {
    config: PannerConfig,
    hrtf: Option<Hrtf>,
}

impl Node {
    fn new(kind: Kind, rate: f64, shared: Arc<Shared>) -> Node {
        let specs = kind.params(rate);
        for (slot, (d, ..)) in shared.params.iter().zip(&specs) {
            slot.store(d.to_bits(), Ordering::Relaxed);
        }
        Node {
            kind,
            inputs: Vec::new(),
            param_inputs: vec![Vec::new(); specs.len()],
            params: specs.into_iter().map(|(d, lo, hi)| Param::new(d, lo, hi)).collect(),
            out: [[0.0; QUANTUM]; 2],
            mono: true,
            silent: true,
            shared,
            released: false,
            src: Source { start: None, stop: None, ended: false },
            wave: Wave::Sine,
            phase: 0.0,
            buf: Buffer { data: None, pos: None, offset: 0.0, duration: None, played: 0.0, looping: false, loop_start: 0.0, loop_end: 0.0 },
            stream: None,
            panner: (kind == Kind::Panner).then(|| Box::new(Panner { config: PannerConfig::default(), hrtf: None })),
            effect: match kind {
                Kind::Biquad => Effect::Biquad(Box::default()),
                Kind::Delay => Effect::Delay(None, true),
                Kind::WaveShaper => Effect::Shaper(Box::default()),
                Kind::Compressor => Effect::Compressor(Box::new(Compressor::new(rate))),
                Kind::Convolver => Effect::Convolver(None),
                _ => Effect::None,
            },
            tail: 0,
            in_cycle: false,
            muted: false,
        }
    }

    fn playing_range(&self, frame: u64) -> Option<(usize, usize)> {
        let start = self.src.start?;
        if self.src.ended {
            return None;
        }
        let end = frame + QUANTUM as u64;
        let stop = self.src.stop.unwrap_or(u64::MAX);
        if start >= end || stop <= frame || stop <= start {
            return None;
        }
        Some((start.saturating_sub(frame) as usize, (stop.min(end) - frame) as usize))
    }
}

pub struct Graph {
    rate: f64,
    pub frame: u64,
    dest: u32,
    nodes: HashMap<u32, Node>,
    order: Vec<u32>,
    order_dirty: bool,
    input: Bus,
    values: Box<[[f32; QUANTUM]; MAX_PARAMS]>,
    still: [bool; MAX_PARAMS],
    listener: Option<u32>,
    listening: Box<[[f32; QUANTUM]; 9]>,
    listener_still: bool,
    hrtf: Option<Arc<HrtfModel>>,
    pub gone: Vec<u32>,
}

fn up_mix(acc: &mut Bus, acc_mono: &mut bool, empty: &mut bool, out: &Bus, mono: bool) {
    if *empty {
        acc[0] = out[0];
        acc[1] = if mono { out[0] } else { out[1] };
        *acc_mono = mono;
        *empty = false;
        return;
    }
    for i in 0..QUANTUM {
        acc[0][i] += out[0][i];
        acc[1][i] += if mono { out[0][i] } else { out[1][i] };
    }
    *acc_mono &= mono;
}

impl Graph {
    pub fn new(rate: f64, dest: u32) -> Graph {
        let mut nodes = HashMap::new();
        nodes.insert(dest, Node::new(Kind::Destination, rate, Arc::new(Shared::new(Kind::Destination))));
        let values = Box::new([[0.0; QUANTUM]; MAX_PARAMS]);
        let listening = Box::new(LISTENER_DEFAULTS.map(|v| [v; QUANTUM]));
        Graph {
            rate,
            frame: 0,
            dest,
            nodes,
            order: Vec::new(),
            order_dirty: true,
            input: [[0.0; QUANTUM]; 2],
            values,
            still: [true; MAX_PARAMS],
            listener: None,
            listening,
            listener_still: true,
            hrtf: None,
            gone: Vec::new(),
        }
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    fn now(&self) -> f64 {
        self.frame as f64 / self.rate
    }

    #[cfg(test)]
    pub fn has(&self, node: u32) -> bool {
        self.nodes.contains_key(&node)
    }

    pub fn add(&mut self, id: u32, kind: Kind, shared: Arc<Shared>) {
        if kind == Kind::Listener {
            self.listener = Some(id);
        }
        self.nodes.insert(id, Node::new(kind, self.rate, shared));
        self.order_dirty = true;
    }

    pub fn connect(&mut self, from: u32, to: u32, param: Option<usize>) {
        if !self.nodes.contains_key(&from) || from == self.dest {
            return;
        }
        let Some(node) = self.nodes.get_mut(&to) else { return };
        let list = match param {
            None => &mut node.inputs,
            Some(i) if i < node.param_inputs.len() => &mut node.param_inputs[i],
            Some(_) => return,
        };
        if !list.contains(&from) {
            list.push(from);
            self.order_dirty = true;
        }
    }

    pub fn disconnect(&mut self, from: u32, to: Option<u32>, param: Option<usize>) {
        for (id, node) in self.nodes.iter_mut() {
            if to.is_some_and(|to| to != *id) {
                continue;
            }
            match (to, param) {
                (Some(_), Some(i)) if i < node.param_inputs.len() => node.param_inputs[i].retain(|&n| n != from),
                (Some(_), Some(_)) => {}
                (Some(_), None) => node.inputs.retain(|&n| n != from),
                (None, _) => {
                    node.inputs.retain(|&n| n != from);
                    for list in &mut node.param_inputs {
                        list.retain(|&n| n != from);
                    }
                }
            }
        }
        self.order_dirty = true;
    }

    pub fn node(&mut self, id: u32, op: NodeOp) {
        let now = self.now();
        let frame = match op {
            NodeOp::Start { when, .. } | NodeOp::Stop(when) => self.frame_at(when),
            _ => 0,
        };
        let Some(node) = self.nodes.get_mut(&id) else { return };
        match op {
            NodeOp::Param(i, op) => {
                if let Some(param) = node.params.get_mut(i) {
                    param.apply(op, now);
                }
            }
            NodeOp::Wave(wave) => node.wave = wave,
            NodeOp::Buffer(data) => node.buf.data = data,
            NodeOp::Loop { looping, start, end } => (node.buf.looping, node.buf.loop_start, node.buf.loop_end) = (looping, start, end),
            NodeOp::Start { offset, duration, .. } => {
                if node.kind.is_source() && node.src.start.is_none() {
                    node.src.start = Some(frame);
                    node.buf.offset = offset.max(0.0);
                    node.buf.duration = duration.filter(|d| d.is_finite() && *d >= 0.0);
                }
            }
            NodeOp::Stop(_) => {
                if node.kind.is_source() {
                    node.src.stop = Some(frame);
                }
            }
            NodeOp::Release => node.released = true,
            NodeOp::Stream(tap) => {
                if node.kind == Kind::Stream {
                    node.stream = tap;
                }
            }
            NodeOp::Panner(config) => {
                if let Some(p) = &mut node.panner {
                    p.config = config;
                }
            }
            NodeOp::Filter(kind) => {
                if let Effect::Biquad(b) = &mut node.effect {
                    b.kind = kind;
                }
            }
            NodeOp::Delay(delay) => {
                if let Effect::Delay(d, _) = &mut node.effect {
                    if let Some(p) = node.params.get_mut(0) {
                        p.max = delay.max_seconds() as f32;
                    }
                    *d = Some(delay);
                    node.tail = 0;
                }
            }
            NodeOp::Curve(curve) => {
                if let Effect::Shaper(s) = &mut node.effect {
                    s.set_curve(curve);
                }
            }
            NodeOp::Oversample(oversample) => {
                if let Effect::Shaper(s) = &mut node.effect {
                    s.set_oversample(oversample);
                }
            }
            NodeOp::Impulse(convolver) => {
                if let Effect::Convolver(c) = &mut node.effect {
                    *c = convolver;
                    node.tail = 0;
                }
            }
        }
        if node.panner.as_ref().is_some_and(|p| p.config.hrtf) && self.hrtf.is_none() {
            self.hrtf = Some(Arc::new(HrtfModel::new(self.rate)));
        }
    }

    fn frame_at(&self, when: f64) -> u64 {
        if !when.is_finite() || when <= 0.0 {
            return self.frame;
        }
        let x = when * self.rate;
        let frame = if (x - x.round()).abs() < 1e-6 { x.round() } else { x.ceil() };
        (frame as u64).max(self.frame)
    }

    pub fn active(&self) -> bool {
        self.nodes
            .values()
            .any(|n| (n.kind.is_source() && n.src.start.is_some() && !n.src.ended) || (n.kind == Kind::Stream && n.stream.is_some()) || n.tail > 0)
    }

    #[cfg(test)]
    pub fn param_value(&self, node: u32, i: usize) -> Option<f32> {
        self.nodes.get(&node).map(|n| n.shared.param(i))
    }

    fn sort(&mut self) {
        for node in self.nodes.values_mut() {
            node.in_cycle = false;
            node.muted = false;
        }
        let cycles = self.order_by_inputs();
        let mut cut = false;
        for cycle in cycles {
            let delays: Vec<u32> = cycle.iter().copied().filter(|id| self.nodes[id].kind == Kind::Delay).collect();
            if delays.is_empty() {
                for id in &cycle {
                    if let Some(node) = self.nodes.get_mut(id) {
                        node.muted = true;
                    }
                }
            } else {
                for id in delays {
                    if let Some(node) = self.nodes.get_mut(&id) {
                        cut |= !node.in_cycle;
                        node.in_cycle = true;
                    }
                }
            }
        }
        if cut {
            for node in self.nodes.values_mut() {
                node.muted = false;
            }
            for cycle in self.order_by_inputs() {
                for id in &cycle {
                    if let Some(node) = self.nodes.get_mut(id) {
                        node.muted = true;
                    }
                }
            }
        }
        self.order_dirty = false;
    }

    fn order_by_inputs(&mut self) -> Vec<Vec<u32>> {
        let mut ids: Vec<u32> = self.nodes.keys().copied().collect();
        ids.sort_unstable();
        let mut state: HashMap<u32, u8> = HashMap::with_capacity(ids.len());
        let mut order = Vec::with_capacity(ids.len());
        let mut cycles = Vec::new();
        for root in ids {
            if state.contains_key(&root) {
                continue;
            }
            let mut stack: Vec<(u32, usize)> = vec![(root, 0)];
            state.insert(root, 1);
            while let Some(&mut (id, ref mut next)) = stack.last_mut() {
                let node = &self.nodes[&id];
                let listener = self.listener.filter(|_| node.kind == Kind::Panner);
                let inputs: &[u32] = if node.in_cycle { &[] } else { &node.inputs };
                let feeds = inputs.iter().chain(node.param_inputs.iter().flatten()).chain(listener.iter());
                if let Some(&input) = feeds.clone().nth(*next) {
                    *next += 1;
                    if !self.nodes.contains_key(&input) {
                        continue;
                    }
                    match state.get(&input) {
                        None => {
                            state.insert(input, 1);
                            stack.push((input, 0));
                        }
                        Some(1) => {
                            let from = stack.iter().position(|(n, _)| *n == input).unwrap_or(0);
                            cycles.push(stack[from..].iter().map(|(n, _)| *n).collect());
                        }
                        Some(_) => {}
                    }
                } else {
                    stack.pop();
                    state.insert(id, 2);
                    order.push(id);
                }
            }
        }
        self.order = order;
        cycles
    }

    pub fn render(&mut self, out: &mut Bus, ended: &mut Vec<u32>) {
        if self.order_dirty {
            self.sort();
        }
        let frame = self.frame;
        for k in 0..self.order.len() {
            let id = self.order[k];
            let Some(mut node) = self.nodes.remove(&id) else { continue };
            self.process(&mut node, frame);
            self.nodes.insert(id, node);
        }
        if let Some(dest) = self.nodes.get(&self.dest)
            && !dest.silent
        {
            let right = if dest.mono { 0 } else { 1 };
            for i in 0..QUANTUM {
                out[0][i] += dest.out[0][i];
                out[1][i] += dest.out[right][i];
            }
        }
        let cut: Vec<u32> = self.order.iter().copied().filter(|id| self.nodes.get(id).is_some_and(|n| n.in_cycle && !n.muted)).collect();
        for id in cut {
            let Some(mut node) = self.nodes.remove(&id) else { continue };
            let (bus, mono, empty) = self.mixed(&node.inputs);
            if let Effect::Delay(Some(d), last_mono) = &mut node.effect {
                d.write(&bus, mono);
                d.advance();
                *last_mono = mono;
                node.tail = if empty { node.tail.saturating_sub(QUANTUM as u64) } else { d.tail_frames(self.rate) };
            }
            self.nodes.insert(id, node);
        }
        self.frame += QUANTUM as u64;
        self.collect(ended);
    }

    fn mixed(&self, inputs: &[u32]) -> (Bus, bool, bool) {
        let mut bus = [[0.0; QUANTUM]; 2];
        let (mut mono, mut empty) = (true, true);
        for input in inputs {
            if let Some(from) = self.nodes.get(input)
                && !from.silent
            {
                up_mix(&mut bus, &mut mono, &mut empty, &from.out, from.mono);
            }
        }
        (bus, mono, empty)
    }

    fn effect(&mut self, node: &mut Node, empty: bool, mono: bool) {
        let rate = self.rate;
        let zero = [[0.0; QUANTUM]; 2];
        let quiet = empty && !node.in_cycle;
        if matches!(node.effect, Effect::Convolver(None) | Effect::Delay(None, _)) || (quiet && node.tail == 0) {
            node.silent = true;
            if let Effect::Compressor(c) = &node.effect {
                node.shared.reduction.store(c.reduction().to_bits(), Ordering::Relaxed);
            }
            return;
        }
        let (input, mono) = if quiet { (&zero, true) } else { (&self.input, mono) };
        let values = &self.values;
        let still = self.still[..node.params.len()].iter().all(|s| *s);
        let mut out = [[0.0; QUANTUM]; 2];
        let (out_mono, tail, decays) = match &mut node.effect {
            Effect::None => return,
            Effect::Biquad(b) => {
                b.process(input, mono, &values[..], still, rate, &mut out);
                (mono, u64::MAX, true)
            }
            Effect::Delay(None, _) | Effect::Convolver(None) => return,
            Effect::Delay(Some(d), last_mono) => {
                if node.in_cycle {
                    d.read(&values[0], QUANTUM as f64, rate, &mut out);
                    (*last_mono, node.tail, false)
                } else {
                    d.write(input, mono);
                    d.read(&values[0], 0.0, rate, &mut out);
                    d.advance();
                    (mono, d.tail_frames(rate), false)
                }
            }
            Effect::Shaper(s) => {
                s.process(input, mono, &mut out);
                (mono, s.tail_frames(), false)
            }
            Effect::Compressor(c) => {
                c.process(input, mono, [0, 1, 2, 3, 4].map(|p| values[p][0]), rate, &mut out);
                node.shared.reduction.store(c.reduction().to_bits(), Ordering::Relaxed);
                (false, (Compressor::TAIL_SECONDS * rate) as u64, false)
            }
            Effect::Convolver(Some(c)) => {
                let out_mono = c.process(input, mono, &mut out);
                (out_mono, c.tail_frames(), false)
            }
        };
        if !node.in_cycle {
            node.tail = if quiet { node.tail.saturating_sub(QUANTUM as u64) } else { tail };
        }
        if quiet && decays && out.iter().flatten().all(|v| v.abs() < QUIET) {
            node.tail = 0;
        }
        node.out = out;
        node.mono = out_mono;
        node.silent = false;
    }

    pub fn skip(&mut self, frames: u64) {
        self.frame += frames;
    }

    fn collect(&mut self, ended: &mut Vec<u32>) {
        let mut gone = Vec::new();
        for (&id, node) in &self.nodes {
            if id == self.dest {
                continue;
            }
            let fed = !node.inputs.is_empty() || node.param_inputs.iter().any(|l| !l.is_empty());
            let done =
                if node.kind.is_source() { node.src.ended || (node.released && node.src.start.is_none()) } else { node.released && !fed && node.tail == 0 };
            if done {
                gone.push(id);
                if node.kind.is_source() && node.src.ended {
                    ended.push(id);
                }
            }
        }
        if gone.is_empty() {
            return;
        }
        for id in &gone {
            self.nodes.remove(id);
        }
        self.gone.extend_from_slice(&gone);
        for node in self.nodes.values_mut() {
            node.inputs.retain(|n| !gone.contains(n));
            for list in &mut node.param_inputs {
                list.retain(|n| !gone.contains(n));
            }
        }
        self.order_dirty = true;
    }

    fn process(&mut self, node: &mut Node, frame: u64) {
        let mut empty = true;
        let mut mono = true;
        for input in &node.inputs {
            if let Some(from) = self.nodes.get(input)
                && !from.silent
            {
                up_mix(&mut self.input, &mut mono, &mut empty, &from.out, from.mono);
            }
        }
        for p in 0..node.params.len() {
            let values = &mut self.values[p];
            self.still[p] = node.params[p].fill(values, frame, self.rate) && node.param_inputs[p].is_empty();
            for input in &node.param_inputs[p] {
                if let Some(from) = self.nodes.get(input)
                    && !from.silent
                {
                    let right = if from.mono { 0 } else { 1 };
                    for (i, v) in values.iter_mut().enumerate() {
                        *v += (from.out[0][i] + from.out[right][i]) * 0.5;
                    }
                }
            }
            let (lo, hi) = (node.params[p].min, node.params[p].max);
            for v in values.iter_mut() {
                *v = v.clamp(lo, hi);
            }
            node.shared.params[p].store(values[QUANTUM - 1].to_bits(), Ordering::Relaxed);
        }
        if node.kind == Kind::Listener {
            self.listening.copy_from_slice(&self.values[..9]);
            self.listener_still = self.still[..9].iter().all(|s| *s);
        }
        if !matches!(node.effect, Effect::None) {
            if node.muted {
                node.silent = true;
            } else {
                self.effect(node, empty, mono);
            }
            return;
        }
        if node.kind == Kind::Analyser
            && let Some(tap) = &node.shared.tap
        {
            Tap::record(tap, &self.input, mono, empty || node.muted);
        }
        let rate = self.rate;
        let input = &self.input;
        let values = &self.values;
        let range = node.playing_range(frame);
        node.silent = match node.kind {
            Kind::Stream => node.stream.is_none(),
            kind if kind.is_source() => range.is_none(),
            _ => empty,
        } || node.muted;
        let (from, to) = range.unwrap_or_default();
        if node.silent
            && let Some(Panner { hrtf: Some(h), .. }) = node.panner.as_deref_mut()
        {
            h.reset();
        }
        match node.kind {
            _ if node.silent => {}
            Kind::Panner => {
                node.mono = false;
                let still = self.still[..6].iter().all(|s| *s) && self.listener_still;
                if let Some(p) = node.panner.as_deref_mut() {
                    pan(p, self.hrtf.as_deref(), input, mono, values, &self.listening, still, &mut node.out);
                }
            }
            Kind::Listener => {}
            Kind::Destination | Kind::Through | Kind::Analyser => {
                node.out = *input;
                node.mono = mono;
            }
            Kind::Biquad | Kind::Delay | Kind::WaveShaper | Kind::Compressor | Kind::Convolver => {}
            Kind::Stream => {
                node.mono = false;
                node.out = [[0.0; QUANTUM]; 2];
                if let Some(tap) = &mut node.stream {
                    tap.read(&mut node.out, 1.0);
                }
            }
            Kind::Gain => {
                node.mono = mono;
                for c in 0..if mono { 1 } else { 2 } {
                    for i in 0..QUANTUM {
                        node.out[c][i] = input[c][i] * values[0][i];
                    }
                }
            }
            Kind::StereoPanner => {
                node.mono = false;
                let half_pi = std::f32::consts::FRAC_PI_2;
                for i in 0..QUANTUM {
                    let pan = values[0][i];
                    if mono {
                        let x = (pan + 1.0) * 0.5;
                        node.out[0][i] = input[0][i] * (x * half_pi).cos();
                        node.out[1][i] = input[0][i] * (x * half_pi).sin();
                    } else if pan <= 0.0 {
                        let x = pan + 1.0;
                        node.out[0][i] = input[0][i] + input[1][i] * (x * half_pi).cos();
                        node.out[1][i] = input[1][i] * (x * half_pi).sin();
                    } else {
                        let x = pan;
                        node.out[0][i] = input[0][i] * (x * half_pi).cos();
                        node.out[1][i] = input[1][i] + input[0][i] * (x * half_pi).sin();
                    }
                }
            }
            Kind::Oscillator | Kind::ConstantSource => {
                node.mono = true;
                node.out[0][..from].fill(0.0);
                node.out[0][to..].fill(0.0);
                if node.kind == Kind::ConstantSource {
                    node.out[0][from..to].copy_from_slice(&values[0][from..to]);
                } else {
                    for i in from..to {
                        let detune = values[1][i];
                        let f = if detune == 0.0 { values[0][i] as f64 } else { values[0][i] as f64 * (detune as f64 / 1200.0).exp2() };
                        let dt = f / rate;
                        node.out[0][i] = oscillator(node.wave, node.phase, dt.abs()) as f32;
                        node.phase += dt;
                        node.phase -= node.phase.floor();
                    }
                }
            }
            Kind::BufferSource => {
                if play_buffer(node, from, to, rate, &values[..]) {
                    node.src.ended = true;
                }
            }
        }
        if node.kind.is_source()
            && let (Some(_), Some(stop)) = (node.src.start, node.src.stop)
            && stop <= frame + QUANTUM as u64
        {
            node.src.ended = true;
        }
    }
}

fn listener_at(values: &[[f32; QUANTUM]; 9], i: usize) -> Listener {
    let v = |k: usize| f64::from(values[k][i]);
    Listener { position: V3::new(v(0), v(1), v(2)), forward: V3::new(v(3), v(4), v(5)), up: V3::new(v(6), v(7), v(8)) }
}

#[allow(clippy::too_many_arguments)]
fn pan(
    p: &mut Panner,
    model: Option<&HrtfModel>,
    input: &Bus,
    mono: bool,
    values: &[[f32; QUANTUM]; MAX_PARAMS],
    listening: &[[f32; QUANTUM]; 9],
    still: bool,
    out: &mut Bus,
) {
    let source = |i: usize| V3::new(f64::from(values[0][i]), f64::from(values[1][i]), f64::from(values[2][i]));
    let orientation = |i: usize| V3::new(f64::from(values[3][i]), f64::from(values[4][i]), f64::from(values[5][i]));
    if p.config.hrtf
        && let Some(model) = model
    {
        let spot = spatial::locate(&p.config, source(0), orientation(0), &listener_at(listening, 0));
        let h = p.hrtf.get_or_insert_with(|| Hrtf::new(model));
        h.process(model, &spot, input, mono, out);
        return;
    }
    p.hrtf = None;
    let mut spot = spatial::locate(&p.config, source(0), orientation(0), &listener_at(listening, 0));
    for i in 0..QUANTUM {
        if !still && i > 0 {
            spot = spatial::locate(&p.config, source(i), orientation(i), &listener_at(listening, i));
        }
        let (gl, gr, left) = spatial::equal_power(spot.azimuth, mono);
        let (gl, gr, g) = (gl as f32, gr as f32, spot.gain as f32);
        let (l, r) = if mono {
            (input[0][i] * gl, input[0][i] * gr)
        } else if left {
            (input[0][i] + input[1][i] * gl, input[1][i] * gr)
        } else {
            (input[0][i] * gl, input[1][i] + input[0][i] * gr)
        };
        out[0][i] = l * g;
        out[1][i] = r * g;
    }
}

fn oscillator(wave: Wave, phase: f64, dt: f64) -> f64 {
    match wave {
        Wave::Sine => (std::f64::consts::TAU * phase).sin(),
        Wave::Square => {
            let naive = if phase < 0.5 { 1.0 } else { -1.0 };
            naive + poly_blep(phase, dt) - poly_blep((phase + 0.5).fract(), dt)
        }
        Wave::Sawtooth => {
            let p = (phase + 0.5).fract();
            2.0 * p - 1.0 - poly_blep(p, dt)
        }
        Wave::Triangle => 1.0 - 4.0 * ((phase + 0.25).fract() - 0.5).abs(),
    }
}

fn poly_blep(t: f64, dt: f64) -> f64 {
    if dt <= 0.0 {
        0.0
    } else if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

pub fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

fn play_buffer(node: &mut Node, from: usize, to: usize, rate: f64, values: &[[f32; QUANTUM]]) -> bool {
    let Some(data) = node.buf.data.clone() else {
        node.out[0].fill(0.0);
        node.mono = true;
        return false;
    };
    let len = data.len();
    let channels = data.channels.len().min(2);
    node.mono = channels < 2;
    for c in &mut node.out {
        c[..from].fill(0.0);
        c[to..].fill(0.0);
    }
    if len == 0 || channels == 0 {
        return true;
    }
    let ratio = data.rate / rate;
    let (loop_start, loop_end) = {
        let (s, e) = (node.buf.loop_start * data.rate, node.buf.loop_end * data.rate);
        let e = if e > 0.0 && e > s { e.min(len as f64) } else { len as f64 };
        let s = if s > 0.0 && s < e { s } else { 0.0 };
        (s, e)
    };
    let mut pos = node.buf.pos.unwrap_or_else(|| (node.buf.offset * data.rate).min(len as f64));
    let limit = node.buf.duration.map(|d| d * data.rate);
    let sample = |c: usize, i: isize, looping: bool| -> f32 {
        let ch = &data.channels[c];
        let i = if looping && loop_end > loop_start {
            let (s, e) = (loop_start as isize, (loop_end.ceil() as isize).max(loop_start as isize + 1));
            if i >= e { s + (i - s) % (e - s) } else { i }
        } else {
            i
        };
        if i < 0 || i as usize >= len { 0.0 } else { ch[i as usize] }
    };
    for i in from..to {
        if !node.buf.looping && (pos >= len as f64 || pos < 0.0) {
            return true;
        }
        if limit.is_some_and(|l| node.buf.played >= l) {
            return true;
        }
        let base = pos.floor();
        let t = (pos - base) as f32;
        let k = base as isize;
        let looping = node.buf.looping;
        for c in 0..channels {
            node.out[c][i] = hermite(sample(c, k - 1, looping), sample(c, k, looping), sample(c, k + 1, looping), sample(c, k + 2, looping), t);
        }
        let detune = values[1][i];
        let speed = values[0][i] as f64 * if detune == 0.0 { 1.0 } else { (detune as f64 / 1200.0).exp2() };
        let step = speed * ratio;
        pos += step;
        node.buf.played += step.abs();
        if node.buf.looping && loop_end > loop_start {
            let span = loop_end - loop_start;
            if pos >= loop_end {
                pos = loop_start + (pos - loop_end) % span;
            } else if pos < loop_start && step < 0.0 {
                pos = loop_end - (loop_start - pos) % span;
            }
        }
    }
    node.buf.pos = Some(pos);
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::audio::budget::Budget;
    use crate::host::audio::spatial::DistanceModel;

    const RATE: f64 = 48000.0;

    fn render(graph: &mut Graph, quanta: usize) -> Vec<f32> {
        let mut left = Vec::new();
        let mut ended = Vec::new();
        for _ in 0..quanta {
            let mut out = [[0.0; QUANTUM]; 2];
            graph.render(&mut out, &mut ended);
            left.extend_from_slice(&out[0]);
        }
        left
    }

    fn frequency(samples: &[f32], rate: f64) -> f64 {
        let ups: Vec<usize> = samples.windows(2).enumerate().filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0).map(|(i, _)| i).collect();
        let (Some(first), Some(last)) = (ups.first(), ups.last()) else { return 0.0 };
        (ups.len() - 1) as f64 * rate / (last - first) as f64
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    #[test]
    fn oscillator_through_gain_has_its_frequency_and_level() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::Oscillator, Arc::new(Shared::new(Kind::Oscillator)));
        g.add(3, Kind::Gain, Arc::new(Shared::new(Kind::Gain)));
        g.connect(2, 3, None);
        g.connect(3, 1, None);
        g.node(3, NodeOp::Param(0, ParamOp::Value(0.5)));
        g.node(2, NodeOp::Param(0, ParamOp::Value(1000.0)));
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        let out = render(&mut g, 375);
        let f = frequency(&out, RATE);
        assert!((f - 1000.0).abs() <= 2.0, "{f}");
        let p = peak(&out);
        assert!((p - 0.5).abs() < 0.01, "{p}");
    }

    #[test]
    fn waveforms_start_as_web_audio_does() {
        assert!(oscillator(Wave::Sine, 0.0, 0.0).abs() < 1e-9);
        assert!((oscillator(Wave::Sine, 0.25, 0.0) - 1.0).abs() < 1e-9);
        assert_eq!(oscillator(Wave::Square, 0.25, 0.0), 1.0);
        assert_eq!(oscillator(Wave::Square, 0.75, 0.0), -1.0);
        assert!(oscillator(Wave::Sawtooth, 0.0, 0.0).abs() < 1e-9);
        assert!((oscillator(Wave::Sawtooth, 0.25, 0.0) - 0.5).abs() < 1e-9);
        assert!(oscillator(Wave::Triangle, 0.0, 0.0).abs() < 1e-9);
        assert!((oscillator(Wave::Triangle, 0.25, 0.0) - 1.0).abs() < 1e-9);
        assert!((oscillator(Wave::Triangle, 0.75, 0.0) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn square_and_sawtooth_keep_their_frequency() {
        for wave in [Wave::Square, Wave::Sawtooth, Wave::Triangle] {
            let mut g = Graph::new(RATE, 1);
            g.add(2, Kind::Oscillator, Arc::new(Shared::new(Kind::Oscillator)));
            g.node(2, NodeOp::Wave(wave));
            g.node(2, NodeOp::Param(0, ParamOp::Value(441.0)));
            g.connect(2, 1, None);
            g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
            let out = render(&mut g, 375);
            let f = frequency(&out, RATE);
            assert!((f - 441.0).abs() <= 2.0, "{wave:?} {f}");
        }
    }

    #[test]
    fn start_and_stop_are_sample_accurate() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::ConstantSource, Arc::new(Shared::new(Kind::ConstantSource)));
        g.connect(2, 1, None);
        g.node(2, NodeOp::Start { when: 100.0 / RATE, offset: 0.0, duration: None });
        g.node(2, NodeOp::Stop(300.0 / RATE));
        let mut ended = Vec::new();
        let mut all = Vec::new();
        for _ in 0..4 {
            let mut out = [[0.0; QUANTUM]; 2];
            g.render(&mut out, &mut ended);
            all.extend_from_slice(&out[0]);
        }
        assert_eq!(all[99], 0.0);
        assert_eq!(all[100], 1.0);
        assert_eq!(all[299], 1.0);
        assert_eq!(all[300], 0.0);
        assert_eq!(ended, vec![2]);
        assert!(!g.has(2), "an ended source goes");
    }

    #[test]
    fn linear_ramp() {
        let mut p = Param::new(0.0, f32::MIN, f32::MAX);
        p.apply(ParamOp::SetAt(0.0, 0.0), 0.0);
        p.apply(ParamOp::Linear(1.0, 1.0), 0.0);
        assert!((p.at(0.25) - 0.25).abs() < 1e-6);
        assert!((p.at(0.5) - 0.5).abs() < 1e-6);
        assert_eq!(p.at(2.0), 1.0);
    }

    #[test]
    fn exponential_ramp() {
        let mut p = Param::new(1.0, f32::MIN, f32::MAX);
        p.apply(ParamOp::SetAt(1.0, 0.0), 0.0);
        p.apply(ParamOp::Exponential(0.01, 2.0), 0.0);
        assert!((p.at(1.0) - 0.1).abs() < 1e-5, "{}", p.at(1.0));
        assert!((p.at(2.0) - 0.01).abs() < 1e-7);
        assert!((p.at(5.0) - 0.01).abs() < 1e-7);
    }

    #[test]
    fn exponential_ramp_from_zero_holds() {
        let mut p = Param::new(0.0, f32::MIN, f32::MAX);
        p.apply(ParamOp::SetAt(0.0, 0.0), 0.0);
        p.apply(ParamOp::Exponential(1.0, 1.0), 0.0);
        assert_eq!(p.at(0.5), 0.0);
        assert_eq!(p.at(1.0), 1.0);
    }

    #[test]
    fn ramp_without_an_event_before_starts_now() {
        let mut p = Param::new(2.0, f32::MIN, f32::MAX);
        p.apply(ParamOp::Linear(4.0, 3.0), 1.0);
        assert_eq!(p.at(0.5), 2.0);
        assert!((p.at(2.0) - 3.0).abs() < 1e-6);
    }

    #[test]
    fn set_target_approaches() {
        let mut p = Param::new(1.0, f32::MIN, f32::MAX);
        p.apply(ParamOp::Target(0.0, 1.0, 0.5), 0.0);
        assert_eq!(p.at(0.5), 1.0);
        let one_tc = p.at(1.5);
        assert!((one_tc - (-1.0f32).exp()).abs() < 1e-5, "{one_tc}");
    }

    #[test]
    fn value_curve_interpolates() {
        let mut p = Param::new(0.0, f32::MIN, f32::MAX);
        p.apply(ParamOp::Curve(vec![0.0, 1.0, 0.0].into(), 1.0, 2.0), 0.0);
        assert_eq!(p.at(0.5), 0.0);
        assert!((p.at(1.5) - 0.5).abs() < 1e-6);
        assert!((p.at(2.0) - 1.0).abs() < 1e-6);
        assert_eq!(p.at(3.5), 0.0);
    }

    #[test]
    fn cancel_and_hold_keeps_the_ramp_value() {
        let mut p = Param::new(0.0, f32::MIN, f32::MAX);
        p.apply(ParamOp::SetAt(0.0, 0.0), 0.0);
        p.apply(ParamOp::Linear(1.0, 2.0), 0.0);
        p.apply(ParamOp::CancelAndHold(1.0), 0.0);
        assert!((p.at(0.5) - 0.25).abs() < 1e-6);
        assert!((p.at(1.0) - 0.5).abs() < 1e-6);
        assert!((p.at(3.0) - 0.5).abs() < 1e-6);
        p.apply(ParamOp::Cancel(0.0), 0.0);
        assert_eq!(p.at(3.0), 0.0);
    }

    #[test]
    fn pruning_keeps_the_values() {
        let mut p = Param::new(1.0, f32::MIN, f32::MAX);
        p.apply(ParamOp::SetAt(1.0, 0.0), 0.0);
        p.apply(ParamOp::Linear(0.0, 1.0), 0.0);
        p.apply(ParamOp::Target(1.0, 2.0, 0.1), 0.0);
        let mut out = [0.0; QUANTUM];
        let before = p.at(2.05);
        p.fill(&mut out, (1.5 * RATE) as u64, RATE);
        assert!((p.at(2.05) - before).abs() < 1e-6);
        p.fill(&mut out, (2.01 * RATE) as u64, RATE);
        assert!((p.at(2.05) - before).abs() < 1e-6);
        p.fill(&mut out, (20.0 * RATE) as u64, RATE);
        assert!(p.events.is_empty());
        assert_eq!(out[0], 1.0);
    }

    #[test]
    fn gain_envelope_shapes_the_output() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::Oscillator, Arc::new(Shared::new(Kind::Oscillator)));
        g.add(3, Kind::Gain, Arc::new(Shared::new(Kind::Gain)));
        g.node(2, NodeOp::Param(0, ParamOp::Value(880.0)));
        g.node(3, NodeOp::Param(0, ParamOp::SetAt(0.0, 0.0)));
        g.node(3, NodeOp::Param(0, ParamOp::Linear(1.0, 0.01)));
        g.node(3, NodeOp::Param(0, ParamOp::Exponential(0.001, 0.2)));
        g.connect(2, 3, None);
        g.connect(3, 1, None);
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        g.node(2, NodeOp::Stop(0.25));
        let out = render(&mut g, 120);
        let window = |t: f64| peak(&out[(t * RATE) as usize..((t + 0.005) * RATE) as usize]);
        assert!(window(0.0) < 0.6, "attack {}", window(0.0));
        assert!(window(0.009) > 0.9, "peak {}", window(0.009));
        let mid = window(0.105);
        assert!((mid - 0.0316).abs() < 0.01, "decay {mid}");
        assert_eq!(peak(&out[(0.25 * RATE) as usize + 1..]), 0.0);
        let f = frequency(&out[..(0.2 * RATE) as usize], RATE);
        assert!((f - 880.0).abs() < 10.0, "{f}");
        assert!(!g.has(2));
    }

    #[test]
    fn buffer_source_plays_resamples_and_loops() {
        let rate = 24000.0;
        let samples: Vec<f32> = (0..2400).map(|i| (std::f64::consts::TAU * 100.0 * i as f64 / rate).sin() as f32).collect();
        let data = Arc::new(AudioData { rate, channels: vec![samples] });
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::BufferSource, Arc::new(Shared::new(Kind::BufferSource)));
        g.node(2, NodeOp::Buffer(Some(data.clone())));
        g.connect(2, 1, None);
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        let mut ended = Vec::new();
        let mut all = Vec::new();
        for _ in 0..60 {
            let mut out = [[0.0; QUANTUM]; 2];
            g.render(&mut out, &mut ended);
            all.extend_from_slice(&out[0]);
        }
        assert_eq!(ended, vec![2]);
        let played = all.iter().rposition(|v| v.abs() > 1e-3).unwrap();
        assert!((played as f64 - 4800.0).abs() < 130.0, "{played}");
        let f = frequency(&all[..4700], RATE);
        assert!((f - 100.0).abs() < 1.0, "{f}");

        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::BufferSource, Arc::new(Shared::new(Kind::BufferSource)));
        g.node(2, NodeOp::Buffer(Some(data)));
        g.node(2, NodeOp::Loop { looping: true, start: 0.0, end: 0.0 });
        g.node(2, NodeOp::Param(0, ParamOp::Value(2.0)));
        g.connect(2, 1, None);
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        let out = render(&mut g, 375);
        let f = frequency(&out, RATE);
        assert!((f - 200.0).abs() < 3.0, "{f}");
        assert!(g.has(2));
    }

    #[test]
    fn stereo_panner_pans_mono_with_equal_power() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::ConstantSource, Arc::new(Shared::new(Kind::ConstantSource)));
        g.add(3, Kind::StereoPanner, Arc::new(Shared::new(Kind::StereoPanner)));
        g.connect(2, 3, None);
        g.connect(3, 1, None);
        g.node(3, NodeOp::Param(0, ParamOp::Value(1.0)));
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        let mut out = [[0.0; QUANTUM]; 2];
        g.render(&mut out, &mut Vec::new());
        assert!(out[0][5].abs() < 1e-6);
        assert!((out[1][5] - 1.0).abs() < 1e-6);
        g.node(3, NodeOp::Param(0, ParamOp::Value(0.0)));
        let mut out = [[0.0; QUANTUM]; 2];
        g.render(&mut out, &mut Vec::new());
        assert!((out[0][5] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn a_node_into_a_param_modulates_it() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::ConstantSource, Arc::new(Shared::new(Kind::ConstantSource)));
        g.add(3, Kind::ConstantSource, Arc::new(Shared::new(Kind::ConstantSource)));
        g.add(4, Kind::Gain, Arc::new(Shared::new(Kind::Gain)));
        g.node(3, NodeOp::Param(0, ParamOp::Value(0.25)));
        g.node(4, NodeOp::Param(0, ParamOp::Value(0.5)));
        g.connect(2, 4, None);
        g.connect(3, 4, Some(0));
        g.connect(4, 1, None);
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        g.node(3, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        let mut out = [[0.0; QUANTUM]; 2];
        g.render(&mut out, &mut Vec::new());
        assert!((out[0][0] - 0.75).abs() < 1e-6);
        assert!((g.param_value(4, 0).unwrap() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn released_nodes_go_once_nothing_feeds_them() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::Oscillator, Arc::new(Shared::new(Kind::Oscillator)));
        g.add(3, Kind::Gain, Arc::new(Shared::new(Kind::Gain)));
        g.connect(2, 3, None);
        g.connect(3, 1, None);
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        g.node(2, NodeOp::Stop(0.01));
        g.node(3, NodeOp::Release);
        render(&mut g, 2);
        assert!(g.has(3), "still fed");
        render(&mut g, 4);
        assert!(!g.has(2) && !g.has(3));
        assert!(!g.active());
    }

    struct Case {
        name: &'static str,
        stereo: bool,
        pos: [f32; 3],
        orient: [f32; 3],
        listener: Option<[[f32; 3]; 3]>,
        config: PannerConfig,
        ramp: bool,
        left: &'static [f32],
        right: &'static [f32],
    }

    fn equal_power_case(c: &Case) -> (Vec<f32>, Vec<f32>) {
        let mut g = Graph::new(RATE, 1);
        g.add(5, Kind::Listener, Arc::new(Shared::new(Kind::Listener)));
        if let Some(l) = c.listener {
            for (i, v) in l.iter().flatten().enumerate() {
                g.node(5, NodeOp::Param(i, ParamOp::Value(*v)));
            }
        }
        if c.stereo {
            let data = Arc::new(AudioData { rate: RATE, channels: vec![vec![1.0; 4096], vec![0.5; 4096]] });
            g.add(2, Kind::BufferSource, Arc::new(Shared::new(Kind::BufferSource)));
            g.node(2, NodeOp::Buffer(Some(data)));
        } else {
            g.add(2, Kind::ConstantSource, Arc::new(Shared::new(Kind::ConstantSource)));
        }
        g.add(3, Kind::Panner, Arc::new(Shared::new(Kind::Panner)));
        g.node(3, NodeOp::Panner(c.config));
        for (i, v) in c.pos.iter().chain(&c.orient).enumerate() {
            g.node(3, NodeOp::Param(i, ParamOp::Value(*v)));
        }
        if c.ramp {
            g.node(3, NodeOp::Param(0, ParamOp::SetAt(-2.0, 0.0)));
            g.node(3, NodeOp::Param(0, ParamOp::Linear(2.0, 1024.0 / RATE)));
        }
        g.connect(2, 3, None);
        g.connect(3, 1, None);
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for _ in 0..8 {
            let mut out = [[0.0; QUANTUM]; 2];
            g.render(&mut out, &mut Vec::new());
            left.extend_from_slice(&out[0]);
            right.extend_from_slice(&out[1]);
        }
        (left, right)
    }

    #[test]
    fn panner_matches_chrome_for_equal_power_distance_and_cones() {
        let cases = [
            Case {
                name: "front",
                stereo: false,
                pos: [0.0, 0.0, -1.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.7071067690849304],
                right: &[0.7071067690849304],
            },
            Case {
                name: "right",
                stereo: false,
                pos: [1.0, 0.0, 0.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[6.123234262925839e-17],
                right: &[1.0],
            },
            Case {
                name: "left30",
                stereo: false,
                pos: [-0.5, 0.0, -0.866],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.8660285472869873],
                right: &[0.49999457597732544],
            },
            Case {
                name: "behind_right",
                stereo: false,
                pos: [1.0, 0.0, 1.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.27059805393218994],
                right: &[0.6532814502716064],
            },
            Case {
                name: "above",
                stereo: false,
                pos: [0.0, 1.0, 0.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.7071067690849304],
                right: &[0.7071067690849304],
            },
            Case {
                name: "elevated",
                stereo: false,
                pos: [1.0, 2.0, -1.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.15622985363006592],
                right: &[0.37717220187187195],
            },
            Case {
                name: "same_point",
                stereo: false,
                pos: [0.0, 0.0, 0.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.7071067690849304],
                right: &[0.7071067690849304],
            },
            Case {
                name: "stereo_left",
                stereo: true,
                pos: [-1.0, 0.0, -1.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.9571067690849304],
                right: &[0.2499999850988388],
            },
            Case {
                name: "stereo_right",
                stereo: true,
                pos: [2.0, 0.0, -1.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.20000000298023224],
                right: &[0.62360680103302],
            },
            Case {
                name: "inverse",
                stereo: false,
                pos: [0.0, 0.0, -10.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ref_distance: 2.0, rolloff: 1.5, ..PannerConfig::default() },
                ramp: false,
                left: &[0.10101525485515594],
                right: &[0.10101525485515594],
            },
            Case {
                name: "linear",
                stereo: false,
                pos: [3.0, 0.0, -4.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { distance: DistanceModel::Linear, ref_distance: 1.0, max_distance: 10.0, rolloff: 0.5, ..PannerConfig::default() },
                ramp: false,
                left: &[0.3478327989578247],
                right: &[0.6956655979156494],
            },
            Case {
                name: "linear_clamp_rolloff",
                stereo: false,
                pos: [3.0, 0.0, -4.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { distance: DistanceModel::Linear, ref_distance: 1.0, max_distance: 10.0, rolloff: 2.0, ..PannerConfig::default() },
                ramp: false,
                left: &[0.24845200777053833],
                right: &[0.49690401554107666],
            },
            Case {
                name: "linear_ref_gt_max",
                stereo: false,
                pos: [0.0, 0.0, -4.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { distance: DistanceModel::Linear, ref_distance: 8.0, max_distance: 2.0, rolloff: 0.5, ..PannerConfig::default() },
                ramp: false,
                left: &[0.5892556309700012],
                right: &[0.5892556309700012],
            },
            Case {
                name: "exponential",
                stereo: false,
                pos: [0.0, 0.0, -8.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { distance: DistanceModel::Exponential, ref_distance: 2.0, rolloff: 0.7, ..PannerConfig::default() },
                ramp: false,
                left: &[0.2679433524608612],
                right: &[0.2679433524608612],
            },
            Case {
                name: "inverse_ref0",
                stereo: false,
                pos: [0.0, 0.0, -8.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ref_distance: 0.0, ..PannerConfig::default() },
                ramp: false,
                left: &[0.0],
                right: &[0.0],
            },
            Case {
                name: "cone_facing",
                stereo: false,
                pos: [0.0, 0.0, -2.0],
                orient: [0.0, 0.0, 1.0],
                listener: None,
                config: PannerConfig { cone_inner: 60.0, cone_outer: 120.0, cone_outer_gain: 0.1, ..PannerConfig::default() },
                ramp: false,
                left: &[0.3535533845424652],
                right: &[0.3535533845424652],
            },
            Case {
                name: "cone_away",
                stereo: false,
                pos: [0.0, 0.0, -2.0],
                orient: [0.0, 0.0, -1.0],
                listener: None,
                config: PannerConfig { cone_inner: 60.0, cone_outer: 120.0, cone_outer_gain: 0.1, ..PannerConfig::default() },
                ramp: false,
                left: &[0.0353553406894207],
                right: &[0.0353553406894207],
            },
            Case {
                name: "cone_between",
                stereo: false,
                pos: [0.0, 0.0, -2.0],
                orient: [0.6, 0.0, -0.8],
                listener: None,
                config: PannerConfig { cone_inner: 60.0, cone_outer: 120.0, cone_outer_gain: 0.1, ..PannerConfig::default() },
                ramp: false,
                left: &[0.0353553406894207],
                right: &[0.0353553406894207],
            },
            Case {
                name: "listener_turned",
                stereo: false,
                pos: [0.0, 0.0, -3.0],
                orient: [1.0, 0.0, 0.0],
                listener: Some([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.3333333432674408],
                right: &[4.4386020192632714e-08],
            },
            Case {
                name: "listener_moved",
                stereo: false,
                pos: [5.0, 0.0, -1.0],
                orient: [1.0, 0.0, 0.0],
                listener: Some([[4.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]),
                config: PannerConfig { ..PannerConfig::default() },
                ramp: false,
                left: &[0.27059805393218994],
                right: &[0.6532814502716064],
            },
            Case {
                name: "ramp",
                stereo: false,
                pos: [-2.0, 0.0, -1.0],
                orient: [1.0, 0.0, 0.0],
                listener: None,
                config: PannerConfig { ..PannerConfig::default() },
                ramp: true,
                left: &[0.4352501928806305, 0.6532814502716064, 0.7071067690849304, 0.27059805393218994, 0.10307992994785309],
                right: &[0.10274861752986908, 0.27059805393218994, 0.7071067690849304, 0.6532814502716064, 0.43589091300964355],
            },
        ];
        for c in &cases {
            let (left, right) = equal_power_case(c);
            let at: &[usize] = if c.ramp { &[0, 256, 512, 768, 1023] } else { &[500] };
            for (k, &i) in at.iter().enumerate() {
                assert!(
                    (left[i] - c.left[k]).abs() < 2e-5 && (right[i] - c.right[k]).abs() < 2e-5,
                    "{} @{i}: {} {} vs {} {}",
                    c.name,
                    left[i],
                    right[i],
                    c.left[k],
                    c.right[k]
                );
            }
        }
    }

    #[test]
    fn a_panner_follows_the_listener_it_depends_on() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::ConstantSource, Arc::new(Shared::new(Kind::ConstantSource)));
        g.add(3, Kind::Panner, Arc::new(Shared::new(Kind::Panner)));
        g.add(9, Kind::Listener, Arc::new(Shared::new(Kind::Listener)));
        g.node(3, NodeOp::Param(0, ParamOp::Value(1.0)));
        g.node(9, NodeOp::Param(0, ParamOp::Value(2.0)));
        g.connect(2, 3, None);
        g.connect(3, 1, None);
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        let mut out = [[0.0; QUANTUM]; 2];
        g.render(&mut out, &mut Vec::new());
        assert!((out[0][0] - 1.0).abs() < 1e-6 && out[1][0].abs() < 1e-6, "the source is left of a listener at x = 2: {} {}", out[0][0], out[1][0]);
    }

    #[test]
    fn hrtf_panner_spatializes_and_stops_cleanly() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::ConstantSource, Arc::new(Shared::new(Kind::ConstantSource)));
        g.add(3, Kind::Panner, Arc::new(Shared::new(Kind::Panner)));
        g.node(3, NodeOp::Panner(PannerConfig { hrtf: true, ..PannerConfig::default() }));
        g.node(3, NodeOp::Param(0, ParamOp::Value(1.0)));
        g.connect(2, 3, None);
        g.connect(3, 1, None);
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        g.node(2, NodeOp::Stop(4.0 * QUANTUM as f64 / RATE));
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for _ in 0..8 {
            let mut out = [[0.0; QUANTUM]; 2];
            g.render(&mut out, &mut Vec::new());
            left.extend_from_slice(&out[0]);
            right.extend_from_slice(&out[1]);
        }
        assert!(left.iter().chain(&right).all(|v| v.is_finite()));
        assert!(right[300] > left[300] && left[300] > 0.0, "{} {}", left[300], right[300]);
        assert!(left[6 * QUANTUM..].iter().chain(&right[6 * QUANTUM..]).all(|v| *v == 0.0));
    }

    #[test]
    fn a_cycle_does_not_hang() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::Gain, Arc::new(Shared::new(Kind::Gain)));
        g.add(3, Kind::Gain, Arc::new(Shared::new(Kind::Gain)));
        g.add(4, Kind::ConstantSource, Arc::new(Shared::new(Kind::ConstantSource)));
        g.connect(4, 2, None);
        g.connect(2, 3, None);
        g.connect(3, 2, None);
        g.connect(3, 1, None);
        g.node(3, NodeOp::Param(0, ParamOp::Value(0.5)));
        g.node(4, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        render(&mut g, 10);
    }

    fn impulse_source(g: &mut Graph, id: u32) {
        g.add(id, Kind::BufferSource, Arc::new(Shared::new(Kind::BufferSource)));
        let mut one = vec![0.0f32; 1];
        one[0] = 1.0;
        g.node(id, NodeOp::Buffer(Some(Arc::new(AudioData { rate: RATE, channels: vec![one] }))));
        g.node(id, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
    }

    #[test]
    fn a_delay_rings_on_after_its_source_ended() {
        let mut g = Graph::new(RATE, 1);
        impulse_source(&mut g, 2);
        g.add(3, Kind::Delay, Arc::new(Shared::new(Kind::Delay)));
        g.node(3, NodeOp::Delay(Box::new(Delay::new(1.0, RATE as f64, &Arc::new(Budget::default())).unwrap())));
        g.node(3, NodeOp::Param(0, ParamOp::Value(0.015625)));
        g.connect(2, 3, None);
        g.connect(3, 1, None);
        g.node(3, NodeOp::Release);
        let out = render(&mut g, 16);
        assert_eq!(out[750], 1.0, "the impulse comes out 2^-6 s = 750 frames later");
        assert_eq!(out.iter().filter(|v| **v != 0.0).count(), 1);
        assert!(g.has(3), "a released delay stays while its tail rings");
        render(&mut g, 400);
        assert!(!g.has(3), "and goes once the tail has passed");
    }

    #[test]
    fn a_feedback_loop_through_a_delay_echoes_every_delay_time() {
        let mut g = Graph::new(RATE, 1);
        impulse_source(&mut g, 2);
        g.add(3, Kind::Delay, Arc::new(Shared::new(Kind::Delay)));
        g.node(3, NodeOp::Delay(Box::new(Delay::new(1.0, RATE as f64, &Arc::new(Budget::default())).unwrap())));
        g.node(3, NodeOp::Param(0, ParamOp::Value(0.0078125)));
        g.add(4, Kind::Gain, Arc::new(Shared::new(Kind::Gain)));
        g.node(4, NodeOp::Param(0, ParamOp::Value(0.5)));
        g.connect(2, 4, None);
        g.connect(4, 3, None);
        g.connect(3, 4, None);
        g.connect(4, 1, None);
        let out = render(&mut g, 16);
        let echoes: Vec<(usize, f32)> = out.iter().enumerate().filter(|(_, v)| v.abs() > 1e-6).map(|(i, v)| (i, *v)).take(4).collect();
        assert_eq!(echoes, [(0, 0.5), (375, 0.25), (750, 0.125), (1125, 0.0625)], "2^-7 s is 375 frames, not 375 + 128");
    }

    #[test]
    fn a_loop_without_a_delay_is_silent() {
        let mut g = Graph::new(RATE, 1);
        g.add(2, Kind::Oscillator, Arc::new(Shared::new(Kind::Oscillator)));
        g.node(2, NodeOp::Start { when: 0.0, offset: 0.0, duration: None });
        g.add(3, Kind::Gain, Arc::new(Shared::new(Kind::Gain)));
        g.add(4, Kind::Gain, Arc::new(Shared::new(Kind::Gain)));
        g.connect(2, 3, None);
        g.connect(3, 4, None);
        g.connect(4, 3, None);
        g.connect(4, 1, None);
        assert!(render(&mut g, 4).iter().all(|v| *v == 0.0));
        g.disconnect(4, Some(3), None);
        assert!(peak(&render(&mut g, 4)) > 0.9, "breaking the loop lets the tone through");
    }

    #[test]
    fn a_filter_decays_after_its_input_stops_and_then_rests() {
        let mut g = Graph::new(RATE, 1);
        impulse_source(&mut g, 2);
        g.add(3, Kind::Biquad, Arc::new(Shared::new(Kind::Biquad)));
        g.node(3, NodeOp::Param(0, ParamOp::Value(200.0)));
        g.node(3, NodeOp::Param(2, ParamOp::Value(20.0)));
        g.connect(2, 3, None);
        g.connect(3, 1, None);
        let out = render(&mut g, 64);
        assert!(out[QUANTUM * 20..].iter().any(|v| v.abs() > 1e-4), "a resonant low-pass rings well past the impulse");
        assert!(g.active(), "the ringing keeps the graph active");
        render(&mut g, 2000);
        assert!(!g.active(), "until it has decayed");
    }
}
