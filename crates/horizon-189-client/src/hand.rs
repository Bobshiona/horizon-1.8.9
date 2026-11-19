//! `ItemRenderer.renderItemInFirstPerson` and `EntityRenderer`'s camera
//! effects (hurt tilt, death roll, view bobbing). The hand is built in view
//! space and drawn after the world with its own 70-degree projection and a
//! cleared depth buffer, as `renderHand` does.
use crate::terrain::{Registry, Vertex};
use glam::{Mat4, Vec3};
use horizon_physics_189::player::Player;
use horizon_protocol_47::nbt::Item;
use std::f32::consts::PI;

/// The per-tick state the hand and camera interpolate between.
#[derive(Clone, Debug, Default)]
pub struct Tracked {
    /// `itemToRender`.
    pub item: Option<Item>,
    /// Previous and current `equippedProgress`.
    pub equip: [f32; 2],
    /// Previous and current `renderArmPitch`/`renderArmYaw`.
    pub arm: [[f32; 2]; 2],
    /// Previous and current `distanceWalkedModified`.
    pub walked: [f32; 2],
    /// Previous and current `cameraYaw`/`cameraPitch`.
    pub camera: [[f32; 2]; 2],
    /// `hurtTime`, set to ten by damage and counted down each tick.
    pub hurt: u8,
    /// `deathTime` while dead.
    pub dead: Option<u32>,
    /// Ticks the held item has been in use.
    pub using: Option<u32>,
}

fn same_stack(a: &Item, b: &Item) -> bool {
    a.id == b.id && a.count == b.count && a.damage == b.damage && a.nbt == b.nbt
}

impl Tracked {
    /// `ItemRenderer.updateEquippedItem`, `EntityPlayerSP`'s arm sway and the
    /// player's bobbing state, once per client tick.
    pub fn tick(&mut self, held: Option<&Item>, player: &Player, using: Option<u32>, alive: bool) {
        self.equip[0] = self.equip[1];
        let changed = match (&self.item, held) {
            (Some(a), Some(b)) => !same_stack(a, b),
            (None, None) => false,
            _ => true,
        };
        let target = if changed { 0.0 } else { 1.0 };
        self.equip[1] += (target - self.equip[1]).clamp(-0.4, 0.4);
        if self.equip[1] < 0.1 {
            self.item = held.cloned();
        }
        self.arm[0] = self.arm[1];
        self.arm[1][0] += (player.pitch - self.arm[1][0]) * 0.5;
        self.arm[1][1] += (player.yaw - self.arm[1][1]) * 0.5;
        self.walked = [player.previous_distance_walked, player.distance_walked];
        self.camera = [player.previous_camera, player.camera];
        self.hurt = self.hurt.saturating_sub(1);
        self.dead = if alive {
            None
        } else {
            Some(self.dead.map_or(0, |ticks| (ticks + 1).min(20)))
        };
        // Starting to use an item re-equips it (`resetEquippedProgress2`).
        if using == Some(0) && self.using.is_none() {
            self.equip[1] = 0.0;
        }
        self.using = using;
    }

    pub fn hurt(&mut self) {
        self.hurt = 10;
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn rotate(degrees: f32, axis: Vec3) -> Mat4 {
    Mat4::from_axis_angle(axis, degrees.to_radians())
}

const X: Vec3 = Vec3::X;
const Y: Vec3 = Vec3::Y;
const Z: Vec3 = Vec3::Z;

/// `hurtCameraEffect` then `setupViewBobbing`, applied before the camera
/// orientation (and to the hand).
pub fn camera_effects(state: &Tracked, partial: f32, bobbing: bool) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    if let Some(dead) = state.dead {
        let time = dead as f32 + partial;
        m *= rotate(40.0 - 8000.0 / (time + 200.0), Z);
    }
    let hurt = f32::from(state.hurt) - partial;
    if hurt >= 0.0 {
        let f = (hurt / 10.0).powi(4) * PI;
        // attackedAtYaw is zero for the local player in 1.8.9.
        m *= rotate(-f.sin() * 14.0, Z);
    }
    if bobbing {
        let walked = -lerp(state.walked[0], state.walked[1], partial);
        let yaw = lerp(state.camera[0][0], state.camera[1][0], partial);
        let pitch = lerp(state.camera[0][1], state.camera[1][1], partial);
        m *= Mat4::from_translation(Vec3::new(
            (walked * PI).sin() * yaw * 0.5,
            -((walked * PI).cos() * yaw).abs(),
            0.0,
        ));
        m *= rotate((walked * PI).sin() * yaw * 3.0, Z);
        m *= rotate(((walked * PI - 0.2).cos() * yaw).abs() * 5.0, X);
        m *= rotate(pitch, X);
    }
    m
}

/// `Item.getItemUseAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UseAction {
    Eat,
    Drink,
    Block,
    Bow,
}

