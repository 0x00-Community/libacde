mod canvas;
mod capture;
mod caret;
mod commands;
mod damage;
mod decoration;
mod dom;
mod engine;
mod error;
mod geometry;
pub mod gpu;
mod hud;
mod layers;
mod net;
mod parse;
mod region;
mod serialize;
mod surface;
mod tree;
mod verify;
mod xml;

pub use blitz_dom;
pub use blitz_paint;
pub use canvas::encode_png;
pub use capture::Capture;
pub use commands::Commands;
pub use decoration::{Bar, ButtonSide, Decoration, FrameButton, FrameColors, FrameStyle, FrameTheme, MenuList, MenuPlace, MenuRow, Rgba, TitleAlign};
pub use dom::{AnimationEvent, AnimationEvents, Dom, NONE, NodeType, Ns, Quad, Viewport};
pub use error::{Error, Result};
pub use geometry::Metrics;
pub use hud::{Hud, HudStats};
pub use layers::{Backdrop, Drawing, Layer, LayerFilter};
pub(crate) use net::resource_file;
pub use net::{Load, Loader};
pub use surface::{ClipboardEntry, ClipboardReadReply, ClipboardTypesReply, DragImage, SurfaceCommand, WindowLook, WindowShadow};

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use blitz_dom::BoundingRect;
use engine::Engine;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub base_url: Option<String>,
    pub resources: Option<Vec<Resource>>,
    pub default_lang: Option<String>,
    pub verify_paint: Option<bool>,
    pub svg_text_path: Option<bool>,
    pub dark: bool,
    pub name: Option<String>,
    pub loader: Option<Loader>,
}

