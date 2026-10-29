//! What the tests that draw share.

#![allow(dead_code)] // each test binary uses a part of this shared module

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The client, to be given its arguments, drawing in a headless compositor of its own on the GPU
/// (tools/headless.sh, or the one the run is already in): no window shows on the desktop, unless
/// `DBOTA_SHOW=1` asks for it.
pub fn client() -> Command {
    let mut command = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/headless.sh"));
    command.arg(env!("CARGO_BIN_EXE_dbota"));
    command
}

/// A running game server, ended when it is dropped.
pub struct Server {
    child: Child,
    pub address: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A game server on a port of its own choosing, started with `args` beside the map and the
/// port, and its address.
pub fn serve(args: &[&str]) -> Server {
    let mut child = Command::new(server_binary())
        .args(["--map", "2", "--port", "0", "--bind", "127.0.0.1"])
        .args(args)
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
    Server { child, address }
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

/// Seconds a scripted run may take.
const PATIENCE: Duration = Duration::from_secs(240);

/// Plays `script` in the real client against the server at `address`, drawing in a headless
/// compositor, and asserts it ran to its end: exit code 0 and every step done.
pub fn play_script(address: &str, script: &str, tag: &str) {
    run_script(address, script, tag, None);
}

/// Plays `script` (the client's `--script`) in a match on a cheats-honouring server of one seat
/// and asserts that every step was done and the client left with exit code 0. `shots` is the
/// folder the script's `shot:` steps save to; it is kept for a person to look at.
pub fn play(name: &str, script: &str, shots: Option<&Path>) {
    let server = serve(&[
        "--cheats",
        "--players",
        "1",
        "--pick-secs",
        "5",
        "--tactical-secs",
        "5",
    ]);
    run_script(&server.address, script, name, shots);
}

fn run_script(address: &str, script: &str, tag: &str, shots: Option<&Path>) {
    let scratch =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("a scratch folder");
    let mut command = client();
    command
        .args(["--addr", address, "--config"])
        .arg(scratch.join("client.conf"))
        .args([
            "--script",
            script,
            "--mute",
            "--no-vsync",
            "--display-mode",
            "windowed",
        ]);
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
        BufReader::new(stdout).read_to_string(&mut text).ok();
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
    let said = said.join().unwrap_or_default();
    let steps = script.split(';').filter(|s| !s.trim().is_empty()).count();
    let done = said.lines().filter(|l| l.starts_with("script: ")).count();
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        code == Some(0) && done == steps,
        "exit {code:?}, {done} of {steps} steps:\n{said}"
    );
}

/// A line to `server` that holds every byte back by `one_way` each way, as a long road does:
/// connect to its address. Each direction keeps its bytes in order.
pub fn delayed(server: &str, one_way: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("bound").to_string();
    let server = server.to_owned();
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let Ok(upstream) = TcpStream::connect(&server) else {
                continue;
            };
            for stream in [&client, &upstream] {
                let _ = stream.set_nodelay(true);
            }
            if let (Ok(a), Ok(b), Ok(c), Ok(d)) = (
                client.try_clone(),
                upstream.try_clone(),
                upstream.try_clone(),
                client.try_clone(),
            ) {
                carry(a, b, one_way);
                carry(c, d, one_way);
            }
        }
    });
    address
}

/// Carries what `from` sends on to `to`, each chunk `delay` after it arrived.
fn carry(mut from: TcpStream, mut to: TcpStream, delay: Duration) {
    let (held, due) = mpsc::channel::<(Instant, Vec<u8>)>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n @ 1..) = from.read(&mut buf) {
            if held
                .send((Instant::now() + delay, buf[..n].to_vec()))
                .is_err()
            {
                return;
            }
        }
    });
    std::thread::spawn(move || {
        for (at, bytes) in due {
            std::thread::sleep(at.saturating_duration_since(Instant::now()));
            if to.write_all(&bytes).is_err() {
                return;
            }
        }
        let _ = to.shutdown(std::net::Shutdown::Write);
    });
}
