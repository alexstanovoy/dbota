//! Which clip each unit's model plays and where in it, set every frame from
//! the simulation: a swing's windup puts the clip's attack point on the
//! simulator's strike, a cast's on the simulator's cast point, and only
//! idling, running and the follow-through keep the clip's own pace.

use bevy::prelude::*;
use bota_client_core::App as Core;
use bota_proto::{AbilityId, UnitKind, UnitView};

use crate::ability_art::art_of;
use crate::coords::sim_facing_to_rotation;
use crate::session::Session;
use crate::units::{Dying, ModelBank, ModelOf, Rig, Unit};

/// Seconds a cast clip is stretched over at the least.
const MIN_CAST_POINT: f32 = 0.05;
/// The clip of a cast with no art of its own.
const PLAIN_CAST: &str = "cast1";
/// World units a second at which a run clip plays at its own pace.
const RUN_PACE: f32 = 300.0;
/// World units a second above which a unit counts as running.
const RUNNING: f32 = 25.0;

/// Where a unit's model is in its clips.
#[derive(Component, Debug, Default)]
pub struct Pose {
    /// The clip playing, by logical name.
    clip: &'static str,
    /// Seconds into the idle or run loop.
    loop_t: f32,
    /// Seconds since the last swing struck, while its follow-through plays.
    since_strike: Option<f32>,
    /// The strike tick of the swing under way, to tell one swing from the next.
    swing_due: Option<u32>,
    /// How many swings it has made, for alternating attack clips.
    swings: u32,
    /// The pace of the last swing: clip seconds per simulated second.
    swing_tempo: f32,
}

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, animate.before(TransformSystems::Propagate));
}

/// What a unit should be showing: a logical clip and the time into it.
struct Wanted {
    clip: &'static str,
    t: f32,
}

fn animate(
    session: NonSend<Session>,
    bank: Res<ModelBank>,
    time: Res<Time>,
    mut units: Query<(&Unit, &mut Pose, &ModelOf, Option<&Rig>, Option<&Dying>)>,
    mut pivots: Query<&mut Transform>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let dt = time.delta_secs();
    for (unit, mut pose, model_of, rig, dying) in &mut units {
        if let Ok(mut pivot) = pivots.get_mut(model_of.0) {
            let rotation = sim_facing_to_rotation(unit.facing);
            if pivot.rotation != rotation {
                pivot.rotation = rotation;
            }
        }
        let (Some(rig), Some(model)) = (rig, unit.model) else {
            continue;
        };
        let clips = &bank.0[model].clips;
        let clip = |name: &str| clips.iter().find(|c| c.0 == name && c.2 > 0.0);
        let live = view.unit(unit.id);
        let wanted = match (dying, live) {
            (Some(d), _) => Wanted {
                clip: "death",
                t: d.0,
            },
            (None, Some(u)) => choose(core, u, unit, &mut pose, dt, |name| {
                clip(name).map(|c| (c.2, c.3))
            }),
            (None, None) => continue,
        };
        let Some((_, node, duration, _)) = clip(wanted.clip).or_else(|| clip("idle")) else {
            continue;
        };
        let Ok(mut player) = players.get_mut(rig.0) else {
            continue;
        };
        if pose.clip != wanted.clip || !player.is_playing_animation(*node) {
            player.stop_all();
            player.play(*node).pause();
            pose.clip = wanted.clip;
        }
        if let Some(active) = player.animation_mut(*node) {
            active.set_seek_time(wanted.t.clamp(0.0, *duration - 1e-3));
        }
    }
}

