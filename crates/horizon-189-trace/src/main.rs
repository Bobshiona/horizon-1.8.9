//! Bob Client's packet-trace tool.
//!
//! `record --listen ADDR --server ADDR --out FILE` sits between the client
//! and an offline-mode server and writes every play packet both ways.
//!
//! `serve --listen ADDR --trace FILE [--until MS] [--crowd N --crowd-at MS
//! [--crowd-until MS] [--sparring] --seed S] [--phase NAME@MS]... [--log FILE]` is that
//! server again: it
//! logs the client in offline, replays the recorded server packets on their
//! recorded timeline, adds the seeded crowd, marks benchmark phases with
//! `BOBTRACE PHASE <name>` chat lines and the end with `BOBTRACE END`, and
//! can log everything the client sends for the packet-parity checks.
//! `check --log FILE [--closed FROM-TO,...]` runs the Watchdog parity rules
//! (`parity`) over a client log, with the ticks a screen was open; it prints
//! `WATCHDOG_PASS` or one `WATCHDOG_FAIL` line per problem and exits 1.
//!
//! Nothing here talks to a real network service; traces and logs stay
//! private (they hold the test world).
#![forbid(unsafe_code)]

mod crowd;
mod parity;
mod trace;

use horizon_protocol_47::{Connection, read_string, read_varint, write_string, write_varint};
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use trace::{Direction, Record};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("record") => parse_record(&args[1..]).and_then(|(l, s, o)| record(&l, &s, o)),
        Some("serve") => parse_serve(&args[1..]).and_then(serve),
        Some("check") => check(&args[1..]),
        _ => Err(io::Error::other(
            "usage: horizon-189-trace record|serve ... (see the source header)",
        )),
    };
    if let Err(error) = result {
        eprintln!("TRACE_ERROR {error}");
        std::process::exit(1);
    }
}

fn value<'a>(args: &'a [String], i: &mut usize) -> io::Result<&'a str> {
    *i += 1;
    args.get(*i)
        .map(String::as_str)
        .ok_or_else(|| io::Error::other("missing value"))
}

fn number<T: std::str::FromStr>(text: &str) -> io::Result<T> {
    text.parse()
        .map_err(|_| io::Error::other(format!("not a number: {text}")))
}

fn parse_record(args: &[String]) -> io::Result<(String, String, PathBuf)> {
    let (mut listen, mut server, mut out) = (None, None, None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" => listen = Some(value(args, &mut i)?.to_owned()),
            "--server" => server = Some(value(args, &mut i)?.to_owned()),
            "--out" => out = Some(PathBuf::from(value(args, &mut i)?)),
            other => return Err(io::Error::other(format!("unknown argument {other}"))),
        }
        i += 1;
    }
    match (listen, server, out) {
        (Some(l), Some(s), Some(o)) => Ok((l, s, o)),
        _ => Err(io::Error::other(
            "record needs --listen, --server and --out",
        )),
    }
}

#[derive(Debug, Default, PartialEq)]
struct Serve {
    listen: String,
    trace: PathBuf,
    until: Option<u32>,
    crowd: usize,
    crowd_at: u32,
    /// When the crowd is removed; never if unset.
    crowd_until: Option<u32>,
    seed: u32,
    /// A still fighter straight ahead of the player, for real attacks.
    sparring: bool,
    phases: Vec<(u32, String)>,
    log: Option<PathBuf>,
}

fn parse_serve(args: &[String]) -> io::Result<Serve> {
    let mut serve = Serve {
        seed: 1,
        ..Serve::default()
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" => serve.listen = value(args, &mut i)?.to_owned(),
            "--trace" => serve.trace = PathBuf::from(value(args, &mut i)?),
            "--until" => serve.until = Some(number(value(args, &mut i)?)?),
            "--crowd" => serve.crowd = number(value(args, &mut i)?)?,
            "--crowd-at" => serve.crowd_at = number(value(args, &mut i)?)?,
            "--crowd-until" => serve.crowd_until = Some(number(value(args, &mut i)?)?),
            "--seed" => serve.seed = number(value(args, &mut i)?)?,
            "--sparring" => serve.sparring = true,
            "--log" => serve.log = Some(PathBuf::from(value(args, &mut i)?)),
            "--phase" => {
                let text = value(args, &mut i)?;
                let (name, at) = text
                    .split_once('@')
                    .ok_or_else(|| io::Error::other("--phase takes NAME@MS"))?;
                serve.phases.push((number(at)?, name.to_owned()));
            }
            other => return Err(io::Error::other(format!("unknown argument {other}"))),
        }
        i += 1;
    }
    if serve.listen.is_empty() || serve.trace.as_os_str().is_empty() {
        return Err(io::Error::other("serve needs --listen and --trace"));
    }
    serve.phases.sort();
    Ok(serve)
}

