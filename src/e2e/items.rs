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
