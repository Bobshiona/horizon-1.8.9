//! Protocol 47 section data. Modern palettes, registries, and world generation
//! have no role in this representation.
#![forbid(unsafe_code)]

pub mod block_entities;
pub mod blocks;
mod decode;
pub mod entities;
mod keys;
pub mod palette;
pub mod variants;
pub use decode::{
    BlockChange, decode_block_change, decode_bulk, decode_chunk, decode_multi_block_change,
};
pub use keys::Keys;
use palette::{Nibbles, States};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockState(pub u16);

impl BlockState {
    pub fn block_id(self) -> u16 {
        self.0 >> 4
    }
    pub fn metadata(self) -> u8 {
        (self.0 & 15) as u8
    }
}

/// A 16-block cube of states and light, stored compactly (see [`palette`]).
#[derive(Debug)]
pub struct Section {
    states: States,
    block_light: Nibbles,
    sky_light: Option<Nibbles>,
}

impl Section {
    /// A section from the wire's arrays: states indexed `y << 8 | z << 4 | x`,
    /// light planes two levels per byte, low nibble first.
    pub fn new(
        states: &[u16; 4096],
        block_light: &[u8; 2048],
        sky_light: Option<&[u8; 2048]>,
    ) -> Self {
        Section {
            states: States::new(states),
            block_light: Nibbles::new(block_light),
            sky_light: sky_light.map(Nibbles::new),
        }
    }

    /// The section a block change creates where none was sent: air, no block
    /// light, full sky light.
    fn empty() -> Self {
        Section {
            states: States::Uniform(0),
            block_light: Nibbles::Uniform(0),
            sky_light: Some(Nibbles::Uniform(15)),
        }
    }

    pub fn state(&self, x: u8, y: u8, z: u8) -> Option<BlockState> {
        let index = section_index(x, y, z)?;
        Some(BlockState(self.states.get(index)))
    }

    /// The raw state at `y << 8 | z << 4 | x`.
    #[inline]
    pub fn state_at(&self, index: usize) -> u16 {
        self.states.get(index)
    }

    pub fn states(&self) -> &States {
        &self.states
    }

    pub fn light(&self, x: u8, y: u8, z: u8) -> Option<(u8, u8)> {
        let index = section_index(x, y, z)?;
        let block = self.block_light.get(index);
        let sky = self.sky_light.as_ref().map_or(0, |light| light.get(index));
        Some((block, sky))
    }

    pub fn has_sky_light(&self) -> bool {
        self.sky_light.is_some()
    }

    /// Bytes this section keeps on the heap.
    pub fn heap_bytes(&self) -> usize {
        self.states.heap_bytes()
            + self.block_light.heap_bytes()
            + self.sky_light.as_ref().map_or(0, Nibbles::heap_bytes)
    }
}

fn section_index(x: u8, y: u8, z: u8) -> Option<usize> {
    if x >= 16 || y >= 16 || z >= 16 {
        return None;
    }
    Some((usize::from(y) << 8) | (usize::from(z) << 4) | usize::from(x))
}

#[derive(Debug)]
pub struct ColumnData {
    pub sections: [Option<Section>; 16],
    pub biomes: Option<Box<[u8; 256]>>,
}

#[derive(Debug)]
pub enum ColumnUpdate {
    Unload {
        x: i32,
        z: i32,
    },
    Data {
        x: i32,
        z: i32,
        full: bool,
        data: Box<ColumnData>,
    },
}

#[derive(Debug)]
pub struct ChunkColumn {
    sections: [Option<Section>; 16],
    biomes: Option<Box<[u8; 256]>>,
    /// Per x/z: the lowest y with no light-blocking block at or above it.
    heights: Box<[u16; 256]>,
}

impl ChunkColumn {
    fn new(sections: [Option<Section>; 16], biomes: Option<Box<[u8; 256]>>) -> Self {
        let mut column = Self {
            sections,
            biomes,
            heights: Box::new([0; 256]),
        };
        for index in 0..256 {
            column.rescan(index);
        }
        column
    }

    fn rescan(&mut self, index: usize) {
        let (x, z) = (index & 15, index >> 4);
        let mut height = 0;
        'sections: for section_y in (0..16).rev() {
            let Some(section) = &self.sections[section_y] else {
                continue;
            };
            for y in (0..16).rev() {
                let state = section.state_at((y << 8) | (z << 4) | x);
                if blocks::blocks_light(state >> 4) {
                    height = (section_y * 16 + y + 1) as u16;
                    break 'sections;
                }
            }
        }
        self.heights[index] = height;
    }

    pub fn section(&self, index: usize) -> Option<&Section> {
        self.sections.get(index)?.as_ref()
    }

    /// Bytes the column's sections keep on the heap.
    pub fn heap_bytes(&self) -> usize {
        self.sections
            .iter()
            .flatten()
            .map(Section::heap_bytes)
            .sum()
    }

    pub fn biome(&self, x: u8, z: u8) -> Option<u8> {
        if x >= 16 || z >= 16 {
            return None;
        }
        Some(self.biomes.as_ref()?[usize::from(z) * 16 + usize::from(x)])
    }
}

