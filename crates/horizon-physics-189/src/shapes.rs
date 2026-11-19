//! 1.8.9 collision boxes: each block's `getCollisionBoundingBox` or
//! `addCollisionBoxesToList`, including the float arithmetic some blocks do
//! with their integer position, and `World.getCollidingBoundingBoxes`.
use crate::{Aabb, Vec3};
use horizon_world_189::{
    BlockState, World,
    blocks::{
        fence_attaches, is_door, is_fence, is_fence_gate, is_stairs, pane_attaches, wall_attaches,
    },
    variants::{Neighbours, door_state, gate_facing, stairs_facing, stairs_shape, stairs_top},
};

/// `new AxisAlignedBB(...)`; the constructor orders each axis.
pub fn bb(min: [f64; 3], max: [f64; 3]) -> Aabb {
    Aabb::new(
        Vec3::new(min[0], min[1], min[2]),
        Vec3::new(max[0], max[1], max[2]),
    )
}

pub fn lo(b: Aabb) -> [f64; 3] {
    b.min_position().components()
}

pub fn hi(b: Aabb) -> [f64; 3] {
    b.max_position().components()
}

pub fn offset(b: Aabb, x: f64, y: f64, z: f64) -> Aabb {
    let (l, h) = (lo(b), hi(b));
    bb(
        [l[0] + x, l[1] + y, l[2] + z],
        [h[0] + x, h[1] + y, h[2] + z],
    )
}

/// `addCoord`: extend only the side the motion points to.
pub fn add_coord(b: Aabb, x: f64, y: f64, z: f64) -> Aabb {
    let (mut l, mut h) = (lo(b), hi(b));
    for (axis, value) in [x, y, z].into_iter().enumerate() {
        if value < 0.0 {
            l[axis] += value;
        } else if value > 0.0 {
            h[axis] += value;
        }
    }
    bb(l, h)
}

pub fn expand(b: Aabb, x: f64, y: f64, z: f64) -> Aabb {
    let (l, h) = (lo(b), hi(b));
    bb(
        [l[0] - x, l[1] - y, l[2] - z],
        [h[0] + x, h[1] + y, h[2] + z],
    )
}

pub fn contract(b: Aabb, x: f64, y: f64, z: f64) -> Aabb {
    let (l, h) = (lo(b), hi(b));
    bb(
        [l[0] + x, l[1] + y, l[2] + z],
        [h[0] - x, h[1] - y, h[2] - z],
    )
}

/// `intersectsWith`: strict on every axis.
pub fn intersects(a: Aabb, b: Aabb) -> bool {
    let (al, ah, bl, bh) = (lo(a), hi(a), lo(b), hi(b));
    (0..3).all(|axis| bh[axis] > al[axis] && bl[axis] < ah[axis])
}

/// `MathHelper.floor_double`.
pub fn floor(value: f64) -> i32 {
    let truncated = value as i32;
    if value < f64::from(truncated) {
        truncated - 1
    } else {
        truncated
    }
}

struct Cell {
    x: f64,
    y: f64,
    z: f64,
}

impl Cell {
    /// Block bounds stored from floats, offset by the position in double.
    fn bounds(&self, min: [f32; 3], max: [f32; 3]) -> Aabb {
        bb(
            [
                self.x + f64::from(min[0]),
                self.y + f64::from(min[1]),
                self.z + f64::from(min[2]),
            ],
            [
                self.x + f64::from(max[0]),
                self.y + f64::from(max[1]),
                self.z + f64::from(max[2]),
            ],
        )
    }
}

fn horizontal_bounds(facing: &str, thickness: f32) -> ([f32; 3], [f32; 3]) {
    let t = thickness;
    match facing {
        "north" => ([0.0, 0.0, 1.0 - t], [1.0, 1.0, 1.0]),
        "south" => ([0.0, 0.0, 0.0], [1.0, 1.0, t]),
        "west" => ([1.0 - t, 0.0, 0.0], [1.0, 1.0, 1.0]),
        _ => ([0.0, 0.0, 0.0], [t, 1.0, 1.0]),
    }
}

/// Identifiers whose collision box is null in 1.8.9.
pub fn passable(id: u16) -> bool {
    matches!(
        id,
        0 | 6
            | 8..=11
            | 27
            | 28
            | 30..=32
            | 36..=40
            | 50
            | 51
            | 55
            | 59
            | 63
            | 66
            | 68
            | 69
            | 70
            | 72
            | 75
            | 76
            | 77
            | 83
            | 90
            | 104..=106
            | 115
            | 119
            | 131
            | 132
            | 141..=143
            | 147
            | 148
            | 157
            | 175..=177
    )
}

