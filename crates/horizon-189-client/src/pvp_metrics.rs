//! Passive one-second input/render windows. This module cannot send actions.
use std::{collections::VecDeque, time::Duration};

const WINDOW: Duration = Duration::from_secs(1);
const CAPACITY: usize = 4096;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    /// Visible render-completion cadence; absent until a 250 ms span exists.
    pub fps: Option<u32>,
    pub left: u32,
    pub right: u32,
}
#[derive(Default)]
pub struct Metrics {
    frames: VecDeque<Duration>,
    clicks: [VecDeque<Duration>; 2],
    held: [bool; 2],
}
fn expire(events: &mut VecDeque<Duration>, now: Duration) {
    while events
        .front()
        .is_some_and(|&t| now.saturating_sub(t) >= WINDOW)
    {
        events.pop_front();
    }
}
fn record(events: &mut VecDeque<Duration>, now: Duration) {
    expire(events, now);
    if events.len() == CAPACITY {
        events.pop_front();
    }
    events.push_back(now);
}
impl Metrics {
    /// Left and right buttons currently held.
    pub fn held(&self) -> [bool; 2] {
        self.held
    }

    pub fn presented(&mut self, now: Duration) {
        record(&mut self.frames, now);
    }
    /// Count button-down transitions only, not repeated input, held buttons,
    /// digging ticks, swing packets, GUI clicks or the click that recaptures.
    pub fn button(&mut self, button: usize, pressed: bool, eligible: bool, now: Duration) {
        let Some(held) = self.held.get_mut(button) else {
            return;
        };
        let count = pressed && !*held && eligible;
        *held = pressed;
        if count {
            record(&mut self.clicks[button], now);
        }
    }
    pub fn release_input(&mut self) {
        self.held = [false; 2];
        for events in &mut self.clicks {
            events.clear();
        }
    }
    pub fn sample(&mut self, now: Duration) -> Sample {
        expire(&mut self.frames, now);
        for events in &mut self.clicks {
            expire(events, now);
        }
        let fps = self
            .frames
            .front()
            .zip(self.frames.back())
            .and_then(|(first, _last)| {
                let span = now.saturating_sub(*first);
                (span >= Duration::from_millis(250))
                    .then(|| ((self.frames.len() - 1) as f64 / span.as_secs_f64()).round() as u32)
            });
        Sample {
            fps,
            left: self.clicks[0].len() as u32,
            right: self.clicks[1].len() as u32,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }
    #[test]
    fn cps_counts_transitions_and_expires_at_one_second() {
        let mut m = Metrics::default();
        m.button(0, true, true, ms(10));
        m.button(0, true, true, ms(20));
        m.button(0, false, true, ms(30));
        m.button(0, true, true, ms(40));
        m.button(1, true, true, ms(50));
        assert_eq!((m.sample(ms(60)).left, m.sample(ms(60)).right), (2, 1));
        assert_eq!((m.sample(ms(1010)).left, m.sample(ms(1010)).right), (1, 1));
        assert_eq!((m.sample(ms(1050)).left, m.sample(ms(1050)).right), (0, 0));
    }
    #[test]
    fn gui_focus_and_recapture_clicks_never_inflate_cps() {
        let mut m = Metrics::default();
        m.button(0, true, false, ms(0)); // GUI or recapture
        m.button(0, true, true, ms(1)); // duplicate while held
        assert_eq!(m.sample(ms(2)).left, 0);
        m.button(0, false, false, ms(3));
        m.button(0, true, true, ms(4));
        assert_eq!(m.sample(ms(5)).left, 1);
        m.release_input();
        assert_eq!(m.sample(ms(6)).left, 0);
        m.button(0, true, true, ms(7));
        assert_eq!(m.sample(ms(8)).left, 1);
    }
    #[test]
    fn fps_uses_visible_interval_span_and_stale_samples_disappear() {
        let mut m = Metrics::default();
        assert_eq!(m.sample(ms(0)).fps, None);
        for n in 0..=60 {
            m.presented(Duration::from_secs_f64(n as f64 / 60.0));
        }
        assert_eq!(m.sample(ms(1000)).fps, Some(60));
        let stalled = m.sample(ms(1500)).fps.unwrap();
        assert!(
            (28..=31).contains(&stalled),
            "idle time must reduce the window rate"
        );
        assert_eq!(m.sample(ms(2100)).fps, None);
        for n in 0..=240 {
            m.presented(Duration::from_secs_f64(3.0 + n as f64 / 240.0));
        }
        assert_eq!(m.sample(ms(4000)).fps, Some(240));
    }
    #[test]
    fn event_storage_is_bounded_even_under_hostile_input() {
        let mut m = Metrics::default();
        for _ in 0..10_000 {
            m.button(0, true, true, ms(1));
            m.button(0, false, true, ms(1));
            m.presented(ms(1));
        }
        assert_eq!(m.clicks[0].len(), CAPACITY);
        assert_eq!(m.frames.len(), CAPACITY);
    }
}
