//! Tracked entities as the 1.8.9 client keeps them: 1/32-block server
//! positions, three-step `setPositionAndRotation2` interpolation for living
//! entities, watcher flags, equipment, arm swings and the body-yaw and limb
//! animation state `EntityLivingBase` derives every tick.
use horizon_protocol_47::{
    entities::{EntityPacket, angle},
    nbt::{Item, Meta},
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Player { slim: bool },
    Mob(u8),
    Object(u8),
    Orb,
}

#[derive(Clone, Debug)]
pub struct Entity {
    pub kind: Kind,
    pub uuid: u128,
    pub profile: Option<std::sync::Arc<horizon_protocol_47::player_list::Profile>>,
    pub skin_parts: u8,
    server: [i32; 3],
    target: [f64; 3],
    target_yaw: f32,
    target_pitch: f32,
    steps: u8,
    pub position: [f64; 3],
    pub previous: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub previous_pitch: f32,
    pub head_yaw: f32,
    pub previous_head_yaw: f32,
    pub body_yaw: f32,
    pub previous_body_yaw: f32,
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub previous_limb_amount: f32,
    /// Watcher index 0: fire 0x01, sneaking 0x02, sprinting 0x08, using 0x10,
    /// invisible 0x20.
    pub flags: u8,
    /// Held item, boots, leggings, chestplate, helmet.
    pub equipment: [Option<Item>; 5],
    /// Ticks the using flag (0x10) has been set: how far a bow is drawn.
    pub use_ticks: u32,
    /// `swingProgressInt` ticks of the six-tick arm swing, if swinging.
    pub swing: Option<u8>,
    pub age: u32,
    /// Remaining vanilla hurt-flash ticks, refreshed by entity status 2.
    pub hurt_time: u8,
    pub health: f32,
    pub variant: u8,
    pub child: bool,
    pub on_ground: bool,
    /// Watcher index 2 `CustomName` and index 3 `AlwaysShowNameTag`.
    pub custom_name: Option<String>,
    pub name_visible: bool,
    /// Armour-stand flags, index 10: small 0x01, marker 0x10.
    pub stand_flags: u8,
    /// Client-side motion of simulated objects, blocks per tick.
    pub motion: [f64; 3],
    /// A dropped item's stack (index 10).
    pub item: Option<Item>,
    /// An arrow's critical flag (index 16).
    pub critical: bool,
    /// An arrow or throwable that hit a block.
    pub stuck: bool,
    /// S0E object data: a falling block's `id | meta << 12`, a shooter...
    pub data: i32,
    /// S0D: who is picking this up and the ticks since (`EntityPickupFX`
    /// runs three). The entity outlives its destroy packet until then.
    pub collector: Option<(i32, u8)>,
}

fn solid(world: &crate::World, at: [f64; 3]) -> bool {
    let [x, y, z] = at.map(|v| v.floor() as i32);
    world.block(x, y, z).is_some_and(|state| {
        let id = state.block_id();
        id != 0 && crate::blocks::block(id).layer == crate::blocks::Layer::Solid
    })
}

