//! The way out, end to end in the real client: keys pressed through the
//! bindings and the menu's buttons clicked (`--script`), with no account and
//! no service. Escape on the menu asks to quit (Escape again and Cancel close
//! the question, OK ends the client with exit code 0); Escape in a live game
//! opens the dashboard, whose DISCONNECT goes back to the menu and RECONNECT
//! takes the seat again, and quitting from there warns the game is under
//! way; a replay is left after Dota's question, back to the menu.
//!
//! They open a window and draw, so they are left out of a plain `cargo
//! test`; `tools/visual_check.sh` runs them on a display. `DBOTA_QUIT_SHOTS`
//! names a folder to keep their screenshots in.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Seconds a run may take.
const PATIENCE: Duration = Duration::from_secs(240);

/// A scratch folder, emptied when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(what: &str) -> Scratch {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("menu-quit-{what}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Where the screenshots go: `DBOTA_QUIT_SHOTS`, else the scratch folder.
fn shots(scratch: &Scratch) -> PathBuf {
    std::env::var_os("DBOTA_QUIT_SHOTS")
        .filter(|d| !d.is_empty())
        .map_or_else(|| scratch.0.join("shots"), PathBuf::from)
}

/// Runs the client with `args` and `script`, its settings and data in
/// `scratch` (no service known, nothing of the user's touched): its exit
/// code (none when killed past [`PATIENCE`]) and the steps it printed.
fn run(scratch: &Scratch, args: &[&str], script: &str) -> (Option<i32>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dbota"))
        .args(args)
        .arg("--config")
        .arg(scratch.0.join("client.conf"))
        .arg("--screenshot-dir")
        .arg(shots(scratch))
        .args(["--script", script])
        .args(["--mute", "--no-vsync", "--display-mode", "windowed"])
        .env("XDG_DATA_HOME", scratch.0.join("data"))
        .env_remove("BOTA_SEAT_KEY")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("dbota runs");
    let stdout = child.stdout.take().expect("its stdout");
    let said = std::thread::spawn(move || {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut BufReader::new(stdout), &mut text).ok();
        text
    });
    let code = wait(&mut child);
    (code, said.join().unwrap_or_default())
}

