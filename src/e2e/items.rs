//! Items in the bag as Dota handles them: Dota's item menu on a right
//! click, and a dragged item's icon at the cursor.

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::BAG_SLOTS;
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, Server};

/// A hero at its fountain with a tango in its bag, and the tango's slot.
fn with_a_tango() -> (Server, Client, u8) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Item {
        item: ItemId::TANGO,
    });
    let slot = client.slot_of(ItemId::TANGO).expect("the tango in the bag");
    assert!(slot < BAG_SLOTS);
    let patience = std::time::Duration::from_secs(20);
    client.within(patience, "the slot's box laid out", |c| {
        c.item_box(slot)
            .and_then(|b| c.centre_of(b))
            .is_some_and(|at| at != Vec2::ZERO)
    });
    (server, client, slot)
}

/// The button whose text shows `word`, shown.
fn button_saying(client: &mut Client, word: &str) -> Option<Entity> {
    let world = client.world_mut();
    let label = world
        .query::<(&Text, &ChildOf, &InheritedVisibility)>()
        .iter(world)
        .find(|(text, _, shown)| text.0 == word && shown.get())
        .map(|(_, parent, _)| parent.parent())?;
    world.get::<Button>(label).map(|_| label)
}

/// A right click on an item in the bag at the fountain opens Dota's item menu, its choices in
/// the install's words: Sell and Move to Stash; Move to Stash puts the item in the stash (the
/// menu was not there; a choice clicked closed it before it was read).
#[test]
fn the_item_menu_offers_sell_and_moves_an_item_to_the_stash() {
    let (_server, mut client, slot) = with_a_tango();
    let texts = client.world().resource::<crate::dota_text::Texts>();
    let label = |key| texts.label(key, &[]).expect("the install's word");
    let (sell, to_stash) = (
        label("DOTA_InventoryMenu_Sell"),
        label("DOTA_InventoryMenu_MoveToStash"),
    );
    let item_box = client.item_box(slot).expect("its box");
    let on = client.centre_of(item_box).expect("laid out");
    client.mouse(on);
    client.click(MouseButton::Right);
    let patience = std::time::Duration::from_secs(10);
    client.within(patience, "the item menu", |c| {
        button_saying(c, &sell).is_some() && button_saying(c, &to_stash).is_some()
    });
    let choice = button_saying(&mut client, &to_stash).expect("Move to Stash");
    client.frames(2);
    let on = client.centre_of(choice).expect("laid out");
    client.mouse(on);
    client.click(MouseButton::Left);
    client.within(patience, "the tango in the stash", |c| {
        c.slot_of(ItemId::TANGO).is_some_and(|s| s >= BAG_SLOTS)
    });
    assert!(
        button_saying(&mut client, &to_stash).is_none(),
        "the menu closed"
    );
}

/// The dragged item's icon, where it is drawn: its middle, in the window's pixels.
fn ghost(client: &mut Client) -> Option<Vec2> {
    let world = client.world_mut();
    let scale = world.resource::<bevy::ui::UiScale>().0.max(0.1);
    world
        .query::<(&Node, &ImageNode, &Visibility, &GlobalZIndex)>()
        .iter(world)
        .find(|(_, image, shown, z)| {
            z.0 == 40 && **shown != Visibility::Hidden && image.image != Handle::default()
        })
        .and_then(
            |(node, ..)| match (node.left, node.top, node.width, node.height) {
                (Val::Px(l), Val::Px(t), Val::Px(w), Val::Px(h)) => {
                    Some(Vec2::new(l + w / 2.0, t + h / 2.0) * scale)
                }
                _ => None,
            },
        )
}

/// An item taken up from its slot shows its icon at the cursor wherever the cursor goes, and
/// let go over another slot it lands there.
#[test]
fn a_dragged_item_follows_the_cursor_and_lands_where_dropped() {
    let (_server, mut client, slot) = with_a_tango();
    let to = (0..BAG_SLOTS)
        .rev()
        .find(|s| client.core().item_in(*s).is_none())
        .expect("a free slot");
    let from_box = client.item_box(slot).expect("its box");
    let from = client.centre_of(from_box).expect("laid out");
    let to_box = client.item_box(to).expect("its box");
    let onto = client.centre_of(to_box).expect("laid out");
    client.mouse(from);
    client.button(MouseButton::Left, ButtonState::Pressed);
    for step in 1..=4 {
        let at = from + (onto - from) * step as f32 / 4.0 + Vec2::new(0.0, -60.0);
        client.mouse(at);
        let drawn = ghost(&mut client).expect("the icon at the cursor");
        assert!(
            drawn.distance(at) < 1.5,
            "the icon at {drawn}, the cursor at {at}"
        );
    }
    client.mouse(onto);
    client.button(MouseButton::Left, ButtonState::Released);
    let patience = std::time::Duration::from_secs(10);
    client.within(patience, "the tango in the slot it was dropped on", |c| {
        c.slot_of(ItemId::TANGO) == Some(to)
    });
    assert!(ghost(&mut client).is_none(), "the icon let go");
}

