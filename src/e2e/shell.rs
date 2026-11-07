//! The menu's shell, as the player meets it: the quit button at one point of the screen on every
//! screen, the settings window opened from the top bar's cog over the dimmed menu and closed by
//! its cross, the pill and the player's card saying who plays and how the menu stands with the
//! service, and Dota's own typefaces from the install.

use std::time::Duration;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::main_menu::{Act, MenuCore, Page};

use super::harness::Client;
use super::menu::{act, local_menu, signed_up};

/// The rectangle of the shown node named `name` (a button by its label), in interface pixels:
/// the mockups' pixels, whatever the window's size.
fn interface_rect(client: &mut Client, name: &str) -> Rect {
    let world = client.world_mut();
    let (node, at) = world
        .query::<(
            &Name,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
        )>()
        .iter(world)
        .find(|(named, _, _, shown)| shown.get() && named.as_str() == name)
        .map(|(_, node, at, _)| (*node, *at))
        .unwrap_or_else(|| panic!("'{name}' is shown"));
    let scale = node.inverse_scale_factor();
    Rect::from_center_size(at.translation * scale, node.size() * scale)
}

/// The window's size in interface pixels.
fn interface_size(client: &mut Client) -> Vec2 {
    let world = client.world_mut();
    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window");
    let size = window.size();
    size / world.resource::<UiScale>().0
}

fn click(client: &mut Client, name: &str) {
    let world = client.world_mut();
    let button = world
        .query_filtered::<(Entity, &Name, &InheritedVisibility), With<Button>>()
        .iter(world)
        .find(|(_, named, shown)| shown.get() && named.as_str() == name)
        .map(|(e, ..)| e)
        .unwrap_or_else(|| panic!("the '{name}' button is shown"));
    let at = client.centre_of(button).expect("laid out");
    client.mouse(at);
    client.click(MouseButton::Left);
    client.frames(5);
}

fn page(client: &Client) -> Page {
    client.world().non_send::<MenuCore>().page()
}

fn settings_open(client: &Client) -> bool {
    client.world().resource::<crate::settings::Screen>().open
}

/// Quit is at 12 from the right and 5 from the top, 44 across, on a page with the top bar, on
/// the way in (no bar), and under the settings window: the player always finds it where it was.
#[test]
fn the_quit_button_stands_at_one_point_on_every_screen() {
    let mut client = local_menu();
    let mut seen = Vec::new();
    for page in [Page::LocalLobby, Page::Replays] {
        act(&mut client, Act::Go(page));
        seen.push((format!("{page:?}"), interface_rect(&mut client, "quit")));
    }
    click(&mut client, "settings");
    client.until("the settings window open", |c| settings_open(c));
    client.frames(5);
    seen.push(("settings".to_owned(), interface_rect(&mut client, "quit")));
    click(&mut client, "close");
    client.until("the settings window closed", |c| !settings_open(c));
    act(&mut client, Act::Go(Page::Service));
    seen.push(("the way in".to_owned(), interface_rect(&mut client, "quit")));
    let width = interface_size(&mut client).x;
    for (screen, quit) in &seen {
        let at = (width - quit.max.x, quit.min.y, quit.width(), quit.height());
        assert!(
            (at.0 - 12.0).abs() < 0.5
                && (at.1 - 5.0).abs() < 0.5
                && (at.2 - 44.0).abs() < 0.5
                && (at.3 - 44.0).abs() < 0.5,
            "{screen}: quit at right {}, top {}, {} by {} (not 12, 5, 44 by 44)",
            at.0,
            at.1,
            at.2,
            at.3
        );
    }
}

/// The cog opens the settings as a window over the menu (the top bar still drawn under the dim,
/// the window under the bar's height), and the window's cross closes it back to the page it was
/// opened from.
#[test]
fn the_settings_window_opens_from_the_cog_over_the_dimmed_menu_and_its_cross_closes_it() {
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Replays));
    click(&mut client, "settings");
    client.until("the settings window open", |c| settings_open(c));
    client.frames(5);
    let window = interface_rect(&mut client, "settings window");
    let size = interface_size(&mut client);
    assert!(
        (window.min.y - 64.0).abs() < 0.5,
        "under the bar: {window:?}"
    );
    assert!(
        (window.width() - 1100.0).abs() < 0.5 && (window.center().x - size.x / 2.0).abs() < 0.5,
        "1100 wide, in the middle of {size:?}: {window:?}"
    );
    // The menu stays drawn under the dim: its bar, and the page the settings were opened from.
    interface_rect(&mut client, "dbota");
    let shown = client.shown_texts();
    assert!(
        shown.iter().any(|t| t == "No replays yet"),
        "the replays under the settings: {shown:?}"
    );
    click(&mut client, "close");
    client.until("the settings window closed", |c| !settings_open(c));
    client.frames(5);
    assert_eq!(page(&client), Page::Replays, "back where it was opened");
}

