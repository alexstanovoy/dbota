//! The lines the events of the last pump fire: an item used, an ability cast, a throw come down,
//! a blow dealt or a mending given, a unit dead.

use bevy::{prelude::*, transform::TransformSystems};
use bota_proto::{
    EntityId, EventKind, Inflictor, ItemId, RuneKind, UnitView, Vec2 as Spot, WorldView,
};

use super::line::{At, Attach, Trigger};
use super::rules::{Rule, Rules, Subject};
use super::{Feeds, Stage, attach_point, carried, item_feeds, on_ground};
use crate::effects::Shooters;
use crate::fog::EventSubject;
use crate::map::Ground;
use crate::particles::{Library, Playing};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        told.after(crate::units::Placed).in_set(super::PresentFacts),
    )
    .add_systems(
        PostUpdate,
        follow
            .after(TransformSystems::Propagate)
            .before(dbota_particles::Step),
    );
}

/// One event-started effect whose origin continues to follow a unit.
#[derive(Component)]
pub(super) struct Following {
    pub handle: crate::particles::Handle,
    pub unit: EntityId,
    pub attach: Attach,
}

fn follow(
    sight: crate::fog::EventSight,
    following: Query<(Entity, &Following)>,
    bodies: Shooters,
    ground: Res<Ground>,
    mut playing: ResMut<Playing>,
    mut commands: Commands,
) {
    for (entity, binding) in &following {
        let Some(instance) = playing.get_mut(binding.handle) else {
            commands.entity(entity).despawn();
            continue;
        };
        if let Some(unit) = sight.visible_unit(binding.unit) {
            let at = attach_point(&ground, &bodies, unit, &binding.attach);
            instance.bind_control_point(
                super::PLACE,
                super::place_of(at),
                super::attach_basis(&bodies, unit, &binding.attach),
                bodies.object_scale(unit.id),
            );
        }
    }
}

/// What an event says happened, in the terms the lines are keyed by.
#[derive(Clone)]
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
    /// The spot a use was aimed at, a throw came down on, a rune appeared at, a warp left from.
    point: Option<Spot>,
    /// The kind of rune the event is about.
    rune: Option<RuneKind>,
}

impl Fact {
    fn of(event: &EventKind, views: &Views) -> Option<Fact> {
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
            EventKind::ItemUsed {
                caster, item, at, ..
            } => Fact {
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
                ..
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
            } => {
                if inflictor.is_some_and(|by| {
                    matches!(by, Inflictor::Item(item)
                    if recovery(item).is_some())
                }) {
                    return None;
                }
                Fact {
                    trigger: Trigger::Hit,
                    by: *inflictor,
                    caster: *source,
                    unit: Some(*target),
                    ..blank
                }
            }
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
            // Told to both sides, the hero that took it only to its own.
            EventKind::Captured { unit, by, .. } => Fact {
                trigger: Trigger::Captured,
                named: Some(*unit),
                caster: *by,
                unit: Some(*unit),
                ..blank
            },
            // A warp's fact is about the gate it came out of: `at=unit` plays at that gate (the
            // unit that came out is the caster, `at=caster`), `at=point` at the gate it left.
            EventKind::GateWarped { from, to, unit } => Fact {
                trigger: Trigger::Warp,
                named: Some(*to),
                caster: *unit,
                unit: Some(*to),
                point: views.find(*from).map(|gate| gate.pos),
                ..blank
            },
            _ => return None,
        })
    }
}

fn recovery(item: ItemId) -> Option<bota_core::AreaSupport> {
    match bota_core::item_def(item)?.effect {
        bota_core::UseSpec::Support(support)
            if (support.health > 0 || support.mana > 0)
                && support.reaches == bota_core::Reach::Combatants =>
        {
            Some(support)
        }
        _ => None,
    }
}

// Recovery cues describe the visible use area even when no pool needed restoring.
fn presentation_facts(event: &EventKind, views: &Views) -> Vec<Fact> {
    let mut facts: Vec<_> = Fact::of(event, views).into_iter().collect();
    let EventKind::ItemUsed {
        caster: Some(caster),
        item,
        ..
    } = event
    else {
        return facts;
    };
    let Some(support) = recovery(*item) else {
        return facts;
    };
    let Some(source) = views.now.unit(*caster) else {
        return facts;
    };
    let radius = bota_core::rules::units(bota_core::widened(
        support.radius,
        bota_client_core::carried_of(source).aoe_bonus,
    ));
    let Some(used) = facts.first().cloned() else {
        return facts;
    };
    for target in &views.now.units {
        let kind = bota_core::kind_def(target.kind);
        if target.hp > 0
            && target.team == source.team
            && (target.kind == bota_proto::UnitKind::Hero || kind.creep)
            && target.pos.within(source.pos, radius)
        {
            facts.push(Fact {
                trigger: Trigger::Hit,
                unit: Some(target.id),
                ..used.clone()
            });
        }
    }
    facts
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
    let item_wave = bota_core::item_def(item)
        .and_then(|def| match def.effect {
            bota_core::UseSpec::Wave(wave) => {
                let bonus = caster
                    .and_then(|id| views.find(id))
                    .map_or(0, |unit| bota_client_core::carried_of(unit).aoe_bonus);
                Some(Vec3::new(
                    bota_core::widened(wave.radius, bonus) as f32,
                    0.0,
                    wave.speed as f32,
                ))
            }
            _ => None,
        })
        .unwrap_or_default();
    Feeds {
        item_wave,
        mode: item_feeds(now.or(before)).0,
        charges: item_feeds(before.or(now)).1,
        ..Feeds::default()
    }
}

