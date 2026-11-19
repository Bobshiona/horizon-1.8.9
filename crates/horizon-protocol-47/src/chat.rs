//! Protocol 47 chat: S02 messages, C01 chat, C14/S3A tab completion.
use crate::{invalid, read_string, read_varint, write_string, write_varint};
use std::io::{self, Cursor, Read};

/// Where an S02 message shows: chat, system message or the action bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Chat,
    System,
    ActionBar,
}

/// S02 Chat Message: JSON component and position byte.
pub fn decode_message(fields: &[u8]) -> io::Result<(String, Position)> {
    let input = &mut Cursor::new(fields);
    let json = read_string(input, 32767)?;
    let mut position = [0];
    input.read_exact(&mut position)?;
    let position = match position[0] {
        0 => Position::Chat,
        1 => Position::System,
        2 => Position::ActionBar,
        _ => return Err(invalid("Unknown chat position")),
    };
    Ok((json, position))
}

/// `GuiChat` caps typed text at 100 characters, as C01 does.
pub const MAX_MESSAGE: usize = 100;

/// C01 Chat Message.
pub fn message(text: &str) -> io::Result<Vec<u8>> {
    let mut out = vec![0x01];
    write_string(&mut out, text, MAX_MESSAGE)?;
    Ok(out)
}

/// C14 Tab-Complete for the text left of the cursor, without a looked-at block.
pub fn tab_complete(text: &str) -> io::Result<Vec<u8>> {
    let mut out = vec![0x14];
    write_string(&mut out, text, 32767)?;
    out.push(0);
    Ok(out)
}

/// S3A Tab-Complete matches.
pub fn decode_tab_complete(fields: &[u8]) -> io::Result<Vec<String>> {
    let input = &mut Cursor::new(fields);
    let count = read_varint(input)?;
    if count < 0 || count as usize > fields.len() {
        return Err(invalid("Invalid tab-complete count"));
    }
    (0..count).map(|_| read_string(input, 32767)).collect()
}

/// C16 Client Status (0 respawn, 1 statistics, 2 inventory achievement).
pub fn client_status(action: i32) -> Vec<u8> {
    let mut out = vec![0x16];
    write_varint(&mut out, action);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_packets_round_trip_their_limits() {
        let mut fields = Vec::new();
        write_string(&mut fields, r#"{"text":"hi"}"#, 32767).unwrap();
        fields.push(2);
        assert_eq!(
            decode_message(&fields).unwrap(),
            (r#"{"text":"hi"}"#.to_owned(), Position::ActionBar)
        );
        fields.pop();
        fields.push(9);
        assert!(decode_message(&fields).is_err());
        assert_eq!(message("/l").unwrap(), [0x01, 2, b'/', b'l']);
        assert!(message(&"a".repeat(101)).is_err());
        assert!(message(&"é".repeat(100)).is_ok());
        assert_eq!(tab_complete("/p").unwrap(), [0x14, 2, b'/', b'p', 0]);
        let mut matches = vec![2];
        write_string(&mut matches, "Bob", 32767).unwrap();
        write_string(&mut matches, "Bobby", 32767).unwrap();
        assert_eq!(decode_tab_complete(&matches).unwrap(), ["Bob", "Bobby"]);
        assert!(decode_tab_complete(&[200, 1]).is_err());
        assert_eq!(client_status(0), [0x16, 0]);
    }
}
