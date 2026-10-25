//! Selling in the strategy phase, end to end in the real client: two items
//! are chosen in the shop, a right click on one opens the item menu, its Sell
//! takes the choice back, and when the match begins the hero carries the one
//! item left, not the two. Drawn on the GPU in a headless compositor, so it is
//! left out of a plain `cargo test`; `tools/visual_check.sh` runs it.

mod common;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Seconds a run may take.
const PATIENCE: Duration = Duration::from_secs(240);

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

/// Sells a chosen item from the strategy phase's item menu: the match then
/// begins with one item in the bag, the one not sold.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn selling_in_the_strategy_phase_takes_the_choice_back() {
    let scratch =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("sell-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("a scratch folder");
    let (mut server, address) = serve();
    // The shop's two first consumables are right clicked (each is chosen),
    // the first slot of the inventory is right clicked, and Sell clicked.
    let script = "await:game;click:pick shadow fiend;click:pick ready;wait:6;\
                  mouse:0.6516,0.2157;wait:0.7;rclick;wait:0.7;\
                  mouse:0.6276,0.2157;wait:0.7;rclick;wait:0.7;\
                  mouse:0.4766,0.275;wait:0.7;rclick;wait:0.7;\
                  mouse:0.49,0.295;wait:0.7;lclick;wait:0.7;\
                  await:horn;wait:4;await:items:1;exit";
    let mut child = common::client()
        .args(["--addr", &address, "--config"])
        .arg(scratch.join("client.conf"))
        .args([
            "--script",
            script,
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
