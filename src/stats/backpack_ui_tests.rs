//! Owner-path reproduction: actual inventory mouse drags, authoritative
//! attributes and the displayed HUD followed for twelve seconds after return.

use super::*;
use crate::e2e::harness::{Client, MID, Server};
use bevy::input::ButtonState;
use bota_client_core::Intent;
use bota_proto::{Cheat, CreepSide, HeroId, ItemId};
use std::time::Duration;

fn attributes_of(client: &Client) -> [i32; 3] {
    let a = client.me().attributes;
    [a.strength, a.agility, a.intelligence].map(Fixed::to_int)
}

fn displayed_attributes(client: &mut Client) -> Option<[String; 3]> {
    let world = client.world_mut();
    let mut values: [Option<String>; 3] = [None, None, None];
    for (line, text, node, visible) in world
        .query::<(&Line, &Text, &Node, &InheritedVisibility)>()
        .iter(world)
    {
        if let Line::Attribute(index, Shown::Sum) = *line
            && node.display != Display::None
            && visible.get()
        {
            values[usize::from(index)] = Some(text.0.clone());
        }
    }
    Some([values[0].take()?, values[1].take()?, values[2].take()?])
}

fn drag(client: &mut Client, from: u8, to: u8) {
    let centre = |c: &mut Client, slot| c.item_box(slot).and_then(|b| c.centre_of(b));
    client.until("both inventory drag endpoints laid out", |c| {
        centre(c, from).is_some() && centre(c, to).is_some()
    });
    let start = centre(client, from).expect("source box");
    let end = centre(client, to).expect("destination box");
    client.mouse(start);
    client.button(MouseButton::Left, ButtonState::Pressed);
    for n in 1..=4 {
        client.mouse(start + (end - start) * n as f32 / 4.0 + Vec2::new(0.0, -60.0));
    }
    client.mouse(end);
    client.button(MouseButton::Left, ButtonState::Released);
}

fn quick_drag(client: &mut Client, from: u8, to: u8) {
    let start = client
        .item_box(from)
        .and_then(|b| client.centre_of(b))
        .expect("source slot");
    let end = client
        .item_box(to)
        .and_then(|b| client.centre_of(b))
        .expect("destination slot");
    client.mouse(start);
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse(end);
    client.button(MouseButton::Left, ButtonState::Released);
}

fn icon(client: &mut Client, slot: u8) -> Option<String> {
    let item_box = client.item_box(slot)?;
    let world = client.world_mut();
    let server = world.resource::<AssetServer>().clone();
    world
        .query::<(&crate::hud::Part, &crate::hud::PartOf, &ImageNode)>()
        .iter(world)
        .find(|(part, of, _)| of.0 == item_box && matches!(part, crate::hud::Part::Icon))
        .and_then(|(_, _, image)| server.get_path(image.image.id()))
        .map(|path| path.to_string())
}

fn scene(item: ItemId) -> (Server, Client, [i32; 3], [i32; 3]) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_nanos(33_333_333),
        ));
    client.core_mut().act(Intent::Stop);
    client.frames(3);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    let base = attributes_of(&client);
    client.cheat(Cheat::Item { item });
    client.until("the item really contributes its stats", |c| {
        attributes_of(c) != base
    });
    let full = attributes_of(&client);
    client.until("the HUD shows the equipped attributes", |c| {
        displayed_attributes(c) == Some(full.map(|n| n.to_string()))
    });
    (server, client, base, full)
}

fn trace_after_return(client: &mut Client, item: ItemId, slot: u8, expected: [i32; 3]) {
    let returned = client.tick();
    client.within(
        Duration::from_secs(60),
        "twelve server seconds after the UI return",
        |c| {
            let elapsed = c.tick().saturating_sub(returned);
            let server = attributes_of(c);
            let hud = displayed_attributes(c);
            let mute = c.me().items[usize::from(slot)].map(|held| held.mute_left);
            if elapsed < 360 {
                return false;
            }
            assert_eq!(
                c.slot_of(item),
                Some(slot),
                "the UI really returned the item to inventory"
            );
            assert_eq!(mute, Some(0), "the authoritative inactivity timer expired");
            assert_eq!(
                server, expected,
                "server stats must not stay lost for ten seconds"
            );
            assert_eq!(
                hud,
                Some(expected.map(|n| n.to_string())),
                "HUD stats must not stay stale after the timer ends"
            );
            true
        },
    );
}

