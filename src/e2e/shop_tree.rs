//! The recipe tree of every composite item lies wholly inside the shop panel, at every window size.

use bevy::prelude::*;
use bota_proto::{HeroId, ItemId};

use super::harness::{Client, Server};
use crate::shop::{Chosen, Place, ShopCell, ShopPanel, Tree};

/// A node's rectangle in the window's physical pixels.
fn rect_of(world: &World, node: Entity) -> Rect {
    let size = world.get::<ComputedNode>(node).expect("laid out").size();
    let centre = world
        .get::<bevy::ui::UiGlobalTransform>(node)
        .expect("placed")
        .translation;
    Rect::from_center_size(centre, size)
}

fn panel_rect(client: &mut Client) -> Rect {
    let world = client.world_mut();
    let panel = world
        .query_filtered::<Entity, With<ShopPanel>>()
        .single(world)
        .expect("the shop panel");
    rect_of(world, panel)
}

fn tree_rect(client: &mut Client) -> Rect {
    let world = client.world_mut();
    let tree = world
        .query_filtered::<Entity, With<Tree>>()
        .single(world)
        .expect("the recipe tree's band");
    rect_of(world, tree)
}

/// The tree's icons that are drawn, as the rectangles they fill.
fn tree_icons(client: &mut Client) -> Vec<(ItemId, Rect)> {
    let world = client.world_mut();
    let found: Vec<(Entity, ItemId)> = world
        .query::<(Entity, &ShopCell, &Place, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, _, place, seen)| **place == Place::Tree && seen.get())
        .map(|(entity, cell, ..)| (entity, ItemId(cell.0)))
        .collect();
    found
        .into_iter()
        .map(|(entity, id)| (id, rect_of(world, entity)))
        .collect()
}

/// The items the shop builds from parts, the widest recipe and the deepest one among them first.
fn composites(client: &Client) -> Vec<ItemId> {
    let catalogue = client.core().catalogue();
    let mut found: Vec<ItemId> = bota_core::sold_items()
        .filter(|id| catalogue.sold(*id).is_some())
        .filter(|id| bota_core::item_def(*id).is_some_and(|def| !def.components.is_empty()))
        .collect();
    let width = |id: &ItemId| bota_core::item_def(*id).map_or(0, |def| def.components.len());
    found.sort_by_key(|id| std::cmp::Reverse(width(id)));
    found
}

/// The panel's own rows from the top, to say where the room went when an icon is out.
fn panel_rows(client: &mut Client) -> Vec<Rect> {
    let world = client.world_mut();
    let panel = world
        .query_filtered::<Entity, With<ShopPanel>>()
        .single(world)
        .expect("the shop panel");
    let rows: Vec<Entity> = world
        .get::<Children>(panel)
        .map(|children| children.iter().collect())
        .unwrap_or_default();
    rows.into_iter()
        .filter_map(|row| {
            world
                .get::<ComputedNode>(row)
                .filter(|node| node.size() != Vec2::ZERO)
                .map(|_| rect_of(world, row))
        })
        .collect()
}

fn choose(client: &mut Client, item: ItemId) {
    client.world_mut().resource_mut::<Chosen>().0 = Some(item.0);
    client.frames(3);
}

/// Windows of the sizes people play in, the owner's 2877 by 1642 among them (the interface scales by a
/// fraction there), and one too small for the panel to be roomy.
const WINDOWS: [&str; 4] = ["1920x1080", "2877x1642", "1280x720", "800x500"];

/// Every composite of the catalogue, chosen: its icons and the panel's own foot are inside the panel,
/// and no two icons lie over each other.
#[test]
fn the_recipe_tree_of_every_composite_lies_inside_the_shop_panel() {
    for window in WINDOWS {
        let server = Server::start(1, &[]);
        let mut client = Client::join(
            &server,
            HeroId::SHADOW_FIEND,
            &["--shop", "upgrades", "--window", window],
        );
        client.frames(3);
        let composites = composites(&client);
        assert!(
            composites.len() > 20,
            "the catalogue has composites to draw"
        );
        for item in composites {
            choose(&mut client, item);
            // The panel settles a frame or two after the window's size is known: read it now.
            let panel = panel_rect(&mut client);
            let icons = tree_icons(&mut client);
            let parts = bota_core::item_def(item).expect("an item").components.len();
            assert!(
                icons.len() > parts,
                "{window} {item:?}: the item and its {parts} parts are drawn, not {icons:?}"
            );
            for (id, rect) in &icons {
                assert!(
                    panel.contains(rect.min) && panel.contains(rect.max),
                    "{window} {item:?}: {id:?} at {rect:?} leaves the panel {panel:?}; the panel's rows: {:?}",
                    panel_rows(&mut client)
                );
            }
            // The tree fills its band from the top to the foot, whatever rows it has: no row is left
            // empty above the item, which would push the rest down and out.
            let band = tree_rect(&mut client);
            let scale = band.height() / crate::shop::TREE_HEIGHT;
            let top = icons
                .iter()
                .map(|(_, rect)| rect.min.y)
                .fold(f32::MAX, f32::min);
            let foot = icons
                .iter()
                .map(|(_, rect)| rect.max.y)
                .fold(f32::MIN, f32::max);
            assert!(
                top - band.min.y <= 8.0 * scale,
                "{window} {item:?}: {} pixels of the band lie empty above the topmost icon",
                top - band.min.y
            );
            assert!(
                band.max.y - foot <= 8.0 * scale,
                "{window} {item:?}: {} pixels of the band lie empty under the lowest icon",
                band.max.y - foot
            );
            for (a, (id_a, rect_a)) in icons.iter().enumerate() {
                for (id_b, rect_b) in &icons[a + 1..] {
                    let overlap = rect_a.intersect(*rect_b);
                    assert!(
                        overlap.is_empty() || overlap.size().min_element() < 1.0,
                        "{window} {item:?}: {id_a:?} {rect_a:?} lies over {id_b:?} {rect_b:?}"
                    );
                }
            }
        }
    }
}