impl Entity {
    /// The client half of `onUpdate` for objects whose server updates are
    /// sparse: gravity, drag, and stopping at solid blocks.
    fn simulate(&mut self, world: &crate::World) {
        let Kind::Object(kind) = self.kind else {
            return;
        };
        if self.collector.is_some() {
            return;
        }
        let step = |position: [f64; 3], motion: [f64; 3]| {
            [
                position[0] + motion[0],
                position[1] + motion[1],
                position[2] + motion[2],
            ]
        };
        match kind {
            // EntityArrow and EntityThrowable: fly until a block, then stop.
            60 | 61 | 62 | 65 | 73 | 75 => {
                if self.stuck {
                    return;
                }
                let next = step(self.position, self.motion);
                if solid(world, next) {
                    self.stuck = true;
                    self.motion = [0.0; 3];
                    return;
                }
                self.position = next;
                if kind == 60 {
                    // EntityArrow faces along its motion while flying.
                    let [x, y, z] = self.motion;
                    self.yaw = x.atan2(z).to_degrees() as f32;
                    self.pitch = y.atan2((x * x + z * z).sqrt()).to_degrees() as f32;
                }
                let (drag, gravity) = match kind {
                    60 => (0.99, 0.05),
                    73 => (0.99, 0.05),
                    75 => (0.99, 0.07),
                    _ => (0.99, 0.03),
                };
                self.motion = self.motion.map(|v| v * drag);
                self.motion[1] -= gravity;
            }
            // EntityItem, EntityTNTPrimed, EntityFallingBlock: fall, land, slide.
            2 | 50 | 70 => {
                self.motion[1] -= 0.04;
                let mut move_by = self.motion;
                let down = [
                    self.position[0],
                    self.position[1] + move_by[1],
                    self.position[2],
                ];
                self.on_ground = false;
                if move_by[1] < 0.0 && solid(world, down) {
                    move_by[1] = (down[1].floor() + 1.0) - self.position[1];
                    self.on_ground = true;
                }
                let side = [
                    self.position[0] + move_by[0],
                    self.position[1] + move_by[1] + 0.01,
                    self.position[2] + move_by[2],
                ];
                if solid(world, side) {
                    move_by[0] = 0.0;
                    move_by[2] = 0.0;
                }
                self.position = step(self.position, move_by);
                if self.on_ground {
                    self.motion[1] = 0.0;
                }
                let ground = match (kind, self.on_ground) {
                    (2, true) => 0.6 * 0.98,
                    (50, true) => 0.98 * 0.7,
                    _ => 0.98,
                };
                self.motion = [
                    self.motion[0] * ground,
                    self.motion[1] * 0.98,
                    self.motion[2] * ground,
                ];
            }
            // Fireballs keep their heading between sparse updates.
            63 | 64 | 66 => self.position = step(self.position, self.motion),
            _ => {}
        }
    }

    /// Where `renderLivingLabel` starts: the entity height plus half a block.
    pub fn label_height(&self) -> f64 {
        let height = match self.kind {
            Kind::Object(78) if self.stand_flags & 0x10 != 0 => 0.0,
            Kind::Object(78) if self.stand_flags & 0x01 != 0 => 0.9875,
            Kind::Object(78) => 1.975,
            Kind::Player { .. } => 1.8,
            _ => 1.8,
        };
        let height = if self.child { height / 2.0 } else { height };
        height + 0.5
    }

    fn new(kind: Kind, uuid: u128, server: [i32; 3], yaw: f32, pitch: f32) -> Self {
        let position = server.map(|value| f64::from(value) / 32.0);
        Self {
            kind,
            uuid,
            profile: None,
            skin_parts: 0x7f,
            server,
            target: position,
            target_yaw: yaw,
            target_pitch: pitch,
            steps: 0,
            position,
            previous: position,
            yaw,
            pitch,
            previous_pitch: pitch,
            head_yaw: yaw,
            previous_head_yaw: yaw,
            body_yaw: yaw,
            previous_body_yaw: yaw,
            limb_swing: 0.0,
            limb_amount: 0.0,
            previous_limb_amount: 0.0,
            flags: 0,
            equipment: Default::default(),
            use_ticks: 0,
            collector: None,
            swing: None,
            age: 0,
            hurt_time: 0,
            health: 20.0,
            variant: 0,
            child: false,
            on_ground: false,
            custom_name: None,
            name_visible: false,
            stand_flags: 0,
            motion: [0.0; 3],
            item: None,
            critical: false,
            stuck: false,
            data: 0,
        }
    }

