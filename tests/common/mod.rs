//! What the tests that draw share.

#![allow(dead_code)] // each test binary uses a part of this shared module

pub mod prepared;
pub mod source_scan;

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

// The client's own list of steering variables and its scrubbed commands, shared by path.
#[path = "../../src/steering.rs"]
pub mod steering;

/// The variables no client inherits, from the list the shell wrappers read too.
pub fn client_environment() -> impl Iterator<Item = &'static str> {
    steering::variables()
}

/// The folders that say where a user's files are, which the isolated client points into its
/// private home.
const LOCATION_VARIABLES: &[&str] = &[
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
    "LOCALAPPDATA",
    "APPDATA",
];

fn is_location(key: &OsStr) -> bool {
    key.to_str()
        .is_some_and(|key| LOCATION_VARIABLES.contains(&key))
}

fn is_isolated(key: &OsStr) -> bool {
    is_location(key)
        || key
            .to_str()
            .is_some_and(|key| client_environment().any(|name| name == key))
}

/// A command that starts `program` (a game server, or anything that must not take the steering
/// variables of the developer's shell) without the variables of `tools/client_environment.txt`.
pub fn server_command(program: impl AsRef<OsStr>) -> Command {
    steering::scrubbed_command(program)
}

/// A client's files live until its last command or child is dropped.
struct ClientScratch(PathBuf);

impl Drop for ClientScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct ClientCommand {
    command: Command,
    scratch: Arc<ClientScratch>,
}

pub struct ClientChild {
    child: Child,
    _scratch: Arc<ClientScratch>,
}

impl std::ops::Deref for ClientChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl std::ops::DerefMut for ClientChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for ClientChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl ClientCommand {
    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.command.arg(arg);
        self
    }
    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(args);
        self
    }
    /// Sets a variable of the client, except those the isolated client keeps for itself: its
    /// home and XDG folders and the variables of `tools/client_environment.txt`, for which a test
    /// that means it uses [`ClientCommand::server`], [`ClientCommand::seat_key`] or
    /// [`ClientCommand::data_dir`].
    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        let key = key.as_ref();
        assert!(
            !is_isolated(key),
            "the isolated client keeps {key:?} to itself: set it with the method that names its purpose"
        );
        self.command.env(key, value);
        self
    }
    /// The game server this client starts when it hosts a local lobby (`BOTA_SERVER`).
    pub fn server(&mut self, binary: &Path) -> &mut Self {
        self.command.env("BOTA_SERVER", binary);
        self
    }
    /// The seat key this client takes (`BOTA_SEAT_KEY`), as a test of the key itself needs.
    pub fn seat_key(&mut self, key: &str) -> &mut Self {
        self.command.env("BOTA_SEAT_KEY", key);
        self
    }
    /// A data folder for this client other than the one in its private home, for a test that
    /// reads what the client kept in it.
    pub fn data_dir(&mut self, dir: &Path) -> &mut Self {
        self.command.env("XDG_DATA_HOME", dir);
        self
    }
    /// Removes a variable of the client, except its home and XDG folders: without `HOME` a
    /// program falls back to the home the system names, which is the user's.
    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut Self {
        let key = key.as_ref();
        assert!(
            !is_location(key),
            "the isolated client keeps {key:?} to itself: removing it hands the client the user's home"
        );
        self.command.env_remove(key);
        self
    }
    pub fn stdout(&mut self, config: impl Into<Stdio>) -> &mut Self {
        self.command.stdout(config);
        self
    }
    pub fn stderr(&mut self, config: impl Into<Stdio>) -> &mut Self {
        self.command.stderr(config);
        self
    }
    pub fn output(&mut self) -> std::io::Result<std::process::Output> {
        self.command.output()
    }
    pub fn spawn(&mut self) -> std::io::Result<ClientChild> {
        self.command.spawn().map(|child| ClientChild {
            child,
            _scratch: Arc::clone(&self.scratch),
        })
    }
    /// The private HOME of this client, which holds all its files.
    pub fn home(&self) -> &Path {
        &self.scratch.0
    }
}

