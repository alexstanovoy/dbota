//! Which clip each unit's model plays and where in it, set every frame from
//! the simulation: a swing's windup puts the clip's attack point on the
//! simulator's strike, a cast's on the simulator's cast point, and only
//! idling, running and the follow-through of a blow that landed keep the
//! clip's own pace. A clip fades in over the one before, as Dota's sequences
//! do.

use std::collections::{HashMap, HashSet};

use bevy::{math::Vec3A, mesh::skinning::SkinnedMesh, prelude::*};
use bota_client_core::App as Core;
use bota_proto::{
    AbilityId, EffectId, EntityId, EventKind, StatusFlags, Team, UnitKind, UnitView, WorldView,
};

use crate::ability_art::art_of;
use crate::camera::RtsCamera;
use crate::coords::sim_facing_to_rotation;
use crate::map::MainSun;
use crate::session::Session;
use crate::shadow_casters::{ShadowFloor, shadow_reaches};
use crate::units::{Dying, ModelBank, ModelOf, Rig, Unit, UnitIndex};

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
/// World units either side of its feet a living unit's model may reach, at any
/// yaw: the widest bind pose's corner in the cache (a Lotus Pool, 652 at the
/// largest scale the map places a building at) and room for its clips to swing
/// past it. `every_posed_model_fits_the_box_it_is_posed_by` measures the
/// bind poses; that the clips stay within a quarter more is assumed, not
/// measured. A dying unit is always posed: the destruction models' clips
/// fling their debris far past any such box.
pub(crate) const MODEL_REACH: f32 = 850.0;
/// World units over its feet a living unit's model may reach: the tallest bind
/// pose (a Divine Sentinel's cube, 533 at the largest placed scale) and room
/// for its clips.
pub(crate) const MODEL_HEIGHT: f32 = 800.0;
/// World units under its feet a living unit's model may reach: the deepest
/// bind pose (a Divine Sentinel, 219 at the largest placed scale) and room for
/// its clips.
pub(crate) const MODEL_DEPTH: f32 = 300.0;
/// World units a posed unit's box is grown by before it is let go: a unit
/// standing at the edge of what is posed is not posed and let go frame after
/// frame (each a change of its components).
const LET_GO_MARGIN: f32 = 200.0;

/// Where a unit's model is in its clips.
#[derive(Component, Debug, Default)]
pub struct Pose {
    /// The clip playing, by logical name.
    clip: &'static str,
    /// Seconds into the idle or run loop.
    loop_t: f32,
    /// Seconds since the last swing struck, while its follow-through plays.
    since_strike: Option<f32>,
    /// The target identifies the current windup; its deadline follows attack speed.
    swing: Option<(EntityId, u32)>,
    /// The tick of the attack whose follow-through is playing.
    struck_at: Option<u32>,
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
pub struct Channels(HashMap<EntityId, AbilityId>);

impl Channels {
    /// The ability a unit is channelling.
    pub fn of(&self, unit: EntityId) -> Option<AbilityId> {
        self.0.get(&unit).copied()
    }

    /// The map objectives heroes channel at (an Outpost captured, a Watcher lit): the wire
    /// tells a cast's caster but not its target, so each hero channelling the ability an
    /// objective's kind is worked with is taken to be at the nearest objective of that kind
    /// within the ability's reach, edge to edge.
    pub fn objectives(&self, view: &WorldView) -> HashSet<EntityId> {
        let place = |u: &UnitView| Vec2::new(u.pos.x.to_f32(), u.pos.y.to_f32());
        self.0
            .iter()
            .filter_map(|(hero, ability)| {
                let hero = view.unit(*hero)?;
                let reach = bota_core::ability_def(*ability)?.range as f32;
                view.units
                    .iter()
                    .filter(|u| bota_core::kind_def(u.kind).interact == Some(*ability))
                    .map(|u| (u, place(u).distance(place(hero))))
                    .filter(|(u, apart)| *apart <= reach + hero.bound.to_f32() + u.bound.to_f32())
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(u, _)| u.id)
            })
            .collect()
    }
}

/// Takes in the casts of the frame and lets go of the channels that ended: [`Channels`] is
/// current after it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteChannels;

