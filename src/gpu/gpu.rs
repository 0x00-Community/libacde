use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Once, OnceLock};
use std::time::Duration;

use crate::vello::Shared;

use super::{Compositor, GpuImage, Offscreen};

type LostListener = Box<dyn Fn(&str) + Send>;
type Waiter = Box<dyn FnOnce(Option<Arc<Gpu>>) + Send>;
pub type ErrorSink = Box<dyn FnMut(wgpu::Error)>;

const WAIT: Duration = Duration::from_secs(10);
const POLL_BUSY: Duration = Duration::from_micros(500);
const POLL_IDLE: Duration = Duration::from_millis(4);

pub struct Gpu {
    pub shared: Arc<Shared>,
    pub info: wgpu::AdapterInfo,
    polls: Mutex<usize>,
    polling: Condvar,
    poller: Once,
    lost: Mutex<HashMap<u64, LostListener>>,
    gone: AtomicBool,
    compositor: OnceLock<Arc<dyn Compositor>>,
}

enum State {
    Unset,
    Expected,
    Own(Option<Arc<Gpu>>),
    Published(Option<Arc<Gpu>>),
}

struct Registry {
    state: State,
    waiters: Vec<Waiter>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry { state: State::Unset, waiters: Vec::new() });
static READY: Condvar = Condvar::new();
static NEXT_LISTENER: AtomicU64 = AtomicU64::new(1);
static REPLACED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static SINK: RefCell<Option<ErrorSink>> = const { RefCell::new(None) };
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn adapters() -> (wgpu::Instance, Vec<Candidate>) {
    let allowing = wgpu::InstanceDescriptor::new_without_display_handle_from_env().flags | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER;
    let strict = instance(allowing - wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER);
    let compliant: Vec<wgpu::AdapterInfo> = pollster::block_on(strict.enumerate_adapters(super::BACKEND)).iter().map(wgpu::Adapter::get_info).collect();
    let all = instance(allowing);
    let named = std::env::var(NAMED).ok().filter(|name| !name.is_empty());
    let mut found: Vec<Candidate> = pollster::block_on(all.enumerate_adapters(super::BACKEND))
        .into_iter()
        .map(|adapter| {
            let info = adapter.get_info();
            let compliant = compliant.iter().any(|c| (c.vendor, c.device, &c.name, &c.driver) == (info.vendor, info.device, &info.name, &info.driver));
            let named = named.as_deref().is_some_and(|name| info.name.contains(name));
            Candidate { adapter, compliant, named }
        })
        .collect();
    found.sort_by_key(Candidate::rank);
    (all, found)
}

pub const NAMED: &str = "MESA_D3D12_DEFAULT_ADAPTER_NAME";

pub const ALLOW_CPU: &str = "ACDE_GPU_ALLOW_CPU";

pub fn allow_cpu() -> bool {
    std::env::var(ALLOW_CPU).is_ok_and(|value| value == "1")
}

fn instance(flags: wgpu::InstanceFlags) -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor { backends: super::BACKEND, flags, ..wgpu::InstanceDescriptor::new_without_display_handle_from_env() })
}

pub type Fits = dyn Fn(&wgpu::Device, &wgpu::Queue, &wgpu::Adapter) -> Result<(), String>;

pub struct Candidate {
    pub adapter: wgpu::Adapter,
    pub compliant: bool,
    pub named: bool,
}

impl Candidate {
    fn rank(&self) -> (bool, u8, u8) {
        let (class, kind) = match (self.adapter.get_info().device_type, self.compliant) {
            (wgpu::DeviceType::DiscreteGpu, true) => (0, 0),
            (wgpu::DeviceType::IntegratedGpu, true) => (0, 1),
            (wgpu::DeviceType::DiscreteGpu, false) => (1, 0),
            (wgpu::DeviceType::IntegratedGpu, false) => (1, 1),
            (wgpu::DeviceType::Cpu, _) => (3, 0),
            _ => (2, 0),
        };
        (!self.named, class, kind)
    }
}

pub fn webgpu_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features() & wgpu::Features::all_webgpu_mask()
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceRequest {
    pub tier: &'static str,
    pub features: wgpu::Features,
    pub limits: wgpu::Limits,
}