fn check(args: &[String]) -> io::Result<()> {
    let (mut log, mut windows) = (None, Vec::new());
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--log" => log = Some(PathBuf::from(value(args, &mut i)?)),
            "--closed" => {
                for part in value(args, &mut i)?.split(',') {
                    windows.push(
                        parity::Window::parse(part)
                            .ok_or_else(|| io::Error::other(format!("bad window {part}")))?,
                    );
                }
            }
            other => return Err(io::Error::other(format!("unknown argument {other}"))),
        }
        i += 1;
    }
    let log = log.ok_or_else(|| io::Error::other("check needs --log"))?;
    let records = trace::read(File::open(log)?)?;
    let sent = records
        .iter()
        .filter(|r| r.direction == Direction::ToServer)
        .count();
    let mut counts = std::collections::BTreeMap::new();
    for record in records
        .iter()
        .filter(|r| r.direction == Direction::ToServer)
    {
        let id = horizon_protocol_47::read_varint(&mut &record.body[..]).unwrap_or(-1);
        *counts.entry(id).or_insert(0u32) += 1;
    }
    let counts: Vec<String> = counts
        .iter()
        .map(|(id, n)| format!("{id:#04x}x{n}"))
        .collect();
    println!("WATCHDOG_SENT {}", counts.join(" "));
    let problems = parity::check(&records, &windows);
    if problems.is_empty() {
        println!("WATCHDOG_PASS packets={sent} windows={}", windows.len());
        return Ok(());
    }
    for problem in &problems {
        println!("WATCHDOG_FAIL {problem}");
    }
    std::process::exit(1);
}

/// Write one record and flush it, so a log stays whole however the process
/// ends (the Watchdog script stops the server once the client is done).
fn log_record(file: &Mutex<BufWriter<File>>, record: &Record) {
    if let Ok(mut file) = file.lock() {
        let _ = trace::write_record(&mut *file, record);
        let _ = file.flush();
    }
}

fn millis(since: Instant) -> u32 {
    since.elapsed().as_millis().min(u128::from(u32::MAX)) as u32
}

/// Proxy one session and write its play packets.
fn record(listen: &str, server: &str, out: PathBuf) -> io::Result<()> {
    let listener = TcpListener::bind(listen)?;
    eprintln!("TRACE_RECORD_LISTENING {listen}");
    let (client, _) = listener.accept()?;
    let upstream = TcpStream::connect(server)?;
    client.set_nodelay(true)?;
    upstream.set_nodelay(true)?;
    let mut client = Connection::new(client);
    let mut upstream = Connection::new(upstream);
    // Handshake and login start, then the server's login replies.
    for _ in 0..2 {
        upstream.send(&client.receive()?)?;
    }
    loop {
        let body = upstream.receive()?;
        client.send(&body)?;
        match body.first() {
            Some(3) => {
                let threshold = read_varint(&mut &body[1..])?;
                upstream.set_compression(threshold);
                client.set_compression(threshold);
            }
            Some(2) => break,
            Some(0) => return Err(io::Error::other("the server refused the login")),
            _ => return Err(io::Error::other("record needs an offline-mode server")),
        }
    }
    let started = Instant::now();
    let mut file = BufWriter::new(File::create(&out)?);
    trace::write_header(&mut file)?;
    let file = Arc::new(Mutex::new(file));
    let (mut server_in, mut server_out) = upstream.split()?;
    let (mut client_in, mut client_out) = client.split()?;
    let shared = file.clone();
    let downstream = std::thread::spawn(move || -> io::Result<()> {
        loop {
            let body = server_in.receive()?;
            log_record(
                &shared,
                &Record {
                    direction: Direction::ToClient,
                    ms: millis(started),
                    body: body.clone(),
                },
            );
            client_out.send(&body)?;
        }
    });
    let result = (|| -> io::Result<()> {
        loop {
            let body = client_in.receive()?;
            log_record(
                &file,
                &Record {
                    direction: Direction::ToServer,
                    ms: millis(started),
                    body: body.clone(),
                },
            );
            server_out.send(&body)?;
        }
    })();
    let _ = server_out.shutdown();
    let _ = downstream.join();
    if let Ok(mut file) = file.lock() {
        file.flush()?;
    }
    eprintln!("TRACE_RECORD_DONE {} ms ({result:?})", millis(started));
    Ok(())
}

/// Recorded packets to send again: not the disconnect that ended the
/// recording (0x40), which would end the replay before its marker.
fn replayable(record: &Record) -> bool {
    record.body.first() != Some(&0x40)
}

