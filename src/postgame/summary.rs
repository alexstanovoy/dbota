//! What a finished match left to look back on, as the screen reads it.
//!
//! The facts are the match metadata file's (`bota_replay::meta`: what each seat held and had
//! earned every game minute, the kills, the purchases, the final items, the end), written by
//! the game server's recorder or worked out from the match's replay. What they mean here is this
//! client's reading: a rate is a total over the minutes, an advantage is one side's total less
//! the other's, and a summary without a file is the final numbers every client is sent.

pub use super::recording::StructureFall;
use bota_proto::{HeroId, ItemId, MatchStats, PlayerView, SlotId, Team};
use bota_replay::meta::{Meta, MetaRecord, stat};

/// Inventory and backpack slots a row shows.
pub const BAG: usize = bota_proto::limits::HERO_ITEM_SLOTS;

const TICKS_PER_SECOND: u32 = bota_core::rules::TICKS_PER_SECOND;

/// One seat's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub slot: SlotId,
    pub team: Team,
    pub hero: HeroId,
    pub level: u8,
    pub kills: u16,
    pub deaths: u16,
    pub assists: u16,
    pub last_hits: Option<u16>,
    pub denies: Option<u16>,
    /// Gold in hand plus the value of what is carried and stashed. Absent for a summary of the
    /// final numbers alone, which count the gold earned only.
    pub net_worth: Option<i32>,
    /// Gold earned a minute of game time, absent when earned gold was not recorded.
    pub gpm: Option<i32>,
    /// Experience earned a minute, absent when total experience was not recorded.
    pub xpm: Option<i32>,
    pub hero_damage: Option<i32>,
    pub tower_damage: Option<i32>,
    pub healing: Option<i32>,
    /// The inventory then the backpack; empty for a summary of the final numbers alone.
    pub items: [Option<ItemId>; BAG],
    /// Optional recording facts retained for the match-results consumer. Old files omit them.
    pub name: Option<String>,
    pub bot: Option<bool>,
    pub left_at: Option<u32>,
}

impl Row {
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("Player {}", usize::from(self.slot.0) + 1))
    }

    pub fn hero_name<'a>(&self, texts: &'a crate::dota_text::Texts) -> &'a str {
        bota_core::hero_def(self.hero).map_or("Unknown hero", |hero| texts.name(hero.key))
    }
}

/// What a side was ahead by at one game minute: the Radiant's total less the Dire's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub net_worth: i32,
    pub xp: i32,
}

/// How much a summary can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// Worked out from the match's metadata file.
    Meta,
    /// The final numbers the match sent alone: no items, no graphs.
    Stats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchSummary {
    pub detail: Detail,
    /// Absent when the file ends before the match was decided.
    pub winner: Option<Team>,
    /// Seconds of game time the match lasted.
    pub seconds: u32,
    /// Radiant's seats then Dire's, each in seat order.
    pub rows: Vec<Row>,
    /// One for the start of the game and one for every whole game minute after, in order.
    pub points: Vec<Point>,
    /// Building falls, with optional location companions in newer recordings.
    pub structures: Vec<StructureFall>,
    pub recording: Option<std::sync::Arc<super::data::Recording>>,
}

fn team_of(side: u8) -> Team {
    if side == 0 { Team::Radiant } else { Team::Dire }
}

fn to_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

fn per_minute(total: i64, seconds: u32) -> i32 {
    to_i32(total.saturating_mul(60) / i64::from(seconds.max(1)))
}

fn value(stats: &[(u16, i64)], id: u16) -> Option<i64> {
    stats.iter().find(|(s, _)| *s == id).map(|(_, v)| *v)
}

/// One seat's samples, in order.
struct Seat<'a> {
    slot: u8,
    team: u8,
    hero: u16,
    samples: Vec<(u32, &'a [(u16, i64)])>,
    items: Option<&'a [Option<u16>]>,
}

