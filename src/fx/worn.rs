//! The lines of what units wear: an effect a modifier, an ability or an item put on one, or a unit
//! of a Dota name standing; and of the runes lying in sight. From the moment a unit shows it until
//! it goes, its particle follows the unit and is fed what the line asks; its sound is made once.

use std::collections::HashMap;

use bevy::prelude::*;
use bota_core::rules::TICKS_PER_SECOND;
use bota_proto::{EntityId, UnitView};

use super::line::{Attach, Fx, Trigger, Whose};
use super::rules::{Rule, Rules, Subject};
use super::{
    Feeds, PLACE, Stage, attach_point, carried, feed, item_feeds, nodes_ready, on_ground, place_of,
};
use crate::effects::Shooters;
use crate::fog::FogGrid;
use crate::map::Ground;
use crate::mixer::{Owner, Started};
use crate::particles::{Handle, Library, Playing};
use crate::runes;
use crate::session::Session;
use crate::units::{Dying, UnitIndex};

/// The worn effects playing: by the wearing unit and the rule, the particle if the line has one
/// and the loop of its sound, which sounds for as long as the effect is worn.
#[derive(Resource, Default)]
struct Worn(HashMap<(EntityId, usize), Wearing>);

/// What one worn effect plays.
#[derive(Clone, Copy)]
struct Wearing {
    particle: Option<Handle>,
    /// The voice of a sound that loops: owned by the wearing, and stopped when it ends. A sound
    /// that plays once plays out by itself, whatever becomes of the wearing.
    voice: Option<Entity>,
    /// Whether the sound loops but has not started yet (it was out of earshot): tried again
    /// while the effect is worn.
    waiting: bool,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Worn>()
        .add_systems(Update, worn.after(crate::units::Placed));
}

/// What a unit holds of a rule's subject.
struct Hold {
    feeds: Feeds,
}

/// What `unit` holds of the rule's subject, if it wears it, or carries it, as the line asks.
fn holding(rule: &Rule, unit: &UnitView) -> Option<Hold> {
    let fx = &rule.fx;
    let held = |stacks, ticks_left: Option<u32>, feeds| Hold {
        feeds: Feeds {
            sight: unit.true_sight_radius.to_f32(),
            stacks: stacks as f32,
            seconds: ticks_left.map_or(0.0, |ticks| ticks as f32 / TICKS_PER_SECOND as f32),
            ..feeds
        },
    };
    match (&rule.subject, fx.trigger) {
        (Subject::Units(units), _) => units.is(unit).then(|| held(0, None, Feeds::default())),
        (Subject::Abilities(ids), Trigger::Casting) => unit
            .cast
            .and_then(|cast| cast.ability)
            .is_some_and(|ability| ids.contains(&ability))
            .then(|| held(0, None, Feeds::default())),
        (Subject::Items(ids), Trigger::Held) => ids.iter().find_map(|id| {
            let item = carried(Some(unit), *id)?;
            let (mode, charges) = item_feeds(Some(item));
            Some(held(
                0,
                None,
                Feeds {
                    mode,
                    charges,
                    ..Feeds::default()
                },
            ))
        }),
        _ => unit.effects.iter().find_map(|effect| {
            let on = match &rule.subject {
                Subject::Effects(ids) => ids.contains(&effect.id),
                _ => rule.names(effect.inflictor),
            };
            let stacks = effect.stacks.unwrap_or(0);
            let own = effect.caster == Some(unit.id);
            let whose = match fx.whose {
                Whose::Any => true,
                Whose::Own => own,
                Whose::Foreign => !own,
            };
            let fits = on && whose && (fx.stacks.0..=fx.stacks.1).contains(&stacks);
            fits.then(|| held(stacks, effect.ticks_left, Feeds::default()))
        }),
    }
}

/// Starts what a unit or a rune (`id`) that has come to wear a rule's subject wears: its
/// particle, held on until it is ended, and its sound, which a loop keeps until the wearing ends.
fn begin(
    fx: &Fx,
    id: EntityId,
    at: Vec3,
    (library, playing): (Option<&Library>, &mut Playing),
    stage: &mut Stage,
) -> Wearing {
    let seed = id.idx ^ id.generation.rotate_left(16);
    let handle = fx
        .particle
        .as_deref()
        .and_then(|name| library?.effect(name))
        .and_then(|effect| playing.hold(effect, seed));
    let mut wearing = Wearing {
        particle: handle,
        voice: None,
        waiting: fx.sound.is_some(),
    };
    sound(fx, id, at, &mut wearing, stage);
    wearing
}

/// Sounds a wearing's sound until it has started (or plays out): a loop out of earshot is tried
/// again every frame; a loop sounding follows its wearer.
fn sound(fx: &Fx, id: EntityId, at: Vec3, wearing: &mut Wearing, stage: &mut Stage) {
    let Some(prefix) = fx.sound.as_deref() else {
        return;
    };
    if let Some(voice) = wearing.voice {
        stage.follow(voice, at);
        return;
    }
    if !wearing.waiting {
        return;
    }
    let seed = id.idx ^ id.generation.rotate_left(16);
    match stage.sound(prefix, at, seed, Owner::Unit(id)) {
        Started::Loop(voice) => {
            wearing.voice = Some(voice);
            wearing.waiting = false;
        }
        Started::Once => wearing.waiting = false,
        // Out of earshot or blocked: a one-shot is lost, a loop is tried again.
        Started::Refused => wearing.waiting = stage.loops(prefix),
    }
}

