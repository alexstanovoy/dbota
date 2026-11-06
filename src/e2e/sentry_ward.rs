//! A planted Sentry Ward as Dota draws it: the ambient effect Dota's item gives it (an eye over
//! the ward and a glow on its lamp) with no true-sight disc on the ground, which no control
//! point of Dota's item feeds.

use bota_client_core::{Intent, Slot};
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, MID, Server};
use crate::particles::{Library, Playing};

/// Control point 1 of the particle's root, where the ground discs read their radius.
const RADIUS_POINT: usize = 1;

#[test]
fn a_planted_sentry_draws_no_true_sight_disc_on_the_ground() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Item {
        item: ItemId::SENTRY_WARD,
    });
    client.until("the Sentry Ward carried", |c| {
        c.slot_of(ItemId::SENTRY_WARD).is_some()
    });
    let slot = client.slot_of(ItemId::SENTRY_WARD).expect("the ward");
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.core_mut().act(Intent::Aimed {
        at: (MID.0 as f32 + 300.0, MID.1 as f32),
        unit: None,
    });
    let started = |c: &Client| {
        let effect = c
            .world()
            .resource::<Library>()
            .effect("ward.sentry.ambient")
            .expect("the effect in the library");
        c.world().resource::<Playing>().started(&effect)
    };
    client.until("the Sentry's ambient effect", |c| started(c) > 0);
    let world = client.world();
    let effect = world
        .resource::<Library>()
        .effect("ward.sentry.ambient")
        .expect("the effect in the library");
    let radius = world
        .resource::<Playing>()
        .instances_of(&effect)
        .map(|i| i.nodes[0].control_points[RADIUS_POINT].position.x)
        .collect::<Vec<_>>();
    assert!(!radius.is_empty(), "the effect plays");
    assert!(
        radius.iter().all(|r| *r == 0.0),
        "the ground discs' radius (control point 1) is fed {radius:?}; Dota's item sets none"
    );
}
