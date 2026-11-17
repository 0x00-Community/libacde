use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::host::bind;
use crate::host::rt::{self, Msg};

pub fn app_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ACDE_APP") {
        return PathBuf::from(dir);
    }
    let exe = std::env::current_exe().ok().and_then(|e| std::fs::canonicalize(e).ok()).unwrap_or_default();
    let bin = exe.parent().map(PathBuf::from).unwrap_or_default();
    let installed = bin.join("app/dist");
    if installed.join("broker.js").is_file() {
        return installed;
    }
    bin.join("../../web/packages/acde-host/dist")
}

struct Handle {
    tx: Sender<Msg>,
    isolate: v8::IsolateHandle,
    stopping: Arc<AtomicBool>,
    killed: Arc<AtomicBool>,
    busy: Arc<AtomicU64>,
    tid: i32,
}

impl Handle {
    fn cpu_ms(&self) -> f64 {
        let stat = std::fs::read_to_string(format!("/proc/self/task/{}/schedstat", self.tid)).unwrap_or_default();
        stat.split_whitespace().next().and_then(|ns| ns.parse::<u64>().ok()).map_or(0.0, |ns| ns as f64 / 1e6)
    }
}

#[derive(Default)]
struct Webs {
    live: bind::Slab<Handle>,
    system: System,
}

#[derive(Default)]
struct System {
    workspace: Option<crate::runtime::Workspace>,
    terminals: Option<crate::runtime::Terminals>,
    #[cfg(target_os = "linux")]
    networks: crate::runtime::Networks,
    #[cfg(target_os = "linux")]
    bluetooth: crate::runtime::Bluetooths,
    #[cfg(target_os = "linux")]
    sounds: crate::runtime::Sounds,
    #[cfg(target_os = "linux")]
    devices: crate::runtime::Devices,
    #[cfg(target_os = "linux")]
    power: crate::runtime::Power,
    #[cfg(target_os = "linux")]
    office: Option<crate::runtime::OfficeService>,
}

struct Grants {
    app: Option<String>,
    storage: Option<crate::storage::Place>,
    fs: Option<crate::runtime::Workspace>,
    picker: crate::host::picker::Picker,
    capture: crate::host::capture::Allowed,
    terminal: Option<crate::runtime::Terminals>,
    #[cfg(target_os = "linux")]
    network: Option<crate::runtime::Networks>,
    #[cfg(target_os = "linux")]
    office: Option<crate::runtime::OfficeService>,
    #[cfg(target_os = "linux")]
    system: bool,
    #[cfg(target_os = "linux")]
    bluetooth: Option<crate::runtime::Bluetooths>,
    #[cfg(target_os = "linux")]
    audio: Option<crate::runtime::Sounds>,
    #[cfg(target_os = "linux")]
    devices: Option<crate::runtime::Devices>,
    #[cfg(target_os = "linux")]
    power: Option<crate::runtime::Power>,
    input: Option<Option<Arc<dyn crate::host::Input>>>,
    #[cfg(target_os = "linux")]
    display: Option<(Option<Arc<dyn crate::host::Display>>, crate::runtime::Power)>,
}

