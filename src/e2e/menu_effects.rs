//! Menu feedback is rendered for pointer and focused activation, with simulated time.
use super::harness::Client;
use bevy::{input::keyboard::Key, prelude::*, time::TimeUpdateStrategy};
use std::time::Duration;

type Paint = Vec<Vec4>;
fn control(client: &mut Client, name: &str) -> Option<Entity> {
    let world = client.world_mut();
    world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, n)| n.as_str() == name)
        .map(|(e, _)| e)
}
fn play() -> Client {
    let mut client = Client::run(&["--menu-preview", "play"]);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            10,
        )));
    client.within(Duration::from_secs(30), "Find Match laid out", |c| {
        control(c, "find match")
            .and_then(|e| c.centre_of(e))
            .is_some()
    });
    client.mouse(Vec2::ZERO);
    client.frames(30);
    client
}
fn paint(client: &mut Client, name: &str) -> Paint {
    let entity = control(client, name).expect("control");
    let world = client.world();
    let colors = if let Some(gradient) = world.get::<BackgroundGradient>(entity) {
        let Gradient::Linear(linear) = &gradient.0[0] else {
            panic!("button uses a linear gradient");
        };
        linear
            .stops
            .iter()
            .map(|stop| stop.color)
            .collect::<Vec<_>>()
    } else {
        vec![
            world
                .get::<BackgroundColor>(entity)
                .expect("painted control")
                .0,
        ]
    };
    colors
        .into_iter()
        .map(|color| {
            let color = color.to_linear();
            Vec4::new(color.red, color.green, color.blue, color.alpha)
        })
        .collect()
}
fn difference(left: &Paint, right: &Paint) -> f32 {
    assert_eq!(
        left.len(),
        right.len(),
        "the same paint layers remain present"
    );
    left.iter()
        .zip(right)
        .map(|(a, b)| a.distance_squared(*b))
        .sum::<f32>()
        .sqrt()
}

#[test]
fn menu_button_hover_transitions_then_settles_and_returns_without_changing_geometry() {
    let mut client = play();
    let rest = paint(&mut client, "find match");
    let button = control(&mut client, "find match").expect("button");
    let bounds = client.centre_of(button).expect("laid out");
    client.hover_ui_with(Duration::from_secs(30), |c| control(c, "find match"));
    let early = paint(&mut client, "find match");
    client.frames(30);
    let settled = paint(&mut client, "find match");
    let full_change = difference(&rest, &settled);
    assert!(
        full_change > 0.01,
        "hover visibly highlights the button: {full_change}"
    );
    let early_change = difference(&rest, &early);
    assert!(
        early_change > 0.0001 && early_change < full_change - 0.0001,
        "the highlight transitions between rest and hover: {early_change} of {full_change}"
    );
    let button = control(&mut client, "find match").expect("same control");
    assert_eq!(
        client.centre_of(button).expect("laid out"),
        bounds,
        "feedback does not move the hit target"
    );
    client.mouse(Vec2::ZERO);
    client.frames(30);
    assert_eq!(paint(&mut client, "find match"), rest);
}

#[test]
fn focused_keyboard_activation_has_pressed_feedback_without_a_mouse_button() {
    let mut client = play();
    for _ in 0..64 {
        let target = control(&mut client, "find match");
        if target.is_some()
            && client
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                == target
        {
            break;
        }
        client.tap_named(KeyCode::Tab, Key::Tab);
    }
    let button = control(&mut client, "find match").expect("button");
    assert_eq!(
        client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(button),
        "real keyboard navigation reaches Find Match"
    );
    client.frames(30);
    let idle = paint(&mut client, "find match");
    client.tap_named(KeyCode::Space, Key::Space);
    client.frames(2);
    let pressed = paint(&mut client, "find match");
    assert!(
        difference(&pressed, &idle) > 0.01,
        "focused activation visibly changes the button"
    );
    assert!(pressed.iter().zip(&idle).all(|(pressed,idle)| pressed.truncate().element_sum() < idle.truncate().element_sum()),
        "pressed feedback darkens the button's colour stops");
    assert!(
        !client
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left),
        "keyboard feedback needs no mouse press"
    );
    client.frames(30);
    assert_eq!(paint(&mut client, "find match"), idle);
}

