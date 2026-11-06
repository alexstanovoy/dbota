//! Inventory gestures select shop details without consuming an active item's click.
use super::{
    harness::{Client, MID, Server},
    shop_queue::{cells, point_item},
};
use crate::shop::Place;
use bevy::prelude::*;
use bota_client_core::{Intent, IssuedOrder, Slot};
use bota_proto::{Cheat, HeroId, ItemId, Order, Vec2 as Spot};

fn point_slot(client: &mut Client, slot: u8) {
    client.until("the inventory slot laid out", |c| {
        c.item_box(slot)
            .and_then(|e| c.centre_of(e))
            .is_some_and(|at| at != Vec2::ZERO)
    });
    let entity = client.item_box(slot).expect("slot");
    client.mouse(client.centre_of(entity).expect("laid out"));
}

fn button_saying(client: &mut Client, word: &str) -> Option<Entity> {
    let world = client.world_mut();
    world
        .query::<(&Text, &ChildOf, &InheritedVisibility, &ComputedNode)>()
        .iter(world)
        .find(|(text, _, shown, node)| text.0 == word && shown.get() && node.size() != Vec2::ZERO)
        .map(|(_, parent, _, _)| parent.parent())
        .filter(|entity| world.get::<Button>(*entity).is_some())
}

#[test]
fn passive_items_and_recipes_clicked_in_inventory_select_their_open_shop_tree() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--shop", "basics"]);
    client.core_mut().act(Intent::SelectHero);
    for item in [ItemId::BOOTS, ItemId::RECIPE_WRAITH_BAND] {
        client.cheat(Cheat::Item { item });
        let slot = client.slot_of(item).expect("owned item");
        point_slot(&mut client, slot);
        let before = client.core().seq();
        client.click(MouseButton::Left);
        assert_eq!(
            client.core().seq(),
            before,
            "passive information selection sends no order"
        );
        client.until("the passive item's tree drawn", |c| {
            cells(c, Place::Tree).iter().any(|(id, _)| *id == item)
        });
        assert!(
            cells(&mut client, Place::Tree)
                .iter()
                .any(|(id, _)| *id == item),
            "passive inventory click must show {item:?} in the shop tree"
        );
        assert_eq!(client.core().aiming, None);
        assert_eq!(client.slot_of(item), Some(slot));
    }
}

#[test]
fn active_inventory_left_click_still_arms_the_item_with_the_shop_open() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--shop", "basics:/boots"]);
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Item {
        item: ItemId::CLARITY,
    });
    let slot = client.slot_of(ItemId::CLARITY).expect("clarity");
    point_item(&mut client, ItemId::BOOTS, Place::Grid);
    client.click(MouseButton::Left);
    point_slot(&mut client, slot);
    client.click(MouseButton::Left);
    assert_eq!(client.core().aiming, Some(Slot::Item(slot)));
    assert!(
        cells(&mut client, Place::Tree)
            .iter()
            .any(|(id, _)| *id == ItemId::BOOTS)
    );
    assert!(
        cells(&mut client, Place::Tree)
            .iter()
            .all(|(id, _)| *id != ItemId::CLARITY)
    );
}

fn show_from_menu(client: &mut Client, slot: u8, item: ItemId) {
    let before = client.core().seq();
    let word = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label("DOTA_InventoryMenu_ShowInShop", &[])
        .expect("native label");
    point_slot(client, slot);
    client.click(MouseButton::Right);
    let button = button_saying(client, &word).expect("the inventory menu offers Show in Shop");
    if !client.core().controls_selection() {
        for key in [
            "DOTA_InventoryMenu_Sell",
            "DOTA_InventoryMenu_MoveToStash",
            "DOTA_InventoryMenu_Disassemble",
            "DOTA_InventoryMenu_LockCombine",
        ] {
            let word = client
                .world()
                .resource::<crate::dota_text::Texts>()
                .label(key, &[])
                .expect("native label");
            assert!(
                button_saying(client, &word).is_none(),
                "an uncontrolled inventory must not offer {word}"
            );
        }
    }
    client.mouse(client.centre_of(button).expect("laid out"));
    client.click(MouseButton::Left);
    client.until("the item's shop tree shown", |c| {
        cells(c, Place::Tree).iter().any(|(id, _)| *id == item)
    });
    assert!(client.world().resource::<crate::input::Panels>().shop);
    assert_eq!(client.core().aiming, None);
    assert_eq!(
        client.core().seq(),
        before,
        "Show in Shop sends no inventory or cast order"
    );
}

