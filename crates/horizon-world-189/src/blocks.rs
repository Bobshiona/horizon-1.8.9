//! The 1.8.9 numeric block registry: identifiers 0 through 197, their render
//! layer and the material facts neighbour logic and liquids depend on.

/// Vanilla `EnumWorldBlockLayer`, plus blocks drawn by tile-entity renderers
/// or nothing at all, and liquids drawn by the fluid renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Invisible,
    Solid,
    Cutout,
    Translucent,
    Liquid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub name: &'static str,
    pub layer: Layer,
    flags: u8,
}

/// `Block.isOpaqueCube`: hides neighbouring faces.
const OPAQUE: u8 = 1;
/// `Block.fullBlock` as constructed (dedicated-server leaves are opaque).
const FULL: u8 = 2;
/// Material opaque, `isFullCube`, not a gourd: fences and walls attach.
const CONNECT: u8 = 4;
/// `Material.blocksMovement` is false.
const PASSABLE: u8 = 8;
const WATER: u8 = 16;
const LAVA: u8 = 32;

const CUBE: u8 = OPAQUE | FULL | CONNECT;

impl Block {
    pub fn opaque_cube(self) -> bool {
        self.flags & OPAQUE != 0
    }
    pub fn full_block(self) -> bool {
        self.flags & FULL != 0
    }
    pub fn attaches_fences(self) -> bool {
        self.flags & CONNECT != 0
    }
    pub fn blocks_movement(self) -> bool {
        self.flags & PASSABLE == 0
    }
    pub fn water(self) -> bool {
        self.flags & WATER != 0
    }
    pub fn lava(self) -> bool {
        self.flags & LAVA != 0
    }
    pub fn liquid(self) -> bool {
        self.flags & (WATER | LAVA) != 0
    }
}

const fn b(name: &'static str, layer: Layer, flags: u8) -> Block {
    Block { name, layer, flags }
}

use Layer::{Cutout as C, Invisible as I, Liquid as L, Solid as S, Translucent as T};

const P: u8 = PASSABLE;

