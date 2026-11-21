//! Persisted client settings: mod toggles, HUD placement and display options,
//! stored as JSON beside the client's other data. Unknown or malformed
//! values fall back to defaults, so a damaged file never blocks a launch.
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// HUD mods, in menu order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudKind {
    Keystrokes,
    Cps,
    Fps,
    Armor,
    Potions,
    Coordinates,
    Ping,
}

impl HudKind {
    pub const ALL: [HudKind; 7] = [
        HudKind::Keystrokes,
        HudKind::Cps,
        HudKind::Fps,
        HudKind::Armor,
        HudKind::Potions,
        HudKind::Coordinates,
        HudKind::Ping,
    ];

    pub fn key(self) -> &'static str {
        match self {
            HudKind::Keystrokes => "keystrokes",
            HudKind::Cps => "cps",
            HudKind::Fps => "fps",
            HudKind::Armor => "armor",
            HudKind::Potions => "potions",
            HudKind::Coordinates => "coordinates",
            HudKind::Ping => "ping",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            HudKind::Keystrokes => "Keystrokes",
            HudKind::Cps => "CPS",
            HudKind::Fps => "FPS",
            HudKind::Armor => "Armor Status",
            HudKind::Potions => "Potion Effects",
            HudKind::Coordinates => "Coordinates",
            HudKind::Ping => "Ping",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&k| k == self).unwrap_or(0)
    }

    /// Default placement as a fraction of the screen (top-left corner).
    fn default_hud(self) -> Hud {
        let (enabled, x, y) = match self {
            HudKind::Keystrokes => (true, 0.01, 0.30),
            HudKind::Cps => (false, 0.01, 0.02),
            HudKind::Fps => (true, 0.01, 0.02),
            HudKind::Armor => (true, 0.86, 0.70),
            HudKind::Potions => (true, 0.01, 0.62),
            HudKind::Coordinates => (false, 0.01, 0.10),
            HudKind::Ping => (true, 0.01, 0.085),
        };
        Hud {
            enabled,
            x,
            y,
            scale: 1.0,
        }
    }
}

/// One HUD mod's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hud {
    pub enabled: bool,
    /// Top-left corner as a fraction of the screen, 0 to 1.
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

/// Gameplay and display toggles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Toggles {
    pub zoom: bool,
    pub smooth_zoom: bool,
    pub toggle_sprint: bool,
    pub toggle_sneak: bool,
    pub fullbright: bool,
    /// Frame cap: 0 follows the display (vsync), otherwise frames per second;
    /// `u32::MAX` is unlimited.
    pub fps_limit: u32,
    /// The sidebar's red score numbers (vanilla shows them).
    pub scoreboard_numbers: bool,
    /// F3+H (`GameSettings.advancedItemTooltips`).
    pub advanced_tooltips: bool,
    /// Vanilla's game menu and options instead of the glass sheets.
    pub vanilla_menu: bool,
    /// Crosshair style: 0 vanilla, 1 gapped cross, 2 dot.
    pub crosshair: u8,
    /// Block outline style (`outline::STYLES`); 0 is vanilla's.
    pub outline: u8,
}

/// Vanilla's video and control options the settings screen edits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Video {
    /// Vertical FOV, 30 to 110 (vanilla's settings range).
    pub fov: f32,
    /// `mouseSensitivity`, 0 to 1 (0.5 shows as 100%).
    pub sensitivity: f32,
    /// Chunks, 2 to 16.
    pub render_distance: u8,
    /// 0 automatic, otherwise the largest allowed scale up to it.
    pub gui_scale: u32,
    pub view_bobbing: bool,
    pub fullscreen: bool,
}

impl Default for Video {
    fn default() -> Self {
        Self {
            fov: 70.0,
            sensitivity: 0.5,
            render_distance: 8,
            gui_scale: 0,
            view_bobbing: true,
            fullscreen: false,
        }
    }
}

/// Crosshair styles the menu cycles through.
pub const CROSSHAIRS: [&str; 3] = ["Vanilla", "Cross", "Dot"];

