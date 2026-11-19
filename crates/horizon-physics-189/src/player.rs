//! The local player's 1.8.9 tick: `EntityPlayerSP.onUpdate` as far as
//! movement goes. Water handling, input, sprint rules, jumping,
//! `moveEntityWithHeading`, `moveFlying` and `moveEntity` with sneak-edge
//! clipping and step-up keep vanilla's field types, float rounding, double
//! promotions and update order. Flying, riding and item use are not ported.
use crate::{
    Aabb, Axis, clip_offset,
    shapes::{add_coord, bb, colliding_boxes, contract, expand, floor, hi, lo, offset},
    trig,
};
use horizon_world_189::{
    BlockState, World,
    blocks::{block, is_fence, is_fence_gate, slipperiness},
};

pub const WIDTH: f32 = 0.6;
pub const HEIGHT: f32 = 1.8;
pub const EYE_HEIGHT: f32 = 1.62;
const STEP_HEIGHT: f32 = 0.6;
const SPEED_IN_AIR: f32 = 0.02;

pub const SPRINT_UUID: u128 = 0x662A6B8D_DA3E_4C1C_8813_96EA6097278D;
pub const SPEED_UUID: u128 = 0x91AEAA56_376B_4498_935B_2F7F68070635;
pub const SLOWNESS_UUID: u128 = 0x7107DE5E_7CE8_4030_940E_514C1F160890;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modifier {
    pub uuid: u128,
    pub amount: f64,
    pub operation: u8,
}

/// `ModifiableAttributeInstance` with Java `HashSet` iteration order: a
/// 16-bucket table indexed by the spread `UUID.hashCode`, chains in
/// insertion order. Floating-point products depend on that order.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    pub base: f64,
    modifiers: Vec<Modifier>,
}

fn bucket(uuid: u128) -> u32 {
    let hilo = ((uuid >> 64) as u64) ^ (uuid as u64);
    let hash = ((hilo >> 32) as u32) ^ (hilo as u32);
    (hash ^ (hash >> 16)) & 15
}

impl Attribute {
    pub fn new(base: f64) -> Self {
        Self {
            base,
            modifiers: Vec::new(),
        }
    }

    pub fn has(&self, uuid: u128) -> bool {
        self.modifiers.iter().any(|modifier| modifier.uuid == uuid)
    }

    pub fn apply(&mut self, modifier: Modifier) {
        self.remove(modifier.uuid);
        self.modifiers.push(modifier);
    }

    pub fn remove(&mut self, uuid: u128) {
        self.modifiers.retain(|modifier| modifier.uuid != uuid);
    }

    /// `S20PacketEntityProperties`: base, then `removeAllModifiers`, then each.
    pub fn replace(&mut self, base: f64, modifiers: &[Modifier]) {
        self.base = base;
        self.modifiers.clear();
        for &modifier in modifiers {
            self.apply(modifier);
        }
    }

    pub fn value(&self) -> f64 {
        let mut ordered: Vec<&Modifier> = self.modifiers.iter().collect();
        ordered.sort_by_key(|modifier| bucket(modifier.uuid));
        let of = |operation: u8| {
            ordered
                .iter()
                .filter(move |modifier| modifier.operation == operation)
        };
        let mut base = self.base;
        for modifier in of(0) {
            base += modifier.amount;
        }
        let mut value = base;
        for modifier in of(1) {
            value += base * modifier.amount;
        }
        for modifier in of(2) {
            value *= 1.0 + modifier.amount;
        }
        value.clamp(0.0, f64::MAX)
    }
}

