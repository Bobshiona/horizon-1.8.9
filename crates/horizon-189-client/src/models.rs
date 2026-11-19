//! Entity and block-entity geometry from vanilla 1.8.9 model definitions.
//! `ModelBox` keeps its six-quad UV layout and each renderer its matrix chain;
//! quads are re-wound outward after transformation so back-face culling holds
//! under the renderers' mirroring scales. Textures live in 128-pixel layers of
//! a separate array, placed at the top-left, so UVs are pixel / 128.
use crate::terrain::Vertex;
use glam::{Mat4, Vec3};
use horizon_world_189::entities::{Entity, Kind};
use std::f32::consts::PI;

#[path = "mob_models.rs"]
mod mobs;

pub const LAYER_SIZE: u32 = 128;

/// Entity texture layers, in upload order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Texture {
    White,
    Steve,
    Alex,
    Chest,
    Trapped,
    Ender,
    ChestDouble,
    TrappedDouble,
    Sign,
    Skeleton,
    WitherSkeleton,
    Zombie,
    Creeper,
    Banner,
    Font,
    Leather1,
    Leather2,
    Chain1,
    Chain2,
    Iron1,
    Iron2,
    Diamond1,
    Diamond2,
    Gold1,
    Gold2,
    WoodSword,
    StoneSword,
    IronSword,
    DiamondSword,
    GoldSword,
    Bow,
    CreeperMob,
    Pig,
    Cow,
    Villager,
    Sheep,
    Wool,
    Chicken,
    Spider,
    CaveSpider,
    Slime,
    Enderman,
    Wolf,
    WolfTame,
    WolfAngry,
    Arrow,
    Particles,
    ExperienceOrb,
}

/// Owned-jar paths for every layer after the generated white one.
pub const SOURCES: [&str; 47] = [
    "minecraft:entity/steve",
    "minecraft:entity/alex",
    "minecraft:entity/chest/normal",
    "minecraft:entity/chest/trapped",
    "minecraft:entity/chest/ender",
    "minecraft:entity/chest/normal_double",
    "minecraft:entity/chest/trapped_double",
    "minecraft:entity/sign",
    "minecraft:entity/skeleton/skeleton",
    "minecraft:entity/skeleton/wither_skeleton",
    "minecraft:entity/zombie/zombie",
    "minecraft:entity/creeper/creeper",
    "minecraft:entity/banner_base",
    "minecraft:font/ascii",
    "minecraft:models/armor/leather_layer_1",
    "minecraft:models/armor/leather_layer_2",
    "minecraft:models/armor/chainmail_layer_1",
    "minecraft:models/armor/chainmail_layer_2",
    "minecraft:models/armor/iron_layer_1",
    "minecraft:models/armor/iron_layer_2",
    "minecraft:models/armor/diamond_layer_1",
    "minecraft:models/armor/diamond_layer_2",
    "minecraft:models/armor/gold_layer_1",
    "minecraft:models/armor/gold_layer_2",
    "minecraft:items/wood_sword",
    "minecraft:items/stone_sword",
    "minecraft:items/iron_sword",
    "minecraft:items/diamond_sword",
    "minecraft:items/gold_sword",
    "minecraft:items/bow_standby",
    "minecraft:entity/creeper/creeper",
    "minecraft:entity/pig/pig",
    "minecraft:entity/cow/cow",
    "minecraft:entity/villager/villager",
    "minecraft:entity/sheep/sheep",
    "minecraft:entity/sheep/sheep_fur",
    "minecraft:entity/chicken",
    "minecraft:entity/spider/spider",
    "minecraft:entity/spider/cave_spider",
    "minecraft:entity/slime/slime",
    "minecraft:entity/enderman/enderman",
    "minecraft:entity/wolf/wolf",
    "minecraft:entity/wolf/wolf_tame",
    "minecraft:entity/wolf/wolf_angry",
    "minecraft:entity/arrow",
    "minecraft:particle/particles",
    "minecraft:entity/experience_orb",
];

impl From<Texture> for u32 {
    fn from(texture: Texture) -> Self {
        texture as u32
    }
}

/// One box of a vanilla model: texture offset, origin, integer size, inflation.
#[derive(Clone, Copy)]
struct Cuboid {
    u: f32,
    v: f32,
    origin: [f32; 3],
    size: [u8; 3],
    inflate: f32,
    mirror: bool,
}

const fn cuboid(u: f32, v: f32, origin: [f32; 3], size: [u8; 3]) -> Cuboid {
    Cuboid {
        u,
        v,
        origin,
        size,
        inflate: 0.0,
        mirror: false,
    }
}

impl Cuboid {
    const fn inflated(mut self, inflate: f32) -> Self {
        self.inflate = inflate;
        self
    }
}

