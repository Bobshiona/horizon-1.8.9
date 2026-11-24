//! Legacy terrain meshing. Every 1.8.9 blockstate variant in the owned jar is
//! baked once, with its quarter turns and UV lock. Meshes are built per
//! column with biome tints, flat lighting, and separate opaque/cutout and
//! translucent vertex lists. Liquids use the still textures' first frame.
use bytemuck::{Pod, Zeroable};
use horizon_assets_189::{Assets, Texture, VariantSet};
use horizon_world_189::{
    BlockState, Keys, World,
    blocks::{Layer, block, is_door, is_fence, is_fence_gate, is_pane, is_stairs},
    variants::{Variant, variant},
};
use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    path::Path,
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub layer: u32,
    pub color: [f32; 3],
    pub hurt: f32,
}

#[derive(Clone)]
struct Face {
    positions: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    layer: u32,
    shade: f32,
    tint: bool,
    cull: Option<[i32; 3]>,
    /// Set for a whole block side: greedy meshing may merge it with its neighbours'.
    grid: Option<crate::greedy::Grid>,
}

impl Face {
    fn baked(
        positions: [[f32; 3]; 4],
        uv: [[f32; 2]; 4],
        layer: u32,
        shade: f32,
        tint: bool,
        cull: Option<[i32; 3]>,
    ) -> Self {
        Face {
            grid: crate::greedy::Grid::of(&positions, &uv, cull),
            positions,
            uv,
            layer,
            shade,
            tint,
            cull,
        }
    }
}

struct Model {
    particle: u32,
    alternatives: Vec<(Vec<Face>, i32)>,
    total: i32,
}

/// Packed terrain corner, 12 bytes in three words:
///
/// - word 0: x (11 bits) | z (11) | sprite layer bits 0-9
/// - word 1: y (15 bits) | layer bit 10 | u (12) | v bits 0-3
/// - word 2: v bits 4-11 | red | green | blue
///
/// Positions are column-local in 1/64 blocks biased by 8 blocks (x and z
/// -8..24, y -8..504); model geometry is in sixteenths, liquid heights in
/// ninths (within 1/128). Texture coordinates are 1/128 sprites up to 32 so a
/// greedy quad can repeat its sprite across a section. The colour is the
/// gamma-space tint x face shade x light, as vanilla's 8-bit vertex colour.
/// Quads are four corners drawn through the shared quad index list.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct TerrainVertex {
    pub packed: [u32; 3],
}

impl TerrainVertex {
    pub const SCALE: f32 = 64.0;
    pub const BIAS: f32 = 8.0;
    /// Texture coordinates in 1/128 sprites, up to 32 sprites.
    pub const UV_SCALE: f32 = 128.0;
    /// Sprite layers the format holds.
    pub const LAYERS: u32 = 1 << 11;

    pub fn new(local: [f32; 3], uv: [f32; 2], layer: u32, color: [f32; 3]) -> Self {
        let axis = |value: f32, bits: u32| {
            ((value + Self::BIAS) * Self::SCALE)
                .round()
                .clamp(0.0, ((1 << bits) - 1) as f32) as u32
        };
        let unit = |value: f32| (value * Self::UV_SCALE).round().clamp(0.0, 4095.0) as u32;
        let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
        let (x, y, z) = (axis(local[0], 11), axis(local[1], 15), axis(local[2], 11));
        let layer = layer.min(Self::LAYERS - 1);
        let (u, v) = (unit(uv[0]), unit(uv[1]));
        Self {
            packed: [
                x | z << 11 | (layer & 1023) << 22,
                y | (layer >> 10) << 15 | u << 16 | (v & 15) << 28,
                v >> 4 | byte(color[0]) << 8 | byte(color[1]) << 16 | byte(color[2]) << 24,
            ],
        }
    }

    /// x, y, z in 1/`SCALE` blocks (biased) and the sprite layer.
    pub fn position(&self) -> [u16; 4] {
        let [a, b, _] = self.packed;
        [
            (a & 2047) as u16,
            (b & 32767) as u16,
            (a >> 11 & 2047) as u16,
            ((a >> 22) | (b >> 15 & 1) << 10) as u16,
        ]
    }

    /// u, v in 1/`UV_SCALE` sprites.
    #[cfg(test)]
    pub fn uv(&self) -> [u16; 2] {
        let [_, b, c] = self.packed;
        [(b >> 16 & 4095) as u16, ((b >> 28) | (c & 255) << 4) as u16]
    }

    /// The gamma-space colour bytes.
    #[cfg(test)]
    pub fn color(&self) -> [u8; 3] {
        let c = self.packed[2];
        [(c >> 8) as u8, (c >> 16) as u8, (c >> 24) as u8]
    }

    #[cfg(test)]
    fn local(&self) -> [f32; 3] {
        let position = self.position();
        std::array::from_fn(|axis| f32::from(position[axis]) / Self::SCALE - Self::BIAS)
    }
}

/// One render pass of a column: four corners per quad, ordered by
/// 16-block section so each section is a contiguous quad range.
#[derive(Default)]
pub struct TerrainPass {
    pub vertices: Vec<TerrainVertex>,
    /// Quad index where each section starts; `sections[16]` is the total.
    pub sections: [u32; 17],
    /// Exact decoded positions per nonempty section: min xyz, max xyz.
    /// Computed once by the mesher, including model overhangs, not per frame.
    pub bounds: [[f32; 6]; 16],
    /// Per section, where each facing group starts (`buckets`), once
    /// `sort_by_facing` has grouped the quads.
    facing: [[u32; 8]; 16],
    faced: bool,
}

/// Facing groups of a section's quads: the six axis directions in `FACES`
/// order, then every quad whose normal is not along an axis.
pub const ANY_FACING: usize = 6;

/// The group of a quad: its winding normal (counter-clockwise is the front, as
/// the terrain pipelines cull) along one axis, else `ANY_FACING`.
pub fn facing(quad: &[TerrainVertex]) -> usize {
    let corner = |index: usize| {
        let position = quad[index].position();
        [0, 1, 2].map(|axis| i64::from(position[axis]))
    };
    let (a, b, c) = (corner(0), corner(1), corner(2));
    let (u, v) = (
        [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
        [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
    );
    let normal = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let axis = match normal.map(|value| value != 0) {
        [true, false, false] => 0,
        [false, true, false] => 1,
        [false, false, true] => 2,
        _ => return ANY_FACING,
    };
    let positive = normal[axis] > 0;
    FACES
        .iter()
        .position(|face| face[axis] == if positive { 1 } else { -1 })
        .expect("an axis direction")
}

impl TerrainPass {
    pub fn quads(&self) -> u32 {
        (self.vertices.len() / 4) as u32
    }

    /// Quad offsets where each facing group of `section` starts, and its end
    /// (`[7]`). Unsorted passes hold everything in `ANY_FACING`.
    pub fn buckets(&self, section: usize) -> [u32; 8] {
        if self.faced {
            return self.facing[section];
        }
        let (start, end) = (self.sections[section], self.sections[section + 1]);
        [start, start, start, start, start, start, start, end]
    }

    /// Group each section's quads by facing, keeping their order within a group
    /// (an overlay stays after the face it covers). Only for passes drawn with
    /// back faces culled: a group facing away from the camera is then invisible.
    pub fn sort_by_facing(&mut self) {
        let mut sorted = vec![TerrainVertex { packed: [0; 3] }; self.vertices.len()];
        let mut groups = Vec::<u8>::new();
        for section in 0..16 {
            let (start, end) = (
                self.sections[section] as usize,
                self.sections[section + 1] as usize,
            );
            groups.clear();
            let mut counts = [0u32; 7];
            for quad in start..end {
                let group = facing(&self.vertices[quad * 4..quad * 4 + 4]);
                groups.push(group as u8);
                counts[group] += 1;
            }
            let mut starts = [start as u32; 8];
            for group in 0..7 {
                starts[group + 1] = starts[group] + counts[group];
            }
            let mut next = starts;
            // Seven-bucket stable counting sort: one classification per quad,
            // no allocation per face or comparison-sort scratch per section.
            for (quad, &group) in (start..end).zip(&groups) {
                let target = next[group as usize] as usize * 4;
                sorted[target..target + 4].copy_from_slice(&self.vertices[quad * 4..quad * 4 + 4]);
                next[group as usize] += 1;
            }
            self.facing[section] = starts;
        }
        self.vertices = sorted;
        self.faced = true;
    }

    fn close_section(&mut self, section: usize) {
        let quads = self.quads();
        let vertices = &self.vertices[self.sections[section] as usize * 4..quads as usize * 4];
        if !vertices.is_empty() {
            let mut low = [u16::MAX; 3];
            let mut high = [0u16; 3];
            for vertex in vertices {
                let xyz = vertex.position();
                for axis in 0..3 {
                    low[axis] = low[axis].min(xyz[axis]);
                    high[axis] = high[axis].max(xyz[axis]);
                }
            }
            self.bounds[section] = std::array::from_fn(|axis| {
                let value = if axis < 3 { low[axis] } else { high[axis - 3] };
                f32::from(value) / TerrainVertex::SCALE - TerrainVertex::BIAS
            });
        }
        for start in &mut self.sections[section + 1..] {
            *start = quads;
        }
    }
}

/// Per-column geometry in column-local coordinates. Solid quads draw first
/// without alpha testing so the GPU can reject hidden surfaces early, cutout
/// quads (sprites with transparent texels) next, translucent last. Block
/// entities use the entity texture array, relative to the mesh origin.
#[derive(Default)]
pub struct ColumnMesh {
    pub solid: TerrainPass,
    pub cutout: TerrainPass,
    pub translucent: TerrainPass,
    pub entity: Vec<Vertex>,
    /// `VisGraph` face connectivity per section (see `visibility`).
    pub visibility: [u64; 16],
}

/// Section faces in `EnumFacing` order: down, up, north, south, west, east.
pub const FACES: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];
/// Every face sees every other: `SetVisibility.setAllVisible(true)`.
pub const ALL_VISIBLE: u64 = (1 << 36) - 1;

/// Bit for "entering through `from`, leaving through `to`".
pub const fn sees(from: usize, to: usize) -> u64 {
    1 << (from * 6 + to)
}

/// `VisGraph.computeVisibility`: which section faces connect through
/// cells that are not opaque cubes. Fewer than 256 opaque cubes sees all;
/// a fully opaque section sees nothing; otherwise each flood-filled region
/// joins every face it touches.
pub fn visibility(opaque: impl Fn(usize) -> bool) -> u64 {
    let mut solid = [false; 4096];
    let mut count = 0;
    for (index, cell) in solid.iter_mut().enumerate() {
        *cell = opaque(index);
        count += usize::from(*cell);
    }
    if count < 256 {
        return ALL_VISIBLE;
    }
    if count == 4096 {
        return 0;
    }
    let mut seen = solid;
    let mut result = 0;
    let mut stack = Vec::new();
    for first in 0..4096 {
        if seen[first] {
            continue;
        }
        seen[first] = true;
        stack.push(first);
        let mut touched = 0u8;
        while let Some(index) = stack.pop() {
            let (x, y, z) = (index & 15, index >> 8, (index >> 4) & 15);
            touched |= u8::from(y == 0)
                | u8::from(y == 15) << 1
                | u8::from(z == 0) << 2
                | u8::from(z == 15) << 3
                | u8::from(x == 0) << 4
                | u8::from(x == 15) << 5;
            let mut visit = |next: usize| {
                if !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            };
            if y > 0 {
                visit(index - 256);
            }
            if y < 15 {
                visit(index + 256);
            }
            if z > 0 {
                visit(index - 16);
            }
            if z < 15 {
                visit(index + 16);
            }
            if x > 0 {
                visit(index - 1);
            }
            if x < 15 {
                visit(index + 1);
            }
        }
        for from in 0..6 {
            for to in 0..6 {
                if touched & (1 << from) != 0 && touched & (1 << to) != 0 {
                    result |= sees(from, to);
                }
            }
        }
    }
    result
}

impl ColumnMesh {
    fn close_section(&mut self, section: usize) {
        self.solid.close_section(section);
        self.cutout.close_section(section);
        self.translucent.close_section(section);
    }
}

pub struct Registry {
    models: Vec<Model>,
    by_variant: HashMap<(String, String), u32>,
    by_file: HashMap<String, u32>,
    static_states: Vec<Option<u32>>,
    missing: u32,
    grass: Vec<[u8; 3]>,
    foliage: Vec<[u8; 3]>,
    water: u32,
    lava: u32,
    pub textures: Vec<Texture>,
    /// Layers with any texel the terrain alpha test (0.1) discards.
    cutout: Vec<bool>,
    /// Variant models that failed to bake, and metadata states whose exact
    /// variant key is absent from the owned blockstates.
    pub model_failures: usize,
    pub inexact: Vec<String>,
    /// 128-pixel entity layers in `models::Texture` order, and font advances.
    pub entity_textures: Vec<Texture>,
    pub glyphs: [u8; 256],
    /// Interface sheets, fonts and language.
    pub gui: crate::ui::Resources,
    /// Looked up for every held and dropped item each frame: `Keys`, not SipHash.
    item_layers: HashMap<i16, u32, Keys>,
    overlays: HashMap<&'static str, u32, Keys>,
    /// Each item sprite layer's extruded mesh, by layer, from its
    /// opaque-pixel edges; `flat_item` for other layers.
    item_meshes: Vec<Option<ItemMesh>>,
    flat_item: ItemMesh,
    block_layers: Vec<u32>,
    /// Blocks whose item model is a separate `*_inventory` block model, by
    /// (id, metadata or 255 for any).
    inventory: HashMap<(u16, u8), u32>,
    /// Merge neighbouring full faces (`greedy`); off only to compare against.
    greedy: bool,
}

/// The ten crack stages a block shows while it is broken.
pub const DESTROY_STAGES: [&str; 10] = [
    "destroy_stage_0",
    "destroy_stage_1",
    "destroy_stage_2",
    "destroy_stage_3",
    "destroy_stage_4",
    "destroy_stage_5",
    "destroy_stage_6",
    "destroy_stage_7",
    "destroy_stage_8",
    "destroy_stage_9",
];

/// Item models that parent their own inventory block model rather than a
/// blockstate variant.
const INVENTORY_MODELS: [((u16, u8), &str); 16] = [
    ((29, 255), "piston_inventory_sticky"),
    ((33, 255), "piston_inventory_normal"),
    ((70, 255), "stone_pressure_plate_inventory"),
    ((77, 255), "stone_button_inventory"),
    ((85, 255), "oak_fence_inventory"),
    ((113, 255), "nether_brick_fence_inventory"),
    ((139, 1), "cobblestone_wall_mossy_inventory"),
    ((139, 255), "cobblestone_wall_inventory"),
    ((143, 255), "wooden_button_inventory"),
    ((147, 255), "light_pressure_plate_inventory"),
    ((148, 255), "heavy_pressure_plate_inventory"),
    ((188, 255), "spruce_fence_inventory"),
    ((189, 255), "birch_fence_inventory"),
    ((190, 255), "jungle_fence_inventory"),
    ((191, 255), "dark_oak_fence_inventory"),
    ((192, 255), "acacia_fence_inventory"),
];

/// Edges between opaque and transparent pixels of a 16-pixel sprite at the
/// top-left of a `size`-pixel layer.
fn sprite_edges(rgba: &[u8], size: usize) -> Vec<[u8; 3]> {
    let opaque = |x: i32, y: i32| {
        (0..16).contains(&x)
            && (0..16).contains(&y)
            && rgba
                .get(((y as usize) * size + x as usize) * 4 + 3)
                .is_some_and(|&a| a > 0)
    };
    let mut edges = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            if !opaque(x, y) {
                continue;
            }
            for (side, (dx, dy)) in [(-1, 0), (1, 0), (0, -1), (0, 1)].into_iter().enumerate() {
                if !opaque(x + dx, y + dy) {
                    edges.push([x as u8, y as u8, side as u8]);
                }
            }
        }
    }
    edges
}

