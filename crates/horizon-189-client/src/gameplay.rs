//! User input drives one ordered legacy interaction controller.
use crate::{hardness::HARDNESS, inventory::Inventory};
use horizon_physics_189::{
    player::{EYE_HEIGHT, Player},
    raycast::{self, Hit},
};
use horizon_protocol_47::{PacketWriter, interaction as packet, nbt::Item};
use horizon_world_189::{World, entities::Entities};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
};

#[derive(Clone, Copy, Debug)]
pub struct Vitals {
    pub health: f32,
    pub max_health: f32,
    pub food: i32,
    pub saturation: f32,
    pub armor: u8,
    pub absorption: f32,
    /// Active potion effects as a bit per effect ID (`hud::POISON` etc.).
    pub effects: u32,
    /// `EntityPlayer.getScore`, metadata index 18.
    pub score: i32,
}
impl Default for Vitals {
    fn default() -> Self {
        Self {
            health: 20.0,
            max_health: 20.0,
            food: 20,
            saturation: 5.0,
            armor: 0,
            absorption: 0.0,
            effects: 0,
            score: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Respawn,
    Attack,
    Use,
    Select(u8),
    OpenInventory,
    CloseInventory,
    Click { slot: i16, right: bool },
}

#[derive(Default)]
pub struct Input {
    pub attack: bool,
    pub using: bool,
    pub actions: VecDeque<Action>,
}

#[derive(Default)]
pub struct Controller {
    digging: Option<([i32; 3], u8, f32, Option<Item>)>,
    delay: u8,
    right_delay: u8,
    miss_delay: u8,
    pub swing: Option<u8>,
    pub using: bool,
    pub haste: Option<i8>,
    pub fatigue: Option<i8>,
    raycaster: raycast::Raycaster,
}

impl Controller {
    fn swing(&mut self, writer: &mut PacketWriter<impl Read + Write>) -> io::Result<()> {
        if self.swing.is_none_or(|v| v >= 3) {
            self.swing = Some(0);
            writer.send(&packet::swing())?;
        }
        Ok(())
    }
    fn abort(&mut self, writer: &mut PacketWriter<impl Read + Write>) -> io::Result<()> {
        if let Some((at, face, _, _)) = self.digging.take() {
            writer.send(&packet::dig(1, at, face)?)?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        input: &mut Input,
        player: &mut Player,
        world: &World,
        entities: &Entities,
        inventory: &mut Inventory,
        game_mode: u8,
        writer: &mut PacketWriter<impl Read + Write>,
    ) -> io::Result<()> {
        if let Some(swing) = &mut self.swing {
            *swing += 1;
            if *swing >= 6 {
                self.swing = None;
            }
        }
        self.right_delay = self.right_delay.saturating_sub(1);
        self.miss_delay = self.miss_delay.saturating_sub(1);
        if !input.attack {
            self.miss_delay = 0;
        }
        let eye = [
            player.position[0],
            player.position[1]
                + f64::from(if player.sneaking {
                    EYE_HEIGHT - 0.08
                } else {
                    EYE_HEIGHT
                }),
            player.position[2],
        ];
        let hit = self.raycaster.select(
            world,
            entities,
            eye,
            player.yaw,
            player.pitch,
            game_mode == 1,
        );
        let mut use_pressed = false;
        while let Some(action) = input.actions.pop_front() {
            match action {
                Action::Respawn => writer.send(&[0x16, 0])?,
                Action::Select(slot) if slot <= 8 && slot != inventory.view.selected => {
                    self.abort(writer)?;
                    if self.using {
                        writer.send(&packet::dig(5, [0; 3], 0)?)?;
                        self.using = false;
                    }
                    inventory.view.selected = slot;
                    writer.send(&packet::held(slot)?)?;
                }
                Action::OpenInventory => {
                    self.abort(writer)?;
                    inventory.local_open();
                    writer.send(&[0x16, 2])?;
                }
                Action::CloseInventory => writer.send(&inventory.close())?,
                Action::Click { slot, right } => {
                    if let Some(packet) = inventory.click(slot, right)? {
                        writer.send(&packet)?;
                    }
                }
                Action::Attack if !inventory.view.open && self.miss_delay == 0 && !self.using => {
                    self.swing(writer)?;
                    match hit {
                        Some(Hit::Entity { id, .. }) => {
                            if game_mode != 3 {
                                writer.send(&packet::use_entity(id, 1, None)?)?;
                                // EntityPlayer.attackTargetEntityWithCurrentItem's
                                // sprint hit reduces horizontal motion locally.
                                if player.sprinting {
                                    player.motion[0] *= 0.6;
                                    player.motion[2] *= 0.6;
                                    player.sprinting = false;
                                }
                            }
                        }
                        Some(Hit::Block { at, face, .. }) if game_mode == 0 || game_mode == 1 => {
                            self.abort(writer)?;
                            if game_mode == 1 && inventory.held().is_some_and(|i| is_sword(i.id)) {
                                continue;
                            }
                            writer.send(&packet::dig(0, at, face)?)?;
                            if game_mode == 1 {
                                self.delay = 5;
                            } else {
                                self.digging = Some((at, face, 0.0, inventory.held().cloned()));
                            }
                        }
                        _ => {
                            if game_mode != 1 {
                                self.miss_delay = 10;
                            }
                        }
                    }
                }
                Action::Use => use_pressed = true,
                _ => {}
            }
        }
        if inventory.view.open {
            self.abort(writer)?;
            if self.using {
                writer.send(&packet::dig(5, [0; 3], 0)?)?;
            }
            self.using = false;
            player.using_item = false;
            return Ok(());
        }
        if self.using && !input.using {
            writer.send(&packet::dig(5, [0; 3], 0)?)?;
            self.using = false;
        }
        if (use_pressed || input.using && self.right_delay == 0)
            && self.digging.is_none()
            && !self.using
        {
            self.right_delay = 4;
            match hit {
                Some(Hit::Entity { id, point }) => {
                    let relative = entities
                        .get(id)
                        .map(|e| std::array::from_fn(|a| (point[a] - e.position[a]) as f32))
                        .unwrap_or([0.0; 3]);
                    writer.send(&packet::use_entity(id, 2, Some(relative))?)?;
                    writer.send(&packet::use_entity(id, 0, None)?)?;
                    let consumes_interaction = entities.get(id).is_some_and(|e| {
                        matches!(
                            e.kind,
                            horizon_world_189::entities::Kind::Mob(100 | 120)
                                | horizon_world_189::entities::Kind::Object(78)
                        )
                    });
                    if !consumes_interaction
                        && game_mode != 3
                        && let Some(item) = inventory.held()
                        && (is_sword(item.id) || item.id == 261)
                    {
                        writer.send(&packet::place([-1, -1, -1], 255, [0.0; 3], Some(item))?)?;
                        self.using = true;
                    }
                }
                Some(Hit::Block { at, face, point }) => {
                    let relative = std::array::from_fn(|a| {
                        (point[a] - f64::from(at[a])).clamp(0.0, 1.0) as f32
                    });
                    writer.send(&packet::place(at, face, relative, inventory.held())?)?;
                    if inventory.held().is_some() {
                        self.swing(writer)?;
                    }
                    let block = world
                        .block(at[0], at[1], at[2])
                        .unwrap_or_default()
                        .block_id();
                    if let Some(item) = inventory.held()
                        && (is_sword(item.id) || item.id == 261)
                        && (player.sneaking
                            || !matches!(
                                block,
                                23 | 25
                                    | 26
                                    | 54
                                    | 58
                                    | 61
                                    | 62
                                    | 64
                                    | 69
                                    | 71
                                    | 77
                                    | 84
                                    | 92
                                    | 93
                                    | 94
                                    | 96
                                    | 107
                                    | 116
                                    | 117
                                    | 120
                                    | 130
                                    | 137
                                    | 143
                                    | 145
                                    | 146
                                    | 149
                                    | 150
                                    | 154
                                    | 158
                                    | 167
                                    | 183..=197
                            ))
                    {
                        writer.send(&packet::place([-1, -1, -1], 255, [0.0; 3], Some(item))?)?;
                        self.using = true;
                    }
                }
                None => {
                    if let Some(item) = inventory.held() {
                        writer.send(&packet::place([-1, -1, -1], 255, [0.0; 3], Some(item))?)?;
                        self.using = is_sword(item.id)
                            || item.id == 261
                            || matches!(item.id,260|282|297|319|320|322|349|350|357|360|363..=367|391..=394|400|412|413|423|424);
                    }
                }
            }
        }
        player.using_item = self.using;
        if !input.attack || self.using {
            self.abort(writer)?;
            return Ok(());
        }
        if self.delay > 0 {
            self.delay -= 1;
            return Ok(());
        }
        let Some(Hit::Block { at, face, .. }) = hit else {
            self.abort(writer)?;
            return Ok(());
        };
        if game_mode != 0 && game_mode != 1 {
            return Ok(());
        }
        let same = self.digging.as_ref().is_some_and(|(pos, _, _, item)| {
            *pos == at && same_tool(item.as_ref(), inventory.held())
        });
        if !same {
            self.abort(writer)?;
            if game_mode == 1 && inventory.held().is_some_and(|i| is_sword(i.id)) {
                return Ok(());
            }
            writer.send(&packet::dig(0, at, face)?)?;
            if game_mode == 1 {
                self.delay = 5;
                return Ok(());
            }
            self.digging = Some((at, face, 0.0, inventory.held().cloned()));
            return Ok(());
        }
        let state = world.block(at[0], at[1], at[2]).unwrap_or_default();
        let cell = eye.map(|v| v.floor() as i32);
        let submerged = world.block(cell[0], cell[1], cell[2]).is_some_and(|s| {
            let level = if s.metadata() >= 8 { 0 } else { s.metadata() };
            let height = (cell[1] + 1) as f32 - (f32::from(level + 1) / 9.0 - 0.11111111);
            matches!(s.block_id(), 8 | 9) && eye[1] < f64::from(height)
        });
        let progress = self.break_speed(
            state.block_id(),
            inventory.held(),
            player,
            inventory,
            submerged,
        );
        let Some((_, _, damage, _)) = &mut self.digging else {
            return Ok(());
        };
        *damage += progress;
        if *damage >= 1.0 {
            writer.send(&packet::dig(2, at, face)?)?;
            self.digging = None;
            self.delay = 5;
        }
        self.swing(writer)?;
        Ok(())
    }

    fn break_speed(
        &self,
        block: u16,
        item: Option<&Item>,
        player: &Player,
        inventory: &Inventory,
        submerged: bool,
    ) -> f32 {
        let hardness = HARDNESS.get(usize::from(block)).copied().unwrap_or(-1.0);
        if hardness < 0.0 {
            return 0.0;
        }
        if hardness == 0.0 {
            return 1.0;
        }
        let id = item.map_or(0, |i| i.id);
        let (mut speed, harvest) = crate::hardness::tool(block, id);
        if speed > 1.0
            && let Some(item) = item
        {
            let efficiency = item.enchantment(32);
            if efficiency > 0 {
                speed += (efficiency * efficiency + 1) as f32;
            }
        }
        if let Some(haste) = self.haste {
            speed *= 1.0 + (f32::from(haste) + 1.0) * 0.2;
        }
        if let Some(fatigue) = self.fatigue {
            speed *= match fatigue {
                0 => 0.3,
                1 => 0.09,
                2 => 0.0027,
                _ => 0.00081,
            };
        }
        if submerged
            && inventory
                .player
                .get(5)
                .and_then(Option::as_ref)
                .is_none_or(|i| i.enchantment(6) == 0)
        {
            speed /= 5.0;
        }
        if !player.on_ground {
            speed /= 5.0;
        }
        speed / hardness / if harvest { 30.0 } else { 100.0 }
    }
}
fn same_tool(a: Option<&Item>, b: Option<&Item>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.id == b.id && a.nbt == b.nbt && (stack_durable(a.id) || a.damage == b.damage)
        }
        _ => false,
    }
}
fn stack_durable(id: i16) -> bool {
    matches!(id,256..=259|261|267..=279|283..=286|290..=294|298..=317|346|359|398)
}
fn is_sword(id: i16) -> bool {
    matches!(id, 267 | 268 | 272 | 276 | 283)
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol_47::{Connection, entities::EntityPacket};
    use horizon_world_189::{BlockState, ColumnData, ColumnUpdate};
    use std::{
        net::{TcpListener, TcpStream},
        time::Duration,
    };

    fn world() -> World {
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
        world
    }
    #[test]
    fn user_attack_swings_before_attack_and_block_use_releases() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let (_, mut writer) = Connection::new(client).split().unwrap();
        let mut peer = Connection::new(peer);
        let mut entities = Entities::default();
        entities.apply(EntityPacket::SpawnPlayer {
            id: 7,
            uuid: 0,
            position: [16, 4 * 32, 2 * 32],
            yaw: 0,
            pitch: 0,
            held_item: 0,
            metadata: vec![],
        });
        let mut player = Player::default();
        player.set_position(0.5, 4.0, 0.5);
        player.on_ground = true;
        let mut inventory = Inventory::default();
        inventory.player[36] = Some(Item {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        });
        let mut controller = Controller::default();
        let mut input = Input {
            attack: true,
            using: false,
            actions: VecDeque::from([Action::Attack]),
        };
        controller
            .tick(
                &mut input,
                &mut player,
                &world(),
                &entities,
                &mut inventory,
                0,
                &mut writer,
            )
            .unwrap();
        assert_eq!(peer.receive().unwrap(), [0x0a]);
        assert_eq!(peer.receive().unwrap(), [0x02, 7, 1]);
        input.attack = false;
        input.using = true;
        input.actions.push_back(Action::Use);
        controller
            .tick(
                &mut input,
                &mut player,
                &world(),
                &Entities::default(),
                &mut inventory,
                0,
                &mut writer,
            )
            .unwrap();
        assert_eq!(peer.receive().unwrap()[0], 8);
        assert!(player.using_item);
        input.using = false;
        controller
            .tick(
                &mut input,
                &mut player,
                &world(),
                &Entities::default(),
                &mut inventory,
                0,
                &mut writer,
            )
            .unwrap();
        assert_eq!(peer.receive().unwrap(), packet::dig(5, [0; 3], 0).unwrap());
        assert!(!player.using_item);
    }

