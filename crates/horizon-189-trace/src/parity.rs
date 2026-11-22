//! The Watchdog rules: what a vanilla 1.8.9 client sends, checked against
//! everything Bob Client sent in a scripted session with every module on.
//!
//! Vanilla sends exactly one movement packet a tick (`onUpdateWalkingPlayer`),
//! so the n-th movement packet is tick n; the script's screen windows are
//! given in those ticks.
use crate::trace::{Direction, Record};

/// C03 Player, C04 Position, C05 Look, C06 Position And Look.
fn is_move(id: i32) -> bool {
    (0x03..=0x06).contains(&id)
}

fn has_position(id: i32) -> bool {
    id == 0x04 || id == 0x06
}

/// What a vanilla client can send in the session: keep-alives, use entity,
/// movement, digging, placing, held item, swings, entity actions, window
/// close, click and confirm, abilities, settings, status and the brand.
const ALLOWED: [i32; 18] = [
    0x00, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f, 0x13, 0x15,
    0x16, 0x17,
];

/// A stretch of ticks with a screen open (inventory, Right Shift menu...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub from: u32,
    pub to: u32,
}

impl Window {
    /// `FROM-TO`.
    pub fn parse(text: &str) -> Option<Self> {
        let (from, to) = text.split_once('-')?;
        let window = Self {
            from: from.trim().parse().ok()?,
            to: to.trim().parse().ok()?,
        };
        (window.from < window.to).then_some(window)
    }
}

/// The fields after the id, as varints.
fn varints(body: &[u8], count: usize) -> Option<Vec<i32>> {
    let mut input = body;
    horizon_protocol_47::read_varint(&mut input).ok()?;
    (0..count)
        .map(|_| horizon_protocol_47::read_varint(&mut input).ok())
        .collect()
}

/// Every rule broken, one line each; empty when the session is vanilla.
pub fn check(records: &[Record], windows: &[Window]) -> Vec<String> {
    let sent: Vec<(&Record, i32)> = records
        .iter()
        .filter(|r| r.direction == Direction::ToServer)
        .map(|r| (r, read_id(&r.body)))
        .collect();
    let mut problems = Vec::new();
    // Nothing a vanilla client would not send.
    for (record, id) in &sent {
        if !ALLOWED.contains(id) {
            problems.push(format!(
                "packet {id:#04x} at {} ms is not vanilla here",
                record.ms
            ));
        }
    }
    // One movement packet a tick: 18-22 in every whole second.
    let moves: Vec<&Record> = sent
        .iter()
        .filter(|(_, id)| is_move(*id))
        .map(|(r, _)| *r)
        .collect();
    if let (Some(first), Some(last)) = (moves.first(), moves.last()) {
        let mut start = first.ms;
        while start + 1000 <= last.ms {
            let n = moves
                .iter()
                .filter(|r| r.ms >= start && r.ms < start + 1000)
                .count();
            if !(18..=22).contains(&n) {
                problems.push(format!(
                    "{n} movement packets in the second from {start} ms (vanilla sends 20)"
                ));
            }
            start += 1000;
        }
    }
    // A position at least every 21st tick: `positionUpdateTicks >= 20` is
    // tested before the counter goes up, so a still player sends 20 packets
    // without a position between ones with it.
    let mut without = 0;
    for (tick, record) in moves.iter().enumerate() {
        if has_position(read_id(&record.body)) {
            without = 0;
        } else {
            without += 1;
            if without > 20 {
                problems.push(format!("no position for 21 ticks by tick {tick}"));
                without = 0;
            }
        }
    }
    // Every attack follows its swing: C0A, then C02 (`clickMouse`).
    for (i, (record, id)) in sent.iter().enumerate() {
        if *id == 0x02
            && varints(&record.body, 2).is_some_and(|v| v[1] == 1)
            && (i == 0 || sent[i - 1].1 != 0x0a)
        {
            problems.push(format!("attack at {} ms without a swing first", record.ms));
        }
    }
    // Sneak and sprint start and stop strictly alternate.
    let (mut sneaking, mut sprinting) = (false, false);
    for (record, id) in &sent {
        if *id != 0x0b {
            continue;
        }
        let Some(action) = varints(&record.body, 2).map(|v| v[1]) else {
            problems.push(format!("unreadable entity action at {} ms", record.ms));
            continue;
        };
        let (state, start, name) = match action {
            0 | 1 => (&mut sneaking, action == 0, "sneak"),
            3 | 4 => (&mut sprinting, action == 3, "sprint"),
            _ => continue,
        };
        if *state == start {
            let verb = if start { "start" } else { "stop" };
            problems.push(format!("{name} {verb} twice at {} ms", record.ms));
        }
        *state = start;
    }
    // No sneak packets while a screen is open. Opening one releases every
    // key, which may stop a sneak in the next two ticks.
    for window in windows {
        let at = |tick: u32| moves.get(tick as usize).map(|r| r.ms);
        let (Some(from), Some(to)) = (at(window.from + 2), at(window.to)) else {
            problems.push(format!("the session ended before window {window:?}"));
            continue;
        };
        for (record, id) in &sent {
            if *id == 0x0b
                && (from..to).contains(&record.ms)
                && varints(&record.body, 2).is_some_and(|v| v[1] == 0 || v[1] == 1)
            {
                problems.push(format!(
                    "sneak packet at {} ms with a screen open (ticks {}-{})",
                    record.ms, window.from, window.to
                ));
            }
        }
    }
    // Settings and the brand once, at login.
    for (id, name) in [(0x15, "client settings"), (0x17, "plugin message")] {
        let n = sent.iter().filter(|(_, i)| *i == id).count();
        if n > 1 {
            problems.push(format!("{name} sent {n} times (once at login)"));
        }
    }
    problems
}

