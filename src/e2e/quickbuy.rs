//! Quickbuy purchase borders reflect current gold and the shop's remaining stock.
use super::harness::{Client, MID, Server};
use crate::shop::{Place, ShopCell};
use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ItemId, ItemSlot, Order, Target};

fn border(client: &mut Client, item: ItemId) -> Color {
    let find = |client: &mut Client| {
        let world = client.world_mut();
        world
            .query::<(&ShopCell, &Place, &BorderColor)>()
            .iter(world)
            .find(|(cell, place, _)| cell.0 == item.0 && **place == Place::Quick)
            .map(|(_, _, color)| color.top)
    };
    client.until("the Quickbuy cell drawn", |c| find(c).is_some());
    client.frames(2);
    find(client).expect("the Quickbuy cell exists")
}

#[test]
fn quickbuy_is_gold_only_when_current_gold_and_stock_allow_purchase_even_away_from_base() {
    // The install's dota_ui_shop_item.css uses CanPurchaseOverlay for purchases,
    // a distinct shopborder_nosecret for secret-shop access, and OutOfStockOverlay.
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let home = client.me().pos;
    client.stand_at(MID);
    assert!(
        !client.core().unit_at_home_shop(client.me().id),
        "the hero is away from the home shop"
    );
    client.core_mut().set_quick_buy(ItemId::BOOTS.0, false);
    client.cheat(Cheat::Gold { amount: -100000 });
    client.frames(3);
    assert_ne!(
        border(&mut client, ItemId::BOOTS),
        crate::ui::GOLDEN,
        "unaffordable boots have no gold border"
    );
    client.cheat(Cheat::Gold { amount: 10000 });
    client.frames(3);
    assert_eq!(
        border(&mut client, ItemId::BOOTS),
        crate::ui::GOLDEN,
        "affordable base items are gold away from the base"
    );
    client.stand_at((home.x.to_int(), home.y.to_int()));
    client
        .core_mut()
        .set_quick_buy(ItemId::OBSERVER_WARD.0, false);
    client.frames(3);
    assert_eq!(
        border(&mut client, ItemId::OBSERVER_WARD),
        crate::ui::GOLDEN,
        "a stocked ward can be bought"
    );
    exhaust_ward_stock(&mut client);
    client
        .core_mut()
        .set_quick_buy(ItemId::OBSERVER_WARD.0, false);
    client.frames(3);
    assert_eq!(
        client
            .core()
            .stock_of(ItemId::OBSERVER_WARD.0)
            .unwrap()
            .left,
        0
    );
    assert_ne!(
        border(&mut client, ItemId::OBSERVER_WARD),
        crate::ui::GOLDEN,
        "gold cannot buy an out-of-stock ward"
    );
}

#[test]
fn quickbuy_secret_components_turn_gold_only_with_a_hero_or_courier_at_the_shop() {
    for map in ["0", "2", "1"] {
        let server = Server::start(1, &["--map", map]);
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
        client.cheat(Cheat::Gold { amount: 100000 });
        client.cheat(Cheat::Invulnerable { on: true });
        client.core_mut().set_quick_buy(ItemId::DEMON_EDGE.0, false);
        client.frames(3);
        if map == "1" {
            assert_eq!(
                border(&mut client, ItemId::DEMON_EDGE),
                crate::ui::GOLDEN,
                "Demo's universal shop permits the purchase at base"
            );
            continue;
        }
        let home = (client.me().pos.x.to_int(), client.me().pos.y.to_int());
        assert_ne!(
            border(&mut client, ItemId::DEMON_EDGE),
            crate::ui::GOLDEN,
            "gold alone cannot buy a secret component from base on map {map}"
        );
        client.stand_at((4260, 10950));
        client.frames(3);
        assert_eq!(
            border(&mut client, ItemId::DEMON_EDGE),
            crate::ui::GOLDEN,
            "the hero's hull touches the secret-shop zone"
        );
        client.stand_at(home);
        client.frames(3);
        assert_ne!(
            border(&mut client, ItemId::DEMON_EDGE),
            crate::ui::GOLDEN,
            "leaving the shop removes purchase gold"
        );
        teleport_courier(&mut client, (4260, 10950));
        assert_eq!(
            border(&mut client, ItemId::DEMON_EDGE),
            crate::ui::GOLDEN,
            "a courier in the shop grants access while the hero stays home"
        );
        teleport_courier(&mut client, home);
        assert_ne!(
            border(&mut client, ItemId::DEMON_EDGE),
            crate::ui::GOLDEN,
            "moving the courier away removes access"
        );
    }
}

