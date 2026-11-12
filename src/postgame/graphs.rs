//! The complete results graph catalogue and bounded axes for recorded samples.

use bota_proto::{SlotId, Team};
use bota_replay::meta::stat;

use super::summary::MatchSummary;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Metric {
    #[default]
    Advantage,
    TeamNetWorth,
    TeamKills,
    NetWorth,
    Experience,
    Level,
    LastHits,
    Denies,
    Kills,
    Deaths,
    Assists,
    HeroDamage,
    BuildingDamage,
    Healing,
}

impl Metric {
    pub const ALL: [Self; 14] = [
        Self::Advantage,
        Self::TeamNetWorth,
        Self::TeamKills,
        Self::NetWorth,
        Self::Experience,
        Self::Level,
        Self::LastHits,
        Self::Denies,
        Self::Kills,
        Self::Deaths,
        Self::Assists,
        Self::HeroDamage,
        Self::BuildingDamage,
        Self::Healing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Advantage => "Team advantage",
            Self::TeamNetWorth => "Team net worth",
            Self::TeamKills => "Team kills",
            Self::NetWorth => "Net worth",
            Self::Experience => "Experience",
            Self::Level => "Level",
            Self::LastHits => "Last hits",
            Self::Denies => "Denies",
            Self::Kills => "Kills",
            Self::Deaths => "Deaths",
            Self::Assists => "Assists",
            Self::HeroDamage => "Hero damage",
            Self::BuildingDamage => "Building damage",
            Self::Healing => "Healing",
        }
    }

    pub fn stat(self) -> Option<u16> {
        match self {
            Self::Advantage => None,
            Self::TeamNetWorth | Self::NetWorth => Some(stat::NET_WORTH),
            Self::TeamKills | Self::Kills => Some(stat::KILLS),
            Self::Experience => Some(stat::XP),
            Self::Level => Some(stat::LEVEL),
            Self::LastHits => Some(stat::LAST_HITS),
            Self::Denies => Some(stat::DENIES),
            Self::Deaths => Some(stat::DEATHS),
            Self::Assists => Some(stat::ASSISTS),
            Self::HeroDamage => Some(stat::HERO_DAMAGE),
            Self::BuildingDamage => Some(stat::STRUCTURE_DAMAGE),
            Self::Healing => Some(stat::HEALING),
        }
    }

    pub fn team(self) -> bool {
        matches!(self, Self::Advantage | Self::TeamNetWorth | Self::TeamKills)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub second: u32,
    pub value: i64,
    pub connected: bool,
}

#[derive(Clone, Debug)]
pub struct Curve {
    pub name: String,
    pub slot: Option<SlotId>,
    pub colour: [u8; 3],
    pub values: Vec<Point>,
}

const SHADES: [[u8; 3]; 10] = [
    [155, 227, 107],
    [95, 184, 74],
    [201, 242, 160],
    [62, 143, 53],
    [134, 208, 122],
    [255, 138, 115],
    [212, 68, 46],
    [255, 192, 176],
    [168, 50, 31],
    [242, 160, 110],
];

pub fn player_colour(summary: &MatchSummary, slot: SlotId) -> [u8; 3] {
    let Some(row) = summary.rows.iter().find(|row| row.slot == slot) else {
        return [197, 203, 208];
    };
    let ordinal = summary
        .rows
        .iter()
        .filter(|other| other.team == row.team)
        .position(|other| other.slot == slot)
        .unwrap_or(0)
        .min(4);
    let index = if row.team == Team::Dire {
        5 + ordinal
    } else {
        ordinal
    };
    SHADES[index]
}

pub fn curves(summary: &MatchSummary, metric: Metric) -> Vec<Curve> {
    let Some(recording) = &summary.recording else {
        return Vec::new();
    };
    if !metric.team() {
        let stat = metric.stat().expect("a player metric has a stat");
        return summary
            .rows
            .iter()
            .filter_map(|row| {
                let samples = recording
                    .samples
                    .iter()
                    .find(|seat| seat.slot == row.slot)?;
                let mut connected = false;
                let mut previous = None;
                let values: Vec<_> = samples
                    .samples
                    .iter()
                    .filter(|sample| sample.second <= summary.seconds)
                    .filter_map(|sample| {
                        let value = sample.value(stat);
                        let point = value.map(|value| Point {
                            second: sample.second,
                            value: value.max(0),
                            connected: connected
                                && previous.is_some_and(|second: u32| {
                                    sample.second.saturating_sub(second) <= 60
                                }),
                        });
                        connected = value.is_some();
                        previous = Some(sample.second);
                        point
                    })
                    .collect();
                (!values.is_empty()).then(|| Curve {
                    name: row.display_name(),
                    slot: Some(row.slot),
                    colour: player_colour(summary, row.slot),
                    values,
                })
            })
            .collect();
    }
    if !recording.roster_complete {
        return Vec::new();
    }
    let mut times: Vec<u32> = recording
        .samples
        .iter()
        .flat_map(|seat| seat.samples.iter().map(|sample| sample.second))
        .collect();
    times.sort_unstable();
    times.dedup();
    if metric == Metric::Advantage {
        return [
            (stat::NET_WORTH, "Net worth", [242, 193, 78]),
            (stat::XP, "Experience", [116, 169, 242]),
        ]
        .into_iter()
        .filter_map(|(stat, name, colour)| {
            aggregate(summary, &times, stat, None).map(|values| Curve {
                name: name.to_owned(),
                slot: None,
                colour,
                values,
            })
        })
        .collect();
    }
    let stat = metric.stat().expect("a team metric has a stat");
    [
        (Team::Radiant, "Radiant", SHADES[0]),
        (Team::Dire, "Dire", SHADES[5]),
    ]
    .into_iter()
    .filter_map(|(team, name, colour)| {
        aggregate(summary, &times, stat, Some(team)).map(|values| Curve {
            name: name.to_owned(),
            slot: None,
            colour,
            values,
        })
    })
    .collect()
}

