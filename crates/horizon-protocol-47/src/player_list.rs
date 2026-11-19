//! Protocol 47 tab-list profiles, including signed texture properties.
use crate::{invalid, read_string, read_varint};
use std::io::{self, Cursor, Read};

#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    /// Preserved for skin/cape resolution without blocking the packet actor.
    pub properties: Vec<Property>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    Add {
        uuid: u128,
        profile: Profile,
        game_mode: i32,
        ping: i32,
        /// Tab-list name as a chat component (JSON), if the server set one.
        display: Option<String>,
    },
    GameMode(u128, i32),
    Latency(u128, i32),
    DisplayName(u128, Option<String>),
    Remove(u128),
}

fn byte(input: &mut impl Read) -> io::Result<u8> {
    let mut b = [0];
    input.read_exact(&mut b)?;
    Ok(b[0])
}

fn count(input: &mut Cursor<&[u8]>) -> io::Result<usize> {
    let n = read_varint(input)?;
    if n < 0 || n as u64 > input.get_ref().len() as u64 - input.position() {
        return Err(invalid("Invalid player list count"));
    }
    Ok(n as usize)
}

pub fn decode(fields: &[u8]) -> io::Result<Vec<Update>> {
    let input = &mut Cursor::new(fields);
    let action = read_varint(input)?;
    if !(0..=4).contains(&action) {
        return Err(invalid("Unknown player list action"));
    }
    let n = count(input)?;
    let mut updates = Vec::new();
    for _ in 0..n {
        let mut uuid = [0; 16];
        input.read_exact(&mut uuid)?;
        let uuid = u128::from_be_bytes(uuid);
        match action {
            0 => {
                let name = read_string(input, 16)?;
                let mut properties = Vec::new();
                for _ in 0..count(input)? {
                    let name = read_string(input, 32767)?;
                    let value = read_string(input, 32767)?;
                    let signature = if byte(input)? != 0 {
                        Some(read_string(input, 32767)?)
                    } else {
                        None
                    };
                    properties.push(Property {
                        name,
                        value,
                        signature,
                    });
                }
                let game_mode = read_varint(input)?;
                let ping = read_varint(input)?;
                let display = if byte(input)? != 0 {
                    Some(read_string(input, 32767)?)
                } else {
                    None
                };
                updates.push(Update::Add {
                    uuid,
                    profile: Profile { name, properties },
                    game_mode,
                    ping,
                    display,
                });
            }
            1 => updates.push(Update::GameMode(uuid, read_varint(input)?)),
            2 => updates.push(Update::Latency(uuid, read_varint(input)?)),
            3 => {
                let display = if byte(input)? != 0 {
                    Some(read_string(input, 32767)?)
                } else {
                    None
                };
                updates.push(Update::DisplayName(uuid, display));
            }
            4 => updates.push(Update::Remove(uuid)),
            _ => unreachable!(),
        }
    }
    if input.position() != fields.len() as u64 {
        return Err(invalid("Trailing player list bytes"));
    }
    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{write_string, write_varint};
    #[test]
    fn signed_profile_properties_and_removal_keep_packet_boundaries() {
        let mut bytes = vec![0, 1];
        bytes.extend_from_slice(&7u128.to_be_bytes());
        write_string(&mut bytes, "Builder", 16).unwrap();
        write_varint(&mut bytes, 1);
        write_string(&mut bytes, "textures", 32767).unwrap();
        write_string(&mut bytes, "encoded-skin-and-cape", 32767).unwrap();
        bytes.push(1);
        write_string(&mut bytes, "signature", 32767).unwrap();
        bytes.extend([0, 0, 0]);
        let updates = decode(&bytes).unwrap();
        let Update::Add { uuid, profile, .. } = &updates[0] else {
            panic!()
        };
        assert_eq!(*uuid, 7);
        assert_eq!(profile.name, "Builder");
        assert_eq!(
            profile.properties[0].signature.as_deref(),
            Some("signature")
        );
        bytes.pop();
        assert!(decode(&bytes).is_err());
        let mut remove = vec![4, 1];
        remove.extend_from_slice(&7u128.to_be_bytes());
        assert_eq!(decode(&remove).unwrap(), vec![Update::Remove(7)]);
    }
}