impl DeviceRequest {
    pub fn tiers(features: wgpu::Features, limits: wgpu::Limits) -> Vec<Self> {
        let fallback = wgpu::Limits::downlevel_defaults().using_resolution(limits.clone());
        let tiers = [
            Self { tier: "WebGPU features", features, limits: limits.clone() },
            Self { tier: "no optional features", features: wgpu::Features::empty(), limits },
            Self { tier: "downlevel limits", features: wgpu::Features::empty(), limits: fallback },
        ];
        let mut unique: Vec<Self> = Vec::new();
        for tier in tiers {
            if !unique.iter().any(|seen| seen.features == tier.features && seen.limits == tier.limits) {
                unique.push(tier);
            }
        }
        unique
    }

    pub fn open<T>(label: &str, tiers: &[Self], mut open: impl FnMut(&Self) -> Result<T, String>) -> Result<(T, &'static str), String> {
        let mut errors = Vec::new();
        for request in tiers {
            match open(request) {
                Ok(device) => {
                    log::info!("{label}: GPU device opened with {}", request.tier);
                    return Ok((device, request.tier));
                }
                Err(e) => {
                    log::warn!("{label}: requesting the GPU device with {} ({:?}) failed: {e}", request.tier, request.features);
                    errors.push(format!("{}: {e}", request.tier));
                }
            }
        }
        Err(errors.join("; "))
    }

    pub fn descriptor<'a>(&self, label: &'a str) -> wgpu::DeviceDescriptor<'a> {
        wgpu::DeviceDescriptor { label: Some(label), required_features: self.features, required_limits: self.limits.clone(), ..Default::default() }
    }
}

pub fn no_adapter(instance: &wgpu::Instance) -> String {
    let (api, hint) = cfg_select! {
        windows => ("Direct3D 12", "install or update the GPU's driver from its vendor or Windows Update"),
        target_os = "macos" => ("Metal", "this Mac's GPU does not support Metal"),
        _ => ("Vulkan", "install the GPU's Vulkan driver (for example vulkan-intel, vulkan-radeon, nvidia-utils) or point VK_DRIVER_FILES at its ICD"),
    };
    let cause = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) {
        Err(error) => format!("wgpu: {error}"),
        Ok(adapter) => format!("only the software adapter {}", adapter.get_info().name),
    };
    let seen = system_gpu().map(|gpu| format!("the system has a GPU ({gpu}) but ")).unwrap_or_default();
    format!("{seen}no usable {api} driver was found: {hint} ({cause})")
}

#[cfg(target_os = "linux")]
fn system_gpu() -> Option<String> {
    let mut cards: Vec<_> = std::fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.strip_prefix("card").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
        .collect();
    cards.sort();
    let card = cards.into_iter().next()?;
    let driver = std::fs::read_link(format!("/sys/class/drm/{card}/device/driver")).ok();
    let driver = driver.as_deref().and_then(|path| path.file_name()).map(|name| format!(", kernel driver {}", name.to_string_lossy()));
    Some(format!("{card}{}", driver.unwrap_or_default()))
}

#[cfg(not(target_os = "linux"))]
fn system_gpu() -> Option<String> {
    None
}

pub fn texture(device: &wgpu::Device, (width, height): (u32, u32), usage: wgpu::TextureUsages, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage,
        view_formats: &[],
    })
}

