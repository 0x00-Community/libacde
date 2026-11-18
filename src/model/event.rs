use std::sync::{Arc, Mutex};

#[cfg(feature = "napi")]
use napi_derive::napi;

#[cfg_attr(feature = "napi", napi(object))]
#[derive(Default, Clone, Debug)]
pub struct Event {
    #[cfg_attr(feature = "napi", napi(js_name = "type"))]
    pub kind: String,
    pub window: Option<u32>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub button: Option<u32>,
    pub delta_x: Option<f64>,
    pub delta_y: Option<f64>,
    pub code: Option<u32>,
    pub key: Option<String>,
    pub title: Option<String>,
    pub app_id: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub time: Option<f64>,
    pub scale: Option<f64>,
    pub mime: Option<Vec<String>>,
    pub data: Option<std::collections::HashMap<String, String>>,
    pub files: Option<Vec<String>>,
    pub parent: Option<u32>,
    pub stamp: Option<f64>,
    pub state: Option<String>,
    pub previous: Option<String>,
    pub instance: Option<u32>,
}

impl Event {
    pub fn named(kind: &str) -> Self {
        Self { kind: kind.into(), ..Default::default() }
    }
}

pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
pub struct Inbox {
    queue: Mutex<Vec<Event>>,
    waker: Mutex<Option<Waker>>,
}

impl Inbox {
    pub fn push(&self, mut event: Event) {
        event.stamp.get_or_insert_with(crate::log::now_ms);
        let mut queue = self.queue.lock().expect("event queue lock");
        let was_empty = queue.is_empty();
        match queue.last_mut() {
            Some(last) if event.kind == "pointermove" && last.kind == "pointermove" => *last = Event { stamp: last.stamp, ..event },
            _ => queue.push(event),
        }
        drop(queue);
        if was_empty {
            self.wake();
        }
    }

    pub fn push_all(&self, events: impl IntoIterator<Item = Event>) {
        let mut queue = self.queue.lock().expect("event queue lock");
        let was_empty = queue.is_empty();
        for mut event in events {
            event.stamp.get_or_insert_with(crate::log::now_ms);
            queue.push(event);
        }
        let wake = was_empty && !queue.is_empty();
        drop(queue);
        if wake {
            self.wake();
        }
    }

    pub fn drain(&self) -> Vec<Event> {
        std::mem::take(&mut *self.queue.lock().expect("event queue lock"))
    }

    pub fn is_empty(&self) -> bool {
        self.queue.lock().expect("event queue lock").is_empty()
    }

    pub fn set_waker(&self, waker: Option<Waker>) {
        *self.waker.lock().expect("event queue lock") = waker;
        if !self.queue.lock().expect("event queue lock").is_empty() {
            self.wake();
        }
    }

    fn wake(&self) {
        let waker = self.waker.lock().expect("event queue lock").clone();
        if let Some(waker) = waker {
            waker();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[test]
    fn events_pushed_together_are_drained_together_and_wake_once() {
        let inbox = Inbox::default();
        let woken = Arc::new(AtomicU32::new(0));
        let count = woken.clone();
        inbox.set_waker(Some(Arc::new(move || _ = count.fetch_add(1, Ordering::SeqCst))));
        inbox.push_all([Event::named("workarea"), Event::named("output")]);
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        let drained = inbox.drain();
        assert_eq!(drained.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["workarea", "output"]);
        assert!(drained.iter().all(|e| e.stamp.is_some()));
        inbox.push_all([]);
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        assert!(inbox.is_empty());
    }
}
