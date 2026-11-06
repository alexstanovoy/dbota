//! Runes and the Bottle as Dota shows them, in a match whose server spawns runes (`--runes all`)
//! and puts one where a test asks (`dota_spawn_rune`): the rune's model and glow where it lies,
//! the minimap's icon of it kept out of sight, the toast of a river rune, a rune taken by a click
//! with its sound, its hero's line, its buff and its lines in the chat and the feed, the Bottle's
//! icon by its charges and by the rune it keeps, and an invisible hero drawn see-through.

use std::time::{Duration, Instant};

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::{Intent, Slot};
use bota_proto::{Cheat, EffectId, HeroId, ItemId, RuneKind, StatusFlags, Vec2 as Spot};

use super::harness::{Client, Server, pose_of, subtree};
use crate::hud::{Part, PartOf};
use crate::minimap::RuneMark;
use crate::runes::RuneBody;

/// The Dota map's top river rune spot.
fn top_spot() -> (i32, i32) {
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    (at.x.to_int(), at.y.to_int())
}

/// Ground far from every rune spot, out of their sight.
const AWAY: (i32, i32) = (4000, 4000);

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(30);

/// A match of one seat that spawns runes, one's Shadow Fiend on the map and the game begun.
fn rune_match() -> (Server, Client) {
    let server = Server::start(1, &["--runes", "all"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    (server, client)
}

/// Puts a rune of `kind` at the rune spot nearest `at`, as `dota_spawn_rune` does.
fn spawn_rune(client: &mut Client, kind: RuneKind, at: (i32, i32)) {
    client.cheat(Cheat::SpawnRune {
        kind,
        at: Spot::from_ints(at.0, at.1),
    });
}

/// The rune of `kind` drawn on the map, if one is: where it stands, and the model it wears (a
/// rune is known by its model).
fn drawn(client: &mut Client, kind: RuneKind) -> Option<(Vec3, String)> {
    let model = format!("rune.{}.model", crate::runes::art(kind));
    let world = client.world_mut();
    world
        .query_filtered::<(&crate::loot::Idle, &Transform), With<RuneBody>>()
        .iter(world)
        .find(|(idle, _)| idle.0 == model)
        .map(|(idle, place)| (place.translation, idle.0.clone()))
}

/// Puts the pointer on a point of the ground, once the camera has settled enough for the
/// ground under the pointer to be that point.
fn point_at(client: &mut Client, (x, y): (f32, f32)) {
    client.within(SOON, "the pointer on the spot", |c| {
        let Some(on) = c.screen_of((x, y)) else {
            return false;
        };
        c.mouse(on);
        c.world()
            .resource::<crate::camera::CursorGround>()
            .0
            .is_some_and(|g| Vec2::new(g.x, -g.z).distance(Vec2::new(x, y)) < 20.0)
    });
}

/// The minimap's rune icons shown: the kind each marks and whether it is seen now.
fn rune_marks(client: &mut Client) -> Vec<(RuneKind, bool)> {
    let world = client.world_mut();
    world
        .query::<(&RuneMark, &Visibility)>()
        .iter(world)
        .filter(|(_, shown)| **shown != Visibility::Hidden)
        .filter_map(|(mark, _)| mark.0.map(|(_, kind, seen)| (kind, seen)))
        .collect()
}

/// How many times the particle effect of a logical name started.
fn started(client: &Client, effect: &str) -> u32 {
    let world = client.world();
    world
        .get_resource::<crate::particles::Library>()
        .and_then(|library| library.effect(effect))
        .map_or(0, |effect| {
            world
                .resource::<crate::particles::Playing>()
                .started(effect)
        })
}

/// A real match whose top rune spot is hidden, with its particle library ready.
fn hidden_spawn_match() -> (Server, Client, Spot) {
    let (server, mut client) = rune_match();
    client.stand_at(AWAY);
    client.within(SOON, "the rune particle library ready", |c| {
        c.world()
            .get_resource::<crate::particles::Library>()
            .is_some_and(|library| library.effect("rune.bounty.prespawn").is_some())
    });
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    client.within(SOON, "the hidden rune spot's fog known", |c| {
        c.world()
            .resource::<crate::fog::FogGrid>()
            .worked_out_at()
            .is_some()
    });
    assert!(
        !client
            .world()
            .resource::<crate::fog::FogGrid>()
            .lit(at.x.to_f32(), at.y.to_f32()),
        "the spot is outside our vision"
    );
    (server, client, at)
}

fn reveal_spawn(client: &mut Client, kind: RuneKind, at: Spot) {
    client.cheat(Cheat::Teleport {
        at: at + Spot::from_ints(200, 0),
    });
    client.within(SOON, "the rune now visible in the snapshot", |c| {
        c.view().runes.iter().any(|r| r.pos == at)
    });
    client.cheat(Cheat::SpawnRune { kind, at });
    client.frames(2);
}

#[test]
fn fog_hidden_water_rune_spawn_does_not_announce_its_spot() {
    let (_server, mut client, at) = hidden_spawn_match();
    client.cheat(Cheat::SpawnRune {
        kind: RuneKind::Water,
        at,
    });
    client.frames(2);
    assert!(
        !client.view().runes.iter().any(|rune| rune.pos == at),
        "the snapshot keeps the hidden rune private"
    );
    let world = client.world_mut();
    assert!(
        world
            .query::<&crate::rune_toast::RuneToast>()
            .iter(world)
            .all(|toast| toast.0.is_none()),
        "a hidden spawn must not announce its spot in the HUD"
    );
    reveal_spawn(&mut client, RuneKind::Water, at);
    let world = client.world_mut();
    assert!(
        world
            .query::<&crate::rune_toast::RuneToast>()
            .iter(world)
            .any(|toast| toast.0.is_some_and(|(kind, ..)| kind == RuneKind::Water)),
        "a visible spawn still announces its spot"
    );
}

#[test]
fn fog_hidden_bounty_rune_spawn_does_not_play_its_particle() {
    let (_server, mut client, at) = hidden_spawn_match();
    let before = started(&client, "rune.bounty.prespawn");
    client.cheat(Cheat::SpawnRune {
        kind: RuneKind::Bounty,
        at,
    });
    client.frames(2);
    assert!(
        !client.view().runes.iter().any(|rune| rune.pos == at),
        "the snapshot keeps the hidden Bounty private"
    );
    assert_eq!(
        before,
        started(&client, "rune.bounty.prespawn"),
        "a hidden Bounty spawn must not start its particle"
    );
    reveal_spawn(&mut client, RuneKind::Bounty, at);
    assert!(
        started(&client, "rune.bounty.prespawn") > before,
        "a visible Bounty spawn still plays its particle"
    );
}

fn recorded_spawns(visible: bool) -> super::fog_pov::ReplayFile {
    let mut game = super::fog_pov::Spectated::start("2");
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    if visible {
        game.cheat(Cheat::Teleport {
            at: at + Spot::from_ints(200, 0),
        });
    }
    game.wait("the recorded spot's vision settled", |_, client| {
        let grid = client.world().resource::<crate::fog::FogGrid>();
        grid.worked_out_at().is_some() && grid.lit(at.x.to_f32(), at.y.to_f32()) == visible
    });
    for kind in [RuneKind::Bounty, RuneKind::Water] {
        game.cheat(Cheat::SpawnRune { kind, at });
        game.wait("the real spawn present in the full stream", |_, client| {
            client.view().runes.iter().any(|rune| {
                rune.pos == at
                    && rune.kind == kind
                    && rune.seen_by.is_some_and(|seen| seen.radiant == visible)
            })
        });
    }
    game.save()
}

#[test]
fn replay_hidden_water_spawn_does_not_announce_its_spot() {
    let file = recorded_spawns(false);
    for slot in [0, 1] {
        let mut client = super::fog_pov::replay_from_side(&file, slot);
        client.frames(2);
        let world = client.world_mut();
        assert!(
            world
                .query::<&crate::rune_toast::RuneToast>()
                .iter(world)
                .all(|toast| toast.0.is_none()),
            "a fogged replay POV must not announce hidden rune coordinates"
        );
    }
}

#[test]
fn replay_hidden_bounty_spawn_does_not_start_its_particle() {
    let file = recorded_spawns(false);
    for slot in [0, 1] {
        let mut client = super::fog_pov::replay_from_side(&file, slot);
        client.frames(2);
        assert_eq!(
            started(&client, "rune.bounty.prespawn"),
            0,
            "a fogged replay POV must not play hidden Bounty effects from the full stream"
        );
    }
}

#[test]
fn replay_visible_rune_spawns_still_show_their_fx_and_hud() {
    let file = recorded_spawns(true);
    let mut client = super::fog_pov::replay_from_side(&file, 0);
    client.frames(2);
    assert!(
        started(&client, "rune.bounty.prespawn") > 0,
        "a visible replay spawn still plays its particle"
    );
    let world = client.world_mut();
    assert!(
        world
            .query::<&crate::rune_toast::RuneToast>()
            .iter(world)
            .any(|toast| toast.0.is_some_and(|(kind, ..)| kind == RuneKind::Water)),
        "a visible replay spawn still announces its spot"
    );
}

#[test]
fn replay_bounty_pickup_fx_follow_the_viewed_side() {
    let mut game = super::fog_pov::Spectated::start("2");
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    game.cheat(Cheat::SpawnRune {
        kind: RuneKind::Bounty,
        at,
    });
    game.wait("the rune before pickup", |_, client| {
        client.view().runes.iter().any(|rune| rune.pos == at)
    });
    game.cheat(Cheat::Teleport { at });
    game.wait("the real pickup recorded", |_, client| {
        !client.view().runes.iter().any(|rune| rune.pos == at)
            && client.view().units.iter().any(|unit| {
                unit.kind == bota_proto::UnitKind::Hero
                    && unit.team == bota_proto::Team::Radiant
                    && unit.pos == at
                    && unit.seen_by.is_some_and(|seen| !seen.dire)
            })
    });
    let file = game.save();
    for (slot, visible) in [(0, true), (1, false)] {
        let mut client = super::fog_pov::replay_from_side(&file, slot);
        client.frames(2);
        assert_eq!(
            started(&client, "rune.bounty.owner") > 0,
            visible,
            "the pickup particle must follow the viewed side's visibility"
        );
        let rune = label(&client, "DOTA_HUD_Rune_Bounty", &[("gold", String::new())]);
        assert_eq!(
            client
                .shown_texts()
                .iter()
                .any(|text| text.contains(rune.trim())),
            visible,
            "the rune feed must follow the same event visibility as its particle"
        );
    }
}

#[test]
fn replay_bottled_rune_contents_stay_private_from_the_enemy_pov() {
    let file = recorded_bottling_with_both_sides_watching();
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    for (slot, visible) in [(0, true), (1, false)] {
        let mut client = super::fog_pov::replay_from_side_with(&file, slot, &["--look", &look]);
        client.frames(2);
        assert_eq!(
            heard(&client, "sound.bottle.cork") > 0,
            visible,
            "a private store event must not play through the enemy POV"
        );
        let rune = label(&client, "DOTA_HUD_Rune_Haste", &[]);
        assert_eq!(
            client
                .shown_texts()
                .iter()
                .any(|text| text.contains(rune.trim())),
            visible,
            "the private bottle must not appear in the enemy's feed"
        );
    }
}

fn recorded_bottling_with_both_sides_watching() -> super::fog_pov::ReplayFile {
    let mut game = super::fog_pov::Spectated::start("2");
    game.cheat(Cheat::Item {
        item: ItemId::BOTTLE,
    });
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    game.cheat(Cheat::Teleport {
        at: at + Spot::from_ints(200, 0),
    });
    game.cheat(Cheat::CreateHero {
        hero: HeroId::SHADOW_FIEND,
        enemy: true,
        at: at + Spot::from_ints(600, 0),
    });
    game.cheat(Cheat::SpawnRune {
        kind: RuneKind::Haste,
        at,
    });
    game.cheat(Cheat::Teleport { at });
    game.wait("the Haste rune really bottled", |driver, _| {
        driver
            .view()
            .unwrap()
            .unit(driver.my_hero().unwrap())
            .unwrap()
            .items
            .iter()
            .flatten()
            .any(|item| item.rune.is_some_and(|rune| rune.kind == RuneKind::Haste))
    });
    assert!(
        game.driver
            .view()
            .unwrap()
            .unit(game.driver.my_hero().unwrap())
            .unwrap()
            .seen_by
            .is_none(),
        "the driving seat is a server-filtered stream"
    );
    game.wait("both sides see the bottling hero", |_, client| {
        client.view().units.iter().any(|unit| {
            unit.owner == Some(bota_proto::SlotId(0))
                && unit.seen_by.is_some_and(|seen| seen.radiant && seen.dire)
        })
    });
    game.save()
}

#[test]
fn a_union_replay_keeps_bottled_contents_private() {
    let file = recorded_bottling_with_both_sides_watching();
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    let mut client = super::fog_pov::replay_observed(&file, None, &["--look", &look], |_| {});
    let fog = client.world().resource::<crate::fog::FogGrid>();
    assert!(fog.on);
    assert!(fog.team.is_none(), "the ordinary union POV");
    assert_eq!(
        heard(&client, "sound.bottle.cork"),
        0,
        "union vision does not grant private bottle contents"
    );
    let rune = label(&client, "DOTA_HUD_Rune_Haste", &[]);
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.contains(rune.trim())),
        "the bottle is absent from the feed too"
    );
}

