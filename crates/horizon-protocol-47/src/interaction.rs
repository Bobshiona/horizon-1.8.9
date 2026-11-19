//! Legacy user-driven interaction packets. All positions and enum values use
//! Protocol 47 widths; slot NBT is preserved in placement/click packets.
use crate::{
    invalid,
    nbt::{Item, write_slot},
    write_varint,
};
use std::io;

pub fn position(at: [i32; 3]) -> io::Result<[u8; 8]> {
    let [x, y, z] = at;
    if !(-33_554_432..33_554_432).contains(&x)
        || !(-33_554_432..33_554_432).contains(&z)
        || !(-2048..2048).contains(&y)
    {
        return Err(invalid("Block position exceeds packed coordinates"));
    }
    Ok((((i64::from(x) & 0x3ff_ffff) << 38)
        | ((i64::from(y) & 0xfff) << 26)
        | (i64::from(z) & 0x3ff_ffff))
        .to_be_bytes())
}

pub fn swing() -> Vec<u8> {
    vec![0x0a]
}

/// C02 action 0 interact, 1 attack, 2 interact-at with entity-relative f32s.
pub fn use_entity(id: i32, action: i32, hit: Option<[f32; 3]>) -> io::Result<Vec<u8>> {
    if !(0..=2).contains(&action) || (action == 2) != hit.is_some() {
        return Err(invalid("Invalid use entity action"));
    }
    let mut out = vec![0x02];
    write_varint(&mut out, id);
    write_varint(&mut out, action);
    if let Some(hit) = hit {
        for v in hit {
            if !v.is_finite() {
                return Err(invalid("Non-finite entity hit"));
            }
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    Ok(out)
}

pub fn held(slot: u8) -> io::Result<Vec<u8>> {
    if slot > 8 {
        return Err(invalid("Invalid hotbar slot"));
    }
    let mut out = vec![0x09];
    out.extend_from_slice(&i16::from(slot).to_be_bytes());
    Ok(out)
}

pub fn dig(action: u8, at: [i32; 3], face: u8) -> io::Result<Vec<u8>> {
    if action > 5 || face > 5 {
        return Err(invalid("Invalid digging action or face"));
    }
    let mut out = vec![0x07, action];
    out.extend(position(at)?);
    out.push(face);
    Ok(out)
}

pub fn place(at: [i32; 3], face: u8, hit: [f32; 3], item: Option<&Item>) -> io::Result<Vec<u8>> {
    if face > 5 && face != 255 {
        return Err(invalid("Invalid placement face"));
    }
    let mut out = vec![0x08];
    out.extend(position(at)?);
    out.push(face);
    write_slot(&mut out, item)?;
    for v in hit {
        if !v.is_finite() || !(0.0..=1.0).contains(&v) {
            return Err(invalid("Invalid placement hit"));
        }
        out.push((v * 16.0) as u8);
    }
    Ok(out)
}

pub fn close_window(window: u8) -> Vec<u8> {
    vec![0x0d, window]
}
pub fn confirm(window: u8, transaction: i16) -> Vec<u8> {
    let mut out = vec![0x0f, window];
    out.extend(transaction.to_be_bytes());
    out.push(1);
    out
}

pub fn click(
    window: u8,
    slot: i16,
    right: bool,
    transaction: i16,
    original: Option<&Item>,
) -> io::Result<Vec<u8>> {
    let mut out = vec![0x0e, window];
    out.extend(slot.to_be_bytes());
    out.push(u8::from(right));
    out.extend(transaction.to_be_bytes());
    out.push(0);
    write_slot(&mut out, original)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::{Tag, read_slot};
    use std::io::Cursor;
    #[test]
    fn interactions_have_legacy_widths_and_preserve_stack_nbt() {
        assert_eq!(use_entity(300, 1, None).unwrap(), [2, 0xac, 2, 1]);
        assert_eq!(held(8).unwrap(), [9, 0, 8]);
        assert!(held(9).is_err());
        assert_eq!(
            dig(0, [-1, 64, -1], 1).unwrap(),
            [7, 0, 255, 255, 255, 193, 3, 255, 255, 255, 1]
        );
        let item = Item {
            id: 276,
            count: 1,
            damage: 7,
            nbt: Some(Tag::Compound(vec![(
                "ench".into(),
                Tag::List(vec![Tag::Compound(vec![
                    ("id".into(), Tag::Short(16)),
                    ("lvl".into(), Tag::Short(1)),
                ])]),
            )])),
        };
        let packet = place([1, 2, 3], 1, [0.25, 1.0, 0.5], Some(&item)).unwrap();
        let mut cursor = Cursor::new(&packet[10..]);
        assert_eq!(read_slot(&mut cursor).unwrap(), Some(item));
        assert_eq!(&packet[packet.len() - 3..], &[4, 16, 8]);
        assert!(place([0; 3], 1, [f32::NAN, 0.0, 0.0], None).is_err());
    }
}