fn wait(child: &mut Child) -> Option<i32> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("waited for") {
            return status.code();
        }
        if started.elapsed() > PATIENCE {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Asserts the client ended with exit code 0 having played every step.
fn played(what: &str, (code, said): (Option<i32>, String), script: &str) {
    let steps = script.split(';').filter(|s| !s.trim().is_empty()).count();
    let done = said.lines().filter(|l| l.starts_with("script: ")).count();
    assert!(
        code == Some(0) && done == steps && !said.contains("timed out"),
        "{what}: exit {code:?}, {done} of {steps} steps:\n{said}"
    );
}

#[test]
#[ignore = "opens a window: tools/visual_check.sh runs it on a display"]
fn escape_on_the_menu_asks_to_quit_and_ok_quits() {
    let scratch = Scratch::new("menu");
    let script = "await:menu;key:Escape;await:popup;shot:quit-confirm.png;key:Escape;\
                  await:nopopup;click:quit;await:popup;click:cancel;await:nopopup;\
                  key:Escape;await:popup;click:ok";
    played("the menu", run(&scratch, &[], script), script);
}

#[test]
#[ignore = "opens a window: tools/visual_check.sh runs it on a display"]
fn a_replay_is_left_for_the_menu() {
    let scratch = Scratch::new("replay");
    let replay = replay();
    let script = "await:game;wait:3;key:Escape;await:menu;wait:1;shot:replay-dashboard.png;\
                  click:disconnect;await:popup;shot:replay-leave.png;click:ok;await:nopopup;\
                  await:button:quit;shot:replay-left.png;key:Escape;await:popup;click:ok";
    let replay = replay.to_str().expect("a UTF-8 path");
    played(
        "a replay",
        run(&scratch, &["--replay", replay, "--pov", "0"], script),
        script,
    );
}

#[test]
#[ignore = "opens a window: tools/visual_check.sh runs it on a display"]
fn a_game_is_disconnected_from_and_rejoined() {
    let scratch = Scratch::new("game");
    let (mut server, address) = serve();
    let script = "await:game;wait:2;key:Escape;await:menu;wait:1;shot:game-dashboard.png;\
                  click:disconnect;await:button:reconnect;wait:1;shot:game-left.png;\
                  click:reconnect;await:game;wait:2;key:Escape;await:menu;click:quit;\
                  await:popup;shot:game-quit-confirm.png;click:ok";
    let result = run(&scratch, &["--addr", &address], script);
    let _ = server.kill();
    let _ = server.wait();
    played("a live game", result, script);
}

/// With no service configured the menu opens on the local pages, and a game's address typed
/// with `text:` (as an input method commits it) joins that game.
#[test]
#[ignore = "opens a window: tools/visual_check.sh runs it on a display"]
fn without_a_service_the_menu_is_local_and_takes_typed_text() {
    let scratch = Scratch::new("local");
    let (mut server, address) = serve();
    let script = format!(
        "await:menu;await:button:create;shot:local-first.png;click:direct connect;\
         text:{address};click:play;await:game;wait:2;key:Escape;await:menu;click:disconnect;\
         await:button:reconnect;click:quit;await:popup;click:ok"
    );
    let result = run(&scratch, &[], &script);
    let _ = server.kill();
    let _ = server.wait();
    played("a local menu", result, &script);
}

/// Dota's LEAVE GAME: from a seat in a match under way it asks, then abandons the match: the
/// other seat is told it won, and the client is back at its menu.
#[test]
#[ignore = "opens a window: tools/visual_check.sh runs it on a display"]
fn leaving_a_game_abandons_it_and_the_other_side_wins() {
    use bota_client_core::{Link, play};
    use bota_proto::HeroId;
    let scratch = Scratch::new("leave");
    let (mut server, address) = serve_seats(2);
    // The first to join holds the first seat; dbota takes the second.
    let (mut link, seated) =
        Link::join(&address, "stays", HeroId::SHADOW_FIEND, None).expect("a seat");
    let stays = std::thread::spawn(move || play(&mut link, seated, &mut (), |_| Vec::new()));
    let script = "await:game;click:pick shadow fiend;click:pick ready;wait:8;key:Escape;await:menu;\
                  wait:1;shot:leave-dashboard.png;click:leave game;await:popup;shot:leave-confirm.png;\
                  click:ok;await:button:quit;shot:leave-menu.png;click:quit;await:popup;click:ok";
    let result = run(&scratch, &["--addr", &address], script);
    let outcome = stays
        .join()
        .expect("the other seat")
        .expect("it saw the end");
    let _ = server.kill();
    let _ = server.wait();
    played("a seat that leaves", result, script);
    assert_eq!(outcome.winner, outcome.team, "the seat that stayed won");
}

/// The replay the visual check plays (`VISUAL_REPLAY`): a match of the wire
/// this client speaks.
fn replay() -> PathBuf {
    std::env::var_os("VISUAL_REPLAY")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .expect("VISUAL_REPLAY names a replay of the current wire")
}

/// A game server of one seat on a port of its own choosing, and its address.
fn serve() -> (Child, String) {
    serve_seats(1)
}

/// A game server of `seats` seats, quick to start, on a port of its own choosing.
fn serve_seats(seats: u8) -> (Child, String) {
    let mut child = Command::new(server_binary())
        .args(["--map", "2", "--port", "0", "--bind", "127.0.0.1"])
        .args([
            "--players",
            &seats.to_string(),
            "--pick-secs",
            "5",
            "--tactical-secs",
            "0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("bota_server starts");
    let mut output = BufReader::new(child.stdout.take().expect("piped"));
    let address = loop {
        let mut line = String::new();
        let read = output.read_line(&mut line).expect("the server's output");
        assert!(read > 0, "the server ended before it listened");
        if let Some(address) = line.trim().strip_prefix("bota_server listening on ") {
            break address.to_owned();
        }
    };
    // Its output is read to the end, so it never writes into a closed pipe.
    std::thread::spawn(move || for _ in output.lines() {});
    (child, address)
}

/// The game server: `BOTA_SERVER_BIN`, else built from the bota workspace
/// dbota builds against.
fn server_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("BOTA_SERVER_BIN") {
        return PathBuf::from(path);
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(&cargo)
        .args(["metadata", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata runs");
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("cargo metadata");
    let manifest = meta["packages"]
        .as_array()
        .and_then(|p| p.iter().find(|p| p["name"] == "bota_client_core"))
        .and_then(|p| p["manifest_path"].as_str())
        .expect("bota_client_core is a dependency");
    let root = Path::new(manifest)
        .ancestors()
        .nth(3)
        .expect("the bota workspace");
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/bota");
    let status = Command::new(&cargo)
        .args(["build", "--quiet", "--release", "-p", "bota_server"])
        .args(["--features", "websocket", "--target-dir"])
        .arg(&target)
        .current_dir(root)
        .status()
        .expect("cargo runs");
    assert!(status.success(), "bota_server builds");
    target.join("release/bota_server")
}