fn aggregate(
    summary: &MatchSummary,
    times: &[u32],
    stat: u16,
    team: Option<Team>,
) -> Option<Vec<Point>> {
    let recording = summary.recording.as_ref()?;
    let mut connected = false;
    let mut previous = None;
    let values: Vec<_> = times
        .iter()
        .filter(|second| **second <= summary.seconds)
        .filter_map(|second| {
            let value = summary
                .rows
                .iter()
                .filter(|row| team.is_none_or(|team| team == row.team))
                .try_fold(0_i64, |total, row| {
                    let value = recording.value(row.slot, stat, *second)?;
                    Some(if team.is_none() && row.team == Team::Dire {
                        total.saturating_sub(value.max(0))
                    } else {
                        total.saturating_add(value.max(0))
                    })
                });
            let point = value.map(|value| Point {
                second: *second,
                value,
                connected: connected
                    && previous.is_some_and(|previous: u32| second.saturating_sub(previous) <= 60),
            });
            connected = value.is_some();
            previous = Some(*second);
            point
        })
        .collect();
    (!values.is_empty()).then_some(values)
}

#[derive(Clone, Debug)]
pub struct Axis {
    pub ticks: Vec<i64>,
    pub minimum: i64,
    pub maximum: i64,
}

impl Axis {
    pub fn of(curves: &[Curve], advantage: bool) -> Self {
        let reach = curves
            .iter()
            .flat_map(|curve| &curve.values)
            .map(|point| point.value.unsigned_abs())
            .max()
            .unwrap_or(1)
            .max(1);
        let intervals = if advantage { 3 } else { 6 };
        let step = round_step(reach, intervals);
        let top = reach
            .div_ceil(step)
            .saturating_mul(step)
            .min(i64::MAX as u64) as i64;
        let count = ((top as u64).div_ceil(step)).min(6) as i64;
        let step = step.min(i64::MAX as u64) as i64;
        let ticks = if advantage {
            (-count..=count)
                .rev()
                .map(|tick| tick.saturating_mul(step))
                .collect()
        } else {
            (0..=count)
                .rev()
                .map(|tick| tick.saturating_mul(step))
                .collect()
        };
        Self {
            ticks,
            minimum: if advantage { -top } else { 0 },
            maximum: top.max(1),
        }
    }

    pub fn fraction(&self, value: i64) -> f32 {
        let range = (self.maximum as f64 - self.minimum as f64).max(1.0);
        ((self.maximum as f64 - value as f64) / range).clamp(0.0, 1.0) as f32
    }
}

fn round_step(reach: u64, intervals: u64) -> u64 {
    let needed = reach.div_ceil(intervals);
    let mut scale = 1_u64;
    for _ in 0..20 {
        for multiplier in [10_u64, 20, 25, 50] {
            let step = scale
                .checked_mul(multiplier)
                .map_or(u64::MAX, |step| step / 10);
            if step >= needed {
                return step;
            }
        }
        scale = scale.saturating_mul(10);
    }
    u64::MAX
}

pub fn time_ticks(seconds: u32) -> Vec<u32> {
    let step = [1, 2, 5, 10, 15, 20, 30, 60, 120]
        .into_iter()
        .find(|minutes| seconds.div_ceil(60) <= minutes * 6)
        .unwrap_or(120)
        * 60;
    let mut ticks: Vec<u32> = (0..seconds)
        .step_by(step as usize)
        .filter(|second| u64::from(seconds - second) * 100 >= u64::from(seconds) * 12)
        .take(6)
        .collect();
    ticks.push(seconds);
    ticks
}

pub fn label_positions(fractions: &[f32], height: f32) -> Vec<f32> {
    // Labels use pixel positions; their container scrolls if a short plot cannot fit them.
    let height = height.max(fractions.len() as f32 * 17.0).max(1.0);
    let gap = 17.0 / height.max(1.0);
    let mut sorted: Vec<(usize, f32)> = fractions.iter().copied().enumerate().collect();
    sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
    for index in 1..sorted.len() {
        sorted[index].1 = sorted[index].1.max(sorted[index - 1].1 + gap);
    }
    if let Some(last) = sorted.last_mut() {
        last.1 = last.1.min(1.0);
    }
    for index in (0..sorted.len().saturating_sub(1)).rev() {
        sorted[index].1 = sorted[index].1.min(sorted[index + 1].1 - gap);
    }
    let mut positions = vec![0.0; sorted.len()];
    for (index, fraction) in sorted {
        positions[index] = fraction.max(0.0) * height;
    }
    positions
}

pub fn compact(value: i64) -> String {
    let magnitude = value.unsigned_abs();
    let sign = if value < 0 { "−" } else { "" };
    if magnitude >= 1000 {
        let tenths = magnitude.saturating_add(50) / 100;
        format!("{sign}{}.{:01}k", tenths / 10, tenths % 10)
    } else {
        format!("{sign}{magnitude}")
    }
}

pub fn clock(second: u32) -> String {
    format!("{}:{:02}", second / 60, second % 60)
}
