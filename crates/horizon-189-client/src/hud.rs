//! `GuiIngame` on the interface layer: crosshair, hotbar, health, armour,
//! food, experience and the action bar, plus `GuiGameOver`.
use crate::{
    gameplay::Vitals,
    inventory::View,
    scoreboard::{Scoreboard, TabList},
    terrain::Registry,
    ui::{Canvas, ICONS, Layout, WIDGETS},
};
use horizon_protocol_47::scoreboard::Title;

/// Potion effects the bars draw differently.
pub const REGENERATION: u32 = 1 << 10;
pub const HUNGER: u32 = 1 << 17;
pub const POISON: u32 = 1 << 19;
pub const WITHER: u32 = 1 << 20;

/// What the overlay shows this frame.
pub struct Frame<'a> {
    pub view: &'a View,
    pub vitals: Vitals,
    pub game_mode: u8,
    pub hardcore: bool,
    /// Experience bar fill (0..1) and level.
    pub experience: (f32, i32),
    pub mouse: (f32, f32),
    /// Client ticks since start, with the partial tick.
    pub tick: f64,
    pub scoreboard: &'a Scoreboard,
    pub tab: &'a TabList,
    pub own_name: &'a str,
    /// The player-list key is held.
    pub tab_held: bool,
    /// Online-mode connections show faces in the tab list.
    pub heads: bool,
    /// Entity-array layer of a player's loaded skin, if any.
    pub skin: &'a dyn Fn(u128) -> Option<u32>,
}

/// Overlay state that outlives one frame.
#[derive(Default)]
pub struct Hud {
    /// `playerHealth`, `lastPlayerHealth`, `healthUpdateCounter` and
    /// `lastSystemTime` of `GuiIngame.renderPlayerStats`.
    health: Option<i32>,
    last_health: i32,
    blink_until: f64,
    settled_at: f64,
    /// The action bar's text and when it was set.
    action_bar: Option<(String, f64)>,
    /// When the death screen opened.
    died_at: Option<f64>,
    title: String,
    subtitle: String,
    /// Fade-in, stay and fade-out ticks.
    title_times: Option<(i32, i32, i32)>,
    /// The tick `titlesTimer` reaches zero.
    title_until: f64,
    /// `highlightingItemStack` (as id, damage, name) and when its name
    /// started showing above the hotbar.
    highlight: Option<((i16, i16, String), f64)>,
}

/// A `GuiGameOver` button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathButton {
    Respawn,
    TitleScreen,
}

/// A deterministic stand-in for `rand.setSeed(updateCounter * 312871)`.
fn jitter(tick: u64, salt: u64, range: u64) -> u64 {
    let mut x = tick.wrapping_mul(312_871) ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    (x >> 29) % range.max(1)
}

impl Hud {
    /// `GuiIngame.setRecordPlaying`: shown for 60 ticks.
    pub fn set_action_bar(&mut self, text: String, tick: f64) {
        self.action_bar = Some((text, tick));
    }

    fn times(&self) -> (i32, i32, i32) {
        self.title_times.unwrap_or((10, 70, 20))
    }

    /// `GuiIngame.displayTitle` as `handleTitle` drives it; text arrives
    /// formatted.
    pub fn set_title(&mut self, title: Title, tick: f64) {
        match title {
            Title::Times {
                fade_in,
                stay,
                fade_out,
            } => {
                self.title_times = Some((fade_in.max(0), stay.max(0), fade_out.max(0)));
                if self.title_until > tick {
                    let (a, b, c) = self.times();
                    self.title_until = tick + f64::from(a + b + c);
                }
            }
            Title::Subtitle(text) => self.subtitle = text,
            Title::Title(text) => {
                self.title = text;
                let (a, b, c) = self.times();
                self.title_until = tick + f64::from(a + b + c);
            }
            Title::Clear => {
                self.title.clear();
                self.subtitle.clear();
                self.title_until = tick;
            }
            Title::Reset => {
                self.subtitle.clear();
                self.title_times = None;
            }
        }
    }

    fn draw_titles(&self, canvas: &mut Canvas<'_>, tick: f64) {
        let left = self.title_until - tick;
        if left <= 0.0 {
            return;
        }
        let (a, b, c) = self.times();
        let (a, b, c) = (f64::from(a), f64::from(b), f64::from(c));
        let mut alpha = 255.0;
        if left > c + b && a > 0.0 {
            alpha = (a + b + c - left) * 255.0 / a;
        }
        if left <= c && c > 0.0 {
            alpha = left * 255.0 / c;
        }
        let alpha = alpha.clamp(0.0, 255.0) as u32;
        if alpha <= 8 {
            return;
        }
        let (cx, cy) = (
            (canvas.layout.width / 2.0).floor(),
            (canvas.layout.height / 2.0).floor(),
        );
        let color = 0x00FF_FFFF | alpha << 24;
        canvas.scale = 4.0;
        let x = cx / 4.0 - (canvas.res.width(&self.title) / 2) as f32;
        canvas.text(&self.title, x, cy / 4.0 - 10.0, color, true);
        canvas.scale = 2.0;
        let x = cx / 2.0 - (canvas.res.width(&self.subtitle) / 2) as f32;
        canvas.text(&self.subtitle, x, cy / 2.0 + 5.0, color, true);
        canvas.scale = 1.0;
    }

