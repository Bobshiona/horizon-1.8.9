//! A seeded crowd of fighting players for the lobby benchmark: each one is
//! added to the tab list, spawned holding a diamond sword, then every tick
//! strafes round its own spot, turns its head, swings, and now and then
//! takes a hit. The same seed always gives the same packets.
use horizon_protocol_47::{write_string, write_varint};

/// Clientbound packet bodies, as Protocol 47 lays them out.
pub mod packet {
    use super::*;

    fn uuid(out: &mut Vec<u8>, uuid: u128) {
        out.extend_from_slice(&uuid.to_be_bytes());
    }

    /// 0x38 Player List Item, action 0 (add), no skin properties.
    pub fn tab_add(players: &[(u128, String)]) -> Vec<u8> {
        let mut out = vec![0x38];
        write_varint(&mut out, 0);
        write_varint(&mut out, players.len() as i32);
        for (id, name) in players {
            uuid(&mut out, *id);
            let _ = write_string(&mut out, name, 16);
            write_varint(&mut out, 0); // properties
            write_varint(&mut out, 0); // survival
            write_varint(&mut out, 25); // ping
            out.push(0); // no display name
        }
        out
    }

    /// 0x0C Spawn Player at fixed-point `position` (1/32 block).
    pub fn spawn(id: i32, player: u128, position: [i32; 3], yaw: i8, item: i16) -> Vec<u8> {
        let mut out = vec![0x0c];
        write_varint(&mut out, id);
        uuid(&mut out, player);
        for v in position {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.push(yaw as u8);
        out.push(0);
        out.extend_from_slice(&item.to_be_bytes());
        out.push(0x7f); // no metadata
        out
    }

    /// 0x04 Entity Equipment: `item` in `slot` (0 is the hand).
    pub fn equipment(id: i32, slot: i16, item: i16) -> Vec<u8> {
        let mut out = vec![0x04];
        write_varint(&mut out, id);
        out.extend_from_slice(&slot.to_be_bytes());
        out.extend_from_slice(&item.to_be_bytes());
        out.push(1);
        out.extend_from_slice(&0i16.to_be_bytes());
        out.push(0); // no NBT
        out
    }

    /// 0x17 Entity Look And Relative Move.
    pub fn move_look(id: i32, delta: [i8; 3], yaw: i8, pitch: i8) -> Vec<u8> {
        let mut out = vec![0x17];
        write_varint(&mut out, id);
        out.extend(delta.map(|d| d as u8));
        out.push(yaw as u8);
        out.push(pitch as u8);
        out.push(1);
        out
    }

    /// 0x19 Entity Head Look.
    pub fn head(id: i32, yaw: i8) -> Vec<u8> {
        let mut out = vec![0x19];
        write_varint(&mut out, id);
        out.push(yaw as u8);
        out
    }

    /// 0x0B Animation 0: swing the arm.
    pub fn swing(id: i32) -> Vec<u8> {
        let mut out = vec![0x0b];
        write_varint(&mut out, id);
        out.push(0);
        out
    }

    /// 0x1A Entity Status 2: hurt.
    pub fn hurt(id: i32) -> Vec<u8> {
        let mut out = vec![0x1a];
        out.extend_from_slice(&id.to_be_bytes());
        out.push(2);
        out
    }

    /// 0x02 Chat, as a system message.
    pub fn chat(text: &str) -> Vec<u8> {
        let mut out = vec![0x02];
        let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
        let _ = write_string(&mut out, &format!("{{\"text\":\"{escaped}\"}}"), 32767);
        out.push(1);
        out
    }

    /// 0x13 Destroy Entities.
    pub fn destroy(ids: &[i32]) -> Vec<u8> {
        let mut out = vec![0x13];
        write_varint(&mut out, ids.len() as i32);
        for &id in ids {
            write_varint(&mut out, id);
        }
        out
    }

    /// 0x00 Keep Alive.
    pub fn keep_alive(id: i32) -> Vec<u8> {
        let mut out = vec![0x00];
        write_varint(&mut out, id);
        out
    }
}

/// xorshift32: small, fast and the same everywhere.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f64 {
        f64::from(self.next() & 0xFFFF) / 65535.0
    }
}

struct Fighter {
    id: i32,
    anchor: [f64; 2],
    /// Current position in fixed point, as the client tracks it.
    fixed: [i32; 3],
    phase: f64,
    speed: f64,
    radius: f64,
    swing_every: u32,
}

/// Entity ids start well above any a real server would use this early.
const FIRST_ID: i32 = 900_000;
const DIAMOND_SWORD: i16 = 276;

