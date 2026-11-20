//! The end of a match a fallen Ancient decided, as the player sees it, frame by frame on a clock
//! the test moves: the hero takes no more orders and the interface steps back, the camera flies
//! to the falling Ancient (never into the ground) and turns about it while the destruction plays,
//! the caption names the winner at its beat and the right stinger plays (the winner's to the
//! winners and to watchers, the loser's to the losers), and ten seconds after the decision the
//! post-game screen comes by itself. The same in 1v1 and on the full map, for either winner,
//! live and in a replay, at a low frame rate and after a long frame.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::{Cheat, HeroId, Target, Team, UnitKind};

use super::harness::{Client, Server};
use crate::finale::{CAPTION_SECS, FLIGHT_SECS, Finale, LONGEST_SECS, Viewer};

/// The frame the clock is moved by: thirty a second.
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 30);

/// A building of one side, as the client is shown it.
fn building(client: &Client, kind: UnitKind, team: Team) -> Option<bota_proto::EntityId> {
    client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == kind && unit.team == team)
        .map(|unit| unit.id)
}

/// The player presses Stop (`S`), an order for its hero.
fn stop(client: &mut Client) {
    client.tap_named(
        KeyCode::KeyS,
        bevy::input::keyboard::Key::Character("s".into()),
    );
}

/// The client's hero fells `kind` of `team` with a cheat (which decides the match: the cheat's
/// tick is the match's last), and the client runs until the end has reached it.
fn fell(client: &mut Client, kind: UnitKind, team: Team) {
    client.cheat(Cheat::AllVision { on: true });
    client.until("the building in sight", |c| {
        building(c, kind, team).is_some()
    });
    let target = building(client, kind, team).expect("in sight");
    // As a match's loading screen does, the models are loaded before anything falls.
    client.until("the models loaded", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    client.until("the building drawn", |c| drawn_with_a_model(c, target));
    client
        .core_mut()
        .act(bota_client_core::Intent::Cheat(Cheat::Destroy {
            target: Target::Unit(target),
        }));
    client.until("the match decided", |c| c.core().over.is_some());
}

fn drawn_with_a_model(client: &mut Client, id: bota_proto::EntityId) -> bool {
    let world = client.world_mut();
    world
        .query::<&crate::units::Unit>()
        .iter(world)
        .any(|unit| unit.id == id && unit.model.is_some())
}

/// One frame `long` on the moved clock, after which frames are `step` long again.
fn frame(client: &mut Client, long: Duration, step: Duration) {
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(long));
    client.step_frames(1);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(step));
}

/// The moved clock's time.
fn now(client: &Client) -> f32 {
    client.world().resource::<Time<Real>>().elapsed_secs()
}

fn focus(client: &mut Client) -> Vec3 {
    let world = client.world_mut();
    world
        .query::<&crate::camera::RtsCamera>()
        .single(world)
        .expect("one camera")
        .focus
}

fn finale(client: &Client) -> &crate::finale::Run {
    client
        .world()
        .resource::<Finale>()
        .run
        .as_ref()
        .expect("the end is being shown")
}

/// The camera: where it stands, and the ground beneath it.
fn camera(client: &mut Client) -> (Vec3, f32) {
    let world = client.world_mut();
    let at = world
        .query_filtered::<&Transform, With<crate::camera::RtsCamera>>()
        .single(world)
        .expect("one camera")
        .translation;
    let ground = world.resource::<crate::map::Ground>().at(at.x, -at.z);
    (at, ground)
}

/// The model the fallen Ancient is drawn with while it falls, if one falls.
fn falling_ancient(client: &mut Client) -> Option<String> {
    let world = client.world_mut();
    let model = world
        .query_filtered::<&crate::units::Unit, With<crate::units::Dying>>()
        .iter(world)
        .find(|unit| unit.kind == UnitKind::Ancient)
        .and_then(|unit| unit.model)?;
    let bank = world.resource::<crate::units::ModelBank>();
    Some(bank.0[model].model.logical.clone())
}

fn hud_shown(client: &mut Client) -> bool {
    let world = client.world_mut();
    world
        .query_filtered::<&InheritedVisibility, With<crate::hud::HudRoot>>()
        .iter(world)
        .any(|seen| seen.get())
}

fn post_game_shown(client: &Client) -> bool {
    client
        .world()
        .non_send::<crate::postgame::PostGame>()
        .visible()
}