pub fn plugin(app: &mut App) {
    app.init_resource::<Channels>().add_systems(
        PostUpdate,
        (note_channels.in_set(NoteChannels), animate)
            .chain()
            .before(bevy::app::AnimationSystems)
            .after(crate::shadow_casters::find_floor),
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

/// The body and its attack target in the same interpolated frame.
struct Drawn<'a> {
    unit: &'a Unit,
    target: Option<Vec2>,
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
        &Transform,
        Option<&Rig>,
        Option<&Dying>,
    )>,
    mut pivots: Query<&mut Transform, (Without<Unit>, Without<RtsCamera>)>,
    mut players: Query<(&mut AnimationPlayer, Has<AnimationGraphHandle>)>,
    placed: Query<&Unit>,
    index: Res<UnitIndex>,
    camera: Single<(&Transform, &Projection), With<RtsCamera>>,
    args: Res<crate::Args>,
    (mut commands, children, skins): (Commands, Query<&Children>, Query<(), With<SkinnedMesh>>),
    (sun, floor): (
        Query<(&GlobalTransform, &DirectionalLight), With<MainSun>>,
        Res<ShadowFloor>,
    ),
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    // The view as this frame's camera has it: Bevy works the camera's frustum
    // out only after the animations, and a unit coming into view by a jump of
    // the camera (a minimap click, the match's start) is posed in its first
    // frame there.
    let (camera, projection) = *camera;
    let frustum = projection.compute_frustum(&GlobalTransform::from(*camera));
    // Without shadow maps (the Low and Potato presets) no unit's shadow is drawn.
    let shadow = sun
        .single()
        .ok()
        .filter(|(_, light)| light.shadow_maps_enabled)
        .map(|(at, _)| at)
        .zip(floor.0);
    let dt = time.delta_secs();
    let worked = channels.objectives(view);
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
        let channel = live
            .and_then(|u| u.channel.filter(|_| u.forced.is_none()))
            .and_then(|c| art_of(c.ability).channel.map(|name| (c, name)));
        let wanted = match (dying, live) {
            (Some(d), _) => Wanted {
                clip: "death",
                t: d.0,
            },
            (None, Some(u)) => {
                let clip = |name: &str| clip(name).map(|c| (c.2, c.3));
                let target = view
                    .aggro
                    .binary_search_by_key(&u.id, |a| a.unit)
                    .ok()
                    .and_then(|at| view.aggro[at].swing)
                    .and_then(|swing| {
                        index
                            .0
                            .get(&swing.target)
                            .and_then(|entity| placed.get(*entity).ok())
                            .map(|target| target.at)
                            .or_else(|| {
                                view.unit(swing.target).map(|target| {
                                    Vec2::new(target.pos.x.to_f32(), target.pos.y.to_f32())
                                })
                            })
                    });
                match channel.and_then(|(channel, name)| Some((channel, name, clip(name)?))) {
                    Some((channel, name, (duration, point))) => {
                        let offset = if art_of(channel.ability).clip == name {
                            point.unwrap_or(0.0)
                        } else {
                            0.0
                        };
                        let elapsed = (channel.elapsed as f32 + core.smooth.alpha())
                            / bota_core::rules::TICKS_PER_SECOND as f32;
                        Wanted {
                            clip: name,
                            t: if duration > 0.0 {
                                (offset + elapsed) % duration
                            } else {
                                0.0
                            },
                        }
                    }
                    None if worked.contains(&unit.id) => {
                        if let Some((duration, _)) = clip("channel") {
                            looping(&mut pose, "channel", duration, dt, 1.0)
                        } else {
                            choose(core, u, Drawn { unit, target }, &mut pose, dt, clip)
                        }
                    }
                    None => choose(core, u, Drawn { unit, target }, &mut pose, dt, clip),
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
        let Ok((mut player, posed)) = players.get_mut(rig.0) else {
            continue;
        };
        let from = clip(pose.clip).map(|c| c.1);
        // A unit whose model could not be in the view nor throw its shadow
        // into it is not posed. Its clip and time still follow the
        // simulation, so it is posed right the frame it comes near. A dying
        // one always is: its fall may reach anywhere near.
        let near = dying.is_some() || in_reach(&frustum, at.translation, posed, shadow);
        let graph = bank.0[model].graph.as_ref();
        if near != posed && (graph.is_some() || !near) {
            let graph = graph.filter(|_| near);
            set_posed(&mut commands, (rig.0, model_of.0), graph, &children, &skins);
        }
        if pose.clip != wanted.clip || !player.is_playing_animation(*node) {
            cross_fade(&mut pose, &mut player, from, *node);
            pose.clip = wanted.clip;
        }
        pose.shown = wanted.t.clamp(0.0, (*duration - 1e-3).max(0.0));
        weigh(&mut pose, &mut player, *node, dt);
        if args.trace_feel
            && let Some(u) = live
        {
            trace(time.elapsed_secs(), view.tick, u, unit, &pose, clips);
        }
        if args.trace_feel && dying.is_some() {
            let shown = clips
                .iter()
                .find(|clip| clip.1 == *node)
                .map_or("?", |clip| clip.0.as_str());
            let died = core
                .session()
                .and_then(|session| session.entity(unit.id))
                .and_then(|track| track.last_death)
                .map_or(0, |death| death.tick);
            println!(
                "dying tick={} died={died} unit={} clip={shown}",
                view.tick, unit.id.idx
            );
        }
    }
}

/// One `--trace-feel` line: what the unit's model shows this frame.
fn trace(
    at: f32,
    tick: u32,
    u: &UnitView,
    drawn: &Unit,
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
        "anim at={at:.3} tick={tick} unit={} kind={:?} neutral={:?} hero={:?} pos={:.0},{:.0} drawn={:.0},{:.0} clip={} t={:.3} point={point} share={:.2} fading={}",
        u.id.idx,
        u.kind,
        u.neutral.and_then(bota_core::NeutralKind::of),
        u.hero,
        u.pos.x.to_f32(),
        u.pos.y.to_f32(),
        drawn.at.x,
        drawn.at.y,
        pose.clip,
        pose.shown,
        pose.share,
        fading.join(",")
    );
}

/// Whether a unit standing at `feet` may be seen or throw its shadow into the
/// view: its model's box ([`MODEL_REACH`] either side, [`MODEL_HEIGHT`] over,
/// [`MODEL_DEPTH`] under its feet), grown by [`LET_GO_MARGIN`] while it is
/// `posed`, meets the frustum, or its shadow cast by the `shadow` sun (when
/// it casts shadows) down to its floor does.
fn in_reach(
    frustum: &bevy::camera::primitives::Frustum,
    feet: Vec3,
    posed: bool,
    shadow: Option<(&GlobalTransform, f32)>,
) -> bool {
    let grow = if posed { LET_GO_MARGIN } else { 0.0 };
    let feet = Vec3A::from(feet);
    let low = feet - Vec3A::new(MODEL_REACH + grow, MODEL_DEPTH + grow, MODEL_REACH + grow);
    let high = feet + Vec3A::new(MODEL_REACH + grow, MODEL_HEIGHT + grow, MODEL_REACH + grow);
    let model = bevy::camera::primitives::Aabb::from_min_max(low.into(), high.into());
    frustum.intersects_obb_identity(&model)
        || shadow.is_some_and(|(sun, floor)| {
            shadow_reaches(frustum, (low, high), sun.forward().as_vec3(), floor)
        })
}

/// Poses a unit's model again (`graph` given), or stops posing it. Unposed,
/// its player has no graph and its skinned meshes no bounds following the
/// joints, so Bevy passes over every joint at once (with a graph it walks each
/// joint through every clip, whatever they weigh) and over the bounds, which
/// hold still with the joints while the model moves with its unit.
fn set_posed(
    commands: &mut Commands,
    (player, pivot): (Entity, Entity),
    graph: Option<&Handle<AnimationGraph>>,
    children: &Query<&Children>,
    skins: &Query<(), With<SkinnedMesh>>,
) {
    use bevy::camera::visibility::DynamicSkinnedMeshBounds;
    let meshes = children
        .iter_descendants(pivot)
        .filter(|e| skins.contains(*e));
    if let Some(graph) = graph {
        commands
            .entity(player)
            .insert(AnimationGraphHandle(graph.clone()));
        for mesh in meshes {
            commands.entity(mesh).insert(DynamicSkinnedMeshBounds);
        }
    } else {
        commands.entity(player).remove::<AnimationGraphHandle>();
        for mesh in meshes {
            commands.entity(mesh).remove::<DynamicSkinnedMeshBounds>();
        }
    }
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
fn weigh(pose: &mut Pose, player: &mut AnimationPlayer, node: AnimationNodeIndex, dt: f32) {
    let step = dt / FADE_SECONDS;
    pose.share = (pose.share + step).min(1.0);
    for fading in &mut pose.fading {
        fading.weight -= step;
        if fading.weight <= 0.0 {
            player.stop(fading.node);
        }
    }
    pose.fading.retain(|f| f.weight > 0.0);
    if let Some(active) = player.animation_mut(node) {
        active.set_seek_time(pose.shown);
        active.set_weight(pose.share);
    }
    for fading in &pose.fading {
        if let Some(active) = player.animation_mut(fading.node) {
            active.set_seek_time(fading.t);
            active.set_weight(fading.weight);
        }
    }
}

/// The clip and time a standing unit shows. `clip` gives a logical clip's
/// duration and point when the model has it.
fn choose(
    core: &Core,
    u: &UnitView,
    drawn: Drawn<'_>,
    pose: &mut Pose,
    dt: f32,
    clip: impl Fn(&str) -> Option<(f32, Option<f32>)>,
) -> Wanted {
    let unit = drawn.unit;
    if u.forced.is_some() {
        pose.swing = None;
        pose.since_strike = None;
        pose.struck_at = None;
        for name in ["flail", "stun"] {
            if let Some((duration, _)) = clip(name) {
                return looping(pose, name, duration, dt, 1.0);
            }
        }
    }
    let rate = f32::from(core.tick_rate().max(1));
    let sub = core.smooth.alpha();
    let tick = core.view().map_or(0, |view| view.tick);
    let snapshot_interval = core.smooth.interval.max(1.0 / rate);
    if let (Some(cast), Some(began)) = (u.cast, core.cast_began(u.id)) {
        // A model without the cast's clip (Dota has none either) goes on
        // with what it was doing.
        let name = cast.ability.map_or(PLAIN_CAST, |id| art_of(id).clip);
        if let Some((duration, point)) = clip(name) {
            // The clip's point lands on the tick the view says the cast goes
            // off, counted from when its cast point was first seen.
            let ran = tick.saturating_sub(began);
            let whole = (ran + cast.goes_off_in) as f32;
            let point = point.unwrap_or(duration * 0.5);
            pose.since_strike = None;
            return Wanted {
                clip: name,
                t: ((ran as f32 + sub) / whole).min(1.0) * point,
            };
        }
    }
    let swing = core
        .view()
        .and_then(|view| {
            view.aggro
                .binary_search_by_key(&u.id, |a| a.unit)
                .ok()
                .and_then(|at| view.aggro[at].swing)
        })
        .filter(|swing| {
            pose.swing.is_some_and(|(target, _)| target == swing.target)
                || swing_left(swing.strike_in, sub, rate) <= snapshot_interval
                || drawn_turn_done(
                    drawn.unit.at,
                    drawn.unit.facing,
                    drawn.target,
                    drawn_turn_rate(u.kind, u.hero, u.neutral),
                )
        });
    if let Some(swing) = swing
        && pose.swing.is_some_and(|(target, _)| target != swing.target)
    {
        pose.swings += 1;
    }
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
        let due = tick.saturating_add(swing.strike_in);
        pose.swing = Some((swing.target, due));
        let windup = (u.attack_point as f32 / 1000.0).max(0.05);
        let left = swing_left(swing.strike_in, sub, rate);
        let point = point.unwrap_or(0.4);
        pose.swing_tempo = point / windup;
        pose.since_strike = Some(0.0);
        return Wanted {
            clip: attack,
            t: (1.0 - left / windup).clamp(0.0, 1.0) * point,
        };
    }
    if let Some((_, due)) = pose.swing.take() {
        pose.swings += 1;
        // A windup gone before its strike tick was broken off (an order, its
        // target falling or getting away): nothing was struck, and Dota cuts
        // the clip there rather than playing the blow's follow-through.
        if tick < due {
            pose.since_strike = None;
            pose.struck_at = None;
        } else {
            pose.struck_at = Some(due);
        }
    }
    if let Some(since) = pose.since_strike.as_mut() {
        *since += dt;
        let last = if pose.swings % 2 == 1 {
            "attack"
        } else {
            "attack2"
        };
        let proc = core.session().and_then(|session| {
            pose.struck_at.and_then(|tick| {
                session
                    .recent_events()
                    .iter()
                    .rev()
                    .take_while(|event| event.tick >= tick.saturating_sub(1))
                    .find_map(|event| match event.kind {
                        EventKind::AbilityCast { caster, ability }
                            if caster == u.id && tick.abs_diff(event.tick) <= 1 =>
                        {
                            art_of(ability).attack
                        }
                        _ => None,
                    })
            })
        });
        let last = proc.filter(|name| clip(name).is_some()).unwrap_or(last);
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
        (
            crate::ability_art::run_clip(&u.effects),
            unit.speed / RUN_PACE,
        )
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

/// Remaining time uses the same presentation clock for the gate and the clip.
fn swing_left(strike_in: u32, sub: f32, rate: f32) -> f32 {
    ((strike_in as f32 - sub) / rate).max(0.0)
}

/// Buildings keep their authored map yaw, so they have no drawn turn to await.
/// Other models follow the simulator's base turn rate; unknown definitions trust it.
fn drawn_turn_rate(
    kind: UnitKind,
    hero: Option<bota_proto::HeroId>,
    neutral: Option<bota_proto::NeutralId>,
) -> u16 {
    if bota_core::kind_def(kind).structure {
        return 0;
    }
    hero.and_then(bota_core::hero_def)
        .map(|hero| hero.unit)
        .or_else(|| {
            neutral
                .and_then(bota_core::NeutralKind::of)
                .map(|neutral| neutral.def())
        })
        .or_else(|| {
            bota_core::FIXED_UNIT_DEFS
                .iter()
                .copied()
                .find(|def| def.kind == kind)
        })
        .map_or(0, |def| def.turn_rate)
}

/// A new windup can wait for the displayed turn. An unknown target position
/// trusts the authoritative check; a zero drawn turn rate needs no visual turn.
/// The caller releases the wait near the strike deadline;
/// ongoing windups follow their target without restarting when its pace changes.
fn drawn_turn_done(from: Vec2, facing: f32, target: Option<Vec2>, turn_rate: u16) -> bool {
    if turn_rate == 0 {
        return true;
    }
    let Some(at) = target else {
        return true;
    };
    let wanted = (at.y - from.y).atan2(at.x - from.x);
    let gap = (wanted - facing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let tolerance =
        f32::from(bota_core::rules::ATTACK_ANGLE_BRADS) * std::f32::consts::TAU / 65536.0;
    gap.abs() <= tolerance
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
/// day or by night; an Outpost a side holds; a Shrine ready to be gathered; a Lotus Pool; a Twin
/// Gate a warp is channelled through.
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
        // Dota's pool plays fountain_channel while a hero plucks; the wire does not tell a
        // pluck under way, so it idles.
        UnitKind::LotusPool => "idle",
        // A Twin Gate a warp is channelled through, at either end, plays its
        // ACT_DOTA_CHANNEL_ABILITY_1 as long as it glows.
        UnitKind::TwinGate if has(EffectId::TWIN_GATE_ACTIVE) => "channel",
        _ => "idle",
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::primitives::Frustum;
    use bevy::math::primitives::ViewFrustum;

    use super::*;

    /// A camera 3000 units over the origin looking straight down with a 25-degree lens: it sees
    /// the ground within ~665 units of it.
    fn view() -> Frustum {
        let projection = Mat4::perspective_rh(25f32.to_radians(), 1.0, 10.0, 10_000.0);
        let camera = Transform::from_xyz(0.0, 3000.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z);
        Frustum(ViewFrustum::from_clip_from_world(
            &(projection * camera.to_matrix().inverse()),
        ))
    }

    /// A unit just past what is posed is not posed, but one posed there is kept posed until it is
    /// past the margin too: one standing at the edge is not posed and let go every frame.
    #[test]
    fn a_unit_at_the_edge_of_the_posed_reach_is_not_let_go_and_posed_again_each_frame() {
        let edge = 665.0 + MODEL_REACH + LET_GO_MARGIN / 2.0;
        let feet = Vec3::new(edge, 0.0, 0.0);
        assert!(
            !in_reach(&view(), feet, false, None),
            "not posed when it comes there"
        );
        assert!(
            in_reach(&view(), feet, true, None),
            "kept posed while it stands there"
        );
        let past = Vec3::new(665.0 + MODEL_REACH + LET_GO_MARGIN * 2.0, 0.0, 0.0);
        assert!(
            !in_reach(&view(), past, true, None),
            "let go past the margin"
        );
    }

    /// A sun 45 degrees up, shining toward `x` (its light falling that way).
    fn sun_toward(x: f32) -> GlobalTransform {
        GlobalTransform::from(Transform::default().looking_to(Vec3::new(x, -1.0, 0.0), Vec3::Y))
    }

    /// A unit whose model is out of the view is posed when its shadow falls into it, and not
    /// when its shadow falls away or there is no shadow at all.
    #[test]
    fn a_unit_out_of_view_is_posed_only_while_its_shadow_falls_into_it() {
        // The box, and its shadow's box with the shadow cull's margin, end short of what the
        // camera sees down to 300 under the ground, where the box's bottom is; its shadow at 45
        // degrees reaches 800 further, into the view.
        let feet = Vec3::new(-(665.0 + MODEL_REACH + 300.0), 0.0, 0.0);
        assert!(
            !in_reach(&view(), feet, false, None),
            "the model alone is out of view"
        );
        let toward = sun_toward(1.0);
        assert!(
            in_reach(&view(), feet, false, Some((&toward, 0.0))),
            "its shadow falls in"
        );
        let away = sun_toward(-1.0);
        assert!(
            !in_reach(&view(), feet, false, Some((&away, 0.0))),
            "its shadow falls away"
        );
    }
}

#[cfg(test)]
mod turn_tests {
    use super::*;

    #[test]
    fn c_turn_zero_rate_has_no_front_but_a_turning_model_does() {
        let behind = Some(Vec2::new(-100.0, 0.0));
        assert_eq!(bota_core::FOUNTAIN.turn_rate, 0);
        assert!(drawn_turn_done(
            Vec2::ZERO,
            0.0,
            behind,
            bota_core::FOUNTAIN.turn_rate
        ));
        let hero = bota_core::hero_def(bota_proto::HeroId::SHADOW_FIEND)
            .expect("hero")
            .unit;
        assert!(hero.turn_rate > 0);
        assert!(!drawn_turn_done(Vec2::ZERO, 0.0, behind, hero.turn_rate));
        assert!(drawn_turn_done(
            Vec2::ZERO,
            std::f32::consts::PI,
            behind,
            hero.turn_rate
        ));
    }

    #[test]
    fn every_structure_kind_has_no_drawn_turn_rate() {
        let mut count = 0;
        for def in bota_core::UNIT_KINDS.iter().filter(|def| def.structure) {
            assert_eq!(
                drawn_turn_rate(def.kind, None, None),
                0,
                "fixed model: {:?}",
                def.kind
            );
            count += 1;
        }
        assert!(count > 0, "the catalogue must contain structures");
        assert!(
            drawn_turn_rate(UnitKind::Hero, Some(bota_proto::HeroId::SHADOW_FIEND), None) > 0,
            "a hero model really turns"
        );
    }

    #[test]
    fn c_turn_an_unknown_target_position_trusts_the_authoritative_facing_check() {
        assert!(drawn_turn_done(
            Vec2::ZERO,
            0.0,
            None,
            bota_core::rules::FAST_TURN_RATE_BRADS
        ));
    }
}
