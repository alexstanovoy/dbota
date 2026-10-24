//! What a replay file holds, for the REPLAYS page: when it was played, by
//! whom with which heroes, how long, and how it ended, as Dota's replay list
//! tells a match. Read through the replay itself (on a thread of its own:
//! a replay is read whole to learn its end), and kept while the file stays
//! the same.

use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
use bota_client_core::ReplayPlayer;
#[cfg(not(target_arch = "wasm32"))]
use bota_proto::{ServerMsg, Team};

/// What the list says of a replay.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    target_arch = "wasm32",
    expect(dead_code, reason = "a browser lists no replay files to read")
)]
pub enum Summary {
    /// Still being read.
    Reading,
    /// Who played what, and how it ended.
    Read {
        /// `Player (Shadow Fiend) vs Bot (Shadow Fiend)`.
        sides: String,
        /// `Dire victory, 1:43`, or `unfinished, 0:52`.
        outcome: String,
    },
    /// It cannot be played: why.
    Broken(String),
}

/// A file as last seen: its size and time, which a change of either reads
/// anew.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stamp {
    pub len: u64,
    pub modified: std::time::SystemTime,
}

/// Why a replay with no match in it has nothing to watch.
const NEVER_STARTED: &str = "its match never started";

#[cfg(not(target_arch = "wasm32"))]
/// Records read from one replay at most: a bound on a hostile file's work.
const MAX_BATCHES: usize = 1 << 20;

