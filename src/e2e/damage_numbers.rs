//! The damage numbers over a hero in a real match: damage that comes in quick succession sums
//! into one number that is swapped for a larger one while its window of a second (counted from
//! the first blow) is open; blows that took nothing draw nothing; the colour is the damage's type
//! (physical white, magical purple, pure yellow, as in the game); a critical strike has a number
//! of its own, outside the sum.
//!
//! The game moves one tick a frame and a frame is a fixed 1/30 s, so every age is counted, not
//! timed.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::FloaterKind;
use bota_proto::{
    AbilityId, AbilitySlot, Cheat, CreepSide, DamageKind, EntityId, EventKind, HeroId, ItemId,
    ModifierSpec, Order, Target, Team, UnitKind,
};

use super::harness::{Client, MID, Server};
use crate::bars::DrawnFloater;

/// The game seconds each frame stands for.
const FRAME: f32 = 1.0 / 30.0;
/// How far from one's hero the enemy hero stands: inside Rot's reach, and a hook's line.
const APART: i32 = 120;
const SOON: Duration = Duration::from_secs(60);

/// The enemy hero.
fn foe(client: &Client) -> Option<EntityId> {
    client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Hero && unit.team == Team::Dire)
        .map(|unit| unit.id)
}

/// One's own hero of `hero` on open ground with an enemy hero `apart` to its east that outlasts
/// the scene (four times the health), no creeps.
fn scene(hero: HeroId, apart: i32) -> (Server, Client, EntityId) {
    let server = Server::start(1, &["--seed", "5"]);
    let mut client = Client::join(&server, hero, &[]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f32(FRAME),
        ));
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::Invulnerable { on: true });
    client.stand_at(MID);
    client.cheat(Cheat::CreateHero {
        hero: HeroId::SHADOW_FIEND,
        enemy: true,
        at: bota_proto::Vec2::from_ints(MID.0 + apart, MID.1),
    });
    client.until("the enemy hero stands near", |c| foe(c).is_some());
    let foe = foe(&client).expect("the enemy hero");
    client.cheat(Cheat::ApplyModifier {
        target: Target::Unit(foe),
        spec: ModifierSpec {
            max_hp: ModifierSpec::MAX_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: bota_proto::MAX_MODIFIER_TICKS,
    });
    client.cheat(Cheat::SetPools {
        health: 100,
        mana: 100,
    });
    (server, client, foe)
}

/// Learns and casts the one's own hero's ability `id` at `target`.
fn learn_and_cast(client: &mut Client, id: AbilityId, target: Target) {
    let slot = AbilitySlot(
        client
            .me()
            .abilities
            .iter()
            .position(|ability| ability.id == id)
            .expect("the ability in the hero's kit") as u8,
    );
    client.core_mut().send_order_to(None, Order::Learn { slot });
    client.until("the ability learned", |c| {
        c.me().abilities[slot.0 as usize].level > 0
    });
    client
        .core_mut()
        .send_order_to(None, Order::Cast { slot, target });
}

/// The numbers of `kind` floating over `unit`, the oldest first.
/// The numbers of `kind` floating over `unit` as (sum, tick of its first blow), the oldest first.
fn numbers(client: &Client, unit: EntityId, kind: FloaterKind) -> Vec<(i32, u32)> {
    client
        .core()
        .floaters
        .iter()
        .filter(|f| f.unit == unit && f.kind == kind)
        .map(|f| (f.amount, f.opened))
        .collect()
}

/// The events told to the client, each once: a frame that pumped nothing tells the last again.
struct Heard {
    seen: u32,
}

impl Heard {
    fn new() -> Self {
        Heard { seen: 0 }
    }

    /// The events of ticks after the last call's.
    fn fresh(&mut self, client: &Client) -> Vec<(u32, EventKind)> {
        let core = client.core();
        let fresh = core
            .told
            .iter()
            .zip(&core.told_ticks)
            .filter(|(_, tick)| **tick > self.seen)
            .map(|(event, tick)| (*tick, event.clone()))
            .collect();
        self.seen = self
            .seen
            .max(core.told_ticks.iter().copied().max().unwrap_or(0));
        fresh
    }

