//! The lines the events of the last pump fire: an item used, an ability cast, a throw come down,
//! a blow dealt or a mending given, a unit dead.

use bevy::prelude::*;
use bota_proto::{
    EntityId, EventKind, Inflictor, ItemId, RuneKind, UnitView, Vec2 as Spot, WorldView,
};

use super::line::{At, Attach, Trigger};
use super::rules::{Rule, Rules, Subject};
use super::{Feeds, Stage, attach_point, carried, item_feeds, on_ground};
use crate::effects::Shooters;
use crate::map::Ground;
use crate::particles::{Library, Playing};
use crate::session::Session;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, told.after(crate::units::Placed));
}

/// What an event says happened, in the terms the lines are keyed by.
struct Fact {
    trigger: Trigger,
    /// The ability or item the event is about.
    by: Option<Inflictor>,
    /// The unit a `unit.` subject is checked against: who struck an attack's blow, who died.
    named: Option<EntityId>,
    /// The user, the dealer of the blow.
    caster: Option<EntityId>,
    /// The unit the fact is about: the one struck, the one dead.
    unit: Option<EntityId>,
    /// The spot a use was aimed at, a throw came down on, a rune appeared at.
    point: Option<Spot>,
    /// The kind of rune the event is about.
    rune: Option<RuneKind>,
}

impl Fact {
    fn of(event: &EventKind) -> Option<Fact> {
        let blank = Fact {
            trigger: Trigger::Use,
            by: None,
            named: None,
            caster: None,
            unit: None,
            point: None,
            rune: None,
        };
        Some(match event {
            EventKind::ItemUsed { caster, item, at } => Fact {
                by: Some(Inflictor::Item(*item)),
                caster: *caster,
                point: *at,
                ..blank
            },
            EventKind::AbilityCast { caster, ability } => Fact {
                trigger: Trigger::Cast,
                by: Some(Inflictor::Ability(*ability)),
                caster: Some(*caster),
                ..blank
            },
            EventKind::Landed {
                inflictor,
                caster,
                at,
            } => Fact {
                trigger: Trigger::Land,
                by: Some(*inflictor),
                caster: *caster,
                point: Some(*at),
                ..blank
            },
            EventKind::Damaged {
                source,
                target,
                inflictor,
                ..
            } => Fact {
                trigger: Trigger::Hit,
                by: *inflictor,
                // An ability's blow is its own: only an attack's belongs to the unit that struck.
                named: source.filter(|_| inflictor.is_none()),
                caster: *source,
                unit: Some(*target),
                ..blank
            },
            EventKind::Healed {
                source,
                target,
                inflictor,
                ..
            } => Fact {
                trigger: Trigger::Hit,
                by: *inflictor,
                caster: *source,
                unit: Some(*target),
                ..blank
            },
            EventKind::Died { unit, .. } => Fact {
                trigger: Trigger::Died,
                named: Some(*unit),
                unit: Some(*unit),
                ..blank
            },
            EventKind::RuneSpawned { kind, at } => Fact {
                trigger: Trigger::Spawn,
                rune: Some(*kind),
                point: Some(*at),
                ..blank
            },
            EventKind::RuneActivated { unit, kind, .. } => Fact {
                trigger: Trigger::Activate,
                rune: Some(*kind),
                caster: Some(*unit),
                ..blank
            },
            // The one item a rune is stored in.
            EventKind::RuneBottled { unit, .. } => Fact {
                trigger: Trigger::Store,
                by: Some(Inflictor::Item(ItemId::BOTTLE)),
                caster: Some(*unit),
                ..blank
            },
            // The pool, which every side is told of; the hero, to a side that sees it.
            EventKind::LotusPlucked { pool, unit, .. } => Fact {
                trigger: Trigger::Pluck,
                named: Some(*pool),
                caster: *unit,
                unit: Some(*pool),
                ..blank
            },
            _ => return None,
        })
    }
}

/// The views the places of a fact are read from.
struct Views<'a> {
    now: &'a WorldView,
    before: Option<&'a WorldView>,
}

impl Views<'_> {
    /// A unit in the view, or in the one before when it has left it (a dead unit's last place).
    fn find(&self, id: EntityId) -> Option<&UnitView> {
        self.now
            .unit(id)
            .or_else(|| self.before.and_then(|v| v.unit(id)))
    }
}

/// Whether the rule is about the fact.
fn about(rule: &Rule, fact: &Fact, views: &Views) -> bool {
    if rule.fx.trigger != fact.trigger {
        return false;
    }
    match &rule.subject {
        Subject::Items(_) | Subject::Abilities(_) => rule.names(fact.by),
        Subject::Units(units) => fact
            .named
            .and_then(|id| views.find(id))
            .is_some_and(|unit| units.is(unit)),
        Subject::Runes(kind) => fact.rune == Some(*kind),
        Subject::Effects(_) => false,
    }
}

/// What the item a use is about gives lines to feed: the attribute it is set to now, the share of
/// its charges it held before the use spent them.
fn item_state(fact: &Fact, views: &Views) -> Feeds {
    let Some(Inflictor::Item(item)) = fact.by else {
        return Feeds::default();
    };
    let caster = fact.caster;
    let now = carried(caster.and_then(|id| views.now.unit(id)), item);
    let before = carried(
        caster.zip(views.before).and_then(|(id, v)| v.unit(id)),
        item,
    );
    Feeds {
        mode: item_feeds(now.or(before)).0,
        charges: item_feeds(before.or(now)).1,
        ..Feeds::default()
    }
}

/// Fires the lines of every fact the last pump told.
fn told(
    session: NonSend<Session>,
    ground: Res<Ground>,
    rules: Res<Rules>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    bodies: Shooters,
    mut stage: Stage,
) {
    let core = &session.0;
    let Some(now) = core.view() else {
        return;
    };
    let views = Views {
        now,
        before: core.prev_view(),
    };
    for (n, event) in core.told.iter().enumerate() {
        let Some(fact) = Fact::of(event) else {
            continue;
        };
        let seed = now.tick.wrapping_mul(31).wrapping_add(n as u32);
        for (_, rule) in rules
            .of(fact.trigger)
            .filter(|(_, r)| about(r, &fact, &views))
        {
            let fx = &rule.fx;
            let unit = match fx.at {
                At::Caster => fact.caster.and_then(|id| now.unit(id)),
                At::CasterBefore => fact
                    .caster
                    .zip(views.before)
                    .and_then(|(id, before)| before.unit(id)),
                At::Unit => fact.unit.and_then(|id| views.find(id)),
                _ => None,
            };
            let at = match unit {
                Some(unit) => Some(attach_point(&ground, &bodies, unit, &fx.attach)),
                None if fx.at == At::Point => fact.point.map(|spot| on_ground(&ground, spot)),
                None => None,
            };
            if let Some(at) = at {
                let place = |attach: &Attach| match unit {
                    Some(unit) => attach_point(&ground, &bodies, unit, attach),
                    None => at,
                };
                let feeds = Feeds {
                    caster: fact
                        .caster
                        .and_then(|id| views.find(id))
                        .map(|u| attach_point(&ground, &bodies, u, &Attach::Origin)),
                    ..item_state(&fact, &views)
                };
                let with = (library.as_deref(), &mut *playing);
                stage.play(fx, (at, seed), with, (&feeds, &place));
            }
        }
    }
}
