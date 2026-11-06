//! Shared shrine progress rewinds under contest (Valve 7.41), then changes sides at zero.
//! https://www.dota2.com/patches/7.41
//! Departure rewinds existing progress rather than resetting it:
//! https://liquipedia.net/dota2/Shrine_of_Wisdom

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use bevy::prelude::*;
use bota_client_core::{Intent, IssuedOrder};
use bota_proto::{Cheat, CreepSide, EffectId, HeroId, Order, Target, Team, UnitKind, Vec2 as At};

use super::harness::{Client, Server, scripted_seat};

const SHRINE: At = bota_core::DOTA_SHRINES[0];

fn radiant_gatherer(server: &Server, phase: Arc<AtomicU8>) {
    let mut applied = None;
    scripted_seat(server, HeroId::PUDGE, move |session| {
        let next = phase.load(Ordering::SeqCst);
        if applied == Some(next) || session.own_hero().is_none() {
            return Vec::new();
        }
        applied = Some(next);
        let at = if next == 1 {
            SHRINE + At::from_ints(-200, 0)
        } else {
            session.statics().fountains()[0]
        };
        let mut orders = vec![IssuedOrder::hero(Order::Cheat {
            cheat: Cheat::Teleport { at },
        })];
        if next == 1 {
            orders.push(IssuedOrder::hero(Order::Move {
                target: Target::None,
            }));
        }
        orders
    });
}

fn shrine_match() -> (Server, Client, Arc<AtomicU8>) {
    let server = Server::start(
        2,
        &[
            "--map",
            "0",
            "--objectives",
            "shrines",
            "--tick-rate",
            "600",
        ],
    );
    let phase = Arc::new(AtomicU8::new(0));
    radiant_gatherer(&server, phase.clone());
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    assert_eq!(client.me().team, Team::Dire);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::AllVision { on: true });
    client.cheat(Cheat::StartGame);
    client.stand_at((SHRINE.x.to_int() + 1100, SHRINE.y.to_int()));
    client.until("the shrine's first activation", shrine_ready);
    (server, client, phase)
}

fn shrine_ready(client: &mut Client) -> bool {
    client
        .view()
        .units
        .iter()
        .any(|u| u.pos == SHRINE && u.effects.iter().any(|e| e.id == EffectId::SHRINE_READY))
}

fn radiant_inside(client: &Client) -> bool {
    client.view().units.iter().any(|u| {
        u.kind == UnitKind::Hero
            && u.team == Team::Radiant
            && u.pos.within(
                SHRINE,
                bota_proto::Fixed::from_int(bota_core::SHRINE_RADIUS),
            )
    })
}

fn experience(client: &Client, team: Team) -> i32 {
    client
        .view()
        .players
        .iter()
        .find(|p| p.team == team)
        .expect("the team has a seat")
        .xp
}

fn unclaimed(client: &mut Client) {
    assert_eq!(experience(client, Team::Radiant), 0);
    assert_eq!(experience(client, Team::Dire), 0);
    assert!(
        shrine_ready(client),
        "the contested shrine remains available"
    );
}

fn begin_contest(client: &mut Client, phase: &AtomicU8) -> (u32, u32) {
    phase.store(1, Ordering::SeqCst);
    client.until("Radiant enters the shrine", |c| radiant_inside(c));
    let first = client.tick();
    client.until("Radiant builds a partial countdown", |c| {
        unclaimed(c);
        c.tick() >= first + bota_core::rules::TICKS_PER_SECOND
    });
    client.core_mut().act(Intent::Cheat(Cheat::Teleport {
        at: SHRINE + At::from_ints(200, 0),
    }));
    client.core_mut().act(Intent::Hold);
    client.until("Dire contests the shrine", |c| {
        c.me().pos.within(
            SHRINE,
            bota_proto::Fixed::from_int(bota_core::SHRINE_RADIUS),
        )
    });
    let second = client.tick();
    unclaimed(client);
    (first, second)
}

fn handoff(client: &mut Client, phase: &AtomicU8, first: u32, second: u32) {
    client.until("both teams reverse the countdown", |c| {
        unclaimed(c);
        assert!(radiant_inside(c));
        assert!(
            c.me().pos.within(
                SHRINE,
                bota_proto::Fixed::from_int(bota_core::SHRINE_RADIUS)
            ),
            "the collector stays in the area instead of chasing"
        );
        c.tick() >= second + 8
    });
    phase.store(2, Ordering::SeqCst);
    client.until("Radiant leaves the partial countdown", |c| {
        !radiant_inside(c)
    });
    // Radiant increments from its entry through the tick before Dire enters,
    // then rewinds every tick, contested or not, to zero at 2*second-first-1.
    let zero = 2 * second - first - 1;
    assert!(
        client.tick() < zero,
        "some Radiant progress remains when it leaves"
    );
    let award = zero + bota_core::SHRINE_COUNTDOWN_TICKS;
    client.until(
        "Dire finishes after rewinding and gathering for three seconds",
        |c| {
            assert!(
                c.me().pos.within(
                    SHRINE,
                    bota_proto::Fixed::from_int(bota_core::SHRINE_RADIUS)
                ),
                "the collector stays after the opponent leaves"
            );
            if c.tick() < award {
                unclaimed(c);
                return false;
            }
            assert_eq!(c.tick(), award, "lockstep exposes the completion tick");
            assert_eq!(
                experience(c, Team::Dire),
                200,
                "entry ticks {first}/{second} require completion at {award}"
            );
            true
        },
    );
    assert_eq!(experience(client, Team::Radiant), 0);
}

#[test]
fn a_contested_shrine_rewinds_then_gives_the_second_team_its_full_countdown() {
    let (_server, mut client, phase) = shrine_match();
    let (first, second) = begin_contest(&mut client, &phase);
    handoff(&mut client, &phase, first, second);
    let expected = crate::hud::xp_ring(crate::hud::xp_share(client.me().level, 200));
    client.until(
        "the second team's experience drawn in the level ring",
        |c| {
            let world = c.world_mut();
            world
                .query::<(&BackgroundGradient, &InheritedVisibility, &ComputedNode)>()
                .iter(world)
                .any(|(gradient, shown, node)| {
                    shown.get() && node.size() != Vec2::ZERO && *gradient == expected
                })
        },
    );
}
