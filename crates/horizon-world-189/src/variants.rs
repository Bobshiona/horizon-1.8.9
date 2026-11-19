//! Blockstate selection: each block's `getStateFromMeta`, then the
//! `getActualState` neighbour properties the 1.8.9 state mappers keep. The
//! result names a file in `assets/minecraft/blockstates` and a variant key with
//! properties in alphabetical order, as the 1.8 resource format writes them.
use crate::{
    BlockState, World,
    blocks::{
        Layer, block, fence_attaches, is_door, is_fence_gate, is_stairs, pane_attaches,
        wall_attaches,
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Variant {
    pub file: String,
    pub key: String,
}

const COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "silver",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];
const WOODS: [&str; 6] = ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak"];
/// `EnumFacing.getFront` order.
const FRONT: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
/// `EnumFacing.getHorizontal` order.
const HORIZONTAL: [&str; 4] = ["south", "west", "north", "east"];
const STONE_SLABS: [&str; 8] = [
    "stone",
    "sandstone",
    "wood_old",
    "cobblestone",
    "brick",
    "stone_brick",
    "nether_brick",
    "quartz",
];

pub fn offset(direction: &str) -> [i32; 3] {
    match direction {
        "down" => [0, -1, 0],
        "up" => [0, 1, 0],
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        _ => [1, 0, 0],
    }
}

fn front(index: u8) -> &'static str {
    FRONT[usize::from(index) % 6]
}

fn horizontal(index: u8) -> &'static str {
    HORIZONTAL[usize::from(index & 3)]
}

/// `EnumFacing.rotateYCCW`.
fn ccw(direction: &str) -> &'static str {
    match direction {
        "north" => "west",
        "west" => "south",
        "south" => "east",
        _ => "north",
    }
}

fn opposite(direction: &str) -> &'static str {
    match direction {
        "north" => "south",
        "south" => "north",
        "west" => "east",
        "east" => "west",
        "up" => "down",
        _ => "up",
    }
}

fn axis_of(direction: &str) -> char {
    match direction {
        "north" | "south" => 'z',
        "west" | "east" => 'x',
        _ => 'y',
    }
}

fn bool_str(value: bool) -> String {
    value.to_string()
}

fn key(mut properties: Vec<(&'static str, String)>) -> String {
    if properties.is_empty() {
        return "normal".to_owned();
    }
    properties.sort_by(|a, b| a.0.cmp(b.0));
    properties
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

pub struct Neighbours<'a> {
    world: &'a World,
    pos: [i32; 3],
}

impl<'a> Neighbours<'a> {
    pub fn new(world: &'a World, pos: [i32; 3]) -> Self {
        Self { world, pos }
    }
    pub fn at(&self, dx: i32, dy: i32, dz: i32) -> BlockState {
        let [x, y, z] = self.pos;
        match (x.checked_add(dx), y.checked_add(dy), z.checked_add(dz)) {
            (Some(x), Some(y), Some(z)) => self.world.block(x, y, z).unwrap_or_default(),
            _ => BlockState(0),
        }
    }
    fn toward(&self, direction: &str) -> BlockState {
        let [dx, dy, dz] = offset(direction);
        self.at(dx, dy, dz)
    }
}

fn simple(file: impl Into<String>) -> Option<Variant> {
    Some(Variant {
        file: file.into(),
        key: "normal".to_owned(),
    })
}

fn with(file: impl Into<String>, properties: Vec<(&'static str, String)>) -> Option<Variant> {
    Some(Variant {
        file: file.into(),
        key: key(properties),
    })
}

fn stairs_file(id: u16) -> &'static str {
    match id {
        53 => "oak_stairs",
        67 => "stone_stairs",
        108 => "brick_stairs",
        109 => "stone_brick_stairs",
        114 => "nether_brick_stairs",
        128 => "sandstone_stairs",
        134 => "spruce_stairs",
        135 => "birch_stairs",
        136 => "jungle_stairs",
        156 => "quartz_stairs",
        163 => "acacia_stairs",
        164 => "dark_oak_stairs",
        _ => "red_sandstone_stairs",
    }
}

/// `BlockStairs` metadata: facing `getFront(5 - (meta & 3))`, top half bit 4.
pub fn stairs_facing(state: BlockState) -> &'static str {
    front(5 - (state.metadata() & 3))
}

pub fn stairs_top(state: BlockState) -> bool {
    state.metadata() & 4 != 0
}

