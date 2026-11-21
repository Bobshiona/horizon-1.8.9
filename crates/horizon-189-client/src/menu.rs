//! The mod menu (Right Shift): a liquid-glass sheet of mod cards with
//! switches, plus a HUD layout editor where cards are dragged into place.
use crate::{
    glass::Panel,
    settings::{HudKind, Settings},
    ui::{Canvas, Layout},
};

/// One card in the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Hud(HudKind),
    Zoom,
    SmoothZoom,
    ToggleSprint,
    ToggleSneak,
    Fullbright,
    FpsLimit,
}

/// Frame caps the FPS card cycles through; 0 follows the display.
pub const FPS_LIMITS: [u32; 7] = [0, 60, 120, 144, 240, 360, u32::MAX];

fn entries() -> Vec<Entry> {
    let mut list: Vec<Entry> = HudKind::ALL.iter().map(|&k| Entry::Hud(k)).collect();
    list.extend([
        Entry::Zoom,
        Entry::SmoothZoom,
        Entry::ToggleSprint,
        Entry::ToggleSneak,
        Entry::Fullbright,
        Entry::FpsLimit,
    ]);
    list
}

impl Entry {
    pub fn title(self) -> &'static str {
        match self {
            Entry::Hud(kind) => kind.title(),
            Entry::Zoom => "Zoom (C)",
            Entry::SmoothZoom => "Smooth Zoom",
            Entry::ToggleSprint => "Toggle Sprint",
            Entry::ToggleSneak => "Toggle Sneak",
            Entry::Fullbright => "Fullbright",
            Entry::FpsLimit => "Frame Limit",
        }
    }

    /// A switch state, or `None` for the frame-limit value card.
    pub fn enabled(self, settings: &Settings) -> Option<bool> {
        let t = &settings.toggles;
        Some(match self {
            Entry::Hud(kind) => settings.hud(kind).enabled,
            Entry::Zoom => t.zoom,
            Entry::SmoothZoom => t.smooth_zoom,
            Entry::ToggleSprint => t.toggle_sprint,
            Entry::ToggleSneak => t.toggle_sneak,
            Entry::Fullbright => t.fullbright,
            Entry::FpsLimit => return None,
        })
    }

    pub fn value(self, settings: &Settings) -> String {
        match settings.toggles.fps_limit {
            0 => "Display".to_owned(),
            u32::MAX => "Unlimited".to_owned(),
            fps => format!("{fps} FPS"),
        }
    }

    /// Flip a switch or step the frame limit.
    pub fn activate(self, settings: &mut Settings) {
        let t = &mut settings.toggles;
        match self {
            Entry::Hud(kind) => {
                let hud = settings.hud_mut(kind);
                hud.enabled = !hud.enabled;
            }
            Entry::Zoom => t.zoom = !t.zoom,
            Entry::SmoothZoom => t.smooth_zoom = !t.smooth_zoom,
            Entry::ToggleSprint => t.toggle_sprint = !t.toggle_sprint,
            Entry::ToggleSneak => t.toggle_sneak = !t.toggle_sneak,
            Entry::Fullbright => t.fullbright = !t.fullbright,
            Entry::FpsLimit => {
                let at = FPS_LIMITS
                    .iter()
                    .position(|&v| v == t.fps_limit)
                    .unwrap_or(0);
                t.fps_limit = FPS_LIMITS[(at + 1) % FPS_LIMITS.len()];
            }
        }
    }
}

/// Where everything sits for a screen size, in GUI pixels.
#[derive(Debug, PartialEq)]
pub struct Geometry {
    pub sheet: [f32; 4],
    pub cards: Vec<(Entry, [f32; 4])>,
    pub edit: [f32; 4],
    pub done: [f32; 4],
}

const CARD_W: f32 = 122.0;
const CARD_H: f32 = 30.0;
const GAP: f32 = 6.0;
const MARGIN: f32 = 12.0;
const HEADER: f32 = 34.0;
const FOOTER: f32 = 34.0;

pub fn geometry(layout: Layout) -> Geometry {
    let list = entries();
    let room = (layout.width - 2.0 * MARGIN - 16.0).max(CARD_W);
    let columns = ((room + GAP) / (CARD_W + GAP)).floor().clamp(1.0, 4.0) as usize;
    let rows = list.len().div_ceil(columns);
    let width = columns as f32 * CARD_W + (columns - 1) as f32 * GAP + 2.0 * MARGIN;
    let height = HEADER + rows as f32 * CARD_H + (rows - 1) as f32 * GAP + FOOTER + MARGIN;
    let x0 = ((layout.width - width) / 2.0).floor().max(0.0);
    let y0 = ((layout.height - height) / 2.0).floor().max(0.0);
    let cards = list
        .into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let (c, r) = ((i % columns) as f32, (i / columns) as f32);
            let x = x0 + MARGIN + c * (CARD_W + GAP);
            let y = y0 + HEADER + r * (CARD_H + GAP);
            (entry, [x, y, x + CARD_W, y + CARD_H])
        })
        .collect();
    let bottom = y0 + height - MARGIN - 20.0;
    let button = 96.0;
    Geometry {
        sheet: [x0, y0, x0 + width, y0 + height],
        cards,
        edit: [x0 + MARGIN, bottom, x0 + MARGIN + button, bottom + 20.0],
        done: [
            x0 + width - MARGIN - button,
            bottom,
            x0 + width - MARGIN,
            bottom + 20.0,
        ],
    }
}

