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
    allow(dead_code, reason = "a browser reads no replay files")
)]
pub enum Summary {
    /// Still being read.
    Reading,
    /// Who played what, and how it ended.
    Read {
        /// `Player (Shadow Fiend) vs Bot (Shadow Fiend)`.
        sides: Sides,
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
#[cfg_attr(
    target_arch = "wasm32",
    allow(dead_code, reason = "a browser reads no replay files")
)]
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
    let mut aborted = None;
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
                ServerMsg::MatchAborted { reason, .. } => aborted = Some(reason),
                _ => {}
            }
        }
    }
    if let Some(err) = player.error()
        && last_tick.is_none()
    {
        // A replay plays only in the build of bota that recorded it.
        let other_version = err
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<bota_client_core::ReplayRefusal>())
            .is_some_and(|refusal| {
                matches!(
                    refusal,
                    bota_client_core::ReplayRefusal::OtherVersion { .. }
                )
            });
        let why = if other_version {
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
        _ if aborted.is_some() => format!(
            "ended before its end ({}), {}",
            aborted.map_or("", |reason| crate::main_menu::results::ended_by_word(
                reason
            )),
            clock(last_tick)
        ),
        Some((Team::Radiant, ticks)) => format!("Radiant victory, {}", clock(ticks)),
        Some((Team::Dire, ticks)) => format!("Dire victory, {}", clock(ticks)),
        Some((Team::Neutral, ticks)) => format!("draw, {}", clock(ticks)),
        None => format!("unfinished, {}", clock(last_tick)),
    };
    // The lobby's names where the replay has them; the match's picks else.
    let named = seats.iter().any(|s| !s.name.is_empty());
    Summary::Read {
        sides: Sides {
            players: String::new(),
            heroes: if named {
                seats
                    .iter()
                    .map(|seat| {
                        (
                            seat.team,
                            (!seat.name.is_empty())
                                .then(|| super::text::line(&seat.name, super::text::NAME)),
                            seat.hero,
                        )
                    })
                    .collect()
            } else {
                picks
                    .iter()
                    .map(|pick| (pick.team, None, Some(pick.hero)))
                    .collect()
            },
        },
        outcome,
    }
}

/// Replay names stay structured until rendering, so the current installed language wins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sides {
    players: String,
    heroes: Vec<(bota_proto::Team, Option<String>, Option<bota_proto::HeroId>)>,
}

impl From<String> for Sides {
    fn from(players: String) -> Self {
        Self {
            players,
            heroes: Vec::new(),
        }
    }
}

impl Sides {
    pub fn text(&self, texts: &crate::dota_text::Texts) -> String {
        if self.heroes.is_empty() {
            return self.players.clone();
        }
        let side = |team| {
            let names: Vec<_> = self
                .heroes
                .iter()
                .filter(|(t, _, _)| *t == team)
                .map(|(_, name, hero)| match (name, hero) {
                    (Some(name), Some(hero)) => format!("{name} ({})", texts.hero_name(*hero)),
                    (Some(name), None) => name.clone(),
                    (None, Some(hero)) => texts.hero_name(*hero).to_owned(),
                    (None, None) => "open".to_owned(),
                })
                .collect();
            if names.is_empty() {
                "nobody".to_owned()
            } else {
                names.join(", ")
            }
        };
        format!(
            "{} vs {}",
            side(bota_proto::Team::Radiant),
            side(bota_proto::Team::Dire)
        )
    }
}

/// Replays read so far, by path and stamp; the reading thread's answers.
#[cfg_attr(
    target_arch = "wasm32",
    allow(dead_code, reason = "a browser reads no replay files")
)]
#[derive(Default)]
pub struct Shelf {
    known: std::collections::HashMap<PathBuf, (Stamp, Summary)>,
    #[cfg(not(target_arch = "wasm32"))]
    asked: Option<std::sync::mpsc::Sender<(PathBuf, Stamp)>>,
    #[cfg(not(target_arch = "wasm32"))]
    told: Option<std::sync::mpsc::Receiver<(PathBuf, Stamp, Summary)>>,
}

#[cfg_attr(
    target_arch = "wasm32",
    allow(dead_code, reason = "a browser reads no replay files")
)]
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

/// When a replay was played, in the page's time zone: `2026-10-24 15:06`.
#[cfg(target_arch = "wasm32")]
pub fn when(time: std::time::SystemTime) -> String {
    let Ok(since) = time.duration_since(std::time::UNIX_EPOCH) else {
        return "unknown date".to_owned();
    };
    let at = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(since.as_millis() as f64));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        at.get_full_year(),
        at.get_month() + 1,
        at.get_date(),
        at.get_hours(),
        at.get_minutes()
    )
}

#[cfg(test)]
mod name_tests {
    #[test]
    fn replay_hero_identities_are_named_by_the_current_install() {
        let root = crate::test_support::scratch_path("replay_list_names");
        std::fs::create_dir_all(root.join("text")).expect("fixture directory");
        std::fs::write(
            root.join("text/labels.json"),
            r#"{"npc_dota_hero_nevermore:n":"Installed Fiend"}"#,
        )
        .expect("fixture names");
        let texts = crate::dota_text::Texts::load(&root);
        let sides = super::Sides {
            players: String::new(),
            heroes: vec![
                (
                    bota_proto::Team::Radiant,
                    Some("Reader".to_owned()),
                    Some(bota_proto::HeroId::SHADOW_FIEND),
                ),
                (
                    bota_proto::Team::Dire,
                    None,
                    Some(bota_proto::HeroId::PUDGE),
                ),
            ],
        };
        assert_eq!(sides.text(&texts), "Reader (Installed Fiend) vs Pudge");
        assert_eq!(
            sides.text(&crate::dota_text::Texts::default()),
            "Reader (Shadow Fiend) vs Pudge"
        );
        std::fs::remove_dir_all(root).expect("remove fixture");
    }
}
