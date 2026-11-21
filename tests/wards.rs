//! Wards, end to end in the real client: an Observer and a Sentry Ward placed in a match are
//! drawn with Dota's ward model (the Sentry's in its own skin) and wear their ambient effects.
//! Drawn on the GPU in a headless compositor, so it is left out of a plain `cargo test`;
//! `tools/visual_check.sh` runs it.

mod common;

use std::path::PathBuf;

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn placed_wards_are_drawn_with_dotas_ward_models_and_effects() {
    let scratch = common::scratch::Scratch::new("wards");
    let shots =
        std::env::var_os("DBOTA_SHOTS").map_or_else(|| scratch.path("shots"), PathBuf::from);
    let server = common::serve(&[
        "--cheats",
        "--players",
        "1",
        "--pick-secs",
        "5",
        "--tactical-secs",
        "5",
        "--immortal-heroes",
    ]);
    let middle = bota_core::rules::MAP_SIZE / 2;
    let look = format!("{middle},{middle}");
    // Plant with a fixed camera at the river, clear of the base's no-ward volume. The mid tower
    // can wound the caster without stopping the placement. Hand out each ward separately.
    let script = "await:game;click:pick shadow fiend;click:pick ready;await:tick:1;await:loaded;\
                  say:-startgame;await:horn;\
                  mouse:0.5,0.5;frames:2;say:-teleport;await:settled;\
                  say:-item ward_observer;await:items:1;\
                  key:Z;mouse:0.57,0.57;frames:2;lclick;await:items:0;\
                  say:-item ward_sentry;await:items:1;\
                  key:Z;mouse:0.6,0.36;frames:2;lclick;\
                  await:model:ward.observer.model;await:model:ward.sentry.model;\
                  await:started:ward.observer.ambient;await:started:ward.sentry.ambient;\
                  await:heard:sound.ward.activate;shot:wards.png;exit";
    common::play_script_with(&server.address, script, "wards", &shots, &["--look", &look]);
}
