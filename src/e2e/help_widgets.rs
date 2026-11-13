//! Context help responds to hover and uses the actual rasterized glyph bounds.
use super::harness::Client;
use bevy::prelude::*;
use bevy::text::TextLayoutInfo;
use std::time::Duration;

fn button(client: &mut Client, name: &str) -> Option<Entity> {
    let world = client.world_mut();
    world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, label)| label.as_str() == name)
        .map(|(entity, _)| entity)
}

#[test]
fn context_help_appears_on_hover_highlights_the_mark_and_never_pins_after_a_click() {
    let mut client = Client::run(&["--menu-preview", "play"]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(10),
        ));
    let (name, expected) = {
        let view = &client.world().resource::<crate::main_menu::Preview>().0;
        let crate::main_menu::PageView::Home { rulesets, .. } = &view.body else {
            panic!("Play")
        };
        let row = rulesets.iter().find(|row| row.rated).expect("ranked queue");
        (
            format!("about {}", row.name.to_lowercase()),
            row.description.clone(),
        )
    };
    fonts_ready(&mut client);
    let control = client.hover_ui_with(Duration::from_secs(30), |client| button(client, &name));
    client.frames(3);
    assert!(
        client.shown_texts().iter().any(|text| text == &expected),
        "hover opens the contextual explanation"
    );
    let lit = question_colour(&mut client, control);
    client.click(MouseButton::Left);
    client.mouse(Vec2::ZERO);
    client
        .world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    client.frames(30);
    assert!(
        !client.shown_texts().iter().any(|text| text == &expected),
        "click does not pin help after the pointer leaves"
    );
    let rest = question_colour(&mut client, control);
    assert!(
        lit.distance(rest) > 0.005,
        "hover visibly highlights the question mark"
    );
}

#[test]
fn keyboard_navigation_reaches_and_reads_context_help_without_pinning_it() {
    let mut client = Client::run(&["--menu-preview", "play"]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(10),
        ));
    fonts_ready(&mut client);
    let (name, expected) = {
        let view = &client.world().resource::<crate::main_menu::Preview>().0;
        let crate::main_menu::PageView::Home { rulesets, .. } = &view.body else {
            panic!("Play")
        };
        let row = rulesets.iter().find(|row| row.rated).expect("ranked queue");
        (
            format!("about {}", row.name.to_lowercase()),
            row.description.clone(),
        )
    };
    client.mouse(Vec2::ZERO);
    client
        .world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    tab_to(&mut client, &name);
    client.frames(30);
    assert!(
        client.shown_texts().iter().any(|line| line == &expected),
        "keyboard focus exposes the same explanation as hovering"
    );
    client.tap_named(KeyCode::Tab, bevy::input::keyboard::Key::Tab);
    client.frames(30);
    assert!(
        !client.shown_texts().iter().any(|line| line == &expected),
        "leaving the focused hint closes it"
    );
}

#[test]
fn settings_context_help_can_be_read_with_the_keyboard() {
    let mut client = Client::run(&["--settings", "hotkeys"]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(10),
        ));
    fonts_ready(&mut client);
    client.mouse(Vec2::ZERO);
    // The first Tab stands in the body, where the keys are; the help is in the header, which is
    // the other way round from there (and gone once the walk has moved into another tab).
    shift_tab_to(&mut client, "hotkey help");
    client.frames(30);
    assert!(
        client.shown_texts().iter().any(|line| line
            == "Click a key, then press the new one. Backspace unbinds, Escape keeps it.")
    );
}

fn shift_tab_to(client: &mut Client, name: &str) {
    for _ in 0..64 {
        if button(client, name).is_some_and(|control| {
            client
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                == Some(control)
        }) {
            return;
        }
        client.key(KeyCode::ShiftLeft, bevy::input::ButtonState::Pressed);
        client.tap_named(KeyCode::Tab, bevy::input::keyboard::Key::Tab);
        client.key(KeyCode::ShiftLeft, bevy::input::ButtonState::Released);
    }
    panic!("context help {name} must be reachable through real Shift+Tab navigation");
}

fn tab_to(client: &mut Client, name: &str) {
    for _ in 0..64 {
        if button(client, name).is_some_and(|control| {
            client
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                == Some(control)
        }) {
            return;
        }
        client.tap_named(KeyCode::Tab, bevy::input::keyboard::Key::Tab);
    }
    panic!("context help {name} must be reachable through real Tab navigation");
}

