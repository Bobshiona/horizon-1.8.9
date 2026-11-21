//! The mod menu (Right Shift): a graphite sheet over the dimmed world with a
//! category rail, type-to-search, and a scrolling grid of mod cards, plus
//! the HUD layout editor where cards are dragged into place. The open
//! category carries the ember brackets; switches are monochrome.
use crate::{
    glass::Panel,
    settings::{CROSSHAIRS, HudKind, Settings},
    style,
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
    VanillaMenu,
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
        Entry::VanillaMenu,
    ]);
    list
}

/// The rail's groups.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    All,
    Hud,
    Gameplay,
    Visual,
    General,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::All,
        Category::Hud,
        Category::Gameplay,
        Category::Visual,
        Category::General,
    ];

    fn title(self) -> &'static str {
        match self {
            Category::All => "All",
            Category::Hud => "HUD",
            Category::Gameplay => "Gameplay",
            Category::Visual => "Visual",
            Category::General => "General",
        }
    }
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
            Entry::VanillaMenu => "Vanilla Menu",
            Entry::Crosshair => "Crosshair",
            Entry::BlockOutline => "Block Outline",
            Entry::FpsLimit => "Frame Limit",
        }
    }

    pub fn category(self) -> Category {
        match self {
            Entry::Hud(_) => Category::Hud,
            Entry::Zoom | Entry::SmoothZoom | Entry::ToggleSprint | Entry::ToggleSneak => {
                Category::Gameplay
            }
            Entry::Fullbright
            | Entry::ScoreboardNumbers
            | Entry::Crosshair
            | Entry::BlockOutline => Category::Visual,
            Entry::FpsLimit | Entry::VanillaMenu => Category::General,
        }
    }

    /// Shown under the title: what the card is part of.
    fn note(self) -> &'static str {
        match self {
            Entry::Hud(_) => "HUD · drag in Edit HUD",
            Entry::FpsLimit => "Frames per second",
            Entry::VanillaMenu => "Esc opens vanilla's menus",
            _ => self.category().title(),
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
            Entry::VanillaMenu => t.vanilla_menu,
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
            Entry::VanillaMenu => t.vanilla_menu = !t.vanilla_menu,
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

/// The entries in `category` whose title contains `query`, any case.
fn shown(category: Category, query: &str) -> Vec<Entry> {
    let query = query.trim().to_lowercase();
    entries()
        .into_iter()
        .filter(|e| category == Category::All || e.category() == category)
        .filter(|e| query.is_empty() || e.title().to_lowercase().contains(&query))
        .collect()
}

/// Where everything sits for a screen size, in GUI pixels.
#[derive(Debug, PartialEq)]
pub struct Geometry {
    pub sheet: [f32; 4],
    pub search: [f32; 4],
    pub rail: Vec<(Category, [f32; 4])>,
    /// Where cards may sit.
    pub content: [f32; 4],
    /// The cards on screen at the current scroll.
    pub cards: Vec<(Entry, [f32; 4])>,
    /// Rows of cards in all, and how many fit.
    pub rows: usize,
    pub visible: usize,
    pub edit: [f32; 4],
    pub done: [f32; 4],
    /// Between Edit HUD and Done, when the sheet is wide enough.
    pub leave: Option<[f32; 4]>,
}

const RAIL_W: f32 = 76.0;
const HEADER: f32 = 36.0;
const FOOTER: f32 = 34.0;
const PAD: f32 = 10.0;
const CARD_H: f32 = 28.0;
const CARD_MIN_W: f32 = 110.0;
const GAP: f32 = 5.0;
const BUTTON_W: f32 = 76.0;

pub fn geometry(layout: Layout, category: Category, query: &str, scroll: usize) -> Geometry {
    let width = (layout.width - 16.0).clamp(240.0, 440.0);
    let height = (layout.height - 16.0).clamp(180.0, 280.0);
    let x0 = ((layout.width - width) / 2.0).floor().max(0.0);
    let y0 = ((layout.height - height) / 2.0).floor().max(0.0);
    let (x1, y1) = (x0 + width, y0 + height);
    let rail = Category::ALL
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let y = y0 + HEADER + 8.0 + i as f32 * 24.0;
            (c, [x0 + 6.0, y, x0 + RAIL_W - 6.0, y + 20.0])
        })
        .collect();
    let content = [
        x0 + RAIL_W + PAD,
        y0 + HEADER + PAD,
        x1 - PAD - 4.0,
        y1 - FOOTER - 6.0,
    ];
    let cw = content[2] - content[0];
    let columns = (((cw + GAP) / (CARD_MIN_W + GAP)).floor() as usize).clamp(1, 3);
    let card_w = (cw - GAP * (columns - 1) as f32) / columns as f32;
    let list = shown(category, query);
    let rows = list.len().div_ceil(columns);
    let visible = (((content[3] - content[1] + GAP) / (CARD_H + GAP)).floor() as usize).max(1);
    let scroll = scroll.min(rows.saturating_sub(visible));
    let cards = list
        .into_iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let (column, row) = (i % columns, i / columns);
            let row = row.checked_sub(scroll).filter(|&r| r < visible)?;
            let x = content[0] + column as f32 * (card_w + GAP);
            let y = content[1] + row as f32 * (CARD_H + GAP);
            Some((entry, [x, y, x + card_w, y + CARD_H]))
        })
        .collect();
    let by = y1 - FOOTER + 8.0;
    let button = |x: f32| [x, by, x + BUTTON_W, by + 18.0];
    Geometry {
        sheet: [x0, y0, x1, y1],
        search: [x1 - PAD - 120.0, y0 + 9.0, x1 - PAD, y0 + 27.0],
        rail,
        content,
        cards,
        rows,
        visible,
        edit: button(x0 + PAD),
        leave: (width >= 3.0 * BUTTON_W + 4.0 * PAD).then(|| button(x0 + 2.0 * PAD + BUTTON_W)),
        done: button(x1 - PAD - BUTTON_W),
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
const OPEN_SECONDS: f32 = 0.18;
const SWITCH_SECONDS: f32 = 0.12;
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
pub(crate) fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// `color` with its alpha scaled by `amount`.
pub(crate) fn fade(color: u32, amount: f32) -> u32 {
    style::fade(color, amount)
}

#[derive(Default)]
pub struct Menu {
    pub open: bool,
    pub editing: bool,
    /// The card being dragged and the grab offset from its corner.
    drag: Option<(HudKind, (f32, f32))>,
    /// Sheet animation, 0 hidden to 1 shown, moving toward `open`.
    shown: f32,
    /// Each card's switch knob, 0 off to 1 on, in `entries` order.
    knobs: Vec<f32>,
    /// Offer Disconnect (sessions started from the launcher).
    pub leave: bool,
    pub category: Category,
    /// Typed while the menu is open; filters the cards by title.
    pub query: String,
    /// Rows scrolled past.
    scroll: usize,
}

impl Menu {
    pub fn active(&self) -> bool {
        self.open || self.editing
    }

    /// The HUD card being dragged in the editor.
    pub fn dragging(&self) -> Option<HudKind> {
        self.drag.filter(|_| self.editing).map(|(kind, _)| kind)
    }

    /// Still drawn: open, editing, or animating closed.
    pub fn visible(&self) -> bool {
        self.active() || self.shown > 0.0
    }

    fn geometry(&self, layout: Layout) -> Geometry {
        geometry(layout, self.category, &self.query, self.scroll)
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

    /// The open menu: the wheel scrolls the cards a row per notch (up is
    /// positive).
    pub fn wheel(&mut self, layout: Layout, notches: i32) {
        if !self.open {
            return;
        }
        let g = self.geometry(layout);
        let most = g.rows.saturating_sub(g.visible);
        let next = self.scroll.min(most) as i64 - i64::from(notches);
        self.scroll = next.clamp(0, most as i64) as usize;
    }

    /// A key while the menu is open: typing searches, Backspace deletes and
    /// Escape clears the search first. True when the menu used the key.
    pub fn key(&mut self, key: winit::keyboard::KeyCode, typed: Option<&str>) -> bool {
        use winit::keyboard::KeyCode;
        if !self.open {
            return false;
        }
        match key {
            KeyCode::Escape if !self.query.is_empty() => self.query.clear(),
            KeyCode::Backspace => {
                self.query.pop();
            }
            _ => {
                let text: String = typed
                    .unwrap_or("")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == ' ')
                    .collect();
                if text.is_empty() || self.query.chars().count() >= 24 {
                    return false;
                }
                self.query.push_str(&text);
            }
        }
        self.scroll = 0;
        true
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
        self.query.clear();
        self.scroll = 0;
    }

    pub fn click(&mut self, layout: Layout, mouse: (f32, f32), settings: &mut Settings) -> Click {
        if !self.open {
            return Click::Nothing;
        }
        let g = self.geometry(layout);
        if let Some((entry, _)) = g.cards.iter().find(|(_, r)| inside(*r, mouse)) {
            entry.activate(settings);
            return Click::Changed;
        }
        if let Some((category, _)) = g.rail.iter().find(|(_, r)| inside(*r, mouse)) {
            self.category = *category;
            self.scroll = 0;
            return Click::Nothing;
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
        if self.editing {
            let w = canvas.layout.width;
            let label = "Drag to move  ·  Scroll to resize  ·  Right Shift to finish";
            let tw = canvas.system_width(label, BODY, Weight::Regular);
            let x = ((w - tw) / 2.0).floor();
            let pill = [x - 10.0, 6.0, x + tw + 10.0, 24.0];
            style::panel(canvas, pill, style::SURFACE, style::EDGE);
            canvas.system_text(
                label,
                x,
                6.0 + (18.0 - BODY) / 2.0,
                BODY,
                Weight::Regular,
                style::TEXT,
            );
            return;
        }
        if self.shown <= 0.0 {
            return;
        }
        let e = ease(self.shown);
        let c = |color: u32| style::fade(color, e);
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        canvas.rect(0.0, 0.0, w, h, style::fade(style::BASE, 0.55 * e));
        // Opening rises the sheet into place as it fades in.
        let lift = ((1.0 - e) * 10.0).round();
        let mut g = self.geometry(canvas.layout);
        let shift = |r: &mut [f32; 4]| {
            r[1] += lift;
            r[3] += lift;
        };
        for r in [
            &mut g.sheet,
            &mut g.search,
            &mut g.content,
            &mut g.edit,
            &mut g.done,
        ] {
            shift(r);
        }
        if let Some(r) = &mut g.leave {
            shift(r);
        }
        for (_, r) in g.rail.iter_mut() {
            shift(r);
        }
        for (_, r) in g.cards.iter_mut() {
            shift(r);
        }
        let [x0, y0, x1, y1] = g.sheet;
        let hover = |r: [f32; 4]| self.open && inside(r, mouse);
        style::panel(canvas, g.sheet, c(style::SURFACE), c(style::EDGE));
        let t = style::hairline(canvas);
        // Header: the mark, the title, and the search field.
        let mark = [x0 + PAD, y0 + 10.0, x0 + PAD + 16.0, y0 + 26.0];
        style::brackets(canvas, mark, c(style::TEXT));
        let hw = canvas.system_width("H", 9.0, Weight::Semibold);
        canvas.system_text(
            "H",
            (mark[0] + mark[2] - hw) / 2.0,
            mark[1] + 3.5,
            9.0,
            Weight::Semibold,
            c(style::TEXT),
        );
        canvas.system_text(
            "Mods",
            mark[2] + 10.0,
            y0 + 13.0,
            TITLE,
            Weight::Semibold,
            c(style::TEXT),
        );
        let searching = !self.query.is_empty();
        let edge = if searching { style::MUTED } else { style::EDGE };
        style::panel(canvas, g.search, c(style::BASE), c(edge));
        let (text, color) = if searching {
            (format!("{}|", self.query), style::TEXT)
        } else {
            ("Type to search".to_owned(), style::MUTED)
        };
        canvas.system_text(
            &text,
            g.search[0] + 8.0,
            g.search[1] + (18.0 - SMALL) / 2.0,
            SMALL,
            Weight::Regular,
            c(color),
        );
        canvas.rect(x0, y0 + HEADER - t, x1, y0 + HEADER, c(style::EDGE));
        canvas.rect(x0, y1 - FOOTER, x1, y1 - FOOTER + t, c(style::EDGE));
        // The rail.
        let rail_x = x0 + RAIL_W;
        canvas.rect(rail_x - t, y0 + HEADER, rail_x, y1 - FOOTER, c(style::EDGE));
        for (category, r) in &g.rail {
            let selected = *category == self.category;
            if selected {
                canvas.rect(r[0], r[1], r[2], r[3], c(style::CARD));
                style::brackets(canvas, *r, c(style::ACCENT));
            } else if hover(*r) {
                canvas.rect(r[0], r[1], r[2], r[3], c(style::HOVER));
            }
            let color = if selected || hover(*r) {
                style::TEXT
            } else {
                style::MUTED
            };
            canvas.system_text(
                category.title(),
                r[0] + 7.0,
                r[1] + (20.0 - BODY) / 2.0,
                BODY,
                Weight::Semibold,
                c(color),
            );
            let count = shown(*category, "").len().to_string();
            let cw = canvas.system_width(&count, SMALL, Weight::Regular);
            canvas.system_text(
                &count,
                r[2] - 7.0 - cw,
                r[1] + (20.0 - SMALL) / 2.0,
                SMALL,
                Weight::Regular,
                c(style::MUTED),
            );
        }
        // The cards.
        let list = entries();
        for (entry, r) in &g.cards {
            let fill = if hover(*r) { style::HOVER } else { style::CARD };
            style::panel(canvas, *r, c(fill), c(style::EDGE));
            canvas.system_text(
                entry.title(),
                r[0] + 9.0,
                r[1] + 5.5,
                BODY,
                Weight::Semibold,
                c(style::TEXT),
            );
            canvas.system_text(
                entry.note(),
                r[0] + 9.0,
                r[1] + 15.5,
                SMALL - 0.5,
                Weight::Regular,
                c(style::MUTED),
            );
            let cy = (r[1] + r[3]) / 2.0;
            match entry.enabled(settings) {
                Some(on) => {
                    let index = list.iter().position(|x| x == entry).unwrap_or(0);
                    let knob = self
                        .knobs
                        .get(index)
                        .copied()
                        .filter(|k| !k.is_nan())
                        .unwrap_or(f32::from(u8::from(on)));
                    square_switch(canvas, r[2] - 26.0, cy - 5.0, ease(knob), e);
                }
                None => {
                    let value = entry.value(settings);
                    let vw = canvas.system_width(&value, BODY - 0.5, Weight::Semibold);
                    let right = r[2] - 9.0;
                    let y = cy - (BODY - 0.5) / 2.0;
                    canvas.system_text(
                        "›",
                        right - 3.0,
                        y,
                        BODY - 0.5,
                        Weight::Regular,
                        c(style::MUTED),
                    );
                    canvas.system_text(
                        &value,
                        right - 7.0 - vw,
                        y,
                        BODY - 0.5,
                        Weight::Semibold,
                        c(style::TEXT),
                    );
                }
            }
        }
        if g.cards.is_empty() {
            let line = format!("No mods match \u{201c}{}\u{201d}", self.query.trim());
            canvas.system_text(
                &line,
                g.content[0],
                g.content[1] + 4.0,
                BODY,
                Weight::Regular,
                c(style::MUTED),
            );
        }
        // A scroll bar when the cards don't all fit.
        if g.rows > g.visible {
            let [_, top, right, bottom] = g.content;
            let track = bottom - top;
            let thumb = (track * g.visible as f32 / g.rows as f32).max(8.0);
            let most = (g.rows - g.visible) as f32;
            let at = top + (track - thumb) * (self.scroll as f32).min(most) / most;
            canvas.rect(right + 2.0, top, right + 3.0, bottom, c(style::EDGE));
            canvas.rect(right + 1.5, at, right + 3.5, at + thumb, c(style::MUTED));
        }
        // The footer.
        flat_button(canvas, g.edit, "Edit HUD", hover(g.edit), false, e);
        if let Some(r) = g.leave.filter(|_| self.leave) {
            flat_button(canvas, r, "Disconnect", hover(r), false, e);
        }
        flat_button(canvas, g.done, "Done", hover(g.done), true, e);
    }
}

/// System-font sizes (line box height in GUI pixels).
const TITLE: f32 = 10.0;
const BODY: f32 = 7.0;
const SMALL: f32 = 6.0;

/// A square monochrome switch, 18x10: a hollow track with a muted knob
/// when off, a filled track with a dark knob when on; `on` slides the knob.
fn square_switch(canvas: &mut Canvas<'_>, x: f32, y: f32, on: f32, opacity: f32) {
    let c = |color: u32| style::fade(color, opacity);
    let track = [x, y, x + 18.0, y + 10.0];
    if on >= 0.5 {
        canvas.rect(track[0], track[1], track[2], track[3], c(style::TEXT));
    } else {
        style::panel(canvas, track, c(style::BASE), c(style::MUTED));
    }
    let kx = x + 2.0 + 8.0 * on;
    let knob = if on >= 0.5 { style::BASE } else { style::MUTED };
    canvas.rect(kx, y + 2.0, kx + 6.0, y + 8.0, c(knob));
}

/// A footer button: graphite, or light for the primary action.
fn flat_button(
    canvas: &mut Canvas<'_>,
    r: [f32; 4],
    label: &str,
    hover: bool,
    primary: bool,
    opacity: f32,
) {
    let c = |color: u32| style::fade(color, opacity);
    let (fill, text) = match (primary, hover) {
        (true, false) => (style::TEXT, style::BASE),
        (true, true) => (0xFFFF_FFFF, style::BASE),
        (false, false) => (style::CARD, style::TEXT),
        (false, true) => (style::HOVER, style::TEXT),
    };
    style::panel(
        canvas,
        r,
        c(fill),
        c(if primary { fill } else { style::EDGE }),
    );
    let w = canvas.system_width(label, BODY - 0.5, Weight::Semibold);
    canvas.system_text(
        label,
        ((r[0] + r[2] - w) / 2.0).floor(),
        r[1] + (r[3] - r[1] - (BODY - 0.5)) / 2.0,
        BODY - 0.5,
        Weight::Semibold,
        c(text),
    );
}

/// The glass sheets' body size, for the pause and options sheets' buttons.
const GLASS_BODY: f32 = 7.5;

/// An iOS-style switch: a pill track and a white knob; `on` slides from 0
/// (off) to 1 (on), blending the track from grey to green.
pub(crate) fn switch(canvas: &mut Canvas<'_>, x: f32, y: f32, on: f32, opacity: f32) {
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

pub(crate) fn button(
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
    let w = canvas.system_width(label, GLASS_BODY, Weight::Semibold);
    canvas.system_text(
        label,
        (rect[0] + (rect[2] - rect[0] - w) / 2.0).floor(),
        rect[1] + (rect[3] - rect[1] - GLASS_BODY) / 2.0,
        GLASS_BODY,
        Weight::Semibold,
        fade(0xFFFF_FFFF, opacity),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(r: [f32; 4]) -> (f32, f32) {
        ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0)
    }

    fn card(g: &Geometry, entry: Entry) -> [f32; 4] {
        g.cards
            .iter()
            .find(|(e, _)| *e == entry)
            .unwrap_or_else(|| panic!("{entry:?} not on screen"))
            .1
    }

    #[test]
    fn the_sheet_fits_every_screen_and_cards_do_not_overlap() {
        for (size, scale) in [
            ((640, 480), 2),
            ((1440, 900), 2),
            ((2880, 1800), 4),
            ((1920, 1080), 3),
            ((1920, 1200), 0),
        ] {
            let layout = Layout::new(size, 1.0, scale);
            for scroll in 0..8 {
                let g = geometry(layout, Category::All, "", scroll);
                assert!(
                    g.sheet[0] >= 0.0
                        && g.sheet[1] >= 0.0
                        && g.sheet[2] <= layout.width + 0.5
                        && g.sheet[3] <= layout.height + 0.5,
                    "{size:?}"
                );
                assert!(!g.cards.is_empty());
                for (i, (_, a)) in g.cards.iter().enumerate() {
                    assert!(
                        a[0] >= g.content[0]
                            && a[2] <= g.content[2] + 0.01
                            && a[1] >= g.content[1]
                            && a[3] <= g.content[3] + 0.01,
                        "{size:?} {a:?} outside {:?}",
                        g.content
                    );
                    for (_, b) in &g.cards[i + 1..] {
                        assert!(a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1]);
                    }
                }
                for (_, r) in &g.rail {
                    assert!(r[3] <= g.edit[1], "{size:?} rail runs into the footer");
                }
            }
        }
    }

    #[test]
    fn clicks_flip_switches_cycle_the_limit_and_drags_move_cards() {
        let layout = Layout::new((1440, 900), 1.0, 2);
        let mut settings = Settings::default();
        let mut menu = Menu::default();
        menu.toggle();
        let g = menu.geometry(layout);
        let sprint = card(&g, Entry::ToggleSprint);
        assert_eq!(
            menu.click(layout, centre(sprint), &mut settings),
            Click::Changed
        );
        assert!(settings.toggles.toggle_sprint);
        // The rail picks a category; General holds the frame limit.
        let (_, general) = *g
            .rail
            .iter()
            .find(|(c, _)| *c == Category::General)
            .unwrap();
        menu.click(layout, centre(general), &mut settings);
        assert_eq!(menu.category, Category::General);
        let g = menu.geometry(layout);
        assert_eq!(g.cards.len(), 2);
        menu.click(layout, centre(card(&g, Entry::FpsLimit)), &mut settings);
        assert_eq!(settings.toggles.fps_limit, 60);
        assert_eq!(
            menu.click(layout, centre(g.edit), &mut settings),
            Click::EditHud
        );
        assert!(menu.editing && !menu.open);
        let cards = [(HudKind::Ping, [10.0, 10.0, 50.0, 26.0])];
        menu.press((20.0, 15.0), &cards);
        assert_eq!(menu.dragging(), Some(HudKind::Ping));
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
        let mut settings = Settings::default();
        let mut menu = Menu::default();
        menu.toggle();
        menu.category = Category::Visual;
        let g = menu.geometry(layout);
        let at = |entry| centre(card(&g, entry));
        menu.click(layout, at(Entry::ScoreboardNumbers), &mut settings);
        assert!(!settings.toggles.scoreboard_numbers);
        let styles: Vec<String> = (0..4)
            .map(|_| {
                menu.click(layout, at(Entry::Crosshair), &mut settings);
                Entry::Crosshair.value(&settings)
            })
            .collect();
        assert_eq!(styles, ["Cross", "Dot", "Vanilla", "Cross"]);
        let outlines: Vec<String> = (0..4)
            .map(|_| {
                menu.click(layout, at(Entry::BlockOutline), &mut settings);
                Entry::BlockOutline.value(&settings)
            })
            .collect();
        assert_eq!(outlines, ["Thick", "White", "Off", "Vanilla"]);
    }

    #[test]
    fn typing_searches_escape_clears_and_the_wheel_scrolls() {
        use winit::keyboard::KeyCode;
        let layout = Layout::new((1440, 900), 1.0, 2);
        let mut menu = Menu::default();
        // Closed (or editing), keys are not the menu's.
        assert!(!menu.key(KeyCode::KeyZ, Some("z")));
        menu.toggle();
        assert!(menu.key(KeyCode::KeyZ, Some("Z")));
        assert!(menu.key(KeyCode::KeyO, Some("o")));
        let g = menu.geometry(layout);
        let found: Vec<Entry> = g.cards.iter().map(|(e, _)| *e).collect();
        assert_eq!(found, [Entry::Zoom, Entry::SmoothZoom]);
        assert!(menu.key(KeyCode::Backspace, None));
        assert_eq!(menu.query, "Z");
        // Escape clears the search before it closes the menu.
        assert!(menu.key(KeyCode::Escape, None));
        assert!(menu.query.is_empty() && menu.open);
        assert!(!menu.key(KeyCode::Escape, None));
        assert!(!menu.key(KeyCode::ArrowUp, None));
        assert!(menu.key(KeyCode::KeyQ, Some("qqq")));
        assert!(menu.geometry(layout).cards.is_empty());
        menu.close();
        assert!(menu.query.is_empty());
        // All 17 cards don't fit a small window: the wheel scrolls by rows
        // and stops at both ends.
        let small = Layout::new((640, 480), 1.0, 2);
        menu.toggle();
        let g = menu.geometry(small);
        assert!(g.rows > g.visible, "{} rows, {} fit", g.rows, g.visible);
        menu.wheel(small, -1);
        assert_eq!(menu.scroll, 1);
        menu.wheel(small, -99);
        assert_eq!(menu.scroll, g.rows - g.visible);
        let last = menu.geometry(small);
        assert!(last.cards.iter().any(|(e, _)| *e == Entry::VanillaMenu));
        menu.wheel(small, 99);
        assert_eq!(menu.scroll, 0);
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
