//! Secret-shop access, receipts and map/UI state through a real match.

use bevy::prelude::*;
use bota_client_core::{Intent, Refused};
use bota_proto::{
    Cheat, EntityId, HeroId, ItemId, ItemSlot, Order, RejectReason, Target, Vec2 as At,
};

use super::harness::{Client, Server};

fn item_count(client: &Client, item: ItemId) -> usize {
    let slot = client.core().my_slot.expect("seated");
    client
        .view()
        .units
        .iter()
        .filter(|unit| unit.owner == Some(slot))
        .flat_map(|unit| unit.items.iter().flatten())
        .filter(|held| held.id == item)
        .count()
        + client
            .view()
            .player(slot)
            .expect("own row")
            .stash
            .as_deref()
            .expect("own stash")
            .iter()
            .flatten()
            .filter(|held| held.id == item)
            .count()
}

fn buy(client: &mut Client, item: ItemId, expected: Option<RejectReason>) {
    let before = item_count(client, item);
    client.core_mut().reject = None;
    client.core_mut().act(Intent::Buy(item.0));
    client.until("the purchase refused or completed", |c| {
        c.core().reject.is_some() || item_count(c, item) > before
    });
    let expected = expected.map(Refused::Rule);
    assert_eq!(
        client.core().reject.as_ref().map(|(reason, _)| reason),
        expected.as_ref(),
        "buy {item:?}"
    );
    assert_eq!(
        item_count(client, item),
        before + usize::from(expected.is_none())
    );
}

#[test]
fn secret_shop_components_require_shop_access_and_reach_the_local_carrier() {
    for map in ["0", "2"] {
        check_secret_shops_on_map(map);
    }
}

fn check_secret_shops_on_map(map: &str) {
    let server = Server::start(1, &["--map", map]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Gold { amount: 100000 });
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::CreepSpawning { on: false });
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .shop = true;
    let hero = client.me().id;
    let home = (client.me().pos.x.to_int(), client.me().pos.y.to_int());
    let info = client.core().statics().expect("match");
    assert!(info.entry(ItemId::DEMON_EDGE).expect("sold").secret_shop);
    assert!(!info.entry(ItemId::BOOTS).expect("sold").secret_shop);
    assert!(!info.metadata().universal_shop);
    assert_eq!(info.secret_shops().len(), 2);
    client.until("Demon Edge darkened outside its shop", |c| {
        shade(c, ItemId::DEMON_EDGE) == Some(true)
    });
    buy(
        &mut client,
        ItemId::DEMON_EDGE,
        Some(RejectReason::NotAtShop),
    );
    assert_shop_marks(&mut client);
    // Front of the actual neutral shopkeepers, from the install's map entities.
    for at in [(4260, 10950), (14020, 7870)] {
        client.stand_at(at);
        client.until("secret component available in the shop", |c| {
            shade(c, ItemId::DEMON_EDGE) == Some(false)
        });
        buy(&mut client, ItemId::DEMON_EDGE, None);
        let slot = client
            .slot_of(ItemId::DEMON_EDGE)
            .expect("the bought component");
        client.until("component shown in the hero inventory", |c| {
            inventory_icon(c, slot, ItemId::DEMON_EDGE)
        });
    }
    client.stand_at((5000, 11000));
    client.until("outside the polygon, component darkened", |c| {
        shade(c, ItemId::VITALITY_BOOSTER) == Some(true)
    });
    buy(
        &mut client,
        ItemId::VITALITY_BOOSTER,
        Some(RejectReason::NotAtShop),
    );
    check_courier_purchase_and_capacity(&mut client, hero, home);
}

