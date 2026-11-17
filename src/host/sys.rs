use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::Value;

use crate::host::bind::{self, IntoJs};
use crate::host::modules::Loader;
use crate::host::rt;

pub struct FileLoader;

const CONDITIONS: [&str; 4] = ["acde", "import", "module", "default"];

const BUILTINS: &[&str] = &[
    "assert",
    "buffer",
    "child_process",
    "crypto",
    "events",
    "fs",
    "http",
    "https",
    "module",
    "net",
    "os",
    "path",
    "process",
    "stream",
    "timers",
    "tls",
    "url",
    "util",
    "vm",
    "worker_threads",
    "zlib",
    "sqlite",
];

impl FileLoader {
    fn package(name: &str, subpath: &str, from: &Path) -> Result<PathBuf, String> {
        let mut dir = Some(from);
        while let Some(d) = dir {
            let root = d.join("node_modules").join(name);
            let manifest = root.join("package.json");
            if manifest.is_file() {
                let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
                let json: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", manifest.display()))?;
                if let Some(exports) = json.get("exports") {
                    let key = if subpath.is_empty() { ".".to_owned() } else { format!("./{subpath}") };
                    if let Some(target) = Self::exports(exports, &key) {
                        return Ok(root.join(target.trim_start_matches("./")));
                    }
                    return Err(format!("acdert: {name} does not export {key}"));
                }
                if subpath.is_empty() {
                    let main = json.get("module").or_else(|| json.get("main")).and_then(Value::as_str).unwrap_or("index.js");
                    return Self::file(&root.join(main));
                }
                return Self::file(&root.join(subpath));
            }
            dir = d.parent();
        }
        Err(format!("acdert: cannot find package {name} from {}", from.display()))
    }

    fn exports(exports: &Value, key: &str) -> Option<String> {
        let map = match exports {
            Value::Object(entries) if entries.keys().any(|k| k.starts_with('.')) => entries,
            _ => return if key == "." { Self::target(exports) } else { None },
        };
        if let Some(value) = map.get(key) {
            return Self::target(value);
        }
        let (head, tail, value) = map
            .iter()
            .filter_map(|(pattern, value)| {
                let (head, tail) = pattern.split_once('*')?;
                (key.starts_with(head) && key.ends_with(tail) && key.len() >= head.len() + tail.len()).then_some((head, tail, value))
            })
            .max_by_key(|(head, _, _)| head.len())?;
        let star = &key[head.len()..key.len() - tail.len()];
        Self::target(value).map(|t| t.replace('*', star))
    }

    fn target(value: &Value) -> Option<String> {
        match value {
            Value::String(s) => Some(s.clone()),
            Value::Array(items) => items.iter().find_map(Self::target),
            Value::Object(conditions) => CONDITIONS.iter().find_map(|c| conditions.get(*c)).and_then(Self::target),
            _ => None,
        }
    }

    fn file(path: &Path) -> Result<PathBuf, String> {
        if path.is_file() {
            return Ok(path.to_owned());
        }
        for ext in ["js", "mjs"] {
            let with = PathBuf::from(format!("{}.{ext}", path.display()));
            if with.is_file() {
                return Ok(with);
            }
        }
        let index = path.join("index.js");
        if index.is_file() {
            return Ok(index);
        }
        Err(format!("acdert: no module at {}", path.display()))
    }

    fn url(path: &Path) -> Result<String, String> {
        let real = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        url::Url::from_file_path(&real).map(String::from).map_err(|_| format!("acdert: {} is not absolute", real.display()))
    }
}

