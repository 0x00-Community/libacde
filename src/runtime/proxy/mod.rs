//! Which proxy a page request goes through, and the tunnel through it.
//!
//! The contract follows Linux: `http_proxy` / `https_proxy` / `all_proxy` / `no_proxy` (either case) decide when any proxy
//! variable is set, on every platform. Without them each platform asks its own settings: Windows the user's Internet settings
//! (WinHTTP, with the automatic configuration script and WPAD), macOS the system configuration (CFNetwork, with PAC); Linux has
//! no further source. Loopback hosts and `no_proxy` hosts always go direct.

use futures_channel::oneshot;
use std::sync::{Arc, OnceLock, mpsc};
use url::Url;

mod rules;
mod tunnel;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub(crate) use tunnel::{Tunnel, open};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    /// a DNS name or an IP address, without brackets
    pub(crate) host: String,
    pub(crate) port: u16,
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.host.contains(':') { write!(f, "[{}]:{}", self.host, self.port) } else { write!(f, "{}:{}", self.host, self.port) }
    }
}

/// One way to reach a host. A route is a list of them, tried in order (as a PAC answer `PROXY a; PROXY b`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Hop {
    Direct,
    /// an HTTP proxy, reached with `CONNECT` for every scheme
    Http(Endpoint),
    /// SOCKS4a (Windows' `socks=`)
    Socks4(Endpoint),
    Socks5(Endpoint),
}

impl std::fmt::Display for Hop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct => f.write_str("DIRECT"),
            Self::Http(at) => write!(f, "http://{at}"),
            Self::Socks4(at) => write!(f, "socks4://{at}"),
            Self::Socks5(at) => write!(f, "socks5://{at}"),
        }
    }
}

/// Where proxies come from. The system's in the program; tests give their own and read no real setting.
pub(crate) trait Proxies: Send + Sync + 'static {
    /// The hops for an `http` or `https` URL (`ws` / `wss` are asked as `http` / `https`), never empty.
    fn find(&self, url: &Url) -> Vec<Hop>;

    /// `find` may block (a PAC script, WPAD), so it runs on the `acde-proxy` thread, not on the I/O thread.
    fn blocking(&self) -> bool {
        false
    }
}

pub(crate) type Shared = Arc<dyn Proxies>;

/// The process' proxies: the environment read once at the first request, else the platform's settings, read per request.
pub(crate) fn system() -> Shared {
    static SYSTEM: OnceLock<Shared> = OnceLock::new();
    SYSTEM.get_or_init(|| Arc::new(System::read(|name| std::env::var(name).ok()))).clone()
}

struct System {
    env: Option<rules::Env>,
    no_proxy: rules::NoProxy,
}

impl System {
    fn read(var: impl Fn(&str) -> Option<String>) -> Self {
        let (env, no_proxy) = rules::Env::read(var);
        Self { env, no_proxy }
    }
}

impl Proxies for System {
    fn find(&self, url: &Url) -> Vec<Hop> {
        if rules::is_loopback(url) || self.no_proxy.matches(url) {
            return vec![Hop::Direct];
        }
        match &self.env {
            Some(env) => env.find(url),
            None => platform(url),
        }
    }

