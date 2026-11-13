//! The settings window as the player uses it: a search that lists, under the collapsed tabs, the
//! sections that hold its matches; a click on a slot of the hotkeys' panel that captures the next
//! key; the resolution that waits for a window or exclusive fullscreen and sizes the window; the
//! profile name typed in; a link of the navigation that scrolls to its section.

use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
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

/// A link of the navigation scrolls the body to its section and marks it as the one in view. The
/// body holds every tab, so the first section of the options is no longer the top of the body, and
/// the sections are counted from the first tab's.
#[test]
fn a_navigation_link_scrolls_to_its_section_and_is_marked() {
    let mut client = open_settings("options");
    let in_view = |client: &Client| client.world().resource::<crate::settings::InView>().0;
    let scrolled = |client: &Client| client.world().resource::<crate::settings::Scroll>().y;
    click(&mut client, "profile link");
    client.frames(5);
    let first = in_view(&client);
    let start = scrolled(&client);
    assert!(
        start > 500.0,
        "the options start below the hotkeys: {start}"
    );
    click(&mut client, "game files link");
    client.frames(5);
    let scrolled_on = scrolled(&client);
    assert!(
        scrolled_on > start + 100.0,
        "the body scrolled on: {start} to {scrolled_on}"
    );
    assert_eq!(
        in_view(&client),
        first + 5,
        "game files is the sixth of the options"
    );
    click(&mut client, "profile link");
    client.frames(5);
    assert!(
        (scrolled(&client) - start).abs() < 1.0,
        "back at the start of the options: {start} then {}",
        scrolled(&client)
    );
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

fn ime_enabled(client: &mut Client) -> bool {
    let world = client.world_mut();
    world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window")
        .ime_enabled
}

/// The input method composes while the search field or the name has the keyboard, and not
/// otherwise: not at rest, not while a hotkey waits for its key.
#[test]
fn the_input_method_is_on_only_while_a_text_field_has_the_keyboard() {
    let mut client = open_settings("options");
    assert!(!ime_enabled(&mut client));
    click(&mut client, "search settings");
    assert!(
        ime_enabled(&mut client),
        "the search field has the keyboard"
    );
    click(&mut client, "name in local and direct games field");
    assert!(ime_enabled(&mut client), "the name has it");
    click(&mut client, "hotkeys");
    click(&mut client, "slot ability 1");
    assert!(!ime_enabled(&mut client), "a hotkey waits for a key");
    client.key(KeyCode::Escape, ButtonState::Pressed);
    client.key(KeyCode::Escape, ButtonState::Released);
    click(&mut client, "search settings");
    assert!(ime_enabled(&mut client));
    click(&mut client, "close");
    let menu_wants = client
        .world()
        .non_send::<crate::main_menu::MenuCore>()
        .wants_text();
    assert_eq!(
        ime_enabled(&mut client),
        menu_wants,
        "closing the window leaves it to the menu"
    );
}

/// A garbled file's huge size is cut to the monitor's: the window is never larger than its screen.
#[test]
fn a_saved_resolution_is_never_applied_larger_than_the_monitor() {
    let mut client = open_settings("video");
    monitor(&mut client);
    {
        let mut settings = client.world_mut().resource_mut::<Settings>();
        settings.display = DisplayMode::Windowed;
        settings.resolution = Some((7680, 4320));
    }
    client.frames(3);
    resized(&mut client);
    client.until("the window cut to the monitor", |c| {
        window_size(c) == (2560, 1440)
    });
    assert!(
        has_text(&mut client, "2560 × 1440"),
        "the field says what is used"
    );
}

fn window_size(client: &mut Client) -> (u32, u32) {
    let world = client.world_mut();
    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window");
    (
        window.resolution.physical_width(),
        window.resolution.physical_height(),
    )
}

/// The compositor's answer to a change of mode, which the window then follows.
fn resized(client: &mut Client) {
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
}

/// Resetting the resolution (the monitor's own size again) resizes the window to it.
#[test]
fn resetting_the_resolution_gives_the_window_the_monitors_size() {
    let mut client = open_settings("video");
    monitor(&mut client);
    click(&mut client, "display mode choice");
    click(&mut client, "windowed");
    click(&mut client, "resolution choice");
    click(&mut client, "1280 × 720");
    resized(&mut client);
    client.until("the window at 1280 x 720", |c| {
        window_size(c) == (1280, 720)
    });
    client.frames(10);
    click(&mut client, "reset resolution");
    client.until("the window at the monitor's size", |c| {
        window_size(c) == (2560, 1440)
    });
}

/// The sizes the list offers are those of the monitor the window is on, not the primary one.
#[test]
fn the_resolution_list_follows_the_monitor_the_window_is_on() {
    use bevy::window::{Monitor, OnMonitor, PrimaryMonitor};
    let mut client = open_settings("video");
    let monitor = |width, height| Monitor {
        name: None,
        physical_width: width,
        physical_height: height,
        physical_position: IVec2::ZERO,
        refresh_rate_millihertz: Some(60_000),
        scale_factor: 1.0,
        video_modes: Vec::new(),
    };
    let world = client.world_mut();
    world.spawn((monitor(3840, 2160), PrimaryMonitor));
    let secondary = world.spawn(monitor(1280, 720)).id();
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window");
    world.entity_mut(window).insert(OnMonitor(secondary));
    client.frames(5);
    assert_eq!(client.world().resource::<Displays>().native, (1280, 720));
}

/// The names of the controls that wear the focus ring.
fn ringed(client: &mut Client) -> Vec<String> {
    let world = client.world_mut();
    world
        .query::<(&Name, &Outline)>()
        .iter(world)
        .filter(|(_, outline)| outline.color != Color::NONE)
        .map(|(name, _)| name.to_string())
        .collect()
}

/// After a field is edited, Tab goes on to the next control, not back to the field.
#[test]
fn tab_after_editing_the_profile_name_goes_on_to_the_next_control() {
    use bevy::input::keyboard::Key;
    let mut client = open_settings("options");
    client.tap_named(KeyCode::Tab, Key::Tab);
    assert_eq!(
        ringed(&mut client),
        ["name in local and direct games field"]
    );
    client.tap_named(KeyCode::Enter, Key::Enter);
    assert!(
        screen(&client, |s| s.naming.is_some()),
        "Enter starts editing"
    );
    client.type_text("mira");
    client.tap_named(KeyCode::Tab, Key::Tab);
    assert!(screen(&client, |s| s.naming.is_none()));
    assert_eq!(ringed(&mut client), ["interface size slider"]);
    assert_eq!(settings(&client).name, "mira");
}

/// A list the keyboard opened is walked with Up and Down, picked from with Enter, and closed with
/// Escape without a change; options that cannot be picked are passed over.
#[test]
fn the_keyboard_walks_an_open_list_and_picks_from_it() {
    use bevy::input::keyboard::Key;
    let mut client = open_settings("video");
    client.tap_named(KeyCode::Tab, Key::Tab);
    client.tap_named(KeyCode::Enter, Key::Enter);
    assert_eq!(
        screen(&client, |s| s.dropdown),
        Some(crate::settings::Choice::DisplayMode)
    );
    client.tap_named(KeyCode::ArrowDown, Key::ArrowDown);
    client.tap_named(KeyCode::Escape, Key::Escape);
    assert_eq!(
        settings(&client).display,
        DisplayMode::Borderless,
        "Escape changes nothing"
    );
    assert!(screen(&client, |s| s.dropdown.is_none()));
    client.tap_named(KeyCode::Enter, Key::Enter);
    client.tap_named(KeyCode::ArrowUp, Key::ArrowUp);
    client.tap_named(KeyCode::Enter, Key::Enter);
    assert_eq!(
        settings(&client).display,
        DisplayMode::Windowed,
        "Up then Enter picks the option above"
    );
    assert!(screen(&client, |s| s.dropdown.is_none()));
}

/// An option the monitor cannot do is passed over by the arrow keys.
#[test]
fn the_arrow_keys_pass_over_an_option_that_cannot_be_picked() {
    use bevy::input::keyboard::Key;
    let mut client = open_settings("video");
    saved_size_the_monitor_lacks(&mut client, DisplayMode::Exclusive);
    // Tab to the resolution (the display mode's reset arrow, now a stop of its own, is before it).
    for _ in 0..8 {
        client.tap_named(KeyCode::Tab, Key::Tab);
        if ringed(&mut client) == ["resolution choice"] {
            break;
        }
    }
    client.tap_named(KeyCode::Enter, Key::Enter);
    assert_eq!(
        screen(&client, |s| s.dropdown),
        Some(crate::settings::Choice::Resolution)
    );
    client.tap_named(KeyCode::ArrowDown, Key::ArrowDown);
    client.tap_named(KeyCode::Enter, Key::Enter);
    assert_eq!(
        settings(&client).resolution,
        None,
        "1600 x 900 was passed over for the native size"
    );
}

/// A window moved to a smaller monitor is cut to it.
#[test]
fn a_saved_window_size_is_cut_again_when_the_window_moves_to_a_smaller_monitor() {
    let mut client = open_settings("video");
    client
        .world_mut()
        .insert_resource(Displays::new((3840, 2160), [(2560, 1440)]));
    {
        let mut settings = client.world_mut().resource_mut::<Settings>();
        settings.display = DisplayMode::Windowed;
        settings.resolution = Some((2560, 1440));
    }
    client.frames(3);
    resized(&mut client);
    client.until("the window at 2560 x 1440", |c| {
        window_size(c) == (2560, 1440)
    });
    client
        .world_mut()
        .insert_resource(Displays::new((1280, 720), [(1280, 720)]));
    client.until("the window cut to the new monitor", |c| {
        window_size(c) == (1280, 720)
    });
}

/// The focus ring touches only the window's own controls: an outline of anything else (a health
/// bar's frame) keeps its colour, with the window open or closed.
#[test]
fn the_focus_ring_leaves_every_other_outline_alone() {
    let mut client = open_settings("video");
    let frame = Color::srgb(1.0, 0.0, 0.0);
    let other = client
        .world_mut()
        .spawn(Outline::new(px(1), px(0), frame))
        .id();
    client.frames(5);
    click(&mut client, "close");
    client.frames(5);
    assert_eq!(
        client
            .world()
            .get::<Outline>(other)
            .expect("an outline")
            .color,
        frame
    );
}

/// The texture setting is a quality the player names, not a size in pixels.
#[test]
fn the_texture_setting_is_a_quality_of_four_names_and_shows_no_pixel_sizes() {
    let mut client = open_settings("video");
    assert!(
        has_text(&mut client, "Texture quality"),
        "{:?}",
        client.shown_texts()
    );
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.to_lowercase().contains("texture size")),
        "no size in the label"
    );
    click(&mut client, "texture quality choice");
    for (name, texels) in [
        ("low", 1024),
        ("medium", 2048),
        ("high", 4096),
        ("ultra", 16_384),
    ] {
        click(&mut client, name);
        assert_eq!(settings(&client).quality.texture_size, texels, "{name}");
        click(&mut client, "texture quality choice");
    }
    assert!(
        client
            .shown_texts()
            .iter()
            .all(|text| !text.chars().all(|c| c.is_ascii_digit()) || text.len() < 4),
        "no pixel count in the list: {:?}",
        client.shown_texts()
    );
}

