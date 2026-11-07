//! Valve's once-per-item-per-Hero backpack grace:
//! https://www.dota2.com/datafeed/patchnotes?version=7.38&language=english
//! https://www.dota2.com/datafeed/patchnotes?version=7.38b&language=english
//! 7.38 introduced it; 7.38b extended it to five seconds.
use super::harness::{Client, MID, Server};
use bota_client_core::{ControlledUnit, Intent};
use bota_proto::{Cheat, CreepSide, HeroId, ItemId, ItemSlot};

// The shop UI also considers the courier; these tests need the hero's geometry.
fn hero_at_own_home_shop(client: &Client) -> bool {
    let session = client.core().session().expect("session");
    let hero = client.me();
    bota_core::at_home_shop(session.map(), hero.team, hero.pos)
}

fn item_in_lane() -> (Server, Client) {
    item_on_map(&[], MID)
}

fn item_on_map(extra: &[&str], at: (i32, i32)) -> (Server, Client) {
    let server = Server::start(1, extra);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::StartGame);
    client.stand_at(at);
    client.cheat(Cheat::Item {
        item: ItemId::POWER_TREADS,
    });
    client.until("Treads equipped", |c| {
        c.slot_of(ItemId::POWER_TREADS) == Some(0)
    });
    (server, client)
}

fn swap(client: &mut Client, from: u8, to: u8) {
    client.core_mut().act(Intent::Swap { from, to });
    client.until("swap acknowledged", |c| {
        c.slot_of(ItemId::POWER_TREADS) == Some(to)
    });
}

#[test]
fn a_first_backpack_return_inside_five_seconds_has_no_server_or_predicted_inactivity() {
    let (server, mut client) = item_in_lane();
    let strength = client.me().attributes.strength;
    swap(&mut client, 0, 6);
    assert!(
        client.me().attributes.strength < strength,
        "backpack removes stats even during grace"
    );
    let grace = client
        .core()
        .session()
        .expect("session")
        .own_player()
        .expect("player")
        .backpack
        .expect("own-side grace metadata");
    assert_ne!(grace.used & (1 << 6), 0);
    assert!(grace.grace_left[0] > 0);
    server.frozen(|| {
        client.core_mut().act(Intent::Swap { from: 6, to: 0 });
        let session = client.core().session().expect("session");
        let applies = client.tick() + bota_client_core::OrderLatency::measured(session).lands();
        assert_eq!(
            session
                .waits()
                .mute_left(ControlledUnit::Hero, ItemSlot(0), applies),
            Some(0),
            "first-entry grace is predicted before acknowledgement"
        );
    });
    client.until("first return acknowledged", |c| {
        c.slot_of(ItemId::POWER_TREADS) == Some(0)
    });
    let item = client.me().items[0].expect("returned Treads");
    assert_eq!(
        item.mute_left, 0,
        "7.38b first-entry grace avoids the new inactivity timer"
    );
    assert_eq!(
        client.me().attributes.strength,
        strength,
        "stats return immediately within grace"
    );
    assert_eq!(
        client.core().session().expect("session").waits().mute_left(
            ControlledUnit::Hero,
            ItemSlot(0),
            client.tick()
        ),
        Some(0)
    );
}

#[test]
fn an_authoritative_different_item_in_the_same_slot_does_not_inherit_the_old_prediction() {
    let (_server, mut client) = item_in_lane();
    client.cheat(Cheat::Item {
        item: ItemId::CIRCLET,
    });
    let circlet = client.slot_of(ItemId::CIRCLET).expect("Circlet");
    client.core_mut().send_order(bota_proto::Order::Put {
        slot: ItemSlot(circlet),
        target: bota_proto::Target::None,
    });
    client.until("Circlet on the ground", |c| {
        c.view().loot.iter().any(|i| i.item == ItemId::CIRCLET)
    });
    let ground = client
        .view()
        .loot
        .iter()
        .find(|i| i.item == ItemId::CIRCLET)
        .expect("ground Circlet")
        .id;
    swap(&mut client, 0, 6);
    let expired = client.tick() + bota_core::rules::BACKPACK_GRACE_TICKS;
    client.until("the first-entry grace expires", |c| c.tick() >= expired);
    swap(&mut client, 6, 0);
    assert!(client.me().items[0].expect("Treads").mute_left > 0);
    client.core_mut().send_order(bota_proto::Order::Put {
        slot: ItemSlot(0),
        target: bota_proto::Target::None,
    });
    client.until("inactive Treads really left their slot", |c| {
        c.me().items[0].is_none()
    });
    client.core_mut().send_order(bota_proto::Order::Take {
        target: bota_proto::Target::Unit(ground),
    });
    client.until("a different stack fills the old slot", |c| {
        c.me().items[0].is_some_and(|i| i.id == ItemId::CIRCLET)
    });
    assert_eq!(client.me().items[0].expect("Circlet").mute_left, 0);
    assert_eq!(
        client.core().session().expect("session").waits().mute_left(
            ControlledUnit::Hero,
            ItemSlot(0),
            client.tick()
        ),
        Some(0),
        "an authoritative active Circlet cannot inherit the previous Treads' slot timer"
    );
}

