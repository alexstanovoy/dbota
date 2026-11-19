//! The hero page's hero: it stands right of the middle with the words unobstructed at its left, and
//! dragging it with the left button or a finger turns it about its vertical axis, with a light
//! inertia that does not depend on the frame rate.

use std::f32::consts::TAU;
use std::time::Duration;

use bevy::input::ButtonState;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::ui_widgets::Button;
use bevy::window::{CursorLeft, PrimaryWindow, WindowEvent, WindowFocused};
use bota_proto::HeroId;

use super::harness::Client;
use super::hero_page::{hero_page, named, rect, texts_under};
use super::menu::{act, resize_menu_window};
use crate::main_menu::hero_stage::{HeroImage, HeroStage};
use crate::main_menu::hero_turn::COAST;
use crate::main_menu::{Act, Page};

const HERO: HeroId = HeroId::SHADOW_FIEND;

/// The hero page with the hero standing on the stage, framed, and time kept by the test: every
/// frame lasts `frame`.
fn standing(frame: Duration) -> Client {
    let mut client = hero_page();
    client.until("the hero standing on the stage", |c| {
        let stage = c.world().resource::<HeroStage>();
        stage.hero == Some(HERO) && stage.ready() && stage.silhouette().is_some()
    });
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(frame));
    client.step_frames(3);
    client
}

fn yaw(client: &Client) -> f32 {
    client.world().resource::<HeroStage>().yaw()
}

/// The angle the model on the stage is turned to.
fn model_yaw(client: &Client) -> f32 {
    let stage = client.world().resource::<HeroStage>();
    let pivot = stage.actor.as_ref().expect("the hero on the stage").pivot;
    client
        .world()
        .get::<Transform>(pivot)
        .expect("the model's pivot")
        .rotation
        .to_euler(EulerRot::YXZ)
        .0
}

/// The angle a drag across `pixels` of the window turns the hero by, as the page defines it: a
/// full turn for a drag across 520 interface pixels.
fn turn_of(client: &Client, pixels: f32) -> f32 {
    pixels / client.world().resource::<UiScale>().0 / 520.0 * TAU
}

/// Where the zone that takes the drag is on screen.
fn zone_centre(client: &mut Client) -> Vec2 {
    let zone = named(client, "hero turn zone").expect("the zone over the hero");
    rect(client, zone).center()
}

fn window(client: &mut Client) -> Entity {
    let world = client.world_mut();
    world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window")
}

/// Presses or lets go of the left button where the pointer is, in the next frame.
fn left_button(client: &mut Client, state: ButtonState) {
    let window = window(client);
    let event = bevy::input::mouse::MouseButtonInput {
        button: MouseButton::Left,
        state,
        window,
    };
    client
        .world_mut()
        .write_message(WindowEvent::MouseButtonInput(event));
    client.world_mut().write_message(event);
}

/// The pointer over `at` for two frames, then the left button pressed for one.
fn take_hold(client: &mut Client, at: Vec2) {
    client.point(at);
    client.step_frames(2);
    left_button(client, ButtonState::Pressed);
    client.step_frames(1);
}

/// A drag to the right at `speed` pixels a second for `seconds`, frame by frame, the button let
/// go in the frame of its last motion; then the frames of `rest` seconds with nothing held.
/// Frames are `frame` long. Returns how far the hero turned over all of it, in radians.
fn fling(
    client: &mut Client,
    from: Vec2,
    speed: f32,
    seconds: f32,
    rest: f32,
    frame: Duration,
) -> f32 {
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(frame));
    let before = yaw(client);
    take_hold(client, from);
    let frames = (seconds / frame.as_secs_f32()).round() as u32;
    for at in 1..=frames {
        client.point(from + Vec2::X * speed * frame.as_secs_f32() * at as f32);
        if at == frames {
            left_button(client, ButtonState::Released);
        }
        client.step_frames(1);
    }
    client.step_frames((rest / frame.as_secs_f32()).round() as usize);
    wrapped(yaw(client) - before)
}

