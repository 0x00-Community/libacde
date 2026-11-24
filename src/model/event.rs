use std::sync::{Arc, Mutex};

#[cfg(feature = "napi")]
use napi_derive::napi;

#[cfg_attr(feature = "napi", napi(object))]
#[derive(Default, Clone, Debug)]
pub struct Event {
    #[cfg_attr(feature = "napi", napi(js_name = "type"))]
    pub kind: EventName,
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
    pub desk: Option<u32>,
    pub pen: Option<Pen>,
}

#[cfg_attr(feature = "napi", napi(object))]
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Pen {
    pub pressure: f64,
    pub tangential_pressure: f64,
    pub tilt_x: f64,
    pub tilt_y: f64,
    pub twist: f64,
    pub eraser: bool,
}

#[cfg(feature = "napi")]
pub type EventName = String;

#[cfg(not(feature = "napi"))]
pub type EventName = std::borrow::Cow<'static, str>;

impl Event {
    pub fn named(kind: impl Into<EventName>) -> Self {
        Self { kind: kind.into(), ..Default::default() }
    }
}

pub type Waker = Arc<dyn Fn() + Send + Sync>;

const MOVES_KEPT: usize = 1024;

#[derive(Default)]
struct Queue {
    events: Vec<Event>,
    moves: usize,
}

impl Queue {
    fn push(&mut self, event: Event) {
        if event.kind != "pointermove" {
            self.moves = 0;
            return self.events.push(event);
        }
        if self.moves < MOVES_KEPT {
            self.moves += 1;
            return self.events.push(event);
        }
        let Some(last) = self.events.last_mut() else { return self.events.push(event) };
        let sum = |a: Option<f64>, b: Option<f64>| a.zip(b).map(|(a, b)| a + b).or(b);
        *last = Event { delta_x: sum(last.delta_x, event.delta_x), delta_y: sum(last.delta_y, event.delta_y), ..event };
    }
}

#[derive(Default)]
pub struct Inbox {
    queue: Mutex<Queue>,
    waker: Mutex<Option<Waker>>,
}

impl Inbox {
    pub fn push(&self, mut event: Event) {
        event.stamp.get_or_insert_with(crate::log::now_ms);
        let mut queue = self.queue.lock().expect("event queue lock");
        let was_empty = queue.events.is_empty();
        queue.push(event);
        drop(queue);
        if was_empty {
            self.wake();
        }
    }

    pub fn push_all(&self, events: impl IntoIterator<Item = Event>) {
        let mut queue = self.queue.lock().expect("event queue lock");
        let was_empty = queue.events.is_empty();
        for mut event in events {
            event.stamp.get_or_insert_with(crate::log::now_ms);
            queue.push(event);
        }
        let wake = was_empty && !queue.events.is_empty();
        drop(queue);
        if wake {
            self.wake();
        }
    }

    pub fn drain(&self) -> Vec<Event> {
        let mut queue = self.queue.lock().expect("event queue lock");
        queue.moves = 0;
        std::mem::take(&mut queue.events)
    }

    pub fn is_empty(&self) -> bool {
        self.queue.lock().expect("event queue lock").events.is_empty()
    }

    pub fn set_waker(&self, waker: Option<Waker>) {
        *self.waker.lock().expect("event queue lock") = waker;
        if !self.is_empty() {
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
        assert_eq!(drained.iter().map(|e| &*e.kind).collect::<Vec<_>>(), ["workarea", "output"]);
        assert!(drained.iter().all(|e| e.stamp.is_some()));
        inbox.push_all([]);
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        assert!(inbox.is_empty());
    }

    fn moved(x: f64, delta: Option<f64>) -> Event {
        Event { kind: "pointermove".into(), x: Some(x), y: Some(0.0), delta_x: delta, delta_y: delta, ..Default::default() }
    }

    #[test]
    fn every_pointer_move_is_kept_with_its_own_stamp() {
        let inbox = Inbox::default();
        for x in 0..5 {
            inbox.push(Event { stamp: Some(f64::from(x)), ..moved(f64::from(x), None) });
        }
        inbox.push(Event::named("frame"));
        inbox.push(moved(9.0, None));
        let drained = inbox.drain();
        let xs: Vec<_> = drained.iter().map(|e| (&*e.kind, e.x, e.stamp)).collect();
        assert_eq!(xs.len(), 7);
        assert_eq!(xs[..5].iter().map(|(_, x, s)| (x.unwrap(), s.unwrap())).collect::<Vec<_>>(), [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0)]);
        assert_eq!(xs[5].0, "frame");
        assert_eq!(xs[6].1, Some(9.0));
    }

    #[test]
    fn a_long_run_of_moves_folds_the_newest_into_the_last_kept_one_with_summed_deltas() {
        let inbox = Inbox::default();
        for x in 0..MOVES_KEPT + 3 {
            inbox.push(moved(x as f64, Some(1.0)));
        }
        let drained = inbox.drain();
        assert_eq!(drained.len(), MOVES_KEPT);
        let last = drained.last().unwrap();
        assert_eq!((last.x, last.delta_x, last.delta_y), (Some((MOVES_KEPT + 2) as f64), Some(4.0), Some(4.0)));
        inbox.push(moved(1.0, None));
        assert_eq!(inbox.drain().len(), 1);
    }
}
