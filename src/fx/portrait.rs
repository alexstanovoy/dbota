//! The portrait's effects. Dota draws a model in its portrait with the particles it wears unless
//! its entry of `portraits.txt` sets `PortraitHideParticles` (no hero's does), so Shadow Fiend's
//! portrait shows his glow, his trail and his eyes as his model in the world does. Each effect a line puts on a
//! unit for as long as it stands (the `worn` lines of `hero.<name>` and `unit.<npc name>`)
//! plays on the copy of the panel's unit's model on the portrait's stage too, at the copy's own
//! nodes and scale, on the portrait's layer alone and stepped as often as the portrait is drawn
//! (see `particles::Instance::portrait`).

use bevy::{prelude::*, transform::TransformSystems};
use bota_proto::{EntityId, UnitView};

use super::line::{Attach, Trigger};
use super::rules::{Rules, Subject};
use super::{Feeds, PLACE, feed, place_of};
use crate::particles::{Handle, Library, Playing};
use crate::session::Session;

/// The effects playing on the stage, by the rule, and what they play for: the panel's unit and
/// the copy of its model on the stage.
#[derive(Resource, Default)]
pub struct StageFx {
    wearer: Option<(EntityId, Entity)>,
    playing: Vec<(usize, Handle)>,
}

impl StageFx {
    /// The effects playing on the portrait's stage.
    #[cfg(test)]
    pub fn handles(&self) -> impl Iterator<Item = Handle> + '_ {
        self.playing.iter().map(|(_, handle)| *handle)
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<StageFx>().add_systems(
        PostUpdate,
        stage_fx
            .after(TransformSystems::Propagate)
            .before(crate::particles::Simulation),
    );
}

/// The copy on the stage (where it stands): its pivot, the frames of its named nodes and its scale.
struct Stand<'a, 'w, 's> {
    pivot: Entity,
    size: f32,
    tree: &'a Query<'w, 's, &'static Children>,
    names: &'a Query<'w, 's, &'static Name>,
    places: &'a Query<'w, 's, &'static GlobalTransform>,
}

impl Stand<'_, '_, '_> {
    fn node(&self, name: &str) -> Option<GlobalTransform> {
        let node = self
            .tree
            .iter_descendants(self.pivot)
            .find(|e| self.names.get(*e).is_ok_and(|n| n.as_str() == name))?;
        self.places.get(node).ok().copied()
    }

    /// Where `attach` is on the copy: its feet (the pivot), over its head, or one of its nodes.
    fn point(&self, unit: &UnitView, attach: &Attach) -> Option<Vec3> {
        let feet = self.places.get(self.pivot).ok()?.translation();
        let over = Vec3::Y * crate::bars::bar_height(unit.kind);
        Some(match attach {
            Attach::Origin => feet,
            Attach::Overhead => feet + over,
            Attach::Node(name) => self.node(name)?.translation(),
        })
    }

    /// The axes of `attach`, in the particles' frame, as `fx::attach_basis` gives a unit's: a
    /// node's own Source axes, or the copy's facing (Bevy forward, the sim's north).
    fn basis(&self, attach: &Attach) -> Mat3 {
        if let Attach::Node(name) = attach
            && let Some(frame) = self.node(name)
        {
            let rotation = frame.to_scale_rotation_translation().1;
            return Mat3::from_cols(
                place_of(rotation * Vec3::X),
                place_of(rotation * Vec3::Y),
                place_of(rotation * Vec3::Z),
            );
        }
        Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2)
    }
}

/// Starts, places and ends the effects the panel's unit wears on its copy on the stage: all end
/// when the stage shows another copy or none.
#[allow(clippy::too_many_arguments)]
fn stage_fx(
    session: NonSend<Session>,
    stage: Res<crate::portrait::Stage>,
    framings: Res<crate::portrait::Framings>,
    bank: Res<crate::units::ModelBank>,
    rules: Res<Rules>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut fx: ResMut<StageFx>,
    tree: Query<&'static Children>,
    names: Query<&'static Name>,
    places: Query<&'static GlobalTransform>,
) {
    let actor = stage.actor.as_ref().filter(|a| a.posed());
    let model = actor.map(|a| bank.0[a.model()].model.logical.as_str());
    let unit = stage
        .unit()
        .and_then(|id| session.0.view()?.unit(id))
        .filter(|_| model.is_some_and(|m| !framings.hides_particles(m)));
    // Another unit of the same model is shown on the same copy: what it wears is its own.
    let wearer = unit.zip(actor).map(|(u, a)| (u.id, a.pivot));
    if fx.wearer != wearer {
        for (_, handle) in fx.playing.drain(..) {
            if let Some(instance) = playing.get_mut(handle) {
                instance.end();
            }
        }
        fx.wearer = wearer;
    }
    let (Some(actor), Some(unit), Some(library)) = (actor, unit, library) else {
        return;
    };
    let copy = Stand {
        pivot: actor.pivot,
        size: actor.size(),
        tree: &tree,
        names: &names,
        places: &places,
    };
    for (n, rule) in rules.of(Trigger::Worn) {
        let Subject::Units(units) = &rule.subject else {
            continue;
        };
        let line = &rule.fx;
        let attaches = line.feeds.iter().filter_map(|(_, what)| match what {
            super::line::Feed::Place(attach) => Some(attach),
            _ => None,
        });
        let placed = std::iter::once(&line.attach)
            .chain(attaches)
            .all(|attach| copy.point(unit, attach).is_some());
        if !units.is(unit) || !placed {
            continue;
        }
        let handle = match fx.playing.iter().find(|(rule, _)| *rule == n) {
            Some((_, handle)) => Some(*handle),
            None => start(line.particle.as_deref(), &library, &mut playing).inspect(|h| {
                fx.playing.push((n, *h));
            }),
        };
        let Some(instance) = handle.and_then(|h| playing.get_mut(h)) else {
            continue;
        };
        let at = copy.point(unit, &line.attach).unwrap_or_default();
        instance.bind_control_point(PLACE, place_of(at), copy.basis(&line.attach), copy.size);
        let place = |attach: &Attach| copy.point(unit, attach).unwrap_or(at);
        feed(line, handle, &mut playing, &Feeds::default(), &place);
    }
}

/// A held effect of the named particle, seen by the portrait's camera alone.
fn start(name: Option<&str>, library: &Library, playing: &mut Playing) -> Option<Handle> {
    let effect = library.effect(name?)?;
    playing.hold_on_stage(effect, 0)
}