/// Every corner of an extruded item mesh is a sprite pixel corner (17 x 17)
/// at the front or the back depth.
const ITEM_POINTS: usize = 17 * 17 * 2;
/// Item face shades: front, back and top; left and right; bottom.
const ITEM_SHADES: [f32; 3] = [1.0, 0.6, 0.5];

/// `ItemModelGenerator`'s extruded sprite in a unit cube, built once per
/// sprite layer: front at z = 8.5/16, back at 7.5/16, and a one-pixel side
/// wherever an opaque pixel meets a transparent one. Drawing only transforms
/// each distinct corner once and emits both windings, as items draw unculled.
struct ItemMesh {
    points: Vec<glam::Vec3>,
    quads: Vec<ItemQuad>,
}

struct ItemQuad {
    /// Indices into `ItemMesh::points`.
    corners: [u16; 4],
    /// Layer-scaled texture coordinates.
    uv: [[f32; 2]; 4],
    /// Index into `ITEM_SHADES`.
    shade: u8,
}

impl ItemMesh {
    /// The front, the back, then one side per `sprite_edges` entry.
    fn new(edges: &[[u8; 3]]) -> Self {
        let mut points = Vec::new();
        let mut quads = Vec::with_capacity(edges.len() + 2);
        let mut slots = [u16::MAX; ITEM_POINTS];
        let scale = 1.0 / crate::models::LAYER_SIZE as f32;
        // Corner (i, j, front): x = i / 16, y = 1 - j / 16, the same values
        // the per-pixel `x0`/`x1` and `y0`/`y1` give.
        let mut quad = |corners: [(u8, u8, bool); 4], uv: [[f32; 2]; 4], shade: u8| {
            let corners = corners.map(|(i, j, front)| {
                let slot =
                    &mut slots[(usize::from(front) * 17 + usize::from(j)) * 17 + usize::from(i)];
                if *slot == u16::MAX {
                    *slot = points.len() as u16;
                    points.push(glam::Vec3::new(
                        f32::from(i) / 16.0,
                        1.0 - f32::from(j) / 16.0,
                        if front { 8.5 / 16.0 } else { 7.5 / 16.0 },
                    ));
                }
                *slot
            });
            quads.push(ItemQuad {
                corners,
                uv: uv.map(|corner| corner.map(|v| v * scale)),
                shade,
            });
        };
        for front in [true, false] {
            quad(
                [
                    (0, 16, front),
                    (16, 16, front),
                    (16, 0, front),
                    (0, 0, front),
                ],
                [[0.0, 16.0], [16.0, 16.0], [16.0, 0.0], [0.0, 0.0]],
                0,
            );
        }
        for &[x, y, side] in edges {
            let (px, py) = (f32::from(x), f32::from(y));
            let uv = [
                [px + 0.01, py + 0.99],
                [px + 0.99, py + 0.99],
                [px + 0.99, py + 0.01],
                [px + 0.01, py + 0.01],
            ];
            let (left, right, top, bottom) = (x, x + 1, y, y + 1);
            let (corners, shade) = match side {
                0 => (
                    [
                        (left, bottom, false),
                        (left, bottom, true),
                        (left, top, true),
                        (left, top, false),
                    ],
                    1,
                ),
                1 => (
                    [
                        (right, bottom, true),
                        (right, bottom, false),
                        (right, top, false),
                        (right, top, true),
                    ],
                    1,
                ),
                2 => (
                    [
                        (left, top, true),
                        (right, top, true),
                        (right, top, false),
                        (left, top, false),
                    ],
                    0,
                ),
                _ => (
                    [
                        (left, bottom, false),
                        (right, bottom, false),
                        (right, bottom, true),
                        (left, bottom, true),
                    ],
                    2,
                ),
            };
            quad(corners, uv, shade);
        }
        Self { points, quads }
    }

    fn emit(&self, out: &mut Vec<Vertex>, layer: u32, matrix: glam::Mat4, color: [f32; 3]) {
        let mut world = [[0.0f32; 3]; ITEM_POINTS];
        for (slot, point) in world.iter_mut().zip(&self.points) {
            *slot = matrix.transform_point3(*point).to_array();
        }
        let colors = ITEM_SHADES.map(|shade| color.map(|c| c * shade));
        out.reserve(self.quads.len() * 12);
        for quad in &self.quads {
            let color = colors[usize::from(quad.shade)];
            out.extend([0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2].map(|i| Vertex {
                position: world[usize::from(quad.corners[i])],
                uv: quad.uv[i],
                layer,
                color,
                hurt: 0.0,
            }));
        }
    }
}

/// The extrusion `Registry::item_mesh` rebuilt from the edges on every call
/// before `ItemMesh`, kept as the reference the cached meshes must match.
#[cfg(test)]
fn item_mesh_reference(
    out: &mut Vec<Vertex>,
    edges: &[[u8; 3]],
    layer: u32,
    matrix: glam::Mat4,
    color: [f32; 3],
) {
    let scale = 1.0 / crate::models::LAYER_SIZE as f32;
    let mut quad = |corners: [[f32; 3]; 4], uv: [[f32; 2]; 4], shade: f32| {
        let corners = corners.map(|p| matrix.transform_point3(glam::Vec3::from(p)).to_array());
        for i in [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] {
            out.push(Vertex {
                position: corners[i],
                uv: uv[i].map(|v| v * scale),
                layer,
                color: color.map(|c| c * shade),
                hurt: 0.0,
            });
        }
    };
    let (front, back) = (8.5 / 16.0, 7.5 / 16.0);
    for z in [front, back] {
        quad(
            [[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]],
            [[0.0, 16.0], [16.0, 16.0], [16.0, 0.0], [0.0, 0.0]],
            1.0,
        );
    }
    for &[x, y, side] in edges {
        let (px, py) = (f32::from(x), f32::from(y));
        let (x0, x1) = (px / 16.0, (px + 1.0) / 16.0);
        let (y0, y1) = (1.0 - (py + 1.0) / 16.0, 1.0 - py / 16.0);
        let uv = [
            [px + 0.01, py + 0.99],
            [px + 0.99, py + 0.99],
            [px + 0.99, py + 0.01],
            [px + 0.01, py + 0.01],
        ];
        let (corners, shade) = match side {
            0 => (
                [
                    [x0, y0, back],
                    [x0, y0, front],
                    [x0, y1, front],
                    [x0, y1, back],
                ],
                0.6,
            ),
            1 => (
                [
                    [x1, y0, front],
                    [x1, y0, back],
                    [x1, y1, back],
                    [x1, y1, front],
                ],
                0.6,
            ),
            2 => (
                [
                    [x0, y1, front],
                    [x1, y1, front],
                    [x1, y1, back],
                    [x0, y1, back],
                ],
                1.0,
            ),
            _ => (
                [
                    [x0, y0, back],
                    [x1, y0, back],
                    [x1, y0, front],
                    [x0, y0, front],
                ],
                0.5,
            ),
        };
        quad(corners, uv, shade);
    }
}

