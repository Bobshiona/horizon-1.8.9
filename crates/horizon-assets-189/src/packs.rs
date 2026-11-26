//! 1.8.9 resource packs (`ResourcePackRepository`): a zip or a folder with
//! `pack.mcmeta` at its root, stacked over the game jar so each file comes
//! from the highest selected pack that has it.
use horizon_assets::resource_source::{Resource, ResourceSource};
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

/// A pack found in the packs folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    /// The file or folder name, which the settings keep.
    pub name: String,
    pub path: PathBuf,
    /// `pack.description` as written (a string or a chat component).
    pub description: String,
    /// `pack.pack_format`; 1.8.9 packs are 1.
    pub format: i64,
    /// `pack.png`, if any.
    pub icon: Option<Vec<u8>>,
}

fn open(path: &Path) -> io::Result<ResourceSource> {
    if path.is_dir() {
        Ok(ResourceSource::directory(path.to_owned()))
    } else {
        ResourceSource::archive(path)
    }
}

/// Every pack in `folder` with a readable `pack.mcmeta`, in name order.
/// Anything else there is left out, as `ResourcePackRepository` does.
pub fn discover(folder: &Path) -> Vec<Pack> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut packs: Vec<Pack> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_str()?.to_owned();
            let zip = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"));
            if !(path.is_dir() || zip) || name.starts_with('.') {
                return None;
            }
            let source = open(&path).ok()?;
            let meta = source.read_limited("pack.mcmeta", 1 << 16).ok()?;
            let (description, format) = metadata(&meta)?;
            Some(Pack {
                name,
                description,
                format,
                icon: source.read_limited("pack.png", 1 << 20).ok(),
                path,
            })
        })
        .collect();
    packs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    packs
}

/// `pack.mcmeta`'s description and pack format. Packs often write loose
/// JSON; only the two fields matter, so a tolerant scan reads them.
pub fn metadata(json: &[u8]) -> Option<(String, i64)> {
    let text = std::str::from_utf8(json)
        .ok()?
        .trim_start_matches('\u{feff}');
    let pack = text.find("\"pack\"")?;
    let body = &text[pack..];
    let format = body
        .find("\"pack_format\"")
        .and_then(|at| {
            let rest = body[at + 13..].trim_start().strip_prefix(':')?.trim_start();
            let digits: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '-')
                .collect();
            digits.parse().ok()
        })
        .unwrap_or(1);
    let description = body
        .find("\"description\"")
        .and_then(|at| {
            let rest = body[at + 13..].trim_start().strip_prefix(':')?.trim_start();
            Some(json_value(rest))
        })
        .unwrap_or_default();
    Some((description, format))
}

/// One JSON value from the start of `text`, as raw JSON.
fn json_value(text: &str) -> String {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, c) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
                if depth == 0 {
                    return text[..=index].to_owned();
                }
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return text[..=index].to_owned();
                }
                if depth < 0 {
                    return text[..index].trim().to_owned();
                }
            }
            ',' if depth == 0 => return text[..index].trim().to_owned(),
            _ => {}
        }
    }
    text.trim().to_owned()
}

/// The jar with `selected` (highest priority first) stacked over it: each
/// `assets/` file comes from the highest pack that has it. Packs that no
/// longer open are skipped.
pub fn stack(jar: &Path, selected: &[PathBuf]) -> io::Result<ResourceSource> {
    Ok(stack_over(ResourceSource::archive(jar)?, selected))
}

/// `stack` over any base source.
pub fn stack_over(base: ResourceSource, selected: &[PathBuf]) -> ResourceSource {
    if selected.is_empty() {
        return base;
    }
    let mut files = BTreeMap::new();
    for name in base.list("assets/") {
        files.insert(name.clone(), Resource::file(base.clone(), name));
    }
    for path in selected.iter().rev() {
        let Ok(pack) = open(path) else {
            continue;
        };
        for name in pack.list("assets/") {
            files.insert(name.clone(), Resource::file(pack.clone(), name));
        }
    }
    ResourceSource::layered(files, base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_metadata_reads_format_and_description() {
        let (description, format) =
            metadata(br#"{"pack":{"pack_format":1,"description":"Faithful 32x"}}"#).unwrap();
        assert_eq!((description.as_str(), format), ("\"Faithful 32x\"", 1));
        let (description, _) = metadata(
            "\u{feff}{ \"pack\" : { \"description\" : {\"text\":\"A\",\"color\":\"gold\"}, \"pack_format\": 2 } }"
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(description, r#"{"text":"A","color":"gold"}"#);
        assert_eq!(metadata(br#"{"pack":{}}"#).unwrap().1, 1);
        assert!(metadata(b"{}").is_none());
    }

    #[test]
    fn higher_packs_win_and_the_jar_fills_the_rest() {
        let root = std::env::temp_dir().join(format!("bob-packs-{}", std::process::id()));
        let make = |name: &str, files: &[(&str, &str)]| {
            let dir = root.join(name);
            for (file, text) in files {
                let path = dir.join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            dir
        };
        let low = make(
            "low",
            &[
                (
                    "pack.mcmeta",
                    r#"{"pack":{"pack_format":1,"description":"low"}}"#,
                ),
                ("assets/minecraft/a.txt", "low"),
                ("assets/minecraft/b.txt", "low"),
            ],
        );
        let high = make(
            "high",
            &[
                (
                    "pack.mcmeta",
                    r#"{"pack":{"pack_format":1,"description":"high"}}"#,
                ),
                ("assets/minecraft/a.txt", "high"),
            ],
        );
        make("not a pack", &[("readme.txt", "x")]);
        let found = discover(&root);
        assert_eq!(
            found.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["high", "low"]
        );
        // A folder stands in for the jar.
        let base = ResourceSource::directory(make(
            "jar",
            &[
                ("assets/minecraft/a.txt", "jar"),
                ("assets/minecraft/c.txt", "jar"),
            ],
        ));
        let stacked = stack_over(base, &[high, low, root.join("gone")]);
        let read = |name: &str| String::from_utf8(stacked.read(name).unwrap()).unwrap();
        assert_eq!(read("assets/minecraft/a.txt"), "high");
        assert_eq!(read("assets/minecraft/b.txt"), "low");
        assert_eq!(read("assets/minecraft/c.txt"), "jar");
        let _ = std::fs::remove_dir_all(&root);
    }
}
