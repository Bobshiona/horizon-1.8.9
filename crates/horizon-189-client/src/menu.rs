//! The mod menu (Right Shift): a liquid-glass sheet of mod cards with
//! switches, plus a HUD layout editor where cards are dragged into place.
use crate::{
    glass::Panel,
    settings::{CROSSHAIRS, HudKind, Settings},
    sysfont::Weight,
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
    ScoreboardNumbers,
    Crosshair,
    BlockOutline,
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
        Entry::ScoreboardNumbers,
        Entry::Crosshair,
        Entry::BlockOutline,
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
            Entry::ScoreboardNumbers => "Scoreboard Numbers",
            Entry::Crosshair => "Crosshair",
            Entry::BlockOutline => "Block Outline",
            Entry::FpsLimit => "Frame Limit",
        }
    }

    /// A switch state, or `None` for value cards (crosshair, outline, frame
    /// limit).
    pub fn enabled(self, settings: &Settings) -> Option<bool> {
        let t = &settings.toggles;
        Some(match self {
            Entry::Hud(kind) => settings.hud(kind).enabled,
            Entry::Zoom => t.zoom,
            Entry::SmoothZoom => t.smooth_zoom,
            Entry::ToggleSprint => t.toggle_sprint,
            Entry::ToggleSneak => t.toggle_sneak,
            Entry::Fullbright => t.fullbright,
            Entry::ScoreboardNumbers => t.scoreboard_numbers,
            Entry::Crosshair | Entry::BlockOutline | Entry::FpsLimit => return None,
        })
    }

    pub fn value(self, settings: &Settings) -> String {
        if self == Entry::BlockOutline {
            let styles = crate::outline::STYLES;
            return styles[usize::from(settings.toggles.outline) % styles.len()].to_owned();
        }
        if self == Entry::Crosshair {
            return CROSSHAIRS[usize::from(settings.toggles.crosshair) % CROSSHAIRS.len()]
                .to_owned();
        }
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
            Entry::ScoreboardNumbers => t.scoreboard_numbers = !t.scoreboard_numbers,
            Entry::Crosshair => t.crosshair = (t.crosshair + 1) % CROSSHAIRS.len() as u8,
            Entry::BlockOutline => {
                t.outline = (t.outline + 1) % crate::outline::STYLES.len() as u8;
            }
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
    /// Between Edit HUD and Done, when the sheet is wide enough.
    pub leave: Option<[f32; 4]>,
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
        leave: (width >= 3.0 * button + 2.0 * GAP + 2.0 * MARGIN).then(|| {
            let mid = x0 + width / 2.0;
            [
                mid - button / 2.0,
                bottom,
                mid + button / 2.0,
                bottom + 20.0,
            ]
        }),
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
    /// Leave the server for the launcher.
    Disconnect,
    Nothing,
}

/// Seconds for the sheet to open or close, and for a switch to slide.
const OPEN_SECONDS: f32 = 0.22;
const SWITCH_SECONDS: f32 = 0.16;
/// HUD card sizes the editor's scroll wheel steps through.
const SCALE_STEP: f32 = 0.1;
const SCALE_RANGE: (f32, f32) = (0.5, 3.0);

fn approach(value: f32, target: f32, step: f32) -> f32 {
    if value < target {
        (value + step).min(target)
    } else {
        (value - step).max(target)
    }
}

/// Ease-out cubic: fast start, gentle landing.
fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// `color` with its alpha scaled by `amount`.
fn fade(color: u32, amount: f32) -> u32 {
    let alpha = ((color >> 24) as f32 * amount.clamp(0.0, 1.0)).round() as u32;
    (alpha << 24) | (color & 0x00FF_FFFF)
}

#[derive(Default)]
pub struct Menu {
    pub open: bool,
    pub editing: bool,
    /// The card being dragged and the grab offset from its corner.
    drag: Option<(HudKind, (f32, f32))>,
    /// Sheet animation, 0 hidden to 1 shown, moving toward `open`.
    shown: f32,
    /// Each card's switch knob, 0 off to 1 on, in menu order.
    knobs: Vec<f32>,
    /// Offer Disconnect (sessions started from the launcher).
    pub leave: bool,
}

