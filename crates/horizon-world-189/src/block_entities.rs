//! Block entity data the 1.8.9 server sends after chunks: sign text (0x33)
//! and skull/banner NBT (0x35). Chests need none; their block is enough.
use horizon_protocol_47::nbt::Tag;
use horizon_protocol_47::player_list::{Profile, Property};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum BlockEntity {
    Sign([String; 4]),
    /// `SkullType` (0 skeleton, 1 wither, 2 zombie, 3 player, 4 creeper),
    /// floor `Rot` in sixteenths of a turn, and a player head's `Owner`.
    Skull {
        kind: u8,
        rotation: u8,
        owner: Option<Owner>,
    },
    /// `Base` dye colour (0 black … 15 white, dye order).
    Banner {
        base: u8,
    },
}

/// A player head's owner (`NBTUtil.readGameProfileFromNBT`): the profile
/// id, or a stand-in from the skin when the server sends none, and the
/// signed `textures` property the skin comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct Owner {
    pub uuid: u128,
    pub profile: Arc<Profile>,
}

impl Owner {
    pub fn from_nbt(tag: &Tag) -> Option<Self> {
        let name = tag
            .get("Name")
            .and_then(Tag::as_str)
            .unwrap_or("")
            .to_owned();
        let textures = tag
            .get("Properties")
            .and_then(|properties| properties.get("textures"))
            .and_then(|list| list.as_list().first())?;
        let value = textures.get("Value").and_then(Tag::as_str)?.to_owned();
        let signature = textures
            .get("Signature")
            .and_then(Tag::as_str)
            .map(str::to_owned);
        let uuid = tag
            .get("Id")
            .and_then(Tag::as_str)
            .and_then(|id| u128::from_str_radix(&id.replace('-', ""), 16).ok())
            .unwrap_or_else(|| {
                // FNV-1a of the skin, so equal heads share one download.
                value
                    .bytes()
                    .fold(0x6c62_272e_07bb_0142_62b8_2175_6295_c58d, |hash, byte| {
                        (hash ^ u128::from(byte))
                            .wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b)
                    })
            });
        Some(Self {
            uuid,
            profile: Arc::new(Profile {
                name,
                properties: vec![Property {
                    name: "textures".into(),
                    value,
                    signature,
                }],
            }),
        })
    }
}

impl BlockEntity {
    /// Update Block Entity actions: 4 skull, 6 banner.
    pub fn from_nbt(action: u8, tag: &Tag) -> Option<Self> {
        match action {
            4 => Some(BlockEntity::Skull {
                kind: tag
                    .get("SkullType")
                    .and_then(Tag::as_int)
                    .unwrap_or(0)
                    .clamp(0, 4) as u8,
                rotation: (tag.get("Rot").and_then(Tag::as_int).unwrap_or(0) & 15) as u8,
                owner: tag.get("Owner").and_then(Owner::from_nbt),
            }),
            6 => Some(BlockEntity::Banner {
                base: (tag.get("Base").and_then(Tag::as_int).unwrap_or(0) & 15) as u8,
            }),
            _ => None,
        }
    }
}

/// Plain text of a 1.8 sign line: a JSON string or chat component tree.
/// Collects every `"text"` value in order and drops `§` formatting codes.
pub fn sign_text(json: &str) -> String {
    fn read_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
        let mut out = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => out.push(' '),
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            out.push(c);
                        }
                    }
                    Some(other) => out.push(other),
                    None => break,
                },
                c => out.push(c),
            }
        }
        out
    }
    let trimmed = json.trim();
    let mut text = String::new();
    let mut chars = trimmed.chars().peekable();
    if trimmed.starts_with('"') {
        chars.next();
        text = read_string(&mut chars);
    } else {
        let mut previous_key = String::new();
        while let Some(c) = chars.next() {
            if c != '"' {
                continue;
            }
            let value = read_string(&mut chars);
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
            if chars.peek() == Some(&':') {
                previous_key = value;
            } else if previous_key == "text" {
                text.push_str(&value);
                previous_key.clear();
            } else {
                previous_key.clear();
            }
        }
    }
    let mut plain = String::with_capacity(text.len());
    let mut skip = false;
    for c in text.chars() {
        if skip {
            skip = false;
        } else if c == '§' {
            skip = true;
        } else {
            plain.push(c);
        }
    }
    plain.chars().take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_lines_flatten_components_and_strip_formatting() {
        assert_eq!(sign_text("\"Hello\""), "Hello");
        assert_eq!(sign_text("\"\""), "");
        assert_eq!(
            sign_text(r#"{"extra":[{"color":"gold","text":"§lBed"},{"text":" Wars"}],"text":""}"#),
            "Bed Wars"
        );
        assert_eq!(sign_text(r#"{"text":"a\"b!"}"#), "a\"b!");
        let skull = Tag::Compound(vec![
            ("SkullType".into(), Tag::Byte(3)),
            ("Rot".into(), Tag::Byte(9)),
        ]);
        assert_eq!(
            BlockEntity::from_nbt(4, &skull),
            Some(BlockEntity::Skull {
                kind: 3,
                rotation: 9,
                owner: None,
            })
        );
        let textures = |id: Option<&str>| {
            let mut owner = vec![
                ("Name".into(), Tag::String("Bob".into())),
                (
                    "Properties".into(),
                    Tag::Compound(vec![(
                        "textures".into(),
                        Tag::List(vec![Tag::Compound(vec![(
                            "Value".into(),
                            Tag::String("e30=".into()),
                        )])]),
                    )]),
                ),
            ];
            if let Some(id) = id {
                owner.push(("Id".into(), Tag::String(id.into())));
            }
            Tag::Compound(vec![
                ("SkullType".into(), Tag::Byte(3)),
                ("Owner".into(), Tag::Compound(owner)),
            ])
        };
        let Some(BlockEntity::Skull {
            owner: Some(owner), ..
        }) = BlockEntity::from_nbt(4, &textures(Some("00000000-0000-0000-0000-00000000002a")))
        else {
            panic!("player head with an owner");
        };
        assert_eq!(owner.uuid, 42);
        assert_eq!(owner.profile.name, "Bob");
        assert_eq!(owner.profile.properties[0].value, "e30=");
        let stand_in = |tag: &Tag| match BlockEntity::from_nbt(4, tag) {
            Some(BlockEntity::Skull {
                owner: Some(owner), ..
            }) => owner.uuid,
            _ => panic!("owner"),
        };
        assert_eq!(stand_in(&textures(None)), stand_in(&textures(None)));
        assert_ne!(stand_in(&textures(None)), 42);
        assert_eq!(BlockEntity::from_nbt(1, &skull), None);
    }
}