#[test]
fn replay_point_effects_and_sounds_follow_the_points_visibility() {
    let mut game = super::fog_pov::Spectated::start("2");
    game.cheat(Cheat::Item {
        item: ItemId::BLOOD_GRENADE,
    });
    let hero = game
        .driver
        .view()
        .unwrap()
        .unit(game.driver.my_hero().unwrap())
        .unwrap();
    let slot = hero
        .items
        .iter()
        .position(|item| item.is_some_and(|item| item.id == ItemId::BLOOD_GRENADE))
        .unwrap();
    let at = hero.pos + Spot::from_ints(100, 0);
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    assert!(hero.seen_by.is_none(), "the source player is a side stream");
    game.client.core_mut().pinned = None;
    let world = game.client.world_mut();
    world
        .query::<&mut crate::camera::RtsCamera>()
        .single_mut(world)
        .unwrap()
        .focus = Vec3::new(at.x.to_f32(), 0.0, -at.y.to_f32());
    game.order(bota_proto::Order::Use {
        slot: bota_proto::ItemSlot(slot as u8),
        target: bota_proto::Target::Pos(at),
    });
    game.wait("the grenade really landed", |_, client| {
        heard(client, "sound.blood_grenade.location") > 0
    });
    let file = game.save();
    for (slot, visible) in [(0, true), (1, false)] {
        let mut client = super::fog_pov::replay_from_side_with(&file, slot, &["--look", &look]);
        client.within(SOON, "the replay point's sight worked out", |c| {
            c.world()
                .resource::<crate::fog::FogGrid>()
                .worked_out_at()
                .is_some()
        });
        assert_eq!(
            client
                .world()
                .resource::<crate::fog::FogGrid>()
                .lit(at.x.to_f32(), at.y.to_f32()),
            visible,
            "the fixture point must really be visible only to its owning side"
        );
        assert_eq!(
            heard(&client, "sound.blood_grenade.location") > 0,
            visible,
            "a point effect's sound must use the point's fog, including full-stream replays"
        );
    }
}

