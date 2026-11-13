//! The menu end to end: the real matchmaking service in-process, launching
//! the real `bota_server`; players are [`MenuCore`]s driven by the acts a
//! person's clicks and keys make, and they play their games through the same
//! opening the client uses, as the client core plays them.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use bota_client_core::{App as Core, Phase, Source};
use bota_core::fountain_pos;
use bota_proto::{HeroId, MapId, Order, Target};
use bota_radovid::proto::RulesetId;

use super::core::{Act, Confirm, GameKind, LOCAL_SEATS, Launch, LocalRules, MenuCore, Page};
use super::launch::{Hosted, Opening, Trust};
use super::store::{Entry, SettingsStore};
use super::view::PageView;
use super::wire_probe::WIRE_PROBE_PATIENCE;
use crate::replays::{Disk, Library};

pub(crate) const PASSWORD: &str = "correct horse battery";
const RULESET: &str = "solo_mid";
static NEXT: AtomicU32 = AtomicU32::new(0);
/// How long any wait for an event lasts before the test calls it stuck. Waits
/// end when their condition holds, so the figure only matters to a stuck run;
/// it is long enough for a machine running many other jobs.
const PATIENCE: u64 = 300;

/// A scratch folder under the target folder, emptied when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(what: &str) -> Scratch {
        let dir = crate::test_support::target_dir().join(format!(
            "menu-tests/{what}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("scratch folder");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The bota workspace dbota builds against, as Cargo resolved it.
fn bota_root() -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
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
    // <root>/crates/bota_client_core/Cargo.toml
    Path::new(manifest)
        .ancestors()
        .nth(3)
        .expect("the bota workspace")
        .to_owned()
}

/// The game server with its WebSocket listener, built once per run from the
/// same bota (`BOTA_SERVER_BIN` names a ready one).
pub(crate) fn server_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let binary = if let Some(path) = std::env::var_os("BOTA_SERVER_BIN") {
            PathBuf::from(path)
        } else {
            build_server()
        };
        wire_checked(binary, bota_radovid::fingerprint(), WIRE_PROBE_PATIENCE)
            .unwrap_or_else(|said| panic!("{said}"))
    })
}

/// `binary`, if it speaks the wire `ours`. Why here: bota moving under a built client makes a
/// server of another wire, and every match then fails minutes in, on the first Hello.
fn wire_checked(binary: PathBuf, ours: u64, patience: Duration) -> Result<PathBuf, String> {
    let theirs = super::wire_probe::wire_of(
        crate::steering::scrubbed_command(&binary),
        &binary,
        patience,
    )?;
    if theirs != ours {
        return Err(format!(
            "{} speaks wire {theirs:016x} but this client was compiled for wire {ours:016x}: \
             rebuild the client against the current bota, or check out the bota it was compiled \
             with",
            binary.display()
        ));
    }
    Ok(binary)
}

/// Builds the game server from the bota workspace dbota builds against.
fn build_server() -> PathBuf {
    let target = crate::test_support::target_dir().join("bota");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "build",
            "--quiet",
            "--release",
            "-p",
            "bota_server",
            "--features",
            "websocket",
        ])
        .arg("--target-dir")
        .arg(&target)
        .current_dir(bota_root())
        .status()
        .expect("cargo runs");
    assert!(status.success(), "bota_server builds");
    target.join("release/bota_server")
}

/// The service, running until dropped.
pub(crate) struct Service {
    pub(crate) public_identity: Option<String>,
    runtime: tokio::runtime::Runtime,
    service: Option<radovid::service::Service>,
    pub(crate) url: String,
    pub(crate) pem: String,
    _dir: Scratch,
}

impl Drop for Service {
    fn drop(&mut self) {
        if let Some(service) = self.service.take() {
            self.runtime.block_on(service.shutdown());
        }
    }
}

fn config(dir: &Path, binary: &Path, save_replays: bool) -> String {
    // One game at a time: the one port the system hands out now.
    let port = free_port();
    format!(
        r#"
listen = "127.0.0.1:0"
database = "{db}"
tls_cert = "unused"
tls_key = "unused"
game_ports = "{port}-{port}"
lobby_teams = [5, 5]
[password]
memory_kib = 1024
iterations = 1
[artifacts]
save_replays = {save_replays}
[limits]
games_per_source = 200
game_capacity = 1
lobby_games_max = 1
# The full rules come first: the default of a lobby.
[[ruleset]]
id = "full"
name = "Full rules"
game = "bota"
game_fingerprint = "{fingerprint:016x}"
teams = [5, 5]
rated = false
[ruleset.launcher]
kind = "local"
binary = "{binary}"
public_url = "ws://127.0.0.1:{{port}}"
version_probe = ["--wire-fingerprint"]
start_timeout_secs = {PATIENCE}
args = ["--map", "0", "--mode", "lockstep", "--ack-timeout-ticks", "900",
        "--match-id", "{{match_id}}", "--bind", "127.0.0.1", "--port", "0",
        "--ws-port", "{{port}}", "--start-after-secs", "{PATIENCE}", "--abandon-unjoined",
        "--empty-after-secs", "{PATIENCE}", "--pick-secs", "1", "--tactical-secs", "0"]
env = {{ BOTA_SEATS = "{{seats}}" }}

[[ruleset]]
id = "{RULESET}"
name = "1x1 Mid Only"
game = "bota"
game_fingerprint = "{fingerprint:016x}"
teams = [1, 1]
rated = true
max_duration_secs = {PATIENCE}
[ruleset.launcher]
kind = "local"
binary = "{binary}"
public_url = "ws://127.0.0.1:{{port}}"
version_probe = ["--wire-fingerprint"]
start_timeout_secs = {PATIENCE}
args = ["--map", "2", "--heroes", "1", "--mode", "lockstep", "--ack-timeout-ticks", "900",
        "--match-id", "{{match_id}}", "--bind", "127.0.0.1", "--port", "0",
        "--ws-port", "{{port}}", "--start-after-secs", "{PATIENCE}", "--abandon-unjoined",
        "--empty-after-secs", "{PATIENCE}", "--pick-secs", "1", "--tactical-secs", "0"]
env = {{ BOTA_SEATS = "{{seats}}" }}
artifact = "{artifacts}/replay-{{match_id}}.brp"
artifact_args = ["--replay", "{{artifact}}"]
# The match's metadata, kept whether or not replays are.
[ruleset.artifacts.meta]
path = "{artifacts}/meta-{{match_id}}.meta"
args = ["--meta", "{{artifact}}"]
max_kib = 1024
access = "audience"

[[ruleset]]
id = "arena"
name = "Bots Arena"
game = "bota"
game_fingerprint = "{fingerprint:016x}"
teams = [1, 1]
rated = true
audience = "bots_only"
[ruleset.launcher]
kind = "local"
binary = "{binary}"
public_url = "ws://127.0.0.1:{{port}}"
version_probe = ["--wire-fingerprint"]
args = ["--map", "2", "--match-id", "{{match_id}}", "--bind", "127.0.0.1", "--port", "0",
        "--ws-port", "{{port}}"]
env = {{ BOTA_SEATS = "{{seats}}" }}

# Two against two: another ruleset to choose in a lobby.
[[ruleset]]
id = "squad"
name = "Squad"
game = "bota"
game_fingerprint = "{fingerprint:016x}"
teams = [2, 2]
rated = false
[ruleset.launcher]
kind = "local"
binary = "{binary}"
public_url = "ws://127.0.0.1:{{port}}"
version_probe = ["--wire-fingerprint"]
args = ["--map", "0", "--match-id", "{{match_id}}", "--bind", "127.0.0.1", "--port", "0",
        "--ws-port", "{{port}}"]
env = {{ BOTA_SEATS = "{{seats}}" }}

# Practice: the full Dota map, free pick. Lobbies of it start alone: the match has the host's seat and nobody against it.
[[ruleset]]
id = "practice"
name = "Practice"
game = "bota"
game_fingerprint = "{fingerprint:016x}"
teams = [1, 1]
rated = false
lobby_min_players = 1
[ruleset.launcher]
kind = "local"
binary = "{binary}"
public_url = "ws://127.0.0.1:{{port}}"
version_probe = ["--wire-fingerprint"]
start_timeout_secs = {PATIENCE}
args = ["--map", "0", "--mode", "lockstep", "--ack-timeout-ticks", "900",
        "--match-id", "{{match_id}}", "--bind", "127.0.0.1", "--port", "0",
        "--ws-port", "{{port}}", "--start-after-secs", "{PATIENCE}", "--abandon-unjoined",
        "--empty-after-secs", "{PATIENCE}", "--pick-secs", "1", "--tactical-secs", "0"]
env = {{ BOTA_SEATS = "{{seats}}" }}
artifact = "{artifacts}/replay-{{match_id}}.brp"
artifact_args = ["--replay", "{{artifact}}"]

# Demo Hero: cheats, free pick, one player started at once; bota's flag byte says it is a Demo.
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
start_timeout_secs = {PATIENCE}
args = ["--map", "0", "--cheats", "--mode", "lockstep", "--ack-timeout-ticks", "900",
        "--match-id", "{{match_id}}", "--bind", "127.0.0.1", "--port", "0",
        "--ws-port", "{{port}}", "--abandon-unjoined", "--empty-after-secs", "{PATIENCE}",
        "--pick-secs", "1", "--tactical-secs", "0"]
env = {{ BOTA_SEATS = "{{seats}}" }}
"#,
        db = dir.join("radovid.db").display(),
        artifacts = dir.join("artifacts").display(),
        fingerprint = bota_radovid::fingerprint(),
        binary = binary.display(),
    )
}

pub(crate) fn service() -> Service {
    start_service(true, |config| config)
}

/// The service, keeping the replays of its matches or not.
fn service_saving(save_replays: bool) -> Service {
    start_service(save_replays, |config| config)
}

/// [`service`] with its configuration changed by `change`.
pub(crate) fn service_with(change: impl FnOnce(String) -> String) -> Service {
    start_service(true, change)
}

pub(crate) fn service_with_unranked() -> Service {
    service_with(|text| {
        let ranked = text
            .split("[[ruleset]]")
            .find(|rule| rule.contains("id = \"solo_mid\""))
            .expect("ranked fixture");
        let casual = ranked
            .replace("id = \"solo_mid\"", "id = \"casual_mid\"")
            .replace("name = \"1x1 Mid Only\"", "name = \"Unranked Mid\"")
            .replace(
                "rated = true",
                "rated = false\nrating_ruleset = \"solo_mid\"",
            );
        format!("{text}\n[[ruleset]]\n{casual}").replace(
            "rated = true",
            "rated = true\nrating_window = { initial = 100, widen_per_minute = 0, maximum = 100 }",
        )
    })
}

fn start_service(save_replays: bool, change: impl FnOnce(String) -> String) -> Service {
    let binary = server_binary().to_owned();
    let dir = Scratch::new("service");
    let path = dir.0.join("radovid.toml");
    std::fs::write(&path, change(config(&dir.0, &binary, save_replays))).expect("config");
    let config = radovid::config::Config::load(&path).expect("a valid configuration");
    let public_identity = config.public_identity.clone();
    let names = vec!["127.0.0.1".to_owned(), "localhost".to_owned()];
    let identity = radovid::tls::self_signed(names).expect("certificate");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let service = runtime
        .block_on(radovid::service::Service::start(config, &identity))
        .expect("the service starts");
    Service {
        public_identity,
        url: format!("wss://127.0.0.1:{}", service.addr.port()),
        pem: identity.cert_pem.clone(),
        runtime,
        service: Some(service),
        _dir: dir,
    }
}

/// A library keeping its replays in the folder `replays` of `dir`.
fn kept_in(dir: &Scratch) -> Library {
    Library::new(Box::new(Disk::new(dir.0.join("replays"))))
}

/// A player: the menu, its store and what it trusts.
pub(crate) struct Player {
    pub(crate) menu: MenuCore,
    trust: Trust,
    store: SettingsStore,
    _dir: Scratch,
}

impl Player {
    pub(crate) fn new(service: Option<&Service>) -> Player {
        let dir = Scratch::new("player");
        let store = SettingsStore::new(Some(dir.0.join(crate::config::FILE_NAME)));
        let trust = Trust::new(service.map(|s| s.pem.as_str())).expect("trust");
        let menu = MenuCore::new(
            store.clone(),
            trust.radovid.clone(),
            service.map(|s| s.url.clone()),
            kept_in(&dir),
        );
        Player {
            menu,
            trust,
            store,
            _dir: dir,
        }
    }

    /// Types into the page's fields, one after another.
    pub(crate) fn fill(&mut self, values: &[&str]) {
        if self.menu.page() == Page::Lobbies && self.menu.join_prompt.is_none() {
            self.menu
                .act(Act::LobbyUi(super::lobby_controls::Action::Create));
        }
        for (at, value) in values.iter().enumerate() {
            self.fill_field(at, value);
        }
    }

    pub(crate) fn fill_field(&mut self, at: usize, value: &str) {
        self.menu.act(Act::Focus(at));
        assert_eq!(self.menu.active_lobby_form().focused_field(), Some(at));
        while !self.menu.active_lobby_form().value(at).is_empty() {
            self.menu.act(Act::Erase);
        }
        self.menu.act(Act::Type(value.to_owned()));
    }

    pub(crate) fn register(&mut self, login: &str) {
        settle(&mut [self], |p| p.menu.page() == Page::Login);
        self.menu.act(Act::Go(Page::Register));
        self.fill(&[login, PASSWORD, PASSWORD]);
        self.menu.act(Act::Submit);
        settle(&mut [self], |p| p.menu.page() == Page::Home);
    }
}