pub const BLOCKS: [Block; 198] = [
    b("air", I, P),
    b("stone", S, CUBE),
    b("grass", S, CUBE),
    b("dirt", S, CUBE),
    b("cobblestone", S, CUBE),
    b("planks", S, CUBE),
    b("sapling", C, P),
    b("bedrock", S, CUBE),
    b("flowing_water", L, P | WATER),
    b("water", L, P | WATER),
    b("flowing_lava", L, P | LAVA),
    b("lava", L, P | LAVA),
    b("sand", S, CUBE),
    b("gravel", S, CUBE),
    b("gold_ore", S, CUBE),
    b("iron_ore", S, CUBE),
    b("coal_ore", S, CUBE),
    b("log", S, CUBE),
    b("leaves", C, FULL),
    b("sponge", S, CUBE),
    b("glass", C, 0),
    b("lapis_ore", S, CUBE),
    b("lapis_block", S, CUBE),
    b("dispenser", S, CUBE),
    b("sandstone", S, CUBE),
    b("noteblock", S, CUBE),
    b("bed", S, 0),
    b("golden_rail", C, P),
    b("detector_rail", C, P),
    b("sticky_piston", S, CONNECT),
    b("web", C, P),
    b("tallgrass", C, P),
    b("deadbush", C, P),
    b("piston", S, CONNECT),
    b("piston_head", S, 0),
    b("wool", S, CUBE),
    b("piston_extension", I, 0),
    b("yellow_flower", C, P),
    b("red_flower", C, P),
    b("brown_mushroom", C, P),
    b("red_mushroom", C, P),
    b("gold_block", S, CUBE),
    b("iron_block", S, CUBE),
    b("double_stone_slab", S, CUBE),
    b("stone_slab", S, 0),
    b("brick_block", S, CUBE),
    b("tnt", S, OPAQUE | FULL),
    b("bookshelf", S, CUBE),
    b("mossy_cobblestone", S, CUBE),
    b("obsidian", S, CUBE),
    b("torch", C, P),
    b("fire", C, P),
    b("mob_spawner", C, CONNECT),
    b("oak_stairs", S, 0),
    b("chest", I, 0),
    b("redstone_wire", C, P),
    b("diamond_ore", S, CUBE),
    b("diamond_block", S, CUBE),
    b("crafting_table", S, CUBE),
    b("wheat", C, P),
    b("farmland", S, 0),
    b("furnace", S, CUBE),
    b("lit_furnace", S, CUBE),
    b("standing_sign", I, 0),
    b("wooden_door", C, 0),
    b("ladder", C, P),
    b("rail", C, P),
    b("stone_stairs", S, 0),
    b("wall_sign", I, 0),
    b("lever", C, P),
    b("stone_pressure_plate", S, 0),
    b("iron_door", C, 0),
    b("wooden_pressure_plate", S, 0),
    b("redstone_ore", S, CUBE),
    b("lit_redstone_ore", S, CUBE),
    b("unlit_redstone_torch", C, P),
    b("redstone_torch", C, P),
    b("stone_button", S, P),
    b("snow_layer", S, P),
    b("ice", T, 0),
    b("snow", S, CUBE),
    b("cactus", C, 0),
    b("clay", S, CUBE),
    b("reeds", C, P),
    b("jukebox", S, CUBE),
    b("fence", S, 0),
    b("pumpkin", S, OPAQUE | FULL),
    b("netherrack", S, CUBE),
    b("soul_sand", S, CUBE),
    b("glowstone", S, OPAQUE | FULL),
    b("portal", T, P),
    b("lit_pumpkin", S, OPAQUE | FULL),
    b("cake", S, 0),
    b("unpowered_repeater", C, P),
    b("powered_repeater", C, P),
    b("stained_glass", T, 0),
    b("trapdoor", C, 0),
    b("monster_egg", S, CUBE),
    b("stonebrick", S, CUBE),
    b("brown_mushroom_block", S, CUBE),
    b("red_mushroom_block", S, CUBE),
    b("iron_bars", C, 0),
    b("glass_pane", C, 0),
    b("melon_block", S, OPAQUE | FULL),
    b("pumpkin_stem", C, P),
    b("melon_stem", C, P),
    b("vine", C, P),
    b("fence_gate", S, 0),
    b("brick_stairs", S, 0),
    b("stone_brick_stairs", S, 0),
    b("mycelium", S, CUBE),
    b("waterlily", C, P),
    b("nether_brick", S, CUBE),
    b("nether_brick_fence", S, 0),
    b("nether_brick_stairs", S, 0),
    b("nether_wart", C, P),
    b("enchanting_table", S, 0),
    b("brewing_stand", C, 0),
    b("cauldron", S, 0),
    b("end_portal", I, P),
    b("end_portal_frame", S, 0),
    b("end_stone", S, CUBE),
    b("dragon_egg", S, 0),
    b("redstone_lamp", S, CUBE),
    b("lit_redstone_lamp", S, CUBE),
    b("double_wooden_slab", S, CUBE),
    b("wooden_slab", S, 0),
    b("cocoa", C, P),
    b("sandstone_stairs", S, 0),
    b("emerald_ore", S, CUBE),
    b("ender_chest", I, 0),
    b("tripwire_hook", C, P),
    b("tripwire", T, P),
    b("emerald_block", S, CUBE),
    b("spruce_stairs", S, 0),
    b("birch_stairs", S, 0),
    b("jungle_stairs", S, 0),
    b("command_block", S, CUBE),
    b("beacon", C, 0),
    b("cobblestone_wall", S, 0),
    b("flower_pot", C, P),
    b("carrots", C, P),
    b("potatoes", C, P),
    b("wooden_button", S, P),
    b("skull", I, P),
    b("anvil", S, 0),
    b("trapped_chest", I, 0),
    b("light_weighted_pressure_plate", S, 0),
    b("heavy_weighted_pressure_plate", S, 0),
    b("unpowered_comparator", C, P),
    b("powered_comparator", C, P),
    b("daylight_detector", S, 0),
    b("redstone_block", S, CUBE),
    b("quartz_ore", S, CUBE),
    b("hopper", S, 0),
    b("quartz_block", S, CUBE),
    b("quartz_stairs", S, 0),
    b("activator_rail", C, P),
    b("dropper", S, CUBE),
    b("stained_hardened_clay", S, CUBE),
    b("stained_glass_pane", T, 0),
    b("leaves2", C, FULL),
    b("log2", S, CUBE),
    b("acacia_stairs", S, 0),
    b("dark_oak_stairs", S, 0),
    b("slime", T, CONNECT),
    b("barrier", I, 0),
    b("iron_trapdoor", C, 0),
    b("prismarine", S, CUBE),
    b("sea_lantern", S, OPAQUE | FULL),
    b("hay_block", S, CUBE),
    b("carpet", S, P),
    b("hardened_clay", S, CUBE),
    b("coal_block", S, CUBE),
    b("packed_ice", S, CUBE),
    b("double_plant", C, P),
    b("standing_banner", I, 0),
    b("wall_banner", I, 0),
    b("daylight_detector_inverted", S, 0),
    b("red_sandstone", S, CUBE),
    b("red_sandstone_stairs", S, 0),
    b("double_stone_slab2", S, CUBE),
    b("stone_slab2", S, 0),
    b("spruce_fence_gate", S, 0),
    b("birch_fence_gate", S, 0),
    b("jungle_fence_gate", S, 0),
    b("dark_oak_fence_gate", S, 0),
    b("acacia_fence_gate", S, 0),
    b("spruce_fence", S, 0),
    b("birch_fence", S, 0),
    b("jungle_fence", S, 0),
    b("dark_oak_fence", S, 0),
    b("acacia_fence", S, 0),
    b("spruce_door", C, 0),
    b("birch_door", C, 0),
    b("jungle_door", C, 0),
    b("acacia_door", C, 0),
    b("dark_oak_door", C, 0),
];

