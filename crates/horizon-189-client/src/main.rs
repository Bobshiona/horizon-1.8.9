//! Native Protocol 47 client entry. World and gameplay integration follow the
//! transport and platform gates recorded in HANDOFF.md.
#![forbid(unsafe_code)]

mod arena;
mod chat;
mod connection;
mod container_input;
mod debug;
mod frame_stats;
mod gameplay;
mod glass;
mod greedy;
mod hand;
mod hardness;
mod hud;
mod hud_mods;
mod inventory;
mod item_data;
mod items;
mod keyboard;
mod launcher;
mod menu;
mod mesh_priority;
mod models;
mod network;
mod options;
mod outline;
mod pacing;
mod particles;
mod pause;
mod pvp_metrics;
mod render_regions;
mod renderer;
mod scene;
#[cfg(test)]
mod scene_tests;
mod scheduling;
mod scoreboard;
mod screenshot;
mod settings;
mod skins;
mod status;
mod style;
mod sysfont;
mod terrain;
mod terrain_bindings;
mod tooltip;
mod trace_bench;
mod ui;
mod updater;
mod view;
mod watchdog;

use horizon_client_platform::is_quit_shortcut_down;

/// mimalloc keeps a heap per thread, so the network, mesh and audio threads never wait on
/// each other's allocations; it counts them in an `alloc-count` build.
#[global_allocator]
static ALLOCATOR: horizon_threads::GameAllocator = horizon_threads::GAME_ALLOCATOR;
use horizon_physics_189::player::{EYE_HEIGHT, Keys};
use horizon_protocol_47::play::PositionLook;
use keyboard::Keyboard;
use renderer::Renderer;
use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

struct App {
    renderer: Option<Renderer>,
    keyboard: Keyboard,
    captured: bool,
    focused: bool,
    auto_captured: bool,
    /// Raw pointer deltas since the last frame, in AppKit points (y down).
    mouse: (f64, f64),
    skip_motion: bool,
    cursor: (f64, f64),
    menu_open: bool,
    sensitivity: f32,
    view_distance: u8,
    frames: u64,
    frame_stats: frame_stats::FrameStats,
    /// Present-to-present times in smoke runs: the frame pacing.
    frame_intervals: frame_stats::FrameStats,
    /// Just-in-time vsync pacing, and (smoke runs) each frame's input age
    /// when shown.
    pacer: pacing::Pacer,
    jit: bool,
    input_ages: frame_stats::FrameStats,
    /// When the pacer asked for the redraw now pending.
    planned: Option<Instant>,
    last_present: Option<Instant>,
    smoke_frames: Option<u64>,
    started: Instant,
    next_frame: Instant,
    /// Redraw cadence, just under the display's refresh period so the vsync
    /// drawable queue sets the rate.
    frame_period: Duration,
    failure: Option<String>,
    assets: Option<std::path::PathBuf>,
    capture: Option<std::path::PathBuf>,
    server: Option<network::Config>,
    connection: connection::Screen,
    network: Option<network::Network>,
    chat: chat::Chat,
    hud: hud::Hud,
    modifiers: ModifiersState,
    clipboard: Option<arboard::Clipboard>,
    gui_scale: u32,
    /// Trackpad pixels not yet worth a wheel notch.
    wheel_pixels: f64,
    smoke_chat: Option<String>,
    /// The player-list key, held with no screen open.
    tab_held: bool,
    smoke_tab: bool,
    smoke_inventory: bool,
    smoke_menu: bool,
    smoke_hud_editor: bool,
    smoke_settings: bool,
    smoke_pause: bool,
    smoke_pitch: Option<f32>,
    bench: bool,
    /// Smoothed CPU cost of a frame for the FPS card.
    frame_time: pvp_metrics::FrameTime,
    bench_gpu: frame_stats::FrameStats,
    bench_cpu: frame_stats::FrameStats,
    bench_wall: frame_stats::FrameStats,
    bench_last: Option<Instant>,
    /// `--watchdog`: the scripted parity session.
    watchdog: Option<watchdog::Watchdog>,
    /// `--bench-trace` statistics, by phase.
    trace_bench: Option<trace_bench::TraceBench>,
    container_pointer: container_input::Pointer,
    particles: particles::Particles,
    /// The last client tick particles advanced to.
    particle_tick: u64,
    player_epoch: u64,
    world_epoch: u64,
    view_bobbing: bool,
    /// F1, F3 and F5.
    view: view::View,
    /// F2: where screenshots go, and the worker's results for chat.
    screenshot_dir: Option<std::path::PathBuf>,
    screenshots: (
        std::sync::mpsc::Sender<Result<String, String>>,
        std::sync::mpsc::Receiver<Result<String, String>>,
    ),
    smoke_screenshot: bool,
    /// `orientCamera`'s block traces.
    camera_rays: horizon_physics_189::raycast::Raycaster,
    /// F3: meshes uploaded in all, the CPU's name (read once), and section
    /// rebuilds over the last second (`RenderChunk.renderChunksUpdated`).
    meshes_applied: u64,
    cpu_name: Option<String>,
    rebuilds: (Instant, u64, u32),
    fov: f32,
    metrics: pvp_metrics::Metrics,
    settings: settings::Settings,
    /// Zoom factor applied to the FOV, eased toward its target.
    zoom: f32,
    zoom_at: Instant,
    /// Toggle Sprint / Toggle Sneak latches.
    sprint_latched: bool,
    /// The fullbright state the terrain was last meshed with.
    fullbright: bool,
    menu: menu::Menu,
    /// The Escape game menu and settings.
    pause: pause::Pause,
    /// The installed app's launcher screen, shown while not in a world.
    launcher: Option<launcher::Launcher>,
    /// The launcher's live scene.
    arena: Option<arena::Arena>,
    updater: Option<updater::Updater>,
    /// Started by the launcher: leaving the game reopens it.
    from_launcher: bool,
    /// Play was pressed during an update: hidden until it is installed.
    closing: bool,
    /// Per-frame block targeting for the outline (no entities: the tick's
    /// own ray says when an entity is aimed at).
    selector: horizon_physics_189::raycast::Raycaster,
    no_entities: horizon_world_189::entities::Entities,
    /// HUD card rectangles from the last frame, for the layout editor.
    hud_rects: Vec<(settings::HudKind, [f32; 4])>,
}

/// What one frame reads from the simulation, under one lock.
struct Snapshot {
    view: Arc<inventory::View>,
    swing: Option<u8>,
    vitals: gameplay::Vitals,
    game_mode: u8,
    experience: (f32, i32),
    hardcore: bool,
    inbox: Vec<network::ChatEvent>,
    scoreboard: Arc<scoreboard::Scoreboard>,
    tab: Arc<scoreboard::TabList>,
    username: String,
    encrypted: bool,
    hand: hand::Tracked,
    effects: Vec<(i32, bool)>,
    own: Option<(u128, Arc<horizon_protocol_47::player_list::Profile>)>,
    entity_id: i32,
    player_epoch: u64,
    world_epoch: u64,
    cracks: Vec<([i32; 3], horizon_world_189::BlockState, u8)>,
    visuals: Vec<particles::Event>,
    breaks: Vec<([i32; 3], horizon_world_189::BlockState)>,
    armor: [Option<horizon_protocol_47::nbt::Item>; 4],
    potions: Vec<(u8, i8, i32)>,
    current: [f64; 3],
    aiming_entity: bool,
    own_body: Option<horizon_world_189::entities::Entity>,
    difficulty: u8,
    time_of_day: i64,
    reduced_debug: bool,
}

impl App {
    fn disconnected(&mut self, event_loop: &ActiveEventLoop, reason: String) {
        if self.smoke_frames.is_some() {
            self.fail(event_loop, reason);
            return;
        }
        eprintln!("NETWORK_DISCONNECTED {reason}");
        self.release_cursor();
        self.network.take();
        if let Some(renderer) = &mut self.renderer {
            renderer.clear_world();
        }
        self.chat = chat::Chat::default();
        self.hud = hud::Hud::default();
        self.particles = particles::Particles::default();
        self.metrics = pvp_metrics::Metrics::default();
        self.player_epoch = 0;
        self.world_epoch = 0;
        self.menu_open = false;
        self.auto_captured = false;
        self.wheel_pixels = 0.0;
        self.connection.lost(reason);
    }

    fn reconnect(&mut self, event_loop: &ActiveEventLoop) {
        if !self.connection.disconnected() {
            return;
        }
        let config = self.server.clone();
        let registry = self.renderer.as_ref().and_then(|r| r.registry.clone());
        if let (Some(config), Some(registry)) = (config, registry) {
            self.connection.connecting();
            eprintln!("NETWORK_RECONNECTING");
            match network::Network::start(config, registry) {
                Ok(network) => self.network = Some(network),
                Err(error) => self.disconnected(event_loop, error.to_string()),
            }
        }
    }

    /// Apply the video settings: FOV, sensitivity and bobbing at once,
    /// render distance to the fog and the server, GUI scale and fullscreen.
    fn apply_video(&mut self) {
        let video = self.settings.video;
        self.fov = video.fov;
        self.sensitivity = video.sensitivity;
        self.view_bobbing = video.view_bobbing;
        self.gui_scale = video.gui_scale;
        if let Some(registry) = self.renderer.as_ref().and_then(|r| r.registry.as_ref())
            && registry.set_fancy_graphics(video.fancy_graphics)
        {
            eprintln!("GRAPHICS fancy={}", video.fancy_graphics);
        }
        if video.render_distance != self.view_distance {
            self.view_distance = video.render_distance;
            if let Some(network) = &self.network
                && let Ok(mut shared) = network.shared.lock()
            {
                shared.view_distance = Some(video.render_distance);
            }
            if let Some(server) = &mut self.server {
                server.view_distance = video.render_distance;
            }
        }
        if let Some(renderer) = &mut self.renderer {
            renderer.set_view_distance(video.render_distance);
            renderer.set_gui_scale(video.gui_scale);
            if renderer.window.fullscreen().is_some() != video.fullscreen {
                renderer.window.set_fullscreen(
                    video
                        .fullscreen
                        .then_some(winit::window::Fullscreen::Borderless(None)),
                );
            }
        }
    }

    fn pause_click(&mut self, event_loop: &ActiveEventLoop, click: pause::Click) {
        match click {
            pause::Click::Changed => {
                self.apply_video();
                self.save_settings();
            }
            pause::Click::Mods => self.menu.toggle(),
            pause::Click::Leave => {
                if self.pause.leave_to_launcher {
                    self.back_to_launcher(event_loop);
                } else {
                    event_loop.exit();
                }
            }
            pause::Click::Resume | pause::Click::Nothing => {}
        }
    }