/// How many times a bound sound, or a hero's line, of a logical name was heard.
fn heard(client: &Client, logical: &str) -> u32 {
    client
        .world()
        .resource::<crate::fx::Heard>()
        .0
        .get(logical)
        .copied()
        .unwrap_or_default()
}

/// The runs of text shown (a tooltip's lines, the chat log's), a span each.
fn shown_spans(client: &mut Client) -> Vec<String> {
    let world = client.world_mut();
    let shown: Vec<Entity> = world
        .query::<(Entity, &Text, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, _, seen)| seen.get())
        .map(|(e, ..)| e)
        .collect();
    world
        .query::<(&TextSpan, &ChildOf)>()
        .iter(world)
        .filter(|(_, parent)| shown.contains(&parent.parent()))
        .map(|(span, _)| span.0.clone())
        .collect()
}

/// One of the install's labels.
fn label(client: &Client, key: &str, values: &[(&str, String)]) -> String {
    client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label(key, values)
        .unwrap_or_else(|| panic!("the install's {key}"))
}

/// The asset path of a texture of the cache.
fn file(client: &Client, logical: &str) -> String {
    client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file(logical)
        .unwrap_or_else(|| panic!("{logical} in the cache"))
}

/// Whether one's hero wears an effect.
fn wears(client: &Client, effect: EffectId) -> bool {
    client.me().effects.iter().any(|e| e.id == effect)
}