#[derive(Debug, Default)]
pub struct World {
    columns: HashMap<(i32, i32), ChunkColumn, Keys>,
    block_entities: HashMap<[i32; 3], block_entities::BlockEntity, Keys>,
    /// Chest and ender chest lids that are open or moving.
    lids: HashMap<[i32; 3], Lid, Keys>,
    no_sky: bool,
}

/// A chest lid as `TileEntityChest` keeps it: players looking inside (block
/// event 1) and the lid angle, 0 shut to 1 open, this tick and the last.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lid {
    pub viewers: u8,
    pub angle: f32,
    pub previous: f32,
}

impl Lid {
    /// `TileEntityChest.update` (aky.c) and `TileEntityEnderChest.update`:
    /// the lid moves a tenth a tick toward open while anyone looks inside.
    pub fn tick(&mut self) {
        self.previous = self.angle;
        if (self.viewers == 0 && self.angle > 0.0) || (self.viewers > 0 && self.angle < 1.0) {
            if self.viewers > 0 {
                self.angle += 0.1;
            } else {
                self.angle -= 0.1;
            }
            self.angle = self.angle.clamp(0.0, 1.0);
        }
    }

    /// Whether the chest draws open this frame rather than as terrain.
    pub fn open(&self) -> bool {
        self.angle > 0.0 || self.previous > 0.0
    }
}

/// Chest, trapped chest and ender chest: the blocks whose lids open.
pub fn has_lid(block: u16) -> bool {
    matches!(block, 54 | 130 | 146)
}

impl World {
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn columns(&self) -> impl Iterator<Item = ((i32, i32), &ChunkColumn)> {
        self.columns
            .iter()
            .map(|(&position, column)| (position, column))
    }

    pub fn column(&self, x: i32, z: i32) -> Option<&ChunkColumn> {
        self.columns.get(&(x, z))
    }

    /// Store or remove one block entity; the column must be loaded.
    pub fn set_block_entity(
        &mut self,
        at: [i32; 3],
        entity: Option<block_entities::BlockEntity>,
    ) -> bool {
        if self
            .column(at[0].div_euclid(16), at[2].div_euclid(16))
            .is_none()
        {
            return false;
        }
        match entity {
            Some(entity) => self.block_entities.insert(at, entity),
            None => self.block_entities.remove(&at),
        };
        true
    }

    pub fn block_entity(&self, at: [i32; 3]) -> Option<&block_entities::BlockEntity> {
        self.block_entities.get(&at)
    }