fn inside(rect: [f32; 4], (x, y): (f32, f32)) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// What a menu click asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Changed,
    EditHud,
    Close,
    Nothing,
}

#[derive(Default)]
pub struct Menu {
    pub open: bool,
    pub editing: bool,
    /// The card being dragged and the grab offset from its corner.
    drag: Option<(HudKind, (f32, f32))>,
}

impl Menu {
    pub fn active(&self) -> bool {
        self.open || self.editing
    }

    pub fn toggle(&mut self) {
        if self.editing {
            self.editing = false;
            self.open = true;
        } else {
            self.open = !self.open;
        }
        self.drag = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.editing = false;
        self.drag = None;
    }

    pub fn click(&mut self, layout: Layout, mouse: (f32, f32), settings: &mut Settings) -> Click {
        if !self.open {
            return Click::Nothing;
        }
        let g = geometry(layout);
        if let Some((entry, _)) = g.cards.iter().find(|(_, r)| inside(*r, mouse)) {
            entry.activate(settings);
            return Click::Changed;
        }
        if inside(g.edit, mouse) {
            self.open = false;
            self.editing = true;
            return Click::EditHud;
        }
        if inside(g.done, mouse) || !inside(g.sheet, mouse) {
            self.close();
            return Click::Close;
        }
        Click::Nothing
    }

    /// Editor: start dragging the card under the mouse.
    pub fn press(&mut self, mouse: (f32, f32), cards: &[(HudKind, [f32; 4])]) {
        if !self.editing {
            return;
        }
        self.drag = cards
            .iter()
            .rev()
            .find(|(_, r)| inside(*r, mouse))
            .map(|&(kind, r)| (kind, (mouse.0 - r[0], mouse.1 - r[1])));
    }

    /// Editor: move the dragged card; true when settings changed.
    pub fn moved(&mut self, layout: Layout, mouse: (f32, f32), settings: &mut Settings) -> bool {
        let Some((kind, (dx, dy))) = self.drag.filter(|_| self.editing) else {
            return false;
        };
        let hud = settings.hud_mut(kind);
        hud.x = ((mouse.0 - dx) / layout.width.max(1.0)).clamp(0.0, 1.0);
        hud.y = ((mouse.1 - dy) / layout.height.max(1.0)).clamp(0.0, 1.0);
        true
    }

    /// Editor: drop the card; true when a drag ended (save now).
    pub fn release(&mut self) -> bool {
        self.drag.take().is_some()
    }

    pub fn draw(&self, canvas: &mut Canvas<'_>, settings: &Settings, mouse: (f32, f32)) {
        canvas.overlay();
        if self.editing {
            let w = canvas.layout.width;
            let label = "Drag cards to move them  ·  Right Shift to finish";
            let tw = canvas.res.width(label) as f32;
            let x = ((w - tw) / 2.0).floor();
            let mut pill = Panel::new(x - 10.0, 6.0, x + tw + 10.0, 24.0);
            pill.radius = 9.0;
            canvas.glass(pill);
            canvas.text(label, x, 11.0, 0xFFF5_F7FA, false);
            return;
        }
        if !self.open {
            return;
        }
        let g = geometry(canvas.layout);
        let [x0, y0, x1, _] = g.sheet;
        let mut sheet = Panel::new(g.sheet[0], g.sheet[1], g.sheet[2], g.sheet[3]);
        sheet.radius = 12.0;
        sheet.shadow = 0.4;
        sheet.tint = [0.05, 0.06, 0.08, 0.46];
        canvas.glass(sheet);
        canvas.text("Horizon", x0 + MARGIN, y0 + 10.0, 0xFFFF_FFFF, false);
        canvas.text(
            "Mods",
            x0 + MARGIN + canvas.res.width("Horizon ") as f32,
            y0 + 10.0,
            0xFFC4_CAD4,
            false,
        );
        let hint = "Right Shift";
        canvas.text(
            hint,
            x1 - MARGIN - canvas.res.width(hint) as f32,
            y0 + 10.0,
            0xFF9A_A3B2,
            false,
        );
        for (entry, rect) in &g.cards {
            let hover = inside(*rect, mouse);
            let mut card = Panel::new(rect[0], rect[1], rect[2], rect[3]);
            card.radius = 7.0;
            card.shadow = 0.12;
            card.tint = if hover {
                [0.26, 0.28, 0.34, 0.50]
            } else {
                [0.12, 0.13, 0.16, 0.42]
            };
            canvas.glass(card);
            canvas.text(
                entry.title(),
                rect[0] + 8.0,
                rect[1] + 11.0,
                0xFFF5_F7FA,
                false,
            );
            match entry.enabled(settings) {
                Some(on) => switch(canvas, rect[2] - 26.0, rect[1] + 9.0, on),
                None => {
                    let value = entry.value(settings);
                    let w = canvas.res.width(&value) as f32;
                    canvas.text(
                        &value,
                        rect[2] - 8.0 - w,
                        rect[1] + 11.0,
                        0xFF6E_B6FF,
                        false,
                    );
                }
            }
        }
        button(canvas, g.edit, "Edit HUD", inside(g.edit, mouse), false);
        button(canvas, g.done, "Done", inside(g.done, mouse), true);
    }
}

