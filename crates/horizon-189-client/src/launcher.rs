//! The launcher screen the installed app opens to: the vanilla title
//! panorama under a liquid-glass card with the Microsoft account, the server
//! list and Play. Sign-in runs on its own thread so the window never waits.
use crate::{
    glass::Panel,
    settings::Settings,
    sysfont::Weight,
    ui::{Canvas, Layout, PANORAMA},
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
    /// Join the selected server: host and port.
    Play(String, u16),
    Restart,
    Quit,
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
enum Target {
    Account,
    Copy,
    Server(usize),
    Remove(usize),
    Field,
    Add,
    Play,
    Quit,
    Update,
}

/// Where everything sits, in GUI pixels.
#[derive(Debug, PartialEq)]
struct Geometry {
    card: [f32; 4],
    targets: Vec<(Target, [f32; 4])>,
    rows: usize,
    message_y: f32,
}

const CARD_W: f32 = 300.0;
const ROW_H: f32 = 22.0;
const PAD: f32 = 12.0;
const TITLE: f32 = 11.0;
const BODY: f32 = 7.5;
const SMALL: f32 = 6.5;
const WHITE: u32 = 0xFFF7_F8FA;
const DIM: u32 = 0xFFC4_CAD4;
const BLUE: u32 = 0xFF6E_B6FF;

fn inside(rect: [f32; 4], (x, y): (f32, f32)) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

fn geometry(layout: Layout, servers: usize) -> Geometry {
    let width = CARD_W.min(layout.width - 16.0).max(200.0);
    let rows = (((layout.height - 172.0) / (ROW_H + 2.0)).floor() as usize).clamp(2, 6);
    let height = 172.0 + rows as f32 * (ROW_H + 2.0) - 2.0 - 24.0;
    let x0 = ((layout.width - width) / 2.0).floor().max(0.0);
    let y0 = ((layout.height - height) / 2.0).floor().max(0.0);
    let x1 = x0 + width;
    let mut targets = vec![
        (
            Target::Update,
            [x1 - PAD - 110.0, y0 + 8.0, x1 - PAD, y0 + 22.0],
        ),
        (
            Target::Account,
            [x1 - PAD - 64.0, y0 + 34.0, x1 - PAD - 6.0, y0 + 50.0],
        ),
        (
            Target::Copy,
            [x1 - PAD - 128.0, y0 + 34.0, x1 - PAD - 70.0, y0 + 50.0],
        ),
    ];
    let list = y0 + 72.0;
    for row in 0..rows.min(servers) {
        let y = list + row as f32 * (ROW_H + 2.0);
        targets.push((
            Target::Remove(row),
            [x1 - PAD - 18.0, y + 4.0, x1 - PAD - 4.0, y + 18.0],
        ));
        targets.push((Target::Server(row), [x0 + PAD, y, x1 - PAD, y + ROW_H]));
    }
    let field = list + rows as f32 * (ROW_H + 2.0) + 4.0;
    targets.push((
        Target::Add,
        [x1 - PAD - 44.0, field, x1 - PAD, field + 18.0],
    ));
    targets.push((
        Target::Field,
        [x0 + PAD, field, x1 - PAD - 50.0, field + 18.0],
    ));
    let footer = field + 38.0;
    let half = (width - 2.0 * PAD - 6.0) / 2.0;
    targets.push((
        Target::Quit,
        [x0 + PAD, footer, x0 + PAD + half * 0.6, footer + 20.0],
    ));
    targets.push((
        Target::Play,
        [x0 + PAD + half * 0.6 + 6.0, footer, x1 - PAD, footer + 20.0],
    ));
    Geometry {
        card: [x0, y0, x1, footer + 20.0 + PAD],
        targets,
        rows,
        message_y: field + 23.0,
    }
}

pub struct Launcher {
    account: Arc<Mutex<Account>>,
    selected: usize,
    /// The address being typed, when the field has focus.
    field: Option<String>,
    message: Option<String>,
    started: std::time::Instant,
}

impl Launcher {
    /// Start with the saved sign-in, checked in the background.
    pub fn new() -> Self {
        let launcher = Self {
            account: Arc::new(Mutex::new(Account::Checking)),
            selected: 0,
            field: None,
            message: None,
            started: std::time::Instant::now(),
        };
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

    /// Validation captures: signed out, never touching the saved sign-in.
    pub fn preview() -> Self {
        Self {
            account: Arc::new(Mutex::new(Account::SignedOut)),
            selected: 0,
            field: None,
            message: None,
            started: std::time::Instant::now(),
        }
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

    pub fn click(
        &mut self,
        layout: Layout,
        mouse: (f32, f32),
        settings: &mut Settings,
        update: &Status,
    ) -> Option<Action> {
        let g = geometry(layout, settings.servers.len());
        let target = g
            .targets
            .iter()
            .find(|(_, rect)| inside(*rect, mouse))
            .map(|(target, _)| *target);
        if target != Some(Target::Field) && target != Some(Target::Add) {
            self.field = None;
        }
        self.message = None;
        match target? {
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
            Target::Quit => return Some(Action::Quit),
            Target::Update => {
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

    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        settings: &Settings,
        update: &Status,
        mouse: (f32, f32),
    ) {
        let (w, h) = (canvas.layout.width, canvas.layout.height);
        panorama(canvas, self.started.elapsed().as_secs_f32());
        canvas.gradient(0.0, 0.0, w, h, 0x3000_0000, 0x7000_0000);
        canvas.overlay();
        let g = geometry(canvas.layout, settings.servers.len());
        let rect = |target| {
            g.targets
                .iter()
                .find(|(t, _)| *t == target)
                .map(|(_, r)| *r)
                .unwrap_or_default()
        };
        let [x0, y0, x1, _] = g.card;
        let mut card = Panel::new(g.card[0], g.card[1], g.card[2], g.card[3]);
        card.radius = 16.0;
        card.shadow = 0.45;
        card.tint = [0.05, 0.06, 0.08, 0.40];
        canvas.glass(card);
        let tw = canvas.system_text(
            "Horizon",
            x0 + PAD,
            y0 + 9.0,
            TITLE,
            Weight::Semibold,
            WHITE,
        );
        canvas.system_text(
            "1.8.9",
            x0 + PAD + tw + 4.0,
            y0 + 9.0,
            TITLE,
            Weight::Regular,
            DIM,
        );
        // Update status, right-aligned; a ready update is a restart button.
        let label = update.label();
        let ready = matches!(update, Status::Ready(_));
        let lw = canvas.system_width(&label, SMALL, Weight::Regular);
        let lx = x1 - PAD - lw;
        if ready {
            let mut pill = Panel::new(lx - 6.0, y0 + 8.0, x1 - PAD + 6.0, y0 + 22.0);
            pill.radius = 7.0;
            pill.shadow = 0.1;
            pill.tint = [0.04, 0.47, 1.0, 0.85];
            canvas.glass(pill);
        }
        let color = match update {
            Status::Failed(_) => 0xFFFF_9F8A,
            Status::Ready(_) => WHITE,
            _ => DIM,
        };
        canvas.system_text(&label, lx, y0 + 11.0, SMALL, Weight::Regular, color);
        // Account row.
        let mut row = Panel::new(x0 + PAD, y0 + 30.0, x1 - PAD, y0 + 54.0);
        row.radius = 8.0;
        row.shadow = 0.1;
        row.tint = [0.12, 0.13, 0.16, 0.42];
        canvas.glass(row);
        let (line, button) = match self.account() {
            Account::Checking => ("Checking your Microsoft account…".to_owned(), None),
            Account::SignedOut => ("Not signed in".to_owned(), Some("Sign In")),
            Account::Failed(reason) => (reason, Some("Retry")),
            Account::Waiting { uri, code } => {
                let site = uri
                    .trim_start_matches("https://")
                    .trim_start_matches("www.");
                (format!("Enter {code} at {site}"), None)
            }
            Account::SignedIn(session) => (session.profile.username.clone(), Some("Sign Out")),
        };
        canvas.system_text(
            &line,
            x0 + PAD + 8.0,
            y0 + 38.0,
            BODY,
            Weight::Semibold,
            WHITE,
        );
        if let Some(text) = button {
            small_button(
                canvas,
                rect(Target::Account),
                text,
                inside(rect(Target::Account), mouse),
            );
        }
        if matches!(self.account(), Account::Waiting { .. }) {
            small_button(canvas, rect(Target::Account), "Waiting", false);
            small_button(
                canvas,
                rect(Target::Copy),
                "Copy Code",
                inside(rect(Target::Copy), mouse),
            );
        }
        canvas.system_text("Servers", x0 + PAD, y0 + 60.0, SMALL, Weight::Semibold, DIM);
        for (index, (name, address)) in settings.servers.iter().enumerate().take(g.rows) {
            let r = rect(Target::Server(index));
            let selected = index == self.selected;
            let mut panel = Panel::new(r[0], r[1], r[2], r[3]);
            panel.radius = 7.0;
            panel.shadow = 0.08;
            panel.tint = if selected {
                [0.04, 0.47, 1.0, 0.55]
            } else if inside(r, mouse) {
                [0.26, 0.28, 0.34, 0.50]
            } else {
                [0.12, 0.13, 0.16, 0.38]
            };
            canvas.glass(panel);
            let ty = r[1] + (ROW_H - BODY) / 2.0;
            canvas.system_text(name, r[0] + 8.0, ty, BODY, Weight::Semibold, WHITE);
            if name != address {
                let aw = canvas.system_width(address, SMALL, Weight::Regular);
                canvas.system_text(
                    address,
                    r[2] - 26.0 - aw,
                    ty + 0.5,
                    SMALL,
                    Weight::Regular,
                    DIM,
                );
            }
            if selected {
                let x = rect(Target::Remove(index));
                canvas.system_text("×", x[0] + 3.0, ty, BODY, Weight::Regular, DIM);
            }
        }
        // Address field and Add.
        let field = rect(Target::Field);
        let mut input = Panel::new(field[0], field[1], field[2], field[3]);
        input.radius = 7.0;
        input.shadow = 0.0;
        input.tint = if self.field.is_some() {
            [0.20, 0.22, 0.28, 0.55]
        } else {
            [0.10, 0.11, 0.14, 0.40]
        };
        canvas.glass(input);
        let (text, color) = match &self.field {
            Some(text) if !text.is_empty() => (format!("{text}|"), WHITE),
            Some(_) => ("|".to_owned(), WHITE),
            None => ("Add a server address…".to_owned(), DIM),
        };
        canvas.system_text(
            &text,
            field[0] + 7.0,
            field[1] + 5.0,
            BODY,
            Weight::Regular,
            color,
        );
        small_button(
            canvas,
            rect(Target::Add),
            "Add",
            inside(rect(Target::Add), mouse),
        );
        if let Some(message) = &self.message {
            canvas.system_text(message, x0 + PAD, g.message_y, SMALL, Weight::Regular, BLUE);
        } else if self.session().is_none() {
            canvas.system_text(
                "Servers on this Mac work without signing in.",
                x0 + PAD,
                g.message_y,
                SMALL,
                Weight::Regular,
                DIM,
            );
        }
        big_button(
            canvas,
            rect(Target::Quit),
            "Quit",
            inside(rect(Target::Quit), mouse),
            false,
        );
        big_button(
            canvas,
            rect(Target::Play),
            "Play",
            inside(rect(Target::Play), mouse),
            true,
        );
    }
}

/// The four side faces of vanilla's title panorama, side by side at the
/// screen's height and drifting slowly left, as the title screen turns.
fn panorama(canvas: &mut Canvas<'_>, seconds: f32) {
    let (w, h) = (canvas.layout.width, canvas.layout.height);
    let face = h.max(w / 2.0);
    let ring = face * 4.0;
    let offset = (seconds * face / 40.0) % ring;
    let scale = face / 256.0;
    let top = (h - face) / 2.0;
    let mut x = -offset;
    let mut index = 0;
    canvas.scale = scale;
    while x < w {
        canvas.sprite(
            PANORAMA + index % 4,
            x / scale,
            top / scale,
            0.0,
            0.0,
            256.0,
            256.0,
            [1.0; 4],
        );
        x += face;
        index += 1;
    }
    canvas.scale = 1.0;
}

fn small_button(canvas: &mut Canvas<'_>, r: [f32; 4], label: &str, hover: bool) {
    let mut panel = Panel::new(r[0], r[1], r[2], r[3]);
    panel.radius = 7.0;
    panel.shadow = 0.1;
    panel.tint = if hover {
        [0.32, 0.35, 0.42, 0.55]
    } else {
        [0.20, 0.22, 0.27, 0.45]
    };
    canvas.glass(panel);
    let tw = canvas.system_width(label, SMALL, Weight::Semibold);
    canvas.system_text(
        label,
        (r[0] + (r[2] - r[0] - tw) / 2.0).floor(),
        r[1] + (r[3] - r[1] - SMALL) / 2.0,
        SMALL,
        Weight::Semibold,
        WHITE,
    );
}

fn big_button(canvas: &mut Canvas<'_>, r: [f32; 4], label: &str, hover: bool, primary: bool) {
    let mut panel = Panel::new(r[0], r[1], r[2], r[3]);
    panel.radius = 10.0;
    panel.shadow = 0.15;
    panel.tint = match (primary, hover) {
        (true, false) => [0.04, 0.47, 1.0, 0.85],
        (true, true) => [0.20, 0.58, 1.0, 0.90],
        (false, false) => [0.20, 0.22, 0.27, 0.45],
        (false, true) => [0.32, 0.35, 0.42, 0.50],
    };
    canvas.glass(panel);
    let tw = canvas.system_width(label, BODY, Weight::Semibold);
    canvas.system_text(
        label,
        (r[0] + (r[2] - r[0] - tw) / 2.0).floor(),
        r[1] + (r[3] - r[1] - BODY) / 2.0,
        BODY,
        Weight::Semibold,
        0xFFFF_FFFF,
    );
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
    fn the_card_fits_and_rows_do_not_overlap() {
        for size in [(640, 480), (1440, 900), (1920, 1200), (2880, 1800)] {
            let layout = Layout::new(size, 1.0, 0);
            let g = geometry(layout, 8);
            assert!(
                g.card[2] <= layout.width + 0.5 && g.card[3] <= layout.height + 0.5,
                "{size:?}"
            );
            let rows: Vec<[f32; 4]> = g
                .targets
                .iter()
                .filter(|(t, _)| matches!(t, Target::Server(_)))
                .map(|(_, r)| *r)
                .collect();
            assert_eq!(rows.len(), g.rows.min(8));
            for pair in rows.windows(2) {
                assert!(pair[0][3] <= pair[1][1]);
            }
        }
    }

    #[test]
    fn playing_needs_a_sign_in_except_on_this_mac() {
        let layout = Layout::new((1440, 900), 1.0, 0);
        let mut settings = Settings::default();
        let mut launcher = Launcher {
            account: Arc::new(Mutex::new(Account::SignedOut)),
            selected: 0,
            field: None,
            message: None,
            started: std::time::Instant::now(),
        };
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
        // Typing an address and pressing Enter adds and selects it.
        let g = geometry(layout, settings.servers.len());
        let field = g
            .targets
            .iter()
            .find(|(t, _)| *t == Target::Field)
            .unwrap()
            .1;
        launcher.click(
            layout,
            (field[0] + 2.0, field[1] + 2.0),
            &mut settings,
            &Status::Off,
        );
        assert!(launcher.typing());
        use winit::keyboard::KeyCode;
        launcher.key(KeyCode::KeyA, Some("na.minemen.club:25566"), &mut settings);
        launcher.key(KeyCode::Enter, None, &mut settings);
        assert!(!launcher.typing());
        assert_eq!(settings.servers.last().unwrap().1, "na.minemen.club:25566");
        assert_eq!(launcher.selected, settings.servers.len() - 1);
    }
}
