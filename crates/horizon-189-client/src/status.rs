//! Live server status for the launcher, as vanilla's server list gets it
//! (`bnm.a`, `ServerPinger.ping`): a handshake with next state 1, a status
//! request, then a ping whose round trip is the latency. Queries run on
//! their own threads and are repeated while the launcher is open.
use horizon_protocol_47::{
    Connection,
    packet::{NextState, STATUS_REQUEST, handshake, ping, pong, status_response},
};
use std::{
    collections::HashMap,
    io,
    net::{TcpStream, ToSocketAddrs},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// What a server reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub online: u32,
    pub max: u32,
    /// The message of the day as plain text, formatting codes removed.
    pub motd: String,
    pub ping_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Pinging,
    Online(Info),
    Offline,
}

/// Ask `host:port` for its status.
pub fn query(host: &str, port: u16, timeout: Duration) -> io::Result<Info> {
    let address = (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::other("no address"))?;
    let stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_nodelay(true)?;
    let mut connection = Connection::new(stream);
    connection.send(&handshake(host, port, NextState::Status)?)?;
    connection.send(STATUS_REQUEST)?;
    let json = status_response(&connection.receive()?)?;
    let (online, max, motd) = parse(&json).ok_or_else(|| io::Error::other("unreadable status"))?;
    let sent = Instant::now();
    let payload = 0x4859_5a4e;
    connection.send(&ping(payload))?;
    if pong(&connection.receive()?)? != payload {
        return Err(io::Error::other("wrong pong"));
    }
    let ping_ms = sent.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    Ok(Info {
        online,
        max,
        motd,
        ping_ms,
    })
}

/// Players online, the maximum, and the plain message of the day.
fn parse(json: &str) -> Option<(u32, u32, String)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let players = value.get("players")?;
    let count = |key| {
        players
            .get(key)?
            .as_u64()
            .map(|n| n.min(u64::from(u32::MAX)) as u32)
    };
    let motd = value.get("description").map(plain).unwrap_or_default();
    Some((count("online")?, count("max")?, motd))
}

/// A chat component (a string, or an object with `text` and `extra`) as
/// one line of plain text without `§` codes.
fn plain(component: &serde_json::Value) -> String {
    let mut out = String::new();
    fn walk(component: &serde_json::Value, out: &mut String) {
        match component {
            serde_json::Value::String(text) => out.push_str(text),
            serde_json::Value::Array(parts) => parts.iter().for_each(|part| walk(part, out)),
            serde_json::Value::Object(map) => {
                if let Some(text) = map.get("text") {
                    walk(text, out);
                }
                if let Some(extra) = map.get("extra") {
                    walk(extra, out);
                }
            }
            _ => {}
        }
    }
    walk(component, &mut out);
    let mut text = String::new();
    let mut chars = out.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else if c == '\n' {
            text.push(' ');
        } else {
            text.push(c);
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How long an answer stands before it is asked again.
const REFRESH: Duration = Duration::from_secs(20);
const TIMEOUT: Duration = Duration::from_secs(5);

/// Status for each server address, refreshed in the background.
#[derive(Default)]
pub struct Pinger {
    states: Arc<Mutex<HashMap<String, (State, Instant)>>>,
}

impl Pinger {
    /// The latest state for `host:port`, starting a query when there is none
    /// or the last answer is stale. A stale answer still shows meanwhile.
    pub fn get(&self, host: &str, port: u16) -> State {
        let key = format!("{host}:{port}");
        let Ok(mut states) = self.states.lock() else {
            return State::Offline;
        };
        let entry = states.get(&key).cloned();
        let due = match &entry {
            None => true,
            Some((State::Pinging, _)) => false,
            Some((_, at)) => at.elapsed() > REFRESH,
        };
        if due {
            // Keep showing the last answer while asking again; the fresh
            // time stops a second query starting meanwhile.
            let shown = entry.map_or(State::Pinging, |(state, _)| state);
            states.insert(key.clone(), (shown, Instant::now()));
            let shared = self.states.clone();
            let host = host.to_owned();
            let key = key.clone();
            let _ = std::thread::Builder::new()
                .name("Server status".into())
                .spawn(move || {
                    let state = match query(&host, port, TIMEOUT) {
                        Ok(info) => State::Online(info),
                        Err(_) => State::Offline,
                    };
                    if let Ok(mut states) = shared.lock() {
                        states.insert(key, (state, Instant::now()));
                    }
                });
        }
        states
            .get(&key)
            .map_or(State::Pinging, |(state, _)| state.clone())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn descriptions_become_plain_text() {
        assert_eq!(
            plain(&serde_json::json!(
                "§aHypixel Network §c[1.8-1.21]\n§6SKYBLOCK"
            )),
            "Hypixel Network [1.8-1.21] SKYBLOCK"
        );
        assert_eq!(
            plain(&serde_json::json!({"text": "A ", "extra": [{"text": "§lB"}, "C"]})),
            "A BC"
        );
        assert_eq!(
            parse(r#"{"players":{"online":41203,"max":200000},"description":"hi"}"#),
            Some((41203, 200000, "hi".into()))
        );
        assert_eq!(parse(r#"{"description":"no players"}"#), None);
    }

    #[test]
    fn a_status_query_reads_players_and_times_the_ping() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut connection = Connection::new(stream);
            let hello = connection.receive().unwrap();
            // Handshake: packet 0, protocol 47, then next state 1 at the end.
            assert_eq!(&hello[..2], &[0, 47]);
            assert_eq!(*hello.last().unwrap(), 1);
            assert_eq!(connection.receive().unwrap(), STATUS_REQUEST);
            let json = r#"{"version":{"name":"1.8.9","protocol":47},"players":{"online":7,"max":20},"description":{"text":"§bLocal"}}"#;
            let mut body = vec![0];
            horizon_protocol_47::write_string(&mut body, json, 32767).unwrap();
            connection.send(&body).unwrap();
            let echo = connection.receive().unwrap();
            connection.send(&echo).unwrap();
        });
        let info = query("127.0.0.1", port, Duration::from_secs(2)).unwrap();
        server.join().unwrap();
        assert_eq!(
            (info.online, info.max, info.motd.as_str()),
            (7, 20, "Local")
        );
        assert!(info.ping_ms < 1000);
        // Nothing listening: offline, not a hang.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        assert!(query("127.0.0.1", port, Duration::from_millis(500)).is_err());
    }
}
