mod client;
mod http;
mod mode;
mod ws;

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

use napi_derive::napi;

pub use client::Client;
pub use mode::{GUEST_KEY, dev_mode, managed};

use crate::runtime::sync::Locked;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const CONNECTIONS: usize = 32;
const HEAD_WAIT: Duration = Duration::from_secs(10);
pub const RELOAD_WAIT: Duration = Duration::from_secs(10);

#[napi(object)]
#[derive(Clone, PartialEq)]
pub struct DevToolsTarget {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub url: String,
}

#[napi(object)]
pub struct DevToolsEndpoint {
    pub port: u32,
}

#[napi]
pub struct DevTools;

#[napi]
impl DevTools {
    #[napi]
    pub fn open() -> Option<DevToolsEndpoint> {
        endpoint().map(|e| DevToolsEndpoint { port: e.port.into() })
    }

    #[napi]
    pub fn targets(targets: Vec<DevToolsTarget>) {
        if let Some(endpoint) = endpoint() {
            endpoint.set_targets(targets);
        }
    }

    #[napi]
    pub fn reloading(web: String) {
        if let Some(endpoint) = endpoint() {
            endpoint.reloading.locked().insert(web, Instant::now() + RELOAD_WAIT);
        }
    }
}

pub fn reloading(web: &str) -> bool {
    endpoint().is_some_and(|e| e.reloading(web))
}

pub fn reloaded(web: &str) {
    if let Some(endpoint) = endpoint() {
        endpoint.reloading.locked().remove(web);
    }
}

pub trait Forward: Send + Sync {
    fn attach(&self, _client: &Client) {}
    fn message(&self, client: &Client, message: &str) -> bool;
    fn detach(&self, _client: &Client) {}
}

pub fn forward(to: Option<Arc<dyn Forward>>) {
    if let Some(endpoint) = endpoint() {
        *endpoint.forward.locked() = to;
    }
}

pub fn port() -> Option<u16> {
    endpoint().map(|e| e.port)
}

struct Endpoint {
    port: u16,
    browser: String,
    limit: usize,
    head_wait: Duration,
    connections: AtomicUsize,
    listener: Mutex<Option<TcpListener>>,
    targets: Mutex<Vec<DevToolsTarget>>,
    clients: Mutex<Vec<Client>>,
    forward: Mutex<Option<Arc<dyn Forward>>>,
    reloading: Mutex<HashMap<String, Instant>>,
}

static ENDPOINT: OnceLock<Option<Endpoint>> = OnceLock::new();

fn endpoint() -> Option<&'static Endpoint> {
    static SERVED: Once = Once::new();
    let endpoint = ENDPOINT.get_or_init(Endpoint::open).as_ref()?;
    SERVED.call_once(|| endpoint.serve());
    Some(endpoint)
}

impl Endpoint {
    fn open() -> Option<Self> {
        if !dev_mode() {
            return None;
        }
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(e) => {
                log::error!("devtools: cannot listen on 127.0.0.1: {e}");
                return None;
            }
        };
        let endpoint = Self::new(listener, CONNECTIONS, HEAD_WAIT).inspect_err(|e| log::error!("devtools: {e}")).ok()?;
        log::info!("devtools: listening on 127.0.0.1:{}", endpoint.port);
        Some(endpoint)
    }

    fn new(listener: TcpListener, limit: usize, head_wait: Duration) -> std::io::Result<Self> {
        Ok(Self {
            port: listener.local_addr()?.port(),
            browser: uuid::Uuid::new_v4().to_string(),
            limit,
            head_wait,
            connections: AtomicUsize::new(0),
            listener: Mutex::new(Some(listener)),
            targets: Mutex::default(),
            clients: Mutex::default(),
            forward: Mutex::default(),
            reloading: Mutex::default(),
        })
    }

    fn serve(&'static self) {
        let Some(listener) = self.listener.locked().take() else { return };
        if let Err(e) = std::thread::Builder::new().name("acde-devtools".into()).spawn(move || http::accept(self, listener)) {
            log::error!("devtools thread: {e}");
        }
    }

    fn reloading(&self, web: &str) -> bool {
        let mut reloading = self.reloading.locked();
        let now = Instant::now();
        reloading.retain(|_, until| *until > now);
        reloading.contains_key(web)
    }

    fn set_targets(&self, mut targets: Vec<DevToolsTarget>) {
        let gone: Vec<Client> = {
            let mut current = self.targets.locked();
            let kept: Vec<DevToolsTarget> = current.iter().filter(|t| !targets.iter().any(|n| n.id == t.id) && self.reloading(&t.id)).cloned().collect();
            targets.extend(kept);
            if *current == targets {
                return;
            }
            *current = targets;
            let clients = self.clients.locked();
            clients.iter().filter(|c| c.target().is_some_and(|id| !current.iter().any(|n| n.id == id))).cloned().collect()
        };
        for client in gone {
            client.close();
        }
    }

    fn target(&self, id: &str) -> Option<DevToolsTarget> {
        self.targets.locked().iter().find(|t| t.id == id).cloned()
    }

    fn attach(&self, client: &Client) {
        self.clients.locked().push(client.clone());
        let forward = self.forward.locked().clone();
        if let Some(forward) = forward {
            forward.attach(client);
        }
    }

    fn detach(&self, client: &Client) {
        self.clients.locked().retain(|c| c.id() != client.id());
        let forward = self.forward.locked().clone();
        if let Some(forward) = forward {
            forward.detach(client);
        }
    }

    fn forwarded(&self, client: &Client, message: &str) -> bool {
        let forward = self.forward.locked().clone();
        forward.is_some_and(|f| f.message(client, message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str) -> DevToolsTarget {
        DevToolsTarget { id: id.into(), kind: "page".into(), title: id.into(), url: String::new() }
    }

    #[test]
    fn a_reloading_target_stays_listed_until_its_page_is_back_or_the_wait_ends() {
        let endpoint = Endpoint::new(TcpListener::bind(("127.0.0.1", 0)).unwrap(), 4, HEAD_WAIT).unwrap();
        endpoint.set_targets(vec![target("desktop"), target("alpha#7")]);
        endpoint.reloading.locked().insert("alpha#7".into(), Instant::now() + RELOAD_WAIT);
        endpoint.set_targets(vec![target("desktop")]);
        let ids = |e: &Endpoint| e.targets.locked().iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&endpoint), ["desktop", "alpha#7"], "kept while it reloads");
        assert!(endpoint.reloading("alpha#7"));
        endpoint.reloading.locked().insert("alpha#7".into(), Instant::now());
        assert!(!endpoint.reloading("alpha#7"), "the mark runs out");
        endpoint.set_targets(vec![target("desktop")]);
        assert_eq!(ids(&endpoint), ["desktop"]);
    }
}
