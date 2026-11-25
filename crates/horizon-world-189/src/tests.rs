use super::*;
use horizon_protocol_47::write_varint;

fn payload(mask: u16, sky: bool, full: bool) -> Vec<u8> {
    let count = mask.count_ones() as usize;
    let mut data =
        vec![0; count * (10240 + if sky { 2048 } else { 0 }) + if full { 256 } else { 0 }];
    for ordinal in 0..count {
        data[ordinal * 8192..ordinal * 8192 + 2]
            .copy_from_slice(&(0x1234u16 + ordinal as u16).to_le_bytes());
        data[count * 8192 + ordinal * 2048] = 0xa3;
        if sky {
            data[count * 10240 + ordinal * 2048] = 0xb4;
        }
    }
    if full {
        let start = data.len() - 256;
        for index in 0..256 {
            data[start + index] = index as u8;
        }
    }
    data
}

fn chunk(x: i32, z: i32, mask: u16, full: bool, data: &[u8]) -> Vec<u8> {
    let mut body = vec![0x21];
    body.extend_from_slice(&x.to_be_bytes());
    body.extend_from_slice(&z.to_be_bytes());
    body.push(u8::from(full));
    body.extend_from_slice(&mask.to_be_bytes());
    write_varint(&mut body, data.len() as i32);
    body.extend_from_slice(data);
    body
}

#[test]
fn sparse_sections_decode_little_endian_states_and_light_planes() {
    let mut world = World::default();
    world.apply(
        decode_chunk(
            &chunk(-1, -2, 0x8004, true, &payload(0x8004, true, true)),
            true,
        )
        .unwrap(),
    );
    let column = world.column(-1, -2).unwrap();
    assert!(column.section(0).is_none());
    let low = column.section(2).unwrap();
    assert_eq!(low.state(0, 0, 0), Some(BlockState(0x1234)));
    assert_eq!(low.state(0, 0, 0).unwrap().block_id(), 0x123);
    assert_eq!(low.state(0, 0, 0).unwrap().metadata(), 4);
    assert_eq!(low.light(0, 0, 0), Some((3, 4)));
    assert_eq!(low.light(1, 0, 0), Some((10, 11)));
    assert_eq!(
        column.section(15).unwrap().state(0, 0, 0),
        Some(BlockState(0x1235))
    );
    assert_eq!(column.biome(3, 7), Some(115));
    assert_eq!(world.block(-16, 32, -32), Some(BlockState(0x1234)));
    assert_eq!(world.block(-1, 32, -17), Some(BlockState(0)));
    assert_eq!(world.block(-16, 0, -32), Some(BlockState(0)));
    assert_eq!(world.block(0, 32, -32), None);
    assert_eq!(world.block(-16, 256, -32), None);
    assert_eq!(low.state(16, 0, 0), None);
}

#[test]
fn partial_updates_preserve_omitted_sections_and_full_updates_clear_them() {
    let mut world = World::default();
    world.apply(decode_chunk(&chunk(0, 0, 3, true, &payload(3, false, true)), false).unwrap());
    world.apply(decode_chunk(&chunk(0, 0, 1, false, &payload(1, false, false)), false).unwrap());
    assert!(world.column(0, 0).unwrap().section(1).is_some());
    assert_eq!(world.column(0, 0).unwrap().biome(3, 7), Some(115));
    world.apply(decode_chunk(&chunk(0, 0, 1, true, &payload(1, false, true)), false).unwrap());
    assert!(world.column(0, 0).unwrap().section(1).is_none());
    assert_eq!(
        world
            .column(0, 0)
            .unwrap()
            .section(0)
            .unwrap()
            .light(0, 0, 0),
        Some((3, 0))
    );
}

#[test]
fn ground_up_empty_mask_removes_a_loaded_column() {
    let mut world = World::default();
    world.apply(decode_chunk(&chunk(0, 0, 1, true, &payload(1, true, true)), true).unwrap());
    assert_eq!(world.block(0, 0, 0), Some(BlockState(0x1234)));
    // bcy.a(go): ground-up, mask 0, biome bytes still attached.
    world.apply(decode_chunk(&chunk(0, 0, 0, true, &payload(0, true, true)), true).unwrap());
    assert!(world.column(0, 0).is_none());
    world.apply(decode_chunk(&chunk(0, 0, 1, false, &payload(1, true, false)), true).unwrap());
    assert!(world.column(0, 0).is_none());
}

#[test]
fn state_index_is_y_then_z_then_x() {
    let mut data = payload(1, false, true);
    let index = (5 * 256 + 7 * 16 + 3) * 2;
    data[index..index + 2].copy_from_slice(&0xabcdu16.to_le_bytes());
    let mut world = World::default();
    world.apply(decode_chunk(&chunk(0, 0, 1, true, &data), false).unwrap());
    assert_eq!(world.block(3, 5, 7), Some(BlockState(0xabcd)));
    assert_eq!(world.block(7, 5, 3), Some(BlockState(0)));
}

