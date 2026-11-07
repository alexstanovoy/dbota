//! A client process can open several matches, each with its own scenery and vision.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::loading::Loading;
use crate::main_menu::{Act, MenuCore, Page};

fn act(client: &mut Client, act: Act) {
    client.world_mut().non_send_mut::<MenuCore>().act(act);
}

fn replace(client: &mut Client, field: usize, text: String) {
    act(client, Act::Focus(field));
    for _ in 0..256 {
        act(client, Act::Erase);
    }
    act(client, Act::Type(text));
}

fn disconnect(client: &mut Client) {
    let old_bars: Vec<_> = client
        .world()
        .resource::<crate::units::UnitIndex>()
        .0
        .values()
        .filter_map(|body| client.world().resource::<crate::bars::Bars>().root(*body))
        .collect();
    act(client, Act::Disconnect);
    act(client, Act::Answer(true));
    client.until("back at the menu", |c| {
        !c.world().non_send::<MenuCore>().in_game()
    });
    client.frames(2);
    assert!(
        !client.world().resource::<Loading>().shown,
        "the loading overlay covers the menu after disconnect"
    );
    for bar in old_bars {
        assert!(
            client.world().get_entity(bar).is_err(),
            "the old health bar remains in the menu"
        );
    }
    assert!(
        channel_fill(client).is_none(),
        "the old channel bar remains in the menu"
    );
}

#[test]
fn consecutive_matches_draw_their_own_forest_photo_and_veil() {
    let mut client = Client::run(&[]);
    let mut previous_chunks = Vec::new();
    for map in ["1", "2", "1"] {
        let server = Server::start(1, &["--map", map]);
        connect(&mut client, &server);
        let mut photographed = false;
        client.until("the next match loaded", |c| {
            let taking = crate::minimap::photographing(c.world_mut());
            photographed |= taking;
            if taking {
                assert!(
                    !c.world().resource::<Loading>().done(),
                    "loading lifted during photography"
                );
                if let Some(veil) = c.world().get_resource::<crate::fog::VeilImage>() {
                    let image = c
                        .world()
                        .resource::<Assets<Image>>()
                        .get(&veil.0)
                        .expect("veil");
                    assert!(
                        image
                            .data
                            .as_ref()
                            .expect("texels")
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|rgba| rgba[3] == 0),
                        "the next photograph is taken through the previous fog"
                    );
                }
            }
            c.world().resource::<Loading>().done()
        });
        assert!(photographed, "map {map}: a fresh photograph was rendered");
        assert_eq!(
            crate::trees::drawn(client.world()).len(),
            client.core().trees().len(),
            "map {map}: every tree belongs to this match"
        );
        let chunks = crate::trees::chunk_entities(client.world());
        assert!(!chunks.is_empty(), "map {map}: forest meshes drawn");
        for old in &previous_chunks {
            assert!(
                client.world().get_entity(*old).is_err(),
                "old forest mesh survives: {old}"
            );
        }
        previous_chunks = chunks;
        pick(&mut client);
        client.until("the next veil drawn", |c| {
            c.world()
                .resource::<crate::fog::FogGrid>()
                .worked_out_at()
                .is_some()
        });
        let world = client.world();
        let n = world.resource::<crate::fog::FogGrid>().n;
        let image = world
            .resource::<Assets<Image>>()
            .get(&world.resource::<crate::fog::VeilImage>().0)
            .expect("veil image");
        assert_eq!(image.size(), UVec2::splat(n as u32));
        assert_eq!(image.data.as_ref().map(Vec::len), Some(n * n * 4));
        disconnect(&mut client);
        drop(server);
    }
}

