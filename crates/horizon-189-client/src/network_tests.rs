//! Real actor/transport regressions on a private loopback socket; no login,
//! credentials, assets, JVM or window are required for actor tests. The ignored
//! full retry test additionally loads an owned registry.
use super::*;
use horizon_protocol_47::{write_string, write_varint};
use horizon_world_189::{ColumnData, ColumnUpdate};
use std::net::TcpListener;

struct Actor {
    packets: Sender<io::Result<Vec<u8>>>,
    peer: Connection<TcpStream>,
    shared: Arc<Mutex<Shared>>,
    world: Arc<RwLock<World>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
    _meshes: Receiver<MeshCommand>,
}
impl Actor {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        server
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (_, mut writer) = Connection::new(client).split().unwrap();
        let (packets, incoming) = mpsc::channel();
        let (mesher, meshes) = mpsc::channel();
        let mut world = World::default();
        world.apply(ColumnUpdate::Data {
            x: 0,
            z: 0,
            full: true,
            data: Box::new(ColumnData {
                sections: Default::default(),
                biomes: None,
            }),
        });
        for x in 0..16 {
            for z in 0..16 {
                world.set_block(x, 3, z, BlockState(1 << 4));
            }
        }
        let world = Arc::new(RwLock::new(world));
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (w, s, cancel) = (world.clone(), shared.clone(), stop.clone());
        let worker = thread::spawn(move || {
            let config = Config {
                host: "127.0.0.1".into(),
                port: 0,
                session: Arc::new(Session::offline("NativeActor", None).unwrap()),
                offline: true,
                autopilot: false,
                autopilot_fly: false,
                view_distance: 2,
            };
            run_actor(
                &w,
                &mesher,
                &s,
                &config,
                &cancel,
                &incoming,
                &mut writer,
                &Default::default(),
            )
        });
        Self {
            packets,
            peer: Connection::new(server),
            shared,
            world,
            stop,
            worker: Some(worker),
            _meshes: meshes,
        }
    }
    fn send(&self, packet: Vec<u8>) {
        self.packets.send(Ok(packet)).unwrap();
    }
    fn next(&mut self, id: u8) -> Vec<u8> {
        for _ in 0..100 {
            let packet = self
                .peer
                .receive()
                .expect("actor failed to send expected packet");
            if packet.first() == Some(&id) {
                return packet;
            }
        }
        panic!("Expected packet absent");
    }
    fn barrier(&mut self, nonce: i32) {
        self.send(play::keep_alive(nonce));
        assert_eq!(self.next(0), play::keep_alive(nonce));
    }
    fn join(&mut self) {
        let mut join = vec![1];
        join.extend(7i32.to_be_bytes());
        join.extend([0, 0, 1, 20]);
        write_string(&mut join, "default", 16).unwrap();
        join.push(0);
        self.send(join);
        assert_eq!(self.peer.receive().unwrap(), play::client_settings(2));
        assert_eq!(self.peer.receive().unwrap(), play::brand().unwrap());
    }
    fn position(&mut self) {
        self.position_at([0.5, 4.0, 0.5]);
    }
    fn position_at(&mut self, position: [f64; 3]) {
        let mut p = vec![8];
        for v in position {
            p.extend(v.to_be_bytes());
        }
        p.extend(0f32.to_be_bytes());
        p.extend(0f32.to_be_bytes());
        p.push(0);
        self.send(p);
        let expected = Movement::PositionLook {
            location: PositionLook {
                position,
                yaw: 0.0,
                pitch: 0.0,
            },
            on_ground: false,
        }
        .encode()
        .unwrap();
        assert_eq!(
            self.next(6),
            expected,
            "S08 reply must be immediate C06 with ground=false"
        );
    }
    fn grounded(&mut self) {
        for _ in 0..50 {
            let p = self
                .peer
                .receive()
                .expect("Movement stopped before reaching ground");
            if matches!(p.first(), Some(3..=6)) && p.last() == Some(&1) {
                return;
            }
        }
        panic!("Ground report absent");
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn respawn(dimension: i32) -> Vec<u8> {
    let mut p = vec![7];
    p.extend(dimension.to_be_bytes());
    p.extend([1, 0]);
    write_string(&mut p, "default", 16).unwrap();
    p
}
#[test]
fn respawn_preserves_same_dimension_world_but_recreates_player_state() {
    let mut actor = Actor::new();
    actor.join();
    actor.position();
    // A nearby fireball remains part of this same WorldClient on respawn.
    let mut object = vec![0x0e, 99, 63];
    for v in [64i32, 160, 64] {
        object.extend(v.to_be_bytes());
    }
    object.extend([0, 0]);
    object.extend(0i32.to_be_bytes());
    actor.send(object);
    let mut effect = vec![0x1d, 7, 8, 2];
    write_varint(&mut effect, 600);
    effect.push(0);
    actor.send(effect);
    actor.grounded();
    actor.barrier(300);
    assert!(
        actor
            .shared
            .lock()
            .unwrap()
            .entities
            .iter()
            .any(|(id, _)| *id == 99)
    );
    actor.send(respawn(0));
    actor.barrier(301);
    assert_eq!(
        actor.world.read().unwrap().column_count(),
        1,
        "same-dimension respawn must retain chunks"
    );
    actor.position();
    actor.grounded();
    actor.barrier(302);
    assert!(
        actor
            .shared
            .lock()
            .unwrap()
            .entities
            .iter()
            .any(|(id, _)| *id == 99),
        "same-dimension entities disappeared"
    );
    actor.shared.lock().unwrap().keys.jump = true;
    let moved = actor.next(4);
    let y = f64::from_be_bytes(moved[9..17].try_into().unwrap());
    assert!(
        (y - 4.419_999_986_886_978).abs() < 1e-6,
        "old jump boost survived new player: {y}"
    );
    actor.shared.lock().unwrap().keys.jump = false;
    actor.send(respawn(-1));
    actor.barrier(303);
    assert_eq!(actor.world.read().unwrap().column_count(), 0);
    let state = actor.shared.lock().unwrap();
    assert!(!state.positioned);
    assert!(
        state.entities.is_empty(),
        "old-dimension entities leaked through snapshot"
    );
    assert_eq!(state.vitals.effects, 0);
}

#[test]
fn respawn_relative_rotation_uses_new_player_and_invalid_positions_are_not_echoed() {
    let mut actor = Actor::new();
    actor.join();
    actor.position();
    actor.send(respawn(0));
    actor.barrier(310);
    let mut correction = vec![8];
    for v in [0.5f64, 4.0, 0.5] {
        correction.extend(v.to_be_bytes());
    }
    correction.extend(10f32.to_be_bytes());
    correction.extend(5f32.to_be_bytes());
    correction.push(24);
    actor.send(correction);
    let reply = actor.next(6);
    assert_eq!(
        f32::from_be_bytes(reply[25..29].try_into().unwrap()),
        -170.0
    );
    assert_eq!(f32::from_be_bytes(reply[29..33].try_into().unwrap()), 5.0);
    let mut invalid = vec![8];
    for v in [f64::from(i32::MAX) + 1024.0, 4.0, 0.5] {
        invalid.extend(v.to_be_bytes());
    }
    invalid.extend([0; 9]);
    actor.send(invalid);
    while let Ok(packet) = actor.peer.receive() {
        if matches!(packet.first(), Some(4 | 6)) {
            let x = f64::from_be_bytes(packet[1..9].try_into().unwrap());
            assert!(
                x <= f64::from(i32::MAX),
                "invalid server position was sent back"
            );
        }
    }
    let error = actor.worker.take().unwrap().join().unwrap().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn full_player_inventory_packets_update_the_visible_shop_rows() {
    use horizon_protocol_47::nbt::{Item, write_slot};
    let item = |id, count| {
        Some(Item {
            id,
            count,
            damage: 0,
            nbt: None,
        })
    };
    let items = |window: u8, slots: Vec<Option<Item>>| {
        let mut body = vec![0x30, window];
        body.extend((slots.len() as i16).to_be_bytes());
        for slot in slots {
            write_slot(&mut body, slot.as_ref()).unwrap();
        }
        body
    };
    let mut actor = Actor::new();
    actor.join();
    actor.position();
    actor.grounded();
    let mut open = vec![0x2d, 1];
    write_string(&mut open, "minecraft:chest", 32).unwrap();
    write_string(&mut open, r#"{"text":"Shop"}"#, 32767).unwrap();
    open.push(27);
    actor.send(open);
    let mut chest = vec![None; 63];
    chest[0] = item(264, 3);
    chest[54] = item(276, 1);
    actor.send(items(1, chest));
    let mut player = vec![None; 45];
    player[9] = item(264, 7);
    player[36] = item(260, 5);
    actor.send(items(0, player));
    actor.send(items(2, vec![item(1, 64); 63])); // A different window is not this active container.
    actor.barrier(340);
    actor.grounded();
    {
        let state = actor.shared.lock().unwrap();
        let view = &state.inventory;
        assert_eq!(view.window, 1);
        assert_eq!(view.slots[0], item(264, 3));
        assert_eq!(view.slots[27], item(264, 7));
        assert_eq!(view.slots[54], item(260, 5));
    }
    actor.send(vec![0x2e, 1]);
    actor.barrier(341);
    actor.grounded();
    let state = actor.shared.lock().unwrap();
    assert_eq!(state.inventory.window, 0);
    assert_eq!(state.inventory.slots[36], item(260, 5));
}

#[test]
fn game_mode_packet_disables_old_flight_without_waiting_for_an_abilities_packet() {
    for mode in [0.0f32, 2.0] {
        let mut actor = Actor::new();
        actor.join();
        actor.position();
        actor.grounded();
        let mut abilities = entity_packets::encode_abilities(entity_packets::Abilities {
            invulnerable: true,
            flying: true,
            allow_flying: true,
            creative: true,
            fly_speed: 0.09,
            walk_speed: 0.15,
        });
        abilities[0] = 0x39;
        actor.send(abilities);
        actor.position_at([0.5, 8.0, 0.5]);
        actor.barrier(350);
        let mut change = vec![0x2b, 3];
        change.extend(mode.to_be_bytes());
        actor.send(change);
        actor.barrier(351);
        let mut lowest = 8.0f64;
        for _ in 0..10 {
            let packet = actor.peer.receive().unwrap();
            assert_ne!(
                packet[0], 0x13,
                "server game-mode change must not synthesize a client ability toggle"
            );
            if matches!(packet[0], 4 | 6) {
                lowest = lowest.min(f64::from_be_bytes(packet[9..17].try_into().unwrap()));
            }
        }
        assert!(
            lowest < 7.95,
            "mode {mode} left the player hovering at {lowest}"
        );
        assert_eq!(actor.shared.lock().unwrap().game_mode, mode as u8);
    }
}

#[test]
fn game_mode_capabilities_preserve_speeds_and_creative_flight_choice() {
    for (mode, was_flying, flying, allow, creative) in [
        (0, true, false, false, false),
        (2, true, false, false, false),
        (1, true, true, true, true),
        (1, false, false, true, true),
        (3, false, true, true, false),
        (99, true, false, false, false),
    ] {
        let mut p = Player::default();
        p.flying = was_flying;
        p.fly_speed = 0.09;
        let mut caps = entity_packets::Abilities {
            flying: was_flying,
            invulnerable: false,
            allow_flying: false,
            creative: false,
            fly_speed: 0.09,
            walk_speed: 0.15,
        };
        let normalized = apply_game_mode(&mut p, &mut caps, mode);
        assert_eq!((p.flying, p.allow_flying), (flying, allow));
        assert_eq!(
            (caps.flying, caps.allow_flying, caps.creative),
            (flying, allow, creative)
        );
        assert_eq!(caps.invulnerable, allow);
        assert_eq!(p.no_clip, mode == 3);
        assert_eq!(
            (p.fly_speed, caps.fly_speed, caps.walk_speed),
            (0.09, 0.09, 0.15)
        );
        assert!(!p.abilities_changed);
        assert_eq!(normalized, if mode == 99 { 0 } else { mode as u8 });
    }
}

#[test]
#[ignore = "Requires HORIZON_189_TEST_JAR pointing at an owned 1.8.9 jar"]
fn retry_after_login_rejection_uses_fresh_state_and_same_session() {
    let jar = std::path::PathBuf::from(std::env::var_os("HORIZON_189_TEST_JAR").unwrap());
    let registry = Arc::new(Registry::open(&jar).unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = Config {
        host: "127.0.0.1".into(),
        port,
        session: Arc::new(Session::offline("NativeRetry", None).unwrap()),
        offline: true,
        autopilot: false,
        autopilot_fly: false,
        view_distance: 2,
    };
    let copy = config.clone();
    assert!(Arc::ptr_eq(&config.session, &copy.session));
    let server = thread::spawn(move || {
        for attempt in 0..2 {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut peer = Connection::new(socket);
            assert_eq!(peer.receive().unwrap()[0], 0); // handshake
            assert_eq!(
                peer.receive().unwrap(),
                login::start("NativeRetry").unwrap()
            );
            if attempt == 0 {
                let mut rejected = vec![0];
                write_string(
                    &mut rejected,
                    r#"{"text":"Retry fixture","color":"red"}"#,
                    32767,
                )
                .unwrap();
                peer.send(&rejected).unwrap();
                continue;
            }
            let mut success = vec![2];
            write_string(&mut success, "00000000000000000000000000000001", 36).unwrap();
            write_string(&mut success, "NativeRetry", 16).unwrap();
            peer.send(&success).unwrap();
            let mut join = vec![1];
            join.extend(9i32.to_be_bytes());
            join.extend([0, 0, 1, 20]);
            write_string(&mut join, "default", 16).unwrap();
            join.push(0);
            peer.send(&join).unwrap();
            assert_eq!(peer.receive().unwrap(), play::client_settings(2));
            assert_eq!(peer.receive().unwrap(), play::brand().unwrap());
            let mut position = vec![8];
            for v in [4.5f64, 70.0, 8.5] {
                position.extend(v.to_be_bytes());
            }
            position.extend(0f32.to_be_bytes());
            position.extend(0f32.to_be_bytes());
            position.push(0);
            peer.send(&position).unwrap();
            let expected = Movement::PositionLook {
                location: PositionLook {
                    position: [4.5, 70.0, 8.5],
                    yaw: 0.0,
                    pitch: 0.0,
                },
                on_ground: false,
            }
            .encode()
            .unwrap();
            loop {
                let packet = peer.receive().unwrap();
                if packet.first() == Some(&6) {
                    assert_eq!(packet, expected);
                    break;
                }
            }
            // Drop must shut the reader and actor down, including their socket.
            while peer.receive().is_ok() {}
        }
    });
    let first = Network::start(config, registry.clone()).unwrap();
    assert!(
        first
            .errors
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .contains("Retry fixture")
    );
    let old_shared = first.shared.clone();
    {
        let mut state = old_shared.lock().unwrap();
        state.input.attack = true;
        state.keys.forward = true;
        state.chat_inbox.push(ChatEvent::ActionBar("stale".into()));
    }
    drop(first);
    let second = Network::start(copy, registry).unwrap();
    assert!(!Arc::ptr_eq(&old_shared, &second.shared));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(error) = second.errors.try_recv() {
            panic!("{error}");
        }
        let state = second.shared.lock().unwrap();
        assert!(!state.input.attack && !state.keys.forward && state.chat_inbox.is_empty());
        if state.positioned {
            assert_eq!(state.username, "NativeRetry");
            assert_eq!(state.entity_id, 9);
            assert_eq!(state.current, [4.5, 70.0, 8.5]);
            break;
        }
        drop(state);
        assert!(Instant::now() < deadline, "retry never positioned");
        thread::sleep(Duration::from_millis(2));
    }
    drop(second);
    server.join().unwrap();
}

#[test]
fn forced_hotbar_changes_sync_once_and_survive_respawn() {
    let mut actor = Actor::new();
    actor.join();
    actor.position();
    actor.grounded();
    // Drain preceding movement through an ordered keepalive barrier.
    actor.barrier(380);
    actor.send(vec![0x09, 4]);
    assert_eq!(actor.next(0x09), vec![0x09, 0, 4]);
    actor.grounded();
    assert_eq!(actor.shared.lock().unwrap().inventory.selected, 4);
    // The same slot never produces a redundant C09.
    actor.send(vec![0x09, 4]);
    actor.barrier(381);
    for _ in 0..3 {
        let packet = actor.peer.receive().unwrap();
        assert_ne!(packet.first(), Some(&0x09));
    }
    // A new player starts at slot zero, but the controller's last sent slot persists.
    actor.send(respawn(0));
    actor.barrier(382);
    actor.position();
    assert_eq!(actor.peer.receive().unwrap(), vec![0x09, 0, 0]);
    actor.grounded();
    // Vanilla ignores values outside InventoryPlayer.getHotbarSize().
    for slot in [9, 127, 255] {
        actor.send(vec![0x09, slot]);
    }
    actor.barrier(383);
    actor.grounded();
    assert_eq!(actor.shared.lock().unwrap().inventory.selected, 0);
}

#[test]
fn mid_session_join_keeps_the_player_but_starts_a_new_controller_and_world() {
    let mut actor = Actor::new();
    actor.join();
    actor.position();
    actor.grounded();
    actor.barrier(390);
    actor.send(vec![0x09, 4]);
    assert_eq!(actor.next(0x09), vec![0x09, 0, 4]);
    assert!(actor.world.read().unwrap().column_count() > 0);
    // handleJoinGame builds a new PlayerControllerMP (currentPlayerItem 0)
    // and a new WorldClient, but Minecraft.loadWorld keeps the existing
    // EntityPlayerSP and its slot 4, synced once the player ticks again.
    let mut join = vec![1];
    join.extend(7i32.to_be_bytes());
    join.extend([0, 0, 1, 20]);
    write_string(&mut join, "default", 16).unwrap();
    join.push(0);
    actor.send(join);
    actor.barrier(391);
    assert_eq!(actor.world.read().unwrap().column_count(), 0);
    assert_eq!(actor.shared.lock().unwrap().inventory.selected, 4);
}

#[test]
fn dying_with_a_container_open_closes_it_like_gui_container_update_screen() {
    let health = |value: f32| {
        let mut p = vec![0x06];
        p.extend(value.to_be_bytes());
        p.push(20);
        p.extend(5f32.to_be_bytes());
        p
    };
    let mut actor = Actor::new();
    actor.join();
    actor.position();
    actor.grounded();
    let mut open = vec![0x2d, 3];
    write_string(&mut open, "minecraft:chest", 32).unwrap();
    write_string(&mut open, r#"{"text":"Shop"}"#, 32767).unwrap();
    open.push(27);
    actor.send(open);
    actor.barrier(400);
    actor.grounded();
    assert!(actor.shared.lock().unwrap().inventory.open);
    // GuiContainer.updateScreen: a dead player closes the screen (C0D) on
    // the next tick, before the death screen can replace it.
    actor.send(health(0.0));
    assert_eq!(actor.next(0x0d), vec![0x0d, 3]);
    actor.barrier(401);
    {
        let state = actor.shared.lock().unwrap();
        assert!(!state.inventory.open);
        assert_eq!(state.inventory.window, 0);
    }
    for _ in 0..3 {
        let packet = actor.peer.receive().unwrap();
        assert_ne!(packet.first(), Some(&0x0d));
    }
    // The player's own inventory (window 0) closes the same way.
    actor.send(respawn(0));
    actor.barrier(402);
    actor.position();
    actor.grounded();
    actor
        .shared
        .lock()
        .unwrap()
        .input
        .actions
        .push_back(crate::gameplay::Action::OpenInventory);
    assert_eq!(actor.next(0x16), vec![0x16, 2]);
    actor.send(health(0.0));
    assert_eq!(actor.next(0x0d), vec![0x0d, 0]);
}

/// Distance walked forward through still water in 20 ticks, after optionally
/// moving Depth Strider III boots from the main inventory onto the feet.
fn water_walk(equip: bool) -> f64 {
    use horizon_protocol_47::nbt::{Item, Tag, write_slot};
    let mut actor = Actor::new();
    actor.join();
    actor.position();
    actor.grounded();
    let boots = Item {
        id: 301,
        count: 1,
        damage: 0,
        nbt: Some(Tag::Compound(vec![(
            "ench".into(),
            Tag::List(vec![Tag::Compound(vec![
                ("id".into(), Tag::Short(8)),
                ("lvl".into(), Tag::Short(3)),
            ])]),
        )])),
    };
    let mut items = vec![0x30, 0];
    items.extend(45i16.to_be_bytes());
    for slot in 0..45 {
        write_slot(&mut items, (slot == 9).then_some(&boots)).unwrap();
    }
    actor.send(items);
    actor.barrier(410);
    if equip {
        use crate::gameplay::Action;
        // Window clicks are predicted; the server sends no S2F for them.
        let mut state = actor.shared.lock().unwrap();
        state.input.actions.extend([
            Action::OpenInventory,
            Action::Click {
                slot: 9,
                mode: 0,
                button: 0,
            },
            Action::Click {
                slot: 8,
                mode: 0,
                button: 0,
            },
            Action::CloseInventory,
        ]);
        drop(state);
        assert_eq!(actor.next(0x0d), vec![0x0d, 0]);
        assert!(actor.shared.lock().unwrap().inventory.slots[8].is_some());
    }
    {
        let mut world = actor.world.write().unwrap();
        for x in 0..16 {
            for z in 0..16 {
                world.set_block(x, 4, z, BlockState(9 << 4));
            }
        }
    }
    actor.barrier(411);
    actor.shared.lock().unwrap().keys.forward = true;
    let z = |packet: &[u8]| f64::from_be_bytes(packet[17..25].try_into().unwrap());
    let mut first = None;
    let mut last = 0.0;
    let mut ticks = 0;
    while ticks < 20 {
        let packet = actor.peer.receive().unwrap();
        if matches!(packet.first(), Some(3..=6)) {
            ticks += 1;
        }
        if matches!(packet.first(), Some(4 | 6)) {
            first.get_or_insert(z(&packet));
            last = z(&packet);
        }
    }
    last - first.unwrap()
}

#[test]
fn depth_strider_follows_predicted_armor_clicks() {
    let (plain, strider) = (water_walk(false), water_walk(true));
    assert!(plain > 0.5, "{plain}");
    assert!(strider > plain * 1.5, "plain {plain} strider {strider}");
}

/// EntityLivingBase.onDeathUpdate: a dead player keeps moving (with no
/// input) and reporting for twenty ticks, then `setDead` removes it from the
/// world's update loop and its movement packets stop until respawn.
/// EntityPlayer.onDeath (S19 status 3) shrinks it to 0.2 and gives it a hop
/// and a drift away from its yaw.
#[test]
fn dead_player_moves_for_twenty_ticks_then_stops_reporting() {
    let mut actor = Actor::new();
    actor.join();
    actor.position();
    actor.grounded();
    actor.barrier(420);
    let mut status = vec![0x1a];
    status.extend(7i32.to_be_bytes());
    status.push(3);
    let mut health = vec![0x06];
    health.extend(0f32.to_be_bytes());
    health.push(20);
    health.extend(5f32.to_be_bytes());
    actor.send(status);
    actor.send(health);
    actor.barrier(421);
    let mut moves = 0;
    let (mut highest, mut west) = (0f64, f64::MAX);
    // Thirty ticks later, a keepalive closes the count.
    thread::sleep(Duration::from_millis(1500));
    actor.send(play::keep_alive(422));
    loop {
        let packet = actor.peer.receive().unwrap();
        if packet == play::keep_alive(422) {
            break;
        }
        if matches!(packet.first(), Some(3..=6)) {
            moves += 1;
        }
        if matches!(packet.first(), Some(4 | 6)) {
            west = west.min(f64::from_be_bytes(packet[1..9].try_into().unwrap()));
            highest = highest.max(f64::from_be_bytes(packet[9..17].try_into().unwrap()));
        }
    }
    assert_eq!(moves, 20);
    // motionY 0.1 and motionX -cos(0) * 0.1 at yaw 0.
    assert!(highest > 4.05, "the body should hop: {highest}");
    assert!(west < 0.45, "the body should drift: {west}");
    // Respawn brings a new, reporting player.
    actor.send(respawn(0));
    actor.barrier(423);
    actor.position();
    actor.grounded();
}
