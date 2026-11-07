//! The settings window as the player uses it: a search that lists, under the collapsed tabs, the
//! sections that hold its matches; a click on a slot of the hotkeys' panel that captures the next
//! key; the resolution that waits for a window or exclusive fullscreen and sizes the window; the
//! profile name typed in; a link of the navigation that scrolls to its section.

use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::config::{DisplayMode, Settings};
use crate::keys::{self, Action, Chord};
use crate::settings::Screen;
use crate::window::Displays;

use super::harness::Client;
use super::menu::local_menu;
use super::shell::{click, settings_open};

/// The menu with the settings window open on `tab`.
fn open_settings(tab: &str) -> Client {
    let mut client = local_menu();
    click(&mut client, "settings");
    client.until("the settings window open", |c| settings_open(c));
    click(&mut client, tab);
    client.frames(5);
    client
}

fn settings(client: &Client) -> Settings {
    client.world().resource::<Settings>().clone()
}

fn screen<R>(client: &Client, read: impl FnOnce(&Screen) -> R) -> R {
    read(client.world().resource::<Screen>())
}

/// A button of the window by name, if it shows.
fn shown(client: &mut Client, name: &str) -> bool {
    let world = client.world_mut();
    world
        .query_filtered::<(&Name, &InheritedVisibility, &ComputedNode), With<Button>>()
        .iter(world)
        .any(|(named, seen, laid)| {
            seen.get() && laid.size() != Vec2::ZERO && named.as_str() == name
        })
}

fn has_text(client: &mut Client, text: &str) -> bool {
    client.shown_texts().iter().any(|t| t == text)
}

/// Only the open tab lists its sections; a search lists, under each tab, only the sections that
/// hold matches, with how many, and dims the tabs that hold none.
#[test]
fn the_navigation_collapses_every_tab_but_the_open_one_and_a_search_shows_where_the_hits_are() {
    let mut client = open_settings("video");
    assert!(
        has_text(&mut client, "On the next start"),
        "the open tab's sections"
    );
    for other in ["Unit actions", "Profile", "Sound"] {
        assert!(
            !has_text(&mut client, other),
            "'{other}' of a collapsed tab shows"
        );
    }
    click(&mut client, "search settings");
    client.type_text("replay");
    assert_eq!(screen(&client, |s| s.query.clone()), "replay");
    assert!(
        has_text(&mut client, "SEARCH"),
        "the title says it is a search"
    );
    assert!(
        has_text(&mut client, "Replays and spectating · 3"),
        "the hotkeys' hits: {:?}",
        client.shown_texts()
    );
    assert!(has_text(&mut client, "Replays · 3"), "the options' hits");
    assert!(
        !has_text(&mut client, "On the next start"),
        "the video tab has no hits and no links"
    );
    assert!(
        has_text(&mut client, "OPTIONS · REPLAYS")
            && has_text(&mut client, "HOTKEYS · REPLAYS AND SPECTATING"),
        "the cards say their tab and section"
    );
    // The controls work in place: a hit's toggle turns.
    let before = settings(&client).replays.keep;
    click(&mut client, "save the replays of my matches");
    assert_eq!(settings(&client).replays.keep, !before);
}

/// Clearing the search brings the tab back; a click on a tab leaves the search.
#[test]
fn clearing_a_search_or_picking_a_tab_ends_it() {
    let mut client = open_settings("video");
    click(&mut client, "search settings");
    client.type_text("shadows");
    assert!(has_text(&mut client, "SEARCH"));
    click(&mut client, "clear search");
    client.frames(3);
    assert!(has_text(&mut client, "VIDEO"), "the tab is back");
    click(&mut client, "search settings");
    client.type_text("shadows");
    click(&mut client, "audio");
    assert!(
        screen(&client, |s| s.query.is_empty()),
        "the search is over"
    );
    assert!(has_text(&mut client, "AUDIO"));
}

