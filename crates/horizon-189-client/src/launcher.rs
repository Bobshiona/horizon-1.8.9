//! The launcher: its own window before the game, drawn over the live arena
//! scene (`arena`) in Minecraft's own visual language, kept pixel-crisp:
//! its font, a vertical hotbar of item slots as the page switcher (keys 1-5
//! pick them, like the hotbar), servers in vanilla's tooltip frame with the
//! server list's ping bars, and vanilla buttons. Pages: Home (servers and
//! Play), Servers, News, Mods and Settings. Play starts the game as its own
//! process; the launcher keeps the account, the server list, the settings
//! and the updates. Sign-in runs on its own thread so the window never
//! waits.
use crate::{
    settings::{CROSSHAIRS, HudKind, Settings},
    status::{Pinger, State},
    terrain::Registry,
    ui::{Canvas, ICONS, Layout, WIDGETS},
    updater::Status,
};
use horizon_auth::Session;
use horizon_protocol_47::nbt::Item;
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

    /// The item in this page's hotbar slot: a bed, a compass, a book,
    /// redstone and a comparator.
    fn item(self) -> i16 {
        match self {
            Page::Home => 355,
            Page::Servers => 345,
            Page::News => 340,
            Page::Mods => 331,
            Page::Settings => 404,
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
    NewsBack,
    NewsNext,
    Mod(usize),
    /// A setting and the step: -1 or 1 for the halves of a stepper's
    /// button, 0 for a switch.
    Setting(usize, i8),
    Restart,
}

/// The launcher's design space, at least 480x300 units. One unit is a whole
/// number of physical pixels, so Minecraft's font and sprites stay crisp.
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    scale: f32,
    width: f32,
    height: f32,
}

fn view(layout: Layout) -> View {
    let factor = layout.factor.max(0.25) as f32;
    let (pw, ph) = (layout.width * factor, layout.height * factor);
    let pixels = (pw / 480.0).min(ph / 300.0).floor().max(1.0);
    let scale = pixels / factor;
    View {
        scale,
        width: layout.width / scale,
        height: layout.height / scale,
    }
}

const LEFT: f32 = 16.0;
const TOP: f32 = 52.0;
/// The right-hand column's width.
const COLUMN: f32 = 200.0;
const CARD_H: f32 = 38.0;
const ROW_H: f32 = 28.0;
/// Mods and settings rows, and their buttons' widths.
const LINE_H: f32 = 24.0;
const MOD_BUTTON: f32 = 46.0;
const SETTING_BUTTON: f32 = 66.0;

fn inside(rect: [f32; 4], (x, y): (f32, f32)) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// A hotbar slot and its label.
fn nav(index: usize) -> [f32; 4] {
    let y = TOP + 6.0 + index as f32 * 28.0;
    [LEFT, y, LEFT + 96.0, y + 22.0]
}

/// Server rows that fit on the Servers page.
fn rows(view: View) -> usize {
    (((view.height - 104.0 - TOP + 4.0) / (ROW_H + 4.0)).floor() as usize).max(1)
}

/// The first row shown so that `selected` is visible.
fn first_row(selected: usize, rows: usize) -> usize {
    (selected + 1).saturating_sub(rows)
}

/// The panel the Mods and Settings rows sit in.
fn sheet(view: View) -> [f32; 4] {
    let x1 = view.width - LEFT;
    [x1 - 340.0, TOP, x1, TOP + 8.0 * LINE_H + 14.0]
}

/// Row `index` of a two-column list in `sheet`: the whole row, and its
/// `button`-wide button on the right.
fn line(sheet: [f32; 4], index: usize, rows: usize, button: f32) -> ([f32; 4], [f32; 4]) {
    let (column, row) = (index / rows, index % rows);
    let width = (sheet[2] - sheet[0] - 18.0) / 2.0;
    let x = sheet[0] + 7.0 + column as f32 * (width + 4.0);
    let y = sheet[1] + 8.0 + row as f32 * LINE_H;
    let row = [x, y, x + width, y + 20.0];
    (row, [row[2] - button, y, row[2], y + 20.0])
}