fn use_action(item: &Item) -> Option<UseAction> {
    match item.id {
        267 | 268 | 272 | 276 | 283 => Some(UseAction::Block),
        261 => Some(UseAction::Bow),
        335 => Some(UseAction::Drink),
        373 if item.damage & 16384 == 0 => Some(UseAction::Drink),
        260
        | 282
        | 297
        | 319
        | 320
        | 322
        | 349
        | 350
        | 357
        | 360
        | 363..=367
        | 391..=394
        | 396
        | 400
        | 411..=413
        | 423
        | 424 => Some(UseAction::Eat),
        _ => None,
    }
}

/// `transformFirstPersonItem`.
fn transform_item(m: Mat4, equip: f32, swing: f32) -> Mat4 {
    let f = (swing * swing * PI).sin();
    let f1 = (swing.sqrt() * PI).sin();
    m * Mat4::from_translation(Vec3::new(0.56, -0.52, -0.71999997))
        * Mat4::from_translation(Vec3::new(0.0, equip * -0.6, 0.0))
        * rotate(45.0, Y)
        * rotate(f * -20.0, Y)
        * rotate(f1 * -20.0, Z)
        * rotate(f1 * -80.0, X)
        * Mat4::from_scale(Vec3::splat(0.4))
}

/// `doItemUsedTransformations`: the swing's lunge.
fn swing_lunge(m: Mat4, swing: f32) -> Mat4 {
    let root = swing.sqrt() * PI;
    m * Mat4::from_translation(Vec3::new(
        -0.4 * root.sin(),
        0.2 * (root * 2.0).sin(),
        -0.2 * (swing * PI).sin(),
    ))
}

/// `doBlockTransformations`: the sword held across the body.
fn block_pose(m: Mat4) -> Mat4 {
    m * Mat4::from_translation(Vec3::new(-0.5, 0.2, 0.0))
        * rotate(30.0, Y)
        * rotate(-80.0, X)
        * rotate(60.0, Y)
}

/// `doBowTransformations`; `used` is the ticks drawn plus the partial tick.
fn bow_pose(m: Mat4, used: f32) -> Mat4 {
    let mut m = m
        * rotate(-18.0, Z)
        * rotate(-12.0, Y)
        * rotate(-8.0, X)
        * Mat4::from_translation(Vec3::new(-0.9, 0.2, 0.0));
    let f = used - 1.0;
    let mut f1 = f / 20.0;
    f1 = ((f1 * f1 + f1 * 2.0) / 3.0).min(1.0);
    if f1 > 0.1 {
        let shake = (f - 0.1) * 1.3;
        m *= Mat4::from_translation(Vec3::new(0.0, shake.sin() * (f1 - 0.1) * 0.01, 0.0));
    }
    m * Mat4::from_translation(Vec3::new(0.0, 0.0, f1 * 0.1))
        * Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0 + f1 * 0.2))
}

/// `performDrinking`, for food and drinks (32-tick use).
fn drink_pose(m: Mat4, used: f32) -> Mat4 {
    let left = 32.0 - used + 1.0;
    let fraction = left / 32.0;
    let mut bob = ((left / 4.0 * PI).cos() * 0.1).abs();
    if fraction >= 0.8 {
        bob = 0.0;
    }
    let lift = 1.0 - fraction.powf(27.0);
    m * Mat4::from_translation(Vec3::new(0.0, bob, 0.0))
        * Mat4::from_translation(Vec3::new(lift * 0.6, lift * -0.5, 0.0))
        * rotate(lift * 90.0, Y)
        * rotate(lift * 10.0, X)
        * rotate(lift * 30.0, Z)
}

/// Blocks whose inventory model is a flat sprite rather than a cube.
pub(crate) fn flat_block(id: i16) -> bool {
    matches!(
        id,
        6 | 27
            | 28
            | 30
            | 31
            | 32
            | 37
            | 38
            | 39
            | 40
            | 50
            | 51
            | 55
            | 59
            | 63
            | 65
            | 66
            | 68
            | 69
            | 75
            | 76
            | 83
            | 101
            | 102
            | 104
            | 105
            | 106
            | 111
            | 115
            | 131
            | 132
            | 141
            | 142
            | 157
            | 160
            | 175
    )
}