    /// Server-sized collision dimensions shared by selection and overlays.
    pub fn dimensions(&self) -> Option<(f32, f32)> {
        let (width, height) = match self.kind {
            Kind::Player { .. } => (0.6, 1.8),
            Kind::Mob(50) => (0.6, 1.7),
            Kind::Mob(51) if self.variant == 1 => (0.72, 2.34),
            Kind::Mob(51 | 54 | 57 | 66) => (0.6, 1.95),
            Kind::Mob(52) => (1.4, 0.9),
            Kind::Mob(53) => (3.6, 10.8),
            Kind::Mob(55 | 62) => {
                let size = f32::from(self.variant.max(1));
                (0.51000005 * size, 0.51000005 * size)
            }
            Kind::Mob(56) => (4.0, 4.0),
            Kind::Mob(58) => (0.6, 2.9),
            Kind::Mob(59) => (0.7, 0.5),
            Kind::Mob(60 | 67) => (0.4, 0.3),
            Kind::Mob(61) => (0.6, 1.8),
            Kind::Mob(63) => (16.0, 8.0),
            Kind::Mob(64) => (0.9, 3.5),
            Kind::Mob(65) => (0.5, 0.9),
            Kind::Mob(68) => (0.85, 0.85),
            Kind::Mob(90) => (0.9, 0.9),
            Kind::Mob(91 | 92 | 96) => (0.9, 1.3),
            Kind::Mob(93) => (0.4, 0.7),
            Kind::Mob(94) => (0.95, 0.95),
            Kind::Mob(95) => (0.6, 0.8),
            Kind::Mob(97) => (0.7, 1.9),
            Kind::Mob(98 | 101) => (0.6, 0.7),
            Kind::Mob(99) => (1.4, 2.9),
            Kind::Mob(100) => (1.4, 1.6),
            Kind::Mob(120) => (0.6, 1.8),
            Kind::Mob(_) => (0.6, 1.8),
            Kind::Object(1) => (1.5, 0.6),
            Kind::Object(10) => (0.98, 0.7),
            Kind::Object(78) => (0.5, 1.975),
            _ => return None,
        };
        Some(if self.child {
            (width * 0.5, height * 0.5)
        } else {
            (width, height)
        })
    }

    pub fn invisible(&self) -> bool {
        self.flags & 0x20 != 0
    }

    pub fn sneaking(&self) -> bool {
        self.flags & 0x02 != 0
    }

    fn living(&self) -> bool {
        matches!(self.kind, Kind::Player { .. } | Kind::Mob(_))
    }

    fn metadata(&mut self, entries: &[(u8, Meta)]) {
        for (index, value) in entries {
            match (index, value) {
                (0, Meta::Byte(flags)) => self.flags = *flags as u8,
                (6, Meta::Float(health)) if self.living() => self.health = *health,
                (12, Meta::Byte(age)) if matches!(self.kind, Kind::Mob(54)) => {
                    self.child = *age != 0
                }
                (12, Meta::Byte(age)) if matches!(self.kind, Kind::Mob(90..=101 | 120)) => {
                    self.child = *age < 0
                }
                (13, Meta::Byte(value)) if self.kind == Kind::Mob(51) => {
                    self.variant = *value as u8
                }
                (16, Meta::Byte(value)) if matches!(self.kind, Kind::Mob(55 | 62 | 91 | 95)) => {
                    self.variant = *value as u8
                }
                (10, Meta::Byte(parts)) if matches!(self.kind, Kind::Player { .. }) => {
                    self.skin_parts = *parts as u8
                }
                (10, Meta::Byte(flags)) if self.kind == Kind::Object(78) => {
                    self.stand_flags = *flags as u8
                }
                (10, Meta::Item(item)) if self.kind == Kind::Object(2) => {
                    self.item = item.clone();
                }
                (16, Meta::Byte(flags)) if self.kind == Kind::Object(60) => {
                    self.critical = flags & 1 != 0;
                }
                (2, Meta::String(name)) => {
                    self.custom_name = (!name.is_empty()).then(|| name.clone());
                }
                (3, Meta::Byte(visible)) => self.name_visible = *visible != 0,
                _ => {}
            }
        }
    }

    /// `setPositionAndRotation2(..., 3, ...)` for living entities; other
    /// entities move immediately.
    fn move_to(&mut self, yaw: Option<f32>, pitch: Option<f32>) {
        let target = self.server.map(|value| f64::from(value) / 32.0);
        let yaw = yaw.unwrap_or(self.target_yaw);
        let pitch = pitch.unwrap_or(self.target_pitch);
        if self.living() {
            self.target = target;
            self.target_yaw = yaw;
            self.target_pitch = pitch;
            self.steps = 3;
        } else {
            self.position = target;
            self.target = target;
            self.yaw = yaw;
            self.pitch = pitch;
            self.target_yaw = yaw;
            self.target_pitch = pitch;
        }
    }