/// What one watched end looked like, frame by frame.
#[derive(Default)]
struct Watched {
    /// When the camera first stood over the Ancient, in seconds from the decision.
    arrived: Option<f32>,
    /// The camera's bearing about the Ancient at the caption and at the end.
    bearings: Vec<f32>,
    /// When the caption first read as it should.
    captioned: Option<f32>,
    /// When the post-game screen came.
    post_game: Option<f32>,
    /// The lowest the camera came above the ground beneath it.
    lowest: f32,
    /// Whether the interface ever showed while the end was shown.
    hud_seen: bool,
    /// Whether an error was told.
    error: Option<String>,
    /// The fallen Ancient's model while it fell.
    falling: Option<String>,
    /// The orders the client sent after the decision.
    orders_after: u64,
}

/// Watches the end from the decision, `step` a frame (one frame `long` at `long_at`), until the
/// post-game screen comes or the longest end and a margin have passed; the player tries to
/// order its hero all along.
fn watch(
    client: &mut Client,
    step: Duration,
    long: Option<(f32, Duration)>,
    caption: &str,
) -> Watched {
    // A live player's orders are counted; a replay's watcher sends none.
    let live = matches!(client.core().source, bota_client_core::Source::Live(_));
    let orders = live.then(|| client.record_sent_orders());
    let sent = orders.as_ref().map_or(0, |orders| orders.get());
    let shot = finale(client).shot;
    let mut seen = Watched {
        lowest: f32::INFINITY,
        ..Watched::default()
    };
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(step));
    let start = now(client) - finale(client).elapsed;
    let mut long = long;
    let mut frames = 0u32;
    while now(client) - start < LONGEST_SECS + 3.0 && seen.post_game.is_none() {
        frames += 1;
        // Now and then the player tries to order its hero: nothing goes out once it is decided.
        if frames.is_multiple_of(10) {
            stop(client);
        }
        let this = match long {
            Some((at, length)) if now(client) - start >= at => {
                long = None;
                length
            }
            _ => step,
        };
        frame(client, this, step);
        let clock = now(client) - start;
        let (at, ground) = camera(client);
        seen.lowest = seen.lowest.min(at.y - ground);
        if seen.arrived.is_none() && focus(client).distance(shot) < 1.0 {
            seen.arrived = Some(clock);
        }
        if client.world().resource::<Finale>().showing() {
            seen.hud_seen |= hud_shown(client);
        }
        if seen.captioned.is_none() && client.shown_texts().iter().any(|text| text == caption) {
            seen.captioned = Some(clock);
            seen.bearings.push((at.x - shot.x).atan2(at.z - shot.z));
        }
        if seen.falling.is_none() {
            seen.falling = falling_ancient(client);
        }
        if seen.error.is_none() {
            seen.error = crate::error_msg::shown(client.world_mut());
        }
        if post_game_shown(client) && seen.post_game.is_none() {
            seen.post_game = Some(clock);
            seen.bearings.push((at.x - shot.x).atan2(at.z - shot.z));
        }
    }
    seen.orders_after = orders.map_or(0, |orders| orders.get() - sent);
    seen
}

/// What every end must have been, whoever won and whoever watched.
fn assert_shown_whole(seen: &Watched, length: f32, step: f32) {
    assert_eq!(seen.orders_after, 0, "no order went out after the decision");
    assert!(!seen.hud_seen, "the interface stepped back");
    assert_eq!(seen.error, None, "no error was told");
    let arrived = seen.arrived.expect("the camera reached the Ancient");
    assert!(
        arrived <= FLIGHT_SECS + 2.0 * step,
        "the flight took {arrived} s"
    );
    let captioned = seen.captioned.expect("the caption was drawn");
    assert!(
        (CAPTION_SECS..=CAPTION_SECS + 2.0 * step).contains(&captioned),
        "the caption came at {captioned} s"
    );
    assert!(
        seen.lowest >= 150.0,
        "the camera kept above the ground: {} at its lowest",
        seen.lowest
    );
    let came = seen.post_game.expect("the post-game screen came by itself");
    assert!(
        (length..=length + 2.0 * step).contains(&came),
        "the end lasted {came} s, not {length}"
    );
    let [at_caption, at_end] = seen.bearings[..] else {
        panic!(
            "the bearing at the caption and at the end: {:?}",
            seen.bearings
        );
    };
    assert!(
        (at_end - at_caption).abs() > 0.1,
        "the camera turned about the Ancient: {at_caption} then {at_end}"
    );
}