impl Loader for FileLoader {
    fn resolve(&self, specifier: &str, referrer: &str) -> Result<String, String> {
        let bare = specifier.strip_prefix("node:").unwrap_or(specifier);
        if specifier.starts_with("node:") || BUILTINS.contains(&bare.split('/').next().unwrap_or("")) {
            return Err(format!("acdert: '{specifier}' is Node's; acde has no Node (services get what they need from ServiceContext)"));
        }
        let base = url::Url::parse(referrer).map_err(|e| format!("acdert: referrer {referrer}: {e}"))?;
        if specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/') {
            let url = base.join(specifier).map_err(|e| format!("acdert: {specifier}: {e}"))?;
            let path = url.to_file_path().map_err(|_| format!("acdert: {url} is not a file"))?;
            return Self::url(&Self::file(&path)?);
        }
        if let Ok(url) = url::Url::parse(specifier) {
            return Ok(url.into());
        }
        let parts: Vec<&str> = specifier.splitn(if specifier.starts_with('@') { 3 } else { 2 }, '/').collect();
        let (name, subpath) = match specifier.starts_with('@') {
            true if parts.len() >= 2 => (format!("{}/{}", parts[0], parts[1]), parts.get(2).copied().unwrap_or("")),
            _ => (parts[0].to_owned(), parts.get(1).copied().unwrap_or("")),
        };
        let from = base.to_file_path().map_err(|_| format!("acdert: cannot import {specifier} from {referrer}"))?;
        let from = from.parent().unwrap_or(Path::new("/"));
        Self::url(&Self::package(&name, subpath, from)?)
    }

    fn load(&self, url: &str) -> Result<String, String> {
        if let Some(source) = crate::host::bundle::source(url) {
            return Ok(source.to_owned());
        }
        let path = url::Url::parse(url).ok().and_then(|u| u.to_file_path().ok()).ok_or_else(|| format!("acdert: {url} is not a file"))?;
        std::fs::read_to_string(&path).map_err(|e| format!("acdert: {}: {e}", path.display()))
    }
}

pub fn object<'s>(scope: &mut v8::PinScope<'s, '_>, argv: &[String]) -> v8::Local<'s, v8::Object> {
    let obj = v8::Object::new(scope);
    crate::methods!(scope, obj,
        "env" => env,
        "proxyEnv" => proxy_env,
        "exit" => exit,
        "cwd" => cwd,
        "homedir" => homedir,
        "languages" => languages,
        "readFile" => read_file,
        "readText" => read_text,
        "writeFile" => write_file,
        "exists" => exists,
        "stat" => stat,
        "readDir" => read_dir,
        "mkdir" => mkdir,
        "rename" => rename,
        "remove" => remove,
        "chmod" => chmod,
        "realpath" => realpath,
        "stdout" => stdout,
        "stderr" => stderr,
        "kill" => kill,
        "run" => run,
        "onSignal" => on_signal,
        "spawnWeb" => crate::host::web::spawn_web,
    );
    #[cfg(target_os = "linux")]
    crate::methods!(scope, obj,
        "openOffice" => crate::host::web::open_office,
        "restoreBluetooth" => crate::host::web::restore_bluetooth,
        "startDevices" => crate::host::web::start_devices,
        "closeOffice" => crate::host::web::close_office,
    );
    #[cfg(unix)]
    bind::method(scope, obj, "getuid", getuid);
    bind::method(scope, obj, "spawn", crate::host::child::spawn);
    #[cfg(windows)]
    bind::method(scope, obj, "spawnBackend", crate::host::backends::spawn);
    #[cfg(windows)]
    bind::method(scope, obj, "stopBackend", crate::host::backends::stop);
    #[cfg(windows)]
    bind::method(scope, obj, "contentType", content_type);
    bind::method(scope, obj, "write", crate::host::child::write);
    bind::method(scope, obj, "endInput", crate::host::child::end_input);
    let argv = argv.to_vec().js(scope);
    bind::set(scope, obj, "argv", argv);
    let platform = bind::string(
        scope,
        match std::env::consts::OS {
            "macos" => "darwin",
            "windows" => "win32",
            os => os,
        },
    );
    bind::set(scope, obj, "platform", platform.into());
    let pid = v8::Number::new(scope, f64::from(std::process::id()));
    bind::set(scope, obj, "pid", pid.into());
    obj
}

fn env(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let obj = v8::Object::new(scope);
    for (key, value) in std::env::vars_os() {
        let (Some(key), Some(value)) = (key.to_str(), value.to_str()) else { continue };
        let value = bind::string(scope, value);
        bind::set(scope, obj, key, value.into());
    }
    rv.set(obj.into());
}

fn proxy_env(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let obj = v8::Object::new(scope);
    for (key, value) in crate::runtime::backend_proxy_variables() {
        let value = bind::string(scope, &value);
        bind::set(scope, obj, &key, value.into());
    }
    rv.set(obj.into());
}

fn exit(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let code = args.get(0).int32_value(scope).unwrap_or(0);
    rt::current().exit(code);
    scope.terminate_execution();
}

fn cwd(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let dir = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_else(|_| "/".into());
    rv.set(bind::string(scope, &dir).into());
}

fn homedir(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let home = std::env::home_dir().map_or_else(|| "/".into(), |h| h.to_string_lossy().into_owned());
    rv.set(bind::string(scope, &home).into());
}

fn languages(scope: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let tags = crate::host::languages::preferred().js(scope);
    rv.set(tags);
}

