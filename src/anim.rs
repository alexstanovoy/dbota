//! Which clip each unit's model plays and where in it, set every frame from
//! the simulation: a swing's windup puts the clip's attack point on the
//! simulator's strike, a cast's on the simulator's cast point, and only
//! idling, running and the follow-through of a blow that landed keep the
//! clip's own pace. A clip fades in over the one before, as Dota's sequences
//! do.

use std::collections::HashMap;

use bevy::{
    camera::primitives::{Frustum, Sphere},
    prelude::*,
};
use bota_client_core::App as Core;
use bota_proto::{AbilityId, EffectId, EntityId, EventKind, StatusFlags, Team, UnitKind, UnitView};

use crate::ability_art::art_of;
use crate::coords::sim_facing_to_rotation;
use crate::session::Session;
use crate::units::{Dying, ModelBank, ModelOf, Rig, Unit};

/// The clip of a cast with no art of its own, and of an item's use.
const PLAIN_CAST: &str = "cast1";
/// Seconds a clip takes to fade in over the one before, which fades out
/// meanwhile: the fade of Dota's unit sequences (their models'
/// `m_flFadeInTime` and `m_flFadeOutTime`).
pub(crate) const FADE_SECONDS: f32 = 0.2;
/// Clips that may fade out under the one playing at once; a quicker change
/// lets the faintest go.
const MAX_FADING: usize = 2;
/// The clip a model plays now and then while it stands idle, when it has one.
pub const RARE_IDLE: &str = "idle_rare";
/// Whole idle loops a unit stands through before its rare idle: this many, and up to three
/// more by the unit, so units standing together do not play it in step.
const LOOPS_BEFORE_RARE: u32 = 4;
/// World units a second at which a run clip plays at its own pace.
const RUN_PACE: f32 = 300.0;
/// World units a second above which a unit counts as running.
const RUNNING: f32 = 25.0;
/// World units out of the camera's view within which a unit's skeleton is
/// still posed: past the tallest model and the shadow it casts into view,
/// and a frame of panning.
const POSED_REACH: f32 = 1500.0;

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
    /// Seconds into the clip playing shown this frame.
    shown: f32,
    /// The weight of the clip playing, rising to one as it fades in.
    share: f32,
    /// The clips fading out under it.
    fading: Vec<Fading>,
    /// Whole idle loops stood through since the unit last did anything else.
    idle_loops: u32,
}

/// A clip fading out under the one playing.
#[derive(Debug)]
struct Fading {
    node: AnimationNodeIndex,
    /// Seconds into it where it was left, where it is held.
    t: f32,
    /// Its weight, falling to nothing.
    weight: f32,
}

impl Pose {
    /// The clip playing, by logical name (`attack2` while the second attack swings).
    pub fn clip(&self) -> &'static str {
        self.clip
    }

    /// Whether a clip is still fading out under the one playing.
    #[cfg(test)]
    pub fn blending(&self) -> bool {
        !self.fading.is_empty()
    }
}

/// The ability each unit in view is channelling, by the cast that began it:
/// which loop its model plays meanwhile.
#[derive(Resource, Default)]
struct Channels(HashMap<EntityId, AbilityId>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Channels>().add_systems(
        PostUpdate,
        (note_channels, animate)
            .chain()
            .before(TransformSystems::Propagate),
    );
}

/// Takes in the casts the last pump told, and lets go of every unit that
/// no longer channels: the view says a unit channels, the cast before says
/// what.
fn note_channels(session: NonSend<Session>, mut channels: ResMut<Channels>) {
    let core = &session.0;
    for event in &core.told {
        if let EventKind::AbilityCast { caster, ability } = event {
            channels.0.insert(*caster, *ability);
        }
    }
    let Some(view) = core.view() else {
        channels.0.clear();
        return;
    };
    channels.0.retain(|id, _| {
        view.unit(*id)
            .is_some_and(|u| u.statuses.bits & StatusFlags::CHANNELLING != 0)
    });
}

/// What a unit should be showing: a logical clip and the time into it.
struct Wanted {
    clip: &'static str,
    t: f32,
}

