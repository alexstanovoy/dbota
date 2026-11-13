//! A played match becomes Home's live model; a new player has no stage.

use super::{
    harness::{Client, Server},
    menu::act,
};
use crate::main_menu::{Act, MenuCore, Page, hero_stage::HeroStage};
use bevy::prelude::*;
use bota_client_core::Transport;
use bota_proto::{ClientMsg, HeroId, ServerMsg};
use std::path::{Path, PathBuf};

struct RecordingFolder(PathBuf);

impl Drop for RecordingFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl RecordingFolder {
    fn new(label: &str) -> Self {
        let directory = crate::test_support::target_dir()
            .join("e2e")
            .join(format!("home_hero-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("private recordings folder");
        Self(directory)
    }
}

fn record(directory: &Path, hero: HeroId, name: &str) -> u64 {
    let replay = directory.join(name);
    let meta = replay.with_extension("meta");
    let mut server = Server::start(
        1,
        &[
            "--replay",
            replay.to_str().unwrap(),
            "--meta",
            meta.to_str().unwrap(),
            "--meta-trailer",
        ],
    );
    let mut link =
        bota_client_core::Link::connect(&server.address, std::time::Duration::from_secs(60))
            .expect("recording connection");
    bota_client_core::join(&mut link, "player", hero, None).expect("the selected hero joins");
    let (mut left, mut ended) = (false, false);
    // One real snapshot proves play; explicit departure ends this lone seat's match without
    // waiting for combat, a clock or random damage to produce the recording.
    for _ in 0..4096 {
        match link.recv().expect("recording messages") {
            Some(ServerMsg::Snapshot { view }) if !left => {
                link.send(&ClientMsg::Ack { tick: view.tick })
                    .expect("tick acknowledged");
                link.send(&ClientMsg::LeaveGame)
                    .expect("explicit departure");
                left = true;
            }
            Some(ServerMsg::Ping) if !left => link.send(&ClientMsg::Pong).expect("pong"),
            Some(ServerMsg::MatchOver { .. }) | None => {
                ended = true;
                break;
            }
            _ => {}
        }
    }
    assert!(left, "the selected hero reached a match snapshot");
    assert!(ended, "the explicit departure ended the match");
    server.wait();
    std::fs::metadata(replay).expect("recorded replay").len()
}

fn actor_matches(client: &Client, hero: HeroId) -> bool {
    let stage = client.world().resource::<HeroStage>();
    stage.hero == Some(hero) && stage.ready()
}

fn assert_empty_stage(client: &mut Client) {
    let world = client.world_mut();
    assert!(
        world.resource::<HeroStage>().actor.is_none(),
        "no actor without a played game"
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<crate::main_menu::hero_stage::HeroImage>>()
            .iter(world)
            .count(),
        0,
        "no hero image placeholder"
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<crate::main_menu::hero_stage::ContactShadow>>()
            .iter(world)
            .count(),
        0,
        "no contact shadow placeholder"
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<crate::main_menu::hero_stage::StageEntity>>()
            .iter(world)
            .count(),
        0,
        "no stage cameras or lights"
    );
}

fn history_case(label: &str, played: bool, spectated: bool, corrupt: bool) {
    let folder = RecordingFolder::new(label);
    let mut index = String::from("dbota-replays 1\n");
    if played {
        let bytes = record(&folder.0, HeroId::SHADOW_FIEND, "played.brp");
        index.push_str(&format!("r\t100\t{bytes}\t-\tlocal\tplayed.brp\t\t\t0\n"));
    }
    if spectated {
        let bytes = record(&folder.0, HeroId::PUDGE, "spectated.brp");
        index.push_str(&format!("r\t200\t{bytes}\t-\tfile\tspectated.brp\t\t\t-\n"));
    }
    if corrupt {
        std::fs::write(folder.0.join("corrupt.brp"), [0; 2048]).expect("corrupt recording");
        std::fs::write(folder.0.join("corrupt.meta"), b"unreadable metadata")
            .expect("corrupt sidecar");
        index.push_str("r\t300\t2048\t-\tlocal\tcorrupt.brp\t\t\t0\n");
    }
    std::fs::write(folder.0.join("index.txt"), index).expect("explicit personal attribution");
    let mut client = Client::run(&["--window", "1440x900"]);
    crate::main_menu::tests::install_local_library(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
        &folder.0,
    );
    act(&mut client, Act::Go(Page::Home));
    client.until(
        "all local metadata answers, including rejected entries",
        |client| {
            crate::main_menu::tests::local_library_scan_complete(
                client.world().non_send::<MenuCore>(),
            )
        },
    );
    client.frames(5);
    let expected = played.then_some(HeroId::SHADOW_FIEND);
    assert_eq!(
        client.world().non_send::<MenuCore>().last_played_hero(),
        expected,
        "only readable, explicitly played matches supply Home's hero"
    );
    if let Some(hero) = expected {
        client.until("the readable played hero on Home", |client| {
            actor_matches(client, hero)
        });
        client.frames(3);
        assert_actor(&mut client, "hero.nevermore.model");
    } else {
        assert_empty_stage(&mut client);
    }
}

#[test]
fn home_uses_the_older_played_hero_when_the_newest_match_was_spectated() {
    history_case("spectated", true, true, false);
}

#[test]
fn home_uses_the_older_played_hero_when_the_newest_recording_is_corrupt() {
    history_case("corrupt", true, false, true);
}

#[test]
fn home_has_no_hero_when_history_contains_only_spectated_or_corrupt_recordings() {
    history_case("unplayed", false, true, true);
}

fn assert_actor(client: &mut Client, logical: &str) {
    let world = client.world_mut();
    let actor = world
        .resource::<HeroStage>()
        .actor
        .as_ref()
        .expect("posed actor");
    let pivot = actor.pivot;
    let bank = world.resource::<crate::units::ModelBank>();
    let model = &bank.0[actor.model];
    assert_eq!(model.model.logical, logical);
    let wearables = model.scenes.as_ref().expect("scenes").wearables.len();
    let idle = model.clips.iter().find(|c| c.0 == "idle").expect("idle").1;
    let parts = super::harness::subtree(world, pivot);
    assert_eq!(
        parts
            .iter()
            .filter(|e| world
                .get::<crate::units::Wearable>(**e)
                .is_some_and(|w| w.bound()))
            .count(),
        wearables
    );
    assert!(parts.iter().any(|e| {
        world
            .get::<AnimationPlayer>(*e)
            .and_then(|p| p.animation(idle))
            .is_some_and(|a| {
                !a.is_paused() && a.repeat_mode() == bevy::animation::RepeatAnimation::Forever
            })
    }));
    assert_framing(world, pivot, true);
}

fn assert_framing(world: &mut World, pivot: Entity, resting: bool) {
    // At 1440×900 the compact menu's logical frame is wider than the stage's 1440-unit anchor.
    let window = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .expect("window");
    let wanted_scale = window.height() / crate::ui::MENU_DESIGN_HEIGHT;
    let stage_left = (window.width() / wanted_scale - crate::ui::MENU_WIDTH) / 2.0;
    let (node, transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<crate::main_menu::hero_stage::HeroImage>>()
        .single(world).expect("Home viewport");
    let rect = Rect::from_center_size(
        transform.translation * node.inverse_scale_factor(),
        node.size() * node.inverse_scale_factor(),
    );
    assert!(
        (rect.min - Vec2::new(stage_left, 100.0)).length() < 0.5,
        "{rect:?}"
    );
    assert!(
        (rect.size() - Vec2::new(1040.0, 720.0)).length() < 0.5,
        "{rect:?}"
    );
    let (camera, projection) = world.query_filtered::<(&GlobalTransform, &Projection), With<crate::main_menu::hero_stage::HeroCamera>>()
        .single(world).expect("the stage camera");
    let Projection::Perspective(projection) = projection else {
        panic!("a narrow perspective lens");
    };
    assert!((projection.fov.to_degrees() - 25.0).abs() < 0.01);
    let matrix = bevy::math::proj::perspective(
        projection.fov,
        projection.aspect_ratio,
        projection.near,
        projection.far,
    ) * Mat4::from(camera.affine().inverse());
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    let mut geometry =
        bevy::ecs::system::SystemState::<crate::main_menu::hero_stage::ModelGeometry>::new(world);
    let points = geometry
        .get(world)
        .expect("model geometry queries")
        .points(pivot)
        .expect("the rendered vertices");
    let vertices = points.len();
    for point in points {
        let projected = matrix.project_point3(point + crate::portrait::STAGE);
        let point = rect.min
            + Vec2::new((projected.x + 1.0) / 2.0, (1.0 - projected.y) / 2.0) * rect.size();
        lo = lo.min(point);
        hi = hi.max(point);
    }
    assert!(
        lo.cmpge(rect.min).all() && hi.cmple(rect.max).all(),
        "the entire figure fits: {lo:?}..{hi:?}"
    );
    if resting {
        assert!(
            (480.0..560.0).contains(&(hi.y - lo.y)),
            "about 520 pixels tall: {lo:?}..{hi:?}"
        );
        assert!(
            ((lo.x + hi.x) / 2.0 - (stage_left + 520.0)).abs() < 25.0,
            "centred in the free space: {lo:?}..{hi:?}"
        );
        println!(
            "Home: {vertices} vertices, height {}, centre {}",
            hi.y - lo.y,
            (lo.x + hi.x) / 2.0
        );
    }
    let (_, node, transform) = world
        .query::<(&Name, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
        .find(|(name, ..)| name.as_str() == "home contact shadow")
        .expect("contact shadow");
    let center = transform.translation * node.inverse_scale_factor();
    assert!(
        (center - Vec2::new(stage_left + 520.0, 770.0)).length() < 0.5,
        "fixed ground line: {center:?}"
    );
}

fn idle_cycle_fits(client: &mut Client) {
    let actor = client
        .world()
        .resource::<HeroStage>()
        .actor
        .as_ref()
        .expect("ready actor");
    let pivot = actor.pivot;
    let (player, node, duration) = actor.idle.expect("idle loop");
    for sample in 0..24 {
        client
            .world_mut()
            .get_mut::<AnimationPlayer>(player)
            .expect("player")
            .animation_mut(node)
            .expect("idle")
            .pause()
            .set_seek_time(duration * (sample as f32 + 0.5) / 24.0);
        client.frames(1);
        assert_framing(client.world_mut(), pivot, false);
    }
    client
        .world_mut()
        .get_mut::<AnimationPlayer>(player)
        .expect("player")
        .animation_mut(node)
        .expect("idle")
        .set_seek_time(0.0)
        .resume();
}

#[derive(Debug, PartialEq, Eq)]
struct StageCounts {
    entities: usize,
    actor_parts: usize,
    images: usize,
    graphs: usize,
    scenes: usize,
    models: usize,
}

fn stage_counts(client: &mut Client) -> StageCounts {
    let world = client.world_mut();
    StageCounts {
        entities: world.query::<Entity>().iter(world).count(),
        actor_parts: world
            .query_filtered::<Entity, With<crate::portrait::ActorPart>>()
            .iter(world)
            .count(),
        images: world.resource::<Assets<Image>>().len(),
        graphs: world.resource::<Assets<AnimationGraph>>().len(),
        scenes: world
            .resource::<Assets<bevy::world_serialization::WorldAsset>>()
            .len(),
        models: world.resource::<crate::units::ModelBank>().0.len(),
    }
}

fn visit_home(client: &mut Client, hero: HeroId) {
    let image = {
        let world = client.world_mut();
        world
            .query_filtered::<&ImageNode, With<crate::main_menu::hero_stage::HeroImage>>()
            .single(world)
            .expect("live viewport")
            .image
            .id()
    };
    act(client, Act::Go(Page::Replays));
    client.frames(8);
    assert_empty_stage(client);
    assert!(
        !client.world().resource::<Assets<Image>>().contains(image),
        "the old stage target is released"
    );
    act(client, Act::Go(Page::Home));
    client.until("Home's existing hero restored", |client| {
        actor_matches(client, hero)
    });
    client.frames(8);
}

fn repeated_home_visits_keep_entities_and_assets_flat(client: &mut Client) {
    // Warm both pages' lazy icons and text before comparing their steady state.
    visit_home(client, HeroId::PUDGE);
    visit_home(client, HeroId::PUDGE);
    let baseline = stage_counts(client);
    for _ in 0..4 {
        visit_home(client, HeroId::PUDGE);
        assert_eq!(
            stage_counts(client),
            baseline,
            "Home visits neither accumulate entities nor assets"
        );
    }
}

#[test]
fn home_uses_the_last_local_games_hero_and_releases_the_stage_when_a_match_opens() {
    let mut client = Client::run(&["--window", "1440x900"]);
    act(&mut client, Act::Go(Page::Home));
    assert_empty_stage(&mut client);
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    let directory = target
        .join("e2e")
        .join(format!("home_hero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let _folder = RecordingFolder(directory.clone());
    std::fs::create_dir_all(&directory).expect("private replay folder");
    let mut index = String::from("dbota-replays 1\n");
    for (when, hero, name, logical) in [
        (100, HeroId::SHADOW_FIEND, "sf.brp", "hero.nevermore.model"),
        (200, HeroId::PUDGE, "pudge.brp", "hero.pudge.model"),
    ] {
        let bytes = record(&directory, hero, name);
        index.push_str(&format!("r\t{when}\t{bytes}\t-\tlocal\t{name}\t\t\t0\n"));
        std::fs::write(directory.join("index.txt"), &index).expect("the explicit local owner");
        crate::main_menu::tests::install_local_library(
            &mut client.world_mut().non_send_mut::<MenuCore>(),
            &directory,
        );
        client.until("the local match selected", |client| {
            client.world().non_send::<MenuCore>().last_played_hero() == Some(hero)
        });
        client.frames(2);
        {
            let world = client.world_mut();
            assert_eq!(
                world
                    .query_filtered::<Entity, With<crate::main_menu::hero_stage::HeroImage>>()
                    .iter(world)
                    .count(),
                1,
                "the last local game creates one live hero viewport"
            );
        }
        client.until("the last local game on Home", |client| {
            actor_matches(client, hero)
        });
        client.frames(3);
        assert_actor(&mut client, logical);
        idle_cycle_fits(&mut client);
        if let Some(out) = std::env::var_os("DBOTA_TEST_ARTIFACTS") {
            let profile = PathBuf::from(out).join(name.trim_end_matches(".brp"));
            std::fs::create_dir_all(&profile).unwrap();
            std::fs::copy(directory.join("matches.json"), profile.join("matches.json"))
                .expect("durable match record for restarting the client");
        }
    }
    repeated_home_visits_keep_entities_and_assets_flat(&mut client);
    let pivot = client
        .world()
        .resource::<HeroStage>()
        .actor
        .as_ref()
        .expect("Pudge")
        .pivot;
    crate::main_menu::tests::use_matching_local_server(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
    );
    act(&mut client, Act::Demo(HeroId::SHADOW_FIEND.0));
    client.until("the next match on screen", |client| {
        client.world().non_send::<MenuCore>().shows_game()
    });
    // The strategy screen stands its own hero on the same stage until the match begins.
    client.until("the match begun", |client| client.core().view().is_some());
    client.frames(5);
    assert!(
        client.world().resource::<HeroStage>().actor.is_none(),
        "entering a match removes the actor"
    );
    assert!(
        client.world().get_entity(pivot).is_err(),
        "the entire actor hierarchy leaves with the stage"
    );
    {
        let world = client.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<crate::main_menu::hero_stage::StageEntity>>()
                .iter(world)
                .count(),
            0,
            "no stage camera or light in the game"
        );
    }
    client.until("the demo hero standing", |client| {
        client.core().my_standing_hero().is_some()
    });
    client.until("the game's model bank loaded", |client| {
        client.world().resource::<crate::units::ModelBank>().0.len() > 2
    });
    {
        let world = client.world();
        let geometry = world.resource::<crate::units::GeometryBank>();
        let meshes = world.resource::<Assets<Mesh>>();
        for id in geometry.meshes.keys() {
            assert_eq!(
                meshes.get(*id).expect("headless mesh").asset_usage,
                bevy::asset::RenderAssetUsages::RENDER_WORLD,
                "game meshes return to GPU-only residency"
            );
        }
        assert_eq!(
            world.resource::<crate::units::ReplacedScenes>().0,
            0,
            "the game reuses the menu's loaded scenes"
        );
    }
    act(&mut client, Act::Disconnect);
    client.until("the demo left", |client| {
        !client.world().non_send::<MenuCore>().in_game()
    });
    act(&mut client, Act::Go(Page::Home));
    client.until("Pudge still on Home after the demo", |client| {
        actor_matches(client, HeroId::PUDGE)
    });
    drop(client);
}
