//! Tap and hold are mutually exclusive; cancellation never emits a tap.
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Tap,
    Start,
    Stop,
}

enum State {
    Pending,
    Held,
    Finished,
}

pub struct Gesture {
    deadline: Instant,
    state: State,
}

impl Gesture {
    pub fn new(now: Instant, delay: Duration) -> Self {
        Self {
            deadline: now + delay,
            state: State::Pending,
        }
    }
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        matches!(self.state, State::Pending).then(|| self.deadline.saturating_duration_since(now))
    }
    pub fn tick(&mut self, now: Instant) -> Action {
        if matches!(self.state, State::Pending) && now >= self.deadline {
            self.state = State::Held;
            Action::Start
        } else {
            Action::None
        }
    }
    pub fn release(&mut self) -> Action {
        let action = match self.state {
            State::Pending => Action::Tap,
            State::Held => Action::Stop,
            State::Finished => Action::None,
        };
        self.state = State::Finished;
        action
    }
    pub fn cancel(&mut self) -> Action {
        let action = if matches!(self.state, State::Held) {
            Action::Stop
        } else {
            Action::None
        };
        self.state = State::Finished;
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_tap_does_not_start_recording() {
        let now = Instant::now();
        let mut gesture = Gesture::new(now, Duration::from_millis(600));
        assert_eq!(gesture.tick(now + Duration::from_millis(300)), Action::None);
        assert_eq!(gesture.release(), Action::Tap);
        assert_eq!(gesture.tick(now + Duration::from_secs(1)), Action::None);
    }
    #[test]
    fn hold_starts_once_stops_on_release_and_never_taps() {
        let now = Instant::now();
        let mut gesture = Gesture::new(now, Duration::from_millis(600));
        assert_eq!(
            gesture.tick(now + Duration::from_millis(600)),
            Action::Start
        );
        assert_eq!(gesture.tick(now + Duration::from_secs(2)), Action::None);
        assert_eq!(gesture.release(), Action::Stop);
        assert_eq!(gesture.release(), Action::None);
    }
    #[test]
    fn sliding_off_cancels_pending_tap_and_releases_active_hold() {
        let now = Instant::now();
        let mut pending = Gesture::new(now, Duration::from_millis(600));
        assert_eq!(pending.cancel(), Action::None);
        assert_eq!(pending.release(), Action::None);
        let mut held = Gesture::new(now, Duration::from_millis(600));
        held.tick(now + Duration::from_secs(1));
        assert_eq!(held.cancel(), Action::Stop);
        assert_eq!(held.release(), Action::None);
    }
}
