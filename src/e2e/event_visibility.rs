//! Positional audio and effect endpoints in full-stream replay POVs.

use std::sync::{Arc, Mutex};

use bevy::audio::AudioPlayer;
use bevy::prelude::*;
use bota_proto::{
    AbilityId, AbilitySlot, Cheat, ItemId, ItemSlot, MapId, Order, Target, Team, UnitKind,
    Vec2 as Spot,
};

use super::fog_pov::{ReplayFile, Spectated, replay_observed};
use super::harness::Client;

type Heard = Arc<Mutex<Vec<Option<(f32, f32)>>>>;

fn driving_hero(client: &Client) -> Option<&bota_proto::UnitView> {
    client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Hero && unit.owner == Some(bota_proto::SlotId(0)))
}

fn listen(client: &mut Client, event: &'static str, heard: Heard) {
    client.world_mut().add_observer(
        move |add: On<Add, AudioPlayer>, mixer: Res<crate::mixer::Mixer>| {
            if let Some((name, at)) = mixer.event_at(add.entity)
                && name == event
            {
                heard.lock().unwrap().push(at);
            }
        },
    );
}

fn record_hidden_death() -> (ReplayFile, Spot) {
    let mut game = Spectated::start("0");
    let at = die_at_a_neutral_camp(&mut game);
    (game.save(), at)
}

fn approach_camp(game: &mut Spectated) -> (Spot, bota_proto::EntityId, bota_proto::EntityId) {
    game.cheat(Cheat::SpawnNeutrals);
    game.cheat(Cheat::Invulnerable { on: false });
    let fountain = bota_core::fountain_pos(bota_core::map_of(MapId(0)), Team::Radiant);
    let at = bota_core::CAMPS
        .iter()
        .filter(|camp| camp.kind == bota_core::CampKind::Ancient)
        .min_by_key(|camp| camp.pos.distance_squared(fountain))
        .unwrap()
        .pos;
    let victim = game.driver.my_hero().unwrap();
    game.cheat(Cheat::Teleport { at });
    game.wait(
        "the hero is hidden from Dire beside the neutral camp",
        |_, client| match client.view().viewer {
            Some(Team::Dire) => driving_hero(client).is_none(),
            None => client.view().units.iter().any(|unit| {
                unit.kind == UnitKind::Hero
                    && unit.owner == Some(bota_proto::SlotId(0))
                    && unit.pos.within(at, bota_core::rules::units(256))
                    && unit.seen_by.is_some_and(|seen| !seen.dire)
            }),
            _ => false,
        },
    );
    // Radiant keeps sight after the victim falls; Dire has no observer here.
    game.cheat(Cheat::CreateHero {
        hero: bota_proto::HeroId::SHADOW_FIEND,
        enemy: false,
        at: at + Spot::from_ints(200, 200),
    });
    let target = game
        .driver
        .view()
        .unwrap()
        .units
        .iter()
        .filter(|unit| unit.kind == UnitKind::CreepNeutral)
        .min_by_key(|unit| unit.pos.distance_squared(at))
        .expect("a real neutral camp")
        .id;
    assert!(
        game.driver.view().unwrap().unit(victim).is_some(),
        "the victim is alive before the attack"
    );
    (at, victim, target)
}

fn die_at_a_neutral_camp(game: &mut Spectated) -> Spot {
    let (at, victim, target) = approach_camp(game);
    game.order(Order::Attack {
        target: Target::Unit(target),
    });
    assert!(
        game.driver.reject.is_none(),
        "the camp attack was accepted: {:?}",
        game.driver.reject
    );
    game.cheat(Cheat::Gold { amount: 1 });
    if game.driver.view().unwrap().unit(victim).is_some() {
        game.cheat(Cheat::ApplyModifier {
            target: Target::None,
            spec: bota_proto::ModifierSpec {
                max_hp: 2500,
                ..bota_proto::ModifierSpec::NOMINAL
            },
            ticks: 3000,
        });
    }
    game.wait("the hero killed by the neutral camp", |driver, _| {
        assert!(driver.reject.is_none(), "the real orders are accepted: {:?}", driver.reject);
        driver.feed.iter().any(|line| matches!(line.told, Some(bota_proto::EventKind::Died { unit, .. }) if unit == victim))
    });
    game.cheat(Cheat::Gold { amount: 1 });
    at
}

