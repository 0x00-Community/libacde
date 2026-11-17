pub const PAGE_URL: &str = "acde-runtime:/page.js";
pub const LAUNCHER_URL: &str = "acde-runtime:/launcher.js";

const PAGE: &str = include_str!(concat!(env!("OUT_DIR"), "/page.js"));
const LAUNCHER: &str = include_str!(concat!(env!("OUT_DIR"), "/launcher.js"));

pub fn source(url: &str) -> Option<&'static str> {
    match url {
        PAGE_URL => Some(PAGE),
        LAUNCHER_URL => Some(LAUNCHER),
        _ => None,
    }
}

pub fn is_bundle(url: &str) -> bool {
    source(url).is_some()
}
