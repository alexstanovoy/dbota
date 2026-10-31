//! A match shared with a client of an older build plays the items both hold: this client's
//! shop offers none past them, neither on the strategy screen before the start nor in the shop
//! once the match runs.

use bevy::prelude::*;
use bota_client_core::{Link, Transport, join, play};
use bota_core::Catalogue;
use bota_proto::{ClientMsg, HeroId, ItemId, ServerMsg};

use super::harness::{Client, Server};
use crate::shop::{Catalog, ShopCell};

/// The catalogue of a build made before the Wind Lace was appended: it holds neither the Wind
/// Lace nor the Blood Grenade nor any item appended after them, however many items this build
/// holds.
fn older() -> Catalogue {
    Catalogue::first(ItemId::WIND_LACE.0).expect("a prefix")
}

/// The client library of an older build: it says the older catalogue where this build's says
/// its own, and is the same in everything else.
struct OlderBuild(Link);

impl Transport for OlderBuild {
    fn recv(&mut self) -> std::io::Result<Option<ServerMsg>> {
        self.0.recv()
    }

    fn send(&mut self, msg: &ClientMsg) -> std::io::Result<()> {
        match msg {
            ClientMsg::Catalogue(_) => self.0.send(&ClientMsg::Catalogue(older().prefix())),
            _ => self.0.send(msg),
        }
    }
}

/// A seat held by a quiet client of the older build, which acknowledges what it is sent.
fn older_seat(server: &Server) {
    let link =
        Link::connect(&server.address, std::time::Duration::from_secs(60)).expect("connected");
    let mut client = OlderBuild(link);
    let seated = join(&mut client, "elder", HeroId::PUDGE, None).expect("a seat");
    std::thread::spawn(move || {
        let _ = play(&mut client, seated, &mut (), |_| Vec::new());
    });
}

/// What the client offers past the older catalogue, by where: the shop's catalog, every shop
/// cell drawn (the shop's and the strategy screen's) and what the client core offers. Empty
/// when it offers nothing past it.
fn offered_past_the_older_items(client: &mut Client) -> Vec<String> {
    let mut past = Vec::new();
    let catalog = client.world().resource::<Catalog>();
    for id in catalog.sections.iter().flat_map(|s| s.items.iter()) {
        if !older().holds(ItemId(*id)) {
            past.push(format!("catalog lists {id}"));
        }
    }
    let world = client.world_mut();
    for cell in world.query::<&ShopCell>().iter(world) {
        if !older().holds(ItemId(cell.0)) {
            past.push(format!("a cell of {}", cell.0));
        }
    }
    let core = client.core();
    if core.catalogue() != older() {
        past.push(format!("the core's catalogue is {:?}", core.catalogue()));
    }
    for item in [ItemId::WIND_LACE, ItemId::BLOOD_GRENADE] {
        if core.offers(item.0) {
            past.push(format!("the core offers {}", item.0));
        }
    }
    past
}

/// The two newest items this build sells are in its own shop, so leaving them out is the
/// match's catalogue at work, and both the strategy screen and the shop leave them out.
#[test]
fn a_match_shared_with_an_older_build_offers_only_the_items_both_hold() {
    assert!(Catalogue::FULL.sold(ItemId::WIND_LACE).is_some());
    assert!(Catalogue::FULL.sold(ItemId::BLOOD_GRENADE).is_some());
    let server = Server::start(2, &["--tactical-secs", "8"]);
    older_seat(&server);
    let mut client = Client::pick(&server, HeroId::SHADOW_FIEND, &[]);
    let mut past = Vec::new();
    client.until("the strategy screen", |c| {
        past = offered_past_the_older_items(c);
        !c.core().in_tactical_phase() || past.is_empty()
    });
    assert!(
        past.is_empty(),
        "the strategy screen offers only the items both builds hold: {past:?}"
    );
    client.until("the match under way", |c| c.core().ears().is_some());
    client.frames(10);
    let past = offered_past_the_older_items(&mut client);
    assert!(
        past.is_empty(),
        "the shop offers only the items both builds hold: {past:?}"
    );
}