fn dire_watches_old_spot(game: &mut Spectated, at: Spot) -> bota_proto::EntityId {
    game.cheat(Cheat::Invulnerable { on: true });
    game.cheat(Cheat::Teleport { at });
    game.cheat(Cheat::CreateHero {
        hero: bota_proto::HeroId::SHADOW_FIEND,
        enemy: true,
        at: at + Spot::from_ints(100, 100),
    });
    game.next_side(Team::Dire);
    game.wait("the opposing hero visible before it leaves", |_, client| {
        driving_hero(client).is_some()
    });
    let last = driving_hero(&game.client).unwrap().id;
    game.wait("Dire has current sight of the old position", |_, client| {
        let fog = client.world().resource::<crate::fog::FogGrid>();
        fog.team == Some(Team::Dire)
            && fog.worked_out_at().is_some()
            && fog.lit(at.x.to_f32(), at.y.to_f32())
    });
    game.client.core_mut().pinned = None;
    let world = game.client.world_mut();
    world
        .query::<&mut crate::camera::RtsCamera>()
        .single_mut(world)
        .unwrap()
        .focus = Vec3::new(at.x.to_f32(), 0.0, -at.y.to_f32());
    last
}

#[test]
fn an_unseen_death_does_not_sound_at_a_still_visible_last_seen_spot() {
    let mut game = Spectated::start("0");
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    let last = dire_watches_old_spot(&mut game, at);
    let heard = Heard::default();
    listen(&mut game.client, "Hero_Nevermore.Death", heard.clone());
    die_at_a_neutral_camp(&mut game);
    game.wait("the public death reaches the filtered stream", |_, _| {
        !heard.lock().unwrap().is_empty()
    });
    assert!(driving_hero(&game.client).is_none());
    assert!(
        game.client.core().known(last).is_some(),
        "the old position is cached"
    );
    assert!(
        game.client
            .world()
            .resource::<crate::fog::FogGrid>()
            .lit(at.x.to_f32(), at.y.to_f32()),
        "the old spot remains visible"
    );
    assert!(
        heard.lock().unwrap().iter().all(Option::is_none),
        "a cached position is not the location of an unseen death"
    );
}

#[test]
fn a_hidden_hero_death_is_announced_without_a_positional_sound() {
    let (file, at) = record_hidden_death();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    for (slot, visible) in [(0, true), (1, false)] {
        let heard = Heard::default();
        let into = heard.clone();
        let mut client = replay_observed(&file, Some(slot), &["--look", &look], |client| {
            listen(client, "Hero_Nevermore.Death", into);
        });
        client.frames(2);
        assert!(
            client
                .core()
                .feed
                .iter()
                .any(|line| matches!(line.kind, bota_client_core::FeedKind::HeroDied { .. })),
            "a real hero death was recorded"
        );
        assert!(
            crate::feed::visible_rows(client.world_mut()) > 0,
            "the public announcement remains in the feed"
        );
        let heard = heard.lock().unwrap();
        assert!(!heard.is_empty(), "the announced death still sounds");
        assert_eq!(
            heard.iter().any(Option::is_some),
            visible,
            "a hidden death must not sound at its private world position"
        );
    }
}