impl Menu {
    pub fn active(&self) -> bool {
        self.open || self.editing
    }

    /// Still drawn: open, editing, or animating closed.
    pub fn visible(&self) -> bool {
        self.active() || self.shown > 0.0
    }

    /// Advance the open/close and switch animations by `dt` seconds.
    pub fn step(&mut self, dt: f32, settings: &Settings) {
        let dt = dt.clamp(0.0, 0.1);
        let target = if self.open { 1.0 } else { 0.0 };
        self.shown = approach(self.shown, target, dt / OPEN_SECONDS);
        let list = entries();
        self.knobs.resize(list.len(), f32::NAN);
        for (knob, entry) in self.knobs.iter_mut().zip(list) {
            if let Some(on) = entry.enabled(settings) {
                let target = f32::from(u8::from(on));
                *knob = if knob.is_nan() {
                    target
                } else {
                    approach(*knob, target, dt / SWITCH_SECONDS)
                };
            }
        }
    }

    /// Editor: the scroll wheel resizes the card under the mouse; true when
    /// settings changed.
    pub fn scroll(
        &mut self,
        mouse: (f32, f32),
        notches: i32,
        cards: &[(HudKind, [f32; 4])],
        settings: &mut Settings,
    ) -> bool {
        if !self.editing || notches == 0 {
            return false;
        }
        let Some(&(kind, _)) = cards.iter().rev().find(|(_, r)| inside(*r, mouse)) else {
            return false;
        };
        let hud = settings.hud_mut(kind);
        let scale = ((hud.scale + SCALE_STEP * notches as f32) / SCALE_STEP).round() * SCALE_STEP;
        let scale = scale.clamp(SCALE_RANGE.0, SCALE_RANGE.1);
        let changed = (scale - hud.scale).abs() > 1e-4;
        hud.scale = scale;
        changed
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
        if self.leave && g.leave.is_some_and(|rect| inside(rect, mouse)) {
            self.close();
            return Click::Disconnect;
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
            let label = "Drag to move  ·  Scroll to resize  ·  Right Shift to finish";
            let tw = canvas.system_width(label, BODY, Weight::Regular);
            let x = ((w - tw) / 2.0).floor();
            let mut pill = Panel::new(x - 10.0, 6.0, x + tw + 10.0, 24.0);
            pill.radius = 9.0;
            canvas.glass(pill);
            canvas.system_text(
                label,
                x,
                6.0 + (18.0 - BODY) / 2.0,
                BODY,
                Weight::Regular,
                WHITE,
            );
            return;
        }
        if self.shown <= 0.0 {
            return;
        }
        // Opening rises the sheet into place as it fades in; closing reverses.
        let e = ease(self.shown);
        let lift = ((1.0 - e) * 12.0).round();
        let mut g = geometry(canvas.layout);
        let shift = |r: &mut [f32; 4]| {
            r[1] += lift;
            r[3] += lift;
        };
        shift(&mut g.sheet);
        shift(&mut g.edit);
        shift(&mut g.done);
        if let Some(rect) = &mut g.leave {
            shift(rect);
        }
        for (_, rect) in &mut g.cards {
            shift(rect);
        }
        let [x0, y0, x1, _] = g.sheet;
        let mut sheet = Panel::new(g.sheet[0], g.sheet[1], g.sheet[2], g.sheet[3]);
        sheet.radius = 14.0;
        sheet.shadow = 0.4;
        sheet.tint = [0.05, 0.06, 0.08, 0.46];
        sheet.opacity = e;
        canvas.glass(sheet);
        let title_y = y0 + 9.0;
        let w = canvas.system_text(
            "Horizon",
            x0 + MARGIN,
            title_y,
            TITLE,
            Weight::Semibold,
            fade(WHITE, e),
        );
        canvas.system_text(
            "Mods",
            x0 + MARGIN + w + 4.0,
            title_y,
            TITLE,
            Weight::Regular,
            fade(0xFFC4_CAD4, e),
        );
        let hint = "Right Shift";
        let hw = canvas.system_width(hint, SMALL, Weight::Regular);
        canvas.system_text(
            hint,
            x1 - MARGIN - hw,
            title_y + (TITLE - SMALL) / 2.0,
            SMALL,
            Weight::Regular,
            fade(0xFFE2_E6EC, e),
        );
        for (index, (entry, rect)) in g.cards.iter().enumerate() {
            let hover = inside(*rect, mouse);
            let mut card = Panel::new(rect[0], rect[1], rect[2], rect[3]);
            card.radius = 8.0;
            card.shadow = 0.12;
            card.tint = if hover && self.open {
                [0.26, 0.28, 0.34, 0.50]
            } else {
                [0.12, 0.13, 0.16, 0.42]
            };
            card.opacity = e;
            canvas.glass(card);
            let text_y = rect[1] + (CARD_H - BODY) / 2.0;
            canvas.system_text(
                entry.title(),
                rect[0] + 9.0,
                text_y,
                BODY,
                Weight::Regular,
                fade(WHITE, e),
            );
            match entry.enabled(settings) {
                Some(on) => {
                    let knob = self
                        .knobs
                        .get(index)
                        .copied()
                        .filter(|k| !k.is_nan())
                        .unwrap_or(f32::from(u8::from(on)));
                    switch(
                        canvas,
                        rect[2] - 27.0,
                        rect[1] + (CARD_H - 11.0) / 2.0,
                        ease(knob),
                        e,
                    );
                }
                None => {
                    let value = entry.value(settings);
                    let vw = canvas.system_width(&value, BODY, Weight::Semibold);
                    canvas.system_text(
                        &value,
                        rect[2] - 9.0 - vw,
                        text_y,
                        BODY,
                        Weight::Semibold,
                        fade(0xFF6E_B6FF, e),
                    );
                }
            }
        }
        let hover = |rect| self.open && inside(rect, mouse);
        button(canvas, g.edit, "Edit HUD", hover(g.edit), false, e);
        button(canvas, g.done, "Done", hover(g.done), true, e);
        if let Some(rect) = g.leave.filter(|_| self.leave) {
            button(canvas, rect, "Disconnect", hover(rect), false, e);
        }
    }
}