#[cfg(windows)]
fn content_type(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let extension = bind::arg_string(scope, &args, 0);
    match crate::host::rt::current().platform().content_type(&extension) {
        Some(mime) => rv.set(bind::string(scope, &mime).into()),
        None => rv.set_null(),
    }
}

fn with_path(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, f: impl FnOnce(&mut v8::PinScope, &Path) -> std::io::Result<()>) {
    let path = bind::arg_string(scope, args, 0);
    if let Err(e) = f(scope, Path::new(&path)) {
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => "ENOENT",
            std::io::ErrorKind::PermissionDenied => "EACCES",
            std::io::ErrorKind::AlreadyExists => "EEXIST",
            std::io::ErrorKind::DirectoryNotEmpty => "ENOTEMPTY",
            std::io::ErrorKind::NotADirectory => "ENOTDIR",
            std::io::ErrorKind::IsADirectory => "EISDIR",
            _ => "EIO",
        };
        let error = crate::host::offload::failure(scope, &format!("{code}: {path}: {e}"), None, code);
        scope.throw_exception(error);
    }
}

fn read_file(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    with_path(scope, &args, |scope, path| {
        let bytes = std::fs::read(path)?;
        rv.set(bind::uint8_array(scope, bytes).into());
        Ok(())
    });
}

fn read_text(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    with_path(scope, &args, |scope, path| {
        let bytes = std::fs::read(path)?;
        rv.set(bind::string(scope, &String::from_utf8_lossy(&bytes)).into());
        Ok(())
    });
}

fn write_file(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    with_path(scope, &args, |scope, path| match bind::with_bytes(args.get(1), |data| std::fs::write(path, data)) {
        Some(written) => written,
        None => std::fs::write(path, bind::arg_string(scope, &args, 1)),
    });
}

fn exists(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let path = bind::arg_string(scope, &args, 0);
    rv.set_bool(Path::new(&path).exists());
}

fn stat(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let path = bind::arg_string(scope, &args, 0);
    let Ok(meta) = std::fs::metadata(&path) else {
        return rv.set_null();
    };
    let link = std::fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    let mode = cfg_select! {
        unix => {{
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode()
        }}
        _ => if meta.permissions().readonly() { 0o444 } else { 0o666 },
    };
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0);
    let stat = crate::js_object!(scope, {
        "isFile" => meta.is_file(),
        "isDirectory" => meta.is_dir(),
        "isSymbolicLink" => link,
        "size" => meta.len(),
        "mtimeMs" => mtime,
        "mode" => mode,
    });
    rv.set(stat.into());
}

fn read_dir(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    with_path(scope, &args, |scope, path| {
        let mut names: Vec<String> = std::fs::read_dir(path)?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        rv.set(names.js(scope));
        Ok(())
    });
}

fn mkdir(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let recursive = args.get(1).is_undefined() || args.get(1).boolean_value(scope);
    with_path(scope, &args, |_, path| match recursive {
        true => std::fs::create_dir_all(path),
        false => std::fs::create_dir(path),
    });
}

fn rename(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let to = bind::arg_string(scope, &args, 1);
    with_path(scope, &args, |_, path| std::fs::rename(path, &to));
}

fn remove(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let recursive = bind::arg_bool(scope, &args, 1);
    with_path(scope, &args, |_, path| {
        let result = match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => match recursive {
                true => std::fs::remove_dir_all(path),
                false => std::fs::remove_dir(path),
            },
            Ok(_) => std::fs::remove_file(path),
            Err(e) => Err(e),
        };
        match result {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    });
}

fn chmod(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let mode = bind::arg_f64(scope, &args, 1) as u32;
    with_path(scope, &args, |_, path| {
        cfg_select! {
            unix => {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            }
            _ => {
                let mut permissions = std::fs::metadata(path)?.permissions();
                permissions.set_readonly(mode & 0o200 == 0);
                std::fs::set_permissions(path, permissions)
            }
        }
    });
}

fn realpath(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    with_path(scope, &args, |scope, path| {
        let real = std::fs::canonicalize(path)?;
        rv.set(bind::string(scope, &real.display().to_string()).into());
        Ok(())
    });
}

fn stdout(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    use std::io::Write;
    let text = bind::arg_string(scope, &args, 0);
    _ = std::io::stdout().write_all(text.as_bytes());
    _ = std::io::stdout().flush();
}

fn stderr(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    use std::io::Write;
    let text = bind::arg_string(scope, &args, 0);
    _ = std::io::stderr().write_all(text.as_bytes());
}

