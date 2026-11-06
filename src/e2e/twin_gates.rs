//! The Twin Gates as Dota shows them, in a real server's match: both gates drawn where the map
//! places them, each glowing in its side's look; a hero's warp with its channel bar, its gate
//! playing its channel clip and glow while the far gate, out of sight, shows nothing, and the
//! hero drawn on the far gate's exit when it lands, with Dota's sounds at the gates and the
//! arrival's burst on it.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_core::DOTA_TWIN_GATES;
use bota_proto::{Cheat, EntityId, HeroId, StatusFlags, UnitKind, Vec2 as Spot};

use super::harness::{Client, Server};
use crate::channel_bar::ChannelBar;
use crate::units::{ModelBank, Unit};

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(30);

/// The gate standing at the map's spot of gate `index` (0 the north-western one).
fn gate(client: &Client, index: usize) -> Option<EntityId> {
    client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::TwinGate && u.pos == DOTA_TWIN_GATES[index].at)
        .map(|u| u.id)
}

/// The model a unit is drawn with and the clip it plays, once its body is there.
fn drawn(client: &mut Client, id: EntityId) -> Option<(String, &'static str)> {
    let body = client.body(id)?;
    let world = client.world_mut();
    let model = world.get::<Unit>(body)?.model?;
    let logical = world.resource::<ModelBank>().0[model].model.logical.clone();
    Some((logical, world.get::<crate::anim::Pose>(body)?.clip()))
}

fn clip(client: &mut Client, id: EntityId) -> Option<&'static str> {
    drawn(client, id).map(|(_, clip)| clip)
}

/// Where a unit's body is drawn, in the simulation's units.
fn drawn_at(client: &Client, id: EntityId) -> Option<Vec2> {
    let body = client.body(id)?;
    Some(client.world().get::<Unit>(body)?.at)
}

/// How many times a bound sound of a logical name was heard.
fn heard(client: &Client, logical: &str) -> u32 {
    client
        .world()
        .resource::<crate::fx::Heard>()
        .0
        .get(logical)
        .copied()
        .unwrap_or_default()
}

/// How many times the particle effect of a logical name started.
fn started(client: &Client, effect: &str) -> u32 {
    let world = client.world();
    world
        .get_resource::<crate::particles::Library>()
        .and_then(|library| library.effect(effect))
        .map_or(0, |effect| {
            world
                .resource::<crate::particles::Playing>()
                .started(effect)
        })
}

fn channelling(client: &Client) -> bool {
    client.me().statuses.bits & StatusFlags::CHANNELLING != 0
}

fn bar_shown(client: &mut Client) -> bool {
    let world = client.world_mut();
    world
        .query_filtered::<&InheritedVisibility, With<ChannelBar>>()
        .single(world)
        .is_ok_and(|seen| seen.get())
}

/// Right-clicks a gate, as a player starts the warp.
fn click_gate(client: &mut Client, index: usize) {
    let at = DOTA_TWIN_GATES[index].at;
    let id = gate(client, index).expect("the gate in view");
    client.core_mut().act(Intent::Smart {
        at: (at.x.to_f32(), at.y.to_f32()),
        unit: Some(id),
    });
}

/// Both gates of the Dota map stand drawn with the map's model where the map places them,
/// idling; seen, the north-western one glows in the Dire's look and the other in the Radiant's
/// (a particle in the fog is not drawn, whatever building it is on).
#[test]
fn the_dota_map_draws_both_gates_idling_each_in_its_sides_glow_when_seen() {
    let server = Server::start(1, &["--map", "0"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.within(SOON, "both gates drawn idling", |c| {
        (0..2).all(|index| {
            gate(c, index).is_some_and(|id| {
                drawn(c, id).is_some_and(|(model, clip)| {
                    model == "objective.team_portal.model" && clip == "idle"
                })
            })
        })
    });
    for (index, glow) in [(0, "twin_gate.dire_ambient"), (1, "twin_gate.ambient")] {
        let before = DOTA_TWIN_GATES[index].at + Spot::from_ints(0, -170);
        client.stand_at((before.x.to_int(), before.y.to_int()));
        client.within(SOON, "the gate seen glowing in its look", |c| {
            started(c, glow) > 0
        });
    }
}

/// A hero warps through the south-eastern gate and back: the channel bar shows, its gate plays
/// its channel clip, glows and hums while the far gate, which its side does not see, idles; the
/// hero is drawn on the far gate's exit when it lands, with the arrival's burst, and the gates
/// sound as it leaves one and comes out of the other.
#[test]
fn a_warp_shows_its_channel_and_lands_the_hero_drawn_on_the_far_exit() {
    let server = Server::start(1, &["--map", "2", "--objectives", "twin_gates"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let near = DOTA_TWIN_GATES[1].at + Spot::from_ints(0, -170);
    client.stand_at((near.x.to_int(), near.y.to_int()));
    let (south_east, north_west) = (
        gate(&client, 1).expect("the south-eastern gate"),
        gate(&client, 0).expect("the north-western gate"),
    );
    let hero = client.me().id;
    let hum = heard(&client, "sound.twin_gate.channel");
    click_gate(&mut client, 1);
    client.within(SOON, "the warp's channel drawn", |c| {
        channelling(c)
            && bar_shown(c)
            && clip(c, south_east) == Some("channel")
            && clip(c, hero) == Some("channel")
            && started(c, "twin_gate.active") > 0
            && heard(c, "sound.twin_gate.channel") > hum
    });
    assert_eq!(
        clip(&mut client, north_west),
        Some("idle"),
        "the far gate's glow is not shown to a side that does not see it"
    );
    assert_eq!(started(&client, "twin_gate.dire_active"), 0);
    // The camera stays on the near gate, to hear it as the hero leaves it and comes back out.
    client.core_mut().pinned = Some(bota_client_core::Pin::Unit(south_east));
    let left = heard(&client, "sound.twin_gate.channel_end");
    let exit = DOTA_TWIN_GATES[0].exit;
    client.within(SOON, "the hero landed and drawn on the far exit", |c| {
        c.me().pos == exit
            && drawn_at(c, hero)
                .is_some_and(|at| at.distance(Vec2::new(exit.x.to_f32(), exit.y.to_f32())) < 1.0)
    });
    client.within(
        SOON,
        "the landing's burst, the gate left sounding, the bar gone",
        |c| {
            started(c, "twin_gate.arrival") > 0
                && heard(c, "sound.twin_gate.channel_end") > left
                && !bar_shown(c)
                && clip(c, south_east) == Some("idle")
        },
    );
    // Back through the north-western gate, which the hero now stands by: it comes out by the
    // camera, at the south-eastern exit, with the gate's sound of a unit coming out.
    let came = heard(&client, "sound.twin_gate.appear");
    click_gate(&mut client, 0);
    client.within(SOON, "the warp back under way", |c| {
        channelling(c) && clip(c, north_west) == Some("channel")
    });
    client.within(SOON, "back on the south-eastern exit", |c| {
        c.me().pos == DOTA_TWIN_GATES[1].exit
    });
    client.within(SOON, "the unit come out sounding, its burst on it", |c| {
        heard(c, "sound.twin_gate.appear") > came
    });
}