/// Ends what a wearing played: its particle lives out its life, its loop fades out.
fn end(wearing: &Wearing, playing: &mut Playing, stage: &mut Stage) {
    if let Some(instance) = wearing.particle.and_then(|h| playing.get_mut(h)) {
        instance.end();
    }
    if let Some(voice) = wearing.voice {
        stage.silence(voice);
    }
}

/// Starts what each unit that has come to wear a rule's subject wears, and each rune that lies in
/// sight, keeps its particle at the unit or the rune and fed, and ends it when the unit stops
/// wearing it, falls or leaves the view, or the rune is taken or leaves the sight. With no match
/// in view nothing stands: every wearing ends.
#[allow(clippy::too_many_arguments)]
fn worn(
    session: NonSend<Session>,
    index: Res<UnitIndex>,
    dying: Query<(), With<Dying>>,
    ground: Res<Ground>,
    fog: Res<FogGrid>,
    rules: Res<Rules>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut worn: ResMut<Worn>,
    bodies: Shooters,
    mut stage: Stage,
) {
    let Some(view) = session.0.view() else {
        for (_, wearing) in worn.0.drain() {
            end(&wearing, &mut playing, &mut stage);
        }
        stage.silence_loops();
        return;
    };
    let standing = |id: &EntityId| {
        index
            .0
            .get(id)
            .is_some_and(|entity| !dying.contains(*entity))
    };
    let wearing = rules
        .of(Trigger::Worn)
        .chain(rules.of(Trigger::Held))
        .chain(rules.of(Trigger::Casting))
        .collect::<Vec<_>>();
    if wearing.is_empty() {
        return;
    }
    worn.0.retain(|(id, rule), wearing| {
        let keep = rules.0.get(*rule).is_some_and(|rule| match rule.subject {
            Subject::Runes(kind) => {
                runes::shown(view, &fog).any(|rune| rune.id == *id && rune.kind == kind)
            }
            _ => {
                standing(id)
                    && view
                        .unit(*id)
                        .is_some_and(|unit| holding(rule, unit).is_some())
            }
        });
        if !keep {
            end(wearing, &mut playing, &mut stage);
        }
        keep
    });
    let scene = Scene {
        ground: &ground,
        bodies: &bodies,
        library: library.as_deref(),
    };
    for unit in view.units.iter().filter(|u| standing(&u.id)) {
        wear_unit(unit, &wearing, &scene, &mut worn, &mut playing, &mut stage);
    }
    for rune in runes::shown(view, &fog) {
        let at = on_ground(&ground, rune.pos);
        for (n, rule) in &wearing {
            if rule.subject != Subject::Runes(rune.kind) {
                continue;
            }
            let handle = worn.0.entry((rune.id, *n)).or_insert_with(|| {
                let with = (library.as_deref(), &mut *playing);
                begin(&rule.fx, rune.id, at, with, &mut stage)
            });
            sound(&rule.fx, rune.id, at, handle, &mut stage);
            if let Some(instance) = handle.particle.and_then(|h| playing.get_mut(h)) {
                instance.set_control_point(PLACE, place_of(at), Vec3::ZERO, false);
            }
        }
    }
}

/// What the effects worn are placed with.
struct Scene<'a> {
    ground: &'a Ground,
    bodies: &'a Shooters<'a, 'a>,
    library: Option<&'a Library>,
}

/// Starts and keeps what one unit wears of each rule it holds.
fn wear_unit(
    unit: &UnitView,
    wearing: &[(usize, &Rule)],
    scene: &Scene,
    worn: &mut Worn,
    playing: &mut Playing,
    stage: &mut Stage,
) {
    for (n, rule) in wearing {
        let Some(held) = holding(rule, unit) else {
            continue;
        };
        let fx = &rule.fx;
        if !worn.0.contains_key(&(unit.id, *n)) && !nodes_ready(fx, scene.bodies, unit) {
            continue;
        }
        let at = attach_point(scene.ground, scene.bodies, unit, &fx.attach);
        let handle = worn
            .0
            .entry((unit.id, *n))
            .or_insert_with(|| begin(fx, unit.id, at, (scene.library, &mut *playing), stage));
        sound(fx, unit.id, at, handle, stage);
        if let Some(instance) = handle.particle.and_then(|h| playing.get_mut(h)) {
            instance.set_control_point(PLACE, place_of(at), Vec3::ZERO, false);
        }
        let place = |attach: &Attach| attach_point(scene.ground, scene.bodies, unit, attach);
        feed(fx, handle.particle, playing, &held.feeds, &place);
    }
}
