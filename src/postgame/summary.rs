//! What a finished match left to look back on, as the screen reads it.
//!
//! The facts are the match metadata file's (`bota_replay::meta`: what each seat held and had
//! earned every game minute, the kills, the purchases, the final items, the end), written by
//! the game server's recorder or worked out from the match's replay. What they mean here is this
//! client's reading: a rate is a total over the minutes, an advantage is one side's total less
//! the other's, and a summary without a file is the final numbers every client is sent.

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
    /// Gold earned a minute of game time.
    pub gpm: i32,
    /// Experience earned a minute of game time.
    pub xpm: i32,
    pub hero_damage: Option<i32>,
    pub tower_damage: Option<i32>,
    pub healing: Option<i32>,
    /// The inventory then the backpack; empty for a summary of the final numbers alone.
    pub items: [Option<ItemId>; BAG],
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
}

fn team_of(side: u8) -> Team {
    if side == 0 { Team::Radiant } else { Team::Dire }
}

fn to_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

fn per_minute(total: i64, minutes: i64) -> i32 {
    to_i32(total / minutes)
}

fn value(stats: &[(u16, i64)], id: u16) -> Option<i64> {
    stats.iter().find(|(s, _)| *s == id).map(|(_, v)| *v)
}

/// One seat's samples, in order.
struct Seat<'a> {
    slot: u8,
    team: u8,
    hero: u16,
    samples: Vec<&'a [(u16, i64)]>,
    items: Option<&'a [Option<u16>]>,
}

fn seats(meta: &Meta) -> Vec<Seat<'_>> {
    let mut seats: Vec<Seat> = Vec::new();
    for record in &meta.records {
        match record {
            MetaRecord::Seat { slot, team, hero } => seats.push(Seat {
                slot: *slot,
                team: *team,
                hero: *hero,
                samples: Vec::new(),
                items: None,
            }),
            MetaRecord::Sample { slot, stats, .. } => {
                if let Some(seat) = seats.iter_mut().find(|s| s.slot == *slot) {
                    seat.samples.push(stats);
                }
            }
            MetaRecord::Inventory { slot, items, .. } => {
                if let Some(seat) = seats.iter_mut().find(|s| s.slot == *slot) {
                    seat.items = Some(items);
                }
            }
            _ => {}
        }
    }
    seats
}

impl MatchSummary {
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
        let seconds = last_tick.saturating_sub(meta.header.pregame_ticks) / tick_rate;
        let minutes = (i64::from(seconds) / 60).max(1);
        let mut rows: Vec<Row> = seats.iter().filter_map(|seat| row(seat, minutes)).collect();
        rows.sort_by_key(|row| (row.team == Team::Dire, row.slot));
        Some(MatchSummary {
            detail: Detail::Meta,
            winner: end.map(|(_, winner)| winner.map_or(Team::Neutral, team_of)),
            seconds,
            rows,
            points: points(&seats),
        })
    }

    /// What the final numbers every client is sent tell, for a match with no file to read:
    /// no items, no graphs.
    pub fn from_stats(winner: Team, stats: &MatchStats, players: &[PlayerView]) -> MatchSummary {
        let seconds = stats
            .duration
            .saturating_sub(bota_core::rules::PREGAME_TICKS)
            / TICKS_PER_SECOND;
        let minutes = (i64::from(seconds) / 60).max(1);
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
                    gpm: per_minute(i64::from(seat.net_worth), minutes),
                    xpm: per_minute(i64::from(player.xp), minutes),
                    hero_damage: Some(seat.hero_damage),
                    tower_damage: Some(seat.structure_damage),
                    healing: None,
                    items: [None; BAG],
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
        }
    }
}

fn row(seat: &Seat, minutes: i64) -> Option<Row> {
    let last = *seat.samples.last()?;
    let get = |id| value(last, id);
    let count = |id| u16::try_from(get(id).unwrap_or(0)).unwrap_or(u16::MAX);
    let earned = get(stat::GOLD_EARNED)
        .or_else(|| get(stat::NET_WORTH))
        .unwrap_or(0);
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
        gpm: per_minute(earned, minutes),
        xpm: per_minute(get(stat::XP).unwrap_or(0), minutes),
        hero_damage: get(stat::HERO_DAMAGE).map(to_i32),
        tower_damage: get(stat::STRUCTURE_DAMAGE).map(to_i32),
        healing: get(stat::HEALING).map(to_i32),
        items,
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
                let Some(stats) = seat.samples.get(minute) else {
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
