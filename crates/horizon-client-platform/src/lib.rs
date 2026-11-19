//! Version-independent platform helpers shared by modern and legacy clients.
#![forbid(unsafe_code)]

use std::collections::HashSet;
use winit::keyboard::{KeyCode, ModifiersState};

/// Native window-closing shortcut. Extracted unchanged from
/// horizon-client-input/src/input.rs to avoid modern gameplay dependencies.
pub fn is_quit_shortcut_down(modifiers: ModifiersState, keys: &HashSet<KeyCode>) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.super_key() && keys.contains(&KeyCode::KeyQ)
    } else {
        modifiers.alt_key() && keys.contains(&KeyCode::F4)
    }
}