    /// `renderScoreboard`. Vanilla draws the text with blending off, so
    /// its `0x20FFFFFF` colour shows opaque.
    fn draw_sidebar(canvas: &mut Canvas<'_>, frame: &Frame<'_>) {
        let Some((title, lines)) = frame.scoreboard.sidebar(frame.own_name) else {
            return;
        };
        let res = canvas.res;
        let mut width = res.width(&title);
        for (name, points) in &lines {
            width = width.max(res.width(&format!("{name}: §c{points}")));
        }
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        let height = (lines.len() * 9) as f32;
        let bottom = (h / 2.0).floor() + (height / 3.0).floor();
        let left = w - width as f32 - 3.0;
        let right = w - 3.0 + 2.0;
        let count = lines.len();
        for (index, (name, points)) in lines.iter().enumerate() {
            let y = bottom - (index + 1) as f32 * 9.0;
            canvas.rect(left - 2.0, y, right, y + 9.0, 0x5000_0000);
            canvas.text(name, left, y, 0xFFFF_FFFF, false);
            let value = format!("§c{points}");
            let value_x = right - res.width(&value) as f32;
            canvas.text(&value, value_x, y, 0xFFFF_FFFF, false);
            if index + 1 == count {
                canvas.rect(left - 2.0, y - 10.0, right, y - 1.0, 0x6000_0000);
                canvas.rect(left - 2.0, y - 1.0, right, y, 0x5000_0000);
                let title_x = left + (width / 2) as f32 - (res.width(&title) / 2) as f32;
                canvas.text(&title, title_x, y - 9.0, 0xFFFF_FFFF, false);
            }
        }
    }

    /// Overlays drawn after chat: the tab list while its key is held.
    pub fn draw_after_chat(&self, canvas: &mut Canvas<'_>, frame: &Frame<'_>) {
        if frame.tab_held && self.died_at.is_none() {
            tab_list(canvas, frame);
        }
    }

    fn death_buttons(layout: Layout) -> [(DeathButton, [f32; 4]); 2] {
        let x = (layout.width / 2.0).floor() - 100.0;
        let y = (layout.height / 4.0).floor();
        [
            (DeathButton::Respawn, [x, y + 72.0, 200.0, 20.0]),
            (DeathButton::TitleScreen, [x, y + 96.0, 200.0, 20.0]),
        ]
    }

    /// The death-screen button under the mouse, once buttons are enabled.
    /// GuiGameOver enables its buttons twenty ticks after it opens.
    pub fn respawn_ready(&self, tick: f64) -> bool {
        self.died_at.is_some_and(|died_at| tick - died_at >= 20.0)
    }

    pub fn death_button_at(
        &self,
        layout: Layout,
        mouse: (f32, f32),
        tick: f64,
    ) -> Option<DeathButton> {
        if !self.respawn_ready(tick) {
            return None;
        }
        Self::death_buttons(layout)
            .into_iter()
            .find(|(_, [x, y, w, h])| {
                mouse.0 >= *x && mouse.1 >= *y && mouse.0 < x + w && mouse.1 < y + h
            })
            .map(|(button, _)| button)
    }

    pub fn draw(&mut self, canvas: &mut Canvas<'_>, registry: &Registry, frame: &Frame<'_>) {
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        let tick = frame.tick.floor() as u64;
        let survival = matches!(frame.game_mode, 0 | 2);
        if frame.vitals.health <= 0.0 {
            self.died_at.get_or_insert(frame.tick);
        } else {
            self.died_at = None;
        }
        if frame.game_mode != 3 {
            canvas.inverted_sprite(
                ICONS,
                (w / 2.0).floor() - 7.0,
                (h / 2.0).floor() - 7.0,
                0.0,
                0.0,
                16.0,
                16.0,
            );
        }
        if frame.game_mode != 3 {
            self.hotbar(canvas, registry, frame);
            self.draw_highlight(canvas, frame, survival);
        }
        if survival {
            self.stats(canvas, frame, tick);
            Self::experience(canvas, frame);
        }
        self.draw_action_bar(canvas, frame.tick);
        self.draw_titles(canvas, frame.tick);
        Self::draw_sidebar(canvas, frame);
        if let Some(died_at) = self.died_at {
            Self::death_screen(canvas, frame, frame.tick - died_at);
        }
    }

