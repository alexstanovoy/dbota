//! Selling in the strategy phase, end to end in the real client: two items
//! are chosen in the shop, a right click takes one purchase back directly,
//! and when the match begins the hero carries the one
//! item left, not the two. Drawn on the GPU in a headless compositor, so it is
//! left out of a plain `cargo test`; `tools/visual_check.sh` runs it.

mod common;

/// Refunds a chosen item with one right click: the match then
/// begins with one item in the bag, the one not sold.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn selling_in_the_strategy_phase_takes_the_choice_back() {
    let server = common::serve(&[
        "--players",
        "1",
        "--pick-secs",
        "5",
        "--tactical-secs",
        "30",
    ]);
    // The shop's two first consumables are right clicked (each is chosen),
    // the first slot of the inventory is right clicked, with no menu action.
    let script = "await:game;click:pick shadow fiend;click:pick ready;await:text:STRATEGY TIME;await:loaded;\
                  mouse:0.6,0.2306;wait:0.7;rclick;wait:0.7;\
                  mouse:0.577,0.2306;wait:0.7;rclick;wait:0.7;\
                  mouse:0.4625,0.309;wait:0.7;rclick;wait:0.7;\
                  await:horn;await:items:1;exit";
    common::play_script(&server.address, script, "sell");
}
