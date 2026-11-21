//! Screen-space interface in vanilla GUI pixels: `ScaledResolution`,
//! `Gui.drawRect`, `drawGradientRect`, `drawTexturedModalRect` and
//! `FontRenderer`. It draws after the world into a non-sRGB view of the
//! drawable, so translucent overlays blend in gamma space as 1.8.9's do.
use horizon_assets_189::{Assets, Texture};
use std::{collections::BTreeMap, error::Error, sync::Arc};

/// Atlas layers are 256 pixels square: the GUI sheets and unicode pages.
pub const ATLAS: u32 = 256;
pub const ASCII: u32 = 0;
pub const ICONS: u32 = 1;
pub const WIDGETS: u32 = 2;
pub const INVENTORY: u32 = 3;
pub const CHEST: u32 = 4;
pub const HOPPER: u32 = 5;
pub const DISPENSER: u32 = 6;
pub const CRAFTING: u32 = 7;
pub const FURNACE: u32 = 8;
const SHEETS: [(&str, u32); 9] = [
    ("minecraft:font/ascii", ASCII),
    ("minecraft:gui/icons", ICONS),
    ("minecraft:gui/widgets", WIDGETS),
    ("minecraft:gui/container/inventory", INVENTORY),
    ("minecraft:gui/container/generic_54", CHEST),
    ("minecraft:gui/container/hopper", HOPPER),
    ("minecraft:gui/container/dispenser", DISPENSER),
    ("minecraft:gui/container/crafting_table", CRAFTING),
    ("minecraft:gui/container/furnace", FURNACE),
];
const FIXED_LAYERS: u32 = 16;
/// A fixed layer nothing writes: glyphs of absent pages sample transparency.
const EMPTY_LAYER: u32 = FIXED_LAYERS - 1;
const PAGE_LAYERS: u32 = 64;
/// Layers for system-font glyphs (`sysfont`), after the unicode pages.
const SYSTEM_LAYERS: u32 = 8;
pub const LAYERS: u32 = FIXED_LAYERS + PAGE_LAYERS + SYSTEM_LAYERS;
/// Vertex `layer` flag: coverage-alpha text, exempt from the alpha test.
pub const SMOOTH: u32 = 0x2000_0000;
/// Vertex `layer` flag: sample the 128-pixel entity/item array instead.
pub const ENTITY: u32 = 0x8000_0000;
pub const UNTEXTURED: u32 = u32::MAX;
/// With an entity layer: the enchantment glint over that item icon.
pub const GLINT: u32 = 0x4000_0000;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub layer: u32,
    /// Gamma-space RGBA, multiplied with the texel.
    pub color: [f32; 4],
}

/// `FontRenderer`'s ascii.png order; NULs are unused cells.
const ASCII_CHARS: &str = "ÀÁÂÈÊËÍÓÔÕÚßãõğİıŒœŞşŴŵžȇ\0\0\0\0\0\0\0 !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~\0ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αβΓπΣσμτΦΘΩδ∞∅∈∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\0";

pub(crate) fn ascii_index(c: char) -> Option<usize> {
    let code = c as u32;
    if (32..127).contains(&code) {
        return Some(code as usize);
    }
    if c == '\0' {
        return None;
    }
    ASCII_CHARS.chars().position(|a| a == c)
}

