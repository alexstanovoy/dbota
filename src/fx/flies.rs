//! The lines of missiles in flight: an ability's or an item's, drawn as the particle Dota flies
//! for it, from the hand of whoever let it go to where it is. A line at `flight` flies one
//! particle for each missile; one at `chain` flies one for all the missiles a source has flying,
//! to the one farthest from the source (a hook's head, its chain's links lying between).

use std::collections::{HashMap, HashSet};

use bevy::{prelude::*, transform::TransformSystems};
use bota_client_core::Flight;
use bota_proto::EntityId;
use heatel::particle::{FloatInput, Operator};

use super::line::{At, Attach, Trigger};
use super::rules::Rules;
use super::{Feeds, attach_point, feed, place_of};
use crate::effects::{MISSILE_HEIGHT, Shooters, steer};
use crate::map::Ground;
use crate::particles::{EffectSight, Handle, Library, Playing};

/// The control points a flight's particle is given.
const LAUNCH: usize = 0;
const TIP: usize = 1;

/// The particles flying: by rule and by the missile or the source they follow.
#[derive(Resource, Default)]
struct Flying(HashMap<(usize, EntityId), Handle>);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Flying>()
        .add_systems(
            First,
            crate::loading::reset::<Flying>.in_set(crate::loading::Begin),
        )
        .add_systems(
            PostUpdate,
            flies
                .after(TransformSystems::Propagate)
                .before(crate::particles::Simulation),
        );
}

/// Where a flight is, in the world's frame.
fn at(ground: &Ground, flight: &Flight, height: f32) -> Vec3 {
    Vec3::new(flight.x, ground.at(flight.x, flight.y) + height, -flight.y)
}

/// A directly placed chain tip uses the ground offset authored for its root particle.
fn chain_height(effect: &dbota_particles::Effect) -> f32 {
    effect.defs[0]
        .system
        .operators
        .iter()
        .find_map(|operator| {
            let Operator::MovementPlaceOnGround(movement) = &operator.class else {
                return None;
            };
            match movement.offset {
                FloatInput::Literal(height) if !movement.scale_offset => Some(height),
                _ => None,
            }
        })
        .unwrap_or(MISSILE_HEIGHT)
}

/// The missile of each key a rule follows: itself, or for a chain the one farthest from its
/// source, `stands` being where the source stands in the sim's frame.
fn followed<'a>(
    chain: bool,
    shown: impl Iterator<Item = &'a Flight>,
    stands: impl Fn(EntityId) -> Option<(f32, f32)>,
) -> HashMap<EntityId, &'a Flight> {
    let mut keyed: HashMap<EntityId, (f32, &Flight)> = HashMap::new();
    for flight in shown {
        let (key, reach) = match (chain, flight.missile.source) {
            (false, _) => (flight.missile.id, 0.0),
            (true, Some(source)) => {
                // A chain's source that is not in the view has no end to hold the chain by.
                let Some((x, y)) = stands(source) else {
                    continue;
                };
                (source, (flight.x - x).powi(2) + (flight.y - y).powi(2))
            }
            (true, None) => continue,
        };
        if keyed.get(&key).is_none_or(|(held, _)| reach > *held) {
            keyed.insert(key, (reach, flight));
        }
    }
    keyed.into_iter().map(|(key, (_, f))| (key, f)).collect()
}

/// Keeps one particle for every missile or source each flight rule follows: started when the
/// missile first shows, held at the hand and the missile while it flies, ended when it is gone.
#[allow(clippy::too_many_arguments)]
fn flies(
    sight: crate::fog::EventSight,
    ground: Res<Ground>,
    rules: Res<Rules>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut flying: ResMut<Flying>,
    bodies: Shooters,
) {
    if sight.core().view().is_none() {
        return;
    }
    let shown = sight.flights();
    let mut alive: HashSet<(usize, EntityId)> = HashSet::new();
    for (n, rule) in rules.of(Trigger::Flies) {
        let fx = &rule.fx;
        let Some(effect) = fx
            .particle
            .as_deref()
            .and_then(|name| library.as_deref()?.effect(name))
        else {
            continue;
        };
        let height = if fx.at == At::Chain {
            chain_height(effect)
        } else {
            MISSILE_HEIGHT
        };
        let own = shown.iter().filter(|f| rule.names(f.missile.inflictor));
        let stands = |id| {
            sight
                .visible_unit(id)
                .map(|u| (u.pos.x.to_f32(), u.pos.y.to_f32()))
        };
        for (key, flight) in followed(fx.at == At::Chain, own, stands) {
            alive.insert((n, key));
            let tip = at(&ground, flight, height);
            let dir = Vec3::new(flight.x - flight.tail_x, 0.0, -(flight.y - flight.tail_y));
            let source = flight
                .missile
                .source
                .and_then(|source| sight.visible_unit(source));
            let handle = match flying.0.get(&(n, key)).copied() {
                Some(handle) => handle,
                None => {
                    let Some(handle) = playing.hold(
                        effect,
                        key.idx ^ key.generation,
                        EffectSight::Projectile(flight.missile.id),
                    ) else {
                        continue;
                    };
                    flying.0.insert((n, key), handle);
                    handle
                }
            };
            playing.note_flight(
                handle,
                &flight.missile,
                sight.knowledge(flight.missile.seen_by),
            );
            let Some(instance) =
                playing.get_mut_with_sight(handle, EffectSight::Projectile(flight.missile.id))
            else {
                continue;
            };
            let hand = source.map_or(tip, |unit| match &fx.attach {
                Attach::Origin => bodies.launch(unit.id).unwrap_or(tip),
                attach => attach_point(&ground, &bodies, unit, attach),
            });
            instance.set_control_point(LAUNCH, place_of(hand), Vec3::ZERO, false);
            if fx.at == At::Chain {
                instance.set_control_point(TIP, place_of(tip), Vec3::ZERO, false);
            } else {
                steer(tip + dir, dir, &mut playing, handle);
            }
            let feeds = Feeds {
                caster: source.map(|unit| attach_point(&ground, &bodies, unit, &Attach::Origin)),
                caster_scale: source.map(|unit| bodies.object_scale(unit.id)),
                flight_tip: tip,
                flight_speed: dir.length() * bota_core::rules::TICKS_PER_SECOND as f32,
                ..Feeds::default()
            };
            feed(fx, Some(handle), &mut playing, &feeds, &|attach| {
                source.map_or(tip, |unit| attach_point(&ground, &bodies, unit, attach))
            });
        }
    }
    flying.0.retain(|key, handle| {
        if alive.contains(key) {
            return true;
        }
        // A tether has no geometry once its travelling endpoint ceases to exist.
        if rules
            .0
            .get(key.0)
            .is_some_and(|rule| rule.fx.at == At::Chain)
        {
            playing.cancel(*handle);
        } else {
            playing.finish_flight(*handle, &sight);
        }
        false
    });
}
