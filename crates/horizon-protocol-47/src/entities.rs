//! Protocol 47 entity, equipment, abilities and block-entity packets.
//! Positions are 1/32-block fixed point; angles are 1/256 turns.
use crate::{
    invalid,
    nbt::{Item, Meta, Tag, read_metadata, read_nbt, read_slot},
    read_string, read_varint,
};
use std::io::{self, Cursor, Read};

#[derive(Clone, Debug, PartialEq)]
pub enum EntityPacket {
    SpawnPlayer {
        id: i32,
        uuid: u128,
        position: [i32; 3],
        yaw: i8,
        pitch: i8,
        held_item: i16,
        metadata: Vec<(u8, Meta)>,
    },
    SpawnObject {
        id: i32,
        kind: u8,
        position: [i32; 3],
        pitch: i8,
        yaw: i8,
        data: i32,
        /// Present when `data` is positive: motion in 1/8000 blocks per tick.
        velocity: Option<[i16; 3]>,
    },
    SpawnMob {
        id: i32,
        kind: u8,
        position: [i32; 3],
        yaw: i8,
        pitch: i8,
        head_yaw: i8,
        metadata: Vec<(u8, Meta)>,
    },
    SpawnOrb {
        id: i32,
        position: [i32; 3],
    },
    Destroy(Vec<i32>),
    Move {
        id: i32,
        delta: Option<[i8; 3]>,
        look: Option<(i8, i8)>,
        on_ground: bool,
    },
    Teleport {
        id: i32,
        position: [i32; 3],
        yaw: i8,
        pitch: i8,
        on_ground: bool,
    },
    HeadLook {
        id: i32,
        yaw: i8,
    },
    Metadata {
        id: i32,
        entries: Vec<(u8, Meta)>,
    },
    Equipment {
        id: i32,
        slot: i16,
        item: Option<Item>,
    },
    Status {
        id: i32,
        status: i8,
    },
    Animation {
        id: i32,
        animation: u8,
    },
}

fn bytes<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn byte(input: &mut impl Read) -> io::Result<i8> {
    Ok(bytes::<1>(input)?[0] as i8)
}

fn int(input: &mut impl Read) -> io::Result<i32> {
    Ok(i32::from_be_bytes(bytes(input)?))
}

fn short(input: &mut impl Read) -> io::Result<i16> {
    Ok(i16::from_be_bytes(bytes(input)?))
}

fn fixed(input: &mut impl Read) -> io::Result<[i32; 3]> {
    Ok([int(input)?, int(input)?, int(input)?])
}

fn finish(input: &Cursor<&[u8]>) -> io::Result<()> {
    if input.position() != input.get_ref().len() as u64 {
        return Err(invalid("Trailing entity packet bytes"));
    }
    Ok(())
}

/// Decode an entity packet's fields (after the ID), or `None` for other IDs.
pub fn decode(id: i32, fields: &[u8]) -> io::Result<Option<EntityPacket>> {
    let mut input = Cursor::new(fields);
    let packet = match id {
        0x04 => EntityPacket::Equipment {
            id: read_varint(&mut input)?,
            slot: short(&mut input)?,
            item: read_slot(&mut input)?,
        },
        0x0b => EntityPacket::Animation {
            id: read_varint(&mut input)?,
            animation: bytes::<1>(&mut input)?[0],
        },
        0x0c => EntityPacket::SpawnPlayer {
            id: read_varint(&mut input)?,
            uuid: u128::from_be_bytes(bytes(&mut input)?),
            position: fixed(&mut input)?,
            yaw: byte(&mut input)?,
            pitch: byte(&mut input)?,
            held_item: short(&mut input)?,
            metadata: read_metadata(&mut input)?,
        },
        0x0e => {
            let id = read_varint(&mut input)?;
            let kind = bytes::<1>(&mut input)?[0];
            let position = fixed(&mut input)?;
            let pitch = byte(&mut input)?;
            let yaw = byte(&mut input)?;
            let data = int(&mut input)?;
            let velocity = if data > 0 {
                Some([short(&mut input)?, short(&mut input)?, short(&mut input)?])
            } else {
                None
            };
            EntityPacket::SpawnObject {
                id,
                kind,
                position,
                pitch,
                yaw,
                data,
                velocity,
            }
        }
        0x0f => {
            let id = read_varint(&mut input)?;
            let kind = bytes::<1>(&mut input)?[0];
            let position = fixed(&mut input)?;
            let yaw = byte(&mut input)?;
            let pitch = byte(&mut input)?;
            let head_yaw = byte(&mut input)?;
            for _ in 0..3 {
                short(&mut input)?;
            }
            EntityPacket::SpawnMob {
                id,
                kind,
                position,
                yaw,
                pitch,
                head_yaw,
                metadata: read_metadata(&mut input)?,
            }
        }
        0x11 => {
            let id = read_varint(&mut input)?;
            let position = fixed(&mut input)?;
            short(&mut input)?;
            EntityPacket::SpawnOrb { id, position }
        }
        0x13 => {
            let count = read_varint(&mut input)?;
            if count < 0 || count as usize > fields.len() {
                return Err(invalid("Invalid destroy count"));
            }
            let mut ids = Vec::with_capacity(count as usize);
            for _ in 0..count {
                ids.push(read_varint(&mut input)?);
            }
            EntityPacket::Destroy(ids)
        }
        0x15..=0x17 => {
            let id_value = read_varint(&mut input)?;
            let delta = (id != 0x16)
                .then(|| -> io::Result<[i8; 3]> {
                    Ok([byte(&mut input)?, byte(&mut input)?, byte(&mut input)?])
                })
                .transpose()?;
            let look = (id != 0x15)
                .then(|| -> io::Result<(i8, i8)> { Ok((byte(&mut input)?, byte(&mut input)?)) })
                .transpose()?;
            EntityPacket::Move {
                id: id_value,
                delta,
                look,
                on_ground: bytes::<1>(&mut input)?[0] != 0,
            }
        }
        0x18 => EntityPacket::Teleport {
            id: read_varint(&mut input)?,
            position: fixed(&mut input)?,
            yaw: byte(&mut input)?,
            pitch: byte(&mut input)?,
            on_ground: bytes::<1>(&mut input)?[0] != 0,
        },
        0x19 => EntityPacket::HeadLook {
            id: read_varint(&mut input)?,
            yaw: byte(&mut input)?,
        },
        0x1a => EntityPacket::Status {
            id: int(&mut input)?,
            status: byte(&mut input)?,
        },
        0x1c => EntityPacket::Metadata {
            id: read_varint(&mut input)?,
            entries: read_metadata(&mut input)?,
        },
        _ => return Ok(None),
    };
    finish(&input)?;
    Ok(Some(packet))
}

