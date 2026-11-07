//! Return Items keeps combine-locked cargo aboard; see Valve's 7.23c notes.
//! Delivery of locked components is allowed again since June 24, 2025.

use bevy::{input::ButtonState, prelude::*};
use bota_client_core::{BAG_SLOTS, Intent, Slot};
use bota_proto::{Cheat, CreepSide, EntityId, HeroId, ItemId, ItemSlot, Order, Target};

use super::super::harness::{Client, MID, Server};
use crate::hud::{Part, PartOf};

fn cargo(client: &Client, courier: EntityId, item: ItemId) -> Option<u8> {
    client
        .view()
        .unit(courier)
        .expect("the courier")
        .items
        .iter()
        .position(|held| held.is_some_and(|held| held.id == item))
        .map(|slot| slot as u8)
}

fn stash_has(client: &Client, item: ItemId) -> bool {
    client
        .view()
        .player(client.core().my_slot.expect("seated"))
        .expect("own row")
        .stash
        .as_deref()
        .expect("own stash")
        .iter()
        .flatten()
        .any(|held| held.id == item)
}

fn locked_icon_shown(client: &mut Client, slot: u8) -> bool {
    let Some(item_box) = client.item_box(slot) else {
        return false;
    };
    let world = client.world_mut();
    world
        .query::<(&Part, &PartOf, &InheritedVisibility)>()
        .iter(world)
        .any(|(part, owner, visible)| *part == Part::Lock && owner.0 == item_box && visible.get())
}

fn loaded_courier() -> (Server, Client, EntityId, bota_proto::Vec2) {
    let server = Server::start(1, &["--mode", "lockstep"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::StartGame);
    let courier = client.core().my_courier().expect("own courier");
    let home = client.view().unit(courier).expect("the courier").pos;
    client.stand_at(MID);
    client.core_mut().act(Intent::Stop);
    client.core_mut().act(Intent::SelectCourier);
    client.cheat(Cheat::Teleport {
        at: client.me().pos + bota_proto::Vec2::from_ints(100, 0),
    });
    client.core_mut().act(Intent::SelectHero);
    for item in [ItemId::BOOTS, ItemId::CLARITY] {
        client.cheat(Cheat::Item { item });
        let slot = client.slot_of(item).expect("granted item");
        client.core_mut().send_order_to(
            None,
            Order::Put {
                slot: ItemSlot(slot),
                target: Target::Unit(courier),
            },
        );
        client.until("the item loaded", |c| cargo(c, courier, item).is_some());
    }
    let slot = cargo(&client, courier, ItemId::BOOTS).expect("boots aboard");
    client.core_mut().send_order_to(
        Some(courier),
        Order::SetCombineLock {
            slot: ItemSlot(slot),
            locked: true,
        },
    );
    client.until("boots locked on the courier", |c| {
        c.view().unit(courier).expect("the courier").items[slot as usize]
            .is_some_and(|held| held.locked == Some(true))
    });
    client.core_mut().act(Intent::SelectCourier);
    client.cheat(Cheat::Teleport { at: home });
    (server, client, courier, home)
}

#[test]
fn return_items_keeps_the_locked_item_and_its_padlock_on_the_courier() {
    let (server, mut client, courier, home) = loaded_courier();
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(1),
        ctrl: false,
    });
    client.until("the unlocked Clarity in the stash", |c| {
        stash_has(c, ItemId::CLARITY)
    });
    assert!(
        !stash_has(&client, ItemId::BOOTS),
        "Return Items must keep locked cargo aboard"
    );
    let boots = cargo(&client, courier, ItemId::BOOTS).expect("locked boots stay aboard");
    client.until("the courier home with its lock still drawn", |c| {
        c.view().unit(courier).expect("the courier").pos == home && locked_icon_shown(c, boots)
    });
    server.frozen(|| {
        client.frames(3);
        assert!(
            locked_icon_shown(&mut client, boots),
            "the selected courier still draws the padlock"
        );
        assert!(cargo(&client, courier, ItemId::CLARITY).is_none());
        assert!(
            client.view().unit(courier).expect("the courier").items[boots as usize]
                .is_some_and(|held| held.locked == Some(true))
        );
    });
}

#[test]
fn a_locked_courier_item_can_be_dragged_into_the_stash_at_the_shop() {
    let (_server, mut client, courier, _) = loaded_courier();
    let from = cargo(&client, courier, ItemId::BOOTS).expect("locked boots aboard");
    client.until("the courier lock drawn before its manual move", |c| {
        locked_icon_shown(c, from)
    });
    let from_box = client.item_box(from).expect("courier slot");
    let start = client.centre_of(from_box).expect("courier slot laid out");
    let stash_box = client.item_box(BAG_SLOTS).expect("stash slot");
    let target = client.centre_of(stash_box).expect("stash slot laid out");
    client.mouse(start);
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse(start + Vec2::new(0.0, -60.0));
    client.mouse(target);
    client.button(MouseButton::Left, ButtonState::Released);
    client.until(
        "the server acknowledges the locked Boots in the stash",
        |c| stash_has(c, ItemId::BOOTS) && cargo(c, courier, ItemId::BOOTS).is_none(),
    );
    assert!(
        cargo(&client, courier, ItemId::CLARITY).is_some(),
        "a manual drag moves only its chosen stack"
    );
    assert!(
        client
            .view()
            .player(client.core().my_slot.expect("seated"))
            .expect("own row")
            .stash
            .as_deref()
            .expect("stash")[0]
            .is_some_and(|held| held.id == ItemId::BOOTS && held.locked == Some(true))
    );
    client.until(
        "the manual move keeps its padlock drawn in the stash",
        |c| locked_icon_shown(c, BAG_SLOTS),
    );
}
