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
use bota_proto::{HeroId, Order, Target};
use bota_radovid::proto::RulesetId;

use super::core::{Act, Confirm, GameKind, Launch, MenuCore, Page, SeatFill};
use super::launch::{Hosted, Opening, Trust};
use super::store::{Entry, SettingsStore};
use super::view::PageView;

const PASSWORD: &str = "correct horse battery";
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
fn server_binary() -> &'static Path {
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
struct Service {
    runtime: tokio::runtime::Runtime,
    service: Option<radovid::service::Service>,
    url: String,
    pem: String,
    _dir: Scratch,
}

impl Drop for Service {
    fn drop(&mut self) {
        if let Some(service) = self.service.take() {
            self.runtime.block_on(service.shutdown());
        }
    }
}

fn config(dir: &Path, binary: &Path) -> String {
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
[limits]
games_per_source = 200
game_capacity = 1
lobby_games_max = 1
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

# Two against two, for the lobby page: its slots and its sides.
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
"#,
        db = dir.join("radovid.db").display(),
        fingerprint = bota_radovid::fingerprint(),
        binary = binary.display(),
    )
}

fn service() -> Service {
    let binary = server_binary().to_owned();
    let dir = Scratch::new("service");
    let path = dir.0.join("radovid.toml");
    std::fs::write(&path, config(&dir.0, &binary)).expect("config");
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
    comes_back_logged_in(alice, &service);
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
    menu.act(Act::Focus(2));
    while !menu.form.value(2).is_empty() {
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
    let mut menu = MenuCore::new(store, trust.radovid.clone(), Some(nowhere));
    menu.replays_dir = Some(dir.0.join("replays"));
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
        matches!(&launch, Launch::Host { name, players: 2, .. } if name == "alice"),
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

    // The local lobby's replay is listed; its match never started, so the
    // list says so and does not open it.
    drop(server);
    alice.left_game(None, false);
    alice.act(Act::Go(Page::Replays));
    assert_eq!(alice.replays.len(), 1, "the lobby's replay");
    let PageView::Replays { list, .. } = alice.view().body else {
        panic!("the replays page");
    };
    assert_eq!(
        list[0].summary,
        super::replay_info::Summary::Broken("its match never started".to_owned())
    );
    alice.act(Act::OpenReplay(0));
    assert!(alice.take_launch().is_none(), "nothing to watch");
    assert!(
        alice
            .view()
            .status
            .is_some_and(|s| s.error && s.text.contains("never started"))
    );
    assert_eq!(alice.page(), Page::Replays);

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

/// Hosts a local lobby on a free port with `fill` chosen for its empty
/// seats, and opens it.
fn host_with(menu: &mut MenuCore, trust: &Trust, fill: SeatFill) -> (Core, Option<Hosted>, u16) {
    menu.act(Act::Go(Page::LocalLobby));
    menu.act(Act::Fill(fill));
    let port = free_port();
    menu.act(Act::Focus(0));
    while !menu.form.value(0).is_empty() {
        menu.act(Act::Erase);
    }
    menu.act(Act::Type(port.to_string()));
    menu.act(Act::Host(false));
    let launch = menu.take_launch().expect("the lobby to open");
    let (core, server) = open_hosted(launch, trust);
    menu.game_started();
    (core, server, port)
}

/// A local lobby's empty seat takes what the lobby page chose, as Dota's
/// lobby fills a slot with a bot: the bot
/// command of the settings, run without a shell, given the game's address
/// in its words and a seat key of its own in `BOTA_SEAT_KEY`, and ended with
/// the lobby. Without a bot command the lobby is not opened and the page
/// says why. The host keeps the first seat, and the lobby page and the hero
/// screen tell the address others join at while a seat is open.
#[cfg(unix)]
#[test]
fn a_local_lobby_fills_its_empty_seats() {
    let dir = Scratch::new("fill");
    let (mut alice, trust) = offline("alice", &dir);

    // A bot asked for with no bot command set: nothing opens, and the page
    // says where to set it.
    alice.act(Act::Go(Page::LocalLobby));
    alice.act(Act::Fill(SeatFill::Bot));
    alice.act(Act::Host(false));
    assert!(
        alice.take_launch().is_none(),
        "no lobby without a bot to fill it"
    );
    assert!(
        alice
            .view()
            .status
            .is_some_and(|s| s.error && s.text.contains("bot command")),
        "{:?}",
        alice.view().status
    );

    // The bot command: a program that writes what it was given and waits.
    let said = dir.0.join("bot-said");
    alice.set_bot_command(&format!(
        "/bin/sh -c 'echo \"$$ $BOTA_SEAT_KEY {{address}}\" > \"$0\"; exec sleep 60' '{}'",
        said.display()
    ));
    let (mut hosted, server, port) = host_with(&mut alice, &trust, SeatFill::Bot);
    pump_until("the host seated", &mut [&mut hosted], |c| {
        c.my_slot.is_some()
    });
    let first = hosted.lobby.slots.first().map(|s| s.slot);
    assert_eq!(hosted.my_slot, first, "the host keeps the first seat");
    assert_eq!(
        alice.hosted_at(),
        Some(format!("127.0.0.1:{port} (this machine only)").as_str()),
        "the address others type"
    );
    let deadline = Instant::now() + Duration::from_secs(PATIENCE);
    let told = loop {
        if let Ok(text) = std::fs::read_to_string(&said)
            && text.ends_with('\n')
        {
            break text;
        }
        assert!(Instant::now() < deadline, "the bot command never ran");
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut words = told.split_whitespace();
    let (Some(pid), Some(key), Some(address)) = (words.next(), words.next(), words.next()) else {
        panic!("a pid, a key and an address: {told:?}");
    };
    assert_eq!(address, format!("127.0.0.1:{port}"));
    assert!(
        key.len() == 64 && key.chars().all(|c| c.is_ascii_hexdigit()),
        "a seat key in BOTA_SEAT_KEY: {key:?}"
    );
    // The lobby's end ends its bot (and waits for it).
    assert!(Path::new(&format!("/proc/{pid}")).exists(), "the bot runs");
    drop((hosted, server));
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "the bot outlived its lobby"
    );
}

/// The gold of a client's own seat, as its view tells.
fn own_gold(core: &Core) -> i32 {
    core.view()
        .zip(core.my_slot)
        .and_then(|(view, slot)| view.player(slot))
        .and_then(|player| player.gold)
        .unwrap_or(0)
}

/// Demo Hero from the menu with no account: a server of its own with
/// cheats on, the hero chosen the one picked, and a cheat honoured.
#[test]
fn demo_hero_starts_a_cheat_match_with_the_hero_chosen() {
    let dir = Scratch::new("demo");
    let (mut menu, trust) = offline("alice", &dir);
    menu.act(Act::Go(Page::DemoHero));
    assert_eq!(menu.page(), Page::DemoHero);
    menu.act(Act::Demo(HeroId::PUDGE.0));
    let launch = menu.take_launch().expect("a demo to start");
    assert!(
        matches!(&launch, Launch::Demo { hero, name, .. } if *hero == HeroId::PUDGE && name == "alice")
    );
    let (mut core, server) = open_hosted(launch, &trust);
    assert!(server.is_some(), "the demo's server is this client's");
    menu.game_started();
    assert_eq!(menu.demo_hero(), Some(HeroId::PUDGE));
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
    // Leaving a demo leaves no seat to take again: its server ends with it.
    menu.act(Act::Dashboard);
    menu.act(Act::Disconnect);
    assert!(menu.take_leave());
    menu.left_game(None, false);
    assert_eq!(menu.page(), Page::DemoHero);
    assert!(!menu.view().reconnect);
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
    for (at, value) in [free_port().to_string(), "1".to_owned()]
        .into_iter()
        .enumerate()
    {
        menu.act(Act::Focus(at));
        while !menu.form.value(at).is_empty() {
            menu.act(Act::Erase);
        }
        menu.act(Act::Type(value));
    }
    menu.act(Act::Host(false));
    let launch = menu.take_launch().expect("the lobby to open");
    assert!(matches!(
        &launch,
        Launch::Host {
            cheats: true,
            players: 1,
            ..
        }
    ));
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

/// The local lobby page walks as it is drawn: its port and players, CREATE, then the address
/// to join with PLAY and WATCH below.
#[test]
fn tab_walks_the_local_lobby_page_in_the_order_it_is_drawn() {
    let dir = Scratch::new("tab_local");
    let (mut alice, _trust) = offline("alice", &dir);
    alice.act(Act::Go(Page::LocalLobby));
    let at = |m: &MenuCore| (m.form.focused_field(), m.form.focused_button().cloned());
    assert_eq!(at(&alice), (Some(0), None));
    for expected in [
        (Some(1), None),
        (None, Some(Act::Host(false))),
        (None, Some(Act::Host(true))),
        (Some(2), None),
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
            ("1x1 Mid Only", true),
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
    settle(&mut [&mut alice, &mut bob], |p| {
        p.menu.page() == Page::Lobby
    });
    (service, alice, bob)
}

/// The slot grid, as the ruleset numbers it team by team: a member sits in a free slot at once,
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
        [("RADIANT", 2), ("DIRE", 2)]
    );
    assert_eq!(seat_of(&grid, "alice"), Some((0, "RADIANT".to_owned())));
    assert_eq!(seat_of(&grid, "bob"), Some((1, "RADIANT".to_owned())));

    // A free slot of the other side, at once; both menus see it.
    bob.menu.act(Act::LobbySlot(3));
    settle(&mut [&mut alice, &mut bob], |p| {
        seat_of(&lobby_of(p), "bob") == Some((3, "DIRE".to_owned()))
    });
    // The owner picks bob to move and clicks another slot.
    alice.menu.act(Act::MoveMember(
        lobby_of(&alice).teams[1].slots[1]
            .who
            .as_ref()
            .expect("bob")
            .account,
    ));
    assert!(lobby_of(&alice).moving.is_some());
    alice.menu.act(Act::LobbySlot(2));
    settle(&mut [&mut alice, &mut bob], |p| {
        seat_of(&lobby_of(p), "bob") == Some((2, "DIRE".to_owned()))
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
    // Practice is a 1v1: the member past its slots now looks on.
    settle(&mut [&mut bob], |p| {
        lobby_of(p).spectators.iter().any(|m| m.name == "bob")
            || seat_of(&lobby_of(p), "bob").is_some()
    });
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