/// `FontRenderer.colorCode`: sixteen colours, then their shadows.
pub(crate) fn color_code(index: usize) -> [f32; 3] {
    let i = index as u32;
    let j = (i >> 3 & 1) * 85;
    let mut r = (i >> 2 & 1) * 170 + j;
    let mut g = (i >> 1 & 1) * 170 + j;
    let mut b = (i & 1) * 170 + j;
    if i == 6 {
        r += 85;
    }
    if i >= 16 {
        (r, g, b) = (r / 4, g / 4, b / 4);
    }
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

/// `0xAARRGGBB` to gamma RGBA.
pub fn argb(color: u32) -> [f32; 4] {
    [
        (color >> 16 & 255) as f32 / 255.0,
        (color >> 8 & 255) as f32 / 255.0,
        (color & 255) as f32 / 255.0,
        (color >> 24) as f32 / 255.0,
    ]
}

/// Owned-jar sheets, fonts and language for the interface.
pub struct Resources {
    pub language: Arc<BTreeMap<String, String>>,
    widths: [u8; 256],
    /// ascii.png cells by advance, for obfuscated text.
    by_width: Vec<Vec<u16>>,
    glyph_sizes: Vec<u8>,
    sheets: Vec<(u32, Texture)>,
    pages: Vec<Option<Vec<u8>>>,
}

impl Resources {
    pub fn load(assets: &Assets) -> Result<Self, Box<dyn Error>> {
        let mut sheets = Vec::new();
        let mut widths = [6u8; 256];
        for (name, layer) in SHEETS {
            let texture = assets.texture(name)?;
            if texture.width > ATLAS || texture.height > ATLAS {
                return Err(format!("{name} exceeds the interface atlas").into());
            }
            if layer == ASCII {
                widths = crate::models::glyph_widths(&texture.rgba, texture.width);
            }
            sheets.push((layer, texture));
        }
        let glyph_sizes = assets.read("font/glyph_sizes.bin", 1 << 16)?;
        if glyph_sizes.len() != 1 << 16 {
            return Err("glyph_sizes.bin is not 65536 bytes".into());
        }
        let pages = (0..=255u8)
            .map(|page| {
                assets
                    .read(
                        &format!("textures/font/unicode_page_{page:02x}.png"),
                        1 << 20,
                    )
                    .ok()
            })
            .collect();
        let mut by_width = vec![Vec::new(); 17];
        for (index, c) in ASCII_CHARS.chars().enumerate() {
            if c != '\0' && c != ' ' {
                by_width[usize::from(widths[index]).min(16)].push(index as u16);
            }
        }
        let mut language = assets.language()?;
        language.extend(serde_json::from_str::<BTreeMap<String, String>>(
            include_str!("pvp_lang.json"),
        )?);
        Ok(Self {
            language: Arc::new(language),
            widths,
            by_width,
            glyph_sizes,
            sheets,
            pages,
        })
    }

    /// `I18n.format` without arguments: the key itself when absent.
    pub fn translate<'a>(&'a self, key: &'a str) -> &'a str {
        self.language.get(key).map_or(key, String::as_str)
    }

    /// `FontRenderer.getCharWidth`.
    pub fn char_width(&self, c: char) -> i32 {
        match c {
            '§' => -1,
            ' ' => 4,
            _ => match ascii_index(c) {
                Some(index) => i32::from(self.widths[index]),
                None => {
                    let size = self.glyph_sizes.get(c as usize).copied().unwrap_or(0);
                    if size == 0 {
                        0
                    } else {
                        let (start, end) = (i32::from(size >> 4), i32::from(size & 15) + 1);
                        (end - start) / 2 + 1
                    }
                }
            },
        }
    }

    /// `FontRenderer.getStringWidth`, including its quirk that colour codes
    /// do not end bold.
    pub fn width(&self, text: &str) -> i32 {
        let mut width = 0;
        let mut bold = false;
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            let mut w = self.char_width(c);
            if w < 0 {
                match chars.next() {
                    Some('l' | 'L') => bold = true,
                    Some('r' | 'R') => bold = false,
                    _ => {}
                }
                w = 0;
            }
            width += w;
            if bold && w > 0 {
                width += 1;
            }
        }
        width
    }

    /// `FontRenderer.sizeStringToWidth`: characters that fit, preferring the
    /// last space; a newline always ends the line.
    fn fit(&self, chars: &[char], width: i32) -> usize {
        let (mut used, mut space, mut bold) = (0, None, false);
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            match c {
                '\n' => {
                    return k;
                }
                '§' if k + 1 < chars.len() => {
                    k += 1;
                    match chars[k] {
                        'l' | 'L' => bold = true,
                        'r' | 'R' => bold = false,
                        c if c.is_ascii_hexdigit() => bold = false,
                        _ => {}
                    }
                }
                _ => {
                    if c == ' ' {
                        space = Some(k);
                    }
                    used += self.char_width(c).max(0);
                    if bold {
                        used += 1;
                    }
                }
            }
            if used > width {
                break;
            }
            k += 1;
        }
        match space {
            Some(space) if k != chars.len() && space < k => space,
            _ => k,
        }
    }

    /// `FontRenderer.wrapFormattedStringToWidth` with a tag per character:
    /// each continuation line starts with the formatting still in effect.
    pub fn wrap(&self, text: &str, tags: &[u16], width: i32) -> Vec<(String, Vec<u16>)> {
        let mut chars: Vec<char> = text.chars().collect();
        let mut tags: Vec<u16> = tags.to_vec();
        tags.resize(chars.len(), 0);
        let mut lines = Vec::new();
        loop {
            let end = self.fit(&chars, width).min(chars.len());
            if end >= chars.len() || lines.len() >= 64 {
                lines.push((chars.iter().collect(), tags));
                return lines;
            }
            let skip = usize::from(matches!(chars.get(end), Some(' ' | '\n')));
            // Always progress, even when one glyph is wider than the line.
            let end = if skip == 0 { end.max(1) } else { end };
            let head: String = chars[..end].iter().collect();
            let carry: Vec<char> = format_codes(&head).chars().collect();
            let mut next = carry.clone();
            next.extend_from_slice(&chars[(end + skip).min(chars.len())..]);
            let mut next_tags = vec![0; carry.len()];
            next_tags.extend_from_slice(&tags[(end + skip).min(tags.len())..]);
            lines.push((head, tags[..end].to_vec()));
            chars = next;
            tags = next_tags;
        }
    }

    /// `FontRenderer.trimStringToWidth` from the start, ignoring formatting.
    pub fn trim(&self, text: &str, width: i32) -> String {
        let mut out = String::new();
        let mut used = 0;
        for c in text.chars() {
            used += self.char_width(c).max(0);
            if used > width {
                break;
            }
            out.push(c);
        }
        out
    }
}