/// A rune lies drawn with Dota's model of its kind at its spot, wearing its glow, a Bounty Rune
/// appearing in its swirl; the minimap shows its icon, and keeps it, as Dota does, once the hero
/// has gone and the rune is out of sight; taken, it plays its sound and burst, the feed tells
/// it in Dota's words and the minimap lets it go.
#[test]
fn a_rune_lies_drawn_and_the_minimap_keeps_it_until_its_spot_is_seen_without_it() {
    let (_server, mut client) = rune_match();
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Bounty, spot);
    client.within(SOON, "the Bounty Rune drawn", |c| {
        drawn(c, RuneKind::Bounty).is_some()
    });
    let (place, model) = drawn(&mut client, RuneKind::Bounty).expect("drawn");
    assert_eq!(model, "rune.bounty.model");
    let (x, y) = (spot.0 as f32, spot.1 as f32);
    assert!(
        Vec2::new(place.x, -place.z).distance(Vec2::new(x, y)) < 1.0,
        "at its spot: {place}"
    );
    assert!(started(&client, "rune.bounty.prespawn") > 0, "its swirl");
    client.within(SOON, "its glow", |c| started(c, "rune.bounty.glow") > 0);
    client.within(SOON, "its icon on the minimap", |c| {
        rune_marks(c) == [(RuneKind::Bounty, true)]
    });
    client.stand_at(AWAY);
    client.within(SOON, "the rune out of sight", |c| {
        drawn(c, RuneKind::Bounty).is_none()
    });
    client.frames(5);
    assert_eq!(
        rune_marks(&mut client),
        [(RuneKind::Bounty, false)],
        "remembered in the fog"
    );
    // Bring the camera within hearing range before the teleport picks up the rune.
    client.stand_at((spot.0 + 400, spot.1));
    client.stand_at(spot);
    client.within(SOON, "the rune taken", |c| {
        heard(c, "sound.rune.bounty") > 0
    });
    client.within(SOON, "the minimap lets it go", |c| rune_marks(c).is_empty());
    assert!(
        started(&client, "rune.bounty.owner") > 0,
        "its burst of gold"
    );
    let rune = label(&client, "DOTA_HUD_Rune_Bounty", &[("gold", String::new())]);
    client.within(SOON, "the feed's line", |c| {
        c.shown_texts().iter().any(|t| t.contains(rune.trim()))
    });
}