/// `ModelRenderer` transform: rotation point, then Z, Y, X rotations.
fn part(base: Mat4, point: [f32; 3], rotation: [f32; 3]) -> Mat4 {
    base * Mat4::from_translation(Vec3::from(point) / 16.0)
        * Mat4::from_rotation_z(rotation[2])
        * Mat4::from_rotation_y(rotation[1])
        * Mat4::from_rotation_x(rotation[0])
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

fn shade(normal: Vec3) -> f32 {
    if normal.y > 0.5 {
        1.0
    } else if normal.y < -0.5 {
        0.5
    } else if normal.z.abs() > normal.x.abs() {
        0.8
    } else {
        0.6
    }
}

/// Append one `ModelBox` under `matrix`, with a uniform tint and layer.
fn add(out: &mut Vec<Vertex>, matrix: Mat4, cube: Cuboid, layer: impl Into<u32>, tint: [f32; 3]) {
    let layer = layer.into();
    let [dx, dy, dz] = cube.size.map(f32::from);
    let [mut x, mut y, mut z] = cube.origin;
    let (mut x2, mut y2, mut z2) = (x + dx, y + dy, z + dz);
    x -= cube.inflate;
    y -= cube.inflate;
    z -= cube.inflate;
    x2 += cube.inflate;
    y2 += cube.inflate;
    z2 += cube.inflate;
    if cube.mirror {
        std::mem::swap(&mut x, &mut x2);
    }
    let v7 = [x, y, z];
    let v0 = [x2, y, z];
    let v1 = [x2, y2, z];
    let v2 = [x, y2, z];
    let v3 = [x, y, z2];
    let v4 = [x2, y, z2];
    let v5 = [x2, y2, z2];
    let v6 = [x, y2, z2];
    let (u, v) = (cube.u, cube.v);
    let quads = [
        (
            [v4, v0, v1, v5],
            u + dz + dx,
            v + dz,
            u + dz + dx + dz,
            v + dz + dy,
        ),
        ([v7, v3, v6, v2], u, v + dz, u + dz, v + dz + dy),
        ([v4, v3, v7, v0], u + dz, v, u + dz + dx, v + dz),
        ([v1, v2, v6, v5], u + dz + dx, v + dz, u + dz + dx + dx, v),
        ([v0, v7, v2, v1], u + dz, v + dz, u + dz + dx, v + dz + dy),
        (
            [v3, v4, v5, v6],
            u + dz + dx + dz,
            v + dz,
            u + dz + dx + dz + dx,
            v + dz + dy,
        ),
    ];
    let centre = matrix.transform_point3(Vec3::new((x + x2) / 2.0, (y + y2) / 2.0, (z + z2) / 2.0));
    let scale = 1.0 / LAYER_SIZE as f32;
    for (corners, u1, v1, u2, v2) in quads {
        let uv = [[u2, v1], [u1, v1], [u1, v2], [u2, v2]].map(|[a, b]| [a * scale, b * scale]);
        let points = corners.map(|corner| matrix.transform_point3(Vec3::from(corner)));
        let normal = (points[1] - points[0]).cross(points[2] - points[0]);
        if normal.length_squared() == 0.0 {
            continue;
        }
        let normal = normal.normalize();
        let middle = (points[0] + points[2]) * 0.5;
        // Wind counter-clockwise as seen from outside the box.
        let outward = normal.dot(middle - centre) >= 0.0;
        let order = if outward {
            [0, 1, 2, 0, 2, 3]
        } else {
            [0, 2, 1, 0, 3, 2]
        };
        let facing = if outward { normal } else { -normal };
        let light = shade(facing);
        for index in order {
            out.push(Vertex {
                position: points[index].to_array(),
                uv: uv[index],
                layer,
                color: tint.map(|channel| channel * light),
                hurt: 0.0,
            });
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Interpolated yaw across the ±180 seam, as `RendererLivingEntity`.
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut delta = b - a;
    while delta < -180.0 {
        delta += 360.0;
    }
    while delta >= 180.0 {
        delta -= 360.0;
    }
    a + delta * t
}

fn position(entity: &Entity, partial: f32, origin: [i64; 3]) -> Vec3 {
    let p = f64::from(partial);
    Vec3::new(
        (entity.previous[0] + (entity.position[0] - entity.previous[0]) * p - origin[0] as f64)
            as f32,
        (entity.previous[1] + (entity.position[1] - entity.previous[1]) * p - origin[1] as f64)
            as f32,
        (entity.previous[2] + (entity.position[2] - entity.previous[2]) * p - origin[2] as f64)
            as f32,
    )
}

/// `ModelPlayer` with `ModelBiped.setRotationAngles`, drawn by `RenderPlayer`.
fn player(
    out: &mut Vec<Vertex>,
    entity: &Entity,
    slim: bool,
    partial: f32,
    origin: [i64; 3],
    registry: Option<&crate::terrain::Registry>,
    binding: Option<&crate::skins::Binding>,
) {
    let mut at = position(entity, partial, origin);
    let sneaking = entity.sneaking();
    if sneaking {
        at.y -= 0.125;
    }
    let body_yaw = lerp_angle(entity.previous_body_yaw, entity.body_yaw, partial);
    let head_yaw = lerp_angle(entity.previous_head_yaw, entity.head_yaw, partial) - body_yaw;
    let amount = lerp(entity.previous_limb_amount, entity.limb_amount, partial).min(1.0);
    let swing = entity.limb_swing - entity.limb_amount * (1.0 - partial);
    let age = entity.age as f32 + partial;
    let base = Mat4::from_translation(at)
        * Mat4::from_rotation_y((180.0 - body_yaw).to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(Vec3::splat(0.9375))
        * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0));
    let degrees = 180.0 / PI;
    let mut head = [
        lerp(entity.previous_pitch, entity.pitch, partial) / degrees,
        head_yaw / degrees,
        0.0,
    ];
    let mut body = [0.0f32; 3];
    let mut right_arm = [(swing * 0.6662 + PI).cos() * 2.0 * amount * 0.5, 0.0, 0.0];
    let mut left_arm = [(swing * 0.6662).cos() * 2.0 * amount * 0.5, 0.0, 0.0];
    let right_leg = [(swing * 0.6662).cos() * 1.4 * amount, 0.0, 0.0];
    let left_leg = [(swing * 0.6662 + PI).cos() * 1.4 * amount, 0.0, 0.0];
    if entity.equipment[0].is_some() {
        if entity.flags & 0x10 != 0
            && entity.equipment[0]
                .as_ref()
                .is_some_and(|i| matches!(i.id, 267 | 268 | 272 | 276 | 283))
        {
            right_arm[0] = right_arm[0] * 0.5 - PI * 0.3;
            right_arm[1] = -PI / 6.0;
        } else {
            right_arm[0] = right_arm[0] * 0.5 - PI / 10.0;
        }
    }
    let arm_width = if slim { 3.0 } else { 4.0 };
    let mut right_arm_point = [-5.0, if slim { 2.5 } else { 2.0 }, 0.0];
    let mut left_arm_point = [5.0, if slim { 2.5 } else { 2.0 }, 0.0];
    if let Some(ticks) = entity.swing {
        let progress = (f32::from(ticks) + partial) / 6.0;
        body[1] = (progress.sqrt() * PI * 2.0).sin() * 0.2;
        right_arm_point[2] = body[1].sin() * 5.0;
        right_arm_point[0] = -body[1].cos() * 5.0;
        left_arm_point[2] = -body[1].sin() * 5.0;
        left_arm_point[0] = body[1].cos() * 5.0;
        right_arm[1] += body[1];
        left_arm[1] += body[1];
        left_arm[0] += body[1];
        let eased = 1.0 - (1.0 - progress).powi(4);
        let lift = (eased * PI).sin();
        let reach = (progress * PI).sin() * -(head[0] - 0.7) * 0.75;
        right_arm[0] -= lift * 1.2 + reach;
        right_arm[1] += body[1] * 2.0;
        right_arm[2] += (progress * PI).sin() * -0.4;
    }
    let (leg_y, leg_z, head_y) = if sneaking {
        body[0] = 0.5;
        right_arm[0] += 0.4;
        left_arm[0] += 0.4;
        (9.0, 4.0, 1.0)
    } else {
        (12.0, 0.1, 0.0)
    };
    right_arm[2] += (age * 0.09).cos() * 0.05 + 0.05;
    left_arm[2] -= (age * 0.09).cos() * 0.05 + 0.05;
    right_arm[0] += (age * 0.067).sin() * 0.05;
    left_arm[0] -= (age * 0.067).sin() * 0.05;
    head[0] = head[0].clamp(-PI / 2.0, PI / 2.0);
    let skin = binding.map_or_else(
        || {
            if slim {
                Texture::Alex as u32
            } else {
                Texture::Steve as u32
            }
        },
        |b| b.skin,
    );
    let white = [1.0; 3];
    let head_matrix = part(base, [0.0, head_y, 0.0], head);
    add(
        out,
        head_matrix,
        cuboid(0.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8]),
        skin,
        white,
    );
    if entity.skin_parts & 64 != 0 {
        add(
            out,
            head_matrix,
            cuboid(32.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8]).inflated(0.5),
            skin,
            white,
        );
    }
    let body_matrix = part(base, [0.0; 3], body);
    add(
        out,
        body_matrix,
        cuboid(16.0, 16.0, [-4.0, 0.0, -2.0], [8, 12, 4]),
        skin,
        white,
    );
    if entity.skin_parts & 2 != 0 {
        add(
            out,
            body_matrix,
            cuboid(16.0, 32.0, [-4.0, 0.0, -2.0], [8, 12, 4]).inflated(0.25),
            skin,
            white,
        );
    }
    let arm = [arm_width as u8, 12, 4];
    let right = part(base, right_arm_point, right_arm);
    let left = part(base, left_arm_point, left_arm);
    let right_x = if slim { -2.0 } else { -3.0 };
    add(
        out,
        right,
        cuboid(40.0, 16.0, [right_x, -2.0, -2.0], arm),
        skin,
        white,
    );
    if entity.skin_parts & 8 != 0 {
        add(
            out,
            right,
            cuboid(40.0, 32.0, [right_x, -2.0, -2.0], arm).inflated(0.25),
            skin,
            white,
        );
    }
    add(
        out,
        left,
        cuboid(32.0, 48.0, [-1.0, -2.0, -2.0], arm),
        skin,
        white,
    );
    if entity.skin_parts & 4 != 0 {
        add(
            out,
            left,
            cuboid(48.0, 48.0, [-1.0, -2.0, -2.0], arm).inflated(0.25),
            skin,
            white,
        );
    }
    let right = part(base, [-1.9, leg_y, leg_z], right_leg);
    let left = part(base, [1.9, leg_y, leg_z], left_leg);
    add(
        out,
        right,
        cuboid(0.0, 16.0, [-2.0, 0.0, -2.0], [4, 12, 4]),
        skin,
        white,
    );
    if entity.skin_parts & 32 != 0 {
        add(
            out,
            right,
            cuboid(0.0, 32.0, [-2.0, 0.0, -2.0], [4, 12, 4]).inflated(0.25),
            skin,
            white,
        );
    }
    add(
        out,
        left,
        cuboid(16.0, 48.0, [-2.0, 0.0, -2.0], [4, 12, 4]),
        skin,
        white,
    );
    if entity.skin_parts & 16 != 0 {
        add(
            out,
            left,
            cuboid(0.0, 48.0, [-2.0, 0.0, -2.0], [4, 12, 4]).inflated(0.25),
            skin,
            white,
        );
    }
    for (slot, item) in entity.equipment.iter().enumerate().skip(1) {
        let Some(item) = item else { continue };
        let Some(texture) = armor_texture(item.id, slot) else {
            continue;
        };
        let inflation = if slot == 2 { 0.5 } else { 1.0 };
        let tint = leather_colour(item);
        let mut draw =
            |matrix, cube: Cuboid| add(out, matrix, cube.inflated(inflation), texture, tint);
        match slot {
            4 => draw(head_matrix, cuboid(0.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8])),
            3 => {
                draw(
                    body_matrix,
                    cuboid(16.0, 16.0, [-4.0, 0.0, -2.0], [8, 12, 4]),
                );
                draw(
                    part(base, right_arm_point, right_arm),
                    cuboid(40.0, 16.0, [-3.0, -2.0, -2.0], [4, 12, 4]),
                );
                let mut cube = cuboid(40.0, 16.0, [-1.0, -2.0, -2.0], [4, 12, 4]);
                cube.mirror = true;
                draw(part(base, left_arm_point, left_arm), cube);
            }
            2 => {
                draw(
                    body_matrix,
                    cuboid(16.0, 16.0, [-4.0, 0.0, -2.0], [8, 12, 4]),
                );
                draw(right, cuboid(0.0, 16.0, [-2.0, 0.0, -2.0], [4, 12, 4]));
                draw(left, cuboid(0.0, 16.0, [-2.0, 0.0, -2.0], [4, 12, 4]));
            }
            1 => {
                draw(right, cuboid(0.0, 16.0, [-2.0, 0.0, -2.0], [4, 12, 4]));
                draw(left, cuboid(0.0, 16.0, [-2.0, 0.0, -2.0], [4, 12, 4]));
            }
            _ => {}
        }
    }
    if let Some(item) = &entity.equipment[0] {
        let hand = part(base, right_arm_point, right_arm)
            * Mat4::from_translation(Vec3::new(-1.0, 9.0, 0.0));
        if item.id < 256 {
            if let Some(registry) = registry {
                registry.held_block(out, item, hand * Mat4::from_scale(Vec3::splat(6.0)));
            }
        } else if let Some(layer) = registry
            .and_then(|r| r.item_layer(item))
            .or_else(|| held_texture(item.id).map(|t| t as u32))
        {
            sprite(
                out,
                hand * Mat4::from_rotation_x(-PI / 2.0) * Mat4::from_rotation_y(PI),
                layer,
            );
        }
    }
    if entity.skin_parts & 1 != 0
        && let Some(cape) = binding.and_then(|b| b.cape)
    {
        let sway = (swing * 6.0).sin() * amount * 32.0;
        let angle = (6.0 + sway + if sneaking { 25.0 } else { 0.0 }).to_radians();
        let matrix = part(
            base,
            [0.0, if sneaking { 2.0 } else { 0.0 }, 2.0],
            [-angle, 0.0, 0.0],
        );
        add(
            out,
            matrix,
            cuboid(0.0, 0.0, [-5.0, 0.0, 0.0], [10, 16, 1]),
            cape,
            [1.0; 3],
        );
    }
}