/// The seats of a file, as many as a match has, each with as many samples as the longest match
/// has (one a minute from minute 0, and the end): the file is someone else's (a replay's tail,
/// or the service's), and its records are only bounded by its length.
fn seats(meta: &Meta) -> Vec<Seat<'_>> {
    let most_samples = bota_replay::limits::MAX_MINUTES as usize + 2;
    let mut seats: Vec<Seat> = Vec::new();
    let end_tick = meta.records.iter().find_map(|record| match record {
        MetaRecord::End { tick, .. } => Some(*tick),
        _ => None,
    });
    for record in &meta.records {
        match record {
            MetaRecord::Seat { slot, team, hero }
                if seats.len() < bota_proto::limits::MAX_SEATS
                    && seats.iter().all(|s| s.slot != *slot) =>
            {
                seats.push(Seat {
                    slot: *slot,
                    team: *team,
                    hero: *hero,
                    samples: Vec::new(),
                    items: None,
                });
            }
            MetaRecord::Sample { tick, slot, stats } if end_tick.is_none_or(|end| *tick <= end) => {
                if let Some(seat) = seats.iter_mut().find(|s| s.slot == *slot)
                    && seat.samples.len() < most_samples
                {
                    seat.samples.push((*tick, stats));
                }
            }
            MetaRecord::Inventory { tick, slot, items }
                if end_tick.is_none_or(|end| *tick <= end) =>
            {
                if let Some(seat) = seats.iter_mut().find(|s| s.slot == *slot) {
                    seat.items = Some(items);
                }
            }
            _ => {}
        }
    }
    for seat in &mut seats {
        seat.samples.sort_by_key(|(tick, _)| *tick);
        seat.samples.reverse();
        seat.samples.dedup_by_key(|(tick, _)| *tick);
        seat.samples.reverse();
    }
    seats
}

impl MatchSummary {
    pub fn comparisons_known(&self) -> bool {
        !self.rows.is_empty()
            && (self.detail == Detail::Stats
                || self.recording.as_ref().is_some_and(|recording| {
                    recording.roster_complete && recording.samples.len() == self.rows.len()
                }))
    }
    pub fn team_total(&self, team: Team, id: u16) -> Option<i64> {
        if !self.comparisons_known() {
            return None;
        }
        self.rows
            .iter()
            .filter(|row| row.team == team)
            .try_fold(0_i64, |total, row| {
                let value = if let Some(recording) = &self.recording {
                    recording.value(row.slot, id, self.seconds)?
                } else {
                    match id {
                        stat::KILLS => i64::from(row.kills),
                        stat::NET_WORTH => i64::from(row.net_worth?),
                        _ => return None,
                    }
                };
                Some(total.saturating_add(value))
            })
    }

