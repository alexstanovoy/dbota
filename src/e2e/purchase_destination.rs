//! Base-shop purchases go to the hero at the shop, and to the stash away from it.

use bevy::prelude::*;
use bota_client_core::{BAG_SLOTS, Intent};
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, MID, Server};

fn icon_in_slot(client: &mut Client, slot: u8, item: ItemId) -> bool {
    let Some(slot_box) = client.item_box(slot) else {
        return false;
    };
    let world = client.world_mut();
    let icon = world
        .resource::<crate::manifest::Manifest>()
        .item_icon(item.0)
        .expect("the item's icon");
    let server = world.resource::<AssetServer>().clone();
    world
        .query::<(
            &crate::hud::Part,
            &crate::hud::PartOf,
            &ImageNode,
            &InheritedVisibility,
        )>()
        .iter(world)
        .any(|(part, of, image, seen)| {
            *part == crate::hud::Part::Icon
                && of.0 == slot_box
                && seen.get()
                && server.get_path(image.image.id()).map(|p| p.to_string()) == Some(icon.clone())
        })
}

fn stash(client: &Client) -> &[Option<bota_proto::ItemView>] {
    client
        .view()
        .player(client.core().my_slot.expect("seated"))
        .expect("own row")
        .stash
        .as_deref()
        .expect("own stash")
}

#[test]
fn base_shop_purchases_fill_hero_at_shop_and_stash_away_from_shop() {
    // https://liquipedia.net/dota2/Base_Shop: base-shop items may be bought
    // remotely into the stash; the courier retrieves them on a separate order.
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Gold { amount: 10000 });
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .shop = true;
    let courier = client.core().my_courier().expect("the owner's courier");
    let map = bota_core::map_of(bota_core::MAP2_ID);
    assert!(bota_core::at_home_shop(
        map,
        client.me().team,
        client.me().pos
    ));
    client.core_mut().act(Intent::Buy(ItemId::IRON_BRANCH.0));
    client.until("the bought branch drawn in the hero's inventory", |c| {
        icon_in_slot(c, 0, ItemId::IRON_BRANCH)
    });
    assert_eq!(
        client.me().items[0].map(|i| i.id),
        Some(ItemId::IRON_BRANCH)
    );
    assert!(stash(&client).iter().all(Option::is_none));
    client.stand_at(MID);
    assert!(!bota_core::at_home_shop(
        map,
        client.me().team,
        client.me().pos
    ));
    client.core_mut().act(Intent::Buy(ItemId::BOOTS.0));
    client.until("the remotely bought boots drawn in the stash", |c| {
        icon_in_slot(c, BAG_SLOTS, ItemId::BOOTS)
    });
    assert_eq!(stash(&client)[0].map(|i| i.id), Some(ItemId::BOOTS));
    assert!(
        !client
            .me()
            .items
            .iter()
            .flatten()
            .any(|i| i.id == ItemId::BOOTS)
    );
    assert!(
        client
            .view()
            .unit(courier)
            .expect("courier at base")
            .items
            .iter()
            .all(Option::is_none),
        "default purchases do not automatically load the courier"
    );
}
