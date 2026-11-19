//! Bounded receive/decode gate against the separately managed vanilla server.
use horizon_protocol_47::{
    Connection, login,
    play::{self, Movement, PositionLook},
};
use horizon_world_189::{ColumnUpdate, World, decode_bulk, decode_chunk};
use std::{
    collections::BTreeMap,
    error::Error,
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let host = args.next().ok_or("Expected HOST [PORT]")?;
    let port = args.next().map_or(Ok(25565u16), |port| port.parse())?;
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    let address = (host.as_str(), port)
        .to_socket_addrs()?
        .next()
        .ok_or("No server addresses")?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.set_nodelay(true)?;
    let mut connection = Connection::new(stream);
    let profile = login::connect(&mut connection, &host, port, "NativeChunkGate", |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "This probe requires the local offline-mode reference server",
        ))
    })?;
    println!("LOGIN_PASS username={}", profile.username);
    let mut world = World::default();
    let mut pose = PositionLook::default();
    let mut sky_light = None;
    let mut block_counts = BTreeMap::<u16, usize>::new();
    let mut correction_count = 0;
    let mut chunk_packets = 0;
    let deadline = Instant::now() + Duration::from_secs(15);
    while world.column_count() < 4 || correction_count == 0 {
        if Instant::now() >= deadline {
            return Err("World probe timed out".into());
        }
        let body = connection.receive()?;
        let updates = match play::decode(&body)? {
            play::Packet::Join(join) => {
                sky_light = Some(join.dimension == 0);
                println!(
                    "JOIN_GAME dimension={} entity_id={}",
                    join.dimension, join.entity_id
                );
                continue;
            }
            play::Packet::KeepAlive(id) => {
                connection.send(&play::keep_alive(id))?;
                continue;
            }
            play::Packet::Compression(threshold) => {
                connection.set_compression(threshold);
                continue;
            }
            play::Packet::Disconnect(reason) => {
                return Err(format!("Server disconnected: {reason}").into());
            }
            play::Packet::Position { location, relative } => {
                for axis in 0..3 {
                    pose.position[axis] = if relative & (1 << axis) != 0 {
                        pose.position[axis] + location.position[axis]
                    } else {
                        location.position[axis]
                    };
                }
                pose.yaw = if relative & 8 != 0 {
                    pose.yaw + location.yaw
                } else {
                    location.yaw
                };
                pose.pitch = if relative & 16 != 0 {
                    pose.pitch + location.pitch
                } else {
                    location.pitch
                };
                connection.send(
                    &Movement::PositionLook {
                        location: pose,
                        on_ground: false,
                    }
                    .encode()?,
                )?;
                correction_count += 1;
                println!(
                    "POSITION_ACK x={} y={} z={}",
                    pose.position[0], pose.position[1], pose.position[2]
                );
                continue;
            }
            play::Packet::Other { id: 0x21, .. } => vec![decode_chunk(
                &body,
                sky_light.ok_or("Chunk arrived before Join Game")?,
            )?],
            play::Packet::Other { id: 0x26, .. } => decode_bulk(&body)?,
            _ => continue,
        };
        chunk_packets += 1;
        for update in updates {
            if let ColumnUpdate::Data { data, .. } = &update {
                for section in data.sections.iter().flatten() {
                    for &state in section.states.iter() {
                        *block_counts.entry(state >> 4).or_default() += 1;
                    }
                }
            }
            world.apply(update);
        }
    }
    println!(
        "VANILLA_WORLD_DECODE_PASS columns={} chunk_packets={} position_acks={} block_ids={block_counts:?}",
        world.column_count(),
        chunk_packets,
        correction_count
    );
    Ok(())
}