fn reach_rune_minute(client: &mut Client, minute: u32) {
    let target = bota_core::rules::Schedule::of(client.core().pregame_ticks()).at_minute(minute);
    let deadline = Instant::now() + Duration::from_secs(240);
    while client.view().game_tick() < target {
        assert!(Instant::now() < deadline, "the match reaches {minute}:00");
        assert!(!client.core().lost, "the connection remains live");
        client.core_mut().pump(0.0);
        std::thread::yield_now();
    }
    client.frames(2);
}

fn rune_drawn_near(client: &mut Client, kind: RuneKind, spot: (i32, i32)) -> bool {
    let model = format!("rune.{}.model", crate::runes::art(kind));
    let at = Vec2::new(spot.0 as f32, spot.1 as f32);
    let world = client.world_mut();
    world
        .query_filtered::<(&crate::loot::Idle, &Transform), With<RuneBody>>()
        .iter(world)
        .any(|(idle, t)| {
            idle.0 == model && Vec2::new(t.translation.x, -t.translation.z).distance(at) < 1000.0
        })
}

fn remembered_rune(client: &Client, id: bota_proto::EntityId) -> Option<crate::runes::Remembered> {
    client
        .world()
        .resource::<crate::runes::RuneMemory>()
        .runes
        .iter()
        .find(|r| r.id == id)
        .copied()
}

fn forget_unseen_water_at_four(
    client: &mut Client,
    water: bota_proto::RuneView,
    jungle: bota_proto::EntityId,
) {
    client.stand_at(AWAY);
    client.until("the Water Rune remembered out of sight", |c| {
        remembered_rune(c, water.id).is_some_and(|r| !r.seen)
    });
    reach_rune_minute(client, 4);
    assert!(
        remembered_rune(client, water.id).is_none(),
        "unseen water expires at its recorded deadline"
    );
    assert!(
        remembered_rune(client, jungle).is_some(),
        "the persistent jungle bounty stays remembered"
    );
    let spot = top_spot();
    assert!(
        !client
            .world()
            .resource::<crate::runes::RuneMemory>()
            .runes
            .iter()
            .any(|r| {
                r.kind == RuneKind::Water
                    && r.at.distance(Vec2::new(spot.0 as f32, spot.1 as f32)) < 1000.0
            }),
        "the unseen 4:00 replacement is not revealed"
    );
}