#[test]
fn show_in_shop_reveals_an_active_item_without_using_it() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Item {
        item: ItemId::CLARITY,
    });
    let slot = client.slot_of(ItemId::CLARITY).expect("clarity");
    let before = client.core().item_in(slot).expect("item");
    show_from_menu(&mut client, slot, ItemId::CLARITY);
    assert_eq!(
        client.core().item_in(slot).expect("item").charges,
        before.charges
    );
}

#[test]
fn an_uncontrolled_enemy_inventory_offers_only_show_in_shop() {
    let server = Server::start(2, &[]);
    let mut stage = 0;
    super::harness::scripted_seat(&server, HeroId::PUDGE, move |session| {
        let Some(hero) = session.own_hero() else {
            return Vec::new();
        };
        let cheat = match stage {
            0 => {
                stage = 1;
                Some(Cheat::Item {
                    item: ItemId::BOOTS,
                })
            }
            1 if hero
                .items
                .iter()
                .flatten()
                .any(|item| item.id == ItemId::BOOTS) =>
            {
                stage = 2;
                Some(Cheat::Teleport {
                    at: Spot::from_ints(MID.0, MID.1),
                })
            }
            _ => None,
        };
        cheat
            .into_iter()
            .map(|cheat| IssuedOrder::hero(Order::Cheat { cheat }))
            .collect()
    });
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::AllVision { on: true });
    let enemy = client
        .view()
        .units
        .iter()
        .find(|unit| unit.hero == Some(HeroId::PUDGE))
        .expect("enemy")
        .id;
    client.until("the enemy's boots visible", |c| {
        c.view().unit(enemy).is_some_and(|unit| {
            unit.items
                .iter()
                .flatten()
                .any(|item| item.id == ItemId::BOOTS)
        })
    });
    let slot = client
        .view()
        .unit(enemy)
        .expect("enemy")
        .items
        .iter()
        .position(|item| item.is_some_and(|item| item.id == ItemId::BOOTS))
        .expect("slot") as u8;
    client.core_mut().act(Intent::SelectUnit(Some(enemy)));
    assert!(!client.core().controls_selection());
    show_from_menu(&mut client, slot, ItemId::BOOTS);
}

#[test]
fn inventory_swaps_and_quickbuy_drops_do_not_select_an_item_in_the_shop() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--shop", "basics"]);
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Item {
        item: ItemId::BOOTS,
    });
    client.cheat(Cheat::Item {
        item: ItemId::CLARITY,
    });
    point_item(&mut client, ItemId::IRON_BRANCH, Place::Grid);
    client.click(MouseButton::Left);
    let from = client.slot_of(ItemId::BOOTS).expect("boots");
    let to = client.slot_of(ItemId::CLARITY).expect("clarity");
    let target = client.item_box(to).expect("target slot");
    let target = client.centre_of(target).expect("laid out");
    server.frozen(|| {
        point_slot(&mut client, from);
        client.button(MouseButton::Left, bevy::input::ButtonState::Pressed);
        client.mouse(target);
        client.button(MouseButton::Left, bevy::input::ButtonState::Released);
        assert_eq!(
            client
                .world()
                .resource::<crate::drag::Settling>()
                .item_in(client.core(), from)
                .expect("swapped preview")
                .id,
            ItemId::CLARITY
        );
        assert_eq!(
            client.world().resource::<crate::shop::Chosen>().0,
            Some(ItemId::IRON_BRANCH.0),
            "an inventory swap must not select its source in the shop"
        );
        client.core_mut().set_quick_buy(ItemId::BOOTS.0, false);
        point_item(&mut client, ItemId::BOOTS, Place::Quick);
        let world = client.world_mut();
        let end = world
            .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
            .single(world)
            .expect("window")
            .cursor_position()
            .expect("pointer");
        point_slot(&mut client, from);
        let before = client.core().seq();
        client.button(MouseButton::Left, bevy::input::ButtonState::Pressed);
        client.mouse(end);
        client.button(MouseButton::Left, bevy::input::ButtonState::Released);
        assert_eq!(client.core().quick_buy.targets(), &[ItemId::CLARITY.0]);
        assert_eq!(
            client.world().resource::<crate::shop::Chosen>().0,
            Some(ItemId::IRON_BRANCH.0),
            "Quickbuy planning must not choose the dragged inventory item"
        );
        assert_eq!(client.core().seq(), before);
    });
}
