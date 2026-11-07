//! A played match becomes Home's live model; a new player has no stage.

use bevy::prelude::*;
use bota_proto::{Cheat, HeroId, ModifierSpec, Order, Target, Team, UnitKind};
use crate::main_menu::{Act, MenuCore, Page, hero_stage::HeroStage};
use super::{harness::{Client, Server}, menu::act};
use std::path::{Path, PathBuf};

struct RecordingFolder(PathBuf);

impl Drop for RecordingFolder {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

fn record(directory: &Path, hero: HeroId, name: &str) -> u64 {
    let replay = directory.join(name);
    let meta = replay.with_extension("meta");
    let mut server = Server::start(1, &["--replay", replay.to_str().unwrap(), "--meta", meta.to_str().unwrap(), "--meta-trailer"]);
    let mut decisions = 0u32;
    bota_client_core::Link::play(&server.address, "player", hero, |session| {
        decisions += 1;
        assert!(decisions < 30_000, "a bounded match to fell one tower");
        let order = bota_client_core::IssuedOrder::hero;
        if decisions == 1 {
            return [Cheat::StartGame, Cheat::Levels { count: 29 }, Cheat::Invulnerable { on: true }, Cheat::AllVision { on: true },
                Cheat::ApplyModifier { target: Target::None, spec: ModifierSpec { physical_damage: ModifierSpec::MAX_SCALE, ..ModifierSpec::NOMINAL }, ticks: 30_000 }]
                .into_iter().map(|cheat| order(Order::Cheat { cheat })).collect();
        }
        let tower = session.current().and_then(|view| view.units.iter().find(|unit| unit.kind == UnitKind::Tower && unit.team == Team::Dire));
        match tower {
            Some(tower) if decisions == 2 => vec![order(Order::Cheat { cheat: Cheat::Teleport { at: tower.pos + bota_proto::Vec2::from_ints(100, 0) } })],
            Some(tower) if decisions.is_multiple_of(30) => vec![order(Order::Attack { target: Target::Unit(tower.id) })],
            _ => Vec::new(),
        }
    }).expect("a real match played to its end");
    server.wait();
    std::fs::metadata(replay).expect("recorded replay").len()
}

fn actor_matches(client: &Client, hero: HeroId) -> bool {
    let stage = client.world().resource::<HeroStage>();
    stage.hero == Some(hero) && stage.actor.as_ref().is_some_and(|actor| actor.posed)
}

fn assert_actor(client: &mut Client, logical: &str) {
    let world = client.world_mut();
    let actor = world.resource::<HeroStage>().actor.as_ref().expect("posed actor");
    let pivot = actor.pivot;
    let bank = world.resource::<crate::units::ModelBank>();
    let model = &bank.0[actor.model];
    assert_eq!(model.model.logical, logical);
    let wearables = model.scenes.as_ref().expect("scenes").wearables.len();
    let idle = model.clips.iter().find(|c| c.0 == "idle").expect("idle").1;
    let parts = super::harness::subtree(world, pivot);
    assert_eq!(parts.iter().filter(|e| world.get::<crate::units::Wearable>(**e).is_some_and(|w| w.bound())).count(), wearables);
    assert!(parts.iter().any(|e| world.get::<AnimationPlayer>(*e).is_some_and(|p| p.is_playing_animation(idle))));
    let (node, transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<crate::main_menu::hero_stage::HeroImage>>()
        .single(world).expect("Home viewport");
    let rect = Rect::from_center_size(transform.translation * node.inverse_scale_factor(), node.size() * node.inverse_scale_factor());
    assert!((rect.min - Vec2::new(456.0, 230.0)).length() < 0.5, "{rect:?}");
    assert!((rect.size() - Vec2::new(300.0, 400.0)).length() < 0.5, "{rect:?}");
}

#[test]
fn home_uses_the_last_local_games_hero_and_releases_the_stage_when_a_match_opens() {
    let mut client = Client::run(&["--window", "1440x900"]);
    act(&mut client, Act::Go(Page::Home));
    assert!(client.world().resource::<HeroStage>().actor.is_none(), "fresh profile has no actor");
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    let directory = target.join("e2e").join(format!("home_hero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let _folder = RecordingFolder(directory.clone());
    std::fs::create_dir_all(&directory).expect("private replay folder");
    let mut index = String::from("dbota-replays 1\n");
    for (when, hero, name, logical) in [(100, HeroId::SHADOW_FIEND, "sf.brp", "hero.nevermore.model"),
        (200, HeroId::PUDGE, "pudge.brp", "hero.pudge.model")] {
        let bytes = record(&directory, hero, name);
        index.push_str(&format!("r\t{when}\t{bytes}\t-\tlocal\t{name}\t\t\t0\n"));
        std::fs::write(directory.join("index.txt"), &index).expect("the explicit local owner");
        crate::main_menu::tests::install_local_library(&mut client.world_mut().non_send_mut::<MenuCore>(), &directory);
        client.until("the last local game on Home", |client| actor_matches(client, hero));
        client.frames(3);
        assert_actor(&mut client, logical);
        if let Some(out) = std::env::var_os("DBOTA_TEST_ARTIFACTS") {
            let profile = PathBuf::from(out).join(name.trim_end_matches(".brp"));
            std::fs::create_dir_all(&profile).unwrap();
            std::fs::copy(directory.join("matches.json"), profile.join("matches.json")).expect("durable match record for restarting the client");
        }
    }
    let pivot = client.world().resource::<HeroStage>().actor.as_ref().expect("Pudge").pivot;
    crate::main_menu::tests::use_matching_local_server(&mut client.world_mut().non_send_mut::<MenuCore>());
    act(&mut client, Act::Demo(HeroId::SHADOW_FIEND.0));
    client.until("the next match on screen", |client| client.world().non_send::<MenuCore>().shows_game());
    assert!(client.world().resource::<HeroStage>().actor.is_none(), "entering a match removes the actor");
    assert!(client.world().get_entity(pivot).is_err(), "the entire actor hierarchy leaves with the stage");
    {
        let world = client.world_mut();
        assert_eq!(world.query_filtered::<Entity, With<crate::main_menu::hero_stage::StageEntity>>().iter(world).count(), 0, "no stage camera or light in the game");
    }
    client.until("the demo hero standing", |client| client.core().my_standing_hero().is_some());
    act(&mut client, Act::Disconnect);
    client.until("the demo left", |client| !client.world().non_send::<MenuCore>().in_game());
    act(&mut client, Act::Go(Page::Home));
    client.until("Pudge still on Home after the demo", |client| actor_matches(client, HeroId::PUDGE));
    drop(client);
}
