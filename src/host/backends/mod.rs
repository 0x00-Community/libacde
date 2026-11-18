mod dir;
mod process;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Once};

use crate::runtime::channel::{self, Mux, Route};

use crate::host::bind::{self, IntoJs};
use crate::host::rt::{self, Msg};
use process::{Process, Spec};

const TRANSPORT: &[&str] = &["ACDE_APP_CHANNEL", "ACDE_APP_SOCKETS", "ACDE_APP_CONTROL"];
const SYSTEM: &[&str] = &["SystemRoot", "SystemDrive", "windir", "TEMP", "TMP", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "USERNAME", "PATHEXT", "ComSpec"];
const CHANNEL_MARK: &[u8] = b"acde-listen-channel/1";
const OLD_PRELOAD: &str = "acde-listen.js does not support the Windows channel (built by an older acde: acde build it again)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Channel,
    Sockets,
}

struct Running {
    app: String,
    generation: u64,
    process: Process,
    mux: Arc<Mux>,
    sockets: Option<PathBuf>,
}

static RUNNING: LazyLock<Mutex<HashMap<u32, Arc<Running>>>> = LazyLock::new(Mutex::default);
static FINISHING: Mutex<()> = Mutex::new(());

thread_local! {
    static EXITS: RefCell<HashMap<u32, v8::Global<v8::Function>>> = RefCell::default();
}

fn running() -> MutexGuard<'static, HashMap<u32, Arc<Running>>> {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner())
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn environment(given: Vec<(String, String)>, transport: &[(&str, String)], system: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = given.into_iter().filter(|(key, _)| !TRANSPORT.iter().any(|t| same(t, key))).collect();
    env.extend(transport.iter().map(|(key, value)| (key.to_string(), value.clone())));
    for key in SYSTEM {
        if !env.iter().any(|(have, _)| same(have, key))
            && let Some(value) = system(key)
        {
            env.push((key.to_string(), value));
        }
    }
    env
}

fn supports_channel(preload: &std::path::Path) -> bool {
    std::fs::read(preload).is_ok_and(|text| text.windows(CHANNEL_MARK.len()).any(|w| w == CHANNEL_MARK))
}

fn finish(pid: u32) {
    let _finishing = FINISHING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(running) = running().remove(&pid) else { return };
    channel::unregister(&running.app, running.generation);
    running.mux.close("the backend has exited");
    running.process.terminate();
    if let Some(dir) = &running.sockets {
        remove(dir);
    }
}

fn remove(dir: &std::path::Path) {
    for _ in 0..20 {
        match std::fs::remove_dir_all(dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => std::thread::sleep(std::time::Duration::from_millis(50)),
            _ => return,
        }
    }
    log::warn!("cannot remove the backend socket directory {}", dir.display());
}

