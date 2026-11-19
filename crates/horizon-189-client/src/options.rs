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
    /// Requested view distance in chunks (vanilla default 8 here, 2..=16).
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
    /// Vanilla `viewBobbing` (on by default).
    pub view_bobbing: bool,
    pub help: bool,
    login: bool,
    offline: bool,
    username: Option<String>,
    uuid: Option<String>,
    access_token: Option<String>,
    login_check: Option<String>,
    server: Option<String>,
    port: Option<u16>,
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
    --offline --username NAME     Use an offline testing identity\n\
    --username NAME --uuid UUID --access-token TOKEN\n\
                                  Use a Minecraft access token for testing\n\
    --server HOST [--port N]       Open a connected native world viewport\n\
    --login-check HOST [--port N]  Verify Protocol 47 login, then disconnect\n\
    --autopilot                   Drive a scripted walk/sprint/jump/sneak path\n\
    --autopilot-fly               Drive a scripted flight (needs allowFlying)\n\
    --capture PNG                 With --smoke-frames, save the final frame\n\
    --smoke-chat TEXT             With --smoke-frames, send TEXT through the chat screen\n\
    --smoke-tab                   With --smoke-frames, hold the player-list key\n\
    --smoke-inventory             With --smoke-frames, open the inventory over a slot\n\
    --sensitivity F               Mouse sensitivity 0.0-1.0 (vanilla default 0.5)\n\
    --render-distance N           View distance in chunks, 2-16 (default 8)\n\
    --gui-scale N                 Interface scale 0-4 (0 automatic, as vanilla)\n\
    --no-view-bobbing             Turn off view bobbing (vanilla has it on)\n\
    --help                        Show options";

impl Options {
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut options = Self {
            sensitivity: 0.5,
            view_distance: 8,
            view_bobbing: true,
            ..Self::default()
        };
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let mut value = || arguments.next().ok_or("Argument requires a value");
            match argument.as_str() {
                "--help" | "-h" => options.help = true,
                "--assets" => options.assets = Some(value()?.into()),
                "--login" => options.login = true,
                "--offline" => options.offline = true,
                "--username" => options.username = Some(value()?),
                "--uuid" => options.uuid = Some(value()?),
                "--access-token" => options.access_token = Some(value()?),
                "--login-check" => options.login_check = Some(value()?),
                "--server" => options.server = Some(value()?),
                "--port" => options.port = Some(value()?.parse()?),
                "--autopilot" => options.autopilot = true,
                "--autopilot-fly" => options.autopilot_fly = true,
                "--capture" => options.capture = Some(value()?.into()),
                "--sensitivity" => {
                    let sensitivity: f32 = value()?.parse()?;
                    if !(0.0..=1.0).contains(&sensitivity) {
                        return Err("--sensitivity must be between 0 and 1".into());
                    }
                    options.sensitivity = sensitivity;
                }
                "--render-distance" => {
                    let chunks: u8 = value()?.parse()?;
                    if !(2..=16).contains(&chunks) {
                        return Err("--render-distance must be between 2 and 16".into());
                    }
                    options.view_distance = chunks;
                }
                "--smoke-chat" => options.smoke_chat = Some(value()?),
                "--smoke-tab" => options.smoke_tab = true,
                "--smoke-inventory" => options.smoke_inventory = true,
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
        if options.smoke_chat.is_some()
            && (options.smoke_frames.is_none() || options.server.is_none())
        {
            return Err("--smoke-chat requires --smoke-frames and --server".into());
        }
        if (options.smoke_tab || options.smoke_inventory) && options.smoke_frames.is_none() {
            return Err("--smoke-tab and --smoke-inventory require --smoke-frames".into());
        }
        if options.capture.is_some() && options.smoke_frames.is_none() {
            return Err("--capture requires --smoke-frames".into());
        }
        if (options.autopilot || options.autopilot_fly) && options.server.is_none() {
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

    /// Prepare a terminal probe or a session owned by the connected viewport.
    pub fn authenticate(&mut self) -> Result<Startup, Box<dyn Error>> {
        if !self.login
            && !self.offline
            && self.access_token.is_none()
            && self.login_check.is_none()
            && self.server.is_none()
        {
            return Ok(Startup::Window(None));
        }
        let session = if self.offline {
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
            let client_id = std::env::var("HORIZON_MSA_CLIENT_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| horizon_auth::DEFAULT_CLIENT_ID.to_owned());
            let code = horizon_auth::begin(client_id.trim())?;
            eprintln!(
                "MICROSOFT_DEVICE_CODE {} {}",
                code.verification_uri, code.user_code
            );
            let session = horizon_auth::finish(client_id.trim(), code)?;
            eprintln!("MICROSOFT_AUTH_PASS");
            session
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
                session,
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

    fn parse(args: &[&str]) -> Result<Options, Box<dyn Error>> {
        Options::parse(args.iter().map(|value| (*value).to_owned()))
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
        let tuned = parse(&["--sensitivity", "0.25", "--render-distance", "12"]).unwrap();
        assert_eq!((tuned.sensitivity, tuned.view_distance), (0.25, 12));
        assert_eq!(parse(&[]).unwrap().sensitivity, 0.5);
        assert!(parse(&["--offline", "--username", "Bob", "--autopilot"]).is_err());
        assert!(parse(&["--access-token"]).is_err());
    }
}
