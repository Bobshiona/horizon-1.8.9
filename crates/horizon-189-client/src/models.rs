//! Entity and block-entity geometry from vanilla 1.8.9 model definitions.
//! `ModelBox` keeps its six-quad UV layout and each renderer its matrix chain;
//! quads are re-wound outward after transformation so back-face culling holds
//! under the renderers' mirroring scales. Textures live in 128-pixel layers of
//! a separate array, placed at the top-left, so UVs are pixel / 128.
use crate::terrain::Vertex;
use glam::{Mat4, Vec3};
use horizon_world_189::entities::{Entity, Kind};
use std::f32::consts::PI;

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
}

/// Owned-jar paths for every layer after the generated white one.
pub const SOURCES: [&str; 14] = [
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
];

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
fn add(out: &mut Vec<Vertex>, matrix: Mat4, cube: Cuboid, layer: Texture, tint: [f32; 3]) {
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
                layer: layer as u32,
                color: tint.map(|channel| channel * light),
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
fn player(out: &mut Vec<Vertex>, entity: &Entity, slim: bool, partial: f32, origin: [i64; 3]) {
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
    let mut head = [entity.pitch / degrees, head_yaw / degrees, 0.0];
    let mut body = [0.0f32; 3];
    let mut right_arm = [(swing * 0.6662 + PI).cos() * 2.0 * amount * 0.5, 0.0, 0.0];
    let mut left_arm = [(swing * 0.6662).cos() * 2.0 * amount * 0.5, 0.0, 0.0];
    let right_leg = [(swing * 0.6662).cos() * 1.4 * amount, 0.0, 0.0];
    let left_leg = [(swing * 0.6662 + PI).cos() * 1.4 * amount, 0.0, 0.0];
    if entity.equipment[0].is_some() {
        right_arm[0] = right_arm[0] * 0.5 - PI / 10.0;
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
    let skin = if slim { Texture::Alex } else { Texture::Steve };
    let white = [1.0; 3];
    let head_matrix = part(base, [0.0, head_y, 0.0], head);
    add(
        out,
        head_matrix,
        cuboid(0.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8]),
        skin,
        white,
    );
    add(
        out,
        head_matrix,
        cuboid(32.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8]).inflated(0.5),
        skin,
        white,
    );
    let body_matrix = part(base, [0.0; 3], body);
    add(
        out,
        body_matrix,
        cuboid(16.0, 16.0, [-4.0, 0.0, -2.0], [8, 12, 4]),
        skin,
        white,
    );
    add(
        out,
        body_matrix,
        cuboid(16.0, 32.0, [-4.0, 0.0, -2.0], [8, 12, 4]).inflated(0.25),
        skin,
        white,
    );
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
    add(
        out,
        right,
        cuboid(40.0, 32.0, [right_x, -2.0, -2.0], arm).inflated(0.25),
        skin,
        white,
    );
    add(
        out,
        left,
        cuboid(32.0, 48.0, [-1.0, -2.0, -2.0], arm),
        skin,
        white,
    );
    add(
        out,
        left,
        cuboid(48.0, 48.0, [-1.0, -2.0, -2.0], arm).inflated(0.25),
        skin,
        white,
    );
    let right = part(base, [-1.9, leg_y, leg_z], right_leg);
    let left = part(base, [1.9, leg_y, leg_z], left_leg);
    add(
        out,
        right,
        cuboid(0.0, 16.0, [-2.0, 0.0, -2.0], [4, 12, 4]),
        skin,
        white,
    );
    add(
        out,
        right,
        cuboid(0.0, 32.0, [-2.0, 0.0, -2.0], [4, 12, 4]).inflated(0.25),
        skin,
        white,
    );
    add(
        out,
        left,
        cuboid(16.0, 48.0, [-2.0, 0.0, -2.0], [4, 12, 4]),
        skin,
        white,
    );
    add(
        out,
        left,
        cuboid(0.0, 48.0, [-2.0, 0.0, -2.0], [4, 12, 4]).inflated(0.25),
        skin,
        white,
    );
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
pub fn entities(out: &mut Vec<Vertex>, entities: &[(i32, Entity)], partial: f32, origin: [i64; 3]) {
    for (_, entity) in entities {
        if entity.invisible() {
            continue;
        }
        match entity.kind {
            Kind::Player { slim } => player(out, entity, slim, partial, origin),
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
    let total: f32 = text
        .chars()
        .map(|c| f32::from(widths[(c as u32).min(255) as usize]))
        .sum();
    let mut x = -total / 2.0;
    let scale = 1.0 / LAYER_SIZE as f32;
    for c in text.chars() {
        let code = (c as u32).min(255) as usize;
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