/// A square of the panel at the top of the hotkeys takes the next key like the row's field, and
/// a key two bindings share is told in the bar.
#[test]
fn a_hotkey_square_captures_the_next_key_and_a_clash_is_told() {
    let mut client = open_settings("hotkeys");
    assert!(shown(&mut client, "slot ability 2"));
    click(&mut client, "slot ability 2");
    assert!(
        screen(&client, |s| s.capturing.is_some()),
        "the square waits for a key"
    );
    client.key(KeyCode::KeyG, ButtonState::Pressed);
    client.key(KeyCode::KeyG, ButtonState::Released);
    let second = keys::index_of(Action::Ability(1)).expect("ability 2");
    assert_eq!(
        settings(&client).keys[second],
        Some(Chord::key(KeyCode::KeyG))
    );
    assert!(screen(&client, |s| s.capturing.is_none()));
    // A is Attack's.
    click(&mut client, "slot ability 2");
    client.key(KeyCode::KeyA, ButtonState::Pressed);
    client.key(KeyCode::KeyA, ButtonState::Released);
    client.frames(3);
    assert!(
        has_text(&mut client, "Ability 2 and Attack share A"),
        "the bar of clashes: {:?}",
        client.shown_texts()
    );
    // The row's field below is the same binding.
    click(&mut client, "key attack");
    client.key(KeyCode::KeyK, ButtonState::Pressed);
    client.key(KeyCode::KeyK, ButtonState::Released);
    client.frames(3);
    assert!(
        !has_text(&mut client, "Ability 2 and Attack share A"),
        "the clash is gone"
    );
}

/// The slots the panel shows are not repeated as rows, and no neutral item has a key yet.
#[test]
fn the_slots_are_not_repeated_as_rows_and_the_neutral_slot_is_unbound() {
    let mut client = open_settings("hotkeys");
    assert!(
        !shown(&mut client, "key ability 1"),
        "a slot's key is not a row"
    );
    assert!(!shown(&mut client, "key item 1"));
    assert!(shown(&mut client, "key learn ability"));
    assert!(
        !shown(&mut client, "slot neutral item"),
        "the neutral slot is no button"
    );
    assert!(has_text(&mut client, "—"), "it reads unbound");
    // A search still finds every binding.
    click(&mut client, "search settings");
    client.type_text("ability 1");
    assert!(shown(&mut client, "key ability 1"));
}

/// The sizes the monitor offers, for the tests: 2560 x 1440 and the sizes under it.
fn monitor(client: &mut Client) {
    client.world_mut().insert_resource(Displays::new(
        (2560, 1440),
        [(1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)],
    ));
    client.frames(3);
}

/// Resolution is a monitor size: it shows the monitor's own and cannot be picked in windowed
/// fullscreen, can in a window, and picking one marks the row changed until it is reset.
#[test]
fn the_resolution_waits_for_a_window_and_a_reset_brings_the_monitors_size_back() {
    let mut client = open_settings("video");
    monitor(&mut client);
    assert_eq!(settings(&client).display, DisplayMode::Borderless);
    assert!(
        has_text(&mut client, "2560 × 1440"),
        "the monitor's own size"
    );
    click(&mut client, "resolution choice");
    assert!(
        screen(&client, |s| s.dropdown.is_none()),
        "windowed fullscreen keeps the desktop's mode"
    );
    click(&mut client, "display mode choice");
    click(&mut client, "windowed");
    click(&mut client, "resolution choice");
    assert!(
        screen(&client, |s| s.dropdown.is_some()),
        "a window takes a size"
    );
    click(&mut client, "1920 × 1080");
    assert_eq!(settings(&client).resolution, Some((1920, 1080)));
    assert!(
        shown(&mut client, "reset resolution"),
        "the row is marked changed"
    );
    click(&mut client, "reset resolution");
    assert_eq!(
        settings(&client).resolution,
        None,
        "the monitor's own size again"
    );
    assert!(!shown(&mut client, "reset resolution"));
    // Sizes over the monitor's are not offered.
    click(&mut client, "resolution choice");
    assert!(!shown(&mut client, "3840 × 2160"));
    assert!(shown(&mut client, "1280 × 720"));
}

/// Picking a size in a window sizes the window to it.
#[test]
fn a_resolution_picked_in_a_window_sizes_the_window() {
    let mut client = open_settings("video");
    monitor(&mut client);
    click(&mut client, "display mode choice");
    click(&mut client, "windowed");
    click(&mut client, "resolution choice");
    click(&mut client, "1280 × 720");
    // The compositor answers the change of mode with a resize, which the window then follows.
    let world = client.world_mut();
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window");
    world.write_message(bevy::window::WindowResized {
        window,
        width: 1920.0,
        height: 1080.0,
    });
    client.until("the window sized", |c| {
        let world = c.world_mut();
        let window = world
            .query_filtered::<&Window, With<PrimaryWindow>>()
            .single(world)
            .expect("the primary window");
        (
            window.resolution.physical_width(),
            window.resolution.physical_height(),
        ) == (1280, 720)
    });
}

