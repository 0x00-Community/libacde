//! Tells a page when the shared GPU device is lost and when the host window has published a new one, so canvases that
//! lost their context (WebGL after webglcontextlost was cancelled) can restore it. Ported from Lang Tan's Akira/feat/webgl
//! (6fa672b, acdev #54 / #59).

use std::cell::RefCell;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::gpu::Gpu;
use crate::host::bind;
use crate::host::rt::{self, Msg};

/// How long a page waits for the host window to open a new device after a loss.
const REPLACEMENT_WAIT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Watch {
    callback: Option<v8::Global<v8::Function>>,
    listener: Option<(Weak<Gpu>, u64)>,
}

thread_local! {
    static WATCH: RefCell<Watch> = RefCell::default();
}

/// Calls `callback("lost")` and `callback("restored")` on this page's thread from now on.
pub fn watch(callback: v8::Global<v8::Function>) {
    let first = WATCH.with(|w| {
        let mut w = w.borrow_mut();
        let first = w.callback.is_none();
        w.callback = Some(callback);
        first
    });
    if first && let Some(gpu) = Gpu::shared() {
        listen(gpu);
    }
}

fn listen(gpu: Arc<Gpu>) {
    let tx = rt::current().sender();
    let weak = Arc::downgrade(&gpu);
    let id = gpu.on_lost(move |_| {
        _ = tx.send(Msg::Task(Box::new(|scope| notify(scope, "lost"))));
        let (tx, weak) = (tx.clone(), weak.clone());
        let spawned = std::thread::Builder::new().name("acde-gpu-watch".into()).spawn(move || {
            let Some(lost) = weak.upgrade() else { return };
            if let Some(now) = Gpu::replacement(&lost, REPLACEMENT_WAIT) {
                _ = tx.send(Msg::Task(Box::new(move |scope| {
                    listen(now);
                    notify(scope, "restored");
                })));
            }
        });
        if let Err(e) = spawned {
            log::warn!("waiting for a new GPU device for canvases: {e}");
        }
    });
    let old = WATCH.with(|w| w.borrow_mut().listener.replace((Arc::downgrade(&gpu), id)));
    forget(old);
}

fn forget(listener: Option<(Weak<Gpu>, u64)>) {
    if let Some((gpu, id)) = listener
        && let Some(gpu) = gpu.upgrade()
    {
        gpu.forget_lost(id);
    }
}

fn notify(scope: &mut v8::PinScope, kind: &str) {
    let Some(callback) = WATCH.with(|w| w.borrow().callback.clone()) else { return };
    let kind = bind::string(scope, kind);
    bind::call(scope, callback, &[kind.into()]);
}

pub fn clear() {
    let watch = WATCH.with(|w| std::mem::take(&mut *w.borrow_mut()));
    forget(watch.listener);
}
