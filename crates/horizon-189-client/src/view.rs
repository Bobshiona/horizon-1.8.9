//! The function-key view toggles `Minecraft.runTick` reads when no screen
//! is open (`ave.s`): F1 hides the GUI, F3 shows the debug overlay and F5
//! cycles the perspective.

use horizon_physics_189::{raycast::Raycaster, trig};
use horizon_world_189::World;
use winit::keyboard::KeyCode;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct View {
    /// `GameSettings.hideGUI` (`avh.aA`).
    pub hide_gui: bool,
    /// `GameSettings.thirdPersonView` (`avh.aB`): 0 first person, 1 behind
    /// the player, 2 in front looking back.
    pub perspective: u8,
    /// `GameSettings.showDebugInfo` (`avh.aC`).
    pub debug: bool,
    /// `RenderManager.debugBoundingBox` (F3+B).
    pub hitboxes: bool,
}

/// F3 combinations the window carries out.
#[derive(Debug, PartialEq, Eq)]
pub enum Combo {
    /// F3+D: `GuiNewChat.clearChatMessages`.
    ClearChat,
    /// F3+A: `RenderGlobal.loadRenderers`.
    ReloadChunks,
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
            KeyCode::F3 => self.debug = !self.debug,
            KeyCode::F5 => {
                self.perspective = if self.perspective >= 2 {
                    0
                } else {
                    self.perspective + 1
                }
            }
            _ => return false,
        }
        true
    }

    /// A key pressed while F3 is held (the same no-screen branch): B toggles
    /// hitboxes here, D and A are for the window.
    pub fn combo(&mut self, key: KeyCode, screen_open: bool) -> Option<Combo> {
        if screen_open {
            return None;
        }
        match key {
            KeyCode::KeyB => {
                self.hitboxes = !self.hitboxes;
                None
            }
            KeyCode::KeyD => Some(Combo::ClearChat),
            KeyCode::KeyA => Some(Combo::ReloadChunks),
            _ => None,
        }
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

    /// `renderHand`: the first-person hand only in first person.
    pub fn hand(self) -> bool {
        self.world_gui() && self.perspective == 0
    }
}

/// `EntityRenderer.orientCamera` (`bfk.f`): the camera's position, yaw and
/// pitch for a perspective. Third person backs four blocks away along the
/// look (toward the face in front view); eight rays from the eye, offset
/// 0.1 per axis, pull it in to the nearest block. As in vanilla each ray's
/// end adds the z offset to x as well.
pub fn camera(
    world: &World,
    rays: &mut Raycaster,
    eye: [f64; 3],
    yaw: f32,
    pitch: f32,
    perspective: u8,
) -> ([f64; 3], f32, f32) {
    if perspective == 0 {
        return (eye, yaw, pitch);
    }
    let mut distance = 4.0f64;
    let turned = if perspective == 2 {
        pitch + 180.0
    } else {
        pitch
    };
    let rad = |deg: f32| deg / 180.0 * std::f32::consts::PI;
    let dx = f64::from(-trig::sin(rad(yaw)) * trig::cos(rad(turned))) * distance;
    let dz = f64::from(trig::cos(rad(yaw)) * trig::cos(rad(turned))) * distance;
    let dy = f64::from(-trig::sin(rad(turned))) * distance;
    for i in 0..8i32 {
        let offset = |bit: i32| f64::from(((i >> bit & 1) * 2 - 1) as f32 * 0.1);
        let (ox, oy, oz) = (offset(0), offset(1), offset(2));
        let start = [eye[0] + ox, eye[1] + oy, eye[2] + oz];
        let end = [eye[0] - dx + ox + oz, eye[1] - dy + oy, eye[2] - dz + oz];
        if let Some(hit) = rays.trace_blocks(world, start, end) {
            let away = (0..3)
                .map(|a| (hit[a] - eye[a]).powi(2))
                .sum::<f64>()
                .sqrt();
            distance = distance.min(away);
        }
    }
    let (yaw, pitch) = if perspective == 2 {
        (yaw + 180.0, -pitch)
    } else {
        (yaw, pitch)
    };
    let (y, p) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    let forward = [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()];
    (
        std::array::from_fn(|a| eye[a] - forward[a] * distance),
        yaw,
        pitch,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        use horizon_world_189::{ColumnData, ColumnUpdate};
        let mut world = World::default();
        for z in [-1, 0] {
            world.apply(ColumnUpdate::Data {
                x: 0,
                z,
                full: true,
                data: Box::new(ColumnData {
                    sections: Default::default(),
                    biomes: None,
                }),
            });
        }
        world
    }

    #[test]
    fn f5_cycles_behind_in_front_and_back_and_hides_the_hand() {
        let mut view = View::default();
        assert!(view.hand());
        let modes: Vec<u8> = (0..4)
            .map(|_| {
                view.key(KeyCode::F5, false);
                view.perspective
            })
            .collect();
        assert_eq!(modes, [1, 2, 0, 1]);
        assert!(!view.hand());
        assert!(!view.key(KeyCode::F5, true));
        assert_eq!(view.perspective, 1);
    }

    #[test]
    fn the_third_person_camera_backs_off_four_blocks_and_stops_at_walls() {
        let mut world = world();
        let mut rays = Raycaster::default();
        let eye = [0.5, 1.5, 0.5];
        assert_eq!(camera(&world, &mut rays, eye, 0.0, 0.0, 0), (eye, 0.0, 0.0));
        // Facing +z: behind is -z, in front +z with the look turned round.
        let (at, yaw, pitch) = camera(&world, &mut rays, eye, 0.0, 0.0, 1);
        assert!(
            (at[2] - (0.5 - 4.0)).abs() < 1e-9 && (yaw, pitch) == (0.0, 0.0),
            "{at:?}"
        );
        let (at, yaw, pitch) = camera(&world, &mut rays, eye, 0.0, 30.0, 2);
        assert_eq!((yaw, pitch), (180.0, -30.0));
        assert!(at[2] > 0.5 + 3.0 && at[1] < 1.5 - 1.9, "{at:?}");
        // Stone two and a half blocks behind the eye pulls the camera in.
        world.set_block(0, 1, -3, horizon_world_189::BlockState(1 << 4));
        world.set_block(0, 2, -3, horizon_world_189::BlockState(1 << 4));
        let (at, _, _) = camera(&world, &mut rays, eye, 0.0, 0.0, 1);
        let pulled = 0.5 - at[2];
        assert!((2.5..2.52).contains(&pulled), "{pulled}");
    }

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
        assert!(view.key(KeyCode::F3, false) && view.debug);
        assert!(!view.key(KeyCode::F3, true) && view.debug);
        assert_eq!(view.combo(KeyCode::KeyB, false), None);
        assert!(view.hitboxes);
        assert_eq!(view.combo(KeyCode::KeyB, true), None);
        assert!(view.hitboxes, "a screen takes the key");
        assert_eq!(view.combo(KeyCode::KeyD, false), Some(Combo::ClearChat));
        assert_eq!(view.combo(KeyCode::KeyA, false), Some(Combo::ReloadChunks));
        assert_eq!(view.combo(KeyCode::KeyQ, false), None);
        assert!(view.key(KeyCode::F1, false));
        assert!(!view.hide_gui);
    }
}