/// Valid replay descriptions can choose any terrain axis, unlike the built-in servers.
fn replay(axis: u32, trees: Vec<bota_proto::Vec2>) -> Vec<u8> {
    use bota_proto::{FrameReader, ReplayRecord, ServerMsg};
    let mut reader = FrameReader::new();
    reader.push(&crate::units::tests::replay_of_heroes_on_grid(1, 1));
    let mut bytes = Vec::new();
    while let Some(mut record) = reader
        .next_message::<ReplayRecord>()
        .expect("fixture record")
    {
        if let ReplayRecord::Msg(ServerMsg::MatchStart { info }) = &mut record {
            info.terrain_cells = axis;
            info.trees = trees.clone();
            info.terrain_rle.clear();
            let mut left = axis * axis;
            while left > 0 {
                let run = left.min(u32::from(u16::MAX));
                info.terrain_rle.push((run as u16, 0x80));
                left -= run;
            }
        }
        if let ReplayRecord::Msg(ServerMsg::Snapshot { view }) = &mut record {
            view.units[0].owner = Some(bota_proto::SlotId(0));
            view.units[0].statuses.bits |= bota_proto::StatusFlags::CHANNELLING;
            view.players[0].unit = Some(view.units[0].id);
            view.tick = if axis == 512 { 1800 } else { 900 };
            view.runes = vec![bota_proto::RuneView {
                id: bota_proto::EntityId {
                    idx: 100,
                    generation: 0,
                },
                pos: bota_proto::Vec2::from_ints(1800, 1800),
                kind: bota_proto::RuneKind::Haste,
                gold: None,
                seen_by: None,
                expires_at: None,
            }];
            view.loot = vec![bota_proto::LootView {
                id: bota_proto::EntityId {
                    idx: 101,
                    generation: 0,
                },
                pos: bota_proto::Vec2::from_ints(1800, 1900),
                item: bota_proto::ItemId::IRON_BRANCH,
                charges: None,
                seen_by: None,
                level: 1,
            }];
        }
        bota_proto::encode_frame(&record, &mut bytes).expect("fixture encodes");
        if let ReplayRecord::Msg(ServerMsg::Snapshot { view }) = &record {
            bota_proto::encode_frame(
                &ReplayRecord::Msg(ServerMsg::Events {
                    tick: view.tick,
                    events: vec![bota_proto::EventKind::AbilityCast {
                        caster: view.units[0].id,
                        ability: bota_proto::AbilityId::CAPTURE,
                    }],
                }),
                &mut bytes,
            )
            .expect("the channel event");
        }
    }
    bytes
}

#[test]
fn consecutive_replays_resize_the_veil_through_the_maximum_terrain_axis() {
    // Hold the client turn through playback, as the other replay tests do.
    let _server = Server::start(1, &[]);
    let mut client = Client::run(&[]);
    let mut old_bodies = Vec::new();
    let mut previous_tree = None;
    for axis in [288, bota_client_core::MAX_TERRAIN_AXIS, 64] {
        eprintln!("checking replay axis {axis}");
        let path = client.replay_dir().join(format!("axis-{axis}.brp"));
        let trees = if axis == 64 {
            Vec::new()
        } else {
            vec![bota_proto::Vec2::from_ints(
                1000 + (axis % 3) as i32 * 600,
                2000,
            )]
        };
        std::fs::create_dir_all(client.replay_dir()).expect("the replay folder");
        std::fs::write(&path, replay(axis, trees.clone())).expect("the replay");
        act(&mut client, Act::Go(Page::Replays));
        replace(&mut client, 0, path.display().to_string());
        act(&mut client, Act::Submit);
        client.until("the replay opened", |c| {
            c.core().terrain_cells() == axis as usize
        });
        client.core_mut().my_slot = Some(bota_proto::SlotId(0));
        client.frames(1);
        for old in &old_bodies {
            assert!(
                client.world().get_entity(*old).is_err(),
                "previous match body survives into loading: {old}"
            );
        }
        assert!(
            client
                .world()
                .resource::<crate::input::ControlGroups>()
                .0
                .iter()
                .all(Option::is_none),
            "the next match inherits control groups"
        );
        client.until("the replay loaded and its veil drawn", |c| {
            c.core().view().is_some()
                && c.world().resource::<Loading>().done()
                && c.world().resource::<crate::fog::FogGrid>().n == axis as usize
        });
        let world = client.world();
        let image = world
            .resource::<Assets<Image>>()
            .get(&world.resource::<crate::fog::VeilImage>().0)
            .expect("veil image");
        assert_eq!(image.size(), UVec2::splat(axis));
        assert_eq!(
            image.data.as_ref().map(Vec::len),
            Some(axis as usize * axis as usize * 4)
        );
        assert_eq!(
            crate::trees::drawn(client.world()).len(),
            trees.len(),
            "the replay forest belongs to this game"
        );
        for tree in &trees {
            assert!(
                tree_shade(&client, *tree) > 100,
                "the new forest shades its ground"
            );
        }
        if let Some(old) = previous_tree {
            assert_eq!(
                tree_shade(&client, old),
                0,
                "the previous forest still shades the ground"
            );
        }
        previous_tree = trees.first().copied();
        client.until("ground entities drawn", |c| {
            let world = c.world_mut();
            world.query::<&crate::runes::RuneBody>().iter(world).count() == 1
                && world.query::<&crate::loot::Loot>().iter(world).count() == 1
        });
        client.until("the channel bar showing", |c| channel_fill(c).is_some());
        assert!(
            channel_fill(&mut client).expect("channel bar") < 5.0,
            "the channel bar inherited the previous start tick"
        );
        let world = client.world_mut();
        old_bodies = world
            .query_filtered::<Entity, Or<(With<crate::runes::RuneBody>, With<crate::loot::Loot>)>>()
            .iter(world)
            .collect();
        world.resource_mut::<crate::input::ControlGroups>().0[0] = Some(bota_proto::EntityId {
            idx: 1,
            generation: 0,
        });
        disconnect(&mut client);
        assert!(
            client
                .world()
                .resource::<crate::runes::RuneMemory>()
                .runes
                .is_empty(),
            "the menu retains the previous match rune knowledge"
        );
    }
}