/// Byte angle to degrees, as `(float)(angle * 360) / 256.0F`.
pub fn angle(value: i8) -> f32 {
    (i32::from(value) * 360) as f32 / 256.0
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Abilities {
    pub invulnerable: bool,
    pub flying: bool,
    pub allow_flying: bool,
    pub creative: bool,
    pub fly_speed: f32,
    pub walk_speed: f32,
}

/// Clientbound 0x39.
pub fn decode_abilities(fields: &[u8]) -> io::Result<Abilities> {
    let mut input = Cursor::new(fields);
    let flags = bytes::<1>(&mut input)?[0];
    let abilities = Abilities {
        invulnerable: flags & 1 != 0,
        flying: flags & 2 != 0,
        allow_flying: flags & 4 != 0,
        creative: flags & 8 != 0,
        fly_speed: f32::from_be_bytes(bytes(&mut input)?),
        walk_speed: f32::from_be_bytes(bytes(&mut input)?),
    };
    finish(&input)?;
    Ok(abilities)
}

/// Serverbound 0x13 `C13PacketPlayerAbilities`.
pub fn encode_abilities(abilities: Abilities) -> Vec<u8> {
    let flags = u8::from(abilities.invulnerable)
        | u8::from(abilities.flying) << 1
        | u8::from(abilities.allow_flying) << 2
        | u8::from(abilities.creative) << 3;
    let mut body = vec![0x13, flags];
    body.extend_from_slice(&abilities.fly_speed.to_be_bytes());
    body.extend_from_slice(&abilities.walk_speed.to_be_bytes());
    body
}

/// Clientbound 0x2F Set Slot: window, slot, item.
pub fn decode_set_slot(fields: &[u8]) -> io::Result<(i8, i16, Option<Item>)> {
    let mut input = Cursor::new(fields);
    let window = byte(&mut input)?;
    let slot = short(&mut input)?;
    let item = read_slot(&mut input)?;
    finish(&input)?;
    Ok((window, slot, item))
}

/// Clientbound 0x30 Window Items: window and every slot.
pub fn decode_window_items(fields: &[u8]) -> io::Result<(u8, Vec<Option<Item>>)> {
    let mut input = Cursor::new(fields);
    let window = bytes::<1>(&mut input)?[0];
    let count = short(&mut input)?;
    if count < 0 || count as usize > fields.len() / 2 {
        return Err(invalid("Invalid window item count"));
    }
    let mut items = Vec::with_capacity(count as usize);
    for _ in 0..count {
        items.push(read_slot(&mut input)?);
    }
    finish(&input)?;
    Ok((window, items))
}

fn position(packed: i64) -> [i32; 3] {
    [
        (packed >> 38) as i32,
        ((packed << 26) >> 52) as i32,
        ((packed << 38) >> 38) as i32,
    ]
}

/// Clientbound 0x33 Update Sign: position and four JSON chat lines.
pub fn decode_sign(fields: &[u8]) -> io::Result<([i32; 3], [String; 4])> {
    let mut input = Cursor::new(fields);
    let at = position(i64::from_be_bytes(bytes(&mut input)?));
    let lines = [
        read_string(&mut input, 32767)?,
        read_string(&mut input, 32767)?,
        read_string(&mut input, 32767)?,
        read_string(&mut input, 32767)?,
    ];
    finish(&input)?;
    Ok((at, lines))
}

/// Clientbound 0x35 Update Block Entity: position, action, optional NBT.
pub fn decode_block_entity(fields: &[u8]) -> io::Result<([i32; 3], u8, Option<Tag>)> {
    let mut input = Cursor::new(fields);
    let at = position(i64::from_be_bytes(bytes(&mut input)?));
    let action = bytes::<1>(&mut input)?[0];
    let nbt = read_nbt(&mut input)?;
    finish(&input)?;
    Ok((at, action, nbt))
}

/// Clientbound 0x24 Block Action: position, the two event bytes and the
/// block id (`S24PacketBlockAction`). Chests send event 1 with the number
/// of players looking inside.
pub fn decode_block_action(fields: &[u8]) -> io::Result<([i32; 3], u8, u8, i32)> {
    let mut input = Cursor::new(fields);
    let at = position(i64::from_be_bytes(bytes(&mut input)?));
    let [event, parameter] = bytes::<2>(&mut input)?;
    let block = read_varint(&mut input)?;
    finish(&input)?;
    Ok((at, event, parameter, block))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write_varint;

    #[test]
    fn block_action_decodes_position_event_and_block() {
        let packed: i64 = (-5i64 << 38) | ((64i64 & 0xfff) << 26) | (12i64 & 0x3ff_ffff);
        let mut fields = packed.to_be_bytes().to_vec();
        fields.extend_from_slice(&[1, 2]);
        write_varint(&mut fields, 54);
        assert_eq!(
            decode_block_action(&fields).unwrap(),
            ([-5, 64, 12], 1, 2, 54)
        );
        fields.push(0);
        assert!(decode_block_action(&fields).is_err());
    }

    #[test]
    fn spawn_move_teleport_destroy_and_metadata_decode() {
        let mut spawn = Vec::new();
        write_varint(&mut spawn, 7);
        spawn.extend_from_slice(&0x0123u128.to_be_bytes());
        for value in [32i32, 64 * 32, -16] {
            spawn.extend_from_slice(&value.to_be_bytes());
        }
        spawn.extend_from_slice(&[64, 0xc0, 0, 0, 0x00, 0x20, 0x7f]);
        let Some(EntityPacket::SpawnPlayer {
            id,
            position,
            yaw,
            metadata,
            ..
        }) = decode(0x0c, &spawn).unwrap()
        else {
            panic!("spawn player");
        };
        assert_eq!((id, position), (7, [32, 2048, -16]));
        assert_eq!(angle(yaw), 90.0);
        assert_eq!(metadata, [(0, Meta::Byte(0x20))]);
        assert_eq!(
            decode(0x17, &[7, 1, 0xff, 2, 0x80, 10, 1]).unwrap(),
            Some(EntityPacket::Move {
                id: 7,
                delta: Some([1, -1, 2]),
                look: Some((-128, 10)),
                on_ground: true
            })
        );
        assert_eq!(
            decode(0x16, &[7, 1, 2, 0]).unwrap(),
            Some(EntityPacket::Move {
                id: 7,
                delta: None,
                look: Some((1, 2)),
                on_ground: false
            })
        );
        assert_eq!(
            decode(0x13, &[2, 7, 9]).unwrap(),
            Some(EntityPacket::Destroy(vec![7, 9]))
        );
        assert!(decode(0x13, &[100, 7]).is_err());
        assert!(decode(0x15, &[7, 1, 2, 3, 1, 9]).is_err());
        assert_eq!(decode(0x22, &[]).unwrap(), None);
    }

    #[test]
    fn abilities_round_trip_and_sign_positions_unpack() {
        let abilities = Abilities {
            invulnerable: false,
            flying: true,
            allow_flying: true,
            creative: false,
            fly_speed: 0.05,
            walk_speed: 0.1,
        };
        let encoded = encode_abilities(abilities);
        assert_eq!(encoded[..2], [0x13, 6]);
        assert_eq!(decode_abilities(&encoded[1..]).unwrap(), abilities);
        let mut sign = ((-5i64 & 0x3ff_ffff) << 38 | 70 << 26 | 3)
            .to_be_bytes()
            .to_vec();
        for line in ["\"a\"", "\"\"", "\"\"", "\"b\""] {
            sign.push(line.len() as u8);
            sign.extend_from_slice(line.as_bytes());
        }
        let (at, lines) = decode_sign(&sign).unwrap();
        assert_eq!(at, [-5, 70, 3]);
        assert_eq!(lines[3], "\"b\"");
    }
}
