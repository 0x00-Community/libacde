use std::sync::LazyLock;
use std::time::{Duration, Instant};

use crate::model::{Event, Inbox};

const SYNCED_FALLBACK: u32 = 4;

static WEB_FPS: LazyLock<Option<Duration>> =
    LazyLock::new(|| std::env::var("ACDE_WEB_FPS").ok().and_then(|v| v.parse::<f64>().ok()).filter(|f| *f > 0.0).map(|fps| Duration::from_secs_f64(1.0 / fps)));

#[derive(Default)]
pub struct Pacer {
    pub want_frame: bool,
    pub animating: bool,
    interval: Option<Duration>,
    last_tick: Option<Instant>,
    told: Option<(u32, u32, u64)>,
    synced: bool,
}

impl Pacer {
    pub fn new(refresh: Option<Duration>) -> Self {
        Self { interval: WEB_FPS.or(refresh), ..Self::default() }
    }

    pub fn set_refresh(&mut self, refresh: Option<Duration>) {
        self.interval = WEB_FPS.or(refresh);
    }

    pub fn set_synced(&mut self, synced: bool) {
        self.synced = synced && WEB_FPS.is_none();
    }

    pub fn synced(&self) -> bool {
        self.synced
    }

    pub fn wants(&self) -> bool {
        self.want_frame || self.animating
    }

    pub fn wait(&self) -> Option<Duration> {
        let interval = self.interval.map(|i| if self.synced { i * SYNCED_FALLBACK } else { i });
        self.wants().then(|| interval.map_or(Duration::ZERO, |i| i.saturating_sub(self.last_tick.map_or(i, |t| t.elapsed()))))
    }

    pub fn vsync(&mut self, inbox: &Inbox) -> Option<f64> {
        if !self.synced || !self.wants() {
            return None;
        }
        self.tick(inbox)
    }

    pub fn tick(&mut self, inbox: &Inbox) -> Option<f64> {
        let now = crate::log::now_ms();
        self.last_tick = Some(Instant::now());
        if std::mem::take(&mut self.want_frame) {
            inbox.push(Event { kind: "frame".into(), time: Some(now), ..Default::default() });
        }
        self.animating.then_some(now)
    }

    pub fn tell_size(&mut self, inbox: &Inbox, (w, h): (u32, u32), scale: f64) {
        let told = (w, h, scale.to_bits());
        if self.told.replace(told) != Some(told) {
            inbox.push(Event { kind: "resize".into(), width: Some(w.into()), height: Some(h.into()), scale: Some(scale), ..Default::default() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_synced_pacer_ticks_on_vsync_and_keeps_the_timer_as_a_fallback() {
        let inbox = Inbox::default();
        let mut pacer = Pacer::new(Some(Duration::from_millis(15)));
        pacer.set_synced(true);
        if WEB_FPS.is_some() {
            assert!(!pacer.synced());
            return;
        }
        assert_eq!(pacer.vsync(&inbox), None);
        assert!(inbox.is_empty(), "no frame was asked for");
        pacer.want_frame = true;
        assert_eq!(pacer.wait(), Some(Duration::ZERO), "the first frame after a pause does not wait for a vsync");
        assert_eq!(pacer.vsync(&inbox), None);
        assert_eq!(inbox.drain().len(), 1);
        pacer.want_frame = true;
        assert!(pacer.wait().is_some_and(|w| w > Duration::from_millis(30)), "while vsyncs arrive the timer does not tick");
        pacer.set_synced(false);
        assert!(pacer.wait().is_some_and(|w| w <= Duration::from_millis(15)));
    }
}
