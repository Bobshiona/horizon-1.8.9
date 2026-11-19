//! Protocol 47 login. Authentication/session-service HTTP is supplied by the
//! caller, keeping account credentials outside the wire protocol crate.
use crate::{
    Connection, invalid,
    packet::{self, NextState},
    read_string, read_varint, write_string, write_varint,
};
use rsa::{PublicKey, pkcs8::DecodePublicKey};
use sha1::{Digest, Sha1};
use std::io::{self, Cursor, Read, Write};

#[derive(Debug, PartialEq, Eq)]
pub struct Profile {
    pub uuid: u128,
    pub username: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Packet {
    Disconnect(String),
    EncryptionRequest {
        server_id: String,
        public_key: Vec<u8>,
        verify_token: Vec<u8>,
    },
    Success(Profile),
    Compression(i32),
}

fn array(input: &mut impl Read, max: usize) -> io::Result<Vec<u8>> {
    let length = read_varint(input)?;
    if length < 0 || length as usize > max {
        return Err(invalid("Invalid login byte-array length"));
    }
    let mut bytes = vec![0; length as usize];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn write_array(output: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    let length = i32::try_from(bytes.len()).map_err(|_| invalid("Login byte array exceeds i32"))?;
    write_varint(output, length);
    output.extend_from_slice(bytes);
    Ok(())
}

pub fn decode(body: &[u8]) -> io::Result<Packet> {
    let mut input = Cursor::new(body);
    let packet = match read_varint(&mut input)? {
        0 => Packet::Disconnect(read_string(&mut input, 32767)?),
        1 => Packet::EncryptionRequest {
            server_id: read_string(&mut input, 20)?,
            public_key: array(&mut input, 8192)?,
            verify_token: array(&mut input, 1024)?,
        },
        2 => {
            let uuid = read_string(&mut input, 36)?;
            let username = read_string(&mut input, 16)?;
            let uuid = parse_uuid(&uuid)?;
            Packet::Success(Profile { uuid, username })
        }
        3 => Packet::Compression(read_varint(&mut input)?),
        _ => return Err(invalid("Unknown Protocol 47 login packet")),
    };
    if input.position() != body.len() as u64 {
        return Err(invalid("Trailing login packet bytes"));
    }
    Ok(packet)
}

pub fn parse_uuid(uuid: &str) -> io::Result<u128> {
    let valid_layout = uuid.len() == 32
        || (uuid.len() == 36
            && [8, 13, 18, 23]
                .iter()
                .all(|&index| uuid.as_bytes()[index] == b'-'));
    if !valid_layout {
        return Err(invalid("Invalid login UUID layout"));
    }
    let compact: String = uuid.chars().filter(|&character| character != '-').collect();
    if compact.len() != 32 || !compact.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("Invalid login UUID"));
    }
    u128::from_str_radix(&compact, 16).map_err(|_| invalid("Invalid login UUID"))
}

pub fn start(username: &str) -> io::Result<Vec<u8>> {
    if username.is_empty()
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(invalid("Invalid Minecraft username"));
    }
    let mut body = vec![0];
    write_string(&mut body, username, 16)?;
    Ok(body)
}

/// SHA-1 digest rendered as Java BigInteger's signed hexadecimal value.
pub fn server_hash(server_id: &str, secret: &[u8; 16], public_key: &[u8]) -> String {
    let mut sha = Sha1::new();
    // Vanilla uses ISO-8859-1 for the server ID.
    sha.update(
        server_id
            .chars()
            .map(|character| u8::try_from(character as u32).unwrap_or(b'?'))
            .collect::<Vec<_>>(),
    );
    sha.update(secret);
    sha.update(public_key);
    signed_hex(&sha.finalize())
}

fn signed_hex(digest: &[u8]) -> String {
    let negative = digest[0] & 0x80 != 0;
    let mut bytes = digest.to_vec();
    if negative {
        let mut carry = true;
        for byte in bytes.iter_mut().rev() {
            *byte = !*byte;
            if carry {
                let (value, overflow) = byte.overflowing_add(1);
                *byte = value;
                carry = overflow;
            }
        }
    }
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let hex = hex.trim_start_matches('0');
    let hex = if hex.is_empty() { "0" } else { hex };
    if negative {
        format!("-{hex}")
    } else {
        hex.to_owned()
    }
}

