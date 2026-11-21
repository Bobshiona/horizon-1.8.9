//! The launcher: a window of its own before the game, in the Horizon look —
//! graphite surfaces with hairline edges, one ember accent kept for Play and
//! the selection, and corner brackets (a hitbox's corners) marking what is
//! selected. A sidebar switches between Home (welcome, Play, quick play with
//! live server status and the latest news), Servers, News, Mods and
//! Settings. Play starts the game as its own process; the launcher keeps the
//! account, the server list, the settings and the updates. Sign-in runs on
//! its own thread so the window never waits.
use crate::{
    settings::{CROSSHAIRS, HudKind, Settings},
    status::{Pinger, State},
    style,
    sysfont::Weight,
    ui::{Canvas, Layout},
    updater::Status,
};
use horizon_auth::Session;
use std::sync::{Arc, Mutex};

/// The Microsoft account as the launcher shows it.
#[derive(Clone)]
pub enum Account {
    Checking,
    SignedOut,
    /// Device-code sign-in: where to go and the code to enter.
    Waiting {
        uri: String,
        code: String,
    },
    SignedIn(Arc<Session>),
    Failed(String),
}

/// What a click or key on the launcher asks the app to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the game on a server: host and port.
    Play(String, u16),
    /// Reopen the launcher on an installed update.
    Restart,
}

/// Split `host[:port]`; the port defaults to 25565.
pub fn address(text: &str) -> Option<(String, u16)> {
    let text = text.trim();
    let (host, port) = match text.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (text, 25565),
    };
    let valid = !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    (valid && port != 0).then(|| (host.to_ascii_lowercase(), port))
}

/// Servers on this Mac accept an offline name when nobody is signed in.
pub fn local(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    Servers,
    News,
    Mods,
    Settings,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Page::Home,
        Page::Servers,
        Page::News,
        Page::Mods,
        Page::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Servers => "Servers",
            Page::News => "News",
            Page::Mods => "Mods",
            Page::Settings => "Settings",
        }
    }

    /// An 8x8 pixel icon: one byte per row, the left pixel in the high bit.
    fn icon(self) -> [u8; 8] {
        match self {
            Page::Home => [0x18, 0x3C, 0x7E, 0xFF, 0x7E, 0x66, 0x66, 0x66],
            Page::Servers => [0xFF, 0xFD, 0xFF, 0x00, 0xFF, 0xFD, 0xFF, 0x00],
            Page::News => [0xFE, 0x82, 0xBA, 0x82, 0xBA, 0x82, 0xB2, 0xFE],
            Page::Mods => [0xEE, 0xEE, 0xEE, 0x00, 0xEE, 0xEE, 0xEE, 0x00],
            Page::Settings => [0x18, 0x5A, 0x3C, 0xE7, 0xE7, 0x3C, 0x5A, 0x18],
        }
    }
}

/// A mod the Mods page switches or cycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mod {
    Hud(HudKind),
    Zoom,
    SmoothZoom,
    ToggleSprint,
    ToggleSneak,
    Fullbright,
    ScoreboardNumbers,
    AdvancedTooltips,
    Crosshair,
    Outline,
}

const MODS: [Mod; 16] = [
    Mod::Hud(HudKind::Keystrokes),
    Mod::Hud(HudKind::Cps),
    Mod::Hud(HudKind::Fps),
    Mod::Hud(HudKind::Armor),
    Mod::Hud(HudKind::Potions),
    Mod::Hud(HudKind::Coordinates),
    Mod::Hud(HudKind::Ping),
    Mod::Zoom,
    Mod::SmoothZoom,
    Mod::ToggleSprint,
    Mod::ToggleSneak,
    Mod::Fullbright,
    Mod::ScoreboardNumbers,
    Mod::AdvancedTooltips,
    Mod::Crosshair,
    Mod::Outline,
];

/// What a card shows on its right.
enum Value {
    Switch(bool),
    Choice(String),
}

impl Mod {
    fn title(self) -> &'static str {
        match self {
            Mod::Hud(kind) => kind.title(),
            Mod::Zoom => "Zoom",
            Mod::SmoothZoom => "Smooth Zoom",
            Mod::ToggleSprint => "Toggle Sprint",
            Mod::ToggleSneak => "Toggle Sneak",
            Mod::Fullbright => "Fullbright",
            Mod::ScoreboardNumbers => "Scoreboard Numbers",
            Mod::AdvancedTooltips => "Advanced Tooltips",
            Mod::Crosshair => "Crosshair",
            Mod::Outline => "Block Outline",
        }
    }

    fn group(self) -> &'static str {
        match self {
            Mod::Hud(_) => "HUD",
            Mod::Zoom | Mod::SmoothZoom => "Zoom",
            Mod::ToggleSprint | Mod::ToggleSneak => "Movement",
            Mod::AdvancedTooltips => "Tooltips · F3 + H",
            _ => "Visual",
        }
    }

    fn value(self, settings: &Settings) -> Value {
        let t = &settings.toggles;
        Value::Switch(match self {
            Mod::Hud(kind) => settings.hud(kind).enabled,
            Mod::Zoom => t.zoom,
            Mod::SmoothZoom => t.smooth_zoom,
            Mod::ToggleSprint => t.toggle_sprint,
            Mod::ToggleSneak => t.toggle_sneak,
            Mod::Fullbright => t.fullbright,
            Mod::ScoreboardNumbers => t.scoreboard_numbers,
            Mod::AdvancedTooltips => t.advanced_tooltips,
            Mod::Crosshair => {
                let name = CROSSHAIRS.get(usize::from(t.crosshair)).unwrap_or(&"");
                return Value::Choice((*name).to_owned());
            }
            Mod::Outline => {
                let name = crate::outline::STYLES
                    .get(usize::from(t.outline))
                    .unwrap_or(&"");
                return Value::Choice((*name).to_owned());
            }
        })
    }

    fn click(self, settings: &mut Settings) {
        let flip = |value: &mut bool| *value = !*value;
        if let Mod::Hud(kind) = self {
            flip(&mut settings.hud_mut(kind).enabled);
            return;
        }
        let t = &mut settings.toggles;
        match self {
            Mod::Hud(_) => {}
            Mod::Zoom => flip(&mut t.zoom),
            Mod::SmoothZoom => flip(&mut t.smooth_zoom),
            Mod::ToggleSprint => flip(&mut t.toggle_sprint),
            Mod::ToggleSneak => flip(&mut t.toggle_sneak),
            Mod::Fullbright => flip(&mut t.fullbright),
            Mod::ScoreboardNumbers => flip(&mut t.scoreboard_numbers),
            Mod::AdvancedTooltips => flip(&mut t.advanced_tooltips),
            Mod::Crosshair => t.crosshair = (t.crosshair + 1) % CROSSHAIRS.len() as u8,
            Mod::Outline => t.outline = (t.outline + 1) % crate::outline::STYLES.len() as u8,
        }
    }
}