#[cfg(windows)]
fn kill(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess};
    let raw = bind::arg_f64(scope, &args, 0) as i64;
    let test = bind::arg_string(scope, &args, 1) == "0";
    let pid = u32::try_from(raw.unsigned_abs()).unwrap_or(0);
    let access = if test { PROCESS_QUERY_LIMITED_INFORMATION } else { PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE };
    let handle = if pid == 0 || raw == -1 { std::ptr::null_mut() } else { unsafe { OpenProcess(access, 0, pid) } };
    if handle.is_null() {
        return rv.set_bool(false);
    }
    let mut code = 0;
    let alive = unsafe { GetExitCodeProcess(handle, &mut code) } != 0 && code == STILL_ACTIVE as u32;
    let sent = alive && (test || unsafe { TerminateProcess(handle, 1) } != 0);
    unsafe { CloseHandle(handle) };
    rv.set_bool(sent);
}

#[cfg(unix)]
fn kill(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    use rustix::process::{Pid, Signal, kill_process, kill_process_group, test_kill_process, test_kill_process_group};
    let raw = bind::arg_f64(scope, &args, 0) as i32;
    let signal = match bind::arg_string(scope, &args, 1).as_str() {
        "" | "SIGTERM" => Some(Signal::TERM),
        "SIGKILL" => Some(Signal::KILL),
        "SIGINT" => Some(Signal::INT),
        "SIGHUP" => Some(Signal::HUP),
        "0" => None,
        other => Some(other.parse().ok().and_then(Signal::from_named_raw).unwrap_or(Signal::TERM)),
    };
    let target = raw.checked_abs().and_then(Pid::from_raw).filter(|_| raw != -1);
    let sent = match (target, signal) {
        (None, _) => false,
        (Some(pid), Some(signal)) if raw > 0 => kill_process(pid, signal).is_ok(),
        (Some(pid), None) if raw > 0 => test_kill_process(pid).is_ok(),
        (Some(group), Some(signal)) => kill_process_group(group, signal).is_ok(),
        (Some(group), None) => test_kill_process_group(group).is_ok(),
    };
    rv.set_bool(sent);
}

fn run(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    let Some(argv) = bind::strings(scope, args.get(0)) else {
        return bind::throw_type_error(scope, "run: argv is not an array of strings");
    };
    let timeout = bind::arg_f64(scope, &args, 1);
    let Some((program, rest)) = argv.split_first() else {
        return bind::throw_type_error(scope, "run: empty argv");
    };
    let child = std::process::Command::new(program)
        .args(rest)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let (status, stdout, stderr) = match child {
        Ok(child) => wait_output(child, timeout),
        Err(e) => (None, String::new(), e.to_string()),
    };
    rv.set(crate::js_object!(scope, {"status" => status, "stdout" => stdout, "stderr" => stderr}).into());
}

fn wait_output(mut child: std::process::Child, timeout_ms: f64) -> (Option<i32>, String, String) {
    use std::io::Read;
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let reader = std::thread::spawn(move || {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        if let Some(out) = out.as_mut() {
            _ = out.read_to_end(&mut o);
        }
        if let Some(err) = err.as_mut() {
            _ = err.read_to_end(&mut e);
        }
        (o, e)
    });
    let deadline = (timeout_ms > 0.0).then(|| std::time::Instant::now() + std::time::Duration::from_secs_f64(timeout_ms / 1000.0));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if deadline.is_some_and(|d| std::time::Instant::now() > d) => {
                _ = child.kill();
                _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
            Err(_) => break None,
        }
    };
    let (o, e) = reader.join().unwrap_or_default();
    (status, String::from_utf8_lossy(&o).into_owned(), String::from_utf8_lossy(&e).into_owned())
}

#[cfg(unix)]
fn getuid(_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue) {
    rv.set_uint32(rustix::process::getuid().as_raw());
}

fn on_signal(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        return bind::throw_type_error(scope, "onSignal: not a function");
    };
    let callback = v8::Global::new(scope, callback);
    signals::listen(rt::current().sender(), callback);
}

fn signalled(callback: std::sync::Arc<bind::SendFunction>, name: &'static str) -> rt::Msg {
    rt::Msg::Task(Box::new(move |scope| {
        let name = bind::string(scope, name);
        bind::call(scope, &callback.0, &[name.into()]);
    }))
}

#[cfg(unix)]
mod signals {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;

    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    use crate::host::rt::Msg;