/// Without an account the pill says OFFLINE and the card names the profile; signed in, the pill
/// names the service, LOG OUT is beside it, and the card names the player alone, with no account
/// number (the header once read "alice #1").
#[test]
fn the_pill_and_the_players_card_say_how_the_menu_stands_and_who_plays() {
    let mut client = local_menu();
    client.within(Duration::from_secs(30), "the pill laid out", |c| {
        c.shown_texts().iter().any(|t| t == "OFFLINE")
    });
    let shown = client.shown_texts();
    assert!(!shown.iter().any(|t| t == "LOG OUT"), "{shown:?}");
    assert_eq!(
        card_names(&mut client),
        [crate::main_menu::DEFAULT_NAME],
        "a profile with no name plays as the default one"
    );
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    let shown = client.shown_texts();
    assert!(!shown.iter().any(|t| t == "OFFLINE"), "{shown:?}");
    let host = service
        .url
        .split_once("://")
        .and_then(|(_, rest)| rest.rsplit_once(':'))
        .map(|(host, _)| host.to_owned())
        .expect("a service address with a port");
    assert!(
        shown.contains(&host),
        "the service's name {host}: {shown:?}"
    );
    assert!(shown.iter().any(|t| t == "LOG OUT"), "{shown:?}");
    // Every tab of an account and the bar's right end fit side by side, a 5:4 window too.
    for size in [UVec2::new(1920, 1080), UVec2::new(1280, 1024)] {
        super::menu::resize_menu_window(&mut client, size);
        let (tabs, gear) = (
            interface_rect(&mut client, "replays"),
            interface_rect(&mut client, "log out"),
        );
        let quit = interface_rect(&mut client, "quit");
        let settings = interface_rect(&mut client, "settings");
        assert!(
            tabs.max.x <= gear.min.x && settings.max.x <= quit.min.x,
            "{size}: the bar's parts overlap: REPLAYS {tabs:?}, LOG OUT {gear:?}, the cog \
             {settings:?}, quit {quit:?}"
        );
    }
    let card = interface_rect(&mut client, "player");
    assert_eq!(
        card_names(&mut client),
        ["alice"],
        "the card names the player alone"
    );
    let size = interface_size(&mut client);
    assert!(
        card.min.x < size.x / 4.0 && card.max.y > size.y * 0.9,
        "the card at the bottom left of {size:?}: {card:?}"
    );
    let numbered: Vec<String> = client
        .shown_texts()
        .into_iter()
        .filter(|t| {
            t.split('#')
                .skip(1)
                .any(|n| n.starts_with(|c: char| c.is_ascii_digit()))
        })
        .collect();
    assert!(numbered.is_empty(), "an account number shown: {numbered:?}");
}

/// The menu draws with the typefaces of the player's Dota install, which the importer took into
/// the asset cache: Reaver for DBOTA, Radiance for the tabs; and they load as fonts.
#[test]
fn the_menu_draws_with_dotas_typefaces_from_the_install() {
    let mut client = local_menu();
    let face_of = |client: &mut Client, words: &str| {
        let world = client.world_mut();
        let font = world
            .query::<(&Text, &TextFont)>()
            .iter(world)
            .find(|(text, _)| text.0 == words)
            .map(|(_, font)| font.font.clone())
            .unwrap_or_else(|| panic!("'{words}' drawn"));
        let bevy::text::FontSource::Handle(handle) = font else {
            panic!("'{words}' in a font of the cache, not a family");
        };
        handle
    };
    for (words, file) in [
        ("DBOTA", "fonts/reaver-bold.otf"),
        ("REPLAYS", "fonts/radiance-semibold.otf"),
    ] {
        let handle = face_of(&mut client, words);
        let path = client
            .world()
            .resource::<AssetServer>()
            .get_path(handle.id())
            .map(|p| p.path().to_string_lossy().into_owned())
            .unwrap_or_default();
        assert_eq!(path, file, "'{words}' drawn in {path}");
        client.within(Duration::from_secs(30), "the typeface loaded", |c| {
            c.world().resource::<Assets<Font>>().contains(&handle)
        });
    }
}

