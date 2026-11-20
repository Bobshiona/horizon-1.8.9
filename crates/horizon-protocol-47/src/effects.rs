//! Bounded visual fields from Protocol 47 S27 and S2A. No player simulation.
use crate::{invalid, read_varint};
use std::io::{self, Cursor, Read};

fn int(input: &mut impl Read) -> io::Result<i32> {
    let mut b = [0; 4];
    input.read_exact(&mut b)?;
    Ok(i32::from_be_bytes(b))
}
fn float(input: &mut impl Read) -> io::Result<f32> {
    let value = f32::from_bits(int(input)? as u32);
    if !value.is_finite() {
        return Err(invalid("Non-finite effect field"));
    }
    Ok(value)
}
fn vector(input: &mut impl Read) -> io::Result<[f32; 3]> {
    Ok([float(input)?, float(input)?, float(input)?])
}
fn end(input: &Cursor<&[u8]>) -> io::Result<()> {
    if input.position() != input.get_ref().len() as u64 {
        return Err(invalid("Trailing effect fields"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Explosion {
    pub position: [f32; 3],
    pub strength: f32,
    pub blocks: Vec<[i32; 3]>,
    pub motion: [f32; 3],
}

pub fn explosion(fields: &[u8]) -> io::Result<Explosion> {
    let mut input = Cursor::new(fields);
    let position = vector(&mut input)?;
    let strength = float(&mut input)?;
    let count = int(&mut input)?;
    if strength < 0.0
        || !(0..=65_536).contains(&count)
        || fields.len().saturating_sub(32) != count as usize * 3
        || position.iter().any(|v| v.abs() > 30_000_128.0)
    {
        return Err(invalid("Invalid explosion bounds"));
    }
    // S27 uses Java casts (toward zero), not floor, for the record origin.
    let origin = position.map(|v| v as i32);
    let mut blocks = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let mut offset = [0; 3];
        input.read_exact(&mut offset)?;
        blocks.push(std::array::from_fn(|a| {
            origin[a] + i32::from(offset[a] as i8)
        }));
    }
    let motion = vector(&mut input)?;
    end(&input)?;
    Ok(Explosion {
        position,
        strength,
        blocks,
        motion,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Particle {
    pub id: i32,
    pub long_distance: bool,
    pub position: [f32; 3],
    pub offset: [f32; 3],
    pub speed: f32,
    pub count: i32,
    pub data: Vec<i32>,
}

pub fn particle(fields: &[u8]) -> io::Result<Particle> {
    let mut input = Cursor::new(fields);
    let id = int(&mut input)?;
    if !(0..=41).contains(&id) {
        return Err(invalid("Unknown legacy particle"));
    }
    let mut flag = [0];
    input.read_exact(&mut flag)?;
    let position = vector(&mut input)?;
    let offset = vector(&mut input)?;
    let speed = float(&mut input)?;
    let count = int(&mut input)?;
    if count < 0 {
        return Err(invalid("Negative particle count"));
    }
    let mut data = Vec::new();
    for _ in 0..match id {
        36 => 2,
        37 | 38 => 1,
        _ => 0,
    } {
        data.push(read_varint(&mut input)?);
    }
    end(&input)?;
    Ok(Particle {
        id,
        long_distance: flag[0] != 0,
        position,
        offset,
        speed,
        count,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explosion_records_use_signed_offsets_and_truncated_origin() {
        let mut b = Vec::new();
        for f in [-1.5f32, 4.0, 2.75, 4.0] {
            b.extend(f.to_be_bytes());
        }
        b.extend(1i32.to_be_bytes());
        b.extend([255, 2, 128]);
        for f in [0.5f32, -0.25, 1.0] {
            b.extend(f.to_be_bytes());
        }
        let e = explosion(&b).unwrap();
        assert_eq!(e.blocks, vec![[-2, 6, -126]]);
        assert_eq!(e.motion, [0.5, -0.25, 1.0]);
        for len in 0..b.len() {
            assert!(explosion(&b[..len]).is_err());
        }
        b.push(0);
        assert!(explosion(&b).is_err());
    }
    #[test]
    fn particle_boundaries_extra_data_and_nonfinite_fields() {
        let mut b = 36i32.to_be_bytes().to_vec();
        b.push(1);
        b.extend([0; 28]);
        b.extend(3i32.to_be_bytes());
        b.extend([1, 2]);
        assert_eq!(particle(&b).unwrap().data, [1, 2]);
        for len in 0..b.len() {
            assert!(particle(&b[..len]).is_err());
        }
        b[5..9].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(particle(&b).is_err());
    }
}