#[test]
fn ui_drag_power_treads_to_backpack_and_back_restores_server_and_hud_for_twelve_seconds() {
    let (_server, mut client, base, full) = scene(ItemId::POWER_TREADS);
    let from = client
        .slot_of(ItemId::POWER_TREADS)
        .expect("equipped Treads");
    drag(&mut client, from, 6);
    client.until("Treads are authoritatively in the backpack", |c| {
        c.slot_of(ItemId::POWER_TREADS) == Some(6)
    });
    assert_eq!(
        attributes_of(&client),
        base,
        "backpack removes the item's attributes"
    );
    drag(&mut client, 6, from);
    client.until("Treads returned by the UI swap", |c| {
        c.slot_of(ItemId::POWER_TREADS) == Some(from)
    });
    trace_after_return(&mut client, ItemId::POWER_TREADS, from, full);
}

#[test]
fn ui_drag_passive_item_into_an_occupied_inventory_slot_restores_server_and_hud_for_twelve_seconds()
{
    let (_server, mut client, _, full) = scene(ItemId::CIRCLET);
    client.cheat(Cheat::Item {
        item: ItemId::CLARITY,
    });
    let from = client.slot_of(ItemId::CIRCLET).expect("equipped Circlet");
    let occupied = client
        .slot_of(ItemId::CLARITY)
        .expect("occupied destination");
    drag(&mut client, from, 6);
    client.until("Circlet in backpack", |c| {
        c.slot_of(ItemId::CIRCLET) == Some(6)
    });
    drag(&mut client, 6, occupied);
    client.until("Circlet returned into the occupied slot", |c| {
        c.slot_of(ItemId::CIRCLET) == Some(occupied)
    });
    assert_eq!(
        client.slot_of(ItemId::CLARITY),
        Some(6),
        "the UI swap displaces the previous stack"
    );
    trace_after_return(&mut client, ItemId::CIRCLET, occupied, full);
}

#[test]
fn ui_drag_back_before_the_first_swap_ack_does_not_leave_the_item_inactive_forever() {
    let (server, mut client, _, full) = scene(ItemId::CIRCLET);
    let from = client.slot_of(ItemId::CIRCLET).expect("equipped Circlet");
    server.frozen(|| {
        // Keep the optimistic preview alive while the real server cannot
        // acknowledge either click sequence, reproducing an ordinary lag window.
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::ZERO,
            ));
        quick_drag(&mut client, from, 6);
        let shown = icon(&mut client, 6).expect("the optimistic backpack icon");
        assert!(
            shown.contains("circlet"),
            "the user really sees the item in the backpack: {shown}"
        );
        assert_eq!(
            client.slot_of(ItemId::CIRCLET),
            Some(from),
            "the first swap is still unacknowledged"
        );
        let pending_before_return = client.core().source.pending_orders();
        quick_drag(&mut client, 6, from);
        assert_eq!(
            client.core().source.pending_orders(),
            pending_before_return + 1,
            "the return drag issues the dependent swap"
        );
    });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_nanos(33_333_333),
        ));
    let before = client.tick();
    client.until("queued mouse orders have reached the server", |c| {
        c.tick() >= before + 3
    });
    trace_after_return(&mut client, ItemId::CIRCLET, from, full);
}