/// The words on the player's card, its initial left out.
fn card_names(client: &mut Client) -> Vec<String> {
    let world = client.world_mut();
    let parts: Vec<(String, Entity)> = world
        .query::<(&Text, &ChildOf)>()
        .iter(world)
        .map(|(text, parent)| (text.0.clone(), parent.parent()))
        .collect();
    parts
        .into_iter()
        .filter(|(_, parent)| {
            world
                .get::<Name>(*parent)
                .is_some_and(|n| n.as_str() == "player")
        })
        .map(|(text, _)| text)
        .collect()
}

fn ui_scale(client: &Client) -> f32 {
    client.world().resource::<UiScale>().0
}

/// The menu is laid out at the window's own scale (one mockup pixel to one pixel at 900 lines,
/// 16:10 and 16:9 alike), not shrunk to make room for the HUD, which it covers; a 5:4 window
/// shrinks it to the 1440 pixels of its frame; a game on screen is still shrunk until its HUD
/// fits a 16:10 window.
#[test]
fn the_menu_keeps_the_windows_scale_and_a_game_shrinks_to_fit_its_hud() {
    for (window, scale) in [
        ("1440x900", 1.0),
        ("1920x1080", 1.2),
        ("1280x1024", 1280.0 / 1440.0),
    ] {
        let mut client = Client::run(&["--window", window]);
        client.within(Duration::from_secs(30), "the menu laid out", |c| {
            (ui_scale(c) - scale).abs() < 1e-3
        });
        client.frames(10);
        assert!(
            (ui_scale(&client) - scale).abs() < 1e-3,
            "the menu at {window}: scale {} (not {scale})",
            ui_scale(&client)
        );
    }
    let server = super::harness::Server::start(1, &[]);
    let mut client = Client::join(
        &server,
        bota_proto::HeroId::SHADOW_FIEND,
        &["--window", "1440x900"],
    );
    client.within(Duration::from_secs(60), "the HUD fitted", |c| {
        ui_scale(c) < 0.99
    });
}

/// The way in (the service, signing in) has no top bar, but a player without an account can
/// always leave it for the pages played without one, whose bar has the settings and SIGN IN.
#[test]
fn the_way_in_lets_a_player_without_an_account_play_and_come_back() {
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Login));
    assert_eq!(page(&client), Page::Service, "no service known yet");
    click(&mut client, "play without an account");
    client.until("a page played without an account", |c| {
        !matches!(page(c), Page::Service | Page::Login | Page::Register)
    });
    client.frames(5);
    for part in ["settings", "replays", "sign in"] {
        interface_rect(&mut client, part);
    }
    click(&mut client, "sign in");
    client.until("back on the way in", |c| page(c) == Page::Service);
}

/// The fullest bar (signed in, over a game, the longest service name the pill shows: a
/// 32-character host) stays whole in the narrowest window the menu fits, the cog left of quit:
/// the tabs it has no room for fold under MORE, whose list shows them; a wide window unfolds them.
#[test]
fn the_fullest_bar_folds_its_last_tabs_under_more_and_stays_whole() {
    let mut client = Client::run(&["--window", "1280x1024"]);
    let mut view = crate::main_menu::preview("dashboard").expect("a preview");
    view.connection = crate::main_menu::Connection::Online("a".repeat(32));
    client
        .world_mut()
        .insert_resource(crate::main_menu::Preview(view));
    client.within(Duration::from_secs(30), "the preview drawn", |c| {
        c.shown_texts().iter().any(|t| t.starts_with("aaaa"))
    });
    client.frames(5);
    let (pill, settings, quit) = (
        interface_rect(&mut client, "connection"),
        interface_rect(&mut client, "settings"),
        interface_rect(&mut client, "quit"),
    );
    let width = interface_size(&mut client).x;
    assert!(
        pill.max.x <= settings.min.x && settings.max.x <= quit.min.x && quit.max.x <= width,
        "the pill {pill:?}, the cog {settings:?}, quit {quit:?} in {width}"
    );
    let shown = |c: &mut Client, name: &str| {
        let world = c.world_mut();
        world
            .query::<(&Name, &InheritedVisibility)>()
            .iter(world)
            .any(|(named, seen)| seen.get() && named.as_str() == name)
    };
    assert!(shown(&mut client, "more"), "MORE holds the tabs folded");
    assert!(!shown(&mut client, "replays"), "REPLAYS, the last, folded");
    click(&mut client, "more");
    client.until("MORE's list open", |c| shown(c, "replays"));
    super::menu::resize_menu_window(&mut client, UVec2::new(1920, 1080));
    client.until("every tab back in the bar", |c| {
        shown(c, "replays") && !shown(c, "more")
    });
}
