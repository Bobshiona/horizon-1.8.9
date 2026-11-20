//! Finite legacy look rays and block/entity selection. The world remains the
//! authority for shapes; unloaded cells never become selectable blocks.
use crate::{
    Aabb,
    shapes::{bb, collision_boxes, expand, hi, lo},
    trig,
};
use horizon_world_189::{World, entities::Entities};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Block {
        at: [i32; 3],
        face: u8,
        point: [f64; 3],
    },
    Entity {
        id: i32,
        point: [f64; 3],
    },
}

pub fn look(yaw: f32, pitch: f32) -> [f64; 3] {
    let cy = trig::cos(-yaw * 0.017453292 - std::f32::consts::PI);
    let sy = trig::sin(-yaw * 0.017453292 - std::f32::consts::PI);
    let cp = -trig::cos(-pitch * 0.017453292);
    [
        f64::from(sy * cp),
        f64::from(trig::sin(-pitch * 0.017453292)),
        f64::from(cy * cp),
    ]
}

/// AABB.calculateIntercept visits X-, X+, Y-, Y+, Z-, Z+ in that order.
pub fn intercept(bounds: Aabb, start: [f64; 3], delta: [f64; 3]) -> Option<(f64, u8)> {
    let (min, max) = (lo(bounds), hi(bounds));
    let mut best = None;
    for (axis, side, face) in [
        (0, min[0], 4),
        (0, max[0], 5),
        (1, min[1], 0),
        (1, max[1], 1),
        (2, min[2], 2),
        (2, max[2], 3),
    ] {
        if delta[axis] * delta[axis] < 1.0000000116860974e-7 {
            continue;
        }
        let t = (side - start[axis]) / delta[axis];
        if !(0.0..=1.0).contains(&t) {
            continue;
        }
        if (0..3).filter(|&a| a != axis).all(|a| {
            let v = start[a] + delta[a] * t;
            v >= min[a] && v <= max[a]
        }) && best.is_none_or(|(nearest, _)| t < nearest)
        {
            best = Some((t, face));
        }
    }
    best
}

#[derive(Default)]
pub struct Raycaster {
    boxes: Vec<Aabb>,
}

