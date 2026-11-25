use horizon_auth::Session;
use std::{
    error::Error,
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

#[derive(Default)]
pub struct Options {
    pub assets: Option<std::path::PathBuf>,
    pub smoke_frames: Option<u64>,
    pub capture: Option<std::path::PathBuf>,
    /// Vanilla `mouseSensitivity`, 0.0 to 1.0 (default 0.5).
    pub sensitivity: f32,
    /// Vertical world FOV, in vanilla's 30..=110 degree settings range.
    pub fov: f32,
    /// Passive FPS and left/right CPS overlay, disabled by default.
    pub pvp_hud: bool,
    /// Start with Graphics: Fast (opaque leaves) instead of the saved setting.
    pub fast_graphics: bool,
    /// Render scale for this run, percent (50 to 100).
    pub render_scale: Option<u8>,
    /// Raw Input (GameController mouse motion) for this run.
    pub raw_input: bool,
    /// Keep Chunks (client-side chunk cache) for this run.
    pub keep_chunks: bool,
    /// Requested view distance in chunks (vanilla default 8 here, 2..=32).
    pub view_distance: u8,
    /// Vanilla `guiScale`: 0 automatic, otherwise the largest allowed scale.
    pub gui_scale: u32,
    /// Validation only: type and send this through the chat screen once in
    /// the world, then reopen the chat so a capture shows it.
    pub smoke_chat: Option<String>,
    /// Validation only: hold the player-list key for the whole smoke run.
    pub smoke_tab: bool,
    /// Validation only: open the inventory and hover the first hotbar slot.
    pub smoke_inventory: bool,
    /// Validation only: open the mod menu once in the world.
    pub smoke_menu: bool,
    /// Validation only: uncapped presentation with per-frame GPU timing.
    pub bench: bool,
    /// `--bench-trace`: the packet-trace benchmark (see `trace_bench`).
    pub bench_trace: bool,
    /// `--watchdog`: the scripted session for the parity suite.
    pub watchdog: bool,
    /// Validation only: look this many degrees down once in the world.
    pub smoke_pitch: Option<f32>,
    /// Create or refresh the app in Applications from this program.
    pub install: bool,
    /// Validation only: show the launcher (signed out, no updates).
    pub smoke_launcher: bool,
    /// Validation only: the launcher page to open, 0 to 4.
    pub smoke_page: usize,
    /// Validation only: open the settings screen once in the world.
    pub smoke_settings: bool,
    /// Validation only: start with the GUI hidden, as after F1.
    pub smoke_hide_gui: bool,
    /// Validation only: start in this F5 perspective (0 to 2).
    pub smoke_perspective: u8,
    /// Validation only: start with the F3 overlay open.
    pub smoke_debug: bool,
    /// Validation only: start with the F3 overlay and Alt+F3's frame-time graph.
    pub smoke_lagometer: bool,
    /// Validation only: start with F3+B hitboxes on.
    pub smoke_hitboxes: bool,
    /// Validation only: open the game menu once in the world.
    pub smoke_pause: bool,
    /// Validation only: use the vanilla-style menus.
    pub smoke_vanilla_menu: bool,
    /// Validation only: open the HUD editor once in the world.
    pub smoke_hud_editor: bool,
    /// Validation only: redraw on the old timer instead of just in time.
    pub no_jit_pacing: bool,
    /// Validation only: take an F2 screenshot into this folder near the end.
    pub smoke_screenshot: Option<std::path::PathBuf>,
    /// With `--install`: the git repository the app updates from.
    pub update_source: Option<String>,
    /// Vanilla `viewBobbing` (on by default).
    pub view_bobbing: bool,
    pub help: bool,
    login: bool,
    /// Keep the Microsoft sign-in in the Keychain between launches.
    save_login: bool,
    sign_out: bool,
    offline: bool,
    username: Option<String>,
    uuid: Option<String>,
    access_token: Option<String>,
    login_check: Option<String>,
    server: Option<String>,
    port: Option<u16>,
    /// `--play`: started by the launcher, which writes the session to stdin.
    play: bool,
    autopilot: bool,
    autopilot_fly: bool,
}

pub enum Startup {
    Window(Option<crate::network::Config>),
    Complete,
}

pub const HELP: &str = "Horizon 1.8.9\n\
    --assets JAR                  Owned 1.8.9 jar for terrain or Metal smoke\n\
    --smoke-frames N              Render N Metal frames and exit\n\
    --login                       Verify Microsoft device-code authentication\n\
    --no-saved-login              Sign in with a device code, without the Keychain\n\
    --sign-out                    Remove the saved Microsoft sign-in and exit\n\
    --offline --username NAME     Use an offline testing identity\n\
    --username NAME --uuid UUID --access-token TOKEN\n\
                                  Use a Minecraft access token for testing\n\
    --server HOST [--port N]       Open a connected native world viewport\n\
    --play HOST[:PORT]             Join from the launcher (session on stdin)\n\
    --login-check HOST [--port N]  Verify Protocol 47 login, then disconnect\n\
    --autopilot                   Drive a scripted walk/sprint/jump/sneak path\n\
    --autopilot-fly               Drive a scripted flight (needs allowFlying)\n\
    --capture PNG                 With --smoke-frames, save the final frame\n\
    --smoke-chat TEXT             With --smoke-frames, send TEXT through the chat screen\n\
    --smoke-tab                   With --smoke-frames, hold the player-list key\n\
    --smoke-inventory             With --smoke-frames, open the inventory over a slot\n\
    --smoke-menu                  With --smoke-frames, open the mod menu\n\
    --bench                       With --smoke-frames, time uncapped CPU and GPU frames\n\
    --bench-trace                 Packet-trace benchmark against horizon-189-trace serve\n\
    --watchdog                    Every module on, scripted session for the parity suite\n\
    --smoke-launcher              With --smoke-frames, show the launcher signed out\n\
    --smoke-page N                With --smoke-launcher, open page N (0 Home to 4 Settings)\n\
    --smoke-settings              With --smoke-frames, open the settings screen\n\
    --smoke-pitch DEGREES         With --smoke-frames, look down (or up, negative) once in the world\n\
    --pvp-hud                     Show passive FPS and left/right CPS\n\
    --fast-graphics               Graphics: Fast (opaque leaves) for this run\n\
    --render-scale PERCENT        World resolution, 50-100% of the window\n\
    --raw-input                   Unaccelerated mouse motion (GameController) for this run\n\
    --keep-chunks                 Keep columns the server unloads inside the render distance\n\
    --fov F                       World vertical FOV 30-110 degrees (default 70)\n\
    --sensitivity F               Mouse sensitivity 0.0-1.0 (vanilla default 0.5)\n\
    --render-distance N           View distance in chunks, 2-32 (default 8)\n\
    --gui-scale N                 Interface scale 0-4 (0 automatic, as vanilla)\n\
    --no-view-bobbing             Turn off view bobbing (vanilla has it on)\n\
    --install [--update-source URL]
                                  Install Horizon 1.8.9.app (updating from URL's main)\n\
    --help                        Show options";

/// The Microsoft client ID (`HORIZON_MSA_CLIENT_ID` overrides the default).
pub(crate) fn client_id() -> String {
    std::env::var("HORIZON_MSA_CLIENT_ID")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| horizon_auth::DEFAULT_CLIENT_ID.to_owned())
}

/// Sign in from the Keychain's refresh token when there is a valid one,
/// otherwise with a device code; then save the newest refresh token. Only
/// fixed markers and status codes are printed, never a token.
fn microsoft_session(client_id: &str, save: bool) -> Result<Session, Box<dyn Error>> {
    if save {
        match horizon_auth::saved::load(client_id) {
            Ok(Some(token)) => match horizon_auth::refresh(client_id, &token) {
                Ok(session) => {
                    eprintln!("MICROSOFT_SAVED_LOGIN_PASS");
                    remember(client_id, &session);
                    return Ok(session);
                }
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    eprintln!("MICROSOFT_SAVED_LOGIN_EXPIRED");
                    if let Err(error) = horizon_auth::saved::forget(client_id) {
                        eprintln!("MICROSOFT_SAVED_LOGIN_UNAVAILABLE {error}");
                    }
                }
                // A network or service failure keeps the saved sign-in for later.
                Err(error) => return Err(error.into()),
            },
            Ok(None) => {}
            Err(error) => eprintln!("MICROSOFT_SAVED_LOGIN_UNAVAILABLE {error}"),
        }
    }
    let code = horizon_auth::begin(client_id)?;
    eprintln!(
        "MICROSOFT_DEVICE_CODE {} {}",
        code.verification_uri, code.user_code
    );
    let session = horizon_auth::finish(client_id, code)?;
    eprintln!("MICROSOFT_AUTH_PASS");
    if save {
        remember(client_id, &session);
    }
    Ok(session)
}