/// Corner shape from the stairs behind and in front, as `getActualState`.
pub fn stairs_shape(near: &Neighbours<'_>, state: BlockState) -> &'static str {
    let facing = stairs_facing(state);
    let top = stairs_top(state);
    let different = |direction: &str| {
        let other = near.toward(direction);
        !is_stairs(other.block_id()) || stairs_facing(other) != facing || stairs_top(other) != top
    };
    let back = near.toward(facing);
    if is_stairs(back.block_id()) && stairs_top(back) == top {
        let back_facing = stairs_facing(back);
        if axis_of(back_facing) != axis_of(facing) && different(opposite(back_facing)) {
            return if back_facing == ccw(facing) {
                "outer_left"
            } else {
                "outer_right"
            };
        }
    }
    let front_state = near.toward(opposite(facing));
    if is_stairs(front_state.block_id()) && stairs_top(front_state) == top {
        let front_facing = stairs_facing(front_state);
        if axis_of(front_facing) != axis_of(facing) && different(front_facing) {
            return if front_facing == ccw(facing) {
                "inner_left"
            } else {
                "inner_right"
            };
        }
    }
    "straight"
}

fn door_file(id: u16) -> &'static str {
    match id {
        64 => "wooden_door",
        71 => "iron_door",
        193 => "spruce_door",
        194 => "birch_door",
        195 => "jungle_door",
        196 => "acacia_door",
        _ => "dark_oak_door",
    }
}

/// Lower-half door facing: `getHorizontal(meta & 3).rotateYCCW()`.
pub fn door_facing(lower_meta: u8) -> &'static str {
    ["east", "south", "west", "north"][usize::from(lower_meta & 3)]
}

/// Combined door properties: (facing, open, hinge right, upper half).
pub fn door_state(near: &Neighbours<'_>, state: BlockState) -> (&'static str, bool, bool, bool) {
    let meta = state.metadata();
    let upper = meta & 8 != 0;
    let (lower, upper_meta) = if upper {
        let below = near.at(0, -1, 0);
        let lower = if below.block_id() == state.block_id() {
            below.metadata()
        } else {
            0
        };
        (lower, meta)
    } else {
        let above = near.at(0, 1, 0);
        let upper_meta = if above.block_id() == state.block_id() {
            above.metadata()
        } else {
            0
        };
        (meta, upper_meta)
    };
    (
        door_facing(lower),
        lower & 4 != 0,
        upper_meta & 1 != 0,
        upper,
    )
}

fn fence_file(id: u16) -> &'static str {
    match id {
        85 => "fence",
        113 => "nether_brick_fence",
        188 => "spruce_fence",
        189 => "birch_fence",
        190 => "jungle_fence",
        191 => "dark_oak_fence",
        _ => "acacia_fence",
    }
}

fn gate_file(id: u16) -> &'static str {
    match id {
        107 => "fence_gate",
        183 => "spruce_fence_gate",
        184 => "birch_fence_gate",
        185 => "jungle_fence_gate",
        186 => "dark_oak_fence_gate",
        _ => "acacia_fence_gate",
    }
}

/// Fence gate facing `getHorizontal(meta)`.
pub fn gate_facing(state: BlockState) -> &'static str {
    horizontal(state.metadata())
}

fn connections(mut attach: impl FnMut(&'static str) -> bool) -> Vec<(&'static str, String)> {
    ["north", "south", "west", "east"]
        .into_iter()
        .map(|side| (side, bool_str(attach(side))))
        .collect()
}

fn rail_shape(meta: u8) -> &'static str {
    [
        "north_south",
        "east_west",
        "ascending_east",
        "ascending_west",
        "ascending_north",
        "ascending_south",
        "south_east",
        "south_west",
        "north_west",
        "north_east",
    ]
    .get(usize::from(meta))
    .copied()
    .unwrap_or("north_south")
}

fn log_axis(meta: u8) -> &'static str {
    ["y", "x", "z", "none"][usize::from((meta >> 2) & 3)]
}

fn wire_attaches(id: u16) -> bool {
    matches!(
        id,
        55 | 69
            | 70
            | 72
            | 75
            | 76
            | 77
            | 93
            | 94
            | 131
            | 143
            | 146
            | 147
            | 148
            | 149
            | 150
            | 151
            | 152
            | 178
            | 28
    )
}