#[test]
fn a_1v1_dire_victory_is_shown_to_the_radiant_loser_to_its_end() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    client.cheat(Cheat::StartGame);
    let orders = client.record_sent_orders();
    stop(&mut client);
    client.frames(2);
    assert!(
        orders.get() > 0,
        "the stop key orders the hero while the match runs"
    );
    fell(&mut client, UnitKind::Tower, Team::Radiant);
    let run = finale(&client);
    assert_eq!(run.winner, Team::Dire);
    assert_eq!(run.viewer, Viewer::Loser);
    assert_eq!(run.cue, "sound.music.radiant_lose", "the loser's stinger");
    let length = run.length;
    assert!(
        (length - 10.0).abs() < 0.01,
        "Dota's ten seconds to its end-game panel: {length}"
    );
    let seen = watch(&mut client, FRAME, None, "DIRE VICTORY");
    assert_eq!(
        seen.falling.as_deref(),
        Some("ancient.radiant.destruction"),
        "the Radiant Ancient falls apart"
    );
    assert_shown_whole(&seen, length, FRAME.as_secs_f32());
}

#[test]
fn a_full_map_radiant_victory_is_shown_to_the_radiant_winner_to_its_end() {
    let server = Server::start(1, &["--map", "0"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    client.cheat(Cheat::StartGame);
    fell(&mut client, UnitKind::Ancient, Team::Dire);
    let run = finale(&client);
    assert_eq!(run.winner, Team::Radiant);
    assert_eq!(run.viewer, Viewer::Winner);
    assert_eq!(run.cue, "sound.music.radiant_win", "the winner's stinger");
    let length = run.length;
    let seen = watch(&mut client, FRAME, None, "RADIANT VICTORY");
    assert_eq!(
        seen.falling.as_deref(),
        Some("ancient.dire.destruction"),
        "the Dire Ancient falls apart"
    );
    assert_shown_whole(&seen, length, FRAME.as_secs_f32());
}

/// At ten frames a second, with one frame of three seconds in the flight, the end keeps to the
/// real clock: the caption at its second and the post-game screen at the end's length.
#[test]
fn the_end_keeps_its_timing_at_a_low_frame_rate_and_after_a_long_frame() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    client.cheat(Cheat::StartGame);
    fell(&mut client, UnitKind::Tower, Team::Dire);
    let length = finale(&client).length;
    let step = Duration::from_millis(100);
    let seen = watch(
        &mut client,
        step,
        Some((0.5, Duration::from_secs(3))),
        "RADIANT VICTORY",
    );
    let captioned = seen.captioned.expect("the caption was drawn");
    assert!(
        captioned <= 3.5 + 0.2,
        "shown in the first frame past its second, after the long frame: {captioned}"
    );
    let came = seen.post_game.expect("the post-game screen came by itself");
    assert!(
        (length..=length + 0.2).contains(&came),
        "the end lasted {came} s, not {length}"
    );
}

/// A replay of a match a fallen Ancient ended shows the end to its watcher with the winner's
/// stinger.
#[test]
fn the_end_of_a_replay_is_shown_to_its_watcher_with_the_winners_stinger() {
    let replay =
        crate::test_support::scratch_path(&format!("victory-replay-{}.brp", std::process::id()));
    {
        let server = Server::start(1, &["--replay", replay.to_str().expect("a path")]);
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
        client.cheat(Cheat::StartGame);
        fell(&mut client, UnitKind::Tower, Team::Radiant);
    }
    let mut watcher = Client::run(&["--replay", replay.to_str().expect("a path"), "--mute"]);
    watcher.until("the replay's end", |c| {
        c.world().resource::<Finale>().run.is_some()
    });
    let run = finale(&watcher);
    assert_eq!(run.winner, Team::Dire);
    assert_eq!(run.viewer, Viewer::Watcher);
    assert_eq!(
        run.cue, "sound.music.dire_win",
        "a watcher hears the winner's"
    );
    let length = run.length;
    let seen = watch(&mut watcher, FRAME, None, "DIRE VICTORY");
    assert_shown_whole(&seen, length, FRAME.as_secs_f32());
    let _ = std::fs::remove_file(&replay);
}

/// A match that ends with no Ancient falling (the service ends it) keeps its short ending: no
/// flight, no caption, the post-game screen at once.
#[test]
fn a_match_the_service_ends_has_no_end_shown() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    client.cheat(Cheat::StartGame);
    server.ask_to_finish();
    client.until("the post-game screen", |c| post_game_shown(c));
    assert!(client.world().resource::<Finale>().run.is_none());
}