#[test]
fn a_hidden_cast_does_not_play_its_core_raze_sound() {
    let mut game = Spectated::start("2");
    game.cheat(Cheat::Invulnerable { on: true });
    game.cheat(Cheat::Levels { count: 1 });
    let hero = game.driver.my_hero().unwrap();
    let unit = game.driver.view().unwrap().unit(hero).unwrap();
    let at = unit.pos;
    let slot = AbilitySlot(
        unit.abilities
            .iter()
            .position(|ability| ability.id == AbilityId::RAZE_NEAR)
            .unwrap() as u8,
    );
    game.order(Order::Learn { slot });
    game.order(Order::Cast {
        slot,
        target: Target::None,
    });
    game.wait("the real raze cast", |driver, _| {
        driver.view().unwrap().unit(hero).unwrap().abilities[slot.0 as usize].cooldown_left > 0
    });
    game.cheat(Cheat::Gold { amount: 1 });
    let file = game.save();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    for (slot, visible) in [(0, true), (1, false)] {
        let heard = Heard::default();
        let into = heard.clone();
        let _client = replay_observed(&file, Some(slot), &["--look", &look], |client| {
            listen(client, "Hero_Nevermore.Shadowraze", into);
        });
        assert_eq!(
            !heard.lock().unwrap().is_empty(),
            visible,
            "core cast cues must honor the caster's sight row"
        );
    }
}

fn started(client: &Client, logical: &str) -> u32 {
    let library = client.world().resource::<crate::particles::Library>();
    let effect = library.effect(logical).expect("the imported blink effect");
    client
        .world()
        .resource::<crate::particles::Playing>()
        .started(effect)
}

fn recorded_blink() -> (ReplayFile, Spot) {
    let mut game = Spectated::start("2");
    game.cheat(Cheat::Invulnerable { on: true });
    game.cheat(Cheat::CreepSpawning { on: false });
    game.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::All,
    });
    game.cheat(Cheat::TimeOfDay { night: true });
    game.cheat(Cheat::Item {
        item: ItemId::BLINK_DAGGER,
    });
    let start = bota_core::DOTA_RUNE_SPOTS[0].at;
    game.cheat(Cheat::Teleport { at: start });
    game.cheat(Cheat::CreateHero {
        hero: bota_proto::HeroId::SHADOW_FIEND,
        enemy: true,
        at: start + Spot::from_ints(100, 100),
    });
    let hero = game.driver.my_hero().unwrap();
    game.wait("the start visible to Dire", |_, client| {
        driving_hero(client).is_some_and(|unit| unit.seen_by.unwrap().dire)
    });
    let unit = game.driver.view().unwrap().unit(hero).unwrap();
    let from = unit.pos;
    let to = from + Spot::from_ints(-800, -800);
    let slot = ItemSlot(
        unit.items
            .iter()
            .position(|item| item.is_some_and(|item| item.id == ItemId::BLINK_DAGGER))
            .unwrap() as u8,
    );
    game.cheat(Cheat::Gold { amount: 1 });
    game.order(Order::Use {
        slot,
        target: Target::Pos(to),
    });
    game.wait("the blink ends out of Dire's sight", |_, client| {
        driving_hero(client).is_some_and(|unit| {
            unit.pos.within(to, bota_core::rules::units(128)) && !unit.seen_by.unwrap().dire
        })
    });
    game.cheat(Cheat::Gold { amount: 1 });
    (game.save(), from)
}

#[test]
fn a_blink_keeps_its_visible_start_and_hides_its_fogged_destination() {
    let (file, from) = recorded_blink();
    let look = format!("{},{}", from.x.to_int(), from.y.to_int());
    for (slot, end_visible) in [(0, true), (1, false)] {
        let heard = Heard::default();
        let into = heard.clone();
        let client = replay_observed(&file, Some(slot), &["--look", &look], |client| {
            listen(client, "DOTA_Item.BlinkDagger.Activate", into);
        });
        assert!(
            started(&client, "item.blink_start") > 0,
            "POV {slot}: the seen start still plays"
        );
        assert_eq!(
            started(&client, "item.blink_end") > 0,
            end_visible,
            "only the owner sees the hidden destination"
        );
        let heard = heard.lock().unwrap();
        assert!(!heard.is_empty(), "the visible start also sounds");
        assert!(
            heard
                .iter()
                .all(|at| at.is_some_and(|(x, y)| Vec2::new(x, y)
                    .distance(Vec2::new(from.x.to_f32(), from.y.to_f32()))
                    < 1.0)),
            "the start must retain its old location, not follow the hero to the hidden end"
        );
    }
}
