//! Blink Dagger, end to end in the real client: a hero that blinks plays Dota's start effect where
//! it stood, its end effect where it lands, and the sound of the dagger. Drawn on the GPU in a
//! headless compositor, so it is left out of a plain `cargo test`; `tools/visual_check.sh` runs it.

mod common;

use std::path::PathBuf;

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_blink_plays_dotas_start_and_end_effects_and_sound() {
    let shots = std::env::var_os("DBOTA_SHOTS").map(PathBuf::from);
    let script = "await:game;click:pick shadow fiend;click:pick ready;await:horn;\
                  say:-item blink;await:items:1;wait:1;\
                  key:Z;mouse:0.72,0.4;wait:0.3;lclick;\
                  await:started:item.blink_start;await:started:item.blink_end;\
                  await:heard:sound.blink.activate;wait:0.4;shot:blink.png;exit";
    common::play("blink", script, shots.as_deref());
}
