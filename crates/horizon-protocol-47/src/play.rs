//! Version-specific primitive play fields. Simulation chooses when to emit
//! movement packets and computes each ground flag.
use crate::{invalid, read_string, read_varint, write_string, write_varint};
use std::io::{self, Cursor, Read};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PositionLook {
    pub position: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Debug, PartialEq)]
pub struct JoinGame {
    pub entity_id: i32,
    pub game_mode: u8,
    pub dimension: i8,
    pub difficulty: u8,
    pub max_players: u8,
    pub level_type: String,
    pub reduced_debug: bool,
}

#[derive(Debug, PartialEq)]
pub struct Respawn {
    pub dimension: i32,
    pub difficulty: u8,
    pub game_mode: u8,
    pub level_type: String,
}

#[derive(Debug, PartialEq)]
pub enum Packet<'a> {
    KeepAlive(i32),
    Join(JoinGame),
    Respawn(Respawn),
    Position {
        location: PositionLook,
        relative: u8,
    },
    Compression(i32),
    Disconnect(String),
    Other {
        id: i32,
        fields: &'a [u8],
    },
}

fn bytes<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub fn decode(body: &[u8]) -> io::Result<Packet<'_>> {
    let mut input = Cursor::new(body);
    let id = read_varint(&mut input)?;
    let packet = match id {
        0 => Packet::KeepAlive(read_varint(&mut input)?),
        1 => Packet::Join(JoinGame {
            entity_id: i32::from_be_bytes(bytes(&mut input)?),
            game_mode: bytes::<1>(&mut input)?[0],
            dimension: bytes::<1>(&mut input)?[0] as i8,
            difficulty: bytes::<1>(&mut input)?[0],
            max_players: bytes::<1>(&mut input)?[0],
            level_type: read_string(&mut input, 16)?,
            reduced_debug: bytes::<1>(&mut input)?[0] != 0,
        }),
        7 => Packet::Respawn(Respawn {
            dimension: i32::from_be_bytes(bytes(&mut input)?),
            difficulty: bytes::<1>(&mut input)?[0],
            game_mode: bytes::<1>(&mut input)?[0],
            level_type: read_string(&mut input, 16)?,
        }),
        8 => {
            let location = PositionLook {
                position: [
                    f64::from_be_bytes(bytes(&mut input)?),
                    f64::from_be_bytes(bytes(&mut input)?),
                    f64::from_be_bytes(bytes(&mut input)?),
                ],
                yaw: f32::from_be_bytes(bytes(&mut input)?),
                pitch: f32::from_be_bytes(bytes(&mut input)?),
            };
            if location.position.iter().any(|value| !value.is_finite())
                || !location.yaw.is_finite()
                || !location.pitch.is_finite()
            {
                return Err(invalid("Non-finite server position or rotation"));
            }
            Packet::Position {
                location,
                relative: bytes::<1>(&mut input)?[0],
            }
        }
        0x40 => Packet::Disconnect(read_string(&mut input, 32767)?),
        0x46 => Packet::Compression(read_varint(&mut input)?),
        _ if (0..=0x49).contains(&id) => {
            return Ok(Packet::Other {
                id,
                fields: &body[input.position() as usize..],
            });
        }
        _ => return Err(invalid("Invalid Protocol 47 play packet ID")),
    };
    if input.position() != body.len() as u64 {
        return Err(invalid("Trailing play packet bytes"));
    }
    Ok(packet)
}

pub fn keep_alive(id: i32) -> Vec<u8> {
    let mut body = vec![0];
    write_varint(&mut body, id);
    body
}

/// Serverbound 0x15 with vanilla defaults: `en_US`, chat full, colours on,
/// every skin part shown. Vanilla clamps view distance to 2..=16 chunks.
/// C15 Client Settings: locale, view distance, chat full with colours,
/// and the shown skin parts (`EnumPlayerModelParts` bits).
pub fn client_settings(view_distance: u8, skin_parts: u8) -> Vec<u8> {
    vec![
        0x15,
        5,
        b'e',
        b'n',
        b'_',
        b'U',
        b'S',
        view_distance.clamp(2, 16),
        0,
        1,
        skin_parts & 0x7f,
    ]
}

pub fn brand() -> io::Result<Vec<u8>> {
    let mut body = vec![0x17];
    write_string(&mut body, "MC|Brand", 20)?;
    write_string(&mut body, "vanilla", 32767)?;
    Ok(body)
}

#[derive(Default)]
pub struct MovementReporter {
    last: PositionLook,
    position_ticks: u8,
}

impl MovementReporter {
    /// Walking-player packet selection, including vanilla's pre-increment
    /// periodic-position check. Call once for each completed simulation tick.
    pub fn report(&mut self, location: PositionLook, on_ground: bool) -> io::Result<Vec<u8>> {
        if location.position.iter().any(|value| !value.is_finite())
            || !location.yaw.is_finite()
            || !location.pitch.is_finite()
        {
            return Err(invalid("Non-finite player report"));
        }
        let dx = location.position[0] - self.last.position[0];
        let dy = location.position[1] - self.last.position[1];
        let dz = location.position[2] - self.last.position[2];
        let position = dx * dx + dy * dy + dz * dz > 9.0e-4 || self.position_ticks >= 20;
        let rotation = location.yaw != self.last.yaw || location.pitch != self.last.pitch;
        let movement = match (position, rotation) {
            (true, true) => Movement::PositionLook {
                location,
                on_ground,
            },
            (true, false) => Movement::Position {
                position: location.position,
                on_ground,
            },
            (false, true) => Movement::Look {
                yaw: location.yaw,
                pitch: location.pitch,
                on_ground,
            },
            (false, false) => Movement::Ground { on_ground },
        };
        let body = movement.encode()?;
        self.position_ticks += 1;
        if position {
            self.last.position = location.position;
            self.position_ticks = 0;
        }
        if rotation {
            self.last.yaw = location.yaw;
            self.last.pitch = location.pitch;
        }
        Ok(body)
    }
}

