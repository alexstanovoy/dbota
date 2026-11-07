//! The concepts the match raises for the hero spoken for, from what the client is told: its
//! seat's orders, the events of the last pump, and what successive views tell (it stands again,
//! the battle begins, the match is won). Each is raised as Dota raises it, for that hero alone;
//! nothing is raised from what the seat's side is not told.

use bota_client_core::App as Core;
use bota_proto::{EntityId, EventKind, Order, SlotId, Team, UnitKind};
use dbota_import::voice::concept::{Concept, Fact, powerup_type};

use super::{Facts, Raised, hero_class};

/// How near an enemy hero must be seen for a deny to be gloated over (Dota's "Requires visible
/// enemy player within 1000 radius", every hero's response page on Liquipedia).
const DENY_WITNESS_RANGE: f32 = 1000.0;

/// What the listener remembers between frames.
#[derive(Default)]
pub struct Memory {
    /// The seat and hero listened for: another starts the memory afresh.
    seat: Option<(SlotId, EntityId)>,
    /// The seat's last order seen, by its tick and the order.
    order: Option<(u32, Order)>,
    /// Whether the hero stood in the last view.
    standing: Option<bool>,
    /// Whether the hero has entered battle, the battle has begun, the match has ended.
    spawned: bool,
    begun: Option<bool>,
    ended: bool,
    /// Whether First Blood has been claimed: by a kill a hero was credited with, or one its
    /// assisting heroes shared (the server's rule, bota_server's `price_of_fall`); a hero that
    /// fell to a tower or a creep alone claims nothing.
    blooded: bool,
    /// The hero's last hits so far: its creep score when the listening began, and one for each
    /// it was told of since.
    last_hits: u16,
}

/// Seconds of game time: since the battle began, negative before.
pub fn game_time(core: &Core) -> f32 {
    let Some(view) = core.view() else {
        return 0.0;
    };
    let rate = f32::from(core.tick_rate().max(1));
    (view.game_tick() as f32 - core.pregame_ticks() as f32) / rate
}

impl Memory {
    /// The concepts raised since the last call, in the order they happened.
    pub fn queries(
        &mut self,
        core: &Core,
        seat: Option<SlotId>,
        fog: &crate::fog::FogGrid,
    ) -> Vec<Raised> {
        let Some((slot, hero)) = seat.and_then(|slot| {
            let unit = core.view()?.player(slot)?.unit?;
            Some((slot, unit))
        }) else {
            *self = Memory::default();
            return Vec::new();
        };
        if self.seat != Some((slot, hero)) {
            // A seat taken up mid-match is listened for from now: what it did before is not
            // answered, and a hero that entered battle before the battle began is not entering
            // it now.
            let begun = game_time(core) >= 0.0;
            let players = core.view().map_or(&[][..], |v| v.players.as_slice());
            let player = players.iter().find(|p| p.slot == slot);
            *self = Memory {
                seat: Some((slot, hero)),
                order: last_order(core, slot, hero),
                spawned: begun,
                begun: Some(begun),
                blooded: players.iter().any(|p| p.kills > 0),
                last_hits: player
                    .and_then(|p| p.creep_score)
                    .map_or(0, |c| c.last_hits),
                ..Memory::default()
            };
        }
        let mut raised = Vec::new();
        self.orders(core, slot, hero, &mut raised);
        self.views(core, slot, hero, &mut raised);
        self.events(core, slot, hero, fog, &mut raised);
        raised
    }

    /// A move or attack order of the seat's hero, once each.
    fn orders(&mut self, core: &Core, slot: SlotId, hero: EntityId, raised: &mut Vec<Raised>) {
        let last = last_order(core, slot, hero);
        if last.is_none() || last == self.order {
            return;
        }
        self.order = last;
        let concept = match last.map(|(_, order)| order) {
            Some(Order::Move { .. }) => Concept::Move,
            Some(Order::Attack { .. }) => Concept::Attack,
            _ => return,
        };
        raised.push((concept, Vec::new()));
    }

    /// What successive views tell: the hero entering battle and standing again, the battle's
    /// start, the match's end.
    fn views(&mut self, core: &Core, slot: SlotId, hero: EntityId, raised: &mut Vec<Raised>) {
        let Some(player) = core.view().and_then(|view| view.player(slot)) else {
            return;
        };
        let standing = player.respawn_left == 0 && core.view().and_then(|v| v.unit(hero)).is_some();
        if standing && !self.spawned {
            self.spawned = true;
            raised.push((Concept::Spawn, Vec::new()));
        } else if standing && self.standing == Some(false) {
            raised.push((Concept::Respawn, Vec::new()));
        }
        self.standing = Some(standing);
        let begun = game_time(core) >= 0.0;
        if begun && self.begun == Some(false) {
            raised.push((Concept::GameStart, Vec::new()));
        }
        self.begun = Some(begun);
        if let Some((winner, _)) = &core.over
            && !self.ended
        {
            self.ended = true;
            let won = *winner == player.team;
            raised.push((
                if won {
                    Concept::Victory
                } else {
                    Concept::Defeat
                },
                Vec::new(),
            ));
        }
    }
}