/// Item sprites drawn in two tinted layers.
const OVERLAY_SPRITES: [&str; 15] = [
    "fishing_rod_cast",
    "bow_pulling_0",
    "bow_pulling_1",
    "bow_pulling_2",
    "potion_bottle_drinkable",
    "potion_bottle_splash",
    "potion_overlay",
    "leather_helmet",
    "leather_chestplate",
    "leather_leggings",
    "leather_boots",
    "leather_helmet_overlay",
    "leather_chestplate_overlay",
    "leather_leggings_overlay",
    "leather_boots_overlay",
];

fn direction(name: &str) -> [i32; 3] {
    match name {
        "down" => [0, -1, 0],
        "up" => [0, 1, 0],
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        _ => [1, 0, 0],
    }
}

/// Blocks whose `getActualState` reads neighbours. Others resolve through a
/// table built once from metadata alone.
fn neighbour_dependent(id: u16, meta: u8) -> bool {
    matches!(id, 2 | 110 | 55 | 104 | 105 | 106 | 131 | 132 | 139 | 175)
        || (id == 3 && meta == 2)
        || is_stairs(id)
        || is_door(id)
        || is_fence(id)
        || is_fence_gate(id)
        || is_pane(id)
}

/// `MathHelper.getPositionRandom`, with Java int multiplication overflow.
fn position_random(x: i32, y: i32, z: i32) -> i64 {
    let seed = i64::from(x.wrapping_mul(3_129_871))
        ^ i64::from(z).wrapping_mul(116_129_781)
        ^ i64::from(y);
    seed.wrapping_mul(seed)
        .wrapping_mul(42_317_861)
        .wrapping_add(seed.wrapping_mul(11))
}

/// `WorldProvider.generateLightBrightnessTable` for the overworld, then a
/// mid-gamma lightmap approximation. Rendering only; no simulation reads it.
/// The Fullbright mod: `gammaSetting` far past "Bright", as fullbright
/// mods set it, so every level renders near full light.
pub static FULLBRIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn brightness(level: u8) -> f32 {
    let f = 1.0 - f32::from(level.min(15)) / 15.0;
    let base = (1.0 - f) / (f * 3.0 + 1.0);
    if FULLBRIGHT.load(std::sync::atomic::Ordering::Relaxed) {
        // EntityRenderer.updateLightmap with gamma 10: the ambient floor,
        // then the gamma curve far past 1, clamped.
        let floor = base * 0.96 + 0.03;
        let lifted = 1.0 - (1.0 - floor).powi(4);
        let gamma = 10.0;
        return (floor * (1.0 - gamma) + lifted * gamma).clamp(0.0, 1.0) * 0.96 + 0.03;
    }
    let lifted = 1.0 - (1.0 - base).powi(4);
    (base * 0.5 + lifted * 0.5) * 0.96 + 0.03
}

#[derive(Clone, Copy)]
enum Climate {
    Normal(f32, f32),
    Swamp,
    Mesa,
    Roofed,
}

/// Base temperature and rainfall of the 1.8.9 biome identifiers. Mutated
/// variants (id + 128) share their parents' climate.
fn climate(id: u8) -> Climate {
    use Climate::Normal as N;
    match id & 127 {
        1 | 16 => N(0.8, 0.4),
        2 | 8 | 17 => N(2.0, 0.0),
        3 | 20 | 25 | 34 => N(0.2, 0.3),
        4 | 18 => N(0.7, 0.8),
        5 | 19 => N(0.25, 0.8),
        6 => Climate::Swamp,
        10..=13 => N(0.0, 0.5),
        14 | 15 => N(0.9, 1.0),
        21 | 22 => N(0.95, 0.9),
        23 => N(0.95, 0.8),
        26 => N(0.05, 0.3),
        27 | 28 => N(0.6, 0.6),
        29 => Climate::Roofed,
        30 | 31 => N(-0.5, 0.4),
        32 | 33 => N(0.3, 0.8),
        35 => N(1.2, 0.0),
        36 => N(1.0, 0.0),
        37..=39 => Climate::Mesa,
        _ => N(0.5, 0.5),
    }
}

fn hex(color: u32) -> [f32; 3] {
    [
        ((color >> 16) & 255) as f32 / 255.0,
        ((color >> 8) & 255) as f32 / 255.0,
        (color & 255) as f32 / 255.0,
    ]
}

fn split(color: u32) -> [u8; 3] {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8]
}

/// Whether the fragment alpha test (`a < 0.1`) can discard any texel.
fn has_cutout(rgba: &[u8]) -> bool {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .any(|texel| f32::from(texel[3]) / 255.0 < 0.1)
}

fn first_frame(image: &Texture) -> Option<Vec<u8>> {
    (image.width == 16 && image.height >= 16).then(|| image.rgba[..1024].to_vec())
}

fn face_shade(name: &str) -> f32 {
    match name {
        "up" => 1.0,
        "down" => 0.5,
        "north" | "south" => 0.8,
        _ => 0.6,
    }
}