#[cfg(not(target_arch = "wasm32"))]
/// Reads the replay at `path` to its end.
pub fn read(path: &Path) -> Summary {
    let mut player = match ReplayPlayer::load(path) {
        Ok(player) => player,
        Err(err) => return Summary::Broken(format!("cannot be opened: {err}")),
    };
    // No waiting on the recorded clock: everything is due at once.
    player.advance_ticks(f64::from(u32::MAX));
    let mut seats: Vec<bota_proto::LobbySlot> = Vec::new();
    let mut picks: Vec<bota_proto::Pick> = Vec::new();
    let mut tick_rate = 30u32;
    let mut pregame = 0u32;
    let mut last_tick = None;
    let mut over = None;
    for _ in 0..MAX_BATCHES {
        let batch = player.poll(0.0);
        if batch.is_empty() && (player.finished() || player.error().is_some()) {
            break;
        }
        for msg in batch {
            match msg {
                ServerMsg::LobbyState { slots, .. } => seats = slots,
                ServerMsg::MatchStart { info } => {
                    tick_rate = u32::from(info.tick_rate).max(1);
                    pregame = info.pregame_ticks;
                    picks = info.picks;
                }
                ServerMsg::Snapshot { view } => last_tick = Some(view.tick),
                ServerMsg::MatchOver { winner, stats } => over = Some((winner, stats.duration)),
                _ => {}
            }
        }
    }
    if let Some(err) = player.error()
        && last_tick.is_none()
    {
        // A replay plays only in the build of bota that recorded it.
        let why = if err.to_string().contains("wire fingerprint") {
            "recorded by another version of the game; only that version plays it"
        } else {
            "damaged, or not a bota replay"
        };
        return Summary::Broken(why.to_owned());
    }
    let Some(last_tick) = last_tick else {
        return Summary::Broken(NEVER_STARTED.to_owned());
    };
    let clock = |ticks: u32| {
        let secs = ticks.saturating_sub(pregame) / tick_rate;
        format!("{}:{:02}", secs / 60, secs % 60)
    };
    let outcome = match over {
        Some((Team::Radiant, ticks)) => format!("Radiant victory, {}", clock(ticks)),
        Some((Team::Dire, ticks)) => format!("Dire victory, {}", clock(ticks)),
        Some((Team::Neutral, ticks)) => format!("draw, {}", clock(ticks)),
        None => format!("unfinished, {}", clock(last_tick)),
    };
    // The lobby's names where the replay has them; the match's picks else.
    let named = seats.iter().any(|s| !s.name.is_empty());
    Summary::Read {
        sides: if named { sides(&seats) } else { heroes(&picks) },
        outcome,
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// `Radiant names vs Dire names`, each with its hero.
fn sides(seats: &[bota_proto::LobbySlot]) -> String {
    let side = |team: Team| {
        let names: Vec<String> = seats
            .iter()
            .filter(|s| s.team == team)
            .map(|s| {
                let name = if s.name.is_empty() {
                    "open"
                } else {
                    s.name.as_str()
                };
                let name = super::text::line(name, super::text::NAME);
                match s.hero.and_then(bota_core::hero_def) {
                    Some(hero) => format!("{name} ({})", hero.name),
                    None => name,
                }
            })
            .collect();
        if names.is_empty() {
            "nobody".to_owned()
        } else {
            names.join(", ")
        }
    };
    format!("{} vs {}", side(Team::Radiant), side(Team::Dire))
}

/// `Radiant heroes vs Dire heroes`.
#[cfg(not(target_arch = "wasm32"))]
fn heroes(picks: &[bota_proto::Pick]) -> String {
    let side = |team: Team| {
        let names: Vec<&str> = picks
            .iter()
            .filter(|p| p.team == team)
            .map(|p| bota_core::hero_def(p.hero).map_or("a hero", |h| h.name))
            .collect();
        if names.is_empty() {
            "nobody".to_owned()
        } else {
            names.join(", ")
        }
    };
    format!("{} vs {}", side(Team::Radiant), side(Team::Dire))
}

/// Replays read so far, by path and stamp; the reading thread's answers.
#[derive(Default)]
pub struct Shelf {
    known: std::collections::HashMap<PathBuf, (Stamp, Summary)>,
    #[cfg(not(target_arch = "wasm32"))]
    asked: Option<std::sync::mpsc::Sender<(PathBuf, Stamp)>>,
    #[cfg(not(target_arch = "wasm32"))]
    told: Option<std::sync::mpsc::Receiver<(PathBuf, Stamp, Summary)>>,
}

impl Shelf {
    /// What is known of `path` as it is now; a file not read yet (or changed
    /// since) is asked for and shows as being read.
    pub fn summary(&mut self, path: &Path, stamp: Stamp) -> Summary {
        if let Some((seen, summary)) = self.known.get(path)
            && *seen == stamp
        {
            return summary.clone();
        }
        // An empty file is a lobby left before its match: nothing to read.
        if stamp.len == 0 {
            let empty = Summary::Broken(NEVER_STARTED.to_owned());
            self.known.insert(path.to_owned(), (stamp, empty.clone()));
            return empty;
        }
        self.known
            .insert(path.to_owned(), (stamp, Summary::Reading));
        self.ask(path, stamp);
        Summary::Reading
    }

    /// What is known of `path` as it is now, asking nothing.
    pub fn peek(&self, path: &Path, stamp: Stamp) -> Summary {
        match self.known.get(path) {
            Some((seen, summary)) if *seen == stamp => summary.clone(),
            _ => Summary::Reading,
        }
    }

    /// Takes in what the reading thread found; whether anything came.
    pub fn take(&mut self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(told) = &self.told else {
                return false;
            };
            let mut any = false;
            while let Ok((path, stamp, summary)) = told.try_recv() {
                self.known.insert(path, (stamp, summary));
                any = true;
            }
            any
        }
        #[cfg(target_arch = "wasm32")]
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ask(&mut self, path: &Path, stamp: Stamp) {
        if self.asked.is_none() {
            let (ask, asked) = std::sync::mpsc::channel::<(PathBuf, Stamp)>();
            let (tell, told) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("dbota-replays".to_owned())
                .spawn(move || {
                    for (path, stamp) in asked {
                        let summary = read(&path);
                        if tell.send((path, stamp, summary)).is_err() {
                            return;
                        }
                    }
                });
            if spawned.is_err() {
                return;
            }
            self.asked = Some(ask);
            self.told = Some(told);
        }
        if let Some(ask) = &self.asked {
            let _ = ask.send((path.to_owned(), stamp));
        }
    }

    /// A browser lists no replay files.
    #[cfg(target_arch = "wasm32")]
    fn ask(&mut self, _path: &Path, _stamp: Stamp) {}
}

/// When a replay was played, in this machine's time zone (the system's
/// time zone database, read by `jiff`): `2026-10-24 15:06`; in UTC, and
/// said so, where the machine names none.
#[cfg(not(target_arch = "wasm32"))]
pub fn when(time: std::time::SystemTime) -> String {
    let Ok(at) = jiff::Timestamp::try_from(time) else {
        return "unknown date".to_owned();
    };
    match jiff::tz::TimeZone::try_system() {
        Ok(zone) => at.to_zoned(zone).strftime("%Y-%m-%d %H:%M").to_string(),
        Err(_) => format!("{} UTC", at.strftime("%Y-%m-%d %H:%M")),
    }
}

/// A browser lists no replay files to date.
#[cfg(target_arch = "wasm32")]
pub fn when(_time: std::time::SystemTime) -> String {
    String::new()
}