#[test]
fn a_pointer_press_clears_keyboard_help_and_its_focus_ring_after_leaving() {
    let mut client = Client::run(&["--menu-preview", "play"]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(10),
        ));
    fonts_ready(&mut client);
    tab_to(&mut client, "about solo mid");
    client.frames(30);
    let control = button(&mut client, "about solo mid").expect("help");
    assert_ne!(
        client
            .world()
            .get::<Outline>(control)
            .expect("focus ring")
            .color,
        Color::NONE
    );
    client.hover_ui_with(Duration::from_secs(30), |client| {
        button(client, "about solo mid")
    });
    client.click(MouseButton::Left);
    client.mouse(Vec2::ZERO);
    client.frames(30);
    assert_eq!(
        client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(control),
        "the pointer keeps logical focus"
    );
    assert_eq!(
        client
            .world()
            .get::<Outline>(control)
            .expect("focus ring")
            .color,
        Color::NONE,
        "a pointer press removes keyboard-only focus decoration"
    );
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|line| line.contains("The middle lane. 2 kills")),
        "a pointer click cannot pin keyboard help"
    );
}

#[test]
fn gamepad_navigation_exposes_shared_context_help() {
    use bevy::input::gamepad::{
        GamepadButton, GamepadSettings, RawGamepadButtonChangedEvent, RawGamepadEvent,
    };
    let mut client = Client::run(&["--menu-preview", "play"]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(10),
        ));
    fonts_ready(&mut client);
    let pad = client
        .world_mut()
        .spawn((Gamepad::default(), GamepadSettings::default()))
        .id();
    client.mouse(Vec2::ZERO);
    for _ in 0..64 {
        if button(&mut client, "about solo mid").is_some_and(|control| {
            client
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                == Some(control)
        }) {
            break;
        }
        for value in [1.0, 0.0] {
            client.world_mut().write_message(RawGamepadEvent::Button(
                RawGamepadButtonChangedEvent::new(pad, GamepadButton::DPadDown, value),
            ));
            client.frames(2);
        }
    }
    let control = button(&mut client, "about solo mid").expect("help");
    assert_eq!(
        client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(control)
    );
    client.frames(30);
    assert_ne!(
        client
            .world()
            .get::<Outline>(control)
            .expect("focus ring")
            .color,
        Color::NONE
    );
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|line| line.contains("The middle lane. 2 kills"))
    );
}

#[test]
fn tooltip_previews_produce_real_hover_states_that_the_pointer_can_leave() {
    for (scene, name) in [
        ("play-tooltip", "about solo mid"),
        ("match-decline", "about declining"),
    ] {
        let mut client = Client::run(&["--menu-preview", scene]);
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(10),
            ));
        fonts_ready(&mut client);
        client.within(
            Duration::from_secs(30),
            "preview pointer on help",
            |client| {
                button(client, name).is_some_and(|entity| {
                    client
                        .world()
                        .get::<bevy::picking::hover::Hovered>(entity)
                        .is_some_and(|hover| hover.get())
                })
            },
        );
        client.frames(30);
        assert!(
            help_is_visible(&mut client),
            "{scene} must actually show help"
        );
        client.mouse(Vec2::ZERO);
        client.frames(30);
        assert!(
            !help_is_visible(&mut client),
            "{scene} is a real hover, not forced visibility"
        );
    }
}

fn help_is_visible(client: &mut Client) -> bool {
    let world = client.world_mut();
    world
        .query::<(&Name, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .any(|(name, node, seen)| {
            name.as_str() == "context help" && seen.get() && node.size().min_element() > 0.0
        })
}

#[test]
fn settings_tab_navigation_reaches_every_existing_content_control_over_play() {
    use std::collections::HashSet;
    for tab in ["hotkeys", "options", "video", "audio"] {
        let mut client = Client::run(&["--menu-preview", "play", "--settings", tab]);
        fonts_ready(&mut client);
        client.frames(5);
        let world = client.world_mut();
        let window = world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, name)| name.as_str() == "settings window")
            .map(|(entity, _)| entity)
            .expect("settings window");
        // The content controls are those of the body. They are known by their names: the list and
        // the header, which follow the tab in view, are built again when the walk moves into
        // another tab (the settings' own tests walk all of them).
        let name_of = |world: &World, entity: Entity| -> String {
            world
                .get::<Name>(entity)
                .expect("a named settings control")
                .to_string()
        };
        let body = world
            .query_filtered::<Entity, With<crate::settings::Body>>()
            .single(world)
            .expect("the settings' body");
        let expected: HashSet<String> = super::harness::subtree(world, body)
            .into_iter()
            .filter(|entity| {
                world.get::<Outline>(*entity).is_some()
                    && world
                        .get::<ComputedNode>(*entity)
                        .is_some_and(|node| node.size().min_element() > 0.0)
                    && world
                        .get::<InheritedVisibility>(*entity)
                        .is_some_and(|seen| seen.get())
            })
            .map(|entity| name_of(world, entity))
            .collect();
        assert!(
            !expected.is_empty() && expected.len() <= 512,
            "bounded real settings controls"
        );
        let mut reached = HashSet::new();
        // The links under a tab in view join the walk as the walk moves into the tab.
        for _ in 0..=2 * expected.len() {
            client.tap_named(KeyCode::Tab, bevy::input::keyboard::Key::Tab);
            let entity = client
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                .expect("settings keyboard focus");
            let name = name_of(client.world(), entity);
            let in_window = std::iter::successors(Some(entity), |entity| {
                client.world().get::<ChildOf>(*entity).map(ChildOf::parent)
            })
            .take(64)
            .any(|ancestor| ancestor == window);
            assert!(
                in_window,
                "{tab}: underlying Play cannot take Settings focus ({name})"
            );
            reached.insert(name);
        }
        assert!(
            expected.is_subset(&reached),
            "{tab}: every existing outlined content control remains Tab-reachable, not {:?}",
            expected.difference(&reached).collect::<Vec<_>>()
        );
    }
}

