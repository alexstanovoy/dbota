//! Items on the ground, end to end in the real client: an item dragged out of the bag lies in the
//! world drawn with Dota's model of it (its own where items.txt has one, else the default
//! box), and a right click on it takes it up again. Drawn on the GPU in a headless compositor, so
//! it is left out of a plain `cargo test`; `tools/visual_check.sh` runs it.

mod common;

use std::path::PathBuf;

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn an_item_on_the_ground_is_drawn_with_its_model_and_taken_up_by_a_click() {
    let shots = std::env::var_os("DBOTA_SHOTS").map(PathBuf::from);
    // Clarity (its own model) and a Mantle of Intelligence (the default box) are handed out and
    // each dragged from the first slot to the ground, then the second is clicked up.
    let script = "await:game;click:pick shadow fiend;click:pick ready;await:horn;\
                  say:-item clarity;await:items:1;wait:1;\
                  mouse:0.6775,0.869;wait:0.3;ldown;mouse:0.56,0.58;wait:0.3;lup;\
                  await:items:0;await:loot:item.clarity.model;\
                  say:-item mantle;await:items:1;wait:1;\
                  mouse:0.6775,0.869;wait:0.3;ldown;mouse:0.62,0.5;wait:0.3;lup;\
                  await:items:0;await:loot:item.default.model;wait:2;\
                  mouse:0.62,0.5;wait:0.5;shot:ground_items.png;rclick;await:items:1;exit";
    common::play("ground_items", script, shots.as_deref());
}
