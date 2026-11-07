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

/// What the listener remembers between frames. What the view counts (kills, last hits) is
/// read from the view, not counted here, so that a memory started on any frame says what one
/// started at the match's beginning says.
#[derive(Default)]
pub struct Memory {
    /// The seat and hero listened for: another starts the memory afresh.
    seat: Option<(SlotId, EntityId)>,
    /// The tick of the last view listened to: a view of an earlier tick (a replay sought back)
    /// starts the memory afresh too.
    tick: u32,
    /// The seat's last order seen, by its tick and the order.
    order: Option<(u32, Order)>,
    /// Whether the hero stood in the last view.
    standing: Option<bool>,
    /// Whether the hero has entered battle, the battle has begun, the match has ended.
    spawned: bool,
    begun: Option<bool>,
    ended: bool,
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
        let Some((slot, hero, tick)) = seat.and_then(|slot| {
            let view = core.view()?;
            Some((slot, view.player(slot)?.unit?, view.tick))
        }) else {
            *self = Memory::default();
            return Vec::new();
        };
        if self.seat != Some((slot, hero)) || tick < self.tick {
            // A seat taken up mid-match is listened for from now: what it did before is not
            // answered, and a hero that entered battle before the battle began is not entering
            // it now. What this frame's events tell is still to be answered.
            let begun = game_time(core) >= 0.0;
            *self = Memory {
                seat: Some((slot, hero)),
                order: last_order(core, slot, hero),
                spawned: begun,
                begun: Some(begun),
                ..Memory::default()
            };
        }
        self.tick = tick;
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
        let first = raised.len();
        let mut claims = claims_before(core);
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
                EventKind::Died { .. } => died(core, hero, fog, event, &mut claims),
                _ => None,
            };
            raised.extend(concept);
        }
        count_last_hits(core, slot, &mut raised[first..]);
    }
}

/// The concept a death raises: the hero's own death; a kill of an enemy hero it struck or was
/// credited with (First Blood when no hero death before it claimed anything, `claims` counting
/// those before it); a last hit or a deny of anything else it struck.
fn died(
    core: &Core,
    hero: EntityId,
    fog: &crate::fog::FogGrid,
    event: &EventKind,
    claims: &mut usize,
) -> Option<Raised> {
    let EventKind::Died {
        unit,
        killer,
        denied,
        credited_hero,
        ..
    } = event
    else {
        return None;
    };
    if *unit == hero {
        *claims += claims_of(event);
        return Some((Concept::Death, Vec::new()));
    }
    let by_hero = *killer == Some(hero) || credited_hero.as_deref() == Some(&hero);
    if let Some(class) = hero_class(core, *unit) {
        let first = *claims == 0;
        *claims += claims_of(event);
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
    // Its count is the view's ([`count_last_hits`]).
    Some((Concept::LastHit, Vec::new()))
}

/// The kills and assists a hero's death adds to the view's counts: a kill for the hero it is
/// credited to, an assist for each assisting hero. First Blood is the server's to claim and no
/// view or event tells it; it goes to the first death credited to a hero or shared among
/// assisting heroes (bota_server's `price_of_fall`), and so to the first that adds to these
/// counts. A hero only debuffed by an enemy before falling to neutrals is counted an assist
/// though the server claims nothing by it: the one death this reading takes for a claim.
fn claims_of(event: &EventKind) -> usize {
    match event {
        EventKind::Died {
            denied,
            assists,
            credited_hero,
            ..
        } => usize::from(credited_hero.is_some() && !denied) + assists.count_ones() as usize,
        _ => 0,
    }
}

/// The claims of hero deaths before the last pump's: what the view's kills and assists count,
/// less what the pump's own deaths add. A listener begun on any frame reads the same.
fn claims_before(core: &Core) -> usize {
    let Some(view) = core.view() else {
        return 0;
    };
    let counted: usize = view
        .players
        .iter()
        .map(|p| usize::from(p.kills) + usize::from(p.assists))
        .sum();
    let told: usize = core
        .told
        .iter()
        .filter(|event| matches!(event, EventKind::Died { unit, .. } if hero_class(core, *unit).is_some()))
        .map(claims_of)
        .sum();
    counted.saturating_sub(told)
}

/// Gives each last hit of a pump its count: the view's creep score already counts them all,
/// the last of them as itself. A seat whose score is not told says no count.
fn count_last_hits(core: &Core, slot: SlotId, raised: &mut [Raised]) {
    let Some(score) = core
        .view()
        .and_then(|view| view.player(slot)?.creep_score)
        .map(|score| usize::from(score.last_hits))
    else {
        return;
    };
    let hits = raised
        .iter()
        .filter(|(c, _)| *c == Concept::LastHit)
        .count();
    let mut count = score.saturating_sub(hits);
    for (_, facts) in raised.iter_mut().filter(|(c, _)| *c == Concept::LastHit) {
        count += 1;
        facts.push((Fact::LastHitCount, count.to_string()));
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
