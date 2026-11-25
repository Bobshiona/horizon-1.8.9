use crate::{ColumnData, ColumnUpdate, Section};
use horizon_protocol_47::read_varint;
use std::io::{self, Cursor, Read};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn bytes<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut result = [0; N];
    input.read_exact(&mut result)?;
    Ok(result)
}

fn boolean(input: &mut impl Read) -> io::Result<bool> {
    Ok(bytes::<1>(input)?[0] != 0)
}

fn data_size(mask: u16, sky_light: bool, full: bool) -> usize {
    mask.count_ones() as usize * (8192 + 2048 + if sky_light { 2048 } else { 0 })
        + if full { 256 } else { 0 }
}

fn column(
    x: i32,
    z: i32,
    full: bool,
    mask: u16,
    sky_light: bool,
    data: &[u8],
) -> io::Result<ColumnUpdate> {
    if full && mask == 0 && data.is_empty() {
        return Ok(ColumnUpdate::Unload { x, z });
    }
    if data.len() != data_size(mask, sky_light, full) {
        return Err(invalid("Chunk column length mismatch"));
    }
    let count = mask.count_ones() as usize;
    let mut sections = std::array::from_fn(|_| None);
    let mut ordinal = 0;
    for (index, section) in sections.iter_mut().enumerate() {
        if mask & (1 << index) == 0 {
            continue;
        }
        let mut states = [0u16; 4096];
        for (state, bytes) in states.iter_mut().zip(
            data[ordinal * 8192..(ordinal + 1) * 8192]
                .as_chunks::<2>()
                .0,
        ) {
            *state = u16::from_le_bytes(*bytes);
        }
        let plane = |offset: usize| -> &[u8; 2048] {
            data[offset..offset + 2048]
                .try_into()
                .expect("2048 light bytes")
        };
        let block_light = plane(count * 8192 + ordinal * 2048);
        let sky = sky_light.then(|| plane(count * (8192 + 2048) + ordinal * 2048));
        *section = Some(Section::new(&states, block_light, sky));
        ordinal += 1;
    }
    let biomes = full.then(|| {
        data[data.len() - 256..]
            .to_vec()
            .into_boxed_slice()
            .try_into()
            .expect("256 biome bytes")
    });
    Ok(ColumnUpdate::Data {
        x,
        z,
        full,
        data: Box::new(ColumnData { sections, biomes }),
    })
}

/// Decode a complete clientbound 0x21 body including its packet ID. Skylight
/// presence comes from the current dimension, never from a guessed data size.
pub fn decode_chunk(body: &[u8], sky_light: bool) -> io::Result<ColumnUpdate> {
    let mut input = Cursor::new(body);
    if read_varint(&mut input)? != 0x21 {
        return Err(invalid("Expected chunk packet 0x21"));
    }
    let x = i32::from_be_bytes(bytes(&mut input)?);
    let z = i32::from_be_bytes(bytes(&mut input)?);
    let full = boolean(&mut input)?;
    let mask = u16::from_be_bytes(bytes(&mut input)?);
    let length = read_varint(&mut input)?;
    let payload = &body[input.position() as usize..];
    if length < 0 || length as usize != payload.len() {
        return Err(invalid("Chunk payload length mismatch"));
    }
    // Vanilla (bcy.a(go)) unloads on ground-up with no sections, whatever
    // follows: a vanilla server still appends the biome bytes.
    if full && mask == 0 {
        return Ok(ColumnUpdate::Unload { x, z });
    }
    column(x, z, full, mask, sky_light, payload)
}

/// Parse and validate every column before returning any updates to the world.
pub fn decode_bulk(body: &[u8]) -> io::Result<Vec<ColumnUpdate>> {
    let mut input = Cursor::new(body);
    if read_varint(&mut input)? != 0x26 {
        return Err(invalid("Expected bulk chunk packet 0x26"));
    }
    let sky_light = boolean(&mut input)?;
    let count = read_varint(&mut input)?;
    // Every entry requires a ten-byte header and 256 biome bytes, even with
    // no sections. Check before allocating peer-controlled metadata storage.
    if count < 0 || count as usize > body.len().saturating_sub(input.position() as usize) / 266 {
        return Err(invalid("Invalid bulk chunk count"));
    }
    let mut headers = Vec::with_capacity(count as usize);
    for _ in 0..count {
        headers.push((
            i32::from_be_bytes(bytes(&mut input)?),
            i32::from_be_bytes(bytes(&mut input)?),
            u16::from_be_bytes(bytes(&mut input)?),
        ));
    }
    let mut offset = input.position() as usize;
    let mut updates = Vec::with_capacity(count as usize);
    for (x, z, mask) in headers {
        let end = offset
            .checked_add(data_size(mask, sky_light, true))
            .ok_or_else(|| invalid("Bulk length overflow"))?;
        let data = body
            .get(offset..end)
            .ok_or_else(|| invalid("Truncated bulk chunk data"))?;
        updates.push(column(x, z, true, mask, sky_light, data)?);
        offset = end;
    }
    if offset != body.len() {
        return Err(invalid("Trailing bulk chunk data"));
    }
    Ok(updates)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockChange {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub state: u16,
}

fn block_state(input: &mut impl Read) -> io::Result<u16> {
    u16::try_from(read_varint(input)?).map_err(|_| invalid("Block state exceeds 16 bits"))
}

/// Decode a complete clientbound 0x23 body: packed position, VarInt state.
pub fn decode_block_change(body: &[u8]) -> io::Result<BlockChange> {
    let mut input = Cursor::new(body);
    if read_varint(&mut input)? != 0x23 {
        return Err(invalid("Expected block change packet 0x23"));
    }
    let packed = i64::from_be_bytes(bytes(&mut input)?);
    let state = block_state(&mut input)?;
    if input.position() != body.len() as u64 {
        return Err(invalid("Trailing block change bytes"));
    }
    Ok(BlockChange {
        x: (packed >> 38) as i32,
        y: ((packed << 26) >> 52) as i32,
        z: ((packed << 38) >> 38) as i32,
        state,
    })
}

/// Decode a complete clientbound 0x22 body. Every record is validated before
/// any is returned.
pub fn decode_multi_block_change(body: &[u8]) -> io::Result<Vec<BlockChange>> {
    let mut input = Cursor::new(body);
    if read_varint(&mut input)? != 0x22 {
        return Err(invalid("Expected multi block change packet 0x22"));
    }
    let chunk_x = i32::from_be_bytes(bytes(&mut input)?);
    let chunk_z = i32::from_be_bytes(bytes(&mut input)?);
    let count = read_varint(&mut input)?;
    // Each record needs at least three bytes.
    if count < 0 || count as usize > body.len().saturating_sub(input.position() as usize) / 3 {
        return Err(invalid("Invalid multi block change count"));
    }
    let mut changes = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let [horizontal, y] = bytes::<2>(&mut input)?;
        let state = block_state(&mut input)?;
        changes.push(BlockChange {
            x: chunk_x
                .wrapping_mul(16)
                .wrapping_add(i32::from(horizontal >> 4)),
            y: i32::from(y),
            z: chunk_z
                .wrapping_mul(16)
                .wrapping_add(i32::from(horizontal & 15)),
            state,
        });
    }
    if input.position() != body.len() as u64 {
        return Err(invalid("Trailing multi block change bytes"));
    }
    Ok(changes)
}
