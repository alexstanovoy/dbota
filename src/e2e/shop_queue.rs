//! Real shop gestures and purchases across a queue containing repeated builds.
use super::harness::{Client, Server};
use crate::shop::{Place, ShopCell};
use bevy::{input::ButtonState, prelude::*};
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ItemId};

fn owned(client: &Client, item: ItemId) -> usize {
    client
        .me()
        .items
        .iter()
        .flatten()
        .filter(|i| i.id == item)
        .count()
}

fn cells(client: &mut Client, place: Place) -> Vec<(ItemId, Vec2)> {
    let world = client.world_mut();
    let mut found: Vec<_> = world
        .query::<(Entity, &ShopCell, &Place, &InheritedVisibility)>()
        .iter(world)
        .filter(|(e, _, p, v)| {
            **p == place
                && v.get()
                && world
                    .get::<ComputedNode>(*e)
                    .is_some_and(|n| n.size().x > 0.0 && n.size().y > 0.0)
        })
        .filter_map(|(e, c, _, _)| {
            world
                .get::<bevy::ui::UiGlobalTransform>(e)
                .map(|t| (ItemId(c.0), t.translation))
        })
        .collect();
    found.sort_by(|a, b| a.1.y.total_cmp(&b.1.y).then(a.1.x.total_cmp(&b.1.x)));
    found
}

fn point_item(client: &mut Client, item: ItemId, place: Place) {
    client.until("the shop item laid out", |c| {
        cells(c, place)
            .iter()
            .any(|(id, p)| *id == item && *p != Vec2::ZERO)
    });
    let at = cells(client, place)
        .into_iter()
        .find(|(id, p)| *id == item && *p != Vec2::ZERO)
        .unwrap()
        .1;
    client.mouse(at);
}

#[test]
fn three_slippers_and_a_circlet_complete_three_queued_wraith_bands() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--shop", "upgrades:/wraith"],
    );
    client.cheat(Cheat::Gold { amount: 10000 });
    for item in [
        ItemId::SLIPPERS,
        ItemId::SLIPPERS,
        ItemId::SLIPPERS,
        ItemId::CIRCLET,
    ] {
        let before = owned(&client, item);
        client.core_mut().act(Intent::Buy(item.0));
        client.until("the requested starting component bought", |c| {
            owned(c, item) > before
        });
    }
    point_item(&mut client, ItemId::WRAITH_BAND, Place::Grid);
    client.key(KeyCode::ControlLeft, ButtonState::Pressed);
    client.key(KeyCode::ShiftLeft, ButtonState::Pressed);
    for _ in 0..3 {
        client.click(MouseButton::Left);
    }
    client.key(KeyCode::ShiftLeft, ButtonState::Released);
    client.key(KeyCode::ControlLeft, ButtonState::Released);
    point_item(&mut client, ItemId::RECIPE_WRAITH_BAND, Place::Quick);
    client.click(MouseButton::Right);
    client.until("the first band assembled", |c| {
        owned(c, ItemId::WRAITH_BAND) == 1
    });
    client.frames(3);
    assert!(
        cells(&mut client, Place::Quick)
            .iter()
            .any(|(id, _)| *id == ItemId::CIRCLET),
        "completing one band must retain the next Circlet and recipe"
    );
    for wanted in [
        ItemId::CIRCLET,
        ItemId::RECIPE_WRAITH_BAND,
        ItemId::CIRCLET,
        ItemId::RECIPE_WRAITH_BAND,
    ] {
        point_item(&mut client, wanted, Place::Quick);
        let tick = client.tick();
        client.click(MouseButton::Right);
        client.until("the purchase processed", |c| {
            c.tick() > tick + 2 && c.core().source.pending_orders() == 0
        });
    }
    client.until("all three bands assembled", |c| {
        owned(c, ItemId::WRAITH_BAND) == 3
    });
    assert!(cells(&mut client, Place::Quick).is_empty());
}

#[test]
fn a_full_quickbuy_queue_keeps_the_plan_and_reports_the_refused_append() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--shop", "basics:/boots"]);
    for _ in 0..bota_client_core::MAX_QUICK_BUY_TARGETS {
        client.core_mut().set_quick_buy(ItemId::BOOTS.0, true);
    }
    point_item(&mut client, ItemId::BOOTS, Place::Grid);
    client.key(KeyCode::ControlLeft, ButtonState::Pressed);
    client.key(KeyCode::ShiftLeft, ButtonState::Pressed);
    client.click(MouseButton::Left);
    assert_eq!(
        client.core().quick_buy.targets().len(),
        bota_client_core::MAX_QUICK_BUY_TARGETS
    );
    assert_eq!(
        crate::error_msg::shown(client.world_mut()).as_deref(),
        Some("Quickbuy is full"),
        "a refused append must be visible"
    );
}

#[test]
fn one_quickbuy_keypress_buys_one_component_even_with_gold_for_the_whole_build() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Gold { amount: 10000 });
    client
        .core_mut()
        .set_quick_buy(ItemId::WRAITH_BAND.0, false);
    client.frames(3);
    let first = ItemId(client.core().quick_buy_parts()[0]);
    client.key(KeyCode::F5, ButtonState::Pressed);
    client.key(KeyCode::F5, ButtonState::Released);
    client.within(
        std::time::Duration::from_secs(15),
        "one component purchased",
        |c| owned(c, first) == 1 && c.core().source.pending_orders() == 0,
    );
    client.frames(3);
    assert_eq!(owned(&client, ItemId::WRAITH_BAND), 0);
    assert_eq!(client.core().quick_buy_parts().len(), 2);
    assert_eq!(
        cells(&mut client, Place::Quick).len(),
        2,
        "only the purchased component leaves the drawn queue"
    );
}

#[test]
fn picking_up_a_queued_whole_item_completes_one_requested_copy() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Item {
        item: ItemId::BOOTS,
    });
    let slot = client.slot_of(ItemId::BOOTS).expect("owned boots");
    let pos = client.me().pos;
    let at = (pos.x.to_f32() + 150.0, pos.y.to_f32());
    client.core_mut().act(Intent::PutAt {
        slot,
        at,
        unit: None,
    });
    client.within(
        std::time::Duration::from_secs(15),
        "boots on the ground",
        |c| {
            c.view().loot.iter().any(|loot| loot.item == ItemId::BOOTS)
                && c.slot_of(ItemId::BOOTS).is_none()
        },
    );
    let loot = client
        .view()
        .loot
        .iter()
        .find(|loot| loot.item == ItemId::BOOTS)
        .expect("dropped")
        .id;
    client.core_mut().set_quick_buy(ItemId::BOOTS.0, false);
    client.core_mut().set_quick_buy(ItemId::BOOTS.0, true);
    client.frames(3);
    assert_eq!(cells(&mut client, Place::Quick).len(), 2);
    assert!(
        client
            .core()
            .lying_at(at)
            .is_some_and(|found| found.id == loot)
    );
    client.core_mut().act(Intent::Smart { at, unit: None });
    client.within(std::time::Duration::from_secs(15), "boots picked up", |c| {
        c.slot_of(ItemId::BOOTS).is_some()
    });
    client.frames(3);
    assert_eq!(client.core().quick_buy.targets(), &[ItemId::BOOTS.0]);
    assert_eq!(
        cells(&mut client, Place::Quick).len(),
        1,
        "an arrival satisfies exactly one queued copy"
    );
}