/// Pumps every menu until `done` holds for all, within [`PATIENCE`].
fn settle(players: &mut [&mut Player], done: impl Fn(&Player) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while !players.iter().all(|p| done(p)) {
        assert!(Instant::now() < deadline, "the menus did not settle");
        for player in players.iter_mut() {
            player.menu.update();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Opens a launch as the client does and wraps it in a client core.
fn open(launch: Launch, trust: &Trust) -> Core {
    let mut opening = Opening::start(launch, trust);
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    loop {
        if let Some(opened) = opening.poll() {
            return Core::new(Source::Live(opened.expect("the game opens").link));
        }
        assert!(Instant::now() < deadline, "the game did not open");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Picks `hero`, readies, then attack-moves on the enemy's fountain.
fn play_step(core: &mut Core, hero: HeroId) {
    core.pump(1.0 / 30.0);
    if core.phase == Phase::Lobby && core.my_slot.is_some() && !core.ready {
        core.act(bota_client_core::Intent::PickHero(hero.0));
        core.act(bota_client_core::Intent::ToggleReady);
    }
    let enemy = core.session().map(|s| fountain_pos(s.map(), s.enemy()));
    if let Some(enemy) = enemy
        && core.view().is_some_and(|v| v.tick % 30 == 0)
        && core.over.is_none()
    {
        core.send_order(Order::Attack {
            target: Target::Pos(enemy),
        });
    }
}

fn solo() -> RulesetId {
    RulesetId(RULESET.to_owned())
}

fn launch_of(player: &mut Player) -> Launch {
    let launch = player.menu.take_launch().expect("a game to open");
    player.menu.game_started();
    launch
}

#[test]
fn two_players_register_find_accept_play_and_see_their_result() {
    let service = service();
    let (mut alice, mut bob) = (Player::new(Some(&service)), Player::new(Some(&service)));
    alice.register("alice");
    bob.register("bob");
    let PageView::Home { rulesets, .. } = alice.menu.view().body else {
        panic!("Play after registering");
    };
    assert_eq!(
        rulesets.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["1x1 Mid Only"],
        "Play lists what Find Match queues for; Squad and Practice are for lobbies"
    );

    alice.menu.act(Act::Find(solo()));
    settle(&mut [&mut alice], |p| p.menu.view().searches.len() == 1);
    assert_eq!(
        alice
            .menu
            .client
            .as_ref()
            .expect("client")
            .searches()
            .entries()[0]
            .position,
        1
    );
    assert_eq!(
        alice.menu.view().finding().0.len(),
        1,
        "the box shows a search nobody was found for"
    );
    bob.menu.act(Act::Find(solo()));
    settle(&mut [&mut alice, &mut bob], |p| {
        p.menu.view().offer.is_some()
    });
    for player in [&mut alice, &mut bob] {
        let view = player.menu.view();
        assert!(
            view.finding().0.is_empty(),
            "the prompt replaces the search box"
        );
        let offer = player.menu.view().offer.expect("MATCH FOUND");
        assert_eq!((offer.accepted, offer.of, offer.answered), (0, 2, false));
        player.menu.act(Act::Accept(offer.proposal));
    }
    // Both accepted: the game server starts and each menu opens its seat. The FINDING MATCH
    // box is gone from the moment a match was found: the prompt, the wait for a server that is
    // started at once and the game show no search.
    settle(&mut [&mut alice, &mut bob], |p| {
        let view = p.menu.view();
        let (searching, waiting) = view.finding();
        assert!(
            searching.is_empty() && waiting.is_empty(),
            "the search box stays: {searching:?} {waiting:?}"
        );
        p.menu.page() == Page::Game
    });
    let mut games = [
        (
            open(launch_of(&mut alice), &alice.trust),
            HeroId::SHADOW_FIEND,
        ),
        (open(launch_of(&mut bob), &bob.trust), HeroId::PUDGE),
    ];
    // Solo mid is a Shadow Fiend mirror: the lobby tells a pool of one, and each seat holds it
    // before anyone picks (bob's try at Pudge below changes nothing).
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while games.iter().any(|(core, _)| core.lobby.heroes.is_empty()) {
        assert!(Instant::now() < deadline, "no lobby state came");
        for (core, _) in &mut games {
            core.pump(1.0 / 30.0);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    for (core, _) in &games {
        assert_eq!(
            core.lobby.heroes,
            [HeroId::SHADOW_FIEND],
            "the pool is one hero"
        );
    }
    // The match ends when it ends, however slowly a loaded machine plays it: only a match that
    // stops moving is stuck.
    let (mut furthest, mut stuck_at) = (0, Instant::now() + Duration::from_secs(PATIENCE));
    while games.iter().any(|(core, _)| core.over.is_none()) {
        let tick = games
            .iter()
            .filter_map(|(core, _)| core.view().map(|v| v.tick))
            .max()
            .unwrap_or(0);
        if tick > furthest {
            (furthest, stuck_at) = (tick, Instant::now() + Duration::from_secs(PATIENCE));
        }
        assert!(
            Instant::now() < stuck_at,
            "the match stopped at tick {furthest}"
        );
        for (core, hero) in &mut games {
            play_step(core, *hero);
            assert!(!core.lost || core.over.is_some(), "the game was lost");
        }
        alice.menu.update();
        bob.menu.update();
        // Lockstep runs as fast as both answer; a client acks every tick,
        // and the server bounds the messages a second.
        std::thread::sleep(Duration::from_millis(2));
    }
    alice.menu.left_game(None, false);
    bob.menu.left_game(None, false);
    settle(&mut [&mut alice, &mut bob], |p| {
        p.menu.page() == Page::Result
    });
    // Both seats were handed the match's metadata file by name, whatever the service does with
    // replays, and it tells the match they played.
    settle(&mut [&mut alice, &mut bob], |p| p.menu.meta_ready());
    for player in [&mut alice, &mut bob] {
        let fetched = player.menu.take_meta().expect("the metadata was fetched");
        let bytes = fetched.bytes.expect("the service had it");
        let meta = bota_replay::meta::decode(&bytes).expect("a metadata file");
        assert!(meta.complete);
        assert_eq!(meta.header.match_id, fetched.match_id.0);
        let summary = crate::postgame::summary::MatchSummary::from_meta(&meta).expect("a summary");
        assert_eq!(summary.rows.len(), 2);
        let cards = player
            .menu
            .local_history
            .cards(player.menu.library.records());
        assert!(
            cards
                .iter()
                .any(|card| card.hero == Some(HeroId::SHADOW_FIEND)),
            "the once-received metadata is retained on this machine, even when replay retention is off"
        );
    }
    let headlines: Vec<String> = [&alice, &bob]
        .iter()
        .map(|p| match p.menu.view().body {
            PageView::Result(result) => {
                assert_eq!(result.seats.len(), 2);
                assert!(result.seats.iter().all(|s| !s.rating.is_empty()), "rated");
                assert_eq!(result.seats[0].hero_key, "npc_dota_hero_nevermore");
                result.headline
            }
            other => panic!("{other:?}"),
        })
        .collect();
    let mut sorted = headlines.clone();
    sorted.sort();
    assert_eq!(sorted, ["DEFEAT", "VICTORY"], "one won: {headlines:?}");
    both_keep_the_replay(&mut alice, &mut bob);
    bob_gets_a_replay_he_was_away_for(bob, &service);
    comes_back_logged_in(alice, &service);
}

/// The name pattern of a replay of the solo mid match `id`.
fn is_replay_of(name: &str, id: u64) -> bool {
    name.len() > 20 && name.ends_with(&format!("-solo_mid-{id}.brp"))
}

/// The match's replay was fetched from the service and kept by both seats with no click: a
/// file in the player's folder named by day, mode and match, a row in the index beside it, a
/// row on the REPLAYS page that opens in the replay viewer, and still there after a restart.
fn both_keep_the_replay(alice: &mut Player, bob: &mut Player) {
    settle(&mut [alice, bob], |p| p.menu.library.records().len() == 1);
    let id = match alice.menu.view().body {
        PageView::Result(_) => alice
            .menu
            .results
            .back()
            .map(|r| r.match_id.0)
            .expect("result"),
        other => panic!("{other:?}"),
    };
    for player in [&mut *alice, &mut *bob] {
        let record = player.menu.library.records()[0].clone();
        assert!(is_replay_of(&record.name, id), "{}", record.name);
        assert_eq!(record.mode, "solo_mid");
        assert_eq!(record.match_id, Some(id));
        let folder = player._dir.0.join("replays");
        let file = folder.join(&record.name);
        assert_eq!(
            std::fs::metadata(&file)
                .expect("the replay is in the folder")
                .len(),
            record.bytes
        );
        settle(&mut [&mut *player], |_| {
            std::fs::read_to_string(folder.join("index.txt"))
                .is_ok_and(|index| index.contains(&record.name))
        });
        assert!(
            player.menu.library.wanted().is_empty(),
            "nothing is waited for any more"
        );
        // The replays page lists it, with the players and how it ended, and opens it.
        player.menu.act(Act::Go(Page::Replays));
        let PageView::Replays { list, shelf, .. } = player.menu.view().body else {
            panic!("the replays page");
        };
        assert_eq!(list.len(), 1);
        assert!(shelf.keeping && shelf.folder.ends_with("replays"));
        let super::replay_info::Summary::Read { sides, outcome } = &list[0].summary else {
            panic!("{:?}", list[0].summary);
        };
        let sides = sides.text(&crate::dota_text::Texts::default());
        assert!(sides.contains("alice") && sides.contains("bob"), "{sides}");
        assert!(outcome.contains("won"), "{outcome}");
        player.menu.act(Act::OpenReplay(record.name.clone()));
        let Some(Launch::Replay(path)) = player.menu.take_launch() else {
            panic!("the replay opens in the viewer");
        };
        assert_eq!(path, file);
        let watched = super::replay_info::read(&path);
        assert!(
            matches!(&watched, super::replay_info::Summary::Read { outcome, .. } if outcome.contains("victory")),
            "{watched:?}"
        );
        player.menu.game_started();
        player.menu.left_game(None, true);
        assert_eq!(player.menu.page(), Page::Replays);
    }
    // After a restart the folder's index lists it again.
    let again = Library::new(Box::new(Disk::new(alice._dir.0.join("replays"))));
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    let mut again = again;
    while again.standing() != crate::replays::Standing::Ready {
        assert!(Instant::now() < deadline, "the folder was not read");
        again.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(again.records().len(), 1);
}

/// A client that was away when its match ended gets the replay at its next sign-in: the match
/// is noted in the folder's index, so a page closed mid-match still gets it.
fn bob_gets_a_replay_he_was_away_for(bob: Player, service: &Service) {
    let Player {
        mut menu,
        trust,
        store,
        _dir,
    } = bob;
    let record = menu.library.records()[0].clone();
    let id = record.match_id.expect("an online match");
    let folder = _dir.0.join("replays");
    menu.library.delete(&record.name);
    menu.library.want(crate::replays::Wanted {
        match_id: id,
        since: crate::replays::now(),
        mode: "solo_mid".to_owned(),
    });
    let mut away = Player {
        menu,
        trust,
        store,
        _dir,
    };
    settle(&mut [&mut away], |_| {
        !folder.join(&record.name).exists()
            && std::fs::read_to_string(folder.join("index.txt"))
                .is_ok_and(|index| index.contains(&format!("w\t{id}\t")))
    });
    drop(away.menu);
    let menu = MenuCore::new(
        away.store.clone(),
        away.trust.radovid.clone(),
        Some(service.url.clone()),
        kept_in(&away._dir),
    );
    let mut back = Player {
        menu,
        trust: away.trust,
        store: away.store,
        _dir: away._dir,
    };
    settle(&mut [&mut back], |p| p.menu.library.records().len() == 1);
    let kept = &back.menu.library.records()[0];
    assert_eq!(kept.match_id, Some(id));
    assert!(folder.join(&kept.name).is_file());
    assert!(back.menu.library.wanted().is_empty());
}

/// A new run of the client logs in with the kept token, without a password;
/// the token is readable by its owner only.
fn comes_back_logged_in(player: Player, service: &Service) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = player._dir.0.join("session");
        let mode = std::fs::metadata(path)
            .expect("the token is kept")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let Player {
        store, trust, _dir, ..
    } = player;
    let menu = MenuCore::new(
        store.clone(),
        trust.radovid.clone(),
        Some(service.url.clone()),
        kept_in(&_dir),
    );
    let mut again = Player {
        menu,
        trust,
        store,
        _dir,
    };
    settle(&mut [&mut again], |p| p.menu.page() == Page::Home);
    again.menu.act(Act::Logout);
    assert!(
        again.store.read(Entry::Session).is_none(),
        "logging out forgets the token"
    );
}

#[test]
fn a_decline_puts_the_other_back_in_place_and_cools_the_decliner_down() {
    let service = service();
    let (mut alice, mut bob) = (Player::new(Some(&service)), Player::new(Some(&service)));
    alice.register("alice");
    bob.register("bob");
    alice.menu.act(Act::Find(solo()));
    settle(&mut [&mut alice], |p| p.menu.view().searches.len() == 1);
    bob.menu.act(Act::Find(solo()));
    settle(&mut [&mut alice, &mut bob], |p| {
        p.menu.view().offer.is_some()
    });
    let offer = bob.menu.view().offer.expect("MATCH FOUND");
    let alices = alice.menu.view().offer.expect("MATCH FOUND");
    alice.menu.act(Act::Accept(alices.proposal));
    settle(&mut [&mut alice, &mut bob], |p| {
        p.menu.view().offer.is_some_and(|o| o.accepted == 1)
    });
    bob.menu.act(Act::Decline(offer.proposal));
    settle(&mut [&mut alice, &mut bob], |p| {
        p.menu.view().offer.is_none()
    });
    settle(&mut [&mut alice], |p| p.menu.view().searches.len() == 1);
    settle(&mut [&mut bob], |p| p.menu.view().searches.is_empty());
    assert_eq!(
        alice
            .menu
            .client
            .as_ref()
            .expect("client")
            .searches()
            .entries()[0]
            .position,
        1,
        "back at its place"
    );
    let PageView::Home { cooldown, .. } = bob.menu.view().body else {
        panic!("Play");
    };
    assert!(
        cooldown.is_some_and(|c| c.starts_with("You can search again in ")),
        "the cooldown shows"
    );
    bob.menu.act(Act::Find(solo()));
    settle(&mut [&mut bob], |p| {
        p.menu.view().status.is_some_and(|s| s.error)
    });
}

/// Types the local lobby page's address to join.
fn type_address(menu: &mut MenuCore, address: &str) {
    menu.act(Act::Focus(1));
    while !menu.form.value(1).is_empty() {
        menu.act(Act::Erase);
    }
    menu.act(Act::Type(address.to_owned()));
}

/// A free loopback port.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port()
}

/// A running game server. Its output stays open: a server whose pipe is closed fails on its next
/// line.
struct Server {
    child: Child,
    _output: BufReader<ChildStdout>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts the server on ports of its own choosing and returns it once it listens, with the TCP
/// and the WebSocket address it announced. A port picked beforehand could be taken by another
/// test's server by the time this one binds it.
fn start_server() -> (Server, String, String) {
    start_server_with(&[])
}

/// [`start_server`] with more of the server's options.
fn start_server_with(extra: &[&str]) -> (Server, String, String) {
    let mut child = crate::steering::scrubbed_command(server_binary())
        .args(extra)
        .args([
            "--map",
            "2",
            "--players",
            "2",
            "--mode",
            "lockstep",
            "--bind",
            "127.0.0.1",
        ])
        // Two human seats would wait out Dota's strategy time before the game starts.
        .args([
            "--tactical-secs",
            "0",
            "--pick-secs",
            "1",
            "--port",
            "0",
            "--ws-port",
            "0",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("bota_server starts");
    let mut output = BufReader::new(child.stdout.take().expect("piped"));
    let (mut tcp, mut ws) = (None, None);
    while tcp.is_none() || ws.is_none() {
        let mut line = String::new();
        let read = output.read_line(&mut line).expect("the server's output");
        assert!(read > 0, "the server ended before it listened");
        match line.trim().strip_prefix("bota_server listening on ") {
            Some(address) if address.starts_with("ws://") => ws = Some(address.to_owned()),
            Some(address) => tcp = Some(address.to_owned()),
            None => {}
        }
    }
    let server = Server {
        child,
        _output: output,
    };
    (server, tcp.expect("announced"), ws.expect("announced"))
}

/// A server for two seats, and a player joined to each over its own
/// transport (TCP, WebSocket) through the menu's Direct page, the match
/// begun.
fn direct_match() -> (Server, Vec<(Core, HeroId)>) {
    let (server, tcp, ws) = start_server();
    let mut cores = Vec::new();
    for (address, hero) in [(tcp, HeroId::SHADOW_FIEND), (ws, HeroId::PUDGE)] {
        let mut player = Player::new(None);
        assert_eq!(
            player.menu.page(),
            Page::LocalLobby,
            "no service known: the local pages"
        );
        player.menu.act(Act::Go(Page::LocalLobby));
        type_address(&mut player.menu, &address);
        player.menu.act(Act::Direct(false));
        let launch = player.menu.take_launch().expect("a game to open");
        assert_eq!(
            player.store.read(Entry::Menu).map(|m| m.contains(&address)),
            Some(true)
        );
        cores.push((join(launch, &player.trust), hero));
    }
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while cores.iter().any(|(c, _)| c.view().is_none()) {
        assert!(Instant::now() < deadline, "the match did not start");
        for (core, hero) in &mut cores {
            play_step(core, *hero);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    (server, cores)
}

/// Opens a launch: the server is listening already, so the connection is made or it is an error.
fn join(launch: Launch, trust: &Trust) -> Core {
    let mut opening = Opening::start(launch, trust);
    let opened = loop {
        if let Some(opened) = opening.poll() {
            break opened;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    match opened {
        Ok(opened) => Core::new(Source::Live(opened.link)),
        Err(err) => panic!("{err}"),
    }
}

#[test]
fn direct_connect_over_tcp_and_websocket_without_the_service() {
    let (_server, cores) = direct_match();
    assert!(
        cores
            .iter()
            .all(|(c, _)| c.phase == Phase::Playing && c.my_slot.is_some())
    );
}

/// Each player's scoreboard tells its own row's gold and ultimate (not
/// learned at level one), and of the other side the score and the level but
/// not its gold or ultimate, nor the items of a hero it has never seen.
#[test]
fn the_scoreboard_tells_a_player_only_what_its_side_may_know() {
    let (_server, mut cores) = direct_match();
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    let standing = |c: &Core| {
        c.view()
            .is_some_and(|v| v.players.iter().all(|p| p.level > 0))
    };
    while !cores
        .iter()
        .all(|(c, _)| standing(c) && c.my_hero().is_some())
    {
        assert!(Instant::now() < deadline, "the heroes did not come");
        for (core, _) in &mut cores {
            core.pump(1.0 / 30.0);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    for (core, _) in &cores {
        let lines = crate::scoreboard::lines(core);
        assert_eq!(lines.len(), 2, "a row a seat");
        let (own, foe): (Vec<_>, Vec<_>) = lines.iter().partition(|l| l.local);
        let (own, foe) = (own[0], foe[0]);
        assert!(!own.foe && foe.foe);
        assert!(own.gold.is_some(), "one's own gold: {own:?}");
        assert_eq!(own.ult, Some(crate::scoreboard::Ult::NotLearned));
        assert_eq!((foe.gold, foe.ult), (None, None));
        assert!(foe.level > 0 && foe.team != own.team);
        assert_eq!(foe.items, [None; 9], "never seen: {foe:?}");
    }
}

/// SETTINGS in the menu's top bar opens the settings screen over the page it
/// was on and DONE brings that page back; what was set there is in the
/// settings file the next start reads, a rebound key among it.
#[test]
fn the_settings_open_from_the_menu_and_last_past_a_restart() {
    let mut player = Player::new(None);
    let before = player.menu.page();
    let page_before = player.menu.view().body;
    player.menu.act(Act::Go(Page::Settings));
    assert_eq!(player.menu.page(), Page::Settings);
    assert_eq!(
        player.menu.view().body,
        page_before,
        "the page stays drawn under the settings"
    );
    assert!(player.menu.view().covers());
    player.menu.leave_settings();
    assert_eq!(player.menu.page(), before, "back where it was opened");

    let mut settings = crate::config::Settings::default();
    let at = crate::keys::index_of(crate::keys::Action::Stop).expect("bound");
    settings.keys[at] = crate::keys::Chord::parse("Ctrl+K");
    settings.volume = 0.25;
    player.store.save_settings(&settings).expect("saved");
    let restarted = SettingsStore::new(Some(player._dir.0.join(crate::config::FILE_NAME)));
    assert_eq!(restarted.load_settings(), settings);
}

/// Opens a launch as the client does, keeping the game server it started.
fn open_hosted(launch: Launch, trust: &Trust) -> (Core, Option<Hosted>) {
    let mut opening = Opening::start(launch, trust);
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    loop {
        if let Some(opened) = opening.poll() {
            let opened = opened.expect("the game opens");
            return (Core::new(Source::Live(opened.link)), opened.server);
        }
        assert!(Instant::now() < deadline, "the game did not open");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Pumps the cores until `done` holds for all, within [`PATIENCE`].
fn pump_until(what: &str, cores: &mut [&mut Core], done: impl Fn(&Core) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while !cores.iter().all(|c| done(c)) {
        assert!(
            Instant::now() < deadline,
            "{what}: {:?}",
            cores
                .iter()
                .map(|c| (
                    c.my_slot,
                    c.lost,
                    c.feed.last().map(|l| crate::feed::text(c, &l.kind))
                ))
                .collect::<Vec<_>>()
        );
        for core in cores.iter_mut() {
            core.pump(1.0 / 30.0);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A menu with no account whose service nobody answers at, named `name`
/// in its settings, keeping its replays in its own folder.
fn offline(name: &str, dir: &Scratch) -> (MenuCore, Trust) {
    let store = SettingsStore::new(Some(dir.0.join(name).join(crate::config::FILE_NAME)));
    let trust = Trust::new(None).expect("trust");
    let nowhere = format!("wss://127.0.0.1:{}", free_port());
    let mut menu = MenuCore::new(store, trust.radovid.clone(), Some(nowhere), kept_in(dir));
    menu.server_binary = Some(server_binary().to_owned());
    menu.set_profile_name(name);
    assert!(
        !menu.logged_in() && !menu.link_open(),
        "nothing is opened for the way in"
    );
    (menu, trust)
}

/// Everything that needs no service works with no account and no service
/// to be reached, as Dota's local games need no Steam login past the
/// client's own: the settings, a local lobby hosted under the profile's
/// name, a player joining it by address, its seat left and taken again
/// with the key it was claimed with, watching a game by address, the
/// local lobby's replay listed and opened, and quitting.
#[test]
fn every_local_path_works_with_no_account_and_no_service() {
    let dir = Scratch::new("local");
    let (mut alice, trust) = offline("alice", &dir);
    let top = alice.page();
    assert_eq!(top, Page::Login, "the way in, nothing more");

    alice.act(Act::Go(Page::Settings));
    assert_eq!(alice.page(), Page::Settings);
    alice.leave_settings();

    // A local lobby, hosted here.
    alice.act(Act::Go(Page::LocalLobby));
    assert_eq!(alice.page(), Page::LocalLobby);
    let port = free_port();
    let lobby_fields = [port.to_string()];
    alice.act(Act::Focus(0));
    while !alice.form.value(0).is_empty() {
        alice.act(Act::Erase);
    }
    alice.act(Act::Type(lobby_fields[0].clone()));
    alice.act(Act::Host(false));
    let launch = alice.take_launch().expect("the lobby to open");
    assert!(
        matches!(&launch, Launch::Host { name, rules: LocalRules::Full, .. } if name == "alice"),
        "{launch:?}"
    );
    let (mut hosted, server) = open_hosted(launch, &trust);
    assert!(server.is_some(), "the lobby's server is this client's");
    alice.game_started();

    // Another player joins it by its address, with no account either.
    let (mut bob, bob_trust) = offline("bob", &dir);
    bob.act(Act::Go(Page::LocalLobby));
    type_address(&mut bob, &format!("127.0.0.1:{port}"));
    bob.act(Act::Direct(false));
    let launch = bob.take_launch().expect("a game to join");
    assert!(matches!(&launch, Launch::Direct { name, key: Some(_), .. } if name == "bob"));
    let mut guest = open(launch, &bob_trust);
    bob.game_started();
    pump_until("both seated", &mut [&mut hosted, &mut guest], |c| {
        c.my_slot.is_some()
    });
    let seat = hosted.my_slot;
    assert_ne!(seat, guest.my_slot);

    // Escape in the game brings the dashboard over it; Escape on its top
    // page goes back to the game; DISCONNECT leaves a seat at once.
    alice.act(Act::Dashboard);
    assert_eq!(alice.page(), top);
    assert_eq!(alice.view().game, Some(GameKind::Play));
    alice.act(Act::Back);
    assert!(alice.shows_game());
    alice.act(Act::Dashboard);
    alice.act(Act::Disconnect);
    assert!(alice.take_leave(), "a seat is left without asking");
    drop(hosted);
    let left = seat.expect("seated");
    pump_until("the seat let go", &mut [&mut guest], |c| {
        c.lobby
            .slots
            .iter()
            .any(|s| s.slot == left && s.role.is_none())
    });
    alice.left_game(None, false);
    assert_eq!(
        alice.page(),
        Page::LocalLobby,
        "back where the lobby was made"
    );
    assert!(alice.view().reconnect);

    // RECONNECT takes the same seat, by the key it was claimed with.
    alice.act(Act::Reconnect);
    let launch = alice.take_launch().expect("the seat to take again");
    assert!(matches!(
        &launch,
        Launch::Direct {
            spectate: false,
            ..
        }
    ));
    let mut back = open(launch, &trust);
    alice.game_started();
    pump_until("seated again", &mut [&mut back, &mut guest], |c| {
        c.my_slot.is_some()
    });
    assert_eq!(back.my_slot, seat, "the same seat");

    // Watching a game by address, with no account.
    let (_watched, tcp, _) = start_server_with(&["--spectators", "open"]);
    let (mut carol, carol_trust) = offline("carol", &dir);
    carol.act(Act::Go(Page::LocalLobby));
    type_address(&mut carol, &tcp);
    carol.act(Act::Direct(true));
    let launch = carol
        .take_launch()
        .unwrap_or_else(|| panic!("a game to watch: {:?}", carol.view().status));
    assert!(matches!(
        &launch,
        Launch::Direct {
            spectate: true,
            key: None,
            ..
        }
    ));
    let mut watcher = open(launch, &carol_trust);
    carol.game_started();
    assert_eq!(carol.view().game, Some(GameKind::Watch));
    pump_until("watching", &mut [&mut watcher], |c| {
        c.lobby.me.is_some() || c.lost
    });
    assert!(!watcher.lost, "a spectator is let in");
    // Leaving a game watched asks first, as Dota's popup does.
    carol.act(Act::Dashboard);
    carol.act(Act::Disconnect);
    assert_eq!(carol.confirm, Some(Confirm::Leave));
    carol.act(Act::Answer(true));
    assert!(carol.take_leave());

    // A lobby whose match never started leaves no replay behind.
    drop(server);
    alice.left_game(None, false);
    alice.act(Act::Go(Page::Replays));
    assert!(
        alice.library.records().is_empty(),
        "a lobby that never began leaves no replay"
    );
    let PageView::Replays { list, .. } = alice.view().body else {
        panic!("the replays page");
    };
    assert!(list.is_empty());

    // Escape backs out to the top page, then asks to quit; Escape again or
    // Cancel closes the question, OK quits.
    alice.act(Act::Back);
    assert_eq!(alice.page(), top);
    alice.act(Act::Back);
    assert_eq!(alice.confirm, Some(Confirm::Quit));
    alice.act(Act::Back);
    assert_eq!(alice.confirm, None);
    alice.act(Act::Quit);
    alice.act(Act::Answer(false));
    assert_eq!(alice.confirm, None);
    assert!(!alice.take_quit());
    alice.act(Act::Quit);
    alice.act(Act::Answer(true));
    assert!(alice.take_quit());
}

/// Opens the local lobby page, chooses `rules` if any, and returns the launch of hosting it on
/// a free port.
fn host_launch(menu: &mut MenuCore, rules: Option<LocalRules>) -> Launch {
    menu.act(Act::Go(Page::LocalLobby));
    if let Some(rules) = rules {
        menu.act(Act::Rules(rules));
    }
    let port = free_port();
    menu.act(Act::Focus(0));
    while !menu.form.value(0).is_empty() {
        menu.act(Act::Erase);
    }
    menu.act(Act::Type(port.to_string()));
    menu.act(Act::Host(false));
    menu.take_launch().expect("the lobby to open")
}

/// A local lobby has ten seats and full rules unless the rules are changed, and changing them
/// changes the rules only; nothing fills the seats nobody takes, so a lobby with one player
/// seated starts a match with exactly one hero (one a seat taken, on the map of the rules).
#[test]
fn a_local_lobby_has_ten_seats_under_any_rules_and_starts_with_the_seated() {
    let dir = Scratch::new("ten");
    for (chosen, rules, map) in [
        (None, LocalRules::Full, MapId(0)),
        (Some(LocalRules::SoloMid), LocalRules::SoloMid, MapId(2)),
        (Some(LocalRules::Full), LocalRules::Full, MapId(0)),
    ] {
        let (mut alice, trust) = offline("alice", &dir);
        let launch = host_launch(&mut alice, chosen);
        assert!(
            matches!(&launch, Launch::Host { rules: got, .. } if *got == rules),
            "{launch:?}"
        );
        let (mut hosted, server) = open_hosted(launch, &trust);
        alice.game_started();
        // The lobby has all ten seats, one of them the host's.
        pump_until("the host seated", &mut [&mut hosted], |c| {
            c.my_slot.is_some() && c.lobby.slots.len() == usize::from(LOCAL_SEATS)
        });
        // Nobody else comes: the host picks and readies, and the match is the host alone.
        let deadline = Instant::now() + Duration::from_secs(PATIENCE);
        while hosted.view().is_none_or(|v| v.tick <= 5) {
            assert!(
                Instant::now() < deadline,
                "{rules:?}: the match did not start"
            );
            play_step(&mut hosted, HeroId::SHADOW_FIEND);
            std::thread::sleep(Duration::from_millis(5));
        }
        let heroes = hosted.view().map_or(0, |v| v.players.len());
        assert_eq!(heroes, 1, "{rules:?}: one player is one hero");
        assert_eq!(hosted.session().map(|s| s.map().id), Some(map));
        drop((hosted, server));
    }
}

/// The gold of a client's own seat, as its view tells.
fn own_gold(core: &Core) -> i32 {
    core.view()
        .zip(core.my_slot)
        .and_then(|(view, slot)| view.player(slot))
        .and_then(|player| player.gold)
        .unwrap_or(0)
}

/// Demo Hero from the menu: a signed-in account asks the service for it, and its match (the Demo
/// ruleset's, a game server of cheats started by the service) comes with the hero chosen to
/// pick; a cheat is honoured, and quitting it ends the match and shows no result.
#[test]
fn demo_hero_starts_a_cheat_match_of_the_service_with_the_hero_chosen() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    alice.menu.act(Act::Go(Page::DemoHero));
    assert_eq!(alice.menu.page(), Page::DemoHero);
    assert!(
        matches!(
            alice.menu.view().body,
            PageView::DemoHero { offered: true, .. }
        ),
        "the service offers a Demo Hero"
    );
    alice.menu.act(Act::Demo(HeroId::PUDGE.0));
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Game);
    assert!(
        alice.menu.lobby.is_none(),
        "no lobby and no countdown for a Demo"
    );
    let launch = launch_of(&mut alice);
    assert!(matches!(&launch, Launch::Demo { hero, .. } if *hero == HeroId::PUDGE));
    let mut core = open(launch, &alice.trust);
    assert_eq!(alice.menu.demo_hero(), Some(HeroId::PUDGE));
    pump_until("seated", &mut [&mut core], |c| c.my_slot.is_some());
    core.act(bota_client_core::Intent::PickHero(HeroId::PUDGE.0));
    core.act(bota_client_core::Intent::ToggleReady);
    pump_until("the hero stands", &mut [&mut core], |c| {
        c.my_hero().is_some()
    });
    assert!(
        core.statics().is_some_and(|s| s.metadata().cheats),
        "a match that cheats"
    );
    let before = own_gold(&core);
    core.act(bota_client_core::Intent::Cheat(bota_proto::Cheat::Gold {
        amount: 5000,
    }));
    pump_until("the gold comes", &mut [&mut core], |c| {
        own_gold(c) >= before + 5000
    });
    // Leaving a Demo Hero ends it: no seat to take again, and its result is no news.
    alice.menu.act(Act::Dashboard);
    alice.menu.act(Act::Disconnect);
    assert!(alice.menu.take_leave_game());
    core.leave_game();
    assert!(!alice.menu.take_leave(), "the leave goes out first");
    alice.menu.left_game(None, false);
    assert_eq!(alice.menu.page(), Page::DemoHero);
    assert!(!alice.menu.view().reconnect);
    // The lockstep server takes the leave with the client's next acknowledgement.
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while !alice.menu.games.is_empty() {
        assert!(Instant::now() < deadline, "the Demo Hero did not end");
        core.pump(1.0 / 30.0);
        alice.menu.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(alice.menu.page(), Page::DemoHero);
    assert!(alice.menu.results.is_empty());
}

/// The Demo Hero is the service's to offer: a menu without one says so, and the Demo ruleset is
/// no game mode of a lobby.
#[test]
fn a_service_without_a_demo_falls_back_to_the_local_sandbox() {
    let service = service_with(|config| config.replace("info = \"01\"", ""));
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    alice.menu.act(Act::Go(Page::DemoHero));
    assert!(matches!(
        alice.menu.view().body,
        PageView::DemoHero { offered: false, .. }
    ));
    alice.menu.act(Act::Demo(HeroId::PUDGE.0));
    // Its own catalogue still offers none; the hero action can use the local sandbox.
    assert!(matches!(
        alice.menu.take_launch(),
        Some(Launch::LocalDemo {
            hero: HeroId::PUDGE,
            ..
        })
    ));
}

/// A local game the host leaves for good (the match ends: the leaver's side loses) leaves its
/// replay: the server's own file, finished by the server, kept when the game is left, listed
/// as a local game and opened by the viewer.
#[test]
fn a_local_game_left_for_good_leaves_its_replay() {
    let dir = Scratch::new("local-replay");
    let (mut menu, trust) = offline("alice", &dir);
    menu.act(Act::Go(Page::LocalLobby));
    let port = free_port();
    for (at, value) in [(0, port.to_string()), (1, "1".to_owned())] {
        menu.act(Act::Focus(at));
        while !menu.form.value(at).is_empty() {
            menu.act(Act::Erase);
        }
        menu.act(Act::Type(value));
    }
    menu.act(Act::Host(false));
    let launch = menu.take_launch().expect("the lobby to open");
    let Launch::Host {
        replay: Some(recording),
        ..
    } = &launch
    else {
        panic!("the server is told where to record: {launch:?}");
    };
    let recording = recording.clone();
    let (mut game, server) = open_hosted(launch, &trust);
    menu.game_started();
    pump_until("seated", &mut [&mut game], |c| c.my_slot.is_some());
    menu.set_local_recording_owner(game.my_slot.expect("the local session's seat"));
    game.act(bota_client_core::Intent::PickHero(HeroId::SHADOW_FIEND.0));
    game.act(bota_client_core::Intent::ToggleReady);
    pump_until("the match runs", &mut [&mut game], |c| {
        c.view().is_some_and(|v| v.tick > 5)
    });
    game.leave_game();
    wait_for_recording(&recording, &mut game);
    drop(server);
    menu.set_game_unsafe(false);
    menu.watch_result(crate::postgame::view::Replay::File(recording.clone()))
        .expect("WATCH is allowed after the game ended");
    assert!(menu.take_leave(), "WATCH first closes the ended session");
    menu.left_game(None, true);
    settle_menu(&mut menu, |m| {
        m.library.records().len() == 1 && m.pending_result_replay.is_none()
    });
    let record = menu.library.records()[0].clone();
    assert_eq!(record.mode, "local");
    let Some(Launch::Replay(launched)) = menu.take_launch() else {
        panic!("post-game WATCH opens the kept recording");
    };
    assert_eq!(
        launched,
        menu.library.path(&record.name).expect("kept replay path")
    );
    assert!(
        launched.is_file(),
        "WATCH follows adoption instead of the removed temporary path"
    );
    menu.game_started();
    menu.left_game(None, true);
    assert_eq!(record.owner, game.my_slot);
    let mut file = std::fs::File::open(menu.library.path(&record.name).unwrap()).unwrap();
    assert!(
        bota_client_core::meta_in_replay(&mut file)
            .unwrap()
            .is_some(),
        "local recordings carry the metadata Profile reads"
    );
    assert!(!recording.exists(), "the recording became the kept replay");
    wait_for_recorded_hero(&mut menu, HeroId::SHADOW_FIEND);
    menu.act(Act::Go(Page::Replays));
    let PageView::Replays { list, .. } = menu.view().body else {
        panic!("the replays page");
    };
    assert_eq!(list.len(), 1);
    menu.act(Act::OpenReplay(record.name.clone()));
    assert!(matches!(menu.take_launch(), Some(Launch::Replay(_))));
}

fn wait_for_recording(recording: &Path, game: &mut Core) {
    // The server ends the match, finishes its file and exits.
    let deadline = Instant::now() + Duration::from_secs(15);
    while std::fs::metadata(recording).map_or(0, |m| m.len()) < 1024
        || std::fs::File::open(recording)
            .ok()
            .and_then(|mut file| bota_client_core::meta_in_replay(&mut file).ok().flatten())
            .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "the server never finished its replay"
        );
        game.pump(1.0 / 30.0);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_recorded_hero(menu: &mut MenuCore, hero: HeroId) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while menu
        .local_history
        .cards(menu.library.records())
        .iter()
        .all(|card| card.hero != Some(hero))
    {
        assert!(
            Instant::now() < deadline,
            "the local recording never reached Profile"
        );
        menu.update();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Pumps one menu until `done` holds, within [`PATIENCE`].
fn settle_menu(menu: &mut MenuCore, done: impl Fn(&MenuCore) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while !done(menu) {
        assert!(Instant::now() < deadline, "the menu did not settle");
        menu.update();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The local lobby's host turns cheats on, as Dota's "Enable cheats": the
/// match it starts honours them and says so.
#[test]
fn a_local_lobby_with_cheats_enabled_is_a_cheat_match() {
    let dir = Scratch::new("lobby-cheats");
    let (mut menu, trust) = offline("alice", &dir);
    menu.act(Act::Go(Page::LocalLobby));
    assert!(!menu.view().host_cheats, "off unless asked for");
    menu.act(Act::ToggleCheats);
    assert!(menu.view().host_cheats);
    menu.act(Act::Focus(0));
    while !menu.form.value(0).is_empty() {
        menu.act(Act::Erase);
    }
    menu.act(Act::Type(free_port().to_string()));
    menu.act(Act::Host(false));
    let launch = menu.take_launch().expect("the lobby to open");
    assert!(matches!(&launch, Launch::Host { cheats: true, .. }));
    let (mut core, _server) = open_hosted(launch, &trust);
    pump_until("seated", &mut [&mut core], |c| c.my_slot.is_some());
    core.act(bota_client_core::Intent::PickHero(HeroId::SHADOW_FIEND.0));
    core.act(bota_client_core::Intent::ToggleReady);
    pump_until("the match starts", &mut [&mut core], |c| {
        c.statics().is_some()
    });
    assert!(core.statics().is_some_and(|s| s.metadata().cheats));
}

/// Types the login page's fields.
fn type_login(player: &mut Player, login: &str, password: &str) {
    player.fill(&[login, password]);
}

/// Pumps the menu for `secs`.
fn sit(player: &mut Player, secs: u64) {
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        player.menu.update();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A person types for minutes on the sign-in form, the service's own deadlines for a silent
/// connection long past: nothing is open meanwhile, nothing typed is lost, and sending it works.
#[test]
fn a_slow_form_holds_no_connection_and_signs_in_when_sent() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    // The longest of the service's deadlines for a link that says nothing is a minute.
    alice.menu.act(Act::Go(Page::Register));
    alice.fill(&["slow_alice"]);
    sit(&mut alice, 70);
    assert!(!alice.menu.link_open(), "a form opens no link");
    assert!(alice.menu.view().status.is_none(), "and nothing went wrong");
    alice.fill(&["slow_alice", PASSWORD, PASSWORD]);
    alice.menu.act(Act::Submit);
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Home);
}

/// A refusal shows on the form with what was typed still there, and no link is left open for it.
#[test]
fn a_wrong_password_leaves_the_form_as_typed_and_the_link_closed() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    alice.menu.act(Act::Logout);
    assert_eq!(alice.menu.page(), Page::Login);
    type_login(&mut alice, "alice", "not the password");
    alice.menu.act(Act::Submit);
    settle(&mut [&mut alice], |p| {
        p.menu.view().status.is_some_and(|s| s.error) && !p.menu.connecting()
    });
    let status = alice.menu.view().status.expect("a message");
    assert_eq!(status.text, "Wrong login or password");
    assert_eq!(alice.menu.page(), Page::Login);
    assert_eq!(alice.menu.form.value(0), "alice");
    assert_eq!(alice.menu.form.value(1), "not the password");
    sit(&mut alice, 12);
    assert!(!alice.menu.link_open());
    assert!(
        alice
            .menu
            .view()
            .status
            .is_some_and(|s| s.text == status.text),
        "no later drop"
    );
    type_login(&mut alice, "alice", PASSWORD);
    alice.menu.act(Act::Submit);
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Home);
}

/// A service that is not there is said on the form, the typed values kept, and a later try
/// works as soon as it is.
#[test]
fn a_service_that_is_down_at_submit_is_said_on_the_form() {
    let dir = Scratch::new("down");
    let (mut alice, _trust) = offline("alice", &dir);
    let mut player = Player {
        store: alice_store(&alice),
        trust: Trust::new(None).expect("trust"),
        menu: {
            alice.act(Act::Go(Page::Register));
            alice
        },
        _dir: dir,
    };
    player.fill(&["alice", PASSWORD, PASSWORD]);
    player.menu.act(Act::Submit);
    settle(&mut [&mut player], |p| {
        p.menu.view().status.is_some_and(|s| s.error) && !p.menu.connecting()
    });
    let status = player.menu.view().status.expect("a message");
    assert!(
        status.text.starts_with("Cannot reach the service"),
        "{}",
        status.text
    );
    assert_eq!(player.menu.page(), Page::Register);
    assert_eq!(player.menu.form.value(0), "alice");
    assert_eq!(player.menu.form.value(1), PASSWORD);
    assert!(!player.menu.link_open());
}

fn alice_store(menu: &MenuCore) -> SettingsStore {
    menu.store.clone()
}

/// Sign in, search, cancel, sit idle until the link is let go, search again: the account stays
/// signed in across the closed link, which the next request opens again.
#[test]
fn a_link_let_go_when_idle_opens_again_for_the_next_search() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    alice.menu.act(Act::Find(solo()));
    settle(&mut [&mut alice], |p| p.menu.view().searches.len() == 1);
    assert!(alice.menu.link_open(), "a search holds the link");
    sit(&mut alice, 12);
    assert!(alice.menu.link_open(), "and keeps it while it lasts");
    let entry = alice.menu.view().searches[0].entry;
    alice.menu.act(Act::Cancel(entry));
    settle(&mut [&mut alice], |p| p.menu.view().searches.is_empty());
    settle(&mut [&mut alice], |p| !p.menu.link_open());
    assert!(alice.menu.logged_in(), "the account stays signed in");
    assert_eq!(alice.menu.page(), Page::Home);
    alice.menu.act(Act::Find(solo()));
    settle(&mut [&mut alice], |p| p.menu.view().searches.len() == 1);
    assert_eq!(
        alice
            .menu
            .client
            .as_ref()
            .expect("client")
            .searches()
            .entries()[0]
            .position,
        1
    );
    assert_eq!(alice.menu.page(), Page::Home, "no page jumped meanwhile");
}

/// Tab goes through the fields and then the buttons of a form, Shift+Tab back, and Enter
/// presses the button that has the keyboard.
#[test]
fn tab_walks_the_form_and_enter_presses_the_focused_button() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    assert_eq!(alice.menu.page(), Page::Login);
    let at = |p: &Player| {
        (
            p.menu.form.focused_field(),
            p.menu.form.focused_button().cloned(),
        )
    };
    assert_eq!(at(&alice), (Some(0), None), "the first empty field");
    for expected in [
        (Some(1), None),
        (None, Some(Act::Submit)),
        (None, Some(Act::Go(Page::Register))),
        (None, Some(Act::Go(Page::Service))),
        (None, Some(Act::Go(Page::LocalLobby))),
        (Some(0), None),
    ] {
        alice.menu.act(Act::NextField);
        assert_eq!(at(&alice), expected);
    }
    alice.menu.act(Act::PreviousField);
    assert_eq!(at(&alice), (None, Some(Act::Go(Page::LocalLobby))));
    alice.menu.act(Act::PreviousField);
    alice.menu.act(Act::PreviousField);
    alice.menu.act(Act::PreviousField);
    assert_eq!(at(&alice), (None, Some(Act::Submit)));
    alice.menu.act(Act::NextField);
    alice.menu.act(Act::Submit);
    assert_eq!(alice.menu.page(), Page::Register, "Enter on CREATE ACCOUNT");
    assert_eq!(
        alice.menu.button_focus(),
        None,
        "a new form starts in its fields"
    );
    // Typing goes to a field only.
    alice.menu.act(Act::NextField);
    alice.menu.act(Act::NextField);
    alice.menu.act(Act::NextField);
    alice.menu.act(Act::Type("x".to_owned()));
    assert!(
        alice
            .menu
            .form
            .fields()
            .iter()
            .all(|f| f.shown().is_empty())
    );
}

/// The local lobby page walks as it is drawn: its port, CREATE, then the address to join with
/// PLAY and WATCH below.
#[test]
fn tab_walks_the_local_lobby_page_in_the_order_it_is_drawn() {
    let dir = Scratch::new("tab_local");
    let (mut alice, _trust) = offline("alice", &dir);
    alice.act(Act::Go(Page::LocalLobby));
    let at = |m: &MenuCore| (m.form.focused_field(), m.form.focused_button().cloned());
    assert_eq!(at(&alice), (Some(0), None));
    for expected in [
        (None, Some(Act::Host(false))),
        (None, Some(Act::Host(true))),
        (Some(1), None),
        (None, Some(Act::Direct(false))),
        (None, Some(Act::Direct(true))),
        (Some(0), None),
    ] {
        alice.act(Act::NextField);
        assert_eq!(at(&alice), expected);
    }
}

/// A ruleset for bots only is not listed to a human, and the one for everyone says who may play
/// it under its name.
#[test]
fn a_ruleset_for_bots_is_not_listed_to_humans() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    let PageView::Home { rulesets, .. } = alice.menu.view().body else {
        panic!("Play after registering");
    };
    assert_eq!(
        rulesets.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["1x1 Mid Only"]
    );
}

/// Makes a lobby from the LOBBIES page with `fields` (its name, and its password if any), then
/// changes its game mode to `mode` in the lobby, as its host.
fn create_lobby(player: &mut Player, mode: &str, fields: &[&str]) {
    player.menu.act(Act::Go(Page::Lobbies));
    player.fill(fields);
    player.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [player], |p| p.menu.page() == Page::Lobby);
    let id = lobby_of(player)
        .settings
        .modes
        .iter()
        .find(|m| m.name == mode)
        .unwrap_or_else(|| panic!("the mode {mode} is offered"))
        .id
        .clone();
    player.menu.act(Act::LobbyUi(super::LobbyAction::Edit));
    player
        .menu
        .act(Act::LobbyUi(super::LobbyAction::EditMode(id)));
    player.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [player], |p| lobby_of(p).settings.mode == mode);
}

/// A new lobby is of the mode the service names its default, first or not, and its host changes
/// it in the lobby among those the service offers this account (not the bots-only one).
#[test]
fn a_new_lobby_is_of_the_services_default_mode_and_its_host_changes_it() {
    let service = service_with(|config| {
        config.replacen("[password]", "lobby_ruleset = \"squad\"\n[password]", 1)
    });
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    alice.menu.act(Act::Go(Page::Lobbies));
    alice.fill(&["my practice"]);
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Lobby);
    let modes = lobby_of(&alice).settings.modes;
    assert_eq!(
        modes
            .iter()
            .map(|m| (m.name.as_str(), m.chosen))
            .collect::<Vec<_>>(),
        [
            ("Full rules", false),
            ("1x1 Mid Only", false),
            ("Squad", true),
            ("Practice", false)
        ],
        "the service's default, none bots-only"
    );
    let practice = modes
        .iter()
        .find(|m| m.name == "Practice")
        .expect("offered");
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Edit));
    alice.menu.act(Act::LobbyUi(super::LobbyAction::EditMode(
        practice.id.clone(),
    )));
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [&mut alice], |p| {
        p.menu
            .lobby
            .as_ref()
            .is_some_and(|(state, _)| state.ruleset.0 == "practice")
    });
}

/// A host starts a lobby of a ruleset that allows it with nobody else: the empty seat stays empty
/// and the match is played by the host alone.
#[test]
fn a_host_starts_a_practice_lobby_alone_and_plays() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    create_lobby(&mut alice, "Practice", &["alone"]);
    let PageView::Lobby(lobby) = alice.menu.view().body else {
        panic!("the lobby page");
    };
    assert!(lobby.starts_short && lobby.waiting.is_none(), "{lobby:?}");
    alice.menu.act(Act::ReadyLobby(true));
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Game);
    let mut game = open(launch_of(&mut alice), &alice.trust);
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while game.view().is_none_or(|v| v.tick <= 5) {
        assert!(Instant::now() < deadline, "the match never began");
        play_step(&mut game, HeroId::SHADOW_FIEND);
        std::thread::sleep(Duration::from_millis(5));
    }
    let view = game.view().expect("the match runs");
    assert_eq!(view.players.len(), 1, "the host alone, no dummy");
}

/// Plays the practice lobby alone, leaves the match for good (the leaver's side loses) and
/// waits for the service to tell the result.
fn play_alone_and_leave(service: &Service) -> Player {
    let mut alice = Player::new(Some(service));
    alice.register("alice");
    create_lobby(&mut alice, "Practice", &["alone"]);
    alice.menu.act(Act::ReadyLobby(true));
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Game);
    let mut game = open(launch_of(&mut alice), &alice.trust);
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while game.view().is_none_or(|v| v.tick <= 5) {
        assert!(Instant::now() < deadline, "the match never began");
        play_step(&mut game, HeroId::SHADOW_FIEND);
        std::thread::sleep(Duration::from_millis(5));
    }
    game.leave_game();
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while alice.menu.results.is_empty() {
        assert!(Instant::now() < deadline, "the match did not end");
        game.pump(1.0 / 30.0);
        alice.menu.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    alice.menu.left_game(None, true);
    alice
}

/// A service that keeps no replays says so when asked for one: the client saves nothing, shows
/// nothing broken, and tells on the REPLAYS page that the service keeps none. Nothing was asked
/// of the game server either.
#[test]
fn a_service_that_keeps_no_replays_leaves_the_client_with_none_and_a_note() {
    let service = service_saving(false);
    let mut alice = play_alone_and_leave(&service);
    settle(&mut [&mut alice], |p| p.menu.service_keeps_none());
    assert!(alice.menu.library.records().is_empty());
    assert!(
        alice.menu.library.wanted().is_empty(),
        "nothing is waited for"
    );
    alice.menu.act(Act::Go(Page::Replays));
    let PageView::Replays { list, shelf, .. } = alice.menu.view().body else {
        panic!("the replays page");
    };
    assert!(list.is_empty() && shelf.service_keeps_none && shelf.keeping);
    let status = alice.menu.view().status;
    assert!(
        status.as_ref().is_none_or(|s| !s.error),
        "nothing looks broken: {status:?}"
    );
    assert!(
        !service._dir.0.join("artifacts").exists(),
        "the game server was never asked for a file"
    );
}

/// The same match where the service keeps replays: the one-player match's replay is kept with
/// no click, whatever way the match ended.
#[test]
fn a_match_left_early_still_leaves_its_replay() {
    let service = service();
    let mut alice = play_alone_and_leave(&service);
    settle(&mut [&mut alice], |p| p.menu.library.records().len() == 1);
    let record = alice.menu.library.records()[0].clone();
    assert_eq!(record.mode, "practice");
    assert!(alice._dir.0.join("replays").join(&record.name).is_file());
    alice.menu.act(Act::Go(Page::Replays));
    alice.menu.act(Act::OpenReplay(record.name.clone()));
    assert!(matches!(alice.menu.take_launch(), Some(Launch::Replay(_))));
}

/// A new lobby is made for the full rules with ten slots, five a side; the owner choosing solo
/// mid changes the rules and nothing else (the slots and who sits in them stay), and the host
/// alone starts a match with exactly one hero: nobody fills the empty slots.
#[test]
fn a_new_lobby_is_full_rules_with_ten_slots_and_solo_mid_changes_the_rules_only() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    alice.menu.act(Act::Go(Page::Lobbies));
    alice.fill(&["default"]);
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Lobby);
    let (state, _) = alice.menu.lobby.as_ref().expect("sat in the lobby");
    assert_eq!(
        state.ruleset.0, "full",
        "the full rules unless another is chosen"
    );
    let slots = |lobby: &super::view::LobbyView| {
        lobby
            .teams
            .iter()
            .map(|t| (t.name.clone(), t.slots.len()))
            .collect::<Vec<_>>()
    };
    let full = [("RADIANT".to_owned(), 5), ("DIRE".to_owned(), 5)];
    assert_eq!(slots(&lobby_of(&alice)), full);

    let solo = lobby_of(&alice)
        .settings
        .modes
        .iter()
        .find(|m| m.name == "1x1 Mid Only")
        .expect("offered")
        .id
        .clone();
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Edit));
    alice
        .menu
        .act(Act::LobbyUi(super::LobbyAction::EditMode(solo)));
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [&mut alice], |p| {
        lobby_of(p).settings.mode == "1x1 Mid Only"
    });
    assert_eq!(slots(&lobby_of(&alice)), full, "ten slots under solo mid");
    assert_eq!(
        seat_of(&lobby_of(&alice), "alice"),
        Some((0, "RADIANT".to_owned()))
    );

    alice.menu.act(Act::ReadyLobby(true));
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Game);
    let mut game = open(launch_of(&mut alice), &alice.trust);
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while game.view().is_none_or(|v| v.tick <= 5) {
        assert!(Instant::now() < deadline, "the match never began");
        play_step(&mut game, HeroId::SHADOW_FIEND);
        std::thread::sleep(Duration::from_millis(5));
    }
    let view = game.view().expect("the match runs");
    assert_eq!(view.players.len(), 1, "the host alone: no bot, no dummy");
}

/// The harness stops at once, naming both wires and the way out, when its server is of another
/// wire than the client it tests (bota moved after the client was compiled).
#[cfg(unix)]
#[test]
fn the_harness_refuses_a_server_of_another_wire_at_once() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("harness_wire");
    let stale = dir.0.join("bota_server");
    std::fs::write(&stale, "#!/bin/sh\necho 0000000000000001\n").expect("a script");
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let said = wire_checked(stale.clone(), 0xabcd, WIRE_PROBE_PATIENCE).expect_err("another wire");
    assert_eq!(
        said,
        format!(
            "{} speaks wire 0000000000000001 but this client was compiled for wire \
             000000000000abcd: rebuild the client against the current bota, or check out the \
             bota it was compiled with",
            stale.display()
        )
    );
    assert_eq!(
        wire_checked(
            server_binary().to_owned(),
            bota_radovid::fingerprint(),
            WIRE_PROBE_PATIENCE,
        )
        .expect("the same"),
        server_binary()
    );
}

/// A server that never answers its wire probe is killed after the patience given, and the harness
/// says which binary hung.
#[cfg(unix)]
#[test]
fn the_harness_gives_up_on_a_server_that_never_answers_its_wire_probe() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("hanging_wire");
    let hanging = dir.0.join("bota_server");
    std::fs::write(&hanging, "#!/bin/sh\nexec sleep 60\n").expect("a script");
    std::fs::set_permissions(&hanging, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let said = wire_checked(hanging.clone(), 0xabcd, Duration::from_secs(1)).expect_err("a hang");
    assert_eq!(
        said,
        format!(
            "{} did not answer its wire probe within 1s and was killed",
            hanging.display()
        )
    );
}

/// A script standing in for a game server, in a scratch folder of its own.
#[cfg(target_os = "linux")]
fn stub_server(what: &str, script: &str) -> (Scratch, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new(what);
    let stub = dir.0.join("bota_server");
    std::fs::write(&stub, script).expect("a script");
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).expect("mode");
    (dir, stub)
}