/// `FontRenderer.getFormatFromString`: the last colour and the styles after it.
pub fn format_codes(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '§' {
            continue;
        }
        match chars.next() {
            Some(code) if code.is_ascii_hexdigit() => {
                out = format!("§{code}");
            }
            Some(code @ ('k'..='o' | 'K'..='O' | 'r' | 'R')) => {
                out.push('§');
                out.push(code);
            }
            _ => {}
        }
    }
    out
}

/// `ScaledResolution` with automatic GUI scale on the logical (point) size,
/// as LWJGL 2 sees a Retina window, then rendered at backing resolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// Physical pixels per GUI pixel.
    pub factor: f64,
    pub width: f32,
    pub height: f32,
    physical: (u32, u32),
}

impl Layout {
    pub fn new(physical: (u32, u32), backing: f64, gui_scale: u32) -> Self {
        let backing = if backing.is_finite() && backing > 0.0 {
            backing
        } else {
            1.0
        };
        let logical = (
            (f64::from(physical.0) / backing).round().max(1.0) as u32,
            (f64::from(physical.1) / backing).round().max(1.0) as u32,
        );
        let limit = if gui_scale == 0 { 1000 } else { gui_scale };
        let mut scale = 1;
        while scale < limit && logical.0 / (scale + 1) >= 320 && logical.1 / (scale + 1) >= 240 {
            scale += 1;
        }
        let factor = f64::from(scale) * backing;
        Self {
            factor,
            width: (f64::from(physical.0) / factor).ceil() as f32,
            height: (f64::from(physical.1) / factor).ceil() as f32,
            physical,
        }
    }

    /// Window pixels to whole GUI pixels, as `Mouse.getX() * w / displayWidth`.
    pub fn gui(&self, cursor: (f64, f64)) -> (f32, f32) {
        (
            (cursor.0 / self.factor).floor() as f32,
            (cursor.1 / self.factor).floor() as f32,
        )
    }

    /// Clip-space scale for the vertex shader.
    fn scale(&self) -> [f32; 2] {
        [
            (2.0 * self.factor / f64::from(self.physical.0.max(1))) as f32,
            (2.0 * self.factor / f64::from(self.physical.1.max(1))) as f32,
        ]
    }
}

/// Unicode font pages resident in atlas layers, and the system font's
/// glyph cache in the layers after them.
pub struct Pages {
    resident: Vec<Option<u32>>,
    next: u32,
    pub system: std::cell::RefCell<crate::sysfont::SystemFont>,
}

impl Default for Pages {
    fn default() -> Self {
        Self {
            resident: vec![None; 256],
            next: FIXED_LAYERS,
            system: std::cell::RefCell::new(crate::sysfont::SystemFont::new(
                FIXED_LAYERS + PAGE_LAYERS,
                SYSTEM_LAYERS,
                ATLAS,
            )),
        }
    }
}

/// Builds one frame's interface geometry.
pub struct Canvas<'a> {
    pub res: &'a Resources,
    pub layout: Layout,
    /// `FontRenderer.fontRandom` stand-in for obfuscated text, per tick.
    pub tick: u64,
    /// `GlStateManager.scale` for the next draws, such as the death title.
    pub scale: f32,
    pub out: Vec<Vertex>,
    /// Drawn first with the inverting blend of the crosshair.
    pub inverted: Vec<Vertex>,
    /// Glass under the interface (0) and under the overlay (1).
    glass: [Vec<crate::glass::Panel>; 2],
    /// `out` length where the overlay begins.
    split: Option<usize>,
    pages: &'a Pages,
    missing: Vec<u8>,
}

#[derive(Clone, Copy, Default)]
struct Style {
    obfuscated: bool,
    bold: bool,
    strikethrough: bool,
    underline: bool,
    italic: bool,
}

impl<'a> Canvas<'a> {
    pub fn new(res: &'a Resources, layout: Layout, tick: u64, pages: &'a Pages) -> Self {
        Self {
            res,
            layout,
            tick,
            scale: 1.0,
            out: Vec::with_capacity(16 * 1024),
            inverted: Vec::new(),
            glass: Default::default(),
            split: None,
            pages,
            missing: Vec::new(),
        }
    }

    fn quad(
        &mut self,
        inverted: bool,
        corners: [[f32; 2]; 4],
        uv: [[f32; 2]; 4],
        layer: u32,
        color: [f32; 4],
    ) {
        let scale = self.scale;
        let out = if inverted {
            &mut self.inverted
        } else {
            &mut self.out
        };
        for i in [0, 1, 2, 0, 2, 3] {
            out.push(Vertex {
                position: corners[i].map(|v| v * scale),
                uv: uv[i],
                layer,
                color,
            });
        }
    }

    /// Physical pixels for `size` GUI pixels of system-font text.
    fn system_pixels(&self, size: f32) -> u32 {
        (f64::from(size) * self.layout.factor).round().max(4.0) as u32
    }

