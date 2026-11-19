//! Additional standard adult mob boxes and their legacy limb transforms.
use super::*;

pub(super) fn render(out: &mut Vec<Vertex>, entity: &Entity, partial: f32, origin: [i64; 3]) {
    let yaw = lerp_angle(entity.previous_body_yaw, entity.body_yaw, partial);
    let head = lerp_angle(entity.previous_head_yaw, entity.head_yaw, partial) - yaw;
    let mut base = Mat4::from_translation(position(entity, partial, origin))
        * Mat4::from_rotation_y((180.0 - yaw).to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0));
    if entity.kind == Kind::Mob(59) {
        base *= Mat4::from_scale(Vec3::splat(0.7));
    }
    if entity.kind == Kind::Mob(55) {
        base *= Mat4::from_scale(Vec3::splat(f32::from(entity.variant.max(1))));
    }
    base *= Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0));
    let limb = entity.limb_swing - entity.limb_amount * (1.0 - partial);
    let amount = lerp(entity.previous_limb_amount, entity.limb_amount, partial).min(1.0);
    let head_angles = [
        lerp(entity.previous_pitch, entity.pitch, partial).to_radians(),
        head.to_radians(),
        0.0,
    ];
    match entity.kind {
        Kind::Mob(91) => sheep(out, entity, base, head_angles, limb, amount),
        Kind::Mob(93) => chicken(out, entity, base, head_angles, limb, amount),
        Kind::Mob(52 | 59) => spider(out, entity, base, head_angles, limb, amount),
        Kind::Mob(55) => slime(out, base),
        Kind::Mob(58) => enderman(out, base, head_angles, limb, amount),
        Kind::Mob(95) => wolf(out, entity, base, head_angles, limb, amount),
        _ => {}
    }
}

fn child_body(base: Mat4, child: bool) -> Mat4 {
    if child {
        base * Mat4::from_scale(Vec3::splat(0.5)) * Mat4::from_translation(Vec3::Y * 1.5)
    } else {
        base
    }
}

fn legs(
    out: &mut Vec<Vertex>,
    base: Mat4,
    points: [[f32; 3]; 4],
    cube: Cuboid,
    texture: Texture,
    limb: f32,
    amount: f32,
) {
    for (point, phase) in points.into_iter().zip([0.0, PI, PI, 0.0]) {
        add(
            out,
            part(
                base,
                point,
                [(limb * 0.6662 + phase).cos() * 1.4 * amount, 0.0, 0.0],
            ),
            cube,
            texture,
            [1.0; 3],
        );
    }
}

fn sheep(
    out: &mut Vec<Vertex>,
    entity: &Entity,
    base: Mat4,
    angles: [f32; 3],
    limb: f32,
    amount: f32,
) {
    let head_base = if entity.child {
        base * Mat4::from_translation(Vec3::new(0.0, 0.5, 0.25))
    } else {
        base
    };
    let head = part(head_base, [0.0, 6.0, -8.0], angles);
    let base = child_body(base, entity.child);
    let body = part(base, [0.0, 5.0, 2.0], [PI / 2.0, 0.0, 0.0]);
    add(
        out,
        head,
        cuboid(0.0, 0.0, [-3.0, -4.0, -6.0], [6, 6, 8]),
        Texture::Sheep,
        [1.0; 3],
    );
    add(
        out,
        body,
        cuboid(28.0, 8.0, [-4.0, -10.0, -7.0], [8, 16, 6]),
        Texture::Sheep,
        [1.0; 3],
    );
    let points = [
        [-3.0, 12.0, 7.0],
        [3.0, 12.0, 7.0],
        [-3.0, 12.0, -5.0],
        [3.0, 12.0, -5.0],
    ];
    legs(
        out,
        base,
        points,
        cuboid(0.0, 16.0, [-2.0, 0.0, -2.0], [4, 12, 4]),
        Texture::Sheep,
        limb,
        amount,
    );
    if entity.variant & 16 != 0 {
        return;
    }
    // Fleece uses sheep's render palette, indexed by dye metadata.
    const COLOURS: [[f32; 3]; 16] = [
        [1.0; 3],
        [0.85, 0.5, 0.2],
        [0.7, 0.3, 0.85],
        [0.4, 0.6, 0.85],
        [0.9, 0.9, 0.2],
        [0.5, 0.8, 0.1],
        [0.95, 0.5, 0.65],
        [0.3; 3],
        [0.6; 3],
        [0.3, 0.5, 0.6],
        [0.5, 0.25, 0.7],
        [0.2, 0.3, 0.7],
        [0.4, 0.3, 0.2],
        [0.4, 0.5, 0.2],
        [0.6, 0.2, 0.2],
        [0.1; 3],
    ];
    let tint = COLOURS[usize::from(entity.variant & 15)];
    add(
        out,
        head,
        cuboid(0.0, 0.0, [-3.0, -4.0, -4.0], [6, 6, 6]).inflated(0.6),
        Texture::Wool,
        tint,
    );
    add(
        out,
        body,
        cuboid(28.0, 8.0, [-4.0, -10.0, -7.0], [8, 16, 6]).inflated(1.75),
        Texture::Wool,
        tint,
    );
    for (point, phase) in points.into_iter().zip([0.0, PI, PI, 0.0]) {
        add(
            out,
            part(
                base,
                point,
                [(limb * 0.6662 + phase).cos() * 1.4 * amount, 0.0, 0.0],
            ),
            cuboid(0.0, 16.0, [-2.0, 0.0, -2.0], [4, 6, 4]).inflated(0.5),
            Texture::Wool,
            tint,
        );
    }
}