    fn hotbar(&self, canvas: &mut Canvas<'_>, registry: &Registry, frame: &Frame<'_>) {
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        let center = (w / 2.0).floor();
        canvas.sprite(
            WIDGETS,
            center - 91.0,
            h - 22.0,
            0.0,
            0.0,
            182.0,
            22.0,
            [1.0; 4],
        );
        let selected = f32::from(frame.view.selected.min(8));
        canvas.sprite(
            WIDGETS,
            center - 92.0 + selected * 20.0,
            h - 23.0,
            0.0,
            22.0,
            24.0,
            22.0,
            [1.0; 4],
        );
        let base = if frame.view.window == 0 {
            36
        } else {
            frame.view.slots.len().saturating_sub(9)
        };
        for slot in 0..9 {
            let x = center - 90.0 + slot as f32 * 20.0 + 2.0;
            let y = h - 19.0;
            if let Some(item) = frame.view.slots.get(base + slot).and_then(Option::as_ref) {
                item_slot(canvas, registry, item, x, y);
            }
        }
    }

    /// `GuiIngame.updateTick`'s highlighted stack and the name it shows for
    /// 40 ticks above the hotbar, fading over the last ten.
    fn draw_highlight(&mut self, canvas: &mut Canvas<'_>, frame: &Frame<'_>, survival: bool) {
        let base = if frame.view.window == 0 {
            36
        } else {
            frame.view.slots.len().saturating_sub(9)
        };
        let held = frame
            .view
            .slots
            .get(base + usize::from(frame.view.selected))
            .and_then(Option::as_ref);
        let Some(item) = held else {
            self.highlight = None;
            return;
        };
        let mut name = crate::tooltip::display_name(item, canvas.res);
        if crate::tooltip::custom_name(item).is_some() {
            name = format!("§o{name}");
        }
        let damageable = crate::tooltip::max_damage(item) > 0;
        let key = (item.id, if damageable { 0 } else { item.damage }, name);
        let same = self
            .highlight
            .as_ref()
            .is_some_and(|(previous, _)| *previous == key);
        if !same {
            self.highlight = Some((key, frame.tick));
        }
        let Some(((_, _, name), since)) = &self.highlight else {
            return;
        };
        let left = 40.0 - (frame.tick - since);
        let alpha = ((left * 256.0 / 10.0) as i32).min(255);
        if alpha > 0 {
            let (w, h) = (canvas.layout.width, canvas.layout.height);
            let x = ((w as i32 - canvas.res.width(name)) / 2) as f32;
            let y = h - 59.0 + if survival { 0.0 } else { 14.0 };
            let name = name.clone();
            canvas.text(&name, x, y, 0x00FF_FFFF | (alpha as u32) << 24, true);
        }
    }