/// The position and yaw in an absolute 0x08 Player Position And Look.
fn teleport(body: &[u8]) -> Option<([f64; 3], f32)> {
    if body.len() < 1 + 3 * 8 + 2 * 4 + 1 || body[0] != 0x08 {
        return None;
    }
    let flags = body[1 + 32];
    if flags & 0b111 != 0 {
        return None;
    }
    let read = |at: usize| f64::from_be_bytes(body[at..at + 8].try_into().unwrap_or([0; 8]));
    let yaw = f32::from_be_bytes(body[25..29].try_into().unwrap_or([0; 4]));
    Some(([read(1), read(9), read(17)], yaw))
}

/// One thing the server sends, at a time on the trace's timeline.
#[derive(Debug, PartialEq)]
enum Event {
    Recorded(usize),
    CrowdSpawn,
    CrowdTick,
    CrowdLeave,
    Phase(usize),
    End,
}

/// Every event of a session in time order (ties in that order).
fn timeline(serve: &Serve, records: &[Record], end: u32) -> Vec<(u32, Event)> {
    let mut events: Vec<(u32, u8, Event)> = records
        .iter()
        .enumerate()
        .map(|(i, r)| (r.ms, 0, Event::Recorded(i)))
        .collect();
    if serve.crowd > 0 && serve.crowd_at < end {
        events.push((serve.crowd_at, 1, Event::CrowdSpawn));
        let stop = serve.crowd_until.unwrap_or(end).min(end);
        let mut at = serve.crowd_at + 50;
        while at < stop {
            events.push((at, 2, Event::CrowdTick));
            at += 50;
        }
        if stop < end {
            events.push((stop, 2, Event::CrowdLeave));
        }
    }
    for (i, (at, _)) in serve.phases.iter().enumerate() {
        events.push((*at, 3, Event::Phase(i)));
    }
    events.push((end + 500, 4, Event::End));
    events.sort_by_key(|(at, order, _)| (*at, *order));
    events.into_iter().map(|(at, _, e)| (at, e)).collect()
}

