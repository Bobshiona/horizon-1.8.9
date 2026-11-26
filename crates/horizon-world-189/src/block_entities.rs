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
    /// `Base` dye colour (0 black … 15 white, dye order) and the `Patterns`
    /// drawn over it: an index into `BANNER_PATTERNS` and a dye colour each.
    Banner {
        base: u8,
        patterns: Vec<(u8, u8)>,
    },
}

/// `TileEntityBanner.EnumBannerPattern` (aku$a) in order: texture name and
/// the id `Patterns` entries use.
pub const BANNER_PATTERNS: [(&str, &str); 39] = [
    ("base", "b"),
    ("square_bottom_left", "bl"),
    ("square_bottom_right", "br"),
    ("square_top_left", "tl"),
    ("square_top_right", "tr"),
    ("stripe_bottom", "bs"),
    ("stripe_top", "ts"),
    ("stripe_left", "ls"),
    ("stripe_right", "rs"),
    ("stripe_center", "cs"),
    ("stripe_middle", "ms"),
    ("stripe_downright", "drs"),
    ("stripe_downleft", "dls"),
    ("small_stripes", "ss"),
    ("cross", "cr"),
    ("straight_cross", "sc"),
    ("triangle_bottom", "bt"),
    ("triangle_top", "tt"),
    ("triangles_bottom", "bts"),
    ("triangles_top", "tts"),
    ("diagonal_left", "ld"),
    ("diagonal_up_right", "rd"),
    ("diagonal_up_left", "lud"),
    ("diagonal_right", "rud"),
    ("circle", "mc"),
    ("rhombus", "mr"),
    ("half_vertical", "vh"),
    ("half_horizontal", "hh"),
    ("half_vertical_right", "vhr"),
    ("half_horizontal_bottom", "hhb"),
    ("border", "bo"),
    ("curly_border", "cbo"),
    ("creeper", "cre"),
    ("gradient", "gra"),
    ("gradient_up", "gru"),
    ("bricks", "bri"),
    ("skull", "sku"),
    ("flower", "flo"),
    ("mojang", "moj"),
];

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
            // TileEntityBanner.initializeBannerData: unknown ids are left out.
            6 => Some(BlockEntity::Banner {
                base: (tag.get("Base").and_then(Tag::as_int).unwrap_or(0) & 15) as u8,
                patterns: tag
                    .get("Patterns")
                    .map(Tag::as_list)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|entry| {
                        let id = entry.get("Pattern").and_then(Tag::as_str)?;
                        let index = BANNER_PATTERNS.iter().position(|(_, code)| *code == id)?;
                        let colour = entry.get("Color").and_then(Tag::as_int).unwrap_or(0) & 15;
                        Some((index as u8, colour as u8))
                    })
                    .collect(),
            }),
            _ => None,
        }
    }
}

/// A 1.8 sign line (`TileEntitySign.signText`) as `getFormattedText`: the
/// chat component's text with its styles as `§` codes, which the renderer
/// draws and fits to the sign. Malformed JSON shows as written.
pub fn sign_text(json: &str) -> String {
    let text = horizon_protocol_47::text::format(json, &|_| None).text;
    // The server sends at most a few hundred characters; keep memory bounded.
    text.chars().take(384).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_lines_keep_their_formatting() {
        // getFormattedText: each component's style codes, its text, then §r.
        assert_eq!(sign_text("\"Hello\""), "Hello§r");
        assert_eq!(sign_text("\"\""), "§r");
        assert_eq!(
            sign_text(r#"{"extra":[{"color":"gold","text":"§lBed"},{"text":" Wars"}],"text":""}"#),
            "§r§6§lBed§r Wars§r"
        );
        assert_eq!(sign_text(r#"{"text":"a\"b!"}"#), "a\"b!§r");
        assert_eq!(
            sign_text(&format!("\"{}\"", "x".repeat(500)))
                .chars()
                .count(),
            384
        );
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
        let pattern = |id: &str, colour: i32| {
            Tag::Compound(vec![
                ("Pattern".into(), Tag::String(id.into())),
                ("Color".into(), Tag::Int(colour)),
            ])
        };
        let banner = Tag::Compound(vec![
            ("Base".into(), Tag::Int(4)),
            (
                "Patterns".into(),
                Tag::List(vec![
                    pattern("cre", 15),
                    pattern("nope", 2),
                    pattern("bo", 0),
                ]),
            ),
        ]);
        assert_eq!(
            BlockEntity::from_nbt(6, &banner),
            Some(BlockEntity::Banner {
                base: 4,
                patterns: vec![(32, 15), (30, 0)],
            })
        );
    }
}