fn see_opening_bounty(client: &mut Client, spot: usize) -> bota_proto::EntityId {
    let at = bota_core::DOTA_RUNE_SPOTS[spot].at;
    client.stand_at((at.x.to_int() + 400, at.y.to_int()));
    client
        .view()
        .runes
        .iter()
        .find(|r| r.kind == RuneKind::Bounty && r.pos == at)
        .expect("the opening bounty at this spawner")
        .id
}

#[test]
fn opening_river_bounties_and_unseen_water_expire_at_their_server_deadlines() {
    let server = Server::start(1, &["--map", "0", "--runes", "all"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::All,
    });
    client.cheat(Cheat::StartGame);
    let jungle = see_opening_bounty(&mut client, 3);
    let river = see_opening_bounty(&mut client, 0);
    client.stand_at(AWAY);
    client.until("both bounties remembered out of sight", |c| {
        [jungle, river]
            .iter()
            .all(|id| remembered_rune(c, *id).is_some_and(|r| !r.seen))
    });
    reach_rune_minute(&mut client, 2);
    assert!(
        remembered_rune(&client, river).is_none(),
        "the expired river bounty leaves fog memory"
    );
    assert!(
        remembered_rune(&client, jungle).is_some_and(|r| !r.seen),
        "the jungle bounty persists"
    );
    let spot = top_spot();
    let at = Spot::from_ints(spot.0, spot.1);
    assert!(
        !client
            .world()
            .resource::<crate::runes::RuneMemory>()
            .runes
            .iter()
            .any(|r| {
                r.kind == RuneKind::Water
                    && r.at.distance(Vec2::new(spot.0 as f32, spot.1 as f32)) < 1000.0
            }),
        "the unseen 2:00 replacement is not revealed"
    );
    client.stand_at((spot.0 + 400, spot.1));
    assert!(
        client
            .view()
            .runes
            .iter()
            .all(|r| r.kind != RuneKind::Bounty || !r.pos.within(at, bota_core::rules::units(1000))),
        "the river's opening bounty is gone"
    );
    let water = client
        .view()
        .runes
        .iter()
        .find(|r| r.kind == RuneKind::Water && r.pos == at)
        .copied()
        .expect("the top Water Rune");
    assert_eq!(
        water.expires_at,
        Some(bota_core::rules::Schedule::of(client.core().pregame_ticks()).at_minute(4))
    );
    client.within(SOON, "water replaces the bounty model", |c| {
        rune_drawn_near(c, RuneKind::Water, spot) && !rune_drawn_near(c, RuneKind::Bounty, spot)
    });
    forget_unseen_water_at_four(&mut client, water, jungle);
}

/// A Haste Rune appearing at the river shows Dota's toast; a right click on it walks the hero
/// there and takes it: its sound, the hero's line, the buff's loop on the hero, its chip on the
/// buff bar and the chat line in Dota's words.
#[test]
fn a_rune_clicked_is_taken_with_its_sound_line_buff_and_chat() {
    let (_server, mut client) = rune_match();
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Haste, spot);
    let top = label(&client, "DOTA_HUD_rune_spawned_top", &[]);
    client.within(SOON, "the toast", |c| {
        let world = c.world_mut();
        let toasted = world
            .query::<(&crate::rune_toast::RuneToast, &Visibility)>()
            .iter(world)
            .any(|(t, shown)| {
                *shown != Visibility::Hidden && matches!(t.0, Some((RuneKind::Haste, false, _)))
            });
        toasted && c.shown_texts().contains(&top)
    });
    client.within(SOON, "the Haste Rune drawn", |c| {
        drawn(c, RuneKind::Haste).is_some()
    });
    point_at(&mut client, (spot.0 as f32, spot.1 as f32));
    client.click(MouseButton::Right);
    client.within(SOON, "the haste on the hero", |c| {
        wears(c, EffectId::RUNE_HASTE)
    });
    client.within(SOON, "its sound and the hero's line", |c| {
        heard(c, "sound.rune.haste") > 0 && heard(c, "sound.voice.nevermore.rune.haste") > 0
    });
    client.within(SOON, "the buff's loop", |c| {
        started(c, "rune.haste.owner") > 0
    });
    let me = client.me().id;
    client.core_mut().act(Intent::SelectUnit(Some(me)));
    client.within(SOON, "the buff's chip", |c| {
        let world = c.world_mut();
        world
            .query::<(&crate::buffs::BuffChip, &InheritedVisibility)>()
            .iter(world)
            .any(|(chip, seen)| {
                seen.get() && chip.shown.is_some_and(|s| s.id == EffectId::RUNE_HASTE.0)
            })
    });
    let name = crate::seat_name::seat_name(client.core(), client.core().my_slot.expect("a seat"));
    let line = label(&client, "DOTA_Chat_Rune_Haste", &[]).replace("%s1", &name);
    client.within(SOON, "the chat line", |c| shown_spans(c).contains(&line));
}