#[derive(Clone, Debug)]
pub struct Player {
    pub bounds: Aabb,
    pub position: [f64; 3],
    pub motion: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub collided_horizontally: bool,
    pub collided_vertically: bool,
    pub sprinting: bool,
    pub sneaking: bool,
    pub in_water: bool,
    in_web: bool,
    jump_ticks: i32,
    sprint_toggle_timer: i32,
    sprinting_ticks_left: i32,
    input_forward: f32,
    input_strafe: f32,
    jump_movement_factor: f32,
    pub speed: Attribute,
    pub jump_boost: Option<i8>,
    pub blindness: bool,
    pub food: i32,
    pub using_item: bool,
    /// `PlayerCapabilities`: flying, allowFlying and the fly speed (0.05F).
    pub flying: bool,
    pub allow_flying: bool,
    pub fly_speed: f32,
    /// Set when the client toggled flight and owes a C13 abilities packet.
    pub abilities_changed: bool,
    /// Highest Depth Strider level on worn armour.
    pub depth_strider: i32,
    fly_toggle_timer: i32,
    previous_jump: bool,
    boxes: Vec<Aabb>,
}

impl Default for Player {
    fn default() -> Self {
        let mut player = Self {
            bounds: bb([0.0; 3], [0.0; 3]),
            position: [0.0; 3],
            motion: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            collided_horizontally: false,
            collided_vertically: false,
            sprinting: false,
            sneaking: false,
            in_water: false,
            in_web: false,
            jump_ticks: 0,
            sprint_toggle_timer: 0,
            sprinting_ticks_left: 0,
            input_forward: 0.0,
            input_strafe: 0.0,
            jump_movement_factor: SPEED_IN_AIR,
            speed: Attribute::new(0.10000000149011612),
            jump_boost: None,
            blindness: false,
            food: 20,
            using_item: false,
            flying: false,
            allow_flying: false,
            fly_speed: 0.05,
            abilities_changed: false,
            depth_strider: 0,
            fly_toggle_timer: 0,
            previous_jump: false,
            boxes: Vec::with_capacity(64),
        };
        player.set_position(0.0, 0.0, 0.0);
        player
    }
}

fn state(world: &World, x: i32, y: i32, z: i32) -> BlockState {
    world.block(x, y, z).unwrap_or_default()
}

/// `Block.isNormalCube`: opaque material, full cube, provides no power.
fn normal_cube(id: u16) -> bool {
    id != 152 && (block(id).attaches_fences() || matches!(id, 86 | 91 | 103))
}

