//! Blood Grenade, end to end in the real client: thrown at an enemy hero, it flies as Dota's
//! particle, bursts where it lands, sounds its throw, its landing and the splatter on the unit
//! it debuffs. Drawn on the GPU in a headless compositor, so it is left out of a plain
//! `cargo test`; `tools/visual_check.sh` runs it.

mod common;

use std::path::PathBuf;

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_blood_grenade_flies_bursts_and_sounds_on_the_units_it_hits() {
    let scratch = common::scratch::Scratch::new("blood-grenade");
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
    // Stage the cast near the river, with a fixed camera and no creep wave. A target hit by
    // another source can still take damage, but it cannot disappear before the grenade lands.
    let script = "await:game;click:pick shadow fiend;click:pick ready;await:tick:1;await:loaded;\
                  say:-disablecreepspawn;say:-startgame;await:horn;\
                  mouse:0.5,0.5;frames:2;say:-teleport;await:settled;\
                  say:-item blood_grenade;await:items:1;\
                  mouse:0.6,0.4;say:-createhero pudge enemy;await:model:hero.pudge.model;\
                  key:Z;mouse:0.6,0.4;frames:2;lclick;\
                  await:heard:sound.blood_grenade.cast;await:started:item.blood_grenade;\
                  await:heard:sound.blood_grenade.location;await:heard:sound.blood_grenade.target;\
                  shot:blood_grenade.png;exit";
    common::play_script_with(
        &server.address,
        script,
        "blood_grenade",
        &shots,
        &["--look", &look],
    );
}
