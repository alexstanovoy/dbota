//! Dota's control options: Right-Click Allies (To Follow, To Attack, To
//! Deny), the Directional Move key, Teleport Requires Hold/Cancel to Stop.

use std::time::Duration;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::{Refused, Undeniable};
use bota_proto::{Cheat, EntityId, HeroId, Order, Target, UnitKind, Vec2 as At};

use super::harness::{Client, MID, Server};

/// The last order one's seat sent.
fn last_order(client: &Client) -> Option<Order> {
    let slot = client.core().my_slot?;
    client
        .core()
        .shown_orders
        .iter()
        .rev()
        .find(|o| o.slot == slot)
        .map(|o| o.order)
}

/// Right clicks a unit where it is drawn, once the pointer is on it; the unit the click got
/// (a creep walks on between frames, into its neighbours or off the pointer: then none) and
/// whether the game let it be denied then.
fn right_click(client: &mut Client, id: EntityId) -> Option<(EntityId, bool)> {
    client.within(Duration::from_secs(10), "the pointer on the unit", |c| {
        let Some(body) = c.body(id) else {
            return false;
        };
        let at = c
            .world()
            .get::<crate::units::Unit>(body)
            .expect("a unit")
            .at;
        let pixel = c.screen_of((at.x, at.y)).expect("on screen");
        c.mouse(pixel);
        let hover = c.world().resource::<crate::pick::Hover>();
        hover.attack.is_some_and(|(on, _)| on == id)
    });
    client.button(MouseButton::Right, ButtonState::Pressed);
    let got = client
        .world()
        .resource::<crate::pick::Hover>()
        .attack
        .map(|(on, _)| on);
    let unit = got.and_then(|got| client.view().unit(got)).cloned();
    client.button(MouseButton::Right, ButtonState::Released);
    client.frames(2);
    let unit = unit?;
    let deniable = bota_core::deniable(
        unit.kind,
        bota_proto::Fixed::from_int(unit.hp),
        bota_proto::Fixed::from_int(unit.max_hp),
    );
    Some((unit.id, deniable))
}

/// A seat at the mid lane with a wave of its own side's creeps and one of the enemy's fighting
/// beside it, run with Right-Click Allies set to `mode`.
fn among_creeps(mode: &str) -> (Server, Client) {
    let server = Server::start(1, &[]);
    let set = format!("right_click_allies={mode}");
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--set", &set]);
    client.stand_at(MID);
    for (enemy, dx) in [(false, -250), (true, 250)] {
        client.cheat(Cheat::CreateCreeps {
            enemy,
            at: At::from_ints(MID.0 + dx, MID.1 + 200),
        });
    }
    (server, client)
}

/// An allied lane creep drawn on screen, worn down below half its health or not.
fn ally_creep(client: &Client, low: bool) -> Option<EntityId> {
    let mine = client.me().team;
    client
        .view()
        .units
        .iter()
        .filter(|u| u.team == mine && u.kind == UnitKind::CreepMelee)
        .filter(|u| (2 * u.hp < u.max_hp) == low && u.hp > 0)
        .find(|u| client.body(u.id).is_some())
        .map(|u| u.id)
}

/// Right-Click Allies as Dota's: To Follow follows an ally whatever its health; To Attack
/// attacks one the game lets be denied and refuses one it does not, saying why, sending
/// nothing; To Deny denies the one and follows the other.
#[test]
fn right_click_allies_follows_attacks_or_denies_as_set() {
    for mode in ["follow", "attack", "deny"] {
        let (_server, mut client) = among_creeps(mode);
        let mut seen = [false; 2];
        // A creep of each health: clicks go on until one of each was got.
        for _ in 0..24 {
            let Some(low) = [false, true]
                .into_iter()
                .find(|low| !seen[usize::from(*low)])
            else {
                break;
            };
            client.until("an allied creep", |c| ally_creep(c, low).is_some());
            let creep = ally_creep(&client, low).expect("there");
            let before = last_order(&client);
            let Some((got, deniable)) = right_click(&mut client, creep) else {
                continue;
            };
            seen[usize::from(deniable)] = true;
            let follow = Order::Move {
                target: Target::Unit(got),
            };
            let attack = Order::Attack {
                target: Target::Unit(got),
            };
            match (mode, deniable) {
                ("attack", false) => {
                    assert_eq!(last_order(&client), before, "{mode}: nothing sent");
                    let said = client.core().reject.as_ref().map(|(r, _)| r.clone());
                    let why = Refused::Undeniable(Undeniable::HealthTooHigh);
                    assert_eq!(said, Some(why), "{mode}: told why");
                }
                ("follow", _) | ("deny", false) => {
                    assert_eq!(
                        last_order(&client),
                        Some(follow),
                        "{mode}, deniable {deniable}"
                    );
                }
                _ => assert_eq!(
                    last_order(&client),
                    Some(attack),
                    "{mode}, deniable {deniable}"
                ),
            }
        }
        assert_eq!(seen, [true, true], "{mode}: a creep of each health clicked");
    }
}

