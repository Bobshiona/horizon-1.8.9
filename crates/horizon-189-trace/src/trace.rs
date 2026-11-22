//! The trace file: every play-state packet body of a session, in order,
//! with its direction and milliseconds since login finished.
//!
//! `BOBTRACE` and a little-endian u16 version, then records of a direction
//! byte (0 server to client, 1 client to server), a u32 time in ms, a u32
//! length and the uncompressed packet body (id varint first).
use std::io::{self, Read, Write};

const MAGIC: &[u8; 8] = b"BOBTRACE";
const VERSION: u16 = 1;
/// Larger than any Protocol 47 packet body.
const MAX_BODY: u32 = 4 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    ToClient,
    ToServer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub direction: Direction,
    pub ms: u32,
    pub body: Vec<u8>,
}

impl Record {
    /// The packet id, the body's leading varint.
    #[cfg(test)]
    pub fn id(&self) -> i32 {
        horizon_protocol_47::read_varint(&mut &self.body[..]).unwrap_or(-1)
    }
}

pub fn write_header(out: &mut impl Write) -> io::Result<()> {
    out.write_all(MAGIC)?;
    out.write_all(&VERSION.to_le_bytes())
}

pub fn write_record(out: &mut impl Write, record: &Record) -> io::Result<()> {
    let direction = match record.direction {
        Direction::ToClient => 0u8,
        Direction::ToServer => 1,
    };
    let length = u32::try_from(record.body.len()).map_err(|_| invalid("body too long"))?;
    out.write_all(&[direction])?;
    out.write_all(&record.ms.to_le_bytes())?;
    out.write_all(&length.to_le_bytes())?;
    out.write_all(&record.body)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Read a whole trace.
pub fn read(mut input: impl Read) -> io::Result<Vec<Record>> {
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    let mut version = [0; 2];
    input.read_exact(&mut version)?;
    if &magic != MAGIC || u16::from_le_bytes(version) != VERSION {
        return Err(invalid("not a version 1 Bob Client trace"));
    }
    let mut records = Vec::new();
    loop {
        let mut direction = [0; 1];
        match input.read_exact(&mut direction) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(records),
            Err(error) => return Err(error),
        }
        let mut word = [0; 4];
        input.read_exact(&mut word)?;
        let ms = u32::from_le_bytes(word);
        input.read_exact(&mut word)?;
        let length = u32::from_le_bytes(word);
        if length == 0 || length > MAX_BODY {
            return Err(invalid("bad record length"));
        }
        let mut body = vec![0; length as usize];
        input.read_exact(&mut body)?;
        let direction = match direction[0] {
            0 => Direction::ToClient,
            1 => Direction::ToServer,
            _ => return Err(invalid("bad record direction")),
        };
        records.push(Record {
            direction,
            ms,
            body,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_round_trip_and_reject_damage() {
        let records = vec![
            Record {
                direction: Direction::ToClient,
                ms: 0,
                body: vec![0x01, 1, 2, 3],
            },
            Record {
                direction: Direction::ToServer,
                ms: 50,
                body: vec![0x06; 42],
            },
        ];
        let mut bytes = Vec::new();
        write_header(&mut bytes).unwrap();
        for record in &records {
            write_record(&mut bytes, record).unwrap();
        }
        assert_eq!(read(&bytes[..]).unwrap(), records);
        assert_eq!(records[1].id(), 6);
        assert!(read(&b"NOTATRACE!"[..]).is_err());
        // A record cut short is an error, not a silently shorter trace.
        assert!(read(&bytes[..bytes.len() - 3]).is_err());
    }
}
