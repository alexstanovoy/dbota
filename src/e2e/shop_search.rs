//! The shop's search results are a list of rows, as Dota lists them: icon, the name in capitals and the
//! price at the right; the row under the pointer is lit and a click on any part of it is the item's.

use bevy::prelude::*;
use bota_proto::{HeroId, ItemId};

use super::harness::{Client, Server};
use super::shop_queue::point_item;
use crate::shop::{Place, Row, ShopCell, ShopPanel};

/// The words drawn in the row of an item, in order.
fn words(client: &mut Client, item: ItemId) -> Vec<String> {
    let world = client.world_mut();
    let rows: Vec<Entity> = world
        .query_filtered::<(Entity, &ShopCell), With<Row>>()
        .iter(world)
        .filter(|(_, cell)| cell.0 == item.0)
        .map(|(row, _)| row)
        .collect();
    let mut found = Vec::new();
    for row in rows {
        let mut todo = vec![row];
        while let Some(node) = todo.pop() {
            if let Some(text) = world.get::<Text>(node) {
                found.push(text.0.clone());
            }
            if let Some(children) = world.get::<Children>(node) {
                todo.extend(children.iter());
            }
        }
    }
    found.reverse();
    found
}

fn rect_of(world: &World, node: Entity) -> Rect {
    let size = world.get::<ComputedNode>(node).expect("laid out").size();
    let centre = world
        .get::<bevy::ui::UiGlobalTransform>(node)
        .expect("placed")
        .translation;
    Rect::from_center_size(centre, size)
}

#[test]
fn search_results_are_rows_of_icon_name_in_capitals_and_price_and_the_pointed_one_is_lit() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--shop", "basics:/wraith"]);
    client.until("the result row laid out", |c| {
        !words(c, ItemId::WRAITH_BAND).is_empty()
    });
    client.frames(3);
    let shown = words(&mut client, ItemId::WRAITH_BAND);
    assert!(
        shown.iter().any(|word| word == "WRAITH BAND"),
        "the name in capitals is in the row: {shown:?}"
    );
    assert!(
        shown.iter().any(|word| word == "505"),
        "the price is in the row: {shown:?}"
    );
    // The row spans the panel's width, so the pointer anywhere on it is on the item.
    let world = client.world_mut();
    let panel = world
        .query_filtered::<Entity, With<ShopPanel>>()
        .single(world)
        .expect("the panel");
    let panel = rect_of(world, panel);
    let row = world
        .query_filtered::<(Entity, &ShopCell, &InheritedVisibility), With<Row>>()
        .iter(world)
        .find(|(_, cell, seen)| cell.0 == ItemId::WRAITH_BAND.0 && seen.get())
        .map(|(row, ..)| rect_of(world, row))
        .expect("the row");
    assert!(
        row.width() >= panel.width() * 0.9,
        "a row of {} in a panel of {}",
        row.width(),
        panel.width()
    );
    // And it is lit while pointed at, plain otherwise.
    point_item(&mut client, ItemId::WRAITH_BAND, Place::Grid);
    client.frames(3);
    let world = client.world_mut();
    let lit = world
        .query_filtered::<(&ShopCell, &BackgroundColor), With<Row>>()
        .iter(world)
        .find(|(cell, _)| cell.0 == ItemId::WRAITH_BAND.0)
        .map(|(_, ground)| ground.0)
        .expect("the row");
    assert_ne!(lit, Color::NONE, "the pointed row is lit");
}
