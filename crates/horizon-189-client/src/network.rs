//! One ordered outbound actor and a bounded blocking reader. The actor owns
//! the legacy world and the local player's fixed 50 ms simulation; the
//! window thread shares keys and look angles and reads tick positions.
use crate::terrain::{ColumnMesh, Registry};
use horizon_auth::Session;
use horizon_physics_189::player::{Keys, Modifier, Player, SLOWNESS_UUID, SPEED_UUID};
use horizon_protocol_47::{
    Connection, PacketWriter, entities as entity_packets, login,
    play::{self, Movement, MovementReporter, PositionLook},
    player as fields,
};
use horizon_world_189::{
    BlockState, ColumnUpdate, World,
    block_entities::{BlockEntity, sign_text},
    decode_block_change, decode_bulk, decode_chunk, decode_multi_block_change,
    entities::{Entities, Entity},
};
use std::{
    collections::{HashMap, HashSet},
    io,
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub session: Arc<Session>,
    pub offline: bool,
    /// Scripted inputs for unattended validation against a reference server.
    pub autopilot: bool,
    /// Scripted flight instead of the walking script (needs allowFlying).
    pub autopilot_fly: bool,
    /// Requested view distance in chunks (2..=32; vanilla clamps to 16).
    pub view_distance: u8,
}

/// One rebuilt column for the renderer; `None` removes it.
pub struct MeshUpdate {
    pub key: (i32, i32),
    pub origin: [i64; 3],
    pub mesh: Option<ColumnMesh>,
    /// From the column first going dirty to its mesh being handed over.
    pub latency: Duration,
}

enum MeshCommand {
    Dirty(Vec<(i32, i32)>),
    Origin([i64; 3]),
    Focus([f64; 3]),
    /// A worker's finished column.
    Done(MeshUpdate),
}

/// Which dirty columns to mesh next: nearest first, never a column that is
/// already being meshed (a newer edit waits for the older mesh to land, so
/// meshes can't arrive out of order), and only results built against the
/// current origin are kept.
#[derive(Default)]
struct Dispatch {
    dirty: HashSet<(i32, i32)>,
    busy: HashSet<(i32, i32)>,
    origin: Option<[i64; 3]>,
    view: Option<crate::mesh_priority::View>,
    /// A fair-service turn every eight dispatches; hot columns keep their dirty age
    /// but must not monopolize that turn after already receiving service.
    dispatched: u64,
    served: HashMap<(i32, i32), Instant>,
    /// When each waiting column first went dirty, for mesh latency.
    since: HashMap<(i32, i32), Instant>,
}

impl Dispatch {
    /// Columns to (re)mesh; keeps the time each first went dirty.
    fn mark(&mut self, keys: impl IntoIterator<Item = (i32, i32)>) {
        let now = Instant::now();
        for key in keys {
            self.since.entry(key).or_insert(now);
            self.dirty.insert(key);
        }
    }

    fn next(&mut self, center: (i64, i64)) -> Option<((i32, i32), [i64; 3])> {
        self.next_at(center, Instant::now())
    }

    fn next_at(&mut self, center: (i64, i64), now: Instant) -> Option<((i32, i32), [i64; 3])> {
        let origin = self.origin?;
        let busy = &self.busy;
        let aged_turn = self.dispatched & 7 == 7;
        let &key = self
            .dirty
            .iter()
            .filter(|key| !busy.contains(key))
            .min_by_key(|&&(x, z)| {
                // u128 also covers hostile i32 column coordinates without overflow.
                let dx = (i128::from(x) - i128::from(center.0)).unsigned_abs();
                let dz = (i128::from(z) - i128::from(center.1)).unsigned_abs();
                let aged = self.since.get(&(x, z)).copied().filter(|since| {
                    now.saturating_duration_since(*since) >= Duration::from_millis(250)
                });
                let class = if aged_turn && aged.is_some() {
                    0u8 // Reserve one slot in eight for aged work, least recently served first.
                } else if dx <= 1 && dz <= 1 {
                    1 // Keep nearby collision terrain first on the ordinary turns.
                } else if aged.is_some() {
                    2
                } else if self.view.as_ref().is_none_or(|v| v.column_visible((x, z))) {
                    3
                } else {
                    4
                };
                let old = class == 0 || class == 2;
                (
                    class,
                    if old {
                        self.served
                            .get(&(x, z))
                            .copied()
                            .unwrap_or(aged.unwrap_or(now))
                    } else {
                        now
                    },
                    if old { aged.unwrap_or(now) } else { now },
                    dx * dx + dz * dz,
                    x,
                    z,
                )
            })?;
        self.dirty.remove(&key);
        self.busy.insert(key);
        self.dispatched = self.dispatched.wrapping_add(1);
        self.served.insert(key, now);
        Some((key, origin))
    }

    /// A finished mesh to forward, or `None` if it was built for an older
    /// origin (its column is meshed again).
    fn finish(&mut self, mut update: MeshUpdate) -> Option<MeshUpdate> {
        self.busy.remove(&update.key);
        if Some(update.origin) == self.origin {
            if !self.dirty.contains(&update.key) {
                if let Some(since) = self.since.remove(&update.key) {
                    update.latency = since.elapsed();
                }
                self.served.remove(&update.key);
            }
            Some(update)
        } else {
            self.dirty.insert(update.key);
            None
        }
    }

    fn idle(&self) -> bool {
        self.dirty.is_empty() && self.busy.is_empty()
    }
}

/// State the window thread and the simulation exchange every frame/tick.
#[derive(Default)]
pub struct Shared {
    /// Movement packets sent: one a tick, the Watchdog script's clock.
    pub moves_sent: u64,
    pub keys: Keys,
    pub input: crate::gameplay::Input,
    pub inventory: Arc<crate::inventory::View>,
    pub swing: Option<u8>,
    pub using_item: bool,
    pub game_mode: u8,
    /// The local player aims at an entity, so no block outline is drawn.
    pub aiming_entity: bool,
    /// The local player as a model for third person, stepped each tick.
    pub own_body: Option<horizon_world_189::entities::Entity>,
    /// F3: Join/Respawn/S41 difficulty, the S03 time of day (ticked here
    /// while the daylight cycle runs) and `hasReducedDebug`.
    pub difficulty: u8,
    pub time_of_day: i64,
    /// Join Game / Respawn level type "flat": `WorldType.FLAT` puts the
    /// horizon at 0 and turns off the low-altitude fog darkening.
    pub flat: bool,
    pub daylight_cycle: bool,
    pub reduced_debug: bool,
    /// A new render distance for the server (C15), sent on the next tick.
    pub view_distance: Option<u8>,
    /// Keep Chunks: the client's render distance, within which columns the
    /// server unloads stay drawn (the client-side chunk cache); `None` is off.
    pub chunk_cache: Option<u8>,
    pub vitals: crate::gameplay::Vitals,
    pub yaw: f32,
    pub pitch: f32,
    pub previous: [f64; 3],
    pub current: [f64; 3],
    pub ticked_at: Option<Instant>,
    pub positioned: bool,
    pub sneaking: bool,
    /// Tracked entities as of the last tick, for interpolated drawing.
    pub entities: Arc<Vec<(i32, Entity)>>,
    /// Chat the window has not taken yet, oldest first.
    pub chat_inbox: Vec<ChatEvent>,
    /// Chat and completion requests the actor has not sent yet.
    pub chat_outbox: Vec<Outgoing>,
    /// Experience bar fill and level (S1F).
    pub experience: (f32, i32),
    pub hardcore: bool,
    pub scoreboard: Arc<crate::scoreboard::Scoreboard>,
    pub tab: Arc<crate::scoreboard::TabList>,
    /// The session's player name and whether the login was encrypted
    /// (online mode, so the tab list shows faces).
    pub username: String,
    pub encrypted: bool,
    /// The local player's first-person and camera state, per tick.
    pub hand: crate::hand::Tracked,
    /// Critical-hit particles to start: entity and whether magic.
    pub effects: Vec<(i32, bool)>,
    /// The local player's UUID and tab-list profile (for the hand's skin).
    pub own: Option<(u128, Arc<horizon_protocol_47::player_list::Profile>)>,
    /// The local player's entity ID from the last join.
    pub entity_id: i32,
    /// A new local player invalidates UI gestures; a new world also invalidates particles.
    pub player_epoch: u64,
    pub world_epoch: u64,
    /// Blocks showing cracks: position, state and stage 0 to 9 (S25 from
    /// others, `curBlockDamageMP` for the local player).
    pub cracks: Vec<([i32; 3], horizon_world_189::BlockState, u8)>,
    /// Blocks broken since the window last looked (effect 2001 and the
    /// local player's own breaks), for break particles.
    pub visuals: Vec<crate::particles::Event>,
    pub breaks: Vec<([i32; 3], horizon_world_189::BlockState)>,
    /// Worn armour, helmet first, for the armour HUD.
    pub armor: [Option<horizon_protocol_47::nbt::Item>; 4],
    /// Active effects: ID, amplifier and remaining ticks (counted down each
    /// tick as `PotionEffect.onUpdate` does on the client).
    pub potions: Vec<(u8, i8, i32)>,
    /// Rebuild every column (a lighting setting changed).
    pub remesh: bool,
}

