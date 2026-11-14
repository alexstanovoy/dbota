//! The announcements of a hero's kills (First Blood, multi-kills, kill streaks and their ends),
//! through the real client against a real server: Shadow Fiend (Dire) against an enemy Pudge it
//! kills again and again, each kill told by the server as any hero's fall, the match's game time
//! moved on by ticks (a lockstep match goes as fast as the client takes it in), never by waiting.

use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ModifierSpec, Target, Vec2};

use super::harness::{Client, MID, Server};
use super::hero_voice::{HANG, Send, Sends, begun, foe, foe_player, foe_seat, send_foe};
use crate::kill_banner::shown as banners;
use crate::kill_calls::{Call, CallLog, MULTI_AFTER_STREAK, MULTI_KILL_SECONDS};
use crate::voice::KillsHeard;

/// Shadow Fiend (Dire) and the enemy Pudge on the map without creeps, the battle begun, Shadow
/// Fiend strong enough to kill Pudge in a few blows.
fn arena(everything_in_sight: bool) -> (Server, Client, Sends) {
    arena_with(everything_in_sight, &[])
}

fn arena_with(everything_in_sight: bool, extra: &[&str]) -> (Server, Client, Sends) {
    let mut options = vec!["--map", "0", "--runes", "none"];
    options.extend_from_slice(extra);
    let server = Server::start(2, &options);
    let sends = foe_seat(&server);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::All,
    });
    client.cheat(Cheat::AllVision {
        on: everything_in_sight,
    });
    begun(&mut client);
    client.cheat(Cheat::Levels { count: 24 });
    client.cheat(Cheat::ApplyModifier {
        target: Target::None,
        spec: ModifierSpec {
            physical_damage: ModifierSpec::MAX_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: bota_proto::MAX_MODIFIER_TICKS,
    });
    client.stand_at(MID);
    (server, client, sends)
}

fn calls(client: &Client) -> Vec<Call> {
    client.world().resource::<CallLog>().0.clone()
}

fn window_ticks(client: &Client) -> u32 {
    MULTI_KILL_SECONDS * u32::from(client.core().tick_rate())
}