impl Registry {
    pub fn open(jar: &Path) -> Result<Self, Box<dyn Error>> {
        let assets = Assets::open(jar)?;
        let mut missing = vec![0; 16 * 16 * 4];
        for (index, pixel) in missing.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let light = ((index % 16) / 8 + (index / 16) / 8) % 2 == 0;
            *pixel = if light {
                [255, 0, 255, 255]
            } else {
                [0, 0, 0, 255]
            };
        }
        let mut textures = vec![Texture {
            width: 16,
            height: 16,
            rgba: missing,
        }];
        let mut layers: BTreeMap<String, u32> = BTreeMap::new();
        let mut layer_for = |sprite: &str, textures: &mut Vec<Texture>| -> u32 {
            if let Some(&layer) = layers.get(sprite) {
                return layer;
            }
            let layer = match assets.texture(sprite).ok().as_ref().and_then(first_frame) {
                Some(rgba) => {
                    textures.push(Texture {
                        width: 16,
                        height: 16,
                        rgba,
                    });
                    textures.len() as u32 - 1
                }
                None => {
                    eprintln!("LEGACY_TEXTURE_MISSING sprite={sprite}");
                    0
                }
            };
            layers.insert(sprite.to_owned(), layer);
            layer
        };
        let mut models = Vec::new();
        let mut by_variant = HashMap::new();
        let mut by_file = HashMap::new();
        let mut failures = 0usize;
        for (file, variants) in assets.blockstates()? {
            for (key, set) in variants {
                let choices = match set {
                    VariantSet::Single(single) => vec![(single, 1)],
                    VariantSet::Weighted(weighted) => weighted,
                };
                let particle = choices
                    .first()
                    .and_then(|(choice, _)| assets.particle_sprite(&choice.model).ok().flatten())
                    .map_or(0, |sprite| layer_for(&sprite, &mut textures));
                let mut alternatives = Vec::with_capacity(choices.len());
                for (choice, weight) in choices {
                    match assets.bake_variant(&choice.model, choice.rotations, choice.uv_lock) {
                        Ok(quads) => {
                            let faces = quads
                                .into_iter()
                                .map(|quad| {
                                    Face::baked(
                                        quad.positions,
                                        quad.uv,
                                        layer_for(&quad.material.sprite, &mut textures),
                                        if quad.shade {
                                            face_shade(quad.facing.name())
                                        } else {
                                            1.0
                                        },
                                        quad.tint_index >= 0,
                                        quad.cull.map(|side| direction(side.name())),
                                    )
                                })
                                .collect();
                            alternatives.push((faces, weight));
                        }
                        Err(error) => {
                            failures += 1;
                            eprintln!(
                                "LEGACY_MODEL_FAILED file={file} variant={key} error={error}"
                            );
                        }
                    }
                }
                if alternatives.is_empty() {
                    continue;
                }
                let total = alternatives.iter().map(|(_, weight)| weight).sum();
                let index = models.len() as u32;
                models.push(Model {
                    particle,
                    alternatives,
                    total,
                });
                by_file.entry(file.clone()).or_insert(index);
                by_variant.insert((file.clone(), key), index);
            }
        }
        let stone = by_variant
            .get(&("stone".to_owned(), "normal".to_owned()))
            .copied()
            .ok_or("Owned assets lack the stone blockstate")?;
        let mut placeholder = models[stone as usize].alternatives[0].0.clone();
        for face in &mut placeholder {
            face.layer = 0;
        }
        let missing = models.len() as u32;
        models.push(Model {
            particle: 0,
            alternatives: vec![(placeholder, 1)],
            total: 1,
        });
        let mut inventory = HashMap::new();
        for (key, name) in INVENTORY_MODELS {
            let Ok(quads) = assets.bake_block(name) else {
                continue;
            };
            let faces = quads
                .into_iter()
                .map(|quad| {
                    Face::baked(
                        quad.positions,
                        quad.uv,
                        layer_for(&quad.material.sprite, &mut textures),
                        if quad.shade {
                            face_shade(quad.facing.name())
                        } else {
                            1.0
                        },
                        quad.tint_index >= 0,
                        quad.cull.map(|side| direction(side.name())),
                    )
                })
                .collect();
            inventory.insert(key, models.len() as u32);
            models.push(Model {
                particle: assets
                    .particle_sprite(name)
                    .ok()
                    .flatten()
                    .map_or(0, |sprite| layer_for(&sprite, &mut textures)),
                alternatives: vec![(faces, 1)],
                total: 1,
            });
        }
        let colormap = |name: &str| -> Result<Vec<[u8; 3]>, Box<dyn Error>> {
            let image = assets.texture(name)?;
            if image.width != 256 || image.height != 256 {
                return Err(format!("{name} must be 256x256").into());
            }
            Ok(image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&[r, g, b, _]| [r, g, b])
                .collect())
        };
        let size = crate::models::LAYER_SIZE as usize;
        let mut entity_textures = vec![Texture {
            width: size as u32,
            height: size as u32,
            rgba: vec![255; size * size * 4],
        }];
        let mut glyphs = [6u8; 256];
        for source in crate::models::SOURCES {
            let mut canvas = vec![0u8; size * size * 4];
            match assets.texture(source) {
                Ok(image) => {
                    let (w, h) = (image.width as usize, image.height as usize);
                    if source.ends_with("entity/explosion") {
                        // Preserve all sixteen cells of the 256px sheet in a 128px layer.
                        for y in 0..size {
                            for x in 0..size {
                                let from = ((y * h / size) * w + x * w / size) * 4;
                                let to = (y * size + x) * 4;
                                canvas[to..to + 4].copy_from_slice(&image.rgba[from..from + 4]);
                            }
                        }
                    } else {
                        for y in 0..h.min(size) {
                            let row = &image.rgba[y * w * 4..(y * w + w.min(size)) * 4];
                            canvas[y * size * 4..y * size * 4 + row.len()].copy_from_slice(row);
                        }
                    }
                    if source.ends_with("font/ascii") {
                        glyphs = crate::models::glyph_widths(&image.rgba, image.width);
                    }
                }
                Err(error) => eprintln!("LEGACY_ENTITY_TEXTURE_MISSING {source} {error}"),
            }
            entity_textures.push(Texture {
                width: size as u32,
                height: size as u32,
                rgba: canvas,
            });
        }
        let mut block_layers = Vec::with_capacity(textures.len());
        for texture in &textures {
            block_layers.push(entity_textures.len() as u32);
            let mut canvas = vec![0u8; size * size * 4];
            for y in 0..16usize {
                canvas[y * size * 4..y * size * 4 + 64]
                    .copy_from_slice(&texture.rgba[y * 64..y * 64 + 64]);
            }
            entity_textures.push(Texture {
                width: size as u32,
                height: size as u32,
                rgba: canvas,
            });
        }
        let mut item_layers = HashMap::default();
        for &(id, name) in crate::items::ITEMS {
            if let Ok(image) = assets.item_texture(name) {
                let w = image.width.min(size as u32) as usize;
                let h = image.height.min(size as u32) as usize;
                let mut canvas = vec![0u8; size * size * 4];
                for y in 0..h {
                    let start = y * image.width as usize * 4;
                    canvas[y * size * 4..y * size * 4 + w * 4]
                        .copy_from_slice(&image.rgba[start..start + w * 4]);
                }
                item_layers.insert(id, entity_textures.len() as u32);
                entity_textures.push(Texture {
                    width: size as u32,
                    height: size as u32,
                    rgba: canvas,
                });
            }
        }
        // Two-layer item sprites the interface tints: potions and leather.
        let mut overlays = HashMap::default();
        for name in OVERLAY_SPRITES {
            if let Ok(image) = assets.texture(&format!("minecraft:items/{name}")) {
                let w = image.width.min(size as u32) as usize;
                let h = image.height.min(size as u32) as usize;
                let mut canvas = vec![0u8; size * size * 4];
                for y in 0..h {
                    let start = y * image.width as usize * 4;
                    canvas[y * size * 4..y * size * 4 + w * 4]
                        .copy_from_slice(&image.rgba[start..start + w * 4]);
                }
                overlays.insert(name, entity_textures.len() as u32);
                entity_textures.push(Texture {
                    width: size as u32,
                    height: size as u32,
                    rgba: canvas,
                });
            }
        }
        let mut item_meshes = Vec::new();
        for &layer in item_layers.values().chain(overlays.values()) {
            let layer = layer as usize;
            if item_meshes.len() <= layer {
                item_meshes.resize_with(layer + 1, || None);
            }
            item_meshes[layer] = Some(ItemMesh::new(&sprite_edges(
                &entity_textures[layer].rgba,
                size,
            )));
        }
        // `RenderGlobal.destroyBlockIcons`, drawn over blocks being broken.
        for name in DESTROY_STAGES {
            if let Ok(image) = assets.texture(&format!("minecraft:blocks/{name}")) {
                let mut canvas = vec![0u8; size * size * 4];
                for y in 0..16usize.min(image.height as usize) {
                    let start = y * image.width as usize * 4;
                    canvas[y * size * 4..y * size * 4 + 64]
                        .copy_from_slice(&image.rgba[start..start + 64]);
                }
                overlays.insert(name, entity_textures.len() as u32);
                entity_textures.push(Texture {
                    width: size as u32,
                    height: size as u32,
                    rgba: canvas,
                });
            }
        }
        // `RenderItem.RES_ITEM_GLINT`, sampled with wrapping by the glint pass.
        if let Ok(image) = assets.texture("minecraft:misc/enchanted_item_glint") {
            let w = image.width.min(size as u32) as usize;
            let h = image.height.min(size as u32) as usize;
            let mut canvas = vec![0u8; size * size * 4];
            for y in 0..h {
                let start = y * image.width as usize * 4;
                canvas[y * size * 4..y * size * 4 + w * 4]
                    .copy_from_slice(&image.rgba[start..start + w * 4]);
            }
            overlays.insert("enchanted_item_glint", entity_textures.len() as u32);
            entity_textures.push(Texture {
                width: size as u32,
                height: size as u32,
                rgba: canvas,
            });
        }
        let grass = colormap("minecraft:colormap/grass")?;
        let foliage = colormap("minecraft:colormap/foliage")?;
        let water = layer_for("minecraft:blocks/water_still", &mut textures);
        let lava = layer_for("minecraft:blocks/lava_still", &mut textures);
        if textures.len() > TerrainVertex::LAYERS as usize {
            return Err("Terrain texture layers exceed the packed vertex format".into());
        }
        let gui = crate::ui::Resources::load(&assets)?;
        let mut registry = Self {
            models,
            by_variant,
            by_file,
            static_states: vec![None; 198 * 16],
            missing,
            grass,
            foliage,
            water,
            lava,
            cutout: textures
                .iter()
                .map(|texture| has_cutout(&texture.rgba))
                .collect(),
            textures,
            model_failures: failures,
            inexact: Vec::new(),
            entity_textures,
            glyphs,
            gui,
            item_meshes,
            flat_item: ItemMesh::new(&[]),
            item_layers,
            overlays,
            block_layers,
            inventory,
            greedy: true,
        };
        let empty = World::default();
        let mut unresolved = Vec::new();
        for raw in 0..198u16 * 16 {
            let state = BlockState(raw);
            if neighbour_dependent(state.block_id(), state.metadata()) {
                continue;
            }
            if let Some(found) = variant(&empty, [0, 64, 0], state) {
                let exact = registry
                    .by_variant
                    .get(&(found.file.clone(), found.key.clone()))
                    .copied();
                if exact.is_none() {
                    unresolved.push(format!("{}:{}", state.block_id(), state.metadata()));
                }
                registry.static_states[usize::from(raw)] = Some(registry.lookup(&found));
            }
        }
        eprintln!(
            "LEGACY_REGISTRY_READY variants={} texture_layers={} model_failures={} inexact_states={}",
            registry.by_variant.len(),
            registry.textures.len(),
            registry.model_failures,
            unresolved.len(),
        );
        registry.inexact = unresolved;
        Ok(registry)
    }

    /// Exact variant, else the file's first variant, else the placeholder.
    fn lookup(&self, found: &Variant) -> u32 {
        if let Some(&model) = self
            .by_variant
            .get(&(found.file.clone(), found.key.clone()))
        {
            return model;
        }
        self.by_file
            .get(&found.file)
            .copied()
            .unwrap_or(self.missing)
    }

    pub fn resolve(&self, world: &World, pos: [i32; 3], state: BlockState) -> Option<u32> {
        let id = state.block_id();
        if usize::from(id) >= 198 {
            return Some(self.missing);
        }
        if !neighbour_dependent(id, state.metadata()) {
            return self.static_states[usize::from(state.0)];
        }
        variant(world, pos, state).map(|found| self.lookup(&found))
    }

    /// A named item sprite for layered icons (see `OVERLAY_SPRITES`).
    pub fn sprite(&self, name: &str) -> Option<u32> {
        self.overlays.get(name).copied()
    }

    pub fn item_layer(&self, item: &horizon_protocol_47::nbt::Item) -> Option<u32> {
        if item.id >= 256 {
            return self.item_layers.get(&item.id).copied();
        }
        let state = BlockState(((item.id as u16) << 4) | (item.damage as u16 & 15));
        let model = self.resolve(&World::default(), [0; 3], state)?;
        let face = self.models[model as usize]
            .alternatives
            .first()?
            .0
            .first()?;
        self.block_layers.get(face.layer as usize).copied()
    }

    pub fn held_block(
        &self,
        out: &mut Vec<Vertex>,
        item: &horizon_protocol_47::nbt::Item,
        matrix: glam::Mat4,
    ) {
        if (1..198).contains(&item.id) {
            let state = BlockState(((item.id as u16) << 4) | (item.damage as u16 & 15));
            self.block_model(out, state, matrix, 1.0);
        }
    }

    /// `EntityDiggingFX` uses the inherited model particle slot and leaf tint.
    pub fn particle(&self, state: BlockState) -> Option<(u32, [f32; 3])> {
        let model = self.resolve(&World::default(), [0; 3], state)?;
        let tint = match (state.block_id(), state.metadata() & 3) {
            (18, 1) => [0x61, 0x99, 0x61],
            (18, 2) => [0x80, 0xA7, 0x55],
            (18 | 161, _) => [0x48u8, 0xB5, 0x18],
            _ => [0xFF; 3],
        }
        .map(|c| f32::from(c) / 255.0);
        Some((
            self.block_layers[self.models[model as usize].particle as usize],
            tint,
        ))
    }

    /// A block state's inventory model centred on the origin (one block
    /// wide), with item tints: the default grass and leaf colours.
    pub fn block_model(
        &self,
        out: &mut Vec<Vertex>,
        state: BlockState,
        matrix: glam::Mat4,
        light: f32,
    ) -> bool {
        let (id, meta) = (state.block_id(), state.metadata());
        let Some(model) = self
            .inventory
            .get(&(id, meta))
            .or_else(|| self.inventory.get(&(id, 255)))
            .copied()
            .or_else(|| self.resolve(&World::default(), [0; 3], state))
        else {
            return false;
        };
        let tint = match (state.block_id(), state.metadata() & 3) {
            (18, 1) => [0x61, 0x99, 0x61],
            (18, 2) => [0x80, 0xA7, 0x55],
            (18 | 161, _) => [0x48, 0xB5, 0x18],
            // ColorizerGrass.getGrassColor(0.5, 1.0).
            _ => self.grass[127 * 256 + 127],
        }
        .map(|c| f32::from(c) / 255.0);
        for face in &self.models[model as usize].alternatives[0].0 {
            let color = if face.tint {
                tint.map(|c| c * face.shade * light)
            } else {
                [face.shade * light; 3]
            };
            for i in [0, 1, 2, 0, 2, 3] {
                out.push(Vertex {
                    position: matrix
                        .transform_point3(
                            glam::Vec3::from_array(face.positions[i]) - glam::Vec3::splat(0.5),
                        )
                        .to_array(),
                    uv: face.uv[i].map(|v| v * 16.0 / crate::models::LAYER_SIZE as f32),
                    layer: self.block_layers[face.layer as usize],
                    color,
                    hurt: 0.0,
                });
            }
        }
        true
    }

    /// `ItemModelGenerator`'s extruded sprite under `matrix` (`ItemMesh`). A
    /// layer without a cached mesh draws only its front and back.
    pub fn item_mesh(
        &self,
        out: &mut Vec<Vertex>,
        layer: u32,
        matrix: glam::Mat4,
        color: [f32; 3],
    ) {
        self.item_meshes
            .get(layer as usize)
            .and_then(Option::as_ref)
            .unwrap_or(&self.flat_item)
            .emit(out, layer, matrix, color);
    }

    pub fn stone_fixture(&self) -> Vec<Vertex> {
        let mut vertices = Vec::new();
        let model = self
            .by_variant
            .get(&("stone".to_owned(), "normal".to_owned()))
            .copied()
            .unwrap_or(self.missing);
        for face in &self.models[model as usize].alternatives[0].0 {
            for index in [0, 1, 2, 0, 2, 3] {
                vertices.push(Vertex {
                    position: face.positions[index],
                    uv: face.uv[index],
                    layer: face.layer,
                    color: [face.shade; 3],
                    hurt: 0.0,
                });
            }
        }
        vertices
    }

    /// `BiomeColorHelper`: the average over the 3x3 columns around a block.
    fn biome_average(&self, world: &World, x: i32, z: i32, grass: bool) -> [f32; 3] {
        let mut sum = [0u32; 3];
        let mut count = 0;
        for dx in -1..=1 {
            for dz in -1..=1 {
                let (bx, bz) = (x.wrapping_add(dx), z.wrapping_add(dz));
                let Some(biome) =
                    world
                        .column(bx.div_euclid(16), bz.div_euclid(16))
                        .and_then(|column| {
                            column.biome(bx.rem_euclid(16) as u8, bz.rem_euclid(16) as u8)
                        })
                else {
                    continue;
                };
                let color = self.biome_color(biome, grass);
                for (total, channel) in sum.iter_mut().zip(color) {
                    *total += u32::from(channel);
                }
                count += 1;
            }
        }
        if count == 0 {
            return hex(if grass { 0x91BD59 } else { 0x77AB2F });
        }
        sum.map(|value| (value / count) as f32 / 255.0)
    }

    /// `ColorizerGrass` / `ColorizerFoliage` lookups plus the biome overrides.
    fn biome_color(&self, biome: u8, grass: bool) -> [u8; 3] {
        let sample = |temperature: f64, rainfall: f64| {
            let temperature = temperature.clamp(0.0, 1.0);
            let rainfall = rainfall.clamp(0.0, 1.0) * temperature;
            let i = ((1.0 - temperature) * 255.0) as usize;
            let j = ((1.0 - rainfall) * 255.0) as usize;
            let map = if grass { &self.grass } else { &self.foliage };
            map.get((j << 8) | i).copied().unwrap_or([255, 0, 255])
        };
        match climate(biome) {
            Climate::Normal(temperature, rainfall) => {
                sample(f64::from(temperature), f64::from(rainfall))
            }
            Climate::Swamp => split(0x6A7039),
            Climate::Mesa => split(if grass { 0x90814D } else { 0x9E814D }),
            Climate::Roofed => {
                let base = sample(0.7, 0.8);
                if grass {
                    let packed =
                        (u32::from(base[0]) << 16) | (u32::from(base[1]) << 8) | u32::from(base[2]);
                    split(((packed & 0xFEFEFE) + 0x28340A) >> 1)
                } else {
                    base
                }
            }
        }
    }

    fn tint(&self, world: &World, pos: [i32; 3], state: BlockState) -> [f32; 3] {
        let meta = state.metadata();
        match state.block_id() {
            2 | 31 | 83 | 175 => self.biome_average(world, pos[0], pos[2], true),
            18 if meta & 3 == 1 => hex(0x619961),
            18 if meta & 3 == 2 => hex(0x80A755),
            18 | 161 | 106 => self.biome_average(world, pos[0], pos[2], false),
            111 => hex(0x208030),
            104 | 105 => {
                let age = u32::from(meta & 7);
                hex(((age * 32) << 16) | ((255 - age * 8) << 8) | (age * 4))
            }
            55 => {
                let f = f32::from(meta) / 15.0;
                let red = if meta == 0 { 0.3 } else { f * 0.6 + 0.4 };
                [
                    red,
                    (f * f * 0.7 - 0.5).max(0.0),
                    (f * f * 0.6 - 0.7).max(0.0),
                ]
            }
            _ => [1.0; 3],
        }
    }

    fn culls(&self, world: &World, state: BlockState, neighbour: [i32; 3]) -> bool {
        let Some(other) = world.block(neighbour[0], neighbour[1], neighbour[2]) else {
            // Unloaded neighbour columns: skip faces nobody can see yet.
            return (0..256).contains(&neighbour[1]);
        };
        let id = state.block_id();
        block(other.block_id()).opaque_cube()
            || (other.block_id() == id
                && match id {
                    20 | 79 | 165 => true,
                    95 | 160 => other.metadata() == state.metadata(),
                    _ => false,
                })
    }

    pub fn mesh_column(&self, world: &World, chunk: (i32, i32), origin: [i64; 3]) -> ColumnMesh {
        let mut mesh = ColumnMesh {
            visibility: [ALL_VISIBLE; 16],
            ..ColumnMesh::default()
        };
        let Some(column) = world.column(chunk.0, chunk.1) else {
            return mesh;
        };
        let mut merger = crate::greedy::Merger::default();
        // A block's visible faces: face, colour, pass (0 solid, 1 cutout, 2 translucent).
        let mut visible: Vec<(&Face, [f32; 3], u8)> = Vec::new();
        for section_y in 0..16 {
            let Some(section) = column.section(section_y) else {
                mesh.close_section(section_y);
                continue;
            };
            let mut states = [0u16; 4096];
            section.states().unpack(&mut states);
            mesh.visibility[section_y] =
                visibility(|index| block(BlockState(states[index]).block_id()).opaque_cube());
            for (index, &raw) in states.iter().enumerate() {
                if raw == 0 {
                    continue;
                }
                let state = BlockState(raw);
                let x = chunk.0.wrapping_mul(16).wrapping_add((index & 15) as i32);
                let y = (section_y * 16 + (index >> 8)) as i32;
                let z = chunk
                    .1
                    .wrapping_mul(16)
                    .wrapping_add(((index >> 4) & 15) as i32);
                let pos = [x, y, z];
                let offset = [
                    (i64::from(x) - origin[0]) as f32,
                    (i64::from(y) - origin[1]) as f32,
                    (i64::from(z) - origin[2]) as f32,
                ];
                let local = [(index & 15) as f32, y as f32, ((index >> 4) & 15) as f32];
                let info = block(state.block_id());
                if info.layer == Layer::Liquid {
                    self.liquid(world, pos, state, local, &mut mesh);
                    continue;
                }
                if self.block_entity(world, pos, state, offset, &mut mesh.entity) {
                    continue;
                }
                let Some(model) = self.resolve(world, pos, state) else {
                    continue;
                };
                let model = &self.models[model as usize];
                let faces = if model.alternatives.len() == 1 {
                    &model.alternatives[0].0
                } else {
                    let seed = position_random(x, y, z);
                    let mut pick = ((seed as i32) >> 16).wrapping_abs() % model.total;
                    let mut chosen = &model.alternatives[0].0;
                    for (faces, weight) in &model.alternatives {
                        pick -= weight;
                        if pick < 0 {
                            chosen = faces;
                            break;
                        }
                    }
                    chosen
                };
                let translucent = info.layer == Layer::Translucent;
                let mut tint = None;
                visible.clear();
                for face in faces {
                    let light_pos = match face.cull {
                        Some([dx, dy, dz]) => {
                            let neighbour = [x.wrapping_add(dx), y + dy, z.wrapping_add(dz)];
                            if self.culls(world, state, neighbour) {
                                continue;
                            }
                            neighbour
                        }
                        None => pos,
                    };
                    let level = light_level(world, light_pos);
                    let light = brightness(level);
                    let color = if face.tint {
                        *tint.get_or_insert_with(|| self.tint(world, pos, state))
                    } else {
                        [1.0; 3]
                    };
                    let pass = if translucent {
                        2
                    } else if self
                        .cutout
                        .get(face.layer as usize)
                        .copied()
                        .unwrap_or(false)
                    {
                        1
                    } else {
                        0
                    };
                    visible.push((
                        face,
                        color.map(|channel| channel * face.shade * light),
                        pass,
                    ));
                }
                // A block's faces in a pass wait for merging only when all of them can, so
                // faces on one plane keep their order (an overlay after its base).
                let whole = |pass: u8| {
                    visible
                        .iter()
                        .all(|(face, _, of)| *of != pass || face.grid.is_some())
                };
                let defer = [self.greedy && whole(0), self.greedy && whole(1)];
                merger.next_block();
                for &(face, color, pass) in &visible {
                    if let Some(grid) = face.grid.filter(|_| pass < 2 && defer[usize::from(pass)]) {
                        merger.push(
                            pass,
                            grid,
                            &face.positions,
                            &face.uv,
                            face.layer,
                            color,
                            local,
                        );
                        continue;
                    }
                    let target = match pass {
                        0 => &mut mesh.solid,
                        1 => &mut mesh.cutout,
                        _ => &mut mesh.translucent,
                    };
                    append(
                        &mut target.vertices,
                        face.positions,
                        face.uv,
                        face.layer,
                        color,
                        local,
                    );
                }
            }
            merger.flush(&mut mesh.solid, &mut mesh.cutout);
            mesh.close_section(section_y);
        }
        // Translucent faces keep their order (blending) and are drawn two-sided.
        mesh.solid.sort_by_facing();
        mesh.cutout.sort_by_facing();
        mesh
    }

    /// Tile-entity-rendered blocks; returns whether the block was one.
    fn block_entity(
        &self,
        world: &World,
        [x, y, z]: [i32; 3],
        state: BlockState,
        offset: [f32; 3],
        out: &mut Vec<Vertex>,
    ) -> bool {
        use crate::models::{self, ChestKind};
        use horizon_world_189::block_entities::BlockEntity;
        let id = state.block_id();
        let meta = state.metadata();
        if !matches!(id, 54 | 63 | 68 | 130 | 144 | 146 | 176 | 177) {
            return false;
        }
        let at = glam::Vec3::from(offset);
        let light = brightness(light_level(world, [x, y, z]));
        let same = |dx: i32, dz: i32| {
            world
                .block(x.wrapping_add(dx), y, z.wrapping_add(dz))
                .is_some_and(|other| other.block_id() == id)
        };
        match id {
            54 | 146 => {
                // The negative-side half of a double chest draws nothing.
                if !(same(-1, 0) || same(0, -1)) {
                    let double = if same(1, 0) {
                        Some(true)
                    } else if same(0, 1) {
                        Some(false)
                    } else {
                        None
                    };
                    let kind = if id == 54 {
                        ChestKind::Normal
                    } else {
                        ChestKind::Trapped
                    };
                    models::chest(out, at, meta, kind, double, light);
                }
            }
            130 => models::chest(out, at, meta, ChestKind::Ender, None, light),
            63 | 68 => {
                let lines = match world.block_entity([x, y, z]) {
                    Some(BlockEntity::Sign(lines)) => lines.clone(),
                    _ => Default::default(),
                };
                models::sign(out, at, id == 63, meta, &lines, &self.glyphs, light);
            }
            144 => {
                let (kind, rotation) = match world.block_entity([x, y, z]) {
                    Some(&BlockEntity::Skull { kind, rotation }) => (kind, rotation),
                    _ => (0, 0),
                };
                models::skull(out, at, meta & 7, kind, rotation, light);
            }
            _ => {
                let base = match world.block_entity([x, y, z]) {
                    Some(&BlockEntity::Banner { base }) => base,
                    _ => 15,
                };
                models::banner(out, at, id == 176, meta, base, light);
            }
        }
        true
    }

    fn liquid(
        &self,
        world: &World,
        [x, y, z]: [i32; 3],
        state: BlockState,
        offset: [f32; 3],
        mesh: &mut ColumnMesh,
    ) {
        let water = block(state.block_id()).water();
        let neighbour =
            |dx: i32, dy: i32, dz: i32| world.block(x.wrapping_add(dx), y + dy, z.wrapping_add(dz));
        let same = |dx: i32, dy: i32, dz: i32| {
            neighbour(dx, dy, dz).is_some_and(|other| {
                let other = block(other.block_id());
                if water { other.water() } else { other.lava() }
            })
        };
        let level = state.metadata();
        let height = if same(0, 1, 0) {
            1.0
        } else {
            1.0 - f32::from(if level >= 8 { 0 } else { level } + 1) / 9.0
        };
        let (layer, target) = if water {
            (self.water, &mut mesh.translucent)
        } else if self
            .cutout
            .get(self.lava as usize)
            .copied()
            .unwrap_or(false)
        {
            (self.lava, &mut mesh.cutout)
        } else {
            (self.lava, &mut mesh.solid)
        };
        let light = if water {
            brightness(light_level(world, [x, y + 1, z]))
        } else {
            1.0
        };
        let uv = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
        let h = height;
        let faces: [([i32; 3], [[f32; 3]; 4], f32); 6] = [
            (
                [0, 1, 0],
                [[0.0, h, 0.0], [0.0, h, 1.0], [1.0, h, 1.0], [1.0, h, 0.0]],
                1.0,
            ),
            (
                [0, -1, 0],
                [
                    [0.0, 0.0, 1.0],
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [1.0, 0.0, 1.0],
                ],
                0.5,
            ),
            (
                [0, 0, -1],
                [
                    [1.0, h, 0.0],
                    [1.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0],
                    [0.0, h, 0.0],
                ],
                0.8,
            ),
            (
                [0, 0, 1],
                [
                    [0.0, h, 1.0],
                    [0.0, 0.0, 1.0],
                    [1.0, 0.0, 1.0],
                    [1.0, h, 1.0],
                ],
                0.8,
            ),
            (
                [-1, 0, 0],
                [
                    [0.0, h, 0.0],
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [0.0, h, 1.0],
                ],
                0.6,
            ),
            (
                [1, 0, 0],
                [
                    [1.0, h, 1.0],
                    [1.0, 0.0, 1.0],
                    [1.0, 0.0, 0.0],
                    [1.0, h, 0.0],
                ],
                0.6,
            ),
        ];
        for ([dx, dy, dz], positions, shade) in faces {
            let skip = same(dx, dy, dz)
                || (dy != 1
                    && neighbour(dx, dy, dz)
                        .is_none_or(|other| block(other.block_id()).opaque_cube()));
            if !skip {
                append(
                    &mut target.vertices,
                    positions,
                    uv,
                    layer,
                    [shade * light; 3],
                    offset,
                );
            }
        }
    }
}

