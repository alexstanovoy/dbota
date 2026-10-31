//! The way out, end to end in the real client: keys pressed through the
//! bindings and the menu's buttons clicked (`--script`), with no account and
//! no service. Escape on the menu asks to quit (Escape again and Cancel close
//! the question, OK ends the client with exit code 0); Escape in a live game
//! opens the dashboard, whose DISCONNECT goes back to the menu and RECONNECT
//! takes the seat again, and quitting from there warns the game is under
//! way; a replay is left after Dota's question, back to the menu.
//!
//! They draw on the GPU (in a headless compositor, tools/headless.sh), so they
//! are left out of a plain `cargo test`; `tools/visual_check.sh` runs them.
//! `DBOTA_QUIT_SHOTS` names a folder to keep their screenshots in.

mod common;

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
    let mut child = common::client()
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
        .stderr(std::fs::File::create(scratch.0.join("stderr.log")).expect("a log file"))
        .spawn()
        .expect("dbota runs");
    let stdout = child.stdout.take().expect("its stdout");
    let said = std::thread::spawn(move || {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut BufReader::new(stdout), &mut text).ok();
        text
    });
    let code = wait(&mut child);
    let mut said = said.join().unwrap_or_default();
    if code != Some(0) {
        // What the client said on stderr is why a run failed.
        said = format!("{said}\nstderr:\n{}", logged(scratch));
    }
    (code, said)
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
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn escape_on_the_menu_asks_to_quit_and_ok_quits() {
    let scratch = Scratch::new("menu");
    let script = "await:menu;key:Escape;await:popup;shot:quit-confirm.png;key:Escape;\
                  await:nopopup;click:quit;await:popup;click:cancel;await:nopopup;\
                  key:Escape;await:popup;click:ok";
    played("the menu", run(&scratch, &[], script), script);
}

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
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

/// An asset cache in `scratch` stamped `stamp`: finished (its index is there) and empty.
fn asset_cache(scratch: &Scratch, stamp: &str) -> PathBuf {
    let cache = scratch.0.join("assets");
    std::fs::create_dir_all(&cache).expect("a cache folder");
    std::fs::write(cache.join("index.txt"), "dbota-cache 1\n").expect("an index");
    std::fs::write(cache.join("stamp.txt"), stamp).expect("a stamp");
    cache
}

/// What the client said on stderr in `scratch`.
fn logged(scratch: &Scratch) -> String {
    std::fs::read_to_string(scratch.0.join("stderr.log")).unwrap_or_default()
}

/// Assets imported by another dbota (their stamp not of this build's asset fingerprint) hold the
/// client at a screen asking for a re-import, its button right there: the replay the command
/// line names never starts; QUIT ends the client.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn assets_of_another_version_hold_the_client_until_imported_again() {
    let scratch = Scratch::new("stale");
    let cache = asset_cache(&scratch, "rev1.1/0000000000000000/1/2");
    let replay = replay();
    let script = "await:button:re-import from dota 2;wait:3;shot:stale-assets.png;click:quit dbota";
    let args = [
        "--cache-dir",
        cache.to_str().expect("a UTF-8 path"),
        "--replay",
        replay.to_str().expect("a UTF-8 path"),
        "--pov",
        "0",
    ];
    played("stale assets", run(&scratch, &args, script), script);
    let log = logged(&scratch);
    assert!(log.contains("is out of date for this version"), "{log}");
    assert!(
        !log.contains("match known"),
        "the replay started on stale assets:\n{log}"
    );
}