fn animate(
    session: NonSend<Session>,
    bank: Res<ModelBank>,
    channels: Res<Channels>,
    time: Res<Time>,
    mut units: Query<(
        &Unit,
        &mut Pose,
        &ModelOf,
        &GlobalTransform,
        Option<&Rig>,
        Option<&Dying>,
    )>,
    mut pivots: Query<&mut Transform>,
    mut players: Query<&mut AnimationPlayer>,
    camera: Single<&Frustum, With<crate::camera::RtsCamera>>,
    args: Res<crate::Args>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let dt = time.delta_secs();
    for (unit, mut pose, model_of, at, rig, dying) in &mut units {
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
        // A clip of no length is a held pose (a Watcher put out).
        let clip = |name: &str| clips.iter().find(|c| c.0 == name);
        let live = view.unit(unit.id);
        let channel = channels.0.get(&unit.id).and_then(|id| art_of(*id).channel);
        let wanted = match (dying, live) {
            (Some(d), _) => Wanted {
                clip: "death",
                t: d.0,
            },
            (None, Some(u)) => {
                let clip = |name: &str| clip(name).map(|c| (c.2, c.3));
                match channel.and_then(|name| Some((name, clip(name)?.0))) {
                    Some((name, duration)) => looping(&mut pose, name, duration, dt, 1.0),
                    None => choose(core, u, unit, &mut pose, dt, clip),
                }
            }
            (None, None) => continue,
        };
        if wanted.clip != "idle" && wanted.clip != RARE_IDLE {
            pose.idle_loops = 0;
        }
        let Some((_, node, duration, _)) = clip(wanted.clip).or_else(|| clip("idle")) else {
            continue;
        };
        let Ok(mut player) = players.get_mut(rig.0) else {
            continue;
        };
        let from = clip(pose.clip).map(|c| c.1);
        // A unit no view sees nor will in a frame is not posed: its clips
        // weigh nothing, so its joints are neither evaluated nor moved
        // (nor propagated). Its clip and time still follow the simulation,
        // so it is posed right the frame it comes near.
        let near = camera.intersects_sphere(
            &Sphere {
                center: at.translation_vec3a(),
                radius: POSED_REACH,
            },
            true,
        );
        if pose.clip != wanted.clip || !player.is_playing_animation(*node) {
            cross_fade(&mut pose, &mut player, from, *node);
            pose.clip = wanted.clip;
        }
        pose.shown = wanted.t.clamp(0.0, (*duration - 1e-3).max(0.0));
        weigh(&mut pose, &mut player, *node, dt, near);
        if args.trace_feel
            && let Some(u) = live
        {
            trace(time.elapsed_secs(), view.tick, u, &pose, clips);
        }
    }
}

/// One `--trace-feel` line: what the unit's model shows this frame.
fn trace(
    at: f32,
    tick: u32,
    u: &UnitView,
    pose: &Pose,
    clips: &[(String, AnimationNodeIndex, f32, Option<f32>)],
) {
    let name = |node| {
        clips
            .iter()
            .find(|c| c.1 == node)
            .map_or("?", |c| c.0.as_str())
    };
    let fading: Vec<String> = pose
        .fading
        .iter()
        .map(|f| format!("{}:{:.2}", name(f.node), f.weight))
        .collect();
    let point = clips
        .iter()
        .find(|c| c.0 == pose.clip)
        .and_then(|c| c.3)
        .map_or("none".to_owned(), |p| format!("{p:.3}"));
    println!(
        "anim at={at:.3} tick={tick} unit={} kind={:?} neutral={:?} pos={:.0},{:.0} clip={} t={:.3} point={point} share={:.2} fading={}",
        u.id.idx,
        u.kind,
        u.neutral.and_then(bota_core::NeutralKind::of),
        u.pos.x.to_f32(),
        u.pos.y.to_f32(),
        pose.clip,
        pose.shown,
        pose.share,
        fading.join(",")
    );
}

/// Starts clip `to` over the one playing (`from`), which fades out as `to`
/// fades in, as Dota cross-fades a unit's sequences; a clip coming back
/// before it faded out picks up from the weight it had left.
fn cross_fade(
    pose: &mut Pose,
    player: &mut AnimationPlayer,
    from: Option<AnimationNodeIndex>,
    to: AnimationNodeIndex,
) {
    let Some(from) = from.filter(|from| *from != to && player.is_playing_animation(*from)) else {
        player.stop_all();
        player.play(to).pause();
        pose.fading.clear();
        pose.share = 1.0;
        return;
    };
    let back = pose
        .fading
        .iter()
        .position(|f| f.node == to)
        .map(|at| pose.fading.swap_remove(at).weight);
    pose.fading.push(Fading {
        node: from,
        t: pose.shown,
        weight: pose.share,
    });
    if pose.fading.len() > MAX_FADING {
        let (at, _) = pose
            .fading
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.weight.total_cmp(&b.1.weight))
            .expect("more than none");
        player.stop(pose.fading.swap_remove(at).node);
    }
    pose.share = back.unwrap_or(0.0);
    if !player.is_playing_animation(to) {
        player.play(to).pause();
    }
}