    /// `renderPlayerStats`: armour, hearts (with absorption, hurt blink,
    /// low-health shake and the regeneration wave) and food.
    fn stats(&mut self, canvas: &mut Canvas<'_>, frame: &Frame<'_>, tick: u64) {
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        let vitals = frame.vitals;
        let health = vitals.health.ceil() as i32;
        match self.health {
            Some(previous) if health < previous => {
                self.blink_until = frame.tick + 20.0;
                self.settled_at = frame.tick;
            }
            Some(previous) if health > previous => {
                self.blink_until = frame.tick + 10.0;
                self.settled_at = frame.tick;
            }
            Some(_) => {}
            None => self.last_health = health,
        }
        if frame.tick - self.settled_at > 20.0 {
            self.last_health = health;
            self.settled_at = frame.tick;
        }
        self.health = Some(health);
        let blink =
            self.blink_until > frame.tick && ((self.blink_until - frame.tick) as u64 / 3) % 2 == 1;
        let last = self.last_health;
        let left = (w / 2.0).floor() - 91.0;
        let right = (w / 2.0).floor() + 91.0;
        let bottom = h - 39.0;
        let max = vitals.max_health.max(1.0);
        let absorption = vitals.absorption.max(0.0);
        let rows = ((max + absorption) / 2.0 / 10.0).ceil().max(1.0) as i32;
        let row_height = (10 - (rows - 2)).max(3) as f32;
        let armor_y = bottom - (rows - 1) as f32 * row_height - 10.0;
        let armor = i32::from(vitals.armor);
        if armor > 0 {
            for i in 0..10 {
                let u = match (i * 2 + 1).cmp(&armor) {
                    std::cmp::Ordering::Less => 34.0,
                    std::cmp::Ordering::Equal => 25.0,
                    std::cmp::Ordering::Greater => 16.0,
                };
                canvas.sprite(
                    ICONS,
                    left + i as f32 * 8.0,
                    armor_y,
                    u,
                    9.0,
                    9.0,
                    9.0,
                    [1.0; 4],
                );
            }
        }
        let regen = (vitals.effects & REGENERATION != 0).then(|| tick % (max + 5.0).ceil() as u64);
        let mut remaining = absorption;
        let hearts = ((max + absorption) / 2.0).ceil() as i32;
        let v = if frame.hardcore { 45.0 } else { 0.0 };
        let kind = if vitals.effects & POISON != 0 {
            52.0
        } else if vitals.effects & WITHER != 0 {
            88.0
        } else {
            16.0
        };
        for heart in (0..hearts.min(100)).rev() {
            let row = ((heart + 1) as f32 / 10.0).ceil() - 1.0;
            let x = left + (heart % 10) as f32 * 8.0;
            let mut y = bottom - row * row_height;
            if health <= 4 {
                y += jitter(tick, heart as u64, 2) as f32;
            }
            if regen == Some(heart as u64) {
                y -= 2.0;
            }
            canvas.sprite(
                ICONS,
                x,
                y,
                if blink { 25.0 } else { 16.0 },
                v,
                9.0,
                9.0,
                [1.0; 4],
            );
            if blink {
                if heart * 2 + 1 < last {
                    canvas.sprite(ICONS, x, y, kind + 54.0, v, 9.0, 9.0, [1.0; 4]);
                }
                if heart * 2 + 1 == last {
                    canvas.sprite(ICONS, x, y, kind + 63.0, v, 9.0, 9.0, [1.0; 4]);
                }
            }
            if remaining > 0.0 {
                let half = remaining == absorption && absorption % 2.0 == 1.0;
                canvas.sprite(
                    ICONS,
                    x,
                    y,
                    kind + if half { 153.0 } else { 144.0 },
                    v,
                    9.0,
                    9.0,
                    [1.0; 4],
                );
                remaining -= 2.0;
            } else {
                if heart * 2 + 1 < health {
                    canvas.sprite(ICONS, x, y, kind + 36.0, v, 9.0, 9.0, [1.0; 4]);
                }
                if heart * 2 + 1 == health {
                    canvas.sprite(ICONS, x, y, kind + 45.0, v, 9.0, 9.0, [1.0; 4]);
                }
            }
        }
        let food = vitals.food;
        let hunger = vitals.effects & HUNGER != 0;
        for i in 0..10 {
            let mut y = bottom;
            if vitals.saturation <= 0.0 && tick.is_multiple_of(food.max(0) as u64 * 3 + 1) {
                y += jitter(tick, 100 + i as u64, 3) as f32 - 1.0;
            }
            let x = right - i as f32 * 8.0 - 9.0;
            let (u, background) = if hunger {
                (52.0, 16.0 + 13.0 * 9.0)
            } else {
                (16.0, 16.0)
            };
            canvas.sprite(ICONS, x, y, background, 27.0, 9.0, 9.0, [1.0; 4]);
            if i * 2 + 1 < food {
                canvas.sprite(ICONS, x, y, u + 36.0, 27.0, 9.0, 9.0, [1.0; 4]);
            }
            if i * 2 + 1 == food {
                canvas.sprite(ICONS, x, y, u + 45.0, 27.0, 9.0, 9.0, [1.0; 4]);
            }
        }
    }

    /// `renderExpBar` and the green outlined level.
    fn experience(canvas: &mut Canvas<'_>, frame: &Frame<'_>) {
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        let left = (w / 2.0).floor() - 91.0;
        let y = h - 29.0;
        canvas.sprite(ICONS, left, y, 0.0, 64.0, 182.0, 5.0, [1.0; 4]);
        let filled = (frame.experience.0.clamp(0.0, 1.0) * 183.0).floor();
        if filled > 0.0 {
            canvas.sprite(ICONS, left, y, 0.0, 69.0, filled.min(182.0), 5.0, [1.0; 4]);
        }
        let level = frame.experience.1;
        if level > 0 {
            let text = level.to_string();
            let x = ((w - canvas.res.width(&text) as f32) / 2.0).floor();
            let y = h - 35.0;
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                canvas.text(&text, x + dx, y + dy, 0xFF00_0000, false);
            }
            canvas.text(&text, x, y, 0xFF80_FF20, false);
        }
    }

    /// The action bar fades over its last 20 of 60 ticks.
    fn draw_action_bar(&mut self, canvas: &mut Canvas<'_>, tick: f64) {
        let Some((text, at)) = &self.action_bar else {
            return;
        };
        let left = 60.0 - (tick - at);
        if left <= 0.0 {
            self.action_bar = None;
            return;
        }
        let alpha = ((left * 255.0 / 20.0) as u32).min(255);
        if alpha > 8 {
            let (w, h) = (canvas.layout.width, canvas.layout.height);
            let x = (w / 2.0).floor() - (canvas.res.width(text) / 2) as f32;
            let text = text.clone();
            canvas.text(&text, x, h - 72.0, 0x00FF_FFFF | alpha << 24, false);
        }
    }

    /// `GuiGameOver.drawScreen`.
    fn death_screen(canvas: &mut Canvas<'_>, frame: &Frame<'_>, since: f64) {
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        canvas.gradient(0.0, 0.0, w, h, 0x6050_0000, 0xA080_3030);
        let title_key = if frame.hardcore {
            "deathScreen.title.hardcore"
        } else {
            "deathScreen.title"
        };
        let title = canvas.res.translate(title_key).to_owned();
        canvas.scale = 2.0;
        canvas.centered(&title, (w / 2.0 / 2.0).floor(), 30.0, 0xFFFF_FFFF, true);
        canvas.scale = 1.0;
        let score = format!(
            "{}: §e{}",
            canvas.res.translate("deathScreen.score"),
            frame.vitals.score
        );
        canvas.centered(&score, (w / 2.0).floor(), 100.0, 0xFFFF_FFFF, true);
        let enabled = since >= 20.0;
        for (button, [x, y, bw, bh]) in Self::death_buttons(canvas.layout) {
            let key = match button {
                DeathButton::Respawn => "deathScreen.respawn",
                DeathButton::TitleScreen => "deathScreen.titleScreen",
            };
            let label = canvas.res.translate(key).to_owned();
            let hovered = frame.mouse.0 >= x
                && frame.mouse.1 >= y
                && frame.mouse.0 < x + bw
                && frame.mouse.1 < y + bh;
            button_widget(canvas, &label, [x, y, bw, bh], enabled, hovered);
        }
    }
}

