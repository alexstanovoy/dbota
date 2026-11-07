//! The preparation storefront follows the install's dedicated starter categories.
use super::harness::{Client, Server};
use crate::shop::{Place, ShopCell};
use bevy::prelude::*;
use bota_proto::{HeroId, ItemId};

fn starting_ids(client: &mut Client) -> Vec<u16> {
    let world = client.world_mut();
    let mut ids: Vec<_> = world
        .query::<(
            Entity,
            &ShopCell,
            &Place,
            &InheritedVisibility,
            &ComputedNode,
        )>()
        .iter(world)
        .filter(|(_, _, place, seen, node)| {
            **place == Place::Grid && seen.get() && node.size() != Vec2::ZERO
        })
        .filter(|(entity, ..)| {
            let mut ancestor = Some(*entity);
            for _ in 0..32 {
                let Some(entity) = ancestor else { return false };
                if world.get::<crate::strategy::Screen>(entity).is_some() {
                    return true;
                }
                ancestor = world.get::<ChildOf>(entity).map(ChildOf::parent);
            }
            false
        })
        .map(|(_, item, ..)| item.0)
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn preparation_draws_only_the_installs_authored_starter_items() {
    let server = Server::start(1, &["--tactical-secs", "120"]);
    let mut client = Client::pick(&server, HeroId::SHADOW_FIEND, &[]);
    assert!(client.core().in_tactical_phase());
    client.until("the preparation storefront laid out", |c| {
        !starting_ids(c).is_empty()
    });
    let manifest = client.world().resource::<crate::manifest::Manifest>();
    let data: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest.root.join("text/shop.json")).expect("native shop data"),
    )
    .expect("shop JSON");
    let mut expected = Vec::new();
    for section in data["shops"]
        .as_array()
        .expect("native categories")
        .iter()
        .filter(|row| {
            row[0].as_str().is_some_and(|key| {
                [
                    "pregame_consumables",
                    "pregame_attributes",
                    "pregame_accessories",
                ]
                .contains(&key)
            })
        })
    {
        for name in section[1]
            .as_array()
            .expect("native items")
            .iter()
            .filter_map(|name| name.as_str())
            .filter_map(|name| name.strip_prefix("item_"))
        {
            if let Some((_, id, _)) = manifest
                .ids
                .iter()
                .find(|(kind, _, key)| kind == "item" && key == name)
                && client.core().offers(*id)
            {
                expected.push(*id);
            }
        }
    }
    expected.sort_unstable();
    expected.dedup();
    assert!(
        !expected.is_empty(),
        "the native starter-list oracle is populated"
    );
    let shown = starting_ids(&mut client);
    assert!(
        !shown.contains(&ItemId::OGRE_AXE.0),
        "unaffordable live-shop items do not belong in the starter list"
    );
    assert_eq!(
        shown, expected,
        "preparation must use authored pregame categories rather than the live shop's categories"
    );
    assert!(
        client
            .world()
            .resource::<crate::shop::Catalog>()
            .sections
            .iter()
            .any(|s| s.items.contains(&ItemId::OGRE_AXE.0)),
        "the live shop keeps its complete catalogue"
    );
}

fn preparation_slot(client: &mut Client, item: ItemId) -> (u8, Vec2) {
    let slot = (0..bota_client_core::BAG_SLOTS)
        .find(|slot| {
            client
                .world()
                .resource::<crate::bag::TacticalBag>()
                .id_at(*slot)
                == Some(item.0)
        })
        .expect("prepared item");
    let world = client.world_mut();
    let entity = world
        .query::<(
            Entity,
            &crate::hud::ItemBox,
            &bevy::ui::UiGlobalTransform,
            &ComputedNode,
        )>()
        .iter(world)
        .find(|(entity, shown, _, node)| {
            if shown.0 != slot || node.size() == Vec2::ZERO {
                return false;
            }
            let mut ancestor = Some(*entity);
            for _ in 0..32 {
                let Some(entity) = ancestor else { return false };
                if world.get::<crate::strategy::Screen>(entity).is_some() {
                    return true;
                }
                ancestor = world.get::<ChildOf>(entity).map(ChildOf::parent);
            }
            false
        })
        .map(|(entity, ..)| entity)
        .expect("the preparation slot laid out");
    client.hover_ui(entity, std::time::Duration::from_secs(20));
    let at = client
        .centre_of(entity)
        .expect("the preparation slot under the pointer");
    (slot, at)
}

#[test]
fn preparation_right_click_refunds_immediately_without_a_menu_or_confirmation() {
    use bota_client_core::Intent;
    let server = Server::start(1, &["--tactical-secs", "120"]);
    let mut client = Client::pick(&server, HeroId::SHADOW_FIEND, &[]);
    client.core_mut().act(Intent::Buy(ItemId::CIRCLET.0));
    client.until("the circlet prepared", |c| {
        c.world().resource::<crate::bag::TacticalBag>().id_at(0) == Some(ItemId::CIRCLET.0)
    });
    let unspent = client.core().gold_after_queue();
    for _ in 0..3 {
        super::shop_queue::point_item(&mut client, ItemId::SLIPPERS, Place::Grid);
        client.click(MouseButton::Right);
        client.until("slippers in the preparation inventory", |c| {
            c.core().queued_buys.contains(&ItemId::SLIPPERS.0)
                && (0..bota_client_core::BAG_SLOTS).any(|slot| {
                    c.world().resource::<crate::bag::TacticalBag>().id_at(slot)
                        == Some(ItemId::SLIPPERS.0)
                })
        });
        assert_eq!(
            client.core().gold_after_queue(),
            unspent
                - bota_core::item_def(ItemId::SLIPPERS)
                    .expect("slippers")
                    .cost
        );
        let (_, at) = preparation_slot(&mut client, ItemId::SLIPPERS);
        client.mouse(at);
        let before = client.core().seq();
        client.click(MouseButton::Right);
        assert!(
            !client.core().queued_buys.contains(&ItemId::SLIPPERS.0),
            "preparation RMB must refund the item without requiring Sell in a menu"
        );
        assert_eq!(
            client.core().gold_after_queue(),
            unspent,
            "the full purchase price returns"
        );
        assert!(
            !client
                .world()
                .resource::<crate::item_menu::Menu>()
                .is_open()
        );
        assert_eq!(
            client.core().seq(),
            before,
            "preparation refunds stay local to the pending purchases"
        );
        client.until("the refunded inventory slot clears", |c| {
            (0..bota_client_core::BAG_SLOTS).all(|slot| {
                c.world().resource::<crate::bag::TacticalBag>().id_at(slot)
                    != Some(ItemId::SLIPPERS.0)
            })
        });
    }
    assert_eq!(client.core().queued_buys, &[ItemId::CIRCLET.0]);
}

