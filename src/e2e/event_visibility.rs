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

fn rune_wearer(view: &bota_proto::WorldView) -> &bota_proto::UnitView {
    view.units
        .iter()
        .find(|unit| unit.owner == Some(bota_proto::SlotId(2)))
        .expect("the created allied rune wearer")
}

fn record_invisible_item_recipient() -> (ReplayFile, Spot) {
    let mut game = Spectated::start("0");
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    game.cheat(Cheat::Invulnerable { on: true });
    game.cheat(Cheat::Teleport {
        at: at + Spot::from_ints(400, 0),
    });
    game.cheat(Cheat::CreateHero {
        hero: bota_proto::HeroId::PUDGE,
        enemy: false,
        at,
    });
    game.cheat(Cheat::SpawnRune {
        kind: bota_proto::RuneKind::Invisibility,
        at,
    });
    game.cheat(Cheat::CreateHero {
        hero: bota_proto::HeroId::PUDGE,
        enemy: true,
        at: at + Spot::from_ints(-1000, 0),
    });
    game.wait(
        "the allied rune wearer invisible on visible ground",
        |_, client| {
            rune_wearer(client.view()).statuses.bits & bota_proto::StatusFlags::INVISIBLE != 0
                && rune_wearer(client.view())
                    .seen_by
                    .is_some_and(|seen| !seen.dire)
                && driving_hero(client)
                    .is_some_and(|hero| hero.seen_by.is_some_and(|seen| seen.dire))
        },
    );
    let point = use_salve_on_recipient(&mut game);
    (game.save(), point)
}

fn use_salve_on_recipient(game: &mut Spectated) -> Spot {
    game.cheat(Cheat::Item {
        item: ItemId::HEALING_SALVE,
    });
    let hero = game.driver.my_hero().unwrap();
    let view = game.driver.view().unwrap();
    let slot = ItemSlot(
        view.unit(hero)
            .unwrap()
            .items
            .iter()
            .position(|item| item.is_some_and(|item| item.id == ItemId::HEALING_SALVE))
            .unwrap() as u8,
    );
    let target = rune_wearer(view).id;
    let point = rune_wearer(view).pos;
    game.order(Order::Use {
        slot,
        target: Target::Unit(target),
    });
    game.wait("the real salve is spent", |driver, _| {
        driver.view().unwrap().unit(hero).unwrap().items[slot.0 as usize].is_none()
    });
    game.cheat(Cheat::Gold { amount: 1 });
    point
}

#[derive(Resource, Default)]
struct ItemPoints(Vec<bool>);

fn observe_item_points(sight: crate::fog::EventSight, mut points: ResMut<ItemPoints>) {
    for event in &sight.core().told {
        if let bota_proto::EventKind::ItemUsed { at: Some(at), .. } = event {
            points
                .0
                .push(sight.allows(event, crate::fog::EventSubject::Point(*at)));
        }
    }
}

#[test]
fn an_invisible_item_target_stays_private_in_replay_despite_visible_ground() {
    let (file, at) = record_invisible_item_recipient();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    for (slot, visible) in [(Some(0), true), (Some(1), false), (None, true)] {
        let mut client = replay_observed(&file, slot, &["--look", &look], |client| {
            client
                .app
                .init_resource::<ItemPoints>()
                .add_systems(Update, observe_item_points);
        });
        client.until("the invisible recipient's ground is visible", |c| {
            c.world()
                .resource::<crate::fog::FogGrid>()
                .lit(at.x.to_f32(), at.y.to_f32())
        });
        assert!(
            started(&client, "item.flask.cast") > 0,
            "the visible caster's use still draws"
        );
        let points = &client.world().resource::<ItemPoints>().0;
        assert!(
            !points.is_empty(),
            "the full stream records the real unit-targeted use"
        );
        assert!(
            points.iter().all(|shown| *shown == visible),
            "POV {slot:?}: visible ground cannot disclose an invisible recipient"
        );
    }
}