#[test]
fn ui_drag_into_a_third_slot_before_acknowledgement_keeps_the_composed_inventory_pickable() {
    let (server, mut client, _, full) = scene(ItemId::CIRCLET);
    client.cheat(Cheat::Item {
        item: ItemId::CLARITY,
    });
    let from = client.slot_of(ItemId::CIRCLET).expect("equipped Circlet");
    let occupied = client
        .slot_of(ItemId::CLARITY)
        .expect("occupied destination");
    server.frozen(|| {
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::ZERO,
            ));
        quick_drag(&mut client, from, 6);
        quick_drag(&mut client, 6, occupied);
        assert!(
            icon(&mut client, from).is_none(),
            "the first source stays empty across both swaps"
        );
        assert!(
            icon(&mut client, occupied).is_some_and(|p| p.contains("circlet")),
            "the moved stack is shown in the third slot"
        );
        assert!(
            icon(&mut client, 6).is_some_and(|p| p.contains("clarity")),
            "the displaced stack is shown in the backpack"
        );
        quick_drag(&mut client, 6, from);
        assert!(
            icon(&mut client, from).is_some_and(|p| p.contains("clarity")),
            "a third drag can pick the displaced preview up"
        );
        assert!(
            icon(&mut client, 6).is_none(),
            "the composed backpack slot is now empty"
        );
    });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_nanos(33_333_333),
        ));
    client.until("all three mouse swaps acknowledged", |c| {
        c.slot_of(ItemId::CIRCLET) == Some(occupied) && c.slot_of(ItemId::CLARITY) == Some(from)
    });
    trace_after_return(&mut client, ItemId::CIRCLET, occupied, full);
}

#[test]
fn ui_drag_back_after_one_second_without_an_ack_still_returns_the_displayed_item() {
    let (server, mut client, _, full) = scene(ItemId::CIRCLET);
    let from = client.slot_of(ItemId::CIRCLET).expect("equipped Circlet");
    server.frozen(|| {
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_nanos(33_333_333),
            ));
        quick_drag(&mut client, from, 6);
        client.frames(36);
        assert!(
            client.core().source.pending_orders() > 0,
            "the real server acknowledgement is still withheld"
        );
        assert!(
            icon(&mut client, 6).is_some_and(|p| p.contains("circlet")),
            "the displayed backpack item must survive more than the old 0.6 second preview timeout"
        );
        quick_drag(&mut client, 6, from);
    });
    client.until("the delayed UI round trip applied", |c| {
        c.core().source.pending_orders() == 0 && c.tick() > 3
    });
    trace_after_return(&mut client, ItemId::CIRCLET, from, full);
}

#[test]
fn ui_drag_a_dead_heros_items_uses_the_same_owner_for_the_kit_and_preview() {
    let (server, mut client, _, _) = scene(ItemId::CIRCLET);
    client.cheat(Cheat::Invulnerable { on: false });
    let theirs = bota_core::fountain_pos(
        client.core().session().expect("session").map(),
        bota_proto::Team::Dire,
    );
    client.cheat(Cheat::Teleport { at: theirs });
    client.within(
        Duration::from_secs(20),
        "the hero dead but its inventory still shown",
        |c| {
            c.core().my_standing_hero().is_none()
                && c.core().item_in(0).is_some_and(|i| i.id == ItemId::CIRCLET)
        },
    );
    server.frozen(|| {
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::ZERO,
            ));
        quick_drag(&mut client, 0, 6);
        assert!(
            icon(&mut client, 6).is_some_and(|p| p.contains("circlet")),
            "the dead hero's kit uses the same preview owner as input"
        );
        quick_drag(&mut client, 6, 1);
    });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_nanos(33_333_333),
        ));
    client.within(
        Duration::from_secs(10),
        "the dead hero's item swaps really applied",
        |c| c.core().item_in(1).is_some_and(|i| i.id == ItemId::CIRCLET),
    );
    client.frames(2);
    assert!(icon(&mut client, 1).is_some_and(|p| p.contains("circlet")));
    assert!(icon(&mut client, 0).is_none());
}