/// `GuiButton.drawButton` from widgets.png.
pub fn button_widget(
    canvas: &mut Canvas<'_>,
    label: &str,
    [x, y, w, h]: [f32; 4],
    enabled: bool,
    hovered: bool,
) {
    let state = if !enabled {
        0.0
    } else if hovered {
        2.0
    } else {
        1.0
    };
    let half = (w / 2.0).floor();
    canvas.sprite(WIDGETS, x, y, 0.0, 46.0 + state * 20.0, half, h, [1.0; 4]);
    canvas.sprite(
        WIDGETS,
        x + half,
        y,
        200.0 - half,
        46.0 + state * 20.0,
        half,
        h,
        [1.0; 4],
    );
    let color = if !enabled {
        0xFFA0_A0A0
    } else if hovered {
        0xFFFF_FFA0
    } else {
        0xFFE0_E0E0
    };
    canvas.centered(label, x + half, y + ((h - 8.0) / 2.0).floor(), color, true);
}

/// `RenderItem.renderItemIntoGUI` for a block model: centred in the slot,
/// flipped in depth, scaled to 10 GUI pixels a block (0.5 * 40 * 0.5),
/// turned 210 degrees about X and -135 about Y, then drawn back to front
/// with back faces culled. False when the block has no model.
fn block_icon(
    canvas: &mut Canvas<'_>,
    registry: &Registry,
    item: &horizon_protocol_47::nbt::Item,
    x: f32,
    y: f32,
) -> bool {
    use glam::{Mat4, Vec3};
    if !(1..198).contains(&item.id) || crate::hand::flat_block(item.id) {
        return false;
    }
    let matrix = Mat4::from_translation(Vec3::new(x + 8.0, y + 8.0, 0.0))
        * Mat4::from_scale(Vec3::new(10.0, 10.0, -10.0))
        * Mat4::from_rotation_x(210f32.to_radians())
        * Mat4::from_rotation_y((-135f32).to_radians())
        // The item models' own `gui` display: fences turn 90, stairs 180.
        * Mat4::from_rotation_y(match item.id {
            85 | 113 | 188..=192 => 90f32.to_radians(),
            53 | 67 | 108 | 109 | 114 | 128 | 134..=136 | 156 | 163 | 164 | 180 => {
                std::f32::consts::PI
            }
            _ => 0.0,
        });
    let mut vertices = Vec::new();
    let state = horizon_world_189::BlockState(((item.id as u16) << 4) | (item.damage as u16 & 15));
    if !registry.block_model(&mut vertices, state, matrix, 1.0) || vertices.is_empty() {
        return false;
    }
    // The flip makes the transform mirror-imaged: transformed cross
    // products point inward, so a face is shown when theirs points away.
    let mirrored = matrix.determinant() < 0.0;
    let mut faces: Vec<(f32, &[crate::terrain::Vertex; 6])> = vertices
        .as_chunks::<6>()
        .0
        .iter()
        .filter(|face| {
            let [a, b, c] = [face[0], face[1], face[2]].map(|v| Vec3::from(v.position));
            let normal = (b - a).cross(c - a);
            (if mirrored { -normal.z } else { normal.z }) > 0.0
        })
        .map(|face| {
            let depth = face.iter().map(|v| v.position[2]).sum::<f32>() / 6.0;
            (depth, face)
        })
        .collect();
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, face) in faces {
        let corner = |i: usize| [face[i].position[0], face[i].position[1]];
        let [r, g, b] = face[0].color;
        canvas.entity_quad(
            [corner(0), corner(1), corner(2), corner(5)],
            [face[0].uv, face[1].uv, face[2].uv, face[5].uv],
            face[0].layer,
            [r, g, b, 1.0],
        );
    }
    true
}

