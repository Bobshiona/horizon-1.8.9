//! Protocol 47 scoreboards, teams, titles and the tab-list header:
//! S3B, S3C, S3D, S3E, S45 and S47.
use crate::{invalid, read_string, read_varint};
use std::io::{self, Cursor, Read};

fn byte(input: &mut impl Read) -> io::Result<u8> {
    let mut b = [0];
    input.read_exact(&mut b)?;
    Ok(b[0])
}

fn int(input: &mut impl Read) -> io::Result<i32> {
    let mut b = [0; 4];
    input.read_exact(&mut b)?;
    Ok(i32::from_be_bytes(b))
}

fn finish(input: &Cursor<&[u8]>) -> io::Result<()> {
    if input.position() != input.get_ref().len() as u64 {
        return Err(invalid("Trailing scoreboard bytes"));
    }
    Ok(())
}

/// S3B Scoreboard Objective.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Objective {
    /// Create (or update) with a display name; `hearts` for that render type.
    Set {
        name: String,
        display: String,
        hearts: bool,
    },
    Remove {
        name: String,
    },
}

pub fn decode_objective(fields: &[u8]) -> io::Result<Objective> {
    let input = &mut Cursor::new(fields);
    let name = read_string(input, 16)?;
    let objective = match byte(input)? {
        1 => Objective::Remove { name },
        0 | 2 => {
            let display = read_string(input, 32)?;
            let hearts = read_string(input, 16)? == "hearts";
            Objective::Set {
                name,
                display,
                hearts,
            }
        }
        _ => return Err(invalid("Unknown objective mode")),
    };
    finish(input)?;
    Ok(objective)
}

/// S3C Update Score. An empty objective on removal means every objective.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Score {
    Set {
        entry: String,
        objective: String,
        value: i32,
    },
    Remove {
        entry: String,
        objective: String,
    },
}

pub fn decode_score(fields: &[u8]) -> io::Result<Score> {
    let input = &mut Cursor::new(fields);
    let entry = read_string(input, 40)?;
    let action = byte(input)?;
    let objective = read_string(input, 16)?;
    let score = match action {
        0 => Score::Set {
            entry,
            objective,
            value: read_varint(input)?,
        },
        1 => Score::Remove { entry, objective },
        _ => return Err(invalid("Unknown score action")),
    };
    finish(input)?;
    Ok(score)
}

/// S3D Display Scoreboard: slot (0 list, 1 sidebar, 2 below name, 3 to 18
/// team-coloured sidebars) and objective, empty to clear.
pub fn decode_display(fields: &[u8]) -> io::Result<(u8, String)> {
    let input = &mut Cursor::new(fields);
    let slot = byte(input)?;
    if slot > 18 {
        return Err(invalid("Unknown display slot"));
    }
    let name = read_string(input, 16)?;
    finish(input)?;
    Ok((slot, name))
}

/// The fields of S3E modes 0 and 2.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TeamInfo {
    pub display: String,
    pub prefix: String,
    pub suffix: String,
    pub friendly_fire: u8,
    /// `always`, `never`, `hideForOtherTeams` or `hideForOwnTeam`.
    pub name_tags: String,
    /// `EnumChatFormatting` index, or -1 for none.
    pub color: i8,
}

/// S3E Teams.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Team {
    Create {
        name: String,
        info: TeamInfo,
        players: Vec<String>,
    },
    Remove {
        name: String,
    },
    Update {
        name: String,
        info: TeamInfo,
    },
    Join {
        name: String,
        players: Vec<String>,
    },
    Leave {
        name: String,
        players: Vec<String>,
    },
}

fn info(input: &mut Cursor<&[u8]>) -> io::Result<TeamInfo> {
    Ok(TeamInfo {
        display: read_string(input, 32)?,
        prefix: read_string(input, 16)?,
        suffix: read_string(input, 16)?,
        friendly_fire: byte(input)?,
        name_tags: read_string(input, 32)?,
        color: byte(input)? as i8,
    })
}

fn players(input: &mut Cursor<&[u8]>) -> io::Result<Vec<String>> {
    let count = read_varint(input)?;
    let left = input.get_ref().len() as u64 - input.position();
    if count < 0 || count as u64 > left {
        return Err(invalid("Invalid team player count"));
    }
    (0..count).map(|_| read_string(input, 40)).collect()
}