impl Webs {
    fn grants(&mut self, scope: &mut v8::PinScope, wanted: v8::Local<v8::Value>) -> Grants {
        let Ok(wanted) = v8::Local::<v8::Object>::try_from(wanted) else {
            return Grants {
                app: None,
                storage: None,
                fs: None,
                picker: crate::host::picker::Picker::new(rt::current().platform(), None),
                capture: Default::default(),
                terminal: None,
                #[cfg(target_os = "linux")]
                network: None,
                #[cfg(target_os = "linux")]
                office: None,
                #[cfg(target_os = "linux")]
                system: false,
                #[cfg(target_os = "linux")]
                bluetooth: None,
                #[cfg(target_os = "linux")]
                audio: None,
                #[cfg(target_os = "linux")]
                devices: None,
                #[cfg(target_os = "linux")]
                power: None,
                input: None,
                #[cfg(target_os = "linux")]
                display: None,
            };
        };
        let fs = match bind::get(scope, wanted, "fs") {
            Some(v) if v.is_true() => Some(Err(None)),
            Some(v) if v.is_array() => Some(Err(Some(bind::strings(scope, v).unwrap_or_default()))),
            Some(v) => v8::Local::<v8::Object>::try_from(v).ok().map(|files| {
                let places = bind::get(scope, files, "places").and_then(|places| bind::strings(scope, places)).unwrap_or_default();
                let paths = bind::get(scope, files, "paths").is_some_and(|paths| paths.is_true());
                Ok(crate::runtime::Files::new(&places, paths))
            }),
            None => None,
        };
        let app = bind::get_string(scope, wanted, "app");
        let capture = crate::host::capture::Allowed {
            camera: bind::get(scope, wanted, "camera").is_some_and(|v| v.is_true()),
            microphone: bind::get(scope, wanted, "microphone").is_some_and(|v| v.is_true()),
        };
        let terminal = bind::get(scope, wanted, "terminal").is_some_and(|v| v.is_true());
        #[cfg(target_os = "linux")]
        let network = bind::get(scope, wanted, "network").is_some_and(|v| v.is_true());
        #[cfg(target_os = "linux")]
        let office = bind::get(scope, wanted, "office").is_some_and(|v| v.is_true());
        #[cfg(target_os = "linux")]
        let computer = bind::get(scope, wanted, "system").is_some_and(|v| v.is_true());
        #[cfg(target_os = "linux")]
        let bluetooth = bind::get(scope, wanted, "bluetooth").is_some_and(|v| v.is_true());
        #[cfg(target_os = "linux")]
        let audio = bind::get(scope, wanted, "audio").is_some_and(|v| v.is_true());
        #[cfg(target_os = "linux")]
        let devices = bind::get(scope, wanted, "devices").is_some_and(|v| v.is_true());
        #[cfg(target_os = "linux")]
        let power = bind::get(scope, wanted, "power").is_some_and(|v| v.is_true());
        let input = bind::get(scope, wanted, "input").is_some_and(|v| v.is_true());
        #[cfg(target_os = "linux")]
        let display = bind::get(scope, wanted, "display").is_some_and(|v| v.is_true());
        let system = &mut self.system;
        let workspace = system.workspace.get_or_insert_with(crate::runtime::Workspace::new);
        let picker = crate::host::picker::Picker::new(rt::current().platform(), app.as_deref().map(|app| workspace.scoped(Some(Vec::new()), Some(app))));
        let fs = fs.and_then(|fs| match fs {
            Ok(files) => app.as_deref().map(|app| workspace.files(files, app)),
            Err(roots) => Some(workspace.scoped(roots, app.as_deref())),
        });
        Grants {
            storage: crate::host::storage::place(scope, wanted),
            #[cfg(target_os = "linux")]
            office: system.office.as_ref().filter(|_| office).map(|service| service.for_web(fs.clone())),
            fs,
            capture,
            app,
            picker,
            terminal: terminal.then(|| system.terminals.get_or_insert_with(crate::runtime::Terminals::new).shared()),
            #[cfg(target_os = "linux")]
            network: network.then(|| system.networks.clone()),
            #[cfg(target_os = "linux")]
            system: computer,
            #[cfg(target_os = "linux")]
            bluetooth: bluetooth.then(|| system.bluetooth.clone()),
            #[cfg(target_os = "linux")]
            audio: audio.then(|| system.sounds.clone()),
            #[cfg(target_os = "linux")]
            devices: devices.then(|| system.devices.clone()),
            #[cfg(target_os = "linux")]
            power: power.then(|| system.power.clone()),
            input: input.then(|| rt::current().platform().input()),
            #[cfg(target_os = "linux")]
            display: display.then(|| (rt::current().platform().display(), system.power.clone())),
        }
    }
}

impl Grants {
    fn install(self, scope: &mut v8::PinScope, natives: v8::Local<v8::Object>) {
        crate::host::capture::install(scope, natives, self.capture, self.app.as_deref());
        crate::host::device::install(scope, natives);
        crate::host::webnative::bind_app(self.app);
        if let Some(place) = self.storage {
            crate::host::storage::install(scope, natives, place);
        }
        if let Some(fs) = self.fs {
            crate::host::fs::install(scope, natives, fs);
        }
        crate::host::picker::install(scope, natives, self.picker);
        if let Some(terminals) = self.terminal {
            crate::host::terminal::install(scope, natives, terminals);
        }
        #[cfg(target_os = "linux")]
        if let Some(networks) = self.network {
            crate::host::network::install(scope, natives, networks);
        }
        #[cfg(target_os = "linux")]
        if let Some(office) = self.office {
            crate::host::office::install(scope, natives, office);
        }
        #[cfg(target_os = "linux")]
        if self.system {
            crate::host::system::install(scope, natives);
        }
        #[cfg(target_os = "linux")]
        if let Some(bluetooth) = self.bluetooth {
            crate::host::bluetooth::install(scope, natives, bluetooth);
        }
        #[cfg(target_os = "linux")]
        if let Some(sounds) = self.audio {
            crate::host::sound::install(scope, natives, sounds);
        }
        #[cfg(target_os = "linux")]
        if let Some(devices) = self.devices {
            crate::host::devices::install(scope, natives, devices);
        }
        #[cfg(target_os = "linux")]
        if let Some(power) = self.power {
            crate::host::power::install(scope, natives, power);
        }
        if let Some(input) = self.input {
            crate::host::input::install(scope, natives, input);
        }
        #[cfg(target_os = "linux")]
        if let Some((display, power)) = self.display {
            crate::host::display::install(scope, natives, display, power);
        }
    }
}