/// System-font sizes (line box height in GUI pixels).
const TITLE: f32 = 9.5;
const BODY: f32 = 7.5;
const SMALL: f32 = 6.5;
const WHITE: u32 = 0xFFF7_F8FA;

/// An iOS-style switch: a pill track and a white knob; `on` slides from 0
/// (off) to 1 (on), blending the track from grey to green.
fn switch(canvas: &mut Canvas<'_>, x: f32, y: f32, on: f32, opacity: f32) {
    let mut track = Panel::new(x, y, x + 18.0, y + 11.0);
    track.radius = 5.5;
    track.shadow = 0.0;
    track.highlight = 0.3;
    let (off, lit) = ([0.42, 0.44, 0.50, 0.70], [0.20, 0.78, 0.35, 0.92]);
    track.tint = std::array::from_fn(|i: usize| off[i] + (lit[i] - off[i]) * on);
    track.opacity = opacity;
    canvas.glass(track);
    let kx = x + 1.0 + 7.0 * on;
    let mut knob = Panel::new(kx, y + 1.0, kx + 9.0, y + 10.0);
    knob.radius = 4.5;
    knob.shadow = 0.2;
    knob.highlight = 0.2;
    knob.tint = [1.0, 1.0, 1.0, 0.96];
    knob.opacity = opacity;
    canvas.glass(knob);
}

