//! Read-only Protocol 47 server-list status and ping.
use horizon_protocol_47::{
    Connection,
    packet::{self, NextState},
};
use std::{
    error::Error,
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let host = args.next().ok_or("usage: status HOST [PORT]")?;
    let port: u16 = args.next().map_or(Ok(25565), |port| port.parse())?;
    if args.next().is_some() {
        return Err("usage: status HOST [PORT]".into());
    }
    let addresses: Vec<_> = (host.as_str(), port).to_socket_addrs()?.collect();
    let mut connected = None;
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, Duration::from_secs(5)) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(error) => last_error = Some(error),
        }
    }
    let stream = connected
        .ok_or_else(|| last_error.unwrap_or_else(|| std::io::Error::other("No addresses")))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.set_nodelay(true)?;
    let mut connection = Connection::new(stream);
    connection.send(&packet::handshake(&host, port, NextState::Status)?)?;
    connection.send(packet::STATUS_REQUEST)?;
    println!("{}", packet::status_response(&connection.receive()?)?);
    let started = Instant::now();
    let nonce = 0x4831_3839_0000_002f;
    connection.send(&packet::ping(nonce))?;
    if packet::pong(&connection.receive()?)? != nonce {
        return Err("Pong payload mismatch".into());
    }
    println!(
        "Protocol 47 status/pong passed, RTT {:?}",
        started.elapsed()
    );
    Ok(())
}