    /// The launcher is on screen: no world and no connection screen.
    fn launcher_shown(&self) -> bool {
        self.launcher.is_some() && self.network.is_none() && !self.connection.active()
    }

    fn launcher_action(&mut self, event_loop: &ActiveEventLoop, action: Option<launcher::Action>) {
        match action {
            Some(launcher::Action::Play(host, port)) => self.launch_game(event_loop, &host, port),
            Some(launcher::Action::Restart)
                if self.updater.as_ref().is_some_and(updater::Updater::restart) =>
            {
                event_loop.exit();
            }
            Some(launcher::Action::Restart) | None => {}
        }
    }

    /// Play from the launcher: start the game as its own process with the
    /// signed-in account (or an offline "Player" for a server on this Mac)
    /// on its stdin, then close. While an update is still building, the
    /// launcher hides instead and closes once the update is installed.
    fn launch_game(&mut self, event_loop: &ActiveEventLoop, host: &str, port: u16) {
        let session = match self.launcher.as_ref().and_then(launcher::Launcher::session) {
            Some(session) => session,
            None => match horizon_auth::Session::offline("Player", None) {
                Ok(session) => Arc::new(session),
                Err(error) => {
                    self.notice(error.to_string());
                    return;
                }
            },
        };
        if let Err(error) = spawn_game(&format!("{host}:{port}"), &options::handoff(&session)) {
            self.notice(format!("Couldn't start the game: {error}"));
            return;
        }
        eprintln!("LAUNCHER_PLAY");
        if self.updating() {
            self.closing = true;
            if let Some(renderer) = &self.renderer {
                renderer.window.set_visible(false);
            }
        } else {
            event_loop.exit();
        }
    }

    /// An update is being checked or built.
    fn updating(&self) -> bool {
        matches!(
            self.updater.as_ref().map(updater::Updater::status),
            Some(updater::Status::Checking | updater::Status::Building(_))
        )
    }

    fn notice(&mut self, message: String) {
        if let Some(launcher) = &mut self.launcher {
            launcher.notice(message);
        }
    }

    /// The launcher changed a setting: apply display changes and save.
    fn settings_changed(&mut self, before: &settings::Settings) {
        if self.settings.video != before.video {
            self.apply_video();
        }
        self.save_settings();
    }

    /// The connection screen's second button: back to the launcher when it
    /// started this session, otherwise quit.
    fn quit_or_back(&mut self, event_loop: &ActiveEventLoop) {
        if self.connection.back {
            self.back_to_launcher(event_loop);
        } else {
            event_loop.exit();
        }
    }

    fn back_to_launcher(&mut self, event_loop: &ActiveEventLoop) {
        if self.from_launcher {
            reopen_launcher();
            event_loop.exit();
            return;
        }
        if self.network.is_some() {
            self.disconnected(event_loop, String::new());
        }
        self.connection.close();
        self.server = None;
        self.release_cursor();
    }

