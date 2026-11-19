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
    collections::HashSet,
    io,
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct Config {
    pub host: String,
    pub port: u16,
    pub session: Session,
    pub offline: bool,
    /// Scripted inputs for unattended validation against a reference server.
    pub autopilot: bool,
    /// Scripted flight instead of the walking script (needs allowFlying).
    pub autopilot_fly: bool,
    /// Requested view distance in chunks (vanilla clamps to 2..=16).
    pub view_distance: u8,
}

/// One rebuilt column for the renderer; `None` removes it.
pub struct MeshUpdate {
    pub key: (i32, i32),
    pub origin: [i64; 3],
    pub mesh: Option<ColumnMesh>,
}

enum MeshCommand {
    Dirty(Vec<(i32, i32)>),
    Origin([i64; 3]),
    Focus([f64; 3]),
}

/// State the window thread and the simulation exchange every frame/tick.
#[derive(Default)]
pub struct Shared {
    pub keys: Keys,
    pub yaw: f32,
    pub pitch: f32,
    pub previous: [f64; 3],
    pub current: [f64; 3],
    pub ticked_at: Option<Instant>,
    pub positioned: bool,
    pub sneaking: bool,
    /// Tracked entities as of the last tick, for interpolated drawing.
    pub entities: Vec<(i32, Entity)>,
}

pub struct Network {
    pub meshes: Receiver<MeshUpdate>,
    pub errors: Receiver<String>,
    pub shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<TcpStream>>>,
    worker: Option<JoinHandle<()>>,
    mesher: Option<JoinHandle<()>>,
}

