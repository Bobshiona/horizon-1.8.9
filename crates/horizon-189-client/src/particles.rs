//! `EffectRenderer` for critical hits: `EntityCrit2FX` emitters spawning
//! `EntityCritFX` particles, and the crit trail of flying arrows. Ticked at
//! 20 Hz on the window thread and drawn as camera-facing quads.
use crate::{models::Texture, terrain::Vertex};
use glam::{Mat4, Vec3};
use horizon_world_189::entities::{Entity, Kind};

struct Particle {
    position: [f64; 3],
    previous: [f64; 3],
    motion: [f64; 3],
    age: u32,
    max_age: u32,
    color: [f32; 3],
    scale: f32,
    texture: u8,
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
}

impl Default for Particles {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            emitters: Vec::new(),
            seed: 0x2545_F491_4F6C_DD1D,
        }
    }
}

/// `EffectRenderer` keeps at most 4000 particles per layer.
const MAX_PARTICLES: usize = 4000;

impl Particles {
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
            position: at,
            previous: at,
            motion,
            age: 0,
            max_age,
            color,
            scale,
            texture: if magic { 66 } else { 65 },
        };
        // The constructor runs one onUpdate.
        if Self::update(&mut particle) {
            self.particles.push(particle);
        }
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
    pub fn tick(&mut self, entities: &[(i32, Entity)]) {
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
        self.particles.retain_mut(Self::update);
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
            let grow = ((particle.age as f32 + partial) / particle.max_age.max(1) as f32 * 32.0)
                .clamp(0.0, 1.0);
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
            let uv = [[u, v + 8.0], [u + 8.0, v + 8.0], [u + 8.0, v], [u, v]];
            for i in [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] {
                out.push(Vertex {
                    position: corners[i].to_array(),
                    uv: uv[i].map(|c| c * texel),
                    layer: Texture::Particles as u32,
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
        particles.tick(&entities);
        let first = particles.len();
        assert!(first > 0 && first <= 16);
        particles.tick(&entities);
        particles.tick(&entities);
        assert!(particles.emitters.is_empty());
        assert!(particles.particles.iter().all(|p| p.texture == 66));
        for _ in 0..20 {
            particles.tick(&entities);
        }
        assert_eq!(particles.len(), 0);
    }
}
