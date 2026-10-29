//! What the tests that draw share.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The client, to be given its arguments, drawing in a headless compositor of its own on the GPU
/// (tools/headless.sh, or the one the run is already in): no window shows on the desktop, unless
/// `DBOTA_SHOW=1` asks for it.
pub fn client() -> Command {
    let mut command = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/headless.sh"));
    command.arg(env!("CARGO_BIN_EXE_dbota"));
    command
}

/// Seconds a scripted run may take.
const PATIENCE: Duration = Duration::from_secs(240);

/// A game server of one seat that honours Dota's cheats, with a pick phase and a strategy phase
/// as short as the script allows, on a port of its own choosing; and its address.
fn serve() -> (Child, String) {
    let mut child = Command::new(server_binary())
        .args([
            "--map",
            "2",
            "--port",
            "0",
            "--bind",
            "127.0.0.1",
            "--cheats",
        ])
        .args(["--players", "1", "--pick-secs", "5", "--tactical-secs", "5"])
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

/// The game server: `BOTA_SERVER_BIN`, else built from the bota workspace dbota builds against.
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

/// Plays `script` (the client's `--script`) in a match on a cheats-honouring server of one seat
/// and asserts that every step was done and the client left with exit code 0. `shots` is the
/// folder the script's `shot:` steps save to; it is kept for a person to look at.
#[allow(dead_code)] // each test file uses the helpers it needs
pub fn play(name: &str, script: &str, shots: Option<&Path>) {
    let scratch =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("a scratch folder");
    let (mut server, address) = serve();
    let mut command = client();
    command
        .args(["--addr", &address, "--config"])
        .arg(scratch.join("client.conf"))
        .args(["--script", script, "--mute", "--no-vsync"])
        .args(["--display-mode", "windowed"]);
    if let Some(shots) = shots {
        std::fs::create_dir_all(shots).expect("the screenshot folder");
        command.arg("--screenshot-dir").arg(shots);
    }
    let mut child = command
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
