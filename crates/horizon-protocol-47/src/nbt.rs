//! Bounded 1.8 network NBT, item slots and entity metadata. Vanilla limits
//! packet NBT with `NBTSizeTracker(2097152)` and nesting depth 512; both are
//! enforced before any peer-controlled allocation.
use crate::{invalid, read_string};
use std::io::{self, Cursor, Read};

const MAX_DEPTH: usize = 512;
const MAX_BYTES: u64 = 2_097_152;

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
}

impl Tag {
    pub fn get(&self, name: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(entries) => entries
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, tag)| tag),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        match *self {
            Tag::Byte(value) => Some(i32::from(value)),
            Tag::Short(value) => Some(i32::from(value)),
            Tag::Int(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_list(&self) -> &[Tag] {
        match self {
            Tag::List(values) => values,
            _ => &[],
        }
    }
}

struct Reader<'a, 'b> {
    input: &'a mut Cursor<&'b [u8]>,
    start: u64,
}

impl Reader<'_, '_> {
    fn budget(&self, extra: u64) -> io::Result<()> {
        let used = self.input.position() - self.start;
        let left = self.input.get_ref().len() as u64 - self.input.position();
        if used + extra > MAX_BYTES || extra > left {
            return Err(invalid("NBT exceeds its size limit or the packet"));
        }
        Ok(())
    }

    fn bytes<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut bytes = [0; N];
        self.input.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn string(&mut self) -> io::Result<String> {
        let length = u16::from_be_bytes(self.bytes()?);
        self.budget(u64::from(length))?;
        let mut bytes = vec![0; usize::from(length)];
        self.input.read_exact(&mut bytes)?;
        // Java modified UTF-8 is close enough for names and sign text.
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn length(&mut self, element: u64) -> io::Result<usize> {
        let length = i32::from_be_bytes(self.bytes()?);
        if length < 0 {
            return Err(invalid("Negative NBT length"));
        }
        self.budget(length as u64 * element)?;
        Ok(length as usize)
    }

    fn payload(&mut self, kind: u8, depth: usize) -> io::Result<Tag> {
        if depth > MAX_DEPTH {
            return Err(invalid("NBT nesting exceeds 512"));
        }
        Ok(match kind {
            1 => Tag::Byte(self.bytes::<1>()?[0] as i8),
            2 => Tag::Short(i16::from_be_bytes(self.bytes()?)),
            3 => Tag::Int(i32::from_be_bytes(self.bytes()?)),
            4 => Tag::Long(i64::from_be_bytes(self.bytes()?)),
            5 => Tag::Float(f32::from_be_bytes(self.bytes()?)),
            6 => Tag::Double(f64::from_be_bytes(self.bytes()?)),
            7 => {
                let length = self.length(1)?;
                let mut bytes = vec![0; length];
                self.input.read_exact(&mut bytes)?;
                Tag::ByteArray(bytes)
            }
            8 => Tag::String(self.string()?),
            9 => {
                let element = self.bytes::<1>()?[0];
                if element > 11 {
                    return Err(invalid("Unknown NBT list element type"));
                }
                let length = self.length(1)?;
                if element == 0 && length > 0 {
                    return Err(invalid("Non-empty NBT list of end tags"));
                }
                let mut values = Vec::with_capacity(length.min(1024));
                for _ in 0..length {
                    values.push(self.payload(element, depth + 1)?);
                }
                Tag::List(values)
            }
            10 => {
                let mut entries = Vec::new();
                loop {
                    let kind = self.bytes::<1>()?[0];
                    if kind == 0 {
                        break;
                    }
                    if kind > 11 {
                        return Err(invalid("Unknown NBT tag type"));
                    }
                    let name = self.string()?;
                    entries.push((name, self.payload(kind, depth + 1)?));
                }
                Tag::Compound(entries)
            }
            11 => {
                let length = self.length(4)?;
                let mut values = Vec::with_capacity(length);
                for _ in 0..length {
                    values.push(i32::from_be_bytes(self.bytes()?));
                }
                Tag::IntArray(values)
            }
            _ => return Err(invalid("Unknown NBT tag type")),
        })
    }
}

/// `PacketBuffer.readNBTTagCompoundFromBuffer`: a 0 byte means none,
/// otherwise a named root compound.
pub fn read_nbt(input: &mut Cursor<&[u8]>) -> io::Result<Option<Tag>> {
    let start = input.position();
    let mut reader = Reader { input, start };
    let kind = reader.bytes::<1>()?[0];
    if kind == 0 {
        return Ok(None);
    }
    if kind != 10 {
        return Err(invalid("Root NBT tag must be a compound"));
    }
    reader.string()?;
    reader.payload(10, 0).map(Some)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: i16,
    pub count: i8,
    pub damage: i16,
    pub nbt: Option<Tag>,
}

impl Item {
    /// Enchantment level from the `ench` list (`{id: short, lvl: short}`).
    pub fn enchantment(&self, id: i32) -> i32 {
        self.nbt
            .as_ref()
            .and_then(|tag| tag.get("ench"))
            .map_or(0, |list| {
                list.as_list()
                    .iter()
                    .filter(|entry| entry.get("id").and_then(Tag::as_int) == Some(id))
                    .filter_map(|entry| entry.get("lvl").and_then(Tag::as_int))
                    .max()
                    .unwrap_or(0)
            })
    }
}

/// `PacketBuffer.readItemStackFromBuffer`.
pub fn read_slot(input: &mut Cursor<&[u8]>) -> io::Result<Option<Item>> {
    let mut id = [0; 2];
    input.read_exact(&mut id)?;
    let id = i16::from_be_bytes(id);
    if id < 0 {
        return Ok(None);
    }
    let mut fields = [0; 3];
    input.read_exact(&mut fields)?;
    Ok(Some(Item {
        id,
        count: fields[0] as i8,
        damage: i16::from_be_bytes([fields[1], fields[2]]),
        nbt: read_nbt(input)?,
    }))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Meta {
    Byte(i8),
    Short(i16),
    Int(i32),
    Float(f32),
    String(String),
    Item(Option<Item>),
    Position([i32; 3]),
    Rotation([f32; 3]),
}

/// `DataWatcher.readWatchedListFromPacketBuffer`: `(type << 5) | index`
/// headers, terminated by 0x7F.
pub fn read_metadata(input: &mut Cursor<&[u8]>) -> io::Result<Vec<(u8, Meta)>> {
    let mut entries = Vec::new();
    loop {
        let mut header = [0];
        input.read_exact(&mut header)?;
        if header[0] == 0x7f {
            return Ok(entries);
        }
        if entries.len() >= 32 {
            return Err(invalid("Too many metadata entries"));
        }
        let index = header[0] & 0x1f;
        let mut four = [0; 4];
        let value = match header[0] >> 5 {
            0 => {
                let mut byte = [0];
                input.read_exact(&mut byte)?;
                Meta::Byte(byte[0] as i8)
            }
            1 => {
                let mut short = [0; 2];
                input.read_exact(&mut short)?;
                Meta::Short(i16::from_be_bytes(short))
            }
            2 => {
                input.read_exact(&mut four)?;
                Meta::Int(i32::from_be_bytes(four))
            }
            3 => {
                input.read_exact(&mut four)?;
                Meta::Float(f32::from_be_bytes(four))
            }
            4 => Meta::String(read_string(input, 32767)?),
            5 => Meta::Item(read_slot(input)?),
            6 => {
                let mut values = [0; 3];
                for value in &mut values {
                    input.read_exact(&mut four)?;
                    *value = i32::from_be_bytes(four);
                }
                Meta::Position(values)
            }
            _ => {
                let mut values = [0.0; 3];
                for value in &mut values {
                    input.read_exact(&mut four)?;
                    *value = f32::from_be_bytes(four);
                }
                Meta::Rotation(values)
            }
        };
        entries.push((index, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boots_with_depth_strider() -> Vec<u8> {
        let mut bytes = vec![0x01, 0x2d, 1, 0, 0, 10, 0, 0];
        // ench: list of compound, one entry {id: 8s, lvl: 3s}
        bytes.extend_from_slice(&[9, 0, 4]);
        bytes.extend_from_slice(b"ench");
        bytes.push(10);
        bytes.extend_from_slice(&1i32.to_be_bytes());
        bytes.extend_from_slice(&[2, 0, 2, b'i', b'd', 0, 8]);
        bytes.extend_from_slice(&[2, 0, 3, b'l', b'v', b'l', 0, 3]);
        bytes.push(0);
        bytes.push(0);
        bytes
    }

    #[test]
    fn slots_parse_enchantments_and_reject_malformed_nbt() {
        let bytes = boots_with_depth_strider();
        let item = read_slot(&mut Cursor::new(&bytes[..])).unwrap().unwrap();
        assert_eq!(item.id, 301);
        assert_eq!(item.enchantment(8), 3);
        assert_eq!(item.enchantment(0), 0);
        assert_eq!(
            read_slot(&mut Cursor::new(&[0xff, 0xff][..])).unwrap(),
            None
        );
        // A list claiming two billion entries is refused before allocating.
        let mut huge = vec![0x01, 0x2d, 1, 0, 0, 10, 0, 0, 9, 0, 1, b'x', 1];
        huge.extend_from_slice(&i32::MAX.to_be_bytes());
        assert!(read_slot(&mut Cursor::new(&huge[..])).is_err());
        let mut deep = vec![10, 0, 0];
        for _ in 0..600 {
            deep.extend_from_slice(&[10, 0, 1, b'a']);
        }
        assert!(read_nbt(&mut Cursor::new(&deep[..])).is_err());
    }

    #[test]
    fn metadata_reads_every_type_until_the_terminator() {
        let mut bytes = vec![0x00, 0x22, 0x21, 0x01, 0x2c, 0x66];
        bytes.extend_from_slice(&20.0f32.to_be_bytes());
        bytes.extend_from_slice(&[0x82, 3, b'B', b'o', b'b', 0x7f]);
        let entries = read_metadata(&mut Cursor::new(&bytes[..])).unwrap();
        assert_eq!(entries[0], (0, Meta::Byte(0x22)));
        assert_eq!(entries[1], (1, Meta::Short(300)));
        assert_eq!(entries[2], (6, Meta::Float(20.0)));
        assert_eq!(entries[3], (2, Meta::String("Bob".into())));
        assert!(read_metadata(&mut Cursor::new(&[0x00, 1][..])).is_err());
    }
}