fn button(
    canvas: &mut Canvas<'_>,
    rect: [f32; 4],
    label: &str,
    hover: bool,
    primary: bool,
    opacity: f32,
) {
    let mut panel = Panel::new(rect[0], rect[1], rect[2], rect[3]);
    panel.radius = 10.0;
    panel.shadow = 0.15;
    panel.tint = match (primary, hover) {
        (true, false) => [0.04, 0.47, 1.0, 0.85],
        (true, true) => [0.20, 0.58, 1.0, 0.90],
        (false, false) => [0.20, 0.22, 0.27, 0.45],
        (false, true) => [0.32, 0.35, 0.42, 0.50],
    };
    panel.opacity = opacity;
    canvas.glass(panel);
    let w = canvas.system_width(label, BODY, Weight::Semibold);
    canvas.system_text(
        label,
        (rect[0] + (rect[2] - rect[0] - w) / 2.0).floor(),
        rect[1] + (rect[3] - rect[1] - BODY) / 2.0,
        BODY,
        Weight::Semibold,
        fade(0xFFFF_FFFF, opacity),
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
        // Disconnect shows only for launcher sessions.
        let leave = g.leave.expect("wide sheet");
        menu.toggle();
        assert_eq!(
            menu.click(layout, centre(leave), &mut settings),
            Click::Nothing
        );
        menu.leave = true;
        assert_eq!(
            menu.click(layout, centre(leave), &mut settings),
            Click::Disconnect
        );
        assert!(!menu.active());
    }

    #[test]
    fn new_cards_flip_numbers_and_cycle_crosshairs() {
        let layout = Layout::new((1440, 900), 1.0, 2);
        let g = geometry(layout);
        let mut settings = Settings::default();
        let mut menu = Menu::default();
        menu.toggle();
        let centre = |entry| {
            let (_, r) = g.cards.iter().find(|(e, _)| *e == entry).unwrap();
            ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0)
        };
        menu.click(layout, centre(Entry::ScoreboardNumbers), &mut settings);
        assert!(!settings.toggles.scoreboard_numbers);
        let styles: Vec<String> = (0..4)
            .map(|_| {
                menu.click(layout, centre(Entry::Crosshair), &mut settings);
                Entry::Crosshair.value(&settings)
            })
            .collect();
        assert_eq!(styles, ["Cross", "Dot", "Vanilla", "Cross"]);
        let outlines: Vec<String> = (0..4)
            .map(|_| {
                menu.click(layout, centre(Entry::BlockOutline), &mut settings);
                Entry::BlockOutline.value(&settings)
            })
            .collect();
        assert_eq!(outlines, ["Thick", "White", "Off", "Vanilla"]);
    }

    #[test]
    fn the_sheet_and_switches_animate_and_stay_drawn_while_closing() {
        let mut settings = Settings::default();
        let mut menu = Menu::default();
        assert!(!menu.visible());
        menu.toggle();
        menu.step(0.05, &settings);
        assert!(menu.shown > 0.0 && menu.shown < 1.0);
        for _ in 0..10 {
            menu.step(0.05, &settings);
        }
        assert_eq!(menu.shown, 1.0);
        // Switches start where their setting is, then slide.
        let sprint = entries()
            .iter()
            .position(|&e| e == Entry::ToggleSprint)
            .unwrap();
        assert_eq!(menu.knobs[sprint], 0.0);
        settings.toggles.toggle_sprint = true;
        menu.step(0.04, &settings);
        assert!(menu.knobs[sprint] > 0.0 && menu.knobs[sprint] < 1.0);
        menu.step(1.0, &settings);
        menu.step(1.0, &settings);
        assert_eq!(menu.knobs[sprint], 1.0);
        menu.close();
        menu.step(0.05, &settings);
        assert!(!menu.active() && menu.visible());
        for _ in 0..10 {
            menu.step(0.05, &settings);
        }
        assert!(!menu.visible());
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert_eq!(fade(0xFF12_3456, 0.5), 0x8012_3456);
    }

    #[test]
    fn scrolling_over_a_card_in_the_editor_resizes_it() {
        let mut settings = Settings::default();
        let mut menu = Menu::default();
        let cards = [(HudKind::Fps, [10.0, 10.0, 60.0, 26.0])];
        assert!(!menu.scroll((20.0, 15.0), 1, &cards, &mut settings));
        menu.editing = true;
        assert!(menu.scroll((20.0, 15.0), 2, &cards, &mut settings));
        assert!((settings.hud(HudKind::Fps).scale - 1.2).abs() < 1e-5);
        assert!(!menu.scroll((200.0, 15.0), 1, &cards, &mut settings));
        menu.scroll((20.0, 15.0), -40, &cards, &mut settings);
        assert_eq!(settings.hud(HudKind::Fps).scale, 0.5);
        assert!(!menu.scroll((20.0, 15.0), -1, &cards, &mut settings));
        menu.scroll((20.0, 15.0), 99, &cards, &mut settings);
        assert_eq!(settings.hud(HudKind::Fps).scale, 3.0);
    }
}