fn exhaust_grace(client: &mut Client) {
    let until = client.tick() + bota_core::rules::BACKPACK_GRACE_TICKS;
    client.until("five-second first-entry grace expires", |c| {
        c.tick() >= until
    });
}

#[test]
fn a_second_backpack_visit_predicts_and_applies_the_normal_six_second_inactivity() {
    let (server, mut client) = item_in_lane();
    swap(&mut client, 0, 6);
    swap(&mut client, 6, 0);
    assert_eq!(client.me().items[0].expect("first return").mute_left, 0);
    swap(&mut client, 0, 6);
    let metadata = client
        .core()
        .session()
        .expect("session")
        .own_player()
        .expect("player")
        .backpack
        .expect("own grace");
    assert_eq!(
        metadata.grace_left[0], 0,
        "7.38: only once per item per Hero"
    );
    server.frozen(|| {
        client.core_mut().act(Intent::Swap { from: 6, to: 0 });
        let session = client.core().session().expect("session");
        let applies = client.tick() + bota_client_core::OrderLatency::measured(session).lands();
        assert_eq!(
            session
                .waits()
                .mute_left(ControlledUnit::Hero, ItemSlot(0), applies),
            Some(bota_core::rules::BACKPACK_MUTE_TICKS)
        );
    });
    client.until("second return acknowledged", |c| {
        c.slot_of(ItemId::POWER_TREADS) == Some(0)
    });
    assert!(client.me().items[0].expect("second return").mute_left > 0);
    let facts = client.core().session().expect("session").waits().snapshot();
    assert_eq!(
        facts.mute_left(ControlledUnit::Hero, ItemSlot(0), client.tick()),
        Some(client.me().items[0].expect("returned stack").mute_left),
        "stored readiness facts match the authoritative timer after settlement"
    );
}

#[test]
fn entering_the_enemy_base_shop_clears_inactivity_before_the_normal_deadline() {
    // Valve: https://www.dota2.com/springcleaning2018/
    // Both base shops are usable: https://liquipedia.net/dota2/Portal:Items
    let (_server, mut client) = item_in_lane();
    swap(&mut client, 0, 6);
    exhaust_grace(&mut client);
    swap(&mut client, 6, 0);
    let left = client.me().items[0].expect("Treads").mute_left;
    assert!(left > 0);
    let natural_expiry = client.tick() + left;
    let shop = bota_core::fountain_pos(
        client.core().session().expect("session").map(),
        bota_proto::Team::Dire,
    );
    // Only the teleport, not `stand_at`: that also waits for the camera to glide to the hero, a
    // glide smoothed over wall time, so the ticks it lets pass vary with the machine's load (up to
    // nearly the whole six seconds). Lockstep ticks follow the client's frames, not the clock.
    client.cheat(Cheat::Teleport {
        at: bota_proto::Vec2::from_ints(shop.x.to_int(), shop.y.to_int()),
    });
    client.until(
        "the inactivity is cleared or its natural deadline passes",
        |c| c.tick() >= natural_expiry || c.me().items[0].expect("Treads").mute_left == 0,
    );
    assert!(
        client.tick() < natural_expiry,
        "the shop should clear the inactivity before its natural expiry at tick {natural_expiry}"
    );
    assert!(client.core().at_any_shop());
    assert!(!hero_at_own_home_shop(&client));
    assert_eq!(
        client.me().items[0].expect("Treads").mute_left,
        0,
        "any shop includes the opposing base shop"
    );
    assert_eq!(
        client.core().session().expect("session").waits().mute_left(
            ControlledUnit::Hero,
            ItemSlot(0),
            client.tick()
        ),
        Some(0)
    );
}