    #[test]
    fn mining_uses_reference_strength_harvest_and_ground_penalties() {
        let inv = Inventory::default();
        let mut p = Player::default();
        p.on_ground = true;
        let c = Controller::default();
        assert_eq!(c.break_speed(1, None, &p, &inv, false), 1.0 / 1.5 / 100.0);
        let pick = Item {
            id: 278,
            count: 1,
            damage: 0,
            nbt: None,
        };
        assert_eq!(
            c.break_speed(1, Some(&pick), &p, &inv, false),
            8.0 / 1.5 / 30.0
        );
        p.on_ground = false;
        assert_eq!(
            c.break_speed(1, Some(&pick), &p, &inv, false),
            (8.0 / 5.0) / 1.5 / 30.0
        );
        assert_eq!(c.break_speed(7, Some(&pick), &p, &inv, false), 0.0);
        let mut w = world();
        w.set_block(0, 1, 1, BlockState(16));
    }

    #[test]
    #[ignore = "requires the managed local vanilla reference server and owned assets"]
    fn local_reference_interaction_gate() {
        assert_eq!(
            std::env::var("HORIZON_189_REFERENCE_GATE").as_deref(),
            Ok("1")
        );
        let jar =
            std::path::PathBuf::from(std::env::var_os("HORIZON_189_TEST_JAR").expect("owned jar"));
        let registry = std::sync::Arc::new(crate::terrain::Registry::open(&jar).unwrap());
        let network = crate::network::Network::start(
            crate::network::Config {
                host: "127.0.0.1".into(),
                port: 25565,
                session: horizon_auth::Session::offline("NativeCombat", None).unwrap(),
                offline: true,
                autopilot: false,
                autopilot_fly: false,
                view_distance: 2,
            },
            registry,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(error) = network.errors.try_recv() {
                panic!("{error}");
            }
            let ready = {
                let s = network.shared.lock().unwrap();
                s.positioned
                    && (s.current[0] - 0.5).abs() < 0.05
                    && s.inventory
                        .slots
                        .get(36)
                        .and_then(Option::as_ref)
                        .is_some_and(|i| i.id == 276)
                    && s.entities.iter().any(|(_, e)| {
                        matches!(e.kind, horizon_world_189::entities::Kind::Mob(54))
                            && (e.position[2] - 2.5).abs() < 0.1
                    })
            };
            let fixture = std::env::var_os("HORIZON_189_FIXTURE_READY")
                .is_some_and(|p| std::path::Path::new(&p).exists());
            if ready && fixture {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "reference fixture setup timed out"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        {
            let mut state = network.shared.lock().unwrap();
            state.input.actions.push_back(Action::Select(0));
            state.input.actions.push_back(Action::Attack);
        }
        loop {
            let hit = network
                .shared
                .lock()
                .unwrap()
                .entities
                .iter()
                .any(|(_, e)| e.hurt_time > 0);
            if hit {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "server never confirmed attack hurt status"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("REFERENCE_ENTITY_ATTACK_CONFIRMED");
        // Use the sword in air, release it, and select the fixture block stack.
        {
            let mut s = network.shared.lock().unwrap();
            s.yaw = 90.0;
            s.pitch = -80.0;
            s.input.using = true;
            s.input.actions.push_back(Action::Use);
        }
        let use_deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !network.shared.lock().unwrap().using_item {
            assert!(
                std::time::Instant::now() < use_deadline,
                "sword use did not begin"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        {
            let mut s = network.shared.lock().unwrap();
            s.input.using = false;
            s.input.actions.push_back(Action::Select(1));
            s.yaw = 0.0;
            s.pitch = 60.0;
        }
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(network.shared.lock().unwrap().inventory.selected, 1);
        assert!(!network.shared.lock().unwrap().using_item);
        {
            let mut s = network.shared.lock().unwrap();
            s.input.actions.push_back(Action::Use);
        }
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            network.shared.lock().unwrap().inventory.slots[37]
                .as_ref()
                .unwrap()
                .count,
            15
        );
        println!("REFERENCE_BLOCK_PLACEMENT_SENT");
        // Leave the server time to report the placed block before mining it.
        std::thread::sleep(Duration::from_millis(400));
        {
            let mut s = network.shared.lock().unwrap();
            s.input.attack = true;
            s.input.actions.push_back(Action::Attack);
        }
        std::thread::sleep(Duration::from_secs(11));
        network.shared.lock().unwrap().input.attack = false;
        println!("REFERENCE_DIG_FINISHED");
        std::thread::sleep(Duration::from_millis(400));
        // Open the chest placed to the right by the managed reference fixture.
        {
            let mut s = network.shared.lock().unwrap();
            s.yaw = -90.0;
            s.pitch = 30.0;
            s.input.actions.push_back(Action::Use);
        }
        loop {
            if network.shared.lock().unwrap().inventory.window != 0 {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "chest did not open");
            std::thread::sleep(Duration::from_millis(20));
        }
        {
            let mut s = network.shared.lock().unwrap();
            assert!(s.inventory.open);
            s.input.actions.push_back(Action::Click {
                slot: 0,
                right: false,
            });
        }
        std::thread::sleep(Duration::from_millis(250));
        assert!(network.shared.lock().unwrap().inventory.cursor.is_some());
        network
            .shared
            .lock()
            .unwrap()
            .input
            .actions
            .push_back(Action::CloseInventory);
        std::thread::sleep(Duration::from_millis(150));
        assert!(!network.shared.lock().unwrap().inventory.open);
        println!("REFERENCE_CONTAINER_CLICK_CONFIRMED");
        println!("REFERENCE_SPECTATOR_REQUESTED");
        loop {
            if network.shared.lock().unwrap().game_mode == 3 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "spectator mode did not arrive"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        {
            let mut s = network.shared.lock().unwrap();
            s.yaw = 0.0;
            s.pitch = 0.0;
            s.keys.forward = true;
        }
        std::thread::sleep(Duration::from_secs(1));
        {
            let mut s = network.shared.lock().unwrap();
            s.keys.forward = false;
            assert!(
                s.current[2] > 3.0,
                "spectator collided with the fixture wall"
            );
        }
        println!("REFERENCE_SPECTATOR_WALL_PASS");
        println!("REFERENCE_DEATH_REQUESTED");
        loop {
            if network.shared.lock().unwrap().vitals.health <= 0.0 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "death health did not arrive"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        network
            .shared
            .lock()
            .unwrap()
            .input
            .actions
            .push_back(Action::Respawn);
        loop {
            let state = network.shared.lock().unwrap();
            let alive = state.vitals.health > 0.0 && state.positioned;
            drop(state);
            if alive {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "respawn did not finish"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        println!("REFERENCE_RESPAWN_CONFIRMED");
        drop(network);
    }
}
