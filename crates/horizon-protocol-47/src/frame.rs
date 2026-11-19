use crate::{cipher::Cipher, invalid, read_varint, write_varint};
use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};
use std::io::{self, Cursor, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};

pub const MAX_FRAME: usize = (1 << 21) - 1;
/// NettyCompressionDecoder in the owned vanilla 1.8.9 client, ei.class.
pub const MAX_UNCOMPRESSED: usize = 2_097_152;

/// Blocking, exact-read transport. A connection is terminal after an IO/codec
/// failure. The owner must drop it, since a partial write advances cipher state.
pub struct Connection<S> {
    stream: S,
    threshold: Option<usize>,
    encrypt: Option<Cipher>,
    decrypt: Option<Cipher>,
    failed: bool,
}

/// Owns only inbound framing and its continuous decryption state.
pub struct PacketReader<S> {
    connection: Connection<S>,
    threshold: Arc<AtomicI32>,
}

/// Owns outbound framing and its continuous encryption state. Keep one owner
/// or serialize all sends through the same actor to preserve packet order.
pub struct PacketWriter<S> {
    connection: Connection<S>,
    threshold: Arc<AtomicI32>,
}

impl Connection<TcpStream> {
    pub fn split(mut self) -> io::Result<(PacketReader<TcpStream>, PacketWriter<TcpStream>)> {
        self.ensure_open()?;
        let threshold = Arc::new(AtomicI32::new(
            self.threshold.map_or(-1, |value| value as i32),
        ));
        let receiver = Connection {
            stream: self.stream.try_clone()?,
            threshold: self.threshold,
            encrypt: None,
            decrypt: self.decrypt.take(),
            failed: false,
        };
        Ok((
            PacketReader {
                connection: receiver,
                threshold: threshold.clone(),
            },
            PacketWriter {
                connection: self,
                threshold,
            },
        ))
    }
}

impl<S: Read + Write> PacketReader<S> {
    pub fn receive(&mut self) -> io::Result<Vec<u8>> {
        self.connection
            .set_compression(self.threshold.load(Ordering::Acquire));
        self.connection.receive()
    }
    pub fn set_compression(&mut self, threshold: i32) {
        self.connection.set_compression(threshold);
        self.threshold.store(threshold, Ordering::Release);
    }
}

impl<S: Read + Write> PacketWriter<S> {
    pub fn send(&mut self, body: &[u8]) -> io::Result<()> {
        self.connection
            .set_compression(self.threshold.load(Ordering::Acquire));
        self.connection.send(body)
    }
    pub fn set_compression(&mut self, threshold: i32) {
        self.connection.set_compression(threshold);
        self.threshold.store(threshold, Ordering::Release);
    }
}

impl PacketWriter<TcpStream> {
    /// Wake a blocked reader as part of connection shutdown.
    pub fn shutdown(&self) -> io::Result<()> {
        match self.connection.stream.shutdown(Shutdown::Both) {
            Err(error) if error.kind() == io::ErrorKind::NotConnected => Ok(()),
            result => result,
        }
    }
}

impl<S: Read + Write> Connection<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            threshold: None,
            encrypt: None,
            decrypt: None,
            failed: false,
        }
    }

    /// Consume the transport and recover its underlying stream.
    pub fn into_inner(self) -> S {
        self.stream
    }

    /// Call only after the plaintext encryption response has been sent.
    pub fn enable_encryption(&mut self, secret: &[u8; 16]) -> io::Result<()> {
        self.ensure_open()?;
        if self.encrypt.is_some() {
            return Err(invalid("Encryption is already enabled"));
        }
        self.encrypt = Some(Cipher::new(secret));
        self.decrypt = Some(Cipher::new(secret));
        Ok(())
    }

    /// A negative login threshold disables compression, as in vanilla.
    pub fn set_compression(&mut self, threshold: i32) {
        self.threshold = usize::try_from(threshold).ok();
    }

    pub fn send(&mut self, body: &[u8]) -> io::Result<()> {
        self.ensure_open()?;
        // Validate and compress before changing the encryption stream state.
        let mut frame = encode(body, self.threshold)?;
        if let Some(cipher) = &mut self.encrypt {
            cipher.apply(&mut frame, true);
        }
        let result = self
            .stream
            .write_all(&frame)
            .and_then(|()| self.stream.flush());
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    pub fn receive(&mut self) -> io::Result<Vec<u8>> {
        self.ensure_open()?;
        let result = self.receive_frame();
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn ensure_open(&self) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Connection failed previously",
            ));
        }
        Ok(())
    }

    fn read_exact(&mut self, bytes: &mut [u8]) -> io::Result<()> {
        self.stream.read_exact(bytes)?;
        if let Some(cipher) = &mut self.decrypt {
            cipher.apply(bytes, false);
        }
        Ok(())
    }

    fn receive_frame(&mut self) -> io::Result<Vec<u8>> {
        let mut length = 0usize;
        for index in 0..3 {
            let mut byte = [0];
            self.read_exact(&mut byte)?;
            length |= usize::from(byte[0] & 0x7f) << (index * 7);
            if byte[0] & 0x80 == 0 {
                if length == 0 {
                    return Err(invalid("Empty packet frame"));
                }
                let mut frame = vec![0; length];
                self.read_exact(&mut frame)?;
                return decode(frame, self.threshold);
            }
        }
        Err(invalid("Frame length exceeds three bytes"))
    }
}

fn encode(body: &[u8], threshold: Option<usize>) -> io::Result<Vec<u8>> {
    if body.is_empty() || body.len() > MAX_UNCOMPRESSED {
        return Err(invalid("Invalid packet body length"));
    }
    let mut payload = Vec::new();
    match threshold {
        None => payload.extend_from_slice(body),
        Some(threshold) if body.len() < threshold => {
            payload.push(0);
            payload.extend_from_slice(body);
        }
        Some(_) => {
            write_varint(&mut payload, body.len() as i32);
            let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
            compressor.write_all(body)?;
            payload.extend_from_slice(&compressor.finish()?);
        }
    }
    if payload.len() > MAX_FRAME {
        return Err(invalid("Encoded frame exceeds 21-bit length"));
    }
    let mut frame = Vec::with_capacity(payload.len() + 3);
    write_varint(&mut frame, payload.len() as i32);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode(frame: Vec<u8>, threshold: Option<usize>) -> io::Result<Vec<u8>> {
    let Some(threshold) = threshold else {
        return Ok(frame);
    };
    let mut cursor = Cursor::new(frame.as_slice());
    let length = read_varint(&mut cursor)?;
    let compressed = &frame[cursor.position() as usize..];
    if length == 0 {
        if compressed.is_empty() {
            return Err(invalid("Empty uncompressed packet"));
        }
        return Ok(compressed.to_vec());
    }
    if length < 0 || (length as usize) < threshold || length as usize > MAX_UNCOMPRESSED {
        return Err(invalid("Invalid uncompressed packet length"));
    }
    // One extra byte detects expansion beyond the declared size. The hard cap
    // bounds allocation even when the peer supplies a malicious zlib stream.
    let mut body = vec![0; length as usize + 1];
    let mut inflater = Decompress::new(true);
    let status = inflater
        .decompress(compressed, &mut body, FlushDecompress::Finish)
        .map_err(|_| invalid("Invalid zlib packet"))?;
    if status != Status::StreamEnd
        || inflater.total_out() != length as u64
        || inflater.total_in() != compressed.len() as u64
    {
        return Err(invalid("Zlib packet length or stream boundary mismatch"));
    }
    body.truncate(length as usize);
    Ok(body)
}
