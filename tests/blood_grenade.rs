//! Blood Grenade, end to end in the real client: thrown at an enemy hero, it flies as Dota's
//! particle, bursts where it lands, sounds its throw, its landing and the splatter on the unit
//! it debuffs. Drawn on the GPU in a headless compositor, so it is left out of a plain
//! `cargo test`; `tools/visual_check.sh` runs it.

mod common;

use std::path::PathBuf;

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_blood_grenade_flies_bursts_and_sounds_on_the_units_it_hits() {
    let shots = std::env::var_os("DBOTA_SHOTS").map(PathBuf::from);
    // The hero walks out of the fountain's reach (it kills an enemy within it), an enemy hero is
    // made where the cursor is, and the grenade thrown at that very spot.
    let script = "await:game;click:pick shadow fiend;click:pick ready;await:horn;\
                  mouse:0.95,0.3;wait:0.3;rclick;wait:8;\
                  mouse:0.6,0.4;wait:0.3;say:-createhero pudge enemy;wait:1;\
                  say:-item blood_grenade;await:items:1;wait:0.5;\
                  key:Z;mouse:0.6,0.4;wait:0.3;lclick;\
                  await:heard:sound.blood_grenade.cast;await:started:item.blood_grenade;\
                  await:heard:sound.blood_grenade.location;await:heard:sound.blood_grenade.target;\
                  shot:blood_grenade.png;exit";
    common::play("blood_grenade", script, shots.as_deref());
}
