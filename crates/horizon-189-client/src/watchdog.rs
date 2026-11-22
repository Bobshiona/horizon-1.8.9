//! `--watchdog`: the scripted session behind the Watchdog packet-parity
//! suite. Every module is switched on, then a fixed script presses keys
//! and mouse buttons through the same handlers the window uses (so every
//! screen and focus guard applies) at fixed ticks. A tick is a movement
//! packet actually sent, which is also how `horizon-189-trace check` counts
//! them, so the script's screen windows line up with the server's log
//! exactly. The script prints its windows for the checker.
use crate::{settings::Settings, view::View};
use winit::{event::MouseButton, keyboard::KeyCode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Key(KeyCode, bool),
    Click(MouseButton, bool),
    /// Lock the cursor into the game, as a click on the window does.
    Capture,
    Done,
}

/// The ticks a screen is open: the inventory, then the Right Shift menu.
pub const WINDOWS: [(u64, u64); 2] = [(110, 140), (145, 165)];

/// Down at `at`, up a tick later.
fn tap(script: &mut Vec<(u64, Step)>, at: u64, key: KeyCode) {
    script.push((at, Step::Key(key, true)));
    script.push((at + 1, Step::Key(key, false)));
}

fn hold(script: &mut Vec<(u64, Step)>, from: u64, to: u64, key: KeyCode) {
    script.push((from, Step::Key(key, true)));
    script.push((to, Step::Key(key, false)));
}

/// Swing at the sparring partner straight ahead, walk, latch sprint, jump,
/// sneak, sneak with the inventory open, sneak with the mod menu open,
/// cycle F5, zoom, then F3+B.
pub fn script() -> Vec<(u64, Step)> {
    let mut s = vec![(10, Step::Capture)];
    for at in (15..=31).step_by(4) {
        s.push((at, Step::Click(MouseButton::Left, true)));
        s.push((at + 1, Step::Click(MouseButton::Left, false)));
    }
    hold(&mut s, 35, 95, KeyCode::KeyW);
    tap(&mut s, 55, KeyCode::ControlLeft);
    hold(&mut s, 75, 90, KeyCode::Space);
    hold(&mut s, 95, 105, KeyCode::ShiftLeft);
    let (inventory, menu) = (WINDOWS[0], WINDOWS[1]);
    tap(&mut s, inventory.0, KeyCode::KeyE);
    hold(&mut s, inventory.0 + 5, inventory.1 - 5, KeyCode::ShiftLeft);
    tap(&mut s, inventory.1, KeyCode::KeyE);
    tap(&mut s, menu.0, KeyCode::ShiftRight);
    hold(&mut s, menu.0 + 5, menu.1 - 5, KeyCode::ShiftLeft);
    tap(&mut s, menu.1, KeyCode::Escape);
    s.push((menu.1 + 2, Step::Capture));
    for at in [170, 172, 174] {
        tap(&mut s, at, KeyCode::F5);
    }
    hold(&mut s, 180, 195, KeyCode::KeyC);
    s.push((200, Step::Key(KeyCode::F3, true)));
    tap(&mut s, 201, KeyCode::KeyB);
    s.push((203, Step::Key(KeyCode::F3, false)));
    s.push((210, Step::Done));
    s.sort_by_key(|(at, _)| *at);
    s
}

/// The windows as `horizon-189-trace check --closed` takes them.
pub fn closed() -> String {
    WINDOWS
        .iter()
        .map(|(from, to)| format!("{from}-{to}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Every mod on at once: all HUD cards, every toggle, a custom crosshair and
/// outline, and hitboxes.
pub fn enable_everything(settings: &mut Settings, view: &mut View) {
    for hud in &mut settings.hud {
        hud.enabled = true;
    }
    let t = &mut settings.toggles;
    t.zoom = true;
    t.smooth_zoom = true;
    t.toggle_sprint = true;
    t.fullbright = true;
    t.scoreboard_numbers = true;
    t.advanced_tooltips = true;
    t.crosshair = 1;
    t.outline = 1;
    view.hitboxes = true;
}

pub struct Watchdog {
    steps: Vec<(u64, Step)>,
    next: usize,
}

impl Watchdog {
    pub fn new() -> Self {
        Self {
            steps: script(),
            next: 0,
        }
    }

    /// The steps due by `tick`, in order, each once.
    pub fn due(&mut self, tick: u64) -> Vec<Step> {
        let mut steps = Vec::new();
        while let Some(&(at, step)) = self.steps.get(self.next) {
            if at > tick {
                break;
            }
            steps.push(step);
            self.next += 1;
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_runs_in_order_once_and_keeps_screens_apart() {
        let script = script();
        assert!(script.windows(2).all(|w| w[0].0 <= w[1].0));
        assert_eq!(script.last(), Some(&(210, Step::Done)));
        // Every key goes up after it goes down.
        for key in [
            KeyCode::KeyW,
            KeyCode::ShiftLeft,
            KeyCode::Space,
            KeyCode::KeyC,
        ] {
            let mut down = false;
            for (_, step) in &script {
                if let Step::Key(k, pressed) = step
                    && *k == key
                {
                    assert_ne!(down, *pressed, "{key:?}");
                    down = *pressed;
                }
            }
            assert!(!down, "{key:?} left held");
        }
        // Sneak is held well inside each screen window.
        for (from, to) in WINDOWS {
            assert!(script.contains(&(from + 5, Step::Key(KeyCode::ShiftLeft, true))));
            assert!(script.contains(&(to - 5, Step::Key(KeyCode::ShiftLeft, false))));
        }
        assert_eq!(closed(), "110-140,145-165");
        let mut watchdog = Watchdog::new();
        assert!(watchdog.due(9).is_empty());
        assert_eq!(watchdog.due(10), [Step::Capture]);
        assert!(watchdog.due(10).is_empty());
        let rest: usize = (11..=300).map(|t| watchdog.due(t).len()).sum();
        assert_eq!(rest + 1, script.len());
    }

    #[test]
    fn everything_turns_on() {
        let mut settings = Settings::default();
        let mut view = View::default();
        enable_everything(&mut settings, &mut view);
        assert!(settings.hud.iter().all(|h| h.enabled));
        assert!(settings.toggles.toggle_sprint && settings.toggles.fullbright);
        assert!(view.hitboxes);
    }
}
