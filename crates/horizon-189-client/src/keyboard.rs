//! Held-key state for the 20 TPS simulation on macOS.
//!
//! AppKit can lose key-up events: during Command chords, when a system
//! shortcut such as Control-Space (input source switching) consumes part of a
//! chord, and across focus changes. Modifier releases are taken from
//! `flagsChanged` (`ModifiersChanged`), which is reliable, and every held key
//! is reconciled against the window server's key state each frame once that
//! source has proven trustworthy on this machine.
use horizon_physics_189::player::Keys;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use winit::keyboard::{KeyCode, ModifiersState};

/// macOS virtual key codes (`kVK_*`) for the keys the client binds.
fn mac_key(key: KeyCode) -> Option<u16> {
    Some(match key {
        KeyCode::KeyA => 0x00,
        KeyCode::KeyS => 0x01,
        KeyCode::KeyD => 0x02,
        KeyCode::KeyW => 0x0d,
        KeyCode::Space => 0x31,
        KeyCode::Escape => 0x35,
        KeyCode::SuperRight => 0x36,
        KeyCode::SuperLeft => 0x37,
        KeyCode::ShiftLeft => 0x38,
        KeyCode::AltLeft => 0x3a,
        KeyCode::ControlLeft => 0x3b,
        KeyCode::ShiftRight => 0x3c,
        KeyCode::AltRight => 0x3d,
        KeyCode::ControlRight => 0x3e,
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
fn physically_down(code: u16) -> bool {
    use objc2_core_graphics::{CGEventSource, CGEventSourceStateID};
    CGEventSource::key_state(CGEventSourceStateID::CombinedSessionState, code)
}

#[cfg(not(target_os = "macos"))]
fn physically_down(_code: u16) -> bool {
    true
}

fn is_modifier(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight
    )
}

#[derive(Default)]
pub struct Keyboard {
    held: HashMap<KeyCode, Instant>,
    set: HashSet<KeyCode>,
    modifiers: ModifiersState,
    /// Presses the hardware state confirmed and contradicted.
    confirmed: u32,
    contradicted: u32,
}

impl Keyboard {
    fn sync(&mut self) {
        self.set = self.held.keys().copied().collect();
    }

    pub fn press(&mut self, key: KeyCode, repeat: bool) {
        if repeat {
            // OS auto-repeat carries no new state.
            return;
        }
        if let Some(code) = mac_key(key) {
            if physically_down(code) {
                self.confirmed += 1;
            } else {
                self.contradicted += 1;
            }
        }
        self.held.insert(key, Instant::now());
        self.sync();
    }

    pub fn release(&mut self, key: KeyCode) {
        self.held.remove(&key);
        self.sync();
    }

    /// `flagsChanged`: authoritative for modifier keys. Command going down
    /// drops other keys, whose key-ups AppKit withholds during the chord.
    pub fn modifiers(&mut self, state: ModifiersState) {
        if state.super_key() && !self.modifiers.super_key() {
            self.held.retain(|&key, _| is_modifier(key));
        }
        let gone = |key: KeyCode| match key {
            KeyCode::ShiftLeft | KeyCode::ShiftRight => !state.shift_key(),
            KeyCode::ControlLeft | KeyCode::ControlRight => !state.control_key(),
            KeyCode::AltLeft | KeyCode::AltRight => !state.alt_key(),
            KeyCode::SuperLeft | KeyCode::SuperRight => !state.super_key(),
            _ => false,
        };
        self.held.retain(|&key, _| !gone(key));
        self.modifiers = state;
        self.sync();
    }

    pub fn clear(&mut self) {
        self.held.clear();
        self.modifiers = ModifiersState::empty();
        self.sync();
    }

    fn trusted(&self) -> bool {
        self.confirmed >= 2 && self.confirmed > self.contradicted.saturating_mul(4)
    }

    /// Drop keys the window server reports as up. Returns whether any were.
    pub fn reconcile(&mut self) -> bool {
        if !self.trusted() {
            return false;
        }
        let settle = Duration::from_millis(60);
        let before = self.held.len();
        self.held.retain(|&key, &mut since| {
            since.elapsed() < settle || mac_key(key).is_none_or(physically_down)
        });
        let changed = self.held.len() != before;
        if changed {
            self.sync();
        }
        changed
    }

    pub fn state(&self) -> ModifiersState {
        self.modifiers
    }

    pub fn held(&self) -> &HashSet<KeyCode> {
        &self.set
    }

    /// Vanilla default bindings. Left Option also sprints: on macOS Control
    /// with Space is the input-source shortcut, which swallows jump presses.
    pub fn keys(&self) -> Keys {
        let down = |key| self.set.contains(&key);
        Keys {
            forward: down(KeyCode::KeyW),
            back: down(KeyCode::KeyS),
            left: down(KeyCode::KeyA),
            right: down(KeyCode::KeyD),
            jump: down(KeyCode::Space),
            sneak: down(KeyCode::ShiftLeft) || down(KeyCode::ShiftRight),
            sprint: down(KeyCode::ControlLeft)
                || down(KeyCode::ControlRight)
                || down(KeyCode::AltLeft),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_flags_release_keys_whose_key_up_was_lost() {
        let mut keyboard = Keyboard::default();
        keyboard.press(KeyCode::ShiftLeft, false);
        keyboard.press(KeyCode::KeyW, false);
        keyboard.modifiers(ModifiersState::SHIFT);
        assert!(keyboard.keys().sneak && keyboard.keys().forward);
        keyboard.modifiers(ModifiersState::empty());
        assert!(!keyboard.keys().sneak);
        assert!(keyboard.keys().forward);
        // Command drops W, whose key-up AppKit would withhold.
        keyboard.modifiers(ModifiersState::SUPER);
        assert!(!keyboard.keys().forward);
        keyboard.press(KeyCode::KeyW, true);
        assert!(!keyboard.keys().forward, "auto-repeat is not a press");
        keyboard.press(KeyCode::AltLeft, false);
        assert!(keyboard.keys().sprint);
        keyboard.clear();
        assert_eq!(keyboard.keys(), Keys::default());
    }
}
