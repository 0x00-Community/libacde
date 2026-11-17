mod audio;
mod backend;
#[cfg(windows)]
mod backends;
mod bind;
#[cfg(target_os = "linux")]
mod bluetooth;
mod bundle;
mod caches;
mod capture;
mod child;
mod device;
#[cfg(target_os = "linux")]
mod devices;
#[cfg(target_os = "linux")]
mod display;
mod dom;
mod encoding;
mod engine;
mod fs;
mod host;
mod idb;
mod input;
mod inspector;
mod languages;
mod modules;
mod napi;
#[cfg(target_os = "linux")]
mod network;
#[cfg(target_os = "linux")]
mod office;
mod offload;
mod picker;
mod platform;
#[cfg(target_os = "linux")]
mod power;
mod profile;
mod rt;
#[cfg(target_os = "linux")]
mod sound;
mod storage;
mod surface;
mod sys;
#[cfg(target_os = "linux")]
mod system;
mod terminal;
mod web;
mod webgl;
#[cfg(target_os = "linux")]
mod webgpu;
mod webnative;

pub use bind::*;
pub use offload::{failure, run as offload};
pub use platform::{Display, Headless, Input, InputSink, Platform, Portal, Shared, Window};
pub use rt::{Msg, current};
pub use sys::Entry;
pub use web::app_dir;
pub use webnative::event_object;

pub const API_LEVEL: u32 = api_level(include_str!("../../../npm/acde-api/api-level"));

const fn api_level(source: &str) -> u32 {
    let bytes = source.as_bytes();
    let (mut i, mut level, mut digits) = (0, 0u32, 0);
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            level = level * 10 + (bytes[i] - b'0') as u32;
            digits += 1;
        } else if digits > 0 {
            break;
        }
        i += 1;
    }
    assert!(level > 0, "npm/acde-api/api-level holds no positive contract level");
    level
}

pub fn version(program: &str) -> String {
    format!("{program} {} (contract level {API_LEVEL})", env!("CARGO_PKG_VERSION"))
}

pub fn start(entry: Entry, args: &[String], platform: Shared) -> i32 {
    sys::start(entry, args, platform)
}

pub fn common(args: &[String], platform: &Shared) -> Option<i32> {
    Some(match args.get(1).map(String::as_str) {
        Some("run") if args.len() > 2 => start(Entry::Script(args[2].clone()), &args[3..], platform.clone()),
        Some("desktops" | "sessions" | "applications" | "open") => start(Entry::Broker, &args[1..], platform.clone()),
        _ => return None,
    })
}

pub fn exit(code: i32) -> ! {
    child::stop_all();
    crate::storage::flush_all();
    crate::log::flush();
    std::process::exit(code);
}