fn serve(serve: Serve) -> io::Result<()> {
    let records: Vec<Record> = trace::read(File::open(&serve.trace)?)?
        .into_iter()
        .filter(|r| r.direction == Direction::ToClient)
        .filter(|r| serve.until.is_none_or(|until| r.ms <= until))
        .filter(replayable)
        .collect();
    let end = serve
        .until
        .unwrap_or_else(|| records.last().map_or(0, |r| r.ms));
    let events = timeline(&serve, &records, end);
    let listener = TcpListener::bind(&serve.listen)?;
    eprintln!(
        "TRACE_SERVE_LISTENING {} records={}",
        serve.listen,
        records.len()
    );
    let (stream, _) = listener.accept()?;
    stream.set_nodelay(true)?;
    let mut connection = Connection::new(stream);
    connection.receive()?; // handshake
    let start = connection.receive()?;
    let mut input = &start[1..];
    let name = read_string(&mut input, 16)?;
    let mut compression = vec![3];
    write_varint(&mut compression, 256);
    connection.send(&compression)?;
    connection.set_compression(256);
    let mut success = vec![2];
    write_string(&mut success, "b0b00000-0000-4000-8000-00000000b0b0", 36)?;
    write_string(&mut success, &name, 16)?;
    connection.send(&success)?;
    let (mut reader, mut writer) = connection.split()?;
    let started = Instant::now();
    let log = match &serve.log {
        Some(path) => {
            let mut file = BufWriter::new(File::create(path)?);
            trace::write_header(&mut file)?;
            Some(Arc::new(Mutex::new(file)))
        }
        None => None,
    };
    let client_log = log.clone();
    let drain = std::thread::spawn(move || {
        while let Ok(body) = reader.receive() {
            if let Some(log) = &client_log {
                log_record(
                    log,
                    &Record {
                        direction: Direction::ToServer,
                        ms: millis(started),
                        body,
                    },
                );
            }
        }
    });
    let mut centre = [0.5, 64.0, 0.5];
    let mut facing = 0.0f32;
    let mut crowd: Option<crowd::Crowd> = None;
    let result = (|| -> io::Result<()> {
        for (at, event) in &events {
            let due = started + Duration::from_millis(u64::from(*at));
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            match event {
                Event::Recorded(i) => {
                    let body = &records[*i].body;
                    if let Some((position, yaw)) = teleport(body) {
                        (centre, facing) = (position, yaw);
                    }
                    writer.send(body)?;
                }
                Event::CrowdSpawn => {
                    let partner = serve.sparring.then_some(facing);
                    let (spawned, packets) =
                        crowd::Crowd::with_partner(serve.crowd, centre, partner, serve.seed);
                    for body in packets {
                        writer.send(&body)?;
                    }
                    crowd = Some(spawned);
                }
                Event::CrowdTick => {
                    if let Some(crowd) = &mut crowd {
                        for body in crowd.tick() {
                            writer.send(&body)?;
                        }
                    }
                }
                Event::CrowdLeave => {
                    if let Some(crowd) = crowd.take() {
                        writer.send(&crowd.leave())?;
                    }
                }
                Event::Phase(i) => {
                    let name = &serve.phases[*i].1;
                    writer.send(&crowd::packet::chat(&format!("BOBTRACE PHASE {name}")))?;
                }
                Event::End => {
                    writer.send(&crowd::packet::chat("BOBTRACE END"))?;
                    // Let the client report and leave before closing.
                    for keep in 0..40 {
                        std::thread::sleep(Duration::from_millis(250));
                        writer.send(&crowd::packet::keep_alive(0x7000 + keep))?;
                    }
                }
            }
        }
        Ok(())
    })();
    let _ = writer.shutdown();
    let _ = drain.join();
    if let Some(log) = log
        && let Ok(mut file) = log.lock()
    {
        file.flush()?;
    }
    eprintln!("TRACE_SERVE_DONE {} ms ({result:?})", millis(started));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn serve_options_parse_and_sort_phases() {
        let serve = parse_serve(&args(
            "--listen 127.0.0.1:25570 --trace t.bin --crowd 80 --crowd-at 2000 \
             --phase flyover@15000 --phase lobby@3000 --until 40000 --log c.bin",
        ))
        .unwrap();
        assert_eq!(serve.crowd, 80);
        assert_eq!(serve.until, Some(40000));
        assert_eq!(
            serve.phases,
            [(3000, "lobby".into()), (15000, "flyover".into())]
        );
        assert!(parse_serve(&args("--trace t.bin")).is_err());
        assert!(parse_serve(&args("--listen x --trace t --phase nope")).is_err());
    }

    #[test]
    fn the_timeline_interleaves_trace_crowd_and_markers_in_order() {
        let serve = Serve {
            crowd: 2,
            crowd_at: 100,
            phases: vec![(100, "lobby".into())],
            ..Serve::default()
        };
        let record = |ms| Record {
            direction: Direction::ToClient,
            ms,
            body: vec![0x00, 1],
        };
        let records = [record(0), record(100), record(260)];
        let events = timeline(&serve, &records, 260);
        let times: Vec<u32> = events.iter().map(|(t, _)| *t).collect();
        assert!(times.windows(2).all(|w| w[0] <= w[1]));
        // At 100 ms: the recorded packet, then the crowd, then the marker.
        let at_100: Vec<&Event> = events
            .iter()
            .filter(|(t, _)| *t == 100)
            .map(|(_, e)| e)
            .collect();
        assert_eq!(
            at_100,
            [&Event::Recorded(1), &Event::CrowdSpawn, &Event::Phase(0)]
        );
        let ticks = events
            .iter()
            .filter(|(_, e)| *e == Event::CrowdTick)
            .count();
        assert_eq!(ticks, 3); // 150, 200, 250
        assert_eq!(events.last(), Some(&(760, Event::End)));
        // Leaving stops the ticks and removes everyone once.
        let leaving = Serve {
            crowd_until: Some(200),
            ..serve
        };
        let events = timeline(&leaving, &records, 260);
        let ticks = events
            .iter()
            .filter(|(_, e)| *e == Event::CrowdTick)
            .count();
        assert_eq!(ticks, 1); // 150
        assert!(events.contains(&(200, Event::CrowdLeave)));
    }

    #[test]
    fn the_recordings_own_disconnect_is_not_replayed() {
        let record = |body: Vec<u8>| Record {
            direction: Direction::ToClient,
            ms: 0,
            body,
        };
        assert!(!replayable(&record(vec![0x40, 2, b'"', b'"'])));
        assert!(replayable(&record(vec![0x21, 0])));
    }

    #[test]
    fn absolute_teleports_move_the_crowd_centre() {
        let mut body = vec![0x08];
        for v in [10.5f64, 70.0, -3.25] {
            body.extend_from_slice(&v.to_be_bytes());
        }
        body.extend_from_slice(&0f32.to_be_bytes());
        body.extend_from_slice(&0f32.to_be_bytes());
        body.push(0);
        assert_eq!(teleport(&body), Some(([10.5, 70.0, -3.25], 0.0)));
        *body.last_mut().unwrap() = 1; // relative x
        assert_eq!(teleport(&body), None);
        assert_eq!(teleport(&[0x08, 1]), None);
    }
}
