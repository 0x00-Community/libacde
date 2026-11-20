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

pub fn adapters() -> (wgpu::Instance, Vec<wgpu::Adapter>) {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor { backends: super::BACKEND, ..wgpu::InstanceDescriptor::new_without_display_handle_from_env() });
    let mut adapters: Vec<_> = pollster::block_on(instance.enumerate_adapters(super::BACKEND)).into_iter().collect();
    adapters.sort_by_key(|a| match a.get_info().device_type {
        wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu => 0,
        wgpu::DeviceType::Cpu => 2,
        _ => 1,
    });
    (instance, adapters)
}

#[cfg(target_os = "linux")]
pub struct GlDevice {
    pub gpu: Arc<Gpu>,
    pub tier: &'static str,
    pub limits: wgpu::Limits,
    pub downlevel: wgpu::DownlevelCapabilities,
    pub vello: Result<(), String>,
}

#[cfg(target_os = "linux")]
impl GlDevice {
    pub const SWITCH: &'static str = "ACDE_GL_ES";
    pub const VELLO_STORAGE_BUFFERS: u32 = 8;
    const VELLO_INVOCATIONS: u32 = 256;

    pub fn enabled() -> bool {
        std::env::var(Self::SWITCH).is_ok_and(|value| value == "1")
    }

    pub fn open(label: &str) -> Result<Self, String> {
        if !Self::enabled() {
            return Err(format!("{}=1 is not set: the OpenGL ES device is off", Self::SWITCH));
        }
        let gl = wgpu::Backends::GL;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: gl, ..wgpu::InstanceDescriptor::new_without_display_handle_from_env() });
        let adapters = pollster::block_on(instance.enumerate_adapters(gl));
        let names: Vec<String> = adapters.iter().map(|a| format!("{} ({:?})", a.get_info().name, a.get_info().device_type)).collect();
        let adapter = adapters
            .into_iter()
            .find(|a| a.get_info().device_type != wgpu::DeviceType::Cpu)
            .ok_or_else(|| if names.is_empty() { "no GL adapter (EGL)".to_owned() } else { format!("only software GL adapters: {}", names.join(", ")) })?;
        let info = adapter.get_info();
        let downlevel = adapter.get_downlevel_capabilities();
        let tiers = DeviceRequest::tiers(webgpu_features(&adapter), adapter.limits());
        let open = |request: &DeviceRequest| pollster::block_on(adapter.request_device(&request.descriptor(label))).map_err(|e| e.to_string());
        let ((device, queue), tier) = DeviceRequest::open(label, &tiers, open).map_err(|e| format!("opening {}: {e}", info.name))?;
        let limits = device.limits();
        let vello = Self::try_vello(&device, &queue, &limits, &downlevel);
        Ok(Self { gpu: Gpu::new(device, queue, info), tier, limits, downlevel, vello })
    }

    pub fn fits_vello(&self) -> bool {
        self.vello.is_ok()
    }

    fn try_vello(device: &wgpu::Device, queue: &wgpu::Queue, limits: &wgpu::Limits, downlevel: &wgpu::DownlevelCapabilities) -> Result<(), String> {
        if !downlevel.flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
            return Err("no compute shaders".into());
        }
        if limits.max_storage_buffers_per_shader_stage < Self::VELLO_STORAGE_BUFFERS {
            return Err(format!("{} storage buffers per stage, vello needs {}", limits.max_storage_buffers_per_shader_stage, Self::VELLO_STORAGE_BUFFERS));
        }
        if limits.max_compute_invocations_per_workgroup < Self::VELLO_INVOCATIONS || limits.max_compute_workgroup_size_x < Self::VELLO_INVOCATIONS {
            return Err(format!("workgroups of {} invocations, vello needs {}", limits.max_compute_invocations_per_workgroup, Self::VELLO_INVOCATIONS));
        }
        let started = std::time::Instant::now();
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::vello::Vello::on_this_thread(device, queue).map(drop)))
            .unwrap_or_else(|_| Err("building vello's pipelines panicked".into()));
        let internal = pollster::block_on(internal.pop());
        let validation = pollster::block_on(validation.pop());
        let errors: Vec<String> = [built.err(), internal.map(|e| e.to_string()), validation.map(|e| e.to_string())].into_iter().flatten().collect();
        if !errors.is_empty() {
            return Err(format!("vello's pipelines do not build: {}", errors.join("; ")));
        }
        log::info!("acde gl: vello's pipelines built in {:.1}ms", started.elapsed().as_secs_f64() * 1000.0);
        Ok(())
    }

    pub fn describe(&self) -> String {
        let (info, limits, flags) = (&self.gpu.info, &self.limits, self.downlevel.flags);
        let flag = |name: &str, f: wgpu::DownlevelFlags| format!("{name}={}", flags.contains(f));
        [
            format!("{} ({:?}, {:?}, driver {} {})", info.name, info.backend, info.device_type, info.driver, info.driver_info),
            format!("device tier {}", self.tier),
            format!("max_storage_buffers_per_shader_stage={}", limits.max_storage_buffers_per_shader_stage),
            format!(
                "max_compute_workgroup_size={}x{}x{}",
                limits.max_compute_workgroup_size_x, limits.max_compute_workgroup_size_y, limits.max_compute_workgroup_size_z
            ),
            format!("max_compute_invocations_per_workgroup={}", limits.max_compute_invocations_per_workgroup),
            format!("max_compute_workgroup_storage_size={}", limits.max_compute_workgroup_storage_size),
            format!("max_storage_buffer_binding_size={}", limits.max_storage_buffer_binding_size),
            format!("max_storage_textures_per_shader_stage={}", limits.max_storage_textures_per_shader_stage),
            flag("COMPUTE_SHADERS", wgpu::DownlevelFlags::COMPUTE_SHADERS),
            flag("FRAGMENT_WRITABLE_STORAGE", wgpu::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE),
            flag("VERTEX_STORAGE", wgpu::DownlevelFlags::VERTEX_STORAGE),
            flag("INDIRECT_EXECUTION", wgpu::DownlevelFlags::INDIRECT_EXECUTION),
            format!("shader model {:?}", self.downlevel.shader_model),
            match &self.vello {
                Ok(()) => "vello=runs".to_owned(),
                Err(e) => format!("vello=no ({e})"),
            },
        ]
        .join(", ")
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
        let (instance, adapters) = adapters();
        let Some(adapter) = adapters.into_iter().next() else { return Err(no_adapter(&instance)) };
        let info = adapter.get_info();
        let tiers = DeviceRequest::tiers(webgpu_features(&adapter), adapter.limits());
        let ((device, queue), _) =
            DeviceRequest::open(label, &tiers, |request| pollster::block_on(adapter.request_device(&request.descriptor(label))).map_err(|e| e.to_string()))
                .map_err(|e| format!("opening {}: {e}", info.name))?;
        Ok((instance, adapter, Self::new(device, queue, info)))
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
