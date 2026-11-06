//! The spinning arrows Dota's courier block shows while the courier is on its way (the
//! `#Spinner` of `dota_hud_courier`: visible while the courier moves, turning once in 1.5 s
//! against the clock, still while it stands).

use bevy::prelude::*;
use bota_proto::{Cheat, HeroId, Order, Target, Vec2 as Spot};

use super::harness::{Client, MID, Server};
use crate::courier::CourierSpinner;

/// Dota's `SpinnerRotate` period.
const TURN_SECONDS: f32 = 1.5;

fn spinner(client: &mut Client) -> (Visibility, f32) {
    let world = client.world_mut();
    let (shown, at) = world
        .query_filtered::<(&InheritedVisibility, &bevy::ui::UiTransform), With<CourierSpinner>>()
        .single(world)
        .expect("the courier block's spinner");
    let visible = if shown.get() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    (visible, at.rotation.as_radians())
}

#[test]
fn the_courier_spinner_shows_and_turns_while_the_courier_moves_and_rests_when_it_stands() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    let courier = client.core().my_courier().expect("the owner's courier");
    client.until("the courier standing", |c| {
        spinner(c).0 == Visibility::Hidden
    });
    let at = client.view().unit(courier).expect("the courier").pos;
    client.core_mut().send_order_to(
        Some(courier),
        Order::Move {
            target: Target::Pos(Spot::from_ints(at.x.to_int() + 1500, at.y.to_int())),
        },
    );
    client.until("the spinner shown", |c| {
        spinner(c).0 == Visibility::Inherited
    });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(250),
        ));
    let before = spinner(&mut client).1;
    client.frames(1);
    let after = spinner(&mut client).1;
    let step = (after - before).rem_euclid(std::f32::consts::TAU);
    let expected =
        (-(0.25 / TURN_SECONDS) * std::f32::consts::TAU).rem_euclid(std::f32::consts::TAU);
    assert!(
        (step - expected).abs() < 0.05,
        "a quarter second turns it {step} rad, Dota's reverse turn is {expected}"
    );
    client.core_mut().send_order_to(
        Some(courier),
        Order::Move {
            target: Target::None,
        },
    );
    client.until("the spinner gone again", |c| {
        spinner(c).0 == Visibility::Hidden
    });
}