    /// Width in GUI pixels of system-font text (the Minecraft font's width
    /// when the system font is missing).
    pub fn system_width(&self, text: &str, size: f32, weight: crate::sysfont::Weight) -> f32 {
        let mut font = self.pages.system.borrow_mut();
        if !font.available() {
            return self.res.width(text) as f32;
        }
        let pixels = self.system_pixels(size);
        let advance: f32 = text
            .chars()
            .filter_map(|c| font.glyph(pixels, weight, c))
            .map(|g| g.advance)
            .sum();
        advance / self.layout.factor as f32
    }

    /// San Francisco text whose line box starts at (x, y) and is `size` GUI
    /// pixels tall, snapped to physical pixels. Returns its width.
    pub fn system_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        weight: crate::sysfont::Weight,
        color: u32,
    ) -> f32 {
        let factor = self.layout.factor as f32;
        let pixels = self.system_pixels(size);
        let mut quads = Vec::new();
        let width = {
            let mut font = self.pages.system.borrow_mut();
            if !font.available() {
                drop(font);
                let top = (y + (size - 8.0) / 2.0).floor();
                return self.text(text, x, top, color, false);
            }
            let (ascent, descent) = font.metrics(pixels, weight);
            // Centre the font's ascent plus descent in the line box.
            let line = size * factor;
            let baseline = (y * factor + (line - ascent - descent) / 2.0 + ascent).round();
            let start = (x * factor).round();
            let mut pen = start;
            for c in text.chars() {
                let Some(glyph) = font.glyph(pixels, weight, c) else {
                    continue;
                };
                if glyph.layer != u32::MAX {
                    let left = (pen + glyph.left).round();
                    let top = baseline + glyph.top.round();
                    quads.push((left, top, glyph));
                }
                pen += glyph.advance;
            }
            (pen - start) / factor
        };
        let color = argb(color);
        let s = 1.0 / ATLAS as f32;
        for (left, top, glyph) in quads {
            let (x0, y0) = (left / factor, top / factor);
            let (w, h) = (glyph.size[0] as f32, glyph.size[1] as f32);
            let (x1, y1) = (x0 + w / factor, y0 + h / factor);
            let (u0, v0) = (glyph.at[0] as f32 * s, glyph.at[1] as f32 * s);
            let (u1, v1) = (u0 + w * s, v0 + h * s);
            self.quad(
                false,
                [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
                [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
                glyph.layer | SMOOTH,
                color,
            );
        }
        width
    }

    /// A liquid-glass panel under what is drawn next in this layer.
    pub fn glass(&mut self, panel: crate::glass::Panel) {
        let scale = self.scale;
        let mut panel = panel;
        for v in &mut panel.rect {
            *v *= scale;
        }
        panel.radius *= scale;
        self.glass[usize::from(self.split.is_some())].push(panel);
    }

    /// Start the overlay: later glass frosts everything drawn so far.
    pub fn overlay(&mut self) {
        self.split.get_or_insert(self.out.len());
    }

    /// This frame's glass layers and the overlay's start in `out`.
    pub fn take_glass(&mut self) -> ([Vec<crate::glass::Panel>; 2], Option<usize>) {
        (std::mem::take(&mut self.glass), self.split)
    }

    /// `Gui.drawRect` with a `0xAARRGGBB` colour; corners in any order.
    pub fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u32) {
        self.gradient(x0, y0, x1, y1, color, color);
    }

    /// `Gui.drawGradientRect`: top colour to bottom colour.
    pub fn gradient(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, top: u32, bottom: u32) {
        let (left, right) = (x0.min(x1), x0.max(x1));
        let (upper, lower) = (y0.min(y1), y0.max(y1));
        let (t, b) = (argb(top), argb(bottom));
        let corners = [[left, upper], [right, upper], [right, lower], [left, lower]];
        let colors = [t, t, b, b];
        for i in [0, 1, 2, 0, 2, 3] {
            self.out.push(Vertex {
                position: corners[i].map(|v| v * self.scale),
                uv: [0.0; 2],
                layer: UNTEXTURED,
                color: colors[i],
            });
        }
    }

    /// `Gui.drawTexturedModalRect` from a 256-pixel sheet.
    #[allow(clippy::too_many_arguments)]
    pub fn sprite(
        &mut self,
        sheet: u32,
        x: f32,
        y: f32,
        u: f32,
        v: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
    ) {
        let s = 1.0 / ATLAS as f32;
        self.quad(
            false,
            [[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
            [
                [u * s, v * s],
                [(u + w) * s, v * s],
                [(u + w) * s, (v + h) * s],
                [u * s, (v + h) * s],
            ],
            sheet,
            color,
        );
    }

    /// The crosshair: drawn with `ONE_MINUS_DST_COLOR, ONE_MINUS_SRC_COLOR`.
    pub fn inverted_sprite(&mut self, sheet: u32, x: f32, y: f32, u: f32, v: f32, w: f32, h: f32) {
        let s = 1.0 / ATLAS as f32;
        self.quad(
            true,
            [[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
            [
                [u * s, v * s],
                [(u + w) * s, v * s],
                [(u + w) * s, (v + h) * s],
                [u * s, (v + h) * s],
            ],
            sheet,
            [1.0; 4],
        );
    }

    /// A 16-pixel item icon from the entity/item array.
    pub fn item_icon(&mut self, layer: u32, x: f32, y: f32) {
        self.item_icon_tinted(layer, x, y, [1.0; 4]);
    }

    /// An item layer multiplied by a gamma-space colour.
    pub fn item_icon_tinted(&mut self, layer: u32, x: f32, y: f32, color: [f32; 4]) {
        let s = 16.0 / crate::models::LAYER_SIZE as f32;
        self.quad(
            false,
            [[x, y], [x + 16.0, y], [x + 16.0, y + 16.0], [x, y + 16.0]],
            [[0.0, 0.0], [s, 0.0], [s, s], [0.0, s]],
            ENTITY | layer,
            color,
        );
    }

    /// A textured quad in GUI pixels from the entity/item array, for
    /// projected 3D icons.
    pub fn entity_quad(
        &mut self,
        corners: [[f32; 2]; 4],
        uv: [[f32; 2]; 4],
        layer: u32,
        color: [f32; 4],
    ) {
        self.quad(false, corners, uv, ENTITY | layer, color);
    }

    /// `RenderItem.renderEffect` over a 16-pixel icon: two passes scrolled
    /// by the clock and turned -50 and 10 degrees, as on held items.
    pub fn item_glint(&mut self, layer: u32, glint: u32, x: f32, y: f32, millis: u64) {
        let s = 16.0 / crate::models::LAYER_SIZE as f32;
        let passes = [
            ((millis % 3000) as f32 / 3000.0 / 8.0, -50f32),
            (-((millis % 4873) as f32 / 4873.0 / 8.0), 10f32),
        ];
        for (scroll, degrees) in passes {
            self.quad(
                false,
                [[x, y], [x + 16.0, y], [x + 16.0, y + 16.0], [x, y + 16.0]],
                [[0.0, 0.0], [s, 0.0], [s, s], [0.0, s]],
                ENTITY | GLINT | layer,
                [scroll, degrees.to_radians(), glint as f32, 0.0],
            );
        }
    }

    /// A player face (and hat layer) from a 64-pixel skin in the entity
    /// array, as `Gui.drawScaledCustomSizeModalRect`.
    pub fn skin_face(&mut self, layer: u32, x: f32, y: f32) {
        let s = 1.0 / crate::models::LAYER_SIZE as f32;
        for u in [8.0, 40.0] {
            self.quad(
                false,
                [[x, y], [x + 8.0, y], [x + 8.0, y + 8.0], [x, y + 8.0]],
                [
                    [u * s, 8.0 * s],
                    [(u + 8.0) * s, 8.0 * s],
                    [(u + 8.0) * s, 16.0 * s],
                    [u * s, 16.0 * s],
                ],
                ENTITY | layer,
                [1.0; 4],
            );
        }
    }

    /// `FontRenderer.drawString`/`drawStringWithShadow`; returns the end x.
    pub fn text(&mut self, text: &str, x: f32, y: f32, color: u32, shadow: bool) -> f32 {
        let color = if color & 0xFC00_0000 == 0 {
            color | 0xFF00_0000
        } else {
            color
        };
        if shadow {
            let shade = (color & 0x00FC_FCFC) >> 2 | (color & 0xFF00_0000);
            let behind = self.render(text, x + 1.0, y + 1.0, shade, true);
            behind.max(self.render(text, x, y, color, false))
        } else {
            self.render(text, x, y, color, false)
        }
    }

    pub fn centered(&mut self, text: &str, x: f32, y: f32, color: u32, shadow: bool) {
        let width = self.res.width(text) as f32;
        self.text(text, x - (width / 2.0).floor(), y, color, shadow);
    }

    /// Characters as obfuscated text replaces them: same width, from ascii.
    fn random_glyph(&self, width: u8, seed: u64) -> Option<char> {
        let candidates = self.res.by_width.get(usize::from(width))?;
        if candidates.is_empty() {
            return None;
        }
        let mixed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ self.tick.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        let index = candidates[(mixed >> 33) as usize % candidates.len()];
        ASCII_CHARS.chars().nth(usize::from(index))
    }

    /// `FontRenderer.renderStringAtPos`.
    fn render(&mut self, text: &str, x: f32, y: f32, color: u32, shadow: bool) -> f32 {
        let base = argb(color);
        let mut rgba = base;
        let mut style = Style::default();
        let mut pos = x;
        let mut chars = text.chars().enumerate().peekable();
        while let Some((index, c)) = chars.next() {
            if c == '§' && chars.peek().is_some() {
                let (_, code) = chars.next().expect("peeked");
                let i = "0123456789abcdefklmnor".find(code.to_ascii_lowercase());
                match i {
                    None | Some(0..=15) => {
                        style = Style::default();
                        let i = i.unwrap_or(15) + if shadow { 16 } else { 0 };
                        let [r, g, b] = color_code(i);
                        rgba = [r, g, b, base[3]];
                    }
                    Some(16) => style.obfuscated = true,
                    Some(17) => style.bold = true,
                    Some(18) => style.strikethrough = true,
                    Some(19) => style.underline = true,
                    Some(20) => style.italic = true,
                    _ => {
                        style = Style::default();
                        rgba = base;
                    }
                }
                continue;
            }
            let mut glyph = c;
            let ascii = ascii_index(c);
            if style.obfuscated
                && let Some(index_in_font) = ascii
                && let Some(swap) = self.random_glyph(self.res.widths[index_in_font], index as u64)
            {
                glyph = swap;
            }
            let unicode_shadow = shadow && ascii_index(glyph).is_none();
            let mut advance = self.glyph(glyph, pos, y, style.italic, rgba, unicode_shadow);
            if style.bold {
                self.glyph(glyph, pos + 1.0, y, style.italic, rgba, unicode_shadow);
                advance += 1.0;
            }
            if style.strikethrough {
                let line = [pos, y + 4.0, pos + advance, y + 3.0];
                self.line(line, rgba);
            }
            if style.underline {
                let line = [pos - 1.0, y + 9.0, pos + advance, y + 8.0];
                self.line(line, rgba);
            }
            pos += advance.trunc();
        }
        pos
    }

    fn line(&mut self, [x0, y0, x1, y1]: [f32; 4], color: [f32; 4]) {
        let (top, bottom) = (y0.min(y1), y0.max(y1));
        self.quad(
            false,
            [[x0, top], [x1, top], [x1, bottom], [x0, bottom]],
            [[0.0; 2]; 4],
            UNTEXTURED,
            color,
        );
    }

    /// `renderDefaultChar`/`renderUnicodeChar`; returns the advance.
    fn glyph(
        &mut self,
        c: char,
        x: f32,
        y: f32,
        italic: bool,
        color: [f32; 4],
        unicode_shadow: bool,
    ) -> f32 {
        let skew = if italic { 1.0 } else { 0.0 };
        let s = 1.0 / ATLAS as f32;
        if c == ' ' {
            return 4.0;
        }
        if let Some(index) = ascii_index(c) {
            let width = f32::from(self.res.widths[index]);
            let (u, v) = ((index % 16 * 8) as f32, (index / 16 * 8) as f32);
            let w = width - 1.01;
            self.quad(
                false,
                [
                    [x + skew, y],
                    [x + w + skew, y],
                    [x + w - skew, y + 7.99],
                    [x - skew, y + 7.99],
                ],
                [
                    [u * s, v * s],
                    [(u + w) * s, v * s],
                    [(u + w) * s, (v + 7.99) * s],
                    [u * s, (v + 7.99) * s],
                ],
                ASCII,
                color,
            );
            return width;
        }
        let code = c as u32;
        let size = self
            .res
            .glyph_sizes
            .get(code as usize)
            .copied()
            .unwrap_or(0);
        if code > 0xFFFF || size == 0 {
            return 0.0;
        }
        let page = (code >> 8) as usize;
        let Some(layer) = self.pages.resident[page] else {
            if !self.missing.contains(&(page as u8)) {
                self.missing.push(page as u8);
            }
            return (self.res.char_width(c)) as f32;
        };
        let (start, end) = (f32::from(size >> 4), f32::from(size & 15) + 1.0);
        let u = (code % 16 * 16) as f32 + start;
        let v = ((code & 255) / 16 * 16) as f32;
        let w = end - start - 0.02;
        let (x, y) = if unicode_shadow {
            (x - 1.0, y - 1.0)
        } else {
            (x, y)
        };
        self.quad(
            false,
            [
                [x + skew, y],
                [x + w / 2.0 + skew, y],
                [x + w / 2.0 - skew, y + 7.99],
                [x - skew, y + 7.99],
            ],
            [
                [u * s, v * s],
                [(u + w) * s, v * s],
                [(u + w) * s, (v + 15.98) * s],
                [u * s, (v + 15.98) * s],
            ],
            layer,
            color,
        );
        ((end - start) / 2.0 + 1.0).trunc()
    }

    /// `GuiScreen.drawHoveringText`: the purple-bordered tooltip.
    pub fn tooltip(&mut self, lines: &[String], mouse: (f32, f32)) {
        if lines.is_empty() {
            return;
        }
        let width = lines.iter().map(|l| self.res.width(l)).max().unwrap_or(0) as f32;
        let mut x = mouse.0 + 12.0;
        let mut y = mouse.1 - 12.0;
        let height = if lines.len() > 1 {
            8.0 + 2.0 + (lines.len() - 1) as f32 * 10.0
        } else {
            8.0
        };
        if x + width > self.layout.width {
            x -= 28.0 + width;
        }
        if y + height + 6.0 > self.layout.height {
            y = self.layout.height - height - 6.0;
        }
        let background = 0xF010_0010;
        self.gradient(
            x - 3.0,
            y - 4.0,
            x + width + 3.0,
            y - 3.0,
            background,
            background,
        );
        self.gradient(
            x - 3.0,
            y + height + 3.0,
            x + width + 3.0,
            y + height + 4.0,
            background,
            background,
        );
        self.gradient(
            x - 3.0,
            y - 3.0,
            x + width + 3.0,
            y + height + 3.0,
            background,
            background,
        );
        self.gradient(
            x - 4.0,
            y - 3.0,
            x - 3.0,
            y + height + 3.0,
            background,
            background,
        );
        self.gradient(
            x + width + 3.0,
            y - 3.0,
            x + width + 4.0,
            y + height + 3.0,
            background,
            background,
        );
        let (top, bottom) = (0x5050_00FF, 0x5028_007F);
        self.gradient(x - 3.0, y - 2.0, x - 2.0, y + height + 2.0, top, bottom);
        self.gradient(
            x + width + 2.0,
            y - 2.0,
            x + width + 3.0,
            y + height + 2.0,
            top,
            bottom,
        );
        self.gradient(x - 3.0, y - 3.0, x + width + 3.0, y - 2.0, top, top);
        self.gradient(
            x - 3.0,
            y + height + 2.0,
            x + width + 3.0,
            y + height + 3.0,
            bottom,
            bottom,
        );
        let mut line_y = y;
        for (index, line) in lines.iter().enumerate() {
            self.text(line, x, line_y, 0xFFFF_FFFF, true);
            line_y += if index == 0 { 12.0 } else { 10.0 };
        }
    }

    /// Unicode pages this frame needed but the atlas lacks.
    pub fn finish(self) -> (Vec<Vertex>, Vec<Vertex>, Vec<u8>) {
        (self.out, self.inverted, self.missing)
    }
}

/// The interface's GPU resources.
pub struct Pass {
    pipeline: wgpu::RenderPipeline,
    invert_pipeline: wgpu::RenderPipeline,
    bindings: wgpu::BindGroup,
    atlas: wgpu::Texture,
    uniform: wgpu::Buffer,
    vertices: wgpu::Buffer,
    capacity: u64,
    inverted: u32,
    count: u32,
    /// Vertices before the overlay.
    split: u32,
    pub pages: Pages,
}

impl Pass {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: wgpu::TextureFormat,
        resources: &Resources,
        entity: &wgpu::Texture,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Legacy interface"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Legacy interface layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1),
                texture(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Legacy interface pipeline"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let make = |blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Legacy interface"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0=>Float32x2, 1=>Float32x2, 2=>Uint32, 3=>Float32x4],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::COLOR,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // Premultiplied, so a glint (alpha 0) adds its colour.
        let pipeline = make(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let invert = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::OneMinusDst,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add,
        };
        let invert_pipeline = make(wgpu::BlendState {
            color: invert,
            alpha: wgpu::BlendComponent::OVER,
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Legacy interface atlas"),
            size: wgpu::Extent3d {
                width: ATLAS,
                height: ATLAS,
                depth_or_array_layers: LAYERS,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Gamma bytes as they are: the interface blends in gamma space.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, image) in &resources.sheets {
            Self::write(queue, &atlas, *layer, image);
        }
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Legacy interface screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let capacity = 256 * 1024;
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Legacy interface vertices"),
            size: capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bindings = Self::bind(device, &layout, &uniform, &atlas, entity);
        Self {
            pipeline,
            invert_pipeline,
            bindings,
            atlas,
            uniform,
            vertices,
            capacity,
            inverted: 0,
            count: 0,
            split: 0,
            pages: Pages::default(),
        }
    }

    fn bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::Buffer,
        atlas: &wgpu::Texture,
        entity: &wgpu::Texture,
    ) -> wgpu::BindGroup {
        let array = wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Legacy interface sampler"),
            ..Default::default()
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Legacy interface resources"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.create_view(&array)),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&entity.create_view(&array)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        })
    }

    fn write(queue: &wgpu::Queue, atlas: &wgpu::Texture, layer: u32, image: &Texture) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            wgpu::Extent3d {
                width: image.width.min(ATLAS),
                height: image.height.min(ATLAS),
                depth_or_array_layers: 1,
            },
        );
    }

    /// Upload system-font glyphs rasterised this frame.
    pub fn upload_glyphs(&mut self, queue: &wgpu::Queue) {
        let uploads = std::mem::take(&mut self.pages.system.get_mut().pending);
        for glyph in uploads {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: glyph.at[0],
                        y: glyph.at[1],
                        z: glyph.layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &glyph.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(glyph.size[0] * 4),
                    rows_per_image: Some(glyph.size[1]),
                },
                wgpu::Extent3d {
                    width: glyph.size[0],
                    height: glyph.size[1],
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Decode and upload unicode pages a frame asked for.
    pub fn load_pages(&mut self, queue: &wgpu::Queue, resources: &Resources, missing: &[u8]) {
        for &page in missing {
            let index = usize::from(page);
            if self.pages.resident[index].is_some() || self.pages.next >= LAYERS {
                continue;
            }
            let decoded = resources.pages[index]
                .as_deref()
                .map(Assets::decode_png)
                .and_then(Result::ok)
                .filter(|image| image.width == ATLAS && image.height == ATLAS);
            match decoded {
                Some(image) => {
                    Self::write(queue, &self.atlas, self.pages.next, &image);
                    self.pages.resident[index] = Some(self.pages.next);
                    self.pages.next += 1;
                }
                None => {
                    // Mark absent pages resident on the empty layer so they
                    // are not decoded again; their glyphs draw nothing.
                    self.pages.resident[index] = Some(EMPTY_LAYER);
                }
            }
        }
    }

    /// Replace this frame's geometry.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: Layout,
        entity_layers: u32,
        inverted: &[Vertex],
        vertices: &[Vertex],
        split: Option<usize>,
    ) -> Result<(), Box<dyn Error>> {
        let [x, y] = layout.scale();
        let screen = [x, y, LAYERS as f32, entity_layers.max(1) as f32];
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&screen));
        let size = (std::mem::size_of_val(inverted) + std::mem::size_of_val(vertices)) as u64;
        if size > self.capacity {
            self.capacity = size
                .next_power_of_two()
                .min(device.limits().max_buffer_size);
            if size > self.capacity {
                return Err("Interface mesh exceeds device buffer limit".into());
            }
            self.vertices = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Legacy interface vertices"),
                size: self.capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !inverted.is_empty() {
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(inverted));
        }
        if !vertices.is_empty() {
            queue.write_buffer(
                &self.vertices,
                std::mem::size_of_val(inverted) as u64,
                bytemuck::cast_slice(vertices),
            );
        }
        self.inverted = u32::try_from(inverted.len())?;
        self.count = u32::try_from(vertices.len())?;
        self.split = u32::try_from(split.unwrap_or(vertices.len()).min(vertices.len()))?;
        Ok(())
    }

    /// Part 0 is the interface (inverted crosshair first), part 1 the
    /// overlay after `Canvas::overlay`.
    pub fn draw_part(&self, pass: &mut wgpu::RenderPass<'_>, part: usize) {
        let range = if part == 0 {
            self.inverted..self.inverted + self.split
        } else {
            self.inverted + self.split..self.inverted + self.count
        };
        if range.is_empty() && (part == 1 || self.inverted == 0) {
            return;
        }
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        if part == 0 && self.inverted > 0 {
            pass.set_pipeline(&self.invert_pipeline);
            pass.draw(0..self.inverted, 0..1);
        }
        if !range.is_empty() {
            pass.set_pipeline(&self.pipeline);
            pass.draw(range, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_table_has_vanilla_cells_and_colours() {
        assert_eq!(ASCII_CHARS.chars().count(), 256);
        assert_eq!(ascii_index('A'), Some(65));
        assert_eq!(ascii_index('À'), Some(0));
        assert_eq!(ascii_index('■'), Some(254));
        assert_eq!(ascii_index('✫'), None);
        assert_eq!(color_code(6), [1.0, 170.0 / 255.0, 0.0]);
        assert_eq!(color_code(16 + 15), [63.0 / 255.0; 3]);
        assert_eq!(color_code(12), [1.0, 85.0 / 255.0, 85.0 / 255.0]);
    }

    #[test]
    fn scaled_resolution_matches_lwjgl_on_retina() {
        // 960x600 points on a 2x display: scale 2 at 480x300, four device
        // pixels per GUI pixel.
        let retina = Layout::new((1920, 1200), 2.0, 0);
        assert_eq!(
            (retina.width, retina.height, retina.factor),
            (480.0, 300.0, 4.0)
        );
        let full_hd = Layout::new((1920, 1080), 1.0, 0);
        assert_eq!((full_hd.width, full_hd.height), (480.0, 270.0));
        assert_eq!(Layout::new((1920, 1080), 1.0, 2).width, 960.0);
        assert_eq!(retina.gui((1919.0, 3.0)), (479.0, 0.0));
    }

    fn fixed_width() -> Resources {
        Resources {
            language: Arc::default(),
            widths: [6; 256],
            by_width: vec![Vec::new(); 17],
            glyph_sizes: vec![0; 1 << 16],
            sheets: Vec::new(),
            pages: vec![None; 256],
        }
    }

    #[test]
    fn wrapping_breaks_at_spaces_and_carries_formatting() {
        let res = fixed_width();
        // Six-pixel glyphs and four-pixel spaces: "aaaa bbbb" is 52 wide.
        assert_eq!(res.width("§laa"), 14);
        let text = "§aaaaa bbbb §lcccc";
        let tags: Vec<u16> = (0..text.chars().count() as u16).collect();
        let lines = res.wrap(text, &tags, 40);
        let texts: Vec<&str> = lines.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, ["§aaaaa", "§abbbb", "§a§lcccc"]);
        // Carried codes are untagged; text keeps its original tags.
        assert_eq!(lines[1].1, [0, 0, 7, 8, 9, 10]);
        assert_eq!(res.wrap("a\nb", &[1, 2, 3], 300).len(), 2);
        assert_eq!(res.trim("abcdef", 13), "ab");
    }

    #[test]
    fn format_codes_carry_the_last_colour_and_later_styles() {
        assert_eq!(format_codes("§a§lHi §cthere§n"), "§c§n");
        assert_eq!(format_codes("plain"), "");
        assert_eq!(format_codes("§l§obold"), "§l§o");
    }
}