fn armor_texture(id: i16, slot: usize) -> Option<Texture> {
    let offset = id.checked_sub(298)?;
    if !(0..20).contains(&offset) || 4 - offset as usize % 4 != slot {
        return None;
    }
    let leggings = slot == 2;
    Some(match (offset / 4, leggings) {
        (0, false) => Texture::Leather1,
        (0, true) => Texture::Leather2,
        (1, false) => Texture::Chain1,
        (1, true) => Texture::Chain2,
        (2, false) => Texture::Iron1,
        (2, true) => Texture::Iron2,
        (3, false) => Texture::Diamond1,
        (3, true) => Texture::Diamond2,
        (4, false) => Texture::Gold1,
        (4, true) => Texture::Gold2,
        _ => return None,
    })
}

fn leather_colour(item: &horizon_protocol_47::nbt::Item) -> [f32; 3] {
    if !(298..=301).contains(&item.id) {
        return [1.0; 3];
    }
    let colour = item
        .nbt
        .as_ref()
        .and_then(|n| n.get("display"))
        .and_then(|n| n.get("color"))
        .and_then(|n| n.as_int())
        .unwrap_or(0xa06540);
    [
        ((colour >> 16) & 255) as f32 / 255.0,
        ((colour >> 8) & 255) as f32 / 255.0,
        (colour & 255) as f32 / 255.0,
    ]
}

fn held_texture(id: i16) -> Option<Texture> {
    Some(match id {
        268 => Texture::WoodSword,
        272 => Texture::StoneSword,
        267 => Texture::IronSword,
        276 => Texture::DiamondSword,
        283 => Texture::GoldSword,
        261 => Texture::Bow,
        _ => return None,
    })
}

/// Double-sided item sprite attached to the animated hand. Alpha cutout
/// retains the owned item's silhouette.
fn sprite(out: &mut Vec<Vertex>, matrix: Mat4, texture: u32) {
    let points = [
        [-8.0, 0.0, 0.0],
        [-8.0, -16.0, 0.0],
        [8.0, -16.0, 0.0],
        [8.0, 0.0, 0.0],
    ];
    let uv = [[0.0, 16.0], [0.0, 0.0], [16.0, 0.0], [16.0, 16.0]];
    for i in [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] {
        out.push(Vertex {
            position: matrix.transform_point3(Vec3::from(points[i])).to_array(),
            uv: uv[i].map(|v| v / LAYER_SIZE as f32),
            layer: texture,
            color: [1.0; 3],
            hurt: 0.0,
        });
    }
}

/// Standard zombie and skeleton boxes with `ModelZombie` arm animation.
fn humanoid_mob(out: &mut Vec<Vertex>, entity: &Entity, partial: f32, origin: [i64; 3]) {
    let skeleton = entity.kind == Kind::Mob(51);
    let texture = if skeleton && entity.variant == 1 {
        Texture::WitherSkeleton
    } else if skeleton {
        Texture::Skeleton
    } else {
        Texture::Zombie
    };
    let yaw = lerp_angle(entity.previous_body_yaw, entity.body_yaw, partial);
    let head_yaw = lerp_angle(entity.previous_head_yaw, entity.head_yaw, partial) - yaw;
    let base = Mat4::from_translation(position(entity, partial, origin))
        * Mat4::from_rotation_y((180.0 - yaw).to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(Vec3::splat(if skeleton && entity.variant == 1 {
            1.2
        } else {
            1.0
        }))
        * Mat4::from_translation(Vec3::new(0.0, -1.507_812_5, 0.0));
    let head_base = if entity.child {
        base * Mat4::from_scale(Vec3::splat(0.75)) * Mat4::from_translation(Vec3::Y)
    } else {
        base
    };
    let base = if entity.child {
        base * Mat4::from_scale(Vec3::splat(0.5)) * Mat4::from_translation(Vec3::Y * 1.5)
    } else {
        base
    };
    let amount = lerp(entity.previous_limb_amount, entity.limb_amount, partial).min(1.0);
    let limb = entity.limb_swing - entity.limb_amount * (1.0 - partial);
    let age = entity.age as f32 + partial;
    let progress = entity
        .swing
        .map_or(0.0, |tick| (f32::from(tick) + partial) / 6.0);
    let strike = (progress * PI).sin();
    let recovery = ((1.0 - (1.0 - progress).powi(2)) * PI).sin();
    let arm_x = -PI / 2.0 - strike * 1.2 + recovery * 0.4;
    let idle_z = (age * 0.09).cos() * 0.05 + 0.05;
    let idle_x = (age * 0.067).sin() * 0.05;
    let white = [1.0; 3];
    let head = part(
        head_base,
        [0.0; 3],
        [
            lerp(entity.previous_pitch, entity.pitch, partial).to_radians(),
            head_yaw.to_radians(),
            0.0,
        ],
    );
    add(
        out,
        head,
        cuboid(0.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8]),
        texture,
        white,
    );
    if !skeleton {
        add(
            out,
            head,
            cuboid(32.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8]).inflated(0.5),
            texture,
            white,
        );
    }
    add(
        out,
        part(base, [0.0; 3], [0.0; 3]),
        cuboid(16.0, 16.0, [-4.0, 0.0, -2.0], [8, 12, 4]),
        texture,
        white,
    );
    let (width, depth, right_x, z) = if skeleton {
        (2, 2, -1.0, -1.0)
    } else {
        (4, 4, -3.0, -2.0)
    };
    for (side, mirror) in [(-1.0, false), (1.0, true)] {
        let arm = part(
            base,
            [side * 5.0, 2.0, 0.0],
            [
                arm_x - side * idle_x,
                side * (0.1 - strike * 0.6),
                -side * idle_z,
            ],
        );
        let mut arm_box = cuboid(
            40.0,
            16.0,
            [if mirror { -1.0 } else { right_x }, -2.0, z],
            [width, 12, depth],
        );
        arm_box.mirror = mirror;
        add(out, arm, arm_box, texture, white);
        let leg = part(
            base,
            [side * 1.9, 12.0, 0.0],
            [
                (limb * 0.6662 + if mirror { PI } else { 0.0 }).cos() * 1.4 * amount,
                0.0,
                0.0,
            ],
        );
        let mut leg_box = cuboid(
            0.0,
            16.0,
            [if skeleton { -1.0 } else { -2.0 }, 0.0, z],
            [width, 12, depth],
        );
        leg_box.mirror = mirror;
        add(out, leg, leg_box, texture, white);
    }
}