/// A display or control option on the Settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setting {
    Fov,
    Sensitivity,
    RenderDistance,
    GuiScale,
    FrameLimit,
    Fullscreen,
    ViewBobbing,
    MenuStyle,
}

const SETTINGS: [Setting; 8] = [
    Setting::Fov,
    Setting::Sensitivity,
    Setting::RenderDistance,
    Setting::GuiScale,
    Setting::FrameLimit,
    Setting::Fullscreen,
    Setting::ViewBobbing,
    Setting::MenuStyle,
];

/// Frame caps the Settings page steps through; 0 follows the display.
const FRAME_LIMITS: [u32; 6] = [0, 60, 120, 144, 240, u32::MAX];

impl Setting {
    fn title(self) -> &'static str {
        match self {
            Setting::Fov => "Field of View",
            Setting::Sensitivity => "Sensitivity",
            Setting::RenderDistance => "Render Distance",
            Setting::GuiScale => "GUI Scale",
            Setting::FrameLimit => "Frame Limit",
            Setting::Fullscreen => "Fullscreen",
            Setting::ViewBobbing => "View Bobbing",
            Setting::MenuStyle => "Vanilla Menus",
        }
    }

    /// On or off, flipped by a click anywhere on the card.
    fn switch(self) -> bool {
        matches!(
            self,
            Setting::Fullscreen | Setting::ViewBobbing | Setting::MenuStyle
        )
    }

    fn value(self, settings: &Settings) -> Value {
        let v = &settings.video;
        Value::Choice(match self {
            Setting::Fov => format!("{:.0}°", v.fov),
            Setting::Sensitivity => format!("{:.0}%", v.sensitivity * 200.0),
            Setting::RenderDistance => format!("{} chunks", v.render_distance),
            Setting::GuiScale if v.gui_scale == 0 => "Auto".to_owned(),
            Setting::GuiScale => v.gui_scale.to_string(),
            Setting::FrameLimit => match settings.toggles.fps_limit {
                0 => "Display".to_owned(),
                u32::MAX => "Unlimited".to_owned(),
                fps => fps.to_string(),
            },
            Setting::Fullscreen => return Value::Switch(v.fullscreen),
            Setting::ViewBobbing => return Value::Switch(v.view_bobbing),
            Setting::MenuStyle => return Value::Switch(settings.toggles.vanilla_menu),
        })
    }

    /// Step by `direction` (-1 or 1); switches flip on any click.
    fn step(self, settings: &mut Settings, direction: i8) {
        let d = f32::from(direction);
        let v = &mut settings.video;
        match self {
            Setting::Fov => v.fov = (v.fov + 5.0 * d).clamp(30.0, 110.0),
            Setting::Sensitivity => {
                v.sensitivity = ((v.sensitivity + 0.05 * d) * 20.0).round().clamp(0.0, 20.0) / 20.0;
            }
            Setting::RenderDistance => {
                v.render_distance =
                    (i16::from(v.render_distance) + i16::from(direction)).clamp(2, 16) as u8;
            }
            Setting::GuiScale => {
                v.gui_scale = (i64::from(v.gui_scale) + i64::from(direction)).clamp(0, 4) as u32;
            }
            Setting::FrameLimit => {
                let limit = &mut settings.toggles.fps_limit;
                let at = FRAME_LIMITS.iter().position(|l| l == limit).unwrap_or(0) as i64;
                let next = (at + i64::from(direction)).clamp(0, FRAME_LIMITS.len() as i64 - 1);
                *limit = FRAME_LIMITS[next as usize];
            }
            Setting::Fullscreen => v.fullscreen = !v.fullscreen,
            Setting::ViewBobbing => v.view_bobbing = !v.view_bobbing,
            Setting::MenuStyle => {
                settings.toggles.vanilla_menu = !settings.toggles.vanilla_menu;
            }
        }
    }
}

/// One entry of the News page.
#[derive(Debug, PartialEq)]
struct Entry {
    title: String,
    lines: Vec<String>,
}

/// `NEWS.md`: `## ` titles, each with `- ` lines, newest first.
fn news(text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            entries.push(Entry {
                title: title.trim().to_owned(),
                lines: Vec::new(),
            });
        } else if let (Some(item), Some(entry)) = (line.strip_prefix("- "), entries.last_mut()) {
            entry.lines.push(item.trim().to_owned());
        }
    }
    entries
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Nav(Page),
    Account,
    Copy,
    Play,
    Server(usize),
    Remove(usize),
    Field,
    Add,
    AllServers,
    LatestNews,
    Mod(usize),
    /// A setting and the step: -1 or 1 for the halves of a stepper, 0 for
    /// a switch.
    Setting(usize, i8),
    Restart,
}

/// The launcher's design space: at least 480x300 units, scaled to fit.
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    scale: f32,
    width: f32,
    height: f32,
}

fn view(layout: Layout) -> View {
    let scale = (layout.width / 480.0).min(layout.height / 300.0).max(0.25);
    View {
        scale,
        width: layout.width / scale,
        height: layout.height / scale,
    }
}

const SIDEBAR: f32 = 48.0;
const MARGIN: f32 = 20.0;
/// Where the page's content starts, below the header.
const TOP: f32 = 46.0;
const HERO_H: f32 = 84.0;
const TILE_H: f32 = 46.0;
const ROW_H: f32 = 24.0;
const CARD_H: f32 = 32.0;
const GAP: f32 = 6.0;

const TITLE: f32 = 12.0;
const BODY: f32 = 7.0;
const SMALL: f32 = 5.5;

fn inside(rect: [f32; 4], (x, y): (f32, f32)) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

fn nav(index: usize) -> [f32; 4] {
    let y = 46.0 + index as f32 * 38.0;
    [6.0, y, SIDEBAR - 6.0, y + 34.0]
}

/// Quick-play tiles across the Home page.
fn tiles(view: View) -> usize {
    let width = view.width - SIDEBAR - 2.0 * MARGIN;
    (((width + GAP) / (120.0 + GAP)).floor() as usize).clamp(1, 4)
}

/// Server rows that fit on the Servers page.
fn rows(view: View) -> usize {
    (((view.height - 62.0 - TOP + 4.0) / (ROW_H + 4.0)).floor() as usize).max(1)
}

/// The first row shown so that `selected` is visible.
fn first_row(selected: usize, rows: usize) -> usize {
    (selected + 1).saturating_sub(rows)
}