/// Waits for the process the stub recorded in `pid` to be gone (or a zombie nobody reaped yet).
#[cfg(target_os = "linux")]
fn recorded_process_ends(pid: &Path) {
    let pid = std::fs::read_to_string(pid).expect("the stub recorded its sleeper");
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    loop {
        match std::fs::read_to_string(format!("/proc/{}/stat", pid.trim())) {
            Err(_) => return,
            Ok(stat)
                if stat
                    .rsplit(") ")
                    .next()
                    .is_some_and(|rest| rest.starts_with('Z')) =>
            {
                return;
            }
            Ok(_) => {}
        }
        assert!(
            Instant::now() < deadline,
            "the stub's sleeper {pid} survived the probe"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A server that exits at once but leaves a background process holding its output open does not
/// keep the probe waiting past its patience, and the leftover is killed with it.
#[cfg(target_os = "linux")]
#[test]
fn the_wire_probe_does_not_wait_for_a_background_process_holding_the_output() {
    let (dir, stub) = stub_server(
        "background_wire",
        "#!/bin/sh\nsleep 100 &\necho $! > \"$(dirname \"$0\")/pid\"\nexit 0\n",
    );
    let said = wire_checked(stub.clone(), 0xabcd, Duration::from_secs(1)).expect_err("no answer");
    assert_eq!(
        said,
        format!(
            "{} did not answer its wire probe within 1s and was killed",
            stub.display()
        )
    );
    recorded_process_ends(&dir.0.join("pid"));
}

/// A server that answers and leaves a background process behind is believed, and the process is
/// killed: the probe owns everything it started.
#[cfg(target_os = "linux")]
#[test]
fn the_wire_probe_kills_what_an_answering_server_left_running() {
    let (dir, stub) = stub_server(
        "leftover_wire",
        "#!/bin/sh\nsleep 100 &\necho $! > \"$(dirname \"$0\")/pid\"\necho 000000000000abcd\nexit 0\n",
    );
    assert_eq!(
        wire_checked(stub.clone(), 0xabcd, Duration::from_secs(PATIENCE)).expect("the answer"),
        stub
    );
    recorded_process_ends(&dir.0.join("pid"));
}

/// A server that floods its output is cut off after a few bytes, and the error quotes only a
/// short prefix of them.
#[cfg(target_os = "linux")]
#[test]
fn the_wire_probe_cuts_off_a_server_that_floods_its_output() {
    let (_dir, stub) = stub_server("flood_wire", "#!/bin/sh\nexec tr '\\0' a < /dev/zero\n");
    let said = wire_checked(stub.clone(), 0xabcd, Duration::from_secs(20)).expect_err("flood");
    assert_eq!(
        said,
        format!(
            "{} did not say its wire in its first 64 bytes: \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
            stub.display()
        )
    );
}

/// A game server of another wire than the client's is not used, and the menu says which it
/// looked at; the real one, named, is found.
#[cfg(unix)]
#[test]
fn a_server_of_another_wire_is_refused_and_the_matching_one_found() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("server_wire");
    let stale = dir.0.join("bota_server");
    std::fs::write(&stale, "#!/bin/sh\necho 0000000000000001\n").expect("a script");
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let error = super::launch::server_binary(Some(&stale), false).expect_err("another wire");
    let said = error.to_string();
    assert!(
        said.contains("no bota_server of this client's wire")
            && said.contains(&stale.display().to_string())
            && said.contains("0000000000000001"),
        "{said}"
    );
    assert_eq!(
        super::launch::server_binary(Some(server_binary()), false).expect("the matching server"),
        server_binary()
    );
}

/// A client that ignores its process environment (the in-process test client) must not start the
/// server `$BOTA_SERVER` or the `PATH` names, while the client of `main` does.
#[test]
fn the_server_is_looked_for_in_the_environment_only_by_a_client_that_reads_it() {
    const MARK: &str = "DBOTA_SERVER_ENVIRONMENT_CHILD";
    if let Some(server) = std::env::var_os(MARK) {
        let error = super::launch::server_binary(None, false)
            .expect_err("BOTA_SERVER and the PATH are not looked in");
        assert!(
            error
                .to_string()
                .starts_with("no bota_server of this client's wire"),
            "{error}"
        );
        assert_eq!(
            super::launch::server_binary(None, true).expect("the environment names a server"),
            Path::new(&server)
        );
        return;
    }
    let server = server_binary();
    let paths = std::env::join_paths(
        std::iter::once(server.parent().expect("a folder").to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ),
    )
    .expect("a PATH");
    let output = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "main_menu::tests::the_server_is_looked_for_in_the_environment_only_by_a_client_that_reads_it",
            "--nocapture",
        ])
        .env(MARK, server)
        .env("BOTA_SERVER", server)
        .env("PATH", paths)
        .output()
        .expect("the child test starts");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

/// The lobby view of a player sitting in one.
fn lobby_of(player: &Player) -> super::view::LobbyView {
    let PageView::Lobby(lobby) = player.menu.view().body else {
        panic!("the lobby page: {:?}", player.menu.page());
    };
    lobby
}

/// Where `name` sits in the lobby: its slot and team, or none for one looking on.
fn seat_of(lobby: &super::view::LobbyView, name: &str) -> Option<(u8, String)> {
    lobby
        .teams
        .iter()
        .flat_map(|team| team.slots.iter().map(move |slot| (&team.name, slot)))
        .find(|(_, slot)| slot.who.as_ref().is_some_and(|who| who.name == name))
        .map(|(team, slot)| (slot.slot, team.clone()))
}

/// Alice makes a password lobby of the Squad mode; bob finds it in the list and sits in it
/// through the password dialog, a wrong password first: it is refused in the dialog, which stays
/// for the right one.
fn squad_lobby() -> (Service, Player, Player) {
    let service = service();
    let (mut alice, mut bob) = (Player::new(Some(&service)), Player::new(Some(&service)));
    alice.register("alice");
    bob.register("bob");
    create_lobby(&mut alice, "Squad", &["slots", PASSWORD]);
    bob.menu.act(Act::Go(Page::Lobbies));
    settle(&mut [&mut bob], |p| !p.menu.lobbies.is_empty());
    let listed = bob.menu.lobbies[0].clone();
    assert!(listed.locked && listed.name == "slots", "{listed:?}");
    bob.menu.act(Act::JoinLobby(listed.id));
    let asked = bob.menu.view().password.expect("the password dialog");
    assert_eq!((asked.lobby.as_str(), asked.error), ("slots", None));
    bob.menu.act(Act::Type("wrong password".to_owned()));
    bob.menu.act(Act::LobbyPassword(true));
    settle(&mut [&mut bob], |p| {
        p.menu.view().password.is_some_and(|d| d.error.is_some())
    });
    let refused = bob.menu.view().password.expect("the dialog stays");
    assert_eq!(refused.error.as_deref(), Some("Wrong password."));
    assert!(refused.field.shown.is_empty(), "the wrong password is gone");
    bob.menu.act(Act::Type(PASSWORD.to_owned()));
    bob.menu.act(Act::Submit);
    // Both are on the lobby page, but each page can be showing a grid from before the other sat
    // down: the lobby is whole once both menus have both members in their slots.
    settle(&mut [&mut alice, &mut bob], |p| {
        p.menu.page() == Page::Lobby && {
            let grid = lobby_of(p);
            seat_of(&grid, "alice").is_some() && seat_of(&grid, "bob").is_some()
        }
    });
    assert!(bob.menu.view().password.is_none(), "the dialog closed");
    // The password is the owner's to give: neither menu shows it, the member's least of all.
    for player in [&alice, &bob] {
        let shown = format!("{:?}", player.menu.view());
        assert!(
            !shown.contains(PASSWORD),
            "the lobby's password shown: {shown}"
        );
    }
    (service, alice, bob)
}

/// The slot grid, ten slots five a side under any ruleset: a member sits in a free slot at once,
/// the owner moves another member and removes one, a member may look on instead.
#[test]
fn members_sit_in_slots_the_owner_moves_and_kicks_and_anyone_may_look_on() {
    let (_service, mut alice, mut bob) = squad_lobby();
    let grid = lobby_of(&alice);
    assert_eq!(
        grid.teams
            .iter()
            .map(|t| (t.name.as_str(), t.slots.len()))
            .collect::<Vec<_>>(),
        [("RADIANT", 5), ("DIRE", 5)]
    );
    // The two meet across the sides.
    assert_eq!(seat_of(&grid, "alice"), Some((0, "RADIANT".to_owned())));
    assert_eq!(seat_of(&grid, "bob"), Some((5, "DIRE".to_owned())));

    // A free slot of the other side, at once; both menus see it.
    bob.menu.act(Act::LobbySlot(8));
    settle(&mut [&mut alice, &mut bob], |p| {
        seat_of(&lobby_of(p), "bob") == Some((8, "DIRE".to_owned()))
    });
    // The owner picks bob to move and clicks another slot.
    alice.menu.act(Act::MoveMember(
        lobby_of(&alice).teams[1].slots[3]
            .who
            .as_ref()
            .expect("bob")
            .account,
    ));
    assert!(lobby_of(&alice).moving.is_some());
    alice.menu.act(Act::LobbySlot(2));
    settle(&mut [&mut alice, &mut bob], |p| {
        seat_of(&lobby_of(p), "bob") == Some((2, "RADIANT".to_owned()))
    });
    assert!(lobby_of(&alice).moving.is_none(), "the move was done");
    // A member who looks on holds no slot.
    bob.menu
        .act(Act::LobbySlot(bota_radovid::proto::LOBBY_NO_SLOT));
    settle(&mut [&mut alice, &mut bob], |p| {
        lobby_of(p).spectators.iter().any(|m| m.name == "bob")
    });
    assert_eq!(seat_of(&lobby_of(&alice), "bob"), None);
    // The owner removes him.
    let bobs = lobby_of(&alice).spectators[0].account;
    alice.menu.act(Act::KickMember(bobs));
    settle(&mut [&mut alice, &mut bob], |p| {
        p.menu.page() != Page::Lobby || lobby_of(p).spectators.is_empty()
    });
    settle(&mut [&mut bob], |p| p.menu.page() != Page::Lobby);
}

/// The settings are the owner's: the game mode and the password change for everyone, the others
/// read them and are refused when they try.
#[test]
fn the_owner_changes_the_lobbys_settings_and_the_others_only_read_them() {
    let (_service, mut alice, mut bob) = squad_lobby();
    let settings = lobby_of(&alice).settings;
    assert!(settings.editable && settings.locked && settings.mode == "Squad");
    assert!(!lobby_of(&bob).settings.editable, "read-only for a member");
    assert!(lobby_of(&bob).settings.modes.is_empty());

    // The owner changes the mode and takes the password off; bob sees both.
    let practice = settings
        .modes
        .iter()
        .find(|m| m.name == "Practice")
        .expect("offered")
        .id
        .clone();
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Edit));
    alice
        .menu
        .act(Act::LobbyUi(super::LobbyAction::EditMode(practice)));
    alice.fill_field(1, PASSWORD);
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [&mut alice, &mut bob], |p| {
        if let Some(editor) = p.menu.view().lobby_ui.editor {
            assert!(editor.error.is_none(), "mode refused: {:?}", editor.error);
        }
        lobby_of(p).settings.mode == "Practice"
            && (!lobby_of(p).settings.editable || p.menu.view().lobby_ui.editor.is_none())
    });
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Edit));
    alice
        .menu
        .act(Act::LobbyUi(super::LobbyAction::RemovePassword));
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [&mut alice, &mut bob], |p| {
        if let Some(editor) = p.menu.view().lobby_ui.editor {
            assert!(
                editor.error.is_none(),
                "password removal refused: {:?}",
                editor.error
            );
        }
        !lobby_of(p).settings.locked
            && (!lobby_of(p).settings.editable || p.menu.view().lobby_ui.editor.is_none())
    });
    // The rules changed and the slots did not: bob keeps his.
    assert_eq!(
        seat_of(&lobby_of(&bob), "bob"),
        Some((5, "DIRE".to_owned()))
    );
    refused_lobby_edits(&mut alice, &mut bob);
}