fn teleport_courier(client: &mut Client, at: (i32, i32)) {
    client.core_mut().act(Intent::SelectCourier);
    client.cheat(Cheat::Teleport {
        at: bota_proto::Vec2::from_ints(at.0, at.1),
    });
    client.core_mut().act(Intent::SelectHero);
    client.frames(3);
}

#[test]
fn quickbuy_requires_a_free_berth_for_an_affordable_base_item() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let home = client.me().pos;
    client.stand_at(MID);
    client.cheat(Cheat::Gold { amount: 10000 });
    for _ in 0..6 {
        let before = stash_count(&client);
        client.core_mut().act(Intent::Buy(ItemId::IRON_BRANCH.0));
        client.until("the branch bought into the stash", |c| {
            stash_count(c) > before
        });
    }
    assert_eq!(stash_count(&client), 6);
    client.core_mut().set_quick_buy(ItemId::BOOTS.0, false);
    client.frames(3);
    assert_ne!(
        border(&mut client, ItemId::BOOTS),
        crate::ui::GOLDEN,
        "a full stash prevents the remote purchase"
    );
    client.stand_at((home.x.to_int(), home.y.to_int()));
    client.frames(3);
    assert_eq!(
        border(&mut client, ItemId::BOOTS),
        crate::ui::GOLDEN,
        "a free slot in the hero bag permits the purchase at base"
    );
}

fn stash_count(client: &Client) -> usize {
    client
        .view()
        .player(client.core().my_slot.expect("seated"))
        .expect("own player")
        .stash
        .as_ref()
        .expect("own stash")
        .iter()
        .flatten()
        .count()
}

#[test]
fn quickbuy_keeps_the_shared_hover_and_chosen_frames() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.core_mut().set_quick_buy(ItemId::BOOTS.0, false);
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .shop = true;
    client.frames(3);
    let world = client.world_mut();
    let cell = world
        .query::<(Entity, &ShopCell, &Place)>()
        .iter(world)
        .find(|(_, item, place)| item.0 == ItemId::BOOTS.0 && **place == Place::Quick)
        .map(|(entity, _, _)| entity)
        .expect("the Quickbuy boots");
    let at = client.centre_of(cell).expect("the Quickbuy cell laid out");
    client.mouse(at);
    client.frames(3);
    assert_eq!(
        border(&mut client, ItemId::BOOTS),
        Color::srgb(0.86, 0.72, 0.36),
        "Quickbuy keeps the shop's hover frame"
    );
    client.click(MouseButton::Left);
    client.mouse(Vec2::new(900.0, 450.0));
    client.frames(3);
    assert_eq!(
        border(&mut client, ItemId::BOOTS),
        Color::srgb(0.92, 0.93, 0.95),
        "Quickbuy keeps the chosen item's white frame"
    );
}

fn exhaust_ward_stock(client: &mut Client) {
    let count = client
        .core()
        .stock_of(ItemId::OBSERVER_WARD.0)
        .expect("ward stock")
        .left;
    assert!(count > 0 && count <= 8, "bounded initial ward stock");
    for _ in 0..count {
        let before = client
            .core()
            .stock_of(ItemId::OBSERVER_WARD.0)
            .unwrap()
            .left;
        client.core_mut().act(Intent::Buy(ItemId::OBSERVER_WARD.0));
        client.until("the shop filled the order", |c| {
            c.core().stock_of(ItemId::OBSERVER_WARD.0).unwrap().left < before
        });
    }
    // Buying the target correctly removes it from Quickbuy. Drop the purchased
    // stack before asking for another, so the out-of-stock cell really stays drawn.
    let slot = client
        .slot_of(ItemId::OBSERVER_WARD)
        .expect("the bought wards");
    client.core_mut().send_order_to(
        None,
        Order::Put {
            slot: ItemSlot(slot),
            target: Target::None,
        },
    );
    client.until("the purchased wards dropped", |c| {
        c.slot_of(ItemId::OBSERVER_WARD).is_none()
    });
}