fn wrapped(angle: f32) -> f32 {
    (angle + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0
}

/// Dragging the hero turns it by the distance dragged, the model on the stage turning with it.
#[test]
fn dragging_the_hero_to_the_right_turns_it_by_the_distance_dragged() {
    let mut client = standing(Duration::from_millis(16));
    assert_eq!(yaw(&client), 0.0, "the hero faces as the page found it");
    let from = zone_centre(&mut client);
    take_hold(&mut client, from);
    for step in 1..=10 {
        client.point(from + Vec2::X * 10.0 * step as f32);
        client.step_frames(1);
    }
    let expected = turn_of(&client, 100.0);
    assert!(
        (yaw(&client) - expected).abs() < 1e-3,
        "a drag across 100 pixels turns the hero by {expected}, not {}",
        yaw(&client)
    );
    assert!(
        (model_yaw(&client) - yaw(&client)).abs() < 1e-4,
        "the model on the stage is turned to the hero's angle"
    );
    left_button(&mut client, ButtonState::Released);
    client.step_frames(1);
}

/// A drag to the left turns it the other way.
#[test]
fn dragging_the_hero_to_the_left_turns_it_the_other_way() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    take_hold(&mut client, from);
    for step in 1..=5 {
        client.point(from - Vec2::X * 10.0 * step as f32);
        client.step_frames(1);
    }
    assert!(
        yaw(&client) < -0.1,
        "the hero turned left: {}",
        yaw(&client)
    );
}

/// Let go in motion the hero keeps turning the same way, slowing down, and comes to rest at an
/// angle further on than where it was let go.
#[test]
fn a_hero_let_go_in_motion_keeps_turning_and_comes_to_rest() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    take_hold(&mut client, from);
    // Gripped, then let go at 600 pixels a second.
    let frame = 0.016;
    for at in 1..=30 {
        client.point(from + Vec2::X * 600.0 * frame * at as f32);
        if at == 30 {
            left_button(&mut client, ButtonState::Released);
        }
        client.step_frames(1);
    }
    let let_go_at = yaw(&client);
    client.step_frames(3);
    let after_three = yaw(&client);
    assert!(
        after_three > let_go_at,
        "the hero went on turning: {let_go_at} then {after_three}"
    );
    let mut steps = Vec::new();
    let mut last = after_three;
    for _ in 0..200 {
        client.step_frames(1);
        steps.push(yaw(&client) - last);
        last = yaw(&client);
    }
    assert!(
        steps.windows(2).all(|pair| pair[1] <= pair[0] + 1e-6),
        "it only slows down: {steps:?}"
    );
    assert_eq!(*steps.last().expect("frames"), 0.0, "and it rests");
    let coast = last - let_go_at;
    assert!(
        (0.1..1.2).contains(&coast),
        "a light inertia, a fraction of a turn: {coast} radians"
    );
    client.step_frames(30);
    assert_eq!(yaw(&client), last, "at rest it stays where it was left");
}

/// A pointer held still before it is let go sends the hero nowhere.
#[test]
fn a_hero_let_go_after_a_pause_stays_where_it_was_let_go() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    take_hold(&mut client, from);
    for at in 1..=20 {
        client.point(from + Vec2::X * 10.0 * at as f32);
        client.step_frames(1);
    }
    client.step_frames(30);
    left_button(&mut client, ButtonState::Released);
    client.step_frames(1);
    let let_go_at = yaw(&client);
    client.step_frames(60);
    assert!(
        (yaw(&client) - let_go_at).abs() < 0.02,
        "no fling after a pause: {let_go_at} then {}",
        yaw(&client)
    );
}

/// The same drag at 30, 60 and 144 frames a second ends at the same angle.
#[test]
fn the_same_drag_ends_at_the_same_angle_at_any_frame_rate() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    let turns: Vec<f32> = [30u32, 60, 144]
        .into_iter()
        .map(|fps| {
            fling(
                &mut client,
                from,
                300.0,
                0.4,
                2.0,
                Duration::from_secs_f64(1.0 / f64::from(fps)),
            )
        })
        .collect();
    for pair in turns.windows(2) {
        assert!(
            (pair[0] - pair[1]).abs() < 0.03 * pair[0].abs(),
            "the angles at 30, 60 and 144 frames a second agree within 3%: {turns:?}"
        );
    }
    assert!(turns[0] > 1.0, "a real turn: {turns:?}");
}

/// A long frame in the middle of a drag (a stall) does not throw the hero: the speed it is let
/// go with is what the pointer really did, not the pointer's pixels over the frame's few
/// milliseconds.
#[test]
fn a_stalled_frame_does_not_fling_the_hero() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    take_hold(&mut client, from);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            500,
        )));
    client.point(from + Vec2::X * 50.0);
    left_button(&mut client, ButtonState::Released);
    client.step_frames(1);
    let dragged = turn_of(&client, 50.0);
    client.step_frames(1);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )));
    client.step_frames(300);
    let slow_coast = dragged / 0.5 * COAST;
    assert!(
        yaw(&client) > dragged && yaw(&client) < dragged + slow_coast * 1.1,
        "50 pixels over half a second is a slow drag ({dragged} radians) that coasts as slowly \
         ({slow_coast} at most): {}",
        yaw(&client)
    );
}