thread_local! {
    static WEBS: RefCell<Webs> = RefCell::new(Webs::default());
    static UPSTREAM: RefCell<Option<(Sender<Msg>, Arc<bind::SendFunction>)>> = const { RefCell::new(None) };
    static ON_MESSAGE: RefCell<Option<v8::Global<v8::Function>>> = const { RefCell::new(None) };
}

fn call_with(scope: &mut v8::PinScope, function: &v8::Global<v8::Function>, bytes: &[u8]) {
    if let Some(value) = crate::host::host::deserialize_bytes(scope, bytes) {
        bind::call(scope, function, &[value]);
    }
}

struct Spec {
    name: String,
    init: Vec<u8>,
    heap_mb: f64,
    env: Vec<(String, String)>,
    grants: Grants,
}

impl Spec {
    const STACK: usize = 8 << 20;
    const START_WAIT: Duration = Duration::from_secs(10);
    const KILLED: i32 = 137;
    const PANICKED: i32 = 134;

    fn spawn(self, id: u32, on_message: Arc<bind::SendFunction>, on_exit: bind::SendFunction, broker: Sender<Msg>) -> Result<Handle, String> {
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let dead = Arc::new(AtomicBool::new(false));
        let name = self.name.clone();
        let platform = rt::current().platform();
        std::thread::Builder::new()
            .name("acde".into())
            .stack_size(Self::STACK)
            .spawn(move || {
                UPSTREAM.with(|u| *u.borrow_mut() = Some((broker.clone(), on_message.clone())));
                let (code, reason) = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run(dead.clone(), &ready, platform))) {
                    Ok(ended) => ended,
                    Err(panic) => {
                        let message =
                            panic.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
                        log::error!("[{name}] panic: {message}");
                        (Self::PANICKED, Some(format!("panic: {message}")))
                    }
                };
                UPSTREAM.with(|u| u.borrow_mut().take());
                let killed = dead.load(Ordering::SeqCst);
                let (code, reason) = if killed { (Self::KILLED, None) } else { (code, reason) };
                _ = broker.send(Msg::Task(Box::new(move |scope| {
                    let _on_message = on_message;
                    WEBS.with(|w| w.borrow_mut().live.remove(&id));
                    let code = v8::Integer::new(scope, code).into();
                    let reason: v8::Local<v8::Value> = match &reason {
                        Some(r) => bind::string(scope, r).into(),
                        None => v8::null(scope).into(),
                    };
                    bind::call(scope, &on_exit.0, &[code, reason]);
                    rt::current().keep_alive(false);
                })));
            })
            .map_err(|e| format!("spawnWeb: {e}"))?;
        started.recv_timeout(Self::START_WAIT).map_err(|_| "spawnWeb: the web thread did not start".to_owned())
    }

    fn run(self, killed: Arc<AtomicBool>, ready: &std::sync::mpsc::SyncSender<Handle>, platform: crate::host::platform::Shared) -> (i32, Option<String>) {
        let heap = (self.heap_mb > 0.0).then_some(self.heap_mb as usize);
        let mut runtime = rt::Runtime::new(heap, &self.name, platform);
        let isolate = runtime.isolate.thread_safe_handle();
        let reason = Arc::new(Mutex::new(None::<String>));
        let stopping = Arc::new(AtomicBool::new(false));
        crate::host::inspector::stop_when(stopping.clone());
        let guard = HeapGuard::install(&mut runtime, isolate.clone(), reason.clone(), stopping.clone());
        let tid = cfg_select! {
            target_os = "linux" => rustix::thread::gettid().as_raw_nonzero().get(),
            _ => 0,
        };
        _ = ready.send(Handle { tx: runtime.lp.sender(), isolate, stopping: stopping.clone(), killed, busy: runtime.lp.busy(), tid });
        let code = run_web(&mut runtime, self.init, &self.env, self.grants, &stopping);
        ON_MESSAGE.with(|m| *m.borrow_mut() = None);
        crate::host::webnative::clear();
        crate::host::backend::clear();
        crate::host::dom::clear_loads();
        crate::host::offload::clear();
        crate::host::idb::clear();
        crate::host::caches::clear();
        crate::host::storage::clear();
        crate::host::host::clear();
        crate::host::audio::clear();
        crate::host::capture::clear();
        crate::host::device::clear();
        crate::host::webgl::clear();
        #[cfg(target_os = "linux")]
        crate::host::webgpu::clear();
        #[cfg(target_os = "linux")]
        crate::host::bluetooth::clear();
        #[cfg(target_os = "linux")]
        crate::host::sound::clear();
        #[cfg(target_os = "linux")]
        crate::host::devices::clear();
        #[cfg(target_os = "linux")]
        crate::host::power::clear();
        crate::host::input::clear();
        #[cfg(target_os = "linux")]
        crate::host::display::clear();
        bind::clear_promises();
        drop(runtime);
        drop(guard);
        let reason = reason.lock().ok().and_then(|r| r.clone());
        (code, reason)
    }
}