/// The held stack after its `FIRST_PERSON` camera transform.
fn held_item(out: &mut Vec<Vertex>, m: Mat4, item: &Item, registry: &Registry, used: Option<f32>) {
    if (1..198).contains(&item.id) && !flat_block(item.id) {
        // `ItemRenderer.renderItem` doubles 3D models, block item models
        // carry no first-person transform except these few, and
        // `RenderItem.renderItem` halves every model.
        let display = match item.id {
            // Fence gates: rotation [0, 90, 0].
            107 | 183..=187 => rotate(90.0, Y),
            // Carpets and snow layers: translation [0, 5.25, 0].
            78 | 171 => Mat4::from_translation(Vec3::new(0.0, 5.25 / 16.0, 0.0)),
            // Trapdoors and daylight sensors: translation [0, 5, 0].
            96 | 151 | 167 | 178 => Mat4::from_translation(Vec3::new(0.0, 5.0 / 16.0, 0.0)),
            _ => Mat4::IDENTITY,
        };
        let m =
            m * Mat4::from_scale(Vec3::splat(2.0)) * display * Mat4::from_scale(Vec3::splat(0.5));
        registry.held_block(out, item, m);
        return;
    }
    // Generated item models: rotation [0, -135, 25] ([0, 45, 25] for the
    // fishing rod and carrot on a stick), translation [0, 4, 2] / 16,
    // scale 1.7; then `RenderItem.renderItem`'s half scale and -0.5 offset.
    let yaw = if matches!(item.id, 346 | 398) {
        45.0
    } else {
        -135.0
    };
    let m = m
        * Mat4::from_translation(Vec3::new(0.0, 0.25, 0.125))
        * rotate(yaw, Y)
        * rotate(25.0, Z)
        * Mat4::from_scale(Vec3::splat(1.7 * 0.5))
        * Mat4::from_translation(Vec3::splat(-0.5));
    let sprite = |name| registry.sprite(name);
    let tint = |color: u32| {
        let [r, g, b, _] = crate::ui::argb(color | 0xFF00_0000);
        [r, g, b]
    };
    let layers: Vec<(Option<u32>, [f32; 3])> = match item.id {
        261 => {
            let pulling = used.map(|t| t as u32).and_then(|t| match t {
                18.. => sprite("bow_pulling_2"),
                14..=17 => sprite("bow_pulling_1"),
                1..=13 => sprite("bow_pulling_0"),
                _ => None,
            });
            vec![(pulling.or_else(|| registry.item_layer(item)), [1.0; 3])]
        }
        373 => vec![
            (
                sprite("potion_overlay"),
                tint(crate::tooltip::potion_color(item)),
            ),
            (
                sprite(if item.damage & 16384 != 0 {
                    "potion_bottle_splash"
                } else {
                    "potion_bottle_drinkable"
                }),
                [1.0; 3],
            ),
        ],
        298..=301 => {
            let index = (item.id - 298) as usize;
            vec![
                (
                    sprite(
                        [
                            "leather_helmet",
                            "leather_chestplate",
                            "leather_leggings",
                            "leather_boots",
                        ][index],
                    ),
                    tint(crate::tooltip::leather_color(item)),
                ),
                (
                    sprite(
                        [
                            "leather_helmet_overlay",
                            "leather_chestplate_overlay",
                            "leather_leggings_overlay",
                            "leather_boots_overlay",
                        ][index],
                    ),
                    [1.0; 3],
                ),
            ]
        }
        _ => vec![(registry.item_layer(item), [1.0; 3])],
    };
    for (layer, color) in layers {
        if let Some(layer) = layer {
            registry.item_mesh(out, layer, m, color);
        }
    }
}

/// `renderPlayerArm` with `RenderPlayer.renderRightArm`.
fn arm(
    out: &mut Vec<Vertex>,
    m: Mat4,
    equip: f32,
    swing: f32,
    skin: u32,
    slim: bool,
    sleeve: bool,
) {
    let root = swing.sqrt() * PI;
    let m =
        m * Mat4::from_translation(Vec3::new(
            -0.3 * root.sin(),
            0.4 * (root * 2.0).sin(),
            -0.4 * (swing * PI).sin(),
        )) * Mat4::from_translation(Vec3::new(0.64000005, -0.6, -0.71999997))
            * Mat4::from_translation(Vec3::new(0.0, equip * -0.6, 0.0))
            * rotate(45.0, Y)
            * rotate(root.sin() * 70.0, Y)
            * rotate((swing * swing * PI).sin() * -20.0, Z)
            * Mat4::from_translation(Vec3::new(-1.0, 3.6, 3.5))
            * rotate(120.0, Z)
            * rotate(200.0, X)
            * rotate(-135.0, Y)
            * Mat4::from_translation(Vec3::new(5.6, 0.0, 0.0));
    crate::models::first_person_arm(out, m, skin, slim, sleeve);
}