#[test]
fn the_secret_shop_exemption_matches_the_server_for_an_expired_first_entry() {
    let (server, mut client) = item_on_map(&["--map", "0"], (4260, 10950));
    swap(&mut client, 0, 6);
    exhaust_grace(&mut client);
    client.stand_at((4260, 10950));
    assert!(client.core().at_any_shop());
    assert!(
        !hero_at_own_home_shop(&client),
        "the hero is outside its own fountain shop"
    );
    server.frozen(|| {
        client.core_mut().act(Intent::Swap { from: 6, to: 0 });
        let session = client.core().session().expect("session");
        let applies = client.tick() + bota_client_core::OrderLatency::measured(session).lands();
        assert_eq!(
            session
                .waits()
                .mute_left(ControlledUnit::Hero, ItemSlot(0), applies),
            Some(0)
        );
    });
    client.until("secret-shop return applied", |c| {
        c.slot_of(ItemId::POWER_TREADS) == Some(0)
    });
    assert_eq!(client.me().items[0].expect("Treads").mute_left, 0);
}

#[test]
fn a_partial_backpack_merge_does_not_give_its_pending_remainder_new_grace() {
    let (server, mut client) = item_in_lane();
    for _ in 0..3 {
        client.cheat(Cheat::Item {
            item: ItemId::MANGO,
        });
    }
    let mango = client.slot_of(ItemId::MANGO).expect("first Mango stack");
    client.core_mut().act(Intent::Swap { from: mango, to: 6 });
    client.until("three Mangos backpacked", |c| {
        c.me().items[6].is_some_and(|i| i.id == ItemId::MANGO && i.charges == Some(3))
    });
    for _ in 0..2 {
        client.cheat(Cheat::Item {
            item: ItemId::MANGO,
        });
    }
    let target = client
        .slot_of(ItemId::MANGO)
        .expect("fresh inventory stack");
    assert_eq!(
        client.me().items[usize::from(target)]
            .expect("target")
            .charges,
        Some(2)
    );
    exhaust_grace(&mut client);
    server.frozen(|| {
        client.core_mut().act(Intent::Swap { from: 6, to: target });
        client.core_mut().act(Intent::Swap { from: 6, to: 2 });
        let session = client.core().session().expect("session");
        let applies = client.tick() + bota_client_core::OrderLatency::measured(session).lands();
        assert_eq!(session.waits().mute_left(ControlledUnit::Hero, ItemSlot(2), applies), Some(bota_core::rules::BACKPACK_MUTE_TICKS), "a merge leaves the expired source remainder in place; it is not a fresh displaced stack");
    });
    client.until("both dependent merge/return commands applied", |c| {
        c.me().items[2].is_some_and(|i| i.id == ItemId::MANGO && i.charges == Some(2))
            && c.me().items[6].is_none()
    });
    assert_eq!(
        client.me().items[usize::from(target)]
            .expect("merged target")
            .charges,
        Some(3)
    );
    assert!(client.me().items[2].expect("returned remainder").mute_left > 0);
}

#[test]
fn a_late_application_keeps_pending_backpack_history_after_the_estimated_tick() {
    let (server, mut client) = item_on_map(&["--order-delay-ticks", "30"], MID);
    swap(&mut client, 0, 6);
    swap(&mut client, 6, 0);
    assert_eq!(client.me().items[0].expect("first return").mute_left, 0);
    let issued = client.tick();
    client.core_mut().act(Intent::Swap { from: 0, to: 6 });
    client.until(
        "past the 16-tick plan, before the actual 31-tick application",
        |c| {
            c.tick() >= issued + 18 && c.me().items[0].is_some_and(|i| i.id == ItemId::POWER_TREADS)
        },
    );
    assert!(
        client.tick() < issued + 30,
        "the server has not applied this move yet"
    );
    server.frozen(|| {
        client.core_mut().act(Intent::Swap { from: 6, to: 0 });
        let session = client.core().session().expect("session");
        let applies = client.tick() + bota_client_core::OrderLatency::measured(session).lands();
        assert_eq!(
            session
                .waits()
                .mute_left(ControlledUnit::Hero, ItemSlot(0), applies),
            Some(bota_core::rules::BACKPACK_MUTE_TICKS),
            "estimated time cannot erase an unconfirmed second-visit stack"
        );
    });
    let after = client.tick() + 65;
    client.until("delayed round trip settled", |c| {
        c.tick() >= after && c.slot_of(ItemId::POWER_TREADS) == Some(0)
    });
    assert!(client.me().items[0].expect("second return").mute_left > 0);
}