impl Raycaster {
    pub fn select(
        &mut self,
        world: &World,
        entities: &Entities,
        eye: [f64; 3],
        yaw: f32,
        pitch: f32,
        creative: bool,
    ) -> Option<Hit> {
        if !eye.iter().all(|v| v.is_finite()) || !yaw.is_finite() || !pitch.is_finite() {
            return None;
        }
        let reach = if creative { 5.0 } else { 4.5 };
        let direction = look(yaw, pitch);
        let delta = direction.map(|v| v * reach);
        let end = std::array::from_fn::<_, 3, _>(|a| eye[a] + delta[a]);
        let area = expand(bb(eye, end), 1.0, 1.0, 1.0);
        let min = lo(area).map(|v| v.floor() as i32);
        let max = hi(area).map(|v| v.floor() as i32);
        let mut nearest = 1.0;
        let mut hit = None;
        let boxes = &mut self.boxes;
        for x in min[0]..=max[0] {
            for y in min[1]..=max[1] {
                for z in min[2]..=max[2] {
                    let at = [x, y, z];
                    let Some(state) = world.block(x, y, z) else {
                        continue;
                    };
                    let id = state.block_id();
                    if id == 0 || matches!(id, 8..=11) {
                        continue;
                    }
                    boxes.clear();
                    collision_boxes(world, at, state, area, boxes);
                    // Plants and wall/standing signs have selection bounds despite a
                    // null collision box. Use their legacy inset plant/sign bounds.
                    if boxes.is_empty()
                        && matches!(id,6|30..=32|37..=40|50|59|63|68|83|104..=106|115|141|142|175..=177)
                    {
                        boxes.push(bb(
                            [f64::from(x) + 0.1, f64::from(y), f64::from(z) + 0.1],
                            [f64::from(x) + 0.9, f64::from(y) + 1.0, f64::from(z) + 0.9],
                        ));
                    }
                    for &bounds in boxes.iter() {
                        if let Some((t, face)) = intercept(bounds, eye, delta)
                            && (t < nearest || hit.is_none())
                        {
                            nearest = t;
                            hit = Some(Hit::Block {
                                at,
                                face,
                                point: std::array::from_fn(|a| eye[a] + delta[a] * t),
                            });
                        }
                    }
                }
            }
        }
        // EntityRenderer.getMouseOver: entities are traced as far as the
        // block reach (six in creative), but in survival one farther than
        // 3.0 makes the whole result a miss, hiding any block behind it.
        let entity_reach = if creative { 6.0 } else { reach };
        let ray = direction.map(|v| v * entity_reach);
        let block_distance = if hit.is_some() {
            nearest * reach
        } else {
            entity_reach
        };
        let mut distance = block_distance;
        let mut pointed = None;
        // Dying entities stay collidable until removed at deathTime 20.
        for (id, entity) in entities.iter() {
            let Some((width, height)) = entity.dimensions() else {
                continue;
            };
            let [x, y, z] = entity.position;
            let half = f64::from(width / 2.0);
            let bounds = expand(
                bb(
                    [x - half, y, z - half],
                    [x + half, y + f64::from(height), z + half],
                ),
                f64::from(0.1f32),
                f64::from(0.1f32),
                f64::from(0.1f32),
            );
            let (min, max) = (lo(bounds), hi(bounds));
            if (0..3).all(|a| eye[a] > min[a] && eye[a] < max[a]) {
                distance = 0.0;
                pointed = Some((id, eye));
            } else if let Some((t, _)) = intercept(bounds, eye, ray) {
                let d = t * entity_reach;
                if d < distance || distance == 0.0 {
                    distance = d;
                    pointed = Some((id, std::array::from_fn(|a| eye[a] + ray[a] * t)));
                }
            }
        }
        if let Some((id, point)) = pointed {
            let far = (0..3)
                .map(|a| (point[a] - eye[a]).powi(2))
                .sum::<f64>()
                .sqrt();
            if !creative && far > 3.0 {
                return None;
            }
            if distance < block_distance || hit.is_none() {
                return Some(Hit::Entity { id, point });
            }
        }
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol_47::entities::EntityPacket;
    use horizon_world_189::{BlockState, ColumnData, ColumnUpdate};
    #[test]
    fn blocks_occlude_entities_and_survival_reach_stays_three() {
        let mut world = World::default();
        world.apply(ColumnUpdate::Data {
            x: 0,
            z: 0,
            full: true,
            data: Box::new(ColumnData {
                sections: Default::default(),
                biomes: None,
            }),
        });
        let mut entities = Entities::default();
        entities.apply(EntityPacket::SpawnPlayer {
            id: 7,
            uuid: 0,
            position: [16, 0, 3 * 32],
            yaw: 0,
            pitch: 0,
            held_item: 0,
            metadata: vec![],
        });
        assert!(matches!(
            Raycaster::default().select(&world, &entities, [0.5, 1.6, 0.5], 0.0, 0.0, false),
            Some(Hit::Entity { id: 7, .. })
        ));
        world.set_block(0, 1, 1, BlockState(16));
        assert!(matches!(
            Raycaster::default().select(&world, &entities, [0.5, 1.6, 0.5], 0.0, 0.0, false),
            Some(Hit::Block {
                at: [0, 1, 1],
                face: 2,
                ..
            })
        ));
        world.set_block(0, 1, 1, BlockState(0));
        entities.apply(EntityPacket::Teleport {
            id: 7,
            position: [16, 0, 5 * 32],
            yaw: 0,
            pitch: 0,
            on_ground: true,
        });
        for _ in 0..3 {
            entities.tick(&horizon_world_189::World::default());
        }
        assert_eq!(
            Raycaster::default().select(&world, &entities, [0.5, 1.6, 0.5], 0.0, 0.0, false),
            None
        );
    }

    /// EntityRenderer.getMouseOver traces entities as far as the block reach
    /// (4.5); in survival an entity nearer than the block but beyond 3.0
    /// turns the result into a miss rather than letting the block through.
    #[test]
    fn entities_beyond_three_blocks_turn_a_block_hit_into_a_miss() {
        let mut world = World::default();
        world.apply(ColumnUpdate::Data {
            x: 0,
            z: 0,
            full: true,
            data: Box::new(ColumnData {
                sections: Default::default(),
                biomes: None,
            }),
        });
        world.set_block(0, 1, 4, BlockState(16));
        let mut entities = Entities::default();
        entities.apply(EntityPacket::SpawnPlayer {
            id: 7,
            uuid: 0,
            position: [16, 0, 4 * 32],
            yaw: 0,
            pitch: 0,
            held_item: 0,
            metadata: vec![],
        });
        let eye = [0.5, 1.6, 0.5];
        assert_eq!(
            Raycaster::default().select(&world, &entities, eye, 0.0, 0.0, false),
            None
        );
        // Creative reaches entities up to six blocks.
        assert!(matches!(
            Raycaster::default().select(&world, &entities, eye, 0.0, 0.0, true),
            Some(Hit::Entity { id: 7, .. })
        ));
        // Without the entity the block is hit.
        assert!(matches!(
            Raycaster::default().select(&world, &Entities::default(), eye, 0.0, 0.0, false),
            Some(Hit::Block { at: [0, 1, 4], .. })
        ));
    }
}