/// The profile name is typed into its field, marks the row changed, and its arrow empties it.
#[test]
fn the_profile_name_is_typed_in_and_reset_empties_it() {
    let mut client = open_settings("options");
    click(&mut client, "name in local and direct games field");
    client.type_text("mira");
    assert_eq!(settings(&client).name, "mira");
    assert!(shown(&mut client, "reset name in local and direct games"));
    click(&mut client, "reset name in local and direct games");
    assert_eq!(settings(&client).name, "");
}

/// A link of the navigation scrolls the body to its section and marks it as the one in view.
#[test]
fn a_navigation_link_scrolls_to_its_section_and_is_marked() {
    let mut client = open_settings("options");
    click(&mut client, "assets link");
    client.frames(5);
    let scrolled = client.world().resource::<crate::settings::Scroll>().y;
    assert!(scrolled > 100.0, "the body scrolled: {scrolled}");
    assert_eq!(
        client.world().resource::<crate::settings::InView>().0,
        5,
        "assets is the sixth"
    );
    click(&mut client, "profile link");
    client.frames(5);
    assert_eq!(client.world().resource::<crate::settings::Scroll>().y, 0.0);
}

/// The monitor of the tests, which lacks 1600 x 900, and a saved 1600 x 900 under `display`.
fn saved_size_the_monitor_lacks(client: &mut Client, display: DisplayMode) {
    client
        .world_mut()
        .insert_resource(Displays::new((2560, 1440), [(1280, 720), (2560, 1440)]));
    let mut settings = client.world_mut().resource_mut::<Settings>();
    settings.display = display;
    settings.resolution = Some((1600, 900));
    client.frames(5);
}

/// A saved size the monitor does not list: a window takes it as it is and lists it as saved;
/// exclusive fullscreen falls back to the nearest listed size below and lists it unsupported,
/// not to be picked; windowed fullscreen shows the monitor's size and keeps the saved one. The
/// field is never "Custom".
#[test]
fn a_saved_resolution_the_monitor_lacks_is_shown_by_mode_and_never_as_custom() {
    let mut client = open_settings("video");
    saved_size_the_monitor_lacks(&mut client, DisplayMode::Windowed);
    assert!(
        has_text(&mut client, "1600 × 900"),
        "a window takes the saved size"
    );
    click(&mut client, "resolution choice");
    assert!(
        has_text(&mut client, "1600 × 900 · saved"),
        "{:?}",
        client.shown_texts()
    );
    click(&mut client, "resolution choice");

    saved_size_the_monitor_lacks(&mut client, DisplayMode::Exclusive);
    assert!(
        has_text(&mut client, "1280 × 720"),
        "the nearest size below: {:?}",
        client.shown_texts()
    );
    click(&mut client, "resolution choice");
    let unsupported = "1600 × 900 · not supported by this monitor";
    assert!(has_text(&mut client, unsupported));
    click(&mut client, unsupported);
    assert_eq!(
        settings(&client).resolution,
        Some((1600, 900)),
        "it cannot be picked"
    );

    saved_size_the_monitor_lacks(&mut client, DisplayMode::Borderless);
    assert!(
        has_text(&mut client, "2560 × 1440"),
        "windowed fullscreen shows the monitor's"
    );
    assert_eq!(
        settings(&client).resolution,
        Some((1600, 900)),
        "the saved size is kept"
    );
    assert!(!has_text(&mut client, "Custom"));
}

/// Tab puts a ring on the first control, Space does what a click does, and a click gives the
/// focus up.
#[test]
fn tab_puts_the_focus_ring_on_a_control_and_space_opens_it() {
    use bevy::input::keyboard::Key;
    let mut client = open_settings("video");
    let ring = |c: &mut Client| -> Vec<String> {
        let world = c.world_mut();
        world
            .query::<(&Name, &Outline)>()
            .iter(world)
            .filter(|(_, outline)| outline.color != Color::NONE)
            .map(|(name, _)| name.to_string())
            .collect()
    };
    assert!(
        ring(&mut client).is_empty(),
        "no ring before the keyboard is used"
    );
    client.tap_named(KeyCode::Tab, Key::Tab);
    assert_eq!(ring(&mut client), ["display mode choice"]);
    client.tap_named(KeyCode::Space, Key::Space);
    assert_eq!(
        screen(&client, |s| s.dropdown),
        Some(crate::settings::Choice::DisplayMode)
    );
    click(&mut client, "audio");
    assert!(ring(&mut client).is_empty(), "a click gives the focus up");
}
