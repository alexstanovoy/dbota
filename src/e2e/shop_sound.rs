//! Dota's shop sounds, in a real match: the hero one plays walking into the range of a shop
//! hears `Shop.Available` once, and walking out `Shop.Unavailable`; other heroes in the shop
//! are not heard.

use std::time::Duration;

use bota_proto::{Cheat, HeroId};

use super::harness::{Client, MID, Server};
use super::sound_limits::listen;

const AVAILABLE: &str = "shop_available";
const UNAVAILABLE: &str = "shop_unavailable";
const PATIENCE: Duration = Duration::from_secs(20);

#[test]
fn walking_into_a_shop_sounds_it_once_and_other_heroes_there_do_not() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let shop = client.me().pos;
    let unavailable = listen(&mut client, UNAVAILABLE);
    client.stand_at(MID);
    client.within(PATIENCE, "the shop left behind", |_| {
        !unavailable.lock().expect("unpoisoned").is_empty()
    });
    client.frames(30);
    assert_eq!(
        unavailable.lock().expect("unpoisoned").len(),
        1,
        "walking out of the shop sounds once"
    );
    let available = listen(&mut client, AVAILABLE);
    for enemy in [false, true] {
        client.cheat(Cheat::CreateHero {
            hero: HeroId::PUDGE,
            enemy,
            at: shop,
        });
    }
    client.frames(120);
    assert_eq!(
        available.lock().expect("unpoisoned").len(),
        0,
        "another hero in the shop sounds it"
    );
    client.cheat(Cheat::Teleport { at: shop });
    client.within(PATIENCE, "the shop entered", |_| {
        !available.lock().expect("unpoisoned").is_empty()
    });
    client.frames(120);
    assert_eq!(
        available.lock().expect("unpoisoned").len(),
        1,
        "walking into the shop sounds once"
    );
}

#[test]
fn walking_into_a_secret_shop_sounds_it_once() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let secret = client
        .core()
        .statics()
        .and_then(|statics| statics.secret_shops().first().map(|shop| shop.pos))
        .expect("the map's secret shop");
    client.stand_at(MID);
    client.frames(60);
    let available = listen(&mut client, AVAILABLE);
    client.cheat(Cheat::Teleport { at: secret });
    client.within(PATIENCE, "the secret shop entered", |_| {
        !available.lock().expect("unpoisoned").is_empty()
    });
    client.frames(120);
    assert_eq!(
        available.lock().expect("unpoisoned").len(),
        1,
        "walking into the secret shop sounds once"
    );
}