fn refused_lobby_edits(alice: &mut Player, bob: &mut Player) {
    // The audience the owner sits in cannot shut him out; a member cannot change anything.
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Edit));
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Audience(
        bota_radovid::proto::Audience::BotsOnly,
    )));
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [&mut *alice], |p| {
        p.menu
            .view()
            .lobby_ui
            .editor
            .is_some_and(|editor| editor.error.is_some())
    });
    bob.menu.act(Act::LobbyUi(super::LobbyAction::Edit));
    assert!(
        bob.menu.view().lobby_ui.editor.is_none(),
        "a guest cannot edit"
    );
    let state = bob.menu.lobby.as_ref().expect("lobby").0.clone();
    bob.menu.ask(
        bota_radovid::proto::ClientMsg::LobbyConfigure {
            lobby: state.id,
            ruleset: state.ruleset,
            audience: bota_radovid::proto::Audience::HumansOnly,
            password: None,
        },
        super::core::Asked::Lobby,
    );
    settle(&mut [&mut *bob], |p| {
        p.menu.view().status.is_some_and(|s| s.error)
    });
    assert_eq!(lobby_of(bob).settings.audience, "Humans and bots");
}

/// The password dialog keeps nothing it was given once it is gone: Cancel and Escape close it
/// with the typed password and the lobby it was for, the page's search stays as typed, the next
/// dialog starts empty, and logging out closes it too, so the next login joins nothing.
#[test]
fn cancelling_or_logging_out_of_a_lobby_password_dialog_clears_its_secret_and_target() {
    let service = service();
    let (mut alice, mut bob) = (Player::new(Some(&service)), Player::new(Some(&service)));
    alice.register("alice");
    bob.register("bob");
    alice.menu.act(Act::Go(Page::Lobbies));
    alice.fill(&["protected", PASSWORD]);
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Lobby);
    bob.menu.act(Act::Go(Page::Lobbies));
    settle(&mut [&mut bob], |p| !p.menu.lobbies.is_empty());
    let lobby = bob.menu.lobbies[0].id;
    bob.menu.act(Act::Focus(0));
    bob.menu.act(Act::Type("protect".to_owned()));
    for close in [Act::LobbyPassword(false), Act::Back] {
        bob.menu.act(Act::JoinLobby(lobby));
        bob.menu.act(Act::Type("discard me".to_owned()));
        bob.menu.act(close);
        assert!(bob.menu.join_prompt.is_none());
        assert_eq!(bob.menu.page(), Page::Lobbies, "the dialog's close only");
        assert_eq!(bob.menu.form.value(0), "protect", "the search stays");
        bob.menu.act(Act::JoinLobby(lobby));
        let fresh = bob.menu.view().password.expect("the dialog again");
        assert!(fresh.field.shown.is_empty(), "a fresh password dialog");
        bob.menu.act(Act::LobbyPassword(false));
    }
    bob.menu.act(Act::JoinLobby(lobby));
    bob.menu.act(Act::Type("discard on logout".to_owned()));
    bob.menu.act(Act::Logout);
    settle(&mut [&mut bob], |p| {
        p.menu.page() == Page::Login && !p.menu.link_open()
    });
    assert!(bob.menu.join_prompt.is_none());
    assert_eq!(bob.menu.form.kind, super::form::FormKind::Login);
    type_login(&mut bob, "bob", PASSWORD);
    bob.menu.act(Act::Submit);
    settle(&mut [&mut bob], |p| p.menu.page() == Page::Home);
    assert!(
        bob.menu.lobby.is_none(),
        "the login was not taken over by a stale join"
    );
}