/// The hero at its fountain with these items in its bag, each given by the cheat.
fn with_items(items: &[ItemId]) -> (Server, Client) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    for item in items {
        client.cheat(Cheat::Item { item: *item });
    }
    (server, client)
}

/// Opens the item menu on a slot's box with a right click, once the box is laid out.
fn open_menu(client: &mut Client, slot: u8) {
    let patience = std::time::Duration::from_secs(20);
    client.within(patience, "the slot's box laid out", |c| {
        c.item_box(slot)
            .and_then(|b| c.centre_of(b))
            .is_some_and(|at| at != Vec2::ZERO)
    });
    let item_box = client.item_box(slot).expect("its box");
    let on = client.centre_of(item_box).expect("laid out");
    client.mouse(on);
    client.click(MouseButton::Right);
}

/// The install's word for a menu key.
fn word(client: &Client, key: &str) -> String {
    client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label(key, &[])
        .expect("the install's word")
}

/// Clicks the menu's choice saying `word`.
fn choose(client: &mut Client, word: &str) {
    let patience = std::time::Duration::from_secs(10);
    client.within(patience, "the choice in the menu", |c| {
        button_saying(c, word).is_some()
    });
    let choice = button_saying(client, word).expect("the choice");
    client.frames(2);
    let on = client.centre_of(choice).expect("laid out");
    client.mouse(on);
    client.click(MouseButton::Left);
}

/// Whether the padlock is drawn on the box of a slot.
fn padlock(client: &mut Client, slot: u8) -> bool {
    let item_box = client.item_box(slot).expect("its box");
    crate::hud::combine_lock_shown(client.world_mut(), item_box)
}

/// A Bracer its parts just built offers Disassemble among the choices, in the install's words,
/// and choosing it gives the three parts back with a padlock on each, as Dota does; an item that
/// is built of nothing is offered no Disassemble.
#[test]
fn the_item_menu_disassembles_a_fresh_build_and_locks_the_parts() {
    let (_server, mut client) =
        with_items(&[ItemId::CIRCLET, ItemId::GAUNTLETS, ItemId::RECIPE_BRACER]);
    let patience = std::time::Duration::from_secs(20);
    client.within(patience, "the bracer built", |c| {
        c.slot_of(ItemId::BRACER).is_some()
    });
    let slot = client.slot_of(ItemId::BRACER).expect("the bracer");
    let disassemble = word(&client, "DOTA_InventoryMenu_Disassemble");
    open_menu(&mut client, slot);
    client.within(patience, "the item menu", |c| {
        button_saying(c, &disassemble).is_some()
    });
    choose(&mut client, &disassemble);
    client.within(patience, "the parts in the bag", |c| {
        [ItemId::CIRCLET, ItemId::GAUNTLETS, ItemId::RECIPE_BRACER]
            .iter()
            .all(|part| c.slot_of(*part).is_some())
            && c.slot_of(ItemId::BRACER).is_none()
    });
    for part in [ItemId::CIRCLET, ItemId::GAUNTLETS, ItemId::RECIPE_BRACER] {
        let slot = client.slot_of(part).expect("the part");
        client.within(patience, "a padlock on the part", |c| padlock(c, slot));
    }
    let circlet = client.slot_of(ItemId::CIRCLET).expect("the circlet");
    open_menu(&mut client, circlet);
    client.frames(4);
    assert!(
        button_saying(&mut client, &disassemble).is_none(),
        "a part is built of nothing"
    );
}

/// Lock Combining on an item that is a part of a recipe puts the padlock on it and turns the
/// choice into Unlock Combining; choosing that takes the padlock off.
#[test]
fn the_item_menu_locks_and_unlocks_combining() {
    let (_server, mut client) = with_items(&[ItemId::CIRCLET]);
    let patience = std::time::Duration::from_secs(20);
    client.within(patience, "the circlet in the bag", |c| {
        c.slot_of(ItemId::CIRCLET).is_some()
    });
    let slot = client.slot_of(ItemId::CIRCLET).expect("the circlet");
    let (lock, unlock) = (
        word(&client, "DOTA_InventoryMenu_LockCombine"),
        word(&client, "DOTA_InventoryMenu_UnlockCombine"),
    );
    assert!(!padlock(&mut client, slot));
    open_menu(&mut client, slot);
    client.within(patience, "Lock Combining offered", |c| {
        button_saying(c, &lock).is_some()
    });
    assert!(button_saying(&mut client, &unlock).is_none());
    choose(&mut client, &lock);
    client.within(patience, "the padlock", |c| padlock(c, slot));
    open_menu(&mut client, slot);
    client.within(patience, "Unlock Combining offered", |c| {
        button_saying(c, &unlock).is_some()
    });
    assert!(button_saying(&mut client, &lock).is_none());
    choose(&mut client, &unlock);
    client.within(patience, "the padlock gone", |c| !padlock(c, slot));
}
