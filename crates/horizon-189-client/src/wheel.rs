//! Scroll wheel and trackpad input in vanilla wheel notches. A mouse wheel
//! reports lines, one notch each. A trackpad reports pixels: 24 make a notch,
//! and for the hotbar each event moves at most one slot and the momentum that
//! keeps scrolling after the fingers lift is ignored, so a flick selects a slot
//! instead of spinning through the bar.
use std::time::{Duration, Instant};
use winit::event::{MouseScrollDelta, TouchPhase};

/// Trackpad pixels per notch.
const PIXELS: f64 = 24.0;
/// A gesture that starts this soon after another ended is that one's momentum
/// (AppKit begins momentum right after the fingers lift; winit reports both
/// phases alike).
const MOMENTUM_GAP: Duration = Duration::from_millis(60);

#[derive(Debug, Default)]
pub struct Wheel {
    pixels: f64,
    /// When the last finger gesture ended.
    ended: Option<Instant>,
    /// Inside a momentum (inertial) gesture.
    momentum: bool,
}

impl Wheel {
    /// Forget partial notches (a screen opened or closed).
    pub fn reset(&mut self) {
        self.pixels = 0.0;
    }

    /// Notches for one wheel event. `hotbar`: at most one per event, and none
    /// from momentum scrolling.
    pub fn notches(
        &mut self,
        delta: MouseScrollDelta,
        phase: TouchPhase,
        now: Instant,
        hotbar: bool,
    ) -> i32 {
        let y = match delta {
            MouseScrollDelta::LineDelta(_, y) => return y.signum() as i32 * i32::from(y != 0.0),
            MouseScrollDelta::PixelDelta(position) => position.y,
        };
        match phase {
            TouchPhase::Started => {
                self.momentum = self
                    .ended
                    .is_some_and(|ended| now.saturating_duration_since(ended) <= MOMENTUM_GAP);
                self.pixels = 0.0;
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.ended = (!self.momentum).then_some(now);
                let was_momentum = std::mem::take(&mut self.momentum);
                if was_momentum && hotbar {
                    return 0;
                }
            }
            TouchPhase::Moved => {}
        }
        if self.momentum && hotbar {
            return 0;
        }
        self.pixels += y;
        let notches = (self.pixels / PIXELS).trunc();
        self.pixels -= notches * PIXELS;
        if hotbar {
            // One slot per event; what is left over can't add a second.
            self.pixels = self.pixels.clamp(-PIXELS, PIXELS);
            notches.clamp(-1.0, 1.0) as i32
        } else {
            notches as i32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;

    fn pixels(y: f64) -> MouseScrollDelta {
        MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, y))
    }

    #[test]
    fn mouse_wheels_step_one_notch_per_line() {
        let mut wheel = Wheel::default();
        let now = Instant::now();
        let line = |y| MouseScrollDelta::LineDelta(0.0, y);
        assert_eq!(wheel.notches(line(1.0), TouchPhase::Moved, now, true), 1);
        assert_eq!(wheel.notches(line(-3.0), TouchPhase::Moved, now, true), -1);
        assert_eq!(wheel.notches(line(0.0), TouchPhase::Moved, now, false), 0);
    }

    #[test]
    fn trackpad_pixels_accumulate_and_the_hotbar_moves_one_slot_per_event() {
        let mut wheel = Wheel::default();
        let now = Instant::now();
        assert_eq!(
            wheel.notches(pixels(10.0), TouchPhase::Started, now, true),
            0
        );
        assert_eq!(wheel.notches(pixels(10.0), TouchPhase::Moved, now, true), 0);
        assert_eq!(wheel.notches(pixels(10.0), TouchPhase::Moved, now, true), 1);
        // A fast swipe: one slot, not four.
        assert_eq!(
            wheel.notches(pixels(100.0), TouchPhase::Moved, now, true),
            1
        );
        assert_eq!(
            wheel.notches(pixels(-30.0), TouchPhase::Moved, now, true),
            0
        );
        // Menus and chat keep every notch.
        let mut menu = Wheel::default();
        assert_eq!(
            menu.notches(pixels(100.0), TouchPhase::Moved, now, false),
            4
        );
    }

    #[test]
    fn momentum_after_the_fingers_lift_does_not_scroll_the_hotbar() {
        let mut wheel = Wheel::default();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        wheel.notches(pixels(0.0), TouchPhase::Started, at(0), true);
        assert_eq!(
            wheel.notches(pixels(30.0), TouchPhase::Moved, at(10), true),
            1
        );
        wheel.notches(pixels(0.0), TouchPhase::Ended, at(20), true);
        // Momentum begins straight away and keeps going.
        assert_eq!(
            wheel.notches(pixels(40.0), TouchPhase::Started, at(25), true),
            0
        );
        for ms in [40, 60, 80] {
            assert_eq!(
                wheel.notches(pixels(40.0), TouchPhase::Moved, at(ms), true),
                0
            );
        }
        assert_eq!(
            wheel.notches(pixels(5.0), TouchPhase::Ended, at(100), true),
            0
        );
        // A new touch later scrolls again.
        wheel.notches(pixels(0.0), TouchPhase::Started, at(400), true);
        assert_eq!(
            wheel.notches(pixels(-30.0), TouchPhase::Moved, at(410), true),
            -1
        );
        // In a menu the same momentum scrolls.
        let mut menu = Wheel::default();
        menu.notches(pixels(0.0), TouchPhase::Started, at(0), false);
        menu.notches(pixels(0.0), TouchPhase::Ended, at(20), false);
        assert_eq!(
            menu.notches(pixels(48.0), TouchPhase::Started, at(25), false),
            2
        );
    }
}
