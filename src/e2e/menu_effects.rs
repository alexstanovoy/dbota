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