/// Unknown identifiers behave as an opaque, collidable cube. Vanilla clients
/// never receive them; a modified server would otherwise open holes.
pub const UNKNOWN: Block = b("unknown", S, CUBE);

pub fn block(id: u16) -> Block {
    BLOCKS.get(usize::from(id)).copied().unwrap_or(UNKNOWN)
}

pub fn is_stairs(id: u16) -> bool {
    matches!(
        id,
        53 | 67 | 108 | 109 | 114 | 128 | 134 | 135 | 136 | 156 | 163 | 164 | 180
    )
}
pub fn is_fence(id: u16) -> bool {
    matches!(id, 85 | 113 | 188..=192)
}
pub fn is_fence_gate(id: u16) -> bool {
    matches!(id, 107 | 183..=187)
}
pub fn is_door(id: u16) -> bool {
    matches!(id, 64 | 71 | 193..=197)
}
pub fn is_pane(id: u16) -> bool {
    matches!(id, 101 | 102 | 160)
}

/// `BlockFence.canConnectTo`.
pub fn fence_attaches(fence: u16, neighbour: u16) -> bool {
    if neighbour == 166 {
        return false;
    }
    if is_fence(neighbour) {
        // Wooden fences share Material.wood; nether brick fences are rock.
        return (fence == 113) == (neighbour == 113);
    }
    is_fence_gate(neighbour) || block(neighbour).attaches_fences()
}

/// `BlockWall.canConnectTo`.
pub fn wall_attaches(neighbour: u16) -> bool {
    neighbour != 166
        && (neighbour == 139 || is_fence_gate(neighbour) || block(neighbour).attaches_fences())
}

/// `BlockPane.canPaneConnectToBlock`.
pub fn pane_attaches(pane: u16, neighbour: u16) -> bool {
    block(neighbour).full_block()
        || neighbour == pane
        || matches!(neighbour, 20 | 95)
        || is_pane(neighbour)
}

/// Non-zero `lightOpacity`: opaque cubes, slabs, stairs, farmland, liquids,
/// leaves, ice and cobwebs. Sky light above the highest of these is full.
pub fn blocks_light(id: u16) -> bool {
    block(id).opaque_cube()
        || is_stairs(id)
        || matches!(id, 8..=11 | 18 | 30 | 44 | 60 | 79 | 126 | 161 | 182)
}

/// Default 0.6F, ice and packed ice 0.98F, slime 0.8F.
pub fn slipperiness(id: u16) -> f32 {
    match id {
        79 | 174 => 0.98,
        165 => 0.8,
        _ => 0.6,
    }
}