/// An item icon with `renderItemOverlayIntoGUI`'s stack count.
pub fn item_slot(
    canvas: &mut Canvas<'_>,
    registry: &Registry,
    item: &horizon_protocol_47::nbt::Item,
    x: f32,
    y: f32,
) {
    // Two-layer sprites: the potion liquid and leather dye are tinted.
    let tint = |color: u32| crate::ui::argb(color | 0xFF00_0000);
    let layered = match item.id {
        373 => {
            let bottle = if item.damage & 16384 != 0 {
                "potion_bottle_splash"
            } else {
                "potion_bottle_drinkable"
            };
            Some((
                "potion_overlay",
                tint(crate::tooltip::potion_color(item)),
                bottle,
            ))
        }
        298..=301 => {
            let index = (item.id - 298) as usize;
            Some((
                [
                    "leather_helmet",
                    "leather_chestplate",
                    "leather_leggings",
                    "leather_boots",
                ][index],
                tint(crate::tooltip::leather_color(item)),
                [
                    "leather_helmet_overlay",
                    "leather_chestplate_overlay",
                    "leather_leggings_overlay",
                    "leather_boots_overlay",
                ][index],
            ))
        }
        _ => None,
    };
    let drawn = layered.is_some_and(|(under, under_tint, over)| {
        let (Some(under), Some(over)) = (registry.sprite(under), registry.sprite(over)) else {
            return false;
        };
        canvas.item_icon_tinted(under, x, y, under_tint);
        canvas.item_icon(over, x, y);
        // The glint follows the whole silhouette: the bottle, or the base
        // leather piece.
        if let Some(glint) = registry
            .sprite("enchanted_item_glint")
            .filter(|_| crate::models::has_glint(item))
        {
            let mask = if item.id == 373 { over } else { under };
            canvas.item_glint(mask, glint, x, y, crate::models::millis());
        }
        true
    });
    let drawn = drawn || block_icon(canvas, registry, item, x, y);
    if !drawn {
        match registry.item_layer(item) {
            Some(layer) => {
                canvas.item_icon(layer, x, y);
                if let Some(glint) = registry
                    .sprite("enchanted_item_glint")
                    .filter(|_| crate::models::has_glint(item))
                {
                    canvas.item_glint(layer, glint, x, y, crate::models::millis());
                }
            }
            None => {
                let id = item.id.to_string();
                canvas.text(&id, x + 1.0, y + 4.0, 0xFFFF_FFFF, true);
            }
        }
    }
    if item.count != 1 {
        let count = if item.count < 1 {
            format!("§c{}", item.count)
        } else {
            item.count.to_string()
        };
        let width = canvas.res.width(&count) as f32;
        canvas.text(&count, x + 17.0 - width, y + 9.0, 0xFFFF_FFFF, true);
    }
    // The durability bar: green to red, black behind.
    let max = crate::tooltip::max_damage(item);
    if max > 0 && item.damage > 0 {
        let health = f64::from(item.damage) / f64::from(max);
        let length = (13.0 - health * 13.0).round().max(0.0) as f32;
        let green = (255.0 - health * 255.0).round().clamp(0.0, 255.0) as u32;
        canvas.rect(x + 2.0, y + 13.0, x + 15.0, y + 15.0, 0xFF00_0000);
        canvas.rect(
            x + 2.0,
            y + 13.0,
            x + 14.0,
            y + 14.0,
            0xFF00_0000 | ((255 - green) / 4) << 16 | 64 << 8,
        );
        canvas.rect(
            x + 2.0,
            y + 13.0,
            x + 2.0 + length,
            y + 14.0,
            0xFF00_0000 | (255 - green) << 16 | green << 8,
        );
    }
}

/// `GuiContainer.guiLeft`/`guiTop` for the open window.
fn origin(view: &View, layout: Layout) -> (f32, f32, crate::inventory::Screen) {
    let screen = view.screen();
    let left = ((layout.width as i32 - screen.size.0 as i32) / 2) as f32;
    let top = ((layout.height as i32 - screen.size.1 as i32) / 2) as f32;
    (left, top, screen)
}

/// The slot under the mouse in the open window.
pub fn hovered_slot(view: &View, layout: Layout, mouse: (f32, f32)) -> Option<i16> {
    if !view.open {
        return None;
    }
    let (left, top, _) = origin(view, layout);
    view.slot_at(mouse.0 - left, mouse.1 - top)
}

