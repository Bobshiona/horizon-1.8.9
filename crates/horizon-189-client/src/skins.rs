//! Bounded asynchronous Mojang CDN skin/cape loading. HTTP and PNG decode
//! stay off the simulation and Metal threads; profiles retain cache identity.
use base64::{Engine, engine::general_purpose::STANDARD};
use horizon_assets_189::{Assets, Texture};
use horizon_protocol_47::player_list::Profile;
use std::{
    collections::HashMap,
    io::Read,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

/// A player's public profile with its skin textures, from Mojang's profile
/// service (the same lookup servers make; no token). For the launcher's
/// scene, which has no server to send it.
pub fn profile(uuid: u128) -> Option<Profile> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .timeout_read(Duration::from_secs(5))
        .redirects(0)
        .build();
    let url = format!("https://sessionserver.mojang.com/session/minecraft/profile/{uuid:032x}");
    let body = agent.get(&url).call().ok()?.into_string().ok()?;
    parse_profile(&body)
}

fn parse_profile(body: &str) -> Option<Profile> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let properties = json
        .get("properties")?
        .as_array()?
        .iter()
        .filter_map(|p| {
            Some(horizon_protocol_47::player_list::Property {
                name: p.get("name")?.as_str()?.to_owned(),
                value: p.get("value")?.as_str()?.to_owned(),
                signature: p
                    .get("signature")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect();
    Some(Profile {
        name: json.get("name")?.as_str()?.to_owned(),
        properties,
    })
}

#[derive(Clone, Debug, PartialEq)]
struct Descriptor {
    skin: String,
    cape: Option<String>,
    slim: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub skin: u32,
    pub cape: Option<u32>,
    pub slim: bool,
}

pub struct Loaded {
    pub uuid: u128,
    pub skin: Texture,
    pub cape: Option<Texture>,
    pub slim: bool,
}

pub struct Loader {
    requests: SyncSender<(u128, Descriptor)>,
    pub results: Receiver<Result<Loaded, (u128, String)>>,
    seen: HashMap<u128, Arc<Profile>>,
    active: std::collections::HashSet<u128>,
    retry: HashMap<u128, std::time::Instant>,
}

fn cdn(value: &str) -> Option<String> {
    let path = value
        .strip_prefix("https://textures.minecraft.net/texture/")
        .or_else(|| value.strip_prefix("http://textures.minecraft.net/texture/"))?;
    if !(32..=64).contains(&path.len()) || !path.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("https://textures.minecraft.net/texture/{path}"))
}

fn descriptor(profile: &Profile) -> Option<Descriptor> {
    let value = &profile
        .properties
        .iter()
        .find(|p| p.name == "textures")?
        .value;
    if value.len() > 32767 {
        return None;
    }
    let bytes = STANDARD.decode(value).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let textures = json.get("textures")?;
    let skin = textures.get("SKIN")?;
    Some(Descriptor {
        skin: cdn(skin.get("url")?.as_str()?)?,
        cape: textures
            .get("CAPE")
            .and_then(|v| v.get("url"))
            .and_then(|v| v.as_str())
            .and_then(cdn),
        slim: skin
            .get("metadata")
            .and_then(|v| v.get("model"))
            .and_then(|v| v.as_str())
            == Some("slim"),
    })
}

fn fetch(agent: &ureq::Agent, url: &str, skin: bool) -> Result<Texture, String> {
    let response = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("Skin PNG exceeds download budget".into());
    }
    let image = Assets::decode_png(&bytes).map_err(|e| e.to_string())?;
    if image.width != 64 || !matches!(image.height, 32 | 64) {
        return Err("Unsupported legacy skin/cape dimensions".into());
    }
    Ok(if skin { normalize_skin(image) } else { image })
}

fn normalize_skin(image: Texture) -> Texture {
    let mut rgba = vec![0; 64 * 64 * 4];
    rgba[..image.rgba.len()].copy_from_slice(&image.rgba);
    if image.height == 32 {
        // The old layout shares right limbs; copy the six mirrored cuboid
        // faces into the modern left-limb cells before ModelPlayer draws.
        for (dx, dy, sx, sy, w, h) in [
            (24, 48, 4, 16, 4, 4),
            (20, 48, 8, 16, 4, 4),
            (16, 52, 8, 20, 4, 12),
            (20, 52, 4, 20, 4, 12),
            (24, 52, 0, 20, 4, 12),
            (28, 52, 12, 20, 4, 12),
            (40, 48, 44, 16, 4, 4),
            (36, 48, 48, 16, 4, 4),
            (32, 52, 48, 20, 4, 12),
            (36, 52, 44, 20, 4, 12),
            (40, 52, 40, 20, 4, 12),
            (44, 52, 52, 20, 4, 12),
        ] {
            for y in 0..h {
                for x in 0..w {
                    let src = ((sy + y) * 64 + sx + w - 1 - x) * 4;
                    let dst = ((dy + y) * 64 + dx + x) * 4;
                    let pixel: [u8; 4] = rgba[src..src + 4].try_into().unwrap();
                    rgba[dst..dst + 4].copy_from_slice(&pixel);
                }
            }
        }
        let opaque = (0..32).all(|y| (32..64).all(|x| rgba[(y * 64 + x) * 4 + 3] >= 128));
        if opaque {
            for y in 0..32 {
                for x in 32..64 {
                    rgba[(y * 64 + x) * 4 + 3] = 0;
                }
            }
        }
    }
    for (x0, y0, x1, y1) in [(0, 0, 32, 16), (0, 16, 64, 32), (16, 48, 48, 64)] {
        for y in y0..y1 {
            for x in x0..x1 {
                rgba[(y * 64 + x) * 4 + 3] = 255;
            }
        }
    }
    Texture {
        width: 64,
        height: 64,
        rgba,
    }
}

impl Loader {
    pub fn new() -> std::io::Result<Self> {
        let (requests, inbox) = mpsc::sync_channel::<(u128, Descriptor)>(128);
        let (send, results) = mpsc::sync_channel(128);
        std::thread::Builder::new()
            .name("Legacy skins".into())
            .spawn(move || {
                let agent = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(3))
                    .timeout_read(Duration::from_secs(5))
                    .redirects(0)
                    .build();
                for (uuid, request) in inbox {
                    let result = fetch(&agent, &request.skin, true).map(|skin| Loaded {
                        uuid,
                        skin,
                        cape: request
                            .cape
                            .as_deref()
                            .and_then(|url| fetch(&agent, url, false).ok()),
                        slim: request.slim,
                    });
                    let result = result.map_err(|error| (uuid, error));
                    if send.send(result).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests,
            results,
            seen: HashMap::new(),
            active: Default::default(),
            retry: Default::default(),
        })
    }
    pub fn retain(&mut self, uuids: impl Iterator<Item = u128>) {
        self.active.clear();
        self.active.extend(uuids);
        self.seen.retain(|uuid, _| self.active.contains(uuid));
        self.retry.retain(|uuid, _| self.active.contains(uuid));
    }
    pub fn failed(&mut self, uuid: u128) {
        self.seen.remove(&uuid);
        self.retry
            .insert(uuid, std::time::Instant::now() + Duration::from_secs(30));
    }
    pub fn request(&mut self, uuid: u128, profile: &Arc<Profile>) {
        if self
            .retry
            .get(&uuid)
            .is_some_and(|at| *at > std::time::Instant::now())
        {
            return;
        }
        self.retry.remove(&uuid);
        if self.seen.len() >= 128 && !self.seen.contains_key(&uuid) {
            return;
        }
        if self
            .seen
            .get(&uuid)
            .is_some_and(|old| Arc::ptr_eq(old, profile))
        {
            return;
        }
        let Some(request) = descriptor(profile) else {
            self.seen.insert(uuid, profile.clone());
            return;
        };
        if self.requests.try_send((uuid, request)).is_ok() {
            self.seen.insert(uuid, profile.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_lookups_keep_the_textures_property() {
        let body =
            r#"{"id":"0123","name":"Bob","properties":[{"name":"textures","value":"e30="}]}"#;
        let profile = parse_profile(body).unwrap();
        assert_eq!(profile.name, "Bob");
        assert_eq!(profile.properties[0].name, "textures");
        assert_eq!(profile.properties[0].value, "e30=");
        assert!(parse_profile("{}").is_none());
    }

    use horizon_protocol_47::player_list::Property;
    #[test]
    fn descriptors_keep_slim_and_capes_on_the_texture_cdn() {
        let hash = "1".repeat(64);
        let json = format!(
            r#"{{"textures":{{"SKIN":{{"url":"http://textures.minecraft.net/texture/{hash}","metadata":{{"model":"slim"}}}},"CAPE":{{"url":"https://textures.minecraft.net/texture/{hash}"}}}}}}"#
        );
        let mut p = Profile {
            name: "Fixture".into(),
            properties: vec![Property {
                name: "textures".into(),
                value: STANDARD.encode(json),
                signature: None,
            }],
        };
        let parsed = descriptor(&p).unwrap();
        assert!(parsed.slim);
        assert!(parsed.cape.is_some());
        assert!(parsed.skin.starts_with("https://"));
        p.properties[0].value =
            STANDARD.encode(r#"{"textures":{"SKIN":{"url":"http://127.0.0.1/private"}}}"#);
        assert!(descriptor(&p).is_none());
        let rgba = vec![255; 64 * 32 * 4];
        let normalized = normalize_skin(Texture {
            width: 64,
            height: 32,
            rgba,
        });
        assert_eq!(normalized.rgba.len(), 64 * 64 * 4);
        assert_eq!(normalized.rgba[(8 * 64 + 40) * 4 + 3], 0);
        assert_eq!(normalized.rgba[(56 * 64 + 40) * 4 + 3], 255);
    }
}