fn tree_shade(client: &Client, at: bota_proto::Vec2) -> u8 {
    let field = &client
        .world()
        .resource::<crate::layers::GroundLayers>()
        .field;
    let image = client
        .world()
        .resource::<Assets<Image>>()
        .get(field)
        .expect("ground field");
    let x = (at.x.to_f32() / crate::layers::FIELD_CELL) as usize;
    let y = (at.y.to_f32() / crate::layers::FIELD_CELL) as usize;
    image.data.as_ref().expect("field texels")[(y * 2 * crate::layers::FIELD_CELLS + x) * 4 + 3]
}

fn channel_fill(client: &mut Client) -> Option<f32> {
    let world = client.world_mut();
    let shown = world
        .query_filtered::<&Visibility, With<crate::channel_bar::ChannelBar>>()
        .single(world)
        .expect("channel bar");
    if *shown == Visibility::Hidden {
        return None;
    }
    let fill = world
        .query_filtered::<&Node, With<crate::channel_bar::ChannelFill>>()
        .single(world)
        .expect("channel fill");
    let Val::Percent(share) = fill.width else {
        panic!("a percent channel fill")
    };
    Some(share)
}

#[test]
fn leaving_preparation_does_not_suppress_the_next_games_purchase_sound() {
    use bevy::time::TimeUpdateStrategy;
    use bota_client_core::CueKind;
    use bota_proto::ItemId;
    use std::time::Duration;
    let mut client = Client::run(&[]);
    let first = Server::start(1, &["--tactical-secs", "120"]);
    connect(&mut client, &first);
    client.until("the first game loaded", |c| {
        c.world().resource::<Loading>().done()
    });
    pick(&mut client);
    assert!(client.core().in_tactical_phase());
    client.core_mut().act(Intent::Buy(ItemId::IRON_BRANCH.0));
    client.until("the starter purchase queued", |c| {
        c.core().queued_buys.len() == 1
    });
    client.frames(2);
    // Only advance the client clock deliberately, so the five-second cue suppression
    // window cannot expire while a loaded machine connects and loads the next game.
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    disconnect(&mut client);
    drop(first);
    let second = Server::start(1, &[]);
    connect(&mut client, &second);
    client.until("the next scenery ready", |c| {
        c.world().resource::<crate::trees::SceneryReady>().0
    });
    client.frames(1);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            250,
        )));
    client.frames(3);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    client.until("the next game loaded", |c| {
        c.world().resource::<Loading>().done()
    });
    pick(&mut client);
    client.until("the next hero standing", |c| c.core().ears().is_some());
    let before = crate::sound::cues_sounded(client.world(), CueKind::Buy);
    client.core_mut().act(Intent::Buy(ItemId::IRON_BRANCH.0));
    client.until("the next purchase in the inventory", |c| {
        c.me()
            .items
            .iter()
            .flatten()
            .any(|item| item.id == ItemId::IRON_BRANCH)
    });
    client.frames(2);
    assert!(
        crate::sound::cues_sounded(client.world(), CueKind::Buy) > before,
        "the previous game's pending purchases suppressed this purchase sound"
    );
    disconnect(&mut client);
}

fn connect(client: &mut Client, server: &Server) {
    act(client, Act::Go(Page::LocalLobby));
    replace(client, 1, server.address.clone());
    act(client, Act::Direct(false));
    client.until("the server connected", |c| c.core().my_slot.is_some());
    client.frames(1);
}

fn pick(client: &mut Client) {
    client.until("the hero picked", |c| {
        if c.core().view().is_none() && !c.core().ready {
            c.core_mut().act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            c.core_mut().act(Intent::ToggleReady);
        }
        c.core().in_tactical_phase() || c.core().ears().is_some()
    });
}