fn grid(x0: f32, x1: f32, columns: usize, index: usize) -> [f32; 4] {
    let width = (x1 - x0 - GAP * (columns - 1) as f32) / columns as f32;
    let (column, row) = (index % columns, index / columns);
    let x = x0 + column as f32 * (width + GAP);
    let y = TOP + row as f32 * (CARD_H + GAP - 1.0);
    [x, y, x + width, y + CARD_H]
}

/// Everything clickable on `page`, in hit-test order.
fn geometry(page: Page, view: View, servers: usize, first: usize) -> Vec<(Target, [f32; 4])> {
    let (x0, x1) = (SIDEBAR + MARGIN, view.width - MARGIN);
    let mut targets: Vec<(Target, [f32; 4])> = Page::ALL
        .iter()
        .enumerate()
        .map(|(i, &page)| (Target::Nav(page), nav(i)))
        .collect();
    targets.push((Target::Account, [x1 - 64.0, 13.0, x1, 29.0]));
    targets.push((Target::Copy, [x1 - 132.0, 13.0, x1 - 70.0, 29.0]));
    match page {
        Page::Home => {
            targets.push((
                Target::Play,
                [x1 - 14.0 - 104.0, TOP + 22.0, x1 - 14.0, TOP + 54.0],
            ));
            let label = TOP + HERO_H + 14.0;
            targets.push((
                Target::AllServers,
                [x1 - 64.0, label - 2.0, x1, label + 8.0],
            ));
            let n = tiles(view);
            let width = (x1 - x0 - GAP * (n - 1) as f32) / n as f32;
            let y = label + 14.0;
            for i in 0..n.min(servers) {
                let x = x0 + i as f32 * (width + GAP);
                targets.push((Target::Server(i), [x, y, x + width, y + TILE_H]));
            }
            let news = y + TILE_H + 14.0;
            if news + 26.0 <= view.height - 8.0 {
                targets.push((Target::LatestNews, [x0, news, x1, news + 26.0]));
            }
        }
        Page::Servers => {
            for (k, i) in (first..servers).take(rows(view)).enumerate() {
                let y = TOP + k as f32 * (ROW_H + 4.0);
                targets.push((Target::Remove(i), [x1 - 22.0, y + 5.0, x1 - 8.0, y + 19.0]));
                targets.push((Target::Server(i), [x0, y, x1, y + ROW_H]));
            }
            let bottom = view.height - 16.0;
            targets.push((Target::Play, [x1 - 104.0, bottom - 32.0, x1, bottom]));
            targets.push((
                Target::Add,
                [x1 - 166.0, bottom - 27.0, x1 - 112.0, bottom - 5.0],
            ));
            targets.push((Target::Field, [x0, bottom - 27.0, x1 - 172.0, bottom - 5.0]));
        }
        Page::News => {}
        Page::Mods => {
            for i in 0..MODS.len() {
                targets.push((Target::Mod(i), grid(x0, x1, 3, i)));
            }
        }
        Page::Settings => {
            for (i, setting) in SETTINGS.iter().enumerate() {
                let r = grid(x0, x1, 2, i);
                if setting.switch() {
                    targets.push((Target::Setting(i, 0), r));
                } else {
                    let mid = r[2] - 40.0;
                    targets.push((Target::Setting(i, -1), [r[0], r[1], mid, r[3]]));
                    targets.push((Target::Setting(i, 1), [mid, r[1], r[2], r[3]]));
                }
            }
            let y = grid(x0, x1, 2, SETTINGS.len())[1];
            targets.push((Target::Restart, [x1 - 74.0, y + 8.0, x1 - 10.0, y + 24.0]));
        }
    }
    targets
}

pub struct Launcher {
    account: Arc<Mutex<Account>>,
    page: Page,
    selected: usize,
    /// The address being typed, when the field has focus.
    field: Option<String>,
    message: Option<String>,
    pinger: Pinger,
    news: Vec<Entry>,
}

impl Launcher {
    fn with(account: Account) -> Self {
        Self {
            account: Arc::new(Mutex::new(account)),
            page: Page::Home,
            selected: 0,
            field: None,
            message: None,
            pinger: Pinger::default(),
            news: news(include_str!("../NEWS.md")),
        }
    }

