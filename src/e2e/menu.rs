//! The main menu: its buttons of a height one width,
//! none wrapping; the header naming the player first, with no account number.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::text::{ComputedTextBlock, LineBreak};
use bevy::window::{PrimaryWindow, WindowResized};

use crate::main_menu::{Act, MenuButton, MenuCore, Page};

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
            // A text field's box is a button too (named by its label for scripts), but it holds
            // what is typed, and clips it rather than lets it wrap.
            if matches!(
                world.get::<MenuButton>(button),
                Some(MenuButton(Act::Focus(_)))
            ) {
                return None;
            }
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
    for page in [Page::LocalLobby, Page::Replays] {
        uniform_buttons(&mut client, page);
    }
    // The Demo Hero page is the service's: an account's.
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "carol");
    uniform_buttons(&mut client, Page::DemoHero);
}

/// The buttons of `page` that show are one width by height and their words stay on one line.
fn uniform_buttons(client: &mut Client, page: Page) {
    act(client, Act::Go(page));
    let shown = buttons(client);
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

/// The menu of a client signed up to a matchmaking service as `login`, on Play.
fn signed_up(service: &crate::main_menu::tests::Service, login: &str) -> Client {
    // One file for each client: the tests of a process run side by side, each with a service
    // (and a certificate) of its own.
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let pem = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "target/e2e/service-{}-{}.pem",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(pem.parent().expect("a folder")).expect("the folder");
    std::fs::write(&pem, &service.pem).expect("the service's certificate");
    let pem_arg = pem.to_str().expect("a UTF-8 path").to_owned();
    let mut client = Client::run(&["--service", &service.url, "--trust-cert", &pem_arg]);
    let page = |c: &Client| c.world().non_send::<MenuCore>().page();
    // A hang guard: the machine may run many other jobs.
    let patience = Duration::from_secs(240);
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

/// The actual physical rectangle of a visible menu button or line, after Bevy's text layout.
fn menu_rectangle(client: &mut Client, label: &str, button: bool) -> Rect {
    let world = client.world_mut();
    let entity = if button {
        world
            .query_filtered::<(Entity, &Name, &InheritedVisibility), With<Button>>()
            .iter(world)
            .find(|(_, name, shown)| shown.get() && name.as_str() == label)
            .map(|(entity, ..)| entity)
    } else {
        world
            .query::<(Entity, &Text, &InheritedVisibility)>()
            .iter(world)
            .find(|(_, text, shown)| shown.get() && text.0 == label)
            .map(|(entity, ..)| entity)
    }
    .expect("the visible menu element");
    let size = world.get::<ComputedNode>(entity).expect("laid out").size();
    let centre = world
        .get::<UiGlobalTransform>(entity)
        .expect("on screen")
        .translation;
    assert!(
        size.cmpgt(Vec2::ZERO).all(),
        "a nonempty rectangle for {label}"
    );
    Rect::from_center_size(centre, size)
}

fn lobby_rectangles(client: &mut Client) -> [Rect; 3] {
    ["create", "search", "refresh"].map(|label| menu_rectangle(client, label, true))
}

fn click_menu_button(client: &mut Client, name: &str) {
    let at = menu_rectangle(client, name, true).center();
    client.mouse(at);
    client.click(MouseButton::Left);
}

fn resize_menu_window(client: &mut Client, size: UVec2) {
    let world = client.world_mut();
    let (window, width, height) = {
        let (entity, mut window) = world
            .query_filtered::<(Entity, &mut Window), With<PrimaryWindow>>()
            .single_mut(world)
            .expect("the primary window");
        window.resolution.set_physical_resolution(size.x, size.y);
        (entity, window.width(), window.height())
    };
    // The headless harness has no window backend to notify the cameras of the new size.
    world.write_message(WindowResized {
        window,
        width,
        height,
    });
    client.frames(5);
}

/// A second account creates the lobby; the client finds it through the list and join button.
fn open_password_dialog(
    client: &mut Client,
    host: &mut crate::main_menu::tests::Player,
    name: &str,
) {
    host.menu.act(Act::Go(Page::Lobbies));
    host.fill(&[name, crate::main_menu::tests::PASSWORD]);
    host.menu.act(Act::CreateLobby);
    client.within(Duration::from_secs(30), "the host's lobby created", |_| {
        host.menu.update();
        host.menu.page() == Page::Lobby
    });
    act(client, Act::Go(Page::Lobbies));
    let join = format!("join {name}");
    client.within(Duration::from_secs(30), "the locked lobby listed", |c| {
        buttons(c).iter().any(|b| b.0 == join && b.1 > 0.0)
    });
    click_menu_button(client, &join);
    assert!(
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .password
            .is_some()
    );
}

fn assert_error_in_bottom_band(client: &mut Client, error: Rect) {
    let world = client.world_mut();
    let size = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window")
        .physical_size()
        .as_vec2();
    assert!(
        (error.center().x - size.x * 0.5).abs() <= size.x * 0.02,
        "bottom-centred error in {size:?}: {error:?}"
    );
    assert!(
        error.min.y >= size.y * 0.8 && error.max.y <= size.y,
        "the error lies in the window's bottom band in {size:?}: {error:?}"
    );
}

/// A wrong password stays in the dialog. A retry after the lobby closes exercises the shared
/// status overlay, so this still detects the original page-flow bug rather than only the popup.
fn refused_password_join_keeps_content_still(
    client: &mut Client,
    host: &mut crate::main_menu::tests::Player,
    name: &str,
) {
    open_password_dialog(client, host, name);
    let before = lobby_rectangles(client);
    act(client, Act::Type("wrong password".to_owned()));
    click_menu_button(client, "ok");
    client.within(
        Duration::from_secs(30),
        "the service refuses the password",
        |c| {
            c.world()
                .non_send::<MenuCore>()
                .view()
                .password
                .is_some_and(|p| p.error.as_deref() == Some("Wrong password: try again"))
        },
    );
    assert_eq!(
        before,
        lobby_rectangles(client),
        "content during the password refusal"
    );
    host.menu.act(Act::LeaveLobby);
    client.within(Duration::from_secs(30), "the lobby closed", |_| {
        host.menu.update();
        host.menu.page() != Page::Lobby
    });
    act(client, Act::Type("another password".to_owned()));
    click_menu_button(client, "ok");
    let error = "That lobby is gone";
    client.within(
        Duration::from_secs(30),
        "the service refuses the stale join",
        |c| {
            c.world()
                .non_send::<MenuCore>()
                .view()
                .status
                .is_some_and(|status| status.error && status.text == error)
        },
    );
    assert_status_overlay_keeps_content_still(client, before, error);
}

fn assert_status_overlay_keeps_content_still(client: &mut Client, before: [Rect; 3], error: &str) {
    client.frames(2);
    assert!(
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .password
            .is_none()
    );
    let during = lobby_rectangles(client);
    let error_rectangle = menu_rectangle(client, error, false);
    click_menu_button(client, "×");
    client.frames(2);
    assert!(
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .status
            .is_none(),
        "error dismissed"
    );
    let after = lobby_rectangles(client);
    eprintln!(
        "menu before: {before:?}\nmenu during: {during:?}\nmenu after: {after:?}\nerror: {error_rectangle:?}"
    );
    assert_eq!(
        before, during,
        "the error must not move content by even one pixel"
    );
    assert_eq!(before, after, "dismissing the error must not move content");
    assert_error_in_bottom_band(client, error_rectangle);
}

/// Real password-dialog joins keep the content fixed, including with the dashboard over a
/// running match. Both window sizes use their actual bounds for the error's bottom band.
#[test]
fn a_failed_lobby_join_overlays_the_bottom_without_moving_menu_content() {
    let service = crate::main_menu::tests::service();
    let mut host = crate::main_menu::tests::Player::new(Some(&service));
    host.register("host");
    let mut client = signed_up(&service, "alice");
    client.within(Duration::from_secs(30), "the menu fonts loaded", |c| {
        let fonts = c.world().resource::<crate::ui::Fonts>();
        let loaded = c.world().resource::<Assets<Font>>();
        loaded.contains(&fonts.regular) && loaded.contains(&fonts.bold)
    });
    client.frames(2);
    for (over_game, size) in [
        (false, UVec2::new(1600, 900)),
        (true, UVec2::new(1280, 800)),
    ] {
        if over_game {
            act(&mut client, Act::Go(Page::DemoHero));
            act(&mut client, Act::Demo(bota_proto::HeroId::PUDGE.0));
            client.within(Duration::from_secs(120), "a real Demo match running", |c| {
                c.core().my_hero().is_some()
            });
            act(&mut client, Act::Dashboard);
        }
        resize_menu_window(&mut client, size);
        assert_eq!(client.world().non_send::<MenuCore>().in_game(), over_game);
        eprintln!("checking menu error: over_game={over_game}, window={size:?}");
        refused_password_join_keeps_content_still(
            &mut client,
            &mut host,
            if over_game { "live" } else { "menu" },
        );
        assert_eq!(client.world().non_send::<MenuCore>().in_game(), over_game);
    }
    act(&mut client, Act::Disconnect);
    client.until("the Demo match disconnected", |c| {
        !c.world().non_send::<MenuCore>().in_game()
    });
}

/// Play shows only what Find Match queues for, each card's subtitle naming who it is for: a human sees the rated 1x1 Mid Only "for Humans and Bots"; the bots'
/// arena, Squad, Practice (lobbies only) and the Demo Hero (its own page) are not on it.
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
    for elsewhere in ["Bots Arena", "Squad", "Practice", "Demo Hero"] {
        assert!(
            !shown.iter().any(|t| t == elsewhere),
            "{elsewhere} on Play: {shown:?}"
        );
    }
}

/// Dota's Demo Hero runs on the service like any game: the signed-in client asks for it from the
/// DEMO HERO page, the service starts its Demo ruleset's cheating game server, the client picks
/// the hero chosen and its panel's buttons and a cheat take effect in the match. Leaving ends it.
#[test]
fn demo_hero_runs_on_the_service_and_its_cheats_take_effect() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    act(&mut client, Act::Go(Page::DemoHero));
    act(&mut client, Act::Demo(bota_proto::HeroId::PUDGE.0));
    // No lobby and no countdown stand between the pick and the game: only the server starting,
    // so this guard is short (the lobby path's countdown alone was 5 s).
    client.within(Duration::from_secs(120), "the Demo Hero stands", |c| {
        c.core().my_hero().is_some()
    });
    assert!(
        client.world().non_send::<MenuCore>().page() == Page::Game,
        "straight into the game"
    );
    assert!(
        client.core().statics().is_some_and(|s| s.metadata().cheats),
        "the service's Demo is a match that cheats"
    );
    assert_eq!(
        client
            .world()
            .non_send::<MenuCore>()
            .demo_hero()
            .map(|h| h.0),
        Some(bota_proto::HeroId::PUDGE.0)
    );
    // The panel's own button, pressed as a person presses it.
    let world = client.world_mut();
    let level_max = world
        .query_filtered::<(Entity, &Name, &InheritedVisibility), With<Button>>()
        .iter(world)
        .find(|(_, name, seen)| seen.get() && name.as_str() == "demo levelmax")
        .map(|(e, ..)| e)
        .expect("the demo panel's Level Max button");
    let at = client.centre_of(level_max).expect("laid out");
    client.mouse(at);
    client.click(MouseButton::Left);
    client.until("level 30", |c| c.me().level >= 30);
    let own_gold = |c: &Client| {
        let core = c.core();
        core.view()
            .zip(core.my_slot)
            .and_then(|(view, slot)| view.player(slot))
            .and_then(|player| player.gold)
            .unwrap_or(0)
    };
    let gold = own_gold(&client);
    client.cheat(bota_proto::Cheat::Gold { amount: 5000 });
    client.until("the gold", |c| own_gold(c) >= gold + 5000);
    // Leaving ends the Demo: back on its page, with no match left to rejoin or to read.
    act(&mut client, Act::Dashboard);
    act(&mut client, Act::Disconnect);
    client.until("back on the Demo Hero page", |c| {
        let menu = c.world().non_send::<MenuCore>();
        menu.page() == Page::DemoHero && !menu.in_game()
    });
    client.until("the match over", |c| {
        c.world().non_send::<MenuCore>().games_running() == 0
    });
    let menu = client.world().non_send::<MenuCore>();
    assert_eq!(menu.page(), Page::DemoHero, "no result page for a Demo");
}

#[test]
fn unlimited_account_limits_are_drawn_and_find_match_still_queues() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    client.within(
        Duration::from_secs(30),
        "the explicit unlimited label",
        |client| {
            client
                .shown_texts()
                .iter()
                .any(|text| text == "Games going: 0 · no account limit")
        },
    );
    click_menu_button(&mut client, "find match");
    client.within(Duration::from_secs(30), "a real queue entry", |client| {
        client.world().non_send::<MenuCore>().view().searches.len() == 1
            && client
                .shown_texts()
                .iter()
                .any(|text| text == "Games going: 1 · no account limit")
    });
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Games going: 1 · no account limit")
    );
}
