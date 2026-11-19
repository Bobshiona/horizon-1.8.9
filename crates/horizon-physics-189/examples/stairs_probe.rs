//! Print the collision boxes of every stairs state with every combination of
//! air or stairs on its four sides, in the format of the private 1.8.9
//! reference harness, so the two outputs can be compared line for line.
use horizon_physics_189::shapes::{bb, collision_boxes, hi, lo};
use horizon_world_189::{BlockState, ColumnData, ColumnUpdate, World};
use std::io::{BufWriter, Write};

fn main() -> std::io::Result<()> {
    let mut world = World::default();
    for (x, z) in [(0, 0), (-1, 0), (0, -1), (-1, -1)] {
        world.apply(ColumnUpdate::Data {
            x,
            z,
            full: true,
            data: Box::new(ColumnData {
                sections: std::array::from_fn(|_| None),
                biomes: None,
            }),
        });
    }
    // North, south, east, west, as the reference harness orders them.
    let sides = [(0, -1), (0, 1), (1, 0), (-1, 0)];
    let mask = bb([-2.0, 60.0, -2.0], [3.0, 70.0, 3.0]);
    let mut out = BufWriter::new(std::io::stdout().lock());
    let mut boxes = Vec::new();
    for meta in 0..8u16 {
        for code in 0..6561u32 {
            let state = BlockState((53 << 4) | meta);
            world.set_block(0, 64, 0, state);
            let mut line = format!("{meta}:");
            let mut rest = code;
            for (index, (dx, dz)) in sides.into_iter().enumerate() {
                let n = rest % 9;
                rest /= 9;
                line.push_str(&n.to_string());
                if index < 3 {
                    line.push(',');
                }
                let neighbour = if n == 0 {
                    0
                } else {
                    (53 << 4) | (n as u16 - 1)
                };
                world.set_block(dx, 64, dz, BlockState(neighbour));
            }
            line.push(' ');
            boxes.clear();
            collision_boxes(&world, [0, 64, 0], state, mask, &mut boxes);
            for b in &boxes {
                let (l, h) = (lo(*b), hi(*b));
                line.push_str(&format!(
                    "[{:?} {:?} {:?} {:?} {:?} {:?}]",
                    l[0], l[1], l[2], h[0], h[1], h[2]
                ));
            }
            writeln!(out, "{line}")?;
        }
    }
    Ok(())
}
