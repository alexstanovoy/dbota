//! A tower's line to its target, end to end in the real client: a hero
//! teleported (a cheat match) to a spot in an enemy tower's reach has the
//! tower's line drawn to it, and teleported on to a spot out of reach, still
//! in the tower's sight, the line is gone. Drawn on the GPU in a headless
//! compositor, so it is left out of a plain `cargo test`;
//! `tools/visual_check.sh` runs it.

mod common;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Seconds a run may take.
const PATIENCE: Duration = Duration::from_secs(240);

/// The window the test plays in, in pixels, and the minimap's frame in it: its inner top-left
/// corner and its inner side (the frame is 250 px with a 3 px border, bottom left).
const WINDOW: (f32, f32) = (1600.0, 900.0);
const MINIMAP_CORNER: (f32, f32) = (11.0, 650.0);
const MINIMAP_SIDE: f32 = 244.0;
/// The map's side in world units.
const MAP_SIDE: f32 = 16384.0;
/// The minimap spot the camera is put on. The hero is teleported under the pointer, and the
/// camera's tilt puts what is under the window's middle about (1320, 970) units up the map from the
/// camera's spot: the middle then falls 110 units from the Dire mid tier-one tower (9740, 9868),
/// which stays in view all along (a unit out of the camera's reach is not drawn, and nor is its
/// line).
const CAMERA: (f32, f32) = (8530.0, 8940.0);
/// Where on the window the hero is teleported to: by the tower, and the top of the window, about 1200
/// units further up the map: out of the tower's 700 reach but in its day sight.
const NEAR_THE_TOWER: (f32, f32) = (0.5, 0.5);
const OUT_OF_ITS_REACH: (f32, f32) = (0.5, 0.1);
const LOOKOUT: (f32, f32) = (0.75, 0.1);
/// How far the camera stands from the ground.
const DISTANCE: &str = "5000";

/// A game server of one seat with a strategy phase long enough to play in,
/// on a port of its own choosing, and its address.
fn serve() -> (Child, String) {
    let mut child = Command::new(server_binary())
        .args(["--map", "2", "--port", "0", "--bind", "127.0.0.1"])
        .args([
            "--players",
            "1",
            "--pick-secs",
            "5",
            "--tactical-secs",
            "30",
            "--cheats",
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
    let root = std::path::Path::new(manifest)
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

/// The camera put on a spot of the world by the minimap.
fn camera_on(world: (f32, f32)) -> String {
    let x = (MINIMAP_CORNER.0 + world.0 / MAP_SIDE * MINIMAP_SIDE) / WINDOW.0;
    let y = (MINIMAP_CORNER.1 + (1.0 - world.1 / MAP_SIDE) * MINIMAP_SIDE) / WINDOW.1;
    format!("mouse:{x:.4},{y:.4};lclick;wait:0.7;")
}

/// The pointer put on a spot of the window and the cheats given there, the last of them the hero's
/// teleport to it.
fn at_the_pointer(spot: (f32, f32), cheats: &[&str]) -> String {
    let mut steps = format!("mouse:{},{};wait:0.7;", spot.0, spot.1);
    for cheat in cheats {
        steps += &format!("say:{cheat};wait:0.7;");
    }
    steps
}

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_tower_stops_drawing_its_line_once_the_hero_is_out_of_reach() {
    let scratch =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tower-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("a scratch folder");
    let (mut server, address) = serve();
    let script = format!(
        "await:game;click:pick shadow fiend;click:pick ready;wait:6;\
         say:-disablecreepspawn;wait:0.7;say:-startgame;await:horn;say:-killcreeps;wait:0.7;{}{}\
         await:towershots:1;{}{}expect:towershots:0;wait:3;expect:towershots:0;exit",
        camera_on(CAMERA),
        at_the_pointer(NEAR_THE_TOWER, &["-teleport"]),
        // An enemy hero stands off to the side, far enough not to fight, so that the tower's side sees
        // the spot.
        at_the_pointer(LOOKOUT, &["-createhero nevermore enemy"]),
        at_the_pointer(OUT_OF_ITS_REACH, &["-teleport"]) + "key:H;wait:0.7;",
    );
    let mut child = common::client()
        .args(["--addr", &address, "--config"])
        .arg(scratch.join("client.conf"))
        .args([
            "--script",
            &script,
            "--distance",
            DISTANCE,
            "--mute",
            "--no-vsync",
            "--display-mode",
            "windowed",
        ])
        .env("XDG_DATA_HOME", scratch.join("data"))
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
    let started = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait().expect("waited for") {
            break status.code();
        }
        if started.elapsed() > PATIENCE {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = server.kill();
    let _ = server.wait();
    let said = said.join().unwrap_or_default();
    let steps = script.split(';').filter(|s| !s.trim().is_empty()).count();
    let done = said.lines().filter(|l| l.starts_with("script: ")).count();
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        code == Some(0) && done == steps,
        "exit {code:?}, {done} of {steps} steps:\n{said}"
    );
}
