use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::sync::{LazyLock, Mutex, Once};
use std::time::{Duration, Instant};

use crate::host::rt::Msg;

static LOOPS: LazyLock<Mutex<HashMap<usize, Sender<Msg>>>> = LazyLock::new(Default::default);

pub fn register(isolate: *mut c_void, tx: Sender<Msg>) {
    LOOPS.lock().unwrap().insert(isolate as usize, tx);
}

pub fn unregister(isolate: *mut c_void) {
    LOOPS.lock().unwrap().remove(&(isolate as usize));
}

fn post(isolate: *mut c_void, msg: Msg) {
    let tx = LOOPS.lock().unwrap().get(&(isolate as usize)).cloned();
    if let Some(tx) = tx {
        _ = tx.send(msg);
    }
}

struct Platform;

impl v8::PlatformImpl for Platform {
    fn post_task(&self, isolate: *mut c_void, task: v8::Task) {
        post(isolate, Msg::V8(task));
    }

    fn post_non_nestable_task(&self, isolate: *mut c_void, task: v8::Task) {
        post(isolate, Msg::V8(task));
    }

    fn post_delayed_task(&self, isolate: *mut c_void, task: v8::Task, delay: f64) {
        delayed(isolate, task, delay);
    }

    fn post_non_nestable_delayed_task(&self, isolate: *mut c_void, task: v8::Task, delay: f64) {
        delayed(isolate, task, delay);
    }

    fn post_idle_task(&self, _isolate: *mut c_void, _task: v8::IdleTask) {}
}

fn delayed(isolate: *mut c_void, task: v8::Task, delay: f64) {
    let delay = Duration::try_from_secs_f64(delay.min(86400.0)).unwrap_or(Duration::ZERO);
    post(isolate, Msg::Delayed(Instant::now() + delay, task));
}

pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        v8::icu::set_default_locale("zh-Hans");
        v8::V8::set_flags_from_string("--js-float16array --js-explicit-resource-management");
        if crate::runtime::dev_mode() {
            v8::V8::set_flags_from_string("--inspector-live-edit");
        }
        let threads = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4).min(4);
        let platform = v8::new_custom_platform(threads, false, false, Platform).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}