/// The launcher's message to the game it starts: one JSON line on the
/// game's stdin with the profile and, when signed in, the access token, so
/// the token never shows in the process list. Without a token the game
/// joins offline (servers on this Mac only).
pub(crate) fn handoff(session: &Session) -> String {
    let mut value = serde_json::json!({
        "username": session.profile.username,
        "uuid": format!("{:032x}", session.profile.uuid),
    });
    if let Some(token) = session.access_token() {
        value["token"] = token.into();
    }
    value.to_string()
}

/// Read [`handoff`]: the session and whether it is offline.
fn read_handoff(mut input: impl std::io::BufRead) -> Result<(Session, bool), Box<dyn Error>> {
    let mut line = String::new();
    input.read_line(&mut line)?;
    let value: serde_json::Value =
        serde_json::from_str(&line).map_err(|_| "--play expects the launcher's session")?;
    let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
    let username = text("username").ok_or("--play expects a username")?;
    let uuid = text("uuid");
    Ok(match text("token") {
        Some(token) => (
            Session::direct(
                username,
                uuid.ok_or("--play expects a UUID")?,
                token.to_owned(),
            )?,
            false,
        ),
        None => (Session::offline(username, uuid)?, true),
    })
}

pub(crate) fn remember(client_id: &str, session: &Session) {
    let Some(token) = session.refresh_token() else {
        eprintln!("MICROSOFT_LOGIN_NOT_SAVED no refresh token was issued");
        return;
    };
    match horizon_auth::saved::store(client_id, token) {
        Ok(()) => eprintln!("MICROSOFT_LOGIN_SAVED"),
        Err(error) => eprintln!("MICROSOFT_LOGIN_NOT_SAVED {error}"),
    }
}