pub struct Crowd {
    fighters: Vec<Fighter>,
    tick: u32,
}

fn byte_angle(degrees: f64) -> i8 {
    ((degrees.rem_euclid(360.0) / 360.0 * 256.0).round() as i32 & 0xFF) as u8 as i8
}

impl Crowd {
    /// `count` players around `centre` (feet position), from `seed`.
    #[cfg(test)]
    pub fn new(count: usize, centre: [f64; 3], seed: u32) -> (Self, Vec<Vec<u8>>) {
        Self::with_partner(count, centre, None, seed)
    }

    /// As `new`, plus with `facing` (the player's yaw) a sparring partner
    /// standing still 2.2 blocks straight ahead, inside attack reach.
    pub fn with_partner(
        count: usize,
        centre: [f64; 3],
        facing: Option<f32>,
        seed: u32,
    ) -> (Self, Vec<Vec<u8>>) {
        let mut rng = Rng(seed.max(1));
        let mut fighters = Vec::with_capacity(count);
        let mut names = Vec::with_capacity(count);
        for i in 0..count {
            // Rings of players 3 to 15 blocks out.
            let angle = rng.unit() * std::f64::consts::TAU;
            let distance = 3.0 + rng.unit() * 12.0;
            let anchor = [
                centre[0] + angle.cos() * distance,
                centre[2] + angle.sin() * distance,
            ];
            let fixed = [
                (anchor[0] * 32.0).round() as i32,
                (centre[1] * 32.0).round() as i32,
                (anchor[1] * 32.0).round() as i32,
            ];
            fighters.push(Fighter {
                id: FIRST_ID + i as i32,
                anchor,
                fixed,
                phase: rng.unit() * std::f64::consts::TAU,
                speed: 0.08 + rng.unit() * 0.12,
                radius: 0.6 + rng.unit() * 1.4,
                swing_every: 3 + rng.next() % 6,
            });
            let uuid = 0xB0B0_0000_0000_4000_8000_0000_0000_0000u128 | i as u128;
            names.push((uuid, format!("Bench{i:02}")));
        }
        if let Some(yaw) = facing {
            // The look vector for a yaw is (-sin, cos) on x and z.
            let yaw = f64::from(yaw).to_radians();
            let at = [centre[0] - yaw.sin() * 2.2, centre[2] + yaw.cos() * 2.2];
            fighters.push(Fighter {
                id: FIRST_ID + count as i32,
                anchor: at,
                fixed: [
                    (at[0] * 32.0).round() as i32,
                    (centre[1] * 32.0).round() as i32,
                    (at[1] * 32.0).round() as i32,
                ],
                phase: 0.0,
                speed: 0.0,
                radius: 0.0,
                swing_every: 5,
            });
            let uuid = 0xB0B0_0000_0000_4000_8000_0000_0000_FFFFu128;
            names.push((uuid, "Sparring".to_owned()));
        }
        let mut packets = vec![packet::tab_add(&names)];
        for (fighter, (uuid, _)) in fighters.iter().zip(&names) {
            packets.push(packet::spawn(
                fighter.id,
                *uuid,
                fighter.fixed,
                0,
                DIAMOND_SWORD,
            ));
            packets.push(packet::equipment(fighter.id, 0, DIAMOND_SWORD));
        }
        (Self { fighters, tick: 0 }, packets)
    }

    /// Remove everyone (the flyover leaves the lobby).
    pub fn leave(&self) -> Vec<u8> {
        let ids: Vec<i32> = self.fighters.iter().map(|f| f.id).collect();
        packet::destroy(&ids)
    }