/// Owned adult creeper, pig, cow and villager model boxes.
fn common_mob(out: &mut Vec<Vertex>, entity: &Entity, partial: f32, origin: [i64; 3]) {
    let yaw = lerp_angle(entity.previous_body_yaw, entity.body_yaw, partial);
    let head_yaw = lerp_angle(entity.previous_head_yaw, entity.head_yaw, partial) - yaw;
    let base = Mat4::from_translation(position(entity, partial, origin))
        * Mat4::from_rotation_y((180.0 - yaw).to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0));
    let amount = lerp(entity.previous_limb_amount, entity.limb_amount, partial).min(1.0);
    let limb = entity.limb_swing - entity.limb_amount * (1.0 - partial);
    let texture = match entity.kind {
        Kind::Mob(50) => Texture::CreeperMob,
        Kind::Mob(90) => Texture::Pig,
        Kind::Mob(92) => Texture::Cow,
        _ => Texture::Villager,
    };
    let mut draw = |matrix, cube| add(out, matrix, cube, texture, [1.0; 3]);
    if entity.kind == Kind::Mob(120) {
        let head = part(
            base,
            [0.0; 3],
            [
                lerp(entity.previous_pitch, entity.pitch, partial).to_radians(),
                head_yaw.to_radians(),
                0.0,
            ],
        );
        draw(head, cuboid(0.0, 0.0, [-4.0, -10.0, -4.0], [8, 10, 8]));
        draw(head, cuboid(24.0, 0.0, [-1.0, -3.0, -6.0], [2, 4, 2]));
        let body = part(base, [0.0; 3], [0.0; 3]);
        draw(body, cuboid(16.0, 20.0, [-4.0, 0.0, -3.0], [8, 12, 6]));
        draw(
            body,
            cuboid(0.0, 38.0, [-4.0, 0.0, -3.0], [8, 18, 6]).inflated(0.5),
        );
        let arms = part(base, [0.0, 3.0, -1.0], [-0.75, 0.0, 0.0]);
        for x in [-8.0, 4.0] {
            draw(arms, cuboid(44.0, 22.0, [x, -2.0, -2.0], [4, 8, 4]));
        }
        draw(arms, cuboid(40.0, 38.0, [-4.0, 2.0, -2.0], [8, 4, 4]));
        for (x, phase) in [(-2.0, 0.0), (2.0, PI)] {
            draw(
                part(
                    base,
                    [x, 12.0, 0.0],
                    [(limb * 0.6662 + phase).cos() * 0.7 * amount, 0.0, 0.0],
                ),
                cuboid(0.0, 22.0, [-2.0, 0.0, -2.0], [4, 12, 4]),
            );
        }
        return;
    }
    let creeper = entity.kind == Kind::Mob(50);
    let cow = entity.kind == Kind::Mob(92);
    let head_at = if creeper {
        [0.0, 6.0, 0.0]
    } else if cow {
        [0.0, 4.0, -8.0]
    } else {
        [0.0, 12.0, -6.0]
    };
    let head_base = if entity.child {
        base * Mat4::from_translation(Vec3::new(
            0.0,
            if cow { 0.5 } else { 0.25 },
            if cow { 0.375 } else { 0.25 },
        ))
    } else {
        base
    };
    let head = part(
        head_base,
        head_at,
        [
            lerp(entity.previous_pitch, entity.pitch, partial).to_radians(),
            head_yaw.to_radians(),
            0.0,
        ],
    );
    let head_box = if creeper {
        cuboid(0.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8])
    } else if cow {
        cuboid(0.0, 0.0, [-4.0, -4.0, -6.0], [8, 8, 6])
    } else {
        cuboid(0.0, 0.0, [-4.0, -4.0, -8.0], [8, 8, 8])
    };
    draw(head, head_box);
    if cow {
        for x in [-5.0, 4.0] {
            draw(head, cuboid(22.0, 0.0, [x, -5.0, -4.0], [1, 3, 1]));
        }
    } else if !creeper {
        draw(head, cuboid(16.0, 16.0, [-2.0, 0.0, -9.0], [4, 3, 1]));
    }
    let base = if entity.child {
        base * Mat4::from_scale(Vec3::splat(0.5)) * Mat4::from_translation(Vec3::Y * 1.5)
    } else {
        base
    };
    let body = if creeper {
        part(base, [0.0, 6.0, 0.0], [0.0; 3])
    } else {
        part(
            base,
            [0.0, if cow { 5.0 } else { 11.0 }, 2.0],
            [PI / 2.0, 0.0, 0.0],
        )
    };
    draw(
        body,
        if creeper {
            cuboid(16.0, 16.0, [-4.0, 0.0, -2.0], [8, 12, 4])
        } else if cow {
            cuboid(18.0, 4.0, [-6.0, -10.0, -7.0], [12, 18, 10])
        } else {
            cuboid(28.0, 8.0, [-5.0, -10.0, -7.0], [10, 16, 8])
        },
    );
    if cow {
        draw(body, cuboid(52.0, 0.0, [-2.0, 2.0, -8.0], [4, 6, 1]));
    }
    let leg_h = if cow { 12 } else { 6 };
    let spread = if creeper {
        2.0
    } else if cow {
        4.0
    } else {
        3.0
    };
    for (x, z, phase) in [
        (-spread, if creeper { 4.0 } else { 7.0 }, 0.0),
        (spread, if creeper { 4.0 } else { 7.0 }, PI),
        (
            -spread,
            if creeper {
                -4.0
            } else if cow {
                -6.0
            } else {
                -5.0
            },
            PI,
        ),
        (
            spread,
            if creeper {
                -4.0
            } else if cow {
                -6.0
            } else {
                -5.0
            },
            0.0,
        ),
    ] {
        draw(
            part(
                base,
                [x, 24.0 - f32::from(leg_h), z],
                [(limb * 0.6662 + phase).cos() * 1.4 * amount, 0.0, 0.0],
            ),
            cuboid(0.0, 16.0, [-2.0, 0.0, -2.0], [4, leg_h, 4]),
        );
    }
}

/// Placeholder size (width, height) and tint for non-player entities.
fn placeholder(kind: Kind) -> ([f32; 2], [f32; 3]) {
    let hostile = [0.75, 0.32, 0.30];
    let passive = [0.42, 0.72, 0.38];
    match kind {
        Kind::Mob(50) => ([0.6, 1.7], hostile),
        Kind::Mob(51 | 54 | 57 | 66) => ([0.6, 1.95], hostile),
        Kind::Mob(52) => ([1.4, 0.9], hostile),
        Kind::Mob(53) => ([3.6, 10.8], hostile),
        Kind::Mob(55 | 62) => ([1.0, 1.0], hostile),
        Kind::Mob(56) => ([4.0, 4.0], hostile),
        Kind::Mob(58) => ([0.6, 2.9], hostile),
        Kind::Mob(59) => ([0.7, 0.5], hostile),
        Kind::Mob(60 | 67) => ([0.4, 0.3], hostile),
        Kind::Mob(61) => ([0.6, 1.8], hostile),
        Kind::Mob(63) => ([16.0, 8.0], hostile),
        Kind::Mob(64) => ([0.9, 3.5], hostile),
        Kind::Mob(65) => ([0.5, 0.9], passive),
        Kind::Mob(68) => ([0.85, 0.85], hostile),
        Kind::Mob(90) => ([0.9, 0.9], passive),
        Kind::Mob(91 | 92 | 96) => ([0.9, 1.3], passive),
        Kind::Mob(93) => ([0.4, 0.7], passive),
        Kind::Mob(94) => ([0.95, 0.95], passive),
        Kind::Mob(95) => ([0.6, 0.8], passive),
        Kind::Mob(97) => ([0.7, 1.9], passive),
        Kind::Mob(98 | 101) => ([0.6, 0.7], passive),
        Kind::Mob(99) => ([1.4, 2.9], [0.78, 0.72, 0.62]),
        Kind::Mob(100) => ([1.4, 1.6], passive),
        Kind::Mob(120) => ([0.6, 1.95], [0.78, 0.62, 0.48]),
        Kind::Object(2) => ([0.25, 0.25], [0.95, 0.85, 0.35]),
        Kind::Object(60) => ([0.5, 0.5], [0.85, 0.85, 0.85]),
        Kind::Object(1) => ([1.5, 0.6], [0.55, 0.40, 0.25]),
        Kind::Object(10) => ([0.98, 0.7], [0.5, 0.5, 0.55]),
        Kind::Object(50) => ([0.98, 0.98], [0.85, 0.25, 0.2]),
        Kind::Object(70) => ([0.98, 0.98], [0.7, 0.7, 0.7]),
        Kind::Object(78) => ([0.5, 1.975], [0.8, 0.8, 0.8]),
        Kind::Orb => ([0.5, 0.5], [0.6, 0.95, 0.3]),
        Kind::Mob(_) => ([0.6, 1.8], hostile),
        _ => ([0.25, 0.25], [0.8, 0.8, 0.8]),
    }
}

