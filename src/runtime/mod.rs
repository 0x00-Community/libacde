#[cfg(target_os = "linux")]
mod atspi;
#[cfg(target_os = "linux")]
mod battery;
#[cfg(target_os = "linux")]
mod bluetooth;
#[cfg(target_os = "linux")]
mod bus;
pub mod channel;
#[cfg(windows)]
mod conpty;
#[cfg(target_os = "linux")]
mod devices;
pub mod devtools;
mod failure;
#[cfg(target_os = "linux")]
mod fcitx;
mod fetch;
pub(crate) mod fs;
#[cfg(target_os = "linux")]
mod greetd;
mod http;
#[cfg(target_os = "linux")]
mod idle;
#[cfg(target_os = "linux")]
mod ime;
mod js;
#[cfg(target_os = "linux")]
mod network;
#[cfg(target_os = "linux")]
mod office;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
pub mod power;
mod proxy;
#[cfg(unix)]
mod pty;
mod rt;
#[cfg(target_os = "linux")]
mod sound;
mod sync;
#[cfg(target_os = "linux")]
mod system;
mod terminal;
#[cfg(target_os = "linux")]
mod voice;
#[cfg(target_os = "linux")]
mod wakelock;
#[cfg(target_os = "linux")]
mod xkb;
#[cfg(target_os = "linux")]
pub use atspi::Accessibility;
#[cfg(target_os = "linux")]
pub use battery::Battery;
#[cfg(target_os = "linux")]
pub use bluetooth::{Bluetooths, BtCall};
#[cfg(target_os = "linux")]
pub use bus::DesktopBus;
#[cfg(target_os = "linux")]
pub use devices::{DevCall, Devices, Topic};
pub use devtools::{GUEST_KEY, dev_mode, managed};
#[cfg(target_os = "linux")]
pub use fcitx::{Fcitx, ImeController, ImeEntry, ImeGroup, ime_apply, ime_catalog, ime_state};
pub use fetch::{Deliver, Dial, Event as FetchEvent, FetchClient};
#[cfg(unix)]
pub use fs::hand_fds;
pub use fs::{EntryKind, Files, Finder, FsCall, FsError, FsReply, LentFile, LentRoot, MAX_CHUNK, Opened, PickedFolders, Range, Workspace};
#[cfg(target_os = "linux")]
pub use greetd::{GreetUser, Greetd};
#[cfg(target_os = "linux")]
pub use idle::Idle;
#[cfg(target_os = "linux")]
pub use ime::{Ime, ImeCapability};
#[cfg(target_os = "linux")]
pub use network::{NetCall, Networks};
#[cfg(target_os = "linux")]
pub use office::{OfficeCall, OfficeService};
#[cfg(target_os = "linux")]
pub use portal::{Appearance, Filter as PortalFilter, choose_folder, open_files, save_file};
#[cfg(target_os = "linux")]
pub use power::{Power, PowerCall, backlight_brightness};
pub use proxy::backend_variables as backend_proxy_variables;
#[cfg(target_os = "linux")]
pub(crate) use sound::pw_context;
#[cfg(target_os = "linux")]
pub use sound::{SoundCall, Sounds};
#[cfg(target_os = "linux")]
pub use system::{SysCall, system_call};
pub use terminal::Terminals;
#[cfg(target_os = "linux")]
pub use voice::Voice;
#[cfg(target_os = "linux")]
pub use wakelock::WakeLock;
#[cfg(target_os = "linux")]
pub use xkb::XkbCatalog;

use napi::{Error, Result, Status};
use serde_json::json;

#[cfg(target_os = "linux")]
pub fn bad_request(message: impl std::fmt::Display) -> String {
    json!({ "code": "bad_request", "message": message.to_string() }).to_string()
}

fn failure(code: &str, message: &str) -> Error {
    Error::new(Status::GenericFailure, json!({"code":code,"message":message}).to_string())
}

#[cfg(target_os = "linux")]
fn unix_now() -> std::time::Duration {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
}

fn state_file(name: &str) -> Option<std::path::PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".local/state")))?;
    Some(state.join("acde").join(name))
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    rt::blocking(work).await.map_err(|_| failure("native_task_failed", "native task did not complete; query the original operation"))?
}

#[cfg(all(test, target_os = "linux"))]
fn private_bus(name: &str) -> Option<(std::process::Child, String, std::path::PathBuf)> {
    use std::io::BufRead;
    static BUSES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = BUSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("acde-{name}-test-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let config = dir.join("bus.conf");
    let policy = "<policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy>";
    std::fs::write(&config, format!("<busconfig><type>session</type><listen>unix:dir={}</listen><auth>EXTERNAL</auth>{policy}</busconfig>", dir.display()))
        .ok()?;
    let mut daemon = std::process::Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .arg("--print-address")
        .arg("--nofork")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    let mut address = String::new();
    std::io::BufReader::new(daemon.stdout.take()?).read_line(&mut address).ok()?;
    let address = address.trim().to_string();
    if address.is_empty() {
        _ = daemon.kill();
        panic!("dbus-daemon printed no address: {:?}", daemon.wait());
    }
    Some((daemon, address, dir))
}