impl Default for Toggles {
    fn default() -> Self {
        Self {
            zoom: true,
            smooth_zoom: true,
            toggle_sprint: false,
            toggle_sneak: false,
            fullbright: false,
            fps_limit: 0,
            scoreboard_numbers: true,
            advanced_tooltips: false,
            vanilla_menu: false,
            crosshair: 0,
            outline: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub hud: [Hud; 7],
    pub toggles: Toggles,
    /// The launcher's server list: shown name and `host[:port]`.
    pub servers: Vec<(String, String)>,
    pub video: Video,
    path: Option<PathBuf>,
}

fn default_servers() -> Vec<(String, String)> {
    [
        ("Hypixel", "mc.hypixel.net"),
        ("Minemen Club", "na.minemen.club"),
        ("This Mac", "localhost"),
    ]
    .map(|(name, address)| (name.to_owned(), address.to_owned()))
    .to_vec()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hud: HudKind::ALL.map(HudKind::default_hud),
            toggles: Toggles::default(),
            servers: default_servers(),
            video: Video::default(),
            path: None,
        }
    }
}

/// The settings file under the user's Application Support folder.
pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library/Application Support/Horizon 1.8.9/settings.json"))
}

fn unit(value: Option<&Value>, default: f32) -> f32 {
    value
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .filter(|v| v.is_finite())
        .map_or(default, |v| v.clamp(0.0, 1.0))
}

impl Settings {
    pub fn hud(&self, kind: HudKind) -> Hud {
        self.hud[kind.index()]
    }

    pub fn hud_mut(&mut self, kind: HudKind) -> &mut Hud {
        &mut self.hud[kind.index()]
    }