/// Fires the lines of every fact the last pump told.
fn told(
    sight: crate::fog::EventSight,
    ground: Res<Ground>,
    rules: Res<Rules>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    bodies: Shooters,
    mut stage: Stage,
) {
    let core = sight.core();
    let Some(now) = core.view() else {
        return;
    };
    let views = Views {
        now,
        before: core.prev_view(),
    };
    for (n, event) in core.told.iter().enumerate() {
        for fact in presentation_facts(event, &views) {
            let seed = now.tick.wrapping_mul(31).wrapping_add(n as u32);
            for (_, rule) in rules
                .of(fact.trigger)
                .filter(|(_, r)| about(r, &fact, &views))
            {
                let fx = &rule.fx;
                let unit = match fx.at {
                    At::Caster => fact.caster.and_then(|id| now.unit(id)),
                    At::CasterBefore => fact.caster.and_then(|id| core.before_event(n, id)),
                    At::Unit => fact.unit.and_then(|id| views.find(id)),
                    _ => None,
                };
                // A use can have a visible start and a fogged end (a blink, for example).
                let subject = match unit {
                    Some(unit) => EventSubject::Unit(unit),
                    None if fx.at == At::Point => {
                        fact.point.map_or(EventSubject::Event, EventSubject::Point)
                    }
                    None => EventSubject::Event,
                };
                if !sight.allows(event, subject) {
                    continue;
                }
                let at = match unit {
                    Some(unit) => Some(event_point(&ground, &bodies, unit, &fx.attach, fx.at)),
                    None if fx.at == At::Point => fact.point.map(|spot| on_ground(&ground, spot)),
                    None => None,
                };
                if let Some(at) = at {
                    let place = |attach: &Attach| match unit {
                        Some(unit) => event_point(&ground, &bodies, unit, attach, fx.at),
                        None => at,
                    };
                    let feeds = Feeds {
                        caster: fact
                            .caster
                            .and_then(|id| {
                                if fx.at == At::CasterBefore {
                                    core.before_event(n, id)
                                } else {
                                    sight.visible_unit(id)
                                }
                            })
                            .map(|u| attach_point(&ground, &bodies, u, &Attach::Origin)),
                        ..item_state(&fact, &views)
                    };
                    let with = (library.as_deref(), &mut *playing);
                    let permission = match (unit, event) {
                        (None, EventKind::ItemUsed { caster, .. })
                            if sight
                                .aim_knowledge(*caster)
                                .is_some_and(|seen| !sight.fog().hides(Some(seen))) =>
                        {
                            crate::particles::EffectSight::Witnessed(
                                sight.aim_knowledge(*caster).unwrap_or_default(),
                            )
                        }
                        (_, EventKind::Died { .. }) => {
                            crate::particles::EffectSight::Witnessed(sight.knowledge(
                                sight.death_witness(event).and_then(|unit| unit.seen_by),
                            ))
                        }
                        (Some(unit), _) => {
                            crate::particles::EffectSight::KnownPoint(sight.knowledge(unit.seen_by))
                        }
                        (
                            None,
                            EventKind::ItemUsed {
                                target: Some(id), ..
                            },
                        ) => crate::particles::EffectSight::KnownPoint(
                            sight.knowledge(now.unit(*id).and_then(|unit| unit.seen_by)),
                        ),
                        _ => crate::particles::EffectSight::Point,
                    };
                    let emitter = unit
                        .map(|unit| unit.id)
                        .or(fact.caster)
                        .or(fact.unit)
                        .or(fact.named);
                    stage.play(fx, (at, seed), with, (&feeds, &place), permission, emitter);
                }
            }
        }
    }
}

fn event_point(
    ground: &Ground,
    bodies: &Shooters,
    unit: &UnitView,
    attach: &Attach,
    at: At,
) -> Vec3 {
    let point = attach_point(ground, bodies, unit, attach);
    if at == At::CasterBefore {
        // Historical effects retain their origin when the same unit is now drawn elsewhere.
        point + on_ground(ground, unit.pos) - attach_point(ground, bodies, unit, &Attach::Origin)
    } else {
        point
    }
}