/// Append the block's collision boxes that intersect `mask`, in vanilla order.
pub fn collision_boxes(
    world: &World,
    pos: [i32; 3],
    state: BlockState,
    mask: Aabb,
    out: &mut Vec<Aabb>,
) {
    let id = state.block_id();
    if passable(id) {
        return;
    }
    let meta = state.metadata();
    let cell = Cell {
        x: f64::from(pos[0]),
        y: f64::from(pos[1]),
        z: f64::from(pos[2]),
    };
    let mut add = |b: Aabb| {
        if intersects(mask, b) {
            out.push(b);
        }
    };
    let near = Neighbours::new(world, pos);
    let (fx, fy, fz) = (pos[0] as f32, pos[1] as f32, pos[2] as f32);
    match id {
        44 | 126 | 182 => {
            if meta & 8 != 0 {
                add(cell.bounds([0.0, 0.5, 0.0], [1.0, 1.0, 1.0]));
            } else {
                add(cell.bounds([0.0; 3], [1.0, 0.5, 1.0]));
            }
        }
        _ if is_stairs(id) => {
            let top = stairs_top(state);
            let (base, step) = if top {
                ((0.5, 1.0), (0.0, 0.5))
            } else {
                ((0.0, 0.5), (0.5, 1.0))
            };
            add(cell.bounds([0.0, base.0, 0.0], [1.0, base.1, 1.0]));
            let facing = stairs_facing(state);
            let shape = stairs_shape(&near, state);
            let (y0, y1) = step;
            if matches!(shape, "straight" | "inner_left" | "inner_right") {
                let (min, max) = match facing {
                    "north" => ([0.0, y0, 0.0], [1.0, y1, 0.5]),
                    "south" => ([0.0, y0, 0.5], [1.0, y1, 1.0]),
                    "west" => ([0.0, y0, 0.0], [0.5, y1, 1.0]),
                    _ => ([0.5, y0, 0.0], [1.0, y1, 1.0]),
                };
                add(cell.bounds(min, max));
            }
            if shape != "straight" {
                let corner = match (shape, facing) {
                    ("outer_left", f) => f,
                    ("outer_right", "north") | ("inner_left", "south") => "east",
                    ("outer_right", "east") | ("inner_left", "west") => "south",
                    ("outer_right", "south") | ("inner_left", "north") => "west",
                    ("outer_right", _) | ("inner_left", _) => "north",
                    (_, "north") => "south",
                    (_, "south") => "north",
                    (_, "west") => "east",
                    _ => "west",
                };
                let (min, max) = match corner {
                    "north" => ([0.0, y0, 0.0], [0.5, y1, 0.5]),
                    "east" => ([0.5, y0, 0.0], [1.0, y1, 0.5]),
                    "south" => ([0.5, y0, 0.5], [1.0, y1, 1.0]),
                    _ => ([0.0, y0, 0.5], [0.5, y1, 1.0]),
                };
                add(cell.bounds(min, max));
            }
        }
        _ if is_fence(id) => {
            let attached = |side: &str| fence_attaches(id, near_block(&near, side));
            let (n, s, w, e) = (
                attached("north"),
                attached("south"),
                attached("west"),
                attached("east"),
            );
            let (mut x0, mut x1, mut z0, mut z1) = (0.375f32, 0.625f32, 0.375f32, 0.625f32);
            if n {
                z0 = 0.0;
            }
            if s {
                z1 = 1.0;
            }
            if n || s {
                add(cell.bounds([x0, 0.0, z0], [x1, 1.5, z1]));
            }
            z0 = 0.375;
            z1 = 0.625;
            if w {
                x0 = 0.0;
            }
            if e {
                x1 = 1.0;
            }
            if w || e || (!n && !s) {
                add(cell.bounds([x0, 0.0, z0], [x1, 1.5, z1]));
            }
        }
        _ if is_fence_gate(id) => {
            if meta & 4 == 0 {
                let facing = gate_facing(state);
                if matches!(facing, "north" | "south") {
                    add(cell.bounds([0.0, 0.0, 0.375], [1.0, 1.5, 0.625]));
                } else {
                    add(cell.bounds([0.375, 0.0, 0.0], [0.625, 1.5, 1.0]));
                }
            }
        }
        139 => {
            let attached = |side: &str| wall_attaches(near_block(&near, side));
            let (n, s, w, e) = (
                attached("north"),
                attached("south"),
                attached("west"),
                attached("east"),
            );
            let (mut x0, mut x1, mut z0, mut z1) = (0.25f32, 0.75f32, 0.25f32, 0.75f32);
            if n {
                z0 = 0.0;
            }
            if s {
                z1 = 1.0;
            }
            if w {
                x0 = 0.0;
            }
            if e {
                x1 = 1.0;
            }
            if n && s && !w && !e {
                x0 = 0.3125;
                x1 = 0.6875;
            } else if !n && !s && w && e {
                z0 = 0.3125;
                z1 = 0.6875;
            }
            add(cell.bounds([x0, 0.0, z0], [x1, 1.5, z1]));
        }
        101 | 102 | 160 => {
            let attached = |side: &str| pane_attaches(id, near_block(&near, side));
            let (n, s, w, e) = (
                attached("north"),
                attached("south"),
                attached("west"),
                attached("east"),
            );
            let any = n || s || w || e;
            if (!w || !e) && any {
                if w {
                    add(cell.bounds([0.0, 0.0, 0.4375], [0.5, 1.0, 0.5625]));
                } else if e {
                    add(cell.bounds([0.5, 0.0, 0.4375], [1.0, 1.0, 0.5625]));
                }
            } else {
                add(cell.bounds([0.0, 0.0, 0.4375], [1.0, 1.0, 0.5625]));
            }
            if (!n || !s) && any {
                if n {
                    add(cell.bounds([0.4375, 0.0, 0.0], [0.5625, 1.0, 0.5]));
                } else if s {
                    add(cell.bounds([0.4375, 0.0, 0.5], [0.5625, 1.0, 1.0]));
                }
            } else {
                add(cell.bounds([0.4375, 0.0, 0.0], [0.5625, 1.0, 1.0]));
            }
        }
        _ if is_door(id) => {
            let (facing, open, right, _) = door_state(&near, state);
            let f = 0.1875f32;
            let (min, max) = if open {
                match (facing, right) {
                    ("east", false) => ([0.0, 0.0, 0.0], [1.0, 1.0, f]),
                    ("east", true) => ([0.0, 0.0, 1.0 - f], [1.0, 1.0, 1.0]),
                    ("south", false) => ([1.0 - f, 0.0, 0.0], [1.0, 1.0, 1.0]),
                    ("south", true) => ([0.0, 0.0, 0.0], [f, 1.0, 1.0]),
                    ("west", false) => ([0.0, 0.0, 1.0 - f], [1.0, 1.0, 1.0]),
                    ("west", true) => ([0.0, 0.0, 0.0], [1.0, 1.0, f]),
                    (_, false) => ([0.0, 0.0, 0.0], [f, 1.0, 1.0]),
                    (_, true) => ([1.0 - f, 0.0, 0.0], [1.0, 1.0, 1.0]),
                }
            } else {
                match facing {
                    "east" => ([0.0, 0.0, 0.0], [f, 1.0, 1.0]),
                    "south" => ([0.0, 0.0, 0.0], [1.0, 1.0, f]),
                    "west" => ([1.0 - f, 0.0, 0.0], [1.0, 1.0, 1.0]),
                    _ => ([0.0, 0.0, 1.0 - f], [1.0, 1.0, 1.0]),
                }
            };
            add(cell.bounds(min, max));
        }
        96 | 167 => {
            let f = 0.1875f32;
            let (min, max) = if meta & 4 != 0 {
                horizontal_bounds(["north", "south", "west", "east"][usize::from(meta & 3)], f)
            } else if meta & 8 != 0 {
                ([0.0, 1.0 - f, 0.0], [1.0, 1.0, 1.0])
            } else {
                ([0.0; 3], [1.0, f, 1.0])
            };
            add(cell.bounds(min, max));
        }
        65 => {
            let facing = match meta % 6 {
                3 => "south",
                4 => "west",
                5 => "east",
                _ => "north",
            };
            let (min, max) = horizontal_bounds(facing, 0.125);
            add(cell.bounds(min, max));
        }
        78 => {
            let layers = f32::from((meta & 7) + 1);
            let top = fy + (layers - 1.0) * 0.125;
            add(bb(
                [cell.x, cell.y, cell.z],
                [cell.x + 1.0, f64::from(top), cell.z + 1.0],
            ));
        }
        81 => {
            let f = 0.0625f32;
            add(bb(
                [f64::from(fx + f), cell.y, f64::from(fz + f)],
                [
                    f64::from((pos[0].wrapping_add(1)) as f32 - f),
                    f64::from((pos[1] + 1) as f32 - f),
                    f64::from((pos[2].wrapping_add(1)) as f32 - f),
                ],
            ));
        }
        88 => add(bb(
            [cell.x, cell.y, cell.z],
            [
                cell.x + 1.0,
                f64::from((pos[1] + 1) as f32 - 0.125),
                cell.z + 1.0,
            ],
        )),
        92 => {
            let f = 0.0625f32;
            let bite = (1.0 + f32::from(meta.min(6)) * 2.0) / 16.0;
            add(bb(
                [f64::from(fx + bite), cell.y, f64::from(fz + f)],
                [
                    f64::from((pos[0].wrapping_add(1)) as f32 - f),
                    f64::from(fy + 0.5 - f),
                    f64::from((pos[2].wrapping_add(1)) as f32 - f),
                ],
            ));
        }
        54 | 146 => {
            let neighbour = |side: &str| near_block(&near, side) == id;
            let (min, max) = if neighbour("north") {
                ([0.0625, 0.0, 0.0], [0.9375, 0.875, 0.9375])
            } else if neighbour("south") {
                ([0.0625, 0.0, 0.0625], [0.9375, 0.875, 1.0])
            } else if neighbour("west") {
                ([0.0, 0.0, 0.0625], [0.9375, 0.875, 0.9375])
            } else if neighbour("east") {
                ([0.0625, 0.0, 0.0625], [1.0, 0.875, 0.9375])
            } else {
                ([0.0625, 0.0, 0.0625], [0.9375, 0.875, 0.9375])
            };
            add(cell.bounds(min, max));
        }
        130 => add(cell.bounds([0.0625, 0.0, 0.0625], [0.9375, 0.875, 0.9375])),
        116 => add(cell.bounds([0.0; 3], [1.0, 0.75, 1.0])),
        120 => {
            add(cell.bounds([0.0; 3], [1.0, 0.8125, 1.0]));
            if meta & 4 != 0 {
                add(cell.bounds([0.3125, 0.8125, 0.3125], [0.6875, 1.0, 0.6875]));
            }
        }
        26 => add(cell.bounds([0.0; 3], [1.0, 0.5625, 1.0])),
        151 | 178 => add(cell.bounds([0.0; 3], [1.0, 0.375, 1.0])),
        111 => add(cell.bounds([0.0; 3], [1.0, 0.015625, 1.0])),
        171 => add(cell.bounds([0.0; 3], [1.0, 0.0625, 1.0])),
        93 | 94 | 149 | 150 => add(cell.bounds([0.0; 3], [1.0, 0.125, 1.0])),
        117 => {
            add(cell.bounds([0.4375, 0.0, 0.4375], [0.5625, 0.875, 0.5625]));
            add(cell.bounds([0.0; 3], [1.0, 0.125, 1.0]));
        }
        118 | 154 => {
            let floor = if id == 118 { 0.3125 } else { 0.625 };
            let f = 0.125f32;
            add(cell.bounds([0.0; 3], [1.0, floor, 1.0]));
            add(cell.bounds([0.0; 3], [f, 1.0, 1.0]));
            add(cell.bounds([0.0; 3], [1.0, 1.0, f]));
            add(cell.bounds([1.0 - f, 0.0, 0.0], [1.0, 1.0, 1.0]));
            add(cell.bounds([0.0, 0.0, 1.0 - f], [1.0, 1.0, 1.0]));
        }
        145 => {
            if meta & 1 != 0 {
                // getHorizontal(meta & 3): west and east lie on the X axis.
                add(cell.bounds([0.0, 0.0, 0.125], [1.0, 1.0, 0.875]));
            } else {
                add(cell.bounds([0.125, 0.0, 0.0], [0.875, 1.0, 1.0]));
            }
        }
        140 => add(cell.bounds([0.3125, 0.0, 0.3125], [0.6875, 0.375, 0.6875])),
        122 => add(cell.bounds([0.0625, 0.0, 0.0625], [0.9375, 1.0, 0.9375])),
        144 => {
            let (min, max) = match meta & 7 {
                2 => ([0.25, 0.25, 0.5], [0.75, 0.75, 1.0]),
                3 => ([0.25, 0.25, 0.0], [0.75, 0.75, 0.5]),
                4 => ([0.5, 0.25, 0.25], [1.0, 0.75, 0.75]),
                5 => ([0.0, 0.25, 0.25], [0.5, 0.75, 0.75]),
                _ => ([0.25, 0.0, 0.25], [0.75, 0.5, 0.75]),
            };
            add(cell.bounds(min, max));
        }
        127 => {
            let age = f32::from((meta >> 2) & 3);
            let i = 4.0 + age * 2.0;
            let j = 5.0 + age * 2.0;
            let f = i / 2.0;
            let y0 = (12.0 - j) / 16.0;
            let (min, max) = match meta & 3 {
                0 => (
                    [(8.0 - f) / 16.0, y0, (15.0 - i) / 16.0],
                    [(8.0 + f) / 16.0, 0.75, 0.9375],
                ),
                2 => (
                    [(8.0 - f) / 16.0, y0, 0.0625],
                    [(8.0 + f) / 16.0, 0.75, (1.0 + i) / 16.0],
                ),
                1 => (
                    [0.0625, y0, (8.0 - f) / 16.0],
                    [(1.0 + i) / 16.0, 0.75, (8.0 + f) / 16.0],
                ),
                _ => (
                    [(15.0 - i) / 16.0, y0, (8.0 - f) / 16.0],
                    [0.9375, 0.75, (8.0 + f) / 16.0],
                ),
            };
            add(cell.bounds(min, max));
        }
        29 | 33 if meta & 8 != 0 => {
            let (min, max) = match meta & 7 {
                0 => ([0.0, 0.25, 0.0], [1.0, 1.0, 1.0]),
                1 => ([0.0; 3], [1.0, 0.75, 1.0]),
                2 => ([0.0, 0.0, 0.25], [1.0, 1.0, 1.0]),
                3 => ([0.0; 3], [1.0, 1.0, 0.75]),
                4 => ([0.25, 0.0, 0.0], [1.0, 1.0, 1.0]),
                _ => ([0.0; 3], [0.75, 1.0, 1.0]),
            };
            add(cell.bounds(min, max));
        }
        34 => {
            let (plate, arm) = match meta & 7 {
                0 => (
                    ([0.0; 3], [1.0, 0.25, 1.0]),
                    ([0.375, 0.25, 0.375], [0.625, 1.0, 0.625]),
                ),
                1 => (
                    ([0.0, 0.75, 0.0], [1.0; 3]),
                    ([0.375, 0.0, 0.375], [0.625, 0.75, 0.625]),
                ),
                2 => (
                    ([0.0; 3], [1.0, 1.0, 0.25]),
                    ([0.25, 0.375, 0.25], [0.75, 0.625, 1.0]),
                ),
                3 => (
                    ([0.0, 0.0, 0.75], [1.0; 3]),
                    ([0.25, 0.375, 0.0], [0.75, 0.625, 0.75]),
                ),
                4 => (
                    ([0.0; 3], [0.25, 1.0, 1.0]),
                    ([0.375, 0.25, 0.25], [0.625, 0.75, 1.0]),
                ),
                _ => (
                    ([0.75, 0.0, 0.0], [1.0; 3]),
                    ([0.0, 0.375, 0.25], [0.75, 0.625, 0.75]),
                ),
            };
            add(cell.bounds(plate.0, plate.1));
            add(cell.bounds(arm.0, arm.1));
        }
        _ => add(cell.bounds([0.0; 3], [1.0; 3])),
    }
}

fn near_block(near: &Neighbours<'_>, side: &str) -> u16 {
    let [dx, dy, dz] = horizon_world_189::variants::offset(side);
    near.at(dx, dy, dz).block_id()
}

/// `World.getCollidingBoundingBoxes` without entities or the world border:
/// columns that are not loaded contribute nothing.
pub fn colliding_boxes(world: &World, mask: Aabb, out: &mut Vec<Aabb>) {
    out.clear();
    let (l, h) = (lo(mask), hi(mask));
    let (x0, x1) = (floor(l[0]), floor(h[0] + 1.0));
    let (y0, y1) = (floor(l[1]), floor(h[1] + 1.0));
    let (z0, z1) = (floor(l[2]), floor(h[2] + 1.0));
    for x in x0..x1 {
        for z in z0..z1 {
            if world.column(x.div_euclid(16), z.div_euclid(16)).is_none() {
                continue;
            }
            for y in y0.saturating_sub(1)..y1 {
                if let Some(state) = world.block(x, y, z) {
                    collision_boxes(world, [x, y, z], state, mask, out);
                }
            }
        }
    }
}