fn open_replay_file(client: &mut Client, path: &std::path::Path) {
    act(client, Act::Go(Page::Replays));
    replace(client, 0, path.display().to_string());
    act(client, Act::Submit);
}

fn replay_paused(client: &Client) -> Option<bool> {
    match &client.core().source {
        bota_client_core::Source::Replay(player) => Some(player.paused),
        _ => None,
    }
}

#[test]
fn the_first_replay_is_held_while_loading() {
    let _server = Server::start(1, &[]);
    let mut client = Client::run(&[]);
    std::fs::create_dir_all(client.replay_dir()).expect("the replay folder");
    let path = client.replay_dir().join("probe-control.brp");
    std::fs::write(&path, replay(288, Vec::new())).expect("the replay");
    open_replay_file(&mut client, &path);
    client.until("the replay's first view", |c| c.core().view().is_some());
    // hold_replay runs before pump in PreUpdate: the pause lands on the frame after the view.
    client.frames(2);
    assert!(
        !client.world().resource::<Loading>().done(),
        "loading finished before the first view: the hold has nothing to hold"
    );
    assert_eq!(
        replay_paused(&client),
        Some(true),
        "a fresh client holds the replay while its loading screen is up"
    );
    client.until("loading completion releases replay playback", |c| {
        c.world().resource::<Loading>().done() && replay_paused(c) == Some(false)
    });
}

#[test]
fn a_replay_opened_after_a_minute_of_uptime_is_held_while_loading() {
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;
    let _server = Server::start(1, &[]);
    let mut client = Client::run(&[]);
    // Virtual time clamps a frame to 250 ms, so 245 frames of 250 ms make 61.25 s of client uptime.
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            250,
        )));
    client.frames(245);
    assert!(
        client.world().resource::<Time>().elapsed_secs() > 60.0,
        "the probe must pass the hold's 60 s of uptime"
    );
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )));
    std::fs::create_dir_all(client.replay_dir()).expect("the replay folder");
    let path = client.replay_dir().join("probe-late.brp");
    std::fs::write(&path, replay(288, Vec::new())).expect("the replay");
    open_replay_file(&mut client, &path);
    client.until("the replay's first view", |c| c.core().view().is_some());
    client.frames(2);
    assert!(
        !client.world().resource::<Loading>().done(),
        "loading finished before the first view: the hold has nothing to hold"
    );
    assert_eq!(
        replay_paused(&client),
        Some(true),
        "a replay opened after a minute of client uptime plays while its loading screen is up"
    );
}

#[test]
fn the_second_replay_is_held_after_leaving_the_first_while_it_loads() {
    let _server = Server::start(1, &[]);
    let mut client = Client::run(&[]);
    std::fs::create_dir_all(client.replay_dir()).expect("the replay folder");
    let first = client.replay_dir().join("probe-first.brp");
    let second = client.replay_dir().join("probe-second.brp");
    std::fs::write(&first, replay(288, Vec::new())).expect("the replay");
    std::fs::write(&second, replay(288, Vec::new())).expect("the replay");
    open_replay_file(&mut client, &first);
    client.until("the first replay's first view", |c| {
        c.core().view().is_some()
    });
    client.frames(2);
    assert!(
        !client.world().resource::<Loading>().done(),
        "the first replay loaded before it could be left mid-loading"
    );
    assert_eq!(
        replay_paused(&client),
        Some(true),
        "the first replay is held"
    );
    disconnect(&mut client);
    open_replay_file(&mut client, &second);
    client.until("the second replay's first view", |c| {
        c.core().view().is_some()
    });
    client.frames(2);
    assert!(
        !client.world().resource::<Loading>().done(),
        "the second replay loaded before its start was checked"
    );
    assert_eq!(
        replay_paused(&client),
        Some(true),
        "the second replay plays while its loading screen is up: the first replay's hold outlived it"
    );
}

#[test]
fn a_replay_resumes_when_its_own_loading_timeout_expires() {
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;
    let _server = Server::start(1, &[]);
    let core = bota_client_core::App::new(bota_client_core::Source::Replay(Box::new(
        bota_client_core::ReplayPlayer::from_reader(std::io::Cursor::new(replay(288, Vec::new()))),
    )));
    let mut client = Client::run_with_core(&["--replay", "in_memory.brp"], core);
    client.until("the startup replay's first view", |c| {
        c.core().view().is_some()
    });
    client.frames(2);
    assert_eq!(
        replay_paused(&client),
        Some(true),
        "epoch-zero replay loading holds playback"
    );
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            250,
        )));
    for _ in 0..245 {
        // Keep an unfinished loading overlay without waiting on actual asset I/O.
        client.world_mut().resource_mut::<Loading>().shown = true;
        client.frames(1);
    }
    assert!(!client.world().resource::<Loading>().done());
    assert_eq!(
        replay_paused(&client),
        Some(false),
        "the replay-relative timeout releases playback"
    );
}

