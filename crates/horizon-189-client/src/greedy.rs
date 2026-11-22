//! Greedy meshing: neighbouring full block faces that look the same become one
//! quad. A face qualifies when it covers its whole block side, lies on the block
//! boundary and maps exactly one sprite across it; two faces merge only when
//! they share the plane, sprite, texture orientation and colour (tint x face
//! shade x light), so light boundaries and biome tint edges stay where they
//! are. The terrain sampler repeats, so a merged quad tiles its sprite once per
//! block as the separate quads did.

use crate::terrain::{TerrainPass, TerrainVertex};

const EPSILON: f32 = 1e-4;

/// How a full face extends over its neighbours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// The face's normal axis and the two axes across it (ascending).
    normal: usize,
    across: [usize; 2],
    /// Each corner's place on the face: 0 or 1 along each `across` axis.
    corners: [[u8; 2]; 4],
    /// The texture's step per block along each `across` axis: a unit vector.
    steps: [[i8; 2]; 2],
}

fn near(value: f32, target: f32) -> bool {
    (value - target).abs() < EPSILON
}

fn unit_step(step: [f32; 2]) -> Option<[i8; 2]> {
    let as_unit = |value: f32| {
        if near(value, 0.0) {
            Some(0)
        } else if near(value, 1.0) {
            Some(1)
        } else if near(value, -1.0) {
            Some(-1)
        } else {
            None
        }
    };
    let step: [i8; 2] = [as_unit(step[0])?, as_unit(step[1])?];
    (step[0].abs() + step[1].abs() == 1).then_some(step)
}

impl Grid {
    /// The grid of a face that covers the whole side `cull` faces, if it does.
    pub fn of(
        positions: &[[f32; 3]; 4],
        uv: &[[f32; 2]; 4],
        cull: Option<[i32; 3]>,
    ) -> Option<Grid> {
        let cull = cull?;
        let normal = cull.iter().position(|&d| d != 0)?;
        if cull.iter().filter(|&&d| d != 0).count() != 1 {
            return None;
        }
        let boundary = if cull[normal] > 0 { 1.0 } else { 0.0 };
        if !positions
            .iter()
            .all(|corner| near(corner[normal], boundary))
        {
            return None;
        }
        let across = match normal {
            0 => [1, 2],
            1 => [0, 2],
            _ => [0, 1],
        };
        let mut corners = [[0u8; 2]; 4];
        let mut seen = [false; 4];
        for (corner, place) in positions.iter().zip(&mut corners) {
            for (slot, &axis) in place.iter_mut().zip(&across) {
                *slot = if near(corner[axis], 0.0) {
                    0
                } else if near(corner[axis], 1.0) {
                    1
                } else {
                    return None;
                };
            }
            let which = usize::from(place[0]) | usize::from(place[1]) << 1;
            if std::mem::replace(&mut seen[which], true) {
                return None;
            }
        }
        let at = |place: [u8; 2]| {
            uv[corners
                .iter()
                .position(|&c| c == place)
                .expect("all four corners")]
        };
        let origin = at([0, 0]);
        let along_a = at([1, 0]);
        let along_b = at([0, 1]);
        let far = at([1, 1]);
        let step_a = [along_a[0] - origin[0], along_a[1] - origin[1]];
        let step_b = [along_b[0] - origin[0], along_b[1] - origin[1]];
        if !(near(far[0], origin[0] + step_a[0] + step_b[0])
            && near(far[1], origin[1] + step_a[1] + step_b[1]))
        {
            return None;
        }
        let steps = [unit_step(step_a)?, unit_step(step_b)?];
        // The two steps must run along different texture axes.
        if (steps[0][0] != 0) == (steps[1][0] != 0) {
            return None;
        }
        Some(Grid {
            normal,
            across,
            corners,
            steps,
        })
    }
}

/// What must match for two cells to merge.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    /// The texture coordinate of the face's (0, 0) corner, in 1/4096 sprites.
    origin: [i32; 2],
    steps: [[i8; 2]; 2],
    corners: [[u8; 2]; 4],
    layer: u32,
    color: [u8; 3],
}

