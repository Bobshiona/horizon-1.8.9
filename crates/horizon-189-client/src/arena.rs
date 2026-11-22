//! The launcher's live scene: your skin, holding a diamond sword, on a
//! small floating island under the 1.8.9 sky, drawn by the same renderer as
//! the game. Each launcher page has its own camera framing; switching pages
//! glides the camera there, and the player's head follows the pointer.
use crate::{launcher::Page, network::MeshUpdate, terrain::Registry};
use horizon_protocol_47::{nbt::Item, play::PositionLook, player_list::Profile};
use horizon_world_189::{
    BlockState, ColumnData, ColumnUpdate, World,
    entities::{Entities, Entity},
};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

const fn block(id: u16, meta: u16) -> BlockState {
    BlockState(id << 4 | meta)
}

const STONE: BlockState = block(1, 0);
const GRASS: BlockState = block(2, 0);
const DIRT: BlockState = block(3, 0);
const GRAVEL: BlockState = block(13, 0);
const LOG: BlockState = block(17, 0);
const LEAVES: BlockState = block(18, 0);
const TALL_GRASS: BlockState = block(31, 1);
const DANDELION: BlockState = block(37, 0);
const POPPY: BlockState = block(38, 0);

/// The island's top surface; the player stands on it at the centre.
const TOP: i32 = 64;
const FEET: [f64; 3] = [0.5, TOP as f64 + 1.0, 0.5];
const RADIUS: f32 = 6.5;

/// A fixed hash for the island's shape, so it is the same every launch.
fn noise(x: i32, z: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (z as u32).wrapping_mul(0x85EB_CA77)
        ^ salt.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// The island: grass over dirt over a stone underside that tapers to a
/// point, a few flowers, and a small oak behind the player.
pub fn world() -> World {
    let mut world = World::default();
    for x in -1..=0 {
        for z in -1..=0 {
            world.apply(ColumnUpdate::Data {
                x,
                z,
                full: true,
                data: Box::new(ColumnData {
                    sections: Default::default(),
                    biomes: Some(Box::new([1; 256])),
                }),
            });
        }
    }
    let r = RADIUS as i32 + 1;
    for x in -r..=r {
        for z in -r..=r {
            let edge = RADIUS - 0.8 + noise(x, z, 1) * 1.2;
            let d = ((x * x + z * z) as f32).sqrt();
            if d > edge {
                continue;
            }
            let depth = ((edge - d) * 1.4 + noise(x, z, 2) * 2.0) as i32 + 1;
            for y in TOP - depth..=TOP {
                let state = match TOP - y {
                    0 => GRASS,
                    1 | 2 => DIRT,
                    _ if noise(x, y, 3) < 0.08 => GRAVEL,
                    _ => STONE,
                };
                world.set_block(x, y, z, state);
            }
            // Flowers and grass, kept off the player's spot.
            let roll = noise(x, z, 4);
            if x * x + z * z > 4 && d < edge - 0.6 {
                let plant = match roll {
                    r if r < 0.07 => Some(POPPY),
                    r if r < 0.12 => Some(DANDELION),
                    r if r < 0.20 => Some(TALL_GRASS),
                    _ => None,
                };
                if let Some(plant) = plant {
                    world.set_block(x, TOP + 1, z, plant);
                }
            }
        }
    }
    // A small oak.
    let (tx, tz) = (-3, -3);
    for y in TOP + 1..=TOP + 4 {
        world.set_block(tx, y, tz, LOG);
    }
    for y in TOP + 3..=TOP + 6 {
        let reach: i32 = if y >= TOP + 5 { 1 } else { 2 };
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                let corner = dx.abs() == reach && dz.abs() == reach;
                if corner && (y == TOP + 6 || noise(dx, dz, y as u32) < 0.5) {
                    continue;
                }
                if (dx, dz) != (0, 0) || y > TOP + 4 {
                    world.set_block(tx + dx, y, tz + dz, LEAVES);
                }
            }
        }
    }
    world
}

/// The island's meshes, one per column.
pub fn meshes(registry: &Registry, world: &World) -> Vec<MeshUpdate> {
    let origin = [0, 0, 0];
    world
        .columns()
        .map(|(key, _)| MeshUpdate {
            key,
            origin,
            mesh: Some(registry.mesh_column(world, key, origin)),
            latency: std::time::Duration::ZERO,
        })
        .collect()
}

