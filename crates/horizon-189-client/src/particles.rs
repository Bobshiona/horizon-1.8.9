//! `EffectRenderer` for critical hits: `EntityCrit2FX` emitters spawning
//! `EntityCritFX` particles, the crit trail of flying arrows, and
//! `EntityDiggingFX` block-break particles. Ticked at 20 Hz on the window
//! thread and drawn as camera-facing quads.
use crate::{models::Texture, terrain::Vertex};
use glam::{Mat4, Vec3};
use horizon_world_189::entities::{Entity, Kind};

pub enum Event {
    Explosion(horizon_protocol_47::effects::Explosion),
    Particle(horizon_protocol_47::effects::Particle),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Crit,
    Digging,
    Smoke,
    NormalExplosion,
    LargeExplosion,
}

struct Huge {
    position: [f64; 3],
    age: u32,
}

struct Particle {
    style: Style,
    position: [f64; 3],
    previous: [f64; 3],
    motion: [f64; 3],
    age: u32,
    max_age: u32,
    color: [f32; 3],
    scale: f32,
    texture: u8,
    /// `EntityDiggingFX`: the block texture layer and its 4-pixel jitter.
    block: Option<(u32, [f32; 2])>,
    on_ground: bool,
}

struct Emitter {
    entity: i32,
    magic: bool,
    age: u8,
}

pub struct Particles {
    particles: Vec<Particle>,
    emitters: Vec<Emitter>,
    seed: u64,
    huge: Vec<Huge>,
}

impl Default for Particles {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            emitters: Vec::new(),
            seed: 0x2545_F491_4F6C_DD1D,
            huge: Vec::new(),
        }
    }
}

/// `EffectRenderer` keeps at most 4000 particles per layer.
const MAX_PARTICLES: usize = 4000;

impl Particles {
    fn gaussian(&mut self) -> f64 {
        (-2.0 * self.random().max(f64::MIN_POSITIVE).ln()).sqrt()
            * (std::f64::consts::TAU * self.random()).cos()
    }

    pub fn event(&mut self, event: &Event, camera: [f64; 3]) {
        match event {
            Event::Explosion(e) => {
                let at = e.position.map(f64::from);
                self.visual(if e.strength >= 2.0 { 2 } else { 1 }, at, [0.0; 3]);
                for &block in e.blocks.iter().take(MAX_PARTICLES / 2) {
                    let point = block.map(|v| f64::from(v) + self.random());
                    let delta: [f64; 3] = std::array::from_fn(|a| point[a] - at[a]);
                    let distance = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
                    if distance <= 0.0 || e.strength <= 0.0 {
                        continue;
                    }
                    let speed = 0.5 / (distance / f64::from(e.strength) + 0.1)
                        * (self.random() * self.random() + 0.3);
                    let motion = delta.map(|v| v / distance * speed);
                    self.visual(0, std::array::from_fn(|a| (point[a] + at[a]) / 2.0), motion);
                    self.visual(11, point, motion);
                }
            }
            Event::Particle(p) => {
                let at = p.position.map(f64::from);
                let distance = (0..3).map(|a| (at[a] - camera[a]).powi(2)).sum::<f64>();
                if distance
                    > if p.long_distance {
                        256.0f64.powi(2)
                    } else {
                        32.0f64.powi(2)
                    }
                {
                    return;
                }
                if p.count == 0 {
                    self.visual(p.id, at, p.offset.map(|v| f64::from(v * p.speed)));
                } else {
                    for _ in 0..p.count.min(MAX_PARTICLES as i32) {
                        let position = std::array::from_fn(|a| {
                            at[a] + self.gaussian() * f64::from(p.offset[a])
                        });
                        let speed = std::array::from_fn(|_| self.gaussian() * f64::from(p.speed));
                        self.visual(p.id, position, speed);
                    }
                }
            }
        }
    }