/// Everything clickable on `page`, in hit-test order.
fn geometry(page: Page, view: View, servers: usize, first: usize) -> Vec<(Target, [f32; 4])> {
    let x1 = view.width - LEFT;
    let x0 = x1 - COLUMN;
    let mut targets: Vec<(Target, [f32; 4])> = Page::ALL
        .iter()
        .enumerate()
        .map(|(i, &page)| (Target::Nav(page), nav(i)))
        .collect();
    targets.push((Target::Account, [x1 - 64.0, 14.0, x1, 34.0]));
    targets.push((Target::Copy, [x1 - 136.0, 14.0, x1 - 70.0, 34.0]));
    match page {
        Page::Home => {
            let mut y = TOP;
            for i in 0..servers.min(3) {
                targets.push((Target::Server(i), [x0, y, x1, y + CARD_H]));
                y += CARD_H + 6.0;
            }
            targets.push((Target::Play, [x0, y + 4.0, x1, y + 44.0]));
            targets.push((Target::AllServers, [x1 - 76.0, y + 50.0, x1, y + 62.0]));
            let bottom = view.height - 14.0;
            targets.push((
                Target::LatestNews,
                [LEFT, bottom - 22.0, LEFT + 230.0, bottom - 10.0],
            ));
        }
        Page::Servers => {
            let x0 = x1 - COLUMN - 60.0;
            for (k, i) in (first..servers).take(rows(view)).enumerate() {
                let y = TOP + k as f32 * (ROW_H + 4.0);
                targets.push((Target::Remove(i), [x1 - 18.0, y + 4.0, x1 - 6.0, y + 16.0]));
                targets.push((Target::Server(i), [x0, y, x1, y + ROW_H]));
            }
            let bottom = view.height - 14.0;
            targets.push((Target::Play, [x0, bottom - 40.0, x1, bottom]));
            targets.push((Target::Add, [x1 - 50.0, bottom - 68.0, x1, bottom - 48.0]));
            targets.push((Target::Field, [x0, bottom - 68.0, x1 - 56.0, bottom - 48.0]));
        }
        Page::News => {
            let bottom = view.height - 14.0;
            targets.push((
                Target::NewsBack,
                [x1 - 50.0, bottom - 20.0, x1 - 26.0, bottom],
            ));
            targets.push((Target::NewsNext, [x1 - 24.0, bottom - 20.0, x1, bottom]));
        }
        Page::Mods => {
            let s = sheet(view);
            for i in 0..MODS.len() {
                targets.push((Target::Mod(i), line(s, i, 8, MOD_BUTTON).0));
            }
        }
        Page::Settings => {
            let s = sheet(view);
            for (i, setting) in SETTINGS.iter().enumerate() {
                let (row, button) = line(s, i, 4, SETTING_BUTTON);
                if setting.switch() {
                    targets.push((Target::Setting(i, 0), row));
                } else {
                    let mid = (button[0] + button[2]) / 2.0;
                    targets.push((Target::Setting(i, -1), [row[0], row[1], mid, row[3]]));
                    targets.push((Target::Setting(i, 1), [mid, row[1], row[2], row[3]]));
                }
            }
            let y = s[1] + 8.0 + 4.0 * LINE_H + 10.0;
            targets.push((Target::Restart, [s[2] - 66.0, y, s[2] - 7.0, y + 20.0]));
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
    news_page: usize,
    started: std::time::Instant,
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
            news_page: 0,
            started: std::time::Instant::now(),
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

    /// The open page.
    pub fn page(&self) -> Page {
        self.page
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
            Target::LatestNews => {
                self.page = Page::News;
                self.news_page = 0;
            }
            Target::NewsBack => self.news_page = self.news_page.saturating_sub(1),
            Target::NewsNext => {
                self.news_page = (self.news_page + 1).min(self.news.len().saturating_sub(1));
            }
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
            // The hotbar's keys pick pages.
            KeyCode::Digit1
            | KeyCode::Digit2
            | KeyCode::Digit3
            | KeyCode::Digit4
            | KeyCode::Digit5 => {
                let digit = typed.and_then(|t| t.chars().next()?.to_digit(10));
                if let Some(page) = digit.and_then(|d| Page::ALL.get(d as usize - 1)) {
                    self.page = *page;
                }
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
        registry: &Registry,
        skin: &dyn Fn(u128) -> Option<u32>,
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
        mc(canvas, "Bob Client", LEFT, 14.0, 3.0, WHITE);
        for (i, page) in Page::ALL.iter().enumerate() {
            let r = nav(i);
            let selected = *page == self.page;
            canvas.sprite(WIDGETS, r[0], r[1], 60.0, 0.0, 22.0, 22.0, [1.0; 4]);
            if selected {
                canvas.sprite(
                    WIDGETS,
                    r[0] - 1.0,
                    r[1] - 1.0,
                    0.0,
                    22.0,
                    24.0,
                    24.0,
                    [1.0; 4],
                );
            }
            crate::hud::item_slot(canvas, registry, &item(page.item()), r[0] + 3.0, r[1] + 3.0);
            let color = if selected || inside(r, mouse) {
                WHITE
            } else {
                0xFFD0_D0D0
            };
            mc(canvas, page.label(), r[0] + 30.0, r[1] + 7.0, 1.0, color);
            mc(
                canvas,
                &(i + 1).to_string(),
                r[0] + 16.0,
                r[1] + 13.0,
                0.5,
                GRAY,
            );
        }
        self.header(canvas, view, skin, &rect, &hover);
        match self.page {
            Page::Home => self.home(canvas, view, registry, settings, update, &rect, &hover),
            Page::Servers => self.servers(canvas, registry, settings, &targets, &rect, mouse),
            Page::News => self.news_page(canvas, view, &rect, &hover),
            Page::Mods => {
                let s = sheet(view);
                tooltip(canvas, s);
                for (i, entry) in MODS.iter().enumerate() {
                    let (row, button) = line(s, i, 8, MOD_BUTTON);
                    let label = match entry.value(settings) {
                        Value::Switch(true) => "ON".to_owned(),
                        Value::Switch(false) => "OFF".to_owned(),
                        Value::Choice(text) => text,
                    };
                    let on = !matches!(entry.value(settings), Value::Switch(false));
                    let color = if on { WHITE } else { GRAY };
                    mc(
                        canvas,
                        entry.title(),
                        row[0] + 3.0,
                        row[1] + 6.0,
                        1.0,
                        color,
                    );
                    vanilla_button(canvas, button, &label, inside(row, mouse), 1.0);
                }
            }
            Page::Settings => {
                let s = sheet(view);
                let bottom = s[1] + 8.0 + 4.0 * LINE_H + 40.0;
                tooltip(canvas, [s[0], s[1], s[2], bottom]);
                for (i, setting) in SETTINGS.iter().enumerate() {
                    let (row, button) = line(s, i, 4, SETTING_BUTTON);
                    let label = match setting.value(settings) {
                        Value::Switch(true) => "ON".to_owned(),
                        Value::Switch(false) => "OFF".to_owned(),
                        Value::Choice(text) => format!("< {text} >"),
                    };
                    mc(
                        canvas,
                        setting.title(),
                        row[0] + 3.0,
                        row[1] + 6.0,
                        1.0,
                        WHITE,
                    );
                    vanilla_button(canvas, button, &label, inside(row, mouse), 1.0);
                }
                let y = s[1] + 8.0 + 4.0 * LINE_H + 10.0;
                canvas.rect(s[0] + 7.0, y - 5.0, s[2] - 7.0, y - 4.0, 0x6050_5070);
                mc(canvas, &update.label(), s[0] + 10.0, y + 6.0, 1.0, GRAY);
                if matches!(update, Status::Ready(_))
                    && let Some(restart) = rect(Target::Restart)
                {
                    vanilla_button(canvas, restart, "Restart", hover(Target::Restart), 1.0);
                }
            }
        }
        if let Some(message) = &self.message {
            let width = mc_width(canvas, message, 1.0);
            let x = view.width - LEFT - width;
            let y = view.height - 12.0;
            tooltip(
                canvas,
                [x - 6.0, y - 6.0, view.width - LEFT + 4.0, y + 12.0],
            );
            mc(canvas, message, x, y, 1.0, YELLOW);
        }
        canvas.scale = 1.0;
    }

    fn header(
        &self,
        canvas: &mut Canvas<'_>,
        view: View,
        skin: &dyn Fn(u128) -> Option<u32>,
        rect: &dyn Fn(Target) -> Option<[f32; 4]>,
        hover: &dyn Fn(Target) -> bool,
    ) {
        let x1 = view.width - LEFT;
        let Some(chip) = rect(Target::Account) else {
            return;
        };
        let right = |canvas: &mut Canvas<'_>, text: &str, right: f32, color: u32| {
            let w = mc_width(canvas, text, 1.0);
            mc(canvas, text, right - w, 20.0, 1.0, color);
        };
        match self.account() {
            Account::Checking => right(canvas, "Checking your account...", x1, GRAY),
            Account::SignedOut => {
                right(canvas, "Signed out", chip[0] - 8.0, GRAY);
                vanilla_button(canvas, chip, "Sign in", hover(Target::Account), 1.0);
            }
            Account::Failed(reason) => {
                right(canvas, &reason, chip[0] - 8.0, GRAY);
                vanilla_button(canvas, chip, "Retry", hover(Target::Account), 1.0);
            }
            Account::Waiting { uri, code } => {
                let site = uri
                    .trim_start_matches("https://")
                    .trim_start_matches("www.");
                if let Some(copy) = rect(Target::Copy) {
                    right(
                        canvas,
                        &format!("Enter {code} at {site}"),
                        copy[0] - 8.0,
                        WHITE,
                    );
                    vanilla_button(canvas, copy, "Copy code", hover(Target::Copy), 1.0);
                }
                vanilla_button(canvas, chip, "Waiting", false, 1.0);
            }
            Account::SignedIn(session) => {
                vanilla_button(canvas, chip, "Sign out", hover(Target::Account), 1.0);
                let name = &session.profile.username;
                let w = mc_width(canvas, name, 1.0);
                let x = chip[0] - 8.0 - w;
                mc(canvas, name, x, 20.0, 1.0, WHITE);
                if let Some(layer) = skin(session.profile.uuid) {
                    let base = canvas.scale;
                    canvas.scale = base * 2.0;
                    canvas.skin_face(layer, (x - 20.0) / 2.0, 16.0 / 2.0);
                    canvas.scale = base;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn home(
        &self,
        canvas: &mut Canvas<'_>,
        view: View,
        registry: &Registry,
        settings: &Settings,
        update: &Status,
        rect: &dyn Fn(Target) -> Option<[f32; 4]>,
        hover: &dyn Fn(Target) -> bool,
    ) {
        for (i, (name, address)) in settings.servers.iter().enumerate() {
            let Some(r) = rect(Target::Server(i)) else {
                break;
            };
            self.server_card(
                canvas,
                r,
                i,
                name,
                address,
                settings,
                hover(Target::Server(i)),
            );
        }
        if let Some(play) = rect(Target::Play) {
            vanilla_button(canvas, play, "", hover(Target::Play), 2.0);
            let label = match settings.servers.get(self.selected) {
                Some((name, _)) => format!("Play {name}"),
                None => "Play".to_owned(),
            };
            let w = mc_width(canvas, &label, 2.0) + 22.0;
            let x = ((play[0] + play[2] - w) / 2.0).round();
            crate::hud::item_slot(canvas, registry, &item(276), x, play[1] + 12.0);
            let color = if hover(Target::Play) { YELLOW } else { WHITE };
            mc(canvas, &label, x + 22.0, play[1] + 13.0, 2.0, color);
        }
        if let Some(r) = rect(Target::AllServers) {
            let color = if hover(Target::AllServers) {
                WHITE
            } else {
                GRAY
            };
            mc(canvas, "All servers >", r[0], r[1] + 2.0, 1.0, color);
        }
        if let (Some(r), Some(entry)) = (rect(Target::LatestNews), self.news.first()) {
            let color = if hover(Target::LatestNews) {
                YELLOW
            } else {
                WHITE
            };
            mc(
                canvas,
                &format!("New: {}", entry.title),
                r[0],
                r[1] + 2.0,
                1.0,
                color,
            );
        }
        mc(canvas, &update.label(), LEFT, view.height - 12.0, 1.0, GRAY);
    }

    /// A server in vanilla's tooltip frame: name, players and the server
    /// list's ping bars; the selected one in a white frame.
    #[allow(clippy::too_many_arguments)]
    fn server_card(
        &self,
        canvas: &mut Canvas<'_>,
        r: [f32; 4],
        index: usize,
        name: &str,
        address: &str,
        settings: &Settings,
        hovered: bool,
    ) {
        let selected = index == self.selected;
        if selected {
            frame(canvas, r, 0xF010_0010, WHITE, 0xFFA0_A0A0);
        } else {
            tooltip(canvas, r);
        }
        let name_color = if hovered || selected {
            WHITE
        } else {
            0xFFE0_E0E0
        };
        mc(canvas, name, r[0] + 7.0, r[1] + 6.0, 1.0, name_color);
        let state = self.state(settings, index);
        let line = match &state {
            Some(State::Online(info)) => format!("{} playing", thousands(info.online)),
            Some(State::Pinging) => "Pinging...".to_owned(),
            _ => "Can't reach".to_owned(),
        };
        let lower = if r[3] - r[1] > 30.0 {
            r[1] + 22.0
        } else {
            r[1] + 16.0
        };
        let mut x = r[0] + 7.0;
        if r[3] - r[1] <= 30.0 && name != address {
            x += mc_width(canvas, name, 1.0) + 8.0;
            mc(canvas, address, x, r[1] + 6.0, 1.0, DARK_GRAY);
            x = r[0] + 7.0;
        }
        mc(canvas, &line, x, lower, 1.0, GRAY);
        // `GuiServerList` ping icon from icons.png.
        let (u, v) = match &state {
            Some(State::Online(info)) => {
                let bars = match info.ping_ms {
                    0..150 => 0.0,
                    150..300 => 1.0,
                    300..600 => 2.0,
                    600..1000 => 3.0,
                    _ => 4.0,
                };
                (0.0, 176.0 + bars * 8.0)
            }
            Some(State::Pinging) => {
                let frame = (self.started.elapsed().as_millis() / 100 % 8) as u16;
                let step = if frame > 4 { 8 - frame } else { frame };
                (10.0, 176.0 + f32::from(step) * 8.0)
            }
            _ => (0.0, 216.0),
        };
        canvas.sprite(ICONS, r[2] - 17.0, r[1] + 6.0, u, v, 10.0, 8.0, [1.0; 4]);
        if let Some(State::Online(info)) = &state {
            let ms = format!("{} ms", info.ping_ms);
            let w = mc_width(canvas, &ms, 1.0);
            mc(canvas, &ms, r[2] - 7.0 - w, lower, 1.0, DARK_GRAY);
        }
    }

    fn servers(
        &self,
        canvas: &mut Canvas<'_>,
        registry: &Registry,
        settings: &Settings,
        targets: &[(Target, [f32; 4])],
        rect: &dyn Fn(Target) -> Option<[f32; 4]>,
        mouse: (f32, f32),
    ) {
        for &(target, r) in targets {
            let Target::Server(i) = target else {
                continue;
            };
            let (name, address) = &settings.servers[i];
            self.server_card(canvas, r, i, name, address, settings, inside(r, mouse));
            if (i == self.selected || inside(r, mouse))
                && let Some(x) = rect(Target::Remove(i))
            {
                let color = if inside(x, mouse) { 0xFFFF_5555 } else { GRAY };
                mc(canvas, "x", x[0] + 3.0, r[3] - 12.0, 1.0, color);
            }
        }
        if let Some(field) = rect(Target::Field) {
            // `GuiTextField`: a grey border round a black box.
            let edge = if self.field.is_some() {
                WHITE
            } else {
                0xFFA0_A0A0
            };
            canvas.rect(
                field[0] - 1.0,
                field[1] - 1.0,
                field[2] + 1.0,
                field[3] + 1.0,
                edge,
            );
            canvas.rect(field[0], field[1], field[2], field[3], 0xFF00_0000);
            let (text, color) = match &self.field {
                Some(text) => (format!("{text}_"), 0xFFE0_E0E0),
                None => ("Add a server address".to_owned(), DARK_GRAY),
            };
            mc(canvas, &text, field[0] + 4.0, field[1] + 6.0, 1.0, color);
        }
        if let Some(add) = rect(Target::Add) {
            vanilla_button(canvas, add, "Add", inside(add, mouse), 1.0);
        }
        if let Some(play) = rect(Target::Play) {
            let over = inside(play, mouse);
            vanilla_button(canvas, play, "", over, 2.0);
            let w = mc_width(canvas, "Play", 2.0) + 22.0;
            let x = ((play[0] + play[2] - w) / 2.0).round();
            crate::hud::item_slot(canvas, registry, &item(276), x, play[1] + 12.0);
            mc(
                canvas,
                "Play",
                x + 22.0,
                play[1] + 13.0,
                2.0,
                if over { YELLOW } else { WHITE },
            );
        }
    }

    fn news_page(
        &self,
        canvas: &mut Canvas<'_>,
        view: View,
        rect: &dyn Fn(Target) -> Option<[f32; 4]>,
        hover: &dyn Fn(Target) -> bool,
    ) {
        let x1 = view.width - LEFT;
        let panel = [x1 - 260.0, TOP, x1, view.height - 40.0];
        tooltip(canvas, panel);
        let index = self.news_page.min(self.news.len().saturating_sub(1));
        if let Some(entry) = self.news.get(index) {
            let x = panel[0] + 10.0;
            mc(canvas, &entry.title, x, panel[1] + 10.0, 1.0, YELLOW);
            let mut y = panel[1] + 26.0;
            for line in &entry.lines {
                for (k, part) in wrap(canvas, line, panel[2] - panel[0] - 30.0)
                    .iter()
                    .enumerate()
                {
                    if y > panel[3] - 12.0 {
                        break;
                    }
                    if k == 0 {
                        mc(canvas, "-", x, y, 1.0, GRAY);
                    }
                    mc(canvas, part, x + 10.0, y, 1.0, 0xFFE0_E0E0);
                    y += 10.0;
                }
                y += 4.0;
            }
        }
        let count = self.news.len().max(1);
        let page = format!("Page {} of {count}", index + 1);
        let w = mc_width(canvas, &page, 1.0);
        mc(canvas, &page, x1 - 58.0 - w, view.height - 26.0, 1.0, GRAY);
        if let Some(back) = rect(Target::NewsBack) {
            vanilla_button(canvas, back, "<", hover(Target::NewsBack), 1.0);
        }
        if let Some(next) = rect(Target::NewsNext) {
            vanilla_button(canvas, next, ">", hover(Target::NewsNext), 1.0);
        }
    }
}

const WHITE: u32 = 0xFFFF_FFFF;
/// Chat colours: `§7`, `§8` and `§e`.
const GRAY: u32 = 0xFFAA_AAAA;
const DARK_GRAY: u32 = 0xFF55_5555;
const YELLOW: u32 = 0xFFFF_FF55;

fn item(id: i16) -> Item {
    Item {
        id,
        count: 1,
        damage: 0,
        nbt: None,
    }
}

/// Minecraft's font at `size` times, with its drop shadow; returns the
/// width drawn.
fn mc(canvas: &mut Canvas<'_>, text: &str, x: f32, y: f32, size: f32, color: u32) -> f32 {
    let base = canvas.scale;
    canvas.scale = base * size;
    let w = canvas.text(text, x / size, y / size, color, true);
    canvas.scale = base;
    w * size
}

fn mc_width(canvas: &Canvas<'_>, text: &str, size: f32) -> f32 {
    canvas.res.width(text) as f32 * size
}

/// A vanilla button (`GuiButton`) at `size` times.
fn vanilla_button(canvas: &mut Canvas<'_>, r: [f32; 4], label: &str, hover: bool, size: f32) {
    let base = canvas.scale;
    canvas.scale = base * size;
    let s = |v: f32| v / size;
    crate::hud::button_widget(
        canvas,
        label,
        [s(r[0]), s(r[1]), s(r[2] - r[0]), s(r[3] - r[1])],
        true,
        hover,
    );
    canvas.scale = base;
}

/// Vanilla's tooltip box (`GuiScreen.drawHoveringText`): near-black violet
/// with a purple border fading down.
fn tooltip(canvas: &mut Canvas<'_>, r: [f32; 4]) {
    frame(canvas, r, 0xF010_0010, 0x5050_00FF, 0x5028_007F);
}

fn frame(canvas: &mut Canvas<'_>, [x0, y0, x1, y1]: [f32; 4], fill: u32, top: u32, bottom: u32) {
    // The fill leaves the four corner pixels open.
    canvas.rect(x0 + 1.0, y0, x1 - 1.0, y1, fill);
    canvas.rect(x0, y0 + 1.0, x0 + 1.0, y1 - 1.0, fill);
    canvas.rect(x1 - 1.0, y0 + 1.0, x1, y1 - 1.0, fill);
    canvas.rect(x0 + 1.0, y0 + 1.0, x1 - 1.0, y0 + 2.0, top);
    canvas.rect(x0 + 1.0, y1 - 2.0, x1 - 1.0, y1 - 1.0, bottom);
    canvas.gradient(x0 + 1.0, y0 + 2.0, x0 + 2.0, y1 - 2.0, top, bottom);
    canvas.gradient(x1 - 2.0, y0 + 2.0, x1 - 1.0, y1 - 2.0, top, bottom);
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

/// Split `text` into lines no wider than `width` in Minecraft's font.
fn wrap(canvas: &Canvas<'_>, text: &str, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && mc_width(canvas, &candidate, 1.0) > width {
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
    use winit::keyboard::KeyCode;

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
    fn units_are_whole_pixels_and_every_page_fits() {
        for (size, backing, scale) in [
            ((640, 480), 1.0, 0),
            ((1440, 900), 2.0, 0),
            ((1920, 1200), 2.0, 0),
            ((1920, 1200), 2.0, 3),
            ((2880, 1800), 2.0, 0),
            ((3456, 2234), 2.0, 0),
        ] {
            let layout = Layout::new(size, backing, scale);
            let v = view(layout);
            let pixels = v.scale * layout.factor as f32;
            assert!((pixels - pixels.round()).abs() < 1e-4, "{size:?} {pixels}");
            assert!(v.width >= 479.0 && v.height >= 299.0, "{size:?} {v:?}");
            for page in Page::ALL {
                let targets = geometry(page, v, 40, 0);
                for (target, r) in &targets {
                    assert!(
                        r[0] >= 0.0 && r[1] >= 0.0 && r[2] <= v.width && r[3] <= v.height,
                        "{size:?} {page:?} {target:?} {r:?}"
                    );
                }
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
                                | Target::Field
                                | Target::Add
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
    fn the_hotbar_and_number_keys_switch_pages_and_rows_change_settings() {
        let layout = Layout::new((1920, 1200), 2.0, 0);
        let v = view(layout);
        let mut launcher = offline();
        let mut settings = Settings::default();
        let off = Status::Off;
        let targets = geometry(Page::Home, v, 3, 0);
        let mods = centre(layout, find(&targets, Target::Nav(Page::Mods)));
        launcher.click(layout, mods, &mut settings, &off);
        assert_eq!(launcher.page, Page::Mods);
        launcher.key(KeyCode::Digit2, Some("2"), &mut settings);
        assert_eq!(launcher.page, Page::Servers);
        launcher.key(KeyCode::Digit4, Some("4"), &mut settings);
        assert_eq!(launcher.page, Page::Mods);
        // The first Mods row is Keystrokes, on by default.
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
        // and selects it; digits typed there are the address's, not pages.
        launcher.page = Page::Servers;
        let targets = launcher.targets(view(layout), &settings);
        let field = centre(layout, find(&targets, Target::Field));
        launcher.click(layout, field, &mut settings, &Status::Off);
        assert!(launcher.typing());
        launcher.key(KeyCode::KeyA, Some("na.minemen.club:25566"), &mut settings);
        launcher.key(KeyCode::Digit1, Some("1"), &mut settings);
        assert_eq!(launcher.page, Page::Servers);
        launcher.key(KeyCode::Backspace, None, &mut settings);
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
    fn news_reads_titles_and_lines_and_pages_stop_at_the_ends() {
        let entries = news(include_str!("../NEWS.md"));
        assert!(entries.len() >= 3);
        assert_eq!(entries[0].title, "Bob Client, with a launcher of its own");
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
        let layout = Layout::new((1920, 1200), 2.0, 0);
        let mut launcher = offline();
        let mut settings = Settings::default();
        launcher.page = Page::News;
        let targets = geometry(Page::News, view(layout), 3, 0);
        let (back, next) = (
            centre(layout, find(&targets, Target::NewsBack)),
            centre(layout, find(&targets, Target::NewsNext)),
        );
        launcher.click(layout, back, &mut settings, &Status::Off);
        assert_eq!(launcher.news_page, 0);
        for _ in 0..20 {
            launcher.click(layout, next, &mut settings, &Status::Off);
        }
        assert_eq!(launcher.news_page, launcher.news.len() - 1);
        assert_eq!(thousands(41203), "41,203");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }
}