#[test]
fn returning_to_the_menu_clears_stash_grab_and_the_shops_chosen_item() {
    use bota_proto::ItemId;
    let mut client = Client::run(&[]);
    let server = Server::start(1, &[]);
    connect(&mut client, &server);
    pick(&mut client);
    client.until("the game loaded", |c| {
        c.world().resource::<Loading>().done()
    });
    let home = client.me().pos;
    client.cheat(bota_proto::Cheat::Gold { amount: 10000 });
    client.stand_at(super::harness::MID);
    for (index, item) in [ItemId::IRON_BRANCH, ItemId::BOOTS].into_iter().enumerate() {
        client.core_mut().act(Intent::Buy(item.0));
        client.until("the remote purchase reaches the stash", |c| {
            c.view()
                .player(c.core().my_slot.unwrap())
                .and_then(|p| p.stash.as_ref())
                .is_some_and(|stash| stash.iter().flatten().count() == index + 1)
        });
    }
    client.stand_at((home.x.to_int(), home.y.to_int()));
    client.world_mut().resource_mut::<crate::shop::Chosen>().0 = Some(ItemId::BOOTS.0);
    client.frames(2);
    let world = client.world_mut();
    assert!(
        world
            .query::<&crate::shop::Place>()
            .iter(world)
            .any(|place| *place == crate::shop::Place::Tree),
        "the first game's chosen item has a drawn build tree"
    );
    let mut grab = crate::shop::Grab::default();
    grab.start(client.core());
    assert!(
        grab.has_session_state(),
        "real stash purchases queue GRAB ALL moves"
    );
    client.world_mut().insert_resource(grab);
    disconnect(&mut client);
    assert!(
        !client
            .world()
            .resource::<crate::shop::Grab>()
            .has_session_state(),
        "neither moves nor the old snapshot tick survive Begin"
    );
    assert_eq!(client.world().resource::<crate::shop::Chosen>().0, None);
    let world = client.world_mut();
    assert!(
        world
            .query::<(&crate::shop::Place, &InheritedVisibility)>()
            .iter(world)
            .all(|(place, shown)| *place != crate::shop::Place::Tree || !shown.get()),
        "the first game's build tree is not drawn over the menu"
    );
    drop(server);
    let next = Server::start(1, &[]);
    connect(&mut client, &next);
    pick(&mut client);
    client.until("the next game loaded", |c| {
        c.world().resource::<Loading>().done()
    });
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .shop = true;
    client.frames(2);
    assert!(
        !client
            .world()
            .resource::<crate::shop::Grab>()
            .has_session_state()
    );
    assert_eq!(client.world().resource::<crate::shop::Chosen>().0, None);
    let world = client.world_mut();
    assert!(
        world
            .query::<&crate::shop::Place>()
            .iter(world)
            .all(|place| *place != crate::shop::Place::Tree),
        "the next game's shop has no build tree selected"
    );
    disconnect(&mut client);
}

#[test]
fn each_broken_replay_reports_its_own_read_error_once() {
    use bevy::log::{
        tracing,
        tracing_subscriber::{self, layer::SubscriberExt},
    };
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let sink = Sink::default();
    let captured = sink.0.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(move || sink.clone()),
    );
    tracing::subscriber::with_default(subscriber, || {
        let _server = Server::start(1, &[]);
        let mut client = Client::run(&[]);
        std::fs::create_dir_all(client.replay_dir()).unwrap();
        for attempt in 1..=2 {
            let path = client.replay_dir().join(format!("broken_{attempt}.brp"));
            std::fs::write(&path, [255_u8; 4]).unwrap();
            open_replay_file(&mut client, &path);
            client.frames(4);
            client.until("the broken replay returns to the menu", |c| {
                !c.world().non_send::<MenuCore>().in_game()
            });
            client.frames(2);
            let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
            assert_eq!(
                output.matches("the replay cannot be read:").count(),
                attempt,
                "one diagnostic for each distinct replay session"
            );
        }
    });
}