/// With no asset cache at all the client is held at the screen that makes one: it offers the
/// Dota 2 folder and a prepared cache (and QUIT), none of the menu is there, and the replay the
/// command line names never starts.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn with_no_assets_the_client_offers_only_the_ways_to_get_them() {
    let scratch = Scratch::new("no-assets");
    let cache = scratch.0.join("assets");
    let replay = replay();
    let script = "await:button:import from dota 2;await:button:use a prepared cache;\
                  await:nobutton:sign in;await:nobutton:local lobby;await:nobutton:demo hero;\
                  await:nobutton:replays;await:nobutton:settings;wait:2;shot:no-assets.png;\
                  click:quit dbota";
    let args = [
        "--cache-dir",
        cache.to_str().expect("a UTF-8 path"),
        "--replay",
        replay.to_str().expect("a UTF-8 path"),
        "--pov",
        "0",
    ];
    played("no assets", run(&scratch, &args, script), script);
    let log = logged(&scratch);
    assert!(log.contains("no asset cache at"), "{log}");
    assert!(
        !log.contains("match known"),
        "the replay started with no assets:\n{log}"
    );
}

/// A prepared cache of this build as a folder and as a zip (the real cache's small files, the
/// importer's own stamp), in `scratch`.
fn prepared(scratch: &Scratch) -> (PathBuf, PathBuf) {
    let files = common::prepared::small_cache();
    let (folder, zip) = (scratch.0.join("friend"), scratch.0.join("friend.zip"));
    common::prepared::write_folder(&folder, &files);
    common::prepared::write_zip(&zip, files.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
    (folder, zip)
}

/// A cache taken with `dbota import-cache`, from a folder or from a zip, is a cache of this
/// build: no question at the start, the replay plays.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn assets_taken_from_a_prepared_cache_play() {
    let scratch = Scratch::new("prepared-play");
    let (folder, zip) = prepared(&scratch);
    let replay = replay();
    let script = "await:game;wait:2;key:Escape;await:menu;click:disconnect;await:popup;click:ok;\
                  await:nopopup;await:button:quit;key:Escape;await:popup;click:ok";
    for (what, from) in [("a folder", folder), ("a zip", zip)] {
        let cache = scratch.0.join(format!("assets-{}", what.replace(' ', "-")));
        let taken = common::prepared::dbota(&cache, "import-cache", &from);
        assert!(
            taken.status.success(),
            "{what}: {}",
            common::prepared::said(&taken)
        );
        let args = [
            "--cache-dir",
            cache.to_str().expect("a UTF-8 path"),
            "--replay",
            replay.to_str().expect("a UTF-8 path"),
            "--pov",
            "0",
        ];
        played(what, run(&scratch, &args, script), script);
        assert!(
            !logged(&scratch).contains("out of date"),
            "{what}: {}",
            logged(&scratch)
        );
    }
}

/// With no asset cache the screen offers a prepared one: its button asks for a path, typed or
/// pasted into the field, and Enter takes the cache there (a restart reads it).
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn the_asset_screen_takes_a_prepared_cache_from_a_typed_path() {
    let scratch = Scratch::new("prepared-menu");
    let (_, zip) = prepared(&scratch);
    let cache = scratch.0.join("assets");
    let script = format!(
        "await:button:use a prepared cache;click:use a prepared cache;await:button:load;\
         text:{};shot:prepared-path.png;key:Enter;await:button:restart dbota;shot:prepared-loaded.png;\
         click:quit dbota",
        zip.display()
    );
    let args = ["--cache-dir", cache.to_str().expect("a UTF-8 path")];

    played("the typed path", run(&scratch, &args, &script), &script);

    let files = common::prepared::small_cache();
    assert_eq!(common::prepared::read_folder(&cache), files);
}