pub fn decode_team(fields: &[u8]) -> io::Result<Team> {
    let input = &mut Cursor::new(fields);
    let name = read_string(input, 16)?;
    let team = match byte(input)? {
        0 => {
            let info = info(input)?;
            Team::Create {
                name,
                info,
                players: players(input)?,
            }
        }
        1 => Team::Remove { name },
        2 => Team::Update {
            name,
            info: info(input)?,
        },
        3 => Team::Join {
            name,
            players: players(input)?,
        },
        4 => Team::Leave {
            name,
            players: players(input)?,
        },
        _ => return Err(invalid("Unknown team mode")),
    };
    finish(input)?;
    Ok(team)
}

/// S45 Title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Title {
    Title(String),
    Subtitle(String),
    Times {
        fade_in: i32,
        stay: i32,
        fade_out: i32,
    },
    Clear,
    Reset,
}

pub fn decode_title(fields: &[u8]) -> io::Result<Title> {
    let input = &mut Cursor::new(fields);
    let title = match read_varint(input)? {
        0 => Title::Title(read_string(input, 32767)?),
        1 => Title::Subtitle(read_string(input, 32767)?),
        2 => Title::Times {
            fade_in: int(input)?,
            stay: int(input)?,
            fade_out: int(input)?,
        },
        3 => Title::Clear,
        4 => Title::Reset,
        _ => return Err(invalid("Unknown title action")),
    };
    finish(input)?;
    Ok(title)
}

/// S47 Player List Header And Footer: two chat components.
pub fn decode_header_footer(fields: &[u8]) -> io::Result<(String, String)> {
    let input = &mut Cursor::new(fields);
    let header = read_string(input, 32767)?;
    let footer = read_string(input, 32767)?;
    finish(input)?;
    Ok((header, footer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{write_string, write_varint};

    fn string(out: &mut Vec<u8>, text: &str) {
        write_string(out, text, 32767).unwrap();
    }

    #[test]
    fn scoreboard_packets_decode_every_mode_and_reject_bad_bytes() {
        let mut objective = Vec::new();
        string(&mut objective, "sidebar");
        objective.push(0);
        string(&mut objective, "§e§lBED WARS");
        string(&mut objective, "integer");
        assert_eq!(
            decode_objective(&objective).unwrap(),
            Objective::Set {
                name: "sidebar".into(),
                display: "§e§lBED WARS".into(),
                hearts: false
            }
        );
        let mut score = Vec::new();
        string(&mut score, "§a");
        score.push(0);
        string(&mut score, "sidebar");
        write_varint(&mut score, 7);
        assert_eq!(
            decode_score(&score).unwrap(),
            Score::Set {
                entry: "§a".into(),
                objective: "sidebar".into(),
                value: 7
            }
        );
        score.push(0);
        assert!(decode_score(&score).is_err());
        let mut display = vec![1];
        string(&mut display, "sidebar");
        assert_eq!(decode_display(&display).unwrap(), (1, "sidebar".into()));
        display[0] = 19;
        assert!(decode_display(&display).is_err());

        let mut team = Vec::new();
        string(&mut team, "red");
        team.push(0);
        for text in ["Red", "§c[R] ", "§f"] {
            string(&mut team, text);
        }
        team.push(3);
        string(&mut team, "hideForOtherTeams");
        team.push(12);
        write_varint(&mut team, 2);
        string(&mut team, "Steve");
        string(&mut team, "Alex");
        let Team::Create { info, players, .. } = decode_team(&team).unwrap() else {
            panic!()
        };
        assert_eq!(info.prefix, "§c[R] ");
        assert_eq!(info.color, 12);
        assert_eq!(players, ["Steve", "Alex"]);
        let mut leave = Vec::new();
        string(&mut leave, "red");
        leave.push(4);
        write_varint(&mut leave, 50);
        assert!(decode_team(&leave).is_err());

        let mut times = vec![2];
        for value in [10i32, 70, 20] {
            times.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(
            decode_title(&times).unwrap(),
            Title::Times {
                fade_in: 10,
                stay: 70,
                fade_out: 20
            }
        );
        assert_eq!(decode_title(&[4]).unwrap(), Title::Reset);
        let mut both = Vec::new();
        string(&mut both, r#"{"text":"Hypixel"}"#);
        string(&mut both, r#""footer""#);
        assert_eq!(decode_header_footer(&both).unwrap().1, r#""footer""#);
    }
}