/// `World.getCombinedLight` for flat shading: blocks with
/// `useNeighborBrightness` (half slabs, stairs, farmland) are light-opaque and
/// store zero, so their cell reads the brightest of above and the four sides.
fn light_level(world: &World, [x, y, z]: [i32; 3]) -> u8 {
    let at = |dx: i32, dy: i32, dz: i32| {
        let (block_light, sky_light) = world.light(x.wrapping_add(dx), y + dy, z.wrapping_add(dz));
        block_light.max(sky_light)
    };
    let id = world.block(x, y, z).unwrap_or_default().block_id();
    if matches!(id, 44 | 126 | 182 | 60) || is_stairs(id) {
        [
            at(0, 1, 0),
            at(1, 0, 0),
            at(-1, 0, 0),
            at(0, 0, 1),
            at(0, 0, -1),
        ]
        .into_iter()
        .max()
        .unwrap_or(15)
    } else {
        at(0, 0, 0)
    }
}

/// One quad's four corners; the shared index list makes its two triangles
/// (0, 1, 2) and (0, 2, 3).
fn append(
    vertices: &mut Vec<TerrainVertex>,
    positions: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    layer: u32,
    color: [f32; 3],
    offset: [f32; 3],
) {
    for index in 0..4 {
        vertices.push(TerrainVertex::new(
            std::array::from_fn(|axis| positions[index][axis] + offset[axis]),
            uv[index],
            layer,
            color,
        ));
    }
}

