//! The macOS system font (San Francisco) for the mod interface, read from
//! the installed system font file at run time (nothing is bundled) and
//! rasterised at the drawable's pixel size, so text is as crisp as native
//! UI. Glyphs are packed into dedicated interface atlas layers on demand and
//! drawn with coverage alpha (no GUI alpha test). Without the font file the
//! interface falls back to the Minecraft font.
use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont, point};
use std::collections::HashMap;

/// The system UI font, a variable font with weight and optical-size axes.
const SYSTEM_FONT: &str = "/System/Library/Fonts/SFNS.ttf";

/// A packed glyph, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// Atlas layer, or `u32::MAX` for glyphs with no ink (spaces).
    pub layer: u32,
    /// Texel origin in the layer.
    pub at: [u32; 2],
    pub size: [u32; 2],
    /// Bitmap left edge from the pen, and top edge from the baseline (down
    /// is positive, so ink above the baseline is negative).
    pub left: f32,
    pub top: f32,
    pub advance: f32,
}

/// An RGBA glyph bitmap waiting for upload.
pub struct Upload {
    pub layer: u32,
    pub at: [u32; 2],
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Semibold,
}

pub struct SystemFont {
    first_layer: u32,
    layers: u32,
    size: u32,
    regular: Option<FontVec>,
    semibold: Option<FontVec>,
    glyphs: HashMap<(u32, Weight, char), Option<Glyph>>,
    /// Shelf packer: layer, x, y and the current row height.
    shelf: (u32, u32, u32, u32),
    pub pending: Vec<Upload>,
}

fn load(weight: f32) -> Option<FontVec> {
    let data = std::fs::read(SYSTEM_FONT).ok()?;
    let mut font = FontVec::try_from_vec(data).ok()?;
    font.set_variation(b"wght", weight);
    Some(font)
}

impl SystemFont {
    pub fn new(first_layer: u32, layers: u32, size: u32) -> Self {
        Self::with_fonts(first_layer, layers, size, load(400.0), load(590.0))
    }

    fn with_fonts(
        first_layer: u32,
        layers: u32,
        size: u32,
        regular: Option<FontVec>,
        semibold: Option<FontVec>,
    ) -> Self {
        Self {
            first_layer,
            layers,
            size,
            regular,
            semibold,
            glyphs: HashMap::new(),
            shelf: (0, 0, 0, 0),
            pending: Vec::new(),
        }
    }

    pub fn available(&self) -> bool {
        self.regular.is_some()
    }

    fn font(&self, weight: Weight) -> Option<&FontVec> {
        match weight {
            Weight::Regular => self.regular.as_ref(),
            Weight::Semibold => self.semibold.as_ref().or(self.regular.as_ref()),
        }
    }

    /// Ascent and descent (positive) in physical pixels for a pixel size.
    pub fn metrics(&self, pixels: u32, weight: Weight) -> (f32, f32) {
        self.font(weight)
            .map_or((pixels as f32 * 0.8, pixels as f32 * 0.2), |f| {
                let scaled = f.as_scaled(PxScale::from(pixels as f32));
                (scaled.ascent(), -scaled.descent())
            })
    }

    /// A glyph for `c`, rasterising and packing it on first use. `None`
    /// for characters the font lacks or once the atlas is full.
    pub fn glyph(&mut self, pixels: u32, weight: Weight, c: char) -> Option<Glyph> {
        let key = (pixels, weight, c);
        if let Some(glyph) = self.glyphs.get(&key) {
            return *glyph;
        }
        let glyph = self.rasterise(pixels, weight, c);
        self.glyphs.insert(key, glyph);
        glyph
    }

    fn rasterise(&mut self, pixels: u32, weight: Weight, c: char) -> Option<Glyph> {
        let font = self.font(weight)?;
        let id = font.glyph_id(c);
        if id.0 == 0 {
            return None;
        }
        let scale = PxScale::from(pixels as f32);
        let advance = font.as_scaled(scale).h_advance(id);
        let Some(outline) = font.outline_glyph(id.with_scale_and_position(scale, point(0.0, 0.0)))
        else {
            return Some(Glyph {
                layer: u32::MAX,
                at: [0, 0],
                size: [0, 0],
                left: 0.0,
                top: 0.0,
                advance,
            });
        };
        let bounds = outline.px_bounds();
        let (w, h) = (bounds.width() as u32, bounds.height() as u32);
        if w == 0 || h == 0 || w > self.size || h > self.size {
            return None;
        }
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        for pixel in rgba.chunks_mut(4) {
            pixel[3] = 0;
        }
        outline.draw(|x, y, coverage| {
            if x < w && y < h {
                rgba[((y * w + x) * 4 + 3) as usize] =
                    (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        let (layer, x, y) = self.place(w, h)?;
        self.pending.push(Upload {
            layer,
            at: [x, y],
            size: [w, h],
            rgba,
        });
        Some(Glyph {
            layer,
            at: [x, y],
            size: [w, h],
            left: bounds.min.x,
            top: bounds.min.y,
            advance,
        })
    }

    /// Shelf-pack a w by h cell with a one-texel gutter; None once every
    /// layer is full.
    fn place(&mut self, w: u32, h: u32) -> Option<(u32, u32, u32)> {
        let (mut layer, mut x, mut y, mut row) = self.shelf;
        if x + w > self.size {
            (x, y, row) = (0, y + row + 1, 0);
        }
        if y + h > self.size {
            (layer, x, y, row) = (layer + 1, 0, 0, 0);
        }
        if layer >= self.layers {
            return None;
        }
        self.shelf = (layer, x + w + 1, y, row.max(h));
        Some((self.first_layer + layer, x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_glyphs_rasterise_pack_and_cache() {
        let mut font = SystemFont::new(10, 2, 256);
        if !font.available() {
            return;
        }
        let a = font
            .glyph(32, Weight::Regular, 'A')
            .expect("the system font draws A");
        assert_eq!(a.layer, 10);
        assert!(a.size[0] > 10 && a.size[1] > 18 && a.advance > 10.0);
        assert!(a.top < -18.0, "ink sits above the baseline");
        assert!(font.pending[0].rgba.chunks(4).any(|p| p[3] == 255));
        assert_eq!(font.glyph(32, Weight::Regular, 'A'), Some(a));
        assert_eq!(font.pending.len(), 1);
        let space = font.glyph(32, Weight::Regular, ' ').unwrap();
        assert_eq!(space.layer, u32::MAX);
        assert!(space.advance > 0.0);
        let b = font.glyph(32, Weight::Semibold, 'B').unwrap();
        assert_ne!(a.at, b.at);
        let (ascent, descent) = font.metrics(32, Weight::Regular);
        assert!(ascent > 20.0 && descent > 3.0);
    }

    #[test]
    fn the_packer_wraps_rows_then_layers_and_then_stops() {
        let mut font = SystemFont::with_fonts(4, 2, 10, None, None);
        assert_eq!(font.place(6, 4), Some((4, 0, 0)));
        assert_eq!(font.place(6, 4), Some((4, 0, 5)));
        assert_eq!(font.place(6, 4), Some((5, 0, 0)));
        assert_eq!(font.place(6, 4), Some((5, 0, 5)));
        assert_eq!(font.place(6, 4), None);
    }
}