    /// Start with the saved sign-in, checked in the background.
    pub fn new() -> Self {
        let launcher = Self::with(Account::Checking);
        launcher.spawn(|client| match horizon_auth::saved::load(client) {
            Ok(Some(token)) => match horizon_auth::refresh(client, &token) {
                Ok(session) => {
                    crate::options::remember(client, &session);
                    Account::SignedIn(Arc::new(session))
                }
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    let _ = horizon_auth::saved::forget(client);
                    Account::SignedOut
                }
                Err(_) => Account::Failed("Couldn't reach Microsoft".into()),
            },
            _ => Account::SignedOut,
        });
        launcher
    }

    /// Validation captures: signed out, never touching the saved sign-in,
    /// opened on `page`.
    pub fn preview(page: Page) -> Self {
        let mut launcher = Self::with(Account::SignedOut);
        launcher.page = page;
        launcher
    }

    fn spawn(&self, work: impl FnOnce(&str) -> Account + Send + 'static) {
        let account = self.account.clone();
        let _ = std::thread::Builder::new()
            .name("Sign-in".into())
            .spawn(move || {
                let next = work(&crate::options::client_id());
                if let Ok(mut account) = account.lock() {
                    *account = next;
                }
            });
    }

    fn account(&self) -> Account {
        self.account
            .lock()
            .map_or(Account::Failed("Sign-in state lost".into()), |a| a.clone())
    }

    /// The signed-in session, if any.
    pub fn session(&self) -> Option<Arc<Session>> {
        match self.account() {
            Account::SignedIn(session) => Some(session),
            _ => None,
        }
    }

    /// Show a one-line message, such as why the game didn't start.
    pub fn notice(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    fn sign_in(&self) {
        if let Ok(mut account) = self.account.lock() {
            *account = Account::Checking;
        }
        let shared = self.account.clone();
        self.spawn(move |client| {
            let code = match horizon_auth::begin(client) {
                Ok(code) => code,
                Err(_) => return Account::Failed("Couldn't reach Microsoft".into()),
            };
            if let Ok(mut account) = shared.lock() {
                *account = Account::Waiting {
                    uri: code.verification_uri.clone(),
                    code: code.user_code.clone(),
                };
            }
            let _ = std::process::Command::new("/usr/bin/open")
                .arg(&code.verification_uri)
                .spawn();
            match horizon_auth::finish(client, code) {
                Ok(session) => {
                    crate::options::remember(client, &session);
                    Account::SignedIn(Arc::new(session))
                }
                Err(_) => Account::Failed("Sign-in didn't finish".into()),
            }
        });
    }

    fn sign_out(&self) {
        let _ = horizon_auth::saved::forget(&crate::options::client_id());
        if let Ok(mut account) = self.account.lock() {
            *account = Account::SignedOut;
        }
    }

    /// Whether typed keys belong to the address field.
    #[cfg(test)]
    fn typing(&self) -> bool {
        self.field.is_some()
    }

    fn targets(&self, view: View, settings: &Settings) -> Vec<(Target, [f32; 4])> {
        let first = first_row(self.selected, rows(view));
        geometry(self.page, view, settings.servers.len(), first)
    }

    pub fn click(
        &mut self,
        layout: Layout,
        mouse: (f32, f32),
        settings: &mut Settings,
        update: &Status,
    ) -> Option<Action> {
        let view = view(layout);
        let mouse = (mouse.0 / view.scale, mouse.1 / view.scale);
        let target = self
            .targets(view, settings)
            .into_iter()
            .find(|(_, rect)| inside(*rect, mouse))
            .map(|(target, _)| target);
        if target != Some(Target::Field) && target != Some(Target::Add) {
            self.field = None;
        }
        self.message = None;
        match target? {
            Target::Nav(page) => self.page = page,
            Target::Account => match self.account() {
                Account::SignedIn(_) => self.sign_out(),
                Account::SignedOut | Account::Failed(_) => self.sign_in(),
                Account::Checking | Account::Waiting { .. } => {}
            },
            Target::Copy => {
                if let Account::Waiting { code, .. } = self.account() {
                    copy(&code);
                    self.message = Some("Code copied".into());
                }
            }
            Target::Server(row) => {
                if row == self.selected && row < settings.servers.len() {
                    return self.play(settings);
                }
                self.selected = row;
            }
            Target::Remove(row) => {
                if row == self.selected && row < settings.servers.len() {
                    settings.servers.remove(row);
                    self.selected = self.selected.min(settings.servers.len().saturating_sub(1));
                } else {
                    self.selected = row;
                }
            }
            Target::Field => {
                self.field.get_or_insert_with(String::new);
            }
            Target::Add => self.add(settings),
            Target::Play => return self.play(settings),
            Target::AllServers => self.page = Page::Servers,
            Target::LatestNews => self.page = Page::News,
            Target::Mod(index) => MODS[index].click(settings),
            Target::Setting(index, step) => SETTINGS[index].step(settings, step),
            Target::Restart => {
                if matches!(update, Status::Ready(_)) {
                    return Some(Action::Restart);
                }
            }
        }
        None
    }

    fn add(&mut self, settings: &mut Settings) {
        let Some(text) = self.field.take() else {
            self.field = Some(String::new());
            return;
        };
        match address(&text) {
            Some(_) if settings.servers.len() >= 32 => {
                self.message = Some("The list is full".into());
            }
            Some((host, port)) => {
                let shown = if port == 25565 {
                    host
                } else {
                    format!("{host}:{port}")
                };
                settings.servers.push((shown.clone(), shown));
                self.selected = settings.servers.len() - 1;
            }
            None if text.trim().is_empty() => {}
            None => {
                self.message = Some("That isn't a server address".into());
                self.field = Some(text);
            }
        }
    }

    fn play(&mut self, settings: &Settings) -> Option<Action> {
        let Some((_, text)) = settings.servers.get(self.selected) else {
            self.message = Some("Add a server first".into());
            return None;
        };
        let Some((host, port)) = address(text) else {
            self.message = Some("That isn't a server address".into());
            return None;
        };
        if self.session().is_none() && !local(&host) {
            self.message = Some("Sign in with Microsoft to join this server".into());
            return None;
        }
        Some(Action::Play(host, port))
    }

    /// A key while the launcher shows; returns an action for Enter.
    pub fn key(
        &mut self,
        key: winit::keyboard::KeyCode,
        typed: Option<&str>,
        settings: &mut Settings,
    ) -> Option<Action> {
        use winit::keyboard::KeyCode;
        if let Some(field) = &mut self.field {
            match key {
                KeyCode::Enter | KeyCode::NumpadEnter => self.add(settings),
                KeyCode::Escape => self.field = None,
                KeyCode::Backspace => {
                    field.pop();
                }
                _ => {
                    for c in typed.unwrap_or("").chars() {
                        if (c.is_ascii_alphanumeric() || ".-:".contains(c)) && field.len() < 64 {
                            field.push(c);
                        }
                    }
                }
            }
            return None;
        }
        match key {
            KeyCode::Enter | KeyCode::NumpadEnter => self.play(settings),
            KeyCode::ArrowDown => {
                self.selected = (self.selected + 1).min(settings.servers.len().saturating_sub(1));
                None
            }
            KeyCode::ArrowUp => {
                self.selected = self.selected.saturating_sub(1);
                None
            }
            _ => None,
        }
    }

    /// The live status of the server at `index` in the list.
    fn state(&self, settings: &Settings, index: usize) -> Option<State> {
        let (_, text) = settings.servers.get(index)?;
        let (host, port) = address(text)?;
        Some(self.pinger.get(&host, port))
    }

    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        settings: &Settings,
        update: &Status,
        mouse: (f32, f32),
    ) {
        let view = view(canvas.layout);
        canvas.scale = view.scale;
        let mouse = (mouse.0 / view.scale, mouse.1 / view.scale);
        let targets = self.targets(view, settings);
        let rect = |target| targets.iter().find(|(t, _)| *t == target).map(|(_, r)| *r);
        let hover = |target| rect(target).is_some_and(|r| inside(r, mouse));
        canvas.rect(0.0, 0.0, view.width, view.height, style::BASE);
        self.sidebar(canvas, view, mouse);
        self.header(canvas, view, &rect, &hover);
        let (x0, x1) = (SIDEBAR + MARGIN, view.width - MARGIN);
        match self.page {
            Page::Home => self.home(canvas, view, settings, update, &rect, &hover),
            Page::Servers => self.servers(canvas, settings, &targets, &rect, &hover, mouse),
            Page::News => self.news_page(canvas, view),
            Page::Mods => {
                for (i, item) in MODS.iter().enumerate() {
                    let r = grid(x0, x1, 3, i);
                    let value = item.value(settings);
                    card(
                        canvas,
                        r,
                        item.title(),
                        item.group(),
                        &value,
                        inside(r, mouse),
                    );
                }
                let y = grid(x0, x1, 3, MODS.len() + 2)[1] + 4.0;
                canvas.system_text(
                    "Move HUD cards in game: Right Shift, then Edit HUD.",
                    x0,
                    y.min(view.height - 26.0),
                    SMALL,
                    Weight::Regular,
                    style::MUTED,
                );
            }
            Page::Settings => {
                for (i, item) in SETTINGS.iter().enumerate() {
                    let r = grid(x0, x1, 2, i);
                    let value = item.value(settings);
                    card(canvas, r, item.title(), "", &value, inside(r, mouse));
                }
                let r = grid(x0, x1, 2, SETTINGS.len());
                let r = [x0, r[1], x1, r[3]];
                style::panel(canvas, r, style::CARD, style::EDGE);
                canvas.system_text(
                    "Updates",
                    r[0] + 10.0,
                    r[1] + 7.0,
                    BODY,
                    Weight::Semibold,
                    style::TEXT,
                );
                canvas.system_text(
                    &update.label(),
                    r[0] + 10.0,
                    r[1] + 18.0,
                    SMALL,
                    Weight::Regular,
                    style::MUTED,
                );
                if matches!(update, Status::Ready(_))
                    && let Some(restart) = rect(Target::Restart)
                {
                    button(canvas, restart, "Restart", hover(Target::Restart));
                }
            }
        }
        if let Some(message) = &self.message {
            let y = view.height - 12.0;
            canvas.rect(x0, y, x0 + 2.0, y + SMALL + 1.0, style::ACCENT);
            canvas.system_text(
                message,
                x0 + 6.0,
                y,
                SMALL + 0.5,
                Weight::Regular,
                style::TEXT,
            );
        }
        canvas.scale = 1.0;
    }

    fn sidebar(&self, canvas: &mut Canvas<'_>, view: View, mouse: (f32, f32)) {
        canvas.rect(0.0, 0.0, SIDEBAR, view.height, style::SURFACE);
        let t = style::hairline(canvas);
        canvas.rect(SIDEBAR - t, 0.0, SIDEBAR, view.height, style::EDGE);
        // The mark: an H in corner brackets.
        let mark = [SIDEBAR / 2.0 - 8.0, 13.0, SIDEBAR / 2.0 + 8.0, 29.0];
        let hw = canvas.system_width("H", 10.0, Weight::Semibold);
        canvas.system_text(
            "H",
            (SIDEBAR - hw) / 2.0,
            16.0,
            10.0,
            Weight::Semibold,
            style::TEXT,
        );
        style::brackets(canvas, mark, style::TEXT);
        for (i, page) in Page::ALL.iter().enumerate() {
            let r = nav(i);
            let selected = *page == self.page;
            let over = inside(r, mouse);
            if selected {
                canvas.rect(r[0], r[1], r[2], r[3], style::CARD);
                style::brackets(canvas, r, style::ACCENT);
            } else if over {
                canvas.rect(r[0], r[1], r[2], r[3], style::HOVER);
            }
            let color = if selected || over {
                style::TEXT
            } else {
                style::MUTED
            };
            let px = 1.25;
            let x = (r[0] + r[2] - 8.0 * px) / 2.0;
            icon(canvas, page.icon(), x, r[1] + 7.0, px, color);
            let label = page.label();
            let lw = canvas.system_width(label, 5.0, Weight::Regular);
            canvas.system_text(
                label,
                (r[0] + r[2] - lw) / 2.0,
                r[1] + 21.0,
                5.0,
                Weight::Regular,
                color,
            );
        }
    }

    fn header(
        &self,
        canvas: &mut Canvas<'_>,
        view: View,
        rect: &dyn Fn(Target) -> Option<[f32; 4]>,
        hover: &dyn Fn(Target) -> bool,
    ) {
        let (x0, x1) = (SIDEBAR + MARGIN, view.width - MARGIN);
        let account = self.account();
        if self.page == Page::Home {
            let (line, name) = match &account {
                Account::SignedIn(session) => ("Welcome back", session.profile.username.clone()),
                _ => ("Welcome to Horizon", "Sign in to play online".to_owned()),
            };
            canvas.system_text(line, x0, 11.0, 6.0, Weight::Regular, style::MUTED);
            canvas.system_text(&name, x0, 19.0, TITLE, Weight::Semibold, style::TEXT);
        } else {
            canvas.system_text(
                self.page.label(),
                x0,
                15.0,
                TITLE,
                Weight::Semibold,
                style::TEXT,
            );
        }
        let Some(chip) = rect(Target::Account) else {
            return;
        };
        let right_text = |canvas: &mut Canvas<'_>, text: &str, right: f32, color: u32| {
            let w = canvas.system_width(text, 6.0, Weight::Regular);
            canvas.system_text(text, right - w, 17.5, 6.0, Weight::Regular, color);
        };
        match &account {
            Account::Checking => right_text(canvas, "Checking account…", x1, style::MUTED),
            Account::SignedOut => button(canvas, chip, "Sign in", hover(Target::Account)),
            Account::Failed(reason) => {
                right_text(canvas, reason, chip[0] - 8.0, style::MUTED);
                button(canvas, chip, "Retry", hover(Target::Account));
            }
            Account::Waiting { uri, code } => {
                let site = uri
                    .trim_start_matches("https://")
                    .trim_start_matches("www.");
                if let Some(copy) = rect(Target::Copy) {
                    let line = format!("Enter {code} at {site}");
                    right_text(canvas, &line, copy[0] - 8.0, style::TEXT);
                    button(canvas, copy, "Copy code", hover(Target::Copy));
                }
                right_text(canvas, "Waiting…", x1, style::MUTED);
            }
            Account::SignedIn(session) => {
                button(canvas, chip, "Sign out", hover(Target::Account));
                let initial = session
                    .profile
                    .username
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_string();
                let face = [chip[0] - 22.0, chip[1], chip[0] - 6.0, chip[3]];
                style::panel(canvas, face, style::CARD, style::EDGE);
                let w = canvas.system_width(&initial, 8.0, Weight::Semibold);
                canvas.system_text(
                    &initial,
                    (face[0] + face[2] - w) / 2.0,
                    face[1] + 4.0,
                    8.0,
                    Weight::Semibold,
                    style::TEXT,
                );
            }
        }
    }

    fn home(
        &self,
        canvas: &mut Canvas<'_>,
        view: View,
        settings: &Settings,
        update: &Status,
        rect: &dyn Fn(Target) -> Option<[f32; 4]>,
        hover: &dyn Fn(Target) -> bool,
    ) {
        let (x0, x1) = (SIDEBAR + MARGIN, view.width - MARGIN);
        let hero = [x0, TOP, x1, TOP + HERO_H];
        style::panel(canvas, hero, style::CARD, style::EDGE);
        let left = hero[0] + 16.0;
        let muted = |canvas: &mut Canvas<'_>, text: &str, x, y, size| {
            canvas.system_text(text, x, y, size, Weight::Regular, style::MUTED)
        };
        canvas.system_text(
            "MINECRAFT 1.8.9",
            left,
            hero[1] + 16.0,
            SMALL,
            Weight::Semibold,
            style::MUTED,
        );
        canvas.system_text(
            "Horizon",
            left,
            hero[1] + 26.0,
            20.0,
            Weight::Semibold,
            style::TEXT,
        );
        muted(canvas, &update.label(), left, hero[1] + 54.0, 6.0);
        if let Some(play) = rect(Target::Play) {
            play_button(canvas, play, hover(Target::Play));
            if let Some((name, _)) = settings.servers.get(self.selected) {
                let line = match self.state(settings, self.selected) {
                    Some(State::Online(info)) => {
                        format!("{name} · {} online", thousands(info.online))
                    }
                    _ => name.clone(),
                };
                let w = canvas.system_width(&line, SMALL, Weight::Regular);
                muted(
                    canvas,
                    &line,
                    (play[0] + play[2] - w) / 2.0,
                    play[3] + 6.0,
                    SMALL,
                );
            }
        }
        let label = TOP + HERO_H + 14.0;
        canvas.system_text(
            "QUICK PLAY",
            x0,
            label,
            SMALL,
            Weight::Semibold,
            style::MUTED,
        );
        let all = "All servers ›";
        let w = canvas.system_width(all, SMALL, Weight::Regular);
        let color = if hover(Target::AllServers) {
            style::TEXT
        } else {
            style::MUTED
        };
        canvas.system_text(all, x1 - w, label, SMALL, Weight::Regular, color);
        for (i, (name, address)) in settings.servers.iter().enumerate() {
            let Some(r) = rect(Target::Server(i)) else {
                break;
            };
            let fill = if hover(Target::Server(i)) {
                style::HOVER
            } else {
                style::CARD
            };
            style::panel(canvas, r, fill, style::EDGE);
            canvas.system_text(
                name,
                r[0] + 10.0,
                r[1] + 8.0,
                BODY + 0.5,
                Weight::Semibold,
                style::TEXT,
            );
            muted(canvas, address, r[0] + 10.0, r[1] + 19.0, SMALL);
            status_line(canvas, self.state(settings, i), r[0] + 10.0, r[1] + 31.0);
            if i == self.selected {
                style::brackets(canvas, r, style::ACCENT);
            }
        }
        if let (Some(r), Some(entry)) = (rect(Target::LatestNews), self.news.first()) {
            let fill = if hover(Target::LatestNews) {
                style::HOVER
            } else {
                style::SURFACE
            };
            style::panel(canvas, r, fill, style::EDGE);
            let lw = canvas.system_text(
                "WHAT'S NEW",
                r[0] + 10.0,
                r[1] + 10.0,
                SMALL,
                Weight::Semibold,
                style::MUTED,
            );
            canvas.system_text(
                &entry.title,
                r[0] + 18.0 + lw,
                r[1] + 9.5,
                BODY - 0.5,
                Weight::Semibold,
                style::TEXT,
            );
            let w = canvas.system_width("›", BODY, Weight::Regular);
            muted(canvas, "›", r[2] - 10.0 - w, r[1] + 9.0, BODY);
        }
    }

    fn servers(
        &self,
        canvas: &mut Canvas<'_>,
        settings: &Settings,
        targets: &[(Target, [f32; 4])],
        rect: &dyn Fn(Target) -> Option<[f32; 4]>,
        hover: &dyn Fn(Target) -> bool,
        mouse: (f32, f32),
    ) {
        for &(target, r) in targets {
            let Target::Server(i) = target else {
                continue;
            };
            let (name, address) = &settings.servers[i];
            let over = inside(r, mouse);
            let fill = if over { style::HOVER } else { style::CARD };
            style::panel(canvas, r, fill, style::EDGE);
            let ty = r[1] + (ROW_H - BODY) / 2.0;
            let nw = canvas.system_text(name, r[0] + 10.0, ty, BODY, Weight::Semibold, style::TEXT);
            if name != address {
                canvas.system_text(
                    address,
                    r[0] + 18.0 + nw,
                    ty + 1.0,
                    SMALL,
                    Weight::Regular,
                    style::MUTED,
                );
            }
            let state = self.state(settings, i);
            let lw = canvas.system_width(&status_text(state.clone()), SMALL, Weight::Regular) + 6.0;
            status_line(canvas, state, r[2] - 30.0 - lw, ty + 1.0);
            if i == self.selected {
                style::brackets(canvas, r, style::ACCENT);
            }
            if (i == self.selected || over)
                && let Some(x) = rect(Target::Remove(i))
            {
                let color = if hover(Target::Remove(i)) {
                    style::TEXT
                } else {
                    style::MUTED
                };
                canvas.system_text("×", x[0] + 3.0, ty, BODY, Weight::Regular, color);
            }
        }
        if let Some(field) = rect(Target::Field) {
            let edge = if self.field.is_some() {
                style::MUTED
            } else {
                style::EDGE
            };
            style::panel(canvas, field, style::SURFACE, edge);
            let (text, color) = match &self.field {
                Some(text) => (format!("{text}|"), style::TEXT),
                None => ("Add a server address…".to_owned(), style::MUTED),
            };
            canvas.system_text(
                &text,
                field[0] + 8.0,
                field[1] + 7.5,
                BODY - 0.5,
                Weight::Regular,
                color,
            );
        }
        if let Some(add) = rect(Target::Add) {
            button(canvas, add, "Add", hover(Target::Add));
        }
        if let Some(play) = rect(Target::Play) {
            play_button(canvas, play, hover(Target::Play));
        }
    }

    fn news_page(&self, canvas: &mut Canvas<'_>, view: View) {
        let (x0, x1) = (SIDEBAR + MARGIN, view.width - MARGIN);
        let mut y = TOP;
        for (i, entry) in self.news.iter().enumerate() {
            if y > view.height - 30.0 {
                break;
            }
            if i == 0 {
                canvas.rect(x0, y + 1.0, x0 + 2.0, y + 9.0, style::ACCENT);
            }
            canvas.system_text(
                &entry.title,
                x0 + 8.0,
                y,
                8.5,
                Weight::Semibold,
                style::TEXT,
            );
            y += 14.0;
            for line in &entry.lines {
                for (k, part) in wrap(canvas, line, 6.0, x1 - x0 - 16.0).iter().enumerate() {
                    if k == 0 {
                        canvas.rect(x0 + 9.0, y + 2.5, x0 + 11.0, y + 4.5, style::MUTED);
                    }
                    canvas.system_text(part, x0 + 16.0, y, 6.0, Weight::Regular, style::MUTED);
                    y += 9.0;
                }
                y += 2.0;
            }
            y += 10.0;
        }
    }
}

