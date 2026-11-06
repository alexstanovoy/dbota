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
    let scale = window_scale(client);
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
                .map(|t| (ItemId(c.0), t.translation / scale))
        })
        .collect();
    found.sort_by(|a, b| a.1.y.total_cmp(&b.1.y).then(a.1.x.total_cmp(&b.1.x)));
    found
}

fn window_scale(client: &mut Client) -> f32 {
    let world = client.world_mut();
    world
        .query::<&Window>()
        .single(world)
        .expect("window")
        .scale_factor()
}

fn drag_icon_at(client: &mut Client) -> Option<Vec2> {
    let scale = window_scale(client);
    let world = client.world_mut();
    world
        .query::<(
            &ImageNode,
            &Visibility,
            &GlobalZIndex,
            &bevy::ui::UiGlobalTransform,
        )>()
        .iter(world)
        .find(|(image, shown, z, _)| {
            z.0 == 40 && **shown != Visibility::Hidden && image.image != Handle::default()
        })
        .map(|(_, _, _, placed)| placed.translation / scale)
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
    client.until("the pointer on the visible shop item", |c| {
        if let Some((_, at)) = cells(c, place).into_iter().find(|(id, _)| *id == item) {
            c.mouse(at);
        }
        let world = c.world_mut();
        world
            .query::<(&ShopCell, &Place, &Interaction)>()
            .iter(world)
            .any(|(cell, at, over)| cell.0 == item.0 && *at == place && *over != Interaction::None)
    });
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
    client.until("the refused append is visible", |c| {
        crate::error_msg::shown(c.world_mut()).as_deref() == Some("Quickbuy is full")
    });
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
fn quick_drop(client: &mut Client) -> Vec2 {
    // Empty hint is part of the actual Quickbuy drop area, alongside the sticky item.
    client.until("the empty Quickbuy hint laid out", |c| {
        let world = c.world_mut();
        world.query::<(Entity, &Text)>().iter(world).any(|(e, t)| {
            t.0.to_lowercase().contains("shift-click")
                && world
                    .get::<ComputedNode>(e)
                    .is_some_and(|n| n.size().x > 0.0)
        })
    });
    let world = client.world_mut();
    let entity = world
        .query::<(Entity, &Text)>()
        .iter(world)
        .find(|(_, t)| t.0.to_lowercase().contains("shift-click"))
        .expect("Quickbuy hint")
        .0;
    client.centre_of(entity).expect("laid out") / window_scale(client)
}

#[test]
fn a_shop_drag_reaches_quickbuy_on_a_display_with_scale_factor_two() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--shop", "basics:/boots"]);
    let world = client.world_mut();
    let id = {
        let (id, mut window) = world
            .query::<(Entity, &mut Window)>()
            .single_mut(world)
            .expect("window");
        window.resolution.set_scale_factor_override(Some(2.0));
        window.resolution.set_physical_resolution(3840, 2160);
        id
    };
    world.write_message(bevy::window::WindowScaleFactorChanged {
        window: id,
        scale_factor: 2.0,
    });
    world.write_message(bevy::window::WindowResized {
        window: id,
        width: 1920.0,
        height: 1080.0,
    });
    point_item(&mut client, ItemId::BOOTS, Place::Grid);
    let end = quick_drop(&mut client);
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse(end);
    client.until("the scaled drag icon at the cursor", |c| {
        drag_icon_at(c).is_some_and(|at| at.distance(end) < 1.0)
    });
    client.button(MouseButton::Left, ButtonState::Released);
    assert_eq!(
        client.core().quick_buy.targets(),
        &[ItemId::BOOTS.0],
        "a logical-pixel cursor must hit Quickbuy at scale factor two"
    );
    client.until("the planned boots drawn", |c| {
        cells(c, Place::Quick).len() == 1
    });
}

#[test]
fn a_double_click_with_two_pixels_of_jitter_buys_once() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--shop", "basics:/boots"]);
    client.cheat(Cheat::Gold { amount: 10000 });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(5),
        ));
    point_item(&mut client, ItemId::BOOTS, Place::Grid);
    client.click(MouseButton::Left);
    let start = cells(&mut client, Place::Grid)
        .into_iter()
        .find(|(id, _)| *id == ItemId::BOOTS)
        .expect("boots")
        .1;
    let gold = client.core().spendable_gold();
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse(start + Vec2::new(2.0, 0.0));
    client.button(MouseButton::Left, ButtonState::Released);
    client.until("exactly one purchase processed", |c| {
        owned(c, ItemId::BOOTS) == 1 && c.core().source.pending_orders() == 0
    });
    assert_eq!(owned(&client, ItemId::BOOTS), 1);
    assert_eq!(
        client.core().spendable_gold(),
        gold - bota_core::item_def(ItemId::BOOTS).expect("boots").cost
    );
}

