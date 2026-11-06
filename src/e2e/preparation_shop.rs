//! The preparation storefront follows the install's dedicated starter categories.
use super::harness::{Client, Server};
use crate::shop::{Place, ShopCell};
use bevy::prelude::*;
use bota_proto::{HeroId, ItemId};

fn starting_ids(client: &mut Client) -> Vec<u16> {
    let world = client.world_mut();
    let mut ids: Vec<_> = world
        .query::<(
            Entity,
            &ShopCell,
            &Place,
            &InheritedVisibility,
            &ComputedNode,
        )>()
        .iter(world)
        .filter(|(_, _, place, seen, node)| {
            **place == Place::Grid && seen.get() && node.size() != Vec2::ZERO
        })
        .filter(|(entity, ..)| {
            let mut ancestor = Some(*entity);
            for _ in 0..32 {
                let Some(entity) = ancestor else { return false };
                if world.get::<crate::strategy::Screen>(entity).is_some() {
                    return true;
                }
                ancestor = world.get::<ChildOf>(entity).map(ChildOf::parent);
            }
            false
        })
        .map(|(_, item, ..)| item.0)
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn preparation_draws_only_the_installs_authored_starter_items() {
    let server = Server::start(1, &["--tactical-secs", "120"]);
    let mut client = Client::pick(&server, HeroId::SHADOW_FIEND, &[]);
    assert!(client.core().in_tactical_phase());
    client.until("the preparation storefront laid out", |c| {
        !starting_ids(c).is_empty()
    });
    let manifest = client.world().resource::<crate::manifest::Manifest>();
    let data: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest.root.join("text/shop.json")).expect("native shop data"),
    )
    .expect("shop JSON");
    let mut expected = Vec::new();
    for section in data["shops"]
        .as_array()
        .expect("native categories")
        .iter()
        .filter(|row| {
            row[0].as_str().is_some_and(|key| {
                [
                    "pregame_consumables",
                    "pregame_attributes",
                    "pregame_accessories",
                ]
                .contains(&key)
            })
        })
    {
        for name in section[1]
            .as_array()
            .expect("native items")
            .iter()
            .filter_map(|name| name.as_str())
            .filter_map(|name| name.strip_prefix("item_"))
        {
            if let Some((_, id, _)) = manifest
                .ids
                .iter()
                .find(|(kind, _, key)| kind == "item" && key == name)
                && client.core().offers(*id)
            {
                expected.push(*id);
            }
        }
    }
    expected.sort_unstable();
    expected.dedup();
    assert!(
        !expected.is_empty(),
        "the native starter-list oracle is populated"
    );
    let shown = starting_ids(&mut client);
    assert!(
        !shown.contains(&ItemId::OGRE_AXE.0),
        "unaffordable live-shop items do not belong in the starter list"
    );
    assert_eq!(
        shown, expected,
        "preparation must use authored pregame categories rather than the live shop's categories"
    );
    assert!(
        client
            .world()
            .resource::<crate::shop::Catalog>()
            .sections
            .iter()
            .any(|s| s.items.contains(&ItemId::OGRE_AXE.0)),
        "the live shop keeps its complete catalogue"
    );
}