/// Complete login over a connected socket. `join` must authenticate this
/// account's profile with the session service for the supplied server hash.
/// An offline-mode server never invokes it. On failure, drop the connection.
pub fn connect<S: Read + Write>(
    connection: &mut Connection<S>,
    host: &str,
    port: u16,
    username: &str,
    mut join: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<Profile> {
    let handshake = packet::handshake(host, port, NextState::Login)?;
    let start = start(username)?;
    connection.send(&handshake)?;
    connection.send(&start)?;
    let mut encrypted = false;
    loop {
        match decode(&connection.receive()?)? {
            Packet::Disconnect(reason) => {
                return Err(io::Error::new(io::ErrorKind::ConnectionAborted, reason));
            }
            Packet::Compression(threshold) => connection.set_compression(threshold),
            Packet::Success(profile) => return Ok(profile),
            Packet::EncryptionRequest {
                server_id,
                public_key,
                verify_token,
            } => {
                if encrypted {
                    return Err(invalid("Repeated encryption request"));
                }
                let key = PublicKey::from_public_key_der(&public_key)
                    .map_err(|_| invalid("Invalid RSA public key"))?;
                let mut secret = [0; 16];
                getrandom::fill(&mut secret)
                    .map_err(|_| io::Error::other("Secure random source unavailable"))?;
                let mut response = vec![1];
                let encrypted_secret = key
                    .pkcs1v15_encrypt(&secret)
                    .map_err(|_| invalid("RSA secret encryption failed"))?;
                let encrypted_token = key
                    .pkcs1v15_encrypt(&verify_token)
                    .map_err(|_| invalid("RSA token encryption failed"))?;
                write_array(&mut response, &encrypted_secret)?;
                write_array(&mut response, &encrypted_token)?;
                join(&server_hash(&server_id, &secret, &public_key))?;
                connection.send(&response)?;
                connection.enable_encryption(&secret)?;
                encrypted = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_signed_hash_vectors() {
        for (text, expected) in [
            ("Notch", "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48"),
            ("jeb_", "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1"),
            ("simon", "88e16a1019277b15d58faf0541e11910eb756f6"),
        ] {
            assert_eq!(signed_hex(&Sha1::digest(text.as_bytes())), expected);
        }
        assert_eq!(signed_hex(&[0; 20]), "0");
        assert_eq!(signed_hex(&[0xff; 20]), "-1");
    }

    #[test]
    fn login_packets_are_version_specific_and_bounded() {
        assert_eq!(decode(&[3, 0x80, 2]).unwrap(), Packet::Compression(256));
        assert_eq!(start("Bob").unwrap(), [0, 3, b'B', b'o', b'b']);
        assert!(start("").is_err());
        assert!(start("a name").is_err());
        assert!(start("abcdefghijklmnopq").is_err());
        assert!(decode(&[3, 0, 0]).is_err());
        assert!(decode(&[4]).is_err());
        assert!(decode(&[1, 0, 255, 255, 255, 255, 15]).is_err());
        assert_eq!(
            parse_uuid("b50ad385-829d-3141-a216-7e7d7539ba7f").unwrap(),
            0xb50ad385829d3141a2167e7d7539ba7f
        );
        assert!(parse_uuid("b50ad385829d-3141a2167e7d7539ba7f").is_err());
    }

    #[test]
    fn encrypted_login_orders_session_join_before_reply_and_preserves_play_stream() {
        use rsa::{PrivateKey, pkcs8::EncodePublicKey};
        use std::{
            net::{TcpListener, TcpStream},
            sync::mpsc,
            time::Duration,
        };

        let key = PrivateKey::new(&mut rand::rngs::SysRng, 1024).unwrap();
        let public_key = key
            .to_public_key()
            .to_public_key_der()
            .unwrap()
            .as_bytes()
            .to_vec();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (join_send, join_receive) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut connection = Connection::new(stream);
            assert_eq!(
                connection.receive().unwrap(),
                packet::handshake("localhost", address.port(), NextState::Login).unwrap()
            );
            assert_eq!(connection.receive().unwrap(), [0, 3, b'B', b'o', b'b']);
            let verify = [0x12, 0x34, 0x56, 0x78];
            let mut request = vec![1, 0];
            write_array(&mut request, &public_key).unwrap();
            write_array(&mut request, &verify).unwrap();
            connection.send(&request).unwrap();
            let response = connection.receive().unwrap();
            let mut response = response.as_slice();
            assert_eq!(read_varint(&mut response).unwrap(), 1);
            let secret = key
                .pkcs1v15_decrypt(&array(&mut response, 8192).unwrap())
                .unwrap();
            let token = key
                .pkcs1v15_decrypt(&array(&mut response, 8192).unwrap())
                .unwrap();
            assert!(response.is_empty());
            assert_eq!(token, verify);
            let secret: [u8; 16] = secret.try_into().unwrap();
            assert_eq!(
                join_receive.try_recv().unwrap(),
                server_hash("", &secret, &public_key)
            );
            connection.enable_encryption(&secret).unwrap();
            connection.send(&[3, 16]).unwrap();
            connection.set_compression(16);
            let mut success = vec![2];
            write_string(&mut success, "b50ad385-829d-3141-a216-7e7d7539ba7f", 36).unwrap();
            write_string(&mut success, "Bob", 16).unwrap();
            connection.send(&success).unwrap();
            connection.send(&[0, 123]).unwrap();
            assert_eq!(connection.receive().unwrap(), [0, 123]);
        });
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut connection = Connection::new(stream);
        let profile = connect(
            &mut connection,
            "localhost",
            address.port(),
            "Bob",
            |hash| {
                join_send.send(hash.to_owned()).unwrap();
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(profile.username, "Bob");
        assert_eq!(profile.uuid, 0xb50ad385829d3141a2167e7d7539ba7f);
        let (mut reader, mut writer) = connection.split().unwrap();
        assert_eq!(reader.receive().unwrap(), [0, 123]);
        writer.send(&[0, 123]).unwrap();
        server.join().unwrap();
        writer.shutdown().unwrap();
    }
}
