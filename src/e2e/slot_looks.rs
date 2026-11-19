//! How the bottom panel's item slots look: the backpack's items greyed, as Dota's backpack
//! shows them, and an active item told from a passive one.

use bevy::prelude::*;
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, Server};
use crate::hud::{Part, PartOf};

/// The tint of the icon an item slot of the bottom panel draws.
fn icon_tint(client: &mut Client, slot: u8) -> Option<Color> {
    let item_box = client.item_box(slot)?;
    let world = client.world_mut();
    let mut parts = world.query::<(&Part, &PartOf, &ImageNode)>();
    parts
        .iter(world)
        .find(|(part, of, _)| of.0 == item_box && matches!(part, Part::Icon))
        .map(|(_, _, image)| image.color)
}

/// How bright a tint is, 0 to 1.
fn brightness(color: Color) -> f32 {
    let c = color.to_srgba();
    (c.red + c.green + c.blue) / 3.0 * c.alpha
}

/// The seventh item of a hero's bag lies in the backpack: its icon is dimmed against the
/// inventory's (it was drawn as bright as theirs).
#[test]
fn a_backpack_items_icon_is_greyed_against_the_inventorys() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    for item in [
        ItemId::BOOTS,
        ItemId::CLARITY,
        ItemId::HEALING_SALVE,
        ItemId::IRON_BRANCH,
        ItemId::TANGO,
        ItemId::QUELLING_BLADE,
        ItemId::CIRCLET,
    ] {
        client.cheat(Cheat::Item { item });
    }
    client.until("the backpack holding the seventh item", |c| {
        c.slot_of(ItemId::CIRCLET).is_some_and(|slot| slot >= 6)
    });
    let backpack = client.slot_of(ItemId::CIRCLET).expect("in the bag");
    client.frames(5);
    let worn = brightness(icon_tint(&mut client, backpack).expect("a backpack slot"));
    let carried = brightness(icon_tint(&mut client, 0).expect("the first slot"));
    assert!(
        worn < carried * 0.8,
        "the backpack's icon ({worn}) is as bright as the inventory's ({carried})"
    );
}

#[test]
fn a_backpacked_banner_draws_as_usable_and_can_be_planted() {
    use bota_client_core::{Intent, Slot};
    use bota_proto::UnitKind;

    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.stand_at(super::harness::MID);
    let item = ItemId::ROSHANS_BANNER;
    client.cheat(Cheat::Item { item });
    let from = client.slot_of(item).expect("the Banner in the bag");
    let slot = bota_client_core::INVENTORY_SLOTS;
    client.core_mut().act(Intent::Swap { from, to: slot });
    client.until("the Banner in the backpack", |c| {
        c.slot_of(item) == Some(slot)
    });
    client.frames(5);
    assert_eq!(icon_tint(&mut client, slot), Some(Color::WHITE));

    let here = client.me().pos;
    let target = client
        .screen_of((here.x.to_f32() + 180., here.y.to_f32()))
        .expect("ground in view");
    let box_node = client.item_box(slot).expect("the Banner's backpack box");
    client.hover_ui(box_node, std::time::Duration::from_secs(10));
    client.click(MouseButton::Left);
    assert_eq!(client.core().aiming, Some(Slot::Item(slot)));
    client.mouse(target);
    client.click(MouseButton::Left);
    client.until("the server accepted and planted the Banner", |c| {
        c.slot_of(item).is_none()
            && c.view()
                .units
                .iter()
                .any(|unit| unit.kind == UnitKind::Banner)
    });
}

/// The edge colour of an item slot's box.
fn edge(client: &mut Client, slot: u8) -> Color {
    let item_box = client.item_box(slot).expect("the slot box");
    client
        .world()
        .get::<BorderColor>(item_box)
        .expect("a bordered box")
        .top
}

/// The background of the key label of an item slot's box, and its text.
fn key_label(client: &mut Client, slot: u8) -> (Color, String) {
    let item_box = client.item_box(slot).expect("the slot box");
    let world = client.world_mut();
    let mut parts = world.query::<(&Part, &PartOf, &Text, &BackgroundColor)>();
    parts
        .iter(world)
        .find(|(part, of, ..)| of.0 == item_box && matches!(part, Part::Key))
        .map(|(_, _, text, bg)| (bg.0, text.0.clone()))
        .expect("a key label with a background")
}

/// Boots work on their own and Clarity is used: Dota sinks the passive one's slot (an inset shadow,
/// stood for here by a dark edge) and sets the active one's key on a badge. The two were the same
/// box, told apart only by the key's letter.
#[test]
fn a_passive_items_slot_has_a_dark_edge_and_an_active_items_key_a_badge() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    for item in [ItemId::BOOTS, ItemId::CLARITY] {
        client.cheat(Cheat::Item { item });
    }
    client.until("both in the bag", |c| {
        c.slot_of(ItemId::BOOTS).is_some() && c.slot_of(ItemId::CLARITY).is_some()
    });
    let (passive, active) = (
        client.slot_of(ItemId::BOOTS).expect("boots"),
        client.slot_of(ItemId::CLARITY).expect("clarity"),
    );
    client.frames(5);
    let (dark, flat) = (edge(&mut client, passive), edge(&mut client, active));
    assert!(
        brightness(dark) < brightness(flat) * 0.7,
        "the passive item's edge ({dark:?}) is not dark against the active one's ({flat:?})"
    );
    let (badge, key) = key_label(&mut client, active);
    assert!(!key.is_empty(), "the active item has a key");
    assert_ne!(badge, Color::NONE, "the active item's key has no badge");
    let (none, nothing) = key_label(&mut client, passive);
    assert!(
        nothing.is_empty() && none == Color::NONE,
        "the passive one has a key"
    );
}
