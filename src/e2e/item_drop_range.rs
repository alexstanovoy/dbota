//! Dota item handling ranges: drop 100, pick up 150, hand to an ally 300.
//! https://liquipedia.net/dota2/Portal:Items

use bevy::{input::ButtonState, prelude::*};
use bota_client_core::Intent;
use bota_proto::{Cheat, CreepSide, Fixed, HeroId, ItemId, UnitKind, Vec2 as At};

use super::harness::{Client, MID, Server};

fn carrying_boots() -> (Server, Client, u8) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    client.core_mut().act(Intent::Stop);
    client.cheat(Cheat::Item {
        item: ItemId::BOOTS,
    });
    let slot = client.slot_of(ItemId::BOOTS).expect("boots in the bag");
    client.until("the boots slot laid out", |c| {
        c.item_box(slot)
            .and_then(|b| c.centre_of(b))
            .is_some_and(|at| at != Vec2::ZERO)
    });
    (server, client, slot)
}

fn drag_to_ground(client: &mut Client, slot: u8, at: At) {
    let item_box = client.item_box(slot).expect("the boots slot");
    client.hover_ui(item_box, std::time::Duration::from_secs(30));
    let from = client
        .centre_of(item_box)
        .expect("the inventory slot laid out");
    let to = client
        .screen_of((at.x.to_f32(), at.y.to_f32()))
        .expect("the drop point on screen");
    client.mouse(from);
    client.button(MouseButton::Left, ButtonState::Pressed);
    assert_eq!(
        client.core().held_item,
        Some(slot),
        "the physical press picks the drawn inventory slot; hovered={:?}, loading={}",
        client
            .world()
            .get::<bevy::picking::hover::Hovered>(item_box)
            .map(|hover| hover.get()),
        client.world().resource::<crate::loading::Loading>().shown
    );
    for step in 1..=4 {
        client.mouse(from.lerp(to, step as f32 / 4.0));
    }
    // The camera is still settling after its teleport-follow. Reproject the requested
    // point until the actual cursor ray reaches it before releasing the drag.
    client.until("the drag cursor on the requested ground point", |c| {
        let to = c
            .screen_of((at.x.to_f32(), at.y.to_f32()))
            .expect("the drop point on screen");
        c.mouse(to);
        c.world()
            .resource::<crate::camera::CursorGround>()
            .0
            .is_some_and(|p| {
                Vec2::new(p.x, -p.z).distance(Vec2::new(at.x.to_f32(), at.y.to_f32())) < 1.0
            })
    });
    client.button(MouseButton::Left, ButtonState::Released);
}

fn ground_boots_drawn(client: &mut Client, at: At) -> bool {
    let world = client.world_mut();
    world
        .query::<(&crate::loot::Loot, &Transform)>()
        .iter(world)
        .any(|(loot, t)| {
            loot.model.as_deref() == Some("item.boots.model")
                && Vec2::new(t.translation.x, -t.translation.z)
                    .distance(Vec2::new(at.x.to_f32(), at.y.to_f32()))
                    < 1.0
        })
}

fn check_drop(distance: i32) {
    let (_server, mut client, slot) = carrying_boots();
    let from = client.me().pos;
    let goal = from + At::from_ints(distance, 0);
    drag_to_ground(&mut client, slot, goal);
    client.until("the boots dropped after walking within 100", |c| {
        if c.slot_of(ItemId::BOOTS).is_some() {
            return false;
        }
        let loot = c
            .view()
            .loot
            .iter()
            .find(|l| l.item == ItemId::BOOTS)
            .expect("the boots on the ground");
        assert!(
            loot.pos.within(goal, Fixed::from_int(5)),
            "the item lands at the dragged point: goal {goal:?}, actual {:?}",
            loot.pos
        );
        assert!(
            c.me().pos.within(loot.pos, Fixed::from_int(100)),
            "the item was dropped too far from its carrier: {:?} -> {:?}",
            c.me().pos,
            loot.pos
        );
        true
    });
    let loot = client
        .view()
        .loot
        .iter()
        .find(|l| l.item == ItemId::BOOTS)
        .copied()
        .expect("the dropped boots in the view");
    client.until("the dropped boots model drawn beside its carrier", |c| {
        let body = c.body(c.me().id).expect("the drawn hero");
        let at = c
            .world()
            .get::<crate::units::Unit>(body)
            .expect("the hero body")
            .at;
        ground_boots_drawn(c, loot.pos)
            && at.distance(Vec2::new(loot.pos.x.to_f32(), loot.pos.y.to_f32())) <= 101.0
    });
    if distance <= 100 {
        assert_eq!(client.me().pos, from, "an in-range drop needs no walk");
    } else {
        assert!(
            !client.me().pos.within(from, Fixed::from_int(100)),
            "the hero walked before releasing the item"
        );
    }
    pick_up_from_125(&mut client, loot);
}

fn pick_up_from_125(client: &mut Client, loot: bota_proto::LootView) {
    // Take from outside drop reach but inside pickup reach, so the new drop limit cannot
    // accidentally narrow pickup reach too.
    client.stand_at((loot.pos.x.to_int() - 125, loot.pos.y.to_int()));
    let before_pickup = client.me().pos;
    assert!(!before_pickup.within(loot.pos, Fixed::from_int(100)));
    assert!(before_pickup.within(loot.pos, Fixed::from_int(150)));
    client.core_mut().act(Intent::Smart {
        at: (loot.pos.x.to_f32(), loot.pos.y.to_f32()),
        unit: None,
    });
    client.until("the boots picked back up", |c| {
        c.slot_of(ItemId::BOOTS).is_some()
    });
    assert_eq!(
        client.me().pos,
        before_pickup,
        "an in-range pickup needs no walk"
    );
}

#[test]
fn dragging_an_item_300_away_walks_the_drawn_hero_within_100_before_dropping() {
    check_drop(300);
}

#[test]
fn dragging_an_item_within_100_drops_and_picks_up_without_walking() {
    check_drop(80);
}

#[test]
fn handing_an_item_to_a_courier_350_away_walks_within_300_first() {
    let (_server, mut client, slot) = carrying_boots();
    let from = client.me().pos;
    let goal = from + At::from_ints(350, 0);
    let courier = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Courier && u.team == client.me().team)
        .expect("the courier")
        .id;
    client.core_mut().act(Intent::SelectCourier);
    client.cheat(Cheat::Teleport { at: goal });
    client.core_mut().act(Intent::Stop);
    client.core_mut().act(Intent::SelectHero);
    client.core_mut().act(Intent::PutAt {
        slot,
        at: (goal.x.to_f32(), goal.y.to_f32()),
        unit: Some(courier),
    });
    client.until("the courier receives the boots within 300", |c| {
        let to = c.view().unit(courier).expect("the courier");
        if !to.items.iter().flatten().any(|i| i.id == ItemId::BOOTS) {
            return false;
        }
        assert!(
            c.me().pos.within(to.pos, Fixed::from_int(300)),
            "the item was handed over too far away"
        );
        assert!(c.slot_of(ItemId::BOOTS).is_none());
        true
    });
    assert!(
        !client.me().pos.within(from, Fixed::from_int(1)),
        "the hero walks before handing over"
    );
}