#[derive(Resource, Default)]
struct Purchases(Vec<bool>);

fn observe_purchases(sight: crate::fog::EventSight, mut purchases: ResMut<Purchases>) {
    for event in &sight.core().told {
        if matches!(event, bota_proto::EventKind::ItemBought { .. }) {
            purchases
                .0
                .push(sight.allows(event, crate::fog::EventSubject::Event));
        }
    }
}

#[test]
fn purchases_stay_private_to_enemy_pov_and_are_public_in_union() {
    let mut game = Spectated::start("2");
    game.order(Order::Buy {
        item: ItemId::IRON_BRANCH,
    });
    game.wait("the real purchase arrives", |driver, _| {
        driver.view().unwrap().units.iter().any(|unit| {
            unit.owner == Some(bota_proto::SlotId(0))
                && unit
                    .items
                    .iter()
                    .flatten()
                    .any(|item| item.id == ItemId::IRON_BRANCH)
        })
    });
    game.cheat(Cheat::Gold { amount: 1 });
    let file = game.save();
    for (pov, allowed) in [(Some(0), true), (Some(1), false), (None, true)] {
        let client = replay_observed(&file, pov, &[], |client| {
            client
                .app
                .init_resource::<Purchases>()
                .add_systems(Update, observe_purchases);
        });
        let observations = &client.world().resource::<Purchases>().0;
        assert!(
            !observations.is_empty(),
            "the full stream contains the purchase"
        );
        assert!(
            observations.iter().all(|seen| *seen == allowed),
            "POV {pov:?}"
        );
    }
}

fn disable_sight(mut grid: ResMut<crate::fog::FogGrid>) {
    grid.on = false;
}

#[test]
fn purchases_are_shown_without_a_sight_grid_and_without_a_selected_side() {
    let mut game = Spectated::start("2");
    game.order(Order::Buy {
        item: ItemId::IRON_BRANCH,
    });
    game.wait("the purchase recorded", |driver, _| {
        driver.view().unwrap().units.iter().any(|unit| {
            unit.owner == Some(bota_proto::SlotId(0))
                && unit
                    .items
                    .iter()
                    .flatten()
                    .any(|item| item.id == ItemId::IRON_BRANCH)
        })
    });
    game.cheat(Cheat::Gold { amount: 1 });
    let file = game.save();
    for (pov, allowed) in [(Some(0), true), (Some(1), false), (None, true)] {
        let client = replay_observed(&file, pov, &[], |client| {
            client
                .app
                .init_resource::<Purchases>()
                .add_systems(Update, (disable_sight, observe_purchases).chain());
        });
        let purchases = &client.world().resource::<Purchases>().0;
        assert!(
            !purchases.is_empty(),
            "the actual purchase reached the full stream"
        );
        assert!(
            purchases.iter().all(|shown| *shown == allowed),
            "POV {pov:?}: no sight grid makes purchases public only without a side"
        );
    }
}

fn particle_by_pov(file: &ReplayFile, logical: &str) {
    for (pov, visible) in [(Some(0), true), (Some(1), false), (None, true)] {
        let client = replay_observed(file, pov, &[], |_| {});
        let effect = client
            .world()
            .resource::<crate::particles::Library>()
            .effect(logical)
            .unwrap();
        let playing = client.world().resource::<crate::particles::Playing>();
        assert_eq!(
            playing.started(effect) > 0,
            visible,
            "POV {pov:?}: {logical} admission"
        );
        if !visible {
            assert_eq!(
                playing.drawn(effect),
                0,
                "POV {pov:?}: {logical} never reaches rendered output"
            );
        }
    }
}

#[test]
fn a_hidden_hero_level_up_does_not_start_its_particle() {
    let mut game = Spectated::start("2");
    game.cheat(Cheat::Levels { count: 1 });
    game.cheat(Cheat::Gold { amount: 1 });
    particle_by_pov(&game.save(), "hero.levelup");
}