impl Network {
    pub fn start(config: Config, registry: Arc<Registry>) -> io::Result<Self> {
        let (send, meshes) = mpsc::channel();
        let (commands, inbox) = mpsc::channel();
        let (failed, errors) = mpsc::sync_channel(1);
        let world = Arc::new(RwLock::new(World::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let socket = Arc::new(Mutex::new(None));
        let cancel = stop.clone();
        let active = socket.clone();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let state = shared.clone();
        let mesher = {
            let (world, stop) = (world.clone(), stop.clone());
            thread::Builder::new()
                .name("Legacy mesher".into())
                .spawn(move || mesh_columns(&registry, &world, &inbox, &send, &stop))?
        };
        let worker = thread::Builder::new()
            .name("Protocol 47".into())
            .spawn(move || {
                if let Err(error) = run(config, &world, commands, &state, &cancel, &active)
                    && !cancel.load(Ordering::Acquire)
                {
                    let _ = failed.try_send(error.to_string());
                }
                eprintln!("NETWORK_STOPPED");
            })?;
        Ok(Self {
            meshes,
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

/// The mesher thread: nearest dirty column first, one column per world read
/// lock, so neither the simulation nor the window thread waits on meshing.
fn mesh_columns(
    registry: &Registry,
    world: &RwLock<World>,
    inbox: &Receiver<MeshCommand>,
    send: &Sender<MeshUpdate>,
    stop: &AtomicBool,
) {
    let mut dirty: HashSet<(i32, i32)> = HashSet::new();
    let mut origin = None;
    let mut focus = [0.0f64; 3];
    let (mut meshed, mut busy) = (0u64, Duration::ZERO);
    while !stop.load(Ordering::Acquire) {
        let idle = dirty.is_empty() || origin.is_none();
        let first = if idle {
            match inbox.recv_timeout(Duration::from_millis(100)) {
                Ok(command) => Some(command),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            None
        };
        for command in first.into_iter().chain(inbox.try_iter()) {
            match command {
                MeshCommand::Dirty(keys) => dirty.extend(keys),
                MeshCommand::Focus(position) => focus = position,
                MeshCommand::Origin(new) => {
                    origin = Some(new);
                    if let Ok(world) = world.read() {
                        dirty.extend(world.columns().map(|(key, _)| key));
                    }
                }
            }
        }
        let Some(origin) = origin else {
            continue;
        };
        let center = (
            (focus[0] / 16.0).floor() as i64,
            (focus[2] / 16.0).floor() as i64,
        );
        let Some(&key) = dirty.iter().min_by_key(|&&(x, z)| {
            let dx = i64::from(x) - center.0;
            let dz = i64::from(z) - center.1;
            dx * dx + dz * dz
        }) else {
            continue;
        };
        dirty.remove(&key);
        let started = Instant::now();
        let mesh = match world.read() {
            Ok(world) => world
                .column(key.0, key.1)
                .is_some()
                .then(|| registry.mesh_column(&world, key, origin)),
            Err(_) => return,
        };
        meshed += 1;
        busy += started.elapsed();
        if meshed.is_multiple_of(256) {
            eprintln!(
                "MESHER_STATS columns={meshed} mean_ms={:.2} pending={}",
                busy.as_secs_f64() * 1000.0 / meshed as f64,
                dirty.len()
            );
        }
        if send.send(MeshUpdate { key, origin, mesh }).is_err() {
            return;
        }
    }
}

fn run(
    config: Config,
    world: &RwLock<World>,
    mesher: Sender<MeshCommand>,
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
    active: &Mutex<Option<TcpStream>>,
) -> io::Result<()> {
    let mut socket = None;
    let mut last_error = None;
    for address in (config.host.as_str(), config.port).to_socket_addrs()? {
        if stop.load(Ordering::Acquire) {
            return Ok(());
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
    let profile = login::connect(
        &mut connection,
        &config.host,
        config.port,
        &config.session.profile.username,
        |hash| config.session.join(hash),
    )?;
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
    let result = run_actor(world, &mesher, shared, &config, stop, &packets, &mut writer);
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

fn run_actor(
    world: &RwLock<World>,
    mesher: &Sender<MeshCommand>,
    shared: &Mutex<Shared>,
    config: &Config,
    stop: &AtomicBool,
    packets: &Receiver<io::Result<Vec<u8>>>,
    writer: &mut PacketWriter<TcpStream>,
) -> io::Result<()> {
    let scripted = config.autopilot || config.autopilot_fly;
    let mut mesh_origin: Option<[i64; 3]> = None;
    let dirty = |keys: Vec<(i32, i32)>| {
        // A closed mesher only means the window is shutting down.
        let _ = mesher.send(MeshCommand::Dirty(keys));
    };
    let mut player = Player::default();
    let mut entities = Entities::default();
    let mut inventory: Vec<Option<horizon_protocol_47::nbt::Item>> = vec![None; 45];
    let mut capabilities = entity_packets::Abilities {
        invulnerable: false,
        flying: false,
        allow_flying: false,
        creative: false,
        fly_speed: 0.05,
        walk_speed: 0.1,
    };
    let mut entity_id = 0;
    let mut positioned = false;
    let mut sky_light = None;
    let mut reporter = MovementReporter::default();
    let (mut server_sprint, mut server_sneak) = (false, false);
    let tick = Duration::from_millis(50);
    let mut next_tick = Instant::now() + tick;
    let mut reports = 0u64;
    while !stop.load(Ordering::Acquire) {
        match packets.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
            Ok(packet) => {
                let body = packet?;
                match play::decode(&body)? {
                    play::Packet::Join(join) => {
                        entity_id = join.entity_id;
                        entities.clear();
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
                        let pose = PositionLook {
                            position: player.position,
                            yaw: player.yaw,
                            pitch: player.pitch,
                        };
                        writer.send(
                            &Movement::PositionLook {
                                location: pose,
                                on_ground: false,
                            }
                            .encode()?,
                        )?;
                        if pose.position.iter().any(|&value| {
                            value < f64::from(i32::MIN) || value > f64::from(i32::MAX)
                        }) {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Server position exceeds block coordinate range",
                            ));
                        }
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
                        write(world)?.apply(update);
                        dirty(around(x, z).to_vec());
                    }
                    play::Packet::Other { id: 0x26, .. } => {
                        let updates = decode_bulk(&body)?;
                        let mut keys = Vec::with_capacity(updates.len() * 5);
                        let mut world = write(world)?;
                        for update in updates {
                            let (ColumnUpdate::Unload { x, z } | ColumnUpdate::Data { x, z, .. }) =
                                update;
                            world.apply(update);
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
                        player.food = fields::decode_health(fields)?.food;
                    }
                    play::Packet::Other { id: 0x07, fields } => {
                        // Respawn: a new dimension or a server switch behind a
                        // proxy. Vanilla discards the world and its entities.
                        let dimension = fields
                            .get(..4)
                            .map(|bytes| {
                                i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                            })
                            .ok_or_else(|| io::Error::other("Short respawn packet"))?;
                        sky_light = Some(dimension == 0);
                        let mut world = write(world)?;
                        let keys: Vec<(i32, i32)> = world.columns().map(|(key, _)| key).collect();
                        *world = World::default();
                        world.set_sky(dimension == 0);
                        drop(world);
                        dirty(keys);
                        entities.clear();
                        positioned = false;
                        lock(shared)?.positioned = false;
                    }
                    play::Packet::Other { id: 0x39, fields } => {
                        let abilities = entity_packets::decode_abilities(fields)?;
                        player.flying = abilities.flying;
                        player.allow_flying = abilities.allow_flying;
                        player.fly_speed = abilities.fly_speed;
                        capabilities = abilities;
                    }
                    play::Packet::Other { id: 0x2f, fields } => {
                        let (window, slot, item) = entity_packets::decode_set_slot(fields)?;
                        if window == 0
                            && let Some(target) = usize::try_from(slot)
                                .ok()
                                .and_then(|slot| inventory.get_mut(slot))
                        {
                            *target = item;
                            player.depth_strider = depth_strider(&inventory);
                        }
                    }
                    play::Packet::Other { id: 0x30, fields } => {
                        let (window, items) = entity_packets::decode_window_items(fields)?;
                        if window == 0 {
                            for (target, item) in inventory.iter_mut().zip(items) {
                                *target = item;
                            }
                            player.depth_strider = depth_strider(&inventory);
                        }
                    }
                    play::Packet::Other { id: 0x33, fields } => {
                        let (at, lines) = entity_packets::decode_sign(fields)?;
                        let text = lines.map(|line| sign_text(&line));
                        if write(world)?.set_block_entity(at, Some(BlockEntity::Sign(text))) {
                            dirty(around(at[0].div_euclid(16), at[2].div_euclid(16)).to_vec());
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
                            entities.apply(packet);
                        }
                    }
                    play::Packet::Other { id: 0x12, fields } => {
                        let (entity, [x, y, z]) = fields::decode_velocity(fields)?;
                        if entity == entity_id {
                            player.set_velocity(x, y, z);
                        }
                    }
                    play::Packet::Other { id: 0x27, fields } => {
                        let [x, y, z] = fields::decode_explosion(fields)?;
                        player.add_velocity(x, y, z);
                    }
                    play::Packet::Other { id: 0x20, fields } => {
                        let (entity, properties) = fields::decode_properties(fields)?;
                        if entity == entity_id {
                            for property in properties {
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
                            apply_effect(&mut player, effect.effect, Some(effect.amplifier));
                        }
                    }
                    play::Packet::Other { id: 0x1e, fields } => {
                        let (entity, effect) = fields::decode_remove_effect(fields)?;
                        if entity == entity_id {
                            apply_effect(&mut player, effect, None);
                        }
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
        let now = Instant::now();
        if now < next_tick {
            continue;
        }
        let late = now.duration_since(next_tick).as_nanos();
        let due = late / tick.as_nanos() + 1;
        // Vanilla's timer executes at most ten accumulated client ticks.
        let world_view = read(world)?;
        for _ in 0..due.min(10) {
            if !positioned || !player.chunk_loaded(&world_view) {
                continue;
            }
            let keys = {
                let mut state = lock(shared)?;
                if scripted {
                    autopilot(reports, config.autopilot_fly, &mut state);
                }
                player.yaw = state.yaw;
                player.pitch = state.pitch;
                state.keys
            };
            player.tick(&world_view, keys);
            // sendPlayerAbilities happens inside onLivingUpdate, before the
            // walking update's actions and movement.
            if std::mem::take(&mut player.abilities_changed) {
                capabilities.flying = player.flying;
                writer.send(&entity_packets::encode_abilities(capabilities))?;
            }
            // EntityPlayerSP.onUpdateWalkingPlayer: actions, then movement.
            if player.sprinting != server_sprint {
                let action = if player.sprinting {
                    fields::START_SPRINTING
                } else {
                    fields::STOP_SPRINTING
                };
                writer.send(&fields::entity_action(entity_id, action))?;
                server_sprint = player.sprinting;
            }
            if player.sneaking != server_sneak {
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
            writer.send(&reporter.report(pose, player.on_ground)?)?;
            {
                let mut state = lock(shared)?;
                state.previous = state.current;
                state.current = player.position;
                state.ticked_at = Some(Instant::now());
                state.sneaking = player.sneaking;
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
                    entities.len()
                );
            }
        }
        drop(world_view);
        if positioned && due > 0 {
            for _ in 0..due.min(10) {
                entities.tick();
            }
            let mut state = lock(shared)?;
            state.entities.clear();
            state
                .entities
                .extend(entities.iter().map(|(id, entity)| (id, entity.clone())));
        }
        let remainder = Duration::from_nanos((late % tick.as_nanos()) as u64);
        next_tick = now + tick - remainder;
        if positioned {
            let _ = mesher.send(MeshCommand::Focus(player.position));
        }
    }
    Ok(())
}