    fn tick(&mut self) {
        self.hurt_time = self.hurt_time.saturating_sub(1);
        self.previous = self.position;
        self.previous_pitch = self.pitch;
        self.previous_head_yaw = self.head_yaw;
        self.previous_body_yaw = self.body_yaw;
        self.previous_limb_amount = self.limb_amount;
        self.age = self.age.wrapping_add(1);
        self.use_ticks = if self.flags & 0x10 != 0 {
            self.use_ticks.saturating_add(1)
        } else {
            0
        };
        if self.steps > 0 {
            let steps = f64::from(self.steps);
            for axis in 0..3 {
                self.position[axis] += (self.target[axis] - self.position[axis]) / steps;
            }
            let turn = wrap(f64::from(self.target_yaw - self.yaw));
            self.yaw = (f64::from(self.yaw) + turn / steps) as f32;
            self.pitch =
                (f64::from(self.pitch) + f64::from(self.target_pitch - self.pitch) / steps) as f32;
            self.steps -= 1;
        }
        if let Some(swing) = &mut self.swing {
            *swing += 1;
            if *swing >= 6 {
                self.swing = None;
            }
        }
        if !self.living() {
            return;
        }
        // EntityLivingBase.onUpdate body yaw and limb swing.
        let dx = self.position[0] - self.previous[0];
        let dz = self.position[2] - self.previous[2];
        let distance = (dx * dx + dz * dz) as f32;
        let mut facing = self.body_yaw;
        if distance > 0.002_500_000_2 {
            facing = (dz.atan2(dx) as f32).to_degrees() - 90.0;
        }
        if self.swing.is_some() {
            facing = self.yaw;
        }
        self.body_yaw += wrap(f64::from(facing - self.body_yaw)) as f32 * 0.3;
        let offset = (wrap(f64::from(self.head_yaw - self.body_yaw)) as f32).clamp(-75.0, 75.0);
        self.body_yaw = self.head_yaw - offset;
        if offset * offset > 2500.0 {
            self.body_yaw += offset * 0.2;
        }
        let speed = ((dx * dx + dz * dz).sqrt() * 4.0).min(1.0) as f32;
        self.limb_amount += (speed - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;
    }
}

/// `MathHelper.wrapAngleTo180_double`.
fn wrap(mut value: f64) -> f64 {
    value %= 360.0;
    if value >= 180.0 {
        value -= 360.0;
    }
    if value < -180.0 {
        value += 360.0;
    }
    value
}

/// `DefaultPlayerSkin.isSlimSkin`: `(uuid.hashCode() & 1) == 1`.
pub fn slim_skin(uuid: u128) -> bool {
    let hilo = ((uuid >> 64) as u64) ^ (uuid as u64);
    (((hilo >> 32) as u32) ^ (hilo as u32)) & 1 == 1
}

#[derive(Debug, Default)]
pub struct Entities {
    map: HashMap<i32, Entity>,
    profiles: HashMap<u128, std::sync::Arc<horizon_protocol_47::player_list::Profile>>,
}

impl Entities {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, id: i32) -> Option<&Entity> {
        self.map.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (i32, &Entity)> {
        self.map.iter().map(|(&id, entity)| (id, entity))
    }

    pub fn clear(&mut self) {
        self.clear_world();
        self.profiles.clear();
    }

    /// New WorldClient, same connection: player-info profiles survive S07.
    pub fn clear_world(&mut self) {
        self.map.clear();
    }

    /// One client tick; objects the client simulates move against `world`.
    pub fn tick(&mut self, world: &crate::World) {
        for entity in self.map.values_mut() {
            entity.tick();
            entity.simulate(world);
            if let Some((_, ticks)) = &mut entity.collector {
                *ticks += 1;
            }
        }
        self.map
            .retain(|_, entity| entity.collector.is_none_or(|(_, ticks)| ticks < 3));
    }