impl Options {
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut options = Self {
            sensitivity: 0.5,
            fov: 70.0,
            view_distance: 8,
            view_bobbing: true,
            save_login: true,
            ..Self::default()
        };
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let mut value = || arguments.next().ok_or("Argument requires a value");
            match argument.as_str() {
                "--help" | "-h" => options.help = true,
                "--assets" => options.assets = Some(value()?.into()),
                "--login" => options.login = true,
                "--no-saved-login" => options.save_login = false,
                "--sign-out" => options.sign_out = true,
                "--offline" => options.offline = true,
                "--username" => options.username = Some(value()?),
                "--uuid" => options.uuid = Some(value()?),
                "--access-token" => options.access_token = Some(value()?),
                "--login-check" => options.login_check = Some(value()?),
                "--server" => options.server = Some(value()?),
                "--play" => {
                    let (host, port) = crate::launcher::address(&value()?)
                        .ok_or("--play needs a server address")?;
                    options.server = Some(host);
                    options.port = Some(port);
                    options.play = true;
                }
                "--port" => options.port = Some(value()?.parse()?),
                "--autopilot" => options.autopilot = true,
                "--autopilot-fly" => options.autopilot_fly = true,
                "--capture" => options.capture = Some(value()?.into()),
                "--pvp-hud" => options.pvp_hud = true,
                "--fast-graphics" => options.fast_graphics = true,
                "--raw-input" => options.raw_input = true,
                "--keep-chunks" => options.keep_chunks = true,
                "--render-scale" => {
                    let percent: f64 = value()?.parse()?;
                    if !(50.0..=100.0).contains(&percent) {
                        return Err("--render-scale must be between 50 and 100".into());
                    }
                    options.render_scale = Some(crate::upscale::percent(percent));
                }
                "--fov" => {
                    let fov: f32 = value()?.parse()?;
                    if !(30.0..=110.0).contains(&fov) {
                        return Err("--fov must be between 30 and 110 degrees".into());
                    }
                    options.fov = fov;
                }
                "--sensitivity" => {
                    let sensitivity: f32 = value()?.parse()?;
                    if !(0.0..=1.0).contains(&sensitivity) {
                        return Err("--sensitivity must be between 0 and 1".into());
                    }
                    options.sensitivity = sensitivity;
                }
                "--render-distance" => {
                    let chunks: u8 = value()?.parse()?;
                    if !(2..=crate::settings::MAX_RENDER_DISTANCE).contains(&chunks) {
                        return Err("--render-distance must be between 2 and 32".into());
                    }
                    options.view_distance = chunks;
                }
                "--smoke-chat" => options.smoke_chat = Some(value()?),
                "--smoke-tab" => options.smoke_tab = true,
                "--smoke-inventory" => options.smoke_inventory = true,
                "--smoke-menu" => options.smoke_menu = true,
                "--bench" => options.bench = true,
                "--watchdog" => {
                    // Every module on and a scripted session (`watchdog`).
                    options.watchdog = true;
                    options.smoke_frames = Some(100_000_000);
                }
                "--bench-trace" => {
                    // Uncapped frames until the trace server's end marker.
                    options.bench = true;
                    options.bench_trace = true;
                    options.smoke_frames = Some(100_000_000);
                }
                "--install" => options.install = true,
                "--smoke-launcher" => options.smoke_launcher = true,
                "--smoke-page" => options.smoke_page = value()?.parse::<usize>()?.min(4),
                "--smoke-settings" => options.smoke_settings = true,
                "--smoke-hide-gui" => options.smoke_hide_gui = true,
                "--smoke-debug" => options.smoke_debug = true,
                "--smoke-lagometer" => {
                    options.smoke_debug = true;
                    options.smoke_lagometer = true;
                }
                "--smoke-hitboxes" => options.smoke_hitboxes = true,
                "--smoke-pause" => options.smoke_pause = true,
                "--smoke-vanilla-menu" => options.smoke_vanilla_menu = true,
                "--smoke-hud-editor" => options.smoke_hud_editor = true,
                "--no-jit-pacing" => options.no_jit_pacing = true,
                "--smoke-perspective" => {
                    let mode: u8 = value()?.parse()?;
                    if mode > 2 {
                        return Err("--smoke-perspective must be 0, 1 or 2".into());
                    }
                    options.smoke_perspective = mode;
                }
                "--smoke-screenshot" => options.smoke_screenshot = Some(value()?.into()),
                "--update-source" => options.update_source = Some(value()?),
                "--smoke-pitch" => {
                    let pitch: f32 = value()?.parse()?;
                    if !(-90.0..=90.0).contains(&pitch) {
                        return Err("--smoke-pitch must be between -90 and 90".into());
                    }
                    options.smoke_pitch = Some(pitch);
                }
                "--no-view-bobbing" => options.view_bobbing = false,
                "--gui-scale" => {
                    let scale: u32 = value()?.parse()?;
                    if scale > 4 {
                        return Err("--gui-scale must be between 0 and 4".into());
                    }
                    options.gui_scale = scale;
                }
                "--smoke-frames" => {
                    let frames = value()?.parse()?;
                    if frames == 0 {
                        return Err("--smoke-frames must be positive".into());
                    }
                    options.smoke_frames = Some(frames);
                }
                _ => return Err("Unknown argument. Use --help".into()),
            }
        }
        if options.help {
            return Ok(options);
        }
        if options.update_source.is_some() && !options.install {
            return Err("--update-source requires --install".into());
        }
        if options.install
            && (options.server.is_some()
                || options.smoke_frames.is_some()
                || options.login
                || options.offline
                || options.sign_out)
        {
            return Err("--install runs on its own".into());
        }
        if options.sign_out
            && (options.login
                || options.offline
                || !options.save_login
                || options.username.is_some()
                || options.access_token.is_some()
                || options.server.is_some()
                || options.login_check.is_some()
                || options.smoke_frames.is_some())
        {
            return Err("--sign-out runs on its own".into());
        }
        if options.play
            && (options.login
                || options.offline
                || options.access_token.is_some()
                || options.username.is_some()
                || options.login_check.is_some())
        {
            return Err("--play takes its account from the launcher".into());
        }
        if !options.save_login && (options.offline || options.access_token.is_some()) {
            return Err("--no-saved-login applies to Microsoft sign-in only".into());
        }
        if options.smoke_chat.is_some()
            && (options.smoke_frames.is_none() || options.server.is_none())
        {
            return Err("--smoke-chat requires --smoke-frames and --server".into());
        }
        if (options.smoke_tab
            || options.smoke_inventory
            || options.smoke_menu
            || options.bench
            || options.smoke_pitch.is_some()
            || options.smoke_launcher
            || options.smoke_settings
            || options.smoke_hide_gui
            || options.smoke_perspective != 0
            || options.smoke_debug
            || options.smoke_hitboxes
            || options.smoke_pause
            || options.smoke_vanilla_menu
            || options.smoke_hud_editor
            || options.smoke_screenshot.is_some())
            && options.smoke_frames.is_none()
        {
            return Err("Smoke flags and --bench require --smoke-frames".into());
        }
        if options.capture.is_some() && options.smoke_frames.is_none() {
            return Err("--capture requires --smoke-frames".into());
        }
        if (options.autopilot
            || options.autopilot_fly
            || options.watchdog
            || options.smoke_chat.is_some()
            || options.smoke_tab
            || options.smoke_inventory)
            && options.server.as_deref().is_some_and(|host| {
                host != "localhost"
                    && !host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            })
        {
            return Err("Scripted validation requires a loopback server".into());
        }
        if (options.autopilot || options.autopilot_fly || options.watchdog)
            && options.server.is_none()
        {
            return Err("--autopilot requires --server".into());
        }
        if options.assets.is_some() && options.smoke_frames.is_none() && options.server.is_none() {
            return Err("--assets requires a connected viewport or Metal smoke".into());
        }
        if options.offline && (options.login || options.access_token.is_some()) {
            return Err("--offline conflicts with online authentication options".into());
        }
        if options.login && options.access_token.is_some() {
            return Err("Choose device-code or direct-token authentication".into());
        }
        if options.access_token.is_some() && (options.username.is_none() || options.uuid.is_none())
        {
            return Err(
                "Direct-token authentication requires --username, --uuid and --access-token".into(),
            );
        }
        if options.offline && options.username.is_none() {
            return Err("--offline requires --username".into());
        }
        if !options.offline
            && options.access_token.is_none()
            && (options.username.is_some() || options.uuid.is_some())
        {
            return Err("Identity flags require --offline or all three direct-token fields".into());
        }
        if options.port.is_some() && options.login_check.is_none() && options.server.is_none() {
            return Err("--port requires --server or --login-check".into());
        }
        if options.smoke_frames.is_some()
            && (options.login_check.is_some() || (options.login && options.server.is_none()))
        {
            return Err("Metal smoke and login verification run separately".into());
        }
        if options.server.is_some() && options.login_check.is_some() {
            return Err("Choose --server or --login-check".into());
        }
        Ok(options)
    }

    /// No mode flags at all: the installed app opening to its launcher.
    pub fn launcher(&self) -> bool {
        !self.login
            && !self.offline
            && !self.sign_out
            && !self.install
            && self.access_token.is_none()
            && self.login_check.is_none()
            && self.server.is_none()
            && self.smoke_frames.is_none()
    }

    /// Prepare a terminal probe or a session owned by the connected viewport.
    /// Started by the launcher with `--play`.
    pub fn started_by_launcher(&self) -> bool {
        self.play
    }

    pub fn authenticate(&mut self) -> Result<Startup, Box<dyn Error>> {
        if self.sign_out {
            let removed = horizon_auth::saved::forget(&client_id())?;
            eprintln!("MICROSOFT_SIGNED_OUT removed={removed}");
            return Ok(Startup::Complete);
        }
        if !self.login
            && !self.offline
            && self.access_token.is_none()
            && self.login_check.is_none()
            && self.server.is_none()
        {
            return Ok(Startup::Window(None));
        }
        let session = if self.play {
            let (session, offline) = read_handoff(std::io::stdin().lock())?;
            self.offline = offline;
            session
        } else if self.offline {
            Session::offline(
                self.username.as_deref().expect("validated username"),
                self.uuid.as_deref(),
            )?
        } else if let Some(token) = self.access_token.take() {
            Session::direct(
                self.username.as_deref().expect("validated username"),
                self.uuid.as_deref().expect("validated UUID"),
                token,
            )?
        } else {
            microsoft_session(&client_id(), self.save_login)?
        };
        eprintln!(
            "IDENTITY_READY username={} uuid={:032x} offline={}",
            session.profile.username, session.profile.uuid, self.offline
        );
        if let Some(host) = &self.login_check {
            let port = self.port.unwrap_or(25565);
            let mut connected = None;
            let mut last_error = None;
            for address in (host.as_str(), port).to_socket_addrs()? {
                match TcpStream::connect_timeout(&address, Duration::from_secs(5)) {
                    Ok(stream) => {
                        connected = Some(stream);
                        break;
                    }
                    Err(error) => last_error = Some(error),
                }
            }
            let stream = connected.ok_or_else(|| {
                last_error
                    .unwrap_or_else(|| std::io::Error::other("Server resolved to no addresses"))
            })?;
            stream.set_read_timeout(Some(Duration::from_secs(30)))?;
            stream.set_write_timeout(Some(Duration::from_secs(10)))?;
            stream.set_nodelay(true)?;
            let mut connection = horizon_protocol_47::Connection::new(stream);
            let profile = horizon_protocol_47::login::connect(
                &mut connection,
                host,
                port,
                &session.profile.username,
                |hash| session.join(hash),
            )?;
            if !self.offline && profile != session.profile {
                return Err("Server login profile differs from authenticated profile".into());
            }
            eprintln!(
                "PROTOCOL_47_LOGIN_PASS username={} uuid={:032x}",
                profile.username, profile.uuid
            );
            return Ok(Startup::Complete);
        }
        if let Some(host) = &self.server {
            if self.assets.is_none() {
                let home =
                    std::env::var_os("HOME").ok_or("No home directory for default asset lookup")?;
                self.assets = Some(
                    std::path::PathBuf::from(home)
                        .join("Library/Application Support/minecraft/versions/1.8.9/1.8.9.jar"),
                );
            }
            return Ok(Startup::Window(Some(crate::network::Config {
                host: host.clone(),
                port: self.port.unwrap_or(25565),
                session: std::sync::Arc::new(session),
                offline: self.offline,
                autopilot: self.autopilot,
                autopilot_fly: self.autopilot_fly,
                view_distance: self.view_distance,
            })));
        }
        if self.offline {
            Ok(Startup::Window(None))
        } else {
            Ok(Startup::Complete)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_hands_its_session_to_the_game_on_stdin() {
        let online = Session::direct(
            "Bob",
            "0123456789abcdef0123456789abcdef",
            "secret-token".into(),
        )
        .unwrap();
        let line = handoff(&online);
        let (session, offline) = read_handoff(std::io::Cursor::new(format!("{line}\n"))).unwrap();
        assert!(!offline);
        assert_eq!(session.profile, online.profile);
        assert_eq!(session.access_token(), Some("secret-token"));
        // Signed out: an offline profile for a server on this Mac.
        let player = Session::offline("Player", None).unwrap();
        let (session, offline) = read_handoff(std::io::Cursor::new(handoff(&player))).unwrap();
        assert!(offline && session.access_token().is_none());
        assert_eq!(session.profile, player.profile);
        assert!(read_handoff(std::io::Cursor::new("not json")).is_err());
        // --play sets the server and refuses other account flags.
        let options = parse(&["--play", "mc.hypixel.net"]).unwrap();
        assert!(options.play && !options.launcher());
        assert_eq!(options.server.as_deref(), Some("mc.hypixel.net"));
        assert_eq!(options.port, Some(25565));
        assert!(parse(&["--play", "localhost", "--offline"]).is_err());
        assert!(parse(&["--play", "bad host"]).is_err());
    }

    fn parse(args: &[&str]) -> Result<Options, Box<dyn Error>> {
        Options::parse(args.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn install_runs_alone_and_takes_a_source() {
        let options =
            parse(&["--install", "--update-source", "https://example.com/r.git"]).expect("ok");
        assert!(options.install);
        assert!(parse(&["--update-source", "https://example.com/r.git"]).is_err());
        assert!(parse(&["--install", "--server", "localhost"]).is_err());
    }

    #[test]
    fn bench_runs_only_inside_a_bounded_smoke() {
        assert!(parse(&["--bench"]).is_err());
        let options = parse(&["--smoke-frames", "10", "--bench"]).expect("bench smoke");
        assert!(options.bench);
        assert!(parse(&["--smoke-pitch", "30"]).is_err());
        assert!(parse(&["--smoke-frames", "10", "--smoke-pitch", "91"]).is_err());
        let tilted = parse(&["--smoke-frames", "10", "--smoke-pitch", "30"]).expect("pitch");
        assert_eq!(tilted.smoke_pitch, Some(30.0));
    }

    #[test]
    fn scripted_validation_rejects_public_servers() {
        for flag in [
            "--autopilot",
            "--autopilot-fly",
            "--smoke-tab",
            "--smoke-inventory",
        ] {
            assert!(parse(&["--server", "mc.hypixel.net", "--smoke-frames", "10", flag]).is_err());
            assert!(parse(&["--server", "127.0.0.1", "--smoke-frames", "10", flag]).is_ok());
        }
        assert!(
            parse(&[
                "--server",
                "mc.hypixel.net",
                "--smoke-frames",
                "10",
                "--smoke-chat",
                "test"
            ])
            .is_err()
        );
    }

    #[test]
    fn pvp_settings_default_off_and_fov_rejects_nonfinite_or_outside_vanilla_range() {
        let defaults = parse(&[]).unwrap();
        assert!(!defaults.pvp_hud);
        assert_eq!(defaults.fov, 70.0);
        for value in ["NaN", "inf", "-inf", "29.9", "110.1"] {
            assert!(parse(&["--fov", value]).is_err());
        }
        for value in ["30", "70", "90", "110"] {
            assert!(parse(&["--fov", value]).is_ok());
        }
        assert!(!defaults.fast_graphics);
        let tuned = parse(&["--pvp-hud", "--fov", "90", "--fast-graphics"]).unwrap();
        assert!(tuned.pvp_hud && tuned.fast_graphics);
        assert_eq!(tuned.fov, 90.0);
        assert_eq!(defaults.render_scale, None);
        assert!(!defaults.raw_input && parse(&["--raw-input"]).unwrap().raw_input);
        assert_eq!(
            parse(&["--render-scale", "73"]).unwrap().render_scale,
            Some(75)
        );
        for value in ["49", "101", "x"] {
            assert!(parse(&["--render-scale", value]).is_err());
        }
    }

    #[test]
    fn authentication_modes_validate_all_required_flags() {
        assert!(parse(&["--offline", "--username", "Bob"]).is_ok());
        assert!(
            parse(&[
                "--username",
                "Bob",
                "--uuid",
                "b50ad385829d3141a2167e7d7539ba7f",
                "--access-token",
                "secret"
            ])
            .is_ok()
        );
        assert!(parse(&["--username", "Bob", "--access-token", "secret"]).is_err());
        assert!(parse(&["--offline"]).is_err());
        assert!(parse(&["--offline", "--login", "--username", "Bob"]).is_err());
        assert!(
            parse(&[
                "--login-check",
                "localhost",
                "--port",
                "25565",
                "--offline",
                "--username",
                "Bob"
            ])
            .is_ok()
        );
        assert!(parse(&["--smoke-frames", "0"]).is_err());
        assert!(parse(&["--sensitivity", "1.5"]).is_err());
        assert!(parse(&["--render-distance", "40"]).is_err());
        assert_eq!(
            parse(&["--render-distance", "32"]).unwrap().view_distance,
            32
        );
        let tuned = parse(&["--sensitivity", "0.25", "--render-distance", "12"]).unwrap();
        assert_eq!((tuned.sensitivity, tuned.view_distance), (0.25, 12));
        assert_eq!(parse(&[]).unwrap().sensitivity, 0.5);
        assert!(parse(&["--offline", "--username", "Bob", "--autopilot"]).is_err());
        assert!(parse(&["--access-token"]).is_err());
    }

    #[test]
    fn saved_sign_in_flags_stay_with_microsoft_sign_in() {
        assert!(parse(&[]).unwrap().save_login);
        assert!(parse(&["--sign-out"]).unwrap().sign_out);
        assert!(parse(&["--sign-out", "--server", "localhost"]).is_err());
        assert!(parse(&["--sign-out", "--login"]).is_err());
        let fresh = parse(&["--login", "--no-saved-login"]).unwrap();
        assert!(!fresh.save_login);
        assert!(parse(&["--server", "localhost", "--no-saved-login"]).is_ok());
        assert!(parse(&["--offline", "--username", "Bob", "--no-saved-login"]).is_err());
    }
}
