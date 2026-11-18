//! The user's Internet settings (Settings > Network > Proxy), read per request through WinHTTP: "Automatically detect
//! settings" (WPAD) and "Use setup script" ask `WinHttpGetProxyForUrl` for this URL; when they give no answer, the manual proxy
//! and its bypass list apply. Runs on the `acde-proxy` thread: WPAD and the script may take seconds.

use super::{Hop, rules};
use std::ffi::c_void;
use std::ptr::null;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use url::Url;
use windows_sys::Win32::Foundation::{GetLastError, GlobalFree};
use windows_sys::Win32::Networking::WinHttp::*;

/// after WPAD found nothing, it is not asked again for this long (each try may wait for DHCP and DNS)
const DETECT_AGAIN: Duration = Duration::from_secs(300);

static DETECTION_FAILED: Mutex<Option<Instant>> = Mutex::new(None);

pub(super) fn find(url: &Url) -> Vec<Hop> {
    let mut config = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) } == 0 {
        log::debug!("proxy: cannot read the Internet settings (error {})", unsafe { GetLastError() });
        return vec![Hop::Direct];
    }
    let detect = config.fAutoDetect != 0;
    let script = take(config.lpszAutoConfigUrl);
    let proxy = take(config.lpszProxy);
    let bypass = take(config.lpszProxyBypass);
    if let Some(hops) = automatic(url, detect, script.as_deref()) {
        return hops;
    }
    named(url, proxy.as_deref(), bypass.as_deref())
}

pub(super) fn manual() -> Option<super::Manual> {
    let mut config = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) } == 0 {
        log::debug!("proxy: cannot read the Internet settings (error {})", unsafe { GetLastError() });
        return None;
    }
    let script = take(config.lpszAutoConfigUrl);
    let proxy = take(config.lpszProxy);
    let bypass = take(config.lpszProxyBypass);
    if let Some(script) = script {
        log::debug!("proxy: app backends get no proxy variables from the setup script {script}");
        return None;
    }
    Some(rules::windows_manual(&proxy?, bypass.as_deref().unwrap_or_default()))
}

fn named(url: &Url, proxy: Option<&str>, bypass: Option<&str>) -> Vec<Hop> {
    match proxy {
        Some(proxy) if !bypass.is_some_and(|bypass| rules::windows_bypass(bypass, url)) => rules::windows_list(proxy, url.scheme() == "https"),
        _ => vec![Hop::Direct],
    }
}

/// WPAD and the setup script; `None` when neither is on or neither answered
fn automatic(url: &Url, detect: bool, script: Option<&str>) -> Option<Vec<Hop>> {
    let detect = detect && DETECTION_FAILED.lock().map_or(true, |failed| failed.is_none_or(|at| at.elapsed() >= DETECT_AGAIN));
    if !detect && script.is_none() {
        return None;
    }
    let session = session()?;
    let script: Option<Vec<u16>> = script.map(wide);
    let target = wide(url.as_str());
    let mut options = WINHTTP_AUTOPROXY_OPTIONS {
        dwFlags: (if detect { WINHTTP_AUTOPROXY_AUTO_DETECT } else { 0 }) | (if script.is_some() { WINHTTP_AUTOPROXY_CONFIG_URL } else { 0 }),
        dwAutoDetectFlags: if detect { WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A } else { 0 },
        lpszAutoConfigUrl: script.as_ref().map_or(null(), |script| script.as_ptr()),
        fAutoLogonIfChallenged: 1,
        ..Default::default()
    };
    let mut info = WINHTTP_PROXY_INFO::default();
    if unsafe { WinHttpGetProxyForUrl(session, target.as_ptr(), &mut options, &mut info) } == 0 {
        let error = unsafe { GetLastError() };
        if detect && error == ERROR_WINHTTP_AUTODETECTION_FAILED {
            DETECTION_FAILED.lock().map(|mut failed| *failed = Some(Instant::now())).ok();
        }
        log::debug!("proxy: no automatic proxy for {url} (WinHTTP error {error})");
        return None;
    }
    let proxy = take(info.lpszProxy);
    let bypass = take(info.lpszProxyBypass);
    if info.dwAccessType != WINHTTP_ACCESS_TYPE_NAMED_PROXY {
        return Some(vec![Hop::Direct]);
    }
    Some(named(url, proxy.as_deref(), bypass.as_deref()))
}

/// one WinHTTP session for the scripts (WinHTTP caches a downloaded script per session); handles are thread safe
fn session() -> Option<*mut c_void> {
    static SESSION: OnceLock<usize> = OnceLock::new();
    let session = *SESSION.get_or_init(|| {
        let agent = wide("acde");
        unsafe { WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_NO_PROXY, null(), null(), 0) as usize }
    });
    (session != 0).then_some(session as *mut c_void)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// a string WinHTTP allocated with `GlobalAlloc`, freed here
fn take(text: *mut u16) -> Option<String> {
    if text.is_null() {
        return None;
    }
    let length = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    let owned = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
    unsafe { GlobalFree(text.cast()) };
    Some(owned).filter(|text| !text.trim().is_empty())
}