/// Geometry for every visible tracked entity at the frame's partial tick.
#[allow(clippy::too_many_arguments)]
pub fn entities(
    out: &mut Vec<Vertex>,
    entities: &[(i32, Entity)],
    partial: f32,
    origin: [i64; 3],
    registry: Option<&crate::terrain::Registry>,
    bindings: Option<&std::collections::HashMap<u128, crate::skins::Binding>>,
    camera: horizon_protocol_47::play::PositionLook,
) {
    for (id, entity) in entities {
        if entity.invisible() {
            continue;
        }
        let start = out.len();
        let binding = bindings.and_then(|b| b.get(&entity.uuid));
        match entity.kind {
            Kind::Player { slim } => player(
                out,
                entity,
                binding.map_or(slim, |b| b.slim),
                partial,
                origin,
                registry,
                binding,
            ),
            Kind::Mob(51 | 54) => humanoid_mob(out, entity, partial, origin),
            Kind::Mob(50 | 90 | 92 | 120) => common_mob(out, entity, partial, origin),
            Kind::Mob(52 | 55 | 58 | 59 | 91 | 93 | 95) => {
                mobs::render(out, entity, partial, origin)
            }
            Kind::Object(2) => {
                if let Some(registry) = registry {
                    dropped_item(out, *id, entity, partial, origin, registry);
                }
            }
            Kind::Object(60) => arrow(out, entity, partial, origin),
            Kind::Object(50 | 70) => {
                if let Some(registry) = registry {
                    falling_block(out, entity, partial, origin, registry);
                }
            }
            Kind::Object(61 | 62 | 63 | 64 | 65 | 66 | 72 | 73 | 75 | 90) | Kind::Orb => {
                if let Some(registry) = registry {
                    billboard_object(out, entity, partial, origin, registry, camera);
                }
            }
            kind => {
                let ([width, height], tint) = placeholder(kind);
                let at = position(entity, partial, origin);
                let yaw = lerp_angle(entity.previous_body_yaw, entity.body_yaw, partial);
                let matrix =
                    Mat4::from_translation(at) * Mat4::from_rotation_y((-yaw).to_radians());
                let w = (width * 16.0).round().clamp(1.0, 255.0) as u8;
                let h = (height * 16.0).round().clamp(1.0, 255.0) as u8;
                let scale = Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                add(
                    out,
                    matrix * scale,
                    cuboid(
                        0.0,
                        0.0,
                        [-f32::from(w) / 2.0, 0.0, -f32::from(w) / 2.0],
                        [w, h, w],
                    ),
                    Texture::White,
                    tint,
                );
            }
        }
        if entity.hurt_time > 0 {
            for vertex in &mut out[start..] {
                vertex.hurt = 0.3;
            }
        }
    }
}

/// A deterministic float in 0..1 for a seed, standing in for `Random`.
fn noise(seed: u64) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// `java.util.Random`, for render-time sequences vanilla seeds explicitly.
struct JavaRandom(u64);

impl JavaRandom {
    fn new(seed: u64) -> Self {
        Self((seed ^ 0x5_DEEC_E66D) & ((1 << 48) - 1))
    }

