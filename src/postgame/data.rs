//! Timed facts retained from the local recording; no curves are inferred from final totals.

use bota_proto::{ItemId, SlotId};
use bota_replay::meta::{Meta, MetaRecord, Who};

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
    pub map: u16,
    pub tick_rate: u32,
    pub pregame_ticks: u32,
    pub samples: Vec<SeatSamples>,
    pub kills: Vec<Kill>,
    pub purchases: Vec<Purchase>,
    pub levels: Vec<Level>,
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
            limited: !meta.complete || meta.skipped > 0,
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
            _ => {}
        }
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

    pub fn killstreak(&self, slot: SlotId) -> u16 {
        let (mut current, mut best) = (0_u16, 0_u16);
        for kill in &self.kills {
            if kill.victim == Some(slot) {
                current = 0;
            }
            if kill.killer == Some(slot) {
                current = current.saturating_add(1);
                best = best.max(current);
            }
        }
        best
    }
}
