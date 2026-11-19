use super::*;
use crate::cipher::Cipher;
use std::io::{Cursor, Read, Write};

#[test]
fn split_compression_change_updates_both_directions() {
    use std::{
        net::{TcpListener, TcpStream},
        time::Duration,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut connection = Connection::new(stream);
        connection.set_compression(0);
        assert_eq!(connection.receive().unwrap(), [1, 2]);
        connection.send(&[3, 4]).unwrap();
    });
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (mut reader, mut writer) = Connection::new(stream).split().unwrap();
    reader.set_compression(0);
    writer.send(&[1, 2]).unwrap();
    assert_eq!(reader.receive().unwrap(), [3, 4]);
    server.join().unwrap();
}

#[test]
fn partial_encrypted_write_is_terminal() {
    struct FailsAfterPrefix(bool);
    impl Read for FailsAfterPrefix {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }
    impl Write for FailsAfterPrefix {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if std::mem::replace(&mut self.0, true) {
                Err(std::io::ErrorKind::BrokenPipe.into())
            } else {
                Ok(bytes.len().min(2))
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut connection = Connection::new(FailsAfterPrefix(false));
    connection.enable_encryption(&[1; 16]).unwrap();
    assert!(connection.send(&[0; 100]).is_err());
    assert_eq!(
        connection.send(&[1]).unwrap_err().kind(),
        std::io::ErrorKind::NotConnected
    );
}

#[test]
fn signed_varints_match_wire_vectors() {
    for (value, expected) in [
        (0, vec![0]),
        (127, vec![127]),
        (128, vec![128, 1]),
        (255, vec![255, 1]),
        (i32::MAX, vec![255, 255, 255, 255, 7]),
        (-1, vec![255, 255, 255, 255, 15]),
        (i32::MIN, vec![128, 128, 128, 128, 8]),
    ] {
        let mut encoded = Vec::new();
        write_varint(&mut encoded, value);
        assert_eq!(encoded, expected);
        assert_eq!(read_varint(&mut encoded.as_slice()).unwrap(), value);
    }
    assert!(read_varint(&mut &[255, 255, 255, 255, 127][..]).is_err());
    assert!(read_varint(&mut &[128; 6][..]).is_err());
    assert!(read_varint(&mut &[128][..]).is_err());
}

#[test]
fn strings_use_java_utf16_limits() {
    let mut body = Vec::new();
    assert!(write_string(&mut body, "😀", 1).is_err());
    write_string(&mut body, "😀", 2).unwrap();
    assert!(read_string(&mut body.as_slice(), 1).is_err());
    assert_eq!(read_string(&mut body.as_slice(), 2).unwrap(), "😀");
    assert!(read_string(&mut &[1, 255][..], 16).is_err());
    assert!(read_string(&mut &[255, 255, 255, 255, 15][..], 16).is_err());
}

#[test]
fn handshake_matches_independent_protocol_47_vector() {
    assert_eq!(
        packet::handshake("localhost", 25565, packet::NextState::Status).unwrap(),
        [
            0, 47, 9, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0x63, 0xdd, 1
        ]
    );
}

fn wire(body: &[u8], threshold: i32) -> Vec<u8> {
    let mut sender = Connection::new(Duplex::default());
    sender.set_compression(threshold);
    sender.send(body).unwrap();
    sender.into_inner().output
}

#[derive(Default)]
struct Duplex {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl Read for Duplex {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let length = bytes.len().min(1);
        self.input.read(&mut bytes[..length])
    }
}

impl Write for Duplex {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let length = bytes.len().min(2);
        self.output.extend_from_slice(&bytes[..length]);
        Ok(length)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn frames_roundtrip_fragmented_reads_and_short_writes() {
    for threshold in [-1, 0, 1, 16, 256] {
        for size in [1, 15, 16, 17, 255, 256, 4096] {
            let body: Vec<u8> = (0..size).map(|index| index as u8).collect();
            let encoded = wire(&body, threshold);
            let mut receiver = Connection::new(Duplex {
                input: Cursor::new(encoded),
                output: Vec::new(),
            });
            receiver.set_compression(threshold);
            assert_eq!(receiver.receive().unwrap(), body);
        }
    }
}

#[test]
fn rejected_frame_poisoning_prevents_reuse() {
    for bytes in [vec![0], vec![128, 128, 128, 1], vec![2, 0]] {
        let mut receiver = Connection::new(Cursor::new(bytes));
        assert!(receiver.receive().is_err());
        assert_eq!(
            receiver.receive().unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );
        assert_eq!(
            receiver.send(&[0]).unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );
    }
}

#[test]
fn compression_transition_keeps_coalesced_next_frame() {
    let mut input = wire(&[3, 0], -1);
    input.extend(wire(&[2, 1, 2, 3], 0));
    let mut receiver = Connection::new(Cursor::new(input));
    assert_eq!(receiver.receive().unwrap(), [3, 0]);
    receiver.set_compression(0);
    assert_eq!(receiver.receive().unwrap(), [2, 1, 2, 3]);
}

#[test]
fn declared_compressed_lengths_are_enforced() {
    let encoded = wire(&[0; 128], 0);
    let mut cursor = Cursor::new(encoded.as_slice());
    read_varint(&mut cursor).unwrap();
    let length_offset = cursor.position() as usize;
    for length in [127, 129] {
        let mut forged = encoded.clone();
        let mut replacement = Vec::new();
        write_varint(&mut replacement, length);
        // Reconstruct the outer prefix when the VarInt width changes.
        let mut payload = replacement;
        payload.extend_from_slice(&encoded[length_offset + 2..]);
        forged.clear();
        write_varint(&mut forged, payload.len() as i32);
        forged.extend(payload);
        let mut receiver = Connection::new(Cursor::new(forged));
        receiver.set_compression(0);
        assert!(receiver.receive().is_err());
    }
    let mut oversized = Vec::new();
    write_varint(&mut oversized, MAX_UNCOMPRESSED as i32 + 1);
    let mut frame = Vec::new();
    write_varint(&mut frame, oversized.len() as i32);
    frame.extend(oversized);
    let mut receiver = Connection::new(Cursor::new(frame));
    receiver.set_compression(0);
    assert!(receiver.receive().is_err());
}

#[test]
fn encryption_matches_zero_key_vector_and_split_stream() {
    // AES-128 with zero key and zero input starts with 0x66.
    let mut cipher = Cipher::new(&[0; 16]);
    let mut plaintext = vec![0; 64];
    cipher.apply(&mut plaintext[..1], true);
    assert_eq!(plaintext[0], 0x66);
    cipher.apply(&mut plaintext[1..], true);
    let ciphertext = plaintext.clone();
    let mut receiver = Cipher::new(&[0; 16]);
    for chunk in plaintext.chunks_mut(3) {
        receiver.apply(chunk, false);
    }
    assert_eq!(plaintext, vec![0; 64]);
    let mut whole = vec![0; 64];
    Cipher::new(&[0; 16]).apply(&mut whole, true);
    assert_eq!(whole, ciphertext);
    // Generated independently with OpenSSL aes-128-cfb8, zero key/IV.
    let hex: String = ciphertext
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        hex,
        concat!(
            "6616f92e42a8f11a911668578ec3aa0f93004aab22b12b26d12896731cd112f6",
            "9c56b650ca75ae13cb73d3de96754a1450087af4ff2ce5e465296e4d46b2c313"
        )
    );
}

#[test]
fn encrypted_frames_preserve_independent_stream_state() {
    let secret = [0x5a; 16];
    let mut sender = Connection::new(Duplex::default());
    sender.enable_encryption(&secret).unwrap();
    sender.set_compression(0);
    sender.send(&[1, 2, 3]).unwrap();
    sender.send(&[4; 1024]).unwrap();
    let mut receiver = Connection::new(Duplex {
        input: Cursor::new(sender.into_inner().output),
        output: Vec::new(),
    });
    receiver.enable_encryption(&secret).unwrap();
    receiver.set_compression(0);
    assert_eq!(receiver.receive().unwrap(), [1, 2, 3]);
    assert_eq!(receiver.receive().unwrap(), [4; 1024]);
    assert!(receiver.enable_encryption(&secret).is_err());
}

#[test]
fn encryption_activation_preserves_coalesced_ciphertext() {
    let secret = [7; 16];
    let mut sender = Connection::new(Duplex::default());
    sender.send(&[1, 0]).unwrap();
    sender.enable_encryption(&secret).unwrap();
    sender.send(&[2, 3, 4]).unwrap();
    let mut receiver = Connection::new(Cursor::new(sender.into_inner().output));
    assert_eq!(receiver.receive().unwrap(), [1, 0]);
    receiver.enable_encryption(&secret).unwrap();
    assert_eq!(receiver.receive().unwrap(), [2, 3, 4]);
}

#[test]
fn loopback_status_and_ping_use_protocol_47_layouts() {
    use std::{
        net::{TcpListener, TcpStream},
        time::Duration,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut connection = Connection::new(stream);
        let handshake = connection.receive().unwrap();
        let mut input = Cursor::new(handshake.as_slice());
        assert_eq!(read_varint(&mut input).unwrap(), 0);
        assert_eq!(read_varint(&mut input).unwrap(), 47);
        assert_eq!(read_string(&mut input, 255).unwrap(), "localhost");
        let mut port = [0; 2];
        input.read_exact(&mut port).unwrap();
        assert_eq!(u16::from_be_bytes(port), address.port());
        assert_eq!(read_varint(&mut input).unwrap(), 1);
        assert_eq!(input.position(), handshake.len() as u64);
        assert_eq!(connection.receive().unwrap(), [0]);
        let mut status = vec![0];
        write_string(
            &mut status,
            r#"{"version":{"name":"1.8.9","protocol":47}}"#,
            32767,
        )
        .unwrap();
        connection.send(&status).unwrap();
        let ping = connection.receive().unwrap();
        assert_eq!(ping, [1, 1, 2, 3, 4, 5, 6, 7, 8]);
        connection.send(&ping).unwrap();
    });
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut client = Connection::new(stream);
    client
        .send(&packet::handshake("localhost", address.port(), packet::NextState::Status).unwrap())
        .unwrap();
    client.send(packet::STATUS_REQUEST).unwrap();
    assert_eq!(
        packet::status_response(&client.receive().unwrap()).unwrap(),
        r#"{"version":{"name":"1.8.9","protocol":47}}"#
    );
    client.send(&packet::ping(0x0102_0304_0506_0708)).unwrap();
    assert_eq!(
        packet::pong(&client.receive().unwrap()).unwrap(),
        0x0102_0304_0506_0708
    );
    server.join().unwrap();
}