pub fn set_error_sink(sink: Option<ErrorSink>) {
    SINK.with(|s| *s.borrow_mut() = sink);
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, info: wgpu::AdapterInfo) -> Arc<Self> {
        Self::with(Shared::new(device, queue), info)
    }

    #[cfg(target_os = "linux")]
    pub fn exporting(device: wgpu::Device, queue: wgpu::Queue, info: wgpu::AdapterInfo) -> Arc<Self> {
        Self::with(Shared::exporting(device, queue), info)
    }

    fn with(shared: Arc<Shared>, info: wgpu::AdapterInfo) -> Arc<Self> {
        let gpu = Arc::new(Self {
            shared,
            info,
            polls: Mutex::new(0),
            polling: Condvar::new(),
            poller: Once::new(),
            lost: Mutex::default(),
            gone: AtomicBool::new(false),
            compositor: OnceLock::new(),
        });
        let weak = Arc::downgrade(&gpu);
        gpu.device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            let error = SINK.with(|sink| match sink.borrow_mut().as_mut() {
                Some(sink) => {
                    sink(error);
                    None
                }
                None => Some(error),
            });
            let Some(error) = error else { return };
            if weak.upgrade().is_some_and(|gpu| gpu.gone()) {
                return log::debug!("GPU error after the device was lost: {error}");
            }
            let thread = std::thread::current();
            log::error!("wgpu error on thread {} with no page to report it to: {error}", thread.name().unwrap_or("unnamed"));
        }));
        let weak = Arc::downgrade(&gpu);
        gpu.device.set_device_lost_callback(move |reason, message| {
            log::error!("web layer GPU device lost ({reason:?}): {message}");
            if let Some(gpu) = weak.upgrade() {
                gpu.mark_gone();
                for listener in lock(&gpu.lost).values() {
                    listener(&message);
                }
            }
        });
        gpu
    }

    pub fn set_compositor(&self, compositor: Arc<dyn Compositor>) -> bool {
        self.compositor.set(compositor).is_ok()
    }

    pub fn offscreen(self: &Arc<Self>) -> Result<Box<dyn Offscreen>, String> {
        let compositor = self.compositor.get().ok_or("this GPU has no compositor")?;
        compositor.offscreen(self.clone())
    }

    pub fn open(label: &str) -> Result<(wgpu::Instance, wgpu::Adapter, Arc<Self>), String> {
        Self::open_with(label, &|_, _, _| Ok(()))
    }

    pub fn open_with(label: &str, fits: &Fits) -> Result<(wgpu::Instance, wgpu::Adapter, Arc<Self>), String> {
        let (instance, candidates) = adapters();
        if candidates.is_empty() {
            return Err(no_adapter(&instance));
        }
        let mut errors = Vec::new();
        for Candidate { adapter, compliant, .. } in candidates {
            let info = adapter.get_info();
            match Self::open_adapter(label, &adapter, compliant, fits) {
                Ok((device, queue)) => {
                    if !errors.is_empty() {
                        log::warn!("{label}: using {} after: {}", info.name, errors.join("; "));
                    }
                    return Ok((instance, adapter, Self::new(device, queue, info)));
                }
                Err(e) => errors.push(format!("{}: {e}", info.name)),
            }
        }
        Err(errors.join("; "))
    }

    fn open_adapter(label: &str, adapter: &wgpu::Adapter, compliant: bool, fits: &Fits) -> Result<(wgpu::Device, wgpu::Queue), String> {
        let tiers = DeviceRequest::tiers(webgpu_features(adapter), adapter.limits());
        let ((device, queue), _) =
            DeviceRequest::open(label, &tiers, |request| pollster::block_on(adapter.request_device(&request.descriptor(label))).map_err(|e| e.to_string()))?;
        if !compliant {
            fits(&device, &queue, adapter).map_err(|e| format!("not a conformant driver, and {e}"))?;
            log::info!("{label}: {} is not a conformant driver, but it does the work", adapter.get_info().name);
        }
        Ok((device, queue))
    }

    pub fn expect() {
        let mut registry = lock(&REGISTRY);
        if matches!(registry.state, State::Unset) {
            registry.state = State::Expected;
        }
    }

    pub fn publish(gpu: Option<Arc<Gpu>>) -> Option<Arc<Gpu>> {
        let mut registry = lock(&REGISTRY);
        if let State::Published(published) = &registry.state {
            return published.clone();
        }
        registry.state = State::Published(gpu.clone());
        READY.notify_all();
        let waiters = std::mem::take(&mut registry.waiters);
        drop(registry);
        for waiter in waiters {
            waiter(gpu.clone());
        }
        gpu
    }

    /// Publishes `gpu` in place of a lost device (the host window's recovery): `get`, `shared` and `when_ready` give the new
    /// one from then on, and everything holding the old one finds it `gone` and moves over.
    pub fn replace(gpu: Arc<Gpu>) {
        let mut registry = lock(&REGISTRY);
        registry.state = State::Published(Some(gpu.clone()));
        REPLACED.store(true, Ordering::SeqCst);
        READY.notify_all();
        let waiters = std::mem::take(&mut registry.waiters);
        drop(registry);
        for waiter in waiters {
            waiter(Some(gpu.clone()));
        }
    }

    /// Waits up to `wait` for a live device other than `lost` to be published (the host window's recovery) and gives it.
    pub fn replacement(lost: &Arc<Gpu>, wait: Duration) -> Option<Arc<Gpu>> {
        let fresh = |state: &State| match state {
            State::Published(Some(gpu)) if !Arc::ptr_eq(gpu, lost) && !gpu.gone() => Some(gpu.clone()),
            _ => None,
        };
        let registry = lock(&REGISTRY);
        let (registry, _) = READY.wait_timeout_while(registry, wait, |r| fresh(&r.state).is_none()).unwrap_or_else(|e| e.into_inner());
        fresh(&registry.state)
    }

    /// Whether a lost device was ever replaced in this process.
    pub fn replaced() -> bool {
        REPLACED.load(Ordering::SeqCst)
    }

    /// Whether `image` lives on this device (its image set holds it): an image of a replaced device does not.
    pub fn owns(&self, image: &GpuImage) -> bool {
        self.images.find(image.id()).is_some_and(|found| std::ptr::eq(Arc::as_ptr(&found), image))
    }

    pub fn get() -> Option<Option<Arc<Gpu>>> {
        match &lock(&REGISTRY).state {
            State::Published(gpu) => Some(gpu.clone()),
            _ => None,
        }
    }

    pub fn when_ready(f: impl FnOnce(Option<Arc<Gpu>>) + Send + 'static) {
        let mut registry = lock(&REGISTRY);
        match &registry.state {
            State::Published(gpu) => {
                let gpu = gpu.clone();
                drop(registry);
                f(gpu);
            }
            _ => registry.waiters.push(Box::new(f)),
        }
    }

    pub fn shared() -> Option<Arc<Gpu>> {
        let mut registry = lock(&REGISTRY);
        if matches!(registry.state, State::Unset) {
            registry.state = State::Own(Self::open("acde offscreen").map(|(.., gpu)| gpu).inspect_err(|e| log::warn!("render: GPU: {e}")).ok());
        }
        let (registry, _) = READY.wait_timeout_while(registry, WAIT, |r| matches!(r.state, State::Expected)).unwrap_or_else(|e| e.into_inner());
        match &registry.state {
            State::Own(gpu) | State::Published(gpu) => gpu.clone(),
            _ => None,
        }
    }

    pub fn gone(&self) -> bool {
        self.gone.load(Ordering::SeqCst)
    }

    pub fn destroy(&self) {
        self.mark_gone();
        self.device.destroy();
    }

    /// Marks the device gone and wakes the poll thread so it ends.
    fn mark_gone(&self) {
        let polls = lock(&self.polls);
        self.gone.store(true, Ordering::SeqCst);
        drop(polls);
        self.polling.notify_all();
    }

    pub fn hold_poll(self: &Arc<Self>) {
        self.poller.call_once(|| {
            let gpu = self.clone();
            if let Err(e) = std::thread::Builder::new().name("acde-gpu-poll".into()).spawn(move || gpu.poll_forever()) {
                log::error!("WebGPU poll thread: {e}");
            }
        });
        *lock(&self.polls) += 1;
        self.polling.notify_all();
    }

    pub fn release_poll(&self) {
        let mut polls = lock(&self.polls);
        *polls = polls.saturating_sub(1);
        self.polling.notify_all();
    }

    fn poll_forever(&self) {
        loop {
            drop(self.polling.wait_while(lock(&self.polls), |p| *p == 0 && !self.gone()).unwrap_or_else(|e| e.into_inner()));
            // a lost device has nothing more to finish; ending here lets it go once its other holders do
            if self.gone() {
                return;
            }
            let pause = match self.device.poll(wgpu::PollType::Poll) {
                Ok(wgpu::PollStatus::QueueEmpty) | Err(_) => POLL_IDLE,
                Ok(_) => POLL_BUSY,
            };
            let polls = lock(&self.polls);
            if *polls > 0 {
                drop(self.polling.wait_timeout(polls, pause).unwrap_or_else(|e| e.into_inner()));
            }
        }
    }

    pub fn on_lost(&self, f: impl Fn(&str) + Send + 'static) -> u64 {
        let id = NEXT_LISTENER.fetch_add(1, Ordering::Relaxed);
        lock(&self.lost).insert(id, Box::new(f));
        id
    }

    pub fn forget_lost(&self, id: u64) {
        lock(&self.lost).remove(&id);
    }
}

impl std::ops::Deref for Gpu {
    type Target = Shared;

    fn deref(&self) -> &Shared {
        &self.shared
    }
}