/// A camera framing around the player: the angle around them (0 is in
/// front, degrees), distance, eye height above the chest, and how far right
/// of the player the camera looks, which moves them left of centre so the
/// page has room on the right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub angle: f32,
    pub distance: f32,
    pub height: f32,
    pub shift: f32,
}

impl Shot {
    pub fn of(page: Page) -> Self {
        let (angle, distance, height, shift) = match page {
            Page::Home => (30.0, 3.5, -0.1, 1.35),
            Page::Servers => (68.0, 7.5, 2.0, 3.3),
            Page::News => (-38.0, 6.2, 0.1, 2.7),
            Page::Mods => (100.0, 9.5, 3.2, 4.6),
            Page::Settings => (140.0, 6.8, 4.2, 2.6),
        };
        Self {
            angle,
            distance,
            height,
            shift,
        }
    }

    fn lerp(self, to: Shot, t: f32) -> Shot {
        if t >= 1.0 {
            return to;
        }
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Shot {
            angle: mix(self.angle, to.angle),
            distance: mix(self.distance, to.distance),
            height: mix(self.height, to.height),
            shift: mix(self.shift, to.shift),
        }
    }

    /// The camera for this framing, `drift` degrees around.
    pub fn camera(self, drift: f32) -> PositionLook {
        let chest = [FEET[0], FEET[1] + 1.1, FEET[2]];
        let a = f64::from(self.angle + drift).to_radians();
        let d = f64::from(self.distance);
        let eye = [
            chest[0] + a.sin() * d,
            chest[1] + f64::from(self.height),
            chest[2] + a.cos() * d,
        ];
        // Screen right, facing the chest, is (-forward.z, 0, forward.x).
        let (fx, fz) = (chest[0] - eye[0], chest[2] - eye[2]);
        let len = (fx * fx + fz * fz).sqrt().max(1e-6);
        let shift = f64::from(self.shift);
        let target = [
            chest[0] - fz / len * shift,
            chest[1],
            chest[2] + fx / len * shift,
        ];
        let (x, y, z) = (target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]);
        let flat = (x * x + z * z).sqrt();
        PositionLook {
            position: eye,
            yaw: (-x).atan2(z).to_degrees() as f32,
            pitch: (-y).atan2(flat).to_degrees() as f32,
        }
    }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Seconds a page change takes to glide the camera.
const GLIDE: f32 = 0.9;

pub struct Arena {
    world: World,
    started: Instant,
    page: Page,
    from: Shot,
    changed: Instant,
    player: Entity,
    /// The signed-in player's profile with skin textures, once fetched.
    profile: Arc<Mutex<Option<(u128, Arc<Profile>)>>>,
    /// Meshes go to the renderer once.
    pub meshed: bool,
}

