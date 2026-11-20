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

#[derive(Clone, Debug)]
pub enum Action {
    Respawn,
    Attack,
    Use,
    Select(u8),
    /// Q in the world: drop one, or the stack with Control.
    Drop {
        all: bool,
    },
    OpenInventory,
    CloseInventory,
    ContainerGesture {
        window: u8,
        clicks: Vec<crate::container_input::Click>,
    },
    /// C0E: pickup, shift/swap/drop, quick-craft (5) and collect (6).
    Click {
        slot: i16,
        mode: u8,
        button: u8,
    },
}

#[derive(Default)]
pub struct Input {
    pub attack: bool,
    pub using: bool,
    pub actions: VecDeque<Action>,
}

#[derive(Default)]
pub struct Controller {
    /// PlayerControllerMP.currentPlayerItem survives replacement of the local player.
    sent_slot: u8,
    digging: Option<([i32; 3], u8, f32, Option<Item>)>,
    delay: u8,
    right_delay: u8,
    miss_delay: u8,
    pub swing: Option<u8>,
    pub using: bool,
    /// Ticks the held item has been in use.
    pub use_ticks: u32,
    /// Critical-hit particles to show: target and whether magic (Sharpness).
    pub effects: Vec<(i32, bool)>,
    /// Blocks this player just broke, for `addBlockDestroyEffects`.
    pub broken: Vec<[i32; 3]>,
    pub haste: Option<i8>,
    pub fatigue: Option<i8>,
    /// `PlayerCapabilities.disableDamage`, from S39.
    pub invulnerable: bool,
    raycaster: raycast::Raycaster,
}

impl Controller {
    pub fn reset_player(&mut self) {
        *self = Self {
            sent_slot: self.sent_slot,
            ..Self::default()
        };
    }

    /// `PlayerControllerMP.syncCurrentPlayItem`.
    pub fn sync_slot(
        &mut self,
        slot: u8,
        writer: &mut PacketWriter<impl Read + Write>,
    ) -> io::Result<()> {
        if slot != self.sent_slot {
            writer.send(&packet::held(slot)?)?;
            self.sent_slot = slot;
        }
        Ok(())
    }

    /// `PlayerControllerMP.sendUseItem` for the held stack: nothing in
    /// spectator, otherwise sync, then C08 at (-1, -1, -1) for any item, and
    /// items with a use duration start being used.
    fn use_held(
        &mut self,
        inventory: &Inventory,
        food: i32,
        game_mode: u8,
        writer: &mut PacketWriter<impl Read + Write>,
    ) -> io::Result<()> {
        let Some(item) = inventory.held() else {
            return Ok(());
        };
        if game_mode == 3 {
            return Ok(());
        }
        self.sync_slot(inventory.view.selected, writer)?;
        writer.send(&packet::place([-1, -1, -1], 255, [0.0; 3], Some(item))?)?;
        // Item.onItemRightClick: ItemFood needs PlayerCapabilities and
        // FoodStats to allow eating, ItemBow an arrow outside creative.
        self.using = has_use_action(item)
            && match item.id {
                261 => {
                    game_mode == 1
                        || inventory.player[9..45]
                            .iter()
                            .flatten()
                            .any(|i| i.id == 262)
                }
                322 => !self.invulnerable,
                id if is_food(id) => food < 20 && !self.invulnerable,
                _ => true,
            };
        Ok(())
    }

    /// The block being dug and `curBlockDamageMP`, 0 to 1.
    pub fn dig_progress(&self) -> Option<([i32; 3], f32)> {
        self.digging
            .as_ref()
            .map(|(at, _, damage, _)| (*at, *damage))
    }