    /// Read `path`, keeping it for `save`; missing or bad values are defaults.
    pub fn load(path: Option<PathBuf>) -> Self {
        let text = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok());
        let mut settings = text.as_deref().map_or_else(Self::default, Self::parse);
        settings.path = path;
        settings
    }

    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
            return settings;
        };
        if let Some(Value::Object(huds)) = root.get("hud") {
            for kind in HudKind::ALL {
                let Some(Value::Object(entry)) = huds.get(kind.key()) else {
                    continue;
                };
                let default = kind.default_hud();
                *settings.hud_mut(kind) = Hud {
                    enabled: entry
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(default.enabled),
                    x: unit(entry.get("x"), default.x),
                    y: unit(entry.get("y"), default.y),
                    scale: entry
                        .get("scale")
                        .and_then(Value::as_f64)
                        .map(|v| v as f32)
                        .filter(|v| v.is_finite())
                        .map_or(default.scale, |v| v.clamp(0.5, 3.0)),
                };
            }
        }
        if let Some(Value::Array(servers)) = root.get("servers") {
            settings.servers = servers
                .iter()
                .filter_map(|entry| {
                    let name = entry.get("name")?.as_str()?;
                    let address = entry.get("address")?.as_str()?;
                    crate::launcher::address(address)?;
                    Some((name.chars().take(48).collect(), address.to_owned()))
                })
                .take(32)
                .collect();
        }
        if let Some(Value::Object(v)) = root.get("video") {
            let d = Video::default();
            let number = |key: &str| v.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());
            let flag =
                |key: &str, default: bool| v.get(key).and_then(Value::as_bool).unwrap_or(default);
            settings.video = Video {
                fov: number("fov").map_or(d.fov, |n| (n as f32).clamp(30.0, 110.0)),
                sensitivity: number("sensitivity")
                    .map_or(d.sensitivity, |n| (n as f32).clamp(0.0, 1.0)),
                render_distance: number("render_distance")
                    .map_or(d.render_distance, |n| n.clamp(2.0, 16.0) as u8),
                gui_scale: number("gui_scale").map_or(d.gui_scale, |n| n.clamp(0.0, 4.0) as u32),
                view_bobbing: flag("view_bobbing", d.view_bobbing),
                fullscreen: flag("fullscreen", d.fullscreen),
            };
        }
        if let Some(Value::Object(t)) = root.get("toggles") {
            let flag =
                |key: &str, default: bool| t.get(key).and_then(Value::as_bool).unwrap_or(default);
            let d = Toggles::default();
            settings.toggles = Toggles {
                zoom: flag("zoom", d.zoom),
                smooth_zoom: flag("smooth_zoom", d.smooth_zoom),
                toggle_sprint: flag("toggle_sprint", d.toggle_sprint),
                toggle_sneak: flag("toggle_sneak", d.toggle_sneak),
                fullbright: flag("fullbright", d.fullbright),
                fps_limit: t
                    .get("fps_limit")
                    .and_then(Value::as_u64)
                    .map_or(d.fps_limit, |v| u32::try_from(v).unwrap_or(u32::MAX)),
                scoreboard_numbers: flag("scoreboard_numbers", d.scoreboard_numbers),
                advanced_tooltips: flag("advanced_tooltips", d.advanced_tooltips),
                vanilla_menu: flag("vanilla_menu", d.vanilla_menu),
                crosshair: t
                    .get("crosshair")
                    .and_then(Value::as_u64)
                    .filter(|&v| v < CROSSHAIRS.len() as u64)
                    .map_or(d.crosshair, |v| v as u8),
                outline: t
                    .get("outline")
                    .and_then(Value::as_u64)
                    .filter(|&v| v < crate::outline::STYLES.len() as u64)
                    .map_or(d.outline, |v| v as u8),
            };
        }
        settings
    }

    pub fn to_json(&self) -> String {
        let mut huds = Map::new();
        for kind in HudKind::ALL {
            let h = self.hud(kind);
            huds.insert(
                kind.key().into(),
                json!({"enabled": h.enabled, "x": h.x, "y": h.y, "scale": h.scale}),
            );
        }
        let t = self.toggles;
        let servers: Vec<Value> = self
            .servers
            .iter()
            .map(|(name, address)| json!({"name": name, "address": address}))
            .collect();
        let root = json!({
            "version": 1,
            "hud": Value::Object(huds),
            "servers": servers,
            "video": {
                "fov": self.video.fov,
                "sensitivity": self.video.sensitivity,
                "render_distance": self.video.render_distance,
                "gui_scale": self.video.gui_scale,
                "view_bobbing": self.video.view_bobbing,
                "fullscreen": self.video.fullscreen,
            },
            "toggles": {
                "zoom": t.zoom,
                "smooth_zoom": t.smooth_zoom,
                "toggle_sprint": t.toggle_sprint,
                "toggle_sneak": t.toggle_sneak,
                "fullbright": t.fullbright,
                "fps_limit": t.fps_limit,
                "scoreboard_numbers": t.scoreboard_numbers,
                "advanced_tooltips": t.advanced_tooltips,
                "vanilla_menu": t.vanilla_menu,
                "crosshair": t.crosshair,
                "outline": t.outline,
            },
        });
        serde_json::to_string_pretty(&root).unwrap_or_default()
    }

    /// Write atomically (temporary file, then rename). Errors are returned
    /// for the caller to report; settings stay in memory either way.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, self.to_json())?;
        std::fs::rename(&temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_reject_bad_values() {
        let mut settings = Settings::default();
        settings.hud_mut(HudKind::Ping).x = 0.5;
        settings.hud_mut(HudKind::Cps).enabled = true;
        settings.toggles.fps_limit = 240;
        settings.toggles.toggle_sprint = true;
        settings.toggles.scoreboard_numbers = false;
        settings.toggles.crosshair = 2;
        settings.toggles.outline = 3;
        settings
            .servers
            .push(("Test".into(), "play.example.com:25570".into()));
        settings.video.fov = 95.0;
        settings.video.render_distance = 12;
        settings.video.view_bobbing = false;
        let parsed = Settings::parse(&settings.to_json());
        assert_eq!(parsed.video, settings.video);
        let wild = Settings::parse(r#"{"video":{"fov":500,"render_distance":1,"gui_scale":9}}"#);
        assert_eq!(
            (
                wild.video.fov,
                wild.video.render_distance,
                wild.video.gui_scale
            ),
            (110.0, 2, 4)
        );
        assert_eq!(parsed.servers, settings.servers);
        assert_eq!(parsed.hud, settings.hud);
        let odd =
            Settings::parse(r#"{"servers":[{"name":"x","address":"bad host"},{"name":"y"}]}"#);
        assert!(odd.servers.is_empty());
        assert_eq!(parsed.toggles, settings.toggles);
        // Damaged files and out-of-range values fall back or clamp.
        assert_eq!(Settings::parse("not json"), Settings::default());
        let odd = Settings::parse(r#"{"hud":{"fps":{"x":7,"y":"no","scale":100}}}"#);
        let fps = odd.hud(HudKind::Fps);
        assert_eq!((fps.x, fps.y, fps.scale), (1.0, 0.02, 3.0));
        let style = Settings::parse(r#"{"toggles":{"crosshair":9,"outline":4}}"#);
        assert_eq!((style.toggles.crosshair, style.toggles.outline), (0, 0));
        assert!(style.toggles.scoreboard_numbers);
    }

    #[test]
    fn save_writes_atomically_and_load_reads_it_back() {
        let dir = std::env::temp_dir().join(format!("h189-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut settings = Settings::load(Some(path.clone()));
        assert_eq!(settings.hud, Settings::default().hud);
        settings.toggles.fullbright = true;
        settings.save().unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        assert!(Settings::load(Some(path)).toggles.fullbright);
        let _ = std::fs::remove_dir_all(dir);
    }
}