impl Arena {
    pub fn new() -> Self {
        let mut tracked = Entities::default();
        tracked.apply(horizon_protocol_47::entities::EntityPacket::SpawnPlayer {
            id: 0,
            uuid: 0,
            position: [0; 3],
            yaw: 0,
            pitch: 0,
            held_item: 0,
            metadata: vec![],
        });
        let mut player = tracked.get(0).cloned().expect("SpawnPlayer makes a player");
        player.skin_parts = 0x7f;
        player.position = FEET;
        player.previous = FEET;
        player.equipment[0] = Some(Item {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        });
        Self {
            world: world(),
            started: Instant::now(),
            page: Page::Home,
            from: Shot::of(Page::Home),
            changed: Instant::now(),
            player,
            profile: Arc::new(Mutex::new(None)),
            meshed: false,
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// Show this player's own skin: fetch their profile in the background.
    /// The lookup is public (Mojang's profile service) and takes no token.
    pub fn show_skin(&self, uuid: u128) {
        if self
            .profile
            .lock()
            .is_ok_and(|p| p.as_ref().is_some_and(|(u, _)| *u == uuid))
        {
            return;
        }
        let shared = self.profile.clone();
        let _ = std::thread::Builder::new()
            .name("Arena skin".into())
            .spawn(move || {
                if let Some(profile) = crate::skins::profile(uuid)
                    && let Ok(mut slot) = shared.lock()
                {
                    *slot = Some((uuid, Arc::new(profile)));
                }
            });
    }

    /// The camera framing now, gliding from the last page's.
    fn shot(&mut self, page: Page) -> Shot {
        let t = ease(self.changed.elapsed().as_secs_f32() / GLIDE);
        let current = self.from.lerp(Shot::of(self.page), t);
        if page != self.page {
            self.from = current;
            self.page = page;
            self.changed = Instant::now();
            return current;
        }
        current
    }

    /// This frame's camera and player. `pointer` is the mouse across the
    /// window, -1 to 1 each way.
    pub fn frame(&mut self, page: Page, pointer: (f32, f32)) -> (PositionLook, Entity) {
        let seconds = self.started.elapsed().as_secs_f32();
        let shot = self.shot(page);
        let drift = (seconds * 0.12).sin() * 7.0;
        let camera = shot.camera(drift);
        let player = &mut self.player;
        // Face the camera, turned a little for a three-quarter pose; the head
        // turns toward the pointer (screen right is the player's left).
        let (dx, dz) = (camera.position[0] - FEET[0], camera.position[2] - FEET[2]);
        let face = (-dx).atan2(dz).to_degrees() as f32;
        let body = face - 24.0;
        let head = face - pointer.0.clamp(-1.0, 1.0) * 40.0;
        let pitch = pointer.1.clamp(-1.0, 1.0) * 25.0;
        player.body_yaw = body;
        player.previous_body_yaw = body;
        player.yaw = body;
        player.head_yaw = head;
        player.previous_head_yaw = head;
        player.pitch = pitch;
        player.previous_pitch = pitch;
        player.age = (seconds * 20.0) as u32;
        if let Ok(profile) = self.profile.lock()
            && let Some((uuid, profile)) = profile.as_ref()
        {
            player.uuid = *uuid;
            player.profile = Some(profile.clone());
        }
        (camera, self.player.clone())
    }

    /// The fetched profile for the renderer's skin loader.
    pub fn own(&self) -> Option<(u128, Arc<Profile>)> {
        self.profile.lock().ok().and_then(|p| p.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_island_is_solid_under_the_player_and_floats() {
        let world = world();
        assert_eq!(world.block(0, TOP, 0), Some(GRASS));
        assert_eq!(world.block(0, TOP + 1, 0), Some(BlockState(0)));
        assert!(
            world
                .block(0, TOP - 3, 0)
                .is_some_and(|b| b != BlockState(0))
        );
        // Air under the point and beyond the edge.
        assert_eq!(world.block(0, TOP - 20, 0), Some(BlockState(0)));
        assert_eq!(world.block(9, TOP, 9), Some(BlockState(0)));
        assert_eq!(world.block(-3, TOP + 2, -3), Some(LOG));
        // The same island every launch.
        let again = super::world();
        for x in -8..8 {
            for z in -8..8 {
                assert_eq!(world.block(x, TOP, z), again.block(x, TOP, z));
            }
        }
    }

    #[test]
    fn every_page_frames_the_player_left_of_centre() {
        for page in Page::ALL {
            let shot = Shot::of(page);
            let camera = shot.camera(0.0);
            let eye = camera.position;
            let (yaw, pitch) = (camera.yaw.to_radians(), camera.pitch.to_radians());
            let forward = [
                -yaw.sin() * pitch.cos(),
                -pitch.sin(),
                yaw.cos() * pitch.cos(),
            ];
            let right = [-forward[2], 0.0, forward[0]];
            let chest = [FEET[0], FEET[1] + 1.1, FEET[2]];
            let to = [
                (chest[0] - eye[0]) as f32,
                (chest[1] - eye[1]) as f32,
                (chest[2] - eye[2]) as f32,
            ];
            let ahead: f32 = (0..3).map(|i| to[i] * forward[i]).sum();
            let across: f32 = (0..3).map(|i| to[i] * right[i]).sum();
            assert!(ahead > 1.0, "{page:?}: the player is in front");
            assert!(across < -0.5, "{page:?}: the player is left of centre");
            assert!(
                eye[1] > chest[1] - 1.0,
                "{page:?}: not from below the island"
            );
        }
    }

    #[test]
    fn changing_page_glides_from_where_the_camera_was() {
        let mut arena = Arena::new();
        let home = arena.shot(Page::Home);
        assert_eq!(home, Shot::of(Page::Home));
        // The first frame on a new page starts where the last one was.
        let start = arena.shot(Page::Servers);
        assert_eq!(start, home);
        arena.changed = Instant::now() - std::time::Duration::from_secs(5);
        assert_eq!(arena.shot(Page::Servers), Shot::of(Page::Servers));
    }
}