/// The icon and the two counts of an item slot of one's hero, as drawn.
fn slot_face(client: &mut Client, slot: u8) -> (Option<String>, String, String) {
    let item_box = client.item_box(slot).expect("the slot's box");
    let world = client.world_mut();
    let server = world.resource::<AssetServer>().clone();
    let mut face = (None, String::new(), String::new());
    let mut parts = world.query::<(&Part, &PartOf, Option<&ImageNode>, Option<&Text>)>();
    for (part, _, image, text) in parts.iter(world).filter(|p| p.1.0 == item_box) {
        match (part, image, text) {
            (Part::Icon, Some(image), _) => {
                face.0 = server.get_path(image.image.id()).map(|p| p.to_string());
            }
            (Part::Charges, _, Some(text)) => face.1 = text.0.clone(),
            (Part::OtherCharges, _, Some(text)) => face.2 = text.0.clone(),
            _ => {}
        }
    }
    face
}

/// The Bottle as Dota draws it: full with its three charges, the medium icon once one is
/// drunk (its pour heard, its swirl on the drinker); a rune walked over goes into it, corked,
/// with the hero's line, its icon then the Bottle holding that rune, the seconds before it goes
/// off on the slot and the rune in Dota's words in the tooltip; used, the rune takes effect.
#[test]
fn the_bottle_shows_its_charges_and_the_rune_it_keeps() {
    let (_server, mut client) = rune_match();
    client.cheat(Cheat::Item {
        item: ItemId::BOTTLE,
    });
    let slot = client
        .slot_of(ItemId::BOTTLE)
        .expect("the Bottle in the bag");
    let full = file(&client, "icon.item.bottle");
    client.within(SOON, "the full Bottle", |c| {
        let face = slot_face(c, slot);
        face.0.as_deref() == Some(full.as_str()) && face.1 == "3"
    });
    // The pour is worn by the hero's drawn body: the drink (2.7 s) must not run out before the
    // hero's model has loaded on a busy machine.
    client.within(SOON, "one's hero drawn", |c| c.body(c.me().id).is_some());
    client.follow_me();
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    let medium = file(&client, "icon.item.bottle_medium");
    client.within(SOON, "a charge drunk", |c| {
        let face = slot_face(c, slot);
        face.0.as_deref() == Some(medium.as_str()) && face.1 == "2"
    });
    client.within(SOON, "the pour", |c| {
        heard(c, "sound.bottle.drink") > 0 && started(c, "item.bottle") > 0
    });
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Arcane, spot);
    client.within(SOON, "the Arcane Rune drawn", |c| {
        drawn(c, RuneKind::Arcane).is_some()
    });
    client.stand_at(spot);
    let arcane = file(&client, "icon.item.bottle_arcane");
    client.within(SOON, "the rune in the Bottle", |c| {
        slot_face(c, slot).0.as_deref() == Some(arcane.as_str())
    });
    let seconds: u32 = slot_face(&mut client, slot)
        .2
        .parse()
        .expect("the seconds it keeps the rune");
    assert!((80..=90).contains(&seconds), "{seconds} s left");
    client.within(SOON, "the cork and the hero's line", |c| {
        heard(c, "sound.bottle.cork") > 0 && heard(c, "sound.voice.nevermore.rune.bottle") > 0
    });
    let item_box = client.item_box(slot).expect("its box");
    let on = client.centre_of(item_box).expect("laid out");
    client.mouse(on);
    let kept = label(&client, "DOTA_Tooltip_Ability_item_bottle_arcane", &[]);
    client.within(SOON, "the kept rune in the tooltip", |c| {
        shown_spans(c).iter().any(|t| t.contains(&kept))
    });
    // Used again once its half second's cooldown since the drink has run out, as a player can.
    client.within(SOON, "the Bottle ready", |c| {
        c.core().item_in(slot).is_some_and(|i| i.cooldown_left == 0)
    });
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.within(SOON, "the rune let out", |c| {
        wears(c, EffectId::RUNE_ARCANE) && heard(c, "sound.rune.arcane") > 0
    });
    client.within(SOON, "the Bottle refilled", |c| {
        slot_face(c, slot).0.as_deref() == Some(full.as_str())
    });
}