    static LISTENING: AtomicBool = AtomicBool::new(false);

    pub fn listen(loop_tx: Sender<Msg>, callback: v8::Global<v8::Function>) {
        if LISTENING.swap(true, Ordering::AcqRel) {
            return;
        }
        let Ok(mut signals) = Signals::new([SIGINT, SIGTERM, SIGHUP]) else {
            return;
        };
        let callback = std::sync::Arc::new(crate::host::bind::SendFunction(callback));
        _ = std::thread::Builder::new().name("acde-signals".into()).spawn(move || {
            for signal in signals.forever() {
                let name = match signal {
                    SIGINT => "SIGINT",
                    SIGHUP => "SIGHUP",
                    _ => "SIGTERM",
                };
                crate::host::inspector::release_pauses();
                _ = loop_tx.send(super::signalled(callback.clone(), name));
            }
        });
    }
}

#[cfg(windows)]
mod signals {
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex};

    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, SetConsoleCtrlHandler};

    use crate::host::rt::Msg;

    static LISTENER: Mutex<Option<(Sender<Msg>, Arc<crate::host::bind::SendFunction>)>> = Mutex::new(None);

    pub fn listen(loop_tx: Sender<Msg>, callback: v8::Global<v8::Function>) {
        let mut listener = LISTENER.lock().unwrap_or_else(|e| e.into_inner());
        if listener.is_some() {
            return;
        }
        *listener = Some((loop_tx, Arc::new(crate::host::bind::SendFunction(callback))));
        unsafe { SetConsoleCtrlHandler(Some(handler), 1) };
    }

    unsafe extern "system" fn handler(event: u32) -> i32 {
        let name = match event {
            CTRL_C_EVENT => "SIGINT",
            CTRL_BREAK_EVENT => "SIGBREAK",
            CTRL_CLOSE_EVENT => "SIGHUP",
            _ => return 0,
        };
        let Some((loop_tx, callback)) = LISTENER.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
            return 0;
        };
        crate::host::inspector::release_pauses();
        i32::from(loop_tx.send(super::signalled(callback, name)).is_ok())
    }
}

pub enum Entry {
    Script(String),
    Broker,
}

pub fn start(entry: Entry, args: &[String], platform: crate::host::platform::Shared) -> i32 {
    let (url, globals) = match entry {
        Entry::Script(file) => match std::fs::canonicalize(&file) {
            Ok(path) => (url::Url::from_file_path(path).map(String::from).unwrap_or_default(), true),
            Err(e) => {
                log::error!("{file}: {e}");
                return 1;
            }
        },
        Entry::Broker => (crate::host::bundle::LAUNCHER_URL.to_owned(), false),
    };
    let mut runtime = rt::Runtime::new(None, "broker", platform);
    crate::host::modules::set_loader(Rc::new(FileLoader));
    runtime.task(|scope| {
        let mut fields: Vec<(&str, v8::Local<v8::Value>)> = vec![("host", crate::host::host::object(scope).into()), ("sys", object(scope, args).into())];
        let napi = crate::host::napi::exports(scope);
        let native = crate::host::rt::current().platform().exports(scope, napi);
        fields.push(("native", native));
        if !globals {
            let natives = bind::object(scope, &fields);
            return boot_module(scope, &url, natives.into());
        }
        let global = scope.get_current_context().global(scope);
        for (name, value) in fields {
            bind::set(scope, global, &format!("acde_{name}"), value);
        }
        crate::host::modules::import(scope, &url);
    });
    runtime.run_until(|| false)
}

pub fn boot_module(scope: &mut v8::PinScope, url: &str, natives: v8::Local<v8::Value>) {
    let Some(promise) = crate::host::modules::import(scope, url) else { return };
    let boot = v8::Function::builder(boot_entry).data(natives).build(scope);
    if let Some(boot) = boot {
        promise.then(scope, boot);
    }
}

fn boot_entry<'s>(scope: &mut v8::PinScope<'s, '_>, args: v8::FunctionCallbackArguments<'s>, mut rv: v8::ReturnValue) {
    let Ok(namespace) = v8::Local::<v8::Object>::try_from(args.get(0)) else { return };
    let entry = bind::get(scope, namespace, "default");
    let Some(Ok(entry)) = entry.map(v8::Local::<v8::Function>::try_from) else {
        return bind::throw_type_error(scope, "acdert: the bundle has no default export function");
    };
    if let Some(result) = bind::call(scope, entry, &[args.data()]) {
        rv.set(result);
    }
}
