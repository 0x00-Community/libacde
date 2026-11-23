use core::time::Duration;

pub const HOLD: Duration = Duration::from_millis(150);
pub const FRAME: Duration = Duration::from_millis(16);

#[derive(Clone, Debug, PartialEq)]
pub enum Event<T> {
    Move(T),
    Down(T),
    Up(T),
    Other(T),
    Wait(Duration),
}

impl<T> Event<T> {
    fn gap_after(&self, before: Option<&Event<T>>) -> Option<Duration> {
        match (before?, self) {
            (Event::Down(_), Event::Move(_)) => Some(HOLD),
            (Event::Move(_), Event::Move(_)) => Some(FRAME),
            (Event::Wait(_), Event::Up(_)) => None,
            (_, Event::Up(_)) => Some(FRAME),
            _ => None,
        }
    }
}

pub fn pace<T>(events: impl IntoIterator<Item = Event<T>>) -> Vec<Event<T>> {
    let mut paced: Vec<Event<T>> = Vec::new();
    for event in events {
        if let Some(gap) = event.gap_after(paced.last()) {
            paced.push(Event::Wait(gap));
        }
        paced.push(event);
    }
    paced
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(events: &[Event<u8>]) -> Vec<String> {
        events
            .iter()
            .map(|e| match e {
                Event::Move(n) => format!("move {n}"),
                Event::Down(n) => format!("down {n}"),
                Event::Up(n) => format!("up {n}"),
                Event::Other(n) => format!("other {n}"),
                Event::Wait(d) => format!("wait {}", d.as_millis()),
            })
            .collect()
    }

    #[test]
    fn a_drag_holds_after_the_press_and_steps_a_frame_apart() {
        let drag = [Event::Move(0), Event::Down(0), Event::Move(1), Event::Move(2), Event::Up(2)];
        assert_eq!(kinds(&pace(drag)), ["move 0", "down 0", "wait 150", "move 1", "wait 16", "move 2", "wait 16", "up 2"]);
    }

    #[test]
    fn a_wait_the_script_wrote_is_not_added_to() {
        let given = [Event::Down(0), Event::Wait(Duration::from_millis(40)), Event::Move(1), Event::Wait(FRAME), Event::Move(2)];
        assert_eq!(kinds(&pace(given.clone())), kinds(&given));
        let release = [Event::Move(1), Event::Wait(Duration::from_millis(5)), Event::Up(1)];
        assert_eq!(kinds(&pace(release.clone())), kinds(&release));
    }

    #[test]
    fn a_click_waits_a_frame_before_the_release_and_other_events_pass_untouched() {
        assert_eq!(kinds(&pace([Event::Down(0), Event::Up(0)])), ["down 0", "wait 16", "up 0"]);
        assert_eq!(kinds(&pace([Event::Other(1), Event::Other(2), Event::Move(3)])), ["other 1", "other 2", "move 3"]);
        assert_eq!(kinds(&pace([Event::Up(0)])), ["up 0"]);
        assert!(pace(Vec::<Event<u8>>::new()).is_empty());
    }

    #[test]
    fn only_the_first_move_after_a_press_holds() {
        let given = [Event::Down(0), Event::Other(9), Event::Move(1)];
        assert_eq!(kinds(&pace(given)), ["down 0", "other 9", "move 1"]);
    }
}