#[test]
fn a_hidden_hero_death_does_not_start_its_particle() {
    let (file, _) = record_hidden_death();
    particle_by_pov(&file, "hero.nevermore.death");
}

#[test]
fn guard_a_hidden_requiem_cast_does_not_start_its_burst() {
    let mut game = Spectated::start("2");
    game.cheat(Cheat::Invulnerable { on: true });
    game.cheat(Cheat::Levels { count: 6 });
    let hero = game.driver.my_hero().unwrap();
    let unit = game.driver.view().unwrap().unit(hero).unwrap();
    let slot = AbilitySlot(
        unit.abilities
            .iter()
            .position(|ability| ability.id == AbilityId::REQUIEM)
            .unwrap() as u8,
    );
    game.order(Order::Learn { slot });
    game.order(Order::Cast {
        slot,
        target: Target::None,
    });
    game.wait("the real requiem cast", |driver, _| {
        driver.view().unwrap().unit(hero).unwrap().abilities[slot.0 as usize].cooldown_left > 0
    });
    game.cheat(Cheat::Gold { amount: 1 });
    particle_by_pov(&game.save(), "hero.nevermore.requiem");
}

fn recorded_grenade() -> (ReplayFile, Spot) {
    let mut game = Spectated::start("2");
    game.cheat(Cheat::Item {
        item: ItemId::BLOOD_GRENADE,
    });
    let hero = game.driver.my_hero().unwrap();
    let unit = game.driver.view().unwrap().unit(hero).unwrap();
    let at = unit.pos + Spot::from_ints(100, 0);
    let slot = ItemSlot(
        unit.items
            .iter()
            .position(|item| item.is_some_and(|item| item.id == ItemId::BLOOD_GRENADE))
            .unwrap() as u8,
    );
    game.order(Order::Use {
        slot,
        target: Target::Pos(at),
    });
    game.wait("the grenade really lands", |driver, _| {
        driver.told.iter().any(|event| {
            matches!(
                event,
                bota_proto::EventKind::Landed {
                    inflictor: bota_proto::Inflictor::Item(ItemId::BLOOD_GRENADE),
                    ..
                }
            )
        })
    });
    game.cheat(Cheat::Gold { amount: 1 });
    (game.save(), at)
}

#[test]
fn a_hidden_grenade_does_not_start_its_flight_or_landing_particle() {
    particle_by_pov(&recorded_grenade().0, "item.blood_grenade");
}

#[test]
fn a_visible_grenade_lands_with_one_drawn_end_cap() {
    let (file, at) = recorded_grenade();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    let client = replay_observed(&file, Some(0), &["--look", &look], |_| {});
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect("item.blood_grenade")
        .unwrap();
    let playing = client.world().resource::<crate::particles::Playing>();
    assert_eq!(
        playing.started(effect),
        1,
        "one real grenade owns its flight and its landing"
    );
    assert!(
        playing.drawn_endings(effect) > 0,
        "the visible landing draws an end cap"
    );
}

fn driving_hero(client: &Client) -> Option<&bota_proto::UnitView> {
    client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Hero && unit.owner == Some(bota_proto::SlotId(0)))
}

