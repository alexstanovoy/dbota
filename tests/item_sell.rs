//! Selling in the strategy phase, end to end in the real client: two items
//! are chosen in the shop, a right click on one opens the item menu, its Sell
//! takes the choice back, and when the match begins the hero carries the one
//! item left, not the two. Drawn on the GPU in a headless compositor, so it is
//! left out of a plain `cargo test`; `tools/visual_check.sh` runs it.

mod common;

/// Sells a chosen item from the strategy phase's item menu: the match then
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
    // the first slot of the inventory is right clicked, and Sell clicked.
    let script = "await:game;click:pick shadow fiend;click:pick ready;wait:6;\
                  mouse:0.6516,0.2157;wait:0.7;rclick;wait:0.7;\
                  mouse:0.6276,0.2157;wait:0.7;rclick;wait:0.7;\
                  mouse:0.4766,0.275;wait:0.7;rclick;wait:0.7;\
                  mouse:0.49,0.295;wait:0.7;lclick;wait:0.7;\
                  await:horn;wait:4;await:items:1;exit";
    common::play_script(&server.address, script, "sell");
}