    /// S0D `Collect Item`: the item, arrow or orb flies to its collector.
    pub fn collect(&mut self, collected: i32, collector: i32) {
        if let Some(entity) = self.map.get_mut(&collected)
            && matches!(entity.kind, Kind::Object(2 | 60) | Kind::Orb)
        {
            entity.collector = Some((collector, 0));
        }
    }

    /// S12 for another entity: objects take it as their motion.
    pub fn set_velocity(&mut self, id: i32, velocity: [i16; 3]) {
        if let Some(entity) = self.map.get_mut(&id)
            && !entity.living()
        {
            entity.motion = velocity.map(|v| f64::from(v) / 8000.0);
            entity.stuck = false;
        }
    }

    pub fn update_profiles(&mut self, updates: Vec<horizon_protocol_47::player_list::Update>) {
        use horizon_protocol_47::player_list::Update;
        for update in updates {
            match update {
                Update::Add { uuid, profile, .. } => {
                    let profile = std::sync::Arc::new(profile);
                    for entity in self.map.values_mut().filter(|entity| entity.uuid == uuid) {
                        entity.profile = Some(profile.clone());
                    }
                    self.profiles.insert(uuid, profile);
                }
                Update::Remove(uuid) => {
                    self.profiles.remove(&uuid);
                }
                _ => {}
            }
        }
    }