#[test]
fn an_unselected_mode_row_has_visible_hover_feedback() {
    let mut client = play();
    let rest = paint(&mut client, "queue all_pick");
    client.hover_ui_with(Duration::from_secs(30), |c| control(c, "queue all_pick"));
    client.frames(30);
    assert!(
        difference(&paint(&mut client, "queue all_pick"), &rest) > 0.01,
        "rows share a visible hover response"
    );
}

#[test]
fn a_pointer_press_removes_keyboard_decoration_from_menu_buttons() {
    let mut client = play();
    let initial = control(&mut client, "find match").expect("button");
    let rest = *client
        .world()
        .get::<BorderColor>(initial)
        .expect("rest edge");
    for _ in 0..64 {
        if control(&mut client, "find match").is_some_and(|button| {
            client
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                == Some(button)
        }) {
            break;
        }
        client.tap_named(KeyCode::Tab, Key::Tab);
    }
    client.frames(30);
    let button = control(&mut client, "find match").expect("button");
    assert_eq!(
        *client
            .world()
            .get::<BorderColor>(button)
            .expect("keyboard edge"),
        BorderColor::all(crate::theme::FOCUS)
    );
    client.hover_ui_with(Duration::from_secs(30), |client| {
        control(client, "find match")
    });
    client.click(MouseButton::Left);
    client.mouse(Vec2::ZERO);
    client.frames(30);
    assert_eq!(
        client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(button),
        "pointer focus remains usable"
    );
    assert_eq!(
        *client
            .world()
            .get::<BorderColor>(button)
            .expect("pointer edge"),
        rest,
        "pointer input removes keyboard-only decoration"
    );
}

fn interface_fonts_ready(client: &mut Client) -> bool {
    let world = client.world();
    let Some(fonts) = world.get_resource::<crate::ui::Fonts>() else {
        return false;
    };
    let server = world.resource::<AssetServer>();
    fonts
        .all()
        .all(|font| server.is_loaded_with_dependencies(font))
}

fn checkbox_click(client: &mut Client, name: &str) {
    client.within(
        Duration::from_secs(30),
        "interface fonts ready",
        interface_fonts_ready,
    );
    client.hover_ui_with(Duration::from_secs(30), |client| control(client, name));
    client.click(MouseButton::Left);
    client.frames(5);
}

fn checkbox_square(client: &mut Client, name: &str) -> Entity {
    let row = control(client, name).expect("named checkbox row");
    let world = client.world();
    super::harness::subtree(world, row)
        .into_iter()
        .find(|entity| world.get::<crate::ui::checkbox::State>(*entity).is_some())
        .expect("the checkbox owned by this row")
}

// WCAG relative luminance: Bevy's to_linear has already decoded sRGB.
fn contrast_ratio(ink: Color, fill: Color) -> f32 {
    let ink = ink.to_linear();
    let fill = fill.to_linear();
    let luminance = |r, g, b| 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let ground = luminance(fill.red, fill.green, fill.blue);
    let shown = luminance(
        ink.red * ink.alpha + fill.red * (1.0 - ink.alpha),
        ink.green * ink.alpha + fill.green * (1.0 - ink.alpha),
        ink.blue * ink.alpha + fill.blue * (1.0 - ink.alpha),
    );
    (shown.max(ground) + 0.05) / (shown.min(ground) + 0.05)
}

