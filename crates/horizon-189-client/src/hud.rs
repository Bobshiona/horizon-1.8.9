//! `GuiIngame` on the interface layer: crosshair, hotbar, health, armour,
//! food, experience and the action bar, plus `GuiGameOver`.
use crate::{
    gameplay::Vitals,
    inventory::View,
    terrain::Registry,
    ui::{Canvas, ICONS, Layout, WIDGETS},
};

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

    fn death_buttons(layout: Layout) -> [(DeathButton, [f32; 4]); 2] {
        let x = (layout.width / 2.0).floor() - 100.0;
        let y = (layout.height / 4.0).floor();
        [
            (DeathButton::Respawn, [x, y + 72.0, 200.0, 20.0]),
            (DeathButton::TitleScreen, [x, y + 96.0, 200.0, 20.0]),
        ]
    }

    /// The death-screen button under the mouse, once buttons are enabled.
    pub fn death_button_at(
        &self,
        layout: Layout,
        mouse: (f32, f32),
        tick: f64,
    ) -> Option<DeathButton> {
        let since = tick - self.died_at?;
        if since < 20.0 {
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
        }
        if survival {
            self.stats(canvas, frame, tick);
            Self::experience(canvas, frame);
        }
        self.draw_action_bar(canvas, frame.tick);
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

/// An item icon with `renderItemOverlayIntoGUI`'s stack count.
pub fn item_slot(
    canvas: &mut Canvas<'_>,
    registry: &Registry,
    item: &horizon_protocol_47::nbt::Item,
    x: f32,
    y: f32,
) {
    match registry.item_layer(item) {
        Some(layer) => canvas.item_icon(layer, x, y),
        None => {
            let id = item.id.to_string();
            canvas.text(&id, x + 1.0, y + 4.0, 0xFFFF_FFFF, true);
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
}

/// The open window: `drawDefaultBackground`, slots, hover highlight and the
/// carried stack. Slot positions are `View::layout` offsets from the centre.
pub fn inventory(canvas: &mut Canvas<'_>, registry: &Registry, view: &View, mouse: (f32, f32)) {
    let (w, h) = (canvas.layout.width, canvas.layout.height);
    canvas.gradient(0.0, 0.0, w, h, 0xC010_1010, 0xD010_1010);
    let (cx, cy) = ((w / 2.0).floor(), (h / 2.0).floor());
    for (slot, x, y) in view.layout() {
        let (x, y) = (cx + x, cy + y);
        canvas.rect(x - 1.0, y - 1.0, x + 17.0, y + 17.0, 0xFF37_3737);
        canvas.rect(x, y, x + 17.0, y + 17.0, 0xFFFF_FFFF);
        canvas.rect(x, y, x + 16.0, y + 16.0, 0xFF8B_8B8B);
        if let Some(item) = view.slots.get(slot as usize).and_then(Option::as_ref) {
            item_slot(canvas, registry, item, x, y);
        }
        if mouse.0 >= x - 1.0 && mouse.1 >= y - 1.0 && mouse.0 < x + 17.0 && mouse.1 < y + 17.0 {
            canvas.rect(x, y, x + 16.0, y + 16.0, 0x80FF_FFFF);
        }
    }
    if let Some(item) = &view.cursor {
        item_slot(canvas, registry, item, mouse.0 - 8.0, mouse.1 - 8.0);
    }
}
