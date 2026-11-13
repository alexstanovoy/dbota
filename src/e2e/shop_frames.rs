//! The shop's gold frame marks what the seat can buy right now and nothing else: it follows the
//! gold, the shop the hero can reach, and what a composite would really cost with the parts held.

use bevy::prelude::*;
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, MID, Server};
use super::shop_queue::cells;
use crate::shop::{Place, ShopCell};

/// The frame of the item's visible icon in `place`.
fn frame(client: &mut Client, item: ItemId, place: Place) -> Color {
    let find = |client: &mut Client| {
        let world = client.world_mut();
        world
            .query::<(
                &ShopCell,
                &Place,
                &BorderColor,
                &InheritedVisibility,
                &ComputedNode,
            )>()
            .iter(world)
            .find(|(cell, at, _, seen, node)| {
                cell.0 == item.0 && **at == place && seen.get() && node.size() != Vec2::ZERO
            })
            .map(|(_, _, color, _, _)| color.top)
    };
    client.until("the icon drawn", |c| find(c).is_some());
    client.frames(2);
    find(client).expect("the icon is drawn")
}

fn is_gold(client: &mut Client, item: ItemId) -> bool {
    frame(client, item, Place::Grid) == crate::ui::GOLDEN
}

fn set_gold(client: &mut Client, amount: i32) {
    client.cheat(Cheat::Gold { amount: -1_000_000 });
    client.cheat(Cheat::Gold { amount });
    client.until("the gold told", |c| c.core().spendable_gold() == amount);
}

fn shopper(search: &str) -> (Server, Client) {
    let server = Server::start(1, &[]);
    let client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--shop", &format!("basics:/{search}")],
    );
    (server, client)
}

/// Gold alone decides among what the hero's own shop sells: the frame follows every change of it.
#[test]
fn the_gold_frame_marks_only_items_the_gold_in_hand_pays_for() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--shop", "basics"]);
    set_gold(&mut client, 400);
    assert!(is_gold(&mut client, ItemId::CLARITY), "50 gold of 400");
    assert!(!is_gold(&mut client, ItemId::BOOTS), "500 gold of 400");
    let shown: Vec<ItemId> = cells(&mut client, Place::Grid)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let gold: Vec<ItemId> = shown
        .iter()
        .copied()
        .filter(|id| is_gold(&mut client, *id))
        .collect();
    assert!(
        gold.len() < shown.len(),
        "not every item of the tab is framed gold"
    );
    for id in &gold {
        let price = client.core().price_of(id.0).0;
        assert!(price <= 400, "{id:?} is framed gold but costs {price}");
    }
    set_gold(&mut client, 1400);
    assert!(is_gold(&mut client, ItemId::BOOTS), "500 gold of 1400");
    set_gold(&mut client, 0);
    for id in shown {
        // The wards of the shop cost nothing.
        let free = client.core().price_of(id.0).0 == 0;
        assert_eq!(is_gold(&mut client, id), free, "{id:?} with no gold");
    }
}

/// An item of the secret shop is bought at a secret shop and nowhere else; away from the base shop a
/// base item still is, into the stash.
#[test]
fn the_gold_frame_follows_the_shop_the_hero_reaches() {
    let (_server, mut client) = shopper("boots");
    client.stand_at(MID);
    set_gold(&mut client, 10_000);
    assert!(
        is_gold(&mut client, ItemId::BOOTS),
        "a base item is bought into the stash"
    );
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .search = "demon edge".to_string();
    assert!(
        !is_gold(&mut client, ItemId::DEMON_EDGE),
        "a secret shop's item is out of reach in the middle of the lane"
    );
}

/// What is framed is what the buy would really cost: parts in hand are not paid for twice.
#[test]
fn the_gold_frame_of_a_composite_counts_the_parts_already_held() {
    let (_server, mut client) = shopper("wraith");
    client.cheat(Cheat::Item {
        item: ItemId::CIRCLET,
    });
    set_gold(&mut client, 349);
    assert!(
        !is_gold(&mut client, ItemId::WRAITH_BAND),
        "a Slippers and a recipe take 350"
    );
    set_gold(&mut client, 350);
    assert!(
        is_gold(&mut client, ItemId::WRAITH_BAND),
        "with the Circlet in hand, 350 buys the band"
    );
    set_gold(&mut client, 505);
    client.cheat(Cheat::Item {
        item: ItemId::CIRCLET,
    });
    assert!(is_gold(&mut client, ItemId::WRAITH_BAND));
}

/// Before the match what is chosen waits in a queue, and the starting gold less the queue decides.
#[test]
fn the_gold_frame_in_the_preparation_follows_the_gold_the_queue_leaves() {
    let server = Server::start(1, &["--tactical-secs", "120"]);
    let mut client = Client::pick(&server, HeroId::SHADOW_FIEND, &[]);
    assert!(client.core().in_tactical_phase());
    client.until("the storefront laid out", |c| {
        !cells(c, Place::Grid).is_empty()
    });
    let shown: Vec<ItemId> = cells(&mut client, Place::Grid)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let price = |client: &Client, id: ItemId| client.core().price_of(id.0).0;
    let gold = bota_core::rules::STARTING_GOLD;
    let big = shown
        .iter()
        .copied()
        .filter(|id| price(&client, *id) <= gold)
        .max_by_key(|id| price(&client, *id))
        .expect("an affordable starter item");
    let spent = price(&client, big);
    assert!(
        spent * 2 > gold,
        "{big:?} costs {spent}: too little to leave less than itself"
    );
    assert!(is_gold(&mut client, big), "{spent} gold of {gold}");
    client.core_mut().queue_buy(big.0);
    client.frames(3);
    let left = gold - spent;
    assert!(
        !is_gold(&mut client, big),
        "{spent} gold of the {left} the queue leaves"
    );
    for id in shown {
        assert_eq!(
            is_gold(&mut client, id),
            price(&client, id) <= left,
            "{id:?} costing {} with {left} left",
            price(&client, id)
        );
    }
}

/// Two queues of the same length and the same cost are not the same queue: replacing a Mantle by a
/// Slippers (140 gold each) changes which recipe the gold left can finish.
#[test]
fn the_gold_frame_follows_a_queue_replaced_by_an_item_of_the_same_cost() {
    let server = Server::start(1, &["--tactical-secs", "120"]);
    let mut client = Client::pick(
        &server,
        HeroId::SHADOW_FIEND,
        &["--shop", "upgrades:/wraith"],
    );
    assert!(client.core().in_tactical_phase());
    client.core_mut().queue_buy(ItemId::MANTLE.0);
    client.frames(5);
    assert!(
        !is_gold(&mut client, ItemId::WRAITH_BAND),
        "no Slippers in the queue: the band is out of reach"
    );
    client.core_mut().unqueue_buy(ItemId::MANTLE.0);
    client.core_mut().queue_buy(ItemId::SLIPPERS.0);
    client.frames(5);
    assert!(
        is_gold(&mut client, ItemId::WRAITH_BAND),
        "a queue holding the Slippers leaves the gold for the rest of the band"
    );
}