    fn next_float(&mut self) -> f32 {
        self.0 = (self.0.wrapping_mul(0x5_DEEC_E66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.0 >> 24) as f32 / (1u32 << 24) as f32
    }
}

fn block_item(item: &horizon_protocol_47::nbt::Item) -> bool {
    (1..198).contains(&item.id) && !crate::hand::flat_block(item.id)
}

/// `RenderEntityItem`: bobbing, spinning, extra copies for bigger stacks
/// and each model's `GROUND` transform.
fn dropped_item(
    out: &mut Vec<Vertex>,
    id: i32,
    entity: &Entity,
    partial: f32,
    origin: [i64; 3],
    registry: &crate::terrain::Registry,
) {
    let Some(item) = &entity.item else {
        return;
    };
    let age = entity.age as f32 + partial;
    let hover = noise(id as u64) * PI * 2.0;
    let bob = (age / 10.0 + hover).sin() * 0.1 + 0.1;
    let cube = block_item(item);
    let (lift, scale) = if cube {
        (3.0 / 16.0, 0.25)
    } else {
        (2.0 / 16.0, 0.5)
    };
    let copies = match item.count {
        49.. => 5,
        33..=48 => 4,
        17..=32 => 3,
        2..=16 => 2,
        _ => 1,
    };
    // `RenderEntityItem` reseeds its `Random` with 187 for every item.
    let mut random = JavaRandom::new(187);
    let mut m = Mat4::from_translation(
        position(entity, partial, origin) + Vec3::new(0.0, bob + 0.25 * scale, 0.0),
    ) * Mat4::from_rotation_y(age / 20.0 + hover);
    if !cube {
        m *= Mat4::from_translation(Vec3::new(0.0, 0.0, -0.046875 * (copies - 1) as f32 * 0.5));
    }
    for copy in 0..copies {
        let mut part = m;
        // Only 3D models scatter their copies; flat ones stack in depth.
        if cube && copy > 0 {
            let mut offset = || (random.next_float() * 2.0 - 1.0) * 0.15;
            let x = offset();
            let y = offset();
            let z = offset();
            part *= Mat4::from_translation(Vec3::new(x, y, z));
        }
        part *= Mat4::from_translation(Vec3::new(0.0, lift, 0.0))
            * Mat4::from_scale(Vec3::splat(scale));
        if cube {
            registry.held_block(out, item, part);
        } else if let Some(layer) = registry.item_layer(item) {
            registry.item_mesh(
                out,
                layer,
                part * Mat4::from_translation(Vec3::splat(-0.5)),
                [1.0; 3],
            );
        }
        if !cube {
            m *= Mat4::from_translation(Vec3::new(0.0, 0.0, 0.046875));
        }
    }
}

/// A textured quad given in `arrow.png` pixels, both sides.
fn pixel_quad(
    out: &mut Vec<Vertex>,
    m: Mat4,
    corners: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    layer: u32,
    color: [f32; 3],
) {
    let corners = corners.map(|p| m.transform_point3(Vec3::from(p)).to_array());
    for i in [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] {
        out.push(Vertex {
            position: corners[i],
            uv: uv[i].map(|v| v / LAYER_SIZE as f32),
            layer,
            color,
            hurt: 0.0,
        });
    }
}

/// `RenderArrow`: a back cross and two crossed shaft planes, 0.05625 scale.
fn arrow(out: &mut Vec<Vertex>, entity: &Entity, partial: f32, origin: [i64; 3]) {
    let yaw = entity.yaw;
    let pitch = lerp(entity.previous_pitch, entity.pitch, partial);
    let m = Mat4::from_translation(position(entity, partial, origin))
        * Mat4::from_rotation_y((yaw - 90.0).to_radians())
        * Mat4::from_rotation_z(pitch.to_radians())
        * Mat4::from_scale(Vec3::splat(0.05625))
        * Mat4::from_translation(Vec3::new(-4.0, 0.0, 0.0));
    let layer = Texture::Arrow as u32;
    pixel_quad(
        out,
        m,
        [
            [-7.0, -2.0, -2.0],
            [-7.0, -2.0, 2.0],
            [-7.0, 2.0, 2.0],
            [-7.0, 2.0, -2.0],
        ],
        [[0.0, 5.0], [5.0, 5.0], [5.0, 10.0], [0.0, 10.0]],
        layer,
        [1.0; 3],
    );
    let mut plane = m;
    for _ in 0..2 {
        plane *= Mat4::from_rotation_x(PI / 2.0);
        pixel_quad(
            out,
            plane,
            [
                [-8.0, -2.0, 0.0],
                [8.0, -2.0, 0.0],
                [8.0, 2.0, 0.0],
                [-8.0, 2.0, 0.0],
            ],
            [[0.0, 0.0], [16.0, 0.0], [16.0, 5.0], [0.0, 5.0]],
            layer,
            [1.0; 3],
        );
    }
}

/// `RenderTNTPrimed` (swelling and flashing over its 80-tick fuse) and
/// `RenderFallingBlock`.
fn falling_block(
    out: &mut Vec<Vertex>,
    entity: &Entity,
    partial: f32,
    origin: [i64; 3],
    registry: &crate::terrain::Registry,
) {
    let at = position(entity, partial, origin) + Vec3::new(0.0, 0.5, 0.0);
    if entity.kind == Kind::Object(50) {
        let fuse = 80.0 - entity.age as f32;
        let left = fuse - partial + 1.0;
        let mut m = Mat4::from_translation(at);
        if left < 10.0 {
            let f = (1.0 - left / 10.0).clamp(0.0, 1.0).powi(4);
            m *= Mat4::from_scale(Vec3::splat(1.0 + f * 0.3));
        }
        let flash = (fuse as i32 / 5) % 2 == 0;
        let light = if flash {
            1.0 + (1.0 - left / 100.0).max(0.0) * 0.8
        } else {
            1.0
        };
        registry.block_model(out, horizon_world_189::BlockState(46 << 4), m, light);
    } else {
        let id = (entity.data & 4095) as u16;
        let meta = ((entity.data >> 12) & 15) as u16;
        if (1..198).contains(&id) {
            registry.block_model(
                out,
                horizon_world_189::BlockState(id << 4 | meta),
                Mat4::from_translation(at),
                1.0,
            );
        }
    }
}

/// Camera-facing sprites: `RenderSnowball` items, `RenderFireball`,
/// `RenderFish`'s bobber and `RenderXPOrb`.
fn billboard_object(
    out: &mut Vec<Vertex>,
    entity: &Entity,
    partial: f32,
    origin: [i64; 3],
    registry: &crate::terrain::Registry,
    camera: horizon_protocol_47::play::PositionLook,
) {
    let facing = Mat4::from_translation(position(entity, partial, origin))
        * Mat4::from_rotation_y((-camera.yaw).to_radians())
        * Mat4::from_rotation_x(camera.pitch.to_radians());
    let item = |id: i16, damage: i16| horizon_protocol_47::nbt::Item {
        id,
        count: 1,
        damage,
        nbt: None,
    };
    let sprite =
        |out: &mut Vec<Vertex>, layer: Option<u32>, size: f32, low: f32, color: [f32; 3]| {
            if let Some(layer) = layer {
                let (x, y0, y1) = (size / 2.0, low * size, (low + 1.0) * size);
                pixel_quad(
                    out,
                    facing,
                    [[-x, y0, 0.0], [x, y0, 0.0], [x, y1, 0.0], [-x, y1, 0.0]],
                    [[0.0, 16.0], [16.0, 16.0], [16.0, 0.0], [0.0, 0.0]],
                    layer,
                    color,
                );
            }
        };
    match entity.kind {
        Kind::Object(kind @ (61 | 62 | 65 | 72 | 75)) => {
            let id = match kind {
                61 => 332,
                62 => 344,
                65 => 368,
                72 => 381,
                _ => 384,
            };
            sprite(
                out,
                registry.item_layer(&item(id, 0)),
                0.25,
                -0.25,
                [1.0; 3],
            );
        }
        Kind::Object(73) => {
            let potion = item(373, (entity.data & 0x7fff) as i16 | 16384);
            let [r, g, b, _] = crate::ui::argb(crate::tooltip::potion_color(&potion) | 0xFF00_0000);
            sprite(
                out,
                registry.sprite("potion_overlay"),
                0.25,
                -0.25,
                [r, g, b],
            );
            sprite(
                out,
                registry.sprite("potion_bottle_splash"),
                0.25,
                -0.25,
                [1.0; 3],
            );
        }
        Kind::Object(kind @ (63 | 64 | 66)) => {
            let size = if kind == 63 { 2.0 } else { 0.5 };
            sprite(
                out,
                registry.item_layer(&item(385, 0)),
                size,
                -0.25,
                [1.0; 3],
            );
        }
        Kind::Object(90) => {
            let (x, y) = (0.25, 0.25);
            pixel_quad(
                out,
                facing,
                [[-x, -y, 0.0], [x, -y, 0.0], [x, y, 0.0], [-x, y, 0.0]],
                [[8.0, 24.0], [16.0, 24.0], [16.0, 16.0], [8.0, 16.0]],
                Texture::Particles as u32,
                [1.0; 3],
            );
        }
        Kind::Orb => {
            let phase = (entity.age as f32 + partial) / 2.0;
            let red = ((phase.sin() + 1.0) * 0.5 * 255.0).floor() / 255.0;
            let blue = ((phase + 4.1887903).sin() + 1.0) * 0.1;
            let (x, y0, y1) = (0.15, -0.075, 0.225);
            pixel_quad(
                out,
                facing,
                [[-x, y0, 0.0], [x, y0, 0.0], [x, y1, 0.0], [-x, y1, 0.0]],
                [[0.0, 16.0], [16.0, 16.0], [16.0, 0.0], [0.0, 0.0]],
                Texture::ExperienceOrb as u32,
                [red, 1.0, blue],
            );
        }
        _ => {}
    }
}

/// `RenderPlayer.renderRightArm` for the first-person hand: the arm and
/// sleeve with `ModelBiped`'s idle sway at age zero.
pub(crate) fn first_person_arm(
    out: &mut Vec<Vertex>,
    m: Mat4,
    skin: u32,
    slim: bool,
    sleeve: bool,
) {
    let point = [-5.0, if slim { 2.5 } else { 2.0 }, 0.0];
    let right = part(m, point, [0.0, 0.0, 0.1]);
    let x = if slim { -2.0 } else { -3.0 };
    let size = [if slim { 3 } else { 4 }, 12, 4];
    add(
        out,
        right,
        cuboid(40.0, 16.0, [x, -2.0, -2.0], size),
        skin,
        [1.0; 3],
    );
    if sleeve {
        add(
            out,
            right,
            cuboid(40.0, 32.0, [x, -2.0, -2.0], size).inflated(0.25),
            skin,
            [1.0; 3],
        );
    }
}

/// Depth-tested player labels. Texture properties remain on each profile
/// for an asynchronous skin/cape provider to consume.
pub fn nametags(
    out: &mut Vec<Vertex>,
    entities: &[(i32, Entity)],
    partial: f32,
    origin: [i64; 3],
    camera: horizon_protocol_47::play::PositionLook,
    widths: &[u8; 256],
    board: &crate::scoreboard::Scoreboard,
    own_name: &str,
) {
    // `RendererLivingEntity.canRenderName` with team name-tag visibility.
    let own_team = board.team_of(own_name).map(|(name, _)| name);
    let below = board.slot(2);
    let label = |out: &mut Vec<Vertex>, at: Vec3, text: &str, row: f32| {
        let f = 0.016_666_668 * 1.6;
        let matrix = Mat4::from_translation(at)
            * Mat4::from_rotation_y((-camera.yaw).to_radians())
            * Mat4::from_rotation_x(camera.pitch.to_radians())
            * Mat4::from_scale(Vec3::new(-f, -f, f));
        text_line(out, matrix, text, row, widths, [1.0; 3]);
    };
    for (_, entity) in entities {
        let distance: f64 = entity
            .position
            .iter()
            .zip(camera.position)
            .map(|(a, b)| (a - b) * (a - b))
            .sum();
        match (&entity.kind, &entity.profile) {
            (Kind::Player { .. }, Some(profile)) => {
                if entity.invisible() {
                    continue;
                }
                if let Some((team, info)) = board.team_of(&profile.name) {
                    let same = own_team == Some(team);
                    let shown = match info.name_tags.as_str() {
                        "never" => false,
                        "hideForOtherTeams" => own_team.is_none() || same,
                        "hideForOwnTeam" => own_team.is_none() || !same,
                        _ => true,
                    };
                    if !shown {
                        continue;
                    }
                }
                let range = if entity.sneaking() { 32.0 } else { 64.0 };
                if distance > range * range {
                    continue;
                }
                let mut at = position(entity, partial, origin)
                    + Vec3::new(0.0, if entity.sneaking() { 2.05 } else { 2.3 }, 0.0);
                // `renderOffsetLivingLabel`: the below-name score within ten
                // blocks, with the name raised one line.
                if let Some((objective, info)) = below
                    && distance < 100.0
                {
                    let points = board.points(objective, &profile.name).unwrap_or(0);
                    label(out, at, &format!("{points} {}", info.display), 0.0);
                    at += Vec3::new(0.0, 9.0 * 1.15 * 0.016_666_668 * 1.6, 0.0);
                }
                label(out, at, &board.format(&profile.name), 0.0);
            }
            (Kind::Mob(_) | Kind::Object(78), _) => {
                let Some(name) = &entity.custom_name else {
                    continue;
                };
                if !entity.name_visible || distance > 64.0 * 64.0 {
                    continue;
                }
                let at = position(entity, partial, origin)
                    + Vec3::new(0.0, entity.label_height() as f32, 0.0);
                label(out, at, name, 0.0);
            }
            _ => {}
        }
    }
}

/// Chest kinds as the tile-entity renderer picks their texture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChestKind {
    Normal,
    Trapped,
    Ender,
}