/// The blockstate file and variant key for a block, or `None` when the block
/// has no block model (air, liquids, tile-entity renderers, barriers).
pub fn variant(world: &World, pos: [i32; 3], state: BlockState) -> Option<Variant> {
    let id = state.block_id();
    let meta = state.metadata();
    if matches!(block(id).layer, Layer::Invisible | Layer::Liquid) {
        return None;
    }
    let near = Neighbours { world, pos };
    let color = COLORS[usize::from(meta)];
    let snowy = || {
        let above = near.at(0, 1, 0).block_id();
        vec![("snowy", bool_str(above == 78 || above == 80))]
    };
    match id {
        1 => simple(
            [
                "stone",
                "granite",
                "smooth_granite",
                "diorite",
                "smooth_diorite",
                "andesite",
                "smooth_andesite",
            ]
            .get(usize::from(meta))
            .copied()
            .unwrap_or("stone"),
        ),
        2 | 110 => with(if id == 2 { "grass" } else { "mycelium" }, snowy()),
        3 => match meta {
            1 => simple("coarse_dirt"),
            2 => with("podzol", snowy()),
            _ => simple("dirt"),
        },
        5 => simple(format!("{}_planks", WOODS[usize::from(meta % 6)])),
        6 => with(
            format!("{}_sapling", WOODS[usize::from((meta & 7) % 6)]),
            vec![("stage", (meta >> 3).to_string())],
        ),
        12 => simple(if meta == 1 { "red_sand" } else { "sand" }),
        17 | 162 => {
            let wood = if id == 17 {
                WOODS[usize::from(meta & 3)]
            } else {
                WOODS[4 + usize::from(meta & 1)]
            };
            with(
                format!("{wood}_log"),
                vec![("axis", log_axis(meta).to_owned())],
            )
        }
        18 | 161 => {
            let wood = if id == 18 {
                WOODS[usize::from(meta & 3)]
            } else {
                WOODS[4 + usize::from(meta & 1)]
            };
            simple(format!("{wood}_leaves"))
        }
        19 => with("sponge", vec![("wet", bool_str(meta & 1 != 0))]),
        23 | 158 => with(
            if id == 23 { "dispenser" } else { "dropper" },
            vec![("facing", front(meta & 7).to_owned())],
        ),
        24 | 179 => {
            let base = if id == 24 {
                "sandstone"
            } else {
                "red_sandstone"
            };
            simple(match meta {
                1 => format!("chiseled_{base}"),
                2 => format!("smooth_{base}"),
                _ => base.to_owned(),
            })
        }
        26 => with(
            "bed",
            vec![
                ("facing", horizontal(meta).to_owned()),
                (
                    "part",
                    if meta & 8 != 0 { "head" } else { "foot" }.to_owned(),
                ),
            ],
        ),
        27 | 28 | 157 => with(
            match id {
                27 => "golden_rail",
                28 => "detector_rail",
                _ => "activator_rail",
            },
            vec![
                ("shape", rail_shape(meta & 7).to_owned()),
                ("powered", bool_str(meta & 8 != 0)),
            ],
        ),
        66 => with("rail", vec![("shape", rail_shape(meta).to_owned())]),
        29 | 33 => with(
            if id == 29 { "sticky_piston" } else { "piston" },
            vec![
                ("facing", front(meta & 7).to_owned()),
                ("extended", bool_str(meta & 8 != 0)),
            ],
        ),
        34 => with(
            "piston_head",
            vec![
                ("facing", front(meta & 7).to_owned()),
                ("short", "false".to_owned()),
                (
                    "type",
                    if meta & 8 != 0 { "sticky" } else { "normal" }.to_owned(),
                ),
            ],
        ),
        31 => simple(match meta {
            0 => "dead_bush",
            2 => "fern",
            _ => "tall_grass",
        }),
        32 => simple("dead_bush"),
        35 => simple(format!("{color}_wool")),
        95 => simple(format!("{color}_stained_glass")),
        159 => simple(format!("{color}_stained_hardened_clay")),
        171 => simple(format!("{color}_carpet")),
        160 => with(
            format!("{color}_stained_glass_pane"),
            connections(|side| pane_attaches(id, near.toward(side).block_id())),
        ),
        101 | 102 => with(
            if id == 101 { "iron_bars" } else { "glass_pane" },
            connections(|side| pane_attaches(id, near.toward(side).block_id())),
        ),
        37 => simple("dandelion"),
        38 => simple(
            [
                "poppy",
                "blue_orchid",
                "allium",
                "houstonia",
                "red_tulip",
                "orange_tulip",
                "white_tulip",
                "pink_tulip",
                "oxeye_daisy",
            ]
            .get(usize::from(meta))
            .copied()
            .unwrap_or("poppy"),
        ),
        43 | 181 => {
            let name = if id == 43 {
                STONE_SLABS[usize::from(meta & 7)]
            } else {
                "red_sandstone"
            };
            Some(Variant {
                file: format!("{name}_double_slab"),
                key: if meta & 8 != 0 { "all" } else { "normal" }.to_owned(),
            })
        }
        125 => simple(format!(
            "{}_double_slab",
            WOODS[usize::from((meta & 7) % 6)]
        )),
        44 | 126 | 182 => {
            let name = match id {
                44 => STONE_SLABS[usize::from(meta & 7)],
                126 => WOODS[usize::from((meta & 7) % 6)],
                _ => "red_sandstone",
            };
            with(
                format!("{name}_slab"),
                vec![(
                    "half",
                    if meta & 8 != 0 { "top" } else { "bottom" }.to_owned(),
                )],
            )
        }
        50 | 75 | 76 => with(
            match id {
                50 => "torch",
                75 => "unlit_redstone_torch",
                _ => "redstone_torch",
            },
            vec![(
                "facing",
                match meta {
                    1 => "east",
                    2 => "west",
                    3 => "south",
                    4 => "north",
                    _ => "up",
                }
                .to_owned(),
            )],
        ),
        51 => Some(Variant {
            file: "fire".to_owned(),
            key: "alt=false,east=false,flip=false,north=false,south=false,upper=0,west=false"
                .to_owned(),
        }),
        55 => {
            let above_opaque = block(near.at(0, 1, 0).block_id()).opaque_cube();
            with(
                "redstone_wire",
                ["north", "south", "west", "east"]
                    .into_iter()
                    .map(|side| {
                        let [dx, _, dz] = offset(side);
                        let beside = near.at(dx, 0, dz).block_id();
                        let value = if !above_opaque && near.at(dx, 1, dz).block_id() == 55 {
                            "up"
                        } else if wire_attaches(beside)
                            || (!block(beside).opaque_cube()
                                && near.at(dx, -1, dz).block_id() == 55)
                        {
                            "side"
                        } else {
                            "none"
                        };
                        (side, value.to_owned())
                    })
                    .collect(),
            )
        }
        53 | 67 | 108 | 109 | 114 | 128 | 134 | 135 | 136 | 156 | 163 | 164 | 180 => with(
            stairs_file(id),
            vec![
                ("facing", stairs_facing(state).to_owned()),
                (
                    "half",
                    if stairs_top(state) { "top" } else { "bottom" }.to_owned(),
                ),
                ("shape", stairs_shape(&near, state).to_owned()),
            ],
        ),
        59 | 141 | 142 => with(
            match id {
                59 => "wheat",
                141 => "carrots",
                _ => "potatoes",
            },
            vec![("age", (meta & 7).to_string())],
        ),
        60 => with("farmland", vec![("moisture", (meta & 7).to_string())]),
        61 | 62 | 65 => {
            let facing = front(meta);
            let facing = if axis_of(facing) == 'y' {
                "north"
            } else {
                facing
            };
            with(
                match id {
                    61 => "furnace",
                    62 => "lit_furnace",
                    _ => "ladder",
                },
                vec![("facing", facing.to_owned())],
            )
        }
        64 | 71 | 193..=197 => {
            let (facing, open, right, upper) = door_state(&near, state);
            with(
                door_file(id),
                vec![
                    ("facing", facing.to_owned()),
                    ("half", if upper { "upper" } else { "lower" }.to_owned()),
                    ("hinge", if right { "right" } else { "left" }.to_owned()),
                    ("open", bool_str(open)),
                ],
            )
        }
        69 => with(
            "lever",
            vec![
                (
                    "facing",
                    [
                        "down_x", "east", "west", "south", "north", "up_z", "up_x", "down_z",
                    ][usize::from(meta & 7)]
                    .to_owned(),
                ),
                ("powered", bool_str(meta & 8 != 0)),
            ],
        ),
        70 | 72 => with(
            if id == 70 {
                "stone_pressure_plate"
            } else {
                "wooden_pressure_plate"
            },
            vec![("powered", bool_str(meta == 1))],
        ),
        147 | 148 | 151 | 178 => with(
            match id {
                147 => "light_weighted_pressure_plate",
                148 => "heavy_weighted_pressure_plate",
                151 => "daylight_detector",
                _ => "daylight_detector_inverted",
            },
            vec![("power", meta.to_string())],
        ),
        73 => simple("redstone_ore"),
        74 => simple("lit_redstone_ore"),
        77 | 143 => with(
            if id == 77 {
                "stone_button"
            } else {
                "wooden_button"
            },
            vec![
                (
                    "facing",
                    match meta & 7 {
                        0 => "down",
                        1 => "east",
                        2 => "west",
                        3 => "south",
                        4 => "north",
                        _ => "up",
                    }
                    .to_owned(),
                ),
                ("powered", bool_str(meta & 8 != 0)),
            ],
        ),
        78 => with("snow_layer", vec![("layers", ((meta & 7) + 1).to_string())]),
        85 | 113 | 188..=192 => with(
            fence_file(id),
            connections(|side| fence_attaches(id, near.toward(side).block_id())),
        ),
        86 | 91 => with(
            if id == 86 { "pumpkin" } else { "lit_pumpkin" },
            vec![("facing", horizontal(meta).to_owned())],
        ),
        90 => with(
            "portal",
            vec![("axis", if meta & 3 == 2 { "z" } else { "x" }.to_owned())],
        ),
        92 => with("cake", vec![("bites", meta.min(6).to_string())]),
        93 | 94 => with(
            if id == 93 {
                "unpowered_repeater"
            } else {
                "powered_repeater"
            },
            vec![
                ("delay", ((meta >> 2) + 1).to_string()),
                ("facing", horizontal(meta).to_owned()),
                ("locked", "false".to_owned()),
            ],
        ),
        149 | 150 => with(
            if id == 149 {
                "unpowered_comparator"
            } else {
                "powered_comparator"
            },
            vec![
                ("facing", horizontal(meta).to_owned()),
                (
                    "mode",
                    if meta & 4 != 0 { "subtract" } else { "compare" }.to_owned(),
                ),
                ("powered", bool_str(meta & 8 != 0)),
            ],
        ),
        96 | 167 => with(
            if id == 96 {
                "trapdoor"
            } else {
                "iron_trapdoor"
            },
            vec![
                (
                    "facing",
                    ["north", "south", "west", "east"][usize::from(meta & 3)].to_owned(),
                ),
                (
                    "half",
                    if meta & 8 != 0 { "top" } else { "bottom" }.to_owned(),
                ),
                ("open", bool_str(meta & 4 != 0)),
            ],
        ),
        97 => simple(format!(
            "{}_monster_egg",
            [
                "stone",
                "cobblestone",
                "stone_brick",
                "mossy_brick",
                "cracked_brick",
                "chiseled_brick"
            ]
            .get(usize::from(meta))
            .copied()
            .unwrap_or("stone")
        )),
        98 => simple(match meta {
            1 => "mossy_stonebrick",
            2 => "cracked_stonebrick",
            3 => "chiseled_stonebrick",
            _ => "stonebrick",
        }),
        99 | 100 => with(
            if id == 99 {
                "brown_mushroom_block"
            } else {
                "red_mushroom_block"
            },
            vec![(
                "variant",
                match meta {
                    1 => "north_west",
                    2 => "north",
                    3 => "north_east",
                    4 => "west",
                    5 => "center",
                    6 => "east",
                    7 => "south_west",
                    8 => "south",
                    9 => "south_east",
                    10 => "stem",
                    14 => "all_outside",
                    15 => "all_stem",
                    _ => "all_inside",
                }
                .to_owned(),
            )],
        ),
        104 | 105 => {
            let fruit = if id == 104 { 86 } else { 103 };
            let facing = if meta & 7 == 7 {
                ["west", "east", "north", "south"]
                    .into_iter()
                    .find(|side| near.toward(side).block_id() == fruit)
                    .unwrap_or("up")
            } else {
                "up"
            };
            with(
                if id == 104 {
                    "pumpkin_stem"
                } else {
                    "melon_stem"
                },
                vec![
                    ("age", (meta & 7).to_string()),
                    ("facing", facing.to_owned()),
                ],
            )
        }
        106 => with(
            "vine",
            vec![
                ("south", bool_str(meta & 1 != 0)),
                ("west", bool_str(meta & 2 != 0)),
                ("north", bool_str(meta & 4 != 0)),
                ("east", bool_str(meta & 8 != 0)),
                (
                    "up",
                    bool_str(block(near.at(0, 1, 0).block_id()).opaque_cube()),
                ),
            ],
        ),
        107 | 183..=187 => {
            let facing = gate_facing(state);
            let sides = if axis_of(facing) == 'z' {
                ["west", "east"]
            } else {
                ["north", "south"]
            };
            let in_wall = sides.iter().any(|side| near.toward(side).block_id() == 139);
            with(
                gate_file(id),
                vec![
                    ("facing", facing.to_owned()),
                    ("in_wall", bool_str(in_wall)),
                    ("open", bool_str(meta & 4 != 0)),
                ],
            )
        }
        115 => with("nether_wart", vec![("age", (meta & 3).to_string())]),
        117 => with(
            "brewing_stand",
            vec![
                ("has_bottle_0", bool_str(meta & 1 != 0)),
                ("has_bottle_1", bool_str(meta & 2 != 0)),
                ("has_bottle_2", bool_str(meta & 4 != 0)),
            ],
        ),
        118 => with("cauldron", vec![("level", (meta & 3).to_string())]),
        120 => with(
            "end_portal_frame",
            vec![
                ("eye", bool_str(meta & 4 != 0)),
                ("facing", horizontal(meta).to_owned()),
            ],
        ),
        123 => simple("redstone_lamp"),
        124 => simple("lit_redstone_lamp"),
        127 => with(
            "cocoa",
            vec![
                ("age", ((meta >> 2) & 3).min(2).to_string()),
                ("facing", horizontal(meta).to_owned()),
            ],
        ),
        131 => with(
            "tripwire_hook",
            vec![
                ("attached", bool_str(meta & 4 != 0)),
                ("facing", horizontal(meta).to_owned()),
                ("powered", bool_str(meta & 8 != 0)),
                (
                    "suspended",
                    bool_str(!block(near.at(0, -1, 0).block_id()).opaque_cube()),
                ),
            ],
        ),
        132 => {
            let mut properties =
                connections(|side| matches!(near.toward(side).block_id(), 131 | 132));
            properties.push(("attached", bool_str(meta & 4 != 0)));
            properties.push(("suspended", bool_str(meta & 2 != 0)));
            with("tripwire", properties)
        }
        139 => {
            let mut properties = connections(|side| wall_attaches(near.toward(side).block_id()));
            properties.push(("up", bool_str(near.at(0, 1, 0).block_id() != 0)));
            with(
                if meta == 1 {
                    "mossy_cobblestone_wall"
                } else {
                    "cobblestone_wall"
                },
                properties,
            )
        }
        140 => with("flower_pot", vec![("contents", "empty".to_owned())]),
        145 => with(
            "anvil",
            vec![
                ("damage", ((meta >> 2) & 3).min(2).to_string()),
                ("facing", horizontal(meta).to_owned()),
            ],
        ),
        154 => {
            let facing = front(meta & 7);
            with(
                "hopper",
                vec![(
                    "facing",
                    if facing == "up" { "down" } else { facing }.to_owned(),
                )],
            )
        }
        155 => match meta {
            1 => simple("chiseled_quartz_block"),
            2..=4 => with(
                "quartz_column",
                vec![("axis", ["y", "x", "z"][usize::from(meta - 2)].to_owned())],
            ),
            _ => simple("quartz_block"),
        },
        168 => simple(match meta {
            1 => "prismarine_bricks",
            2 => "dark_prismarine",
            _ => "prismarine",
        }),
        170 => with(
            "hay_block",
            vec![(
                "axis",
                match meta & 12 {
                    4 => "x",
                    8 => "z",
                    _ => "y",
                }
                .to_owned(),
            )],
        ),
        175 => {
            let upper = meta & 8 != 0;
            let kind = if upper {
                let below = near.at(0, -1, 0);
                if below.block_id() == 175 {
                    below.metadata() & 7
                } else {
                    0
                }
            } else {
                meta & 7
            };
            with(
                [
                    "sunflower",
                    "syringa",
                    "double_grass",
                    "double_fern",
                    "double_rose",
                    "paeonia",
                ]
                .get(usize::from(kind))
                .copied()
                .unwrap_or("sunflower"),
                vec![("half", if upper { "upper" } else { "lower" }.to_owned())],
            )
        }
        _ if is_door(id) || is_fence_gate(id) => None,
        _ => simple(block(id).name),
    }
}
