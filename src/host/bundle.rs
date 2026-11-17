use std::borrow::Cow;

pub const PAGE_URL: &str = "acde-runtime:/page.js";
pub const LAUNCHER_URL: &str = "acde-runtime:/launcher.js";

const PAGE: &str = include_str!(concat!(env!("OUT_DIR"), "/page.js"));
const LAUNCHER: &str = include_str!(concat!(env!("OUT_DIR"), "/launcher.js"));

fn file_name(url: &str) -> Option<&'static str> {
    match url {
        PAGE_URL => Some("page.js"),
        LAUNCHER_URL => Some("launcher.js"),
        _ => None,
    }
}

fn embedded(name: &str) -> &'static str {
    if name == "page.js" { PAGE } else { LAUNCHER }
}

#[cfg(debug_assertions)]
fn from_dir(name: &str) -> Option<String> {
    let dir = std::env::var_os("ACDEV_JS_DIR")?;
    let path = std::path::Path::new(&dir).join(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) => {
            log::warn!("ACDEV_JS_DIR: {}: {e}; using the built-in {name}", path.display());
            None
        }
    }
}

#[cfg(not(debug_assertions))]
fn from_dir(_: &str) -> Option<String> {
    None
}

pub fn source(url: &str) -> Option<Cow<'static, str>> {
    let name = file_name(url)?;
    Some(from_dir(name).map_or(Cow::Borrowed(embedded(name)), Cow::Owned))
}

pub fn is_bundle(url: &str) -> bool {
    file_name(url).is_some()
}