/// An iOS-style switch: a pill track and a white knob.
fn switch(canvas: &mut Canvas<'_>, x: f32, y: f32, on: bool) {
    let mut track = Panel::new(x, y, x + 18.0, y + 11.0);
    track.radius = 5.5;
    track.shadow = 0.0;
    track.highlight = 0.3;
    track.tint = if on {
        [0.20, 0.78, 0.35, 0.92]
    } else {
        [0.42, 0.44, 0.50, 0.70]
    };
    canvas.glass(track);
    let kx = if on { x + 8.0 } else { x + 1.0 };
    let mut knob = Panel::new(kx, y + 1.0, kx + 9.0, y + 10.0);
    knob.radius = 4.5;
    knob.shadow = 0.2;
    knob.highlight = 0.2;
    knob.tint = [1.0, 1.0, 1.0, 0.96];
    canvas.glass(knob);
}

fn button(canvas: &mut Canvas<'_>, rect: [f32; 4], label: &str, hover: bool, primary: bool) {
    let mut panel = Panel::new(rect[0], rect[1], rect[2], rect[3]);
    panel.radius = 10.0;
    panel.shadow = 0.15;
    panel.tint = match (primary, hover) {
        (true, false) => [0.04, 0.47, 1.0, 0.85],
        (true, true) => [0.20, 0.58, 1.0, 0.90],
        (false, false) => [0.20, 0.22, 0.27, 0.45],
        (false, true) => [0.32, 0.35, 0.42, 0.50],
    };
    canvas.glass(panel);
    let w = canvas.res.width(label) as f32;
    canvas.text(
        label,
        (rect[0] + (rect[2] - rect[0] - w) / 2.0).floor(),
        rect[1] + 6.0,
        0xFFFF_FFFF,
        false,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_fits_every_screen_and_cards_do_not_overlap() {
        for (size, scale) in [
            ((640, 480), 2),
            ((1440, 900), 2),
            ((2880, 1800), 4),
            ((1920, 1080), 3),
        ] {
            let layout = Layout::new(size, 1.0, scale);
            let g = geometry(layout);
            assert!(
                g.sheet[0] >= 0.0 && g.sheet[2] <= layout.width + 0.5,
                "{size:?}"
            );
            for (i, (_, a)) in g.cards.iter().enumerate() {
                assert!(a[2] <= g.sheet[2] && a[3] <= g.edit[1]);
                for (_, b) in &g.cards[i + 1..] {
                    assert!(a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1]);
                }
            }
        }
    }

    #[test]
    fn clicks_flip_switches_cycle_the_limit_and_drags_move_cards() {
        let layout = Layout::new((1440, 900), 1.0, 2);
        let g = geometry(layout);
        let mut settings = Settings::default();
        let mut menu = Menu::default();
        menu.toggle();
        let centre = |r: [f32; 4]| ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
        let (_, sprint) = g
            .cards
            .iter()
            .find(|(e, _)| *e == Entry::ToggleSprint)
            .unwrap();
        assert_eq!(
            menu.click(layout, centre(*sprint), &mut settings),
            Click::Changed
        );
        assert!(settings.toggles.toggle_sprint);
        let (_, fps) = g.cards.iter().find(|(e, _)| *e == Entry::FpsLimit).unwrap();
        menu.click(layout, centre(*fps), &mut settings);
        assert_eq!(settings.toggles.fps_limit, 60);
        assert_eq!(
            menu.click(layout, centre(g.edit), &mut settings),
            Click::EditHud
        );
        assert!(menu.editing && !menu.open);
        let cards = [(HudKind::Ping, [10.0, 10.0, 50.0, 26.0])];
        menu.press((20.0, 15.0), &cards);
        assert!(menu.moved(layout, (120.0, 65.0), &mut settings));
        let ping = settings.hud(HudKind::Ping);
        assert_eq!(
            (ping.x, ping.y),
            (110.0 / layout.width, 60.0 / layout.height)
        );
        assert!(menu.release());
        menu.toggle();
        assert!(menu.open && !menu.editing);
        assert_eq!(menu.click(layout, (1.0, 1.0), &mut settings), Click::Close);
        assert!(!menu.active());
    }
}
