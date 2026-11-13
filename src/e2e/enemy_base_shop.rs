//! Enemy base shops offer the visitor's purchases and the same personal stash.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ItemId, Team};

use super::harness::{Client, MID, Server};
use super::items::button_saying;
use super::purchase_destination::{icon_in_slot, stash};

fn client(server: &Server) -> Client {
    let mut client = Client::join(server, HeroId::SHADOW_FIEND, &[]);
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::Gold { amount: 10000 });
    client
}

fn enemy_base(client: &mut Client) {
    let team = if client.me().team == Team::Radiant {
        Team::Dire
    } else {
        Team::Radiant
    };
    let map = client.core().session().expect("match").map();
    let at = bota_core::hero_spawn_pos(map, team);
    client.stand_at((at.x.to_int(), at.y.to_int()));
    assert!(
        client.me().pos.within(at, bota_proto::Fixed::from_int(32)),
        "the hero really reached the opposing shop"
    );
    client.until("the HUD shows the current purse", |c| {
        let gold = c.core().spendable_gold().to_string();
        crate::shop::drawn_gold(c.world_mut()) == Some(gold)
    });
}

fn marker<T: Component>(client: &mut Client) -> Entity {
    let world = client.world_mut();
    world
        .query_filtered::<Entity, With<T>>()
        .single(world)
        .expect("HUD node")
}

/// Puts the pointer on a node and waits until the interface says it is over it: a click that comes
/// before that goes to nobody, as the pointer's hover reaches the interface some frames after it
/// arrives.
pub(super) fn aim(client: &mut Client, node: Entity) {
    client.hover_ui(node, std::time::Duration::from_secs(20));
}

pub(super) fn click_node(client: &mut Client, node: Entity) {
    client.until("the button laid out", |c| {
        c.centre_of(node).is_some_and(|at| at != Vec2::ZERO)
    });
    aim(client, node);
    client.click(MouseButton::Left);
}

fn shown<T: Component>(client: &mut Client) -> bool {
    let world = client.world_mut();
    world
        .query_filtered::<(&ComputedNode, &InheritedVisibility), With<T>>()
        .iter(world)
        .any(|(node, seen)| seen.get() && node.size() != Vec2::ZERO)
}

fn shop_skin(client: &mut Client) -> Option<String> {
    let world = client.world_mut();
    let art = world
        .query_filtered::<&ImageNode, With<crate::shop::ShopButton>>()
        .single(world)
        .ok()?;
    world
        .resource::<AssetServer>()
        .get_path(art.image.id())
        .map(|path| path.to_string())
}

#[test]
fn enemy_base_lights_the_shop_and_shows_the_same_stash_with_grab_all() {
    let server = Server::start(1, &[]);
    let mut client = client(&server);
    client.stand_at(MID);
    client.core_mut().act(Intent::Buy(ItemId::BOOTS.0));
    client.until("remote boots in the personal stash", |c| {
        stash(c)
            .iter()
            .flatten()
            .any(|item| item.id == ItemId::BOOTS)
    });
    assert!(!client.world().resource::<crate::input::Panels>().shop);
    assert!(!shown::<crate::shop::GrabAll>(&mut client));
    enemy_base(&mut client);
    let lit = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file("shop.button_inrange")
        .expect("native in-range art");
    assert_eq!(
        shop_skin(&mut client),
        Some(lit),
        "the enemy base is an in-range base shop"
    );
    assert_eq!(
        crate::shop::drawn_gold_style(client.world_mut()).map(|(_, ink)| ink),
        Some(Color::WHITE)
    );
    assert!(
        shown::<crate::shop::GrabAll>(&mut client),
        "the closed shop still exposes the active stash at either base"
    );
    let grab = marker::<crate::shop::GrabAll>(&mut client);
    click_node(&mut client, grab);
    client.until("GRAB ALL delivers the stashed boots", |c| {
        icon_in_slot(c, 0, ItemId::BOOTS) && stash(c).iter().all(Option::is_none)
    });
    let shop = marker::<crate::shop::ShopButton>(&mut client);
    click_node(&mut client, shop);
    client.until("the shop grid opens at the enemy base", |c| {
        if !c.world().resource::<crate::input::Panels>().shop {
            return false;
        }
        let world = c.world_mut();
        world
            .query::<(
                &crate::shop::ShopCell,
                &crate::shop::Place,
                &ComputedNode,
                &InheritedVisibility,
            )>()
            .iter(world)
            .any(|(_, place, node, seen)| {
                *place == crate::shop::Place::Grid && seen.get() && node.size() != Vec2::ZERO
            })
    });
}

fn menu(client: &mut Client, item: ItemId) {
    let slot = client.slot_of(item).expect("owned item");
    let node = client.item_box(slot).expect("drawn slot");
    aim(client, node);
    client.click(MouseButton::Right);
    let info = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label("DOTA_InventoryMenu_ShowInShop", &[])
        .expect("native menu label");
    client.until("the inventory menu is drawn", |c| {
        button_saying(c, &info).is_some()
    });
}

#[test]
fn enemy_base_menu_moves_items_to_the_stash_and_sells_them_immediately() {
    let server = Server::start(1, &[]);
    let mut client = client(&server);
    client.core_mut().act(Intent::Buy(ItemId::BOOTS.0));
    client.until("boots in the drawn inventory", |c| {
        icon_in_slot(c, 0, ItemId::BOOTS)
    });
    enemy_base(&mut client);
    menu(&mut client, ItemId::BOOTS);
    let texts = client.world().resource::<crate::dota_text::Texts>();
    let sell = texts
        .label("DOTA_InventoryMenu_Sell", &[])
        .expect("native Sell");
    let to_stash = texts
        .label("DOTA_InventoryMenu_MoveToStash", &[])
        .expect("native Move to Stash");
    assert!(
        button_saying(&mut client, &sell).is_some(),
        "an item at the enemy base offers Sell rather than Mark for Sell"
    );
    let put = button_saying(&mut client, &to_stash).expect("either base offers Move to Stash");
    click_node(&mut client, put);
    client.until("boots drawn in the same stash", |c| {
        icon_in_slot(c, bota_client_core::BAG_SLOTS, ItemId::BOOTS)
    });
    let grab = marker::<crate::shop::GrabAll>(&mut client);
    click_node(&mut client, grab);
    client.until("boots retrieved at the enemy base", |c| {
        icon_in_slot(c, 0, ItemId::BOOTS)
    });
    menu(&mut client, ItemId::BOOTS);
    let sell = button_saying(&mut client, &sell).expect("Sell");
    let gold = client.core().spendable_gold();
    click_node(&mut client, sell);
    client.until("the sale completes at the enemy base", |c| {
        c.slot_of(ItemId::BOOTS).is_none()
            && stash(c).iter().all(Option::is_none)
            && c.core().spendable_gold()
                >= gold + bota_core::item_def(ItemId::BOOTS).expect("boots").cost / 2
    });
}
