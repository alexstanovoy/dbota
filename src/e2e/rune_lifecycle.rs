//! Rune lifetimes in a real match. Bounties persist at the jungle spawner;
//! water and power runes are replaced on the river's two-minute cadence.
//! https://liquipedia.net/dota2/Runes

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bota_proto::{Cheat, CreepSide, EntityId, HeroId, RuneKind, RuneView};

use super::harness::{Client, Server};

const PATIENCE: Duration = Duration::from_secs(240);

/// Advance the real lockstep connection without rendering every intermediate
/// tick. Checkpoints below run the whole app and inspect its drawn state.
fn reach_minute(client: &mut Client, minute: u32) {
    let target = bota_core::rules::Schedule::of(client.core().pregame_ticks()).at_minute(minute);
    let deadline = Instant::now() + PATIENCE;
    while client.view().game_tick() < target {
        assert!(
            Instant::now() < deadline,
            "the real match reaches {minute}:00"
        );
        assert!(!client.core().lost, "the connection remains live");
        client.core_mut().pump(0.0);
        std::thread::yield_now();
    }
    client.frames(2);
}

fn visit(client: &mut Client, spot: usize) {
    let at = bota_core::DOTA_RUNE_SPOTS[spot].at;
    client.stand_at((at.x.to_int() + 400, at.y.to_int()));
    assert!(
        !client
            .me()
            .pos
            .within(at, bota_core::rules::units(bota_core::RUNE_PICKUP_RANGE)),
        "observing the runes does not pick them up"
    );
}

fn near(client: &Client, spot: usize) -> Vec<RuneView> {
    let at = bota_core::DOTA_RUNE_SPOTS[spot].at;
    client
        .view()
        .runes
        .iter()
        .copied()
        .filter(|r| r.pos.within(at, bota_core::rules::units(1000)))
        .collect()
}

fn drawn(client: &mut Client, spot: usize, kind: RuneKind, count: usize) {
    let at = bota_core::DOTA_RUNE_SPOTS[spot].at;
    let model = format!("rune.{}.model", crate::runes::art(kind));
    client.within(PATIENCE, "every visible rune has its own drawn body", |c| {
        let world = c.world_mut();
        world
            .query_filtered::<(&crate::loot::Idle, &Transform), With<crate::runes::RuneBody>>()
            .iter(world)
            .filter(|(idle, pos)| {
                idle.0 == model
                    && Vec2::new(pos.translation.x, -pos.translation.z)
                        .distance(Vec2::new(at.x.to_f32(), at.y.to_f32()))
                        < 1000.0
            })
            .count()
            == count
    });
}

fn jungle(client: &mut Client, count: usize, first: EntityId) {
    const RADIANT: usize = 3;
    visit(client, RADIANT);
    let runes = near(client, RADIANT);
    assert_eq!(
        runes.len(),
        count,
        "one more jungle bounty every four minutes"
    );
    assert!(runes.iter().all(|r| r.kind == RuneKind::Bounty));
    let positions: std::collections::BTreeSet<_> = runes.iter().map(|r| r.pos).collect();
    assert_eq!(
        positions.len(),
        count,
        "accumulated bounties occupy distinct positions"
    );
    assert!(
        runes.iter().any(|r| r.id == first),
        "the original bounty still exists"
    );
    drawn(client, RADIANT, RuneKind::Bounty, count);
    client.within(
        PATIENCE,
        "each bounty body stands at its own snapshot position",
        |c| {
            let world = c.world_mut();
            let bodies: Vec<_> = world
                .query_filtered::<(&crate::loot::Idle, &Transform), With<crate::runes::RuneBody>>()
                .iter(world)
                .filter(|(idle, _)| idle.0 == "rune.bounty.model")
                .map(|(_, transform)| Vec2::new(transform.translation.x, -transform.translation.z))
                .collect();
            runes.iter().all(|r| {
                bodies
                    .iter()
                    .any(|pos| pos.distance(Vec2::new(r.pos.x.to_f32(), r.pos.y.to_f32())) < 1.0)
            })
        },
    );
}

fn water(client: &mut Client) -> [EntityId; 2] {
    std::array::from_fn(|spot| {
        visit(client, spot);
        let water: Vec<_> = near(client, spot)
            .into_iter()
            .filter(|r| r.kind == RuneKind::Water)
            .collect();
        assert_eq!(
            water.len(),
            1,
            "exactly one water rune at river spot {spot}"
        );
        drawn(client, spot, RuneKind::Water, 1);
        water[0].id
    })
}

fn power(client: &mut Client) -> RuneView {
    let mut runes = Vec::new();
    for spot in [0, 1] {
        visit(client, spot);
        let seen = near(client, spot);
        assert!(
            !seen.iter().any(|r| r.kind == RuneKind::Water),
            "water is gone from both river spots"
        );
        for rune in seen.iter().filter(|r| r.kind.is_power()) {
            drawn(client, spot, rune.kind, 1);
        }
        runes.extend(seen.into_iter().filter(|r| r.kind.is_power()));
    }
    assert_eq!(runes.len(), 1, "one power rune across both river spots");
    runes[0]
}

#[test]
fn jungle_bounties_persist_while_untaken_water_and_power_runes_are_replaced() {
    let server = Server::start(1, &["--map", "0", "--runes", "all"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::StartGame);
    visit(&mut client, 3);
    let opening = near(&client, 3);
    assert_eq!(opening.len(), 1);
    let first = opening[0].id;
    jungle(&mut client, 1, first);
    reach_minute(&mut client, 2);
    jungle(&mut client, 1, first);
    let first_water = water(&mut client);
    reach_minute(&mut client, 4);
    let next_water = water(&mut client);
    assert!(
        next_water.iter().all(|id| !first_water.contains(id)),
        "both untaken water runes are replaced"
    );
    jungle(&mut client, 2, first);
    reach_minute(&mut client, 6);
    let six = power(&mut client);
    assert!(
        !client
            .view()
            .runes
            .iter()
            .any(|r| r.kind == RuneKind::Water)
    );
    jungle(&mut client, 2, first);
    reach_minute(&mut client, 8);
    let eight = power(&mut client);
    assert_ne!(
        six.id, eight.id,
        "untaken power is replaced, not accumulated"
    );
    assert_ne!(six.kind, eight.kind, "the rotation advances");
    jungle(&mut client, 3, first);
}