fn chicken(
    out: &mut Vec<Vertex>,
    entity: &Entity,
    base: Mat4,
    angles: [f32; 3],
    limb: f32,
    amount: f32,
) {
    let head_base = if entity.child {
        base * Mat4::from_translation(Vec3::new(0.0, 0.3125, 0.125))
    } else {
        base
    };
    let head = part(head_base, [0.0, 15.0, -4.0], angles);
    let base = child_body(base, entity.child);
    for cube in [
        cuboid(0.0, 0.0, [-2.0, -6.0, -2.0], [4, 6, 3]),
        cuboid(14.0, 0.0, [-2.0, -4.0, -4.0], [4, 2, 2]),
        cuboid(14.0, 4.0, [-1.0, -2.0, -3.0], [2, 2, 2]),
    ] {
        add(out, head, cube, Texture::Chicken, [1.0; 3]);
    }
    add(
        out,
        part(base, [0.0, 16.0, 0.0], [PI / 2.0, 0.0, 0.0]),
        cuboid(0.0, 9.0, [-3.0, -4.0, -3.0], [6, 8, 6]),
        Texture::Chicken,
        [1.0; 3],
    );
    for (x, phase) in [(-2.0, 0.0), (1.0, PI)] {
        add(
            out,
            part(
                base,
                [x, 19.0, 1.0],
                [(limb * 0.6662 + phase).cos() * 1.4 * amount, 0.0, 0.0],
            ),
            cuboid(26.0, 0.0, [-1.0, 0.0, -3.0], [3, 5, 3]),
            Texture::Chicken,
            [1.0; 3],
        );
    }
    // Server ground flags keep resting wings folded; flight uses a flap pose.
    let flap = if entity.on_ground {
        0.0
    } else {
        (entity.age as f32 * 2.0).sin().abs() * 1.5
    };
    for (side, x) in [(-1.0, 0.0), (1.0, -1.0)] {
        add(
            out,
            part(base, [side * 4.0, 13.0, 0.0], [0.0, 0.0, -side * flap]),
            cuboid(24.0, 13.0, [x, 0.0, -3.0], [1, 4, 6]),
            Texture::Chicken,
            [1.0; 3],
        );
    }
}

fn spider(
    out: &mut Vec<Vertex>,
    entity: &Entity,
    base: Mat4,
    angles: [f32; 3],
    limb: f32,
    amount: f32,
) {
    let texture = if entity.kind == Kind::Mob(59) {
        Texture::CaveSpider
    } else {
        Texture::Spider
    };
    add(
        out,
        part(base, [0.0, 15.0, -3.0], angles),
        cuboid(32.0, 4.0, [-4.0, -4.0, -8.0], [8, 8, 8]),
        texture,
        [1.0; 3],
    );
    add(
        out,
        part(base, [0.0, 15.0, 0.0], [0.0; 3]),
        cuboid(0.0, 0.0, [-3.0, -3.0, -3.0], [6, 6, 6]),
        texture,
        [1.0; 3],
    );
    add(
        out,
        part(base, [0.0, 15.0, 9.0], [0.0; 3]),
        cuboid(0.0, 12.0, [-5.0, -4.0, -6.0], [10, 8, 12]),
        texture,
        [1.0; 3],
    );
    for (z, yaw, spread, phase) in [
        (2.0, PI / 4.0, PI / 4.0, 0.0),
        (1.0, PI / 8.0, PI / 4.0 * 0.74, PI),
        (0.0, -PI / 8.0, PI / 4.0 * 0.74, PI / 2.0),
        (-1.0, -PI / 4.0, PI / 4.0, PI * 1.5),
    ] {
        let turn = -(limb * 0.6662 * 2.0 + phase).cos() * 0.4 * amount;
        let lift = (limb * 0.6662 + phase).sin().abs() * 0.4 * amount;
        for side in [-1.0, 1.0] {
            add(
                out,
                part(
                    base,
                    [side * 4.0, 15.0, z],
                    [0.0, -side * (yaw + turn), side * (spread - lift)],
                ),
                cuboid(
                    18.0,
                    0.0,
                    [if side < 0.0 { -15.0 } else { -1.0 }, -1.0, -1.0],
                    [16, 2, 2],
                ),
                texture,
                [1.0; 3],
            );
        }
    }
}