/// `TileEntityChestRenderer` for a closed chest. `double` is the side the
/// partner lies on (+X or +Z), if this half draws the large model.
pub fn chest(
    out: &mut Vec<Vertex>,
    at: Vec3,
    facing: u8,
    kind: ChestKind,
    double: Option<bool>,
    light: f32,
) {
    let rotation: f32 = match facing {
        2 => 180.0,
        4 => 90.0,
        5 => -90.0,
        _ => 0.0,
    };
    let mut matrix = Mat4::from_translation(at + Vec3::new(0.0, 1.0, 1.0))
        * Mat4::from_scale(Vec3::new(1.0, -1.0, -1.0))
        * Mat4::from_translation(Vec3::splat(0.5));
    if double == Some(true) && facing == 2 {
        matrix *= Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0));
    }
    if double == Some(false) && facing == 5 {
        matrix *= Mat4::from_translation(Vec3::new(0.0, 0.0, -1.0));
    }
    matrix *=
        Mat4::from_rotation_y(rotation.to_radians()) * Mat4::from_translation(Vec3::splat(-0.5));
    let tint = [light; 3];
    let (texture, width) = match (kind, double.is_some()) {
        (ChestKind::Ender, _) => (Texture::Ender, 14u8),
        (ChestKind::Normal, false) => (Texture::Chest, 14),
        (ChestKind::Trapped, false) => (Texture::Trapped, 14),
        (ChestKind::Normal, true) => (Texture::ChestDouble, 30),
        (ChestKind::Trapped, true) => (Texture::TrappedDouble, 30),
    };
    let knob_x = if width == 30 { 16.0 } else { 8.0 };
    add(
        out,
        part(matrix, [1.0, 7.0, 15.0], [0.0; 3]),
        cuboid(0.0, 0.0, [0.0, -5.0, -14.0], [width, 5, 14]),
        texture,
        tint,
    );
    add(
        out,
        part(matrix, [knob_x, 7.0, 15.0], [0.0; 3]),
        cuboid(0.0, 0.0, [-1.0, -2.0, -15.0], [2, 4, 1]),
        texture,
        tint,
    );
    add(
        out,
        part(matrix, [1.0, 6.0, 1.0], [0.0; 3]),
        cuboid(0.0, 19.0, [0.0, 0.0, 0.0], [width, 10, 14]),
        texture,
        tint,
    );
}

/// `FontRenderer` glyph advances from `ascii.png`, as `readFontTexture`.
pub fn glyph_widths(font: &[u8], width: u32) -> [u8; 256] {
    let mut widths = [0u8; 256];
    let cell = (width / 16) as usize;
    for (code, slot) in widths.iter_mut().enumerate() {
        let (cx, cy) = (code % 16 * cell, code / 16 * cell);
        let mut last = 0;
        for x in (0..cell).rev() {
            let opaque = (0..cell).any(|y| {
                let index = ((cy + y) * width as usize + cx + x) * 4 + 3;
                font.get(index).is_some_and(|&alpha| alpha > 0)
            });
            if opaque {
                last = x + 1;
                break;
            }
        }
        *slot = if code == 32 { 4 } else { (last + 1) as u8 };
    }
    widths
}

fn text_line(
    out: &mut Vec<Vertex>,
    matrix: Mat4,
    text: &str,
    y: f32,
    widths: &[u8; 256],
    colour: [f32; 3],
) {
    // `§` colour codes recolour, other codes are skipped; glyphs outside
    // ascii.png (unicode pages) are left out of world text.
    let base = colour;
    let glyphs: Vec<(usize, [f32; 3])> = {
        let mut colour = base;
        let mut glyphs = Vec::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '§' {
                match chars.next().map(|c| c.to_ascii_lowercase()) {
                    Some(code @ ('0'..='9' | 'a'..='f')) => {
                        let index = code.to_digit(16).unwrap_or(15) as usize;
                        colour = crate::ui::color_code(index);
                    }
                    Some('r') => colour = base,
                    _ => {}
                }
                continue;
            }
            if let Some(index) = crate::ui::ascii_index(c) {
                glyphs.push((index, colour));
            }
        }
        glyphs
    };
    let total: f32 = glyphs
        .iter()
        .map(|(code, _)| f32::from(widths[*code]))
        .sum();
    let mut x = (-total / 2.0).trunc();
    let scale = 1.0 / LAYER_SIZE as f32;
    for (code, colour) in glyphs {
        let advance = f32::from(widths[code]);
        if code != 32 && advance > 1.0 {
            let (u, v) = ((code % 16 * 8) as f32, (code / 16 * 8) as f32);
            let w = advance - 1.01;
            let corners = [
                Vec3::new(x, y, 0.0),
                Vec3::new(x, y + 7.99, 0.0),
                Vec3::new(x + w, y + 7.99, 0.0),
                Vec3::new(x + w, y, 0.0),
            ]
            .map(|point| matrix.transform_point3(point).to_array());
            let uv = [[u, v], [u, v + 7.99], [u + w, v + 7.99], [u + w, v]]
                .map(|[a, b]| [a * scale, b * scale]);
            // Text faces the sign front; draw both windings so either side is visible.
            for index in [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] {
                out.push(Vertex {
                    position: corners[index],
                    uv: uv[index],
                    layer: Texture::Font as u32,
                    color: colour,
                    hurt: 0.0,
                });
            }
        }
        x += advance;
    }
}

/// `TileEntitySignRenderer`: standing signs rotate by metadata sixteenths;
/// wall signs face their metadata direction and hide the post.
pub fn sign(
    out: &mut Vec<Vertex>,
    at: Vec3,
    standing: bool,
    meta: u8,
    lines: &[String; 4],
    widths: &[u8; 256],
    light: f32,
) {
    let scale = 0.666_666_7f32;
    let mut matrix = Mat4::from_translation(at + Vec3::new(0.5, 0.75 * scale, 0.5));
    if standing {
        matrix *= Mat4::from_rotation_y((-(f32::from(meta) * 360.0) / 16.0).to_radians());
    } else {
        let rotation: f32 = match meta {
            2 => 180.0,
            4 => 90.0,
            5 => -90.0,
            _ => 0.0,
        };
        matrix *= Mat4::from_rotation_y((-rotation).to_radians())
            * Mat4::from_translation(Vec3::new(0.0, -0.3125, -0.4375));
    }
    let model = matrix * Mat4::from_scale(Vec3::new(scale, -scale, -scale));
    let tint = [light; 3];
    add(
        out,
        part(model, [0.0; 3], [0.0; 3]),
        cuboid(0.0, 0.0, [-12.0, -14.0, -1.0], [24, 12, 2]),
        Texture::Sign,
        tint,
    );
    if standing {
        add(
            out,
            part(model, [0.0; 3], [0.0; 3]),
            cuboid(0.0, 14.0, [-1.0, -2.0, -1.0], [2, 14, 2]),
            Texture::Sign,
            tint,
        );
    }
    let text_scale = 0.015_625 * scale;
    let text = matrix
        * Mat4::from_translation(Vec3::new(0.0, 0.5 * scale, 0.07 * scale))
        * Mat4::from_scale(Vec3::new(text_scale, -text_scale, text_scale));
    for (row, line) in lines.iter().enumerate() {
        text_line(out, text, line, row as f32 * 10.0 - 20.0, widths, [0.0; 3]);
    }
}

/// `TileEntitySkullRenderer` with `ModelSkeletonHead` (or the player head).
pub fn skull(out: &mut Vec<Vertex>, at: Vec3, facing: u8, kind: u8, rotation: u8, light: f32) {
    let (offset, yaw) = match facing {
        2 => (Vec3::new(0.5, 0.25, 0.74), 0.0),
        3 => (Vec3::new(0.5, 0.25, 0.26), 180.0),
        4 => (Vec3::new(0.74, 0.25, 0.5), 270.0),
        5 => (Vec3::new(0.26, 0.25, 0.5), 90.0),
        _ => (Vec3::new(0.5, 0.0, 0.5), f32::from(rotation) * 360.0 / 16.0),
    };
    let texture = match kind {
        1 => Texture::WitherSkeleton,
        2 => Texture::Zombie,
        3 => Texture::Steve,
        4 => Texture::Creeper,
        _ => Texture::Skeleton,
    };
    let matrix = Mat4::from_translation(at + offset) * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0));
    let head = part(matrix, [0.0; 3], [0.0, yaw.to_radians(), 0.0]);
    add(
        out,
        head,
        cuboid(0.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8]),
        texture,
        [light; 3],
    );
    if kind == 3 {
        add(
            out,
            head,
            cuboid(32.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8]).inflated(0.25),
            texture,
            [light; 3],
        );
    }
}

