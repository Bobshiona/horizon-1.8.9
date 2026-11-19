//! Version-specific adapter for the owned 1.8.9 jar. Archive reads, cuboid
//! inheritance and quad baking reuse Horizon. No exporters or JVM execute.
#![forbid(unsafe_code)]

pub use horizon_assets::blockstate::{Variant as ModelVariant, VariantSet};
use horizon_assets::{
    bake::{BlockModelTransform, ModelQuad, bake_block_model, bake_model},
    blockstate::BlockStateDefinition,
    model::CuboidModel,
    resolve::ModelLibrary,
    resource_source::ResourceSource,
};
use std::{
    collections::BTreeMap,
    error::Error,
    io::{self, Cursor},
    path::Path,
};

pub struct Assets {
    source: ResourceSource,
    models: ModelLibrary,
    pub model_count: usize,
}

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Assets {
    pub fn open(jar: &Path) -> Result<Self, Box<dyn Error>> {
        Self::from_source(ResourceSource::archive(jar)?)
    }

    pub fn from_source(source: ResourceSource) -> Result<Self, Box<dyn Error>> {
        let prefix = "assets/minecraft/models/block/";
        let mut models = BTreeMap::new();
        for path in source.list(prefix) {
            let Some(name) = path
                .strip_prefix(prefix)
                .and_then(|name| name.strip_suffix(".json"))
            else {
                continue;
            };
            let json = source.read_limited(&path, 1 << 20)?;
            let model = CuboidModel::from_json(std::str::from_utf8(&json)?).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("{path}: {error}"))
            })?;
            models.insert(format!("minecraft:block/{name}"), model);
        }
        if models.is_empty() {
            return Err("Archive contains no Minecraft block models".into());
        }
        let model_count = models.len();
        Ok(Self {
            source,
            models: ModelLibrary::new(models)?,
            model_count,
        })
    }

    pub fn bake_block(&self, name: &str) -> Result<Vec<ModelQuad>, Box<dyn Error>> {
        let name = Self::model_name(name);
        if !self.models.resolves(&name) {
            return Err(format!("Block model or parent missing: {name}").into());
        }
        Ok(bake_model(&self.models.resolve(&name)?)?)
    }

    pub fn language(&self) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
        let bytes = self
            .source
            .read_limited("assets/minecraft/lang/en_US.lang", 1 << 20)?;
        let text = std::str::from_utf8(&bytes)?;
        Ok(text
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect())
    }

    /// First generated-item texture, following legacy item-model parents.
    pub fn item_texture(&self, name: &str) -> Result<Texture, Box<dyn Error>> {
        use horizon_assets::model::TextureSlot;
        let mut name = format!("minecraft:item/{name}");
        for _ in 0..32 {
            let (namespace, path) = name.split_once(':').unwrap_or(("minecraft", &name));
            let bytes = self
                .source
                .read_limited(&format!("assets/{namespace}/models/{path}.json"), 1 << 20)?;
            let model = CuboidModel::from_json(std::str::from_utf8(&bytes)?)?;
            if let Some(TextureSlot::Material(material)) = model.textures.get("layer0") {
                return self.texture(&material.sprite);
            }
            let Some(parent) = model.parent else { break };
            name = if parent.contains(':') {
                parent
            } else {
                format!("minecraft:{parent}")
            };
        }
        Err("Item has no generated sprite".into())
    }

    /// Every `assets/minecraft/blockstates/*.json` variant map, keyed by file
    /// stem. 1.8 files have no multipart section.
    pub fn blockstates(&self) -> Result<Vec<(String, Vec<(String, VariantSet)>)>, Box<dyn Error>> {
        let prefix = "assets/minecraft/blockstates/";
        let mut files = Vec::new();
        for path in self.source.list(prefix) {
            let Some(name) = path
                .strip_prefix(prefix)
                .and_then(|name| name.strip_suffix(".json"))
            else {
                continue;
            };
            let json = self.source.read_limited(&path, 1 << 20)?;
            let definition =
                BlockStateDefinition::from_json(std::str::from_utf8(&json)?).map_err(|error| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("{path}: {error}"))
                })?;
            if let Some(variants) = definition.variants {
                files.push((name.to_owned(), variants));
            }
        }
        if files.is_empty() {
            return Err("Archive contains no Minecraft blockstates".into());
        }
        Ok(files)
    }

    /// Bake a blockstate variant's model with its quarter turns and UV lock.
    pub fn bake_variant(
        &self,
        name: &str,
        rotations: [u8; 3],
        uv_lock: bool,
    ) -> Result<Vec<ModelQuad>, Box<dyn Error>> {
        let name = Self::model_name(name);
        if !self.models.resolves(&name) {
            return Err(format!("Block model or parent missing: {name}").into());
        }
        Ok(bake_block_model(
            &self.models.resolve(&name)?,
            BlockModelTransform::new(rotations, uv_lock),
        )?)
    }

    fn model_name(name: &str) -> String {
        // 1.8 blockstate model references omit the block/ directory.
        let (namespace, path) = name.split_once(':').unwrap_or(("minecraft", name));
        let path = path.strip_prefix("block/").unwrap_or(path);
        format!("{namespace}:block/{path}")
    }

    /// Return the PNG's complete vertical animation strip as RGBA8.
    pub fn texture(&self, identifier: &str) -> Result<Texture, Box<dyn Error>> {
        let (namespace, name) = identifier
            .split_once(':')
            .unwrap_or(("minecraft", identifier));
        let data = self
            .source
            .read_limited(&format!("assets/{namespace}/textures/{name}.png"), 16 << 20)?;
        Self::decode_png(&data)
    }

    pub fn decode_png(data: &[u8]) -> Result<Texture, Box<dyn Error>> {
        let mut decoder = png::Decoder::new(Cursor::new(data));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info()?;
        let size = reader
            .output_buffer_size()
            .ok_or("PNG output size overflow")?;
        if size > 64 << 20 {
            return Err("Decoded texture exceeds the upload budget".into());
        }
        let mut decoded = vec![0; size];
        let info = reader.next_frame(&mut decoded)?;
        let decoded = &decoded[..info.buffer_size()];
        let rgba_len = (info.width as usize)
            .checked_mul(info.height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .filter(|&bytes| bytes <= 64 << 20)
            .ok_or("RGBA texture exceeds the upload budget")?;
        let mut rgba = Vec::with_capacity(rgba_len);
        match info.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(decoded),
            png::ColorType::Rgb => {
                for &[r, g, b] in decoded.as_chunks::<3>().0 {
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
            }
            png::ColorType::Grayscale => {
                for &gray in decoded {
                    rgba.extend_from_slice(&[gray, gray, gray, 255]);
                }
            }
            png::ColorType::GrayscaleAlpha => {
                for &[gray, alpha] in decoded.as_chunks::<2>().0 {
                    rgba.extend_from_slice(&[gray, gray, gray, alpha]);
                }
            }
            png::ColorType::Indexed => return Err("PNG palette was not expanded".into()),
        }
        Ok(Texture {
            width: info.width,
            height: info.height,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_assets::resource_source::Resource;

    #[test]
    fn legacy_parent_and_texture_slots_bake_through_upstream() {
        let source = ResourceSource::indexed(BTreeMap::from([
            ("assets/minecraft/models/block/test.json".to_owned(), Resource::bytes(br##"{"parent":"block/base","textures":{"all":"blocks/stone"}}"##.to_vec())),
            ("assets/minecraft/models/block/base.json".to_owned(), Resource::bytes(br##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all","cullface":"up"}}}]}"##.to_vec())),
        ]));
        let assets = Assets::from_source(source).unwrap();
        let quads = assets.bake_block("test").unwrap();
        assert_eq!(quads.len(), 1);
        for name in ["minecraft:test", "block/test", "minecraft:block/test"] {
            assert_eq!(assets.bake_block(name).unwrap(), quads);
        }
        assert_eq!(quads[0].material.sprite, "minecraft:blocks/stone");
        assert!(quads[0].positions.iter().all(|position| position[1] == 1.0));
        assert!(assets.bake_block("missing").is_err());
    }
}