fn question_colour(client: &mut Client, control: Entity) -> Vec4 {
    let world = client.world_mut();
    super::harness::subtree(world, control)
        .into_iter()
        .find_map(|entity| {
            (world.get::<Text>(entity)?.0 == "?")
                .then(|| world.get::<TextColor>(entity).expect("ink").0)
        })
        .map(|color| {
            let color = color.to_linear();
            Vec4::new(color.red, color.green, color.blue, color.alpha)
        })
        .expect("question mark")
}

#[test]
fn the_help_question_marks_rasterized_ink_is_centered_within_one_pixel() {
    for args in [
        vec!["--menu-preview", "play"],
        vec!["--settings", "hotkeys"],
    ] {
        let mut client = Client::run(&args);
        fonts_ready(&mut client);
        client.within(Duration::from_secs(30), "question mark rasterized", |c| {
            question_ink(c).is_some()
        });
        client.frames(3);
        let (ink, centre) = question_ink(&mut client).expect("rasterized help glyph");
        let error = (ink.center() - centre).abs();
        assert!(
            error.max_element() <= 1.0,
            "the glyph's actual ink, not its line box, is centred: ink {ink:?}, circle {centre:?}, error {error:?}"
        );
    }
}

fn question_ink(client: &mut Client) -> Option<(Rect, Vec2)> {
    let world = client.world_mut();
    let (glyph, node, transform, parent) = world
        .query::<(
            &Text,
            &TextLayoutInfo,
            &ComputedNode,
            &UiGlobalTransform,
            &ChildOf,
            &InheritedVisibility,
        )>()
        .iter(world)
        .find(|(text, layout, _, _, _, visible)| {
            text.0 == "?" && visible.get() && layout.glyphs.len() == 1
        })
        .map(|(_, layout, node, transform, parent, _)| {
            (layout.glyphs[0].clone(), *node, *transform, parent.parent())
        })?;
    let image = world
        .resource::<Assets<Image>>()
        .get(glyph.atlas_info.texture)?;
    let raster = glyph.atlas_info.rect;
    assert!(
        raster.size().max_element() <= 128.0,
        "bounded help glyph raster"
    );
    let mut low = Vec2::splat(f32::INFINITY);
    let mut high = Vec2::splat(f32::NEG_INFINITY);
    let matrix = bevy::math::Affine2::from(transform);
    for y in raster.min.y as u32..raster.max.y as u32 {
        for x in raster.min.x as u32..raster.max.x as u32 {
            if image.get_color_at(x, y).ok()?.alpha() < 0.5 {
                continue;
            }
            let local = Vec2::new(x as f32, y as f32) - raster.center()
                + glyph.position
                + node.content_box().min;
            low = low.min(matrix.transform_point2(local));
            high = high.max(matrix.transform_point2(local + Vec2::ONE));
        }
    }
    assert!(
        low.is_finite() && high.is_finite(),
        "the help mark has visible raster ink"
    );
    Some((
        Rect::from_corners(low, high),
        world.get::<UiGlobalTransform>(parent)?.translation,
    ))
}

fn fonts_ready(client: &mut Client) {
    client.within(Duration::from_secs(30), "all interface fonts loaded", |c| {
        let world = c.world();
        let Some(fonts) = world.get_resource::<crate::ui::Fonts>() else {
            return false;
        };
        let Some(server) = world.get_resource::<AssetServer>() else {
            return false;
        };
        fonts
            .all()
            .all(|font| server.is_loaded_with_dependencies(font))
    });
}