    /// `EntityPlayerSP.swingItem`: C0A every call; the local animation only
    /// restarts when idle or past half-way (`EntityLivingBase.swingItem`).
    fn swing(&mut self, writer: &mut PacketWriter<impl Read + Write>) -> io::Result<()> {
        if self.swing.is_none_or(|v| v >= 3) {
            self.swing = Some(0);
        }
        writer.send(&packet::swing())
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
        self.sync_slot(inventory.view.selected, writer)?;
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
                // A key or wheel selection only changes currentItem. C09
                // waits for syncCurrentPlayItem, and EntityPlayer.onUpdate
                // drops an item in use that is no longer held, without a
                // release packet; a dig in progress restarts from
                // onPlayerDamageBlock's tool check.
                Action::Select(slot) if slot <= 8 && slot != inventory.view.selected => {
                    self.using = false;
                    inventory.view.selected = slot;
                }
                Action::OpenInventory => {
                    self.abort(writer)?;
                    inventory.local_open();
                    writer.send(&[0x16, 2])?;
                }
                Action::CloseInventory => writer.send(&inventory.close())?,
                Action::ContainerGesture { window, clicks } => {
                    if inventory.accepts_gesture(window, clicks.len()) && clicks.len() <= 83 {
                        for click in clicks {
                            if let Some(packet) =
                                inventory.click_mode(click.slot, click.mode, click.button)?
                            {
                                writer.send(&packet)?;
                            }
                        }
                    }
                }
                Action::Click { slot, mode, button } => {
                    if let Some(packet) = inventory.click_mode(slot, mode, button)? {
                        writer.send(&packet)?;
                    }
                }
                Action::Drop { all } if !inventory.view.open && game_mode != 3 => {
                    // EntityPlayerSP.dropOneItem: DROP_ALL_ITEMS or DROP_ITEM
                    // at the origin, facing down, even with an empty hand. The
                    // stack is not changed locally; the server's S2F follows.
                    writer.send(&packet::dig(if all { 3 } else { 4 }, [0; 3], 0)?)?;
                }
                Action::Attack if !inventory.view.open && self.miss_delay == 0 && !self.using => {
                    self.swing(writer)?;
                    match hit {
                        Some(Hit::Entity { id, .. }) => {
                            // attackEntity syncs after clickMouse's swing and
                            // sends C02 in every mode (a spectator spectates).
                            self.sync_slot(inventory.view.selected, writer)?;
                            writer.send(&packet::use_entity(id, 1, None)?)?;
                            if game_mode != 3 {
                                // attackTargetEntityWithCurrentItem: other players
                                // accept the client-side hit, so crits show at once.
                                let other_player = entities.get(id).is_some_and(|e| {
                                    matches!(
                                        e.kind,
                                        horizon_world_189::entities::Kind::Player { .. }
                                    )
                                });
                                let critical = player.fall_distance > 0.0
                                    && !player.on_ground
                                    && !player.in_water
                                    && !player.blindness;
                                if other_player && self.effects.len() < 64 {
                                    if critical {
                                        self.effects.push((id, false));
                                    }
                                    if inventory.held().is_some_and(|i| i.enchantment(16) > 0) {
                                        self.effects.push((id, true));
                                    }
                                }
                                // attackTargetEntityWithCurrentItem: with knockback
                                // (the enchantment, plus one while sprinting) and a
                                // target whose client-side attackEntityFrom succeeds
                                // (other players, boats, minecarts; not mobs), motion
                                // drops to 0.6 and sprinting stops.
                                let knockback = inventory.held().map_or(0, |i| i.enchantment(19))
                                    + i32::from(player.sprinting);
                                let accepted = entities.get(id).is_some_and(|e| {
                                    matches!(
                                        e.kind,
                                        horizon_world_189::entities::Kind::Player { .. }
                                            | horizon_world_189::entities::Kind::Object(1 | 10)
                                    )
                                });
                                if accepted && knockback > 0 {
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
                                self.broken.push(at);
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
                // onStoppedUsingItem syncs before C07 RELEASE_USE_ITEM.
                self.sync_slot(inventory.view.selected, writer)?;
                writer.send(&packet::dig(5, [0; 3], 0)?)?;
            }
            self.using = false;
            player.using_item = false;
            return Ok(());
        }
        if self.using && !input.using {
            self.sync_slot(inventory.view.selected, writer)?;
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
                    let kind = entities.get(id).map(|e| e.kind);
                    // isPlayerRightClickingOnEntity syncs first; an armour
                    // stand's interactAt consumes the click, so INTERACT and
                    // the item use follow only for other entities.
                    self.sync_slot(inventory.view.selected, writer)?;
                    writer.send(&packet::use_entity(id, 2, Some(relative))?)?;
                    let stand = kind == Some(horizon_world_189::entities::Kind::Object(78));
                    if !stand {
                        writer.send(&packet::use_entity(id, 0, None)?)?;
                    }
                    let consumed = stand
                        || matches!(
                            kind,
                            Some(horizon_world_189::entities::Kind::Mob(100 | 120))
                        );
                    if !consumed {
                        self.use_held(inventory, player.food, game_mode, writer)?;
                    }
                }
                Some(Hit::Block { at, face, point }) => {
                    let relative = std::array::from_fn(|a| {
                        (point[a] - f64::from(at[a])).clamp(0.0, 1.0) as f32
                    });
                    // onPlayerRightClick syncs first.
                    self.sync_slot(inventory.view.selected, writer)?;
                    writer.send(&packet::place(at, face, relative, inventory.held())?)?;
                    let block = world
                        .block(at[0], at[1], at[2])
                        .unwrap_or_default()
                        .block_id();
                    // onBlockActivated runs unless sneaking with an item;
                    // otherwise the item's onItemUse may consume the click.
                    let activated = (!player.sneaking || inventory.held().is_none())
                        && interactive_block(block);
                    let consumed = activated
                        || inventory
                            .held()
                            .is_some_and(|item| item_use_on_block(item, block, face));
                    // rightClickMouse swings only for a consumed click and
                    // otherwise falls through to sendUseItem.
                    if consumed {
                        self.swing(writer)?;
                    } else {
                        self.use_held(inventory, player.food, game_mode, writer)?;
                    }
                }
                None => self.use_held(inventory, player.food, game_mode, writer)?,
            }
        }
        // onItemUseFinish: food and drinks end after 32 ticks without a
        // release packet; the server finishes them too.
        if self.using {
            self.use_ticks += 1;
            let consumable = inventory
                .held()
                .is_some_and(|i| !is_sword(i.id) && i.id != 261);
            if consumable && self.use_ticks >= 32 {
                self.using = false;
            }
        }
        if !self.using {
            self.use_ticks = 0;
        }
        player.using_item = self.using;
        if !input.attack || self.using {
            self.abort(writer)?;
            return Ok(());
        }
        let Some(Hit::Block { at, face, .. }) = hit else {
            self.abort(writer)?;
            return Ok(());
        };
        if game_mode != 0 && game_mode != 1 {
            return Ok(());
        }
        // onPlayerDamageBlock: sync first, then count blockHitDelay down,
        // only while a block is targeted.
        self.sync_slot(inventory.view.selected, writer)?;
        if self.delay > 0 {
            self.delay -= 1;
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
            self.broken.push(at);
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
/// Blocks whose `onBlockActivated` handles a right-click (containers,
/// doors, levers, beds, crafting and the like).
fn interactive_block(block: u16) -> bool {
    matches!(
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
    )
}

/// Whether `Item.onItemUse` normally consumes a click on `block` (placing a
/// block or applying the item), so rightClickMouse sends no use after it.
/// Items without onItemUse (swords, bows, food, potions, buckets, rods,
/// throwables) and conditional ones off their target fall through to
/// sendUseItem.
fn item_use_on_block(item: &Item, block: u16, face: u8) -> bool {
    let rail = matches!(block, 27 | 28 | 66 | 157);
    let side = (2..=5).contains(&face);
    match item.id {
        1..=255 => true,
        // Flint and steel, fire charges, spawn eggs, fireworks, armour stands.
        259 | 383 | 385 | 401 | 416 => true,
        // Hoes till dirt and grass from above or the side.
        290..=294 => matches!(block, 2 | 3) && face != 0,
        // Seeds, carrots and potatoes plant on farmland; wart on soul sand.
        295 | 361 | 362 | 391 | 392 => block == 60 && face == 1,
        372 => block == 88 && face == 1,
        // Paintings and item frames hang on sides.
        321 | 389 => side,
        // Doors and beds go on top faces; signs and skulls on any but the
        // bottom; ItemReed-style blocks and redstone on any face.
        324 | 330 | 355 | 427..=431 => face == 1,
        323 | 397 => face != 0,
        331 | 338 | 354 | 356 | 379 | 380 | 390 | 404 => true,
        // Minecarts need rails.
        328 | 342 | 343 | 407 | 408 => rail,
        // Bone meal on growing plants, cocoa beans on jungle log sides.
        351 if item.damage == 15 => {
            matches!(block, 2 | 6 | 31 | 59 | 104 | 105 | 141 | 142 | 175)
        }
        351 if item.damage == 3 => block == 17 && side,
        // Leads tie to fences.
        420 => matches!(block, 85 | 113 | 188..=192),
        _ => false,
    }
}

/// Items with a use duration: swords block, bows draw, food and drinks.
fn has_use_action(item: &Item) -> bool {
    is_sword(item.id)
        || item.id == 261
        || item.id == 335
        || (item.id == 373 && item.damage & 16384 == 0)
        || is_food(item.id)
}

/// `ItemFood` and its subclasses (soups, fish, golden apples).
fn is_food(id: i16) -> bool {
    matches!(
        id,
        260 | 282
            | 297
            | 319
            | 320
            | 322
            | 349
            | 350
            | 357
            | 360
            | 363..=367
            | 391..=394
            | 400
            | 412
            | 413
            | 423
            | 424
    )
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
    /// EntityPlayerSP.swingItem sends C0A on every call, even while the arm
    /// is still mid-swing: each click's C02 follows its own C0A, and digging
    /// swings every tick. Only the local animation restarts at half-way.
    #[test]
    fn every_swing_sends_c0a_even_mid_animation() {
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
        let world = world();
        let mut player = Player::default();
        player.set_position(0.5, 4.0, 0.5);
        player.on_ground = true;
        let mut inventory = Inventory::default();
        let mut controller = Controller::default();
        for click in 0..3 {
            let mut input = Input {
                attack: true,
                using: false,
                actions: VecDeque::from([Action::Attack]),
            };
            controller
                .tick(
                    &mut input,
                    &mut player,
                    &world,
                    &entities,
                    &mut inventory,
                    0,
                    &mut writer,
                )
                .unwrap();
            assert_eq!(peer.receive().unwrap(), [0x0a], "click {click}");
            assert_eq!(peer.receive().unwrap(), [0x02, 7, 1], "click {click}");
        }
        // The animation restarted only at the first click: not yet half-way.
        assert_eq!(controller.swing, Some(2));
    }

    /// Item.onItemRightClick decides whether use starts: food needs hunger
    /// (golden apples excepted) and a vulnerable player, a bow needs an
    /// arrow outside creative. C08 is sent either way.
    #[test]
    fn use_starts_only_when_on_item_right_click_would() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let (_, mut writer) = Connection::new(client).split().unwrap();
        let mut peer = Connection::new(peer);
        let world = world();
        let entities = Entities::default();
        let item = |id| {
            Some(Item {
                id,
                count: 1,
                damage: 0,
                nbt: None,
            })
        };
        // (held, food, game mode, invulnerable, arrow in main inventory)
        for (held, food, mode, invulnerable, arrow, expected) in [
            (364, 20, 0, false, false, false),
            (364, 19, 0, false, false, true),
            (364, 10, 0, true, false, false),
            (322, 20, 0, false, false, true),
            (322, 10, 1, true, false, false),
            (261, 20, 0, false, false, false),
            (261, 20, 0, false, true, true),
            (261, 20, 1, true, false, true),
            (276, 20, 0, false, false, true),
            (335, 20, 0, false, false, true),
        ] {
            let mut player = Player::default();
            player.set_position(0.5, 4.0, 0.5);
            player.pitch = -90.0;
            player.food = food;
            let mut inventory = Inventory::default();
            inventory.player[36] = item(held);
            if arrow {
                inventory.player[20] = item(262);
            }
            let mut controller = Controller {
                invulnerable,
                ..Controller::default()
            };
            let mut input = Input {
                attack: false,
                using: true,
                actions: VecDeque::from([Action::Use]),
            };
            controller
                .tick(
                    &mut input,
                    &mut player,
                    &world,
                    &entities,
                    &mut inventory,
                    mode,
                    &mut writer,
                )
                .unwrap();
            assert_eq!(peer.receive().unwrap()[0], 0x08, "{held}");
            assert_eq!(controller.using, expected, "{held} food {food} mode {mode}");
            if controller.using {
                input.using = false;
                controller
                    .tick(
                        &mut input,
                        &mut player,
                        &world,
                        &entities,
                        &mut inventory,
                        mode,
                        &mut writer,
                    )
                    .unwrap();
                assert_eq!(peer.receive().unwrap()[0], 0x07);
            }
        }
    }

    /// EntityPlayerSP.dropOneItem sends DROP_ITEM/DROP_ALL_ITEMS whatever is
    /// held and changes nothing locally: the server's slot update follows.
    #[test]
    fn drop_key_sends_c07_without_predicting_the_stack() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let (_, mut writer) = Connection::new(client).split().unwrap();
        let mut peer = Connection::new(peer);
        let entities = Entities::default();
        let world = world();
        let mut player = Player::default();
        player.set_position(0.5, 4.0, 0.5);
        let mut inventory = Inventory::default();
        let pearls = Some(Item {
            id: 368,
            count: 4,
            damage: 0,
            nbt: None,
        });
        inventory.player[36] = pearls.clone();
        inventory.view.slots[36] = pearls.clone();
        let mut controller = Controller::default();
        for (selected, all, mode, status) in [
            (0, false, 0, 4),
            (0, true, 0, 3),
            (1, false, 0, 4),
            (0, false, 3, 4),
        ] {
            inventory.view.selected = selected;
            let mut input = Input {
                attack: false,
                using: false,
                actions: VecDeque::from([Action::Drop { all }]),
            };
            controller
                .tick(
                    &mut input,
                    &mut player,
                    &world,
                    &entities,
                    &mut inventory,
                    mode,
                    &mut writer,
                )
                .unwrap();
            if mode == 3 {
                break;
            }
            let mut packet = peer.receive().unwrap();
            if packet[0] == 0x09 {
                packet = peer.receive().unwrap();
            }
            assert_eq!(packet, packet::dig(status, [0; 3], 0).unwrap());
            assert_eq!(inventory.player[36], pearls);
            assert_eq!(inventory.view.slots[36], pearls);
        }
        // Spectators drop nothing: only the tick's own sync precedes the next.
        controller.sync_slot(5, &mut writer).unwrap();
        assert_eq!(peer.receive().unwrap(), [0x09, 0, 0]);
        assert_eq!(peer.receive().unwrap(), [0x09, 0, 5]);
    }
    /// PlayerControllerMP.syncCurrentPlayItem: a selection only changes
    /// `currentItem`; C09 follows at the next updateController or just
    /// before an attack/use packet, and switching away from a used item
    /// clears it locally without a release packet.
    #[test]
    fn hotbar_selection_syncs_like_player_controller_mp() {
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
        let sword = Some(Item {
            id: 276,
            count: 1,
            damage: 0,
            nbt: None,
        });
        inventory.player[41] = sword.clone();
        let world = world();
        let mut controller = Controller::default();
        let mut tick = |controller: &mut Controller,
                        inventory: &mut Inventory,
                        player: &mut Player,
                        actions: Vec<Action>,
                        attack: bool,
                        using: bool| {
            let mut input = Input {
                attack,
                using,
                actions: VecDeque::from(actions),
            };
            controller
                .tick(
                    &mut input,
                    player,
                    &world,
                    &entities,
                    inventory,
                    0,
                    &mut writer,
                )
                .unwrap();
        };
        // A selection alone sends nothing until the next tick.
        tick(
            &mut controller,
            &mut inventory,
            &mut player,
            vec![Action::Select(3)],
            false,
            false,
        );
        tick(
            &mut controller,
            &mut inventory,
            &mut player,
            vec![],
            false,
            false,
        );
        assert_eq!(peer.receive().unwrap(), [0x09, 0, 3]);
        // Select then attack in one tick: swing, sync, attack.
        tick(
            &mut controller,
            &mut inventory,
            &mut player,
            vec![Action::Select(5), Action::Attack],
            true,
            false,
        );
        assert_eq!(peer.receive().unwrap(), [0x0a]);
        assert_eq!(peer.receive().unwrap(), [0x09, 0, 5]);
        assert_eq!(peer.receive().unwrap(), [0x02, 7, 1]);
        // Block with the sword on the player: interact-at, interact, use.
        tick(
            &mut controller,
            &mut inventory,
            &mut player,
            vec![Action::Use],
            false,
            true,
        );
        assert_eq!(peer.receive().unwrap()[..3], [0x02, 7, 2]);
        assert_eq!(peer.receive().unwrap(), [0x02, 7, 0]);
        assert_eq!(peer.receive().unwrap()[0], 0x08);
        assert!(controller.using);
        // Switching away clears the use locally: no C07 release, then C09.
        tick(
            &mut controller,
            &mut inventory,
            &mut player,
            vec![Action::Select(6)],
            false,
            false,
        );
        assert!(!controller.using);
        tick(
            &mut controller,
            &mut inventory,
            &mut player,
            vec![],
            false,
            false,
        );
        assert_eq!(peer.receive().unwrap(), [0x09, 0, 6]);
        // Re-selecting the synced slot is silent.
        tick(
            &mut controller,
            &mut inventory,
            &mut player,
            vec![Action::Select(6)],
            false,
            false,
        );
        tick(
            &mut controller,
            &mut inventory,
            &mut player,
            vec![Action::Select(2)],
            false,
            false,
        );
        tick(
            &mut controller,
            &mut inventory,
            &mut player,
            vec![],
            false,
            false,
        );
        assert_eq!(peer.receive().unwrap(), [0x09, 0, 2]);
    }

    /// Minecraft.rightClickMouse: an unconsumed block or entity click is
    /// followed by sendUseItem for whatever is held (eating a golden apple
    /// at the floor or at a player); an armour stand's INTERACT_AT consumes
    /// the click, so no INTERACT or use follows.
    #[test]
    fn right_click_uses_held_food_after_unconsumed_block_and_entity_clicks() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let (_, mut writer) = Connection::new(client).split().unwrap();
        let mut peer = Connection::new(peer);
        let mut world = world();
        world.set_block(0, 3, 0, BlockState(1 << 4));
        let mut player = Player::default();
        player.set_position(0.5, 4.0, 0.5);
        player.on_ground = true;
        player.pitch = 90.0;
        let mut inventory = Inventory::default();
        inventory.player[36] = Some(Item {
            id: 322,
            count: 1,
            damage: 0,
            nbt: None,
        });
        let mut controller = Controller::default();
        let mut use_once = |controller: &mut Controller,
                            player: &mut Player,
                            entities: &Entities,
                            inventory: &mut Inventory| {
            let mut input = Input {
                attack: false,
                using: true,
                actions: VecDeque::from([Action::Use]),
            };
            controller
                .tick(
                    &mut input,
                    player,
                    &world,
                    entities,
                    inventory,
                    0,
                    &mut writer,
                )
                .unwrap();
        };
        // The floor: block placement, then the apple's use.
        let none = Entities::default();
        use_once(&mut controller, &mut player, &none, &mut inventory);
        let place = peer.receive().unwrap();
        assert_eq!(place[0], 0x08);
        assert_ne!(place[1..9], [0xff; 8][..]);
        let used = peer.receive().unwrap();
        assert_eq!(used[0], 0x08);
        assert_eq!(
            used[1..9],
            packet::place([-1, -1, -1], 255, [0.0; 3], None).unwrap()[1..9]
        );
        assert!(controller.using);
        // A player ahead: INTERACT_AT, INTERACT, then the apple's use.
        controller.using = false;
        controller.right_delay = 0;
        player.pitch = 0.0;
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
        use_once(&mut controller, &mut player, &entities, &mut inventory);
        assert_eq!(peer.receive().unwrap()[..3], [0x02, 7, 2]);
        assert_eq!(peer.receive().unwrap(), [0x02, 7, 0]);
        assert_eq!(peer.receive().unwrap()[0], 0x08);
        assert!(controller.using);
        // An armour stand: INTERACT_AT only.
        controller.using = false;
        controller.right_delay = 0;
        let mut stands = Entities::default();
        stands.apply(EntityPacket::SpawnObject {
            id: 9,
            kind: 78,
            position: [16, 4 * 32, 2 * 32],
            pitch: 0,
            yaw: 0,
            data: 0,
            velocity: None,
        });
        use_once(&mut controller, &mut player, &stands, &mut inventory);
        assert_eq!(peer.receive().unwrap()[..3], [0x02, 9, 2]);
        assert!(!controller.using);
        // Nothing else was sent: the next packet is a fresh swing.
        controller.swing(&mut writer).unwrap();
        assert_eq!(peer.receive().unwrap(), [0x0a]);
    }

    /// attackTargetEntityWithCurrentItem: the 0.6 slow-down and sprint stop
    /// need knockback (sprinting or the enchantment) and a target whose
    /// client-side attackEntityFrom succeeds: other players, not mobs.
    #[test]
    fn hit_slowdown_needs_knockback_and_a_player_target() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_peer, _) = listener.accept().unwrap();
        let (_, mut writer) = Connection::new(client).split().unwrap();
        let world = world();
        let attack = |entities: &Entities,
                      sprinting: bool,
                      knockback: bool,
                      writer: &mut PacketWriter<TcpStream>| {
            let mut player = Player::default();
            player.set_position(0.5, 4.0, 0.5);
            player.on_ground = true;
            player.sprinting = sprinting;
            player.motion = [0.2, 0.0, 0.3];
            let mut inventory = Inventory::default();
            let nbt = knockback.then(|| {
                use horizon_protocol_47::nbt::Tag;
                Tag::Compound(vec![(
                    "ench".into(),
                    Tag::List(vec![Tag::Compound(vec![
                        ("id".into(), Tag::Short(19)),
                        ("lvl".into(), Tag::Short(1)),
                    ])]),
                )])
            });
            inventory.player[36] = Some(Item {
                id: 276,
                count: 1,
                damage: 0,
                nbt,
            });
            let mut input = Input {
                attack: true,
                using: false,
                actions: VecDeque::from([Action::Attack]),
            };
            Controller::default()
                .tick(
                    &mut input,
                    &mut player,
                    &world,
                    entities,
                    &mut inventory,
                    0,
                    writer,
                )
                .unwrap();
            (player.sprinting, player.motion)
        };
        let mut players = Entities::default();
        players.apply(EntityPacket::SpawnPlayer {
            id: 7,
            uuid: 0,
            position: [16, 4 * 32, 2 * 32],
            yaw: 0,
            pitch: 0,
            held_item: 0,
            metadata: vec![],
        });
        let mut mobs = Entities::default();
        mobs.apply(EntityPacket::SpawnMob {
            id: 8,
            kind: 54,
            position: [16, 4 * 32, 2 * 32],
            yaw: 0,
            pitch: 0,
            head_yaw: 0,
            metadata: Vec::new(),
        });
        let slowed = [0.2 * 0.6, 0.0, 0.3 * 0.6];
        assert_eq!(attack(&players, true, false, &mut writer), (false, slowed));
        assert_eq!(attack(&players, false, true, &mut writer), (false, slowed));
        assert_eq!(
            attack(&players, false, false, &mut writer),
            (false, [0.2, 0.0, 0.3])
        );
        // A zombie's client-side attackEntityFrom fails: no slow-down.
        assert_eq!(
            attack(&mobs, true, false, &mut writer),
            (true, [0.2, 0.0, 0.3])
        );
    }

    /// A spectator's left click still swings and sends attackEntity's C02
    /// (the server makes it spectate the target); only the local hit is
    /// skipped.
    #[test]
    fn spectator_attack_sends_c02_to_spectate() {
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
        player.sprinting = true;
        let mut inventory = Inventory::default();
        let mut input = Input {
            attack: true,
            using: false,
            actions: VecDeque::from([Action::Attack]),
        };
        Controller::default()
            .tick(
                &mut input,
                &mut player,
                &world(),
                &entities,
                &mut inventory,
                3,
                &mut writer,
            )
            .unwrap();
        assert_eq!(peer.receive().unwrap(), [0x0a]);
        assert_eq!(peer.receive().unwrap(), [0x02, 7, 1]);
        assert!(player.sprinting);
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
        inventory.view.selected = 4;
        inventory.player[40] = Some(Item {
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
        assert_eq!(peer.receive().unwrap(), [0x09, 0, 4]);
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
                session: std::sync::Arc::new(
                    horizon_auth::Session::offline("NativeCombat", None).unwrap(),
                ),
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
                mode: 0,
                button: 0,
            });
        }
        std::thread::sleep(Duration::from_millis(250));
        assert!(network.shared.lock().unwrap().inventory.cursor.is_some());
        {
            let mut s = network.shared.lock().unwrap();
            let window = s.inventory.window;
            let clicks = vec![(-999, 0), (1, 1), (2, 1), (-999, 2)]
                .into_iter()
                .map(|(slot, button)| crate::container_input::Click {
                    slot,
                    mode: 5,
                    button,
                })
                .collect();
            s.input
                .actions
                .push_back(Action::ContainerGesture { window, clicks });
        }
        std::thread::sleep(Duration::from_millis(300));
        {
            let s = network.shared.lock().unwrap();
            assert_eq!(s.inventory.slots[1].as_ref().unwrap().count, 1);
            assert_eq!(s.inventory.slots[2].as_ref().unwrap().count, 1);
            assert_eq!(s.inventory.cursor.as_ref().unwrap().count, 1);
        }
        println!("REFERENCE_DRAG_SPLIT_SENT");
        // Wait for the runner's independent server NBT assertion before collecting.
        std::thread::sleep(Duration::from_millis(500));
        {
            let mut s = network.shared.lock().unwrap();
            s.input.actions.push_back(Action::Click {
                slot: 3,
                mode: 6,
                button: 0,
            });
        }
        std::thread::sleep(Duration::from_millis(300));
        {
            let s = network.shared.lock().unwrap();
            assert!(s.inventory.slots[1].is_none() && s.inventory.slots[2].is_none());
            assert_eq!(s.inventory.cursor.as_ref().unwrap().count, 3);
        }
        println!("REFERENCE_COLLECT_SENT");
        std::thread::sleep(Duration::from_millis(500));
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