/// Directional Move, bound to a key (the install's templates bind none): held, the hero walks
/// the straight line towards the cursor; let go, it stops.
#[test]
fn the_directional_move_key_walks_towards_the_cursor_and_stops_when_let_go() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--set", "key.move_direction=K"],
    );
    client.stand_at(MID);
    let start = client.me().pos;
    let towards = (MID.0 as f32 + 900.0, MID.1 as f32);
    let pixel = client.screen_of(towards).expect("on screen");
    client.mouse(pixel);
    client.key(KeyCode::KeyK, ButtonState::Pressed);
    client.frames(2);
    assert!(
        matches!(last_order(&client), Some(Order::MoveDirection { .. })),
        "a directional move: {:?}",
        last_order(&client)
    );
    client.within(Duration::from_secs(10), "walked towards the cursor", |c| {
        c.me().pos.x.to_f32() > start.x.to_f32() + 150.0
    });
    let walked = client.me().pos;
    let drift = (walked.y.to_f32() - start.y.to_f32()).abs();
    assert!(drift < 60.0, "a straight line east: {drift} off it");
    client.key(KeyCode::KeyK, ButtonState::Released);
    client.frames(2);
    assert_eq!(
        last_order(&client),
        Some(Order::Move {
            target: Target::None
        }),
        "let go: stopped"
    );
}

/// One's hero channelling a town portal to its side's tower, then a right click on the ground,
/// run with `extra`: whether the channel still runs a moment after the click.
fn teleport_then_click(extra: &[&str]) -> bool {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, extra);
    client.follow_me();
    client.cheat(Cheat::Refresh);
    let mine = client.me().team;
    let tower = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Tower && u.team == mine)
        .map(|u| (u.id, u.pos))
        .expect("a tower of one's side");
    let core = client.core_mut();
    core.act(bota_client_core::Intent::Press {
        slot: bota_client_core::Slot::Item(bota_client_core::TOWN_PORTAL_SLOT),
        ctrl: false,
    });
    core.act(bota_client_core::Intent::Aimed {
        at: (tower.1.x.to_f32(), tower.1.y.to_f32()),
        unit: Some(tower.0),
    });
    let me = client.me().id;
    let channels = |c: &Client| crate::teleport::channel_share(c.view(), me).is_some();
    client.within(
        Duration::from_secs(10),
        "the town portal channelling",
        |c| channels(c),
    );
    let at = client.me().pos;
    let ground = (at.x.to_f32() + 300.0, at.y.to_f32() + 300.0);
    let pixel = client.screen_of(ground).expect("on screen");
    client.mouse(pixel);
    client.click(MouseButton::Right);
    client.frames(15);
    channels(&client)
}

/// Dota's Teleport Requires Hold/Cancel to Stop: off (Dota's default) a move given while a town
/// portal channels breaks it off; on, the move waits for the landing and the channel runs on.
#[test]
fn teleport_requires_hold_keeps_the_channel_through_a_move() {
    assert!(
        !teleport_then_click(&[]),
        "off: the move breaks the channel"
    );
    let on = ["--set", "teleport_requires_halt=true"];
    assert!(teleport_then_click(&on), "on: the channel runs on");
}

/// Whether one's hero channels.
fn channelling(client: &Client) -> bool {
    client.me().statuses.bits & bota_proto::StatusFlags::CHANNELLING != 0
}

/// Pudge channelling Dismember on an enemy hero beside him, then a right click on the ground,
/// run with `extra`: whether the channel still runs a moment after the click.
fn dismember_then_click(extra: &[&str]) -> bool {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, extra);
    client.stand_at(MID);
    client.cheat(Cheat::Levels { count: 5 });
    let slot = client
        .me()
        .abilities
        .iter()
        .position(|a| a.id == bota_proto::AbilityId::DISMEMBER)
        .expect("Dismember") as u8;
    let press = |ctrl| bota_client_core::Intent::Press {
        slot: bota_client_core::Slot::Ability(slot),
        ctrl,
    };
    client.core_mut().act(press(true));
    client.frames(3);
    client.cheat(Cheat::CreateHero {
        hero: HeroId::SHADOW_FIEND,
        enemy: true,
        at: At::from_ints(MID.0 + 120, MID.1),
    });
    let mine = client.me().team;
    let enemy = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.team != mine)
        .map(|u| (u.id, u.pos))
        .expect("the enemy hero");
    client.core_mut().act(press(false));
    client.core_mut().act(bota_client_core::Intent::Aimed {
        at: (enemy.1.x.to_f32(), enemy.1.y.to_f32()),
        unit: Some(enemy.0),
    });
    client.within(Duration::from_secs(10), "Dismember channelling", |c| {
        channelling(c)
    });
    let at = client.me().pos;
    let ground = (at.x.to_f32() - 300.0, at.y.to_f32() + 300.0);
    let pixel = client.screen_of(ground).expect("on screen");
    client.mouse(pixel);
    client.click(MouseButton::Right);
    client.frames(10);
    channelling(&client)
}

/// Dota's Abilities Require Hold/Cancel to Stop: off (Dota's default) a move given while an
/// ability channels breaks it off; on, the move waits for the channel's end and it runs on.
#[test]
fn abilities_require_hold_keeps_a_channel_through_a_move() {
    assert!(
        !dismember_then_click(&[]),
        "off: the move breaks the channel"
    );
    let on = ["--set", "channels_require_stop=true"];
    assert!(dismember_then_click(&on), "on: the channel runs on");
}