    pub fn apply(&mut self, packet: EntityPacket) {
        match packet {
            EntityPacket::SpawnPlayer {
                id,
                uuid,
                position,
                yaw,
                pitch,
                metadata,
                ..
            } => {
                let mut entity = Entity::new(
                    Kind::Player {
                        slim: slim_skin(uuid),
                    },
                    uuid,
                    position,
                    angle(yaw),
                    angle(pitch),
                );
                entity.profile = self.profiles.get(&uuid).cloned();
                entity.metadata(&metadata);
                self.map.insert(id, entity);
            }
            EntityPacket::SpawnMob {
                id,
                kind,
                position,
                yaw,
                pitch,
                head_yaw,
                metadata,
            } => {
                let mut entity =
                    Entity::new(Kind::Mob(kind), 0, position, angle(yaw), angle(pitch));
                entity.head_yaw = angle(head_yaw);
                entity.metadata(&metadata);
                self.map.insert(id, entity);
            }
            EntityPacket::SpawnObject {
                id,
                kind,
                position,
                pitch,
                yaw,
                data,
                velocity,
            } => {
                let mut entity =
                    Entity::new(Kind::Object(kind), 0, position, angle(yaw), angle(pitch));
                entity.data = data;
                if let Some(velocity) = velocity {
                    entity.motion = velocity.map(|v| f64::from(v) / 8000.0);
                }
                self.map.insert(id, entity);
            }
            EntityPacket::SpawnOrb { id, position } => {
                self.map
                    .insert(id, Entity::new(Kind::Orb, 0, position, 0.0, 0.0));
            }
            EntityPacket::Destroy(ids) => {
                for id in ids {
                    // A pickup in flight stays for its three ticks.
                    if self.map.get(&id).is_some_and(|e| e.collector.is_none()) {
                        self.map.remove(&id);
                    }
                }
            }
            EntityPacket::Move {
                id,
                delta,
                look,
                on_ground,
            } => {
                if let Some(entity) = self.map.get_mut(&id) {
                    entity.on_ground = on_ground;
                    if let Some(delta) = delta {
                        for (server, step) in entity.server.iter_mut().zip(delta) {
                            *server = server.wrapping_add(i32::from(step));
                        }
                    }
                    let (yaw, pitch) = look.map_or((None, None), |(yaw, pitch)| {
                        (Some(angle(yaw)), Some(angle(pitch)))
                    });
                    entity.move_to(yaw, pitch);
                }
            }
            EntityPacket::Teleport {
                id,
                position,
                yaw,
                pitch,
                on_ground,
            } => {
                if let Some(entity) = self.map.get_mut(&id) {
                    entity.on_ground = on_ground;
                    entity.server = position;
                    entity.move_to(Some(angle(yaw)), Some(angle(pitch)));
                }
            }
            EntityPacket::HeadLook { id, yaw } => {
                if let Some(entity) = self.map.get_mut(&id) {
                    entity.head_yaw = angle(yaw);
                }
            }
            EntityPacket::Metadata { id, entries } => {
                if let Some(entity) = self.map.get_mut(&id) {
                    entity.metadata(&entries);
                }
            }
            EntityPacket::Equipment { id, slot, item } => {
                if let Some(entity) = self.map.get_mut(&id)
                    && let Some(target) = usize::try_from(slot)
                        .ok()
                        .and_then(|slot| entity.equipment.get_mut(slot))
                {
                    *target = item;
                }
            }
            EntityPacket::Status { id, status: 2 } => {
                if let Some(entity) = self.map.get_mut(&id)
                    && entity.living()
                {
                    entity.hurt_time = 10;
                    entity.limb_amount = 1.5;
                }
            }
            EntityPacket::Status { id, status: 3 } => {
                if let Some(entity) = self.map.get_mut(&id)
                    && entity.living()
                {
                    entity.health = 0.0;
                }
            }
            EntityPacket::Status { .. } => {}
            EntityPacket::Animation { id, animation } => {
                if animation == 0
                    && let Some(entity) = self.map.get_mut(&id)
                {
                    entity.swing = Some(0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn(entities: &mut Entities) {
        entities.apply(EntityPacket::SpawnPlayer {
            id: 5,
            uuid: 0,
            position: [0, 64 * 32, 0],
            yaw: 0,
            pitch: 0,
            held_item: 0,
            metadata: vec![],
        });
    }

    #[test]
    fn dimension_change_retains_profiles_for_players_that_spawn_again() {
        use horizon_protocol_47::player_list::{Profile, Update};
        let mut entities = Entities::default();
        entities.update_profiles(vec![Update::Add {
            uuid: 0,
            profile: Profile {
                name: "NativeProfile".into(),
                properties: vec![],
            },
            game_mode: 0,
            ping: 0,
            display: None,
        }]);
        spawn(&mut entities);
        let before = entities.get(5).unwrap().profile.as_ref().unwrap().clone();
        entities.clear_world();
        assert!(entities.is_empty());
        spawn(&mut entities);
        assert!(std::sync::Arc::ptr_eq(
            &before,
            entities.get(5).unwrap().profile.as_ref().unwrap()
        ));
        entities.clear();
        spawn(&mut entities);
        assert!(
            entities.get(5).unwrap().profile.is_none(),
            "fresh connection must discard old profiles"
        );
    }

    #[test]
    fn mob_metadata_updates_shearing_size_and_child_collision_dimensions() {
        let mut entities = Entities::default();
        for (id, kind, metadata) in [
            (1, 91, vec![(12, Meta::Byte(-1)), (16, Meta::Byte(30))]),
            (2, 55, vec![(16, Meta::Byte(4))]),
        ] {
            entities.apply(EntityPacket::SpawnMob {
                id,
                kind,
                position: [0; 3],
                yaw: 0,
                pitch: 0,
                head_yaw: 0,
                metadata,
            });
        }
        let sheep = entities.get(1).unwrap();
        assert!(sheep.child);
        assert_eq!(sheep.variant, 30);
        assert_eq!(sheep.dimensions(), Some((0.45, 0.65)));
        assert_eq!(
            entities.get(2).unwrap().dimensions(),
            Some((0.51000005 * 4.0, 0.51000005 * 4.0))
        );
    }

    #[test]
    fn living_moves_interpolate_over_three_ticks_and_destroy_removes() {
        let mut entities = Entities::default();
        spawn(&mut entities);
        entities.apply(EntityPacket::Move {
            id: 5,
            delta: Some([96, 0, 0]),
            look: None,
            on_ground: true,
        });
        let mut xs = Vec::new();
        for _ in 0..4 {
            entities.tick(&crate::World::default());
            xs.push(entities.get(5).unwrap().position[0]);
        }
        assert_eq!(xs, [1.0, 2.0, 3.0, 3.0]);
        let walker = entities.get(5).unwrap();
        assert!(walker.limb_amount > 0.0);
        entities.apply(EntityPacket::Metadata {
            id: 5,
            entries: vec![(0, Meta::Byte(0x22))],
        });
        let flagged = entities.get(5).unwrap();
        assert!(flagged.invisible() && flagged.sneaking());
        entities.apply(EntityPacket::Destroy(vec![5]));
        assert!(entities.is_empty());
    }

    #[test]
    fn hurt_status_uses_fixed_width_id_and_expires_after_ten_ticks() {
        let mut entities = Entities::default();
        spawn(&mut entities);
        let packet = horizon_protocol_47::entities::decode(0x1a, &[0, 0, 0, 5, 2])
            .unwrap()
            .unwrap();
        entities.apply(packet);
        assert_eq!(entities.get(5).unwrap().hurt_time, 10);
        for _ in 0..9 {
            entities.tick(&crate::World::default());
        }
        assert_eq!(entities.get(5).unwrap().hurt_time, 1);
        entities.apply(EntityPacket::Status { id: 5, status: 2 });
        assert_eq!(entities.get(5).unwrap().hurt_time, 10);
        for _ in 0..10 {
            entities.tick(&crate::World::default());
        }
        assert_eq!(entities.get(5).unwrap().hurt_time, 0);
        assert!(horizon_protocol_47::entities::decode(0x1a, &[5, 2]).is_err());
        assert!(horizon_protocol_47::entities::decode(0x1a, &[0, 0, 0, 5, 2, 0]).is_err());
    }

    #[test]
    fn swings_last_six_ticks_and_skins_follow_the_uuid_hash() {
        let mut entities = Entities::default();
        spawn(&mut entities);
        entities.apply(EntityPacket::Animation {
            id: 5,
            animation: 0,
        });
        for _ in 0..5 {
            entities.tick(&crate::World::default());
        }
        assert!(entities.get(5).unwrap().swing.is_some());
        entities.tick(&crate::World::default());
        assert!(entities.get(5).unwrap().swing.is_none());
        assert!(!slim_skin(0));
        assert!(slim_skin(1));
        assert!(!slim_skin(1 | 1 << 64));
    }

    #[test]
    fn collected_items_outlive_their_destroy_for_the_pickup_flight() {
        let world = crate::World::default();
        let mut entities = Entities::default();
        entities.apply(EntityPacket::SpawnOrb {
            id: 7,
            position: [0, 64 * 32, 0],
        });
        entities.collect(7, 99);
        entities.apply(EntityPacket::Destroy(vec![7]));
        assert!(entities.get(7).is_some());
        entities.tick(&world);
        entities.tick(&world);
        assert_eq!(entities.get(7).and_then(|e| e.collector), Some((99, 2)));
        entities.tick(&world);
        assert!(entities.get(7).is_none());
        // Without a collector, a destroy removes at once.
        entities.apply(EntityPacket::SpawnOrb {
            id: 8,
            position: [0, 64 * 32, 0],
        });
        entities.apply(EntityPacket::Destroy(vec![8]));
        assert!(entities.get(8).is_none());
    }

    #[test]
    fn dropped_items_fall_and_land_while_arrows_fly_and_stick() {
        let world = crate::World::default();
        let mut entities = Entities::default();
        entities.apply(EntityPacket::SpawnObject {
            id: 1,
            kind: 2,
            position: [16, 10 * 32, 16],
            pitch: 0,
            yaw: 0,
            data: 1,
            velocity: Some([0, 1600, 0]),
        });
        entities.tick(&world);
        let item = entities.get(1).unwrap();
        // Gravity first, then the move: 0.2 - 0.04 blocks up.
        assert!((item.position[1] - (10.0 + 0.16)).abs() < 1e-9);
        entities.apply(EntityPacket::SpawnObject {
            id: 2,
            kind: 60,
            position: [0, 0, 0],
            pitch: 0,
            yaw: 0,
            data: 1,
            velocity: Some([8000, 0, 0]),
        });
        entities.tick(&world);
        let arrow = entities.get(2).unwrap();
        assert_eq!(arrow.position, [1.0, 0.0, 0.0]);
        assert!((arrow.yaw - 90.0).abs() < 1e-4);
        assert!((arrow.motion[1] + 0.05).abs() < 1e-9);
    }
}