fn bulk() -> Vec<u8> {
    let mut body = vec![0x26, 1, 2];
    for (x, z, mask) in [(1i32, -2i32, 1u16), (-3, 4, 0x8000)] {
        body.extend_from_slice(&x.to_be_bytes());
        body.extend_from_slice(&z.to_be_bytes());
        body.extend_from_slice(&mask.to_be_bytes());
    }
    body.extend(payload(1, true, true));
    body.extend(payload(0x8000, true, true));
    body
}

#[test]
fn bulk_layouts_split_columns_and_validate_every_byte() {
    let bytes = bulk();
    let updates = decode_bulk(&bytes).unwrap();
    assert_eq!(updates.len(), 2);
    let mut world = World::default();
    for update in updates {
        world.apply(update);
    }
    assert!(world.column(1, -2).unwrap().section(0).is_some());
    assert!(world.column(-3, 4).unwrap().section(15).is_some());
    assert!(decode_bulk(&bytes[..bytes.len() - 1]).is_err());
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode_bulk(&trailing).is_err());
    assert!(decode_bulk(&[0x26, 1, 0xff, 0xff, 0xff, 0xff, 7]).is_err());
}

#[test]
fn single_chunk_rejects_wrong_dimension_and_malformed_lengths() {
    let data = payload(1, true, true);
    let body = chunk(0, 0, 1, true, &data);
    assert!(decode_chunk(&body, false).is_err());
    assert!(decode_chunk(&body[..body.len() - 1], true).is_err());
    let mut trailing = body.clone();
    trailing.push(0);
    assert!(decode_chunk(&trailing, true).is_err());
    assert!(decode_chunk(&[0x26], true).is_err());
    assert!(decode_bulk(&[0x26, 0, 0]).unwrap().is_empty());
}

fn loaded_world(blocks: &[([i32; 3], u16)]) -> World {
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
    for &([x, y, z], state) in blocks {
        assert!(world.set_block(x, y, z, BlockState(state)));
    }
    world
}

#[test]
fn block_changes_unpack_signed_positions_and_validate_counts() {
    let (x, y, z) = (-3_000_000i64, 255i64, -1i64);
    let packed = ((x & 0x3ff_ffff) << 38) | ((y & 0xfff) << 26) | (z & 0x3ff_ffff);
    let mut body = vec![0x23];
    body.extend_from_slice(&packed.to_be_bytes());
    write_varint(&mut body, (53 << 4) | 3);
    assert_eq!(
        decode_block_change(&body).unwrap(),
        BlockChange {
            x: -3_000_000,
            y: 255,
            z: -1,
            state: (53 << 4) | 3
        }
    );
    body.push(0);
    assert!(decode_block_change(&body).is_err());
    let mut multi = vec![0x22];
    multi.extend_from_slice(&(-1i32).to_be_bytes());
    multi.extend_from_slice(&2i32.to_be_bytes());
    write_varint(&mut multi, 1);
    multi.extend_from_slice(&[0xf3, 70]);
    write_varint(&mut multi, 1 << 4);
    assert_eq!(
        decode_multi_block_change(&multi).unwrap(),
        [BlockChange {
            x: -1,
            y: 70,
            z: 35,
            state: 1 << 4
        }]
    );
    multi[9] = 100;
    assert!(decode_multi_block_change(&multi).is_err());
}

#[test]
fn set_block_creates_sections_and_ignores_unloaded_columns() {
    let mut world = loaded_world(&[]);
    assert_eq!(world.block(5, 70, 5), Some(BlockState(0)));
    assert!(world.set_block(5, 70, 5, BlockState(1 << 4)));
    assert_eq!(world.block(5, 70, 5), Some(BlockState(1 << 4)));
    assert_eq!(world.light(5, 71, 5), (0, 15));
    assert!(!world.set_block(100, 70, 100, BlockState(1 << 4)));
    assert!(!world.set_block(0, 256, 0, BlockState(1 << 4)));
}