fn checkbox_interior(client: &mut Client, name: &str, checked: bool, enabled: bool) {
    let square = checkbox_square(client, name);
    let world = client.world();
    let outer = world.get::<ComputedNode>(square).expect("box layout");
    let ground = world.get::<BackgroundColor>(square).expect("box fill").0;
    let edge = world.get::<BorderColor>(square).expect("box edge").top;
    let mut strokes = Vec::new();
    let mut insets = Vec::new();
    for entity in super::harness::subtree(world, square)
        .into_iter()
        .filter(|entity| *entity != square)
    {
        let Some(node) = world.get::<ComputedNode>(entity) else {
            continue;
        };
        if node.size().min_element() <= 0.0
            || !world
                .get::<InheritedVisibility>(entity)
                .is_some_and(|seen| seen.get())
        {
            continue;
        }
        let fill = world
            .get::<BackgroundColor>(entity)
            .map_or(Color::NONE, |paint| paint.0);
        let line = world
            .get::<BorderColor>(entity)
            .map_or(Color::NONE, |paint| paint.top);
        let ratio = node.unrounded_size().x / node.unrounded_size().y;
        if ratio > 2.0 && fill.alpha() > 0.01 {
            let axis = bevy::math::Affine2::from(
                *world
                    .get::<UiGlobalTransform>(entity)
                    .expect("drawn stroke"),
            )
            .matrix2
            .x_axis;
            strokes.push((axis.y.atan2(axis.x), fill));
        }
        if (ratio - 1.0).abs() < 0.01
            && node.size().x < outer.size().x * 0.6
            && node.border().min_inset.min_element() > 0.0
            && node.border().max_inset.min_element() > 0.0
            && line.alpha() > 0.01
        {
            println!(
                "{name}: inset physical {:?}, unrounded {:?}",
                node.size(),
                node.unrounded_size()
            );
            insets.push(line);
        }
    }
    if checked {
        assert_eq!(
            strokes.len(),
            2,
            "{name}: checked means two drawn check strokes"
        );
        assert!(
            strokes.iter().any(|(angle, _)| (0.6..0.9).contains(angle)),
            "descending check stroke"
        );
        assert!(
            strokes
                .iter()
                .any(|(angle, _)| (-0.9..-0.6).contains(angle)),
            "ascending check stroke"
        );
        assert!(insets.is_empty(), "checked does not show the empty inset");
    } else {
        assert!(strokes.is_empty(), "empty does not show the check mark");
        assert_eq!(
            insets.len(),
            1,
            "{name}: empty means one drawn inset outline"
        );
    }
    let ink: Vec<_> = strokes
        .into_iter()
        .map(|(_, color)| color)
        .chain(insets)
        .collect();
    // WCAG1.4.11 exempts inactive controls; their original opacity is pinned below.
    if enabled {
        assert_eq!(
            ground.alpha(),
            1.0,
            "enabled mark has a specified opaque background"
        );
        for color in ink {
            let ratio = contrast_ratio(color, ground);
            assert!(ratio >= 3.0, "{name}: mark contrast {ratio}:1 is below 3:1");
        }
    } else {
        let expected = 0.4_f32.powf(2.2);
        for color in [ground, edge].into_iter().chain(ink) {
            assert!(
                (color.alpha() - expected).abs() < 0.00001,
                "{name}: disabled foreground alpha {}, expected {expected}",
                color.alpha()
            );
        }
    }
}

#[test]
fn the_local_lobby_checkbox_has_visible_inner_design_when_empty() {
    let mut client = super::menu::local_menu();
    super::menu::act(
        &mut client,
        crate::main_menu::Act::Go(crate::main_menu::Page::LocalLobby),
    );
    client.frames(4);
    assert!(
        !client
            .world()
            .non_send::<crate::main_menu::MenuCore>()
            .view()
            .host_cheats
    );
    checkbox_interior(&mut client, "enable cheats", false, true);
}

#[test]
fn the_local_lobby_checkbox_has_a_visible_mark_when_checked() {
    let mut client = super::menu::local_menu();
    super::menu::act(
        &mut client,
        crate::main_menu::Act::Go(crate::main_menu::Page::LocalLobby),
    );
    checkbox_click(&mut client, "enable cheats");
    assert!(
        client
            .world()
            .non_send::<crate::main_menu::MenuCore>()
            .view()
            .host_cheats
    );
    checkbox_interior(&mut client, "enable cheats", true, true);
}

#[test]
fn settings_checkbox_shows_the_check_when_checked_and_the_inset_when_empty() {
    let mut client = Client::run(&["--settings", "options"]);
    checkbox_click(&mut client, "search settings");
    client.type_text("save the replays");
    client
        .world_mut()
        .resource_mut::<crate::config::Settings>()
        .replays
        .keep = true;
    client.frames(3);
    checkbox_interior(&mut client, "save the replays of my matches", true, true);
    checkbox_click(&mut client, "save the replays of my matches");
    assert!(
        !client
            .world()
            .resource::<crate::config::Settings>()
            .replays
            .keep
    );
    checkbox_interior(&mut client, "save the replays of my matches", false, true);
}