    pub fn block_entities(
        &self,
    ) -> impl Iterator<Item = ([i32; 3], &block_entities::BlockEntity)> + '_ {
        self.block_entities.iter().map(|(&at, entity)| (at, entity))
    }

    /// Block event 1 on a chest (`receiveClientEvent`): how many players
    /// look inside. Events for other blocks, or unloaded ones, are ignored.
    pub fn chest_viewers(&mut self, at: [i32; 3], viewers: u8) {
        if self
            .block(at[0], at[1], at[2])
            .is_some_and(|state| has_lid(state.block_id()))
        {
            self.lids.entry(at).or_default().viewers = viewers;
        }
    }

    /// One client tick of every lid. Returns the chests that start or stop
    /// drawing open, whose columns need meshing again.
    pub fn tick_lids(&mut self) -> Vec<[i32; 3]> {
        let mut changed = Vec::new();
        let positions: Vec<[i32; 3]> = self.lids.keys().copied().collect();
        for at in positions {
            let chest = self
                .block(at[0], at[1], at[2])
                .is_some_and(|state| has_lid(state.block_id()));
            let Some(lid) = self.lids.get_mut(&at) else {
                continue;
            };
            let was = lid.open();
            if chest {
                lid.tick();
            } else {
                *lid = Lid::default();
            }
            if was != lid.open() {
                changed.push(at);
            }
            if *lid == Lid::default() {
                self.lids.remove(&at);
            }
        }
        changed
    }

    /// The lid of a chest drawing open, if it is.
    pub fn lid(&self, at: [i32; 3]) -> Option<Lid> {
        self.lids.get(&at).copied().filter(Lid::open)
    }

    /// Every chest drawing open.
    pub fn open_lids(&self) -> impl Iterator<Item = ([i32; 3], Lid)> + '_ {
        self.lids
            .iter()
            .filter(|(_, lid)| lid.open())
            .map(|(&at, &lid)| (at, lid))
    }

    fn drop_block_entities(&mut self, x: i32, z: i32) {
        self.block_entities
            .retain(|at, _| (at[0].div_euclid(16), at[2].div_euclid(16)) != (x, z));
        self.lids
            .retain(|at, _| (at[0].div_euclid(16), at[2].div_euclid(16)) != (x, z));
    }

    pub fn apply(&mut self, update: ColumnUpdate) {
        match update {
            ColumnUpdate::Unload { x, z } => {
                self.columns.remove(&(x, z));
                self.drop_block_entities(x, z);
            }
            ColumnUpdate::Data { x, z, full, data } => {
                if full {
                    self.drop_block_entities(x, z);
                    self.columns
                        .insert((x, z), ChunkColumn::new(data.sections, data.biomes));
                } else if let Some(column) = self.columns.get_mut(&(x, z)) {
                    // Vanilla ignores partial updates for columns it has not loaded.
                    for (existing, update) in column.sections.iter_mut().zip(data.sections) {
                        if update.is_some() {
                            *existing = update;
                        }
                    }
                    for index in 0..256 {
                        column.rescan(index);
                    }
                }
            }
        }
    }

    /// Apply a server block change. Vanilla discards changes to columns it has
    /// not loaded; a missing section becomes an empty one first.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: BlockState) -> bool {
        if !(0..256).contains(&y) {
            return false;
        }
        let Some(column) = self.columns.get_mut(&(x.div_euclid(16), z.div_euclid(16))) else {
            return false;
        };
        let section = column.sections[y as usize >> 4].get_or_insert_with(Section::empty);
        let index = ((y as usize & 15) << 8)
            | ((z.rem_euclid(16) as usize) << 4)
            | x.rem_euclid(16) as usize;
        section.states.set(index, state.0);
        let column_index = index & 0xff;
        let height = &mut column.heights[column_index];
        if blocks::blocks_light(state.block_id()) {
            *height = (*height).max(y as u16 + 1);
        } else if *height == y as u16 + 1 {
            column.rescan(column_index);
        }
        true
    }

    /// Whether the dimension has sky light (the overworld). Defaults on.
    pub fn set_sky(&mut self, sky: bool) {
        self.no_sky = !sky;
    }

    /// Block and sky light, with unloaded or open-sky positions fully lit.
    /// Cells above the column's highest light-blocking block see the sky
    /// directly; this corrects light the server sent before a block change,
    /// which vanilla clients relight themselves.
    pub fn light(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        if y >= 256 {
            return (0, 15);
        }
        if y < 0 {
            return (0, 0);
        }
        let Some(column) = self.column(x.div_euclid(16), z.div_euclid(16)) else {
            return (0, 15);
        };
        let open = !self.no_sky
            && y as u16
                >= column.heights[(z.rem_euclid(16) as usize) << 4 | x.rem_euclid(16) as usize];
        let (block, sky) = self.stored_light(column, x, y, z);
        (block, if open { 15 } else { sky })
    }

    fn stored_light(&self, column: &ChunkColumn, x: i32, y: i32, z: i32) -> (u8, u8) {
        match column.section(y as usize >> 4) {
            None => (0, 15),
            Some(section) => section
                .light(
                    x.rem_euclid(16) as u8,
                    (y & 15) as u8,
                    z.rem_euclid(16) as u8,
                )
                .map_or((0, 15), |(block, sky)| {
                    (block, if section.has_sky_light() { sky } else { 0 })
                }),
        }
    }

    /// Height of the column's highest light-blocking block plus one.
    pub fn height(&self, x: i32, z: i32) -> Option<u16> {
        let column = self.column(x.div_euclid(16), z.div_euclid(16))?;
        Some(column.heights[(z.rem_euclid(16) as usize) << 4 | x.rem_euclid(16) as usize])
    }

    /// None distinguishes an unloaded column/out-of-world Y from known air.
    pub fn block(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        if !(0..256).contains(&y) {
            return None;
        }
        let column = self.column(x.div_euclid(16), z.div_euclid(16))?;
        match column.section(y as usize >> 4) {
            None => Some(BlockState(0)),
            Some(section) => section.state(
                x.rem_euclid(16) as u8,
                (y & 15) as u8,
                z.rem_euclid(16) as u8,
            ),
        }
    }
}

#[cfg(test)]
mod tests;
