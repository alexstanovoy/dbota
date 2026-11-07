//! What a click on the ground beside a big building gets: the building's click box follows its
//! body, so the ground next to it can be walked to and around (reported: the box of the
//! Ancient, towers and barracks was too big, walking around them was impossible).

use std::time::Duration;

use bevy::prelude::*;
use bota_proto::{EntityId, HeroId, UnitKind, UnitView};

use super::harness::{Client, Server};

/// A building of the seat's side, of the kind.
fn own_building(client: &Client, kind: UnitKind) -> UnitView {
    let mine = client.me().team;
    client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == kind && u.team == mine)
        .max_by_key(|u| u.bound)
        .unwrap_or_else(|| panic!("a {kind:?} of the seat's side"))
        .clone()
}

/// What the pointer on a ground point of bota's plane would pick: the left click's and the
/// right click's.
fn picked_at(client: &mut Client, at: (f32, f32)) -> (Option<EntityId>, Option<EntityId>) {
    // The camera is still settling after the hero's teleport-follow: reproject the point until
    // the cursor's ray reaches it, or the pointer lands beside where the test means it.
    client.until("the cursor on the requested ground point", |c| {
        let pixel = c.screen_of(at).expect("on screen");
        c.mouse(pixel);
        c.world()
            .resource::<crate::camera::CursorGround>()
            .0
            .is_some_and(|p| Vec2::new(p.x, -p.z).distance(Vec2::new(at.0, at.1)) < 1.0)
    });
    let hover = client.world().resource::<crate::pick::Hover>();
    (
        hover.select.map(|(id, _)| id),
        hover.attack.map(|(id, _)| id),
    )
}

/// The ground a body's width away from the Ancient, a tower and a barracks (outside what their
/// models fill, inside the hull the sim keeps units out of) to the east, west and
/// the camera's side picks nothing, while their bodies still pick them.
#[test]
fn ground_beside_the_ancient_towers_and_barracks_picks_nothing_but_their_bodies_do() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    for kind in [UnitKind::Tower, UnitKind::Barracks, UnitKind::Ancient] {
        let building = own_building(&client, kind);
        let (x, y) = (building.pos.x.to_f32(), building.pos.y.to_f32());
        let reach = building.bound.to_f32();
        client.stand_at((x as i32, y as i32 - 700));
        let body = picked_at(&mut client, (x, y));
        assert_eq!(
            body,
            (Some(building.id), Some(building.id)),
            "{kind:?}: its body is picked"
        );
        for (name, dx, dy) in [
            ("east", reach, 0.0),
            ("west", -reach, 0.0),
            ("south", 0.0, -reach),
        ] {
            assert_eq!(
                picked_at(&mut client, (x + dx, y + dy)),
                (None, None),
                "{kind:?}: the ground {reach} units {name} of it"
            );
        }
    }
}

/// A hero walks round a tower along its edge by right clicks on the ground beside it: each
/// click is a move there, and it gets there.
#[test]
fn a_hero_walks_round_a_tower_by_right_clicks_beside_it() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let tower = own_building(&client, UnitKind::Tower);
    let (x, y) = (tower.pos.x.to_f32(), tower.pos.y.to_f32());
    let reach = tower.bound.to_f32() + 90.0;
    client.stand_at((x as i32 - reach as i32, y as i32));
    let me = client.me().id;
    for (dx, dy) in [(0.0, -reach), (reach, 0.0)] {
        let goal = (x + dx, y + dy);
        let pixel = client.screen_of(goal).expect("on screen");
        client.mouse(pixel);
        client.click(MouseButton::Right);
        client.within(
            Duration::from_secs(20),
            "the hero at the clicked ground",
            |c| {
                let at = c.view().unit(me).expect("one's hero").pos;
                (at.x.to_f32() - goal.0).hypot(at.y.to_f32() - goal.1) < 60.0
            },
        );
    }
}
