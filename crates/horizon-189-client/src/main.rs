//! Native Protocol 47 client entry. World and gameplay integration follow the
//! transport and platform gates recorded in HANDOFF.md.
#![forbid(unsafe_code)]

mod chat;
mod frame_stats;
mod gameplay;
mod hardness;
mod hud;
mod inventory;
mod items;
mod keyboard;
mod models;
mod network;
mod options;
mod renderer;
mod scene;
#[cfg(test)]
mod scene_tests;
mod skins;
mod terrain;
mod ui;

use horizon_client_platform::is_quit_shortcut_down;
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
    smoke_frames: Option<u64>,
    started: Instant,
    next_frame: Instant,
    failure: Option<String>,
    assets: Option<std::path::PathBuf>,
    capture: Option<std::path::PathBuf>,
    server: Option<network::Config>,
    network: Option<network::Network>,
    chat: chat::Chat,
    hud: hud::Hud,
    modifiers: ModifiersState,
    clipboard: Option<arboard::Clipboard>,
    gui_scale: u32,
    /// Trackpad pixels not yet worth a wheel notch.
    wheel_pixels: f64,
    smoke_chat: Option<String>,
}

impl App {
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
    fn release_cursor(&mut self) {
        if let Some(renderer) = &self.renderer {
            let _ = renderer.window.set_cursor_grab(CursorGrabMode::None);
            renderer.window.set_cursor_visible(true);
        }
        self.captured = false;
        self.mouse = (0.0, 0.0);
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
            self.keyboard.keys()
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
        let factor = f * f * f * 8.0;
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
            .with_title("Horizon 1.8.9")
            .with_inner_size(LogicalSize::new(960.0, 600.0))
            .with_min_inner_size(LogicalSize::new(320.0, 240.0));
        let result = event_loop
            .create_window(attributes)
            .map_err(|error| error.to_string())
            .and_then(|window| {
                pollster::block_on(Renderer::new(
                    Arc::new(window),
                    self.assets.clone(),
                    self.server.is_some(),
                    self.view_distance,
                    self.gui_scale,
                ))
                .map_err(|error| error.to_string())
            });
        match result {
            Ok(renderer) => {
                if let Some(config) = self.server.take() {
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
                if self.smoke_frames.is_some() {
                    renderer.window.focus_window();
                }
                renderer.window.request_redraw();
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
                    let pressed = event.state == ElementState::Pressed;
                    if self.chat.open {
                        if !pressed {
                            self.keyboard.release(key);
                        } else if key == KeyCode::KeyQ
                            && self.modifiers.super_key()
                            && !self.modifiers.shift_key()
                        {
                            event_loop.exit();
                        } else {
                            self.chat_key(key, event.text.as_deref());
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
                        && !event.repeat
                        && !screen
                        && self.network.is_some()
                        && matches!(key, KeyCode::KeyT | KeyCode::Slash)
                        && !self.modifiers.super_key()
                    {
                        self.open_chat(if key == KeyCode::Slash { "/" } else { "" });
                        return;
                    }
                    if event.state == ElementState::Pressed {
                        self.keyboard.press(key, event.repeat);
                    } else {
                        self.keyboard.release(key);
                    }
                    if is_quit_shortcut_down(self.keyboard.state(), self.keyboard.held()) {
                        event_loop.exit();
                        return;
                    }
                    if event.state == ElementState::Pressed && !event.repeat {
                        if let Some(network) = &self.network
                            && let Ok(mut state) = network.shared.lock()
                        {
                            use gameplay::Action;
                            let action = if state.vitals.health <= 0.0 {
                                if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter) {
                                    Some(Action::Respawn)
                                } else {
                                    None
                                }
                            } else {
                                match key {
                                    KeyCode::Digit1 => Some(Action::Select(0)),
                                    KeyCode::Digit2 => Some(Action::Select(1)),
                                    KeyCode::Digit3 => Some(Action::Select(2)),
                                    KeyCode::Digit4 => Some(Action::Select(3)),
                                    KeyCode::Digit5 => Some(Action::Select(4)),
                                    KeyCode::Digit6 => Some(Action::Select(5)),
                                    KeyCode::Digit7 => Some(Action::Select(6)),
                                    KeyCode::Digit8 => Some(Action::Select(7)),
                                    KeyCode::Digit9 => Some(Action::Select(8)),
                                    KeyCode::KeyE if !state.inventory.open => {
                                        Some(Action::OpenInventory)
                                    }
                                    KeyCode::KeyE | KeyCode::Escape if state.inventory.open => {
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
                            self.release_cursor();
                        }
                    }
                    self.publish_keys();
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.cursor = (position.x, position.y),
            WindowEvent::MouseWheel { delta, .. } if self.chat.open => {
                let notches = self.notches(delta);
                if notches != 0 {
                    self.chat
                        .wheel(f64::from(notches), self.modifiers.shift_key());
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
            } => {
                let pressed = button_state == ElementState::Pressed;
                let mouse = self.mouse_gui();
                if self.chat.open {
                    if pressed
                        && button == MouseButton::Left
                        && let Some(registry) =
                            self.renderer.as_ref().and_then(|r| r.registry.clone())
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
                if pressed
                    && button == MouseButton::Left
                    && death == Some(hud::DeathButton::TitleScreen)
                {
                    event_loop.exit();
                    return;
                }
                let mut open = false;
                if let Some(network) = &self.network
                    && let Ok(mut state) = network.shared.lock()
                {
                    open = state.inventory.open || state.vitals.health <= 0.0;
                    if open && pressed {
                        if let Some(layout) = layout {
                            let x = mouse.0 - (layout.width / 2.0).floor();
                            let y = mouse.1 - (layout.height / 2.0).floor();
                            if state.vitals.health <= 0.0 {
                                if button == MouseButton::Left
                                    && death == Some(hud::DeathButton::Respawn)
                                    && state.input.actions.len() < 128
                                {
                                    state.input.actions.push_back(gameplay::Action::Respawn);
                                }
                            } else if let Some(slot) = state.inventory.slot_at(x, y)
                                && state.input.actions.len() < 128
                            {
                                state.input.actions.push_back(gameplay::Action::Click {
                                    slot,
                                    right: button == MouseButton::Right,
                                });
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
                    || self.chat.open;
                if menu && self.captured {
                    self.release_cursor();
                }
                if !menu && self.menu_open && self.focused && self.smoke_frames.is_none() {
                    self.capture_cursor();
                }
                self.menu_open = menu;
                self.apply_look();
                let pose = self.camera_pose();
                let tick = self.tick();
                let mouse = self.mouse_gui();
                let snapshot = self.network.as_ref().and_then(|n| {
                    n.shared.lock().ok().map(|mut s| {
                        (
                            s.inventory.clone(),
                            s.swing,
                            s.using_item,
                            s.vitals,
                            s.game_mode,
                            s.experience,
                            s.hardcore,
                            std::mem::take(&mut s.chat_inbox),
                        )
                    })
                });
                let registry = self.renderer.as_ref().and_then(|r| r.registry.clone());
                if let (Some(snapshot), Some(registry)) = (&snapshot, &registry) {
                    for event in &snapshot.7 {
                        match event {
                            network::ChatEvent::Message(message) => {
                                self.chat.receive(&registry.gui, message, tick);
                            }
                            network::ChatEvent::ActionBar(text) => {
                                self.hud.set_action_bar(text.clone(), tick);
                            }
                            network::ChatEvent::Completions(matches) => {
                                if let Some(listing) = self.chat.completions(matches.clone()) {
                                    self.chat.notice(&registry.gui, &listing, tick);
                                }
                            }
                        }
                    }
                }
                if let Some(renderer) = &mut self.renderer {
                    if let Some((pose, partial, entities)) = pose {
                        renderer.set_pose(pose);
                        let ui = snapshot.as_ref().map(|s| (s.0.clone(), s.1, s.2, s.3));
                        if let Err(error) = renderer.set_entities(&entities, partial, ui.as_ref()) {
                            self.fail(event_loop, error);
                            return;
                        }
                        if let Some(snapshot) = &snapshot {
                            let (hud, chat) = (&mut self.hud, &mut self.chat);
                            let result = renderer.draw_ui(tick, |canvas, registry| {
                                let frame = hud::Frame {
                                    view: &snapshot.0,
                                    vitals: snapshot.3,
                                    game_mode: snapshot.4,
                                    hardcore: snapshot.6,
                                    experience: snapshot.5,
                                    mouse,
                                    tick,
                                };
                                hud.draw(canvas, registry, &frame);
                                chat.draw(canvas, tick, mouse);
                                if snapshot.0.open && snapshot.3.health > 0.0 {
                                    hud::inventory(canvas, registry, &snapshot.0, mouse);
                                }
                            });
                            if let Err(error) = result {
                                self.fail(event_loop, error);
                                return;
                            }
                        }
                    }
                    let last = self
                        .smoke_frames
                        .is_some_and(|limit| self.frames + 1 >= limit)
                        && (self.network.is_none() || renderer.world_ready());
                    let capture = if last { self.capture.take() } else { None };
                    match renderer.render(capture.as_deref(), self.smoke_frames.is_some()) {
                        Ok(true) => {
                            if self.network.is_none() || renderer.world_ready() {
                                self.frames += 1;
                                if self.smoke_frames.is_some() && capture.is_none() {
                                    self.frame_stats.record(frame_started.elapsed());
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
                if self.smoke_frames.is_some_and(|limit| self.frames >= limit) {
                    let mode = if self.network.is_some() {
                        "WORLD_METAL"
                    } else {
                        "METAL"
                    };
                    self.frame_stats.report();
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
        // Upload rebuilt columns within a frame-sized budget.
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(6) {
            let update = self
                .network
                .as_ref()
                .and_then(|network| network.meshes.try_recv().ok());
            let Some(update) = update else {
                break;
            };
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
        if positioned && let Some(text) = self.smoke_chat.take() {
            self.open_chat("");
            for c in text.chars() {
                self.chat_key(KeyCode::KeyA, Some(&c.to_string()));
            }
            self.chat_key(KeyCode::Enter, None);
            self.open_chat("/");
            eprintln!("SMOKE_CHAT_SENT chars={}", text.chars().count());
        }
        if let Some(error) = self
            .network
            .as_ref()
            .and_then(|network| network.errors.try_recv().ok())
        {
            self.fail(event_loop, error);
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
            }
            self.next_frame = now + Duration::from_millis(16);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.release_cursor();
        self.network.take();
    }
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
    let server = match options.authenticate()? {
        options::Startup::Complete => return Ok(()),
        options::Startup::Window(server) => server,
    };
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
        smoke_frames,
        started: Instant::now(),
        next_frame: Instant::now(),
        failure: None,
        assets: options.assets,
        capture: options.capture,
        server,
        network: None,
        chat: chat::Chat::default(),
        hud: hud::Hud::default(),
        modifiers: ModifiersState::empty(),
        clipboard: None,
        gui_scale: options.gui_scale,
        wheel_pixels: 0.0,
        smoke_chat: options.smoke_chat,
    };
    EventLoop::new()?.run_app(&mut app)?;
    if let Some(error) = app.failure {
        return Err(error.into());
    }
    Ok(())
}