    /// The blows of `damage` on `unit` among them, as (tick, amount).
    fn blows(&mut self, client: &Client, unit: EntityId, damage: DamageKind) -> Vec<(u32, i32)> {
        self.fresh(client)
            .into_iter()
            .filter_map(|(tick, event)| match event {
                EventKind::Damaged {
                    target,
                    amount,
                    kind,
                    ..
                } if target == unit && kind == damage => Some((tick, amount)),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn rot_ticking_over_a_second_shows_one_number_that_grows_to_the_sum_and_a_second_after_the_window()
{
    let (_server, mut client, foe) = scene(HeroId::PUDGE, APART);
    learn_and_cast(&mut client, AbilityId::ROT, Target::None);
    let mut heard = Heard::new();
    let window = client.core().damage_window_ticks();
    let mut blows: Vec<(u32, i32)> = Vec::new();
    let mut biggest_number = 0;
    let mut two_at_once = false;
    let (mut smallest, mut largest) = (f32::MAX, 0.0_f32);
    client.within(
        SOON,
        "two rot numbers over the enemy, one a window after the other",
        |c| {
            // Pudge also strikes the enemy with his weapon: only rot's blows are magical.
            blows.extend(heard.blows(c, foe, DamageKind::Magical));
            let live = numbers(c, foe, FloaterKind::Magic);
            for pair in live.windows(2) {
                assert!(
                    pair[1].1 - pair[0].1 >= window,
                    "a second number floated {} ticks into the first's window of {window}: {live:?}",
                    pair[1].1 - pair[0].1
                );
            }
            // Each number is exactly the damage rot took in its window, no more and no less.
            for (sum, opened) in &live {
                let took: i32 = blows
                    .iter()
                    .filter(|(tick, _)| (*opened..opened + window).contains(tick))
                    .map(|(_, amount)| amount)
                    .sum();
                assert_eq!(*sum, took, "the number opened at tick {opened} among {blows:?}");
            }
            biggest_number = biggest_number.max(live.iter().map(|n| n.0).max().unwrap_or(0));
            for number in drawn(c, FloaterKind::Magic) {
                smallest = smallest.min(number.size);
                largest = largest.max(number.size);
            }
            two_at_once |= live.len() >= 2;
            two_at_once
        },
    );
    let biggest_tick = blows.iter().map(|(_, amount)| *amount).max().unwrap_or(0);
    assert!(biggest_tick > 0, "rot did damage the enemy");
    assert!(
        largest > smallest,
        "the number grew with its sum: {smallest} to {largest} px"
    );
    assert!(
        largest <= crate::bars::DAMAGE_SIZE * crate::bars::GROWTH_MAX + 1e-3,
        "and its size is bounded: {largest} px"
    );
    assert!(
        biggest_number >= 3 * biggest_tick,
        "the number grew to {biggest_number} over ticks of {biggest_tick}"
    );
}

#[test]
fn rot_ticks_that_take_nothing_draw_no_number() {
    let (_server, mut client, foe) = scene(HeroId::PUDGE, APART);
    client.cheat(Cheat::ApplyModifier {
        target: Target::Unit(foe),
        spec: ModifierSpec {
            magic_resist: ModifierSpec::MAX_RESIST,
            ..ModifierSpec::NOMINAL
        },
        ticks: bota_proto::MAX_MODIFIER_TICKS,
    });
    learn_and_cast(&mut client, AbilityId::ROT, Target::None);
    let mut heard = Heard::new();
    let mut nothing = 0;
    client.within(SOON, "five rot ticks that took nothing", |c| {
        let blows = heard.blows(c, foe, DamageKind::Magical);
        assert!(
            blows.iter().all(|(_, amount)| *amount == 0),
            "the enemy takes nothing from rot: {blows:?}"
        );
        nothing += blows.len();
        assert!(
            numbers(c, foe, FloaterKind::Magic).is_empty(),
            "a magical number floats over the enemy: {:?}",
            numbers(c, foe, FloaterKind::Magic)
        );
        nothing >= 5
    });
}

/// The drawn floaters of `kind`.
fn drawn(client: &mut Client, kind: FloaterKind) -> Vec<DrawnFloater> {
    crate::bars::drawn_floaters(client.world_mut())
        .into_iter()
        .filter(|f| f.kind == kind)
        .collect()
}

fn srgb(color: Color) -> [u8; 3] {
    let c = color.to_srgba();
    [c.red, c.green, c.blue].map(|v| (v * 255.0).round() as u8)
}

#[test]
fn physical_magical_and_pure_damage_are_drawn_white_purple_and_yellow_side_by_side() {
    let (_server, mut client, foe) = scene(HeroId::SHADOW_FIEND, APART);
    for kind in [DamageKind::Physical, DamageKind::Magical, DamageKind::Pure] {
        client.cheat(Cheat::Damage {
            target: Target::Unit(foe),
            amount: 40,
            kind,
        });
    }
    let kinds = [FloaterKind::Damage, FloaterKind::Magic, FloaterKind::Pure];
    client.within(SOON, "the three numbers drawn", |c| {
        kinds.iter().all(|kind| !drawn(c, *kind).is_empty())
    });
    let [physical, magical, pure] = kinds.map(|kind| drawn(&mut client, kind).remove(0));
    assert_eq!(srgb(physical.color), [255, 255, 255], "physical: white");
    assert_eq!(srgb(magical.color), [184, 158, 255], "magical: the purple");
    assert_eq!(srgb(pure.color), [255, 200, 33], "pure: yellow");
    let x = |f: &DrawnFloater| client.centre_of(f.entity).expect("laid out").x;
    assert!(
        x(&physical) < x(&magical) && x(&magical) < x(&pure),
        "the numbers stand side by side, not over one another: {} {} {}",
        x(&physical),
        x(&magical),
        x(&pure)
    );
}

#[test]
fn a_physical_blow_during_a_burn_floats_beside_the_burns_number_not_into_it() {
    let (_server, mut client, foe) = scene(HeroId::SHADOW_FIEND, APART);
    for (amount, kind) in [
        (10, DamageKind::Magical),
        (12, DamageKind::Physical),
        (10, DamageKind::Magical),
    ] {
        client.cheat(Cheat::Damage {
            target: Target::Unit(foe),
            amount,
            kind,
        });
    }
    client.within(SOON, "both numbers floating", |c| {
        !numbers(c, foe, FloaterKind::Damage).is_empty()
            && !numbers(c, foe, FloaterKind::Magic).is_empty()
    });
    assert_eq!(numbers(&client, foe, FloaterKind::Magic).len(), 1);
    assert!(
        numbers(&client, foe, FloaterKind::Magic)[0].0 >= 10,
        "the burn's number holds the burn's damage only"
    );
}

#[test]
fn a_meat_hook_that_catches_a_hero_floats_a_pure_damage_number_in_yellow() {
    let (_server, mut client, foe) = scene(HeroId::PUDGE, 300);
    let at = client.view().unit(foe).expect("the enemy").pos;
    learn_and_cast(&mut client, AbilityId::MEAT_HOOK, Target::Pos(at));
    client.within(SOON, "the hook's damage drawn", |c| {
        !drawn(c, FloaterKind::Pure).is_empty()
    });
    let hook = drawn(&mut client, FloaterKind::Pure).remove(0);
    assert_eq!(srgb(hook.color), [255, 200, 33]);
    assert!(
        drawn(&mut client, FloaterKind::Magic).is_empty(),
        "the hook is not drawn as magic"
    );
}

#[test]
fn a_critical_strike_floats_a_number_of_its_own_in_red_outside_the_sum() {
    let (_server, mut client, foe) = scene(HeroId::PUDGE, APART);
    client.cheat(Cheat::Item {
        item: ItemId::CRYSTALYS,
    });
    client.core_mut().send_order_to(
        None,
        Order::Attack {
            target: Target::Unit(foe),
        },
    );
    let mut heard = Heard::new();
    let (mut plain, mut critical) = (Vec::new(), Vec::new());
    client.within(SOON, "a critical strike and a plain blow", |c| {
        for (_, event) in heard.fresh(c) {
            if let EventKind::Damaged {
                target,
                amount,
                inflictor: None,
                critical: strike,
                ..
            } = event
                && target == foe
            {
                if strike { &mut critical } else { &mut plain }.push(amount);
            }
        }
        !critical.is_empty() && !plain.is_empty() && !drawn(c, FloaterKind::Crit).is_empty()
    });
    let crit = drawn(&mut client, FloaterKind::Crit).remove(0);
    assert_eq!(srgb(crit.color), [255, 48, 48], "a critical strike is red");
    assert!(
        critical
            .iter()
            .any(|amount| crit.words == amount.to_string()),
        "the crit number {} is a critical blow of {critical:?}",
        crit.words
    );
    let marks = crate::bars::drawn_marks(client.world_mut());
    assert_eq!(
        marks.len(),
        1,
        "one lightning mark is drawn beside the number"
    );
    let (mark, digits) = (
        client.centre_of(marks[0]).expect("the mark laid out"),
        client.centre_of(crit.entity).expect("the number laid out"),
    );
    assert!(
        mark.x > digits.x && (mark.y - digits.y).abs() < crit.size,
        "the mark at {mark} stands to the right of the number at {digits}"
    );
    let summed = numbers(&client, foe, FloaterKind::Damage);
    assert!(
        summed.iter().all(|(amount, _)| !critical.contains(amount)),
        "the plain blows' sums {summed:?} do not hold a critical blow {critical:?}"
    );
}