/// A cache of another build typed into the field is refused, the terminal says which build it
/// is for, and the screen offers the ways again; nothing is installed.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn the_asset_screen_refuses_a_prepared_cache_of_another_build() {
    let scratch = Scratch::new("prepared-refused");
    let mut files = common::prepared::small_cache();
    files.insert(
        "stamp.txt".to_owned(),
        b"rev1.1/0000000000000000/1/2".to_vec(),
    );
    let zip = scratch.0.join("stale.zip");
    common::prepared::write_zip(&zip, files.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
    let cache = scratch.0.join("assets");
    let script = format!(
        "await:button:use a prepared cache;click:use a prepared cache;await:button:load;\
         text:{};key:Enter;await:nobutton:cancel;await:button:use a prepared cache;\
         shot:prepared-refused.png;click:quit dbota",
        zip.display()
    );
    let args = ["--cache-dir", cache.to_str().expect("a UTF-8 path")];

    played("a refused cache", run(&scratch, &args, &script), &script);

    let log = logged(&scratch);
    assert!(
        log.contains("is for assets rev1.1/0000000000000000"),
        "{log}"
    );
    assert!(!cache.exists(), "a refused cache was installed");
}

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
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
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn without_a_service_the_menu_is_local_and_takes_typed_text() {
    let scratch = Scratch::new("local");
    let (mut server, address) = serve();
    let script = format!(
        "await:menu;await:button:create;shot:local-first.png;key:Tab;key:Tab;key:Tab;\
         text:{address};click:play;await:game;wait:2;key:Escape;await:menu;click:disconnect;\
         await:button:reconnect;click:quit;await:popup;click:ok"
    );
    let result = run(&scratch, &[], &script);
    let _ = server.kill();
    let _ = server.wait();
    played("a local menu", result, &script);
}

/// Dota's LEAVE GAME: from a seat in a match under way it asks, then leaves the match: the
/// other seat is told it won, and the client is back at its menu.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn leaving_a_game_loses_it_and_the_other_side_wins() {
    use bota_client_core::{Link, play};
    use bota_proto::HeroId;
    let scratch = Scratch::new("leave");
    let (mut server, address) = serve_seats(2);
    // The first to join holds the first seat; dbota takes the second.
    let (mut link, seated) =
        Link::join(&address, "stays", HeroId::SHADOW_FIEND, None).expect("a seat");
    let (done, outcome) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(play(&mut link, seated, &mut (), |_| Vec::new()));
    });
    let script = "await:game;click:pick shadow fiend;click:pick ready;wait:8;key:Escape;await:menu;\
                  wait:1;shot:leave-dashboard.png;click:leave game;await:popup;shot:leave-confirm.png;\
                  click:ok;await:button:quit;shot:leave-menu.png;click:quit;await:popup;click:ok";
    let result = run(&scratch, &["--addr", &address], script);
    // A seat that did not leave keeps the other one playing for good: end the server then, so a
    // failure fails at once instead of holding the run.
    if result.0 != Some(0) || result.1.contains("timed out") {
        let _ = server.kill();
        let _ = server.wait();
        played("a seat that leaves", result.clone(), script);
    }
    let outcome = outcome
        .recv_timeout(PATIENCE)
        .expect("the other seat saw the end in time")
        .expect("it saw the end");
    let _ = server.kill();
    let _ = server.wait();
    played("a seat that leaves", result, script);
    assert_eq!(outcome.winner, outcome.team, "the seat that stayed won");
}

/// A solo-mid style pool of one hero: the pick screen shows that hero alone, with no Ready
/// button to press, and the seat is given the hero as it sits.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_pool_of_one_shows_only_its_hero_and_nothing_to_choose() {
    let scratch = Scratch::new("forced");
    // A second seat nobody takes keeps the pick screen up.
    let (mut server, address) = serve_with(2, &["--heroes", "1"]);
    let script = "await:game;await:button:pick shadow fiend;await:nobutton:pick pudge;\
                  await:nobutton:pick ready;wait:1;shot:forced-pick.png;key:Escape;await:menu;\
                  click:disconnect;await:button:reconnect;click:quit;await:popup;click:ok";
    let result = run(&scratch, &["--addr", &address], script);
    let _ = server.kill();
    let _ = server.wait();
    played("a forced pick", result, script);
}