#[derive(Clone, Debug)]
pub struct Resource {
    pub prefix: String,
    pub dirs: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl From<kurbo::Rect> for Rect {
    fn from(r: kurbo::Rect) -> Self {
        Self { x: r.x0, y: r.y0, width: r.width(), height: r.height() }
    }
}

impl From<BoundingRect> for Rect {
    fn from(r: BoundingRect) -> Self {
        Self { x: r.x, y: r.y, width: r.width, height: r.height }
    }
}

impl Rect {
    fn union(self, o: Rect) -> Rect {
        let empty = |r: &Rect| r.width <= 0.0 || r.height <= 0.0;
        match (empty(&self), empty(&o)) {
            (true, _) => o,
            (_, true) => self,
            _ => {
                let (x, y) = (self.x.min(o.x), self.y.min(o.y));
                let right = (self.x + self.width).max(o.x + o.width);
                let bottom = (self.y + self.height).max(o.y + o.height);
                Rect { x, y, width: right - x, height: bottom - y }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f64,
    pub actual_bounding_box_left: f64,
    pub actual_bounding_box_right: f64,
    pub actual_bounding_box_ascent: f64,
    pub actual_bounding_box_descent: f64,
    pub font_bounding_box_ascent: f64,
    pub font_bounding_box_descent: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaretPosition {
    pub node: u32,
    pub offset: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    pub count: u32,
    pub total_ms: f64,
    pub max_ms: f64,
}

impl Timing {
    pub(crate) fn add(&mut self, ms: f64) {
        self.count += 1;
        self.total_ms += ms;
        self.max_ms = self.max_ms.max(ms);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub resolve: Timing,
    pub frame_locked: Timing,
    pub frame: Timing,
    pub resolve_ms: f64,
    pub paint_ms: f64,
    pub damage_ms: f64,
    pub layers_ms: f64,
    pub resolves: u32,
    pub fragments_recorded: u32,
    pub fragments_reused: u32,
    pub animating: bool,
    pub paint_mismatches: u32,
    pub damage_by_fragment: u32,
    pub nodes: u32,
    pub images: u32,
    pub image_bytes: f64,
    pub fonts: u32,
    pub live_ids: u32,
    pub malloc_bytes: f64,
    pub panics: u32,
    pub damage_mismatches: u32,
}

pub struct Frame {
    pub size: (u32, u32),
    pub scale: f64,
    pub animating: bool,
    pub commit: u64,
    pub trace: Option<crate::log::Trace>,
    pub plane: Plane,
    pub panels: Vec<Panel>,
}

impl Frame {
    pub fn absorb(&mut self, old: Frame) {
        self.plane.absorb(old.plane);
        for old in old.panels {
            if let Some(panel) = self.panels.iter_mut().find(|p| p.id == old.id && p.size == old.size) {
                panel.plane.absorb(old.plane);
            }
        }
        self.trace = crate::log::Trace::earliest(old.trace, self.trace);
    }

    pub fn whole(&self) -> crate::gpu::Scene {
        let plane = &self.plane;
        let commands = match plane.layers.is_empty() {
            true => plane.drawing.commands.to_vec(),
            false => plane.layers.iter().flat_map(|l| l.drawing.scene().commands).collect(),
        };
        crate::gpu::Scene { tolerance: plane.drawing.tolerance, commands }
    }
}

pub struct Panel {
    pub id: u32,
    pub size: (u32, u32),
    pub plane: Plane,
    pub extent: Option<Rect>,
    pub reach: Option<Rect>,
}

#[derive(Default)]
pub struct Plane {
    pub drawing: Drawing,
    pub layers: Vec<Layer>,
    pub damage: Rect,
    pub damage_rects: Vec<Rect>,
    pub bounds: Option<Rect>,
}

impl Plane {
    fn damaged(&mut self, area: Rect) {
        self.damage = self.damage.union(area);
        self.damage_rects.push(area);
        self.collapse();
    }

    fn absorb(&mut self, old: Plane) {
        self.damage = self.damage.union(old.damage);
        self.damage_rects.extend(old.damage_rects);
        self.collapse();
    }

    fn collapse(&mut self) {
        if self.damage_rects.len() > 8 {
            self.damage_rects = vec![self.damage];
        }
    }
}

const _FRAME_IS_SEND: () = {
    const fn send<T: Send>() {}
    send::<Frame>()
};

const _HANDLES_ARE_SEND: () = {
    const fn send<T: Send + Sync>() {}
    send::<RenderHandle>();
    send::<Dom>();
};

pub type Waker = Box<dyn Fn() + Send>;

#[derive(Default)]
struct Slot {
    frame: Option<Frame>,
    waker: Option<Waker>,
    pending: bool,
    wanted: bool,
    answered: bool,
}

pub(crate) enum Cmd {
    Tick(f64),
    ScrollAt(f32, f32, f64, f64, f64),
    Pointer(f32, f32),
    Commit(u64),
    Invalidate,
}

struct Shared {
    engine: parking_lot::Mutex<Engine>,
    queue: parking_lot::Mutex<Vec<Cmd>>,
    queued: AtomicBool,
    pings: parking_lot::Mutex<Vec<SyncSender<()>>>,
    slot: Mutex<Slot>,
    commits: AtomicU64,
    touched: AtomicBool,
    tasks: AtomicU64,
}

impl Shared {
    fn push(&self, cmd: Cmd, wake: &Sender<()>) {
        self.queue.lock().push(cmd);
        self.queued.store(true, Ordering::Release);
        _ = wake.send(());
    }

    fn requeue(&self, held: Vec<Cmd>, wake: &Sender<()>) {
        if held.is_empty() {
            return;
        }
        self.queue.lock().splice(0..0, held);
        self.queued.store(true, Ordering::Release);
        _ = wake.send(());
    }

    fn ping(&self, reply: SyncSender<()>, wake: &Sender<()>) {
        self.pings.lock().push(reply);
        _ = wake.send(());
    }

    fn drain(&self, engine: &mut Engine) -> bool {
        if !self.queued.load(Ordering::Acquire) {
            return false;
        }
        let cmds = {
            let mut queue = self.queue.lock();
            self.queued.store(false, Ordering::Release);
            std::mem::take(&mut *queue)
        };
        let mut cmds = cmds.into_iter().peekable();
        let mut present = false;
        while let Some(cmd) = cmds.next() {
            if matches!(cmd, Cmd::Pointer(..)) && matches!(cmds.peek(), Some(Cmd::Pointer(..))) {
                continue;
            }
            if engine.uncommitted && matches!(cmd, Cmd::Pointer(..) | Cmd::ScrollAt(..)) {
                engine.held.push(cmd);
                continue;
            }
            engine.apply(cmd);
            present = true;
        }
        present
    }

    fn lock(&self, wake: &Sender<()>) -> parking_lot::MutexGuard<'_, Engine> {
        let mut engine = self.engine.lock();
        if self.drain(&mut engine) {
            engine.want_present = true;
            _ = wake.send(());
        }
        engine
    }

    fn call<T>(&self, wake: &Sender<()>, f: impl FnOnce(&mut Engine) -> Result<T>) -> Result<T> {
        self.call_in(wake, None, f)
    }

    fn script_call<T>(&self, wake: &Sender<()>, f: impl FnOnce(&mut Engine) -> Result<T>) -> Result<T> {
        self.call_in(wake, Some(self.tasks.load(Ordering::Acquire)), f)
    }

    fn call_in<T>(&self, wake: &Sender<()>, task: Option<u64>, f: impl FnOnce(&mut Engine) -> Result<T>) -> Result<T> {
        let mut engine = self.lock(wake);
        engine.in_task = task;
        let result = catch_unwind(AssertUnwindSafe(|| f(&mut engine))).unwrap_or_else(|_| {
            log::error!("panicked on a call");
            engine.recover();
            Err(Error::new("OperationError: render panicked on this call, see stderr"))
        });
        engine.in_task = None;
        result
    }
}

pub struct Document {
    shared: Arc<Shared>,
    wake: Sender<()>,
    _arrivals: Arc<Sender<()>>,
}

impl Document {
    pub fn new(width: u32, height: u32, scale: f32, options: &Options) -> Result<Self> {
        let (wake, rx) = channel();
        let arrivals = Arc::new(wake.clone());
        let weak = Arc::downgrade(&arrivals);
        let (ready, started) = sync_channel(1);
        let options = options.clone();
        std::thread::Builder::new()
            .name("acde-render".into())
            .spawn(move || {
                let engine = catch_unwind(AssertUnwindSafe(move || Engine::new(width, height, scale, &options, weak)))
                    .unwrap_or_else(|_| Err(Error::new("render: panicked while creating the document")));
                match engine {
                    Ok(engine) => {
                        let shared = Arc::new(Shared {
                            engine: parking_lot::Mutex::new(engine),
                            queue: Default::default(),
                            queued: AtomicBool::new(false),
                            pings: Default::default(),
                            slot: Default::default(),
                            commits: AtomicU64::new(0),
                            touched: AtomicBool::new(false),
                            tasks: AtomicU64::new(0),
                        });
                        _ = ready.send(Ok(shared.clone()));
                        run(&shared, &rx);
                    }
                    Err(e) => _ = ready.send(Err(e)),
                }
            })
            .map_err(|e| Error::new(format!("render: spawn thread: {e}")))?;
        let shared = started.recv().map_err(|_| Error::new("render: thread exited"))??;
        Ok(Self { shared, wake, _arrivals: arrivals })
    }

    pub fn dom(&self) -> Dom {
        Dom { shared: self.shared.clone(), wake: self.wake.clone() }
    }

    pub fn handle(&self) -> RenderHandle {
        RenderHandle { shared: self.shared.clone(), wake: self.wake.clone() }
    }

    pub fn sync(&self) {
        let (reply, done) = sync_channel(1);
        self.shared.ping(reply, &self.wake);
        _ = done.recv_timeout(Duration::from_secs(10));
    }

    pub fn scene(&self) -> crate::gpu::Scene {
        self.shared.call(&self.wake, |engine| Ok(engine.scene())).unwrap_or_else(|e| {
            log::error!("{e}");
            crate::gpu::Scene::new()
        })
    }
}

#[derive(Clone)]
pub struct RenderHandle {
    shared: Arc<Shared>,
    wake: Sender<()>,
}

impl RenderHandle {
    pub fn tick(&self, now_ms: f64) {
        self.shared.push(Cmd::Tick(now_ms), &self.wake);
    }

    pub fn frame(&self) -> Option<Frame> {
        let mut slot = self.shared.slot.lock().ok()?;
        if slot.pending && slot.frame.is_some() && !slot.answered {
            slot.pending = false;
            slot.wanted = true;
            drop(slot);
            self.shared.push(Cmd::Commit(self.shared.commits.load(Ordering::Relaxed)), &self.wake);
            return None;
        }
        (slot.wanted, slot.answered) = (false, false);
        let frame = slot.frame.take();
        if std::mem::take(&mut slot.pending) {
            drop(slot);
            self.shared.push(Cmd::Commit(self.shared.commits.load(Ordering::Relaxed)), &self.wake);
        }
        frame
    }

    pub fn scroll_at(&self, x: f32, y: f32, dx: f64, dy: f64) {
        self.shared.push(Cmd::ScrollAt(x, y, dx, dy, crate::log::now_ms()), &self.wake);
    }

    pub fn pointer(&self, x: f32, y: f32) {
        self.shared.push(Cmd::Pointer(x, y), &self.wake);
    }

    pub fn commit(&self) -> u64 {
        let seq = self.shared.commits.fetch_add(1, Ordering::Relaxed) + 1;
        self.shared.push(Cmd::Commit(seq), &self.wake);
        seq
    }

    pub fn invalidate(&self) {
        self.shared.push(Cmd::Invalidate, &self.wake);
    }

    pub fn set_waker(&self, waker: Waker) {
        if let Ok(mut slot) = self.shared.slot.lock() {
            if slot.frame.is_some() || slot.pending {
                waker();
            }
            slot.waker = Some(waker);
        }
    }
}

fn run(shared: &Shared, rx: &Receiver<()>) {
    while rx.recv().is_ok() {
        while rx.try_recv().is_ok() {}
        let pings = std::mem::take(&mut *shared.pings.lock());
        let mut engine = shared.engine.lock();
        let mut present = shared.drain(&mut engine) | std::mem::take(&mut engine.want_present) | engine.take_arrived();
        while present && engine.is_dirty() {
            present = present_frame(shared, &mut engine);
        }
        if !engine.uncommitted
            && !engine.is_dirty()
            && let Ok(mut slot) = shared.slot.lock()
            && slot.wanted
            && slot.frame.is_some()
        {
            (slot.wanted, slot.answered) = (false, true);
            if let Some(wake) = &slot.waker {
                wake();
            }
        }
        drop(engine);
        for ping in pings {
            _ = ping.send(());
        }
    }
}

fn yield_lock(shared: &Shared, engine: &mut parking_lot::MutexGuard<'_, Engine>) -> bool {
    parking_lot::MutexGuard::bump(engine);
    shared.drain(engine)
}

fn present_frame(shared: &Shared, engine: &mut parking_lot::MutexGuard<'_, Engine>) -> bool {
    if engine.uncommitted {
        return false;
    }
    if let Ok(mut slot) = shared.slot.lock()
        && slot.frame.is_some()
        && !slot.wanted
    {
        slot.pending = true;
        return false;
    }
    let failed = |engine: &mut Engine| {
        log::error!("panicked while painting, frame dropped");
        engine.drop_frame();
        engine.recover();
    };
    let started = std::time::Instant::now();
    if catch_unwind(AssertUnwindSafe(|| _ = engine.resolved())).is_err() {
        failed(engine);
        return false;
    }
    let resolved = crate::log::since_ms(started);
    let mut again = yield_lock(shared, engine);
    if engine.uncommitted {
        return again;
    }
    let paint_started = std::time::Instant::now();
    let Ok(painted) = catch_unwind(AssertUnwindSafe(|| engine.paint_frame())) else {
        failed(engine);
        return false;
    };
    let locked = crate::log::since_ms(paint_started);
    again |= yield_lock(shared, engine);
    let finish_started = std::time::Instant::now();
    let Ok(mut frame) = catch_unwind(AssertUnwindSafe(|| engine.finish_frame(painted))) else {
        failed(engine);
        return false;
    };
    let finished = crate::log::since_ms(finish_started);
    engine.frame_timed(locked, resolved + locked + finished);
    log::debug!(target: "perf::frame", "{} commit={} resolve={resolved:.3} paint={locked:.3} finish={finished:.3} ms={:.3}", engine.name, frame.commit, resolved + locked + finished);
    let Ok(mut slot) = shared.slot.lock() else {
        return again;
    };
    if let Some(old) = slot.frame.take() {
        frame.absorb(old);
    }
    slot.frame = Some(frame);
    slot.answered = std::mem::take(&mut slot.wanted);
    if let Some(wake) = &slot.waker {
        wake();
    }
    again
}

impl Engine {
    fn apply(&mut self, cmd: Cmd) {
        let applied = catch_unwind(AssertUnwindSafe(|| match cmd {
            Cmd::Tick(now_ms) => self.tick(now_ms),
            Cmd::ScrollAt(x, y, dx, dy, at) => self.scroll_at(x, y, dx, dy, at),
            Cmd::Pointer(x, y) => self.pointer(x, y),
            Cmd::Commit(seq) => self.committed = self.committed.max(seq),
            Cmd::Invalidate => self.invalidate(),
        }));
        if applied.is_err() {
            log::error!("panicked, command dropped");
            self.recover();
        }
    }
}

pub enum Pixels {
    Cpu { width: u32, height: u32, rgba: Vec<u8> },
    Gpu(std::sync::Arc<crate::gpu::GpuImage>),
}
