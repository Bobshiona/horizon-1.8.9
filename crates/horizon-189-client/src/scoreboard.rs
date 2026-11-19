//! The client `Scoreboard` (objectives, scores, display slots, teams) and the
//! tab list (`NetworkPlayerInfo`, header and footer), as the protocol actor
//! applies them and the interface reads them.
use horizon_protocol_47::{
    player_list::Update,
    scoreboard::{Objective, Score, Team, TeamInfo},
};
use std::collections::HashMap;

/// Bounds against servers that never remove what they add.
const MAX_OBJECTIVES: usize = 256;
const MAX_ENTRIES: usize = 4096;
const MAX_TEAMS: usize = 4096;
const MAX_TAB: usize = 1024;

#[derive(Clone, Debug)]
pub struct ObjectiveInfo {
    pub display: String,
    pub hearts: bool,
}

#[derive(Clone, Default)]
pub struct Scoreboard {
    objectives: HashMap<String, ObjectiveInfo>,
    /// Objective name, then score holder, then points.
    scores: HashMap<String, HashMap<String, i32>>,
    slots: [Option<String>; 19],
    teams: HashMap<String, TeamInfo>,
    /// Score holder or player name to team name.
    membership: HashMap<String, String>,
}

impl Scoreboard {
    pub fn objective(&mut self, packet: Objective) {
        match packet {
            Objective::Set {
                name,
                display,
                hearts,
            } => {
                if self.objectives.len() < MAX_OBJECTIVES || self.objectives.contains_key(&name) {
                    self.objectives
                        .insert(name, ObjectiveInfo { display, hearts });
                }
            }
            Objective::Remove { name } => {
                self.objectives.remove(&name);
                self.scores.remove(&name);
                for slot in &mut self.slots {
                    if slot.as_deref() == Some(name.as_str()) {
                        *slot = None;
                    }
                }
            }
        }
    }

    /// `handleUpdateScore`: unknown objectives are ignored; removal with an
    /// empty objective clears the holder everywhere.
    pub fn score(&mut self, packet: Score) {
        match packet {
            Score::Set {
                entry,
                objective,
                value,
            } => {
                if !self.objectives.contains_key(&objective) {
                    return;
                }
                let scores = self.scores.entry(objective).or_default();
                if scores.len() < MAX_ENTRIES || scores.contains_key(&entry) {
                    scores.insert(entry, value);
                }
            }
            Score::Remove { entry, objective } if objective.is_empty() => {
                for scores in self.scores.values_mut() {
                    scores.remove(&entry);
                }
            }
            Score::Remove { entry, objective } => {
                if let Some(scores) = self.scores.get_mut(&objective) {
                    scores.remove(&entry);
                }
            }
        }
    }

    pub fn display(&mut self, slot: u8, name: String) {
        if let Some(target) = self.slots.get_mut(usize::from(slot)) {
            *target = (!name.is_empty()).then_some(name);
        }
    }

    /// `ScorePlayerTeam` changes; joining a team leaves the previous one.
    pub fn team(&mut self, packet: Team) {
        match packet {
            Team::Create {
                name,
                info,
                players,
            } => {
                if self.teams.len() >= MAX_TEAMS && !self.teams.contains_key(&name) {
                    return;
                }
                self.teams.insert(name.clone(), info);
                self.join(&name, players);
            }
            Team::Remove { name } => {
                self.teams.remove(&name);
                self.membership.retain(|_, team| *team != name);
            }
            Team::Update { name, info } => {
                if let Some(team) = self.teams.get_mut(&name) {
                    *team = info;
                }
            }
            Team::Join { name, players } => self.join(&name, players),
            Team::Leave { name, players } => {
                for player in players {
                    if self.membership.get(&player) == Some(&name) {
                        self.membership.remove(&player);
                    }
                }
            }
        }
    }

    fn join(&mut self, name: &str, players: Vec<String>) {
        if !self.teams.contains_key(name) {
            return;
        }
        for player in players {
            if self.membership.len() < MAX_ENTRIES || self.membership.contains_key(&player) {
                self.membership.insert(player, name.to_owned());
            }
        }
    }