/// A touch of finger `id` in the next frame.
fn touch(client: &mut Client, id: u64, phase: TouchPhase, at: Vec2) {
    let window = window(client);
    client
        .world_mut()
        .write_message(WindowEvent::TouchInput(TouchInput {
            phase,
            position: at,
            window,
            force: None,
            id,
        }));
    client.step_frames(1);
}

/// A finger drags the hero as the mouse does.
#[test]
fn a_finger_dragging_across_the_hero_turns_it() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    touch(&mut client, 7, TouchPhase::Started, from);
    for step in 1..=10 {
        touch(
            &mut client,
            7,
            TouchPhase::Moved,
            from + Vec2::X * 10.0 * step as f32,
        );
    }
    let expected = turn_of(&client, 100.0);
    assert!(
        (yaw(&client) - expected).abs() < 1e-3,
        "a drag of a finger across 100 pixels turns the hero by {expected}, not {}",
        yaw(&client)
    );
    touch(&mut client, 7, TouchPhase::Ended, from + Vec2::X * 100.0);
    client.step_frames(300);
    assert!(yaw(&client) > expected, "the hero coasts after the finger");
}

/// A second finger on the hero pulls neither against the first nor takes it from it: while the
/// first holds the hero the second's motion turns nothing, and its lifting lets nothing go.
#[test]
fn a_second_finger_on_the_hero_neither_turns_it_nor_takes_it_from_the_first() {
    let mut client = standing(Duration::from_millis(16));
    let zone = named(&mut client, "hero turn zone").expect("the zone over the hero");
    let bounds = rect(&client, zone);
    let from = bounds.center();
    let beside = from + Vec2::new(0.0, 60.0);
    assert!(
        bounds.contains(beside),
        "both fingers start inside the turn zone"
    );
    touch(&mut client, 7, TouchPhase::Started, from);
    touch(&mut client, 8, TouchPhase::Started, beside);
    for step in 1..=10 {
        touch(
            &mut client,
            8,
            TouchPhase::Moved,
            beside + Vec2::X * 10.0 * step as f32,
        );
    }
    assert_eq!(
        yaw(&client),
        0.0,
        "the second finger's motion turns nothing"
    );
    touch(&mut client, 8, TouchPhase::Ended, beside + Vec2::X * 100.0);
    for step in 1..=10 {
        touch(
            &mut client,
            7,
            TouchPhase::Moved,
            from + Vec2::X * 10.0 * step as f32,
        );
    }
    let expected = turn_of(&client, 100.0);
    assert!(
        (yaw(&client) - expected).abs() < 1e-3,
        "the first finger still holds the hero after the second is lifted: {expected}, not {}",
        yaw(&client)
    );
}

/// The right button does not turn the hero, and a drag that starts on a control (any button of
/// the page) neither turns it nor is taken from the control.
#[test]
fn only_the_left_button_on_the_hero_turns_it() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    client.point(from);
    client.step_frames(2);
    let window = window(&mut client);
    let right = bevy::input::mouse::MouseButtonInput {
        button: MouseButton::Right,
        state: ButtonState::Pressed,
        window,
    };
    client
        .world_mut()
        .write_message(WindowEvent::MouseButtonInput(right));
    client.world_mut().write_message(right);
    client.step_frames(1);
    for step in 1..=10 {
        client.point(from + Vec2::X * 10.0 * step as f32);
        client.step_frames(1);
    }
    assert_eq!(yaw(&client), 0.0, "the right button does not turn the hero");
}

