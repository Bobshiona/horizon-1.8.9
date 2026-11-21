//! The function-key view toggles `Minecraft.runTick` reads when no screen
//! is open (`ave.s`): F1 hides the GUI.

use winit::keyboard::KeyCode;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct View {
    /// `GameSettings.hideGUI` (`avh.aA`).
    pub hide_gui: bool,
}

impl View {
    /// A fresh key press. Vanilla hands keys to the open screen instead, so
    /// these only act with none open. Returns whether the key was used.
    pub fn key(&mut self, key: KeyCode, screen_open: bool) -> bool {
        if screen_open {
            return false;
        }
        match key {
            KeyCode::F1 => self.hide_gui = !self.hide_gui,
            _ => return false,
        }
        true
    }

    /// `EntityRenderer.updateCameraAndRender`: the in-game overlay (HUD,
    /// chat, mods) draws unless the GUI is hidden with no screen open.
    pub fn overlay(self, screen_open: bool) -> bool {
        !self.hide_gui || screen_open
    }

    /// `renderHand`, `isDrawBlockOutline` and `Minecraft.isGuiEnabled`
    /// (name tags) all skip while the GUI is hidden, screen or not.
    pub fn world_gui(self) -> bool {
        !self.hide_gui
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f1_hides_the_gui_only_without_a_screen_and_screens_bring_the_overlay_back() {
        let mut view = View::default();
        assert!(view.overlay(false) && view.world_gui());
        assert!(!view.key(KeyCode::F1, true), "a screen takes the key");
        assert!(!view.hide_gui);
        assert!(view.key(KeyCode::F1, false));
        assert!(view.hide_gui);
        assert!(!view.overlay(false));
        assert!(
            view.overlay(true),
            "GuiChat and other screens still draw it"
        );
        assert!(!view.world_gui());
        assert!(!view.key(KeyCode::KeyW, false));
        assert!(view.key(KeyCode::F1, false));
        assert!(!view.hide_gui);
    }
}
