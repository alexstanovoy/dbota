//! Quickbuy purchase borders reflect current gold and the shop's remaining stock.
use super::harness::{Client, MID, Server};
use crate::shop::{Place, ShopCell};
use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ItemId, ItemSlot, Order, Target};

pub(super) fn border(client: &mut Client, item: ItemId) -> Color {
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
    let before = last_order(&client);
    client.core_mut().act(Intent::QuickBuy);
    assert_eq!(
        last_order(&client),
        before,
        "Quickbuy skips depleted stock just as its border does"
    );
    purchase_feedback(&mut client, "dota_hud_error_item_out_of_stock");
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
    let before = last_order(&client);
    client.core_mut().act(Intent::QuickBuy);
    assert_eq!(
        last_order(&client),
        before,
        "Quickbuy sends nothing without a free destination"
    );
    purchase_feedback(&mut client, "dota_hud_error_cant_purchase_inventory_full");
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

fn last_order(client: &Client) -> Option<Order> {
    client.core().shown_orders.last().map(|order| order.order)
}

#[test]
fn quickbuy_skips_unreachable_secret_components_and_buys_an_available_part() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Gold { amount: -100000 });
    client.cheat(Cheat::Gold { amount: 2500 });
    client
        .core_mut()
        .set_quick_buy(ItemId::MONKEY_KING_BAR.0, false);
    assert!(!client.core().at_secret_shop());
    assert_ne!(
        border(&mut client, ItemId::DEMON_EDGE),
        crate::ui::GOLDEN,
        "the secret component is unavailable"
    );
    assert_eq!(
        border(&mut client, ItemId::BLITZ_KNUCKLES),
        crate::ui::GOLDEN,
        "the affordable base component is available"
    );
    client.core_mut().act(Intent::QuickBuy);
    assert_eq!(
        last_order(&client),
        Some(Order::Buy {
            item: ItemId::BLITZ_KNUCKLES
        }),
        "keyboard Quickbuy must agree with the displayed availability"
    );
    client.until("the available component bought by the server", |c| {
        c.slot_of(ItemId::BLITZ_KNUCKLES).is_some()
    });
    client.cheat(Cheat::Gold { amount: 10000 });
    client.core_mut().reject = None;
    client.core_mut().act(Intent::Buy(ItemId::DEMON_EDGE.0));
    assert_eq!(
        last_order(&client),
        Some(Order::Buy {
            item: ItemId::DEMON_EDGE
        }),
        "explicit player buys still reach the server"
    );
    client.until("the authoritative shop refusal", |c| {
        c.core().reject.is_some()
    });
    assert_eq!(
        client.core().reject.as_ref().map(|(why, _)| why),
        Some(&bota_client_core::Refused::Rule(
            bota_proto::RejectReason::NotAtShop
        ))
    );
}

fn split_treads(client: &mut Client, complete: bool) {
    client.cheat(Cheat::Gold { amount: 10000 });
    client.cheat(Cheat::Item {
        item: ItemId::BOOTS,
    });
    client.stand_at(MID);
    client.core_mut().act(Intent::Buy(ItemId::GLOVES.0));
    client.until("gloves in stash", |c| stash_has(c, ItemId::GLOVES));
    if complete {
        client.core_mut().act(Intent::Buy(ItemId::BELT.0));
        client.until("belt in stash", |c| stash_has(c, ItemId::BELT));
    }
    client
        .core_mut()
        .set_quick_buy(ItemId::POWER_TREADS.0, false);
}

fn stash_has(client: &Client, item: ItemId) -> bool {
    client
        .view()
        .player(client.core().my_slot.unwrap())
        .unwrap()
        .stash
        .as_ref()
        .unwrap()
        .iter()
        .flatten()
        .any(|i| i.id == item)
}

