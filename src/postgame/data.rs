//! Timed facts retained from the local recording; no curves are inferred from final totals.

use bota_proto::{EntityId, ItemId, SlotId};
use bota_replay::meta::{Meta, MetaRecord, Who};

use crate::kill_calls::{Fall, MULTI_KILL_SECONDS, Tracker};

const MAX_EVENTS: usize = 16_384;
pub const MAX_SECONDS: u32 = bota_replay::limits::MAX_MINUTES * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub second: u32,
    pub values: [Option<i64>; bota_replay::meta::stat::HEALING as usize + 1],
}

impl Sample {
    pub fn value(&self, stat: u16) -> Option<i64> {
        self.values.get(usize::from(stat)).copied().flatten()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeatSamples {
    pub slot: SlotId,
    pub samples: Vec<Sample>,
}

impl SeatSamples {
    pub fn at(&self, second: u32) -> Option<&Sample> {
        let index = self
            .samples
            .binary_search_by_key(&second, |sample| sample.second)
            .ok()?;
        self.samples.get(index)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kill {
    pub second: u32,
    pub victim: Option<SlotId>,
    pub killer: Option<SlotId>,
    pub gold: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Purchase {
    /// Signed so purchases before the horn retain their actual time.
    pub second: i64,
    pub slot: SlotId,
    pub item: ItemId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    pub second: u32,
    pub slot: SlotId,
    pub level: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    pub match_id: u64,
    pub profiles: Option<std::sync::Arc<bota_replay::meta::Profiles>>,
    pub map: u16,
    pub tick_rate: u32,
    pub pregame_ticks: u32,
    pub samples: Vec<SeatSamples>,
    pub kills: Vec<Kill>,
    pub purchases: Vec<Purchase>,
    pub levels: Vec<Level>,
    /// The side of each seat (0 Radiant, 1 Dire), which tells a deny from a kill.
    seat_teams: Vec<(SlotId, u8)>,
    pub limited: bool,
    pub roster_complete: bool,
    pub restricted: bool,
    end_tick: Option<u32>,
}

fn hero(who: Who) -> Option<SlotId> {
    match who {
        Who::Hero(slot) => Some(SlotId(slot)),
        Who::Other => None,
    }
}

impl Recording {
    pub fn from_meta(meta: &Meta, slots: impl Iterator<Item = SlotId>) -> Self {
        let mut recording = Self {
            match_id: meta.header.match_id,
            profiles: meta.profiles().map(std::sync::Arc::new),
            map: meta.header.map,
            tick_rate: u32::from(meta.header.tick_rate.max(1)),
            pregame_ticks: meta.header.pregame_ticks,
            samples: slots
                .take(bota_proto::limits::MAX_SEATS)
                .map(|slot| SeatSamples {
                    slot,
                    samples: Vec::new(),
                })
                .collect(),
            kills: Vec::new(),
            purchases: Vec::new(),
            levels: Vec::new(),
            seat_teams: Vec::new(),
            limited: !meta.complete || meta.damaged,
            roster_complete: true,
            restricted: false,
            end_tick: meta.records.iter().find_map(|record| match record {
                MetaRecord::End { tick, .. } => Some(*tick),
                _ => None,
            }),
        };
        recording.roster_complete = meta
            .records
            .iter()
            .filter_map(|record| match record {
                MetaRecord::Seat { slot, .. } => Some(*slot),
                _ => None,
            })
            .all(|slot| recording.has_slot(slot));
        for record in &meta.records {
            recording.push(record);
        }
        for seat in &mut recording.samples {
            seat.samples.sort_by_key(|sample| sample.second);
            // The final sample can share a minute boundary; the last recorded fact wins.
            seat.samples.reverse();
            seat.samples.dedup_by_key(|sample| sample.second);
            seat.samples.reverse();
        }
        recording.kills.sort_by_key(|event| event.second);
        recording.purchases.sort_by_key(|event| event.second);
        recording.levels.sort_by_key(|event| event.second);
        recording
    }

    pub fn restricted_to(&self, slots: impl Iterator<Item = SlotId>) -> Self {
        let slots: Vec<_> = slots.take(bota_proto::limits::MAX_SEATS).collect();
        let mut recording = self.clone();
        recording.restricted = true;
        recording.roster_complete = false;
        recording.samples.retain(|seat| slots.contains(&seat.slot));
        recording.kills.retain(|kill| {
            kill.victim.is_some_and(|slot| slots.contains(&slot))
                || kill.killer.is_some_and(|slot| slots.contains(&slot))
        });
        recording
            .purchases
            .retain(|purchase| slots.contains(&purchase.slot));
        recording.levels.retain(|level| slots.contains(&level.slot));
        recording
    }

    pub fn complete(&self) -> bool {
        self.end_tick.is_some()
    }

    pub fn second(&self, tick: u32) -> u32 {
        tick.saturating_sub(self.pregame_ticks)
            .checked_div(self.tick_rate)
            .unwrap_or(0)
            .min(MAX_SECONDS)
    }

    fn push(&mut self, record: &MetaRecord) {
        match record {
            MetaRecord::Sample { tick, slot, stats } => {
                if self.end_tick.is_some_and(|end| *tick > end) {
                    return;
                }
                let second = self.second(*tick);
                if let Some(seat) = self.samples.iter_mut().find(|seat| seat.slot.0 == *slot) {
                    if seat.samples.len() < bota_replay::limits::MAX_MINUTES as usize + 2 {
                        let mut values = [None; bota_replay::meta::stat::HEALING as usize + 1];
                        for (id, value) in stats {
                            if let Some(cell) = values.get_mut(usize::from(*id))
                                && cell.is_none()
                            {
                                *cell = Some(*value);
                            }
                        }
                        seat.samples.push(Sample { second, values });
                    } else {
                        self.limited = true;
                    }
                }
            }
            MetaRecord::Kill {
                tick,
                victim,
                killer,
                gold,
            } if self.end_tick.is_none_or(|end| *tick <= end) => {
                if self.kills.len() < MAX_EVENTS {
                    self.kills.push(Kill {
                        second: self.second(*tick),
                        victim: hero(*victim),
                        killer: killer.and_then(hero),
                        gold: *gold,
                    });
                } else {
                    self.limited = true;
                }
            }
            MetaRecord::Purchase { tick, slot, item }
                if self.has_slot(*slot) && self.end_tick.is_none_or(|end| *tick <= end) =>
            {
                if self.purchases.len() < MAX_EVENTS {
                    let second = (i64::from(*tick) - i64::from(self.pregame_ticks))
                        .div_euclid(i64::from(self.tick_rate));
                    self.purchases.push(Purchase {
                        second,
                        slot: SlotId(*slot),
                        item: ItemId(*item),
                    });
                } else {
                    self.limited = true;
                }
            }
            MetaRecord::LevelUp { tick, slot, level }
                if self.has_slot(*slot) && self.end_tick.is_none_or(|end| *tick <= end) =>
            {
                if self.levels.len() < bota_proto::limits::MAX_SEATS * 30 {
                    self.levels.push(Level {
                        second: self.second(*tick),
                        slot: SlotId(*slot),
                        level: *level,
                    });
                } else {
                    self.limited = true;
                }
            }
            MetaRecord::Seat { slot, team, .. }
                if self.seat_teams.len() < bota_proto::limits::MAX_SEATS
                    && self.team_of(SlotId(*slot)).is_none() =>
            {
                self.seat_teams.push((SlotId(*slot), *team));
            }
            _ => {}
        }
    }

    fn team_of(&self, slot: SlotId) -> Option<u8> {
        self.seat_teams
            .iter()
            .find(|(seat, _)| *seat == slot)
            .map(|(_, team)| *team)
    }

    fn has_slot(&self, slot: u8) -> bool {
        self.samples.iter().any(|seat| seat.slot.0 == slot)
    }

    pub fn value(&self, slot: SlotId, stat: u16, second: u32) -> Option<i64> {
        self.samples
            .iter()
            .find(|seat| seat.slot == slot)?
            .at(second)?
            .value(stat)
    }

    /// The most kills `slot`'s hero made without falling, counted by the rule of the live
    /// announcements ([`Tracker`]): a fall to an enemy ends the streak, a deny (a hero of its own
    /// side struck last) does not. The file does not say neutral creeps from lane creeps, so a
    /// fall to either ends it here; the live count, which is told, leaves it for neutrals.
    pub fn killstreak(&self, slot: SlotId) -> u16 {
        let window = MULTI_KILL_SECONDS.saturating_mul(self.tick_rate.max(1));
        let mut tracker = Tracker::new(window, true);
        let me = body_of(Some(slot));
        let mut best = 0;
        for kill in &self.kills {
            tracker.fall(self.fall_of(kill));
            best = best.max(tracker.streak_of(me));
        }
        best
    }

    /// What a recorded kill is to the count: credited to an enemy hero, or a deny.
    fn fall_of(&self, kill: &Kill) -> Fall {
        let denied = match (kill.killer, kill.victim) {
            (Some(killer), Some(victim)) => {
                killer == victim
                    || matches!(
                        (self.team_of(killer), self.team_of(victim)),
                        (Some(a), Some(b)) if a == b
                    )
            }
            _ => false,
        };
        Fall {
            tick: kill.second.saturating_mul(self.tick_rate),
            victim: body_of(kill.victim),
            credited: kill
                .killer
                .filter(|_| !denied)
                .map(|killer| body_of(Some(killer))),
            ends_streak: !denied,
            shared: false,
        }
    }
}

/// The handle the count knows a seat's hero by (a hero no seat drives has one of its own), for a
/// recording has no bodies.
fn body_of(slot: Option<SlotId>) -> EntityId {
    EntityId {
        idx: slot.map_or(u32::MAX, |slot| u32::from(slot.0)),
        generation: 0,
    }
}

#[cfg(test)]
mod tests {
    use bota_proto::{EventKind, Team};
    use bota_replay::meta::MetaHeader;

    use super::*;

    const TICK_RATE: u16 = 30;
    /// Radiant's seats 0 and 2, Dire's 1 and 3.
    const SEATS: [(u8, u8); 4] = [(0, 0), (1, 1), (2, 0), (3, 1)];

    /// A fall as the match told it: who fell, who struck last, whether it was a deny.
    struct Death(u8, u8, bool);

    fn recording(falls: &[Death]) -> Recording {
        let mut records: Vec<_> = SEATS
            .iter()
            .map(|&(slot, team)| MetaRecord::Seat {
                slot,
                team,
                hero: 1,
            })
            .collect();
        for (n, Death(victim, killer, _)) in falls.iter().enumerate() {
            records.push(MetaRecord::Kill {
                tick: (n as u32 + 1) * u32::from(TICK_RATE),
                victim: Who::Hero(*victim),
                killer: Some(Who::Hero(*killer)),
                gold: 0,
            });
        }
        let meta = Meta {
            header: MetaHeader {
                magic: MetaHeader::MAGIC,
                schema: MetaHeader::SCHEMA,
                wire_fingerprint: 0,
                rules_version: 0,
                match_id: 1,
                map: 0,
                tick_rate: TICK_RATE,
                pregame_ticks: 0,
                catalogue_items: 0,
                catalogue_digest: 0,
            },
            records,
            skipped: 0,
            damaged: false,
            complete: true,
        };
        Recording::from_meta(&meta, SEATS.iter().map(|&(slot, _)| SlotId(slot)))
    }

    /// The longest streak of a seat by the live count, fed the falls as the server tells them.
    fn live_streak(falls: &[Death], slot: u8) -> u16 {
        let team = |slot: u8| match SEATS.iter().find(|seat| seat.0 == slot) {
            Some((_, 0)) => Team::Radiant,
            _ => Team::Dire,
        };
        let id = |slot: u8| body_of(Some(SlotId(slot)));
        let mut tracker = Tracker::new(MULTI_KILL_SECONDS * u32::from(TICK_RATE), true);
        let mut best = 0;
        for (n, Death(victim, killer, denied)) in falls.iter().enumerate() {
            let event = EventKind::Died {
                unit: id(*victim),
                killer: Some(id(*killer)),
                denied: *denied,
                gold: 0,
                bounty: None,
                assists: 0,
                awards: None,
                credited_hero: (!denied).then(|| Box::new(id(*killer))),
            };
            let fall = crate::kill_calls::fall_of(&event, n as u32, Some(team(*killer)))
                .expect("a death is a fall");
            tracker.fall(fall);
            best = best.max(tracker.streak_of(id(slot)));
        }
        best
    }

    fn kill(victim: u8, killer: u8) -> Death {
        Death(victim, killer, false)
    }

    #[test]
    fn a_deny_in_the_middle_of_a_streak_leaves_it_in_the_score_card_as_in_the_live_count() {
        let falls = [
            kill(1, 0),
            kill(3, 0),
            Death(0, 2, true),
            kill(1, 0),
            kill(3, 0),
        ];
        assert_eq!(live_streak(&falls, 0), 4);
        assert_eq!(recording(&falls).killstreak(SlotId(0)), 4);
    }

    #[test]
    fn a_fall_to_an_enemy_in_the_middle_of_a_streak_ends_it_in_the_score_card_as_in_the_live_count()
    {
        let falls = [kill(1, 0), kill(3, 0), kill(0, 1), kill(1, 0), kill(3, 0)];
        assert_eq!(live_streak(&falls, 0), 2);
        assert_eq!(recording(&falls).killstreak(SlotId(0)), 2);
    }

    #[test]
    fn a_hero_that_killed_itself_keeps_its_streak_in_the_score_card_as_in_the_live_count() {
        let falls = [kill(1, 0), kill(3, 0), Death(0, 0, true), kill(1, 0)];
        assert_eq!(live_streak(&falls, 0), 3);
        assert_eq!(recording(&falls).killstreak(SlotId(0)), 3);
    }
}