/// The seat's latest order for its hero, as shown.
fn last_order(core: &Core, slot: SlotId, hero: EntityId) -> Option<(u32, Order)> {
    core.shown_orders
        .iter()
        .find(|s| s.slot == slot && s.unit.is_none_or(|u| u == hero))
        .map(|s| (s.tick, s.order))
}

impl Memory {
    /// The concepts the last pump's events raise for the hero.
    fn events(
        &mut self,
        core: &Core,
        slot: SlotId,
        hero: EntityId,
        fog: &crate::fog::FogGrid,
        raised: &mut Vec<Raised>,
    ) {
        for event in &core.told {
            let concept = match event {
                EventKind::AbilityCast { caster, ability } if *caster == hero => {
                    bota_core::ability_def(*ability).map(|a| {
                        (
                            Concept::CastExecute,
                            vec![(Fact::AbilityName, a.key.to_owned())],
                        )
                    })
                }
                EventKind::LevelUp { unit, level } if *unit == hero => {
                    Some((Concept::LevelUp, vec![(Fact::Level, level.to_string())]))
                }
                EventKind::ItemBought { slot: buyer, item } if *buyer == slot => {
                    bota_core::item_def(*item)
                        .map(|i| (Concept::Purchase, vec![(Fact::ItemName, i.key.to_owned())]))
                }
                EventKind::RuneActivated { unit, kind, .. } if *unit == hero => Some((
                    Concept::Powerup,
                    vec![(Fact::PowerupType, powerup_type(*kind))],
                )),
                EventKind::RuneBottled { unit, .. } if *unit == hero => {
                    Some((Concept::Bottling, Vec::new()))
                }
                EventKind::Died { .. } => self.died(core, hero, fog, event),
                _ => None,
            };
            raised.extend(concept);
        }
    }

    /// The concept a death raises: the hero's own death; a kill of an enemy hero it struck or
    /// was credited with (First Blood when no kill had claimed it); a last hit or a deny of
    /// anything else it struck.
    fn died(
        &mut self,
        core: &Core,
        hero: EntityId,
        fog: &crate::fog::FogGrid,
        event: &EventKind,
    ) -> Option<Raised> {
        let EventKind::Died {
            unit,
            killer,
            denied,
            assists,
            credited_hero,
            ..
        } = event
        else {
            return None;
        };
        if *unit == hero {
            self.blooded |= credited_hero.is_some() || *assists != 0;
            return Some((Concept::Death, Vec::new()));
        }
        let by_hero = *killer == Some(hero) || credited_hero.as_deref() == Some(&hero);
        if let Some(class) = hero_class(core, *unit) {
            let first = !self.blooded;
            self.blooded |= credited_hero.is_some() || *assists != 0;
            let facts: Facts = vec![
                (Fact::TargetClassName, class.to_owned()),
                (Fact::FirstBlood, u8::from(first).to_string()),
            ];
            return (by_hero && !denied).then_some((Concept::Kill, facts));
        }
        if *killer != Some(hero) {
            return None;
        }
        if *denied {
            return witnessed(core, hero, fog).then_some((Concept::Deny, Vec::new()));
        }
        self.last_hits = self.last_hits.saturating_add(1);
        Some((
            Concept::LastHit,
            vec![(Fact::LastHitCount, self.last_hits.to_string())],
        ))
    }
}

/// Whether an enemy hero stands in sight within [`DENY_WITNESS_RANGE`] of the hero.
fn witnessed(core: &Core, hero: EntityId, fog: &crate::fog::FogGrid) -> bool {
    let Some(view) = core.view() else {
        return false;
    };
    let Some(me) = view.unit(hero) else {
        return false;
    };
    let team: Team = me.team;
    let (x, y) = (me.pos.x.to_f32(), me.pos.y.to_f32());
    view.units.iter().any(|u| {
        let (dx, dy) = (u.pos.x.to_f32() - x, u.pos.y.to_f32() - y);
        u.kind == UnitKind::Hero
            && u.team != team
            && !fog.hides(u.seen_by)
            && dx.hypot(dy) <= DENY_WITNESS_RANGE
    })
}