#[test]
fn f07_review_quickbuy_with_all_parts_split_sends_nothing_and_emits_no_purchase() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    split_treads(&mut client, true);
    assert!(client.core().missing_for(ItemId::POWER_TREADS.0).is_empty());
    assert_eq!(
        client.core().purchase_availability(ItemId::POWER_TREADS.0),
        bota_client_core::PurchaseAvailability::NothingToBuy
    );
    let before = last_order(&client);
    client.core_mut().act(Intent::QuickBuy);
    assert_eq!(
        last_order(&client),
        before,
        "zero missing parts must never send a buy"
    );
    purchase_feedback(&mut client, "dota_hud_error_item_already_purchased");
    client.cheat(Cheat::Gold { amount: 1 });
    assert!(
        !client
            .core()
            .session()
            .unwrap()
            .recent_events()
            .iter()
            .any(|e| matches!(
                e.kind,
                bota_proto::EventKind::ItemBought {
                    item: ItemId::POWER_TREADS,
                    ..
                }
            )),
        "no phantom ItemBought across the barrier"
    );
    client.core_mut().reject = None;
    client.core_mut().act(Intent::Buy(ItemId::POWER_TREADS.0));
    client.until("the server's explicit empty-plan refusal", |c| {
        matches!(
            c.core().reject,
            Some((
                bota_client_core::Refused::Rule(bota_proto::RejectReason::NothingToBuy),
                _
            ))
        )
    });
    client.cheat(Cheat::Gold { amount: 1 });
    assert!(
        !client
            .core()
            .session()
            .unwrap()
            .recent_events()
            .iter()
            .any(|e| matches!(
                e.kind,
                bota_proto::EventKind::ItemBought {
                    item: ItemId::POWER_TREADS,
                    ..
                }
            )),
        "an explicit empty buy emits no purchase either"
    );
}

#[test]
fn f07_review_quickbuy_counts_the_stash_and_buys_only_the_remaining_part() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    split_treads(&mut client, false);
    assert_eq!(
        client.core().missing_for(ItemId::POWER_TREADS.0),
        vec![ItemId::BAND]
    );
    client.core_mut().act(Intent::QuickBuy);
    client.until("missing part bought remotely", |c| {
        stash_has(c, ItemId::BAND)
    });
    assert!(client.slot_of(ItemId::BOOTS).is_some());
    assert!(stash_has(&client, ItemId::GLOVES));
    assert!(
        client.slot_of(ItemId::POWER_TREADS).is_none(),
        "split parts do not assemble away from base"
    );
}

#[test]
fn f07_review_quickbuy_shows_the_installs_gold_error_when_nothing_is_affordable() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Gold { amount: -100000 });
    client.core_mut().set_quick_buy(ItemId::BOOTS.0, false);
    client.core_mut().act(Intent::QuickBuy);
    client.frames(3);
    let expected = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label("dota_hud_error_not_enough_gold", &[])
        .unwrap();
    assert_eq!(crate::error_msg::shown(client.world_mut()), Some(expected));
}

fn purchase_feedback(client: &mut Client, key: &str) {
    client.frames(3);
    let wanted = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label(key, &[])
        .expect("the install's error line");
    assert_eq!(crate::error_msg::shown(client.world_mut()), Some(wanted));
}

#[test]
fn f07_review_quickbuy_shows_the_secret_shop_reason_without_sending_an_order() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Gold { amount: 10000 });
    client.core_mut().set_quick_buy(ItemId::DEMON_EDGE.0, false);
    let before = last_order(&client);
    client.core_mut().act(Intent::QuickBuy);
    assert_eq!(last_order(&client), before);
    purchase_feedback(&mut client, "dota_hud_error_secret_shop_not_in_range");
}

#[test]
fn quickbuy_skips_an_unavailable_entry_and_completes_later_copies_individually() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Gold { amount: 10000 });
    client.core_mut().set_quick_buy(ItemId::DEMON_EDGE.0, false);
    client.core_mut().set_quick_buy(ItemId::BOOTS.0, true);
    client.core_mut().set_quick_buy(ItemId::BOOTS.0, true);
    assert_eq!(
        client.core().quick_buy_parts(),
        &[ItemId::DEMON_EDGE.0, ItemId::BOOTS.0, ItemId::BOOTS.0]
    );
    for count in 1..=2 {
        client.core_mut().act(Intent::QuickBuy);
        assert_eq!(
            last_order(&client),
            Some(Order::Buy {
                item: ItemId::BOOTS
            }),
            "an unreachable earlier entry must not block later available copies"
        );
        client.until("one more requested copy bought", |c| {
            c.me()
                .items
                .iter()
                .flatten()
                .filter(|item| item.id == ItemId::BOOTS)
                .count()
                == count
                && c.core().quick_buy.targets().len() == 3 - count
        });
        assert_eq!(
            client.core().quick_buy.targets().first(),
            Some(&ItemId::DEMON_EDGE.0),
            "unavailable entries stay queued"
        );
    }
    assert_eq!(client.core().quick_buy.targets(), &[ItemId::DEMON_EDGE.0]);
    let before = client.core().seq();
    client.core_mut().act(Intent::QuickBuy);
    assert_eq!(
        client.core().seq(),
        before,
        "only unavailable entries remain, so no order is sent"
    );
    purchase_feedback(&mut client, "dota_hud_error_secret_shop_not_in_range");
}
