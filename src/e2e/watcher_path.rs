//! Walking around a Watcher's collision body in a real match.

use bevy::prelude::Vec2;
use bota_client_core::Intent;
use bota_proto::{Cheat, Fixed, HeroId, UnitKind, Vec2 as Spot};

use super::harness::{Client, Server};

const RADIANT_SHRINE_WATCHER: Spot = Spot::from_ints(1408, 10272);

fn drawn_position(client: &Client) -> Vec2 {
    let body = client.body(client.me().id).expect("the drawn hero");
    client
        .world()
        .get::<crate::units::Unit>(body)
        .expect("the hero body")
        .at
}

#[test]
fn a_move_order_across_a_watcher_routes_the_drawn_hero_around_its_body() {
    cross_watcher("0", bota_core::DOTA_WATCHERS[4], 300);
}

#[test]
fn a_move_just_beyond_the_shrine_watcher_routes_around_its_body() {
    cross_watcher("0", RADIANT_SHRINE_WATCHER, 50);
}

#[test]
fn a_move_just_beyond_the_shrine_watcher_routes_around_its_body_on_solo_mid() {
    cross_watcher("2", RADIANT_SHRINE_WATCHER, 50);
}

fn cross_watcher(map: &str, at: Spot, destination_offset: i32) {
    let server = Server::start(1, &["--map", map, "--objectives", "watchers"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.stand_at((at.x.to_int() - 300, at.y.to_int()));
    let watcher = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        .expect("the watcher");
    let separation = watcher.collision + client.me().collision;
    let centre = Vec2::new(at.x.to_f32(), at.y.to_f32());
    let goal = centre + Vec2::new(destination_offset as f32, 0.0);
    let goal_fixed = at + Spot::from_ints(destination_offset, 0);
    assert!(
        !goal_fixed.within(at, separation),
        "the destination is outside the watcher body"
    );
    let started = client.tick();
    client.core_mut().act(Intent::MoveTo((goal.x, goal.y)));
    client.until("the hero crosses to the other side of the watcher", |c| {
        assert!(
            !c.me().pos.within(at, separation - Fixed::from_int(1)),
            "the server hero must not enter the watcher body"
        );
        assert!(
            drawn_position(c).distance(centre) >= separation.to_f32() - 1.0,
            "the drawn hero must route around the watcher body"
        );
        assert!(
            c.tick() - started < 8 * bota_core::rules::TICKS_PER_SECOND,
            "the hero gets stuck instead of routing around the watcher: {:?}",
            c.me().pos
        );
        c.me().pos.within(goal_fixed, Fixed::from_int(24))
    });
    client.until("the drawn hero reaches the destination", |c| {
        drawn_position(c).distance(goal) < 24.0
    });
}