    pub fn team_of(&self, entry: &str) -> Option<(&str, &TeamInfo)> {
        let name = self.membership.get(entry)?;
        Some((name.as_str(), self.teams.get(name)?))
    }

    /// `ScorePlayerTeam.formatPlayerName`.
    pub fn format(&self, entry: &str) -> String {
        match self.team_of(entry) {
            Some((_, team)) => format!("{}{entry}{}", team.prefix, team.suffix),
            None => entry.to_owned(),
        }
    }

    pub fn slot(&self, slot: usize) -> Option<(&str, &ObjectiveInfo)> {
        let name = self.slots.get(slot)?.as_deref()?;
        Some((name, self.objectives.get(name)?))
    }

    pub fn points(&self, objective: &str, entry: &str) -> Option<i32> {
        self.scores.get(objective)?.get(entry).copied()
    }

    /// `renderScoreboard`'s objective (the own team colour's sidebar, else
    /// slot 1) and its lines, lowest first, at most 15, `#` holders hidden.
    pub fn sidebar(&self, own_name: &str) -> Option<(String, Vec<(String, i32)>)> {
        let coloured = self
            .team_of(own_name)
            .map(|(_, team)| team.color)
            .filter(|color| (0..16).contains(color))
            .and_then(|color| self.slot(3 + color as usize));
        let (name, objective) = coloured.or_else(|| self.slot(1))?;
        let mut lines: Vec<(&String, i32)> = self
            .scores
            .get(name)
            .into_iter()
            .flatten()
            .filter(|(entry, _)| !entry.starts_with('#'))
            .map(|(entry, points)| (entry, *points))
            .collect();
        lines.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| b.0.to_lowercase().cmp(&a.0.to_lowercase()))
        });
        let skip = lines.len().saturating_sub(15);
        let lines = lines
            .into_iter()
            .skip(skip)
            .map(|(entry, points)| (self.format(entry), points))
            .collect();
        Some((objective.display.clone(), lines))
    }
}

#[derive(Clone, Debug)]
pub struct TabEntry {
    pub name: String,
    pub game_mode: i32,
    pub ping: i32,
    /// Formatted display name, when the server set one.
    pub display: Option<String>,
}

#[derive(Clone, Default)]
pub struct TabList {
    pub entries: HashMap<u128, TabEntry>,
    pub header: Option<String>,
    pub footer: Option<String>,
}

impl TabList {
    /// S38 updates; display names arrive formatted already.
    pub fn apply(&mut self, updates: &[Update], format: &dyn Fn(&str) -> String) {
        for update in updates {
            match update {
                Update::Add {
                    uuid,
                    profile,
                    game_mode,
                    ping,
                    display,
                } => {
                    if self.entries.len() < MAX_TAB || self.entries.contains_key(uuid) {
                        self.entries.insert(
                            *uuid,
                            TabEntry {
                                name: profile.name.clone(),
                                game_mode: *game_mode,
                                ping: *ping,
                                display: display.as_deref().map(format),
                            },
                        );
                    }
                }
                Update::GameMode(uuid, mode) => {
                    if let Some(entry) = self.entries.get_mut(uuid) {
                        entry.game_mode = *mode;
                    }
                }
                Update::Latency(uuid, ping) => {
                    if let Some(entry) = self.entries.get_mut(uuid) {
                        entry.ping = *ping;
                    }
                }
                Update::DisplayName(uuid, display) => {
                    if let Some(entry) = self.entries.get_mut(uuid) {
                        entry.display = display.as_deref().map(format);
                    }
                }
                Update::Remove(uuid) => {
                    self.entries.remove(uuid);
                }
            }
        }
    }