/// Five a side, as Dota's lobby holds: the top bar of a ten-seat match shows five places of
/// each side.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn the_top_bar_of_a_ten_seat_match_holds_five_a_side() {
    use bota_client_core::Link;
    use bota_proto::HeroId;
    let scratch = Scratch::new("topbar");
    let (mut server, address) = serve_seats(10);
    for n in 0..9 {
        let address = address.clone();
        std::thread::spawn(move || {
            let _ = Link::play(&address, &format!("idle {n}"), HeroId::SHADOW_FIEND, |_| {
                Vec::new()
            });
        });
    }
    let script = "await:game;click:pick shadow fiend;click:pick ready;await:horn;\
                  await:seats:5,5;wait:1;shot:topbar-5v5.png;key:Escape;await:menu;\
                  click:disconnect;await:button:reconnect;click:quit;await:popup;click:ok";
    let result = run(&scratch, &["--addr", &address], script);
    let _ = server.kill();
    let _ = server.wait();
    played("a ten-seat match", result, script);
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
    serve_with(seats, &[])
}

/// [`serve_seats`] with more of the server's arguments.
fn serve_with(seats: u8, more: &[&str]) -> (Child, String) {
    let mut child = Command::new(server_binary())
        .args(more)
        .args(["--map", if seats > 2 { "0" } else { "2" }])
        .args(["--port", "0", "--bind", "127.0.0.1"])
        .args([
            "--players",
            &seats.to_string(),
            "--pick-secs",
            "5",
            "--tactical-secs",
            "0",
            // A client that loads a match on a busy machine says nothing for seconds: the
            // server must not let it go for silence (0 waits for ever).
            "--idle-timeout-secs",
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

/// A matchmaking service in this process with Dota's Demo Hero ruleset alone, launching the real
/// game server; the client signs up to it and reaches Demo Hero through it. Shut down when dropped.
struct DemoService {
    runtime: tokio::runtime::Runtime,
    service: Option<radovid::service::Service>,
    url: String,
    /// The service's certificate, on disk for the client's `--trust-cert`.
    pem: PathBuf,
}

impl DemoService {
    fn start(scratch: &Scratch) -> DemoService {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("a free port")
            .port();
        let config = format!(
            r#"
listen = "127.0.0.1:0"
database = "{db}"
tls_cert = "unused"
tls_key = "unused"
game_ports = "{port}-{port}"
[password]
memory_kib = 1024
iterations = 1
[limits]
game_capacity = 1
lobby_games_max = 1
[[ruleset]]
id = "demo"
name = "Demo Hero"
game = "bota"
game_fingerprint = "{fingerprint:016x}"
teams = [1, 1]
rated = false
audience = "humans_only"
instant_solo = true
info = "01"
[ruleset.launcher]
kind = "local"
binary = "{binary}"
public_url = "ws://127.0.0.1:{{port}}"
version_probe = ["--wire-fingerprint"]
args = ["--map", "0", "--cheats", "--match-id", "{{match_id}}", "--bind", "127.0.0.1",
        "--port", "0", "--ws-port", "{{port}}", "--abandon-unjoined", "--empty-after-secs", "60",
        "--tactical-secs", "0"]
env = {{ BOTA_SEATS = "{{seats}}" }}
"#,
            db = scratch.0.join("radovid.db").display(),
            fingerprint = bota_radovid::fingerprint(),
            binary = server_binary().display(),
        );
        let path = scratch.0.join("radovid.toml");
        std::fs::write(&path, config).expect("the configuration");
        let config = radovid::config::Config::load(&path).expect("a valid configuration");
        let identity =
            radovid::tls::self_signed(vec!["127.0.0.1".to_owned()]).expect("a certificate");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime");
        let service = runtime
            .block_on(radovid::service::Service::start(config, &identity))
            .expect("the service starts");
        let pem = scratch.0.join("service.pem");
        std::fs::write(&pem, &identity.cert_pem).expect("the certificate");
        DemoService {
            url: format!("wss://127.0.0.1:{}", service.addr.port()),
            runtime,
            service: Some(service),
            pem,
        }
    }

    /// The client's arguments to reach the service.
    fn args(&self) -> [String; 4] {
        [
            "--service".to_owned(),
            self.url.clone(),
            "--trust-cert".to_owned(),
            self.pem.to_str().expect("a UTF-8 path").to_owned(),
        ]
    }
}

impl Drop for DemoService {
    fn drop(&mut self) {
        if let Some(service) = self.service.take() {
            self.runtime.block_on(service.shutdown());
        }
    }
}

/// The script steps that create the account `demo` on the service and open the DEMO HERO page.
const SIGN_UP: &str = "await:menu;await:button:create account;click:create account;\
                       await:button:back;text:demo;key:Tab;text:correct horse battery;key:Tab;\
                       text:correct horse battery;click:create account;await:button:demo hero;\
                       click:demo hero;await:button:demo shadow fiend;";

/// Plays `script` after [`SIGN_UP`] against a [`DemoService`]: the client's exit code, what it
/// said, and the whole script.
fn run_demo(scratch: &Scratch, script: &str) -> (Option<i32>, String, String) {
    let service = DemoService::start(scratch);
    let script = format!("{SIGN_UP}{script}");
    let args = service.args();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, said) = run(scratch, &args, &script);
    (code, said, script)
}

/// Dota's Demo Hero from the menu, through the service: the hero chosen on the whole map
/// with the demo panel at the side and the match marked as one that cheats; the panel's buttons
/// and a `-command` typed into the chat do what they say (the hero at level 30, the gold
/// given), and Quit goes back to the menu. `DBOTA_QUIT_SHOTS` keeps the screenshots.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn demo_hero_plays_a_hero_with_every_cheat() {
    let scratch = Scratch::new("demo");
    let (code, said, script) = run_demo(
        &scratch,
        "wait:1;shot:demo-menu.png;click:demo shadow fiend;await:game;await:horn;\
         await:button:demo levelmax;wait:3;shot:demo-panel.png;click:demo levelmax;\
         await:level:30;say:-gold 20000;await:gold:20000;click:demo freespells;\
         click:demo spawnenemy;click:demo creepwave;wait:3;shot:demo-cheats.png;\
         click:demo night;wait:2;shot:demo-night.png;click:demo quit;await:menu;\
         wait:1;shot:demo-left.png;key:Escape;key:Escape;await:popup;click:ok",
    );
    played("Demo Hero", (code, said), &script);
}

/// The scene's lights as a `light` step printed them: sun colour and lux, ambient colour and
/// brightness, in linear light.
#[derive(Debug, Clone, Copy)]
struct Lights {
    sun: [f32; 3],
    lux: f32,
    ambient: [f32; 3],
    brightness: f32,
}

impl Lights {
    fn parse(said: &str) -> Vec<Lights> {
        let numbers = |text: &str| -> Vec<f32> {
            text.split(',')
                .map(|n| n.parse().expect("a number"))
                .collect()
        };
        said.lines()
            .filter_map(|l| l.strip_prefix("light: sun="))
            .map(|l| {
                let (sun, ambient) = l.split_once(" ambient=").expect("both lights");
                let (sun, ambient) = (numbers(sun), numbers(ambient));
                Lights {
                    sun: [sun[0], sun[1], sun[2]],
                    lux: sun[3],
                    ambient: [ambient[0], ambient[1], ambient[2]],
                    brightness: ambient[3],
                }
            })
            .collect()
    }
}

/// The owner's report: at night the light does not differ in colour from the day's. Demo Hero
/// by day (after the horn), then its Night button: the sun's and the ambient light's colour and
/// strength the client lights the scene with are read from the scene, and must differ by what
/// Dota's map has them differ by (its `lighting.txt` in the cache). `DBOTA_QUIT_SHOTS` keeps the
/// two frames.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn night_light_differs_from_day_light() {
    let scratch = Scratch::new("night-light");
    let (code, said, script) = run_demo(
        &scratch,
        "click:demo shadow fiend;await:game;await:horn;await:daylight:day;wait:1;\
         light;shot:light-day.png;click:demo night;await:daylight:night;wait:1;\
         light;shot:light-night.png;click:demo quit;await:menu;key:Escape;key:Escape;\
         await:popup;click:ok",
    );
    played("night light", (code, said.clone()), &script);
    let lights = Lights::parse(&said);
    assert_eq!(lights.len(), 2, "{said}");
    let (day, night) = (lights[0], lights[1]);
    let map = Dota::read();
    let close = |got: f32, want: f32| (got - want).abs() <= 0.01 * want.abs().max(1e-3);
    for (what, day_rgb, night_rgb, pair) in [
        ("sun", day.sun, night.sun, map.sun),
        ("ambient", day.ambient, night.ambient, map.ambient),
    ] {
        for c in 0..3 {
            let want = day_rgb[c] * pair[1][c] / pair[0][c];
            assert!(
                close(night_rgb[c], want),
                "{what} channel {c}: night {} but Dota's colours make it {want} (day {}):\n{said}",
                night_rgb[c],
                day_rgb[c]
            );
        }
        // Night's light is Dota's moonlight blue: blue gains on red by Dota's measure.
        let blue_over_red = |rgb: [f32; 3]| rgb[2] / rgb[0];
        assert!(
            blue_over_red(night_rgb) > 1.25 * blue_over_red(day_rgb),
            "{what} does not turn blue by night:\n{said}"
        );
    }
    // Dota's strengths by night against day are its scales' ratios.
    assert!(close(night.lux, day.lux * map.sun_scale), "{said}");
    assert!(
        close(night.brightness, day.brightness * map.ambient_scale),
        "{said}"
    );
    // And by Dota's luminance the night sun is dimmer than the day's.
    let luminance = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    assert!(luminance(night.sun) < 0.9 * luminance(day.sun), "{said}");
    assert!(
        luminance(night.ambient) < 0.9 * luminance(day.ambient)
            || night.ambient[2] > 1.05 * day.ambient[2],
        "{said}"
    );
}

