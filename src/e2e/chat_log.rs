//! What the chat log holds: the lines people type and nothing else. A rune taken, a building's
//! fall or a hero's death is told by its banner or its row of the feed, never by a line of the
//! chat; a long line runs on beside its speaker and wraps within the log's box.

use std::time::Duration;

use bevy::prelude::*;
use bota_proto::{Audience, Cheat, HeroId, Order, RuneKind, Target, Team, UnitKind};

use super::harness::{Client, Server};
use super::runes::{drawn, rune_match, spawn_rune, take_rune, top_spot};

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(60);

/// The chat lines on the screen now.
fn chat(client: &mut Client) -> Vec<String> {
    crate::chat::shown_lines(client.world_mut())
}

/// A match of one seat, the game begun, that may speak.
fn talking_match() -> (Server, Client) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.until("the game may be spoken in", |c| {
        c.core().can_say() && c.world().resource::<crate::loading::Loading>().done()
    });
    (server, client)
}

/// A player takes a Water Rune and a Haste Rune: each is told by its banner at the left of the
/// screen once, and the chat log stays empty.
#[test]
fn a_rune_taken_is_told_by_its_banner_and_adds_no_chat_line() {
    let (_server, mut client) = rune_match();
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    client.until("the game shown", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    for (kind, word) in [(RuneKind::Water, "water"), (RuneKind::Haste, "haste")] {
        spawn_rune(&mut client, kind, spot);
        client.within(SOON, "the rune drawn", |c| drawn(c, kind).is_some());
        take_rune(&mut client, kind, spot);
        client.within(SOON, "its banner", |c| {
            crate::feed::toasts(c.world_mut())
                .iter()
                .any(|(_, banner)| {
                    banner
                        .iter()
                        .any(|segment| segment.text.to_lowercase().contains(word))
                })
        });
    }
    client.frames(5);
    assert_eq!(chat(&mut client), Vec::<String>::new(), "the chat log");
}

/// A tower falls to one's hero: its banner stands at the left and the chat log stays empty.
#[test]
fn a_tower_falling_is_told_by_its_banner_and_adds_no_chat_line() {
    let server = Server::start(1, &["--map", "0", "--tick-rate", "600"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::Levels { count: 24 });
    let enemy = if client.me().team == Team::Radiant {
        Team::Dire
    } else {
        Team::Radiant
    };
    let tower = client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Tower && u.team == enemy)
        .min_by_key(|u| (u.pos.x.to_int() - 9740).abs() + (u.pos.y.to_int() - 9868).abs())
        .expect("an enemy tower")
        .clone();
    client.stand_at((tower.pos.x.to_int() - 300, tower.pos.y.to_int()));
    client.core_mut().send_order_to(
        None,
        Order::Attack {
            target: Target::Unit(tower.id),
        },
    );
    client.within(SOON, "the tower's banner", |c| {
        !crate::feed::toasts(c.world_mut()).is_empty()
    });
    client.frames(5);
    assert_eq!(chat(&mut client), Vec::<String>::new(), "the chat log");
}

/// A hero dies: its row stands in the kill feed at the top right and the chat log stays empty.
#[test]
fn a_hero_dying_is_told_by_the_kill_feed_and_adds_no_chat_line() {
    let (_server, mut client) = talking_match();
    client.cheat(Cheat::Suicide);
    client.within(SOON, "the kill feed's row", |c| {
        !crate::feed::shown(c.world_mut()).is_empty()
    });
    client.frames(5);
    assert_eq!(chat(&mut client), Vec::<String>::new(), "the chat log");
}

/// One's own lines, to the allies and to everyone, are the only lines of the log, each under its
/// speaker's name, the allies' marked as such.
#[test]
fn a_typed_message_appears_in_the_chat_with_the_tag_of_its_audience() {
    let (_server, mut client) = talking_match();
    client
        .core_mut()
        .say(Audience::Team, "to the team")
        .expect("a fit line");
    client
        .core_mut()
        .say(Audience::All, "to everyone")
        .expect("a fit line");
    client.within(SOON, "both lines drawn", |c| chat(c).len() == 2);
    let lines = chat(&mut client);
    assert!(
        lines[0].starts_with("[ALLIES] ") && lines[0].ends_with(": to the team"),
        "the allies' line: {:?}",
        lines[0]
    );
    assert!(
        !lines[1].starts_with('[') && lines[1].ends_with(": to everyone"),
        "everyone's line: {:?}",
        lines[1]
    );
}

/// A line far longer than the log is wide runs on beside its speaker's name to the box's right
/// edge and wraps there, line after line within the box (it was squeezed into a narrow column
/// beside the name, a word or two a line).
#[test]
fn a_long_message_wraps_within_the_chat_box_beside_its_speaker() {
    let (_server, mut client) = talking_match();
    client
        .core_mut()
        .say(Audience::All, "short")
        .expect("a fit line");
    client.within(SOON, "the short line drawn", |c| {
        crate::chat::shown_boxes(c.world_mut()).len() == 1
    });
    let one_line = crate::chat::shown_boxes(client.world_mut())[0]
        .body
        .height();
    let long = "the quick brown fox jumps over the lazy dog "
        .repeat(3)
        .trim()
        .chars()
        .take(120)
        .collect::<String>();
    client
        .core_mut()
        .say(Audience::All, &long)
        .expect("a fit line");
    client.within(SOON, "the long line drawn", |c| {
        crate::chat::shown_boxes(c.world_mut()).len() == 2
    });
    client.frames(4);
    let boxes = crate::chat::shown_boxes(client.world_mut());
    let long = &boxes[1];
    assert!(
        long.row.max.x <= long.log.max.x + 1.0 && long.row.min.x >= long.log.min.x - 1.0,
        "the row {:?} stays within the log {:?}",
        long.row,
        long.log
    );
    assert!(
        (long.body.max.x - long.row.max.x).abs() <= 1.0,
        "the body {:?} runs to the row's right edge {:?}",
        long.body,
        long.row
    );
    assert!(
        long.body.height() >= 2.0 * one_line,
        "the body wraps into at least two lines: {} high against {one_line} for one",
        long.body.height()
    );
}