    /// A key from the window, or from the Watchdog script, which presses
    /// keys exactly where the keyboard does.
    fn key_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
        text: Option<&str>,
    ) {
        // `dispatchKeypresses`: F2 works over any screen.
        if pressed
            && !repeat
            && key == KeyCode::F2
            && let Some(renderer) = &mut self.renderer
        {
            renderer.screenshot = true;
        }
        if self.launcher_shown() {
            if pressed {
                if key == KeyCode::KeyQ && self.modifiers.super_key() {
                    event_loop.exit();
                    return;
                }
                let before = self.settings.clone();
                let action = self
                    .launcher
                    .as_mut()
                    .and_then(|launcher| launcher.key(key, text, &mut self.settings));
                if self.settings != before {
                    self.settings_changed(&before);
                }
                self.launcher_action(event_loop, action);
            }
            return;
        }
        if self.connection.active() {
            if pressed && !repeat {
                match self.connection.key_action(key, self.modifiers.super_key()) {
                    Some(connection::Action::Reconnect) => self.reconnect(event_loop),
                    Some(connection::Action::Quit) => self.quit_or_back(event_loop),
                    None => {}
                }
            }
            return;
        }
        if self.chat.open {
            if !pressed {
                self.keyboard.release(key);
            } else if key == KeyCode::KeyQ
                && self.modifiers.super_key()
                && !self.modifiers.shift_key()
            {
                event_loop.exit();
            } else {
                self.chat_key(key, text);
            }
            return;
        }
        let screen = self.network.as_ref().is_some_and(|network| {
            network
                .shared
                .lock()
                .is_ok_and(|s| s.inventory.open || s.vitals.health <= 0.0)
        });
        if pressed
            && !repeat
            && !screen
            && self.network.is_some()
            && matches!(key, KeyCode::KeyT | KeyCode::Slash)
            && !self.modifiers.super_key()
        {
            self.open_chat(if key == KeyCode::Slash { "/" } else { "" });
            return;
        }
        // Right Shift opens the mod menu over the world; while it
        // or the HUD editor is up it takes every key.
        if pressed && !repeat && key == KeyCode::ShiftRight && !screen && self.network.is_some() {
            self.menu.toggle();
            if !self.menu.active() {
                self.save_settings();
            }
            return;
        }
        if self.menu.active() {
            // Typing searches the open menu; Escape clears the
            // search first, then closes.
            let used = pressed && self.menu.key(key, text);
            if pressed && !used && !repeat && key == KeyCode::Escape {
                self.menu.close();
                self.save_settings();
            }
            if !pressed {
                self.keyboard.release(key);
            }
            return;
        }
        if self.pause.active() {
            if pressed && !repeat && key == KeyCode::Escape {
                self.pause.back();
                self.save_settings();
            }
            if !pressed {
                self.keyboard.release(key);
            }
            return;
        }
        if key == KeyCode::Tab {
            self.tab_held = pressed && !screen;
        }
        if pressed && !repeat {
            let toggles = self.settings.toggles;
            if toggles.toggle_sprint
                && matches!(
                    key,
                    KeyCode::ControlLeft | KeyCode::ControlRight | KeyCode::AltLeft
                )
            {
                self.sprint_latched = !self.sprint_latched;
            }
        }
        if pressed {
            self.keyboard.press(key, repeat);
        } else {
            self.keyboard.release(key);
        }
        if is_quit_shortcut_down(self.keyboard.state(), self.keyboard.held()) {
            event_loop.exit();
            return;
        }
        if pressed && !repeat {
            if self.network.is_some() {
                if key != KeyCode::F3 && self.keyboard.down(KeyCode::F3) {
                    match self.view.combo(key, screen) {
                        Some(view::Combo::ClearChat) => self.chat.clear(),
                        // F3+H saves the options, as vanilla does.
                        Some(view::Combo::AdvancedTooltips) => {
                            let t = &mut self.settings.toggles;
                            t.advanced_tooltips = !t.advanced_tooltips;
                            self.save_settings();
                        }
                        Some(view::Combo::ReloadChunks) => {
                            if let Some(network) = &self.network
                                && let Ok(mut shared) = network.shared.lock()
                            {
                                shared.remesh = true;
                            }
                        }
                        None => {}
                    }
                }
                self.view.key(key, screen);
            }
            if let Some(network) = &self.network
                && let Ok(mut state) = network.shared.lock()
            {
                use gameplay::Action;
                let action = if state.vitals.health <= 0.0 {
                    // Enter stands in for the Respawn button, which
                    // GuiGameOver enables only after twenty ticks.
                    if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter)
                        && self.hud.respawn_ready(self.tick())
                    {
                        Some(Action::Respawn)
                    } else {
                        None
                    }
                } else if state.inventory.open {
                    // GuiContainer.keyTyped: hotbar keys and Q act
                    // on the hovered slot.
                    let hovered = self.renderer.as_ref().and_then(|r| {
                        hud::hovered_slot(&state.inventory, r.layout, self.mouse_gui())
                    });
                    let digit = hotbar_digit(key);
                    match (key, hovered, digit) {
                        (KeyCode::KeyE | KeyCode::Escape, _, _) => {
                            self.container_pointer.clear();
                            Some(Action::CloseInventory)
                        }
                        (_, Some(slot), Some(button)) if state.inventory.cursor.is_none() => {
                            Some(Action::Click {
                                slot,
                                mode: 2,
                                button,
                            })
                        }
                        (KeyCode::KeyQ, Some(slot), _) => Some(Action::Click {
                            slot,
                            mode: 4,
                            button: u8::from(self.modifiers.control_key()),
                        }),
                        _ => None,
                    }
                } else {
                    match key {
                        KeyCode::KeyQ => Some(Action::Drop {
                            all: self.modifiers.control_key(),
                        }),
                        KeyCode::Digit1 => Some(Action::Select(0)),
                        KeyCode::Digit2 => Some(Action::Select(1)),
                        KeyCode::Digit3 => Some(Action::Select(2)),
                        KeyCode::Digit4 => Some(Action::Select(3)),
                        KeyCode::Digit5 => Some(Action::Select(4)),
                        KeyCode::Digit6 => Some(Action::Select(5)),
                        KeyCode::Digit7 => Some(Action::Select(6)),
                        KeyCode::Digit8 => Some(Action::Select(7)),
                        KeyCode::Digit9 => Some(Action::Select(8)),
                        KeyCode::KeyE if !state.inventory.open => Some(Action::OpenInventory),
                        KeyCode::KeyE | KeyCode::Escape if state.inventory.open => {
                            self.container_pointer.clear();
                            Some(Action::CloseInventory)
                        }
                        _ => None,
                    }
                };
                if let Some(action) = action
                    && state.input.actions.len() < 128
                {
                    state.input.actions.push_back(action);
                }
            }
            if key == KeyCode::Escape {
                // GuiIngameMenu, when no other screen is open.
                if !screen && self.network.is_some() {
                    self.pause.open();
                }
                self.release_cursor();
            }
        }
        self.publish_keys();
    }

    /// A mouse button from the window, or from the Watchdog script.
    fn mouse_input(&mut self, event_loop: &ActiveEventLoop, button: MouseButton, pressed: bool) {
        if self.launcher_shown() {
            if pressed
                && button == MouseButton::Left
                && let Some(layout) = self.renderer.as_ref().map(|r| r.layout)
            {
                let mouse = self.mouse_gui();
                let status = self
                    .updater
                    .as_ref()
                    .map_or(updater::Status::Off, updater::Updater::status);
                let before = self.settings.clone();
                let action = self.launcher.as_mut().and_then(|launcher| {
                    launcher.click(layout, mouse, &mut self.settings, &status)
                });
                if self.settings != before {
                    self.settings_changed(&before);
                }
                self.launcher_action(event_loop, action);
            }
            return;
        }
        if self.connection.active() {
            if pressed
                && button == MouseButton::Left
                && let Some(renderer) = &self.renderer
            {
                match self.connection.action_at(renderer.layout, self.mouse_gui()) {
                    Some(connection::Action::Reconnect) => self.reconnect(event_loop),
                    Some(connection::Action::Quit) => self.quit_or_back(event_loop),
                    None => {}
                }
            }
            return;
        }
        if self.pause.active() {
            if button == MouseButton::Left
                && let Some(layout) = self.renderer.as_ref().map(|r| r.layout)
            {
                if pressed {
                    let mouse = self.mouse_gui();
                    let click = self.pause.press(layout, mouse, &mut self.settings);
                    self.pause_click(event_loop, click);
                } else if self.pause.release() {
                    self.save_settings();
                }
            }
            return;
        }
        if self.menu.active() {
            let mouse = self.mouse_gui();
            if button == MouseButton::Left
                && let Some(layout) = self.renderer.as_ref().map(|r| r.layout)
            {
                if !pressed {
                    if self.menu.release() {
                        self.save_settings();
                    }
                } else if self.menu.editing {
                    self.menu.press(mouse, &self.hud_rects);
                } else {
                    let click = self.menu.click(layout, mouse, &mut self.settings);
                    if click != menu::Click::Nothing {
                        self.save_settings();
                    }
                    if click == menu::Click::Disconnect {
                        self.back_to_launcher(event_loop);
                    }
                }
            }
            return;
        }
        if let Some(index) = match button {
            MouseButton::Left => Some(0),
            MouseButton::Right => Some(1),
            _ => None,
        } {
            let eligible = self.focused
                && self.captured
                && !self.chat.open
                && self.network.as_ref().is_some_and(|network| {
                    network
                        .shared
                        .lock()
                        .is_ok_and(|s| !s.inventory.open && s.vitals.health > 0.0)
                });
            self.metrics
                .button(index, pressed, eligible, self.started.elapsed());
        }
        let mouse = self.mouse_gui();
        if self.chat.open {
            if pressed
                && button == MouseButton::Left
                && let Some(registry) = self.renderer.as_ref().and_then(|r| r.registry.clone())
            {
                let height = self.renderer.as_ref().map_or(0.0, |r| r.layout.height);
                let request = self.chat.click(&registry.gui, height, mouse);
                self.chat_request(request);
            }
            return;
        }
        let layout = self.renderer.as_ref().map(|r| r.layout);
        let tick = self.tick();
        let death = layout.and_then(|layout| self.hud.death_button_at(layout, mouse, tick));
        if pressed && button == MouseButton::Left && death == Some(hud::DeathButton::TitleScreen) {
            event_loop.exit();
            return;
        }
        let mut open = false;
        if let Some(network) = &self.network
            && let Ok(mut state) = network.shared.lock()
        {
            open = state.inventory.open || state.vitals.health <= 0.0;
            if open {
                if let Some(layout) = layout {
                    if state.vitals.health <= 0.0 {
                        if pressed
                            && button == MouseButton::Left
                            && death == Some(hud::DeathButton::Respawn)
                            && state.input.actions.len() < 128
                        {
                            state.input.actions.push_back(gameplay::Action::Respawn);
                        }
                    } else if matches!(button, MouseButton::Left | MouseButton::Right)
                        && state.input.actions.len() < 128
                    {
                        // GuiContainer.mouseClicked: a slot, or -999
                        // outside the window to drop the carried stack.
                        let slot =
                            hud::hovered_slot(&state.inventory, layout, mouse).or_else(|| {
                                (!hud::inside_window(&state.inventory, layout, mouse))
                                    .then_some(-999)
                            });
                        let button = u8::from(button == MouseButton::Right);
                        let clicks = if pressed {
                            self.container_pointer.press(
                                &state.inventory,
                                slot,
                                button,
                                self.modifiers.shift_key(),
                                Instant::now(),
                            )
                        } else {
                            self.container_pointer
                                .release(&state.inventory, slot, button)
                        };
                        if !clicks.is_empty() && state.input.actions.len() < 128 {
                            let window = state.inventory.window;
                            state
                                .input
                                .actions
                                .push_back(gameplay::Action::ContainerGesture { window, clicks });
                        }
                    }
                }
            } else if self.captured {
                let action = match button {
                    MouseButton::Left => {
                        state.input.attack = pressed;
                        Some(gameplay::Action::Attack)
                    }
                    MouseButton::Right => {
                        state.input.using = pressed;
                        Some(gameplay::Action::Use)
                    }
                    _ => None,
                };
                if pressed
                    && let Some(action) = action
                    && state.input.actions.len() < 128
                {
                    state.input.actions.push_back(action);
                }
            }
        }
        if !open && !self.captured && pressed && button == MouseButton::Left {
            self.capture_cursor();
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl ToString) {
        self.failure = Some(error.to_string());
        event_loop.exit();
    }

    /// `setIngameFocus`: lock and hide the cursor, discard the warp delta.
    fn capture_cursor(&mut self) {
        let Some(renderer) = &self.renderer else {
            return;
        };
        match renderer.window.set_cursor_grab(CursorGrabMode::Locked) {
            Ok(()) => {
                renderer.window.set_cursor_visible(false);
                self.captured = true;
                self.mouse = (0.0, 0.0);
                self.skip_motion = true;
                eprintln!("cursor_captured=true");
                self.publish_keys();
            }
            Err(error) => eprintln!("cursor_capture_failed={error}"),
        }
    }

    /// `setIngameNotInFocus`: release the cursor and unpress every key, as
    /// `KeyBinding.unPressAllKeys` does when a screen opens.
    /// Per-frame mod state: zoom easing, the frame cap's present mode and
    /// the fullbright remesh.
    fn update_mods(&mut self) {
        let toggles = self.settings.toggles;
        let now = Instant::now();
        let dt = now.duration_since(self.zoom_at).as_secs_f32().min(0.25);
        self.zoom_at = now;
        let target = if toggles.zoom && self.captured && self.keyboard.down(KeyCode::KeyC) {
            0.25
        } else {
            1.0
        };
        self.zoom = if toggles.smooth_zoom {
            self.zoom + (target - self.zoom) * (1.0 - (-dt * 14.0).exp())
        } else {
            target
        };
        if (self.zoom - target).abs() < 1e-3 {
            self.zoom = target;
        }
        self.menu.step(dt, &self.settings);
        self.pause.step(dt);
        let vsync = self.schedule() == pacing::Schedule::Vsync;
        if let Some(renderer) = &mut self.renderer {
            renderer.bench = self.bench;
            if renderer.set_vsync(vsync) {
                self.pacer.reset_phase();
                self.planned = None;
                self.next_frame = now;
            }
        }
        if toggles.fullbright != self.fullbright {
            self.fullbright = toggles.fullbright;
            terrain::FULLBRIGHT.store(self.fullbright, std::sync::atomic::Ordering::Relaxed);
            if let Some(network) = &self.network
                && let Ok(mut shared) = network.shared.lock()
            {
                shared.remesh = true;
            }
        }
    }

    /// How frames are scheduled for the frame limit: benchmarks render back
    /// to back offscreen, smoke runs follow the display, and interactive
    /// frames preserve the requested limit in every window mode.
    fn schedule(&self) -> pacing::Schedule {
        if self.bench {
            return pacing::Schedule::Timer(Duration::ZERO);
        }
        if self.smoke_frames.is_some() {
            return pacing::Schedule::Vsync;
        }
        pacing::schedule(self.settings.toggles.fps_limit)
    }

    /// The wait between redraws for the frame cap.
    fn redraw_period(&self) -> Duration {
        match self.schedule() {
            pacing::Schedule::Vsync => self.frame_period,
            pacing::Schedule::Timer(period) => period,
        }
    }

    fn save_settings(&self) {
        if let Err(error) = self.settings.save() {
            eprintln!("SETTINGS_SAVE_FAILED {error}");
        }
    }

    fn release_cursor(&mut self) {
        if let Some(renderer) = &self.renderer {
            let _ = renderer.window.set_cursor_grab(CursorGrabMode::None);
            renderer.window.set_cursor_visible(true);
        }
        self.container_pointer.clear();
        self.metrics.release_input();
        self.captured = false;
        self.mouse = (0.0, 0.0);
        self.tab_held = false;
        self.keyboard.clear();
        if let Some(network) = &self.network
            && let Ok(mut state) = network.shared.lock()
        {
            state.input.attack = false;
            state.input.using = false;
        }
        self.publish_keys();
    }

    /// `Minecraft.updateCounter` as a continuous tick count.
    fn tick(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 20.0
    }

    /// Wheel motion in vanilla notches; trackpads report pixels.
    fn notches(&mut self, delta: winit::event::MouseScrollDelta) -> i32 {
        match delta {
            winit::event::MouseScrollDelta::LineDelta(_, y) => y.signum() as i32,
            winit::event::MouseScrollDelta::PixelDelta(p) => {
                self.wheel_pixels += p.y;
                let notches = (self.wheel_pixels / 24.0).trunc();
                self.wheel_pixels -= notches * 24.0;
                notches as i32
            }
        }
    }

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }

    fn mouse_gui(&self) -> (f32, f32) {
        self.renderer
            .as_ref()
            .map_or((0.0, 0.0), |r| r.layout.gui(self.cursor))
    }

    fn open_chat(&mut self, text: &str) {
        let tick = self.tick();
        self.chat.open_with(text, tick);
        self.release_cursor();
    }

    fn chat_request(&mut self, request: chat::Request) {
        let outgoing = match request {
            chat::Request::Send { text, .. } => Some(network::Outgoing::Message(text)),
            chat::Request::Complete(text) => Some(network::Outgoing::Complete(text)),
            chat::Request::Copy(url) => {
                if let Some(clipboard) = self.clipboard() {
                    let _ = clipboard.set_text(url.clone());
                }
                if let Some(registry) = self.renderer.as_ref().and_then(|r| r.registry.clone()) {
                    let tick = self.tick();
                    self.chat.notice(
                        &registry.gui,
                        &format!("§7Link copied to the clipboard: {url}"),
                        tick,
                    );
                }
                None
            }
            chat::Request::Closed | chat::Request::None => None,
        };
        if let Some(outgoing) = outgoing
            && let Some(network) = &self.network
            && let Ok(mut state) = network.shared.lock()
            && state.chat_outbox.len() < 64
        {
            state.chat_outbox.push(outgoing);
        }
    }

    /// A key while `GuiChat` is open: Command shortcuts, then the editor.
    fn chat_key(&mut self, key: KeyCode, typed: Option<&str>) {
        let m = self.modifiers;
        if m.super_key() && !m.alt_key() && !m.control_key() {
            match key {
                KeyCode::KeyA => self.chat.select_all(),
                KeyCode::KeyC | KeyCode::KeyX => {
                    let selected = self.chat.selected_text();
                    if !selected.is_empty() {
                        if let Some(clipboard) = self.clipboard() {
                            let _ = clipboard.set_text(selected);
                        }
                        if key == KeyCode::KeyX {
                            self.chat.insert("");
                        }
                    }
                }
                KeyCode::KeyV => {
                    let pasted = self.clipboard().and_then(|c| c.get_text().ok());
                    if let Some(text) = pasted {
                        self.chat.insert(&text);
                    }
                }
                _ => {}
            }
            if matches!(
                key,
                KeyCode::KeyA | KeyCode::KeyC | KeyCode::KeyX | KeyCode::KeyV
            ) {
                return;
            }
        }
        let request = self.chat.key(key, typed, m);
        self.chat_request(request);
    }

    /// Movement keys act only while the cursor is captured (no screen open).
    fn publish_keys(&self) {
        let Some(network) = &self.network else {
            return;
        };
        let keys = if self.captured {
            let mut keys = self.keyboard.keys();
            let toggles = self.settings.toggles;
            if toggles.toggle_sprint {
                keys.sprint = self.sprint_latched;
            }
            keys
        } else {
            Keys::default()
        };
        if let Ok(mut shared) = network.shared.lock() {
            shared.keys = if shared.inventory.open || shared.vitals.health <= 0.0 {
                Keys::default()
            } else {
                keys
            };
        }
    }

    /// `EntityRenderer.updateCameraAndRender`, once per frame: sensitivity
    /// `f = s * 0.6 + 0.2`, factor `f³ * 8`, then `Entity.setAngles`. LWJGL
    /// reports vertical motion upward-positive; AppKit downward-positive.
    fn apply_look(&mut self) {
        let (dx, dy) = std::mem::take(&mut self.mouse);
        if std::mem::take(&mut self.skip_motion) || !self.captured {
            return;
        }
        let Some(network) = &self.network else {
            return;
        };
        let f = self.sensitivity * 0.6 + 0.2;
        // Zoom slows the look with the narrower view.
        let factor = f * f * f * 8.0 * self.zoom;
        let yaw_delta = dx as f32 * factor;
        let pitch_delta = (-dy) as f32 * factor;
        if (yaw_delta == 0.0 && pitch_delta == 0.0)
            || !yaw_delta.is_finite()
            || !pitch_delta.is_finite()
        {
            return;
        }
        if let Ok(mut shared) = network.shared.lock() {
            shared.yaw = (f64::from(shared.yaw) + f64::from(yaw_delta) * 0.15) as f32;
            shared.pitch = ((f64::from(shared.pitch) - f64::from(pitch_delta) * 0.15) as f32)
                .clamp(-90.0, 90.0);
        }
    }

    /// Eye pose and tracked entities at this frame's partial tick.
    #[allow(clippy::type_complexity)]
    fn camera_pose(
        &self,
    ) -> Option<(
        PositionLook,
        f32,
        Arc<Vec<(i32, horizon_world_189::entities::Entity)>>,
    )> {
        let shared = self.network.as_ref()?.shared.lock().ok()?;
        if !shared.positioned {
            return None;
        }
        let partial = shared.ticked_at.map_or(1.0, |at| {
            (at.elapsed().as_secs_f64() / 0.05).clamp(0.0, 1.0)
        });
        let eye = f64::from(if shared.sneaking {
            EYE_HEIGHT - 0.08
        } else {
            EYE_HEIGHT
        });
        let mut position: [f64; 3] = std::array::from_fn(|axis| {
            shared.previous[axis] + (shared.current[axis] - shared.previous[axis]) * partial
        });
        position[1] += eye;
        Some((
            PositionLook {
                position,
                yaw: shared.yaw,
                pitch: shared.pitch,
            },
            partial as f32,
            shared.entities.clone(),
        ))
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Bob Client")
            // Benchmark windows never take desktop input. Physical dimensions
            // keep paired runs independent of display/backing-scale changes.
            .with_visible(!self.bench)
            .with_inner_size(if self.bench {
                winit::dpi::Size::Physical(winit::dpi::PhysicalSize::new(960, 600))
            } else {
                winit::dpi::Size::Logical(LogicalSize::new(960.0, 600.0))
            })
            .with_min_inner_size(LogicalSize::new(320.0, 240.0));
        let result = event_loop
            .create_window(attributes)
            .map_err(|error| error.to_string())
            .and_then(|window| {
                pollster::block_on(Renderer::new(
                    Arc::new(window),
                    self.assets.clone(),
                    self.server.is_some() || self.launcher.is_some(),
                    self.view_distance,
                    self.gui_scale,
                    self.fov,
                ))
                .map_err(|error| error.to_string())
            });
        match result {
            Ok(renderer) => {
                // Meshing starts with the saved Graphics setting.
                if let Some(registry) = &renderer.registry {
                    registry.set_fancy_graphics(self.settings.video.fancy_graphics);
                }
                if let Some(config) = self.server.clone() {
                    let Some(registry) = renderer.registry.clone() else {
                        self.fail(event_loop, "Connected viewport requires owned assets");
                        return;
                    };
                    match network::Network::start(config, registry) {
                        Ok(network) => self.network = Some(network),
                        Err(error) => {
                            self.fail(event_loop, error);
                            return;
                        }
                    }
                }
                if self.smoke_frames.is_some() && !self.bench {
                    renderer.window.focus_window();
                }
                if (self.launcher.is_some() || self.from_launcher) && self.settings.video.fullscreen
                {
                    renderer
                        .window
                        .set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                }
                renderer.window.request_redraw();
                self.frame_period = frame_period(&renderer.window);
                self.pacer.set_period(refresh_period(&renderer.window));
                self.renderer = Some(renderer);
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size);
                    self.frame_period = frame_period(&renderer.window);
                    self.pacer.set_period(refresh_period(&renderer.window));
                }
            }
            // A move can change displays and with them the refresh rate.
            WindowEvent::Moved(_) => {
                if let Some(renderer) = &self.renderer {
                    self.frame_period = frame_period(&renderer.window);
                    self.pacer.set_period(refresh_period(&renderer.window));
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                eprintln!("backing_scale={scale_factor}");
            }
            WindowEvent::Focused(focused) => {
                eprintln!("focused={focused}");
                self.focused = focused;
                if !focused {
                    self.release_cursor();
                }
            }
            WindowEvent::Occluded(true) => self.release_cursor(),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                self.keyboard.modifiers(modifiers.state());
                self.publish_keys();
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                if is_synthetic {
                    return;
                }
                if let PhysicalKey::Code(key) = event.physical_key {
                    self.key_input(
                        event_loop,
                        key,
                        event.state == ElementState::Pressed,
                        event.repeat,
                        event.text.as_deref(),
                    );
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                if self.pause.active()
                    && let Some(layout) = self.renderer.as_ref().map(|r| r.layout)
                {
                    let mouse = self.mouse_gui();
                    if self.pause.moved(layout, mouse, &mut self.settings) == pause::Click::Changed
                    {
                        self.apply_video();
                    }
                }
                if self.menu.editing
                    && let Some(layout) = self.renderer.as_ref().map(|r| r.layout)
                {
                    let mouse = self.mouse_gui();
                    self.menu.moved(layout, mouse, &mut self.settings);
                }
                if let (Some(network), Some(renderer)) = (&self.network, &self.renderer)
                    && let Ok(state) = network.shared.lock()
                    && state.inventory.open
                {
                    self.container_pointer.moved(
                        &state.inventory,
                        hud::hovered_slot(&state.inventory, renderer.layout, self.mouse_gui()),
                    );
                }
            }
            WindowEvent::MouseWheel { .. } if self.connection.active() => {}
            WindowEvent::MouseWheel { delta, .. } if self.chat.open => {
                let notches = self.notches(delta);
                if notches != 0 {
                    self.chat
                        .wheel(f64::from(notches), self.modifiers.shift_key());
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.menu.editing => {
                let notches = self.notches(delta);
                let mouse = self.mouse_gui();
                if self
                    .menu
                    .scroll(mouse, notches, &self.hud_rects, &mut self.settings)
                {
                    self.save_settings();
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.menu.open => {
                let notches = self.notches(delta);
                if let Some(layout) = self.renderer.as_ref().map(|r| r.layout) {
                    self.menu.wheel(layout, notches);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.captured => {
                let amount = f64::from(self.notches(delta));
                if let Some(network) = &self.network
                    && let Ok(mut state) = network.shared.lock()
                    && amount != 0.0
                    && state.input.actions.len() < 128
                {
                    let current = state
                        .input
                        .actions
                        .iter()
                        .rev()
                        .find_map(|a| {
                            if let gameplay::Action::Select(slot) = a {
                                Some(*slot)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(state.inventory.selected);
                    state.input.actions.push_back(gameplay::Action::Select(
                        (i32::from(current) - if amount > 0.0 { 1 } else { -1 }).rem_euclid(9)
                            as u8,
                    ));
                }
            }
            WindowEvent::MouseInput {
                state: button_state,
                button,
                ..
            } => self.mouse_input(event_loop, button, button_state == ElementState::Pressed),
            WindowEvent::RedrawRequested => {
                if self.smoke_frames.is_some_and(|limit| self.frames >= limit) {
                    return;
                }
                let frame_started = Instant::now();
                if self.keyboard.reconcile() {
                    eprintln!("keys_reconciled=true");
                    self.publish_keys();
                }
                let menu = self
                    .network
                    .as_ref()
                    .and_then(|network| {
                        network
                            .shared
                            .lock()
                            .ok()
                            .map(|s| s.inventory.open || s.vitals.health <= 0.0)
                    })
                    .unwrap_or(false)
                    || self.chat.open
                    || self.menu.active()
                    || self.pause.active();
                if menu && self.captured {
                    self.release_cursor();
                }
                if !menu && self.menu_open && self.focused && self.smoke_frames.is_none() {
                    self.capture_cursor();
                }
                self.menu_open = menu;
                self.update_mods();
                // This frame's schedule, as `update_mods` set the present
                // mode: paced frames teach the pacer.
                let vsync = self.schedule() == pacing::Schedule::Vsync;
                self.apply_look();
                let pose = self.camera_pose();
                let tick = self.tick();
                let mouse = self.mouse_gui();
                if pose.is_some() {
                    self.connection.connected();
                }
                let mut snapshot = self.network.as_ref().and_then(|n| {
                    n.shared.lock().ok().map(|mut s| Snapshot {
                        view: s.inventory.clone(),
                        swing: s.swing,
                        vitals: s.vitals,
                        game_mode: s.game_mode,
                        experience: s.experience,
                        hardcore: s.hardcore,
                        inbox: std::mem::take(&mut s.chat_inbox),
                        scoreboard: s.scoreboard.clone(),
                        tab: s.tab.clone(),
                        username: s.username.clone(),
                        encrypted: s.encrypted,
                        hand: s.hand.clone(),
                        effects: std::mem::take(&mut s.effects),
                        own: s.own.clone(),
                        entity_id: s.entity_id,
                        player_epoch: s.player_epoch,
                        world_epoch: s.world_epoch,
                        cracks: s.cracks.clone(),
                        visuals: std::mem::take(&mut s.visuals),
                        breaks: std::mem::take(&mut s.breaks),
                        armor: s.armor.clone(),
                        potions: s.potions.clone(),
                        current: s.current,
                        aiming_entity: s.aiming_entity,
                        own_body: s.own_body.clone(),
                        difficulty: s.difficulty,
                        time_of_day: s.time_of_day,
                        reduced_debug: s.reduced_debug,
                    })
                });
                if let Some(s) = &snapshot {
                    if self.player_epoch != s.player_epoch {
                        self.player_epoch = s.player_epoch;
                        self.container_pointer.clear();
                        self.metrics.release_input();
                    }
                    if self.world_epoch != s.world_epoch {
                        self.world_epoch = s.world_epoch;
                        self.particles = particles::Particles::default();
                    }
                }
                if let Some(s) = &mut snapshot
                    && let Some(preview) = self.container_pointer.preview(&s.view)
                {
                    s.view = Arc::new(preview);
                }
                let registry = self.renderer.as_ref().and_then(|r| r.registry.clone());
                if let (Some(snapshot), Some(registry)) = (&snapshot, &registry) {
                    for event in &snapshot.inbox {
                        match event {
                            network::ChatEvent::Message(message) => {
                                if let Some(bench) = &mut self.trace_bench
                                    && bench.chat(&message.unformatted)
                                {
                                    continue;
                                }
                                self.chat.receive(&registry.gui, message, tick);
                            }
                            network::ChatEvent::ActionBar(text) => {
                                self.hud.set_action_bar(text.clone(), tick);
                            }
                            network::ChatEvent::Title(title) => {
                                self.hud.set_title(title.clone(), tick);
                            }
                            network::ChatEvent::Completions(matches) => {
                                if let Some(listing) = self.chat.completions(matches.clone()) {
                                    self.chat.notice(&registry.gui, &listing, tick);
                                }
                            }
                        }
                    }
                }
                if let Some(snapshot) = &snapshot {
                    for event in &snapshot.visuals {
                        self.particles.event(
                            event,
                            pose.as_ref().map_or([0.0; 3], |(pose, _, _)| pose.position),
                        );
                    }
                    for &(entity, magic) in &snapshot.effects {
                        self.particles.emit(entity, magic);
                    }
                    if let Some(registry) = &registry {
                        for &(at, state) in &snapshot.breaks {
                            if let Some((layer, color)) = registry.particle(state) {
                                self.particles.destroy(at, layer, color);
                            }
                        }
                    }
                }
                if let Some((_, _, entities)) = &pose {
                    let now = tick.floor() as u64;
                    // Refresh shape snapshots without waiting on the network writer.
                    let world = self.network.as_ref().and_then(|n| n.world.try_read().ok());
                    for _ in 0..now.saturating_sub(self.particle_tick).min(10) {
                        self.particles.tick(entities, world.as_deref());
                    }
                    self.particle_tick = now;
                }
                while let Ok(result) = self.screenshots.1.try_recv() {
                    let text = screenshot::message(&result);
                    eprintln!("SCREENSHOT {text}");
                    if let Some(registry) = self.renderer.as_ref().and_then(|r| r.registry.clone())
                    {
                        let tick = self.tick();
                        self.chat.notice(&registry.gui, &text, tick);
                    }
                }
                if let Some(renderer) = &mut self.renderer {
                    // The launcher's live scene: the island, your skin, and
                    // the page's camera.
                    if let (Some(arena), Some(launcher)) = (&mut self.arena, &self.launcher)
                        && self.network.is_none()
                    {
                        if !arena.meshed
                            && let Some(registry) = renderer.registry.clone()
                        {
                            arena.meshed = true;
                            for update in arena::meshes(&registry, arena.world()) {
                                if let Err(error) = renderer.apply_mesh(update) {
                                    self.fail(event_loop, error);
                                    return;
                                }
                            }
                        }
                        if let Some(session) = launcher.session() {
                            arena.show_skin(session.profile.uuid);
                        }
                        let layout = renderer.layout;
                        let pointer = (
                            mouse.0 / layout.width.max(1.0) * 2.0 - 1.0,
                            mouse.1 / layout.height.max(1.0) * 2.0 - 1.0,
                        );
                        let (camera, player) = arena.frame(launcher.page(), pointer);
                        renderer.set_pose(camera);
                        let own = arena.own();
                        let name = own.as_ref().map_or(String::new(), |(_, p)| p.name.clone());
                        if let Err(error) = renderer.set_entities(
                            &[(0, player)],
                            1.0,
                            &scoreboard::Scoreboard::default(),
                            &name,
                            own.as_ref(),
                            None,
                            &[],
                            &[],
                            None,
                        ) {
                            self.fail(event_loop, error);
                            return;
                        }
                    }
                    if let Some((pose, partial, entities)) = pose {
                        if let Some(snapshot) = &snapshot {
                            let effects =
                                hand::camera_effects(&snapshot.hand, partial, self.view_bobbing);
                            renderer.set_camera_effects(effects);
                        }
                        // F5: `orientCamera` moves the view; the player's own
                        // eye still picks blocks and casts the fishing line.
                        let world = self.network.as_ref().and_then(|n| n.world.try_read().ok());
                        let (position, yaw, pitch) = match &world {
                            Some(world) => view::camera(
                                world,
                                &mut self.camera_rays,
                                pose.position,
                                pose.yaw,
                                pose.pitch,
                                self.view.perspective,
                            ),
                            None => (pose.position, pose.yaw, pose.pitch),
                        };
                        let camera = PositionLook {
                            position,
                            yaw,
                            pitch,
                        };
                        let in_water = world
                            .as_ref()
                            .is_some_and(|world| hand::eye_in_water(world, camera.position));
                        drop(world);
                        let fov = snapshot.as_ref().map_or(self.fov, |s| {
                            hand::world_fov(self.fov, &s.hand, partial, in_water)
                        });
                        renderer.set_fov(fov * self.zoom);
                        renderer.set_pose(camera);
                        if let Some(network) = &self.network {
                            network.prioritize(renderer.mesh_view());
                        }
                        let empty = scoreboard::Scoreboard::default();
                        let (board, own_name) = snapshot
                            .as_ref()
                            .map_or((&empty, ""), |s| (&*s.scoreboard, s.username.as_str()));
                        let mut extra = Vec::new();
                        if let Some(scene_origin) = renderer.origin() {
                            self.particles.draw(
                                &mut extra,
                                partial,
                                scene_origin,
                                pose.yaw,
                                pose.pitch,
                            );
                        }
                        let own = snapshot.as_ref().and_then(|s| s.own.as_ref());
                        let world_gui = self.view.world_gui();
                        let first_person = self.view.perspective == 0;
                        renderer.names = world_gui;
                        // `doRenderEntity` skips the debug box when debug info is reduced.
                        renderer.hitboxes = self.view.hitboxes
                            && !snapshot.as_ref().is_some_and(|s| s.reduced_debug);
                        renderer.own_body = snapshot
                            .as_ref()
                            .filter(|s| !first_person && s.game_mode != 3)
                            .and_then(|s| own_body(s, pose));
                        let hand_shown = self.view.hand();
                        let hand_frame = snapshot
                            .as_ref()
                            .filter(|s| s.game_mode != 3 && hand_shown)
                            .map(|s| {
                                let own_uuid = s.own.as_ref().map(|(uuid, _)| *uuid);
                                let (skin, slim) = own_uuid
                                    .and_then(|uuid| renderer.skin_of(uuid))
                                    .unwrap_or_else(|| {
                                        let slim = own_uuid
                                            .is_some_and(horizon_world_189::entities::slim_skin);
                                        let default = if slim {
                                            models::Texture::Alex
                                        } else {
                                            models::Texture::Steve
                                        };
                                        (default as u32, slim)
                                    });
                                hand::Frame {
                                    state: &s.hand,
                                    partial,
                                    swing: s
                                        .swing
                                        .map_or(0.0, |v| ((f32::from(v) + partial) / 6.0).min(1.0)),
                                    skin,
                                    slim,
                                    sleeve: true,
                                    invisible: s.vitals.effects & (1 << 14) != 0,
                                    bobbing: self.view_bobbing,
                                    look: [pose.pitch, pose.yaw],
                                    millis: models::millis(),
                                    cast: entities.iter().any(|(_, entity)| {
                                        entity.kind == horizon_world_189::entities::Kind::Object(90)
                                            && entity.data == s.entity_id
                                    }),
                                }
                            });
                        if let Err(error) = renderer.set_hand(hand_frame.as_ref()) {
                            self.fail(event_loop, error);
                            return;
                        }
                        // `RenderFish` draws the local player's line from the camera.
                        // Third person draws it from the model's hand instead.
                        let angler = snapshot
                            .as_ref()
                            .filter(|s| s.game_mode != 3 && first_person)
                            .map(|s| models::Angler {
                                id: s.entity_id,
                                eye: pose.position,
                                yaw: pose.yaw,
                                pitch: pose.pitch,
                                swing: s
                                    .swing
                                    .map_or(0.0, |v| ((f32::from(v) + partial) / 6.0).min(1.0)),
                            });
                        // EntityRenderer.getMouseOver for the outline, each frame:
                        // only where vanilla draws it (players who may edit, not
                        // while aiming at an entity).
                        let outline = crate::outline::style(self.settings.toggles.outline)
                            .zip(snapshot.as_ref())
                            .filter(|(_, s)| {
                                matches!(s.game_mode, 0 | 1) && !s.aiming_entity && world_gui
                            })
                            .and_then(|(style, s)| {
                                let world = self.network.as_ref()?.world.try_read().ok()?;
                                let hit = self.selector.select(
                                    &world,
                                    &self.no_entities,
                                    pose.position,
                                    pose.yaw,
                                    pose.pitch,
                                    s.game_mode == 1,
                                );
                                let Some(horizon_physics_189::raycast::Hit::Block { at, .. }) = hit
                                else {
                                    return None;
                                };
                                let bounds =
                                    horizon_physics_189::raycast::selection_box(&world, at)?;
                                Some((
                                    horizon_physics_189::shapes::lo(bounds),
                                    horizon_physics_189::shapes::hi(bounds),
                                    style,
                                ))
                            });
                        if let Err(error) = renderer.set_entities(
                            &entities,
                            partial,
                            board,
                            own_name,
                            own,
                            angler.as_ref(),
                            snapshot.as_ref().map_or(&[][..], |s| &s.cracks),
                            &extra,
                            outline,
                        ) {
                            self.fail(event_loop, error);
                            return;
                        }
                        // GuiInventory's player model.
                        let model = snapshot
                            .as_ref()
                            .filter(|s| {
                                s.view.open
                                    && s.view.kind == inventory::Kind::Player
                                    && s.vitals.health > 0.0
                            })
                            .and_then(|s| {
                                let at = hud::inventory_model_point(&s.view, renderer.layout);
                                own_model(s).map(|entity| (entity, at))
                            });
                        let shown = model.as_ref().map(|(entity, _)| entity);
                        let at = model.as_ref().map_or((0.0, 0.0), |(_, at)| *at);
                        if let Err(error) = renderer.set_inventory_model(shown, at, mouse) {
                            self.fail(event_loop, error);
                            return;
                        }
                        if let Some(snapshot) = &snapshot {
                            let (hud, chat) = (&mut self.hud, &mut self.chat);
                            let tab_held = self.tab_held || self.smoke_tab;
                            let sample = self.metrics.sample(self.started.elapsed());
                            let frame_ms = self.frame_time.millis();
                            let (settings, menu, rects) =
                                (&self.settings, &self.menu, &mut self.hud_rects);
                            let pause = &self.pause;
                            let overlay = self.view.overlay(
                                snapshot.view.open
                                    || snapshot.vitals.health <= 0.0
                                    || chat.open
                                    || menu.visible()
                                    || pause.active(),
                            );
                            let keys = self.keyboard.keys();
                            let buttons = self.metrics.held();
                            let ping = snapshot
                                .own
                                .as_ref()
                                .and_then(|(uuid, _)| snapshot.tab.entries.get(uuid))
                                .map(|entry| entry.ping);
                            let yaw = pose.yaw;
                            let debug_text = (self.view.debug && overlay).then(|| {
                                let cpu = self.cpu_name.get_or_insert_with(cpu_name).clone();
                                let (display, gpu, sections, view_distance) =
                                    renderer.debug_facts();
                                let network = self.network.as_ref();
                                let stats = network.map(|n| &n.mesh_stats);
                                let sent = stats.map_or(0, |s| {
                                    s.sent.load(std::sync::atomic::Ordering::Relaxed)
                                });
                                if self.rebuilds.0.elapsed() >= Duration::from_secs(1) {
                                    let meshed = sent.saturating_sub(self.rebuilds.1);
                                    self.rebuilds = (
                                        Instant::now(),
                                        sent,
                                        u32::try_from(meshed * 16).unwrap_or(u32::MAX),
                                    );
                                }
                                let world = network.and_then(|n| n.world.try_read().ok());
                                let feet = snapshot.current;
                                let block = feet.map(|v| v.floor() as i32);
                                let here = world.as_ref().and_then(|world| {
                                    let column = world
                                        .column(block[0].div_euclid(16), block[2].div_euclid(16))?;
                                    Some(debug::Here {
                                        biome: column.biome(
                                            block[0].rem_euclid(16) as u8,
                                            block[2].rem_euclid(16) as u8,
                                        ),
                                        light: world.light(block[0], block[1], block[2]),
                                        difficulty: snapshot.difficulty,
                                        time_of_day: snapshot.time_of_day,
                                    })
                                });
                                let looking_at = world
                                    .as_ref()
                                    .filter(|_| !snapshot.aiming_entity)
                                    .and_then(|world| {
                                        let hit = self.selector.select(
                                            world,
                                            &self.no_entities,
                                            pose.position,
                                            pose.yaw,
                                            pose.pitch,
                                            snapshot.game_mode == 1,
                                        );
                                        let Some(horizon_physics_189::raycast::Hit::Block {
                                            at,
                                            ..
                                        }) = hit
                                        else {
                                            return None;
                                        };
                                        let state = world.block(at[0], at[1], at[2])?;
                                        Some((
                                            at,
                                            horizon_world_189::blocks::block(state.block_id()).name,
                                        ))
                                    });
                                let columns = world.as_ref().map_or(0, |w| w.column_count());
                                drop(world);
                                let limit = self.settings.toggles.fps_limit;
                                let info = debug::Info {
                                    fps: sample.fps.unwrap_or(0),
                                    chunk_updates: self.rebuilds.2,
                                    fps_limit: (limit != 0 && limit != u32::MAX).then_some(limit),
                                    vsync,
                                    sections,
                                    view_distance,
                                    mesher: [
                                        stats.map_or(0, |s| {
                                            s.pending.load(std::sync::atomic::Ordering::Relaxed)
                                        }),
                                        usize::try_from(sent.saturating_sub(self.meshes_applied))
                                            .unwrap_or(0),
                                        stats.map_or(0, |s| {
                                            s.idle.load(std::sync::atomic::Ordering::Relaxed)
                                        }),
                                    ],
                                    entities: (renderer.entities_drawn, entities.len() + 1),
                                    particles: self.particles.len(),
                                    columns,
                                    feet,
                                    yaw: pose.yaw,
                                    pitch: pose.pitch,
                                    here,
                                    looking_at,
                                    reduced: snapshot.reduced_debug,
                                    build: updater::BUILD.unwrap_or("dev"),
                                    cpu: &cpu,
                                    display,
                                    gpu: &gpu,
                                };
                                (debug::left(&info), debug::right(&info))
                            });
                            let result = renderer.draw_ui(tick, |canvas, registry, skin| {
                                let frame = hud::Frame {
                                    view: &snapshot.view,
                                    vitals: snapshot.vitals,
                                    game_mode: snapshot.game_mode,
                                    hardcore: snapshot.hardcore,
                                    experience: snapshot.experience,
                                    mouse,
                                    tick,
                                    scoreboard: &snapshot.scoreboard,
                                    tab: &snapshot.tab,
                                    own_name: &snapshot.username,
                                    tab_held,
                                    heads: snapshot.encrypted,
                                    skin,
                                    sidebar_numbers: settings.toggles.scoreboard_numbers,
                                    crosshair: settings.toggles.crosshair,
                                };
                                if overlay {
                                    hud.draw(canvas, registry, &frame);
                                }
                                if let Some((left, right)) = &debug_text {
                                    debug::draw(canvas, left, right);
                                }
                                // The mods would sit under the debug text.
                                if overlay
                                    && debug_text.is_none()
                                    && ((!snapshot.view.open && snapshot.vitals.health > 0.0)
                                        || menu.editing)
                                {
                                    let mods = hud_mods::ModFrame {
                                        settings,
                                        keys,
                                        buttons,
                                        cps: [sample.left, sample.right],
                                        fps: sample.fps,
                                        frame_ms,
                                        armor: &snapshot.armor,
                                        potions: &snapshot.potions,
                                        position: snapshot.current,
                                        yaw,
                                        ping,
                                    };
                                    *rects = hud_mods::draw(
                                        canvas,
                                        registry,
                                        &mods,
                                        menu.editing,
                                        menu.dragging(),
                                    );
                                }
                                if overlay {
                                    chat.draw(canvas, tick, mouse);
                                    hud.draw_after_chat(canvas, &frame);
                                }
                                if snapshot.view.open && snapshot.vitals.health > 0.0 {
                                    hud::inventory(
                                        canvas,
                                        registry,
                                        &snapshot.view,
                                        mouse,
                                        settings.toggles.advanced_tooltips,
                                    );
                                }
                                if menu.visible() {
                                    menu.draw(canvas, settings, mouse);
                                }
                                pause.draw(canvas, settings, mouse);
                            });
                            if let Err(error) = result {
                                self.fail(event_loop, error);
                                return;
                            }
                        }
                    }
                    if self.connection.active() {
                        let screen = &self.connection;
                        if let Err(error) =
                            renderer.draw_ui(tick, |canvas, _, _| screen.draw(canvas, mouse))
                        {
                            self.fail(event_loop, error);
                            return;
                        }
                    } else if self.network.is_none()
                        && let Some(launcher) = &self.launcher
                    {
                        let status = self
                            .updater
                            .as_ref()
                            .map_or(updater::Status::Off, updater::Updater::status);
                        let settings = &self.settings;
                        if let Err(error) = renderer.draw_ui(tick, |canvas, registry, skin| {
                            launcher.draw(canvas, registry, skin, settings, &status, mouse);
                        }) {
                            self.fail(event_loop, error);
                            return;
                        }
                    }
                    let last = self
                        .smoke_frames
                        .is_some_and(|limit| self.frames + 1 >= limit)
                        && (self.network.is_none() || renderer.world_ready());
                    let capture = if last { self.capture.take() } else { None };
                    if self
                        .smoke_frames
                        .is_some_and(|limit| self.frames + 120 >= limit)
                        && (self.network.is_none() || renderer.world_ready())
                        && std::mem::take(&mut self.smoke_screenshot)
                    {
                        renderer.screenshot = true;
                    }
                    let offscreen = renderer.offscreen_frames;
                    let readback = capture.is_some() || renderer.screenshot;
                    match renderer.render(capture.as_deref(), self.smoke_frames.is_some()) {
                        Ok(true) => {
                            if vsync && !readback && renderer.offscreen_frames == offscreen {
                                let age = self.pacer.presented(pacing::Frame {
                                    planned: self.planned.take().filter(|_| self.jit),
                                    sampled: frame_started,
                                    waited: renderer.acquire_wait,
                                    acquired: renderer.acquired_at,
                                    presented: renderer.presented_at,
                                });
                                if let Some(age) = age
                                    && self.smoke_frames.is_some()
                                    && (self.network.is_none() || renderer.world_ready())
                                {
                                    self.input_ages.record(age);
                                }
                                if self.jit {
                                    self.next_frame = self.pacer.next_start(Instant::now());
                                }
                            }
                            if let Some(shot) = renderer.shot.take() {
                                let done = self.screenshots.0.clone();
                                match (shot, &self.screenshot_dir) {
                                    (Ok(pixels), Some(dir)) => {
                                        screenshot::save(pixels, dir.clone(), done);
                                    }
                                    (Ok(_), None) => {
                                        let _ = done.send(Err("no home folder".into()));
                                    }
                                    (Err(error), _) => {
                                        let _ = done.send(Err(error));
                                    }
                                }
                            }
                            if self.network.is_none() || renderer.world_ready() {
                                self.frames += 1;
                                if renderer.offscreen_frames == offscreen {
                                    self.metrics.presented(self.started.elapsed());
                                }
                                self.frame_time.record(
                                    frame_started
                                        .elapsed()
                                        .saturating_sub(renderer.acquire_wait + renderer.gpu_time),
                                );
                                if self.bench && capture.is_none() {
                                    let now = Instant::now();
                                    let gpu = renderer.gpu_time;
                                    self.bench_gpu.record(gpu);
                                    self.bench_cpu
                                        .record(frame_started.elapsed().saturating_sub(gpu));
                                    let draws = renderer.take_draws();
                                    if let Some(last) = self.bench_last.replace(now) {
                                        self.bench_wall.record(now - last);
                                        if let Some(bench) = &mut self.trace_bench {
                                            let allocations = bench.main_thread_allocations();
                                            bench.frame(&trace_bench::Frame {
                                                interval: now - last,
                                                cpu: frame_started.elapsed().saturating_sub(gpu),
                                                gpu,
                                                draws,
                                                allocations,
                                            });
                                        }
                                    }
                                }
                                if self.smoke_frames.is_some() && capture.is_none() {
                                    self.frame_stats.record(frame_started.elapsed());
                                    // Pacing counts only frames the display took:
                                    // macOS throttles a covered window.
                                    let now = Instant::now();
                                    let shown = renderer.offscreen_frames == offscreen;
                                    let last = self.last_present.take();
                                    if shown {
                                        if let Some(last) = last {
                                            self.frame_intervals.record(now - last);
                                        }
                                        self.last_present = Some(now);
                                    }
                                }
                            }
                        }
                        Ok(false) => {}
                        Err(error) => {
                            self.fail(event_loop, error);
                            return;
                        }
                    }
                }
                if self.trace_bench.as_ref().is_some_and(|bench| bench.ended)
                    && let Some(bench) = self.trace_bench.take()
                {
                    for line in bench.report() {
                        eprintln!("{line}");
                    }
                    eprintln!("BOBTRACE_DONE frames={}", self.frames);
                    event_loop.exit();
                    return;
                }
                if self.smoke_frames.is_some_and(|limit| self.frames >= limit) {
                    let mode = if self.network.is_some() {
                        "WORLD_METAL"
                    } else {
                        "METAL"
                    };
                    self.frame_stats.report("FRAME_SUBMIT_US");
                    self.frame_intervals.report("FRAME_INTERVAL_US");
                    self.input_ages.report("FRAME_INPUT_AGE_US");
                    eprintln!(
                        "PACING jit={} misses={} budget_us={}",
                        self.jit,
                        self.pacer.misses,
                        self.pacer.budget().as_micros()
                    );
                    if self.bench {
                        self.bench_cpu.report("BENCH_CPU_US");
                        self.bench_gpu.report("BENCH_GPU_US");
                        self.bench_wall.report("BENCH_FRAME_US");
                        eprintln!("BENCH_FPS mean={:.0}", self.bench_wall.mean_rate());
                    }
                    eprintln!(
                        "{mode}_SMOKE_PASS frames={} elapsed_ms={} columns={} offscreen_frames={}",
                        self.frames,
                        self.started.elapsed().as_millis(),
                        self.renderer.as_ref().map_or(0, Renderer::column_count),
                        self.renderer.as_ref().map_or(0, |r| r.offscreen_frames)
                    );
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if self.captured
            && let DeviceEvent::MouseMotion { delta } = event
            && delta.0.is_finite()
            && delta.1.is_finite()
        {
            self.mouse.0 += delta.0;
            self.mouse.1 += delta.1;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.closing {
            // The game is running; finish installing the update, then go.
            if self.updating() {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(500),
                ));
            } else {
                event_loop.exit();
            }
            return;
        }
        // Upload rebuilt columns within a frame-sized budget, and when pacing
        // just in time only in the idle time before the next frame starts.
        let started = Instant::now();
        let paced = self.schedule() == pacing::Schedule::Vsync && self.pacer.has_phase();
        let upload = pacing::upload_budget(started, self.next_frame, self.jit, paced);
        while started.elapsed() < upload {
            let update = self
                .network
                .as_ref()
                .and_then(|network| network.meshes.try_recv().ok());
            let Some(update) = update else {
                break;
            };
            self.meshes_applied += 1;
            if let Some(bench) = &mut self.trace_bench
                && update.mesh.is_some()
            {
                bench.mesh(update.latency);
            }
            if let Some(renderer) = &mut self.renderer
                && let Err(error) = renderer.apply_mesh(update)
            {
                self.fail(event_loop, error);
                return;
            }
        }
        // Vanilla takes ingame focus once the world loads.
        let positioned = self
            .network
            .as_ref()
            .and_then(|network| network.shared.lock().ok().map(|shared| shared.positioned))
            .unwrap_or(false);
        if positioned && self.focused && !self.auto_captured && self.smoke_frames.is_none() {
            self.auto_captured = true;
            self.capture_cursor();
        }
        if positioned
            && let Some(pitch) = self.smoke_pitch.take()
            && let Some(network) = &self.network
            && let Ok(mut shared) = network.shared.lock()
        {
            shared.pitch = pitch;
        }
        if positioned && std::mem::take(&mut self.smoke_pause) {
            self.pause.open();
        }
        if positioned && std::mem::take(&mut self.smoke_settings) {
            self.pause.open_settings();
        }
        if positioned && std::mem::take(&mut self.smoke_menu) {
            self.menu.toggle();
        }
        if positioned && std::mem::take(&mut self.smoke_hud_editor) {
            self.menu.editing = true;
        }
        if positioned && std::mem::take(&mut self.smoke_inventory) {
            if let Some(network) = &self.network
                && let Ok(mut state) = network.shared.lock()
            {
                state
                    .input
                    .actions
                    .push_back(gameplay::Action::OpenInventory);
            }
            if let Some(renderer) = &self.renderer {
                let layout = renderer.layout;
                let (left, top) = (
                    ((layout.width as i32 - 176) / 2) as f64,
                    ((layout.height as i32 - 166) / 2) as f64,
                );
                self.cursor = ((left + 16.0) * layout.factor, (top + 150.0) * layout.factor);
            }
            eprintln!("SMOKE_INVENTORY_OPENED");
        }
        if positioned && let Some(text) = self.smoke_chat.take() {
            self.open_chat("");
            for c in text.chars() {
                self.chat_key(KeyCode::KeyA, Some(&c.to_string()));
            }
            self.chat_key(KeyCode::Enter, None);
            self.open_chat("/");
            eprintln!("SMOKE_CHAT_SENT chars={}", text.chars().count());
        }
        if self.watchdog.is_some() {
            let tick = self
                .network
                .as_ref()
                .and_then(|n| n.shared.lock().ok().map(|s| s.moves_sent))
                .unwrap_or(0);
            let steps = self
                .watchdog
                .as_mut()
                .map(|w| w.due(tick))
                .unwrap_or_default();
            for step in steps {
                match step {
                    watchdog::Step::Key(key, pressed) => {
                        self.key_input(event_loop, key, pressed, false, None);
                    }
                    watchdog::Step::Click(button, pressed) => {
                        self.mouse_input(event_loop, button, pressed);
                    }
                    watchdog::Step::Capture => {
                        self.capture_cursor();
                        // A smoke window may not hold focus to lock the
                        // pointer; the game still treats it as captured.
                        self.captured = true;
                        self.publish_keys();
                    }
                    watchdog::Step::Done => {
                        eprintln!("WATCHDOG_SCRIPT_DONE ticks={tick}");
                        event_loop.exit();
                        return;
                    }
                }
            }
        }
        if let Some(error) = self
            .network
            .as_ref()
            .and_then(|network| network.errors.try_recv().ok())
        {
            self.disconnected(event_loop, error);
            return;
        }
        if self
            .smoke_frames
            .is_some_and(|limit| self.started.elapsed() > Duration::from_secs(limit / 20 + 30))
        {
            self.fail(
                event_loop,
                "Metal smoke timed out before presenting the requested frames",
            );
            return;
        }
        let now = Instant::now();
        if now >= self.next_frame {
            if let Some(renderer) = &self.renderer {
                renderer.window.request_redraw();
                self.planned = Some(self.next_frame);
            }
            self.next_frame = now + self.redraw_period();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.release_cursor();
        self.network.take();
    }
}

/// The hotbar slot a number key selects.
fn hotbar_digit(key: KeyCode) -> Option<u8> {
    Some(match key {
        KeyCode::Digit1 => 0,
        KeyCode::Digit2 => 1,
        KeyCode::Digit3 => 2,
        KeyCode::Digit4 => 3,
        KeyCode::Digit5 => 4,
        KeyCode::Digit6 => 5,
        KeyCode::Digit7 => 6,
        KeyCode::Digit8 => 7,
        KeyCode::Digit9 => 8,
        _ => return None,
    })
}

/// 96% of the window's display refresh period (16 ms at 60 Hz, 8 ms at
/// 120 Hz), or 16 ms when the display does not report a rate. Redrawing back
/// to back instead starves CoreAnimation of its drawables on the main thread.
fn frame_period(window: &Window) -> Duration {
    let millihertz = window
        .current_monitor()
        .and_then(|monitor| monitor.refresh_rate_millihertz())
        .filter(|&rate| (20_000..=500_000).contains(&rate));
    millihertz.map_or(Duration::from_millis(16), |rate| {
        Duration::from_micros(960_000_000 / u64::from(rate))
    })
}

/// The display's exact refresh period (16.67 ms when it reports none).
fn refresh_period(window: &Window) -> Duration {
    window
        .current_monitor()
        .and_then(|monitor| monitor.refresh_rate_millihertz())
        .filter(|&rate| (20_000..=500_000).contains(&rate))
        .map_or(Duration::from_micros(16_667), |rate| {
            Duration::from_nanos(1_000_000_000_000 / u64::from(rate))
        })
}

fn main() -> Result<(), Box<dyn Error>> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err("This client targets macOS Apple Silicon".into());
    }
    let mut options = options::Options::parse(std::env::args().skip(1))?;
    if options.help {
        println!("{}", options::HELP);
        return Ok(());
    }
    if options.install {
        let source = match options.update_source.as_deref() {
            Some(url) => Some(
                updater::Source::parse(&serde_json::json!({ "repository": url }).to_string())
                    .ok_or("--update-source must be a plain https git URL")?,
            ),
            None => None,
        };
        let bundle = updater::install(source.as_ref())?;
        println!("INSTALLED {}", bundle.display());
        return Ok(());
    }
    let _benchmark_activity = options.bench.then(|| {
        let guard = horizon_threads::activity::benchmark_activity();
        eprintln!("BENCH_ACTIVITY_READY scoped=true");
        guard
    });
    let preview = options.smoke_launcher;
    let launcher_mode = options.launcher() || preview;
    let from_launcher = options.started_by_launcher();
    let watchdog = options.watchdog;
    let smoke_page = options.smoke_page;
    if launcher_mode {
        let home = std::env::var_os("HOME").ok_or("No home directory")?;
        let jar = std::path::PathBuf::from(home)
            .join("Library/Application Support/minecraft/versions/1.8.9/1.8.9.jar");
        if !jar.is_file() {
            alert(
                "Bob Client needs your copy of Minecraft 1.8.9. Open the Minecraft Launcher, \
                 play 1.8.9 once so it downloads, then open Bob Client again.",
            );
            return Ok(());
        }
        options.assets = Some(jar);
    }
    let server = match options.authenticate()? {
        options::Startup::Complete => return Ok(()),
        options::Startup::Window(server) => server,
    };
    scheduling::apply("render-input", scheduling::Class::Interactive);
    let smoke_frames = options.smoke_frames;
    let mut app = App {
        renderer: None,
        keyboard: Keyboard::default(),
        captured: false,
        focused: false,
        auto_captured: false,
        mouse: (0.0, 0.0),
        skip_motion: false,
        cursor: (0.0, 0.0),
        menu_open: false,
        sensitivity: options.sensitivity,
        view_distance: options.view_distance,
        frames: 0,
        frame_stats: Default::default(),
        frame_intervals: Default::default(),
        pacer: pacing::Pacer::new(Duration::from_micros(16_667)),
        jit: !options.no_jit_pacing,
        input_ages: Default::default(),
        planned: None,
        last_present: None,
        smoke_frames,
        started: Instant::now(),
        next_frame: Instant::now(),
        frame_period: Duration::from_millis(16),
        failure: None,
        assets: options.assets,
        capture: options.capture,
        connection: {
            let mut screen = connection::Screen::new(server.is_some());
            screen.back = launcher_mode || from_launcher;
            screen
        },
        server,
        network: None,
        chat: chat::Chat::default(),
        hud: hud::Hud::default(),
        modifiers: ModifiersState::empty(),
        clipboard: None,
        gui_scale: options.gui_scale,
        wheel_pixels: 0.0,
        smoke_chat: options.smoke_chat,
        tab_held: false,
        smoke_tab: options.smoke_tab,
        smoke_inventory: options.smoke_inventory,
        smoke_menu: options.smoke_menu,
        smoke_hud_editor: options.smoke_hud_editor,
        smoke_settings: options.smoke_settings,
        smoke_pause: options.smoke_pause,
        smoke_pitch: options.smoke_pitch,
        bench: options.bench,
        frame_time: Default::default(),
        bench_gpu: Default::default(),
        bench_cpu: Default::default(),
        bench_wall: Default::default(),
        bench_last: None,
        trace_bench: options.bench_trace.then(trace_bench::TraceBench::new),
        watchdog: watchdog.then(watchdog::Watchdog::new),
        container_pointer: container_input::Pointer::default(),
        particles: particles::Particles::default(),
        particle_tick: 0,
        player_epoch: 0,
        world_epoch: 0,
        view_bobbing: options.view_bobbing,
        view: view::View {
            hide_gui: options.smoke_hide_gui,
            perspective: options.smoke_perspective,
            debug: options.smoke_debug,
            hitboxes: options.smoke_hitboxes,
        },
        screenshot_dir: options
            .smoke_screenshot
            .clone()
            .or_else(screenshot::directory),
        screenshots: std::sync::mpsc::channel(),
        smoke_screenshot: options.smoke_screenshot.is_some(),
        camera_rays: Default::default(),
        meshes_applied: 0,
        cpu_name: None,
        rebuilds: (Instant::now(), 0, 0),
        fov: options.fov,
        metrics: pvp_metrics::Metrics::default(),
        settings: {
            // Validation runs use the defaults and never touch the file.
            let mut settings = settings::Settings::load(
                smoke_frames
                    .is_none()
                    .then(settings::default_path)
                    .flatten(),
            );
            if options.smoke_vanilla_menu {
                settings.toggles.vanilla_menu = true;
            }
            if options.fast_graphics {
                settings.video.fancy_graphics = false;
            }
            if options.pvp_hud {
                settings.hud_mut(settings::HudKind::Fps).enabled = true;
                settings.hud_mut(settings::HudKind::Cps).enabled = true;
            }
            settings
        },
        menu: {
            let mut menu = menu::Menu::default();
            menu.leave = launcher_mode || from_launcher;
            menu
        },
        pause: {
            let mut pause = pause::Pause::default();
            pause.leave_to_launcher = launcher_mode || from_launcher;
            pause
        },
        launcher: launcher_mode.then(|| {
            if preview {
                launcher::Launcher::preview(launcher::Page::ALL[smoke_page])
            } else {
                launcher::Launcher::new()
            }
        }),
        updater: (launcher_mode && !preview).then(updater::Updater::start),
        arena: launcher_mode.then(arena::Arena::new),
        from_launcher,
        closing: false,
        selector: Default::default(),
        no_entities: Default::default(),
        hud_rects: Vec::new(),
        zoom: 1.0,
        zoom_at: Instant::now(),
        sprint_latched: false,
        fullbright: false,
    };
    if launcher_mode || from_launcher {
        // The app's own settings, not command-line defaults.
        app.apply_video();
    }
    if watchdog {
        watchdog::enable_everything(&mut app.settings, &mut app.view);
        eprintln!("WATCHDOG_WINDOWS {}", watchdog::closed());
    }
    EventLoop::new()?.run_app(&mut app)?;
    if let Some(error) = app.failure {
        if launcher_mode || from_launcher {
            alert(&format!("Bob Client stopped: {error}"));
        }
        return Err(error.into());
    }
    Ok(())
}

/// Start the game for `address` and write the launcher's `handoff` line to
/// its stdin. Its log goes to `game.log` beside the settings; nothing it
/// logs carries a token.
fn spawn_game(address: &str, handoff: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let log = updater::support_dir().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::File::create(dir.join("game.log")).ok()
    });
    let mut child = Command::new(updater::program()?)
        .arg("--play")
        .arg(address)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(log.map_or_else(Stdio::null, Stdio::from))
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "{handoff}")?;
    }
    Ok(())
}

/// Open the launcher again after the game, from the program on disk (so an
/// update installed meanwhile is the one that opens).
fn reopen_launcher() {
    use std::process::{Command, Stdio};
    if let Ok(program) = updater::program() {
        let _ = Command::new(program)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

/// The local player as an entity for the inventory model: its skin,
/// armour and held item, standing at the origin.
/// The local player in the world for third person: the ticked body with
/// this frame's skin, equipment, swing and look.
fn own_body(
    snapshot: &Snapshot,
    pose: PositionLook,
) -> Option<(i32, horizon_world_189::entities::Entity)> {
    let mut body = snapshot.own_body.clone()?;
    if let Some((uuid, profile)) = &snapshot.own {
        body.uuid = *uuid;
        body.profile = Some(profile.clone());
    }
    body.skin_parts = 0x7f;
    let [helmet, chest, legs, boots] = snapshot.armor.clone();
    body.equipment = [snapshot.hand.item.clone(), boots, legs, chest, helmet];
    body.swing = snapshot.swing;
    body.hurt_time = snapshot.hand.hurt;
    body.health = snapshot.vitals.health;
    body.death_time = snapshot.hand.dead.map_or(0, |ticks| ticks.min(20) as u8);
    if snapshot.vitals.effects & (1 << 14) != 0 {
        body.flags |= 0x20;
    }
    // The head follows the live look, as the camera does.
    body.head_yaw = pose.yaw;
    body.previous_head_yaw = pose.yaw;
    body.pitch = pose.pitch;
    body.previous_pitch = pose.pitch;
    Some((snapshot.entity_id, body))
}

fn own_model(snapshot: &Snapshot) -> Option<horizon_world_189::entities::Entity> {
    let uuid = snapshot.own.as_ref().map_or(0, |(uuid, _)| *uuid);
    let mut tracked = horizon_world_189::entities::Entities::default();
    tracked.apply(horizon_protocol_47::entities::EntityPacket::SpawnPlayer {
        id: 0,
        uuid,
        position: [0; 3],
        yaw: 0,
        pitch: 0,
        held_item: 0,
        metadata: vec![],
    });
    let mut entity = tracked.get(0)?.clone();
    entity.profile = snapshot.own.as_ref().map(|(_, profile)| profile.clone());
    entity.skin_parts = 0x7f;
    // Held item, then boots, leggings, chestplate and helmet.
    let [helmet, chest, legs, boots] = snapshot.armor.clone();
    entity.equipment = [snapshot.hand.item.clone(), boots, legs, chest, helmet];
    Some(entity)
}

/// A native alert, for the app opened from Finder where errors have no
/// terminal. The message is passed as an argument, never as script text.
fn alert(message: &str) {
    let _ = std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "display alert \"Bob Client\" message (item 1 of argv)",
            "-e",
            "end run",
            "--",
            message,
        ])
        .status();
}

/// F3's CPU line: the core count and `machdep.cpu.brand_string`.
fn cpu_name() -> String {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let brand = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|brand| !brand.is_empty())
        .unwrap_or_else(|| "Apple".into());
    format!("{cores}x {brand}")
}
