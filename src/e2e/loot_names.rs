//! The name over an item lying on the ground, in a replay watched through a seat: told for what
//! that seat's side sees and not for what only the other side does.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, Server, quiet_seat};

const SOON: Duration = Duration::from_secs(30);
// Outside enemy tower sight and the mid lane, even if loading carries the match into daytime.
const DROP_AT: (i32, i32) = (12000, 4000);

/// Whether the name over the hovered item shows.
fn name_shown(client: &mut Client) -> bool {
    let world = client.world_mut();
    world
        .query_filtered::<&Visibility, With<crate::loot::ItemName>>()
        .iter(world)
        .any(|shown| *shown != Visibility::Hidden)
}

/// A replay of a match in which seat 1 dropped a tango away from seat 0's fountain and lane;
/// where it lies, and the tick it lay at.
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
    client.stand_at(DROP_AT);
    client.cheat(Cheat::Item {
        item: ItemId::TANGO,
    });
    let slot = client.slot_of(ItemId::TANGO).expect("the tango in the bag");
    // Drop within reach instead of depending on a path through the surrounding trees.
    let at = client.me().pos;
    let spot = (at.x.to_f32(), at.y.to_f32());
    client.core_mut().act(Intent::PutAt {
        slot,
        at: spot,
        unit: None,
    });
    client.within(SOON, "the tango on the ground", |c| {
        c.view().loot.iter().any(|l| l.item == ItemId::TANGO)
    });
    let dropped = client
        .view()
        .loot
        .iter()
        .find(|loot| loot.item == ItemId::TANGO)
        .expect("the dropped tango")
        .pos;
    let spot = (dropped.x.to_f32(), dropped.y.to_f32());
    let tick = client.tick();
    drop(client);
    server.wait();
    (replay, spot, tick)
}

/// Plays the replay watched through seat `pov` to `tick`, the camera free over `spot`, the pointer
/// on it.
fn watch(replay: &std::path::Path, pov: &str, spot: (f32, f32), tick: u32) -> Client {
    // A fixed look suppresses late initial hero placement while its model is loading.
    let look = format!("{},{}", spot.0, spot.1);
    let mut client = Client::run(&[
        "--replay",
        replay.to_str().expect("UTF-8"),
        "--pov",
        pov,
        "--look",
        &look,
    ]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::ZERO,
        ));
    client.core_mut().pinned = None;
    let bota_client_core::Source::Replay(player) = &mut client.core_mut().source else {
        panic!("a replay");
    };
    player.paused = true;
    client.until("the replay's first snapshot", |c| c.core().view().is_some());
    // Retain the replay, including its decoder state, while the UI loads. Loading can unpause
    // playback; an idle source lets presentation time run without consuming replay time or data.
    let source = std::mem::replace(
        &mut client.core_mut().source,
        crate::main_menu::idle().0.source,
    );
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(100),
        ));
    client.until("the replay scenery and loading screen ready", |c| {
        c.world().resource::<crate::loading::Loading>().done()
            && c.world().resource::<crate::minimap::PhotoTaken>().0
    });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::ZERO,
        ));
    client.core_mut().source = source;
    let advance = tick
        .checked_sub(client.tick())
        .expect("the item tick follows the first snapshot");
    if let bota_client_core::Source::Replay(player) = &mut client.core_mut().source {
        // This queues an additive clock jump. Queue it once, even if decoding needs many frames.
        player.advance_ticks(f64::from(advance));
    }
    client.until("the replay at exactly the dropped item tick", |c| {
        c.tick() == tick && !c.view().loot.is_empty()
    });
    client.within(SOON, "the cursor at the item with its POV fog ready", |c| {
        let Some(pixel) = c.screen_of(spot) else {
            return false;
        };
        c.mouse(pixel);
        c.world()
            .resource::<crate::camera::CursorGround>()
            .0
            .is_some_and(|at| Vec2::new(at.x, -at.z).distance(Vec2::new(spot.0, spot.1)) < 1.0)
            && c.world().resource::<crate::fog::FogGrid>().worked_out_at() == Some(c.tick())
    });
    assert_eq!(
        client.tick(),
        tick,
        "loading and aiming must not advance the replay"
    );
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
    seeing.within(SOON, "the visible item's name laid out", name_shown);
    assert_eq!(seeing.tick(), tick, "naming must not advance the replay");
    assert!(name_shown(&mut seeing), "seat 1's side dropped it");
}