/// The end lifts the fog for everyone, as Dota's does: once the Ancient falls the seat's fog
/// holds no fogged cell, in 1v1 and on the full map.
#[test]
fn the_end_lifts_the_fog_for_the_seat_in_1v1_and_on_the_full_map() {
    for (map, kind) in [("2", UnitKind::Tower), ("0", UnitKind::Ancient)] {
        let server = Server::start(1, &["--map", map]);
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
        client.cheat(Cheat::StartGame);
        let fogged = |c: &Client| {
            let grid = c.world().resource::<crate::fog::FogGrid>();
            grid.cells.iter().filter(|seen| !**seen).count()
        };
        client.until("the fog worked out", |c| {
            c.world()
                .resource::<crate::fog::FogGrid>()
                .worked_out_at()
                .is_some()
        });
        assert!(fogged(&client) > 0, "map {map}: fog before the end");
        fell(&mut client, kind, Team::Dire);
        client.until("the fog lifted", |c| fogged(c) == 0);
        assert!(
            client.world().resource::<Finale>().showing(),
            "map {map}: lifted while the end is shown"
        );
    }
}

/// Escape during the end goes on to what follows it, as Dota's end lets a player go on (its
/// Continue, or a disconnect): the end fades out from there and the post-game screen follows the
/// fade. A click on the world skips nothing.
#[test]
fn escape_during_the_end_goes_on_after_its_fade_and_a_click_skips_nothing() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    client.cheat(Cheat::StartGame);
    fell(&mut client, UnitKind::Tower, Team::Dire);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    while finale(&client).elapsed < 3.0 {
        client.step_frames(1);
    }
    client.click(MouseButton::Left);
    client.step_frames(5);
    assert!(
        client.world().resource::<Finale>().showing(),
        "a click skips nothing"
    );
    let length = finale(&client).length;
    let start = now(&client) - finale(&client).elapsed;
    client.tap_named(KeyCode::Escape, bevy::input::keyboard::Key::Escape);
    let pressed = now(&client) - start;
    while !post_game_shown(&client) {
        assert!(
            now(&client) - start < pressed + 1.0,
            "the post-game screen within the fade of the Escape"
        );
        client.step_frames(1);
    }
    let left = now(&client) - start;
    assert!(
        left >= pressed + crate::finale::FADE_SECS - FRAME.as_secs_f32(),
        "the end faded out first: Escape at {pressed} s, left at {left} s"
    );
    assert!(left < length, "the end was cut short");
}

/// A connection lost during the end changes nothing of it: it plays to its length, nothing is
/// told over it, and the post-game screen comes by itself.
#[test]
fn the_end_plays_to_its_length_when_the_connection_is_lost_during_it() {
    let mut server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    client.cheat(Cheat::StartGame);
    fell(&mut client, UnitKind::Tower, Team::Dire);
    server.crash();
    let length = finale(&client).length;
    let seen = watch(&mut client, FRAME, None, "RADIANT VICTORY");
    assert_shown_whole(&seen, length, FRAME.as_secs_f32());
}

/// A spectator of a match is shown its end as a watcher: the winner's stinger and caption, to
/// its length.
#[test]
fn a_spectator_is_shown_the_end_as_a_watcher_with_the_winners_stinger() {
    // In real time: a watched lockstep match would wait for nobody.
    let server = Server::start(1, &["--spectators", "open", "--mode", "realtime"]);
    let mut player = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    let mut watcher = Client::run(&["--addr", &server.address, "--spectate", "--mute"]);
    watcher.until("the match seen", |c| c.core().view().is_some());
    player.cheat(Cheat::StartGame);
    fell(&mut player, UnitKind::Tower, Team::Radiant);
    watcher.until("the end reached the watcher", |c| {
        c.world().resource::<Finale>().run.is_some()
    });
    let run = finale(&watcher);
    assert_eq!(run.winner, Team::Dire);
    assert_eq!(run.viewer, Viewer::Watcher);
    assert_eq!(run.cue, "sound.music.dire_win", "a watcher hears the winner's");
    let length = run.length;
    let seen = watch(&mut watcher, FRAME, None, "DIRE VICTORY");
    assert_shown_whole(&seen, length, FRAME.as_secs_f32());
}

/// One sound that started: when (seconds of the moved clock), its file, its channel if tuned.
type Heard = Vec<(f32, String, Option<crate::channels::Channel>)>;