fn check_courier_purchase_and_capacity(client: &mut Client, hero: EntityId, home: (i32, i32)) {
    client.stand_at(home);
    let courier = client.core().my_courier().expect("courier");
    client.core_mut().act(Intent::SelectCourier);
    client.cheat(Cheat::Teleport {
        at: At::from_ints(4260, 10950),
    });
    client.core_mut().act(Intent::SelectHero);
    client.until("courier grants access while the hero stays at base", |c| {
        shade(c, ItemId::ENERGY_BOOSTER) == Some(false)
    });
    buy(client, ItemId::ENERGY_BOOSTER, None);
    assert!(
        client
            .view()
            .unit(courier)
            .expect("courier")
            .items
            .iter()
            .flatten()
            .any(|held| held.id == ItemId::ENERGY_BOOSTER)
    );
    assert!(
        !client
            .me()
            .items
            .iter()
            .flatten()
            .any(|held| held.id == ItemId::ENERGY_BOOSTER)
    );
    client.core_mut().act(Intent::SelectCourier);
    client.until("booster shown in the courier inventory", |c| {
        inventory_icon(c, 0, ItemId::ENERGY_BOOSTER)
    });
    client.core_mut().act(Intent::SelectHero);
    check_split_recipe_receipt(client, courier);
    client.stand_at((4260, 10950));
    check_secret_shop_sale(client);
    fill_courier_from_hero(client, hero, courier);
    client.stand_at(home);
    assert!(
        client
            .view()
            .unit(courier)
            .expect("courier")
            .pos
            .within(At::from_ints(4260, 10950), bota_proto::Fixed::from_int(32)),
        "the full courier remains at the secret shop while the hero is home"
    );
    buy(
        client,
        ItemId::DEMON_EDGE,
        Some(RejectReason::InventoryFull),
    );
}

fn check_secret_shop_sale(client: &mut Client) {
    let slot = client
        .slot_of(ItemId::DEMON_EDGE)
        .expect("the bought component");
    let before = item_count(client, ItemId::DEMON_EDGE);
    client.core_mut().act(Intent::Sell(slot));
    client.until("the component sold at the secret shop", |c| {
        item_count(c, ItemId::DEMON_EDGE) < before
    });
}

fn fill_courier_from_hero(client: &mut Client, hero: EntityId, courier: EntityId) {
    let free = client
        .view()
        .unit(courier)
        .expect("courier")
        .items
        .iter()
        .filter(|slot| slot.is_none())
        .count();
    assert!(free <= bota_proto::COURIER_ITEM_SLOTS);
    for _ in 0..free {
        client.cheat(Cheat::Item {
            item: ItemId::IRON_BRANCH,
        });
        let slot = client.slot_of(ItemId::IRON_BRANCH).expect("granted branch");
        let before = client
            .view()
            .unit(courier)
            .expect("courier")
            .items
            .iter()
            .flatten()
            .count();
        client.core_mut().send_order_to(
            Some(hero),
            Order::Put {
                slot: ItemSlot(slot),
                target: Target::Unit(courier),
            },
        );
        client.until("branch passed through the real item order", |c| {
            c.view()
                .unit(courier)
                .expect("courier")
                .items
                .iter()
                .flatten()
                .count()
                > before
        });
    }
}

fn check_split_recipe_receipt(client: &mut Client, courier: EntityId) {
    client.stand_at(super::harness::MID);
    client.core_mut().reject = None;
    client.core_mut().act(Intent::Buy(ItemId::BUTTERFLY.0));
    client.until("the recipe refused or its secret parts received", |c| {
        c.core().reject.is_some()
            || c.view()
                .unit(courier)
                .expect("courier")
                .items
                .iter()
                .flatten()
                .any(|i| i.id == ItemId::EAGLESONG)
    });
    assert!(
        client.core().reject.is_none(),
        "recipe purchase: {:?}",
        client.core().reject
    );
    let held = client.view().unit(courier).expect("courier");
    for id in [ItemId::EAGLESONG, ItemId::TALISMAN_OF_EVASION] {
        assert!(
            held.items.iter().flatten().any(|item| item.id == id),
            "secret part {id:?} on courier"
        );
    }
    let player = client
        .view()
        .player(client.core().my_slot.expect("seated"))
        .expect("own row");
    // 7.41f item_recipe_butterfly: Eagle Song, Claymore, Talisman of Evasion.
    assert!(
        player
            .stash
            .as_deref()
            .expect("own stash")
            .iter()
            .flatten()
            .any(|item| item.id == ItemId::CLAYMORE),
        "base-shop part stays in the stash"
    );
}