/// Shadow Fiend kills Pudge (standing it up first if it lies dead); the call the kill made.
fn kill_foe(client: &mut Client, sends: &Sends) -> Call {
    let seen = calls(client).len();
    send_foe(client, sends, Vec2::from_ints(MID.0 + 250, MID.1));
    let (id, pos) = foe(client).map(|u| (u.id, u.pos)).expect("the foe");
    // Frail, so that it falls in a few blows.
    client.cheat(Cheat::ApplyModifier {
        target: Target::Unit(id),
        spec: ModifierSpec {
            max_hp: ModifierSpec::MIN_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: bota_proto::MAX_MODIFIER_TICKS,
    });
    client.core_mut().act(Intent::AttackAt {
        at: (pos.x.to_f32(), pos.y.to_f32()),
        unit: Some(id),
    });
    client.within(HANG, "the foe's fall called", |c| calls(c).len() > seen);
    calls(client)[seen]
}

/// Lets `ticks` of the match's game time pass.
fn pass(client: &mut Client, ticks: u32) {
    let until = client.tick() + ticks;
    client.within(HANG, "the game time passed", |c| c.tick() >= until);
}

fn spoken(client: &Client) -> Vec<(String, std::time::Duration)> {
    client.world().resource::<KillsHeard>().0.clone()
}

fn said(client: &mut Client) -> Vec<String> {
    banners(client.world_mut())
        .into_iter()
        .map(|(text, _)| text.trim().to_owned())
        .collect()
}

fn name_of_foe(client: &Client) -> String {
    let slot = foe_player(client).expect("the foe's row").slot;
    crate::seat_name::seat_name(client.core(), slot, Some(texts(client)))
}

fn my_name(client: &Client) -> String {
    let slot = client.core().my_slot.expect("a seat");
    crate::seat_name::seat_name(client.core(), slot, Some(texts(client)))
}

fn texts(client: &Client) -> &crate::dota_text::Texts {
    client.world().resource::<crate::dota_text::Texts>()
}

/// The first hero kill of the match is First Blood, called once: the later kills are not.
#[test]
fn first_blood_is_announced_once_for_the_first_kill_only() {
    let (_server, mut client, sends) = arena(true);
    let first = kill_foe(&mut client, &sends);
    assert!(first.first_blood, "the first kill");
    let (killer, victim) = (my_name(&client), name_of_foe(&client));
    assert_eq!(
        said(&mut client),
        [format!("{killer} drew first blood by killing {victim}")],
        "the banner tells who drew first blood and from whom"
    );
    let second = kill_foe(&mut client, &sends);
    assert!(!second.first_blood, "the second kill");
    let heard: Vec<_> = spoken(&client).into_iter().map(|(line, _)| line).collect();
    assert_eq!(
        heard
            .iter()
            .filter(|line| *line == "sound.announcer.kill.first_blood")
            .count(),
        1,
        "the announcer says it once: {heard:?}"
    );
}

/// Three quick kills: a double and a triple inside the window, the triple also a kill streak of
/// three, which the announcer speaks first and the banner shows first.
#[test]
fn a_double_and_a_triple_kill_inside_the_window_are_called_and_a_streak_comes_first() {
    let (_server, mut client, sends) = arena(true);
    let window = window_ticks(&client);
    let first = kill_foe(&mut client, &sends);
    let second = kill_foe(&mut client, &sends);
    assert_eq!(second.multi, Some(2));
    let third = kill_foe(&mut client, &sends);
    assert!(
        third.tick - first.tick < window,
        "the scene's kills are quick: {} ticks of {window}",
        third.tick - first.tick
    );
    assert_eq!((third.multi, third.streak), (Some(3), Some(3)));
    let killer = my_name(&client);
    let shown = said(&mut client);
    assert_eq!(
        shown[shown.len() - 2..],
        [
            format!("{killer} is on a killing spree"),
            "with a TRIPLE kill!".to_owned()
        ],
        "the spree, then the triple without the name"
    );
    assert_eq!(
        spoken(&client),
        [
            (
                "sound.announcer.kill.first_blood".to_owned(),
                std::time::Duration::ZERO
            ),
            (
                "sound.announcer.kill.double".to_owned(),
                std::time::Duration::ZERO
            ),
            (
                "sound.announcer.kill.streak.3".to_owned(),
                std::time::Duration::ZERO
            ),
            ("sound.announcer.kill.triple".to_owned(), MULTI_AFTER_STREAK),
        ],
        "first blood alone, a double alone, then the spree and, a pause behind it, the triple"
    );
}

/// A kill after the window has run out joins no multi-kill, but the streak goes on.
#[test]
fn a_kill_after_the_window_is_no_double_but_goes_on_the_streak() {
    let (_server, mut client, sends) = arena(true);
    let window = window_ticks(&client);
    kill_foe(&mut client, &sends);
    kill_foe(&mut client, &sends);
    let third = kill_foe(&mut client, &sends);
    pass(&mut client, window);
    let late = kill_foe(&mut client, &sends);
    assert!(late.tick >= third.tick + window);
    assert_eq!(late.multi, None, "past the window");
    assert_eq!(late.streak, Some(4), "the streak goes on");
}

/// Kills spread wider than the window make a streak and no multi-kill: its first three names,
/// in the install's words, spoken and shown.
#[test]
fn a_streak_is_named_spree_dominating_and_mega_kill_as_it_grows() {
    let (_server, mut client, sends) = arena(true);
    let window = window_ticks(&client);
    let killer = my_name(&client);
    let mut streaks = Vec::new();
    let mut words = Vec::new();
    for _ in 0..5 {
        pass(&mut client, window);
        let call = kill_foe(&mut client, &sends);
        assert_eq!(call.multi, None, "every kill is alone in its window");
        streaks.push(call.streak);
        words.push(said(&mut client).last().cloned().unwrap_or_default());
    }
    assert_eq!(streaks, [None, None, Some(3), Some(4), Some(5)]);
    assert_eq!(
        words[2..],
        [
            format!("{killer} is on a killing spree"),
            format!("{killer} is dominating"),
            format!("{killer} is on a mega kill streak"),
        ]
    );
    let heard: Vec<_> = spoken(&client).into_iter().map(|(line, _)| line).collect();
    assert_eq!(
        heard[1..],
        [
            "sound.announcer.kill.streak.3",
            "sound.announcer.kill.streak.4",
            "sound.announcer.kill.streak.5"
        ],
        "the announcer's lines after First Blood"
    );
}

/// A hero on a streak who falls to an enemy hero has it ended: the event column says so, naming
/// the fallen hero, and the next kill starts a new streak.
#[test]
fn an_enemy_hero_ending_a_streak_is_told_in_the_event_column_and_the_streak_starts_over() {
    let (_server, mut client, sends) = arena(true);
    let window = window_ticks(&client);
    for _ in 0..3 {
        pass(&mut client, window);
        kill_foe(&mut client, &sends);
    }
    // Shadow Fiend frail and harmless, the foe hard-hitting and sturdy at its side.
    send_foe(&mut client, &sends, Vec2::from_ints(MID.0 + 100, MID.1));
    let foe_id = foe(&client).map(|u| u.id).expect("the foe");
    client.cheat(Cheat::ApplyModifier {
        target: Target::Unit(foe_id),
        spec: ModifierSpec {
            max_hp: ModifierSpec::MAX_SCALE,
            physical_damage: ModifierSpec::MAX_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: bota_proto::MAX_MODIFIER_TICKS,
    });
    client.cheat(Cheat::ApplyModifier {
        target: Target::None,
        spec: ModifierSpec {
            max_hp: ModifierSpec::MIN_SCALE,
            physical_damage: ModifierSpec::MIN_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: 20_000,
    });
    let seen = calls(&client).len();
    sends
        .lock()
        .expect("the foe's queue")
        .push_back(Send::Strike);
    client.within(HANG, "the streak ended", |c| calls(c).len() > seen);
    let ended = calls(&client)[seen];
    assert_eq!(ended.ended, Some(3), "{ended:?}");
    let mine = client
        .view()
        .player(client.core().my_slot.expect("a seat"))
        .and_then(|p| p.unit);
    assert_eq!(Some(ended.victim), mine, "my hero fell");
    let victim = my_name(&client);
    client.within(HANG, "the ended streak in the event column", |c| {
        crate::feed::shown(c.world_mut())
            .iter()
            .any(|(left, _, text)| {
                *left
                    && text.contains(&victim)
                    && text.contains("Killing Spree 3x Kill Streak ended")
            })
    });
    sends
        .lock()
        .expect("the foe's queue")
        .push_back(Send::Peace);
    client.cheat(Cheat::ClearModifiers {
        target: Target::None,
    });
    client.cheat(Cheat::Respawn);
    client.cheat(Cheat::ApplyModifier {
        target: Target::None,
        spec: ModifierSpec {
            physical_damage: ModifierSpec::MAX_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: bota_proto::MAX_MODIFIER_TICKS,
    });
    client.stand_at(MID);
    let again = kill_foe(&mut client, &sends);
    assert_eq!(again.streak, None, "the streak starts over after the fall");
}

/// A kill far from the viewer's hero, in a match with the fog on, is announced all the same:
/// First Blood is called and spoken. (In a game of two sides the viewer's own side always has a
/// hero in the fight, so the kill itself is never hidden from it; what is far is the viewer.)
#[test]
fn a_kill_far_from_the_viewer_in_the_fog_is_announced() {
    let (_server, mut client, sends) = arena(false);
    let far = Vec2::from_ints(MID.0 - 5500, MID.1 - 5500);
    {
        let mut queue = sends.lock().expect("the foe's queue");
        queue.push_back(Send::To(far));
        queue.push_back(Send::Create(HeroId::PUDGE, far + Vec2::from_ints(200, 0)));
        queue.push_back(Send::Strike);
    }
    client.within(HANG, "the fall in the fog called", |c| !calls(c).is_empty());
    let call = calls(&client)[0];
    assert!(call.first_blood);
    let mine = client.me().pos;
    assert!(
        mine.distance_squared(far) > 4_000 * 4_000,
        "the viewer's hero stands far from the fight"
    );
    assert_eq!(
        spoken(&client)[0].0,
        "sound.announcer.kill.first_blood",
        "and spoken"
    );
}

/// The hero's own line for a kill answers First Blood on the very kill the announcement calls so.
#[test]
fn the_hero_voice_and_the_announcement_agree_on_which_kill_is_first_blood() {
    let (mut client, players, answer) = super::hero_voice::kill_scene(|_, _| {});
    super::hero_voice::first_blood_answered(&mut client, &players, answer);
    let made = calls(&client);
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(made[0].first_blood);
    assert_eq!(
        spoken(&client)[0].0,
        "sound.announcer.kill.first_blood",
        "the announcer calls the same kill"
    );
}

/// The recording of a match in which Shadow Fiend kills Pudge `kills` times, as a spectator saw
/// it, and the calls the player's client made on the way.
fn recorded_kills(kills: usize) -> (super::fog_pov::ReplayFile, Vec<Call>) {
    let (server, mut client, sends) = arena_with(true, &["--spectators", "open"]);
    let mut recording = super::fog_pov::Recording::open(&server, false);
    for _ in 0..kills {
        kill_foe(&mut client, &sends);
    }
    let until = client.tick();
    client.within(HANG, "the recording caught up", |_| {
        recording.poll();
        recording.tick + 2 >= until
    });
    (recording.save(server), calls(&client))
}

/// What a call tells besides the bodies' handles, which a recording may name otherwise.
fn gist(calls: &[Call]) -> Vec<(u32, bool, Option<u16>, Option<u16>, Option<u16>)> {
    calls
        .iter()
        .map(|c| (c.tick, c.first_blood, c.multi, c.streak, c.ended))
        .collect()
}

/// A recording played again announces the same kills at the same ticks as the match did live.
#[test]
fn a_replay_announces_the_same_kills_at_the_same_ticks_as_the_match_did() {
    let (file, live) = recorded_kills(2);
    assert_eq!(live.len(), 2, "two kills called live: {live:?}");
    let replayed = super::fog_pov::replay_observed(&file, None, &[], |_| {});
    assert_eq!(gist(&calls(&replayed)), gist(&live));
}

/// A client that begins to listen after the first kill was made is shown it by the scores, and
/// calls no First Blood on the second.
#[test]
fn a_client_joining_after_the_first_kill_calls_no_first_blood_on_the_second() {
    use clap::Parser;
    let (file, live) = recorded_kills(2);
    assert!(live[0].first_blood && !live[1].first_blood, "{live:?}");
    let path = file.path().to_str().expect("a UTF-8 path");
    let args = crate::Args::parse_from(["dbota", "--replay", path]);
    let mut core = crate::session::open(&args).expect("the replay opens").0;
    let mut counting = crate::kill_calls::Counting::default();
    let mut made = Vec::new();
    for _ in 0..1_000_000 {
        core.pump(1.0 / f32::from(core.tick_rate().max(1)));
        let Some(tick) = core.view().map(|view| view.tick) else {
            continue;
        };
        // Listening begins in the frame after the first kill's.
        if tick > live[0].tick {
            made.extend(counting.read(&core, 0));
        }
        if let bota_client_core::Source::Replay(player) = &core.source
            && (player.finished() || player.error().is_some())
        {
            break;
        }
    }
    assert_eq!(made.len(), 1, "only the second kill: {made:?}");
    assert!(!made[0].first_blood);
}