pub enum Movement {
    Ground {
        on_ground: bool,
    },
    Position {
        position: [f64; 3],
        on_ground: bool,
    },
    Look {
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    },
    PositionLook {
        location: PositionLook,
        on_ground: bool,
    },
}

impl Movement {
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let (id, position, rotation, on_ground) = match *self {
            Self::Ground { on_ground } => (3, None, None, on_ground),
            Self::Position {
                position,
                on_ground,
            } => (4, Some(position), None, on_ground),
            Self::Look {
                yaw,
                pitch,
                on_ground,
            } => (5, None, Some([yaw, pitch]), on_ground),
            Self::PositionLook {
                location,
                on_ground,
            } => (
                6,
                Some(location.position),
                Some([location.yaw, location.pitch]),
                on_ground,
            ),
        };
        let mut body = Vec::with_capacity(34);
        body.push(id);
        if let Some(position) = position {
            for coordinate in position {
                if !coordinate.is_finite() {
                    return Err(invalid("Non-finite movement coordinate"));
                }
                body.extend_from_slice(&coordinate.to_be_bytes());
            }
        }
        if let Some(rotation) = rotation {
            for angle in rotation {
                if !angle.is_finite() {
                    return Err(invalid("Non-finite movement angle"));
                }
                body.extend_from_slice(&angle.to_be_bytes());
            }
        }
        body.push(u8::from(on_ground));
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawn_fields_are_complete_bounded_and_keep_signed_dimension() {
        let mut body = vec![7];
        body.extend((-1i32).to_be_bytes());
        body.extend([2, 3]);
        write_string(&mut body, "default", 16).unwrap();
        assert_eq!(
            decode(&body).unwrap(),
            Packet::Respawn(Respawn {
                dimension: -1,
                difficulty: 2,
                game_mode: 3,
                level_type: "default".into()
            })
        );
        for len in 0..body.len() {
            assert!(decode(&body[..len]).is_err());
        }
        body.push(0);
        assert!(decode(&body).is_err());
    }

    #[test]
    fn stationary_report_counter_matches_preincrement_vanilla_order() {
        let mut reporter = MovementReporter::default();
        let mut pose = PositionLook {
            position: [1.0, 4.0, 1.0],
            yaw: 0.0,
            pitch: 0.0,
        };
        assert_eq!(reporter.report(pose, true).unwrap()[0], 4);
        for _ in 0..20 {
            assert_eq!(reporter.report(pose, true).unwrap(), [3, 1]);
        }
        assert_eq!(reporter.report(pose, true).unwrap()[0], 4);
        pose.yaw = 15.0;
        assert_eq!(reporter.report(pose, true).unwrap()[0], 5);
        pose.position[0] += 0.1;
        pose.pitch = 5.0;
        assert_eq!(reporter.report(pose, false).unwrap()[0], 6);
        assert_eq!(
            brand().unwrap(),
            [
                0x17, 8, b'M', b'C', b'|', b'B', b'r', b'a', b'n', b'd', 7, b'v', b'a', b'n', b'i',
                b'l', b'l', b'a'
            ]
        );
    }

    #[test]
    fn client_settings_clamp_view_distance_like_vanilla() {
        assert_eq!(client_settings(8, 0x7f)[7], 8);
        assert_eq!(client_settings(0, 0x7f)[7], 2);
        assert_eq!(client_settings(40, 0x7f)[7], 16);
        assert_eq!(client_settings(8, 0x7f).len(), 11);
        // The skin parts byte comes last, seven bits.
        assert_eq!(client_settings(8, 0x41)[10], 0x41);
        assert_eq!(client_settings(8, 0xff)[10], 0x7f);
    }

    #[test]
    fn keep_alive_is_varint_and_movement_keeps_float_widths() {
        assert_eq!(keep_alive(300), [0, 0xac, 2]);
        assert_eq!(decode(&[0, 0xac, 2]).unwrap(), Packet::KeepAlive(300));
        assert!(decode(&[0, 0xac, 2, 0]).is_err());
        assert_eq!(
            Movement::Ground { on_ground: true }.encode().unwrap(),
            [3, 1]
        );
        assert_eq!(
            Movement::Position {
                position: [1.0, 2.0, 3.0],
                on_ground: false
            }
            .encode()
            .unwrap()
            .len(),
            26
        );
        let look = Movement::Look {
            yaw: 90.0,
            pitch: -45.0,
            on_ground: true,
        }
        .encode()
        .unwrap();
        assert_eq!(look, [5, 0x42, 0xb4, 0, 0, 0xc2, 0x34, 0, 0, 1]);
        assert_eq!(
            Movement::PositionLook {
                location: PositionLook::default(),
                on_ground: false
            }
            .encode()
            .unwrap()
            .len(),
            34
        );
        assert!(
            Movement::Look {
                yaw: f32::NAN,
                pitch: 0.0,
                on_ground: false
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn server_position_keeps_relative_flags_and_exact_precision() {
        let location = PositionLook {
            position: [30_000_000.125, 64.0, -0.25],
            yaw: 123.5,
            pitch: -10.25,
        };
        let mut body = Movement::PositionLook {
            location,
            on_ground: false,
        }
        .encode()
        .unwrap();
        body[0] = 8;
        *body.last_mut().unwrap() = 0x1f;
        assert_eq!(
            decode(&body).unwrap(),
            Packet::Position {
                location,
                relative: 0x1f
            }
        );
        assert!(decode(&body[..body.len() - 1]).is_err());
    }
}