#[test]
fn disabled_checked_and_empty_boxes_keep_the_faint_look_and_refuse_pointer_and_keyboard() {
    for checked in [false, true] {
        let mut client = Client::run(&["--settings", "audio"]);
        client.frames(2);
        {
            let mut settings = client.world_mut().resource_mut::<crate::config::Settings>();
            settings.muted = false;
            settings.voice = checked;
        }
        client.frames(3);
        checkbox_click(&mut client, "mute all sound");
        assert!(client.world().resource::<crate::config::Settings>().muted);
        checkbox_interior(&mut client, "hero voice responses", checked, false);
        checkbox_click(&mut client, "hero voice responses");
        assert_eq!(
            client.world().resource::<crate::config::Settings>().voice,
            checked,
            "disabled pointer press does not toggle"
        );
        let square = checkbox_square(&mut client, "hero voice responses");
        for _ in 0..128 {
            if client
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                == Some(square)
            {
                break;
            }
            client.tap_named(KeyCode::Tab, Key::Tab);
        }
        assert_eq!(
            client
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get(),
            Some(square),
            "real Tab reaches the disabled control for inspection"
        );
        for (code, key) in [(KeyCode::Space, Key::Space), (KeyCode::Enter, Key::Enter)] {
            client.tap_named(code, key);
            assert_eq!(
                client.world().resource::<crate::config::Settings>().voice,
                checked,
                "disabled keyboard activation does not toggle"
            );
        }
        checkbox_interior(&mut client, "hero voice responses", checked, false);
    }
}

#[test]
fn checkboxes_preserve_their_callers_corner_radii() {
    for (args, name, radius) in [
        (vec![], "enable cheats", 3.0_f32),
        (vec!["--menu-preview", "lobby-edit"], "enable cheats", 0.0),
        (vec!["--settings", "audio"], "hero voice responses", 0.0),
    ] {
        let mut client = Client::run(&args);
        client.frames(3);
        if args.is_empty() {
            super::menu::act(
                &mut client,
                crate::main_menu::Act::Go(crate::main_menu::Page::LocalLobby),
            );
        }
        let square = checkbox_square(&mut client, name);
        let node = client.world().get::<ComputedNode>(square).expect("box");
        let expected = radius / node.inverse_scale_factor();
        let corners = node.border_radius();
        for corner in [
            corners.top_left,
            corners.top_right,
            corners.bottom_left,
            corners.bottom_right,
        ] {
            assert!(
                (corner - Vec2::splat(expected)).abs().max_element()
                    <= f32::EPSILON * expected.max(1.0) * 4.0,
                "{name}: preserve radius {radius}, actual {corner:?}"
            );
        }
    }
}

#[test]
fn checkbox_hover_uses_shared_feedback_without_changing_value_or_geometry() {
    let mut client = super::menu::local_menu();
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            10,
        )));
    super::menu::act(
        &mut client,
        crate::main_menu::Act::Go(crate::main_menu::Page::LocalLobby),
    );
    client.within(
        Duration::from_secs(30),
        "interface fonts ready",
        interface_fonts_ready,
    );
    client.within(Duration::from_secs(30), "the checkbox laid out", |client| {
        control(client, "enable cheats").is_some_and(|row| {
            super::harness::subtree(client.world(), row)
                .into_iter()
                .any(|entity| {
                    client
                        .world()
                        .get::<crate::ui::checkbox::State>(entity)
                        .is_some()
                        && client
                            .world()
                            .get::<ComputedNode>(entity)
                            .is_some_and(|node| node.size().min_element() > 0.0)
                })
        })
    });
    let square = checkbox_square(&mut client, "enable cheats");
    let before = client
        .world()
        .get::<BackgroundColor>(square)
        .expect("box fill")
        .0
        .to_linear();
    let size = client
        .world()
        .get::<ComputedNode>(square)
        .expect("box")
        .unrounded_size();
    client.hover_ui_with(Duration::from_secs(30), |client| {
        control(client, "enable cheats")
    });
    client.frames(30);
    let after = client
        .world()
        .get::<BackgroundColor>(square)
        .expect("hover fill")
        .0
        .to_linear();
    let change = Vec3::new(
        after.red - before.red,
        after.green - before.green,
        after.blue - before.blue,
    )
    .length();
    assert!(
        change > 0.01,
        "hover produces a material visible checkbox highlight: {change}"
    );
    assert_eq!(
        client
            .world()
            .get::<ComputedNode>(square)
            .expect("same box")
            .unrounded_size(),
        size
    );
    assert!(
        !client
            .world()
            .non_send::<crate::main_menu::MenuCore>()
            .view()
            .host_cheats
    );
    client.mouse(Vec2::ZERO);
    client.frames(30);
    assert_eq!(
        client
            .world()
            .get::<BackgroundColor>(square)
            .expect("restored fill")
            .0
            .to_linear(),
        before
    );
}