    /// `GuiPlayerTabOverlay`'s order: players before spectators, then team
    /// name, then player name.
    pub fn sorted<'a>(&'a self, scoreboard: &Scoreboard) -> Vec<(u128, &'a TabEntry)> {
        let mut entries: Vec<(u128, &TabEntry)> = self
            .entries
            .iter()
            .map(|(uuid, entry)| (*uuid, entry))
            .collect();
        entries.sort_by(|a, b| {
            let team = |entry: &TabEntry| {
                scoreboard
                    .team_of(&entry.name)
                    .map_or(String::new(), |(name, _)| name.to_owned())
            };
            (a.1.game_mode == 3)
                .cmp(&(b.1.game_mode == 3))
                .then_with(|| team(a.1).cmp(&team(b.1)))
                .then_with(|| a.1.name.cmp(&b.1.name))
        });
        entries
    }

    /// `GuiPlayerTabOverlay.getPlayerName`.
    pub fn name(entry: &TabEntry, scoreboard: &Scoreboard) -> String {
        entry
            .display
            .clone()
            .unwrap_or_else(|| scoreboard.format(&entry.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol_47::player_list::Profile;

    fn team(name: &str, prefix: &str, color: i8, players: &[&str]) -> Team {
        Team::Create {
            name: name.into(),
            info: TeamInfo {
                prefix: prefix.into(),
                color,
                ..Default::default()
            },
            players: players.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn sidebar_sorts_formats_and_prefers_the_team_colour_slot() {
        let mut board = Scoreboard::default();
        for name in ["main", "red"] {
            board.objective(Objective::Set {
                name: name.into(),
                display: format!("§e{name}"),
                hearts: false,
            });
        }
        board.display(1, "main".into());
        for (entry, value) in [("b", 2), ("a", 2), ("#hidden", 9), ("§1", 5)] {
            board.score(Score::Set {
                entry: entry.into(),
                objective: "main".into(),
                value,
            });
        }
        board.team(team("lines", "§aLine ", -1, &["§1"]));
        let (title, lines) = board.sidebar("Steve").unwrap();
        assert_eq!(title, "§emain");
        assert_eq!(
            lines,
            [("b".into(), 2), ("a".into(), 2), ("§aLine §1".into(), 5)]
        );
        // A red team member sees the red sidebar slot.
        board.team(team("reds", "§c", 12, &["Steve"]));
        board.display(3 + 12, "red".into());
        assert_eq!(board.sidebar("Steve").unwrap().0, "§ered");
        board.objective(Objective::Remove { name: "red".into() });
        assert_eq!(board.sidebar("Steve").unwrap().0, "§emain");
        // Removing a holder without an objective clears it everywhere.
        board.score(Score::Remove {
            entry: "a".into(),
            objective: String::new(),
        });
        assert_eq!(board.sidebar("Steve").unwrap().1.len(), 2);
        // Unknown objectives never collect scores.
        board.score(Score::Set {
            entry: "x".into(),
            objective: "nope".into(),
            value: 1,
        });
        assert_eq!(board.points("nope", "x"), None);
    }

    #[test]
    fn teams_move_members_and_tab_entries_sort_spectators_last() {
        let mut board = Scoreboard::default();
        board.team(team("a", "§9", 9, &["Alex"]));
        board.team(team("b", "§c", 12, &["Alex", "Steve"]));
        assert_eq!(board.format("Alex"), "§cAlex");
        board.team(Team::Leave {
            name: "a".into(),
            players: vec!["Steve".into()],
        });
        assert_eq!(board.team_of("Steve").unwrap().0, "b");
        board.team(Team::Remove { name: "b".into() });
        assert!(board.team_of("Alex").is_none());

        let add = |uuid: u128, name: &str, game_mode: i32| Update::Add {
            uuid,
            profile: Profile {
                name: name.into(),
                properties: Vec::new(),
            },
            game_mode,
            ping: 40,
            display: None,
        };
        let mut tab = TabList::default();
        tab.apply(
            &[add(1, "Zed", 3), add(2, "Bob", 0), add(3, "Amy", 0)],
            &|json| json.to_owned(),
        );
        tab.apply(&[Update::DisplayName(2, Some("§6Bob".into()))], &|j| {
            j.to_owned()
        });
        let order: Vec<&str> = tab
            .sorted(&board)
            .iter()
            .map(|(_, e)| e.name.as_str())
            .collect();
        assert_eq!(order, ["Amy", "Bob", "Zed"]);
        assert_eq!(TabList::name(&tab.entries[&2], &board), "§6Bob");
        tab.apply(&[Update::Remove(1)], &|j| j.to_owned());
        assert_eq!(tab.entries.len(), 2);
    }
}
