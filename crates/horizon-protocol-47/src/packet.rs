//! Handshake and status layouts verified against minecraft-data's 1.8 schema.
use crate::{VERSION, invalid, read_string, read_varint, write_string, write_varint};
use std::io::{self, Cursor};

#[derive(Clone, Copy)]
pub enum NextState {
    Status = 1,
    Login = 2,
}

pub fn handshake(host: &str, port: u16, next: NextState) -> io::Result<Vec<u8>> {
    let mut body = vec![0];
    write_varint(&mut body, VERSION);
    write_string(&mut body, host, 255)?;
    body.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut body, next as i32);
    Ok(body)
}

pub const STATUS_REQUEST: &[u8] = &[0];

pub fn status_response(body: &[u8]) -> io::Result<String> {
    let mut cursor = Cursor::new(body);
    if read_varint(&mut cursor)? != 0 {
        return Err(invalid("Expected status response packet 0x00"));
    }
    let json = read_string(&mut cursor, 32767)?;
    if cursor.position() != body.len() as u64 {
        return Err(invalid("Trailing status response bytes"));
    }
    Ok(json)
}

pub fn ping(payload: i64) -> Vec<u8> {
    let mut body = vec![1];
    body.extend_from_slice(&payload.to_be_bytes());
    body
}

pub fn pong(body: &[u8]) -> io::Result<i64> {
    if body.len() != 9 || body[0] != 1 {
        return Err(invalid("Expected status pong packet 0x01 and i64 payload"));
    }
    Ok(i64::from_be_bytes(
        body[1..].try_into().expect("length checked"),
    ))
}
