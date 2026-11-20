//! Wards, end to end in the real client: an Observer and a Sentry Ward placed in a match are
//! drawn with Dota's ward model (the Sentry's in its own skin) and wear their ambient effects.
//! Drawn on the GPU in a headless compositor, so it is left out of a plain `cargo test`;
//! `tools/visual_check.sh` runs it.

mod common;

use std::path::PathBuf;

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn placed_wards_are_drawn_with_dotas_ward_models_and_effects() {
    let shots = std::env::var_os("DBOTA_SHOTS").map(PathBuf::from);
    // Teleport past the base's no-ward volume and recenter the camera before planting.
    // Each ward is handed out (`-item`; both together would make a Ward Dispenser) and planted
    // from the hero's first item key at the ground beside him.
    let script = "await:game;click:pick shadow fiend;click:pick ready;await:tick:1;await:loaded;\
                  say:-startgame;await:horn;\
                  mouse:0.95,0.3;say:-teleport;await:settled;key:F1;frames:2;\
                  say:-item ward_observer;await:items:1;\
                  key:Z;mouse:0.57,0.57;frames:2;lclick;await:items:0;\
                  say:-item ward_sentry;await:items:1;\
                  key:Z;mouse:0.6,0.36;frames:2;lclick;\
                  await:model:ward.observer.model;await:model:ward.sentry.model;\
                  await:started:ward.observer.ambient;await:started:ward.sentry.ambient;\
                  await:heard:sound.ward.activate;shot:wards.png;exit";
    common::play("wards", script, shots.as_deref());
}
