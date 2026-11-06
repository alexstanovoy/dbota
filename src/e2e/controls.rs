//! Dota's control options: Right-Click Allies (To Follow, To Attack, To
//! Deny), the Directional Move key, Teleport Requires Hold/Cancel to Stop.

use std::time::Duration;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::{Refused, Undeniable};
use bota_proto::{
    Cheat, CreepSide, EntityId, HeroId, ModifierSpec, Order, Target, Team, UnitKind, Vec2 as At,
};

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
    // The click's own rule, on the health the view gave: the same one the client decided by.
    let deniable = crate::input::deny_click(unit.kind, unit.hp, unit.max_hp);
    Some((unit.id, deniable))
}

/// A seat at the mid lane with a wave of its own side's creeps and one of the enemy's fighting
/// beside it, run with Right-Click Allies set to `mode`.
fn among_creeps(mode: &str) -> (Server, Client) {
    let set = format!("right_click_allies={mode}");
    among_creeps_with(&["--set", &set])
}

/// [`among_creeps`] with the client started with `extra` arguments: none leaves every setting
/// at its default.
fn among_creeps_with(extra: &[&str]) -> (Server, Client) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, extra);
    client.stand_at(MID);
    for (enemy, dx) in [(false, -250), (true, 250)] {
        client.cheat(Cheat::CreateCreeps {
            enemy,
            at: At::from_ints(MID.0 + dx, MID.1 + 200),
        });
    }
    (server, client)
}