fn generation() -> Result<PathBuf, String> {
    static SWEPT: Once = Once::new();
    let base = dir::base(std::env::var_os("XDG_RUNTIME_DIR").as_deref(), &std::env::temp_dir());
    std::fs::create_dir_all(&base).map_err(|e| format!("cannot create {}: {e}", base.display()))?;
    SWEPT.call_once(|| {
        for left in dir::sweep(&base, dir::alive) {
            log::info!("removed {}, left by a runtime that is gone", left.display());
        }
    });
    let dir = base.join(dir::generation_name());
    if !dir::fits(&dir) {
        return Err(format!("{} is too long for a socket path ({} bytes at most)", dir.join("65535.sock").display(), dir::SUN_PATH - 1));
    }
    let sid = dir::user_sid().map_err(|e| e.to_string())?;
    dir::create_protected(&dir, &dir::sddl(&sid)).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn spawn(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let argv = bind::strings(scope, args.get(0)).unwrap_or_default();
    let Some((program, rest)) = argv.split_first() else {
        return bind::throw_type_error(scope, "spawnBackend: argv is not a non-empty array of strings");
    };
    let Ok(options) = v8::Local::<v8::Object>::try_from(args.get(1)) else {
        return bind::throw_type_error(scope, "spawnBackend: options is not an object");
    };
    let Ok(on_exit) = v8::Local::<v8::Function>::try_from(args.get(2)) else {
        return bind::throw_type_error(scope, "spawnBackend: onExit is not a function");
    };
    let Some(app) = bind::get_string(scope, options, "app").filter(|app| !app.is_empty()) else {
        return bind::throw_type_error(scope, "spawnBackend: options.app is missing");
    };
    let mode = match bind::get_string(scope, options, "mode").as_deref() {
        Some("channel") => Mode::Channel,
        Some("sockets") => Mode::Sockets,
        _ => return bind::throw_type_error(scope, "spawnBackend: options.mode is 'channel' or 'sockets'"),
    };
    let cwd = bind::get_string(scope, options, "cwd");
    let prefix = bind::get_string(scope, options, "prefix").unwrap_or_default();
    let given = bind::get(scope, options, "env").map(|env| bind::own_string_pairs(scope, env)).unwrap_or_default();
    if mode == Mode::Channel {
        let preload = bind::get_string(scope, options, "preload").unwrap_or_default();
        if !supports_channel(std::path::Path::new(&preload)) {
            return bind::throw_error(scope, OLD_PRELOAD);
        }
    }
    let sockets = match mode {
        Mode::Channel => None,
        Mode::Sockets => match generation() {
            Ok(dir) => Some(dir),
            Err(e) => return bind::throw_error(scope, &e),
        },
    };
    let transport = match &sockets {
        None => vec![("ACDE_APP_CHANNEL", "fd".to_owned())],
        Some(dir) => vec![("ACDE_APP_SOCKETS", dir.to_string_lossy().into_owned()), ("ACDE_APP_CONTROL", "fd".to_owned())],
    };
    let env = environment(given, &transport, |key| std::env::var(key).ok());
    let started = process::start(&Spec { program, args: rest, cwd: cwd.as_deref(), env: &env });
    let started = match started {
        Ok(started) => started,
        Err(e) => {
            if let Some(dir) = &sockets {
                remove(dir);
            }
            return bind::throw_error(scope, &format!("spawn {program}: {e}"));
        }
    };
    let pid = started.process.pid;
    let mux = match Mux::start(started.to_backend, started.from_backend) {
        Ok(mux) => mux,
        Err(e) => {
            started.process.terminate();
            if let Some(dir) = &sockets {
                remove(dir);
            }
            return bind::throw_error(scope, &format!("spawn {program}: {e}"));
        }
    };
    let route = match &sockets {
        None => Route::Channel(mux.clone()),
        Some(dir) => Route::Sockets(dir.clone()),
    };
    let generation = channel::register(&app, route);
    _ = crate::host::child::each_line(started.log, move |line| log::info!("{prefix}{line}"));
    let backend = Arc::new(Running { app, generation, process: started.process, mux, sockets });
    running().insert(pid, backend.clone());
    let tx = rt::current().sender();
    let waiter = std::thread::Builder::new().name("acde-backend-wait".into()).spawn(move || {
        let code = backend.process.wait(None);
        drop(backend);
        finish(pid);
        _ = tx.send(Msg::Task(Box::new(move |scope| exited(scope, pid, code))));
    });
    if let Err(e) = waiter {
        finish(pid);
        return bind::throw_error(scope, &format!("spawn {program}: {e}"));
    }
    let on_exit = v8::Global::new(scope, on_exit);
    EXITS.with(|exits| exits.borrow_mut().insert(pid, on_exit));
    rt::current().keep_alive(true);
    rv.set_uint32(pid);
}

fn exited(scope: &mut v8::PinScope, pid: u32, code: Option<i32>) {
    let Some(on_exit) = EXITS.with(|exits| exits.borrow_mut().remove(&pid)) else { return };
    rt::current().keep_alive(false);
    let (code, signal) = (code.js(scope), None::<i32>.js(scope));
    bind::call(scope, on_exit, &[code, signal]);
}

fn stop_running(running: Arc<Running>, ms: u32) -> std::io::Result<()> {
    _ = running.mux.shutdown();
    std::thread::Builder::new().name("acde-backend-stop".into()).spawn(move || {
        if running.process.wait(Some(ms)).is_none() {
            running.process.terminate();
        }
    })?;
    Ok(())
}

pub fn stop(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let pid = bind::arg_u32(scope, &args, 0);
    let ms = bind::arg_u32(scope, &args, 1);
    let Some(backend) = running().get(&pid).cloned() else { return rv.set_bool(false) };
    if let Err(e) = stop_running(backend, ms) {
        return bind::throw_error(scope, &format!("stopBackend {pid}: {e}"));
    }
    rv.set_bool(true);
}

pub fn stop_all() {
    let left: Vec<(u32, Arc<Running>)> = running().iter().map(|(pid, backend)| (*pid, backend.clone())).collect();
    for (pid, backend) in left {
        backend.process.terminate();
        _ = backend.process.wait(Some(2000));
        finish(pid);
    }
    drop(FINISHING.lock().unwrap_or_else(|e| e.into_inner()));
}