/// A server message for the window: chat, the action bar or completions.
pub enum ChatEvent {
    Message(horizon_protocol_47::text::Formatted),
    ActionBar(String),
    Completions(Vec<String>),
    /// S45 with any text already formatted.
    Title(horizon_protocol_47::scoreboard::Title),
}

/// What the window asks the actor to send.
pub enum Outgoing {
    Message(String),
    Complete(String),
}

/// Messages beyond this are dropped oldest-first if the window stalls.
const INBOX_LIMIT: usize = 512;

/// F3's mesher numbers, written by the mesher thread.
#[derive(Default)]
pub struct MeshStats {
    /// Latest rendered camera, overwritten without queueing or blocking the window.
    view: RwLock<Option<crate::mesh_priority::View>>,
    /// Columns waiting or being meshed.
    pub pending: AtomicUsize,
    /// Meshers with no column.
    pub idle: AtomicUsize,
    /// Meshes sent to the window in all.
    pub sent: AtomicU64,
}

pub struct Network {
    pub meshes: Receiver<MeshUpdate>,
    pub mesh_stats: Arc<MeshStats>,
    /// The world, for the window's particle collisions (`try_read` only).
    pub world: Arc<RwLock<World>>,
    pub errors: Receiver<String>,
    pub shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<TcpStream>>>,
    worker: Option<JoinHandle<()>>,
    mesher: Option<JoinHandle<()>>,
}

impl Network {
    pub fn prioritize(&self, view: Option<crate::mesh_priority::View>) {
        if let Ok(mut latest) = self.mesh_stats.view.try_write() {
            *latest = view;
        }
    }

