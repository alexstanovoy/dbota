//! Dead purchases are visible in the carried kit and can be returned during that death.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ItemId, RejectReason};

use super::enemy_base_shop::{aim, click_node};
use super::harness::{Client, MID, Server};
use super::items::button_saying;
use super::purchase_destination::{icon_in_slot, stash};

fn shopper(server: &Server) -> Client {
    let mut client = Client::join(server, HeroId::SHADOW_FIEND, &["--shop", "basics:/boots"]);
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Gold { amount: 10000 });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::StartGame);
    client
}

fn die(client: &mut Client) {
    let enemy = bota_core::fountain_pos(
        client.core().session().expect("match").map(),
        bota_proto::Team::Dire,
    );
    client.cheat(Cheat::Teleport { at: enemy });
    client.within(
        std::time::Duration::from_secs(30),
        "the dead hero's kit",
        |c| {
            c.core().my_standing_hero().is_none()
                && c.view()
                    .player(c.core().my_slot.expect("slot"))
                    .expect("player")
                    .kit
                    .is_some()
        },
    );
}

#[test]
fn a_shop_purchase_while_dead_draws_in_inventory_and_refunds_through_the_menu() {
    let server = Server::start(1, &[]);
    let mut client = shopper(&server);
    client.cheat(Cheat::Item {
        item: ItemId::IRON_BRANCH,
    });
    die(&mut client);
    super::shop_queue::point_item(&mut client, ItemId::BOOTS, crate::shop::Place::Grid);
    let gold = client.core().spendable_gold();
    client.click(MouseButton::Right);
    client.until(
        "the server processed the purchase and the HUD caught up",
        |c| {
            let current = c.core().spendable_gold();
            current < gold
                && c.core().source.pending_orders() == 0
                && crate::shop::drawn_gold(c.world_mut()) == Some(current.to_string())
        },
    );
    assert!(client.core().my_standing_hero().is_none());
    assert!(
        icon_in_slot(&mut client, 1, ItemId::BOOTS),
        "the purchased item is drawn in the dead inventory rather than the stash"
    );
    assert!(stash(&client).iter().all(Option::is_none));
    let node = client.item_box(1).expect("new item slot");
    aim(&mut client, node);
    client.click(MouseButton::Right);
    let texts = client.world().resource::<crate::dota_text::Texts>();
    let info = texts
        .label("DOTA_InventoryMenu_ShowInShop", &[])
        .expect("menu label");
    let sell = texts.label("DOTA_InventoryMenu_Sell", &[]).expect("Sell");
    client.until("the dead item's menu drawn", |c| {
        button_saying(c, &info).is_some()
    });
    let sell = button_saying(&mut client, &sell)
        .expect("a new dead purchase offers Sell, not courier marking");
    click_node(&mut client, sell);
    client.until("the dead purchase refunded", |c| {
        c.core().item_in(1).is_none() && c.core().spendable_gold() >= gold
    });
    assert!(
        client.core().my_standing_hero().is_none(),
        "the refund occurred during the same death"
    );
    let old = client.item_box(0).expect("old item slot");
    aim(&mut client, old);
    client.click(MouseButton::Right);
    client.until("the older item's menu drawn", |c| {
        button_saying(c, &info).is_some()
    });
    let sell_label = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label("DOTA_InventoryMenu_Sell", &[])
        .expect("Sell");
    assert!(
        button_saying(&mut client, &sell_label).is_none(),
        "a pre-death item offers no refused sale"
    );
    client.core_mut().reject = None;
    client.core_mut().act(Intent::Sell(0));
    client.until("the older item is still protected", |c| {
        matches!(
            c.core().reject,
            Some((bota_client_core::Refused::Rule(RejectReason::HeroDead), _))
        )
    });
    assert!(icon_in_slot(&mut client, 0, ItemId::IRON_BRANCH));
}

#[test]
fn grab_all_moves_stashed_items_into_the_dead_heros_drawn_inventory() {
    let server = Server::start(1, &[]);
    let mut client = shopper(&server);
    client.cheat(Cheat::Invulnerable { on: true });
    client.stand_at(MID);
    client.core_mut().act(Intent::Buy(ItemId::QUELLING_BLADE.0));
    client.until("the remote item in stash", |c| {
        stash(c)
            .iter()
            .flatten()
            .any(|s| s.id == ItemId::QUELLING_BLADE)
    });
    client.cheat(Cheat::Invulnerable { on: false });
    die(&mut client);
    let visible_grab = |client: &mut Client| {
        let world = client.world_mut();
        world
            .query_filtered::<(Entity, &Visibility), With<crate::shop::GrabAll>>()
            .iter(world)
            .find(|(_, visible)| **visible != Visibility::Hidden)
            .map(|(entity, _)| entity)
    };
    let grab = visible_grab(&mut client).expect("Grab All is available while dead");
    click_node(&mut client, grab);
    client.until("Grab All's item drawn in the dead kit", |c| {
        icon_in_slot(c, 0, ItemId::QUELLING_BLADE)
    });
    assert!(stash(&client).iter().all(Option::is_none));
    assert!(client.core().my_standing_hero().is_none());
}

#[test]
fn quickbuy_uses_dead_inventory_space_even_when_the_stash_is_full() {
    let server = Server::start(1, &[]);
    let mut client = shopper(&server);
    client.cheat(Cheat::Invulnerable { on: true });
    client.stand_at(MID);
    for count in 1..=bota_core::rules::STASH_SLOTS {
        client.core_mut().act(Intent::Buy(ItemId::QUELLING_BLADE.0));
        client.until("one more remote item fills the stash", |c| {
            stash(c).iter().flatten().count() == count
        });
    }
    client.cheat(Cheat::Invulnerable { on: false });
    die(&mut client);
    client.core_mut().set_quick_buy(ItemId::BOOTS.0, false);
    assert_eq!(
        super::quickbuy::border(&mut client, ItemId::BOOTS),
        crate::ui::GOLDEN,
        "a dead inventory berth makes the component buyable"
    );
    client.key(KeyCode::F5, bevy::input::ButtonState::Pressed);
    client.key(KeyCode::F5, bevy::input::ButtonState::Released);
    client.until("Quickbuy boots drawn in the dead inventory", |c| {
        icon_in_slot(c, 0, ItemId::BOOTS)
    });
    assert!(client.core().my_standing_hero().is_none());
    assert_eq!(
        stash(&client).iter().flatten().count(),
        bota_core::rules::STASH_SLOTS
    );
}