    /// What a metadata file tells. None when it names no seat or holds no sample of one.
    pub fn from_meta(meta: &Meta) -> Option<MatchSummary> {
        let seats = seats(meta);
        if seats.iter().all(|s| s.samples.is_empty()) {
            return None;
        }
        let end = meta.records.iter().find_map(|r| match r {
            MetaRecord::End { tick, winner } => Some((*tick, *winner)),
            _ => None,
        });
        let last_tick = meta
            .records
            .iter()
            .filter_map(|r| match r {
                MetaRecord::Sample { tick, .. } => Some(*tick),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let tick_rate = u32::from(meta.header.tick_rate.max(1));
        let last_tick = end.map_or(last_tick, |(tick, _)| tick);
        let seconds = (last_tick.saturating_sub(meta.header.pregame_ticks) / tick_rate)
            .min(super::data::MAX_SECONDS);
        let mut rows: Vec<Row> = seats.iter().filter_map(|seat| row(seat, seconds)).collect();
        rows.sort_by_key(|row| (row.team == Team::Dire, row.slot));
        super::recording::enrich_rows(meta, &mut rows);
        let recording = super::data::Recording::from_meta(meta, rows.iter().map(|row| row.slot));
        Some(MatchSummary {
            detail: Detail::Meta,
            winner: end.map(|(_, winner)| winner.map_or(Team::Neutral, team_of)),
            seconds,
            rows,
            points: points(&seats),
            structures: super::recording::structures(meta),
            recording: Some(std::sync::Arc::new(recording)),
        })
    }

    /// What the final numbers every client is sent tell, for a match with no file to read:
    /// no items, no graphs.
    pub fn from_stats(
        winner: Team,
        stats: &MatchStats,
        players: &[PlayerView],
        pregame_ticks: u32,
    ) -> MatchSummary {
        let seconds = stats.duration.saturating_sub(pregame_ticks) / TICKS_PER_SECOND;
        let mut rows: Vec<Row> = stats
            .slots
            .iter()
            .filter_map(|seat| {
                let player = players.iter().find(|p| p.slot == seat.slot)?;
                if player.team == Team::Neutral {
                    return None;
                }
                Some(Row {
                    slot: seat.slot,
                    team: player.team,
                    hero: player.hero,
                    level: player.level,
                    kills: seat.kills,
                    deaths: seat.deaths,
                    assists: seat.assists,
                    last_hits: Some(seat.last_hits),
                    denies: Some(seat.denies),
                    net_worth: None,
                    gpm: Some(per_minute(i64::from(seat.net_worth), seconds)),
                    xpm: Some(per_minute(i64::from(player.xp), seconds)),
                    hero_damage: Some(seat.hero_damage),
                    tower_damage: Some(seat.structure_damage),
                    healing: None,
                    items: [None; BAG],
                    name: None,
                    bot: None,
                    left_at: stats
                        .leaves
                        .iter()
                        .filter(|left| left.slot == seat.slot)
                        .map(|left| left.tick)
                        .min(),
                })
            })
            .collect();
        rows.sort_by_key(|row| (row.team == Team::Dire, row.slot));
        MatchSummary {
            detail: Detail::Stats,
            winner: Some(winner),
            seconds,
            rows,
            points: Vec::new(),
            structures: Vec::new(),
            recording: None,
        }
    }
}

fn row(seat: &Seat, seconds: u32) -> Option<Row> {
    let last = seat.samples.last()?.1;
    let get = |id| value(last, id);
    let count = |id| u16::try_from(get(id).unwrap_or(0)).unwrap_or(u16::MAX);
    let earned = get(stat::GOLD_EARNED);
    let mut items = [None; BAG];
    for (to, held) in items.iter_mut().zip(seat.items.unwrap_or_default()) {
        *to = held.map(ItemId);
    }
    Some(Row {
        slot: SlotId(seat.slot),
        team: team_of(seat.team),
        hero: HeroId(seat.hero),
        level: u8::try_from(get(stat::LEVEL).unwrap_or(1)).unwrap_or(u8::MAX),
        kills: count(stat::KILLS),
        deaths: count(stat::DEATHS),
        assists: count(stat::ASSISTS),
        last_hits: get(stat::LAST_HITS).map(|_| count(stat::LAST_HITS)),
        denies: get(stat::DENIES).map(|_| count(stat::DENIES)),
        net_worth: get(stat::NET_WORTH).map(to_i32),
        gpm: earned.map(|earned| per_minute(earned, seconds)),
        xpm: get(stat::XP).map(|xp| per_minute(xp, seconds)),
        hero_damage: get(stat::HERO_DAMAGE).map(to_i32),
        tower_damage: get(stat::STRUCTURE_DAMAGE).map(to_i32),
        healing: get(stat::HEALING).map(to_i32),
        items,
        name: None,
        bot: None,
        left_at: None,
    })
}

/// A point for every minute sample: the last sample of a seat is the end of the match, not a
/// minute.
fn points(seats: &[Seat]) -> Vec<Point> {
    let minutes = seats
        .iter()
        .map(|s| s.samples.len().saturating_sub(1))
        .max()
        .unwrap_or(0);
    (0..minutes)
        .map(|minute| {
            let mut ahead = Point {
                net_worth: 0,
                xp: 0,
            };
            for seat in seats {
                let Some((_, stats)) = seat.samples.get(minute) else {
                    continue;
                };
                let sign = if seat.team == 0 { 1 } else { -1 };
                ahead.net_worth = ahead
                    .net_worth
                    .saturating_add(sign * to_i32(value(stats, stat::NET_WORTH).unwrap_or(0)));
                ahead.xp = ahead
                    .xp
                    .saturating_add(sign * to_i32(value(stats, stat::XP).unwrap_or(0)));
            }
            ahead
        })
        .collect()
}