fn vec_add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn length(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// `Vec3.normalize`.
fn normalize(v: [f64; 3]) -> [f64; 3] {
    let d = length(v);
    if d < 1.0e-4 {
        [0.0; 3]
    } else {
        [v[0] / d, v[1] / d, v[2] / d]
    }
}

/// `isAreaLoaded(x0, y0, z0, x1, y1, z1, true)`, inclusive block bounds.
fn area_loaded(world: &World, min: [i32; 3], max: [i32; 3]) -> bool {
    if max[1] < 0 || min[1] >= 256 {
        return false;
    }
    for cx in (min[0] >> 4)..=(max[0] >> 4) {
        for cz in (min[2] >> 4)..=(max[2] >> 4) {
            if world.column(cx, cz).is_none() {
                return false;
            }
        }
    }
    true
}

fn cells(b: Aabb) -> ([i32; 3], [i32; 3]) {
    let (l, h) = (lo(b), hi(b));
    (
        [floor(l[0]), floor(l[1]), floor(l[2])],
        [floor(h[0] + 1.0), floor(h[1] + 1.0), floor(h[2] + 1.0)],
    )
}

/// `World.isMaterialInBB` / `isAnyLiquid` over the half-open cell range.
fn any_in(world: &World, b: Aabb, test: impl Fn(u16) -> bool) -> bool {
    let (min, max) = cells(b);
    for x in min[0]..max[0] {
        for y in min[1]..max[1] {
            for z in min[2]..max[2] {
                if test(state(world, x, y, z).block_id()) {
                    return true;
                }
            }
        }
    }
    false
}

/// `BlockLiquid.getEffectiveFlowDecay` for water.
fn water_decay(world: &World, x: i32, y: i32, z: i32) -> i32 {
    let found = state(world, x, y, z);
    if !block(found.block_id()).water() {
        return -1;
    }
    let level = i32::from(found.metadata());
    if level >= 8 { 0 } else { level }
}

/// `BlockLiquid.getFlowVector` for water, `EnumFacing.Plane.HORIZONTAL` order.
fn flow_vector(world: &World, x: i32, y: i32, z: i32) -> [f64; 3] {
    const HORIZONTAL: [[i32; 2]; 4] = [[0, -1], [1, 0], [0, 1], [-1, 0]];
    let mut vector = [0.0; 3];
    let own = water_decay(world, x, y, z);
    for [dx, dz] in HORIZONTAL {
        let (nx, nz) = (x.wrapping_add(dx), z.wrapping_add(dz));
        let mut decay = water_decay(world, nx, y, nz);
        if decay < 0 {
            if !block(state(world, nx, y, nz).block_id()).blocks_movement() {
                decay = water_decay(world, nx, y - 1, nz);
                if decay >= 0 {
                    let k = decay - (own - 8);
                    vector = vec_add(vector, [f64::from(dx * k), 0.0, f64::from(dz * k)]);
                }
            }
        } else {
            let l = decay - own;
            vector = vec_add(vector, [f64::from(dx * l), 0.0, f64::from(dz * l)]);
        }
    }
    if state(world, x, y, z).metadata() >= 8 {
        let solid = |x: i32, y: i32, z: i32| {
            let id = state(world, x, y, z).block_id();
            let info = block(id);
            !info.water() && id != 79 && info.blocks_movement()
        };
        for [dx, dz] in HORIZONTAL {
            let (nx, nz) = (x.wrapping_add(dx), z.wrapping_add(dz));
            if solid(nx, y, nz) || solid(nx, y + 1, nz) {
                vector = vec_add(normalize(vector), [0.0, -6.0, 0.0]);
                break;
            }
        }
    }
    normalize(vector)
}

impl Player {
    /// `Entity.setPosition`.
    pub fn set_position(&mut self, x: f64, y: f64, z: f64) {
        self.position = [x, y, z];
        let half = f64::from(WIDTH / 2.0);
        self.bounds = bb(
            [x - half, y, z - half],
            [x + half, y + f64::from(HEIGHT), z + half],
        );
    }

    /// `NetHandlerPlayClient.handlePlayerPosLook`. Returns the position and
    /// rotation for the immediate C06 reply.
    pub fn teleport(&mut self, position: [f64; 3], yaw: f32, pitch: f32, relative: u8) {
        let mut target = position;
        for axis in 0..3 {
            if relative & (1 << axis) != 0 {
                target[axis] += self.position[axis];
            } else {
                self.motion[axis] = 0.0;
            }
        }
        // S08 flag bits: X, Y, Z, Y_ROT (yaw), X_ROT (pitch).
        let yaw = if relative & 8 != 0 {
            yaw + self.yaw
        } else {
            yaw
        };
        let pitch = if relative & 16 != 0 {
            pitch + self.pitch
        } else {
            pitch
        };
        self.set_position(target[0], target[1], target[2]);
        self.yaw = yaw % 360.0;
        self.pitch = pitch % 360.0;
    }

    /// `S12PacketEntityVelocity` for the local player.
    pub fn set_velocity(&mut self, x: i16, y: i16, z: i16) {
        self.motion = [
            f64::from(x) / 8000.0,
            f64::from(y) / 8000.0,
            f64::from(z) / 8000.0,
        ];
    }

    /// `S27PacketExplosion` player knockback.
    pub fn add_velocity(&mut self, x: f32, y: f32, z: f32) {
        self.motion[0] += f64::from(x);
        self.motion[1] += f64::from(y);
        self.motion[2] += f64::from(z);
    }

    /// `EntityLivingBase.setSprinting` with `EntityPlayerSP`'s 600-tick timer.
    pub fn set_sprinting(&mut self, sprinting: bool) {
        self.sprinting = sprinting;
        self.sprinting_ticks_left = if sprinting { 600 } else { 0 };
        self.speed.remove(SPRINT_UUID);
        if sprinting {
            self.speed.apply(Modifier {
                uuid: SPRINT_UUID,
                amount: 0.30000001192092896,
                operation: 2,
            });
        }
    }

    /// Whether `EntityPlayerSP.onUpdate` runs this tick.
    pub fn chunk_loaded(&self, world: &World) -> bool {
        world
            .column(floor(self.position[0]) >> 4, floor(self.position[2]) >> 4)
            .is_some()
    }

    pub fn tick(&mut self, world: &World, keys: Keys) {
        self.handle_water(world);
        self.living_update(world, keys);
    }

    fn handle_water(&mut self, world: &World) {
        let area = contract(
            expand(self.bounds, 0.0, -0.4000000059604645, 0.0),
            0.001,
            0.001,
            0.001,
        );
        let (min, max) = cells(area);
        self.in_water = false;
        if !area_loaded(world, min, max) {
            return;
        }
        let mut flow = [0.0; 3];
        for x in min[0]..max[0] {
            for y in min[1]..max[1] {
                for z in min[2]..max[2] {
                    let found = state(world, x, y, z);
                    if !block(found.block_id()).water() {
                        continue;
                    }
                    let level = found.metadata();
                    let percent = f32::from(if level >= 8 { 0 } else { level } + 1) / 9.0;
                    let surface = f64::from((y + 1) as f32 - percent);
                    if f64::from(max[1]) >= surface {
                        self.in_water = true;
                        flow = vec_add(flow, flow_vector(world, x, y, z));
                    }
                }
            }
        }
        if length(flow) > 0.0 {
            let flow = normalize(flow);
            for (motion, push) in self.motion.iter_mut().zip(flow) {
                *motion += push * 0.014;
            }
        }
    }

    fn in_lava(&self, world: &World) -> bool {
        any_in(
            world,
            expand(
                self.bounds,
                -0.10000000149011612,
                -0.4000000059604645,
                -0.10000000149011612,
            ),
            |id| block(id).lava(),
        )
    }

    fn on_ladder(&self, world: &World) -> bool {
        let id = state(
            world,
            floor(self.position[0]),
            floor(lo(self.bounds)[1]),
            floor(self.position[2]),
        )
        .block_id();
        id == 65 || id == 106
    }

    fn open_space(world: &World, x: i32, y: i32, z: i32) -> bool {
        !normal_cube(state(world, x, y, z).block_id())
            && !normal_cube(state(world, x, y + 1, z).block_id())
    }

    /// `EntityPlayerSP.pushOutOfBlocks`.
    fn push_out_of_blocks(&mut self, world: &World, x: f64, y: f64, z: f64) {
        let (bx, by, bz) = (floor(x), floor(y), floor(z));
        let dx = x - f64::from(bx);
        let dz = z - f64::from(bz);
        if Self::open_space(world, bx, by, bz) {
            return;
        }
        let mut side = -1;
        let mut best = 9999.0;
        if Self::open_space(world, bx - 1, by, bz) && dx < best {
            best = dx;
            side = 0;
        }
        if Self::open_space(world, bx + 1, by, bz) && 1.0 - dx < best {
            best = 1.0 - dx;
            side = 1;
        }
        if Self::open_space(world, bx, by, bz - 1) && dz < best {
            best = dz;
            side = 4;
        }
        if Self::open_space(world, bx, by, bz + 1) && 1.0 - dz < best {
            side = 5;
        }
        let f = 0.1f32;
        match side {
            0 => self.motion[0] = f64::from(-f),
            1 => self.motion[0] = f64::from(f),
            4 => self.motion[2] = f64::from(-f),
            5 => self.motion[2] = f64::from(f),
            _ => {}
        }
    }

    fn living_update(&mut self, world: &World, keys: Keys) {
        // EntityPlayerSP.onLivingUpdate
        if self.sprinting_ticks_left > 0 {
            self.sprinting_ticks_left -= 1;
            if self.sprinting_ticks_left == 0 {
                self.set_sprinting(false);
            }
        }
        if self.sprint_toggle_timer > 0 {
            self.sprint_toggle_timer -= 1;
        }
        let was_sneaking = self.sneaking;
        let was_forward = self.input_forward >= 0.8;
        let was_jumping = self.previous_jump;
        self.previous_jump = keys.jump;
        // MovementInputFromOptions.updatePlayerMoveState
        let mut strafe = 0.0f32;
        let mut forward = 0.0f32;
        if keys.forward {
            forward += 1.0;
        }
        if keys.back {
            forward -= 1.0;
        }
        if keys.left {
            strafe += 1.0;
        }
        if keys.right {
            strafe -= 1.0;
        }
        self.sneaking = keys.sneak;
        if keys.sneak {
            strafe = (f64::from(strafe) * 0.3) as f32;
            forward = (f64::from(forward) * 0.3) as f32;
        }
        if self.using_item {
            strafe *= 0.2;
            forward *= 0.2;
            self.sprint_toggle_timer = 0;
        }
        self.input_forward = forward;
        self.input_strafe = strafe;
        let half_width = f64::from(WIDTH) * 0.35;
        let base = lo(self.bounds)[1] + 0.5;
        let [x, _, z] = self.position;
        self.push_out_of_blocks(world, x - half_width, base, z + half_width);
        self.push_out_of_blocks(world, x - half_width, base, z - half_width);
        self.push_out_of_blocks(world, x + half_width, base, z - half_width);
        self.push_out_of_blocks(world, x + half_width, base, z + half_width);
        let fed = self.food as f32 > 6.0 || self.allow_flying;
        if self.on_ground
            && !was_sneaking
            && !was_forward
            && forward >= 0.8
            && !self.sprinting
            && fed
            && !self.blindness
            && !self.using_item
        {
            if self.sprint_toggle_timer <= 0 && !keys.sprint {
                self.sprint_toggle_timer = 7;
            } else {
                self.set_sprinting(true);
            }
        }
        if !self.sprinting
            && forward >= 0.8
            && fed
            && !self.blindness
            && !self.using_item
            && keys.sprint
        {
            self.set_sprinting(true);
        }
        if self.sprinting && (forward < 0.8 || self.collided_horizontally || !fed) {
            self.set_sprinting(false);
        }
        // Double-tap jump toggles flight within seven ticks.
        if self.allow_flying && !was_jumping && keys.jump {
            if self.fly_toggle_timer == 0 {
                self.fly_toggle_timer = 7;
            } else {
                self.flying = !self.flying;
                self.abilities_changed = true;
                self.fly_toggle_timer = 0;
            }
        }
        if self.flying {
            let lift = f64::from(self.fly_speed * 3.0);
            if keys.sneak {
                self.motion[1] -= lift;
            }
            if keys.jump {
                self.motion[1] += lift;
            }
        }
        // EntityPlayer.onLivingUpdate
        if self.fly_toggle_timer > 0 {
            self.fly_toggle_timer -= 1;
        }
        // EntityLivingBase.onLivingUpdate
        if self.jump_ticks > 0 {
            self.jump_ticks -= 1;
        }
        for motion in &mut self.motion {
            if motion.abs() < 0.005 {
                *motion = 0.0;
            }
        }
        if keys.jump {
            if self.in_water || self.in_lava(world) {
                self.motion[1] += 0.03999999910593033;
            } else if self.on_ground && self.jump_ticks == 0 {
                self.jump();
                self.jump_ticks = 10;
            }
        } else {
            self.jump_ticks = 0;
        }
        let strafe = self.input_strafe * 0.98;
        let forward = self.input_forward * 0.98;
        if self.flying {
            // EntityPlayer.moveEntityWithHeading while flying.
            let vertical = self.motion[1];
            let factor = self.jump_movement_factor;
            self.jump_movement_factor = self.fly_speed * if self.sprinting { 2.0 } else { 1.0 };
            self.move_with_heading(world, strafe, forward);
            self.motion[1] = vertical * 0.6;
            self.jump_movement_factor = factor;
        } else {
            self.move_with_heading(world, strafe, forward);
        }
        // EntityPlayer.onLivingUpdate after the living update.
        self.jump_movement_factor = if self.sprinting {
            (f64::from(SPEED_IN_AIR) + f64::from(SPEED_IN_AIR) * 0.3) as f32
        } else {
            SPEED_IN_AIR
        };
        // Survival flight ends on landing.
        if self.on_ground && self.flying {
            self.flying = false;
            self.abilities_changed = true;
        }
    }

    fn jump(&mut self) {
        self.motion[1] = f64::from(0.42f32);
        if let Some(amplifier) = self.jump_boost {
            self.motion[1] += f64::from((i32::from(amplifier) + 1) as f32 * 0.1);
        }
        if self.sprinting {
            let f = self.yaw * 0.017_453_292;
            self.motion[0] -= f64::from(trig::sin(f) * 0.2);
            self.motion[2] += f64::from(trig::cos(f) * 0.2);
        }
    }

    /// `Entity.moveFlying`.
    fn move_flying(&mut self, mut strafe: f32, mut forward: f32, friction: f32) {
        let mut f = strafe * strafe + forward * forward;
        if f >= 1.0e-4 {
            f = trig::sqrt_float(f);
            if f < 1.0 {
                f = 1.0;
            }
            f = friction / f;
            strafe *= f;
            forward *= f;
            let angle = self.yaw * std::f32::consts::PI / 180.0;
            let sin = trig::sin(angle);
            let cos = trig::cos(angle);
            self.motion[0] += f64::from(strafe * cos - forward * sin);
            self.motion[2] += f64::from(forward * cos + strafe * sin);
        }
    }

    fn slipperiness_below(&self, world: &World) -> f32 {
        let id = state(
            world,
            floor(self.position[0]),
            floor(lo(self.bounds)[1]) - 1,
            floor(self.position[2]),
        )
        .block_id();
        slipperiness(id)
    }

    fn offset_in_liquid(&mut self, world: &World, x: f64, y: f64, z: f64) -> bool {
        let moved = offset(self.bounds, x, y, z);
        colliding_boxes(world, moved, &mut self.boxes);
        self.boxes.is_empty() && !any_in(world, moved, |id| block(id).liquid())
    }

    /// `EntityLivingBase.moveEntityWithHeading` for a walking player.
    fn move_with_heading(&mut self, world: &World, strafe: f32, forward: f32) {
        if self.in_water && !self.flying {
            let start = self.position[1];
            let mut friction = 0.8f32;
            let mut acceleration = 0.02f32;
            let mut strider = self.depth_strider.min(3) as f32;
            if !self.on_ground {
                strider *= 0.5;
            }
            if strider > 0.0 {
                friction += (0.546_000_06 - friction) * strider / 3.0;
                acceleration += (self.speed.value() as f32 - acceleration) * strider / 3.0;
            }
            self.move_flying(strafe, forward, acceleration);
            let [x, y, z] = self.motion;
            self.move_entity(world, x, y, z);
            self.motion[0] *= f64::from(friction);
            self.motion[1] *= 0.800000011920929;
            self.motion[2] *= f64::from(friction);
            self.motion[1] -= 0.02;
            let lift = self.motion[1] + 0.6000000238418579 - self.position[1] + start;
            if self.collided_horizontally
                && self.offset_in_liquid(world, self.motion[0], lift, self.motion[2])
            {
                self.motion[1] = 0.30000001192092896;
            }
            return;
        }
        if !self.flying && self.in_lava(world) {
            let start = self.position[1];
            self.move_flying(strafe, forward, 0.02);
            let [x, y, z] = self.motion;
            self.move_entity(world, x, y, z);
            for motion in &mut self.motion {
                *motion *= 0.5;
            }
            self.motion[1] -= 0.02;
            let lift = self.motion[1] + 0.6000000238418579 - self.position[1] + start;
            if self.collided_horizontally
                && self.offset_in_liquid(world, self.motion[0], lift, self.motion[2])
            {
                self.motion[1] = 0.30000001192092896;
            }
            return;
        }
        let mut friction = 0.91f32;
        if self.on_ground {
            friction = self.slipperiness_below(world) * 0.91;
        }
        let factor = 0.162_771_36f32 / (friction * friction * friction);
        let acceleration = if self.on_ground {
            self.speed.value() as f32 * factor
        } else {
            self.jump_movement_factor
        };
        self.move_flying(strafe, forward, acceleration);
        friction = 0.91;
        if self.on_ground {
            friction = self.slipperiness_below(world) * 0.91;
        }
        let ladder = self.on_ladder(world);
        if ladder {
            let limit = f64::from(0.15f32);
            self.motion[0] = self.motion[0].clamp(-limit, limit);
            self.motion[2] = self.motion[2].clamp(-limit, limit);
            if self.motion[1] < -0.15 {
                self.motion[1] = -0.15;
            }
            if self.sneaking && self.motion[1] < 0.0 {
                self.motion[1] = 0.0;
            }
        }
        let [x, y, z] = self.motion;
        self.move_entity(world, x, y, z);
        if self.collided_horizontally && self.on_ladder(world) {
            self.motion[1] = 0.2;
        }
        let loaded = world
            .column(
                (self.position[0] as i32) >> 4,
                (self.position[2] as i32) >> 4,
            )
            .is_some();
        if loaded {
            self.motion[1] -= 0.08;
        } else {
            self.motion[1] = if self.position[1] > 0.0 { -0.1 } else { 0.0 };
        }
        self.motion[1] *= 0.9800000190734863;
        self.motion[0] *= f64::from(friction);
        self.motion[2] *= f64::from(friction);
    }

    fn clip_all(&self, moving: Aabb, axis: Axis, mut value: f64) -> f64 {
        for &obstacle in &self.boxes {
            value = clip_offset(obstacle, moving, axis, value);
        }
        value
    }

    fn edge_clear(&mut self, world: &World, x: f64, z: f64) -> bool {
        colliding_boxes(world, offset(self.bounds, x, -1.0, z), &mut self.boxes);
        self.boxes.is_empty()
    }

    /// `Entity.moveEntity` without entity collisions.
    pub fn move_entity(&mut self, world: &World, mut x: f64, mut y: f64, mut z: f64) {
        if self.in_web {
            self.in_web = false;
            x *= 0.25;
            y *= 0.05000000074505806;
            z *= 0.25;
            self.motion = [0.0; 3];
        }
        let (mut d3, d4, mut d5) = (x, y, z);
        let edge = self.on_ground && self.sneaking;
        if edge {
            let step = |value: &mut f64| {
                if *value < 0.05 && *value >= -0.05 {
                    *value = 0.0;
                } else if *value > 0.0 {
                    *value -= 0.05;
                } else {
                    *value += 0.05;
                }
            };
            while x != 0.0 && self.edge_clear(world, x, 0.0) {
                step(&mut x);
                d3 = x;
            }
            while z != 0.0 && self.edge_clear(world, 0.0, z) {
                step(&mut z);
                d5 = z;
            }
            while x != 0.0 && z != 0.0 && self.edge_clear(world, x, z) {
                step(&mut x);
                d3 = x;
                step(&mut z);
                d5 = z;
            }
        }
        colliding_boxes(world, add_coord(self.bounds, x, y, z), &mut self.boxes);
        let original = self.bounds;
        y = self.clip_all(self.bounds, Axis::Y, y);
        self.bounds = offset(self.bounds, 0.0, y, 0.0);
        let may_step = self.on_ground || (d4 != y && d4 < 0.0);
        x = self.clip_all(self.bounds, Axis::X, x);
        self.bounds = offset(self.bounds, x, 0.0, 0.0);
        z = self.clip_all(self.bounds, Axis::Z, z);
        self.bounds = offset(self.bounds, 0.0, 0.0, z);
        if may_step && (d3 != x || d5 != z) {
            let (d11, d7, d8) = (x, y, z);
            let unstepped = self.bounds;
            self.bounds = original;
            y = f64::from(STEP_HEIGHT);
            colliding_boxes(world, add_coord(self.bounds, d3, y, d5), &mut self.boxes);
            let mut raised = self.bounds;
            let swept = add_coord(raised, d3, 0.0, d5);
            let d9 = self.clip_all(swept, Axis::Y, y);
            raised = offset(raised, 0.0, d9, 0.0);
            let d15 = self.clip_all(raised, Axis::X, d3);
            raised = offset(raised, d15, 0.0, 0.0);
            let d16 = self.clip_all(raised, Axis::Z, d5);
            raised = offset(raised, 0.0, 0.0, d16);
            let mut lifted = self.bounds;
            let d17 = self.clip_all(lifted, Axis::Y, y);
            lifted = offset(lifted, 0.0, d17, 0.0);
            let d18 = self.clip_all(lifted, Axis::X, d3);
            lifted = offset(lifted, d18, 0.0, 0.0);
            let d19 = self.clip_all(lifted, Axis::Z, d5);
            lifted = offset(lifted, 0.0, 0.0, d19);
            if d15 * d15 + d16 * d16 > d18 * d18 + d19 * d19 {
                x = d15;
                z = d16;
                y = -d9;
                self.bounds = raised;
            } else {
                x = d18;
                z = d19;
                y = -d17;
                self.bounds = lifted;
            }
            y = self.clip_all(self.bounds, Axis::Y, y);
            self.bounds = offset(self.bounds, 0.0, y, 0.0);
            if d11 * d11 + d8 * d8 >= x * x + z * z {
                x = d11;
                y = d7;
                z = d8;
                self.bounds = unstepped;
            }
        }
        let (l, h) = (lo(self.bounds), hi(self.bounds));
        self.position = [(l[0] + h[0]) / 2.0, l[1], (l[2] + h[2]) / 2.0];
        self.collided_horizontally = d3 != x || d5 != z;
        self.collided_vertically = d4 != y;
        self.on_ground = self.collided_vertically && d4 < 0.0;
        let (bx, by, bz) = (
            floor(self.position[0]),
            floor(self.position[1] - 0.20000000298023224),
            floor(self.position[2]),
        );
        let mut below = state(world, bx, by, bz).block_id();
        if below == 0 {
            let under = state(world, bx, by - 1, bz).block_id();
            if is_fence(under) || under == 139 || is_fence_gate(under) {
                below = under;
            }
        }
        if d3 != x {
            self.motion[0] = 0.0;
        }
        if d5 != z {
            self.motion[2] = 0.0;
        }
        if d4 != y {
            // Block.onLanded; BlockSlime bounces unless sneaking.
            if below == 165 && !self.sneaking {
                if self.motion[1] < 0.0 {
                    self.motion[1] = -self.motion[1];
                }
            } else {
                self.motion[1] = 0.0;
            }
        }
        if !edge && self.on_ground && below == 165 && self.motion[1].abs() < 0.1 && !self.sneaking {
            let scale = 0.4 + self.motion[1].abs() * 0.2;
            self.motion[0] *= scale;
            self.motion[2] *= scale;
        }
        self.block_collisions(world);
    }

    /// `Entity.doBlockCollisions`: cobwebs and soul sand, once per cell.
    fn block_collisions(&mut self, world: &World) {
        let (l, h) = (lo(self.bounds), hi(self.bounds));
        let min = [
            floor(l[0] + 0.001),
            floor(l[1] + 0.001),
            floor(l[2] + 0.001),
        ];
        let max = [
            floor(h[0] - 0.001),
            floor(h[1] - 0.001),
            floor(h[2] - 0.001),
        ];
        if !area_loaded(world, min, max) {
            return;
        }
        for x in min[0]..=max[0] {
            for y in min[1]..=max[1] {
                for z in min[2]..=max[2] {
                    match state(world, x, y, z).block_id() {
                        30 => self.in_web = true,
                        88 => {
                            self.motion[0] *= 0.4;
                            self.motion[2] *= 0.4;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "player_tests.rs"]
mod tests;