fn slime(out: &mut Vec<Vertex>, base: Mat4) {
    let matrix = part(base, [0.0; 3], [0.0; 3]);
    for cube in [
        cuboid(0.0, 16.0, [-3.0, 17.0, -3.0], [6, 6, 6]),
        cuboid(32.0, 0.0, [-3.25, 18.0, -3.5], [2, 2, 2]),
        cuboid(32.0, 4.0, [1.25, 18.0, -3.5], [2, 2, 2]),
        cuboid(32.0, 8.0, [0.0, 21.0, -3.5], [1, 1, 1]),
        cuboid(0.0, 0.0, [-4.0, 16.0, -4.0], [8, 8, 8]),
    ] {
        add(out, matrix, cube, Texture::Slime, [1.0; 3]);
    }
}

fn enderman(out: &mut Vec<Vertex>, base: Mat4, angles: [f32; 3], limb: f32, amount: f32) {
    add(
        out,
        part(base, [0.0, -14.0, 0.0], angles),
        cuboid(0.0, 0.0, [-4.0, -8.0, -4.0], [8, 8, 8]),
        Texture::Enderman,
        [1.0; 3],
    );
    add(
        out,
        part(base, [0.0, -14.0, 0.0], [0.0; 3]),
        cuboid(32.0, 16.0, [-4.0, 0.0, -2.0], [8, 12, 4]),
        Texture::Enderman,
        [1.0; 3],
    );
    for (x, y, phase) in [
        (-3.0, -12.0, PI),
        (5.0, -12.0, 0.0),
        (-2.0, -2.0, 0.0),
        (2.0, -2.0, PI),
    ] {
        let angle = ((limb * 0.6662 + phase).cos() * 0.7 * amount).clamp(-0.4, 0.4);
        add(
            out,
            part(base, [x, y, 0.0], [angle, 0.0, 0.0]),
            cuboid(
                56.0,
                0.0,
                [-1.0, if y == -12.0 { -2.0 } else { 0.0 }, -1.0],
                [2, 30, 2],
            ),
            Texture::Enderman,
            [1.0; 3],
        );
    }
}

fn wolf(
    out: &mut Vec<Vertex>,
    entity: &Entity,
    base: Mat4,
    angles: [f32; 3],
    limb: f32,
    amount: f32,
) {
    let texture = if entity.variant & 2 != 0 {
        Texture::WolfAngry
    } else if entity.variant & 4 != 0 {
        Texture::WolfTame
    } else {
        Texture::Wolf
    };
    let head_base = if entity.child {
        base * Mat4::from_translation(Vec3::new(0.0, 0.3125, 0.125))
    } else {
        base
    };
    let head = part(head_base, [-1.0, 13.5, -7.0], angles);
    let base = child_body(base, entity.child);
    for cube in [
        cuboid(0.0, 0.0, [-3.0, -3.0, -2.0], [6, 6, 4]),
        cuboid(16.0, 14.0, [-3.0, -5.0, 0.0], [2, 2, 1]),
        cuboid(16.0, 14.0, [1.0, -5.0, 0.0], [2, 2, 1]),
        cuboid(0.0, 10.0, [-1.5, 0.0, -5.0], [3, 3, 4]),
    ] {
        add(out, head, cube, texture, [1.0; 3]);
    }
    add(
        out,
        part(base, [0.0, 14.0, 2.0], [PI / 2.0, 0.0, 0.0]),
        cuboid(18.0, 14.0, [-4.0, -2.0, -3.0], [6, 9, 6]),
        texture,
        [1.0; 3],
    );
    add(
        out,
        part(base, [-1.0, 14.0, -3.0], [PI / 2.0, 0.0, 0.0]),
        cuboid(21.0, 0.0, [-4.0, -3.0, -3.0], [8, 6, 7]),
        texture,
        [1.0; 3],
    );
    legs(
        out,
        base,
        [
            [-2.5, 16.0, 7.0],
            [0.5, 16.0, 7.0],
            [-2.5, 16.0, -4.0],
            [0.5, 16.0, -4.0],
        ],
        cuboid(0.0, 18.0, [-1.0, 0.0, -1.0], [2, 8, 2]),
        texture,
        limb,
        amount,
    );
    let tail = if entity.variant & 2 != 0 {
        PI / 5.0
    } else {
        PI * 0.63
    };
    add(
        out,
        part(
            base,
            [-1.0, 12.0, 8.0],
            [tail, (limb * 0.6662).cos() * 1.4 * amount, 0.0],
        ),
        cuboid(9.0, 18.0, [-1.0, 0.0, -1.0], [2, 8, 2]),
        texture,
        [1.0; 3],
    );
}