/// The clip and time a standing unit shows. `clip` gives a logical clip's
/// duration and point when the model has it.
fn choose(
    core: &Core,
    u: &UnitView,
    unit: &Unit,
    pose: &mut Pose,
    dt: f32,
    clip: impl Fn(&str) -> Option<(f32, Option<f32>)>,
) -> Wanted {
    let rate = f32::from(core.tick_rate().max(1));
    let sub = core.smooth.alpha();
    if let Some(began) = core.cast_began(u.id) {
        let (name, cast_point) = cast_clip(core, u);
        if let Some((duration, point)) = clip(name) {
            let elapsed =
                (core.view().map_or(began, |v| v.tick) - began) as f32 / rate + sub / rate;
            let point = point.unwrap_or(duration * 0.5);
            pose.since_strike = None;
            return Wanted {
                clip: name,
                t: elapsed / cast_point * point,
            };
        }
    }
    let swing = core.view().and_then(|view| {
        view.aggro
            .binary_search_by_key(&u.id, |a| a.unit)
            .ok()
            .and_then(|at| view.aggro[at].swing)
    });
    let attack = if pose.swings.is_multiple_of(2) {
        "attack"
    } else {
        "attack2"
    };
    let attack = if clip(attack).is_some() {
        attack
    } else {
        "attack"
    };
    if let (Some(swing), Some((_, point))) = (swing, clip(attack)) {
        let tick = core.view().map_or(0, |v| v.tick);
        let due = tick + swing.strike_in;
        if pose.swing_due.is_some_and(|d| d.abs_diff(due) > 1) {
            pose.swings += 1;
        }
        pose.swing_due = Some(due);
        let windup = (u.attack_point as f32 / 1000.0).max(0.05);
        let left = ((swing.strike_in as f32 - sub) / rate).max(0.0);
        let point = point.unwrap_or(0.4);
        pose.swing_tempo = point / windup;
        pose.since_strike = Some(0.0);
        return Wanted {
            clip: attack,
            t: (1.0 - left / windup).clamp(0.0, 1.0) * point,
        };
    }
    if pose.swing_due.take().is_some() {
        pose.swings += 1;
    }
    if let Some(since) = pose.since_strike.as_mut() {
        *since += dt;
        let last = if pose.swings % 2 == 1 {
            "attack"
        } else {
            "attack2"
        };
        let last = if clip(last).is_some() { last } else { "attack" };
        if let Some((duration, point)) = clip(last) {
            let t = point.unwrap_or(0.4) + *since * pose.swing_tempo.max(0.5);
            if t < duration && unit.speed < RUNNING {
                return Wanted { clip: last, t };
            }
        }
        pose.since_strike = None;
    }
    let (name, pace) = if unit.speed > RUNNING && unit.kind != UnitKind::Tower {
        ("run", unit.speed / RUN_PACE)
    } else {
        ("idle", 1.0)
    };
    let duration = clip(name).map_or(1.0, |c| c.0);
    pose.loop_t = (pose.loop_t + dt * pace) % duration.max(0.01);
    Wanted {
        clip: name,
        t: pose.loop_t,
    }
}

/// The clip of the cast a unit is in, and the simulator's cast point of it
/// in seconds, from the latest order its seat is known to have given. A
/// cast whose order is not known shows the first raze.
fn cast_clip(core: &Core, u: &UnitView) -> (&'static str, f32) {
    if u.neutral.is_some() {
        return neutral_cast(u);
    }
    let ability = crate::overlay::cast_ability(core, u);
    // A cast whose order is unknown shows the first raze.
    let id = ability.unwrap_or(AbilityId::RAZE_NEAR);
    let name = art_of(id).clip;
    let point = core.cast_point_secs(u, id).unwrap_or(0.0);
    // A clip is stretched over the point; one of no length plays at its own pace.
    (name, point.max(MIN_CAST_POINT))
}

/// The clip and cast point of a neutral creep's cast. No seat orders a
/// neutral, so the ability is the active one that went on cooldown last: the
/// longest cooldown left.
fn neutral_cast(u: &UnitView) -> (&'static str, f32) {
    let ability = u
        .abilities
        .iter()
        .filter(|a| !a.passive)
        .max_by_key(|a| a.cooldown_left)
        .map(|a| a.id);
    let point = ability
        .and_then(bota_core::ability_def)
        .map_or(0, |def| def.point);
    (
        ability.map_or(PLAIN_CAST, |id| art_of(id).clip),
        (point as f32 / 1000.0).max(MIN_CAST_POINT),
    )
}
