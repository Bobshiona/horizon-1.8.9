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
    server: [i32; 3],
    target: [f64; 3],
    target_yaw: f32,
    target_pitch: f32,
    steps: u8,
    pub position: [f64; 3],
    pub previous: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
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
    /// `swingProgressInt` ticks of the six-tick arm swing, if swinging.
    pub swing: Option<u8>,
    pub age: u32,
    /// Remaining vanilla hurt-flash ticks, refreshed by entity status 2.
    pub hurt_time: u8,
}

impl Entity {
    fn new(kind: Kind, uuid: u128, server: [i32; 3], yaw: f32, pitch: f32) -> Self {
        let position = server.map(|value| f64::from(value) / 32.0);
        Self {
            kind,
            uuid,
            server,
            target: position,
            target_yaw: yaw,
            target_pitch: pitch,
            steps: 0,
            position,
            previous: position,
            yaw,
            pitch,
            head_yaw: yaw,
            previous_head_yaw: yaw,
            body_yaw: yaw,
            previous_body_yaw: yaw,
            limb_swing: 0.0,
            limb_amount: 0.0,
            previous_limb_amount: 0.0,
            flags: 0,
            equipment: Default::default(),
            swing: None,
            age: 0,
            hurt_time: 0,
        }
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
            if let (0, Meta::Byte(flags)) = (index, value) {
                self.flags = *flags as u8;
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
        self.previous_head_yaw = self.head_yaw;
        self.previous_body_yaw = self.body_yaw;
        self.previous_limb_amount = self.limb_amount;
        self.age = self.age.wrapping_add(1);
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
        self.map.clear();
    }

    pub fn tick(&mut self) {
        for entity in self.map.values_mut() {
            entity.tick();
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
                ..
            } => {
                self.map.insert(
                    id,
                    Entity::new(Kind::Object(kind), 0, position, angle(yaw), angle(pitch)),
                );
            }
            EntityPacket::SpawnOrb { id, position } => {
                self.map
                    .insert(id, Entity::new(Kind::Orb, 0, position, 0.0, 0.0));
            }
            EntityPacket::Destroy(ids) => {
                for id in ids {
                    self.map.remove(&id);
                }
            }
            EntityPacket::Move {
                id, delta, look, ..
            } => {
                if let Some(entity) = self.map.get_mut(&id) {
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
                ..
            } => {
                if let Some(entity) = self.map.get_mut(&id) {
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
            entities.tick();
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
            entities.tick();
        }
        assert_eq!(entities.get(5).unwrap().hurt_time, 1);
        entities.apply(EntityPacket::Status { id: 5, status: 2 });
        assert_eq!(entities.get(5).unwrap().hurt_time, 10);
        for _ in 0..10 {
            entities.tick();
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
            entities.tick();
        }
        assert!(entities.get(5).unwrap().swing.is_some());
        entities.tick();
        assert!(entities.get(5).unwrap().swing.is_none());
        assert!(!slim_skin(0));
        assert!(slim_skin(1));
        assert!(!slim_skin(1 | 1 << 64));
    }
}
