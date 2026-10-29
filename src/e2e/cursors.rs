//! Dota's mouse cursors from the install: the arrow red over an enemy and
//! green over a friend, the attack arrow once attack-move is armed.

use bevy::prelude::*;
use bevy::window::{CursorIcon, CustomCursor, PrimaryWindow};
use bota_proto::{Cheat, EntityId, HeroId, Vec2 as At};

use super::harness::{Client, MID, Server};

/// The cache file of the image the window's cursor wears, if it wears one.
fn worn(client: &mut Client) -> Option<String> {
    let world = client.world_mut();
    let icon = world
        .query_filtered::<&CursorIcon, With<PrimaryWindow>>()
        .single(world)
        .ok()?;
    let CursorIcon::Custom(CustomCursor::Image(image)) = icon else {
        return None;
    };
    let server = world.resource::<AssetServer>();
    server.get_path(image.handle.id()).map(|p| p.to_string())
}

/// Puts the pointer on a unit where it is drawn, and waits for the cursor Dota's install has
/// under `logical`.
fn points_at(client: &mut Client, on: Option<EntityId>, logical: &str) {
    let file = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file(logical)
        .expect("the install's cursor in the cache");
    let patience = std::time::Duration::from_secs(20);
    client.within(patience, logical, |c| {
        let at = match on {
            Some(id) => {
                let Some(body) = c.body(id) else {
                    return false;
                };
                c.world()
                    .get::<crate::units::Unit>(body)
                    .expect("a unit")
                    .at
            }
            // Bare ground between the hero and the enemy.
            None => Vec2::new(MID.0 as f32, MID.1 as f32 + 450.0),
        };
        let pixel = c.screen_of((at.x, at.y)).expect("on screen");
        c.mouse(pixel);
        worn(c).as_deref() == Some(file.as_str())
    });
}

/// The pointer over an enemy hero wears Dota's red arrow, over one's own hero the green one,
/// over the ground the plain one, and with attack-move armed over the enemy the attack arrow:
/// each the install's own image (the window's arrow stood in before).
#[test]
fn the_cursor_is_dotas_own_for_what_a_click_would_do() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: true,
        at: At::from_ints(MID.0 + 450, MID.1),
    });
    let enemy = |c: &Client| {
        let mine = c.me().team;
        c.view()
            .units
            .iter()
            .find(|u| u.kind == bota_proto::UnitKind::Hero && u.team != mine)
            .map(|u| u.id)
    };
    client.until("the enemy drawn", |c| {
        enemy(c).and_then(|id| c.body(id)).is_some()
    });
    let (foe, me) = (enemy(&client).expect("there"), client.me().id);
    points_at(&mut client, Some(foe), "cursor.default_enemy");
    points_at(&mut client, Some(me), "cursor.default_team");
    points_at(&mut client, None, "cursor.default");
    client.key(KeyCode::KeyA, bevy::input::ButtonState::Pressed);
    client.key(KeyCode::KeyA, bevy::input::ButtonState::Released);
    points_at(&mut client, Some(foe), "cursor.attack_enemy");
}