#[test]
fn shop_items_drag_into_quickbuy_without_purchase_and_control_drag_appends() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--shop", "basics:/boots"]);
    client.cheat(Cheat::Gold { amount: 10000 });
    let end = quick_drop(&mut client);
    let gold = client.core().spendable_gold();
    point_item(&mut client, ItemId::BOOTS, Place::Grid);
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse(end);
    client.until("the dragged shop icon at the cursor", |c| {
        drag_icon_at(c).is_some_and(|at| at.distance(end) < 1.0)
    });
    assert!(
        drag_icon_at(&mut client)
            .expect("drawn ghost")
            .distance(end)
            < 1.0
    );
    client.button(MouseButton::Left, ButtonState::Released);
    client.until("the shop drop's planned cell drawn", |c| {
        cells(c, Place::Quick).len() == 1
    });
    assert_eq!(
        client.core().quick_buy.targets(),
        &[ItemId::BOOTS.0],
        "shop drag queues the build"
    );
    assert_eq!(
        client.core().spendable_gold(),
        gold,
        "planning does not purchase"
    );
    let end = cells(&mut client, Place::Quick)[0].1;
    point_item(&mut client, ItemId::BOOTS, Place::Grid);
    client.click(MouseButton::Left);
    point_item(&mut client, ItemId::BOOTS, Place::Tree);
    client.key(KeyCode::ControlLeft, ButtonState::Pressed);
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse(end);
    client.button(MouseButton::Left, ButtonState::Released);
    client.key(KeyCode::ControlLeft, ButtonState::Released);
    client.until("the appended copy drawn", |c| {
        cells(c, Place::Quick).len() == 2
    });
    assert_eq!(
        client.core().quick_buy.targets(),
        &[ItemId::BOOTS.0, ItemId::BOOTS.0]
    );
    assert_eq!(
        client.core().spendable_gold(),
        gold,
        "a second click dragged away must not double-click purchase"
    );
    point_item(&mut client, ItemId::BOOTS, Place::Grid);
    client.click(MouseButton::Right);
    client.until("plain right-click buys the same affordable item", |c| {
        owned(c, ItemId::BOOTS) == 1 && c.core().source.pending_orders() == 0
    });
    assert_eq!(
        client.core().spendable_gold(),
        gold - bota_core::item_def(ItemId::BOOTS).expect("boots").cost
    );
}

#[test]
fn inventory_items_drag_into_quickbuy_without_moving_the_owned_item() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.core_mut().act(Intent::SelectHero);
    client.core_mut().act(Intent::Buy(ItemId::BOOTS.0));
    client.until("owned boots", |c| c.slot_of(ItemId::BOOTS).is_some());
    let slot = client.slot_of(ItemId::BOOTS).expect("owned boots");
    let end = quick_drop(&mut client);
    let entity = client.item_box(slot).expect("inventory slot");
    let start = client.centre_of(entity).expect("laid out");
    client.mouse(start);
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse(end);
    client.button(MouseButton::Left, ButtonState::Released);
    client.until("the owned-item drop's planned cell drawn", |c| {
        cells(c, Place::Quick).len() == 1
    });
    assert_eq!(
        client.core().quick_buy.targets(),
        &[ItemId::BOOTS.0],
        "owned item can plan another copy"
    );
    assert_eq!(
        cells(&mut client, Place::Quick).len(),
        1,
        "the planned copy is drawn"
    );
    assert_eq!(client.slot_of(ItemId::BOOTS), Some(slot));
    assert_eq!(client.core().held_item, None);
}

#[test]
fn an_inventory_quickbuy_drop_is_consumed_before_world_release_with_a_stale_hud_block() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(super::harness::MID);
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Item {
        item: ItemId::BOOTS,
    });
    let slot = client.slot_of(ItemId::BOOTS).expect("owned boots");
    let end = quick_drop(&mut client);
    client.app.add_systems(
        Update,
        (|mouse: Res<ButtonInput<MouseButton>>, mut over: ResMut<crate::input::OverHud>| {
            // The current drop hit must consume the release even if the earlier HUD-wide
            // snapshot has not caught up. The real input and server order path still runs.
            if mouse.just_released(MouseButton::Left) {
                over.0 = false;
            }
        })
        .before(crate::input::mouse),
    );
    let entity = client.item_box(slot).expect("slot");
    let start = client.centre_of(entity).expect("laid out");
    client.mouse(start);
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse(end);
    client.button(MouseButton::Left, ButtonState::Released);
    assert_eq!(
        client.core().quick_buy.targets(),
        &[ItemId::BOOTS.0],
        "the drop must be consumed before a world PutAt can take it"
    );
    client.until("the planned boots drawn", |c| {
        cells(c, Place::Quick).len() == 1
    });
    assert_eq!(client.slot_of(ItemId::BOOTS), Some(slot));
    assert!(
        client
            .view()
            .loot
            .iter()
            .all(|loot| loot.item != ItemId::BOOTS)
    );
}
