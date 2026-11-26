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

pub mod packs;

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

    /// The jar with resource packs (highest priority first) stacked over it.
    pub fn open_with_packs(
        jar: &Path,
        packs: &[std::path::PathBuf],
    ) -> Result<Self, Box<dyn Error>> {
        Self::from_source(packs::stack(jar, packs)?)
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

    /// Inherited and rebound particle slot, independent of baked face order.
    pub fn particle_sprite(&self, name: &str) -> Result<Option<String>, Box<dyn Error>> {
        let model = self.models.resolve(&Self::model_name(name))?;
        Ok(model.textures.get("particle").map(|m| m.sprite.clone()))
    }

    pub fn bake_block(&self, name: &str) -> Result<Vec<ModelQuad>, Box<dyn Error>> {
        let name = Self::model_name(name);
        if !self.models.resolves(&name) {
            return Err(format!("Block model or parent missing: {name}").into());
        }
        Ok(bake_model(&self.models.resolve(&name)?)?)
    }

    /// Raw bytes of one jar resource, such as `font/glyph_sizes.bin`.
    pub fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(self
            .source
            .read_limited(&format!("assets/minecraft/{path}"), limit)?)
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
                return self.texture_at_vanilla_size(&material.sprite);
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

    /// A texture at the jar's own size: a resource pack's larger version
    /// (2x, 4x… of the jar's) is box-averaged down, so models laid out for
    /// vanilla's pixels still line up. Otherwise as `texture`.
    pub fn texture_at_vanilla_size(&self, identifier: &str) -> Result<Texture, Box<dyn Error>> {
        let texture = self.texture(identifier)?;
        let Some(jar) = self.source.underlay() else {
            return Ok(texture);
        };
        let (namespace, name) = identifier
            .split_once(':')
            .unwrap_or(("minecraft", identifier));
        let Ok(data) =
            jar.read_limited(&format!("assets/{namespace}/textures/{name}.png"), 16 << 20)
        else {
            return Ok(texture);
        };
        let reader = png::Decoder::new(Cursor::new(data)).read_info()?;
        let (width, height) = (reader.info().width, reader.info().height);
        Ok(downscale(texture, width, height))
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

/// `texture` brought to `width` x `height` when it is a whole power-of-two
/// multiple of that size; unchanged otherwise.
pub fn downscale(texture: Texture, width: u32, height: u32) -> Texture {
    if width == 0 || texture.width <= width || !texture.width.is_multiple_of(width) {
        return texture;
    }
    let step = texture.width / width;
    if !step.is_power_of_two() || texture.height != height * step {
        return texture;
    }
    let (step, source_width) = (step as usize, texture.width as usize);
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut sum = [0u32; 4];
            for dy in 0..step {
                for dx in 0..step {
                    let at = ((y * step + dy) * source_width + x * step + dx) * 4;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(texture.rgba[at + channel]);
                    }
                }
            }
            for channel in 0..4 {
                rgba[(y * width as usize + x) * 4 + channel] =
                    (sum[channel] / (step * step) as u32) as u8;
            }
        }
    }
    Texture {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_assets::resource_source::Resource;

    #[test]
    fn larger_pack_textures_average_down_to_the_jars_size() {
        let texture = Texture {
            width: 4,
            height: 2,
            rgba: [
                [0, 0, 0, 255],
                [255, 255, 255, 255],
                [10, 20, 30, 255],
                [10, 20, 30, 255],
            ]
            .repeat(2)
            .concat(),
        };
        let small = downscale(texture, 2, 1);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(&small.rgba[..4], &[127, 127, 127, 255]);
        assert_eq!(&small.rgba[4..], &[10, 20, 30, 255]);
        let odd = Texture {
            width: 6,
            height: 2,
            rgba: vec![0; 48],
        };
        assert_eq!(downscale(odd, 2, 1).width, 6, "3x is left alone");
    }

    #[test]
    fn legacy_parent_and_texture_slots_bake_through_upstream() {
        let source = ResourceSource::indexed(BTreeMap::from([
            ("assets/minecraft/models/block/test.json".to_owned(), Resource::bytes(br##"{"parent":"block/base","textures":{"all":"blocks/stone","particle":"#all"}}"##.to_vec())),
            ("assets/minecraft/models/block/base.json".to_owned(), Resource::bytes(br##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all","cullface":"up"}}}]}"##.to_vec())),
        ]));
        let assets = Assets::from_source(source).unwrap();
        let quads = assets.bake_block("test").unwrap();
        assert_eq!(quads.len(), 1);
        assert_eq!(
            assets.particle_sprite("test").unwrap().as_deref(),
            Some("minecraft:blocks/stone")
        );
        for name in ["minecraft:test", "block/test", "minecraft:block/test"] {
            assert_eq!(assets.bake_block(name).unwrap(), quads);
        }
        assert_eq!(quads[0].material.sprite, "minecraft:blocks/stone");
        assert!(quads[0].positions.iter().all(|position| position[1] == 1.0));
        assert!(assets.bake_block("missing").is_err());
    }
}