/// The quality presets are High, Medium and Low; there is no Potato.
#[test]
fn the_quality_presets_offer_high_medium_and_low_and_no_potato() {
    let mut client = open_settings("video");
    click(&mut client, "quality preset choice");
    for option in ["high", "medium", "low"] {
        assert!(shown(&mut client, option), "{option} is offered");
    }
    assert!(!shown(&mut client, "potato"), "Potato is gone");
}

fn wheel(client: &mut Client, lines: f32) {
    let window = client
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(client.world())
        .expect("the window");
    client
        .world_mut()
        .write_message(bevy::input::mouse::MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Line,
            x: 0.0,
            y: lines,
            window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    client.frames(3);
}

/// The wheel carries on from the end of one tab into the next, the navigation opening the sections
/// of the tab it has reached: one body, not one per tab.
#[test]
fn the_wheel_scrolls_through_every_tab_and_the_navigation_follows() {
    let mut client = open_settings("hotkeys");
    assert!(
        has_text(&mut client, "Unit actions"),
        "the hotkeys' sections are linked"
    );
    let mut tabs = vec![screen(&client, |s| s.tab)];
    let mut last = 0.0;
    for _ in 0..400 {
        wheel(&mut client, -2.0);
        let at = client.world().resource::<crate::settings::Scroll>().y;
        assert!(
            at >= last,
            "scrolling down only goes down: {last} then {at}"
        );
        last = at;
        let tab = screen(&client, |s| s.tab);
        if tabs.last() != Some(&tab) {
            tabs.push(tab);
            let links = ["Unit actions", "Profile", "Display", "Sound"];
            let linked: Vec<&str> = links
                .into_iter()
                .filter(|link| has_text(&mut client, link))
                .collect();
            let want = match tab {
                crate::settings::Tab::Hotkeys => "Unit actions",
                crate::settings::Tab::Options => "Profile",
                crate::settings::Tab::Video => "Display",
                crate::settings::Tab::Audio => "Sound",
            };
            assert_eq!(linked, [want], "only the sections of {tab:?} are linked");
        }
    }
    assert_eq!(
        tabs,
        [
            crate::settings::Tab::Hotkeys,
            crate::settings::Tab::Options,
            crate::settings::Tab::Video,
            crate::settings::Tab::Audio
        ],
        "every tab was reached in order"
    );
}

/// A tab's button scrolls the one body to that tab, and a wheel turned back leaves it again.
#[test]
fn a_tab_button_scrolls_to_its_tab_and_the_wheel_leaves_it() {
    let mut client = open_settings("hotkeys");
    click(&mut client, "video");
    client.frames(5);
    assert_eq!(screen(&client, |s| s.tab), crate::settings::Tab::Video);
    let video = client.world().resource::<crate::settings::Scroll>().y;
    assert!(
        video > 500.0,
        "the body went down to the video tab: {video}"
    );
    assert!(has_text(&mut client, "Display"), "its sections are linked");
    for _ in 0..40 {
        wheel(&mut client, 3.0);
    }
    assert_eq!(screen(&client, |s| s.tab), crate::settings::Tab::Hotkeys);
    assert_eq!(client.world().resource::<crate::settings::Scroll>().y, 0.0);
}

/// The box of the shown node called `name`, in the window's pixels (where the pointer is put).
fn track_of(client: &mut Client, name: &str) -> Rect {
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
    Rect::from_center_size(at.translation, node.size())
}

/// The mixer: a fader and a mute for each channel, upright, side by side on one line, the master
/// last.
#[test]
fn the_audio_tab_is_a_mixer_with_a_fader_and_a_mute_for_every_channel() {
    let mut client = open_settings("audio");
    let names = [
        "effects",
        "hero voices",
        "announcer",
        "ambience",
        "interface",
        "master volume",
    ];
    let mut faders = Vec::new();
    for name in names {
        faders.push(track_of(&mut client, &format!("{name} slider")));
    }
    for (name, fader) in names.iter().zip(&faders) {
        assert!(
            fader.height() > 3.0 * fader.width(),
            "{name} is upright: {fader:?}"
        );
    }
    for pair in faders.windows(2) {
        assert!(
            (pair[0].center().y - pair[1].center().y).abs() < 1.0,
            "one line"
        );
        assert!(pair[0].center().x < pair[1].center().x, "left to right");
    }
    for (name, fader) in [
        "mute effects",
        "mute hero voices",
        "mute the announcer",
        "mute ambience",
        "mute the interface",
        "mute all sound",
    ]
    .into_iter()
    .zip(&faders)
    {
        let mute = track_of(&mut client, name);
        assert!(mute.min.y > fader.max.y, "{name} stands under its fader");
        assert!(
            (mute.center().x - fader.center().x).abs() < 2.0,
            "{name} under its own: {mute:?} {fader:?}"
        );
    }
    for word in [
        "EFFECTS",
        "HERO VOICES",
        "ANNOUNCER",
        "AMBIENCE",
        "INTERFACE",
        "MASTER",
    ] {
        assert!(has_text(&mut client, word), "{word} names its strip");
    }
}

/// Where along a fader's height a share of it stands: its foot is 0.
fn on_fader(fader: Rect, share: f32) -> Vec2 {
    Vec2::new(fader.center().x, fader.max.y - share * fader.height())
}

fn tried(client: &Client) -> Vec<(crate::channels::Channel, f32)> {
    client
        .world()
        .resource::<crate::channels::Heard>()
        .0
        .clone()
}

/// A fader dragged sets the level of its channel and sounds that channel at each step, so the
/// level is tuned by ear; nothing else changes.
#[test]
fn dragging_a_fader_sets_its_channel_and_sounds_it_while_it_moves() {
    use crate::channels::Channel;
    let mut client = open_settings("audio");
    let fader = track_of(&mut client, "effects slider");
    let before = settings(&client);
    client.mouse(on_fader(fader, 0.3));
    client.button(MouseButton::Left, ButtonState::Pressed);
    for share in [0.4, 0.55, 0.7, 0.8] {
        client.point(on_fader(fader, share));
        client.frames(2);
    }
    client.button(MouseButton::Left, ButtonState::Released);
    client.frames(2);
    let after = settings(&client);
    assert!(
        (after.mix.level(Channel::Effects) - 0.8).abs() < 0.03,
        "the level follows the cursor: {}",
        after.mix.level(Channel::Effects)
    );
    for other in [
        Channel::Voices,
        Channel::Announcer,
        Channel::Ambience,
        Channel::Interface,
    ] {
        assert_eq!(
            after.mix.level(other),
            before.mix.level(other),
            "{other:?} untouched"
        );
    }
    assert_eq!(after.volume, before.volume, "the master is untouched");
    let heard = tried(&client);
    assert!(
        heard.len() >= 3,
        "tried at the steps of the drag: {heard:?}"
    );
    assert!(
        heard
            .iter()
            .all(|(channel, _)| *channel == Channel::Effects)
    );
    let (_, last) = *heard.last().expect("a last try");
    assert!(
        (last - 0.5 * after.mix.level(Channel::Effects)).abs() < 0.01,
        "the last try sounds at the level it ended on: {last}"
    );
    assert!(
        heard.windows(2).all(|pair| pair[0].1 < pair[1].1),
        "each try louder as the fader rises: {heard:?}"
    );
}

/// A muted channel sounds nothing in its try; the others still do.
#[test]
fn a_muted_channel_makes_no_sound_in_its_try_and_the_other_channels_do() {
    use crate::channels::Channel;
    let mut client = open_settings("audio");
    click(&mut client, "mute effects");
    assert!(settings(&client).mix.muted(Channel::Effects));
    let fader = track_of(&mut client, "effects slider");
    client.mouse(on_fader(fader, 0.5));
    client.click(MouseButton::Left);
    client.frames(2);
    assert!(tried(&client).is_empty(), "silent: {:?}", tried(&client));
    let interface = track_of(&mut client, "interface slider");
    client.mouse(on_fader(interface, 0.5));
    client.click(MouseButton::Left);
    client.frames(2);
    let heard = tried(&client);
    assert!(
        !heard.is_empty()
            && heard
                .iter()
                .all(|(channel, _)| *channel == Channel::Interface),
        "{heard:?}"
    );
    click(&mut client, "mute effects");
    assert!(
        !settings(&client).mix.muted(Channel::Effects),
        "unmuted again"
    );
}

/// The master fader and its mute are the settings' own volume and mute-all, and the master is
/// tried through a channel that is not muted.
#[test]
fn the_master_strip_sets_the_master_volume_and_the_mute_all_and_is_tried_aloud() {
    use crate::channels::Channel;
    let mut client = open_settings("audio");
    click(&mut client, "mute effects");
    let fader = track_of(&mut client, "master volume slider");
    client.mouse(on_fader(fader, 0.25));
    client.click(MouseButton::Left);
    client.frames(2);
    assert!((settings(&client).volume - 0.25).abs() < 0.03);
    let heard = tried(&client);
    assert!(
        !heard.is_empty()
            && heard
                .iter()
                .all(|(channel, _)| *channel != Channel::Effects),
        "heard through a channel that sounds: {heard:?}"
    );
    click(&mut client, "mute all sound");
    assert!(settings(&client).muted);
}

/// The keyboard tunes a fader: Tab to it, Up and Down move it a step, each step tried; Space on a
/// mute button turns the mute.
#[test]
fn the_keyboard_tunes_a_fader_and_turns_a_mute() {
    use crate::channels::Channel;
    use bevy::input::keyboard::Key;
    let mut client = open_settings("audio");
    let mut stood = Vec::new();
    for _ in 0..8 {
        client.tap_named(KeyCode::Tab, Key::Tab);
        stood = ringed(&mut client);
        if stood == ["effects slider"] {
            break;
        }
    }
    assert_eq!(stood, ["effects slider"], "Tab reaches the first fader");
    client.tap_named(KeyCode::ArrowDown, Key::ArrowDown);
    client.tap_named(KeyCode::ArrowDown, Key::ArrowDown);
    assert!((settings(&client).mix.level(Channel::Effects) - 0.98).abs() < 0.005);
    assert_eq!(tried(&client).len(), 2, "each step is tried");
    client.tap_named(KeyCode::ArrowUp, Key::ArrowUp);
    assert!((settings(&client).mix.level(Channel::Effects) - 0.99).abs() < 0.005);
    for _ in 0..12 {
        client.tap_named(KeyCode::Tab, Key::Tab);
        if ringed(&mut client) == ["mute effects"] {
            break;
        }
    }
    assert_eq!(ringed(&mut client), ["mute effects"]);
    client.tap_named(KeyCode::Space, Key::Space);
    assert!(settings(&client).mix.muted(Channel::Effects));
}

/// The mixer's reset arrow puts every strip back, the master included.
#[test]
fn the_mixers_reset_arrow_puts_every_strip_back() {
    use crate::channels::Channel;
    let mut client = open_settings("audio");
    click(&mut client, "mute ambience");
    let fader = track_of(&mut client, "interface slider");
    client.mouse(on_fader(fader, 0.2));
    client.click(MouseButton::Left);
    client.frames(3);
    let master = track_of(&mut client, "master volume slider");
    client.mouse(on_fader(master, 0.9));
    client.click(MouseButton::Left);
    client.frames(3);
    click(
        &mut client,
        "reset mixer: volume and mute of every kind of sound",
    );
    let fresh = Settings::default();
    let now = settings(&client);
    assert_eq!(now.mix, fresh.mix);
    assert_eq!(now.volume, fresh.volume);
    assert!(!now.mix.muted(Channel::Ambience));
}

/// A list that would run out of the window below its button opens above it, so that every option
/// can be reached with the pointer.
#[test]
fn a_list_that_would_leave_the_window_opens_above_its_button() {
    let mut client = open_settings("video");
    click(&mut client, "texture quality choice");
    client.frames(3);
    let height = {
        let world = client.world_mut();
        world
            .query_filtered::<&Window, With<PrimaryWindow>>()
            .single(world)
            .expect("the window")
            .physical_height() as f32
    };
    for option in ["low", "medium", "high", "ultra"] {
        let place = track_of(&mut client, option);
        assert!(
            place.min.y >= 0.0 && place.max.y <= height,
            "{option} stands inside the window ({height}): {place:?}"
        );
    }
}

/// The hero's voice has no match to speak in on the menu, so the first try of its fader reads a
/// hero's pack, and the line sounds once the pack is here.
#[test]
fn the_voices_fader_reads_a_hero_pack_in_the_menu_and_then_sounds_a_line() {
    use crate::channels::Channel;
    let mut client = open_settings("audio");
    let fader = track_of(&mut client, "hero voices slider");
    client.mouse(on_fader(fader, 0.6));
    client.click(MouseButton::Left);
    assert!(
        tried(&client).is_empty(),
        "nothing to say before the pack is read"
    );
    client.until("a line of the hero's voice sounded", |c| {
        tried(c)
            .iter()
            .any(|(channel, _)| *channel == Channel::Voices)
    });
    let (_, gain) = tried(&client)[0];
    assert!(
        (gain - 0.5 * settings(&client).mix.level(Channel::Voices)).abs() < 0.01,
        "at the level set: {gain}"
    );
}

/// With the settings' list narrowed by a search (so that no scrolling changes what shows), Tab walks
/// every control that shows (the search and its clear, the tabs, the links, the faders and mutes,
/// the reset arrow of a changed row, the header's close) and wraps; Shift+Tab walks the same.
#[test]
fn tab_and_shift_tab_walk_every_control_of_the_window_and_wrap() {
    use bevy::input::keyboard::Key;
    let mut client = open_settings("audio");
    click(&mut client, "mute ambience");
    click(&mut client, "search settings");
    client.type_text("sound");
    client.tap_named(KeyCode::Enter, Key::Enter);
    client.frames(5);
    let expected = crate::settings::shown_controls(client.world_mut());
    for name in [
        "search settings",
        "clear search",
        "audio",
        "close",
        "mute ambience",
        "master volume slider",
        "reset mixer: volume and mute of every kind of sound",
    ] {
        assert!(expected.contains(name), "{name} shows: {expected:?}");
    }
    for shift in [false, true] {
        let mut reached = std::collections::BTreeSet::new();
        let mut first = None;
        for _ in 0..expected.len() + 3 {
            if shift {
                client.key(KeyCode::ShiftLeft, ButtonState::Pressed);
            }
            client.tap_named(KeyCode::Tab, Key::Tab);
            if shift {
                client.key(KeyCode::ShiftLeft, ButtonState::Released);
            }
            let ringed = ringed(&mut client);
            assert_eq!(ringed.len(), 1, "one control has the ring: {ringed:?}");
            if first.as_ref() == Some(&ringed[0]) {
                break;
            }
            first.get_or_insert_with(|| ringed[0].clone());
            reached.insert(ringed[0].clone());
        }
        assert_eq!(
            reached, expected,
            "shift {shift}: the walk reached every control"
        );
    }
}

/// Space on a control that is a button does what a click on it does: the close cross closes, the
/// reset arrow puts the row back.
#[test]
fn space_presses_the_focused_close_cross_and_reset_arrow() {
    use bevy::input::keyboard::Key;
    let mut client = open_settings("audio");
    click(&mut client, "mute ambience");
    assert!(
        settings(&client)
            .mix
            .muted(crate::channels::Channel::Ambience)
    );
    for _ in 0..400 {
        client.tap_named(KeyCode::Tab, Key::Tab);
        if ringed(&mut client) == ["reset mixer: volume and mute of every kind of sound"] {
            break;
        }
    }
    client.tap_named(KeyCode::Space, Key::Space);
    assert!(
        !settings(&client)
            .mix
            .muted(crate::channels::Channel::Ambience)
    );
    let mut walked = Vec::new();
    for _ in 0..400 {
        client.tap_named(KeyCode::Tab, Key::Tab);
        let now = ringed(&mut client);
        if now == ["close"] {
            break;
        }
        walked.push(now);
    }
    let before = ringed(&mut client);
    client.tap_named(KeyCode::Space, Key::Space);
    assert!(
        !settings_open(&client),
        "the cross closed the window (the ring was on {before:?}; walked {:?})",
        &walked[walked.len().saturating_sub(12)..]
    );
}

/// Scrolling from tab to tab builds again only what follows the tab (the list at the left and the
/// header); the body's controls keep their entities, so a hand on a slider, a hover or the
/// keyboard's focus is not lost at a tab's edge.
#[test]
fn scrolling_into_another_tab_keeps_the_body_s_controls() {
    let mut client = open_settings("hotkeys");
    client.frames(5);
    let slider = {
        let world = client.world_mut();
        world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, name)| name.as_str() == "interface size slider")
            .map(|(entity, _)| entity)
            .expect("a control of the options")
    };
    let mut known: std::collections::HashSet<Entity> = client
        .world_mut()
        .query::<Entity>()
        .iter(client.world())
        .collect();
    let mut created = 0;
    for _ in 0..100 {
        wheel(&mut client, -5.0);
        let now: std::collections::HashSet<Entity> = client
            .world_mut()
            .query::<Entity>()
            .iter(client.world())
            .collect();
        created += now.difference(&known).count();
        known = now;
    }
    assert_eq!(
        screen(&client, |s| s.tab),
        crate::settings::Tab::Audio,
        "the scroll crossed the tabs"
    );
    assert!(
        client.world().get_entity(slider).is_ok(),
        "the body's control is the same entity"
    );
    assert!(
        created < 700,
        "only the list and the header were built again: {created} new entities"
    );
}