/// Dota's lighting of the map as the cache has it (`map/mid/lighting.txt`), per light the linear
/// colours by day and by night, and the night's scale against the day's.
struct Dota {
    sun: [[f32; 3]; 2],
    ambient: [[f32; 3]; 2],
    sun_scale: f32,
    ambient_scale: f32,
}

impl Dota {
    fn read() -> Dota {
        let cache = std::env::var_os("DBOTA_ASSETS").map_or_else(
            || {
                PathBuf::from(std::env::var_os("HOME").expect("a home"))
                    .join(".cache/dbota")
                    .join(dbota_import::import::folder_name())
            },
            PathBuf::from,
        );
        let text = std::fs::read_to_string(cache.join("map/mid/lighting.txt"))
            .expect("the cache's lighting.txt");
        let line = |light: &str, time: &str| -> [f32; 4] {
            let line = text
                .lines()
                .find(|l| l.starts_with(&format!("{light} {time} ")))
                .expect("a lighting line");
            let n: Vec<f32> = line
                .split_whitespace()
                .skip(2)
                .map(|n| n.parse().expect("a number"))
                .collect();
            [n[0], n[1], n[2], n[3]]
        };
        let linear = |c: f32| {
            let c = c / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let rgb = |l: [f32; 4]| [linear(l[0]), linear(l[1]), linear(l[2])];
        Dota {
            sun: [rgb(line("sun", "day")), rgb(line("sun", "night"))],
            ambient: [rgb(line("ambient", "day")), rgb(line("ambient", "night"))],
            sun_scale: line("sun", "night")[3] / line("sun", "day")[3],
            ambient_scale: line("ambient", "night")[3] / line("ambient", "day")[3],
        }
    }
}