/// Records every sound that starts from now on.
fn hear(client: &mut Client) -> std::sync::Arc<std::sync::Mutex<Heard>> {
    let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let into = heard.clone();
    client.world_mut().add_observer(
        move |add: On<Add<bevy::audio::AudioPlayer>>,
              players: Query<(&bevy::audio::AudioPlayer, Option<&crate::channels::Tuned>)>,
              server: Res<AssetServer>,
              time: Res<Time<Real>>| {
            let Ok((player, tuned)) = players.get(add.entity) else {
                return;
            };
            let path = server
                .get_path(player.0.id())
                .map_or_else(String::new, |path| path.to_string());
            into.lock().expect("unpoisoned").push((
                time.elapsed_secs(),
                path,
                tuned.map(|tuned| tuned.channel),
            ));
        },
    );
    heard
}

/// What the end sounds like, heard by proxy (the sounds that start, their files and channels):
/// the loser's stinger from the decision on the Music channel, the announcer's line as the caption
/// comes in on the Announcer's, the Ancient's own destruction, and nothing else over them; and
/// once the end is over nothing of it keeps sounding.
#[test]
fn the_end_sounds_its_stinger_and_the_announcers_line_and_nothing_after_it() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let heard = hear(&mut client);
    fell(&mut client, UnitKind::Tower, Team::Radiant);
    let decided = now(&client) - finale(&client).elapsed;
    let seen = watch(&mut client, FRAME, None, "DIRE VICTORY");
    assert!(seen.post_game.is_some(), "the end was left");
    let heard = heard.lock().expect("unpoisoned").clone();
    let after: Vec<_> = heard
        .iter()
        .filter(|(at, ..)| *at >= decided - FRAME.as_secs_f32())
        .map(|(at, path, channel)| (at - decided, path.as_str(), *channel))
        .collect();
    use crate::channels::Channel;
    let started = |part: &str| after.iter().find(|(_, path, _)| path.contains(part)).copied();
    let (at, _, channel) = started("stingers/radiant_lose").expect("the loser's stinger");
    assert!(at < 2.0 * FRAME.as_secs_f32(), "the stinger at the decision: {at}");
    assert_eq!(channel, Some(Channel::Music));
    let (at, _, channel) = started("announcer_victory_dire").expect("the announcer's line");
    assert!(
        (CAPTION_SECS..CAPTION_SECS + 2.0 * FRAME.as_secs_f32()).contains(&at),
        "the line with the caption: {at}"
    );
    assert_eq!(channel, Some(Channel::Announcer));
    let others: Vec<_> = after
        .iter()
        .filter(|(_, path, _)| {
            !["stingers/radiant_lose", "announcer_victory_dire", "ancient_destruction"]
                .iter()
                .any(|part| path.contains(part))
        })
        .collect();
    assert!(others.is_empty(), "nothing else sounds over the end: {others:?}");
    client.frames(3);
    let world = client.world_mut();
    let left = world
        .query_filtered::<&bevy::audio::AudioPlayer, With<crate::mixer::GameAudio>>()
        .iter(world)
        .count();
    assert_eq!(left, 0, "nothing of the match sounds after its end");
}

/// The fall plays the reference's destruction effects where the Ancient stands (the children of
/// its `destroyfx` the client draws), the scene still holds them at the sixth second, and what is
/// left of the Ancient stays in sight to the end: nothing goes out of existence in view.
#[test]
fn the_fall_plays_its_destruction_and_leaves_its_ruin_to_the_end() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    client.cheat(Cheat::StartGame);
    fell(&mut client, UnitKind::Tower, Team::Dire);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    let effects: Vec<_> = ["e", "f", "g", "h", "j", "l"]
        .map(|child| {
            client
                .world()
                .resource::<crate::particles::Library>()
                .effect(&format!("ancient.dire.destroy.{child}"))
                .cloned()
                .unwrap_or_else(|| panic!("ancient.dire.destroy.{child} is in the library"))
        })
        .into();
    let playing = |client: &Client| {
        let playing = client.world().resource::<crate::particles::Playing>();
        effects
            .iter()
            .map(|effect| playing.instances_of(effect).count())
            .sum::<usize>()
    };
    while finale(&client).elapsed < 6.0 {
        client.step_frames(1);
    }
    for effect in &effects {
        let started = client
            .world()
            .resource::<crate::particles::Playing>()
            .started(effect);
        assert_eq!(started, 1, "each destruction effect started once");
    }
    assert!(playing(&client) > 0, "the destruction still plays at the sixth second");
    while finale(&client).elapsed < LONGEST_SECS - 0.1 {
        client.step_frames(1);
        assert_eq!(
            falling_ancient(&mut client).as_deref(),
            Some("ancient.dire.destruction"),
            "the ruin stays to the end"
        );
    }
}
