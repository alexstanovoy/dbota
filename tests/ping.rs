//! The ping the corner of the screen tells, end to end in the real client: a game server behind
//! a line that holds every byte back 25 ms each way, which no multiple of the 33 ms tick comes
//! near (50 ms of round trip), and then 35 ms each way (70 ms). The client times its connection
//! on the socket, so the corner tells the line's ping within a few milliseconds, not the tick's
//! multiple a ping read off the sim would. Drawn on the GPU in a headless compositor, so it is
//! left out of a plain `cargo test`; `tools/visual_check.sh` runs it.

mod common;

use std::time::Duration;

fn ping_told_over(one_way_ms: u64, low: u32, high: u32) {
    let server = common::serve(&["--players", "1", "--pick-secs", "5"]);
    let line = common::delayed(&server.address, Duration::from_millis(one_way_ms));
    let script = format!(
        "await:game;click:pick shadow fiend;click:pick ready;wait:3;\
         await:ping:{low},{high};exit"
    );
    common::play_script(&line, &script, "ping");
}

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn the_corner_tells_the_lines_ping_and_not_a_multiple_of_the_tick() {
    // A tick-read ping is 33, 66 or 100 here; the awaited ranges are 8 ms around the line.
    ping_told_over(25, 42, 58);
    ping_told_over(35, 62, 78);
}

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_click_shows_its_marker_and_the_bright_cursor_at_once_and_the_corner_tells_the_receipt() {
    // 400 ms each way: a click is received 800 ms after it is given, far more than a tick. The
    // marker and the lightened cursor are up as the order goes out, before the server confirms
    // it, and are plain again once it has; the corner then tells how long it took, as Dota's
    // `dota_report_order_latency` does. The server runs 4 times faster than the clock so that
    // the horn is not waited for long: game time is told as 30 ticks a second all the same.
    let server = common::serve(&[
        "--players",
        "1",
        "--pick-secs",
        "5",
        "--tactical-secs",
        "1",
        "--tick-rate",
        "120",
    ]);
    let line = common::delayed(&server.address, Duration::from_millis(400));
    common::play_script(
        &line,
        "await:game;click:pick shadow fiend;click:pick ready;await:horn;\
         mouse:0.5,0.4;wait:0.3;rclick;await:pending;await:orders:780,880;await:settled;exit",
        "orders",
    );
}