/// Whatever a press lands on, if it is a control, the hero does not take the drag that follows.
/// Every control of the page is tried, in the order of their names; the press of one that takes
/// the player off the page is followed by the way back, and the one that starts a demo (and so
/// leaves the menu for a match) is left out.
#[test]
fn a_drag_that_starts_on_a_control_does_not_turn_the_hero() {
    let mut client = standing(Duration::from_millis(16));
    let mut controls: Vec<(String, Vec2)> = {
        let world = client.world_mut();
        world
            .query_filtered::<(Entity, &Name, &InheritedVisibility), With<Button>>()
            .iter(world)
            .filter(|(.., seen)| seen.get())
            .map(|(entity, name, _)| (entity, name.to_string()))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(entity, name)| (name, rect(&client, entity).center()))
            .collect()
    };
    controls.retain(|(name, _)| !name.contains("demo"));
    controls.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(
        controls.len() >= 3,
        "the page has controls to try: {controls:?}"
    );
    for (name, at) in controls {
        take_hold(&mut client, at);
        for step in 1..=5 {
            client.point(at + Vec2::new(8.0 * step as f32, 0.0));
            client.step_frames(1);
        }
        left_button(&mut client, ButtonState::Released);
        client.step_frames(2);
        assert_eq!(
            yaw(&client),
            0.0,
            "a drag from the {name} control turned the hero"
        );
        if named(&mut client, "hero turn zone").is_none() {
            act(&mut client, Act::InspectHero(HERO));
            client.until("the hero's page again", |c| {
                named(c, "hero turn zone").is_some()
            });
            client.until("the hero standing on the stage again", |c| {
                let stage = c.world().resource::<HeroStage>();
                stage.hero == Some(HERO) && stage.ready()
            });
        }
    }
}

/// An open hand over the hero, a closed one while it is held, the arrow again after.
#[test]
fn the_cursor_is_an_open_hand_over_the_hero_and_a_closed_one_while_it_turns() {
    let mut client = standing(Duration::from_millis(16));
    client.point(Vec2::new(5.0, 500.0));
    client.step_frames(3);
    assert_eq!(worn(&client), Some("cursor.default"), "off the hero");
    let from = zone_centre(&mut client);
    client.point(from);
    client.step_frames(3);
    assert_eq!(worn(&client), Some("cursor.grab"), "over the hero");
    left_button(&mut client, ButtonState::Pressed);
    client.step_frames(1);
    client.point(from + Vec2::X * 400.0);
    client.step_frames(2);
    assert_eq!(
        worn(&client),
        Some("cursor.grabbing"),
        "held, even where the pointer has left the zone"
    );
    left_button(&mut client, ButtonState::Released);
    client.step_frames(3);
    assert_eq!(worn(&client), Some("cursor.default"), "let go off the hero");
}

/// A hero held and let go of at once by the next press in the same frame stays held: the release
/// and the press both count.
#[test]
fn a_release_and_a_press_in_one_frame_leave_the_hero_held_by_the_press() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    take_hold(&mut client, from);
    left_button(&mut client, ButtonState::Released);
    left_button(&mut client, ButtonState::Pressed);
    client.step_frames(1);
    let before = yaw(&client);
    for step in 1..=10 {
        client.point(from + Vec2::X * 10.0 * step as f32);
        client.step_frames(1);
    }
    let expected = turn_of(&client, 100.0);
    assert!(
        (yaw(&client) - before - expected).abs() < 1e-3,
        "the new press holds the hero: it turns {expected} for 100 pixels, not {}",
        yaw(&client) - before
    );
    left_button(&mut client, ButtonState::Released);
    client.step_frames(2);
    assert_eq!(
        worn(&client),
        Some("cursor.grab"),
        "and its release lets go"
    );
}

#[test]
fn a_press_and_release_in_one_frame_leave_the_hero_unheld() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    client.point(from);
    client.step_frames(2);
    left_button(&mut client, ButtonState::Pressed);
    left_button(&mut client, ButtonState::Released);
    client.step_frames(1);
    assert_eq!(
        worn(&client),
        Some("cursor.grab"),
        "the release restores the open hand"
    );
    client.point(from + Vec2::X * 50.0);
    client.step_frames(2);
    assert_eq!(
        yaw(&client),
        0.0,
        "moving without a hold does not turn the hero"
    );
}

fn worn(client: &Client) -> Option<&'static str> {
    client.world().resource::<crate::cursor::Cursors>().worn()
}

/// What happens to the window while a button is held on the hero and let go where the client is
/// told nothing (outside the window or the page's canvas).
enum Loss {
    PointerLeftTheWindow,
    WindowLostFocus,
}

fn lose(client: &mut Client, how: &Loss) {
    let window = window(client);
    match how {
        Loss::PointerLeftTheWindow => {
            client
                .world_mut()
                .write_message(WindowEvent::CursorLeft(CursorLeft { window }));
            client.world_mut().write_message(CursorLeft { window });
        }
        Loss::WindowLostFocus => {
            let event = WindowFocused {
                window,
                focused: false,
            };
            client
                .world_mut()
                .write_message(WindowEvent::WindowFocused(event.clone()));
            client.world_mut().write_message(event);
        }
    }
    client.step_frames(2);
}