/// An allied lane creep drawn on screen, worn down well below half its health (`low`) or well
/// above it. A creep near the line could cross it between the snapshot the click was decided on
/// and the one the test reads afterwards, and the click would be right about a health the test
/// no longer sees: the clicks go to creeps whose side of the line the next ticks cannot change.
/// A worn creep is also kept from death: one within a tenth of its health could fall before the
/// hero's blow lands, and then nobody denies it. The one with least health goes: the fewer blows
/// it takes, the sooner the deny.
fn ally_creep(client: &Client, low: bool) -> Option<EntityId> {
    let mine = client.me().team;
    client
        .view()
        .units
        .iter()
        .filter(|u| u.team == mine && u.kind == UnitKind::CreepMelee && u.hp > 0)
        .filter(|u| {
            if low {
                u.max_hp < 10 * u.hp && 10 * u.hp < 4 * u.max_hp
            } else {
                10 * u.hp > 6 * u.max_hp
            }
        })
        .filter(|u| client.body(u.id).is_some())
        .min_by_key(|u| u.hp)
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

/// Dota's default: with no setting touched, a right click on an own creep below half its
/// health attacks it, the hero kills it and the game counts a deny; above half the click only
/// follows.
#[test]
fn a_default_right_click_denies_a_worn_creep_and_follows_a_healthy_one() {
    let (_server, mut client) = among_creeps_with(&[]);
    // The lane's creeps march on and the enemy wave wears them down while the hero closes in, so
    // a worn creep could leave or fall before a blow of the normal strength lands: the hero's
    // blows are made strong enough to deny a worn creep with the first.
    let me = client.me().id;
    client.cheat(Cheat::ApplyModifier {
        target: Target::Unit(me),
        spec: ModifierSpec {
            physical_damage: ModifierSpec::MAX_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: 100_000,
    });
    let click_creep = |client: &mut Client, low: bool| loop {
        client.until("an allied creep", |c| ally_creep(c, low).is_some());
        let creep = ally_creep(client, low).expect("there");
        if let Some((got, _)) = right_click(client, creep) {
            return got;
        }
    };
    let healthy = click_creep(&mut client, false);
    assert_eq!(
        last_order(&client),
        Some(Order::Move {
            target: Target::Unit(healthy)
        }),
        "above half: a follow"
    );
    // The enemy wave would keep wearing the allies down and finish the worn creep in the race
    // for its last points: a deny is the hero's kill alone, so once a creep is worn the enemy
    // goes.
    client.until("an allied creep worn", |c| ally_creep(c, true).is_some());
    client.cheat(Cheat::KillCreeps {
        which: match client.me().team {
            Team::Radiant => CreepSide::Dire,
            Team::Dire => CreepSide::Radiant,
            Team::Neutral => unreachable!("a hero is on a side"),
        },
    });
    let worn = click_creep(&mut client, true);
    assert_eq!(
        last_order(&client),
        Some(Order::Attack {
            target: Target::Unit(worn)
        }),
        "below half: an attack, with no attack key pressed"
    );
    client.until("the worn creep denied", |c| {
        c.core()
            .floaters
            .iter()
            .any(|f| f.kind == bota_client_core::FloaterKind::Deny && f.unit == worn)
    });
    assert!(
        client.view().unit(worn).is_none_or(|u| u.hp <= 0),
        "the denied creep is dead"
    );
}

/// What the client wrote while Follow was the default (f01f00f to e24efe2): every setting, the
/// right click's among them, as `follow`.
const FILE_FROM_WHEN_FOLLOW_WAS_THE_DEFAULT: &str =
    "# dbota settings. Keys use the names listed in the client README.
volume = 0.80
muted = false
voice_responses = true
ui_scale = 1.00
damage_numbers = true
edge_pan = true
animate_portrait = true
scoreboard_items = true
teleport_requires_halt = false
channels_require_stop = false
right_click_allies = follow
save_replays = true
";

/// A player whose settings file was written while To Follow was the default has the right click
/// follow a worn creep, though they never chose it; the default is Dota's, To Deny, and a file
/// that predates its return does not hold the player to the old one. A choice made since
/// (any file with a version) is kept.
#[test]
fn a_settings_file_from_when_follow_was_the_default_still_denies_by_default() {
    let server = Server::start(1, &[]);
    let mut client = Client::join_with_settings(
        &server,
        HeroId::SHADOW_FIEND,
        FILE_FROM_WHEN_FOLLOW_WAS_THE_DEFAULT,
    );
    client.stand_at(MID);
    for (enemy, dx) in [(false, -250), (true, 250)] {
        client.cheat(Cheat::CreateCreeps {
            enemy,
            at: At::from_ints(MID.0 + dx, MID.1 + 200),
        });
    }
    client.until("a worn allied creep", |c| ally_creep(c, true).is_some());
    let creep = ally_creep(&client, true).expect("there");
    let (got, _) = loop {
        if let Some(hit) = right_click(&mut client, creep) {
            break hit;
        }
    };
    assert_eq!(
        last_order(&client),
        Some(Order::Attack {
            target: Target::Unit(got)
        }),
        "the click on a worn ally attacks it"
    );
}

/// What the client wrote before the inventory keys became Z X C V B N: every binding, the
/// defaults of then among them (the items on the digits, Camera Lock on Y, the Shop on B).
const FILE_FROM_WHEN_ITEMS_WERE_ON_DIGITS: &str = "# dbota settings.
volume = 0.80
key.camera_lock = Y
key.item_1 = 1
key.item_2 = 2
key.item_3 = 3
key.item_4 = 4
key.item_5 = 5
key.item_6 = 6
key.shop = B
";

/// A player whose settings file holds the inventory keys of the old defaults (the digits) never
/// chose them: the keys are Dota's Z X C V B N, as for a player with no file. A key rebound to
/// something else, once the file has a version, is kept.
#[test]
fn a_settings_file_from_when_items_were_on_digits_still_uses_z_x_c_v_b_n_by_default() {
    let server = Server::start(1, &[]);
    let mut client = Client::join_with_settings(
        &server,
        HeroId::SHADOW_FIEND,
        FILE_FROM_WHEN_ITEMS_WERE_ON_DIGITS,
    );
    client.cheat(Cheat::Item {
        item: bota_proto::ItemId::TANGO,
    });
    let slot = client
        .slot_of(bota_proto::ItemId::TANGO)
        .expect("the tango");
    let key = [
        KeyCode::KeyZ,
        KeyCode::KeyX,
        KeyCode::KeyC,
        KeyCode::KeyV,
        KeyCode::KeyB,
        KeyCode::KeyN,
    ][usize::from(slot)];
    client.key(key, ButtonState::Pressed);
    client.key(key, ButtonState::Released);
    client.frames(2);
    assert!(
        client.core().aiming.is_some(),
        "{key:?} takes the tango of slot {slot} up for aiming"
    );
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
    let sent = client.tick();
    client.click(MouseButton::Right);
    // Whatever the move does to the channel is done a few ticks on, not a few frames on: the
    // server answers at its own pace.
    client.until("ticks past the move", |c| c.tick() >= sent + 6);
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
    let sent = client.tick();
    client.click(MouseButton::Right);
    client.until("ticks past the move", |c| c.tick() >= sent + 6);
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