/// Index list for `quads` quads drawn from four-corner runs.
pub fn quad_indices(quads: u32) -> Vec<u32> {
    (0..quads)
        .flat_map(|quad| [0, 1, 2, 0, 2, 3].map(|corner| quad * 4 + corner))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16 x 16 sprite in a `LAYER_SIZE` canvas, opaque where `mask` says.
    fn sprite(mask: impl Fn(usize, usize) -> bool) -> Vec<u8> {
        let size = crate::models::LAYER_SIZE as usize;
        let mut rgba = vec![0u8; size * size * 4];
        for y in 0..16 {
            for x in 0..16 {
                if mask(x, y) {
                    rgba[(y * size + x) * 4..(y * size + x) * 4 + 4]
                        .copy_from_slice(&[200, 100, 50, 255]);
                }
            }
        }
        rgba
    }

    /// Item transforms as the hand, held and dropped item paths build them,
    /// plus mirrored, non-uniformly scaled and far-from-origin ones.
    fn item_matrices() -> Vec<glam::Mat4> {
        use glam::{Mat4, Quat, Vec3};
        let mut matrices = vec![
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::splat(-0.5)),
            Mat4::from_scale(Vec3::splat(2.0))
                * Mat4::from_translation(Vec3::new(0.0, 0.25, 0.125))
                * Mat4::from_rotation_y((-135f32).to_radians())
                * Mat4::from_rotation_z(25f32.to_radians())
                * Mat4::from_scale(Vec3::splat(1.7 * 0.5))
                * Mat4::from_translation(Vec3::splat(-0.5)),
            Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
                * Mat4::from_rotation_x(1.3)
                * Mat4::from_translation(Vec3::new(1234.5, 70.25, -987.125)),
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
        };
        for _ in 0..24 {
            matrices.push(Mat4::from_scale_rotation_translation(
                Vec3::new(next() * 3.0, next() * 3.0, next() * 3.0),
                Quat::from_euler(
                    glam::EulerRot::YXZ,
                    next() * 4.0,
                    next() * 4.0,
                    next() * 4.0,
                ),
                Vec3::new(next() * 300.0, next() * 100.0, next() * 300.0),
            ));
        }
        matrices
    }

    fn assert_same_vertices(cached: &[Vertex], reference: &[Vertex], what: &str) {
        assert_eq!(cached.len(), reference.len(), "{what}: vertex count");
        for (index, (a, b)) in cached.iter().zip(reference).enumerate() {
            assert_eq!(
                bytemuck::bytes_of(a),
                bytemuck::bytes_of(b),
                "{what}: vertex {index}: {a:?} != {b:?}"
            );
        }
    }

    #[test]
    fn cached_item_meshes_emit_the_per_call_extrusion_bit_for_bit() {
        let size = crate::models::LAYER_SIZE as usize;
        let mut seed = 0x9e37_79b9u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 24
        };
        let mut sprites = vec![
            ("empty", sprite(|_, _| false)),
            ("full", sprite(|_, _| true)),
            ("single pixel", sprite(|x, y| x == 7 && y == 9)),
            // Every pixel has four sides: every lattice corner is used.
            ("checkerboard", sprite(|x, y| (x + y) % 2 == 0)),
            // A sword-like diagonal blade three pixels wide, touching the borders.
            ("diagonal", sprite(|x, y| (x + y).abs_diff(15) <= 1)),
            (
                "border ring",
                sprite(|x, y| x == 0 || y == 0 || x == 15 || y == 15),
            ),
        ];
        for density in [32, 128, 224] {
            let bits: Vec<bool> = (0..256).map(|_| noise() < density).collect();
            sprites.push(("noise", sprite(|x, y| bits[y * 16 + x])));
        }
        let colors = [[1.0; 3], [0.3, 0.7, 0.9], [0.123, 1.0, 0.456]];
        for (name, rgba) in &sprites {
            let edges = sprite_edges(rgba, size);
            let mesh = ItemMesh::new(&edges);
            assert!(mesh.points.len() <= ITEM_POINTS, "{name}");
            assert_eq!(mesh.quads.len(), edges.len() + 2, "{name}");
            for (m, matrix) in item_matrices().into_iter().enumerate() {
                for (c, color) in colors.into_iter().enumerate() {
                    let layer = 7 + m as u32;
                    // Appending after existing vertices, as the entity stream does.
                    let mut cached = vec![Vertex::zeroed(); 3];
                    let mut reference = cached.clone();
                    mesh.emit(&mut cached, layer, matrix, color);
                    item_mesh_reference(&mut reference, &edges, layer, matrix, color);
                    assert_same_vertices(&cached, &reference, &format!("{name} m{m} c{c}"));
                }
            }
        }
        // The checkerboard reaches every corner of the 17 x 17 x 2 lattice.
        let checker = ItemMesh::new(&sprite_edges(&sprites[3].1, size));
        assert_eq!(checker.points.len(), ITEM_POINTS);
    }

    #[test]
    #[ignore = "Requires HORIZON_189_TEST_JAR pointing at an owned 1.8.9 jar"]
    fn every_owned_item_layer_emits_the_per_call_extrusion_bit_for_bit()
    -> Result<(), Box<dyn Error>> {
        let jar = std::env::var_os("HORIZON_189_TEST_JAR").ok_or("Set HORIZON_189_TEST_JAR")?;
        let registry = Registry::open(Path::new(&jar))?;
        let size = crate::models::LAYER_SIZE as usize;
        // Before the cache, edges existed for item layers and the two-layer
        // sprites only; every other layer drew its front and back alone.
        let extruded: std::collections::HashSet<u32> = registry
            .item_layers
            .values()
            .copied()
            .chain(
                OVERLAY_SPRITES
                    .iter()
                    .filter_map(|name| registry.sprite(name)),
            )
            .collect();
        assert!(extruded.len() > 100, "{} extruded layers", extruded.len());
        let mut compared = 0;
        for layer in 0..registry.entity_textures.len() as u32 {
            let edges = if extruded.contains(&layer) {
                sprite_edges(&registry.entity_textures[layer as usize].rgba, size)
            } else {
                Vec::new()
            };
            for matrix in item_matrices().into_iter().take(8) {
                let mut cached = Vec::new();
                let mut reference = Vec::new();
                registry.item_mesh(&mut cached, layer, matrix, [0.5, 0.75, 1.0]);
                item_mesh_reference(&mut reference, &edges, layer, matrix, [0.5, 0.75, 1.0]);
                assert_same_vertices(&cached, &reference, &format!("layer {layer}"));
                compared += 1;
            }
        }
        let sword = registry.item_layers[&276];
        let mut vertices = Vec::new();
        registry.item_mesh(&mut vertices, sword, glam::Mat4::IDENTITY, [1.0; 3]);
        eprintln!(
            "compared={compared} diamond_sword_vertices={}",
            vertices.len()
        );
        Ok(())
    }

    #[test]
    fn culling_bounds_include_quantized_model_overhangs_without_neighbor_sections() {
        let mut pass = TerrainPass::default();
        for position in [
            [-7.5, 15.5, -2.25],
            [22.25, 15.5, -2.25],
            [22.25, 17.125, 23.984375],
            [-7.5, 17.125, 23.984375],
        ] {
            pass.vertices
                .push(TerrainVertex::new(position, [31.0, 7.0], 2047, [1.0; 3]));
        }
        pass.close_section(0);
        pass.close_section(1);
        for position in [
            [0.0, 32.0, 0.0],
            [1.0, 32.0, 0.0],
            [1.0, 33.0, 1.0],
            [0.0, 33.0, 1.0],
        ] {
            pass.vertices
                .push(TerrainVertex::new(position, [0.0; 2], 1024, [1.0; 3]));
        }
        pass.close_section(2);
        assert_eq!(
            pass.bounds[0],
            [-7.5, 15.5, -2.25, 22.25, 17.125, 23.984375]
        );
        assert_eq!(pass.bounds[1], [0.0; 6]);
        assert_eq!(pass.bounds[2], [0.0, 32.0, 0.0, 1.0, 33.0, 1.0]);
        assert_eq!(&pass.sections[..4], &[0, 1, 1, 2]);
        assert!(pass.bounds.iter().flatten().all(|n| n.is_finite()));
    }

    #[test]
    fn position_random_keeps_java_int_overflow_before_widening() {
        let x = 1000i32.wrapping_mul(3_129_871);
        assert_ne!(i64::from(x), 1000i64 * 3_129_871);
        let seed = i64::from(x) ^ (-1000i64).wrapping_mul(116_129_781) ^ 64;
        let expected = seed
            .wrapping_mul(seed)
            .wrapping_mul(42_317_861)
            .wrapping_add(seed.wrapping_mul(11));
        assert_eq!(position_random(1000, 64, -1000), expected);
        assert_eq!(position_random(0, 0, 0), 0);
    }

    #[test]
    #[ignore = "Requires HORIZON_189_TEST_JAR pointing at an owned 1.8.9 jar"]
    fn every_metadata_state_binds_an_owned_blockstate_variant() -> Result<(), Box<dyn Error>> {
        let jar = std::env::var_os("HORIZON_189_TEST_JAR").ok_or("Set HORIZON_189_TEST_JAR")?;
        let registry = Registry::open(Path::new(&jar))?;
        assert_eq!(registry.model_failures, 0);
        let log = registry.particle(BlockState(17 << 4)).unwrap().0;
        let assets = Assets::open(Path::new(&jar))?;
        let bark = first_frame(&assets.texture("blocks/log_oak")?).unwrap();
        let texture = &registry.entity_textures[log as usize];
        for y in 0..16 {
            assert_eq!(
                &texture.rgba[y * 128 * 4..(y * 128 + 16) * 4],
                &bark[y * 16 * 4..(y + 1) * 16 * 4]
            );
        }
        // Metadata values vanilla never stores fall back to the file's first
        // variant; every value vanilla writes must match exactly.
        eprintln!("inexact={:?}", registry.inexact);
        assert!(registry.inexact.len() < 64, "{:?}", registry.inexact);
        Ok(())
    }

    #[test]
    fn packed_terrain_corners_keep_model_precision_and_ranges() {
        // Model coordinates are sixteenths; liquids use ninths.
        for local in [[0.0, 0.0, 0.0], [16.0, 256.0, 16.0], [-0.5, 255.9375, 16.5]] {
            let vertex = TerrainVertex::new(local, [0.0, 1.0], 360, [1.0, 0.5, 0.0]);
            assert_eq!(vertex.local(), local);
            assert_eq!(vertex.position()[3], 360);
            assert_eq!(vertex.uv(), [0, 128]);
            assert_eq!(vertex.color(), [255, 128, 0]);
        }
        let liquid = TerrainVertex::new([0.0, 63.0 + 8.0 / 9.0, 0.0], [0.5; 2], 1, [0.8; 3]);
        assert!((liquid.local()[1] - (63.0 + 8.0 / 9.0)).abs() <= 0.5 / TerrainVertex::SCALE);
        assert_eq!(std::mem::size_of::<TerrainVertex>(), 12);
        // Every field at its limits survives the packing.
        let far = TerrainVertex::new([23.9, 503.9, -8.0], [31.99, 16.0], 2047, [0.2, 0.0, 1.0]);
        assert_eq!(far.local(), [23.90625, 503.90625, -8.0]);
        assert_eq!(far.uv(), [4095, 2048]);
        assert_eq!(far.position()[3], 2047);
        assert_eq!(far.color(), [51, 0, 255]);
        let mixed = TerrainVertex::new([1.0, 2.0, 3.0], [0.5, 0.125], 1024, [0.0; 3]);
        assert_eq!((mixed.position()[3], mixed.uv()), (1024, [64, 16]));
    }

    /// A unit face of the block at `at` facing `FACES[group]`, wound
    /// counter-clockwise when seen from outside, plus a tag in its colour.
    fn unit_face(group: usize, at: [f32; 3], tag: f32) -> [TerrainVertex; 4] {
        let normal = FACES[group].map(|d| d as f32);
        let axis = normal.iter().position(|&d| d != 0.0).unwrap();
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        let plane = if normal[axis] > 0.0 { 1.0 } else { 0.0 };
        let corner = |u: f32, v: f32| {
            let mut p = at;
            p[axis] += plane;
            p[a] += u;
            p[b] += v;
            TerrainVertex::new(p, [u, v], 1, [tag, 0.0, 0.0])
        };
        // (a, b, normal) is right-handed, so a -> b turns counter-clockwise
        // about +axis; reverse it for a face along -axis.
        if normal[axis] > 0.0 {
            [
                corner(0.0, 0.0),
                corner(1.0, 0.0),
                corner(1.0, 1.0),
                corner(0.0, 1.0),
            ]
        } else {
            [
                corner(0.0, 0.0),
                corner(0.0, 1.0),
                corner(1.0, 1.0),
                corner(1.0, 0.0),
            ]
        }
    }

    #[test]
    fn quads_group_by_winding_normal_and_keep_their_order_within_a_group() {
        for group in 0..6 {
            assert_eq!(facing(&unit_face(group, [3.0, 70.0, 5.0], 0.0)), group);
        }
        // A cross-shaped plant quad is diagonal: it can face anywhere.
        let diagonal = [
            TerrainVertex::new([0.0, 0.0, 0.0], [0.0; 2], 1, [0.0; 3]),
            TerrainVertex::new([1.0, 0.0, 1.0], [0.0; 2], 1, [0.0; 3]),
            TerrainVertex::new([1.0, 1.0, 1.0], [0.0; 2], 1, [0.0; 3]),
            TerrainVertex::new([0.0, 1.0, 0.0], [0.0; 2], 1, [0.0; 3]),
        ];
        assert_eq!(facing(&diagonal), ANY_FACING);
        let mut pass = TerrainPass::default();
        pass.close_section(0);
        // Section 1: an up face, a diagonal, a west face, a second up face
        // (an overlay), a down face. Section 2 is empty, section 3 has one east face.
        for quad in [
            unit_face(1, [0.0, 16.0, 0.0], 0.1),
            diagonal,
            unit_face(4, [0.0, 16.0, 0.0], 0.3),
            unit_face(1, [0.0, 16.0, 0.0], 0.4),
            unit_face(0, [0.0, 17.0, 0.0], 0.5),
        ] {
            pass.vertices.extend_from_slice(&quad);
        }
        pass.close_section(1);
        pass.close_section(2);
        pass.vertices
            .extend_from_slice(&unit_face(5, [2.0, 50.0, 2.0], 0.9));
        for section in 3..16 {
            pass.close_section(section);
        }
        let before = pass.vertices.len();
        pass.sort_by_facing();
        assert_eq!(pass.vertices.len(), before);
        // down, up, up, west, any; buckets: down 0..1, up 1..3, west 3..4, any 4..5.
        assert_eq!(pass.buckets(1), [0, 1, 3, 3, 3, 4, 4, 5]);
        let red: Vec<u8> = pass.vertices[0..20]
            .chunks(4)
            .map(|q| q[0].color()[0])
            .collect();
        let byte = |v: f32| (v * 255.0).round() as u8;
        assert_eq!(red, [byte(0.5), byte(0.1), byte(0.4), byte(0.3), 0]);
        assert_eq!(pass.buckets(2), [5; 8]);
        assert_eq!(pass.buckets(3), [5, 5, 5, 5, 5, 5, 6, 6]);
        // Sorting moves quads only within their section: the bounds still hold.
        assert_eq!(pass.sections[16], 6);
        // An unsorted pass keeps everything in one group.
        let mut plain = TerrainPass::default();
        plain
            .vertices
            .extend_from_slice(&unit_face(1, [0.0; 3], 0.0));
        for section in 0..16 {
            plain.close_section(section);
        }
        assert_eq!(plain.buckets(0), [0, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    #[ignore = "Requires HORIZON_189_TEST_JAR pointing at an owned 1.8.9 jar"]
    fn every_baked_cull_face_groups_under_its_own_side() -> Result<(), Box<dyn Error>> {
        // Vanilla faces draw front-facing (counter-clockwise, as the pipelines
        // cull) from the side they cull against: facing groups must agree, which
        // pins the winding convention to the jar's 12,000-odd cull faces.
        let jar = std::env::var_os("HORIZON_189_TEST_JAR").ok_or("Set HORIZON_189_TEST_JAR")?;
        let registry = Registry::open(Path::new(&jar))?;
        let mut checked = 0;
        let mut mismatched = 0;
        for model in &registry.models {
            for (faces, _) in &model.alternatives {
                for face in faces {
                    let Some(cull) = face.cull else {
                        continue;
                    };
                    let mut quad = Vec::new();
                    append(
                        &mut quad,
                        face.positions,
                        face.uv,
                        face.layer,
                        [1.0; 3],
                        [4.0, 64.0, 4.0],
                    );
                    let side = FACES.iter().position(|&f| f == cull).unwrap();
                    let group = facing(&quad);
                    if group != side && group != ANY_FACING {
                        // A few vanilla faces (pane bars) carry a cull side they do
                        // not lie on; their winding, which the GPU culls by, decides.
                        let axis = cull.iter().position(|&d| d != 0).unwrap();
                        let boundary = if cull[axis] > 0 { 1.0 } else { 0.0 };
                        assert!(
                            face.positions
                                .iter()
                                .any(|p| (p[axis] - boundary).abs() > 1e-4),
                            "face on its cull side {cull:?} groups as {group}"
                        );
                        mismatched += 1;
                    }
                    checked += usize::from(group == side);
                }
            }
        }
        assert!(
            checked > 10_000 && mismatched < 32,
            "{checked} / {mismatched}"
        );
        Ok(())
    }

    #[test]
    fn quads_index_two_triangles_from_four_corners() {
        assert_eq!(quad_indices(2), [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
        let mut corners = Vec::new();
        append(
            &mut corners,
            [[0.0; 3], [0.0, 1.0, 0.0], [1.0; 3], [1.0, 0.0, 0.0]],
            [[0.0; 2]; 4],
            3,
            [1.0; 3],
            [2.0, 0.0, 0.0],
        );
        assert_eq!(corners.len(), 4);
        assert_eq!(corners[2].local(), [3.0, 1.0, 1.0]);
    }

    #[test]
    fn sections_record_contiguous_quad_ranges() {
        let mut pass = TerrainPass::default();
        pass.close_section(0);
        append(
            &mut pass.vertices,
            [[0.0; 3]; 4],
            [[0.0; 2]; 4],
            1,
            [1.0; 3],
            [0.0; 3],
        );
        append(
            &mut pass.vertices,
            [[0.0; 3]; 4],
            [[0.0; 2]; 4],
            1,
            [1.0; 3],
            [0.0; 3],
        );
        pass.close_section(1);
        pass.close_section(2);
        append(
            &mut pass.vertices,
            [[0.0; 3]; 4],
            [[0.0; 2]; 4],
            1,
            [1.0; 3],
            [0.0; 3],
        );
        for section in 3..16 {
            pass.close_section(section);
        }
        assert_eq!(&pass.sections[..4], &[0, 0, 2, 2]);
        assert!(pass.sections[4..].iter().all(|&start| start == 3));
        assert_eq!(pass.quads(), 3);
    }

    #[test]
    fn section_visibility_follows_vis_graph() {
        assert_eq!(visibility(|_| false), ALL_VISIBLE);
        assert_eq!(visibility(|_| true), 0);
        // Under 256 opaque cubes everything is visible, however placed.
        assert_eq!(visibility(|index| index < 255), ALL_VISIBLE);
        // A full floor at y = 8 splits the bottom from the top.
        let floor = visibility(|index| index >> 8 == 8);
        assert_eq!(floor & sees(0, 1), 0);
        assert_ne!(floor & sees(0, 2), 0);
        assert_ne!(floor & sees(1, 5), 0);
        assert_ne!(floor & sees(3, 4), 0);
        // A sealed pocket touching no face adds nothing; open air above it
        // still joins the other faces.
        let pocket = visibility(|index| {
            let (x, y, z) = (index & 15, index >> 8, (index >> 4) & 15);
            !(y < 2 || (x == 5 && y == 5 && z == 5))
        });
        assert_eq!(pocket & (sees(1, 0) | sees(1, 1)), 0);
        assert_ne!(pocket & sees(0, 2), 0);
        assert_eq!(pocket, pocket_expected());
    }

    fn pocket_expected() -> u64 {
        // The air layer y < 2 touches down and all four sides, not up.
        let faces = [0, 2, 3, 4, 5];
        let mut bits = 0;
        for from in faces {
            for to in faces {
                bits |= sees(from, to);
            }
        }
        bits
    }

    #[test]
    fn alpha_tested_sprites_are_cutout() {
        let mut rgba = vec![255u8; 16 * 16 * 4];
        assert!(!has_cutout(&rgba));
        rgba[7] = 26; // 26/255 passes the 0.1 test.
        assert!(!has_cutout(&rgba));
        rgba[7] = 25;
        assert!(has_cutout(&rgba));
    }

    #[test]
    #[ignore = "Requires HORIZON_189_TEST_JAR pointing at an owned 1.8.9 jar"]
    fn baked_faces_fit_the_packed_format_and_split_by_alpha() -> Result<(), Box<dyn Error>> {
        let jar = std::env::var_os("HORIZON_189_TEST_JAR").ok_or("Set HORIZON_189_TEST_JAR")?;
        let registry = Registry::open(Path::new(&jar))?;
        for model in &registry.models {
            for (faces, _) in &model.alternatives {
                for face in faces {
                    assert!(
                        // Rotations leave float error the packer's clamp absorbs.
                        face.uv
                            .iter()
                            .flatten()
                            .all(|v| (-1e-4..=1.0 + 1e-4).contains(v)),
                        "{:?}",
                        face.uv
                    );
                    assert!(
                        face.positions
                            .iter()
                            .flatten()
                            .all(|v| (-1.0..=2.0).contains(v))
                    );
                    assert!(face.layer < TerrainVertex::LAYERS);
                }
            }
        }
        let cutout = |file: &str| -> Vec<bool> {
            let model = registry.by_file[file] as usize;
            registry.models[model].alternatives[0]
                .0
                .iter()
                .map(|face| registry.cutout[face.layer as usize])
                .collect()
        };
        assert!(cutout("stone").iter().all(|&cut| !cut));
        assert!(cutout("oak_leaves").iter().all(|&cut| cut));
        // Grass sides draw the tinted overlay over the solid dirt side.
        let grass = cutout("grass");
        assert!(grass.contains(&true) && grass.contains(&false));
        Ok(())
    }

    /// Every quad cut into its block-sized cells: pass, normal axis, plane, cell,
    /// sprite, colour and the texture at the cell's corners (whole sprites dropped).
    fn cells(mesh: &ColumnMesh) -> Vec<(u8, usize, u16, [u16; 2], u16, [u8; 3], [[i32; 2]; 3])> {
        let mut out = Vec::new();
        for (pass, terrain) in [(0u8, &mesh.solid), (1, &mesh.cutout)] {
            for quad in terrain.vertices.chunks(4) {
                let normal = (0..3)
                    .find(|&axis| {
                        quad.iter()
                            .all(|v| v.position()[axis] == quad[0].position()[axis])
                    })
                    .expect("axis-aligned full faces only");
                let across: Vec<usize> = (0..3).filter(|&axis| axis != normal).collect();
                let block = |value: u16| i32::from(value) / TerrainVertex::SCALE as i32;
                let low = across
                    .iter()
                    .map(|&a| quad.iter().map(|v| block(v.position()[a])).min().unwrap())
                    .collect::<Vec<_>>();
                let high = across
                    .iter()
                    .map(|&a| quad.iter().map(|v| block(v.position()[a])).max().unwrap())
                    .collect::<Vec<_>>();
                // The quad's texture as an affine function of the two positions.
                let corner = |a: i32, b: i32| {
                    quad.iter()
                        .find(|v| {
                            block(v.position()[across[0]]) == a
                                && block(v.position()[across[1]]) == b
                        })
                        .unwrap()
                };
                let origin = corner(low[0], low[1]);
                let (along_a, along_b) = (corner(high[0], low[1]), corner(low[0], high[1]));
                let uv_at = |a: i32, b: i32| -> [i32; 2] {
                    std::array::from_fn(|i| {
                        let o = i32::from(origin.uv()[i]);
                        let da = (i32::from(along_a.uv()[i]) - o) / (high[0] - low[0]);
                        let db = (i32::from(along_b.uv()[i]) - o) / (high[1] - low[1]);
                        o + da * (a - low[0]) + db * (b - low[1])
                    })
                };
                for a in low[0]..high[0] {
                    for b in low[1]..high[1] {
                        let corners = [uv_at(a, b), uv_at(a + 1, b), uv_at(a, b + 1)];
                        let shift = corners[0].map(|v| v.div_euclid(128) * 128);
                        let corners = corners.map(|c| [c[0] - shift[0], c[1] - shift[1]]);
                        out.push((
                            pass,
                            normal,
                            quad[0].position()[normal],
                            [a as u16, b as u16],
                            quad[0].position()[3],
                            quad[0].color(),
                            corners,
                        ));
                    }
                }
            }
        }
        out.sort_unstable();
        out
    }

    #[test]
    #[ignore = "Requires HORIZON_189_TEST_JAR pointing at an owned 1.8.9 jar"]
    fn greedy_meshing_covers_exactly_the_cells_and_texels_of_per_block_faces()
    -> Result<(), Box<dyn Error>> {
        use horizon_world_189::{ColumnData, ColumnUpdate, Section};
        let jar = std::env::var_os("HORIZON_189_TEST_JAR").ok_or("Set HORIZON_189_TEST_JAR")?;
        let mut registry = Registry::open(Path::new(&jar))?;
        // Rolling grass over dirt and stone with a lake, gravel, leaves and
        // patchy sky light, over a 3 x 3 neighbourhood so column edges cull.
        let mut world = World::default();
        let mut seed = 0x2545_f491_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut sections: [Option<Section>; 16] = std::array::from_fn(|_| None);
                for (section_y, section) in sections.iter_mut().enumerate().take(5) {
                    let mut states = [0u16; 4096];
                    let mut sky = [0u8; 2048];
                    for index in 0..4096 {
                        let (x, y, z) =
                            (index & 15, section_y * 16 + (index >> 8), (index >> 4) & 15);
                        let height = 60 + (x / 3 + z / 5) % 4 + usize::from((cx + cz) % 2 == 0);
                        states[index] = if y < height - 4 {
                            1 << 4
                        } else if y < height {
                            3 << 4
                        } else if y == height {
                            if next() % 9 == 0 { 13 << 4 } else { 2 << 4 }
                        } else if y == height + 1 && next() % 7 == 0 {
                            18 << 4
                        } else if y == height + 1 && x < 4 {
                            9 << 4
                        } else {
                            0
                        };
                        let level = if y > height + 2 {
                            15
                        } else {
                            (next() % 3) as u8 * 6
                        };
                        sky[index >> 1] |= level << ((index & 1) * 4);
                    }
                    *section = Some(Section::new(&states, &[0; 2048], Some(&sky)));
                }
                world.apply(ColumnUpdate::Data {
                    x: cx,
                    z: cz,
                    full: true,
                    data: Box::new(ColumnData {
                        sections,
                        biomes: None,
                    }),
                });
            }
        }
        let merged = registry.mesh_column(&world, (0, 0), [0, 0, 0]);
        registry.greedy = false;
        let single = registry.mesh_column(&world, (0, 0), [0, 0, 0]);
        if std::env::var_os("BOB_MESH_TIMING").is_some() {
            // Alternating runs, so load on the machine falls on both alike.
            let mut times = [Vec::new(), Vec::new()];
            for round in 0..400 {
                registry.greedy = round % 2 == 0;
                let start = std::time::Instant::now();
                std::hint::black_box(registry.mesh_column(&world, (0, 0), [0, 0, 0]));
                times[round % 2].push(start.elapsed().as_micros());
            }
            for (name, mut list) in ["greedy", "per-block"].into_iter().zip(times) {
                list.sort_unstable();
                eprintln!(
                    "MESH_TIMING {name} p50_us={} p90_us={} p99_us={}",
                    list[list.len() / 2],
                    list[list.len() * 9 / 10],
                    list[list.len() * 99 / 100]
                );
            }
            registry.greedy = false;
        }
        eprintln!(
            "MESH_PARITY_PASS base_vertices={} merged_vertices={} base_triangles={} merged_triangles={}",
            single.solid.vertices.len() + single.cutout.vertices.len(),
            merged.solid.vertices.len() + merged.cutout.vertices.len(),
            (single.solid.quads() + single.cutout.quads()) * 2,
            (merged.solid.quads() + merged.cutout.quads()) * 2,
        );
        // Patchy light keeps this terrain from merging much; flat lit floors merge far more.
        assert!(
            merged.solid.quads() * 4 < single.solid.quads() * 3,
            "{} vs {}",
            merged.solid.quads(),
            single.solid.quads()
        );
        // Liquids, translucent faces and block entities do not merge.
        assert_eq!(merged.translucent.vertices, single.translucent.vertices);
        assert_eq!(merged.visibility, single.visibility);
        // Only full faces merge; check them cell by cell.
        let full = |mesh: &ColumnMesh| ColumnMesh {
            solid: TerrainPass {
                vertices: mesh.solid.vertices.clone(),
                sections: [0; 17],
                ..TerrainPass::default()
            },
            cutout: TerrainPass {
                vertices: mesh.cutout.vertices.clone(),
                sections: [0; 17],
                ..TerrainPass::default()
            },
            ..ColumnMesh::default()
        };
        assert_eq!(cells(&full(&merged)), cells(&full(&single)));
        Ok(())
    }

    #[test]
    fn plains_climate_samples_the_reference_colormap_cell() {
        let Climate::Normal(t, r) = climate(1) else {
            panic!("plains is a normal climate");
        };
        let t = f64::from(t).clamp(0.0, 1.0);
        let r = f64::from(r).clamp(0.0, 1.0) * t;
        // Vanilla promotes 0.8F to 0.800000011920929 before the int cast.
        assert_eq!(((1.0 - t) * 255.0) as usize, 50);
        assert_eq!(((1.0 - r) * 255.0) as usize, 173);
        assert!(matches!(climate(129), Climate::Normal(0.8, 0.4)));
        assert!(brightness(15) > 0.98 && brightness(0) < 0.05);
    }
}
