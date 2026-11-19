//! Protocol 47 fields that drive the local player's simulation: entity
//! actions out; health, velocity, explosion knockback, attributes and potion
//! effects in. Decoders take the packet's fields after its ID.
use crate::{invalid, read_string, read_varint, write_varint};
use std::io::{self, Cursor, Read};

pub const START_SNEAKING: i32 = 0;
pub const STOP_SNEAKING: i32 = 1;
pub const START_SPRINTING: i32 = 3;
pub const STOP_SPRINTING: i32 = 4;

/// Serverbound 0x0B `C0BPacketEntityAction`.
pub fn entity_action(entity_id: i32, action: i32) -> Vec<u8> {
    let mut body = vec![0x0b];
    write_varint(&mut body, entity_id);
    write_varint(&mut body, action);
    write_varint(&mut body, 0);
    body
}

fn bytes<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn f32_be(input: &mut impl Read) -> io::Result<f32> {
    Ok(f32::from_be_bytes(bytes(input)?))
}

fn finish(input: &Cursor<&[u8]>) -> io::Result<()> {
    if input.position() != input.get_ref().len() as u64 {
        return Err(invalid("Trailing player packet bytes"));
    }
    Ok(())
}

fn remaining(input: &Cursor<&[u8]>) -> usize {
    input
        .get_ref()
        .len()
        .saturating_sub(input.position() as usize)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Health {
    pub health: f32,
    pub food: i32,
    pub saturation: f32,
}

/// Clientbound 0x06.
pub fn decode_health(fields: &[u8]) -> io::Result<Health> {
    let mut input = Cursor::new(fields);
    let health = Health {
        health: f32_be(&mut input)?,
        food: read_varint(&mut input)?,
        saturation: f32_be(&mut input)?,
    };
    finish(&input)?;
    Ok(health)
}

/// Clientbound 0x12: entity ID and velocity in 1/8000 blocks per tick.
pub fn decode_velocity(fields: &[u8]) -> io::Result<(i32, [i16; 3])> {
    let mut input = Cursor::new(fields);
    let entity = read_varint(&mut input)?;
    let velocity = [
        i16::from_be_bytes(bytes(&mut input)?),
        i16::from_be_bytes(bytes(&mut input)?),
        i16::from_be_bytes(bytes(&mut input)?),
    ];
    finish(&input)?;
    Ok((entity, velocity))
}

/// Clientbound 0x27: the player's knockback after the affected records.
pub fn decode_explosion(fields: &[u8]) -> io::Result<[f32; 3]> {
    let mut input = Cursor::new(fields);
    for _ in 0..4 {
        f32_be(&mut input)?;
    }
    let count = i32::from_be_bytes(bytes(&mut input)?);
    if count < 0 || count as usize > remaining(&input) / 3 {
        return Err(invalid("Invalid explosion record count"));
    }
    input.set_position(input.position() + count as u64 * 3);
    let motion = [
        f32_be(&mut input)?,
        f32_be(&mut input)?,
        f32_be(&mut input)?,
    ];
    finish(&input)?;
    Ok(motion)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub key: String,
    pub base: f64,
    /// UUID, amount, operation.
    pub modifiers: Vec<(u128, f64, u8)>,
}

/// Clientbound 0x20 `S20PacketEntityProperties`.
pub fn decode_properties(fields: &[u8]) -> io::Result<(i32, Vec<Property>)> {
    let mut input = Cursor::new(fields);
    let entity = read_varint(&mut input)?;
    let count = i32::from_be_bytes(bytes(&mut input)?);
    // Each property needs at least a one-byte string, a double and a VarInt.
    if count < 0 || count as usize > remaining(&input) / 10 {
        return Err(invalid("Invalid entity property count"));
    }
    let mut properties = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let key = read_string(&mut input, 64)?;
        let base = f64::from_be_bytes(bytes(&mut input)?);
        let modifiers = read_varint(&mut input)?;
        if modifiers < 0 || modifiers as usize > remaining(&input) / 25 {
            return Err(invalid("Invalid attribute modifier count"));
        }
        let mut list = Vec::with_capacity(modifiers as usize);
        for _ in 0..modifiers {
            let uuid = u128::from_be_bytes(bytes(&mut input)?);
            let amount = f64::from_be_bytes(bytes(&mut input)?);
            let operation = bytes::<1>(&mut input)?[0];
            list.push((uuid, amount, operation));
        }
        properties.push(Property {
            key,
            base,
            modifiers: list,
        });
    }
    finish(&input)?;
    Ok((entity, properties))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub entity: i32,
    pub effect: u8,
    pub amplifier: i8,
    pub duration: i32,
}

/// Clientbound 0x1D. The amplifier is a signed byte on the wire.
pub fn decode_effect(fields: &[u8]) -> io::Result<Effect> {
    let mut input = Cursor::new(fields);
    let effect = Effect {
        entity: read_varint(&mut input)?,
        effect: bytes::<1>(&mut input)?[0],
        amplifier: bytes::<1>(&mut input)?[0] as i8,
        duration: read_varint(&mut input)?,
    };
    bytes::<1>(&mut input)?;
    finish(&input)?;
    Ok(effect)
}

/// Clientbound 0x1E: entity ID and effect ID.
pub fn decode_remove_effect(fields: &[u8]) -> io::Result<(i32, u8)> {
    let mut input = Cursor::new(fields);
    let entity = read_varint(&mut input)?;
    let effect = bytes::<1>(&mut input)?[0];
    finish(&input)?;
    Ok((entity, effect))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_fields_round_trip_and_reject_bad_counts() {
        assert_eq!(entity_action(300, START_SPRINTING), [0x0b, 0xac, 2, 3, 0]);
        let mut health = Vec::new();
        health.extend_from_slice(&20.0f32.to_be_bytes());
        health.push(18);
        health.extend_from_slice(&5.0f32.to_be_bytes());
        assert_eq!(decode_health(&health).unwrap().food, 18);
        assert!(decode_health(&health[..8]).is_err());
        let velocity = [7, 0x1f, 0x40, 0xff, 0x38, 0, 0];
        assert_eq!(decode_velocity(&velocity).unwrap(), (7, [8000, -200, 0]));
        let mut explosion = vec![0; 16];
        explosion.extend_from_slice(&1i32.to_be_bytes());
        explosion.extend_from_slice(&[1, 2, 3]);
        for value in [0.5f32, 0.25, -1.0] {
            explosion.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(decode_explosion(&explosion).unwrap(), [0.5, 0.25, -1.0]);
        explosion[19] = 200;
        assert!(decode_explosion(&explosion).is_err());
        let mut properties = vec![9];
        properties.extend_from_slice(&1i32.to_be_bytes());
        properties.push(21);
        properties.extend_from_slice(b"generic.movementSpeed");
        properties.extend_from_slice(&0.1f64.to_be_bytes());
        properties.push(1);
        properties.extend_from_slice(&0x662A6B8D_DA3E_4C1C_8813_96EA6097278Du128.to_be_bytes());
        properties.extend_from_slice(&0.3f64.to_be_bytes());
        properties.push(2);
        let (entity, decoded) = decode_properties(&properties).unwrap();
        assert_eq!(entity, 9);
        assert_eq!(decoded[0].modifiers[0].2, 2);
        assert_eq!(
            decode_effect(&[9, 1, 0xff, 20, 0]).unwrap(),
            Effect {
                entity: 9,
                effect: 1,
                amplifier: -1,
                duration: 20
            }
        );
        assert_eq!(decode_remove_effect(&[9, 8]).unwrap(), (9, 8));
    }
}