/// `12345` as `12,345`.
fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn status_text(state: Option<State>) -> String {
    match state {
        Some(State::Online(info)) => {
            format!("{} online · {} ms", thousands(info.online), info.ping_ms)
        }
        Some(State::Pinging) => "Pinging…".to_owned(),
        Some(State::Offline) | None => "Can't reach".to_owned(),
    }
}

/// A status dot and its line: solid when online, hollow otherwise.
fn status_line(canvas: &mut Canvas<'_>, state: Option<State>, x: f32, y: f32) {
    let online = matches!(state, Some(State::Online(_)));
    let dot = [x, y + 1.5, x + 3.0, y + 4.5];
    if online {
        canvas.rect(dot[0], dot[1], dot[2], dot[3], style::TEXT);
    } else {
        style::panel(canvas, dot, style::BASE, style::MUTED);
    }
    let color = if online { style::TEXT } else { style::MUTED };
    canvas.system_text(
        &status_text(state),
        x + 6.0,
        y,
        SMALL,
        Weight::Regular,
        color,
    );
}

/// Split `text` into lines no wider than `width` at `size`.
fn wrap(canvas: &Canvas<'_>, text: &str, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && canvas.system_width(&candidate, size, Weight::Regular) > width {
            lines.push(std::mem::replace(&mut line, word.to_owned()));
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Draw an 8x8 pixel icon at `px` units per pixel, one rect per run.
fn icon(canvas: &mut Canvas<'_>, bits: [u8; 8], x: f32, y: f32, px: f32, color: u32) {
    for (row, byte) in bits.iter().enumerate() {
        let mut col = 0;
        while col < 8 {
            if byte & (0x80 >> col) == 0 {
                col += 1;
                continue;
            }
            let start = col;
            while col < 8 && byte & (0x80 >> col) != 0 {
                col += 1;
            }
            let y0 = y + row as f32 * px;
            canvas.rect(
                x + start as f32 * px,
                y0,
                x + col as f32 * px,
                y0 + px,
                color,
            );
        }
    }
}

/// The one accent: ember with a pixel play mark.
fn play_button(canvas: &mut Canvas<'_>, r: [f32; 4], hover: bool) {
    let fill = if hover {
        style::ACCENT_HOVER
    } else {
        style::ACCENT
    };
    canvas.rect(r[0], r[1], r[2], r[3], fill);
    let label = "PLAY";
    let size = 9.0;
    let tw = canvas.system_width(label, size, Weight::Semibold);
    let total = 5.0 + 5.0 + tw;
    let x = ((r[0] + r[2] - total) / 2.0).round();
    let cy = ((r[1] + r[3]) / 2.0).round();
    // A 5x9 pixel triangle.
    for i in 0..9 {
        let w = 5.0 - (i as f32 - 4.0).abs();
        let y = cy - 4.5 + i as f32;
        canvas.rect(x, y, x + w, y + 1.0, style::ON_ACCENT);
    }
    canvas.system_text(
        label,
        x + 10.0,
        cy - size / 2.0,
        size,
        Weight::Semibold,
        style::ON_ACCENT,
    );
}

fn button(canvas: &mut Canvas<'_>, r: [f32; 4], label: &str, hover: bool) {
    let fill = if hover { style::HOVER } else { style::CARD };
    style::panel(canvas, r, fill, style::EDGE);
    let size = 6.0;
    let tw = canvas.system_width(label, size, Weight::Semibold);
    canvas.system_text(
        label,
        (r[0] + r[2] - tw) / 2.0,
        (r[1] + r[3] - size) / 2.0,
        size,
        Weight::Semibold,
        style::TEXT,
    );
}

/// A mod or setting card: title, a muted line, and its switch or value.
fn card(
    canvas: &mut Canvas<'_>,
    r: [f32; 4],
    title: &str,
    group: &str,
    value: &Value,
    hover: bool,
) {
    let fill = if hover { style::HOVER } else { style::CARD };
    style::panel(canvas, r, fill, style::EDGE);
    let ty = if group.is_empty() {
        r[1] + (CARD_H - BODY) / 2.0
    } else {
        r[1] + 7.0
    };
    canvas.system_text(title, r[0] + 10.0, ty, BODY, Weight::Semibold, style::TEXT);
    if !group.is_empty() {
        canvas.system_text(
            group,
            r[0] + 10.0,
            r[1] + 18.0,
            SMALL,
            Weight::Regular,
            style::MUTED,
        );
    }
    let cy = (r[1] + r[3]) / 2.0;
    match value {
        Value::Switch(on) => {
            let s = [r[2] - 28.0, cy - 5.0, r[2] - 10.0, cy + 5.0];
            if *on {
                canvas.rect(s[0], s[1], s[2], s[3], style::TEXT);
                canvas.rect(s[2] - 8.0, s[1] + 2.0, s[2] - 2.0, s[3] - 2.0, style::BASE);
            } else {
                style::panel(canvas, s, style::BASE, style::MUTED);
                canvas.rect(s[0] + 2.0, s[1] + 2.0, s[0] + 8.0, s[3] - 2.0, style::MUTED);
            }
        }
        Value::Choice(text) => {
            let size = 6.5;
            let tw = canvas.system_width(text, size, Weight::Semibold);
            let right = r[2] - 10.0;
            let y = cy - size / 2.0;
            canvas.system_text("›", right - 3.0, y, size, Weight::Regular, style::MUTED);
            canvas.system_text(
                text,
                right - 8.0 - tw,
                y,
                size,
                Weight::Semibold,
                style::TEXT,
            );
            canvas.system_text(
                "‹",
                right - 16.0 - tw,
                y,
                size,
                Weight::Regular,
                style::MUTED,
            );
        }
    }
}

/// Put `text` on the clipboard.
fn copy(text: &str) {
    use std::io::Write;
    if let Ok(mut child) = std::process::Command::new("/usr/bin/pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offline() -> Launcher {
        Launcher::with(Account::SignedOut)
    }

    fn find(targets: &[(Target, [f32; 4])], target: Target) -> [f32; 4] {
        targets
            .iter()
            .find(|(t, _)| *t == target)
            .unwrap_or_else(|| panic!("{target:?}"))
            .1
    }

    /// The GUI-pixel point at the centre of a design-unit rect.
    fn centre(layout: Layout, r: [f32; 4]) -> (f32, f32) {
        let s = view(layout).scale;
        (s * (r[0] + r[2]) / 2.0, s * (r[1] + r[3]) / 2.0)
    }

    #[test]
    fn addresses_parse_hosts_and_ports() {
        assert_eq!(
            address("mc.hypixel.net"),
            Some(("mc.hypixel.net".into(), 25565))
        );
        assert_eq!(
            address(" Play.Example.com:25570 "),
            Some(("play.example.com".into(), 25570))
        );
        assert_eq!(address("localhost"), Some(("localhost".into(), 25565)));
        for bad in ["", "host:0", "host:x", "a b", "-x;rm", ":25565"] {
            assert_eq!(address(bad), None, "{bad:?}");
        }
        assert!(local("localhost") && local("127.0.0.1") && !local("mc.hypixel.net"));
    }

    #[test]
    fn every_page_fits_and_nothing_overlaps() {
        for size in [
            (640, 480),
            (1440, 900),
            (1920, 1200),
            (2880, 1800),
            (3456, 2234),
        ] {
            let layout = Layout::new(size, 2.0, 0);
            let v = view(layout);
            assert!(v.width >= 479.0 && v.height >= 299.0, "{size:?} {v:?}");
            for page in Page::ALL {
                let targets = geometry(page, v, 40, 0);
                for (target, r) in &targets {
                    assert!(
                        r[0] >= 0.0 && r[1] >= 0.0 && r[2] <= v.width && r[3] <= v.height,
                        "{size:?} {page:?} {target:?} {r:?}"
                    );
                }
                // Cards, tiles, rows and nav items never overlap each other.
                let solid: Vec<_> = targets
                    .iter()
                    .filter(|(t, _)| {
                        matches!(
                            t,
                            Target::Nav(_)
                                | Target::Server(_)
                                | Target::Mod(_)
                                | Target::Play
                                | Target::Setting(_, 0)
                        )
                    })
                    .collect();
                for (i, (a, ra)) in solid.iter().enumerate() {
                    for (b, rb) in &solid[i + 1..] {
                        let apart =
                            ra[2] <= rb[0] || rb[2] <= ra[0] || ra[3] <= rb[1] || rb[3] <= ra[1];
                        assert!(apart, "{size:?} {page:?} {a:?} {ra:?} / {b:?} {rb:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_sidebar_switches_pages_and_cards_change_settings() {
        let layout = Layout::new((1920, 1200), 2.0, 0);
        let v = view(layout);
        let mut launcher = offline();
        let mut settings = Settings::default();
        let off = Status::Off;
        let targets = geometry(Page::Home, v, 3, 0);
        let mods = centre(layout, find(&targets, Target::Nav(Page::Mods)));
        launcher.click(layout, mods, &mut settings, &off);
        assert_eq!(launcher.page, Page::Mods);
        // The first Mods card is Keystrokes, on by default.
        assert!(settings.hud(HudKind::Keystrokes).enabled);
        let targets = geometry(Page::Mods, v, 3, 0);
        let keystrokes = centre(layout, find(&targets, Target::Mod(0)));
        launcher.click(layout, keystrokes, &mut settings, &off);
        assert!(!settings.hud(HudKind::Keystrokes).enabled);
        // Settings: the right half of a stepper raises, the left lowers.
        launcher.page = Page::Settings;
        let targets = geometry(Page::Settings, v, 3, 0);
        let fov = settings.video.fov;
        let raise = centre(layout, find(&targets, Target::Setting(0, 1)));
        launcher.click(layout, raise, &mut settings, &off);
        assert_eq!(settings.video.fov, fov + 5.0);
        let lower = centre(layout, find(&targets, Target::Setting(0, -1)));
        launcher.click(layout, lower, &mut settings, &off);
        assert_eq!(settings.video.fov, fov);
        for _ in 0..30 {
            SETTINGS[0].step(&mut settings, 1);
        }
        assert_eq!(settings.video.fov, 110.0);
        let limit = SETTINGS
            .iter()
            .position(|s| *s == Setting::FrameLimit)
            .unwrap();
        for _ in 0..9 {
            SETTINGS[limit].step(&mut settings, 1);
        }
        assert_eq!(settings.toggles.fps_limit, u32::MAX);
        // Restart only answers once an update is installed.
        let restart = centre(layout, find(&targets, Target::Restart));
        let up_to_date = Status::UpToDate;
        assert_eq!(
            launcher.click(layout, restart, &mut settings, &up_to_date),
            None
        );
        let ready = Status::Ready("abc".into());
        assert_eq!(
            launcher.click(layout, restart, &mut settings, &ready),
            Some(Action::Restart)
        );
    }

    #[test]
    fn playing_needs_a_sign_in_except_on_this_mac() {
        let layout = Layout::new((1440, 900), 2.0, 0);
        let mut settings = Settings::default();
        let mut launcher = offline();
        let first = settings.servers[0].1.clone();
        assert!(!local(&address(&first).unwrap().0));
        assert_eq!(launcher.play(&settings), None);
        assert!(launcher.message.is_some());
        let mac = settings
            .servers
            .iter()
            .position(|(_, a)| a == "localhost")
            .unwrap();
        launcher.selected = mac;
        assert_eq!(
            launcher.play(&settings),
            Some(Action::Play("localhost".into(), 25565))
        );
        // On the Servers page, typing an address and pressing Enter adds
        // and selects it.
        launcher.page = Page::Servers;
        let targets = launcher.targets(view(layout), &settings);
        let field = centre(layout, find(&targets, Target::Field));
        launcher.click(layout, field, &mut settings, &Status::Off);
        assert!(launcher.typing());
        use winit::keyboard::KeyCode;
        launcher.key(KeyCode::KeyA, Some("na.minemen.club:25566"), &mut settings);
        launcher.key(KeyCode::Enter, None, &mut settings);
        assert!(!launcher.typing());
        assert_eq!(settings.servers.last().unwrap().1, "na.minemen.club:25566");
        assert_eq!(launcher.selected, settings.servers.len() - 1);
        // A long list scrolls so the selection stays on screen.
        for i in 0..20 {
            settings
                .servers
                .push((format!("s{i}"), format!("s{i}.example.com")));
        }
        launcher.selected = settings.servers.len() - 1;
        let targets = launcher.targets(view(layout), &settings);
        find(&targets, Target::Server(launcher.selected));
    }

    #[test]
    fn news_reads_titles_and_lines_newest_first() {
        let entries = news(include_str!("../NEWS.md"));
        assert!(entries.len() >= 3);
        assert_eq!(entries[0].title, "A launcher of its own");
        assert!(
            entries
                .iter()
                .all(|e| !e.lines.is_empty() && !e.title.is_empty())
        );
        assert_eq!(
            news("# Head\nintro\n## One\n- a\n- b\n## Two\n- c"),
            vec![
                Entry {
                    title: "One".into(),
                    lines: vec!["a".into(), "b".into()],
                },
                Entry {
                    title: "Two".into(),
                    lines: vec!["c".into()],
                },
            ]
        );
        assert_eq!(thousands(41203), "41,203");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }
}
