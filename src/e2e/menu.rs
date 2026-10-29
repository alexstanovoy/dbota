//! The main menu: its buttons of a height one width,
//! none wrapping; the header naming the player first, with no account number.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::text::{ComputedTextBlock, LineBreak};

use crate::main_menu::{Act, MenuCore, Page};

use super::harness::Client;

/// The menu's buttons of words on screen, laid out: name, width, height, and whether their words
/// stay on one line.
fn buttons(client: &mut Client) -> Vec<(String, f32, f32, bool)> {
    let world = client.world_mut();
    let shown: Vec<(Entity, String, Vec2)> = world
        .query_filtered::<(Entity, &Name, &ComputedNode, &InheritedVisibility), With<Button>>()
        .iter(world)
        .filter(|(.., seen)| seen.get())
        .map(|(e, name, node, _)| (e, name.to_string(), node.size() * node.inverse_scale_factor))
        .collect();
    shown
        .into_iter()
        .filter_map(|(button, name, size)| {
            // A button of words alone: not an icon's, not a box ticked beside its words, not
            // one of the top bar's tabs (underlined, as wide as their words, as Dota's are).
            let node = world.get::<Node>(button)?;
            if node.border.top == Val::Px(0.0) && node.border.bottom != Val::Px(0.0) {
                return None;
            }
            let children = world.get::<Children>(button)?;
            let words = children.first().copied().filter(|_| children.len() == 1)?;
            world.get::<Text>(words)?;
            let one_line = world
                .get::<TextLayout>(words)
                .is_some_and(|l| l.linebreak == LineBreak::NoWrap)
                && world.get::<ComputedTextBlock>(words).is_some();
            Some((name, size.x, size.y, one_line))
        })
        .collect()
}

/// The menu open with no service known: its local pages.
fn local_menu() -> Client {
    let mut client = Client::run(&[]);
    client.within(Duration::from_secs(30), "the menu laid out", |c| {
        buttons(c).iter().any(|b| b.1 > 0.0)
    });
    client
}

fn act(client: &mut Client, act: Act) {
    client.world_mut().non_send_mut::<MenuCore>().act(act);
    client.frames(5);
}

/// Every page's buttons of one height are laid out one width, whatever they say, and their
/// words never wrap (a longer label made a wider button, a long one wrapped onto two lines);
/// a lone glyph's button (the close cross) is square.
#[test]
fn the_menus_buttons_of_a_height_are_one_width_and_never_wrap() {
    let mut client = local_menu();
    for page in [Page::LocalLobby, Page::Replays, Page::DemoHero] {
        act(&mut client, Act::Go(page));
        let shown = buttons(&mut client);
        assert!(!shown.is_empty(), "{page:?}: buttons");
        let mut widths: BTreeMap<i32, Vec<(String, f32)>> = BTreeMap::new();
        for (name, w, h, one_line) in &shown {
            assert!(one_line, "{page:?}: '{name}' may wrap");
            if name.chars().count() > 2 {
                widths
                    .entry(h.round() as i32)
                    .or_default()
                    .push((name.clone(), *w));
            }
        }
        for (height, row) in widths {
            let first = row[0].1;
            assert!(
                row.iter().all(|(_, w)| (w - first).abs() < 0.5),
                "{page:?}: buttons {height} high of several widths: {row:?}"
            );
        }
    }
}

/// The menu of a client signed up to a matchmaking service as `login`, on Play.
fn signed_up(service: &crate::main_menu::tests::Service, login: &str) -> Client {
    let pem = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("target/e2e/service-{}.pem", std::process::id()));
    std::fs::create_dir_all(pem.parent().expect("a folder")).expect("the folder");
    std::fs::write(&pem, &service.pem).expect("the service's certificate");
    let pem_arg = pem.to_str().expect("a UTF-8 path").to_owned();
    let mut client = Client::run(&["--service", &service.url, "--trust-cert", &pem_arg]);
    let page = |c: &Client| c.world().non_send::<MenuCore>().page();
    let patience = Duration::from_secs(60);
    client.within(patience, "the sign-in page", |c| page(c) == Page::Login);
    act(&mut client, Act::Go(Page::Register));
    let password = crate::main_menu::tests::PASSWORD;
    for (at, value) in [login, password, password].into_iter().enumerate() {
        act(&mut client, Act::Focus(at));
        act(&mut client, Act::Type(value.to_owned()));
    }
    act(&mut client, Act::Submit);
    client.within(patience, "signed up, on Play", |c| page(c) == Page::Home);
    client.frames(5);
    let _ = std::fs::remove_file(&pem);
    client
}

/// Signed in, the top bar names the player first, then its buttons (SETTINGS, LOG OUT), as
/// Dota's does, and shows no account number (it read "alice #1" after the buttons).
#[test]
fn the_header_names_the_player_first_with_no_account_number() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    let world = client.world_mut();
    let named = world
        .query::<(Entity, &Text, &InheritedVisibility)>()
        .iter(world)
        .find(|(_, text, seen)| seen.get() && text.0.contains("alice"))
        .map(|(e, text, _)| (e, text.0.clone()));
    let (name, said) = named.expect("the player's name in the top bar");
    assert_eq!(said, "alice", "the name alone, no account number");
    let bar = world.get::<ChildOf>(name).expect("in the bar").parent();
    let order: Vec<String> = world
        .get::<Children>(bar)
        .expect("the bar's parts")
        .iter()
        .filter_map(|part| {
            if part == name {
                return Some("NAME".to_owned());
            }
            world.get::<Name>(part).map(|n| n.to_string())
        })
        .collect();
    let at = |what: &str| order.iter().position(|o| o == what);
    let (named_at, settings, log_out) = (at("NAME"), at("settings"), at("log out"));
    assert!(
        named_at.is_some() && named_at < settings && settings < log_out,
        "the name, then SETTINGS, then LOG OUT: {order:?}"
    );
    let numbered = client
        .shown_texts()
        .into_iter()
        .filter(|t| {
            t.split('#')
                .skip(1)
                .any(|n| n.starts_with(|c: char| c.is_ascii_digit()))
        })
        .collect::<Vec<_>>();
    assert!(numbered.is_empty(), "an account number shown: {numbered:?}");
}

/// Play shows only what Find Match queues for, each card's subtitle naming who it is for: a human sees the rated 1x1 Mid Only "for Humans and Bots"; the bots'
/// arena, Squad and Practice (lobbies only) are not on it.
#[test]
fn play_shows_the_queued_rulesets_with_their_audience() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "bob");
    let shown = client.shown_texts();
    assert!(
        shown.iter().any(|t| t == "1x1 Mid Only"),
        "the queue's card: {shown:?}"
    );
    assert!(
        shown.iter().any(|t| t.ends_with("· Humans and Bots")),
        "its audience in its subtitle: {shown:?}"
    );
    for elsewhere in ["Bots Arena", "Squad", "Practice"] {
        assert!(
            !shown.iter().any(|t| t == elsewhere),
            "{elsewhere} on Play: {shown:?}"
        );
    }
}