/// Whether the mouse is over the window's background, where clicks do not
/// drop the carried stack.
pub fn inside_window(view: &View, layout: Layout, mouse: (f32, f32)) -> bool {
    let (left, top, screen) = origin(view, layout);
    mouse.0 >= left
        && mouse.1 >= top
        && mouse.0 < left + screen.size.0
        && mouse.1 < top + screen.size.1
}

/// `GuiContainer.drawScreen`: background, sheet, slots with the hover
/// highlight, labels, the carried stack, then the hovered item's tooltip.
pub fn inventory(canvas: &mut Canvas<'_>, registry: &Registry, view: &View, mouse: (f32, f32)) {
    let (w, h) = (canvas.layout.width, canvas.layout.height);
    canvas.gradient(0.0, 0.0, w, h, 0xC010_1010, 0xD010_1010);
    let (left, top, screen) = origin(view, canvas.layout);
    for [x, y, u, v, pw, ph] in &screen.parts {
        canvas.sprite(screen.sheet, left + x, top + y, *u, *v, *pw, *ph, [1.0; 4]);
    }
    if view.kind == crate::inventory::Kind::Furnace {
        // `GuiFurnace`: the flame (burn left of 13) and the arrow (cook of 24).
        let [burn, item_burn, cook, total] = view.properties.map(i32::from);
        if burn > 0 {
            let full = if item_burn == 0 { 200 } else { item_burn };
            let k = (burn * 13 / full) as f32;
            canvas.sprite(
                screen.sheet,
                left + 56.0,
                top + 36.0 + 12.0 - k,
                176.0,
                12.0 - k,
                14.0,
                k + 1.0,
                [1.0; 4],
            );
        }
        let l = if total != 0 && cook != 0 {
            cook * 24 / total
        } else {
            0
        } as f32;
        canvas.sprite(
            screen.sheet,
            left + 79.0,
            top + 34.0,
            176.0,
            14.0,
            l + 1.0,
            16.0,
            [1.0; 4],
        );
    }
    let hovered = view.slot_at(mouse.0 - left, mouse.1 - top);
    for &(slot, x, y) in &screen.slots {
        let (x, y) = (left + x, top + y);
        if let Some(item) = view.slots.get(slot as usize).and_then(Option::as_ref) {
            item_slot(canvas, registry, item, x, y);
        }
        if hovered == Some(slot) {
            canvas.rect(x, y, x + 16.0, y + 16.0, 0x80FF_FFFF);
        }
    }
    let res = canvas.res;
    let title = if view.window == 0 {
        res.translate("container.crafting").to_owned()
    } else {
        view.title.clone()
    };
    let title_x = screen
        .title
        .0
        .unwrap_or_else(|| ((screen.size.0 as i32) / 2 - res.width(&title) / 2) as f32);
    canvas.text(
        &title,
        left + title_x,
        top + screen.title.1,
        0xFF40_4040,
        false,
    );
    if let Some((x, y)) = screen.inventory_label {
        let label = res.translate("container.inventory").to_owned();
        canvas.text(&label, left + x, top + y, 0xFF40_4040, false);
    }
    match &view.cursor {
        Some(item) => item_slot(canvas, registry, item, mouse.0 - 8.0, mouse.1 - 8.0),
        None => {
            if let Some(item) = hovered
                .and_then(|slot| view.slots.get(slot as usize))
                .and_then(Option::as_ref)
            {
                let lines = crate::tooltip::lines(item, res);
                canvas.tooltip(&lines, mouse);
            }
        }
    }
}