#[test]
fn neighbour_properties_follow_vanilla_actual_state() {
    use variants::{Neighbours, stairs_shape, variant};
    // Oak fence (85) next to stone and a nether brick fence (113).
    let world = loaded_world(&[
        ([0, 64, 0], 85 << 4),
        ([1, 64, 0], 1 << 4),
        ([-1, 64, 0], 113 << 4),
        ([0, 64, 1], 107 << 4),
    ]);
    let fence = variant(&world, [0, 64, 0], BlockState(85 << 4)).unwrap();
    assert_eq!(fence.file, "fence");
    assert_eq!(fence.key, "east=true,north=false,south=true,west=false");
    // Stairs facing east (meta 0) with a north-facing stair behind them.
    let world = loaded_world(&[([0, 64, 0], 53 << 4), ([1, 64, 0], (53 << 4) | 3)]);
    let near = Neighbours::new(&world, [0, 64, 0]);
    assert_eq!(stairs_shape(&near, BlockState(53 << 4)), "outer_left");
    // Door halves share facing/open from the lower half and hinge from the upper.
    let world = loaded_world(&[
        ([0, 64, 0], (64 << 4) | 4 | 1),
        ([0, 65, 0], (64 << 4) | 8 | 1),
    ]);
    let upper = variant(&world, [0, 65, 0], BlockState((64 << 4) | 8 | 1)).unwrap();
    assert_eq!(upper.key, "facing=south,half=upper,hinge=right,open=true");
    // Snow above grass turns it snowy.
    let world = loaded_world(&[([0, 64, 0], 2 << 4), ([0, 65, 0], 78 << 4)]);
    assert_eq!(
        variant(&world, [0, 64, 0], BlockState(2 << 4)).unwrap().key,
        "snowy=true"
    );
    assert_eq!(variant(&world, [0, 64, 0], BlockState(9 << 4)), None);
}

#[test]
fn open_sky_is_fully_lit_and_heights_follow_block_changes() {
    let mut world = loaded_world(&[]);
    // The test sections carry no sky light; the heightmap supplies it.
    assert_eq!(world.height(3, 3), Some(0));
    assert!(world.set_block(3, 70, 3, BlockState(1 << 4)));
    assert_eq!(world.height(3, 3), Some(71));
    assert_eq!(world.light(3, 71, 3).1, 15);
    assert!(world.set_block(3, 40, 3, BlockState(44 << 4)));
    assert_eq!(world.height(3, 3), Some(71));
    assert!(world.set_block(3, 70, 3, BlockState(0)));
    assert_eq!(world.height(3, 3), Some(41));
    assert_eq!(world.light(3, 41, 3).1, 15);
}

#[test]
fn sections_are_stored_compactly_and_block_changes_grow_them() {
    // Stone below y 8, air above, sky light 15 above and 0 below in every section.
    let mut world = World::default();
    let mut data = vec![0u8; 10240 + 2048 + 256];
    for index in 0..2048 {
        data[index * 2..index * 2 + 2].copy_from_slice(&(1u16 << 4).to_le_bytes());
    }
    data[10240 + 1024..10240 + 2048].fill(0xff);
    world.apply(decode_chunk(&chunk(3, 4, 1, true, &data), true).unwrap());
    let column = world.column(3, 4).unwrap();
    // Two states in 4 bits plus a two-entry palette; uniform block light; mixed sky light.
    assert_eq!(column.heap_bytes(), 2048 + 4 + 2048);
    assert!(column.heap_bytes() < (8192 + 2048 + 2048) / 2);
    assert_eq!(world.light(48, 3, 64), (0, 0));
    assert_eq!(world.light(48, 12, 64), (0, 15));
    // Every distinct state a block change brings widens the palette, never loses a block.
    for n in 0..300u16 {
        let (x, z) = (48 + i32::from(n % 16), 64 + i32::from(n / 16 % 16));
        let y = 8 + i32::from(n / 256);
        assert!(world.set_block(x, y, z, BlockState((n + 2) << 4)));
    }
    for n in 0..300u16 {
        let (x, z) = (48 + i32::from(n % 16), 64 + i32::from(n / 16 % 16));
        let y = 8 + i32::from(n / 256);
        assert_eq!(world.block(x, y, z), Some(BlockState((n + 2) << 4)));
    }
    assert_eq!(world.block(50, 2, 70), Some(BlockState(1 << 4)));
    assert_eq!(world.block(50, 15, 70), Some(BlockState(0)));
    // A change in a section the server never sent makes an empty, sky-lit one.
    assert!(world.set_block(48, 200, 64, BlockState(1 << 4)));
    let made = world.column(3, 4).unwrap().section(12).unwrap();
    assert_eq!(made.light(1, 8, 0), Some((0, 15)));
    assert_eq!(made.heap_bytes(), 2048 + 4);
}

// Vanilla `NetHandlerPlayClient.handleChunkData` (bcy.a(go)) unloads when the
// packet is ground-up with a zero section mask (go.e() && go.d() == 0), even
// though a vanilla server still sends the 256 biome bytes with it.
#[test]
fn ground_up_empty_mask_unloads_even_with_biome_bytes() {
    assert!(matches!(
        decode_chunk(&chunk(4, -5, 0, true, &payload(0, true, true)), true).unwrap(),
        ColumnUpdate::Unload { x: 4, z: -5 }
    ));
    assert!(matches!(
        decode_chunk(&chunk(4, -5, 0, true, &[]), true).unwrap(),
        ColumnUpdate::Unload { x: 4, z: -5 }
    ));
}