    pub fn start(config: Config, registry: Arc<Registry>) -> io::Result<Self> {
        let (send, meshes) = mpsc::channel();
        let (commands, inbox) = mpsc::channel();
        let finished = commands.clone();
        let (failed, errors) = mpsc::sync_channel(1);
        let world = Arc::new(RwLock::new(World::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let socket = Arc::new(Mutex::new(None));
        let cancel = stop.clone();
        let active = socket.clone();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let state = shared.clone();
        let language = registry.gui.language.clone();
        let window_world = world.clone();
        let mesh_stats = Arc::new(MeshStats::default());
        let mesher = {
            let (world, stop, stats) = (world.clone(), stop.clone(), mesh_stats.clone());
            thread::Builder::new()
                .name("Legacy mesher".into())
                .spawn(move || {
                    mesh_columns(&registry, &world, &inbox, &finished, &send, &stop, &stats);
                })?
        };
        let worker = thread::Builder::new()
            .name("Protocol 47".into())
            .spawn(move || {
                crate::scheduling::apply("tick", crate::scheduling::Class::Interactive);
                if let Err(error) = run(
                    config, &world, commands, &state, &cancel, &active, &language,
                ) && !cancel.load(Ordering::Acquire)
                {
                    let _ = failed.try_send(error.to_string());
                }
                eprintln!("NETWORK_STOPPED");
            })?;
        Ok(Self {
            meshes,
            mesh_stats,
            world: window_world,
            errors,
            shared,
            stop,
            socket,
            worker: Some(worker),
            mesher: Some(mesher),
        })
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(active) = self.socket.lock()
            && let Some(socket) = active.as_ref()
        {
            let _ = socket.shutdown(Shutdown::Both);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(mesher) = self.mesher.take() {
            let _ = mesher.join();
        }
    }
}

/// Meshing workers: every core but the window, protocol and coordinator
/// threads, at least one.
fn mesher_count() -> usize {
    thread::available_parallelism().map_or(1, |cores| cores.get().saturating_sub(3).clamp(1, 8))
}

/// The mesher coordinator: hands the nearest dirty columns to worker
/// threads, one column per world read lock each, so neither the simulation
/// nor the window thread waits on meshing.
fn mesh_columns(
    registry: &Arc<Registry>,
    world: &Arc<RwLock<World>>,
    inbox: &Receiver<MeshCommand>,
    finished: &mpsc::Sender<MeshCommand>,
    send: &Sender<MeshUpdate>,
    stop: &Arc<AtomicBool>,
    stats: &MeshStats,
) {
    crate::scheduling::apply("mesh-coordinator", crate::scheduling::Class::Utility);
    let (jobs, queue) = mpsc::channel::<((i32, i32), [i64; 3])>();
    let queue = Arc::new(Mutex::new(queue));
    let workers = mesher_count();
    let mut handles = Vec::new();
    for index in 0..workers {
        let (registry, world, queue, finished, stop) = (
            registry.clone(),
            world.clone(),
            queue.clone(),
            finished.clone(),
            stop.clone(),
        );
        let spawned = thread::Builder::new()
            .name(format!("Legacy mesher {index}"))
            .spawn(move || {
                crate::scheduling::apply("mesher", crate::scheduling::Class::Utility);
                loop {
                    let job = match queue.lock() {
                        Ok(queue) => queue.recv(),
                        Err(_) => return,
                    };
                    let Ok((key, origin)) = job else {
                        return;
                    };
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    let mesh = match world.read() {
                        Ok(world) => world
                            .column(key.0, key.1)
                            .is_some()
                            .then(|| registry.mesh_column(&world, key, origin)),
                        Err(_) => return,
                    };
                    if finished
                        .send(MeshCommand::Done(MeshUpdate {
                            key,
                            origin,
                            mesh,
                            latency: Duration::ZERO,
                        }))
                        .is_err()
                    {
                        return;
                    }
                }
            });
        match spawned {
            Ok(handle) => handles.push(handle),
            Err(error) => eprintln!("MESHER_SPAWN_FAILED {error}"),
        }
    }
    eprintln!("MESHER_THREADS count={}", handles.len());
    let mut dispatch = Dispatch::default();
    let mut graphics = registry.graphics_epoch();
    let mut focus = [0.0f64; 3];
    let mut meshed = 0u64;
    let mut started: Option<Instant> = None;
    'run: while !stop.load(Ordering::Acquire) && !handles.is_empty() {
        let first = match inbox.recv_timeout(Duration::from_millis(100)) {
            Ok(command) => Some(command),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        // The Graphics setting changed how leaves mesh: every column again.
        let epoch = registry.graphics_epoch();
        if epoch != graphics {
            graphics = epoch;
            if let Ok(world) = world.read() {
                dispatch.mark(world.columns().map(|(key, _)| key));
            }
        }
        for command in first.into_iter().chain(inbox.try_iter()) {
            match command {
                MeshCommand::Dirty(keys) => dispatch.mark(keys),
                MeshCommand::Focus(position) => focus = position,
                MeshCommand::Origin(new) => {
                    dispatch.origin = Some(new);
                    if let Ok(world) = world.read() {
                        dispatch.mark(world.columns().map(|(key, _)| key));
                    }
                }
                MeshCommand::Done(update) => {
                    meshed += 1;
                    if let Some(update) = dispatch.finish(update) {
                        if send.send(update).is_err() {
                            break 'run;
                        }
                        stats.sent.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        let center = (
            (focus[0] / 16.0).floor() as i64,
            (focus[2] / 16.0).floor() as i64,
        );
        if let Ok(view) = stats.view.try_read() {
            if dispatch.view.is_none() && view.is_some() {
                eprintln!("MESH_VIEW_READY source=live-camera");
            }
            dispatch.view = *view;
        }
        while dispatch.busy.len() < handles.len() {
            let Some(job) = dispatch.next(center) else {
                break;
            };
            started.get_or_insert_with(Instant::now);
            if jobs.send(job).is_err() {
                break 'run;
            }
        }
        stats.pending.store(
            dispatch.dirty.len() + dispatch.busy.len(),
            Ordering::Relaxed,
        );
        stats.idle.store(
            handles.len().saturating_sub(dispatch.busy.len()),
            Ordering::Relaxed,
        );
        if dispatch.idle()
            && let Some(since) = started.take()
        {
            eprintln!(
                "MESHER_IDLE meshed={meshed} elapsed_ms={}",
                since.elapsed().as_millis()
            );
        }
    }
    drop(jobs);
    for handle in handles {
        let _ = handle.join();
    }
}

fn run(
    config: Config,
    world: &RwLock<World>,
    mesher: Sender<MeshCommand>,
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
    active: &Mutex<Option<TcpStream>>,
    language: &std::collections::BTreeMap<String, String>,
) -> io::Result<()> {
    let mut socket = None;
    let mut last_error = None;
    for address in (config.host.as_str(), config.port).to_socket_addrs()? {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if (config.autopilot || config.autopilot_fly) && !address.ip().is_loopback() {
            return Err(io::Error::other("Scripted movement requires loopback"));
        }
        match TcpStream::connect_timeout(&address, Duration::from_secs(5)) {
            Ok(connected) => {
                socket = Some(connected);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let socket = socket
        .ok_or_else(|| last_error.unwrap_or_else(|| io::Error::other("No server addresses")))?;
    socket.set_read_timeout(Some(Duration::from_secs(30)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    socket.set_nodelay(true)?;
    *active
        .lock()
        .map_err(|_| io::Error::other("Connection cancellation lock failed"))? =
        Some(socket.try_clone()?);
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut connection = Connection::new(socket);
    let mut encrypted = false;
    let profile = login::connect(
        &mut connection,
        &config.host,
        config.port,
        &config.session.profile.username,
        |hash| {
            encrypted = true;
            config.session.join(hash)
        },
    )?;
    {
        let mut state = lock(shared)?;
        state.username = profile.username.clone();
        state.encrypted = encrypted;
    }
    if !config.offline && profile != config.session.profile {
        return Err(io::Error::other(
            "Server profile differs from the authenticated profile",
        ));
    }
    eprintln!("NETWORK_LOGIN_PASS username={}", profile.username);
    let (mut reader, mut writer) = connection.split()?;
    let (received, packets) = mpsc::sync_channel(32);
    let incoming = thread::Builder::new()
        .name("Protocol 47 reader".into())
        .spawn(move || {
            // Knockback, velocity and entity moves arrive here: decrypting and
            // inflating them must not wait behind background work on the
            // efficiency cores.
            crate::scheduling::apply("packet-decoder", crate::scheduling::Class::Interactive);
            loop {
                let packet = reader.receive();
                let failed = packet.is_err();
                if let Ok(body) = &packet
                    && let Ok(play::Packet::Compression(threshold)) = play::decode(body)
                {
                    reader.set_compression(threshold);
                }
                if received.send(packet).is_err() || failed {
                    break;
                }
            }
        })?;
    let result = run_actor(
        world,
        &mesher,
        shared,
        &config,
        stop,
        &packets,
        &mut writer,
        language,
    );
    drop(packets);
    let _ = writer.shutdown();
    let _ = incoming.join();
    result
}

fn lock(shared: &Mutex<Shared>) -> io::Result<std::sync::MutexGuard<'_, Shared>> {
    shared
        .lock()
        .map_err(|_| io::Error::other("Shared simulation state poisoned"))
}

/// Keep Chunks: whether column `x`, `z` lies within `distance` columns
/// (vanilla's square) of the player's column.
fn within(center: (i32, i32), distance: u8, x: i32, z: i32) -> bool {
    x.abs_diff(center.0).max(z.abs_diff(center.1)) <= u32::from(distance)
}

/// The client-side chunk cache: an unload the server sends for a column still
/// inside the client's render distance is kept (the column stays drawn, frozen
/// as last seen); data for a kept column takes it back. Returns whether the
/// world changed.
fn apply_column(
    world: &mut World,
    update: ColumnUpdate,
    cached: &mut std::collections::HashSet<(i32, i32)>,
    keep: Option<((i32, i32), u8)>,
) -> bool {
    match update {
        ColumnUpdate::Unload { x, z }
            if keep.is_some_and(|(center, distance)| within(center, distance, x, z))
                && world.column(x, z).is_some() =>
        {
            cached.insert((x, z));
            false
        }
        update => {
            let (ColumnUpdate::Unload { x, z } | ColumnUpdate::Data { x, z, .. }) = &update;
            cached.remove(&(*x, *z));
            world.apply(update);
            true
        }
    }
}

/// Kept columns now outside the render distance (or all of them, with the
/// cache off), unloaded and returned.
fn evict_cached(
    world: &mut World,
    cached: &mut std::collections::HashSet<(i32, i32)>,
    keep: Option<((i32, i32), u8)>,
) -> Vec<(i32, i32)> {
    let gone: Vec<(i32, i32)> = cached
        .iter()
        .copied()
        .filter(|&(x, z)| !keep.is_some_and(|(center, distance)| within(center, distance, x, z)))
        .collect();
    for &(x, z) in &gone {
        cached.remove(&(x, z));
        world.apply(ColumnUpdate::Unload { x, z });
    }
    gone
}

/// The column and its four neighbours, whose culling and connections it affects.
fn around(x: i32, z: i32) -> [(i32, i32); 5] {
    [
        (x, z),
        (x.wrapping_add(1), z),
        (x.wrapping_sub(1), z),
        (x, z.wrapping_add(1)),
        (x, z.wrapping_sub(1)),
    ]
}

fn write(world: &RwLock<World>) -> io::Result<std::sync::RwLockWriteGuard<'_, World>> {
    world
        .write()
        .map_err(|_| io::Error::other("World lock poisoned"))
}

fn read(world: &RwLock<World>) -> io::Result<std::sync::RwLockReadGuard<'_, World>> {
    world
        .read()
        .map_err(|_| io::Error::other("World lock poisoned"))
}

/// `EnchantmentHelper.getDepthStriderModifier`: highest level (enchantment 8)
/// on the armour slots 5 to 8 of the player inventory window.
fn depth_strider(inventory: &[Option<horizon_protocol_47::nbt::Item>]) -> i32 {
    inventory
        .get(5..9)
        .into_iter()
        .flatten()
        .flatten()
        .map(|item| item.enchantment(8))
        .max()
        .unwrap_or(0)
}

/// WorldSettings.GameType.configurePlayerCapabilities. This is a server
/// update, not a user flight toggle; it does not owe a C13 packet.
fn apply_game_mode(
    player: &mut Player,
    capabilities: &mut entity_packets::Abilities,
    id: i32,
) -> u8 {
    let mode = if (0..=3).contains(&id) { id as u8 } else { 0 };
    capabilities.invulnerable = matches!(mode, 1 | 3);
    capabilities.creative = mode == 1;
    capabilities.allow_flying = matches!(mode, 1 | 3);
    capabilities.flying = match mode {
        1 => capabilities.flying,
        3 => true,
        _ => false,
    };
    player.allow_flying = capabilities.allow_flying;
    player.flying = capabilities.flying;
    player.no_clip = mode == 3;
    mode
}

fn apply_effect(player: &mut Player, effect: u8, amplifier: Option<i8>) {
    let modifier = |uuid: u128, per_level: f64, amplifier: i8| Modifier {
        uuid,
        amount: per_level * f64::from(i32::from(amplifier) + 1),
        operation: 2,
    };
    match (effect, amplifier) {
        (1, Some(level)) => player
            .speed
            .apply(modifier(SPEED_UUID, 0.20000000298023224, level)),
        (1, None) => player.speed.remove(SPEED_UUID),
        (2, Some(level)) => {
            player
                .speed
                .apply(modifier(SLOWNESS_UUID, -0.15000000596046448, level))
        }
        (2, None) => player.speed.remove(SLOWNESS_UUID),
        (8, level) => player.jump_boost = level,
        (15, level) => player.blindness = level.is_some(),
        _ => {}
    }
}

/// The validation script: idle, walk, sprint, sprint-jump, sneak, strafe while
/// turning, then backwards jumps, as ticks since the first position.
fn autopilot(tick: u64, fly: bool, shared: &mut Shared) {
    let mut keys = Keys::default();
    if fly {
        // Double-tap jump (toggle within seven ticks), climb, cruise, sprint,
        // then sneak down until landing ends the flight.
        match tick {
            20 | 21 | 24..40 => keys.jump = true,
            40..80 => keys.forward = true,
            80..100 => (keys.forward, keys.sprint) = (true, true),
            100..160 => keys.sneak = true,
            _ => {}
        }
        shared.keys = keys;
        if tick == 200 {
            eprintln!("AUTOPILOT_DONE ticks={tick}");
        }
        return;
    }
    match tick {
        20..60 => keys.forward = true,
        60..100 => (keys.forward, keys.sprint) = (true, true),
        100..160 => (keys.forward, keys.sprint, keys.jump) = (true, true, true),
        160..190 => (keys.forward, keys.sneak) = (true, true),
        190..230 => {
            keys.left = true;
            shared.yaw += 4.5;
        }
        230..260 => (keys.back, keys.jump) = (true, true),
        _ => {}
    }
    shared.keys = keys;
    if tick == 300 {
        eprintln!("AUTOPILOT_DONE ticks={tick}");
    }
}

fn push_chat(shared: &Mutex<Shared>, event: ChatEvent) -> io::Result<()> {
    let mut state = lock(shared)?;
    if state.chat_inbox.len() >= INBOX_LIMIT {
        state.chat_inbox.remove(0);
    }
    state.chat_inbox.push(event);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_actor(
    world: &RwLock<World>,
    mesher: &Sender<MeshCommand>,
    shared: &Mutex<Shared>,
    config: &Config,
    stop: &AtomicBool,
    packets: &Receiver<io::Result<Vec<u8>>>,
    writer: &mut PacketWriter<TcpStream>,
    language: &std::collections::BTreeMap<String, String>,
) -> io::Result<()> {
    let translate = |key: &str| language.get(key).cloned();
    let formatted = |json: &str| horizon_protocol_47::text::format(json, &translate).text;
    let mut board = crate::scoreboard::Scoreboard::default();
    let mut tab = crate::scoreboard::TabList::default();
    let (mut board_dirty, mut tab_dirty) = (false, false);
    let scripted = config.autopilot || config.autopilot_fly;
    let mut mesh_origin: Option<[i64; 3]> = None;
    let dirty = |keys: Vec<(i32, i32)>| {
        // A closed mesher only means the window is shutting down.
        let _ = mesher.send(MeshCommand::Dirty(keys));
    };
    let mut player = Player::default();
    let mut entities = Entities::default();
    let mut inventory = crate::inventory::Inventory::default();
    let mut controller = crate::gameplay::Controller::default();
    let mut game_mode = 0;
    let mut vitals = crate::gameplay::Vitals::default();
    let mut potions: Vec<(u8, i8, i32)> = Vec::new();
    let mut hand = crate::hand::Tracked::default();
    let mut own_body: Option<horizon_world_189::entities::Entity> = None;
    let mut capabilities = entity_packets::Abilities {
        invulnerable: false,
        flying: false,
        allow_flying: false,
        creative: false,
        fly_speed: 0.05,
        walk_speed: 0.1,
    };
    let mut entity_id = 0;
    // Other players' S25 crack stages, by breaker.
    let mut breaking: std::collections::HashMap<i32, ([i32; 3], u8)> = Default::default();
    // Keep Chunks: columns the server unloaded that stay drawn.
    let mut cached: std::collections::HashSet<(i32, i32)> = Default::default();
    let keep = |player: &Player, shared: &Mutex<Shared>| -> io::Result<Option<((i32, i32), u8)>> {
        let center = (
            (player.position[0] / 16.0).floor() as i32,
            (player.position[2] / 16.0).floor() as i32,
        );
        Ok(lock(shared)?.chunk_cache.map(|distance| (center, distance)))
    };
    let mut positioned = false;
    let mut sky_light = None;
    let mut dimension = None;
    let mut reporter = MovementReporter::default();
    let (mut server_sprint, mut server_sneak) = (false, false);
    // EntityLivingBase.deathTime: at 20 the dead player is set dead and no
    // longer updated, so it stops moving and reporting until respawn.
    let mut death_time = 0u32;
    // GuiDownloadTerrain's tick count while it is open.
    let mut terrain: Option<u32> = None;
    let tick = Duration::from_millis(50);
    let mut next_tick = Instant::now() + tick;
    let mut reports = 0u64;
    while !stop.load(Ordering::Acquire) {
        match packets.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
            Ok(packet) => {
                let body = packet?;
                match play::decode(&body)? {
                    play::Packet::Join(join) => {
                        lock(shared)?.flat = join.level_type.eq_ignore_ascii_case("flat");
                        // handleJoinGame: a new PlayerControllerMP (nothing
                        // sent yet) and a new, empty WorldClient. loadWorld
                        // keeps the existing EntityPlayerSP (inventory,
                        // selection, reported position, sprint and sneak) and
                        // preparePlayerToSpawn resets its size and motion.
                        if entity_id != 0 {
                            controller = crate::gameplay::Controller::default();
                            let mut world = write(world)?;
                            let keys: Vec<_> = world.columns().map(|(key, _)| key).collect();
                            *world = World::default();
                            drop(world);
                            dirty(keys);
                            breaking.clear();
                            cached.clear();
                            mesh_origin = None;
                            let [x, y, z] = player.position;
                            player.width = horizon_physics_189::player::WIDTH;
                            player.height = horizon_physics_189::player::HEIGHT;
                            player.set_position(x, y, z);
                            player.motion = [0.0; 3];
                            player.pitch = 0.0;
                            vitals.health = vitals.max_health;
                            positioned = false;
                            let mut state = lock(shared)?;
                            state.positioned = false;
                            state.pitch = 0.0;
                            state.world_epoch = state.world_epoch.wrapping_add(1);
                        }
                        entity_id = join.entity_id;
                        terrain = Some(0);
                        game_mode = apply_game_mode(
                            &mut player,
                            &mut capabilities,
                            i32::from(join.game_mode & 7),
                        );
                        {
                            let mut state = lock(shared)?;
                            state.hardcore = join.game_mode & 8 != 0;
                            state.entity_id = join.entity_id;
                            state.difficulty = join.difficulty;
                            state.reduced_debug = join.reduced_debug;
                        }
                        // A new WorldClient brings a new scoreboard.
                        board = crate::scoreboard::Scoreboard::default();
                        board_dirty = true;
                        entities.clear();
                        dimension = Some(i32::from(join.dimension));
                        sky_light = Some(join.dimension == 0);
                        write(world)?.set_sky(join.dimension == 0);
                        writer.send(&play::client_settings(config.view_distance))?;
                        writer.send(&play::brand()?)?;
                    }
                    play::Packet::KeepAlive(id) => writer.send(&play::keep_alive(id))?,
                    play::Packet::Compression(threshold) => writer.set_compression(threshold),
                    play::Packet::Disconnect(reason) => {
                        return Err(io::Error::new(io::ErrorKind::ConnectionAborted, reason));
                    }
                    play::Packet::Position { location, relative } => {
                        {
                            let state = lock(shared)?;
                            player.yaw = state.yaw;
                            player.pitch = state.pitch;
                        }
                        player.teleport(location.position, location.yaw, location.pitch, relative);
                        // The first S08 closes GuiDownloadTerrain.
                        terrain = None;
                        let pose = PositionLook {
                            position: player.position,
                            yaw: player.yaw,
                            pitch: player.pitch,
                        };
                        if pose.position.iter().any(|&value| {
                            value < f64::from(i32::MIN) || value > f64::from(i32::MAX)
                        }) {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Server position exceeds block coordinate range",
                            ));
                        }
                        writer.send(
                            &Movement::PositionLook {
                                location: pose,
                                on_ground: false,
                            }
                            .encode()?,
                        )?;
                        let origin = [
                            (pose.position[0] / 16.0).floor() as i64 * 16,
                            0,
                            (pose.position[2] / 16.0).floor() as i64 * 16,
                        ];
                        let far = mesh_origin.is_none_or(|current: [i64; 3]| {
                            (current[0] - origin[0]).abs() > 1024
                                || (current[2] - origin[2]).abs() > 1024
                        });
                        if far {
                            mesh_origin = Some(origin);
                            let _ = mesher.send(MeshCommand::Focus(pose.position));
                            let _ = mesher.send(MeshCommand::Origin(origin));
                        }
                        {
                            let mut state = lock(shared)?;
                            state.yaw = player.yaw;
                            state.pitch = player.pitch;
                            state.previous = player.position;
                            state.current = player.position;
                            state.ticked_at = Some(Instant::now());
                            state.positioned = true;
                        }
                        positioned = true;
                        eprintln!(
                            "NETWORK_POSITION x={} y={} z={}",
                            pose.position[0], pose.position[1], pose.position[2]
                        );
                    }
                    play::Packet::Other { id: 0x21, .. } => {
                        let update = decode_chunk(
                            &body,
                            sky_light.ok_or_else(|| io::Error::other("Chunk before Join Game"))?,
                        )?;
                        let (ColumnUpdate::Unload { x, z } | ColumnUpdate::Data { x, z, .. }) =
                            update;
                        let keep = keep(&player, shared)?;
                        let mut world = write(world)?;
                        let mut keys = Vec::new();
                        if apply_column(&mut world, update, &mut cached, keep) {
                            keys.extend(around(x, z));
                        }
                        for (x, z) in evict_cached(&mut world, &mut cached, keep) {
                            keys.extend(around(x, z));
                        }
                        drop(world);
                        if !keys.is_empty() {
                            dirty(keys);
                        }
                    }
                    play::Packet::Other { id: 0x26, .. } => {
                        let updates = decode_bulk(&body)?;
                        let mut keys = Vec::with_capacity(updates.len() * 5);
                        let keep = keep(&player, shared)?;
                        let mut world = write(world)?;
                        for update in updates {
                            let (ColumnUpdate::Unload { x, z } | ColumnUpdate::Data { x, z, .. }) =
                                update;
                            if apply_column(&mut world, update, &mut cached, keep) {
                                keys.extend(around(x, z));
                            }
                        }
                        for (x, z) in evict_cached(&mut world, &mut cached, keep) {
                            keys.extend(around(x, z));
                        }
                        drop(world);
                        dirty(keys);
                    }
                    play::Packet::Other {
                        id: 0x22 | 0x23, ..
                    } => {
                        let changes = if body[0] == 0x23 {
                            vec![decode_block_change(&body)?]
                        } else {
                            decode_multi_block_change(&body)?
                        };
                        let mut keys = Vec::new();
                        let mut world = write(world)?;
                        for change in changes {
                            if world.set_block(
                                change.x,
                                change.y,
                                change.z,
                                BlockState(change.state),
                            ) {
                                keys.extend(around(
                                    change.x.div_euclid(16),
                                    change.z.div_euclid(16),
                                ));
                            }
                        }
                        drop(world);
                        dirty(keys);
                    }
                    play::Packet::Other { id: 0x06, fields } => {
                        let health = fields::decode_health(fields)?;
                        // EntityPlayerSP.setPlayerSPHealth: a loss is a hurt.
                        if health.health < vitals.health {
                            hand.hurt();
                        }
                        player.food = health.food;
                        vitals.health = health.health;
                        vitals.food = health.food;
                        vitals.saturation = health.saturation;
                    }
                    play::Packet::Respawn(respawn) => {
                        lock(shared)?.flat = respawn.level_type.eq_ignore_ascii_case("flat");
                        // bcy.handleRespawn creates WorldClient only when the dimension changes.
                        // Minecraft.respawnPlayer always creates a fresh EntityPlayerSP.
                        let world_changed = dimension != Some(respawn.dimension);
                        dimension = Some(respawn.dimension);
                        inventory = crate::inventory::Inventory::default();
                        controller.reset_player();
                        vitals = crate::gameplay::Vitals::default();
                        potions.clear();
                        player = Player::default();
                        // PlayerControllerMP.flipPlayer initializes the new player's yaw.
                        player.yaw = -180.0;
                        capabilities = entity_packets::Abilities {
                            invulnerable: false,
                            flying: false,
                            allow_flying: false,
                            creative: false,
                            fly_speed: 0.05,
                            walk_speed: 0.1,
                        };
                        game_mode = apply_game_mode(
                            &mut player,
                            &mut capabilities,
                            i32::from(respawn.game_mode),
                        );
                        reporter = MovementReporter::default();
                        server_sprint = false;
                        server_sneak = false;
                        hand = crate::hand::Tracked::default();
                        sky_light = Some(respawn.dimension == 0);
                        if world_changed {
                            terrain = Some(0);
                            let mut world = write(world)?;
                            let keys: Vec<_> = world.columns().map(|(key, _)| key).collect();
                            *world = World::default();
                            world.set_sky(respawn.dimension == 0);
                            drop(world);
                            dirty(keys);
                            entities.clear_world();
                            breaking.clear();
                            cached.clear();
                            mesh_origin = None;
                        }
                        positioned = false;
                        let mut state = lock(shared)?;
                        state.positioned = false;
                        state.yaw = player.yaw;
                        state.pitch = player.pitch;
                        state.sneaking = false;
                        state.player_epoch = state.player_epoch.wrapping_add(1);
                        state.inventory = Arc::new(inventory.view.clone());
                        state.vitals = vitals;
                        state.hand = hand.clone();
                        own_body = None;
                        state.own_body = None;
                        state.swing = None;
                        state.using_item = false;
                        state.game_mode = game_mode;
                        state.input.actions.clear();
                        state.cracks.clear();
                        state.difficulty = respawn.difficulty;
                        if world_changed {
                            state.world_epoch = state.world_epoch.wrapping_add(1);
                            state.entities = Arc::default();
                            state.effects.clear();
                            state.visuals.clear();
                            state.breaks.clear();
                        }
                    }
                    play::Packet::Other { id: 0x38, fields } => {
                        let updates = horizon_protocol_47::player_list::decode(fields)?;
                        let own = config.session.profile.uuid;
                        for update in &updates {
                            if let horizon_protocol_47::player_list::Update::Add {
                                uuid,
                                profile,
                                ..
                            } = update
                                && *uuid == own
                            {
                                lock(shared)?.own = Some((own, Arc::new(profile.clone())));
                            }
                        }
                        tab.apply(&updates, &formatted);
                        tab_dirty = true;
                        entities.update_profiles(updates);
                    }
                    // S03: total world time, then the time of day, negated
                    // when the daylight cycle is off (WorldClient.setWorldTime).
                    play::Packet::Other { id: 0x03, fields } => {
                        if let Some(time) = fields.get(8..16) {
                            let time = i64::from_be_bytes(time.try_into().unwrap_or([0; 8]));
                            let mut state = lock(shared)?;
                            state.daylight_cycle = time >= 0;
                            state.time_of_day = time.saturating_abs();
                        }
                    }
                    // S41 server difficulty.
                    play::Packet::Other { id: 0x41, fields } => {
                        if let Some(&difficulty) = fields.first() {
                            lock(shared)?.difficulty = difficulty;
                        }
                    }
                    play::Packet::Other { id: 0x47, fields } => {
                        let (header, footer) =
                            horizon_protocol_47::scoreboard::decode_header_footer(fields)?;
                        let shown = |json: &str| {
                            let text = formatted(json);
                            (!horizon_protocol_47::text::strip(&text).is_empty()).then_some(text)
                        };
                        tab.header = shown(&header);
                        tab.footer = shown(&footer);
                        tab_dirty = true;
                    }
                    play::Packet::Other { id: 0x3b, fields } => {
                        board.objective(horizon_protocol_47::scoreboard::decode_objective(fields)?);
                        board_dirty = true;
                    }
                    play::Packet::Other { id: 0x3c, fields } => {
                        board.score(horizon_protocol_47::scoreboard::decode_score(fields)?);
                        board_dirty = true;
                    }
                    play::Packet::Other { id: 0x3d, fields } => {
                        let (slot, name) = horizon_protocol_47::scoreboard::decode_display(fields)?;
                        board.display(slot, name);
                        board_dirty = true;
                    }
                    play::Packet::Other { id: 0x3e, fields } => {
                        board.team(horizon_protocol_47::scoreboard::decode_team(fields)?);
                        board_dirty = true;
                    }
                    play::Packet::Other { id: 0x45, fields } => {
                        use horizon_protocol_47::scoreboard::Title;
                        let title = match horizon_protocol_47::scoreboard::decode_title(fields)? {
                            Title::Title(json) => Title::Title(formatted(&json)),
                            Title::Subtitle(json) => Title::Subtitle(formatted(&json)),
                            other => other,
                        };
                        push_chat(shared, ChatEvent::Title(title))?;
                    }
                    play::Packet::Other { id: 0x39, fields } => {
                        let abilities = entity_packets::decode_abilities(fields)?;
                        player.flying = abilities.flying;
                        player.allow_flying = abilities.allow_flying;
                        player.fly_speed = abilities.fly_speed;
                        capabilities = abilities;
                    }
                    play::Packet::Other { id: 0x2b, fields } => {
                        if fields.len() == 5 && fields[0] == 3 {
                            let id = f32::from_be_bytes(fields[1..5].try_into().unwrap()) as i32;
                            game_mode = apply_game_mode(&mut player, &mut capabilities, id);
                        }
                    }
                    play::Packet::Other { id: 0x09, fields } => {
                        if let Some(slot) = crate::inventory::server_hotbar(fields)? {
                            inventory.view.selected = slot;
                        }
                    }
                    play::Packet::Other { id: 0x2d, fields } => {
                        let title = |json: &str| {
                            horizon_protocol_47::text::format(json, &translate).unformatted
                        };
                        inventory.open_packet(fields, &title)?;
                    }
                    play::Packet::Other { id: 0x2e, fields } => {
                        if fields.first() == Some(&inventory.view.window) {
                            inventory.close();
                        }
                    }
                    play::Packet::Other { id: 0x31, fields } => {
                        let (window, property, value) = crate::inventory::window_property(fields)?;
                        if window == inventory.view.window
                            && let Some(slot) = usize::try_from(property)
                                .ok()
                                .and_then(|index| inventory.view.properties.get_mut(index))
                        {
                            *slot = value;
                        }
                    }
                    play::Packet::Other { id: 0x32, fields } => {
                        if let Some(packet) = inventory.confirm(fields)? {
                            writer.send(&packet)?;
                        }
                    }
                    play::Packet::Other { id: 0x2f, fields } => {
                        let (window, slot, item) = entity_packets::decode_set_slot(fields)?;
                        inventory.set_slot(window as u8, slot, item);
                    }
                    play::Packet::Other { id: 0x30, fields } => {
                        let (window, items) = entity_packets::decode_window_items(fields)?;
                        inventory.items(window, items);
                    }
                    play::Packet::Other { id: 0x33, fields } => {
                        let (at, lines) = entity_packets::decode_sign(fields)?;
                        let text = lines.map(|line| sign_text(&line));
                        if write(world)?.set_block_entity(at, Some(BlockEntity::Sign(text))) {
                            dirty(around(at[0].div_euclid(16), at[2].div_euclid(16)).to_vec());
                        }
                    }
                    play::Packet::Other { id: 0x24, fields } => {
                        // TileEntityChest/EnderChest.receiveClientEvent(1, viewers).
                        let (at, event, viewers, _) = entity_packets::decode_block_action(fields)?;
                        if event == 1 {
                            write(world)?.chest_viewers(at, viewers);
                        }
                    }
                    play::Packet::Other { id: 0x35, fields } => {
                        let (at, action, nbt) = entity_packets::decode_block_entity(fields)?;
                        let entity = nbt
                            .as_ref()
                            .and_then(|tag| BlockEntity::from_nbt(action, tag));
                        if entity.is_some() && write(world)?.set_block_entity(at, entity) {
                            dirty(around(at[0].div_euclid(16), at[2].div_euclid(16)).to_vec());
                        }
                    }
                    play::Packet::Other { id, fields }
                        if matches!(
                            id,
                            0x04 | 0x0b | 0x0c | 0x0e | 0x0f | 0x11 | 0x13 | 0x15..=0x1a | 0x1c
                        ) =>
                    {
                        if let Some(packet) = entity_packets::decode(id, fields)? {
                            match &packet {
                                entity_packets::EntityPacket::Status { id, status: 2 }
                                    if *id == entity_id =>
                                {
                                    hand.hurt();
                                }
                                // EntityPlayer.handleStatusUpdate 22 and 23.
                                entity_packets::EntityPacket::Status {
                                    id,
                                    status: status @ (22 | 23),
                                } if *id == entity_id => {
                                    lock(shared)?.reduced_debug = *status == 22;
                                }
                                // handleStatusUpdate(3) runs EntityPlayer.onDeath.
                                entity_packets::EntityPacket::Status { id, status: 3 }
                                    if *id == entity_id =>
                                {
                                    player.on_death();
                                }
                                entity_packets::EntityPacket::Animation {
                                    id,
                                    animation: animation @ (4 | 5),
                                } => {
                                    let mut state = lock(shared)?;
                                    if state.effects.len() < 64 {
                                        state.effects.push((*id, *animation == 5));
                                    }
                                }
                                _ => {}
                            }
                            if let entity_packets::EntityPacket::Metadata { id, entries } = &packet
                                && *id == entity_id
                            {
                                for (index, value) in entries {
                                    match (index, value) {
                                        (17, horizon_protocol_47::nbt::Meta::Float(amount)) => {
                                            vitals.absorption = *amount;
                                        }
                                        (18, horizon_protocol_47::nbt::Meta::Int(score)) => {
                                            vitals.score = *score;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            entities.apply(packet);
                        }
                    }
                    play::Packet::Other { id: 0x12, fields } => {
                        let (entity, [x, y, z]) = fields::decode_velocity(fields)?;
                        if entity == entity_id {
                            player.set_velocity(x, y, z);
                        } else {
                            entities.set_velocity(entity, [x, y, z]);
                        }
                    }
                    play::Packet::Other { id: 0x28, fields } => {
                        // S28 Effect: id, position, data, global.
                        if let [a, b, c, d, rest @ ..] = fields
                            && i32::from_be_bytes([*a, *b, *c, *d]) == 2001
                            && rest.len() >= 12
                        {
                            let mut packed = [0u8; 8];
                            packed.copy_from_slice(&rest[..8]);
                            let packed = i64::from_be_bytes(packed);
                            let data = i32::from_be_bytes([rest[8], rest[9], rest[10], rest[11]]);
                            let at = [
                                (packed >> 38) as i32,
                                ((packed << 26) >> 52) as i32,
                                ((packed << 38) >> 38) as i32,
                            ];
                            // Block.getStateId: id + (meta << 12).
                            let state = horizon_world_189::BlockState(
                                (((data & 0xfff) as u16) << 4) | ((data >> 12) & 15) as u16,
                            );
                            let mut shared_state = lock(shared)?;
                            if shared_state.breaks.len() < 64 {
                                shared_state.breaks.push((at, state));
                            }
                        }
                    }
                    play::Packet::Other { id: 0x25, fields } => {
                        // S25 Block Break Animation: breaker, position, stage.
                        let mut input = std::io::Cursor::new(fields);
                        let breaker = horizon_protocol_47::read_varint(&mut input)?;
                        let mut packed = [0u8; 8];
                        std::io::Read::read_exact(&mut input, &mut packed)?;
                        let mut stage = [0u8; 1];
                        std::io::Read::read_exact(&mut input, &mut stage)?;
                        let packed = i64::from_be_bytes(packed);
                        let at = [
                            (packed >> 38) as i32,
                            ((packed << 26) >> 52) as i32,
                            ((packed << 38) >> 38) as i32,
                        ];
                        if stage[0] < 10 {
                            breaking.insert(breaker, (at, stage[0]));
                        } else {
                            breaking.remove(&breaker);
                        }
                    }
                    play::Packet::Other { id: 0x0d, fields } => {
                        let mut input = std::io::Cursor::new(fields);
                        let collected = horizon_protocol_47::read_varint(&mut input)?;
                        let collector = horizon_protocol_47::read_varint(&mut input)?;
                        entities.collect(collected, collector);
                    }
                    play::Packet::Other { id: 0x2a, fields } => {
                        let packet = horizon_protocol_47::effects::particle(fields)?;
                        if matches!(packet.id, 0..=2 | 11 | 12) {
                            let mut state = lock(shared)?;
                            if state.visuals.len() < 64 {
                                state
                                    .visuals
                                    .push(crate::particles::Event::Particle(packet));
                            }
                        }
                    }
                    play::Packet::Other { id: 0x27, fields } => {
                        let explosion = horizon_protocol_47::effects::explosion(fields)?;
                        let [x, y, z] = explosion.motion;
                        player.add_velocity(x, y, z);
                        let mut keys = Vec::new();
                        let mut world = write(world)?;
                        for &[x, y, z] in &explosion.blocks {
                            if world.set_block(x, y, z, BlockState(0)) {
                                world.set_block_entity([x, y, z], None);
                                keys.extend(around(x.div_euclid(16), z.div_euclid(16)));
                            }
                        }
                        drop(world);
                        dirty(keys);
                        let mut state = lock(shared)?;
                        if state.visuals.len() < 64 {
                            state
                                .visuals
                                .push(crate::particles::Event::Explosion(explosion));
                        }
                    }
                    play::Packet::Other { id: 0x20, fields } => {
                        let (entity, properties) = fields::decode_properties(fields)?;
                        if entity == entity_id {
                            for property in properties {
                                if property.key == "generic.maxHealth" {
                                    let mut attribute =
                                        horizon_physics_189::player::Attribute::new(property.base);
                                    for &(uuid, amount, operation) in &property.modifiers {
                                        attribute.apply(Modifier {
                                            uuid,
                                            amount,
                                            operation,
                                        });
                                    }
                                    vitals.max_health = attribute.value() as f32;
                                }
                                if property.key == "generic.movementSpeed" {
                                    let modifiers: Vec<Modifier> = property
                                        .modifiers
                                        .iter()
                                        .map(|&(uuid, amount, operation)| Modifier {
                                            uuid,
                                            amount,
                                            operation,
                                        })
                                        .collect();
                                    player.speed.replace(property.base, &modifiers);
                                }
                            }
                        }
                    }
                    play::Packet::Other { id: 0x1d, fields } => {
                        let effect = fields::decode_effect(fields)?;
                        if effect.entity == entity_id {
                            if effect.effect < 32 {
                                vitals.effects |= 1 << effect.effect;
                            }
                            match effect.effect {
                                3 => controller.haste = Some(effect.amplifier),
                                4 => controller.fatigue = Some(effect.amplifier),
                                _ => {}
                            }
                            apply_effect(&mut player, effect.effect, Some(effect.amplifier));
                            potions.retain(|p| p.0 != effect.effect);
                            potions.push((effect.effect, effect.amplifier, effect.duration));
                        }
                    }
                    play::Packet::Other { id: 0x1e, fields } => {
                        let (entity, effect) = fields::decode_remove_effect(fields)?;
                        if entity == entity_id {
                            if effect < 32 {
                                vitals.effects &= !(1 << effect);
                            }
                            match effect {
                                3 => controller.haste = None,
                                4 => controller.fatigue = None,
                                _ => {}
                            }
                            apply_effect(&mut player, effect, None);
                            potions.retain(|p| p.0 != effect);
                        }
                    }
                    play::Packet::Other { id: 0x02, fields } => {
                        let (json, position) = horizon_protocol_47::chat::decode_message(fields)?;
                        let message = horizon_protocol_47::text::format(&json, &translate);
                        let event = if position == horizon_protocol_47::chat::Position::ActionBar {
                            ChatEvent::ActionBar(message.unformatted)
                        } else {
                            ChatEvent::Message(message)
                        };
                        push_chat(shared, event)?;
                    }
                    play::Packet::Other { id: 0x3a, fields } => {
                        let matches = horizon_protocol_47::chat::decode_tab_complete(fields)?;
                        push_chat(shared, ChatEvent::Completions(matches))?;
                    }
                    play::Packet::Other { id: 0x1f, fields } => {
                        let input = &mut io::Cursor::new(fields);
                        let mut bar = [0; 4];
                        io::Read::read_exact(input, &mut bar)?;
                        let level = horizon_protocol_47::read_varint(input)?;
                        lock(shared)?.experience = (f32::from_be_bytes(bar), level);
                    }
                    _ => {}
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Packet reader stopped",
                ));
            }
        }
        let outgoing = std::mem::take(&mut lock(shared)?.chat_outbox);
        for request in outgoing {
            match request {
                Outgoing::Message(text) => {
                    writer.send(&horizon_protocol_47::chat::message(&text)?)?;
                }
                Outgoing::Complete(text) => {
                    writer.send(&horizon_protocol_47::chat::tab_complete(&text)?)?;
                }
            }
        }
        let now = Instant::now();
        if now < next_tick {
            continue;
        }
        // Publish scoreboard and tab changes once per tick, not per packet.
        if board_dirty || tab_dirty {
            let mut state = lock(shared)?;
            if std::mem::take(&mut board_dirty) {
                state.scoreboard = Arc::new(board.clone());
            }
            if std::mem::take(&mut tab_dirty) {
                state.tab = Arc::new(tab.clone());
            }
        }
        let late = now.duration_since(next_tick).as_nanos();
        let due = late / tick.as_nanos() + 1;
        // Vanilla's timer executes at most ten accumulated client ticks.
        // World.updateEntities ticks chest lids even without a player.
        {
            let mut lids = Vec::new();
            let mut world = write(world)?;
            for _ in 0..due.min(10) {
                lids.extend(world.tick_lids());
            }
            drop(world);
            if !lids.is_empty() {
                dirty(
                    lids.iter()
                        .map(|at| (at[0].div_euclid(16), at[2].div_euclid(16)))
                        .collect(),
                );
            }
        }
        let mut world_view = read(world)?;
        for _ in 0..due.min(10) {
            // Minecraft.runTick: updateController, then the open screen's
            // updateScreen, even without a ticking player (downloading
            // terrain, unloaded chunk).
            controller.sync_slot(inventory.view.selected, writer)?;
            // GuiDownloadTerrain.updateScreen: C00 with key 0 every second.
            if let Some(progress) = &mut terrain {
                *progress += 1;
                if progress.is_multiple_of(20) {
                    writer.send(&play::keep_alive(0))?;
                }
            }
            if !positioned || !player.updates() {
                continue;
            }
            let (keys, mut input) = {
                let mut state = lock(shared)?;
                if scripted {
                    autopilot(reports, config.autopilot_fly, &mut state);
                }
                player.yaw = state.yaw;
                player.pitch = state.pitch;
                (
                    state.keys,
                    crate::gameplay::Input {
                        attack: state.input.attack,
                        using: state.input.using,
                        actions: std::mem::take(&mut state.input.actions),
                    },
                )
            };
            if vitals.health <= 0.0 {
                // GuiContainer.updateScreen closes an open screen (C0D) for a
                // dead player; GuiGameOver only appears once none is open.
                if inventory.view.open {
                    input.actions.clear();
                    input
                        .actions
                        .push_back(crate::gameplay::Action::CloseInventory);
                } else {
                    input
                        .actions
                        .retain(|a| matches!(a, crate::gameplay::Action::Respawn));
                }
                input.attack = false;
                input.using = false;
            }
            controller.invulnerable = capabilities.invulnerable;
            controller.tick(
                &mut input,
                &mut player,
                &world_view,
                &entities,
                &mut inventory,
                game_mode,
                writer,
            )?;
            // Client-side placements and breaks land before the player moves,
            // as rightClickMouse/clickMouse run before the entity update.
            if !controller.edits.is_empty() {
                drop(world_view);
                let mut keys = Vec::new();
                let mut edited = write(world)?;
                for (at, state) in controller.edits.drain(..) {
                    if edited.set_block(at[0], at[1], at[2], state) {
                        keys.extend(around(at[0].div_euclid(16), at[2].div_euclid(16)));
                    }
                }
                drop(edited);
                dirty(keys);
                world_view = read(world)?;
            }
            {
                let mut state = lock(shared)?;
                state.inventory = Arc::new(inventory.view.clone());
                state.swing = controller.swing;
                state.using_item = controller.using;
                state.game_mode = game_mode;
                const ARMOR: [[u8; 4]; 5] = [
                    [1, 3, 2, 1],
                    [2, 5, 4, 1],
                    [2, 6, 5, 2],
                    [3, 8, 6, 3],
                    [2, 5, 3, 1],
                ];
                vitals.armor = inventory.player[5..9]
                    .iter()
                    .flatten()
                    .filter_map(|item| {
                        let offset = item.id.checked_sub(298)?;
                        if (0..20).contains(&offset) {
                            Some(ARMOR[offset as usize / 4][offset as usize % 4])
                        } else {
                            None
                        }
                    })
                    .sum();
                state.vitals = vitals;
                for potion in &mut potions {
                    potion.2 = potion.2.saturating_sub(1).max(0);
                }
                state.armor = std::array::from_fn(|i| inventory.player[5 + i].clone());
                state.potions.clone_from(&potions);
                if state.input.actions.is_empty() {
                    state.input.actions = std::mem::take(&mut input.actions);
                }
            }
            player.no_clip = game_mode == 3;
            death_time = if vitals.health > 0.0 {
                0
            } else {
                death_time.saturating_add(1)
            };
            let updated = death_time <= 20;
            if updated {
                // getDepthStriderModifier reads the worn armour every move,
                // including armour placed by predicted window clicks.
                player.depth_strider = depth_strider(&inventory.player);
                // isMovementBlocked: a dead player still moves, without input.
                let keys = if vitals.health > 0.0 {
                    keys
                } else {
                    Keys::default()
                };
                player.tick(&world_view, keys);
            }
            // sendPlayerAbilities happens inside onLivingUpdate, before the
            // walking update's actions and movement.
            if std::mem::take(&mut player.abilities_changed) && updated {
                capabilities.flying = player.flying;
                writer.send(&entity_packets::encode_abilities(capabilities))?;
            }
            // EntityPlayerSP.onUpdateWalkingPlayer: actions, then movement.
            if updated && player.sprinting != server_sprint {
                let action = if player.sprinting {
                    fields::START_SPRINTING
                } else {
                    fields::STOP_SPRINTING
                };
                writer.send(&fields::entity_action(entity_id, action))?;
                server_sprint = player.sprinting;
            }
            if updated && player.sneaking != server_sneak {
                let action = if player.sneaking {
                    fields::START_SNEAKING
                } else {
                    fields::STOP_SNEAKING
                };
                writer.send(&fields::entity_action(entity_id, action))?;
                server_sneak = player.sneaking;
            }
            let pose = PositionLook {
                position: player.position,
                yaw: player.yaw,
                pitch: player.pitch,
            };
            if updated {
                writer.send(&reporter.report(pose, player.on_ground)?)?;
            }
            {
                let mut state = lock(shared)?;
                if updated {
                    state.moves_sent += 1;
                }
                if let Some(chunks) = state.view_distance.take() {
                    writer.send(&play::client_settings(chunks))?;
                }
                state.previous = state.current;
                if state.daylight_cycle {
                    state.time_of_day = state.time_of_day.saturating_add(1);
                }
                state.current = player.position;
                state.ticked_at = Some(Instant::now());
                state.sneaking = player.sneaking;
                hand.tick(
                    inventory.held(),
                    &player,
                    controller.using.then_some(controller.use_ticks),
                    vitals.health > 0.0,
                    capabilities.walk_speed,
                );
                state.hand = hand.clone();
                // Watcher flags: sneaking 0x02, sprinting 0x08, using 0x10.
                let flags = u8::from(player.sneaking) << 1
                    | u8::from(player.sprinting) << 3
                    | u8::from(controller.using) << 4;
                if own_body.is_none() {
                    own_body = local_body();
                }
                if let Some(body) = &mut own_body {
                    body.tick_local(player.position, player.yaw, player.pitch, flags);
                    state.own_body = Some(body.clone());
                }
                // `RenderGlobal.drawBlockDamageTexture`'s list, own dig last.
                let own = controller.dig_progress().and_then(|(at, damage)| {
                    let stage = (damage * 10.0) as i32 - 1;
                    (0..10).contains(&stage).then_some((at, stage as u8))
                });
                for at in controller.broken.drain(..) {
                    if let Some(block) = world_view
                        .block(at[0], at[1], at[2])
                        .filter(|block| block.block_id() != 0)
                        && state.breaks.len() < 64
                    {
                        state.breaks.push((at, block));
                    }
                }
                state.cracks.clear();
                for (at, stage) in breaking.values().copied().chain(own) {
                    if let Some(block) = world_view
                        .block(at[0], at[1], at[2])
                        .filter(|block| block.block_id() != 0)
                    {
                        state.cracks.push((at, block, stage));
                    }
                }
                state.aiming_entity = controller.aiming_entity;
                if state.effects.len() < 64 {
                    state.effects.append(&mut controller.effects);
                }
                controller.effects.clear();
            }
            reports += 1;
            if reports.is_multiple_of(20) {
                eprintln!(
                    "PLAYER_TICKS reports={reports} on_ground={} x={:.3} y={:.3} z={:.3} sprinting={} flying={} entities={}",
                    player.on_ground,
                    player.position[0],
                    player.position[1],
                    player.position[2],
                    player.sprinting,
                    player.flying,
                    entities.len(),
                );
            }
        }
        drop(world_view);
        if positioned && due > 0 {
            let world_view = read(world)?;
            for _ in 0..due.min(10) {
                entities.tick(&world_view);
            }
            drop(world_view);
            let mut state = lock(shared)?;
            let snapshot = Arc::make_mut(&mut state.entities);
            snapshot.clear();
            snapshot.extend(entities.iter().map(|(id, entity)| (id, entity.clone())));
        }
        let remainder = Duration::from_nanos((late % tick.as_nanos()) as u64);
        next_tick = now + tick - remainder;
        if positioned {
            let _ = mesher.send(MeshCommand::Focus(player.position));
        }
        if std::mem::take(&mut lock(shared)?.remesh) {
            let keys: Vec<_> = read(world)?.columns().map(|(key, _)| key).collect();
            let _ = mesher.send(MeshCommand::Dirty(keys));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;

/// A player model for the local player, whose position and look the tick
/// sets; skin, equipment and swing are filled in each frame.
fn local_body() -> Option<horizon_world_189::entities::Entity> {
    let mut tracked = horizon_world_189::entities::Entities::default();
    tracked.apply(horizon_protocol_47::entities::EntityPacket::SpawnPlayer {
        id: 0,
        uuid: 0,
        position: [0; 3],
        yaw: 0,
        pitch: 0,
        held_item: 0,
        metadata: vec![],
    });
    tracked.get(0).cloned()
}

#[cfg(test)]
mod dispatch_tests {
    use super::{Dispatch, MeshUpdate};

    fn done(key: (i32, i32), origin: [i64; 3]) -> MeshUpdate {
        MeshUpdate {
            key,
            origin,
            mesh: None,
            latency: std::time::Duration::ZERO,
        }
    }

    #[test]
    fn mesh_latency_runs_from_first_dirty_to_the_last_rebuild() {
        let mut dispatch = Dispatch {
            origin: Some([0; 3]),
            ..Dispatch::default()
        };
        dispatch.mark([(0, 0)]);
        let first = dispatch.since[&(0, 0)];
        std::thread::sleep(std::time::Duration::from_millis(5));
        // Dirty again before it meshes: the clock keeps the first time.
        dispatch.mark([(0, 0)]);
        assert_eq!(dispatch.since[&(0, 0)], first);
        let (key, origin) = dispatch.next((0, 0)).unwrap();
        // Edited while meshing: this mesh is stale, so no latency yet.
        dispatch.mark([(0, 0)]);
        let stale = dispatch.finish(done(key, origin)).unwrap();
        assert_eq!(stale.latency, std::time::Duration::ZERO);
        let (key, origin) = dispatch.next((0, 0)).unwrap();
        let fresh = dispatch.finish(done(key, origin)).unwrap();
        assert!(fresh.latency >= std::time::Duration::from_millis(5));
        assert!(dispatch.since.is_empty());
    }

    #[test]
    fn meshing_priority_handles_extreme_server_column_coordinates() {
        let mut dispatch = Dispatch {
            origin: Some([0; 3]),
            ..Dispatch::default()
        };
        dispatch.mark([(i32::MIN, i32::MIN), (0, 2)]);
        assert_eq!(dispatch.next((0, 0)), Some(((0, 2), [0; 3])));
        assert_eq!(dispatch.next((0, 0)), Some(((i32::MIN, i32::MIN), [0; 3])));
    }

    #[test]
    fn continuously_redirtied_nearby_work_cannot_starve_an_old_hidden_column() {
        use std::time::{Duration, Instant};
        let mut dispatch = Dispatch {
            origin: Some([0; 3]),
            ..Dispatch::default()
        };
        dispatch.mark([(1, 0), (0, -4)]);
        let now = Instant::now() + Duration::from_millis(300);
        let mut serviced_hidden = false;
        for _ in 0..8 {
            let (key, origin) = dispatch.next_at((0, 0), now).unwrap();
            if key == (0, -4) {
                serviced_hidden = true;
                break;
            }
            // Another edit arrives before this nearby mesh completes, retaining its dirty age.
            dispatch.mark([key]);
            dispatch.finish(done(key, origin));
        }
        assert!(
            serviced_hidden,
            "reserve service for old work even under saturated nearby edits"
        );
    }

    #[test]
    fn frustum_priority_keeps_near_columns_first_and_ages_hidden_work() {
        use glam::Vec4;
        use std::time::{Duration, Instant};
        for origin in [[0; 3], [30_000_000, 0, -30_000_000]] {
            let center = (origin[0] / 16, origin[2] / 16);
            let key = |x, z| ((center.0 + x) as i32, (center.1 + z) as i32);
            let planes = [Vec4::new(0.0, 0.0, 1.0, 0.0); 5];
            let mut dispatch = Dispatch {
                origin: Some(origin),
                view: crate::mesh_priority::View::new(origin, planes),
                ..Dispatch::default()
            };
            dispatch.mark([key(1, 0), key(0, -2), key(0, 4)]);
            assert_eq!(
                dispatch.next(center).unwrap().0,
                key(1, 0),
                "near collision neighbourhood first"
            );
            assert_eq!(
                dispatch.next(center).unwrap().0,
                key(0, 4),
                "visible before a nearer hidden column"
            );
            let now = Instant::now() + Duration::from_millis(300);
            dispatch.mark([key(0, 3)]);
            dispatch.since.insert(key(0, 3), now);
            assert_eq!(
                dispatch.next_at(center, now).unwrap().0,
                key(0, -2),
                "old hidden work cannot starve"
            );
            dispatch.view = crate::mesh_priority::View::new(origin, [Vec4::splat(f32::NAN); 5]);
            assert!(
                dispatch.view.is_none(),
                "invalid camera falls back to distance ordering"
            );
            assert_eq!(dispatch.next(center).unwrap().0, key(0, 3));
        }
    }

    #[test]
    fn columns_mesh_nearest_first_and_never_twice_at_once() {
        let mut dispatch = Dispatch::default();
        dispatch.dirty.extend([(5, 0), (1, 0), (2, 2)]);
        assert_eq!(dispatch.next((0, 0)), None, "no origin yet");
        dispatch.origin = Some([0; 3]);
        assert_eq!(dispatch.next((0, 0)), Some(((1, 0), [0; 3])));
        // Edited again while meshing: it waits for the first mesh.
        dispatch.dirty.insert((1, 0));
        assert_eq!(dispatch.next((0, 0)), Some(((2, 2), [0; 3])));
        assert_eq!(dispatch.next((0, 0)), Some(((5, 0), [0; 3])));
        assert_eq!(dispatch.next((0, 0)), None);
        assert!(dispatch.finish(done((1, 0), [0; 3])).is_some());
        assert_eq!(dispatch.next((0, 0)), Some(((1, 0), [0; 3])));
        assert!(!dispatch.idle());
    }

    #[test]
    fn meshes_for_an_old_origin_are_rebuilt_not_shown() {
        let mut dispatch = Dispatch {
            origin: Some([0; 3]),
            ..Dispatch::default()
        };
        dispatch.dirty.insert((3, 3));
        let (key, origin) = dispatch.next((0, 0)).expect("job");
        dispatch.origin = Some([1024, 0, 0]);
        assert!(dispatch.finish(done(key, origin)).is_none());
        assert_eq!(dispatch.next((0, 0)), Some(((3, 3), [1024, 0, 0])));
        assert!(dispatch.finish(done((3, 3), [1024, 0, 0])).is_some());
        assert!(dispatch.idle());
    }
}