fn courier_with_item() -> (Server, Client, bota_proto::EntityId) {
    let (server, mut client, _, _) = scene(ItemId::CIRCLET);
    let courier = client.core().my_courier().expect("courier");
    client.core_mut().send_order_to(
        Some(courier),
        bota_proto::Order::Cheat {
            cheat: Cheat::Teleport {
                at: bota_proto::Vec2::from_ints(MID.0, MID.1),
            },
        },
    );
    client.until("courier beside the hero", |c| {
        c.view()
            .unit(courier)
            .is_some_and(|u| u.pos.within(c.me().pos, Fixed::from_int(100)))
    });
    client.core_mut().send_order(bota_proto::Order::Put {
        slot: bota_proto::ItemSlot(0),
        target: bota_proto::Target::Unit(courier),
    });
    client.until("the courier holds the Circlet", |c| {
        c.view()
            .unit(courier)
            .is_some_and(|u| u.items[0].is_some_and(|i| i.id == ItemId::CIRCLET))
    });
    client.core_mut().act(Intent::SelectUnit(Some(courier)));
    let mut previous = None;
    let mut stable = 0;
    client.within(
        Duration::from_secs(5),
        "the selected courier's inventory layout settles",
        |c| {
            let centres: Option<Vec<_>> = [0, 1, 2]
                .into_iter()
                .map(|slot| c.item_box(slot).and_then(|e| c.centre_of(e)))
                .collect();
            stable = if centres.is_some() && centres == previous {
                stable + 1
            } else {
                0
            };
            previous = centres;
            stable >= 3 && icon(c, 0).is_some_and(|p| p.contains("circlet"))
        },
    );
    assert_eq!(
        client.core().commanded(),
        Some(courier),
        "input commands the selected courier"
    );
    assert_eq!(
        crate::hud::subject(client.core()).1.map(|u| u.id),
        Some(courier),
        "the panel is about that same courier"
    );
    assert_eq!(
        client.core().item_in(0).map(|i| i.id),
        Some(ItemId::CIRCLET)
    );
    assert!(
        icon(&mut client, 0).is_some_and(|p| p.contains("circlet")),
        "the courier's source icon is drawn before the drag"
    );
    (server, client, courier)
}

#[test]
fn ui_drag_a_selected_couriers_items_uses_its_inventory_for_input_and_drawing() {
    let (server, mut client, courier) = courier_with_item();
    server.frozen(|| {
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::ZERO,
            ));
        let before = client.core().seq();
        quick_drag(&mut client, 0, 1);
        assert_eq!(
            client.core().seq(),
            before + 1,
            "the selected courier drag issues its swap: held={:?}, selected={:?}",
            client.core().held_item,
            client.core().selected
        );
        let shown = client
            .world()
            .resource::<crate::drag::Settling>()
            .over(client.core(), 1);
        assert!(
            icon(&mut client, 1).is_some_and(|p| p.contains("circlet")),
            "the selected non-hero owns the displayed preview: commanded={:?}, preview={shown:?}",
            client.core().commanded()
        );
        quick_drag(&mut client, 1, 2);
    });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_nanos(33_333_333),
        ));
    client.until("both courier swaps applied", |c| {
        c.view()
            .unit(courier)
            .is_some_and(|u| u.items[2].is_some_and(|i| i.id == ItemId::CIRCLET))
    });
    client.frames(2);
    assert!(icon(&mut client, 2).is_some_and(|p| p.contains("circlet")));
    assert!(icon(&mut client, 0).is_none());
}

#[test]
fn a_refused_inventory_drag_rolls_its_preview_back_without_waiting_for_a_timeout() {
    let (server, mut client, _, _) = scene(ItemId::CIRCLET);
    let sequence = server.frozen(|| {
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::ZERO,
            ));
        quick_drag(&mut client, 0, bota_client_core::TOWN_PORTAL_SLOT);
        assert!(
            icon(&mut client, bota_client_core::TOWN_PORTAL_SLOT)
                .is_some_and(|p| p.contains("circlet")),
            "the invalid destination was optimistically drawn"
        );
        client.core().seq()
    });
    client.within(
        Duration::from_secs(10),
        "the real server refuses a Circlet in the scroll slot",
        |c| c.core().rejected_orders.contains(&sequence),
    );
    client.frames(2);
    assert_eq!(client.slot_of(ItemId::CIRCLET), Some(0));
    assert!(
        icon(&mut client, 0).is_some_and(|p| p.contains("circlet")),
        "explicit refusal restores the authoritative inventory even when display time is frozen"
    );
    assert!(
        !icon(&mut client, bota_client_core::TOWN_PORTAL_SLOT)
            .is_some_and(|p| p.contains("circlet"))
    );
}