#[test]
fn live_inventory_right_click_keeps_the_context_menu_instead_of_selling() {
    use bota_client_core::Intent;
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.core_mut().act(Intent::SelectHero);
    client.core_mut().act(Intent::Buy(ItemId::SLIPPERS.0));
    client.until("slippers bought", |c| c.slot_of(ItemId::SLIPPERS).is_some());
    let slot = client.slot_of(ItemId::SLIPPERS).expect("slippers");
    let entity = client.item_box(slot).expect("slot");
    client.hover_ui(entity, std::time::Duration::from_secs(20));
    let before = client.core().seq();
    let gold = client.core().spendable_gold();
    client.click(MouseButton::Right);
    assert!(
        client
            .world()
            .resource::<crate::item_menu::Menu>()
            .is_open()
    );
    assert_eq!(client.slot_of(ItemId::SLIPPERS), Some(slot));
    assert_eq!(client.core().spendable_gold(), gold);
    assert_eq!(client.core().seq(), before);
}

#[test]
fn preparation_right_click_refunds_the_whole_consumable_stack() {
    use bota_client_core::Intent;
    let server = Server::start(1, &["--tactical-secs", "120"]);
    let mut client = Client::pick(&server, HeroId::SHADOW_FIEND, &[]);
    for item in [ItemId::CIRCLET, ItemId::CLARITY, ItemId::CLARITY] {
        client.core_mut().act(Intent::Buy(item.0));
    }
    client.until("the clarity stack prepared", |c| {
        (0..bota_client_core::BAG_SLOTS).any(|slot| {
            c.world()
                .resource::<crate::bag::TacticalBag>()
                .item(slot, bota_proto::SlotId(0))
                .is_some_and(|item| item.id == ItemId::CLARITY && item.charges == Some(2))
        })
    });
    let (_, at) = preparation_slot(&mut client, ItemId::CLARITY);
    client.mouse(at);
    let before = client.core().seq();
    client.click(MouseButton::Right);
    assert_eq!(
        client.core().queued_buys,
        &[ItemId::CIRCLET.0],
        "selling a displayed stack refunds every purchase in that stack"
    );
    assert_eq!(
        client.core().gold_after_queue(),
        bota_core::rules::STARTING_GOLD
            - bota_core::item_def(ItemId::CIRCLET).expect("circlet").cost
    );
    assert!(
        !client
            .world()
            .resource::<crate::item_menu::Menu>()
            .is_open()
    );
    assert_eq!(client.core().seq(), before);
    client.until("the clarity stack disappears", |c| {
        (0..bota_client_core::BAG_SLOTS).all(|slot| {
            c.world().resource::<crate::bag::TacticalBag>().id_at(slot) != Some(ItemId::CLARITY.0)
        })
    });
}

#[test]
fn preparation_right_click_refunds_both_ward_kinds_in_a_dispenser() {
    use bota_client_core::Intent;
    let server = Server::start(1, &["--tactical-secs", "120"]);
    let mut client = Client::pick(&server, HeroId::SHADOW_FIEND, &[]);
    for item in [ItemId::CIRCLET, ItemId::OBSERVER_WARD, ItemId::SENTRY_WARD] {
        client.core_mut().act(Intent::Buy(item.0));
    }
    client.until("the dispenser prepared", |c| {
        (0..bota_client_core::BAG_SLOTS).any(|slot| {
            c.world().resource::<crate::bag::TacticalBag>().id_at(slot)
                == Some(ItemId::WARD_DISPENSER.0)
        })
    });
    let (_, at) = preparation_slot(&mut client, ItemId::WARD_DISPENSER);
    client.mouse(at);
    let before = client.core().seq();
    client.click(MouseButton::Right);
    assert_eq!(
        client.core().queued_buys,
        &[ItemId::CIRCLET.0],
        "the combined inventory id refunds the two purchased ward kinds"
    );
    assert_eq!(
        client.core().gold_after_queue(),
        bota_core::rules::STARTING_GOLD
            - bota_core::item_def(ItemId::CIRCLET).expect("circlet").cost
    );
    assert!(
        !client
            .world()
            .resource::<crate::item_menu::Menu>()
            .is_open()
    );
    assert_eq!(client.core().seq(), before);
    client.until("the dispenser disappears", |c| {
        (0..bota_client_core::BAG_SLOTS).all(|slot| {
            c.world().resource::<crate::bag::TacticalBag>().id_at(slot)
                != Some(ItemId::WARD_DISPENSER.0)
        })
    });
}
