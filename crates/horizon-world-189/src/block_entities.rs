//! Block entity data the 1.8.9 server sends after chunks: sign text (0x33)
//! and skull/banner NBT (0x35). Chests need none; their block is enough.
use horizon_protocol_47::nbt::Tag;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockEntity {
    Sign([String; 4]),
    /// `SkullType` (0 skeleton, 1 wither, 2 zombie, 3 player, 4 creeper) and
    /// floor `Rot` in sixteenths of a turn.
    Skull {
        kind: u8,
        rotation: u8,
    },
    /// `Base` dye colour (0 black … 15 white, dye order).
    Banner {
        base: u8,
    },
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
                rotation: 9
            })
        );
        assert_eq!(BlockEntity::from_nbt(1, &skull), None);
    }
}