    fn blocking(&self) -> bool {
        self.env.is_none() && cfg!(any(windows, target_os = "macos"))
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub(crate) struct Manual {
    pub(crate) http: Option<Hop>,
    pub(crate) https: Option<Hop>,
    pub(crate) bypass: Vec<String>,
}

const VARIABLES: [&str; 4] = ["http_proxy", "https_proxy", "all_proxy", "no_proxy"];
const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

pub fn backend_variables() -> Vec<(String, String)> {
    variables(|name| std::env::var(name).ok(), cfg!(windows), manual)
}

fn variables(var: impl Fn(&str) -> Option<String>, windows: bool, system: impl FnOnce() -> Option<Manual>) -> Vec<(String, String)> {
    let names = |name: &str| if windows { vec![name.to_ascii_uppercase()] } else { vec![name.to_owned(), name.to_ascii_uppercase()] };
    let set = |name: &str| var(name).filter(|value| !value.trim().is_empty());
    let mut found: Vec<(String, String)> = VARIABLES.into_iter().flat_map(names).filter_map(|name| set(&name).map(|value| (name, value))).collect();
    let proxied = |found: &[(String, String)]| found.iter().any(|(name, _)| !name.eq_ignore_ascii_case("no_proxy"));
    if proxied(&found) {
        return found;
    }
    let Some(manual) = system() else { return found };
    let mut add = |name: &str, value: String| found.extend(names(name).into_iter().map(|name| (name, value.clone())));
    let socks = [&manual.http, &manual.https].into_iter().flatten().find_map(|hop| match hop {
        Hop::Socks4(at) => Some(format!("socks4a://{at}")),
        Hop::Socks5(at) => Some(format!("socks5h://{at}")),
        _ => None,
    });
    for (name, hop) in [("http_proxy", &manual.http), ("https_proxy", &manual.https)] {
        if let Some(Hop::Http(at)) = hop {
            add(name, format!("http://{at}"));
        }
    }
    if let Some(socks) = socks {
        add("all_proxy", socks);
    }
    if !proxied(&found) {
        return found;
    }
    let session = names("no_proxy").into_iter().find_map(|name| set(&name)).into_iter();
    let bypass: Vec<String> = session.chain(manual.bypass).chain(LOOPBACK.map(str::to_owned)).collect();
    found.retain(|(name, _)| !name.eq_ignore_ascii_case("no_proxy"));
    found.extend(names("no_proxy").into_iter().map(|name| (name, bypass.join(","))));
    found
}

fn manual() -> Option<Manual> {
    cfg_select! {
        windows => { windows::manual() }
        target_os = "macos" => { macos::manual() }
        _ => { None }
    }
}

fn platform(url: &Url) -> Vec<Hop> {
    cfg_select! {
        windows => { windows::find(url) }
        target_os = "macos" => { macos::find(url) }
        _ => { let _ = url; vec![Hop::Direct] }
    }
}

/// Where a request to a remote URL goes.
pub(crate) enum Route {
    /// the runtime's ordinary client, with its pool
    Direct,
    Through(Vec<Hop>, Target),
}

/// The host a tunnel leads to, and what the request through it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) secure: bool,
    /// the `Host` header: the port only when it is not the scheme's default
    pub(crate) authority: String,
    /// the path and query (the fragment is never sent)
    pub(crate) path: String,
}

impl Target {
    pub(crate) fn of(url: &Url) -> Option<Self> {
        let host = match url.host()? {
            url::Host::Domain(name) => name.to_owned(),
            url::Host::Ipv4(ip) => ip.to_string(),
            url::Host::Ipv6(ip) => ip.to_string(),
        };
        let port = url.port_or_known_default()?;
        let secure = matches!(url.scheme(), "https" | "wss");
        let authority = match url.port() {
            Some(port) => format!("{}:{port}", url.host_str()?),
            None => url.host_str()?.to_owned(),
        };
        Some(Self { host, port, secure, authority, path: url[url::Position::BeforePath..url::Position::AfterQuery].to_owned() })
    }

    /// `host:port` of a `CONNECT` (IPv6 in brackets)
    pub(crate) fn endpoint(&self) -> Endpoint {
        Endpoint { host: self.host.clone(), port: self.port }
    }
}

/// Asks `proxies` about `url` (already checked: http, https, ws or wss with a host).
pub(crate) async fn route(proxies: &Shared, url: &str) -> Route {
    let Ok(parsed) = Url::parse(url) else { return Route::Direct };
    let Some(target) = Target::of(&parsed) else { return Route::Direct };
    let mut asked = parsed;
    let scheme = if target.secure { "https" } else { "http" };
    if asked.scheme() != scheme && asked.set_scheme(scheme).is_err() {
        return Route::Direct;
    }
    let hops = if proxies.blocking() {
        let (answer, answered) = oneshot::channel();
        let proxies = proxies.clone();
        worker(Box::new(move || {
            _ = answer.send(proxies.find(&asked));
        }));
        answered.await.unwrap_or_else(|_| vec![Hop::Direct])
    } else {
        proxies.find(&asked)
    };
    if hops.is_empty() || hops == [Hop::Direct] {
        return Route::Direct;
    }
    log::debug!("proxy: {} through {}", target.endpoint(), hops.iter().map(Hop::to_string).collect::<Vec<_>>().join(", "));
    Route::Through(hops, target)
}

type Job = Box<dyn FnOnce() + Send>;

/// One thread answers the questions that may block (WinHTTP's WPAD and PAC, CFNetwork's PAC with its run loop), in order.
fn worker(job: Job) {
    static JOBS: OnceLock<Option<mpsc::Sender<Job>>> = OnceLock::new();
    let jobs = JOBS.get_or_init(|| {
        let (jobs, received) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new().name("acde-proxy".into()).spawn(move || {
            for job in received {
                job();
            }
        });
        spawned.map_err(|error| log::error!("proxy: cannot start the acde-proxy thread: {error}")).ok().map(|_| jobs)
    });
    // without the thread the job is dropped with its answer: the request goes direct
    if let Some(jobs) = jobs {
        _ = jobs.send(job);
    }
}

/// A fixed answer, for tests.
#[cfg(test)]
pub(crate) struct Fixed(pub(crate) Vec<Hop>);

#[cfg(test)]
impl Proxies for Fixed {
    fn find(&self, _: &Url) -> Vec<Hop> {
        self.0.clone()
    }
}

#[cfg(test)]
pub(crate) mod tests;