#[cfg(target_os = "linux")]
pub fn restore_bluetooth(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let bluetooth = WEBS.with(|w| w.borrow().system.bluetooth.clone());
    if let Err(e) = std::thread::Builder::new().name("acde-bluetooth".into()).spawn(move || bluetooth.restore()) {
        bind::throw_error(scope, &format!("restoreBluetooth: {e}"));
    }
}

#[cfg(target_os = "linux")]
pub fn start_devices(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    WEBS.with(|w| w.borrow().system.devices.start());
}

#[cfg(target_os = "linux")]
pub fn open_office(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let opened = WEBS.with(|w| {
        let mut w = w.borrow_mut();
        let system = &mut w.system;
        if system.office.is_some() {
            return Ok(());
        }
        let workspace = system.workspace.get_or_insert_with(crate::runtime::Workspace::new).scoped(None, None);
        system.office = Some(crate::runtime::OfficeService::open(&workspace)?);
        Ok::<(), String>(())
    });
    match opened {
        Ok(()) => rv.set_null(),
        Err(message) => rv.set(bind::string(scope, &message).into()),
    }
}

#[cfg(target_os = "linux")]
pub fn close_office(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    if let Some(office) = WEBS.with(|w| w.borrow_mut().system.office.take()) {
        office.close();
    }
}

pub fn spawn_web(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let name = bind::arg_string(scope, &args, 0);
    let Some(init) = crate::host::host::serialize_value(scope, args.get(1)) else { return };
    let heap_mb = bind::arg_f64(scope, &args, 2);
    let env = bind::own_string_pairs(scope, args.get(3));
    let (Ok(on_message), Ok(on_exit)) = (v8::Local::<v8::Function>::try_from(args.get(4)), v8::Local::<v8::Function>::try_from(args.get(5))) else {
        return bind::throw_type_error(scope, "spawnWeb: onMessage and onExit must be functions");
    };
    let on_message = Arc::new(bind::SendFunction(v8::Global::new(scope, on_message)));
    let on_exit = bind::SendFunction(v8::Global::new(scope, on_exit));
    let broker = rt::current();
    let grants = WEBS.with(|w| w.borrow_mut().grants(scope, args.get(6)));
    let id = WEBS.with(|w| w.borrow_mut().live.next_id());
    let handle = match (Spec { name, init, heap_mb, env, grants }).spawn(id, on_message, on_exit, broker.sender()) {
        Ok(handle) => handle,
        Err(e) => return bind::throw_error(scope, &e),
    };
    broker.keep_alive(true);
    WEBS.with(|w| w.borrow_mut().live.insert(id, handle));
    let obj = v8::Object::new(scope);
    let data: v8::Local<v8::Value> = v8::Integer::new_from_unsigned(scope, id).into();
    crate::methods!(scope, obj, data = data;
        "post" => post_to_web,
        "terminate" => terminate_web,
        "busyMs" => busy_ms,
        "cpuMs" => cpu_ms,
    );
    rv.set(obj.into());
}

fn with_handle<R>(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, f: impl FnOnce(&Handle) -> R) -> Option<R> {
    let id = args.data().uint32_value(scope)?;
    WEBS.with(|w| w.borrow().live.get(&id).map(f))
}

fn post_to_web(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Some(bytes) = crate::host::host::serialize_value(scope, args.get(0)) else { return };
    with_handle(scope, &args, |h| {
        _ = h.tx.send(Msg::Task(Box::new(move |scope| {
            if let Some(function) = ON_MESSAGE.with(|m| m.borrow().clone()) {
                call_with(scope, &function, &bytes);
            }
        })));
    });
}

