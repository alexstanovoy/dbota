//! The history and hero records of this machine, projected from its local replay library.
//! A spectator file with no recorded local owner may be listed but cannot count as "your" win.
use bota_proto::HeroId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResultKind {
    Won,
    Lost,
    Draw,
    Undecided,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub file: String,
    pub when: u64,
    pub mode: String,
    pub hero: Option<HeroId>,
    pub result: ResultKind,
    pub seconds: Option<u32>,
    pub kda: Option<(u16, u16, u16)>,
    pub net_worth: Option<i32>,
    pub watchable: bool,
    pub details_available: bool,
}

impl Match {
    pub fn length(&self) -> String {
        self.seconds
            .map_or_else(|| "—".to_owned(), |s| format!("{}:{:02}", s / 60, s % 60))
    }
    pub fn kda_text(&self) -> String {
        self.kda
            .map_or_else(|| "—".to_owned(), |(k, d, a)| format!("{k} / {d} / {a}"))
    }
    pub fn hero_name(&self) -> &'static str {
        self.hero
            .and_then(bota_core::hero_def)
            .map_or("Unknown player", |hero| hero.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeroRecord {
    pub hero: HeroId,
    pub games: usize,
    pub wins: usize,
}

impl HeroRecord {
    pub fn percentage(&self) -> usize {
        (self.wins * 100 + self.games / 2)
            .checked_div(self.games)
            .unwrap_or(0)
    }
}

pub fn heroes(matches: &[Match]) -> Vec<HeroRecord> {
    let mut heroes: Vec<HeroRecord> = Vec::new();
    for game in matches.iter().filter(|game| {
        matches!(
            game.result,
            ResultKind::Won | ResultKind::Lost | ResultKind::Draw
        )
    }) {
        let Some(hero) = game.hero else {
            continue;
        };
        let index = heroes
            .iter()
            .position(|row| row.hero == hero)
            .unwrap_or_else(|| {
                heroes.push(HeroRecord {
                    hero,
                    games: 0,
                    wins: 0,
                });
                heroes.len() - 1
            });
        heroes[index].games += 1;
        heroes[index].wins += usize::from(game.result == ResultKind::Won);
    }
    heroes.sort_by_key(|hero| (std::cmp::Reverse(hero.games), hero.hero.0));
    heroes
}

pub fn mode_name(mode: &str) -> &str {
    match mode {
        "solo_mid" => "Solo Mid",
        "all_pick" | "standard" => "All Pick",
        "local" => "Local lobby",
        "file" => "Replay file",
        other => other,
    }
}