/// Whether every mesh of one's hero is drawn see-through.
fn see_through(client: &mut Client) -> bool {
    let Some(body) = client.body(client.me().id) else {
        return false;
    };
    let world = client.world_mut();
    let parts: Vec<Handle<crate::fog::FoggedMaterial>> = world
        .query::<(
            &crate::fog::HeroPart,
            &MeshMaterial3d<crate::fog::FoggedMaterial>,
        )>()
        .iter(world)
        .filter(|(part, _)| part.0 == body)
        .map(|(_, material)| material.0.clone())
        .collect();
    let materials = world.resource::<Assets<crate::fog::FoggedMaterial>>();
    !parts.is_empty()
        && parts.iter().all(|handle| {
            materials
                .get(handle)
                .is_some_and(|m| m.base.alpha_mode == AlphaMode::Blend)
        })
}

/// Under an Invisibility Rune one's hero is drawn see-through, as Dota shows an invisible hero
/// to its own side; an armed attack clicked on a rune destroys it, which the feed tells.
#[test]
fn an_invisible_hero_is_see_through_and_an_attack_click_destroys_a_rune() {
    let (_server, mut client) = rune_match();
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Invisibility, spot);
    client.within(SOON, "the Invisibility Rune drawn", |c| {
        drawn(c, RuneKind::Invisibility).is_some()
    });
    client.stand_at(spot);
    client.within(SOON, "the hero invisible", |c| {
        c.me().statuses.bits & StatusFlags::INVISIBLE != 0
    });
    client.within(SOON, "drawn see-through", see_through);
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Shield, spot);
    client.within(SOON, "the Shield Rune drawn", |c| {
        drawn(c, RuneKind::Shield).is_some()
    });
    client.key(KeyCode::KeyA, ButtonState::Pressed);
    client.key(KeyCode::KeyA, ButtonState::Released);
    point_at(&mut client, (spot.0 as f32, spot.1 as f32));
    client.click(MouseButton::Left);
    let denied = label(&client, "DOTA_HUD_Rune_DenyShield", &[]);
    client.within(SOON, "the rune destroyed, told in the feed", |c| {
        drawn(c, RuneKind::Shield).is_none()
            && c.shown_texts().iter().any(|t| t.contains(denied.trim()))
    });
    assert!(
        !wears(&client, EffectId::RUNE_SHIELD),
        "destroyed, not taken"
    );
}

/// A rune lying on the ground turns in its idle clip, as Dota's runes do (the model was posed
/// once and then stood still, because the clip was never started on it).
#[test]
fn a_rune_lying_on_the_ground_plays_its_idle_animation() {
    let (_server, mut client) = rune_match();
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Haste, spot);
    client.within(SOON, "the Haste Rune drawn", |c| {
        drawn(c, RuneKind::Haste).is_some()
    });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(100),
        ));
    let body = {
        let world = client.world_mut();
        world
            .query_filtered::<Entity, With<RuneBody>>()
            .single(world)
            .expect("one rune")
    };
    client.within(SOON, "its model's animation playing", |c| {
        let world = c.world();
        subtree(world, body).into_iter().any(|e| {
            world
                .get::<AnimationPlayer>(e)
                .is_some_and(|player| player.playing_animations().next().is_some())
        })
    });
    let before = pose_of(&client, body);
    client.frames(5);
    let after = pose_of(&client, body);
    assert!(
        before.iter().zip(&after).any(|(b, a)| b.1 != a.1),
        "half a second later no part of the rune's model has moved"
    );
}