#[test]
fn a_full_hero_at_the_secret_shop_buys_into_its_courier_at_the_same_shop() {
    let server = Server::start(1, &["--map", "2"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Gold { amount: 100000 });
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.stand_at((4260, 10950));
    let courier = client.core().my_courier().expect("courier");
    client.core_mut().act(Intent::SelectCourier);
    client.cheat(Cheat::Teleport {
        at: At::from_ints(4260, 10950),
    });
    client.core_mut().act(Intent::SelectHero);
    for _ in 0..(bota_core::rules::INVENTORY_SLOTS + bota_core::rules::BACKPACK_SLOTS) {
        client.cheat(Cheat::Item {
            item: ItemId::IRON_BRANCH,
        });
    }
    assert!(client.me().items.iter().all(Option::is_some));
    buy(&mut client, ItemId::DEMON_EDGE, None);
    assert!(
        !client
            .me()
            .items
            .iter()
            .flatten()
            .any(|held| held.id == ItemId::DEMON_EDGE)
    );
    assert!(
        client
            .view()
            .unit(courier)
            .expect("courier")
            .items
            .iter()
            .flatten()
            .any(|held| held.id == ItemId::DEMON_EDGE)
    );
    client.core_mut().act(Intent::SelectCourier);
    client.until("Demon Edge drawn in the courier bag", |c| {
        inventory_icon(c, 0, ItemId::DEMON_EDGE)
    });
}

#[test]
fn demo_universal_shop_keeps_secret_components_available_at_base() {
    let server = Server::start(1, &["--map", "1"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Gold { amount: 10000 });
    assert!(
        client
            .core()
            .statics()
            .expect("match")
            .metadata()
            .universal_shop
    );
    buy(&mut client, ItemId::DEMON_EDGE, None);
    assert!(
        client
            .me()
            .items
            .iter()
            .flatten()
            .any(|held| held.id == ItemId::DEMON_EDGE)
    );
}

fn shade(client: &mut Client, item: ItemId) -> Option<bool> {
    let world = client.world_mut();
    world
        .query::<(&crate::shop::ShopAccessShade, &ChildOf, &Visibility)>()
        .iter(world)
        .find_map(|(_, parent, shown)| {
            let parent = parent.parent();
            (world.get::<crate::shop::ShopCell>(parent)?.0 == item.0
                && world.get::<InheritedVisibility>(parent)?.get())
            .then_some(*shown != Visibility::Hidden)
        })
}

fn inventory_icon(client: &mut Client, slot: u8, item: ItemId) -> bool {
    let Some(slot_box) = client.item_box(slot) else {
        return false;
    };
    let world = client.world_mut();
    let icon = world
        .resource::<crate::manifest::Manifest>()
        .item_icon(item.0)
        .expect("icon");
    let server = world.resource::<AssetServer>().clone();
    world
        .query::<(
            &crate::hud::Part,
            &crate::hud::PartOf,
            &ImageNode,
            &InheritedVisibility,
        )>()
        .iter(world)
        .any(|(part, parent, image, shown)| {
            *part == crate::hud::Part::Icon
                && parent.0 == slot_box
                && shown.get()
                && server
                    .get_path(image.image.id())
                    .is_some_and(|path| path.to_string() == icon)
        })
}

fn assert_shop_marks(client: &mut Client) {
    let expected = [
        At {
            x: bota_proto::Fixed { raw: 271050048 },
            y: bota_proto::Fixed { raw: 731640824 },
        },
        At {
            x: bota_proto::Fixed { raw: 924197952 },
            y: bota_proto::Fixed { raw: 524834464 },
        },
    ];
    client.until("both neutral shops marked through fog", |c| {
        shop_marks(c).len() == 2
    });
    let marks = shop_marks(client);
    for pos in expected {
        assert!(
            marks.iter().any(|mark| mark.0 == pos),
            "shopkeeper {pos:?} marked"
        );
    }
    for (pos, left, top, path) in marks {
        assert!(expected.contains(&pos), "shopkeeper {pos:?}");
        let inner = crate::minimap::SIDE - 6.0;
        let size = bota_core::rules::MAP_SIZE as f32;
        assert!((left - (pos.x.to_f32() / size * inner - 7.0)).abs() < 0.01);
        assert!((top - ((1.0 - pos.y.to_f32() / size) * inner - 7.0)).abs() < 0.01);
        assert_eq!(
            Some(path),
            client
                .world()
                .resource::<crate::manifest::Manifest>()
                .file("icon.minimap.shop")
        );
    }
}

fn shop_marks(client: &mut Client) -> Vec<(At, f32, f32, String)> {
    let world = client.world_mut();
    let server = world.resource::<AssetServer>().clone();
    world
        .query::<(
            &crate::minimap::ShopMark,
            &Node,
            &ImageNode,
            &InheritedVisibility,
        )>()
        .iter(world)
        .filter_map(|(mark, node, image, shown)| {
            if !shown.get() {
                return None;
            }
            let (Val::Px(left), Val::Px(top)) = (node.left, node.top) else {
                return None;
            };
            Some((
                mark.0?,
                left,
                top,
                server.get_path(image.image.id())?.to_string(),
            ))
        })
        .collect()
}