    fn visual(&mut self, id: i32, at: [f64; 3], speed: [f64; 3]) {
        if !at.iter().chain(&speed).all(|v| v.is_finite()) {
            return;
        }
        if id == 2 {
            if self.huge.len() < 64 {
                self.huge.push(Huge {
                    position: at,
                    age: 0,
                });
            }
            return;
        }
        let style = match id {
            0 => Style::NormalExplosion,
            1 => Style::LargeExplosion,
            11 | 12 => Style::Smoke,
            _ => return,
        };
        let (motion, color, scale, max_age) = match style {
            Style::LargeExplosion => (
                [0.0; 3],
                (self.random() * 0.6 + 0.4) as f32,
                20.0 * (1.0 - speed[0] as f32 * 0.5),
                6 + (self.random() * 4.0) as u32,
            ),
            Style::NormalExplosion => (
                speed.map(|v| v + (self.random() * 2.0 - 1.0) * 0.050_000_000_745_058_06),
                (self.random() * 0.3 + 0.7) as f32,
                (self.random() * self.random() * 6.0 + 1.0) as f32,
                (16.0 / (self.random() * 0.8 + 0.2)) as u32 + 2,
            ),
            _ => {
                // EntitySmokeFX starts from EntityFX's normalized random velocity.
                let mut motion: [f64; 3] =
                    std::array::from_fn(|_| (self.random() * 2.0 - 1.0) * 0.4);
                let length = motion.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-9);
                let f = (self.random() + self.random() + 1.0) * 0.15;
                for a in 0..3 {
                    motion[a] = motion[a] / length * f * 0.4;
                }
                motion[1] += 0.1;
                for a in 0..3 {
                    motion[a] = motion[a] * 0.100_000_001_490_116_12 + speed[a];
                }
                let multiplier = if id == 12 { 2.5 } else { 1.0 };
                let color = (self.random() * 0.3) as f32;
                let scale = ((self.random() * 0.5 + 0.5) * 2.0 * 0.75 * multiplier) as f32;
                let age = (8.0 / (self.random() * 0.8 + 0.2)) as u32;
                (
                    motion,
                    color,
                    scale,
                    (age as f32 * multiplier as f32) as u32,
                )
            }
        };
        if self.particles.len() >= MAX_PARTICLES {
            self.particles.remove(0);
        }
        self.particles.push(Particle {
            style,
            position: at,
            previous: at,
            motion,
            age: 0,
            max_age: max_age.max(1),
            color: [color; 3],
            scale,
            texture: 7,
            block: None,
            on_ground: false,
        });
    }

    fn update_visual(p: &mut Particle, world: Option<&horizon_world_189::World>) -> bool {
        p.previous = p.position;
        if p.age >= p.max_age {
            return false;
        }
        p.age += 1;
        if p.style == Style::LargeExplosion {
            return true;
        }
        p.texture = (7i32 - (p.age * 8 / p.max_age) as i32).max(0) as u8;
        p.motion[1] += 0.004;
        let before_y = p.position[1];
        for axis in [1, 0, 2] {
            let mut next = p.position;
            next[axis] += p.motion[axis];
            let [x, y, z] = next.map(|v| v.floor() as i32);
            let solid = world.and_then(|w| w.block(x, y, z)).is_some_and(|s| {
                s.block_id() != 0
                    && horizon_world_189::blocks::block(s.block_id()).layer
                        == horizon_world_189::blocks::Layer::Solid
            });
            if solid {
                p.on_ground |= axis == 1 && p.motion[axis] < 0.0;
                p.motion[axis] = 0.0;
            } else {
                p.position = next;
            }
        }
        if p.style == Style::Smoke && before_y == p.position[1] {
            p.motion[0] *= 1.1;
            p.motion[2] *= 1.1;
        }
        let drag = if p.style == Style::Smoke {
            0.959_999_978_542_327_9
        } else {
            0.899_999_976_158_142_1
        };
        for value in &mut p.motion {
            *value *= drag;
        }
        if p.on_ground {
            p.motion[0] *= 0.699_999_988_079_071;
            p.motion[2] *= 0.699_999_988_079_071;
        }
        true
    }

    /// `Math.random()` stand-in.
    fn random(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `onCriticalHit`/`onEnchantmentCritical` or S0B animation 4 and 5.
    pub fn emit(&mut self, entity: i32, magic: bool) {
        if self.emitters.len() < 64 {
            self.emitters.push(Emitter {
                entity,
                magic,
                age: 0,
            });
        }
    }

    /// `EntityCritFX` (and its magic variant) at `at` with `speed`.
    fn crit(&mut self, at: [f64; 3], speed: [f64; 3], magic: bool) {
        if self.particles.len() >= MAX_PARTICLES {
            self.particles.remove(0);
        }
        // EntityFX(world, x, y, z, xSpeed, ySpeed, zSpeed).
        let mut motion = [0.0; 3];
        for (axis, value) in motion.iter_mut().enumerate() {
            *value = speed[axis] + (self.random() * 2.0 - 1.0) * 0.4;
        }
        let f = (self.random() + self.random() + 1.0) * 0.15;
        let length = motion.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-9);
        for value in &mut motion {
            *value = *value / length * f * 0.4;
        }
        motion[1] += 0.1;
        // EntityCritFX: damp the random part, add 40% of the given speed.
        for (axis, value) in motion.iter_mut().enumerate() {
            *value = *value * 0.1 + speed[axis] * 0.4;
        }
        let grey = (self.random() * 0.3 + 0.6) as f32;
        let color = if magic {
            [grey * 0.3, grey * 0.8, grey]
        } else {
            [grey; 3]
        };
        let scale = ((self.random() * 0.5 + 0.5) * 2.0 * 0.75) as f32;
        let max_age = (6.0 / (self.random() * 0.8 + 0.6)) as u32;
        let mut particle = Particle {
            style: Style::Crit,
            position: at,
            previous: at,
            motion,
            age: 0,
            max_age,
            color,
            scale,
            texture: if magic { 66 } else { 65 },
            block: None,
            on_ground: false,
        };
        // The constructor runs one onUpdate.
        if Self::update(&mut particle) {
            self.particles.push(particle);
        }
    }

    /// `addBlockDestroyEffects`: 64 `EntityDiggingFX` on a 4x4x4 grid in
    /// the block, each flung from its centre. `layer` is the block's
    /// particle texture and `color` its tint.
    pub fn destroy(&mut self, at: [i32; 3], layer: u32, color: [f32; 3]) {
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    let offset = [i, j, k].map(|n| (f64::from(n) + 0.5) / 4.0);
                    let position: [f64; 3] =
                        std::array::from_fn(|axis| f64::from(at[axis]) + offset[axis]);
                    let speed = offset.map(|v| v - 0.5);
                    self.digging(position, speed, layer, color);
                }
            }
        }
    }

    fn digging(&mut self, at: [f64; 3], speed: [f64; 3], layer: u32, color: [f32; 3]) {
        if self.particles.len() >= MAX_PARTICLES {
            self.particles.remove(0);
        }
        // EntityFX(world, x, y, z, xSpeed, ySpeed, zSpeed).
        let mut motion = [0.0; 3];
        for (axis, value) in motion.iter_mut().enumerate() {
            *value = speed[axis] + (self.random() * 2.0 - 1.0) * 0.4;
        }
        let f = (self.random() + self.random() + 1.0) * 0.15;
        let length = motion.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-9);
        for value in &mut motion {
            *value = *value / length * f * 0.4;
        }
        motion[1] += 0.1;
        let scale = ((self.random() * 0.5 + 0.5) * 2.0 / 2.0) as f32;
        let max_age = (4.0 / (self.random() * 0.9 + 0.1)) as u32;
        let jitter = [(self.random() * 3.0) as f32, (self.random() * 3.0) as f32];
        self.particles.push(Particle {
            style: Style::Digging,
            position: at,
            previous: at,
            motion,
            age: 0,
            max_age,
            color: color.map(|c| c * 0.6),
            scale,
            texture: 0,
            block: Some((layer, jitter)),
            on_ground: false,
        });
    }

    /// `EntityFX.onUpdate` for block particles: gravity, block collision,
    /// drag and ground friction.
    fn update_digging(particle: &mut Particle, world: Option<&horizon_world_189::World>) -> bool {
        particle.previous = particle.position;
        if particle.age >= particle.max_age {
            return false;
        }
        particle.age += 1;
        particle.motion[1] -= 0.04;
        let solid = |at: [f64; 3]| {
            world.is_some_and(|world| {
                let [x, y, z] = at.map(|v| v.floor() as i32);
                world.block(x, y, z).is_some_and(|state| {
                    let id = state.block_id();
                    id != 0
                        && horizon_world_189::blocks::block(id).layer
                            == horizon_world_189::blocks::Layer::Solid
                })
            })
        };
        particle.on_ground = false;
        for axis in [1, 0, 2] {
            let mut next = particle.position;
            next[axis] += particle.motion[axis];
            if solid(next) {
                if axis == 1 && particle.motion[1] < 0.0 {
                    particle.on_ground = true;
                }
                particle.motion[axis] = 0.0;
            } else {
                particle.position = next;
            }
        }
        for value in &mut particle.motion {
            *value *= 0.98;
        }
        if particle.on_ground {
            particle.motion[0] *= 0.7;
            particle.motion[2] *= 0.7;
        }
        true
    }

    /// `EntityCritFX.onUpdate`; false once it has expired.
    fn update(particle: &mut Particle) -> bool {
        particle.previous = particle.position;
        if particle.age >= particle.max_age {
            return false;
        }
        particle.age += 1;
        for axis in 0..3 {
            particle.position[axis] += particle.motion[axis];
        }
        particle.color[1] *= 0.96;
        particle.color[2] *= 0.9;
        for value in &mut particle.motion {
            *value *= 0.699_999_988_079_071;
        }
        particle.motion[1] -= 0.019_999_999_552_965_164;
        true
    }

    /// One client tick: emitters burst around their entity for three ticks,
    /// critical arrows trail, particles move and expire.
    pub fn tick(&mut self, entities: &[(i32, Entity)], world: Option<&horizon_world_189::World>) {
        let mut huge = std::mem::take(&mut self.huge);
        huge.retain_mut(|h| {
            for _ in 0..6 {
                let at = h
                    .position
                    .map(|v| v + (self.random() - self.random()) * 4.0);
                self.visual(1, at, [f64::from(h.age) / 8.0, 0.0, 0.0]);
            }
            h.age += 1;
            h.age < 8
        });
        self.huge = huge;
        let mut emitters = std::mem::take(&mut self.emitters);
        emitters.retain_mut(|emitter| {
            let Some((_, entity)) = entities.iter().find(|(id, _)| *id == emitter.entity) else {
                return false;
            };
            let (width, height) = if entity.child { (0.3, 0.9) } else { (0.6, 1.8) };
            for _ in 0..16 {
                let d = [
                    self.random() * 2.0 - 1.0,
                    self.random() * 2.0 - 1.0,
                    self.random() * 2.0 - 1.0,
                ];
                if d.iter().map(|v| v * v).sum::<f64>() <= 1.0 {
                    let at = [
                        entity.position[0] + d[0] * width / 4.0,
                        entity.position[1] + height / 2.0 + d[1] * height / 4.0,
                        entity.position[2] + d[2] * width / 4.0,
                    ];
                    self.crit(at, [d[0], d[1] + 0.2, d[2]], emitter.magic);
                }
            }
            emitter.age += 1;
            emitter.age < 3
        });
        self.emitters = emitters;
        for (_, entity) in entities {
            if entity.kind == Kind::Object(50) && entity.age < 80 {
                let [x, y, z] = entity.position;
                self.visual(11, [x, y + 0.5, z], [0.0; 3]);
            }
            if entity.kind == Kind::Object(60) && entity.critical && !entity.stuck {
                let [mx, my, mz] = entity.motion;
                for k in 0..4 {
                    let k = f64::from(k) / 4.0;
                    let at = [
                        entity.position[0] + mx * k,
                        entity.position[1] + my * k,
                        entity.position[2] + mz * k,
                    ];
                    self.crit(at, [-mx, -my + 0.2, -mz], false);
                }
            }
        }
        self.particles.retain_mut(|particle| {
            if particle.block.is_some() {
                Self::update_digging(particle, world)
            } else if particle.style == Style::Crit {
                Self::update(particle)
            } else {
                Self::update_visual(particle, world)
            }
        });
    }

    /// `EntityFX.renderParticle` quads, growing to full size as
    /// `EntityCritFX.renderParticle` does.
    pub fn draw(
        &self,
        out: &mut Vec<Vertex>,
        partial: f32,
        origin: [i64; 3],
        yaw: f32,
        pitch: f32,
    ) {
        let facing =
            Mat4::from_rotation_y((-yaw).to_radians()) * Mat4::from_rotation_x(pitch.to_radians());
        let right = facing.transform_vector3(Vec3::X);
        let up = facing.transform_vector3(Vec3::Y);
        let texel = 1.0 / crate::models::LAYER_SIZE as f32;
        for particle in &self.particles {
            let grow = if matches!(
                particle.style,
                Style::Digging | Style::LargeExplosion | Style::NormalExplosion
            ) {
                1.0
            } else {
                ((particle.age as f32 + partial) / particle.max_age.max(1) as f32 * 32.0)
                    .clamp(0.0, 1.0)
            };
            let size = 0.1 * particle.scale * grow;
            let p = f64::from(partial);
            let at = Vec3::from_array(std::array::from_fn(|a| {
                (particle.previous[a] + (particle.position[a] - particle.previous[a]) * p
                    - origin[a] as f64) as f32
            }));
            let (u, v) = (
                f32::from(particle.texture % 16) * 8.0,
                f32::from(particle.texture / 16) * 8.0,
            );
            let corners = [
                at - right * size - up * size,
                at + right * size - up * size,
                at + right * size + up * size,
                at - right * size + up * size,
            ];
            // Block particles show a 4-pixel patch of the block texture.
            let (uv, layer) = match particle.block {
                Some((layer, [x, y])) => {
                    let (u, v) = (x / 4.0 * 16.0, y / 4.0 * 16.0);
                    (
                        [[u, v + 4.0], [u + 4.0, v + 4.0], [u + 4.0, v], [u, v]],
                        layer,
                    )
                }
                None if particle.style == Style::LargeExplosion => {
                    let frame = (((particle.age as f32 + partial) * 15.0 / particle.max_age as f32)
                        as u32)
                        .min(15);
                    let u = (frame % 4) as f32 * 32.0;
                    let v = (frame / 4) as f32 * 32.0;
                    (
                        [
                            [u, v + 31.968],
                            [u + 31.968, v + 31.968],
                            [u + 31.968, v],
                            [u, v],
                        ],
                        Texture::Explosion as u32,
                    )
                }
                None => (
                    [[u, v + 8.0], [u + 8.0, v + 8.0], [u + 8.0, v], [u, v]],
                    Texture::Particles as u32,
                ),
            };
            for i in [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] {
                out.push(Vertex {
                    position: corners[i].to_array(),
                    uv: uv[i].map(|c| c * texel),
                    layer,
                    color: particle.color,
                    hurt: 0.0,
                });
            }
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.particles.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huge_explosion_emits_six_children_for_eight_ticks_then_expires() {
        let mut p = Particles::default();
        p.visual(2, [0.0; 3], [0.0; 3]);
        for _ in 0..8 {
            p.tick(&[], None);
        }
        assert!(p.huge.is_empty());
        assert!(p.particles.len() >= 36 && p.particles.len() <= 48);
        let mut vertices = Vec::new();
        p.draw(&mut vertices, 0.5, [0; 3], 0.0, 0.0);
        assert!(!vertices.is_empty());
        assert!(
            vertices
                .iter()
                .all(|v| v.position.iter().chain(&v.uv).all(|v| v.is_finite()))
        );
        assert!(
            vertices
                .iter()
                .all(|v| v.layer == Texture::Explosion as u32)
        );
        for _ in 0..20 {
            p.tick(&[], None);
        }
        assert!(p.particles.is_empty());
    }

    #[test]
    fn packet_count_zero_velocity_distance_and_budget() {
        use horizon_protocol_47::effects::Particle;
        let mut p = Particles::default();
        let mut packet = Particle {
            id: 11,
            long_distance: false,
            position: [0.0; 3],
            offset: [1.0, 2.0, 3.0],
            speed: 2.0,
            count: 0,
            data: vec![],
        };
        p.event(&Event::Particle(packet.clone()), [100.0; 3]);
        assert_eq!(p.len(), 0);
        p.event(&Event::Particle(packet.clone()), [0.0; 3]);
        assert_eq!(p.len(), 1);
        for (a, expected) in [2.0, 4.0, 6.0].iter().enumerate() {
            assert!((p.particles[0].motion[a] - expected).abs() < 0.1);
        }
        packet.count = i32::MAX;
        p.event(&Event::Particle(packet), [0.0; 3]);
        assert_eq!(p.len(), MAX_PARTICLES);
        for _ in 0..120 {
            p.tick(&[], None);
        }
        assert!(p.particles.is_empty());
    }

    #[test]
    fn crit_bursts_last_three_ticks_and_particles_expire() {
        let mut particles = Particles::default();
        let entities = vec![(7, {
            let mut packet_entities = horizon_world_189::entities::Entities::default();
            packet_entities.apply(horizon_protocol_47::entities::EntityPacket::SpawnMob {
                id: 7,
                kind: 54,
                position: [0, 2048, 0],
                yaw: 0,
                pitch: 0,
                head_yaw: 0,
                metadata: Vec::new(),
            });
            packet_entities.get(7).unwrap().clone()
        })];
        particles.emit(7, true);
        particles.tick(&entities, None);
        let first = particles.len();
        assert!(first > 0 && first <= 16);
        particles.tick(&entities, None);
        particles.tick(&entities, None);
        assert!(particles.emitters.is_empty());
        assert!(particles.particles.iter().all(|p| p.texture == 66));
        for _ in 0..20 {
            particles.tick(&entities, None);
        }
        assert_eq!(particles.len(), 0);
    }

    #[test]
    fn block_breaks_spawn_sixty_four_falling_particles() {
        let mut particles = Particles::default();
        particles.destroy([0, 64, 0], 3, [1.0; 3]);
        assert_eq!(particles.len(), 64);
        let rising: Vec<f64> = particles.particles.iter().map(|p| p.motion[1]).collect();
        particles.tick(&[], None);
        // Gravity pulls every surviving particle down.
        for (particle, before) in particles.particles.iter().zip(rising) {
            assert!(particle.motion[1] < before);
        }
        for _ in 0..40 {
            particles.tick(&[], None);
        }
        assert_eq!(particles.len(), 0);
    }
}