/// `GuiPlayerTabOverlay.renderPlayerlist`: header, columns of at most 20
/// players with faces, list-slot scores and ping bars, then the footer.
fn tab_list(canvas: &mut Canvas<'_>, frame: &Frame<'_>) {
    let res = canvas.res;
    let board = frame.scoreboard;
    let objective = board.slot(0);
    let mut entries = frame.tab.sorted(board);
    let mut name_width = 0;
    let mut score_width = 0;
    for (_, entry) in &entries {
        name_width = name_width.max(res.width(&TabList::name(entry, board)));
        if let Some((name, info)) = objective
            && !info.hearts
        {
            let points = board.points(name, &entry.name).unwrap_or(0);
            score_width = score_width.max(res.width(&format!(" {points}")));
        }
    }
    entries.truncate(80);
    let count = entries.len();
    let mut columns = 1;
    let mut rows = count;
    while rows > 20 {
        columns += 1;
        rows = count.div_ceil(columns);
    }
    let w = canvas.layout.width as i32;
    let score_space = match objective {
        Some((_, info)) if info.hearts => 90,
        Some(_) => score_width,
        None => 0,
    };
    let head = if frame.heads { 9 } else { 0 };
    let column =
        (columns as i32 * (head + name_width + score_space + 13)).min(w - 50) / columns as i32;
    let left = w / 2 - (column * columns as i32 + (columns as i32 - 1) * 5) / 2;
    let mut y = 10;
    let mut span = column * columns as i32 + (columns as i32 - 1) * 5;
    let wrap = |text: &Option<String>| {
        text.as_ref().map(|text| {
            res.wrap(text, &[], w - 50)
                .into_iter()
                .map(|(line, _)| line)
                .collect::<Vec<_>>()
        })
    };
    let header = wrap(&frame.tab.header);
    let footer = wrap(&frame.tab.footer);
    for line in header.iter().chain(footer.iter()).flatten() {
        span = span.max(res.width(line));
    }
    let band = |canvas: &mut Canvas<'_>, top: i32, lines: i32| {
        canvas.rect(
            (w / 2 - span / 2 - 1) as f32,
            (top - 1) as f32,
            (w / 2 + span / 2 + 1) as f32,
            (top + lines * 9) as f32,
            0x8000_0000,
        );
    };
    if let Some(header) = &header {
        band(canvas, y, header.len() as i32);
        for line in header {
            canvas.text(
                line,
                (w / 2 - res.width(line) / 2) as f32,
                y as f32,
                0xFFFF_FFFF,
                true,
            );
            y += 9;
        }
        y += 1;
    }
    band(canvas, y, rows as i32);
    for index in 0..count {
        let (col, row) = ((index / rows) as i32, (index % rows) as i32);
        let mut x = left + col * column + col * 5;
        let top = y + row * 9;
        canvas.rect(
            x as f32,
            top as f32,
            (x + column) as f32,
            (top + 8) as f32,
            0x20FF_FFFF,
        );
        let (uuid, entry) = entries[index];
        if frame.heads {
            let layer = (frame.skin)(uuid).unwrap_or_else(|| {
                if horizon_world_189::entities::slim_skin(uuid) {
                    crate::models::Texture::Alex.into()
                } else {
                    crate::models::Texture::Steve.into()
                }
            });
            canvas.skin_face(layer, x as f32, top as f32);
            x += 9;
        }
        let name = TabList::name(entry, board);
        if entry.game_mode == 3 {
            canvas.text(
                &format!("§o{name}"),
                x as f32,
                top as f32,
                0x90FF_FFFF,
                true,
            );
        } else {
            canvas.text(&name, x as f32, top as f32, 0xFFFF_FFFF, true);
            if let Some((objective, _)) = objective {
                let start = x + name_width + 1;
                if score_space > 5 {
                    let points = board.points(objective, &entry.name).unwrap_or(0);
                    let value = format!("§e{points}");
                    let value_x = start + score_space - res.width(&value);
                    canvas.text(&value, value_x as f32, top as f32, 0xFFFF_FFFF, true);
                }
            }
        }
        let bars = match entry.ping {
            ping if ping < 0 => 5.0,
            0..150 => 0.0,
            150..300 => 1.0,
            300..600 => 2.0,
            600..1000 => 3.0,
            _ => 4.0,
        };
        let ping_x = x - head + column - 11;
        canvas.sprite(
            ICONS,
            ping_x as f32,
            top as f32,
            0.0,
            176.0 + bars * 8.0,
            10.0,
            8.0,
            [1.0; 4],
        );
    }
    if let Some(footer) = &footer {
        let mut y = y + rows as i32 * 9 + 1;
        band(canvas, y, footer.len() as i32);
        for line in footer {
            canvas.text(
                line,
                (w / 2 - res.width(line) / 2) as f32,
                y as f32,
                0xFFFF_FFFF,
                true,
            );
            y += 9;
        }
    }
}

/// Optional local metrics; labels come from the client's translation table.
pub fn pvp_overlay(
    canvas: &mut Canvas<'_>,
    registry: &Registry,
    metrics: crate::pvp_metrics::Sample,
) {
    let fps = metrics
        .fps
        .map_or_else(|| "--".to_owned(), |fps| fps.to_string());
    let fps = registry
        .gui
        .translate("horizon.pvp.fps")
        .replacen("%s", &fps, 1);
    let cps = registry
        .gui
        .translate("horizon.pvp.cps")
        .replacen("%s", &metrics.left.to_string(), 1)
        .replacen("%s", &metrics.right.to_string(), 1);
    canvas.text(&fps, 3.0, 3.0, 0xFFFF_FFFF, true);
    canvas.text(&cps, 3.0, 13.0, 0xFFFF_FFFF, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawn_waits_for_the_death_screen_buttons() {
        let mut hud = Hud::default();
        assert!(!hud.respawn_ready(100.0));
        hud.died_at = Some(100.0);
        assert!(!hud.respawn_ready(100.0));
        assert!(!hud.respawn_ready(119.9));
        assert!(hud.respawn_ready(120.0));
    }
}