#[test]
fn every_help_anchor_in_play_lobbies_and_settings_keeps_its_own_row_uncovered() {
    let mut checked = 0;
    for scene in [
        "play",
        "play-bots",
        "play-full",
        "play-cooldown",
        "match-found",
        "lobbies",
        "lobby-password",
        "lobby-owner",
        "lobby-edit",
        "lobby-start",
    ] {
        let mut client = Client::run(&["--menu-preview", scene]);
        checked += check_all_help_rows(&mut client, scene);
    }
    for tab in ["hotkeys", "options", "video", "audio"] {
        let mut client = Client::run(&["--settings", tab]);
        checked += check_all_help_rows(&mut client, tab);
    }
    assert!(checked >= 5, "the audit must exercise actual help anchors");
}

fn help_controls(client: &mut Client) -> Vec<String> {
    let world = client.world_mut();
    world
        .query::<(&Name, &ChildOf)>()
        .iter(world)
        .filter(|(name, _)| name.as_str() == "context help")
        .map(|(_, parent)| {
            world
                .get::<Name>(parent.parent())
                .expect("named help anchor")
                .to_string()
        })
        .collect()
}

fn check_all_help_rows(client: &mut Client, scene: &str) -> usize {
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(10),
        ));
    fonts_ready(client);
    client.frames(4);
    let names = help_controls(client);
    println!("help anchor audit {scene}: {}", names.len());
    for name in &names {
        check_help_row(client, name);
    }
    names.len()
}

fn check_help_row(client: &mut Client, name: &str) {
    client.mouse(Vec2::ZERO);
    client
        .world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    client.frames(20);
    let control = button(client, name).expect("help control");
    let row = if client
        .world()
        .get::<crate::ui::help::Hint>(control)
        .is_some()
    {
        client
            .world()
            .get::<ChildOf>(control)
            .expect("anchor row")
            .parent()
    } else {
        control
    };
    let tip = super::harness::subtree(client.world(), control)
        .into_iter()
        .find(|entity| {
            client
                .world()
                .get::<Name>(*entity)
                .is_some_and(|name| name.as_str() == "context help")
        })
        .expect("help tooltip");
    client.point(client.centre_of(control).expect("laid-out anchor"));
    let mut visible = 0;
    for _ in 0..30 {
        client.frames(1);
        if !client
            .world()
            .get::<InheritedVisibility>(tip)
            .is_some_and(|seen| seen.get())
        {
            continue;
        }
        let Some(tip_rect) = drawn_rect(client.world(), tip) else {
            continue;
        };
        let row_rect = drawn_rect(client.world(), row).expect("drawn anchor row");
        let separation = (row_rect.min - tip_rect.max)
            .max(tip_rect.min - row_rect.max)
            .max_element();
        assert!(
            separation >= 4.0,
            "{name}: tooltip {tip_rect:?} must have a gap from its entire row {row_rect:?}, including its label"
        );
        let world = client.world_mut();
        let window = world
            .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
            .single(world)
            .expect("window");
        let extent = UVec2::new(window.physical_width(), window.physical_height()).as_vec2();
        assert!(
            tip_rect.min.cmpge(Vec2::ZERO).all() && tip_rect.max.cmple(extent).all(),
            "{name}: flipped help stays inside the viewport"
        );
        visible += 1;
    }
    assert!(
        visible > 0,
        "{name}: hiding help permanently does not satisfy placement"
    );
}

fn drawn_rect(world: &World, entity: Entity) -> Option<Rect> {
    let node = world.get::<ComputedNode>(entity)?;
    if node.size().min_element() <= 0.0 {
        return None;
    }
    Some(Rect::from_center_size(
        world.get::<UiGlobalTransform>(entity)?.translation,
        node.size(),
    ))
}

#[test]
fn help_flips_to_the_available_side_when_its_row_is_at_the_screen_edge() {
    let mut client = Client::run(&["--menu-preview", "play"]);
    fonts_ready(&mut client);
    client.frames(4);
    let control = button(&mut client, "about solo mid").expect("mode help");
    let row = client
        .world()
        .get::<ChildOf>(control)
        .expect("row")
        .parent();
    let world = client.world_mut();
    let mut page = row;
    for _ in 0..64 {
        let Some(parent) = world.get::<ChildOf>(page) else {
            break;
        };
        page = parent.parent();
    }
    assert!(world.get::<ChildOf>(page).is_none(), "bounded UI ancestry");
    world.entity_mut(row).insert((
        ChildOf(page),
        Node {
            position_type: PositionType::Absolute,
            left: px(4),
            top: px(140),
            width: px(320),
            height: px(60),
            align_items: AlignItems::Center,
            ..default()
        },
    ));
    client.frames(4);
    check_help_row(&mut client, "about solo mid");
}
