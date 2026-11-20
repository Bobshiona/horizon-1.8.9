//! Legacy terrain meshing. Every 1.8.9 blockstate variant in the owned jar is
//! baked once, with its quarter turns and UV lock. Meshes are built per
//! column with biome tints, flat lighting, and separate opaque/cutout and
//! translucent vertex lists. Liquids use the still textures' first frame.
use bytemuck::{Pod, Zeroable};
use horizon_assets_189::{Assets, Texture, VariantSet};
use horizon_world_189::{
    BlockState, World,
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
}

struct Model {
    alternatives: Vec<(Vec<Face>, i32)>,
    total: i32,
}

/// Per-column vertices; translucent geometry draws after everything else.
/// Block entities use the entity texture array.
#[derive(Default)]
pub struct ColumnMesh {
    pub opaque: Vec<Vertex>,
    pub translucent: Vec<Vertex>,
    pub entity: Vec<Vertex>,
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
    /// Variant models that failed to bake, and metadata states whose exact
    /// variant key is absent from the owned blockstates.
    pub model_failures: usize,
    pub inexact: Vec<String>,
    /// 128-pixel entity layers in `models::Texture` order, and font advances.
    pub entity_textures: Vec<Texture>,
    pub glyphs: [u8; 256],
    /// Interface sheets, fonts and language.
    pub gui: crate::ui::Resources,
    item_layers: HashMap<i16, u32>,
    overlays: HashMap<&'static str, u32>,
    /// Opaque-pixel edges of each item sprite layer (`x`, `y`, side: 0 left,
    /// 1 right, 2 top, 3 bottom), for the extruded item mesh.
    item_edges: HashMap<u32, Vec<[u8; 3]>>,
    block_layers: Vec<u32>,
    /// Blocks whose item model is a separate `*_inventory` block model, by
    /// (id, metadata or 255 for any).
    inventory: HashMap<(u16, u8), u32>,
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
fn brightness(level: u8) -> f32 {
    let f = 1.0 - f32::from(level.min(15)) / 15.0;
    let base = (1.0 - f) / (f * 3.0 + 1.0);
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
                let mut alternatives = Vec::with_capacity(choices.len());
                for (choice, weight) in choices {
                    match assets.bake_variant(&choice.model, choice.rotations, choice.uv_lock) {
                        Ok(quads) => {
                            let faces = quads
                                .into_iter()
                                .map(|quad| Face {
                                    positions: quad.positions,
                                    uv: quad.uv,
                                    layer: layer_for(&quad.material.sprite, &mut textures),
                                    shade: if quad.shade {
                                        face_shade(quad.facing.name())
                                    } else {
                                        1.0
                                    },
                                    tint: quad.tint_index >= 0,
                                    cull: quad.cull.map(|side| direction(side.name())),
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
                .map(|quad| Face {
                    positions: quad.positions,
                    uv: quad.uv,
                    layer: layer_for(&quad.material.sprite, &mut textures),
                    shade: if quad.shade {
                        face_shade(quad.facing.name())
                    } else {
                        1.0
                    },
                    tint: quad.tint_index >= 0,
                    cull: quad.cull.map(|side| direction(side.name())),
                })
                .collect();
            inventory.insert(key, models.len() as u32);
            models.push(Model {
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
        let mut item_layers = HashMap::new();
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
        let mut overlays = HashMap::new();
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
        let mut item_edges = HashMap::new();
        for &layer in item_layers.values().chain(overlays.values()) {
            item_edges.insert(
                layer,
                sprite_edges(&entity_textures[layer as usize].rgba, size),
            );
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
            textures,
            model_failures: failures,
            inexact: Vec::new(),
            entity_textures,
            glyphs,
            gui,
            item_edges,
            item_layers,
            overlays,
            block_layers,
            inventory,
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

    /// `EntityDiggingFX`'s texture and tint for a block: the first face of
    /// its model (the particle texture is not baked) and leaf colours.
    pub fn particle(&self, state: BlockState) -> Option<(u32, [f32; 3])> {
        let model = self.resolve(&World::default(), [0; 3], state)?;
        let face = self.models[model as usize]
            .alternatives
            .first()?
            .0
            .first()?;
        let tint = match (state.block_id(), state.metadata() & 3) {
            (18, 1) => [0x61, 0x99, 0x61],
            (18, 2) => [0x80, 0xA7, 0x55],
            (18 | 161, _) => [0x48u8, 0xB5, 0x18],
            _ => [0xFF; 3],
        }
        .map(|c| f32::from(c) / 255.0);
        Some((
            self.block_layers[face.layer as usize],
            if face.tint { tint } else { [1.0; 3] },
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

    /// `ItemModelGenerator`'s extruded sprite in a unit cube: front at
    /// z = 8.5/16, back at 7.5/16, and a one-pixel side wherever an opaque
    /// pixel meets a transparent one. Both windings, as items draw unculled.
    pub fn item_mesh(
        &self,
        out: &mut Vec<Vertex>,
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
        for &[x, y, side] in self
            .item_edges
            .get(&layer)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
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

    pub fn stone_fixture(&self) -> Vec<Vertex> {
        let mut vertices = Vec::new();
        let model = self
            .by_variant
            .get(&("stone".to_owned(), "normal".to_owned()))
            .copied()
            .unwrap_or(self.missing);
        for face in &self.models[model as usize].alternatives[0].0 {
            append(
                &mut vertices,
                face.positions,
                face.uv,
                face.layer,
                [face.shade; 3],
                [0.0; 3],
            );
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
        let mut mesh = ColumnMesh::default();
        let Some(column) = world.column(chunk.0, chunk.1) else {
            return mesh;
        };
        for section_y in 0..16 {
            let Some(section) = column.section(section_y) else {
                continue;
            };
            for (index, &raw) in section.states.iter().enumerate() {
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
                let info = block(state.block_id());
                if info.layer == Layer::Liquid {
                    self.liquid(world, pos, state, offset, &mut mesh);
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
                let target = if info.layer == Layer::Translucent {
                    &mut mesh.translucent
                } else {
                    &mut mesh.opaque
                };
                let mut tint = None;
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
                    append(
                        target,
                        face.positions,
                        face.uv,
                        face.layer,
                        color.map(|channel| channel * face.shade * light),
                        offset,
                    );
                }
            }
        }
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
        } else {
            (self.lava, &mut mesh.opaque)
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
                append(target, positions, uv, layer, [shade * light; 3], offset);
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

fn append(
    vertices: &mut Vec<Vertex>,
    positions: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    layer: u32,
    color: [f32; 3],
    offset: [f32; 3],
) {
    for index in [0, 1, 2, 0, 2, 3] {
        vertices.push(Vertex {
            position: std::array::from_fn(|axis| positions[index][axis] + offset[axis]),
            uv: uv[index],
            layer,
            color,
            hurt: 0.0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Metadata values vanilla never stores fall back to the file's first
        // variant; every value vanilla writes must match exactly.
        eprintln!("inexact={:?}", registry.inexact);
        assert!(registry.inexact.len() < 64, "{:?}", registry.inexact);
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
