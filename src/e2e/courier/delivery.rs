//! The courier's delivery ability hands items over at 400, independently of manual transfers.
//! https://liquipedia.net/dota2/Courier#Transfer_Items

use bota_client_core::{Intent, Slot};
use bota_proto::{Cheat, CreepSide, EntityId, Fixed, HeroId, ItemId, Vec2 as At};

use super::super::harness::{Client, MID, Server};

fn loaded_courier(distance: i32) -> (Server, Client, EntityId, At) {
    let server = Server::start(1, &["--mode", "lockstep"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    client.core_mut().act(Intent::Stop);
    client.cheat(Cheat::Item {
        item: ItemId::BOOTS,
    });
    let slot = client.slot_of(ItemId::BOOTS).expect("boots in the bag");
    let courier = client.core().my_courier().expect("the courier");
    client.core_mut().act(Intent::SelectCourier);
    client.cheat(Cheat::Teleport {
        at: At::from_ints(MID.0 + 100, MID.1),
    });
    client.core_mut().act(Intent::Stop);
    client.core_mut().act(Intent::SelectHero);
    client.core_mut().act(Intent::PutAt {
        slot,
        at: (MID.0 as f32 + 100.0, MID.1 as f32),
        unit: Some(courier),
    });
    client.until("boots loaded onto the courier", |c| {
        c.view()
            .unit(courier)
            .unwrap()
            .items
            .iter()
            .flatten()
            .any(|i| i.id == ItemId::BOOTS)
    });
    let goal = client.me().pos + At::from_ints(distance, 0);
    client.core_mut().act(Intent::SelectCourier);
    client.cheat(Cheat::Teleport { at: goal });
    client.core_mut().act(Intent::Stop);
    (server, client, courier, goal)
}

fn check_delivery(distance: i32) {
    let (server, mut client, courier, from) = loaded_courier(distance);
    let owner = client.me().pos;
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(3),
        ctrl: false,
    });
    let mut previous = from;
    let mut observed_tick = client.tick();
    let first_tick = observed_tick;
    client.until("boots handed over on reaching 400", |c| {
        if c.tick() != observed_tick {
            assert_eq!(
                c.tick(),
                observed_tick + 1,
                "lockstep delivery must expose every snapshot tick"
            );
            observed_tick = c.tick();
        }
        let carrier = c.view().unit(courier).expect("the courier");
        if distance <= 400 {
            assert_eq!(
                carrier.pos, from,
                "an in-range courier must deliver before walking"
            );
        }
        if !c.me().items.iter().flatten().any(|i| i.id == ItemId::BOOTS) {
            previous = carrier.pos;
            return false;
        }
        // Delivery runs before movement; the receiving snapshot may already include another step.
        assert!(previous.within(owner, Fixed::from_int(400)));
        if distance > 400 {
            let step = bota_core::per_tick(carrier.move_speed);
            assert!(
                !previous.within(owner, Fixed::from_int(400) - step),
                "handoff must occur within one movement step of first entering 400 range"
            );
        }
        assert!(
            !carrier
                .items
                .iter()
                .flatten()
                .any(|i| i.id == ItemId::BOOTS)
        );
        eprintln!(
            "courier distance {distance}: contiguous snapshots {first_tick}..{}",
            c.tick()
        );
        true
    });
    client.core_mut().act(Intent::SelectHero);
    server.frozen(|| {
        client.until("the delivered boots icon in the owner's inventory", |c| {
            let slot = c.slot_of(ItemId::BOOTS).expect("the delivered boots");
            super::super::purchase_destination::icon_in_slot(c, slot, ItemId::BOOTS)
        });
    });
}

#[test]
fn courier_delivery_at_350_fills_the_drawn_inventory_without_walking_closer() {
    check_delivery(350);
}

#[test]
fn courier_delivery_from_outside_400_hands_over_as_soon_as_it_enters_reach() {
    check_delivery(450);
}