fn listen(client: &mut Client, event: &'static str, heard: Heard) {
    client.world_mut().add_observer(
        move |add: On<Add<AudioPlayer>>, mixer: Res<crate::mixer::Mixer>| {
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
        let lines = crate::feed::shown(client.world_mut());
        assert!(
            lines.iter().any(|(left, _, text)| !left
                && text.contains("Shadow Fiend")
                && text.contains("slain")),
            "the hero's death remains in the kill feed: {lines:?}"
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
fn a_hidden_throwers_core_attack_cue_is_silent_and_a_visible_thrower_sounds() {
    let (file, at) = record_hidden_death();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    for (slot, visible) in [(Some(0), true), (Some(1), false), (None, true)] {
        let heard = Heard::default();
        let into = heard.clone();
        let _client = replay_observed(&file, slot, &["--look", &look], |client| {
            listen(client, "Hero_Nevermore.Attack", into);
        });
        assert_eq!(
            !heard.lock().unwrap().is_empty(),
            visible,
            "POV {slot:?}: attack cues may name and locate only a visible thrower"
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
    // Keep a complete visual lifetime after the use; EOF pauses particle simulation.
    let settled = game.driver.view().unwrap().tick + u32::from(game.driver.tick_rate());
    game.wait("the post-blink visual lifetime recorded", |driver, _| {
        driver.view().unwrap().tick >= settled
    });
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

fn darken_the_death_frame(
    session: NonSend<crate::session::Session>,
    mut fog: ResMut<crate::fog::FogGrid>,
) {
    if session
        .0
        .told
        .iter()
        .any(|event| matches!(event, bota_proto::EventKind::Died { .. }))
    {
        // The async worker is allowed to finish the post-death grid before presentation.
        fog.cells.fill(false);
    }
}

#[test]
fn own_hero_death_keeps_its_witnessed_effect_after_the_sight_grid_goes_dark() {
    let (file, at) = record_hidden_death();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    let mut client = replay_observed(&file, Some(0), &["--look", &look], |client| {
        client.app.add_systems(
            Update,
            darken_the_death_frame.before(crate::effects::deaths),
        );
    });
    client.until("the hero's sole source of sight is gone", |c| {
        let fog = c.world().resource::<crate::fog::FogGrid>();
        fog.worked_out_at().is_some_and(|tick| tick == c.tick())
            && !fog.lit(at.x.to_f32(), at.y.to_f32())
    });
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect("hero.nevermore.death")
        .unwrap();
    let playing = client.world().resource::<crate::particles::Playing>();
    assert!(
        playing.started(effect) > 0,
        "the owner witnessed its death before losing vision"
    );
    assert!(
        playing.drawn(effect) > 0,
        "the witnessed death contributes actual rendered output despite the now-dark grid"
    );
}

fn darken_item_use_frame(
    session: NonSend<crate::session::Session>,
    mut fog: ResMut<crate::fog::FogGrid>,
) {
    if session
        .0
        .told
        .iter()
        .any(|event| matches!(event, bota_proto::EventKind::ItemUsed { .. }))
    {
        fog.cells.fill(false);
    }
}

#[test]
fn the_owners_replay_keeps_its_aimed_blink_point_without_current_sight() {
    let (file, from) = recorded_blink();
    let look = format!("{},{}", from.x.to_int() - 800, from.y.to_int() - 800);
    let client = replay_observed(&file, Some(0), &["--look", &look], |client| {
        // Blink's flash is shorter than the general replay harness's 100 ms frame.
        client.app.add_systems(
            Startup,
            |mut clock: ResMut<bevy::time::TimeUpdateStrategy>| {
                *clock = bevy::time::TimeUpdateStrategy::ManualDuration(
                    std::time::Duration::from_secs_f32(1.0 / 60.0),
                );
            },
        );
        client.app.init_resource::<ItemPoints>().add_systems(
            Update,
            (darken_item_use_frame, observe_item_points)
                .chain()
                .before(crate::fx::PresentFacts),
        );
    });
    let points = &client.world().resource::<ItemPoints>().0;
    assert!(!points.is_empty(), "the actual use reached presentation");
    assert!(
        points.iter().all(|shown| *shown),
        "the owner retains every aimed point when current ground sight is gone"
    );
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect("item.blink_end")
        .unwrap();
    let playing = client.world().resource::<crate::particles::Playing>();
    assert!(
        playing.started(effect) > 0,
        "the owner knows the point it aimed at"
    );
    assert!(
        playing.drawn(effect) > 0,
        "the known aim draws despite the async grid being dark"
    );
}
