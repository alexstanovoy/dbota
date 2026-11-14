//! Match rows projected from locally retained records or the configured matchmaker.
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
    pub players: Vec<bota_proto::SeatIdentity>,
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
    pub fn hero_name<'a>(&self, texts: &'a crate::dota_text::Texts) -> &'a str {
        self.hero
            .and_then(bota_core::hero_def)
            .map_or("Unknown player", |hero| texts.name(hero.key))
    }
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