/// One deferred face: the model face it came from, its colour and block.
struct Pending<'a> {
    /// Sort order: pass, then the face's rank among the block's faces on the same
    /// plane (an overlay after its base), then plane, then row and column.
    order: (u8, u8, u8, u8, u8, u8),
    key: Key,
    grid: Grid,
    positions: &'a [[f32; 3]; 4],
    uv: &'a [[f32; 2]; 4],
    color: [f32; 3],
    /// The block's column-local corner.
    local: [f32; 3],
}

impl Pending<'_> {
    fn place(&self) -> usize {
        usize::from(self.order.4) * 16 + usize::from(self.order.5)
    }
}

const EMPTY: u16 = u16::MAX;

/// Faces of one section waiting to be merged.
pub struct Merger<'a> {
    pending: Vec<Pending<'a>>,
    /// Per block: how many faces each (pass, normal side) has deferred so far.
    ranks: [[u8; 6]; 2],
    /// One plane's cells while merging: an index into `pending`, or `EMPTY`.
    cells: [u16; 256],
}

impl Default for Merger<'_> {
    fn default() -> Self {
        Merger {
            pending: Vec::new(),
            ranks: [[0; 6]; 2],
            cells: [EMPTY; 256],
        }
    }
}

fn color_bytes(color: [f32; 3]) -> [u8; 3] {
    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

impl<'a> Merger<'a> {
    /// Start a new block's faces.
    pub fn next_block(&mut self) {
        self.ranks = [[0; 6]; 2];
    }

    /// Defer a full face; `pass` 0 is solid, 1 cutout. `local` is the block's
    /// column-local corner (x 0-15, y 0-255, z 0-15).
    pub fn push(
        &mut self,
        pass: u8,
        grid: Grid,
        positions: &'a [[f32; 3]; 4],
        uv: &'a [[f32; 2]; 4],
        layer: u32,
        color: [f32; 3],
        local: [f32; 3],
    ) {
        let origin_uv = uv[grid
            .corners
            .iter()
            .position(|&c| c == [0, 0])
            .expect("corner")];
        let key = Key {
            origin: origin_uv.map(|value| (value * 4096.0).round() as i32),
            steps: grid.steps,
            corners: grid.corners,
            layer,
            color: color_bytes(color),
        };
        let normal_side = grid.normal * 2 + usize::from(positions[0][grid.normal] > 0.5);
        let rank = &mut self.ranks[usize::from(pass)][normal_side];
        let block = [local[0] as u8, (local[1] as u32 & 15) as u8, local[2] as u8];
        self.pending.push(Pending {
            order: (
                pass,
                *rank,
                normal_side as u8,
                block[grid.normal],
                block[grid.across[1]],
                block[grid.across[0]],
            ),
            key,
            grid,
            positions,
            uv,
            color,
            local,
        });
        *rank = rank.saturating_add(1);
    }

    /// Merge and emit everything deferred, solid faces into `solid` and cutout
    /// faces into `cutout`.
    pub fn flush(&mut self, solid: &mut TerrainPass, cutout: &mut TerrainPass) {
        if self.pending.is_empty() {
            return;
        }
        self.pending.sort_unstable_by_key(|pending| pending.order);
        let plane = |pending: &Pending| {
            (
                pending.order.0,
                pending.order.1,
                pending.order.2,
                pending.order.3,
            )
        };
        let mut start = 0;
        while start < self.pending.len() {
            let group = plane(&self.pending[start]);
            let end = start
                + self.pending[start..]
                    .iter()
                    .take_while(|pending| plane(pending) == group)
                    .count();
            for (index, pending) in self.pending[start..end].iter().enumerate() {
                self.cells[pending.place()] = (start + index) as u16;
            }
            let target = if group.0 == 0 {
                &mut *solid
            } else {
                &mut *cutout
            };
            // Row by row: a cell still set when reached starts a rectangle.
            for index in start..end {
                let first = &self.pending[index];
                let place = first.place();
                if self.cells[place] != index as u16 {
                    continue;
                }
                let (a, b) = (place % 16, place / 16);
                let cells = &self.cells;
                let pending = &self.pending;
                let same = |cell: usize| {
                    let at = cells[cell];
                    at != EMPTY && pending[usize::from(at)].key == first.key
                };
                let mut width = 1;
                while a + width < 16 && same(b * 16 + a + width) {
                    width += 1;
                }
                let mut height = 1;
                while b + height < 16 && (a..a + width).all(|x| same((b + height) * 16 + x)) {
                    height += 1;
                }
                for row in b..b + height {
                    self.cells[row * 16 + a..row * 16 + a + width].fill(EMPTY);
                }
                emit(&self.pending[index], [width, height], target);
            }
            start = end;
        }
        self.pending.clear();
    }
}

fn emit(cell: &Pending, size: [usize; 2], target: &mut TerrainPass) {
    let grid = &cell.grid;
    let extra = [size[0] as f32 - 1.0, size[1] as f32 - 1.0];
    let mut uv = *cell.uv;
    let mut positions = *cell.positions;
    for corner in 0..4 {
        let place = grid.corners[corner];
        for (across, (&axis, &extra)) in grid.across.iter().zip(&extra).enumerate() {
            let reach = f32::from(place[across]) * extra;
            positions[corner][axis] += reach;
            for (value, &step) in uv[corner].iter_mut().zip(&grid.steps[across]) {
                *value += reach * f32::from(step);
            }
        }
    }
    // The sampler repeats: a whole-sprite shift keeps every texel and keeps the
    // coordinates positive for the vertex format.
    for axis in 0..2 {
        let lowest = uv
            .iter()
            .map(|corner| corner[axis])
            .fold(f32::INFINITY, f32::min);
        let shift = lowest.floor();
        if shift < 0.0 {
            for corner in &mut uv {
                corner[axis] -= shift;
            }
        }
    }
    for corner in 0..4 {
        target.vertices.push(TerrainVertex::new(
            std::array::from_fn(|axis| positions[corner][axis] + cell.local[axis]),
            uv[corner],
            cell.key.layer,
            cell.color,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vanilla's baked up face of a full cube: corners and texture as `FaceBakery` lays them out.
    fn up_face() -> ([[f32; 3]; 4], [[f32; 2]; 4]) {
        (
            [
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [1.0, 1.0, 0.0],
            ],
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        )
    }

    #[test]
    fn only_whole_boundary_faces_with_one_sprite_per_block_have_a_grid() {
        let (positions, uv) = up_face();
        let grid = Grid::of(&positions, &uv, Some([0, 1, 0])).unwrap();
        assert_eq!((grid.normal, grid.across), (1, [0, 2]));
        // No cull side, an inset face (a slab top), half a sprite, a rotated
        // texture that still maps one sprite per block.
        assert!(Grid::of(&positions, &uv, None).is_none());
        let slab = positions.map(|[x, _, z]| [x, 0.5, z]);
        assert!(Grid::of(&slab, &uv, Some([0, 1, 0])).is_none());
        let half = uv.map(|[u, v]| [u * 0.5, v]);
        assert!(Grid::of(&positions, &half, Some([0, 1, 0])).is_none());
        let turned = [uv[1], uv[2], uv[3], uv[0]];
        assert!(Grid::of(&positions, &turned, Some([0, 1, 0])).is_some());
    }

    fn floor(colors: impl Fn(usize, usize) -> [f32; 3]) -> TerrainPass {
        let (positions, uv) = up_face();
        let mut merger = Merger::default();
        let grid = Grid::of(&positions, &uv, Some([0, 1, 0])).unwrap();
        for z in 0..16 {
            for x in 0..16 {
                merger.next_block();
                merger.push(
                    0,
                    grid,
                    &positions,
                    &uv,
                    7,
                    colors(x, z),
                    [x as f32, 64.0, z as f32],
                );
            }
        }
        let (mut solid, mut cutout) = (TerrainPass::default(), TerrainPass::default());
        merger.flush(&mut solid, &mut cutout);
        assert!(cutout.vertices.is_empty());
        solid
    }

    #[test]
    fn a_uniform_floor_is_one_quad_and_light_edges_split_it() {
        let one = floor(|_, _| [0.8; 3]);
        assert_eq!(one.quads(), 1);
        // The quad spans the section and repeats the sprite once per block.
        let corners: Vec<_> = one.vertices.iter().map(|v| (v.position, v.uv)).collect();
        let at = |x: f32, z: f32| TerrainVertex::new([x, 65.0, z], [0.0; 2], 0, [0.0; 3]).position;
        assert!(
            corners
                .iter()
                .any(|&(p, uv)| p[..3] == at(16.0, 16.0)[..3] && uv == [16 * 2048, 16 * 2048])
        );
        assert!(
            corners
                .iter()
                .any(|&(p, uv)| p[..3] == at(0.0, 0.0)[..3] && uv == [0, 0])
        );
        // A torch-lit square in the middle: the dark floor around it splits.
        let lit = floor(|x, z| {
            if (6..10).contains(&x) && (6..10).contains(&z) {
                [1.0; 3]
            } else {
                [0.5; 3]
            }
        });
        let bright = lit
            .vertices
            .chunks(4)
            .filter(|quad| quad[0].color[0] == 255)
            .count();
        assert_eq!(bright, 1);
        assert!(lit.quads() <= 5, "{}", lit.quads());
        // Every block's area is covered exactly once.
        let area: f32 = lit
            .vertices
            .chunks(4)
            .map(|quad| {
                let xs = quad.iter().map(|v| f32::from(v.position[0]));
                let zs = quad.iter().map(|v| f32::from(v.position[2]));
                let span = |values: Vec<f32>| {
                    (values.iter().cloned().fold(f32::MIN, f32::max)
                        - values.iter().cloned().fold(f32::MAX, f32::min))
                        / TerrainVertex::SCALE
                };
                span(xs.collect()) * span(zs.collect())
            })
            .sum();
        assert_eq!(area, 256.0);
    }

    #[test]
    fn mirrored_textures_shift_back_to_positive_coordinates() {
        // A face whose texture runs backwards along x: u falls 1 -> 0 as x rises.
        let (positions, uv) = up_face();
        let mirrored = uv.map(|[u, v]| [1.0 - u, v]);
        let grid = Grid::of(&positions, &mirrored, Some([0, 1, 0])).unwrap();
        let mut merger = Merger::default();
        for x in 0..3 {
            merger.next_block();
            merger.push(
                1,
                grid,
                &positions,
                &mirrored,
                2,
                [1.0; 3],
                [x as f32, 0.0, 0.0],
            );
        }
        let (mut solid, mut cutout) = (TerrainPass::default(), TerrainPass::default());
        merger.flush(&mut solid, &mut cutout);
        assert_eq!((solid.quads(), cutout.quads()), (0, 1));
        // u spans three sprites; the corner at x = 0 keeps its larger value.
        let us: Vec<u16> = cutout.vertices.iter().map(|v| v.uv[0]).collect();
        assert_eq!(
            *us.iter().max().unwrap() - *us.iter().min().unwrap(),
            3 * 2048
        );
        let at_origin = cutout
            .vertices
            .iter()
            .find(|v| {
                v.position[0] == TerrainVertex::new([0.0; 3], [0.0; 2], 0, [0.0; 3]).position[0]
            })
            .unwrap();
        assert_eq!(at_origin.uv[0], 3 * 2048);
    }

    #[test]
    fn an_overlay_on_the_same_plane_stays_after_its_base() {
        let (positions, uv) = up_face();
        let grid = Grid::of(&positions, &uv, Some([0, 1, 0])).unwrap();
        let mut merger = Merger::default();
        for x in 0..2 {
            merger.next_block();
            merger.push(0, grid, &positions, &uv, 1, [1.0; 3], [x as f32, 0.0, 0.0]);
            merger.push(0, grid, &positions, &uv, 2, [0.5; 3], [x as f32, 0.0, 0.0]);
        }
        let (mut solid, mut cutout) = (TerrainPass::default(), TerrainPass::default());
        merger.flush(&mut solid, &mut cutout);
        let layers: Vec<u16> = solid
            .vertices
            .chunks(4)
            .map(|quad| quad[0].position[3])
            .collect();
        assert_eq!(layers, [1, 2]);
    }
}