/// `EnumDyeColor` map colours by dye damage (Base: 0 black … 15 white).
fn dye(base: u8) -> [f32; 3] {
    const COLOURS: [u32; 16] = [
        0x191919, 0x993333, 0x667F33, 0x664C33, 0x334CB2, 0x7F3FB2, 0x4C7F99, 0x999999, 0x4C4C4C,
        0xF27FA5, 0x7FCC19, 0xE5E533, 0x6699D8, 0xB24CD8, 0xD87F33, 0xFFFFFF,
    ];
    let c = COLOURS[usize::from(base & 15)];
    [
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

/// `TileEntityBannerRenderer` with `ModelBanner`; the cloth takes the base dye.
pub fn banner(out: &mut Vec<Vertex>, at: Vec3, standing: bool, meta: u8, base: u8, light: f32) {
    let scale = 0.666_666_7f32;
    let mut matrix = Mat4::from_translation(at + Vec3::new(0.5, 0.75 * scale, 0.5));
    if standing {
        matrix *= Mat4::from_rotation_y((-(f32::from(meta) * 360.0) / 16.0).to_radians());
    } else {
        let rotation: f32 = match meta {
            2 => 180.0,
            4 => 90.0,
            5 => -90.0,
            _ => 0.0,
        };
        matrix *= Mat4::from_rotation_y((-rotation).to_radians())
            * Mat4::from_translation(Vec3::new(0.0, -0.3125, -0.4375));
    }
    let model = matrix * Mat4::from_scale(Vec3::new(scale, -scale, -scale));
    let wood = [light; 3];
    let cloth = dye(base).map(|channel| channel * light);
    add(
        out,
        part(model, [0.0, -32.0, 0.0], [-0.0125 * PI, 0.0, 0.0]),
        cuboid(0.0, 0.0, [-10.0, 0.0, -2.0], [20, 40, 1]),
        Texture::Banner,
        cloth,
    );
    if standing {
        add(
            out,
            part(model, [0.0; 3], [0.0; 3]),
            cuboid(44.0, 0.0, [-1.0, -30.0, -1.0], [2, 42, 2]),
            Texture::Banner,
            wood,
        );
    }
    add(
        out,
        part(model, [0.0; 3], [0.0; 3]),
        cuboid(0.0, 42.0, [-10.0, -32.0, -1.0], [20, 2, 2]),
        Texture::Banner,
        wood,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches_the_seed_187_sequence() {
        let mut random = JavaRandom::new(187);
        assert_eq!(random.next_float().to_bits(), 1_060_997_411);
        assert!((random.next_float() - 0.583_058_1).abs() < 1e-6);
        assert_eq!(random.next_float().to_bits(), 1_054_896_808);
    }

    #[test]
    fn skin_and_cape_bindings_follow_player_parts_metadata() {
        use horizon_protocol_47::{entities::EntityPacket, nbt::Meta};
        use horizon_world_189::entities::Entities;
        let mut tracked = Entities::default();
        tracked.apply(EntityPacket::SpawnPlayer {
            id: 1,
            uuid: 7,
            position: [0; 3],
            yaw: 0,
            pitch: 0,
            held_item: 0,
            metadata: vec![],
        });
        let bindings = std::collections::HashMap::from([(
            7,
            crate::skins::Binding {
                skin: 1000,
                cape: Some(1001),
                slim: true,
            },
        )]);
        let mut out = Vec::new();
        entities(
            &mut out,
            &[(1, tracked.get(1).unwrap().clone())],
            0.5,
            [0; 3],
            None,
            Some(&bindings),
            Default::default(),
        );
        assert!(out.iter().any(|v| v.layer == 1000));
        assert!(out.iter().any(|v| v.layer == 1001));
        tracked.apply(EntityPacket::Metadata {
            id: 1,
            entries: vec![(10, Meta::Byte(0x7e))],
        });
        out.clear();
        entities(
            &mut out,
            &[(1, tracked.get(1).unwrap().clone())],
            0.5,
            [0; 3],
            None,
            Some(&bindings),
            Default::default(),
        );
        assert!(out.iter().any(|v| v.layer == 1000));
        assert!(out.iter().all(|v| v.layer != 1001));
    }

    #[test]
    fn common_mob_meshes_use_owned_layers_and_shearing_removes_fleece() {
        use horizon_protocol_47::{entities::EntityPacket, nbt::Meta};
        use horizon_world_189::entities::Entities;
        for (kind, texture) in [
            (52, Texture::Spider),
            (59, Texture::CaveSpider),
            (55, Texture::Slime),
            (58, Texture::Enderman),
            (91, Texture::Sheep),
            (93, Texture::Chicken),
            (95, Texture::Wolf),
        ] {
            let mut tracked = Entities::default();
            tracked.apply(EntityPacket::SpawnMob {
                id: 1,
                kind,
                position: [0; 3],
                yaw: 0,
                pitch: 0,
                head_yaw: 0,
                metadata: vec![],
            });
            let mut out = Vec::new();
            entities(
                &mut out,
                &[(1, tracked.get(1).unwrap().clone())],
                0.5,
                [0; 3],
                None,
                None,
                Default::default(),
            );
            assert!(out.iter().any(|v| v.layer == texture as u32));
            assert!(out.iter().flat_map(|v| v.position).all(f32::is_finite));
            if kind == 91 {
                assert!(out.iter().any(|v| v.layer == Texture::Wool as u32));
                tracked.apply(EntityPacket::Metadata {
                    id: 1,
                    entries: vec![(16, Meta::Byte(16))],
                });
                out.clear();
                entities(
                    &mut out,
                    &[(1, tracked.get(1).unwrap().clone())],
                    0.5,
                    [0; 3],
                    None,
                    None,
                    Default::default(),
                );
                assert!(out.iter().all(|v| v.layer != Texture::Wool as u32));
            }
        }
    }

    #[test]
    fn equipment_meshes_follow_slots_and_hidden_players_emit_nothing() {
        use horizon_protocol_47::{
            entities::EntityPacket,
            nbt::{Item, Meta},
        };
        use horizon_world_189::entities::Entities;
        let mut tracked = Entities::default();
        tracked.apply(EntityPacket::SpawnPlayer {
            id: 1,
            uuid: 0,
            position: [0; 3],
            yaw: 0,
            pitch: 0,
            held_item: 0,
            metadata: vec![],
        });
        for (slot, id) in [(0, 276), (1, 313), (2, 312), (3, 311), (4, 310)] {
            tracked.apply(EntityPacket::Equipment {
                id: 1,
                slot,
                item: Some(Item {
                    id,
                    count: 1,
                    damage: 0,
                    nbt: None,
                }),
            });
        }
        let entities = vec![(1, tracked.get(1).unwrap().clone())];
        let mut vertices = Vec::new();
        super::entities(
            &mut vertices,
            &entities,
            0.5,
            [0; 3],
            None,
            None,
            Default::default(),
        );
        for layer in [Texture::Diamond1, Texture::Diamond2, Texture::DiamondSword] {
            assert!(vertices.iter().any(|v| v.layer == layer as u32));
        }
        assert!(vertices.iter().flat_map(|v| v.position).all(f32::is_finite));
        tracked.apply(EntityPacket::Metadata {
            id: 1,
            entries: vec![(0, Meta::Byte(0x20))],
        });
        vertices.clear();
        super::entities(
            &mut vertices,
            &[(1, tracked.get(1).unwrap().clone())],
            0.5,
            [0; 3],
            None,
            None,
            Default::default(),
        );
        assert!(vertices.is_empty());
    }

    #[test]
    fn boxes_emit_six_outward_faces_with_layer_scaled_uvs() {
        let mut out = Vec::new();
        add(
            &mut out,
            Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0)),
            cuboid(0.0, 0.0, [0.0; 3], [8, 8, 8]),
            Texture::Steve,
            [1.0; 3],
        );
        assert_eq!(out.len(), 36);
        // Every triangle winds counter-clockwise from outside despite the
        // mirroring scale.
        for triangle in out.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(triangle[i].position));
            let normal = (b - a).cross(c - a);
            let centre = (a + b + c) / 3.0 - Vec3::new(-4.0, -4.0, 4.0);
            assert!(normal.dot(centre) > 0.0);
        }
        assert!(
            out.iter()
                .all(|v| v.uv[0] <= 32.0 / 128.0 && v.uv[1] <= 16.0 / 128.0)
        );
        let mut font = vec![0u8; 128 * 128 * 4];
        // Glyph 'A' (65) cell at (8, 32): opaque columns 0..=4.
        for x in 0..5 {
            font[((32 + 3) * 128 + 8 + x) * 4 + 3] = 255;
        }
        let widths = glyph_widths(&font, 128);
        assert_eq!((widths[65], widths[32]), (6, 4));
    }
}
