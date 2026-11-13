//! Buying a composite again while the parts of the first copy still wait apart from its spares: the
//! shop's right click buys one more copy each time, through the real server.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, MID, Server};
use super::purchase_destination::stash;
use super::shop_queue::{cells, point_item};
use crate::shop::Place;

/// How many of `item` the hero's bag, backpack and stash hold.
fn held(client: &Client, item: ItemId) -> usize {
    client
        .me()
        .items
        .iter()
        .flatten()
        .chain(stash(client).iter().flatten())
        .filter(|held| held.id == item)
        .count()
}

/// Right-clicks the composite in the shop's results until the server has answered the order.
fn right_click_buy(client: &mut Client, item: ItemId) {
    point_item(client, item, Place::Grid);
    let tick = client.tick();
    client.click(MouseButton::Right);
    client.until("the buy order answered", |c| {
        c.tick() > tick + 2 && c.core().source.pending_orders() == 0
    });
    client.frames(3);
}

fn shopper(search: &str, spares: &[ItemId]) -> (Server, Client) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--shop", &format!("upgrades:/{search}")],
    );
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Gold { amount: 10_000 });
    for item in spares {
        client.cheat(Cheat::Item { item: *item });
    }
    (server, client)
}

/// The owner's play: three Circlets, then the shop's right click on Wraith Band, again and again. Far
/// from the shop the parts wait in the stash and the Circlets stay in the bag; every click is one more
/// copy and none shows the line "already purchased", which this purchase never earned.
#[test]
fn a_second_and_third_wraith_band_bought_far_from_the_shop_buy_their_slippers_and_recipe_again() {
    let (_server, mut client) = shopper("wraith", &[ItemId::CIRCLET; 3]);
    client.stand_at(MID);
    for copies in 1..=3 {
        let gold = client.core().spendable_gold();
        right_click_buy(&mut client, ItemId::WRAITH_BAND);
        assert_eq!(
            crate::error_msg::shown(client.world_mut()),
            None,
            "copy {copies} is bought without a refusal"
        );
        // The purse also fills as the match runs: what was paid is at most the 350 of the two parts.
        client.until("the purchase paid", |c| {
            gold - c.core().spendable_gold() >= 300
        });
        let spent = gold - client.core().spendable_gold();
        assert!(
            spent <= 350,
            "copy {copies} costs a Slippers and a recipe, not {spent}"
        );
        client.until("the parts told", |c| {
            held(c, ItemId::SLIPPERS) == copies && held(c, ItemId::RECIPE_WRAITH_BAND) == copies
        });
    }
    assert_eq!(
        held(&client, ItemId::CIRCLET),
        3,
        "the Circlets wait for their parts"
    );
}

/// The same play at the shop: each copy is built as it is bought.
#[test]
fn three_wraith_bands_bought_at_the_shop_with_three_circlets_are_built_one_by_one() {
    let (_server, mut client) = shopper("wraith", &[ItemId::CIRCLET; 3]);
    // At the home shop, whatever the hero's spawn: the stash builds with the bag there.
    let map = client.core().session().expect("a match").map();
    let home = bota_core::fountain_pos(map, client.me().team);
    client.stand_at((home.x.to_int(), home.y.to_int()));
    for copies in 1..=3 {
        right_click_buy(&mut client, ItemId::WRAITH_BAND);
        // The server builds the copy in the tick after it is bought; the view follows.
        client.until("the copy built", |c| held(c, ItemId::WRAITH_BAND) == copies);
        assert_eq!(held(&client, ItemId::CIRCLET), 3 - copies);
    }
    assert!(
        cells(&mut client, Place::Grid)
            .iter()
            .any(|(id, _)| *id == ItemId::WRAITH_BAND)
    );
}
