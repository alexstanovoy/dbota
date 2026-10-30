//! The lines of missiles in flight: an ability's or an item's, drawn as the particle Dota flies
//! for it, from the hand of whoever let it go to where it is. A line at `flight` flies one
//! particle for each missile; one at `chain` flies one for all the missiles a source has flying,
//! to the one farthest from the source (a hook's head, its chain's links lying between).

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bota_client_core::{Flight, flights};
use bota_proto::EntityId;

use super::line::{At, Trigger};
use super::rules::Rules;
use super::{Feeds, feed, place_of};
use crate::effects::{Shooters, steer};
use crate::map::Ground;
use crate::particles::{Handle, Library, Playing};
use crate::session::Session;

/// How high missiles fly over the ground, in world units.
const FLIGHT: f32 = 110.0;
/// The control points a flight's particle is given.
const LAUNCH: usize = 0;
const TIP: usize = 1;

/// The particles flying: by rule and by the missile or the source they follow.
#[derive(Resource, Default)]
struct Flying(HashMap<(usize, EntityId), Handle>);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Flying>()
        .add_systems(Update, flies.after(crate::units::Placed));
}

/// Where a flight is, in the world's frame.
fn at(ground: &Ground, flight: &Flight) -> Vec3 {
    Vec3::new(flight.x, ground.at(flight.x, flight.y) + FLIGHT, -flight.y)
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
    session: NonSend<Session>,
    ground: Res<Ground>,
    rules: Res<Rules>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut flying: ResMut<Flying>,
    bodies: Shooters,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let shown = flights(view, core.prev_view(), core.smooth.alpha());
    let mut alive: HashSet<(usize, EntityId)> = HashSet::new();
    for (n, rule) in rules.of(Trigger::Flies) {
        let fx = &rule.fx;
        let own = shown.iter().filter(|f| rule.names(f.missile.inflictor));
        let stands = |id| view.unit(id).map(|u| (u.pos.x.to_f32(), u.pos.y.to_f32()));
        for (key, flight) in followed(fx.at == At::Chain, own, stands) {
            alive.insert((n, key));
            let tip = at(&ground, flight);
            let dir = Vec3::new(flight.x - flight.tail_x, 0.0, -(flight.y - flight.tail_y));
            let hand = flight
                .missile
                .source
                .and_then(|source| bodies.launch(source))
                .unwrap_or(tip);
            let handle = match flying.0.get(&(n, key)).copied() {
                Some(handle) => handle,
                None => {
                    let Some(handle) = fx
                        .particle
                        .as_deref()
                        .and_then(|name| library.as_deref()?.effect(name))
                        .and_then(|effect| playing.hold(effect, key.idx ^ key.generation))
                    else {
                        continue;
                    };
                    flying.0.insert((n, key), handle);
                    handle
                }
            };
            let Some(instance) = playing.get_mut(handle) else {
                continue;
            };
            instance.set_control_point(LAUNCH, place_of(hand), Vec3::ZERO, false);
            if fx.at == At::Chain {
                instance.set_control_point(TIP, place_of(tip), Vec3::ZERO, false);
            } else {
                steer(tip + dir, dir, &mut playing, handle);
            }
            feed(fx, Some(handle), &mut playing, &Feeds::default(), &|_| tip);
        }
    }
    flying.0.retain(|key, handle| {
        let keep = alive.contains(key);
        if !keep && let Some(instance) = playing.get_mut(*handle) {
            instance.end();
        }
        keep
    });
}
