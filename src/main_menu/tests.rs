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
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "target/menu-tests/{what}-{}-{}",
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
        if let Some(path) = std::env::var_os("BOTA_SERVER_BIN") {
            return PathBuf::from(path);
        }
        let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/bota");
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
    })
}

/// The service, running until dropped.
pub(crate) struct Service {
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
fn service_with(change: impl FnOnce(String) -> String) -> Service {
    start_service(true, change)
}

fn start_service(save_replays: bool, change: impl FnOnce(String) -> String) -> Service {
    let binary = server_binary().to_owned();
    let dir = Scratch::new("service");
    let path = dir.0.join("radovid.toml");
    std::fs::write(&path, change(config(&dir.0, &binary, save_replays))).expect("config");
    let config = radovid::config::Config::load(&path).expect("a valid configuration");
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
struct Player {
    menu: MenuCore,
    trust: Trust,
    store: SettingsStore,
    _dir: Scratch,
}

impl Player {
    fn new(service: Option<&Service>) -> Player {
        let dir = Scratch::new("player");
        let store = SettingsStore::new(Some(dir.0.join("client.conf")));
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
    fn fill(&mut self, values: &[&str]) {
        self.menu.act(Act::Focus(0));
        for (at, value) in values.iter().enumerate() {
            self.menu.act(Act::Focus(at));
            while !self.menu.form.value(at).is_empty() {
                self.menu.act(Act::Erase);
            }
            self.menu.act(Act::Type((*value).to_owned()));
        }
    }

    fn register(&mut self, login: &str) {
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
    assert_eq!(alice.menu.view().searches[0].position, 1);
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
    let headlines: Vec<String> = [&alice, &bob]
        .iter()
        .map(|p| match p.menu.view().body {
            PageView::Result(result) => {
                assert_eq!(result.seats.len(), 2);
                assert!(result.seats.iter().all(|s| !s.rating.is_empty()), "rated");
                assert_eq!(result.seats[0].hero, "Shadow Fiend");
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
        alice.menu.view().searches[0].position,
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
    let mut child = Command::new(server_binary())
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
    player.menu.act(Act::Go(Page::Settings));
    assert_eq!(player.menu.page(), Page::Settings);
    assert_eq!(player.menu.view().body, PageView::Settings);
    assert!(player.menu.view().covers());
    player.menu.leave_settings();
    assert_eq!(player.menu.page(), before, "back where it was opened");

    let mut settings = crate::config::Settings::default();
    let at = crate::keys::index_of(crate::keys::Action::Stop).expect("bound");
    settings.keys[at] = crate::keys::Chord::parse("Ctrl+K");
    settings.volume = 0.25;
    settings.clarity = crate::config::Clarity::Off;
    player.store.save_settings(&settings).expect("saved");
    let restarted = SettingsStore::new(Some(player._dir.0.join("client.conf")));
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
    let store = SettingsStore::new(Some(dir.0.join(name).join("client.conf")));
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
fn a_service_without_a_demo_offers_none() {
    let service = service_with(|config| config.replace("info = \"01\"", ""));
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    alice.menu.act(Act::Go(Page::DemoHero));
    assert!(matches!(
        alice.menu.view().body,
        PageView::DemoHero { offered: false, .. }
    ));
    alice.menu.act(Act::Demo(HeroId::PUDGE.0));
    assert!(alice.menu.take_launch().is_none());
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
    game.act(bota_client_core::Intent::PickHero(HeroId::SHADOW_FIEND.0));
    game.act(bota_client_core::Intent::ToggleReady);
    pump_until("the match runs", &mut [&mut game], |c| {
        c.view().is_some_and(|v| v.tick > 5)
    });
    game.leave_game();
    // The server ends the match, finishes its file and exits.
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    while std::fs::metadata(&recording).map_or(0, |m| m.len()) < 1024 {
        assert!(
            Instant::now() < deadline,
            "the server never finished its replay"
        );
        game.pump(1.0 / 30.0);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(server);
    menu.left_game(None, true);
    settle_menu(&mut menu, |m| m.library.records().len() == 1);
    let record = menu.library.records()[0].clone();
    assert_eq!(record.mode, "local");
    assert!(!recording.exists(), "the recording became the kept replay");
    menu.act(Act::Go(Page::Replays));
    let PageView::Replays { list, .. } = menu.view().body else {
        panic!("the replays page");
    };
    assert_eq!(list.len(), 1);
    menu.act(Act::OpenReplay(record.name.clone()));
    assert!(matches!(menu.take_launch(), Some(Launch::Replay(_))));
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
    assert_eq!(alice.menu.view().searches[0].position, 1);
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
        rulesets
            .iter()
            .map(|r| (r.name.as_str(), r.audience))
            .collect::<Vec<_>>(),
        [("1x1 Mid Only", "Humans and Bots")]
    );
}

/// Makes a lobby of the game mode `mode` by name from the LOBBIES page.
fn create_lobby(player: &mut Player, mode: &str, name: &str) {
    player.menu.act(Act::Go(Page::Lobbies));
    let PageView::Lobbies { modes, .. } = player.menu.view().body else {
        panic!("the lobbies page");
    };
    let id = modes
        .iter()
        .find(|m| m.name == mode)
        .unwrap_or_else(|| panic!("the mode {mode} is offered: {modes:?}"))
        .id
        .clone();
    player.menu.act(Act::LobbyMode(id));
    player.fill(&[name]);
    player.menu.act(Act::CreateLobby);
    settle(&mut [player], |p| p.menu.page() == Page::Lobby);
}

/// A lobby is made for the game mode chosen among those the service offers this account (not
/// the bots-only one), and it is that ruleset's.
#[test]
fn a_lobby_is_made_for_the_game_mode_chosen() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    alice.menu.act(Act::Go(Page::Lobbies));
    let PageView::Lobbies { modes, .. } = alice.menu.view().body else {
        panic!("the lobbies page");
    };
    assert_eq!(
        modes
            .iter()
            .map(|m| (m.name.as_str(), m.chosen))
            .collect::<Vec<_>>(),
        [
            ("Full rules", true),
            ("1x1 Mid Only", false),
            ("Squad", false),
            ("Practice", false)
        ],
        "the first until one is chosen, none bots-only"
    );
    create_lobby(&mut alice, "Practice", "my practice");
    let (state, _) = alice.menu.lobby.as_ref().expect("sat in the lobby");
    assert_eq!(state.ruleset.0, "practice");
}

/// A host starts a lobby of a ruleset that allows it with nobody else: the empty seat stays empty
/// and the match is played by the host alone.
#[test]
fn a_host_starts_a_practice_lobby_alone_and_plays() {
    let service = service();
    let mut alice = Player::new(Some(&service));
    alice.register("alice");
    create_lobby(&mut alice, "Practice", "alone");
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
    create_lobby(&mut alice, "Practice", "alone");
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
    alice.menu.act(Act::CreateLobby);
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
    alice.menu.act(Act::ConfigureMode(solo));
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
    let error = super::launch::server_binary(Some(&stale)).expect_err("another wire");
    let said = error.to_string();
    assert!(
        said.contains("no bota_server of this client's wire")
            && said.contains(&stale.display().to_string())
            && said.contains("0000000000000001"),
        "{said}"
    );
    assert_eq!(
        super::launch::server_binary(Some(server_binary())).expect("the matching server"),
        server_binary()
    );
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

/// Alice makes a password lobby of the Squad mode and bob sits in it by name and password.
fn squad_lobby() -> (Service, Player, Player) {
    let service = service();
    let (mut alice, mut bob) = (Player::new(Some(&service)), Player::new(Some(&service)));
    alice.register("alice");
    bob.register("bob");
    alice.menu.act(Act::Go(Page::Lobbies));
    let PageView::Lobbies { modes, .. } = alice.menu.view().body else {
        panic!("the lobbies page");
    };
    let squad = modes.iter().find(|m| m.name == "Squad").expect("offered");
    alice.menu.act(Act::LobbyMode(squad.id.clone()));
    alice.fill(&["slots", PASSWORD]);
    alice.menu.act(Act::CreateLobby);
    settle(&mut [&mut alice], |p| p.menu.page() == Page::Lobby);
    bob.menu.act(Act::Go(Page::Lobbies));
    bob.fill(&["slots", PASSWORD]);
    bob.menu.act(Act::Submit);
    // Both are on the lobby page, but each page can be showing a grid from before the other sat
    // down: the lobby is whole once both menus have both members in their slots.
    settle(&mut [&mut alice, &mut bob], |p| {
        p.menu.page() == Page::Lobby && {
            let grid = lobby_of(p);
            seat_of(&grid, "alice").is_some() && seat_of(&grid, "bob").is_some()
        }
    });
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
    alice.menu.act(Act::ConfigureMode(practice));
    settle(&mut [&mut alice, &mut bob], |p| {
        lobby_of(p).settings.mode == "Practice"
    });
    alice.menu.act(Act::ConfigurePassword);
    settle(&mut [&mut alice, &mut bob], |p| {
        !lobby_of(p).settings.locked
    });
    // The rules changed and the slots did not: bob keeps his.
    assert_eq!(
        seat_of(&lobby_of(&bob), "bob"),
        Some((5, "DIRE".to_owned()))
    );
    // The audience the owner sits in cannot shut him out; a member cannot change anything.
    alice.menu.act(Act::ConfigureAudience(
        bota_radovid::proto::Audience::BotsOnly,
    ));
    settle(&mut [&mut alice], |p| {
        p.menu.view().status.is_some_and(|s| s.error)
    });
    bob.menu.act(Act::ConfigureAudience(
        bota_radovid::proto::Audience::HumansOnly,
    ));
    settle(&mut [&mut bob], |p| {
        p.menu.view().status.is_some_and(|s| s.error)
    });
    assert_eq!(lobby_of(&bob).settings.audience, "Humans and Bots");
}