fn read_id(body: &[u8]) -> i32 {
    horizon_protocol_47::read_varint(&mut &body[..]).unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(ms: u32, body: Vec<u8>) -> Record {
        Record {
            direction: Direction::ToServer,
            ms,
            body,
        }
    }

    /// Ten seconds of vanilla: a position every other tick, a look between,
    /// login packets, and one swing-then-attack.
    fn vanilla() -> Vec<Record> {
        let mut records = vec![packet(0, vec![0x15, 0]), packet(0, vec![0x17, 0])];
        for tick in 0..200u32 {
            let id = if tick % 2 == 0 { 0x06 } else { 0x05 };
            records.push(packet(tick * 50 + 1, vec![id, 0]));
        }
        records.push(packet(3002, vec![0x0a]));
        records.push(packet(3002, vec![0x02, 9, 1]));
        records.sort_by_key(|r| r.ms);
        records
    }

    fn moves_at(records: &[Record], tick: usize) -> u32 {
        records
            .iter()
            .filter(|r| is_move(r.id()))
            .nth(tick)
            .unwrap()
            .ms
    }

    #[test]
    fn a_vanilla_session_passes() {
        assert_eq!(
            check(&vanilla(), &[Window { from: 40, to: 80 }]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn each_rule_catches_its_break() {
        // A packet vanilla would not send here: chat.
        let mut s = vanilla();
        s.push(packet(500, vec![0x01, 0]));
        assert!(check(&s, &[])[0].contains("0x01"));
        // Two movement packets a tick for a second.
        let mut s = vanilla();
        for k in 0..20 {
            s.push(packet(4000 + k * 50 + 25, vec![0x03, 0]));
        }
        s.sort_by_key(|r| r.ms);
        assert!(
            check(&s, &[])
                .iter()
                .any(|p| p.contains("movement packets"))
        );
        // Vanilla's longest run without a position is 20 packets; 21 is not.
        let mut s = vanilla();
        for r in s.iter_mut().filter(|r| r.id() == 0x06) {
            r.body[0] = 0x05;
        }
        for (tick, r) in s.iter_mut().filter(|r| is_move(r.id())).enumerate() {
            if tick % 21 == 0 {
                r.body[0] = 0x06;
            }
        }
        assert!(check(&s, &[]).iter().all(|p| !p.contains("no position")));
        // Forty ticks without a position.
        let mut s = vanilla();
        for r in s.iter_mut().filter(|r| r.id() == 0x06 && r.ms < 2000) {
            r.body[0] = 0x05;
        }
        assert!(check(&s, &[]).iter().any(|p| p.contains("no position")));
        // An attack without its swing.
        let mut s = vanilla();
        s.retain(|r| r.id() != 0x0a);
        assert!(check(&s, &[]).iter().any(|p| p.contains("without a swing")));
        // Sprint started twice.
        let mut s = vanilla();
        s.push(packet(1000, vec![0x0b, 9, 3, 0]));
        s.push(packet(1100, vec![0x0b, 9, 3, 0]));
        s.sort_by_key(|r| r.ms);
        assert!(
            check(&s, &[])
                .iter()
                .any(|p| p.contains("sprint start twice"))
        );
        // A sneak while the inventory is open; the stop right as it opens is
        // allowed.
        let mut s = vanilla();
        let window = Window { from: 40, to: 80 };
        let opened = moves_at(&s, 40);
        s.push(packet(1500, vec![0x0b, 9, 0, 0]));
        s.push(packet(opened + 10, vec![0x0b, 9, 1, 0]));
        s.sort_by_key(|r| r.ms);
        assert!(check(&s, std::slice::from_ref(&window)).is_empty());
        s.push(packet(moves_at(&s, 60) + 5, vec![0x0b, 9, 0, 0]));
        s.sort_by_key(|r| r.ms);
        let problems = check(&s, &[window]);
        assert!(
            problems.iter().any(|p| p.contains("screen open")),
            "{problems:?}"
        );
        // Settings sent again by a module.
        let mut s = vanilla();
        s.push(packet(7000, vec![0x15, 0]));
        assert!(
            check(&s, &[])
                .iter()
                .any(|p| p.contains("client settings sent 2"))
        );
        // A window after the session ended is reported, not skipped.
        assert!(!check(&vanilla(), &[Window { from: 400, to: 450 }]).is_empty());
    }

    #[test]
    fn windows_parse() {
        assert_eq!(
            Window::parse("200-260"),
            Some(Window { from: 200, to: 260 })
        );
        assert_eq!(Window::parse("260-200"), None);
        assert_eq!(Window::parse("x"), None);
    }
}
