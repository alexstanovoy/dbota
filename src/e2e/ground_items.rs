//! An item lying on the ground plays its model's idle clip, as Dota's dropped items do.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::{Cheat, HeroId, ItemId, ItemSlot, Order, Target};

use super::harness::{Client, Server, pose_of, subtree};
use crate::loot::Loot;

#[test]
fn an_item_lying_on_the_ground_plays_its_idle_animation() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Item {
        item: ItemId::SENTRY_WARD,
    });
    client.until("the ward carried", |c| {
        c.slot_of(ItemId::SENTRY_WARD).is_some()
    });
    let slot = client.slot_of(ItemId::SENTRY_WARD).expect("the ward");
    client.core_mut().send_order_to(
        None,
        Order::Put {
            slot: ItemSlot(slot),
            target: Target::None,
        },
    );
    let lying = |c: &mut Client| {
        let world = c.world_mut();
        world
            .query::<(Entity, &Loot)>()
            .iter(world)
            .find(|(_, loot)| loot.model.is_some())
            .map(|(e, _)| e)
    };
    client.within(
        Duration::from_secs(30),
        "the item drawn on the ground",
        |c| lying(c).is_some(),
    );
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    let item = lying(&mut client).expect("the item");
    client.within(
        Duration::from_secs(30),
        "its model's animation playing",
        |c| {
            let world = c.world();
            subtree(world, item).into_iter().any(|e| {
                world
                    .get::<AnimationPlayer>(e)
                    .is_some_and(|p| p.playing_animations().next().is_some())
            })
        },
    );
    let before = pose_of(&client, item);
    client.frames(5);
    let after = pose_of(&client, item);
    assert!(
        before.iter().zip(&after).any(|(b, a)| b.1 != a.1),
        "half a second later no part of the item's model has moved"
    );
}