/// Holds the hero still, loses the pointer as `how`, and moves the pointer over the zone again
/// with the button up in the client's view: the hero must not follow, and no closed hand stays.
fn the_hold_ends_when_the_pointer_is_lost(how: Loss) {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    take_hold(&mut client, from);
    client.step_frames(30);
    assert_eq!(worn(&client), Some("cursor.grabbing"), "held");
    lose(&mut client, &how);
    assert_ne!(
        worn(&client),
        Some("cursor.grabbing"),
        "the closed hand stays on though the pointer is gone"
    );
    let at_rest = yaw(&client);
    for step in 1..=10 {
        client.point(from + Vec2::X * 20.0 * step as f32);
        client.step_frames(1);
    }
    assert_eq!(
        yaw(&client),
        at_rest,
        "the hero follows a pointer whose button was let go outside"
    );
}

#[test]
fn a_hero_held_by_a_pointer_that_leaves_the_window_is_let_go() {
    the_hold_ends_when_the_pointer_is_lost(Loss::PointerLeftTheWindow);
}

#[test]
fn a_hero_held_in_a_window_that_loses_focus_is_let_go() {
    the_hold_ends_when_the_pointer_is_lost(Loss::WindowLostFocus);
}

/// A page left under a drag takes the hold and the closed hand with it, though nothing of the
/// turntable runs once the hero page is gone.
#[test]
fn leaving_the_hero_page_under_a_drag_lets_go_of_the_hold_and_the_hand() {
    let mut client = standing(Duration::from_millis(16));
    let from = zone_centre(&mut client);
    take_hold(&mut client, from);
    assert_eq!(worn(&client), Some("cursor.grabbing"), "held");
    act(&mut client, Act::Go(Page::Heroes));
    client.step_frames(3);
    client.point(Vec2::new(5.0, 500.0));
    client.step_frames(3);
    assert_eq!(
        worn(&client),
        Some("cursor.default"),
        "no hand off the hero page"
    );
}

/// At any window the hero stands in the right half of the page, clear of the words at its left
/// (the text column's widest line) and clear of every control, and its drag zone covers no
/// control either.
#[test]
fn the_hero_stands_right_of_the_middle_clear_of_the_words_at_every_window_size() {
    let mut client = standing(Duration::from_millis(16));
    for size in [
        UVec2::new(1280, 720),
        UVec2::new(1920, 1080),
        UVec2::new(900, 1000),
        UVec2::new(3440, 1440),
    ] {
        resize_menu_window(&mut client, size);
        client.step_frames(5);
        let image = {
            let world = client.world_mut();
            world
                .query_filtered::<Entity, With<HeroImage>>()
                .single(world)
                .expect("the hero's picture")
        };
        let picture = rect(&client, image);
        let silhouette = client
            .world()
            .resource::<HeroStage>()
            .silhouette()
            .expect("the hero framed");
        let scale = picture.size() / HeroStage::PICTURE;
        let model = Rect::from_corners(
            picture.min + silhouette.min * scale,
            picture.min + silhouette.max * scale,
        );
        let words = named(&mut client, "hero words").expect("the page's words");
        let words_right = texts_under(client.world(), words)
            .into_iter()
            .map(|text| rect(&client, text).max.x)
            .fold(f32::MIN, f32::max);
        let width = size.x as f32;
        assert!(
            model.min.x >= width / 2.0,
            "{size}: the hero stands in the right half, from {} on of {width}",
            model.min.x
        );
        assert!(
            model.max.x <= width,
            "{size}: the hero stays in the window, to {} of {width}",
            model.max.x
        );
        assert!(
            model.min.x > words_right,
            "{size}: the hero (from {}) clears the words (to {words_right})",
            model.min.x
        );
        let zone_node = named(&mut client, "hero turn zone").expect("the zone");
        let zone = rect(&client, zone_node);
        let buttons: Vec<(String, Rect)> = {
            let world = client.world_mut();
            world
                .query_filtered::<(Entity, &Name, &InheritedVisibility), With<Button>>()
                .iter(world)
                .filter(|(.., seen)| seen.get())
                .map(|(entity, name, _)| (entity, name.to_string()))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|(entity, name)| (name, rect(&client, entity)))
                .collect()
        };
        for (name, button) in buttons {
            assert!(
                zone.intersect(button).is_empty(),
                "{size}: the drag zone {zone:?} covers the {name} control {button:?}"
            );
        }
    }
}