/// What the window knows this frame about the local player.
pub struct Frame<'a> {
    pub state: &'a Tracked,
    pub partial: f32,
    /// Arm swing progress, 0 to 1.
    pub swing: f32,
    pub skin: u32,
    pub slim: bool,
    pub sleeve: bool,
    pub invisible: bool,
    pub bobbing: bool,
    /// The camera's pitch and yaw this frame.
    pub look: [f32; 2],
}

/// The first-person hand and held item in view space.
pub fn draw(out: &mut Vec<Vertex>, frame: &Frame<'_>, registry: &Registry) {
    let state = frame.state;
    let t = frame.partial;
    let equip = 1.0 - lerp(state.equip[0], state.equip[1], t);
    // rotateWithPlayerRotations: the arm lags the look.
    let [pitch, yaw] = frame.look;
    let arm_pitch = lerp(state.arm[0][0], state.arm[1][0], t);
    let arm_yaw = lerp(state.arm[0][1], state.arm[1][1], t);
    let mut m = camera_effects(state, t, frame.bobbing)
        * rotate((pitch - arm_pitch) * 0.1, X)
        * rotate((yaw - arm_yaw) * 0.1, Y);
    let used = state.using.map(|ticks| ticks as f32 + t);
    match &state.item {
        Some(item) => {
            match (used, use_action(item)) {
                (Some(used), Some(UseAction::Eat | UseAction::Drink)) => {
                    m = transform_item(drink_pose(m, used), equip, 0.0);
                }
                (Some(_), Some(UseAction::Block)) => m = block_pose(transform_item(m, equip, 0.0)),
                (Some(used), Some(UseAction::Bow)) => {
                    m = bow_pose(transform_item(m, equip, 0.0), used)
                }
                _ => m = transform_item(swing_lunge(m, frame.swing), equip, frame.swing),
            }
            held_item(out, m, item, registry, used);
        }
        None if !frame.invisible => {
            arm(
                out,
                m,
                equip,
                frame.swing,
                frame.skin,
                frame.slim,
                frame.sleeve,
            );
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equip_progress_dips_and_returns_like_update_equipped_item() {
        let player = Player::default();
        let sword = Item {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        };
        let mut state = Tracked {
            equip: [1.0; 2],
            ..Tracked::default()
        };
        state.tick(Some(&sword), &player, None, true);
        // A new stack lowers the hand 0.4 per tick before swapping.
        assert!((state.equip[1] - 0.6).abs() < 1e-6);
        state.tick(Some(&sword), &player, None, true);
        state.tick(Some(&sword), &player, None, true);
        assert_eq!(state.item.as_ref().map(|i| i.id), Some(276));
        // Then it rises 0.4 per tick back to fully equipped.
        for _ in 0..3 {
            state.tick(Some(&sword), &player, None, true);
        }
        assert!((state.equip[1] - 1.0).abs() < 1e-6);
        // Starting to block re-equips once.
        state.tick(Some(&sword), &player, Some(0), true);
        assert_eq!(state.equip[1], 0.0);
        state.hurt();
        state.tick(Some(&sword), &player, Some(1), true);
        assert_eq!(state.hurt, 9);
        assert_eq!(use_action(&sword), Some(UseAction::Block));
    }

    #[test]
    fn hurt_tilt_peaks_and_bobbing_is_still_on_the_spot() {
        let mut state = Tracked {
            hurt: 10,
            ..Tracked::default()
        };
        assert!(camera_effects(&state, 0.0, true).abs_diff_eq(Mat4::IDENTITY, 1e-5));
        state.hurt = 8;
        let tilted = camera_effects(&state, 0.0, true);
        assert!(tilted.x_axis.y.abs() > 0.1, "roll about Z");
        state.hurt = 0;
        state.camera = [[0.1, 0.0]; 2];
        state.walked = [0.5; 2];
        let bob = camera_effects(&state, 0.0, true);
        assert_ne!(bob, Mat4::IDENTITY);
        assert_eq!(camera_effects(&state, 0.0, false), Mat4::IDENTITY);
    }
}
