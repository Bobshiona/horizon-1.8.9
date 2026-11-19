//! Minecraft 1.8.9 transport. Packet definitions follow Protocol 47.
//! Modern Horizon registries and packet layouts stay outside this crate.
#![forbid(unsafe_code)]

pub mod chat;
mod cipher;
pub mod entities;
mod frame;
pub mod interaction;
pub mod login;
pub mod nbt;
pub mod packet;
pub mod play;
pub mod player;
pub mod player_list;
#[cfg(test)]
mod tests;
pub mod text;

pub use frame::{Connection, MAX_FRAME, MAX_UNCOMPRESSED, PacketReader, PacketWriter};
use std::io::{self, Read};

pub const VERSION: i32 = 47;

pub(crate) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Java signed int VarInts use unsigned shifts of the two's-complement bits.
pub fn write_varint(output: &mut Vec<u8>, value: i32) {
    let mut bits = value as u32;
    while bits >= 0x80 {
        output.push(bits as u8 | 0x80);
        bits >>= 7;
    }
    output.push(bits as u8);
}

pub fn read_varint(input: &mut impl Read) -> io::Result<i32> {
    let mut result = 0u32;
    for index in 0..5 {
        let mut byte = [0];
        input.read_exact(&mut byte)?;
        if index == 4 && byte[0] & 0xf0 != 0 {
            return Err(invalid("VarInt exceeds 32 bits"));
        }
        result |= u32::from(byte[0] & 0x7f) << (index * 7);
        if byte[0] & 0x80 == 0 {
            return Ok(result as i32);
        }
    }
    Err(invalid("VarInt exceeds five bytes"))
}

pub fn write_string(output: &mut Vec<u8>, value: &str, max_units: usize) -> io::Result<()> {
    if value.encode_utf16().count() > max_units || value.len() > max_units.saturating_mul(4) {
        return Err(invalid("String exceeds its protocol limit"));
    }
    let length = i32::try_from(value.len()).map_err(|_| invalid("String length exceeds i32"))?;
    write_varint(output, length);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

pub fn read_string(input: &mut impl Read, max_units: usize) -> io::Result<String> {
    let length = read_varint(input)?;
    if length < 0 || length as usize > max_units.saturating_mul(4) {
        return Err(invalid("Invalid encoded String length"));
    }
    let mut bytes = vec![0; length as usize];
    input.read_exact(&mut bytes)?;
    let value = String::from_utf8(bytes).map_err(|_| invalid("String is invalid UTF-8"))?;
    if value.encode_utf16().count() > max_units {
        return Err(invalid("Decoded String exceeds its protocol limit"));
    }
    Ok(value)
}