/// A join refused because the list was out of date (here the lobby closed since) says why and
/// asks for the list again, so the gone lobby leaves it.
#[test]
fn a_refused_join_says_why_and_lists_the_lobbies_again() {
    let service = service();
    let (mut alice, mut bob) = (Player::new(Some(&service)), Player::new(Some(&service)));
    alice.register("alice");
    bob.register("bob");
    alice.menu.act(Act::Go(Page::Lobbies));
    alice.fill(&["short lived"]);
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Lobby);
    bob.menu.act(Act::Go(Page::Lobbies));
    settle(&mut [&mut bob], |p| !p.menu.lobbies.is_empty());
    let lobby = bob.menu.lobbies[0].id;
    alice.menu.act(Act::LeaveLobby);
    settle(&mut [&mut alice], |p| p.menu.page() != Page::Lobby);
    bob.menu.act(Act::JoinLobby(lobby));
    settle(&mut [&mut bob], |p| {
        p.menu.view().status.is_some_and(|s| s.error) && p.menu.lobbies.is_empty()
    });
    assert_eq!(
        bob.menu.view().status.map(|s| s.text).as_deref(),
        Some("That lobby is gone")
    );
}

/// LEAVE beside RECONNECT, asked and answered: the seat is not offered again, and its game is
/// told. After a game this client does not host, the game unreachable, it is forgotten. After the
/// local lobby's own game, left in the picks, the lobby's real server is told over loopback (its
/// seat taken back with its key, the surrender made sure): the server cancels the match and ends
/// by itself; the client stops nothing.
#[test]
fn leave_after_a_local_lobby_tells_its_server_which_ends_by_itself() {
    use super::leave::{Outcome, tell};
    use std::sync::atomic::AtomicBool;
    let dir = Scratch::new("leave-host");
    let (mut menu, trust) = offline("host", &dir);
    let leave = |menu: &mut MenuCore| {
        assert!(menu.view().reconnect, "RECONNECT offered");
        menu.act(Act::LeaveSeat);
        assert!(menu.view().confirm.is_some(), "LEAVE asks first");
        menu.act(Act::Answer(true));
        menu.take_leave_order().expect("LEAVE tells the game")
    };

    menu.act(Act::Go(Page::LocalLobby));
    menu.act(Act::Focus(1));
    menu.act(Act::Type("127.0.0.1:1".to_owned()));
    menu.act(Act::Direct(false));
    menu.take_launch().expect("a direct game");
    menu.game_started();
    menu.left_game(None, false);
    let (id, _, _) = leave(&mut menu);
    assert!(menu.take_launch().is_none(), "no game is opened for it");
    menu.leave_done(id, Outcome::Unreachable("refused".to_owned()));
    assert!(!menu.view().reconnect, "an unreachable game is forgotten");
    assert_eq!(menu.page(), Page::LocalLobby, "the menu stays where it was");

    let launch = host_launch(&mut menu, None);
    let Launch::Host { port, .. } = &launch else {
        panic!("a local lobby: {launch:?}");
    };
    let port = *port;
    let mut opening = Opening::start(launch, &trust);
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    let opened = loop {
        if let Some(opened) = opening.poll() {
            break opened.expect("the lobby opens");
        }
        assert!(Instant::now() < deadline, "the lobby did not open");
        std::thread::sleep(Duration::from_millis(5));
    };
    let _hosted: Option<Hosted> = opened.server;
    let listening = || std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
    assert!(listening(), "the lobby's server runs");
    menu.game_started();
    menu.set_game_in_picks(true);
    // The seat is left as a DISCONNECT leaves it.
    drop(opened.link);
    menu.left_game(None, false);
    let (id, left, until) = leave(&mut menu);
    assert!(
        matches!(&left, Launch::Direct { hosted: true, .. }),
        "the lobby's own seat: {left:?}"
    );
    let args = <crate::Args as clap::Parser>::parse_from(["dbota"]);
    let (connect, hello) =
        super::launch::leave_connector(&left, &trust, &args).expect("a connector");
    let outcome = tell(connect, &hello, until, &AtomicBool::new(false));
    assert_eq!(outcome, Outcome::Left, "the lobby's server was told");
    menu.leave_done(id, outcome);
    assert!(!menu.view().reconnect, "nothing to take again");
    assert_eq!(
        menu.view().status.map(|s| s.text).as_deref(),
        Some("You left the game: the match is cancelled")
    );
    // The server, still held by this client, cancelled the match and ended by itself.
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while listening() {
        assert!(Instant::now() < deadline, "the lobby's server played on");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// What LEAVE says after the local lobby's own game: mid-match its server runs on while anyone
/// plays (this client cannot know who does, and stops nothing); in the picks the match is
/// cancelled, whoever else sat in it.
#[test]
fn leave_after_a_local_lobby_says_what_becomes_of_its_match() {
    use super::leave::Outcome;
    let dir = Scratch::new("leave-host-words");
    let (mut menu, _trust) = offline("host", &dir);
    for (in_picks, said) in [
        (
            false,
            "You left the game: the match goes on for whoever plays",
        ),
        (true, "You left the game: the match is cancelled"),
    ] {
        host_launch(&mut menu, None);
        menu.game_started();
        menu.set_game_in_picks(in_picks);
        menu.left_game(None, false);
        menu.act(Act::LeaveSeat);
        menu.act(Act::Answer(true));
        let (id, _, _) = menu.take_leave_order().expect("LEAVE tells the game");
        menu.leave_done(id, Outcome::Left);
        assert_eq!(menu.view().status.map(|s| s.text).as_deref(), Some(said));
    }
}

/// A new local lobby replaces the server a leave was telling: that leave is stopped and its
/// outcome never taken, so nothing of the old game reaches the new lobby on the same port.
#[test]
fn a_new_local_lobby_stops_the_leave_of_the_old_one() {
    use super::leave::Outcome;
    let dir = Scratch::new("leave-host-new");
    let (mut menu, _trust) = offline("host", &dir);
    host_launch(&mut menu, None);
    menu.game_started();
    menu.left_game(None, false);
    menu.act(Act::LeaveSeat);
    menu.act(Act::Answer(true));
    let (old, _, _) = menu.take_leave_order().expect("LEAVE tells the game");
    host_launch(&mut menu, None);
    assert!(menu.take_cancel_leave(), "the old lobby's leave is stopped");
    // A leave not yet started when the new lobby comes in the same frame never starts.
    menu.game_started();
    menu.left_game(None, false);
    menu.act(Act::LeaveSeat);
    menu.act(Act::Answer(true));
    host_launch(&mut menu, None);
    assert!(
        menu.take_leave_order().is_none(),
        "the unstarted leave is dropped"
    );
    menu.leave_done(old, Outcome::Left);
    assert_ne!(
        menu.view().status.map(|s| s.text).as_deref(),
        Some("You left the game: the match goes on for whoever plays"),
        "the stopped leave's outcome is not taken"
    );
}

/// While a game is told the match is left, its seat is not taken again: RECONNECT's own way back
/// (here the same address) is refused, saying why; another game opens.
#[test]
fn a_game_being_left_is_not_joined_again_until_it_is_told() {
    let dir = Scratch::new("leave-rejoin");
    let (mut menu, _trust) = offline("leaver", &dir);
    let direct = |menu: &mut MenuCore, address: &str| {
        menu.act(Act::Go(Page::LocalLobby));
        menu.act(Act::Focus(1));
        while !menu.form.value(1).is_empty() {
            menu.act(Act::Erase);
        }
        menu.act(Act::Type(address.to_owned()));
        menu.act(Act::Direct(false));
    };
    direct(&mut menu, "127.0.0.1:1");
    menu.take_launch().expect("a direct game");
    menu.game_started();
    menu.left_game(None, false);
    menu.act(Act::LeaveSeat);
    menu.act(Act::Answer(true));
    menu.take_leave_order().expect("a leave");
    direct(&mut menu, "127.0.0.1:1");
    assert!(
        menu.take_launch().is_none(),
        "the game being left is not joined"
    );
    assert_eq!(
        menu.view().status.map(|s| s.text).as_deref(),
        Some("That game is being told you left it: wait until it is")
    );
    direct(&mut menu, "127.0.0.1:2");
    assert!(menu.take_launch().is_some(), "another game opens");
}

/// A leave is known by its id: its outcome is taken while it is under way, and never once it was
/// given up at its deadline (a connection that completes just then holds nothing of the menu),
/// nor for another leave.
#[test]
fn a_leave_given_up_at_its_deadline_is_never_taken_back() {
    use super::leave::Outcome;
    let dir = Scratch::new("leave-deadline");
    let (mut menu, _trust) = offline("leaver", &dir);
    let leave_a_game = |menu: &mut MenuCore| {
        menu.act(Act::Go(Page::LocalLobby));
        menu.act(Act::Focus(1));
        while !menu.form.value(1).is_empty() {
            menu.act(Act::Erase);
        }
        menu.act(Act::Type("127.0.0.1:1".to_owned()));
        menu.act(Act::Direct(false));
        menu.take_launch().expect("a direct game");
        menu.game_started();
        menu.left_game(None, false);
        menu.act(Act::LeaveSeat);
        menu.act(Act::Answer(true));
        let (id, _, _) = menu.take_leave_order().expect("a leave");
        id
    };
    let status = |menu: &MenuCore| menu.view().status.map(|s| s.text);

    let late = leave_a_game(&mut menu);
    menu.leave_times_out();
    menu.update();
    let given_up = Some("The game could not be told you left it: it is forgotten here".to_owned());
    assert_eq!(status(&menu), given_up);
    menu.leave_done(late, Outcome::Left);
    assert_eq!(
        status(&menu),
        given_up,
        "a leave given up is not taken back"
    );

    let next = leave_a_game(&mut menu);
    assert_ne!(next, late);
    menu.leave_done(late, Outcome::Left);
    assert_eq!(
        status(&menu).as_deref(),
        Some("Leaving the game…"),
        "nor taken for another leave"
    );
    menu.leave_done(next, Outcome::Left);
    assert_eq!(status(&menu).as_deref(), Some("You left the game"));
    assert!(!menu.view().reconnect);
}

/// While the server of the lobby this client hosts runs, the client cannot know who plays in it:
/// quitting and hosting another lobby both say they end the game it hosts for everyone, and a
/// new lobby is hosted only when that is answered yes.
#[test]
fn quitting_or_hosting_again_says_it_ends_the_hosted_game() {
    let dir = Scratch::new("rehost");
    let (mut menu, _trust) = offline("host", &dir);
    host_launch(&mut menu, None);
    menu.game_started();
    menu.left_game(None, false);
    menu.set_hosting(true);
    menu.act(Act::Quit);
    let asked = menu.view().confirm.expect("the question");
    assert!(
        asked.text.contains("ends the game you host for everyone"),
        "{asked:?}"
    );
    menu.act(Act::Answer(false));
    menu.act(Act::Go(Page::LocalLobby));
    menu.act(Act::Host(false));
    assert!(
        menu.take_launch().is_none(),
        "nothing is hosted before the answer"
    );
    let asked = menu.view().confirm.expect("the question");
    assert!(
        asked
            .text
            .contains("ends the game you host now for everyone"),
        "{asked:?}"
    );
    menu.act(Act::Answer(true));
    assert!(
        matches!(menu.take_launch(), Some(Launch::Host { .. })),
        "the new lobby, once answered"
    );
}

/// Signed in with nothing in flight, the service goes away: the menu says it was disconnected and
/// the pill says the link is lost (RECONNECTING, a click retries), not the service's name in green
/// as if still connected (it stayed green: no retry was due, as nothing live was on the link).
#[test]
fn a_lost_idle_link_shows_in_the_pill_not_as_online() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    settle(&mut [&mut alice], |p| p.menu.idle());
    assert!(matches!(
        alice.menu.view().connection,
        super::view::Connection::Online(_)
    ));
    drop(service);
    settle(&mut [&mut alice], |p| {
        p.menu
            .view()
            .status
            .is_some_and(|status| status.text.starts_with("Disconnected from the service"))
    });
    let view = alice.menu.view();
    assert!(
        matches!(view.connection, super::view::Connection::Lost(_)),
        "{:?} while {:?}",
        view.connection,
        view.status
    );
}

#[test]
fn an_unknown_proposal_refusal_explains_that_the_offer_is_no_longer_available() {
    assert_eq!(
        super::core::error_text(&bota_radovid::proto::Error::UnknownProposal),
        "This match offer is no longer available"
    );
}

#[test]
fn menu_hosted_loading_draws_the_elapsed_launch_clock() {
    let dir = Scratch::new("loading-clock");
    let (mut menu, _) = offline("alice", &dir);
    let now = bevy::platform::time::Instant::now();
    menu.page = Page::Game;
    menu.launching = true;
    menu.opening_since = Some(now - Duration::from_secs(3));
    let mut view = menu.view();
    view.body = menu.body_at(now);
    let mut client = crate::e2e::harness::Client::run(&["--menu-preview", "home"]);
    client.world_mut().resource_mut::<super::Preview>().0 = view;
    client.frames(4);
    assert!(client.shown_texts().iter().any(|text| text == "0:03"));
}

pub(crate) fn lose_menu_control(menu: &mut MenuCore) {
    menu.lost(bota_radovid::ClientError::Closed);
}

pub(crate) fn keep_menu_session_alive(menu: &MenuCore, service: &Service) -> bota_radovid::Client {
    use bota_radovid::proto::{ClientMsg, Reply};
    let token = menu
        .client
        .as_ref()
        .expect("menu client")
        .session()
        .expect("session");
    let trust = bota_radovid::Tls::trust_pem(&service.pem).expect("trust");
    let mut client =
        bota_radovid::Client::open(&service.url, &trust).expect("open another control link");
    let until = Instant::now() + Duration::from_secs(PATIENCE);
    loop {
        assert!(
            Instant::now() < until,
            "the second control session did not sign in"
        );
        match client.poll().expect("control reply") {
            Some(bota_radovid::Incoming::Reply {
                result: Ok(Reply::Welcome { .. }),
                ..
            }) => {
                client
                    .request(ClientMsg::Resume {
                        session: token,
                        client: Default::default(),
                    })
                    .expect("resume sent");
            }
            Some(bota_radovid::Incoming::Reply {
                result: Ok(Reply::LoggedIn { .. }),
                ..
            }) => return client,
            _ => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}

pub(crate) fn menu_control_ready(menu: &MenuCore) -> bool {
    menu.link_ready()
}

#[test]
fn queue_shortcut_and_drawer_send_the_same_selected_audiences() {
    use bota_radovid::proto::Audience;
    let service = service();
    for (index, (drawer, bots)) in [(false, false), (false, true), (true, false), (true, true)]
        .into_iter()
        .enumerate()
    {
        let mut player = Player::new(Some(&service));
        player.register(&format!("audience_{index}"));
        if bots {
            player.menu.act(Act::Play(super::PlayAction::Bots));
        }
        player.menu.act(if drawer {
            Act::Play(super::PlayAction::Find)
        } else {
            Act::Find(solo())
        });
        settle(&mut [&mut player], |p| !p.menu.view().searches.is_empty());
        let audiences: Vec<_> = player
            .menu
            .client
            .as_ref()
            .expect("connected")
            .searches()
            .entries()
            .iter()
            .map(|entry| entry.audience)
            .collect();
        let expected = if bots {
            vec![Audience::HumansOnly, Audience::Everyone]
        } else {
            vec![Audience::HumansOnly]
        };
        assert_eq!(audiences, expected, "drawer={drawer}, bots={bots}");
        let entry = player.menu.view().searches[0].entry;
        player.menu.act(Act::Cancel(entry));
        settle(&mut [&mut player], |p| p.menu.view().searches.is_empty());
    }
}

pub(crate) fn install_local_library(core: &mut MenuCore, directory: &Path) {
    core.library = Library::new(Box::new(crate::replays::Disk::new(directory.to_owned())));
    core.store = SettingsStore::new(Some(directory.join("settings.conf")));
    core.local_history =
        super::local_matches::Archive::load(&core.store, core.library.policy().max_count);
}

pub(crate) fn library_titles_ready(core: &MenuCore) -> bool {
    !core.library.records().is_empty()
        && core.library.records().iter().all(|record| {
            !matches!(
                core.summary_of(record),
                super::replay_info::Summary::Reading
            )
        })
}

pub(crate) fn local_library_scan_complete(core: &MenuCore) -> bool {
    core.library.standing() == crate::replays::Standing::Ready
        && core
            .local_meta
            .initial_scan_complete(core.library.records())
}

/// Text owned by the menu page, excluding the game HUD that may remain behind an opaque menu.
pub(crate) fn page_words(world: &mut bevy::prelude::World) -> Vec<String> {
    use bevy::prelude::*;
    let texts: Vec<_> = world
        .query::<(Entity, &Text, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, _, visible)| visible.get())
        .map(|(entity, text, _)| (entity, text.0.clone()))
        .collect();
    texts
        .into_iter()
        .filter_map(|(mut entity, text)| {
            for _ in 0..32 {
                if world.get::<super::screens::PageRoot>(entity).is_some() {
                    return Some(text);
                }
                entity = world.get::<ChildOf>(entity)?.parent();
            }
            None
        })
        .collect()
}

/// The asset paths of the pictures at least `least` pixels wide that the menu's page shows.
pub(crate) fn page_pictures(world: &mut bevy::prelude::World, least: f32) -> Vec<String> {
    use bevy::prelude::*;
    let pictures: Vec<_> = world
        .query::<(Entity, &ImageNode, &InheritedVisibility, &ComputedNode)>()
        .iter(world)
        .filter(|(_, _, visible, laid)| visible.get() && laid.size().x >= least)
        .filter_map(|(entity, image, ..)| Some((entity, image.image.path()?.to_string())))
        .collect();
    pictures
        .into_iter()
        .filter_map(|(mut entity, path)| {
            for _ in 0..32 {
                if world.get::<super::screens::PageRoot>(entity).is_some() {
                    return Some(path);
                }
                entity = world.get::<ChildOf>(entity)?.parent();
            }
            None
        })
        .collect()
}

#[test]
fn demo_from_the_catalogue_can_launch_a_local_sandbox_without_an_account() {
    let mut player = Player::new(None);
    player.menu.act(Act::Demo(HeroId::SHADOW_FIEND.0));
    assert!(
        player.menu.take_launch().is_some(),
        "DEMO must not require a service account"
    );
}

pub(crate) fn avatar_crop_ready(core: &MenuCore) -> bool {
    !core.avatar.view.busy && core.avatar.view.preview.is_some()
}

pub(crate) fn use_matching_local_server(core: &mut MenuCore) {
    core.server_binary = Some(server_binary().to_owned());
}

pub(crate) fn lost_result() -> super::history::ResultKind {
    super::history::ResultKind::Lost
}

pub(crate) fn service_with_two_rated_modes() -> Service {
    start_service(true, |config| {
        config.replacen("rated = false", "rated = true", 1)
    })
}

pub(crate) fn profile_with_only_one_mode(core: &mut MenuCore) {
    use bota_radovid::proto::{AccountId, AccountKind, PublicAccount, Rating, RatingSummary};
    core.account_profile = Some(PublicAccount {
        id: AccountId(1),
        nickname: "rated_player".to_owned(),
        kind: AccountKind::Human,
        online: false,
        bot_version: None,
        avatar: None,
        ratings: vec![(
            RulesetId("solo_mid".to_owned()),
            RatingSummary {
                rating: Rating {
                    rating: 1524.0,
                    deviation: 100.0,
                },
                games: 41,
                wins: 20,
                draws: 0,
                losses: 21,
                rank: Some(12),
            },
        )],
    });
}

#[test]
fn a_local_game_keeps_its_metadata_with_replay_saving_off() {
    local_history_without_replay(false);
}

#[test]
fn a_local_game_keeps_metadata_when_saving_is_turned_off_during_play() {
    local_history_without_replay(true);
}

fn local_history_without_replay(during_play: bool) {
    let dir = Scratch::new("local-history-without-replay");
    let (mut menu, trust) = offline("local_owner", &dir);
    if !during_play {
        menu.set_replay_policy(crate::replays::Policy {
            keep: false,
            ..Default::default()
        });
    }
    menu.act(Act::Go(Page::LocalLobby));
    for (at, value) in [(0, free_port().to_string()), (1, "1".to_owned())] {
        menu.act(Act::Focus(at));
        while !menu.form.value(at).is_empty() {
            menu.act(Act::Erase);
        }
        menu.act(Act::Type(value));
    }
    menu.act(Act::Host(false));
    let (mut game, mut server) = open_hosted(menu.take_launch().unwrap(), &trust);
    menu.game_started();
    pump_until("seated", &mut [&mut game], |core| core.my_slot.is_some());
    menu.set_local_recording_owner(game.my_slot.unwrap());
    game.act(bota_client_core::Intent::PickHero(HeroId::SHADOW_FIEND.0));
    game.act(bota_client_core::Intent::ToggleReady);
    pump_until("the match runs", &mut [&mut game], |core| {
        core.view().is_some_and(|view| view.tick > 5)
    });
    if during_play {
        menu.set_replay_policy(crate::replays::Policy {
            keep: false,
            ..Default::default()
        });
    }
    game.leave_game();
    let deadline = Instant::now() + Duration::from_secs(15);
    while server.as_mut().is_some_and(Hosted::running) {
        assert!(
            Instant::now() < deadline,
            "the hosted server finishes its files and exits"
        );
        game.pump(1.0 / 30.0);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(server);
    menu.left_game(None, true);
    let deadline = Instant::now() + Duration::from_secs(15);
    while menu.last_played_hero().is_none() && Instant::now() < deadline {
        menu.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(menu.last_played_hero(), Some(HeroId::SHADOW_FIEND));
    assert!(
        menu.library.records().is_empty(),
        "metadata retention is independent of replay files"
    );
}

pub(crate) fn persist_browser_history_with_a_failed_blob(directory: &Path) {
    let (_, metadata) = crate::postgame::recorded_match_for_menu();
    let store = SettingsStore::new(Some(directory.join("settings.conf")));
    let mut archive = super::local_matches::Archive::default();
    archive.insert(
        super::local_matches::Saved {
            key: "quota-history".to_owned(),
            file: None,
            when: 100,
            mode: "local".to_owned(),
            owner: Some(bota_proto::SlotId(0)),
            metadata: std::fs::read(metadata).unwrap(),
            metadata_key: None,
            cached: None,
            result: None,
            summary: None,
            temporary: None,
        },
        25,
    );
    super::archive_storage::write_snapshot(
        archive.entries(),
        true,
        |_, _| Err(std::io::Error::other("browser metadata quota exceeded")),
        |text| store.write(Entry::MatchHistory, text),
    )
    .expect("the small index still fits");
}

#[test]
fn local_hero_demo_preserves_the_clients_server_lookup_policy() {
    let dir = Scratch::new("demo-server-policy");
    for allowed in [false, true] {
        let (menu, _) = offline("alice", &dir);
        let mut menu = menu.with_server_from_environment(allowed);
        menu.act(Act::Demo(HeroId::PUDGE.0));
        let Some(Launch::LocalDemo {
            hero,
            server_environment,
            ..
        }) = menu.take_launch()
        else {
            panic!("the hero action must open a local sandbox without a service");
        };
        assert_eq!(hero, HeroId::PUDGE);
        assert_eq!(
            server_environment, allowed,
            "a local demo uses the client's lookup policy"
        );
    }
}

fn refuse_taken_lobby(player: &mut Player) {
    player.menu.act(Act::Go(Page::Lobbies));
    player.fill(&["Taken room"]);
    player.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    settle(&mut [player], |p| {
        p.menu
            .view()
            .lobby_ui
            .editor
            .is_some_and(|e| e.error.as_deref() == Some("A lobby of that name is open already"))
    });
}

fn running_practice_player(service: &Service) -> (Player, bota_client_core::App) {
    let mut alice = Player::new(Some(service));
    alice.register("alice");
    create_lobby(&mut alice, "Practice", &["Running room"]);
    alice.menu.act(Act::ReadyLobby(true));
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Game);
    let mut game = open(launch_of(&mut alice), &alice.trust);
    let until = Instant::now() + Duration::from_secs(PATIENCE);
    while game.view().is_none_or(|v| v.tick <= 5) {
        assert!(Instant::now() < until, "practice began");
        play_step(&mut game, HeroId::SHADOW_FIEND);
    }
    alice.menu.game_started();
    assert!(alice.menu.in_game());
    alice.menu.act(Act::Dashboard);
    (alice, game)
}

#[test]
fn a_refused_create_does_not_replace_settings_with_a_creation_warning_on_link_loss() {
    let service = service();
    let (mut alice, mut bob) = (Player::new(Some(&service)), Player::new(Some(&service)));
    alice.register("alice");
    bob.register("bob");
    create_lobby(&mut bob, "Full rules", &["Taken room"]);
    refuse_taken_lobby(&mut alice);
    alice.menu.act(Act::Go(Page::Settings));
    alice.menu.test_exhaust_control_retry();
    lose_menu_control(&mut alice.menu);
    assert_eq!(alice.menu.page(), Page::Settings);
    let status = alice.menu.view().status.expect("disconnect explained");
    assert_eq!(
        status.text,
        "Disconnected from the service: the connection ended"
    );
}

#[test]
fn a_refused_create_does_not_pull_a_running_game_into_the_lobby_browser_on_link_loss() {
    let service = service();
    let mut bob = Player::new(Some(&service));
    bob.register("bob");
    create_lobby(&mut bob, "Full rules", &["Taken room"]);
    let (mut alice, game) = running_practice_player(&service);
    refuse_taken_lobby(&mut alice);
    alice.menu.act(Act::ReturnToGame);
    alice.menu.test_exhaust_control_retry();
    lose_menu_control(&mut alice.menu);
    assert_eq!(alice.menu.page(), Page::Game);
    assert!(alice.menu.in_game());
    assert!(game.view().is_some());
    assert_eq!(
        alice.menu.view().status.expect("disconnect explained").text,
        "Disconnected from the service: the connection ended"
    );
}

#[test]
fn an_unanswered_create_warns_without_covering_the_running_game_when_its_link_drops() {
    let service = service();
    let (mut alice, game) = running_practice_player(&service);
    alice.menu.act(Act::Go(Page::Lobbies));
    alice.fill(&["Unanswered room"]);
    alice.menu.act(Act::LobbyUi(super::LobbyAction::Save));
    alice.menu.act(Act::ReturnToGame);
    lose_menu_control(&mut alice.menu);
    assert_eq!(alice.menu.page(), Page::Game);
    assert!(game.view().is_some());
    assert_eq!(
        alice
            .menu
            .view()
            .status
            .expect("creation uncertainty explained")
            .text,
        "Lobby creation was not confirmed. It may still exist; refresh Lobbies before trying again."
    );
}

pub(crate) fn registered_public_profile(service: &Service) -> (u64, String) {
    let mut player = Player::new(Some(service));
    player.register("profile_target");
    let me = player
        .menu
        .client
        .as_ref()
        .expect("client")
        .me()
        .expect("account");
    let identity = (me.id.0, format!("{}#{}", me.nickname, me.id.0));
    upload_profile_avatar(&mut player.menu);
    identity
}

fn upload_profile_avatar(menu: &mut MenuCore) {
    // A solid PNG is protocol input for a user-uploaded avatar.
    let png = vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 32, 0, 0, 0, 32, 8,
        6, 0, 0, 0, 115, 122, 122, 244, 0, 0, 0, 47, 73, 68, 65, 84, 120, 218, 237, 206, 33, 1, 0,
        0, 8, 3, 48, 130, 145, 142, 136, 20, 130, 24, 55, 19, 243, 171, 158, 189, 164, 18, 16, 16,
        16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 72, 7, 30, 115, 205, 60, 151, 190, 23, 53,
        16, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let client = menu.client.as_mut().expect("client");
    let seq = client
        .request(bota_radovid::proto::ClientMsg::SetAvatar { png })
        .expect("upload request");
    let mut uploaded = false;
    for _ in 0..32 {
        let incoming = client
            .wait(Duration::from_secs(10))
            .expect("upload response")
            .expect("upload deadline");
        if let bota_radovid::Incoming::Reply { seq: found, result } = incoming
            && found == seq
        {
            assert!(
                matches!(
                    result,
                    Ok(bota_radovid::proto::Reply::Avatar {
                        avatar: Some(_),
                        ..
                    })
                ),
                "the fixture's avatar was accepted: {result:?}"
            );
            uploaded = true;
            break;
        }
    }
    assert!(
        uploaded,
        "the avatar upload completed within the bounded reply stream"
    );
}

pub(crate) fn served_leaderboard_profile(service: &Service) -> (u64, String) {
    for name in ["unranked_one", "unranked_two"] {
        let mut player = Player::new(Some(service));
        player.register(name);
    }
    let mut played = served_postmatch(service);
    upload_profile_avatar(&mut played.menu);
    let me = played
        .menu
        .client
        .as_ref()
        .expect("client")
        .me()
        .expect("account");
    (me.id.0, format!("{}#{}", me.nickname, me.id.0))
}

pub(crate) fn own_cached_profile_id(core: &MenuCore) -> Option<u64> {
    core.account_profile.as_ref().map(|account| account.id.0)
}

pub(crate) fn install_profile_history(menu: &mut MenuCore, metadata: Vec<u8>) {
    menu.local_history = super::local_matches::Archive::default();
    menu.local_history.insert(
        super::local_matches::Saved {
            key: "profile-history".into(),
            file: None,
            when: 1794062400,
            mode: "all_pick".into(),
            owner: None,
            metadata,
            metadata_key: None,
            cached: None,
            result: None,
            summary: None,
            temporary: None,
        },
        50,
    );
}

pub(crate) fn public_profile_request_count(menu: &MenuCore) -> u64 {
    menu.profiles.requests
}

pub(crate) fn drawn_profile_avatar(world: &mut bevy::prelude::World) -> Option<[u8; 4]> {
    use bevy::prelude::*;
    let handle = world
        .query_filtered::<&ImageNode, With<super::avatar_draw::AvatarPicture>>()
        .iter(world)
        .next()?
        .image
        .clone();
    let image = world.resource::<Assets<Image>>().get(&handle)?;
    image.data.as_ref()?.get(..4)?.try_into().ok()
}

#[test]
fn changing_matchmaker_clears_the_old_login_before_a_public_profile_can_be_queried() {
    let service = service();
    let mut player = Player::new(Some(&service));
    player.register("scoped_viewer");
    assert!(player.menu.logged_in());
    let trap = std::net::TcpListener::bind("127.0.0.1:0").expect("new service trap");
    trap.set_nonblocking(true).expect("nonblocking");
    let next = format!("ws://{}/control", trap.local_addr().expect("address"));
    player.menu.act(Act::Go(Page::Service));
    player.fill(&[&next]);
    player.menu.act(Act::Submit);
    assert!(
        !player.menu.logged_in(),
        "a login is scoped to the matchmaker that authenticated it"
    );
    assert!(!player.menu.link_ready());
    assert!(
        player.store.read(Entry::Session).is_none(),
        "the old credential is not resumed at another service"
    );
    assert!(
        player
            .menu
            .inspect_player(super::player_profile::Target {
                service: next,
                account: 42
            })
            .is_err()
    );
    assert_eq!(player.menu.profiles.requests, 0);
    assert!(matches!(trap.accept(), Err(error) if error.kind()==std::io::ErrorKind::WouldBlock));
}

#[test]
fn a_profile_connection_loss_keeps_back_navigation_to_the_original_history() {
    let service = service();
    let mut player = Player::new(Some(&service));
    player.register("return_viewer");
    player.menu.act(Act::Go(Page::MatchHistory));
    player
        .menu
        .inspect_player(super::player_profile::Target {
            service: service.url.clone(),
            account: 42,
        })
        .expect("open public profile");
    player.menu.lost(bota_radovid::ClientError::Closed);
    assert_eq!(player.menu.page(), Page::PlayerProfile(42));
    player.menu.act(Act::Back);
    assert_eq!(
        player.menu.page(),
        Page::MatchHistory,
        "a failed lookup does not discard the inspected match's return page"
    );
}

pub(crate) struct ServedPostmatch {
    pub menu: MenuCore,
    pub game: Core,
    pub id: u64,
    pub other: u64,
    pub other_slot: u8,
    pub other_name: String,
    pub metadata: Vec<u8>,
    pub replay: Vec<u8>,
}

/// Real registration, proposal, launcher, game sockets, permanent departure and served outcome.
pub(crate) fn served_postmatch(service: &Service) -> ServedPostmatch {
    served_postmatch_visibility(service, false)
}
pub(crate) fn served_postmatch_visibility(
    service: &Service,
    hidden_opponent: bool,
) -> ServedPostmatch {
    let (mut left, mut right) = (Player::new(Some(service)), Player::new(Some(service)));
    left.register("live_viewer");
    right.register("served_opponent");
    let me = right
        .menu
        .client
        .as_ref()
        .expect("client")
        .me()
        .expect("account");
    let other = me.id.0;
    let other_name = format!("{}#{}", me.nickname, other);
    if hidden_opponent {
        let script = "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute(\"INSERT INTO ratings(account,ruleset,rating,deviation,volatility,games,wins,draws,losses,last_played,kind,hidden) VALUES(?,'solo_mid',1500,350,0.06,1,0,0,1,1,0,1) ON CONFLICT(account,ruleset) DO UPDATE SET hidden=1\",(int(sys.argv[2]),)); c.execute(\"UPDATE ratings SET hidden=1 WHERE account=?\",(int(sys.argv[2]),)); assert c.execute(\"SELECT COUNT(*) FROM ratings WHERE account=? AND hidden=1 AND games>0\",(int(sys.argv[2]),)).fetchone()[0]==1; c.commit()";
        let status = std::process::Command::new("python3")
            .args(["-c", script])
            .arg(service._dir.0.join("radovid.db"))
            .arg(other.to_string())
            .status()
            .expect("fixture visibility");
        assert!(status.success());
        use_declared_public_fixture_identity(&mut left.menu, service);
    }
    left.menu.act(Act::Find(solo()));
    right.menu.act(Act::Find(solo()));
    settle(&mut [&mut left, &mut right], |player| {
        player.menu.view().offer.is_some()
    });
    for player in [&mut left, &mut right] {
        let proposal = player.menu.view().offer.expect("offer").proposal;
        player.menu.act(Act::Accept(proposal));
    }
    settle(&mut [&mut left, &mut right], |player| {
        player.menu.page() == Page::Game
    });
    let launch = launch_of(&mut left);
    let Launch::Seat(handoff) = &launch else {
        panic!("seat")
    };
    let id = handoff.match_id.0;
    let other_slot = handoff
        .seats
        .iter()
        .find(|seat| seat.account.0 == other)
        .expect("opponent slot")
        .slot;
    let mut game = open(launch, &left.trust);
    let mut opponent = open(launch_of(&mut right), &right.trust);
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while game.view().is_none_or(|view| view.tick < 3)
        || opponent.view().is_none_or(|view| view.tick < 3)
    {
        assert!(Instant::now() < deadline, "the real match starts");
        for core in [&mut game, &mut opponent] {
            core.pump(1.0 / 30.0);
            if core.my_slot.is_some() && core.view().is_none() && !core.ready {
                core.act(bota_client_core::Intent::PickHero(HeroId::SHADOW_FIEND.0));
                core.act(bota_client_core::Intent::ToggleReady);
            }
        }
        left.menu.update();
        right.menu.update();
        std::thread::sleep(Duration::from_millis(1));
    }
    opponent.leave_game();
    while game.over.is_none() || left.menu.results.is_empty() || !left.menu.meta_ready() {
        assert!(
            Instant::now() < deadline,
            "the service delivers the actual result and metadata"
        );
        game.pump(1.0 / 30.0);
        opponent.pump(1.0 / 30.0);
        left.menu.update();
        right.menu.update();
        std::thread::sleep(Duration::from_millis(1));
    }
    let artifacts = service._dir.0.join("artifacts");
    ServedPostmatch {
        menu: left.menu,
        game,
        id,
        other,
        other_slot,
        other_name,
        metadata: std::fs::read(artifacts.join(format!("meta-{id}.meta"))).expect("server sidecar"),
        replay: std::fs::read(artifacts.join(format!("replay-{id}.brp"))).expect("server replay"),
    }
}

pub(crate) fn visitor_public_matches(
    menu: &MenuCore,
) -> Option<std::sync::Arc<Vec<super::history::Match>>> {
    match menu.visitor_view() {
        PageView::Profile { matches, .. } => Some(matches),
        _ => None,
    }
}

/// The fixture's public name represents the very same authenticated loopback service.
/// No resolver hook or file-provided address is used to establish the transport.
pub(crate) fn use_declared_public_fixture_identity(menu: &mut MenuCore, service: &Service) {
    assert!(menu.logged_in());
    assert!(bota_radovid::same_matchmaker(
        &menu.prefs.service,
        &service.url
    ));
    menu.prefs.service = service
        .public_identity
        .clone()
        .expect("configured public identity");
}
pub(crate) fn portable_service() -> Service {
    service_with(|config| {
        format!("public_identity = \"wss://match.example/control\"\n{config}")
            .replace(
                "BOTA_SEATS = \"{seats}\"",
                "BOTA_SEATS = \"{seats}\", BOTA_PROFILE_ACCOUNTS = \"{accounts}\"",
            )
            .replace(
                "artifact_args = [\"--replay\", \"{artifact}\"]",
                "artifact_args = [\"--replay\", \"{artifact}\", \"--meta-trailer\"]",
            )
    })
}

pub(crate) fn profile_target(service: &Service, account: u64) -> super::player_profile::Target {
    super::player_profile::Target {
        service: service.url.clone(),
        account,
    }
}

pub(crate) fn rank_in_served_board(menu: &MenuCore, account: u64) -> u64 {
    u64::from(
        menu.board
            .rows
            .iter()
            .find(|row| row.account == account)
            .expect("served row")
            .rank,
    )
}

pub(crate) fn probe_public_deviation(service:&Service,account:u64)->f64 {
    let mut player=Player::new(Some(service));
    player.register("confidence_probe");
    let client=player.menu.client.as_mut().expect("client");
    let sequence=client.request(bota_radovid::proto::ClientMsg::Account{account:bota_radovid::proto::AccountId(account)}).expect("public profile request");
    for _ in 0..32 {
        let incoming=client.wait(Duration::from_secs(10)).expect("real reply").expect("reply deadline");
        if let bota_radovid::Incoming::Reply{seq,result}=incoming &&seq==sequence {
            let bota_radovid::proto::Reply::Account{account}=result.expect("visible profile") else {panic!("profile response")};
            return account.ratings.iter().find(|(mode,_)|mode.0=="solo_mid").expect("played mode").1.rating.deviation;
        }
    }
    panic!("public profile reply missing");
}
pub(crate) fn withhold_confidence_profile(service:&Service,account:u64,banned:bool) {
    let script="import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); q='UPDATE accounts SET banned=1,ban_until=NULL,ban_reason=\"test restriction\" WHERE id=?' if sys.argv[3]=='ban' else 'UPDATE ratings SET hidden=1 WHERE account=?'; cur=c.execute(q,(int(sys.argv[2]),)); assert cur.rowcount>0; c.commit()";
    assert!(std::process::Command::new("python3").args(["-c",script]).arg(service._dir.0.join("radovid.db"))
        .arg(account.to_string()).arg(if banned {"ban"}else{"hide"}).status().expect("private fixture visibility").success());
}