/// The client draws in a private headless compositor.
pub fn client() -> ClientCommand {
    isolated_client(&asset_cache(), None, true)
}

/// A client for commands that prepare their own cache, without a compositor.
pub fn binary(cache: &Path) -> ClientCommand {
    isolated_client(cache, None, false)
}

/// Runs the isolated client through a caller's process wrapper.
pub fn wrapped(prefix: Command) -> ClientCommand {
    isolated_client(&asset_cache(), Some(prefix), true)
}

fn isolated_client(cache: &Path, prefix: Option<Command>, headless: bool) -> ClientCommand {
    refuse_managed_cache(cache);
    static NEXT_CLIENT: AtomicU32 = AtomicU32::new(0);
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let scratch = (0..1024)
        .find_map(|_| {
            let path = root.join(format!(
                "client-{}-{}",
                std::process::id(),
                NEXT_CLIENT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => Some(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => panic!(
                    "cannot create private client directory {}: {error}",
                    path.display()
                ),
            }
        })
        .expect("no free private client directory");
    let mut command = match prefix {
        Some(mut prefix) => {
            if headless {
                prefix.arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/headless.sh"));
            }
            prefix.arg(env!("CARGO_BIN_EXE_dbota"));
            prefix
        }
        None if headless => {
            let mut command =
                Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/headless.sh"));
            command.arg(env!("CARGO_BIN_EXE_dbota"));
            command
        }
        None => Command::new(env!("CARGO_BIN_EXE_dbota")),
    };
    command
        .env("HOME", &scratch)
        .env("XDG_CONFIG_HOME", scratch.join("config"))
        .env("XDG_DATA_HOME", scratch.join("data"))
        .env("XDG_CACHE_HOME", root.join("gpu-cache"))
        .env("XDG_STATE_HOME", scratch.join("state"))
        .env("LOCALAPPDATA", scratch.join("data"))
        .env("APPDATA", scratch.join("data"))
        .env("DBOTA_ASSETS", cache);
    for name in client_environment() {
        command.env_remove(name);
    }
    ClientCommand {
        command,
        scratch: Arc::new(ClientScratch(scratch)),
    }
}

/// Panics when `cache` is the user's managed asset cache or inside it (`~/.cache/dbota/...`,
/// also through symbolic links): a client run would write the use stamps its maintenance reads.
pub fn refuse_managed_cache(cache: &Path) {
    assert!(
        !dbota_import::cache_dir::inside_managed_cache(cache),
        "DBOTA_ASSETS names the managed asset cache {}: import into a private folder",
        cache.display()
    );
}

pub fn asset_cache() -> PathBuf {
    let cache = std::env::var_os("DBOTA_ASSETS")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .expect("DBOTA_ASSETS must name a test asset cache");
    refuse_managed_cache(&cache);
    assert!(
        cache.join(dbota_import::cache_dir::INDEX).is_file(),
        "test asset cache has no index: {}",
        cache.display()
    );
    cache
}

/// A running game server, ended when it is dropped.
pub struct Server {
    child: Child,
    pub address: String,
}

impl Server {
    /// Kills the server and waits until it is gone: nothing listens at its address after.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// The server's exit code once it ends by itself, waited for `within` at most.
    pub fn exit_code(&mut self, within: Duration) -> Option<i32> {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill();
    }
}

/// A game server on a port of its own choosing, started with `args` beside the map and the
/// port, and its address.
pub fn serve(args: &[&str]) -> Server {
    let mut child = server_command(server_binary())
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

/// The game server: `BOTA_SERVER_BIN`, else built from the bota workspace dbota builds against.
#[allow(dead_code)] // each test file uses the helpers it needs
pub fn server_binary() -> PathBuf {
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
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
            PathBuf::from,
        )
        .join("bota");
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
        .arg(scratch.join("settings.conf"))
        .arg("--cache-dir")
        .arg(asset_cache())
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
        .data_dir(&scratch.join("data"))
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