fn terminate_web(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    with_handle(scope, &args, |h| {
        h.killed.store(true, Ordering::SeqCst);
        h.stopping.store(true, Ordering::SeqCst);
        h.isolate.terminate_execution();
        _ = h.tx.send(Msg::Wake);
    });
}

fn busy_ms(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let ms = with_handle(scope, &args, |h| {
        let since = h.busy.load(Ordering::Relaxed);
        if since == 0 { 0 } else { rt::busy_stamp().saturating_sub(since).max(1) }
    })
    .unwrap_or(0);
    rv.set_double(ms as f64);
}

fn cpu_ms(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    rv.set_double(with_handle(scope, &args, Handle::cpu_ms).unwrap_or(0.0));
}

struct HeapGuard {
    handle: v8::IsolateHandle,
    reason: Arc<Mutex<Option<String>>>,
    stopping: Arc<AtomicBool>,
}

impl HeapGuard {
    fn install(runtime: &mut rt::Runtime, handle: v8::IsolateHandle, reason: Arc<Mutex<Option<String>>>, stopping: Arc<AtomicBool>) -> Box<Self> {
        let guard = Box::new(Self { handle, reason, stopping });
        let data = &*guard as *const Self as *mut std::ffi::c_void;
        runtime.isolate.add_near_heap_limit_callback(Self::near_limit, data);
        guard
    }

    extern "C" fn near_limit(data: *mut std::ffi::c_void, current: usize, _initial: usize) -> usize {
        let guard = unsafe { &*(data as *const Self) };
        if let Ok(mut reason) = guard.reason.lock() {
            reason.get_or_insert_with(|| "memory".to_owned());
        }
        guard.stopping.store(true, Ordering::SeqCst);
        guard.handle.terminate_execution();
        current + current / 2
    }
}

fn run_web(runtime: &mut rt::Runtime, init: Vec<u8>, env: &[(String, String)], grants: Grants, stopping: &Arc<AtomicBool>) -> i32 {
    crate::host::modules::set_loader(std::rc::Rc::new(crate::host::dom::WebLoader));
    let env = env.to_vec();
    runtime.task(move |scope| {
        let natives = v8::Object::new(scope);
        let host = crate::host::host::object(scope);
        bind::set(scope, natives, "host", host.into());
        let sys = crate::host::sys::object(scope, &[]);
        bind::set(scope, natives, "sys", sys.into());
        let create = v8::Function::new(scope, crate::host::dom::create_document).unwrap();
        bind::set(scope, natives, "createDocument", create.into());
        crate::host::webnative::install(scope, natives);
        grants.install(scope, natives);
        crate::host::webgl::install(scope, natives);
        #[cfg(target_os = "linux")]
        crate::host::webgpu::install(scope, natives);
        crate::host::audio::install(scope, natives);
        let port = v8::Object::new(scope);
        crate::methods!(scope, port,
            "post" => post_to_broker,
            "onMessage" => set_on_message,
        );
        bind::set(scope, natives, "port", port.into());
        if let Some(init) = crate::host::host::deserialize_bytes(scope, &init) {
            crate::host::backend::install(scope, natives, init);
            bind::set(scope, natives, "init", init);
        }
        let env: Vec<(&str, v8::Local<v8::Value>)> = env.iter().map(|(key, value)| (key.as_str(), bind::string(scope, value).into())).collect();
        let env = bind::object(scope, &env);
        bind::set(scope, natives, "env", env.into());
        if let Some(own) = v8::Function::builder(own_env).data(env.into()).build(scope) {
            bind::set(scope, sys, "env", own.into());
        }
        crate::host::sys::boot_module(scope, crate::host::bundle::PAGE_URL, natives.into());
    });
    runtime.lp.keep_alive(true);
    runtime.run_until(|| stopping.load(Ordering::SeqCst))
}

fn own_env(_: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    rv.set(args.data());
}

fn post_to_broker(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Some(bytes) = crate::host::host::serialize_value(scope, args.get(0)) else { return };
    let Some((broker, on_message)) = UPSTREAM.with(|u| u.borrow().clone()) else { return };
    _ = broker.send(Msg::Task(Box::new(move |scope| call_with(scope, &on_message.0, &bytes))));
}

fn set_on_message(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Ok(function) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        return bind::throw_type_error(scope, "port.onMessage: not a function");
    };
    let function = v8::Global::new(scope, function);
    ON_MESSAGE.with(|m| *m.borrow_mut() = Some(function));
}