    /// One server tick of movement, looks, swings and hits.
    pub fn tick(&mut self) -> Vec<Vec<u8>> {
        self.tick += 1;
        let tick = self.tick;
        let mut packets = Vec::with_capacity(self.fighters.len() * 3);
        for (index, f) in self.fighters.iter_mut().enumerate() {
            let t = f.phase + f64::from(tick) * f.speed;
            let target = [
                f.anchor[0] + t.cos() * f.radius,
                f.anchor[1] + t.sin() * f.radius,
            ];
            let next = [
                (target[0] * 32.0).round() as i32,
                f.fixed[1],
                (target[1] * 32.0).round() as i32,
            ];
            let delta = [
                (next[0] - f.fixed[0]).clamp(-128, 127),
                0,
                (next[2] - f.fixed[2]).clamp(-128, 127),
            ];
            f.fixed = [f.fixed[0] + delta[0], f.fixed[1], f.fixed[2] + delta[2]];
            // Face along the strafe, as a player circling an opponent does.
            let yaw = byte_angle((t + std::f64::consts::FRAC_PI_2).to_degrees() + 90.0);
            packets.push(packet::move_look(f.id, delta.map(|d| d as i8), yaw, 0));
            packets.push(packet::head(f.id, yaw));
            if (tick + index as u32).is_multiple_of(f.swing_every) {
                packets.push(packet::swing(f.id));
            }
            if (tick + index as u32 * 7).is_multiple_of(23) {
                packets.push(packet::hurt(f.id));
            }
        }
        packets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol_47::{entities, player_list};

    fn split(body: &[u8]) -> (i32, &[u8]) {
        let mut input = body;
        let id = horizon_protocol_47::read_varint(&mut input).unwrap();
        (id, input)
    }

    #[test]
    fn crowd_packets_decode_with_the_clients_own_decoders() {
        let (mut crowd, spawn) = Crowd::new(80, [0.5, 64.0, 0.5], 7);
        let (id, fields) = split(&spawn[0]);
        assert_eq!(id, 0x38);
        let tab = player_list::decode(fields).unwrap();
        assert_eq!(tab.len(), 80);
        let mut spawned = 0;
        for body in &spawn[1..] {
            let (id, fields) = split(body);
            match entities::decode(id, fields).unwrap() {
                Some(entities::EntityPacket::SpawnPlayer { held_item, .. }) => {
                    assert_eq!(held_item, DIAMOND_SWORD);
                    spawned += 1;
                }
                Some(entities::EntityPacket::Equipment { .. }) => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(spawned, 80);
        for _ in 0..40 {
            for body in crowd.tick() {
                let (id, fields) = split(&body);
                assert!(entities::decode(id, fields).unwrap().is_some(), "{id:#x}");
            }
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_session() {
        let run = |seed| {
            let (mut crowd, mut all) = Crowd::new(20, [0.0, 70.0, 0.0], seed);
            for _ in 0..100 {
                all.extend(crowd.tick());
            }
            all
        };
        assert_eq!(run(3), run(3));
        assert_ne!(run(3), run(4));
        // Positions stay near their anchors: moves never run away.
        let (mut crowd, _) = Crowd::new(10, [0.0, 70.0, 0.0], 9);
        for _ in 0..2000 {
            crowd.tick();
        }
        for f in &crowd.fighters {
            let dx = f64::from(f.fixed[0]) / 32.0 - f.anchor[0];
            let dz = f64::from(f.fixed[2]) / 32.0 - f.anchor[1];
            assert!((dx * dx + dz * dz).sqrt() < f.radius + 0.2);
        }
    }

    #[test]
    fn the_sparring_partner_stands_in_reach_straight_ahead() {
        // Facing south (yaw 0) is +z.
        let (crowd, packets) = Crowd::with_partner(2, [0.5, 64.0, 0.5], Some(0.0), 1);
        let partner = crowd.fighters.last().unwrap();
        assert_eq!(partner.fixed, [16, 64 * 32, (2.7f64 * 32.0).round() as i32]);
        assert_eq!(crowd.fighters.len(), 3);
        assert_eq!(packets.len(), 1 + 3 * 2);
        // Facing west (yaw 90) is -x; it never moves.
        let (mut crowd, _) = Crowd::with_partner(0, [0.0, 64.0, 0.0], Some(90.0), 1);
        let start = crowd.fighters[0].fixed;
        assert_eq!(start[0], (-2.2f64 * 32.0).round() as i32);
        for _ in 0..50 {
            crowd.tick();
        }
        assert_eq!(crowd.fighters[0].fixed, start);
    }

    #[test]
    fn chat_markers_and_keep_alives_are_well_formed() {
        let chat = packet::chat("BOBTRACE PHASE lobby");
        let (id, fields) = split(&chat);
        assert_eq!(id, 0x02);
        let mut input = fields;
        let json = horizon_protocol_47::read_string(&mut input, 32767).unwrap();
        assert_eq!(json, r#"{"text":"BOBTRACE PHASE lobby"}"#);
        assert_eq!(input, [1]);
        assert_eq!(packet::keep_alive(300), [0x00, 0xAC, 0x02]);
        let (crowd, _) = Crowd::new(3, [0.0, 64.0, 0.0], 1);
        let leave = crowd.leave();
        let (id, fields) = split(&leave);
        assert_eq!(
            horizon_protocol_47::entities::decode(id, fields).unwrap(),
            Some(horizon_protocol_47::entities::EntityPacket::Destroy(vec![
                FIRST_ID,
                FIRST_ID + 1,
                FIRST_ID + 2
            ]))
        );
    }
}