/// Moves a fade on by `dt` and weighs the clip playing, at `pose.shown`,
/// and each fading out, held where it was left; a faded clip is let go.
fn weigh(
    pose: &mut Pose,
    player: &mut AnimationPlayer,
    node: AnimationNodeIndex,
    dt: f32,
    near: bool,
) {
    let step = dt / FADE_SECONDS;
    pose.share = (pose.share + step).min(1.0);
    for fading in &mut pose.fading {
        fading.weight -= step;
        if fading.weight <= 0.0 {
            player.stop(fading.node);
        }
    }
    pose.fading.retain(|f| f.weight > 0.0);
    let shown = |weight: f32| if near { weight } else { 0.0 };
    if let Some(active) = player.animation_mut(node) {
        active.set_seek_time(pose.shown);
        active.set_weight(shown(pose.share));
    }
    for fading in &pose.fading {
        if let Some(active) = player.animation_mut(fading.node) {
            active.set_seek_time(fading.t);
            active.set_weight(shown(fading.weight));
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
    if let (Some(cast), Some(began)) = (u.cast, core.cast_began(u.id)) {
        // A model without the cast's clip (Dota has none either) goes on
        // with what it was doing.
        let name = cast.ability.map_or(PLAIN_CAST, |id| art_of(id).clip);
        if let Some((duration, point)) = clip(name) {
            // The clip's point lands on the tick the view says the cast goes
            // off, counted from when its cast point was first seen.
            let ran = core.view().map_or(began, |v| v.tick).saturating_sub(began);
            let whole = (ran + cast.goes_off_in) as f32;
            let point = point.unwrap_or(duration * 0.5);
            pose.since_strike = None;
            return Wanted {
                clip: name,
                t: ((ran as f32 + sub) / whole).min(1.0) * point,
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
        let due = tick.saturating_add(swing.strike_in);
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
    if let Some(due) = pose.swing_due.take() {
        pose.swings += 1;
        // A windup gone before its strike tick was broken off (an order, its
        // target falling or getting away): nothing was struck, and Dota cuts
        // the clip there rather than playing the blow's follow-through.
        if core.view().is_some_and(|v| v.tick < due) {
            pose.since_strike = None;
        }
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
        (standing(core, u), 1.0)
    };
    let name = if clip(name).is_some() { name } else { "idle" };
    let duration = clip(name).map_or(1.0, |c| c.0);
    if name == "idle"
        && let Some(rare) = rare_idle(pose, u.id, dt, &clip)
    {
        return rare;
    }
    let before = pose.loop_t;
    let wanted = looping(pose, name, duration, dt, pace);
    if name == "idle" && pose.loop_t < before {
        pose.idle_loops = pose.idle_loops.saturating_add(1);
    }
    wanted
}

/// The rare idle a unit standing idle plays, once, after its share of whole idle loops; none
/// when its model has no rare idle or it is not yet time.
fn rare_idle(
    pose: &mut Pose,
    id: EntityId,
    dt: f32,
    clip: &impl Fn(&str) -> Option<(f32, Option<f32>)>,
) -> Option<Wanted> {
    let (duration, _) = clip(RARE_IDLE)?;
    if pose.clip == RARE_IDLE {
        pose.loop_t += dt;
        if pose.loop_t < duration {
            return Some(Wanted {
                clip: RARE_IDLE,
                t: pose.loop_t,
            });
        }
        pose.loop_t = 0.0;
        pose.idle_loops = 0;
        return None;
    }
    let after = LOOPS_BEFORE_RARE + (id.idx ^ id.generation).wrapping_mul(0x9E37_79B9) % 4;
    if pose.idle_loops < after {
        return None;
    }
    pose.loop_t = 0.0;
    Some(Wanted {
        clip: RARE_IDLE,
        t: 0.0,
    })
}

/// A loop moved on by `dt` at `pace`.
fn looping(pose: &mut Pose, clip: &'static str, duration: f32, dt: f32, pace: f32) -> Wanted {
    pose.loop_t = (pose.loop_t + dt * pace) % duration.max(0.01);
    Wanted {
        clip,
        t: pose.loop_t,
    }
}

/// The loop a unit plays standing: a map objective's shows its state, as
/// Dota's models do: a Watcher out until the horn and while put out, lit by
/// day or by night; an Outpost a side holds; a Shrine ready to be gathered.
fn standing(core: &Core, u: &UnitView) -> &'static str {
    let has = |id: EffectId| u.effects.iter().any(|e| e.id == id);
    let Some(view) = core.view() else {
        return "idle";
    };
    match u.kind {
        UnitKind::Watcher if has(EffectId::LAMP_OFF) => "off",
        UnitKind::Watcher if view.game_tick() < core.pregame_ticks() => "off",
        UnitKind::Watcher if has(EffectId::VISION_GRANT) => {
            if crate::daylight::night(core).unwrap_or(false) {
                "lit_night"
            } else {
                "lit"
            }
        }
        UnitKind::Outpost if u.team != Team::Neutral => "captured",
        UnitKind::Shrine if has(EffectId::SHRINE_READY) => "active",
        _ => "idle",
    }
}
