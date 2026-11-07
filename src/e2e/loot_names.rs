//! The name over an item lying on the ground, in a replay watched through a seat: told for what
//! that seat's side sees and not for what only the other side does.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, MID, Server, quiet_seat};

const SOON: Duration = Duration::from_secs(30);

/// Whether the name over the hovered item shows.
fn name_shown(client: &mut Client) -> bool {
    let world = client.world_mut();
    world
        .query_filtered::<&Visibility, With<crate::loot::ItemName>>()
        .iter(world)
        .any(|shown| *shown != Visibility::Hidden)
}

/// A replay of a match in which seat 1 dropped a tango at the middle of the map, far from seat 0's
/// fountain; where it lies, and the tick it lay at.
fn record_a_drop() -> (std::path::PathBuf, (f32, f32), u32) {
    let replay =
        crate::test_support::scratch_path(&format!("loot-names-{}.brp", std::process::id()));
    let mut server = Server::start(
        2,
        &[
            "--replay",
            replay.to_str().expect("UTF-8"),
            "--empty-after-secs",
            "1",
        ],
    );
    quiet_seat(&server, HeroId::PUDGE);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Item {
        item: ItemId::TANGO,
    });
    let slot = client.slot_of(ItemId::TANGO).expect("the tango in the bag");
    let spot = (MID.0 as f32 + 300.0, MID.1 as f32);
    client.core_mut().act(Intent::PutAt {
        slot,
        at: spot,
        unit: None,
    });
    client.within(SOON, "the tango on the ground", |c| {
        c.view().loot.iter().any(|l| l.item == ItemId::TANGO)
    });
    client.frames(30);
    let tick = client.tick();
    drop(client);
    server.wait();
    (replay, spot, tick)
}

/// Plays the replay watched through seat `pov` to `tick`, the camera free over `spot`, the pointer
/// on it.
fn watch(replay: &std::path::Path, pov: &str, spot: (f32, f32), tick: u32) -> Client {
    let mut client = Client::run(&["--replay", replay.to_str().expect("UTF-8"), "--pov", pov]);
    client.until("the replay at the dropped item", |c| {
        let now = c.tick();
        if let bota_client_core::Source::Replay(player) = &mut c.core_mut().source {
            if now < tick {
                player.advance_ticks(f64::from(tick - now));
            }
            player.paused = now >= tick;
        }
        now >= tick && !c.view().loot.is_empty()
    });
    client.core_mut().pinned = None;
    {
        let world = client.world_mut();
        let ground = world.resource::<crate::map::Ground>().at(spot.0, spot.1);
        world
            .query::<&mut crate::camera::RtsCamera>()
            .single_mut(world)
            .expect("the game's camera")
            .focus = Vec3::new(spot.0, ground, -spot.1);
    }
    client.frames(5);
    let pixel = client.screen_of(spot).expect("on screen");
    client.mouse(pixel);
    client
}

/// The name of an item the watched seat's side does not see is not shown, though the replay's view
/// holds it; seen through the seat that does see it, it is.
#[test]
fn an_item_named_in_a_replay_is_one_the_watched_side_sees() {
    let (replay, spot, tick) = record_a_drop();
    let mut blind = watch(&replay, "0", spot, tick);
    assert!(
        !name_shown(&mut blind),
        "seat 0's side does not see the item"
    );
    drop(blind);
    let mut seeing = watch(&replay, "1", spot, tick);
    assert!(name_shown(&mut seeing), "seat 1's side dropped it");
}
