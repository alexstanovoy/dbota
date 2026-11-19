//! The services of the way in, end to end: the whole client against a real matchmaking service
//! (in this process, with no game server behind it) and against servers that are something else.
//! A service is checked before anyone signs in to it; each outcome is told in its own words and a
//! service that did not pass shows no login form. No test contacts the public service: the menus
//! of a test are the menus of a new player with a service of the test's own as the built-in one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use radovid::test_support::impostor::{Behaviour, Impostor, SEND_CAP};

use crate::main_menu::{
    Act, BUILT_IN_HOST, BuiltInGuard, MAX_ADDED_SERVICES, MenuButton, MenuCore, Page, ServiceAct,
    Verdict, built_in_for_test,
};

use super::harness::Client;
use super::menu::act;

use crate::main_menu::tests::port_lease;

/// A hang guard for what the client does at once; the checks end on their own.
const PATIENCE: Duration = Duration::from_secs(240);
static NEXT: AtomicU32 = AtomicU32::new(0);
const PASSWORD: &str = crate::main_menu::tests::PASSWORD;

/// A matchmaking service in this process with no rulesets and no game behind it, on a port of its
/// own (or a leased one), stopped when dropped.
struct Bare {
    runtime: tokio::runtime::Runtime,
    service: Option<radovid::service::Service>,
    url: String,
    pem: String,
    database: PathBuf,
    _game_port: port_lease::PortLease,
}

impl Bare {
    fn start(name: &str) -> Bare {
        let identity =
            radovid::tls::self_signed(vec!["127.0.0.1".to_owned(), "localhost".to_owned()])
                .expect("a certificate");
        Self::start_with(name, "127.0.0.1:0", &identity)
    }

    fn start_with(name: &str, listen: &str, identity: &radovid::tls::Identity) -> Bare {
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let stem = format!("services-{}-{number}", std::process::id());
        let database = crate::test_support::scratch_path(&format!("{stem}.db"));
        let game_port = port_lease::PortLease::new();
        // The one ruleset is never played: a service lists at least one, and nothing here queues.
        let text = format!(
            r#"
server_name = "{name}"
listen = "{listen}"
database = "{database}"
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
id = "idle"
name = "Idle"
game = "idle"
game_fingerprint = "0000000000000001"
teams = [1, 1]
rated = false
[ruleset.launcher]
kind = "local"
binary = "/nonexistent/idle-game"
public_url = "wss://127.0.0.1:{{port}}"
args = []
"#,
            database = database.display(),
            port = game_port.port(),
        );
        let path = crate::test_support::scratch_path(&format!("{stem}.toml"));
        std::fs::write(&path, text).expect("the configuration");
        let config = radovid::config::Config::load(&path).expect("a valid configuration");
        let _ = std::fs::remove_file(&path);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let service = runtime
            .block_on(radovid::service::Service::start(config, identity))
            .expect("the service starts");
        Bare {
            url: format!("wss://127.0.0.1:{}", service.addr.port()),
            pem: identity.cert_pem.clone(),
            service: Some(service),
            runtime,
            database,
            _game_port: game_port,
        }
    }

    fn port(&self) -> u16 {
        self.url
            .rsplit(':')
            .next()
            .and_then(|p| p.parse().ok())
            .expect("a port")
    }
}

impl Drop for Bare {
    fn drop(&mut self) {
        if let Some(service) = self.service.take() {
            self.runtime.block_on(service.shutdown());
        }
        let _ = std::fs::remove_file(&self.database);
    }
}

/// A file of the certificates a client trusts, `--trust-cert`'s.
fn trusting(pems: &[&str]) -> String {
    let path = crate::test_support::scratch_path(&format!(
        "services-trust-{}-{}.pem",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, pems.concat()).expect("the certificates");
    path.to_str().expect("a UTF-8 path").to_owned()
}

fn menu(client: &Client) -> &MenuCore {
    client.world().non_send::<MenuCore>()
}

fn page(client: &Client) -> Page {
    menu(client).page()
}

fn shows(client: &mut Client, text: &str) -> bool {
    client.shown_texts().iter().any(|shown| shown == text)
}

/// The acts of the buttons on screen.
fn button_acts(client: &mut Client) -> Vec<Act> {
    let world = client.world_mut();
    world
        .query::<(&MenuButton, &InheritedVisibility, &ComputedNode)>()
        .iter(world)
        .filter(|(_, shown, node)| shown.get() && node.size() != Vec2::ZERO)
        .map(|(button, ..)| button.0.clone())
        .collect()
}

/// Whether the sign-in page's fields and buttons are on screen.
fn sign_in_is_open(client: &mut Client) -> bool {
    let acts = button_acts(client);
    acts.contains(&Act::Submit)
        || (acts.contains(&Act::Go(Page::Register)) && shows(client, "Login"))
}

/// A client of a new player whose built-in service is `built_in`, trusting `pems`.
fn new_player(built_in: &str, pems: &[&str], files: &[(&str, &str)]) -> (Client, BuiltInGuard) {
    new_player_with(&[], built_in, pems, files)
}

/// [`new_player`] with more of the command line, such as the window's size.
fn new_player_with(
    extra: &[&str],
    built_in: &str,
    pems: &[&str],
    files: &[(&str, &str)],
) -> (Client, BuiltInGuard) {
    let guard = built_in_for_test(built_in);
    let trust = trusting(pems);
    let mut args = vec!["--trust-cert", trust.as_str()];
    args.extend_from_slice(extra);
    let client = Client::run_with_files(&args, files);
    (client, guard)
}

/// Types `address` into the Service page's field and adds it, as a person does.
fn add_service(client: &mut Client, address: &str) {
    act(client, Act::Go(Page::Service));
    act(client, Act::Focus(0));
    act(client, Act::Type(address.to_owned()));
    act(client, Act::Submit);
}

/// Runs frames until the check of `address` has found what it will.
fn wait_for_verdict(client: &mut Client, address: &str) {
    client.within(PATIENCE, &format!("the check of {address}"), |c| {
        menu(c)
            .verdict_for_test(address)
            .is_some_and(|verdict| verdict != Verdict::Checking)
    });
}

fn wait_for_text(client: &mut Client, what: &str, text: &str) {
    client.within(PATIENCE, what, |c| shows(c, text));
}

#[test]
fn a_new_player_lands_on_the_built_in_services_sign_in_which_works_and_signs_up() {
    let service = Bare::start("Test arena");
    let (mut client, _built_in) = new_player(&service.url, &[&service.pem], &[]);
    assert_eq!(
        page(&client),
        Page::Login,
        "no offline lobby for a new player"
    );
    assert_eq!(
        menu(&client).chosen_service_for_test().as_deref(),
        Some(service.url.as_str())
    );
    wait_for_text(&mut client, "the service passed", "Public service — works");
    let world = client.world_mut();
    let mut summary = world.query::<(&Name, &ComputedNode)>();
    let (_, node) = summary
        .iter(world)
        .find(|(name, _)| name.as_str() == "service check summary")
        .expect("the service summary is laid out");
    let height = node.size().y * node.inverse_scale_factor();
    assert!(
        (0.0..30.0).contains(&height) && height > 0.0,
        "the public service summary fits one line, got {height} logical pixels"
    );
    // One quiet line: the address and what the service calls itself are for the Service page.
    for detail in [service.url.as_str(), "Works", "Test arena", "Service"] {
        assert!(
            !shows(&mut client, detail)
                && !shows(
                    &mut client,
                    &format!("It calls itself {detail}. You can sign in.")
                ),
            "the card does not say {detail:?}"
        );
    }
    assert!(
        button_acts(&mut client).contains(&Act::Go(Page::Service)),
        "CHANGE SERVICE is by the line"
    );
    client.within(PATIENCE, "the sign-in form", sign_in_is_open);
    act(&mut client, Act::Go(Page::Register));
    for (at, value) in ["newcomer", PASSWORD, PASSWORD].into_iter().enumerate() {
        act(&mut client, Act::Focus(at));
        act(&mut client, Act::Type(value.to_owned()));
    }
    act(&mut client, Act::Submit);
    client.within(PATIENCE, "signed up, on Play", |c| page(c) == Page::Home);
}

#[test]
fn while_the_service_is_checked_there_is_no_login_form_and_the_frames_go_on() {
    let silent = Impostor::start(Behaviour::Silent);
    let (mut client, _built_in) = new_player(&silent.url, &[&silent.pem], &[]);
    client.within(PATIENCE, "the check under way", |c| {
        shows(c, "Checking the service…")
    });
    // The server never answers: a check that blocked the frame loop could not be stepped here.
    client.step_frames(30);
    assert!(shows(&mut client, "Checking the service…"));
    assert!(!sign_in_is_open(&mut client));
    assert!(!shows(&mut client, "Password"));
    assert!(!button_acts(&mut client).contains(&Act::Submit));
    // One check at a time for a service, whatever is pressed.
    act(
        &mut client,
        Act::Service(ServiceAct::Recheck(silent.url.clone())),
    );
    assert_eq!(menu(&client).checks_running_for_test(), 1);
    silent.wait_for_a_connection();
}

#[test]
fn a_check_is_dropped_when_the_player_moves_on_to_another_service() {
    let silent = Impostor::start(Behaviour::Silent);
    let other = Bare::start("Other");
    let (mut client, _built_in) = new_player(&silent.url, &[&silent.pem, &other.pem], &[]);
    client.within(PATIENCE, "the check under way", |c| {
        menu(c).checks_running_for_test() == 1
    });
    silent.wait_for_a_connection();
    add_service(&mut client, &other.url);
    client.within(PATIENCE, "the other service passed", |c| shows(c, "Works"));
    // The silent server sees its connection end: the check was cancelled, not left to time out.
    silent.wait_for_a_connection_to_end();
    assert_eq!(menu(&client).checks_running_for_test(), 0);
}

/// One scenario of the table of outcomes: what is typed, and what the sign-in then says.
struct Outcome {
    what: &'static str,
    address: String,
    headline: &'static str,
}

#[test]
fn each_failed_check_says_its_own_thing_and_opens_no_login_form() {
    let real = Bare::start("Real");
    let echo = Impostor::start(Behaviour::Echo);
    let web = Impostor::start(Behaviour::PlainHttp);
    let hung = Impostor::start(Behaviour::HangUp);
    let newer = Impostor::start(Behaviour::Refuses(radovid_proto_mismatch(1)));
    let older = Impostor::start(Behaviour::Refuses(radovid_proto_mismatch(-1)));
    let oversized = Impostor::start(Behaviour::Oversized(1024 * 1024));
    let endless = Impostor::start(Behaviour::Endless);
    // Its certificate is the one thing the client is not given.
    let stranger = Impostor::start(Behaviour::Echo);
    let closed = port_lease::PortLease::new();
    let table = [
        Outcome {
            what: "a wrong path on a running service",
            address: format!("{}/wrong", real.url),
            headline: "Something answers, but it is not this kind of service",
        },
        Outcome {
            what: "a port nobody listens on",
            address: format!("wss://127.0.0.1:{}", closed.port()),
            headline: "Nobody is listening at this address",
        },
        Outcome {
            what: "an echo server",
            address: echo.url.clone(),
            headline: "Something answers, but it is not this kind of service",
        },
        Outcome {
            what: "a web server without TLS",
            address: web.url.clone(),
            headline: "Something answers, but it is not this kind of service",
        },
        Outcome {
            what: "a server that hangs up",
            address: hung.url.clone(),
            headline: "The service hung up without answering",
        },
        Outcome {
            what: "a newer service",
            address: newer.url.clone(),
            headline: "The service is newer than this client",
        },
        Outcome {
            what: "an older service",
            address: older.url.clone(),
            headline: "The service is older than this client",
        },
        Outcome {
            what: "a certificate nobody vouches for",
            address: stranger.url.clone(),
            headline: "The connection is not secure",
        },
        Outcome {
            what: "a host that does not exist",
            address: "wss://nothing-here.invalid".to_owned(),
            headline: "Cannot find the host",
        },
        Outcome {
            what: "a first message over the bound",
            address: oversized.url.clone(),
            headline: "Something answers, but it is not this kind of service",
        },
        Outcome {
            what: "a first message that never ends",
            address: endless.url.clone(),
            headline: "Something answers, but it is not this kind of service",
        },
    ];
    let trust = [
        real.pem.as_str(),
        echo.pem.as_str(),
        web.pem.as_str(),
        hung.pem.as_str(),
        newer.pem.as_str(),
        older.pem.as_str(),
        oversized.pem.as_str(),
        endless.pem.as_str(),
    ];
    let (mut client, _built_in) = new_player(&real.url, &trust, &[]);
    client.within(PATIENCE, "the built-in service passed", |c| {
        shows(c, "Public service — works")
    });
    for outcome in &table {
        add_service(&mut client, &outcome.address);
        let what = format!("{}: {}", outcome.what, outcome.headline);
        wait_for_verdict(&mut client, &outcome.address);
        act(&mut client, Act::Go(Page::Login));
        assert!(
            shows(&mut client, outcome.headline),
            "{what}: on the sign-in card"
        );
        assert!(!sign_in_is_open(&mut client), "{what}: no login form");
        assert!(!shows(&mut client, "Password"), "{what}: no password field");
        let acts = button_acts(&mut client);
        assert!(
            !acts.contains(&Act::Submit) && !acts.contains(&Act::Go(Page::Register)),
            "{what}: no way to sign in or register: {acts:?}"
        );
        assert!(
            acts.iter()
                .any(|act| matches!(act, Act::Service(ServiceAct::Recheck(_)))),
            "{what}: the check can be run again"
        );
        let words = client.shown_texts().join(" | ");
        assert!(
            !words.to_lowercase().contains("os error") && !words.contains("Errno"),
            "{what}: no error text of the system on screen: {words}"
        );
    }
    endless.wait_for_a_connection_to_end();
    assert!(
        endless.sent() < SEND_CAP / 2,
        "the client read {} bytes of a message that never ends",
        endless.sent()
    );
}

/// The refusal a service of another protocol version makes: `by` versions from the client's.
fn radovid_proto_mismatch(by: i32) -> bota_radovid::proto::Error {
    let ours = i32::from(bota_radovid::proto::PROTO_VERSION);
    bota_radovid::proto::Error::ProtoMismatch {
        server: u16::try_from(ours + by).expect("a version"),
    }
}

#[test]
fn a_check_that_runs_out_of_time_says_nobody_answered() {
    let silent = Impostor::start(Behaviour::Silent);
    let (mut client, _built_in) = new_player(&silent.url, &[&silent.pem], &[]);
    // A limit already spent: the check ends at its first step, with no clock to wait for.
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .set_check_patience_for_test(Duration::ZERO);
    act(
        &mut client,
        Act::Service(ServiceAct::Recheck(silent.url.clone())),
    );
    wait_for_text(&mut client, "the time-out told", "Nobody answered in time");
    assert!(!sign_in_is_open(&mut client));
}

#[test]
fn a_service_that_starts_after_a_failed_check_opens_sign_in_on_check_again() {
    let lease = port_lease::PortLease::new();
    let identity = radovid::tls::self_signed(vec!["127.0.0.1".to_owned()]).expect("a certificate");
    let url = format!("wss://127.0.0.1:{}", lease.port());
    let (mut client, _built_in) = new_player(&url, &[&identity.cert_pem], &[]);
    wait_for_text(
        &mut client,
        "the refusal told",
        "Nobody is listening at this address",
    );
    assert!(!sign_in_is_open(&mut client));
    // The service comes up, and the check is run again as its button says.
    let _service = Bare::start_with("Late", &format!("127.0.0.1:{}", lease.port()), &identity);
    act(&mut client, Act::Service(ServiceAct::Recheck(url)));
    wait_for_text(&mut client, "the service passed", "Public service — works");
    client.within(PATIENCE, "the sign-in form", sign_in_is_open);
}

#[test]
fn services_added_removed_and_chosen_are_kept_across_a_restart() {
    let built_in = Bare::start("Built in");
    let (second, third) = (Bare::start("Second"), Bare::start("Third"));
    let pems = [
        built_in.pem.as_str(),
        second.pem.as_str(),
        third.pem.as_str(),
    ];
    let (mut client, guard) = new_player(&built_in.url, &pems, &[]);
    client.within(PATIENCE, "the built-in service passed", |c| {
        shows(c, "Public service — works")
    });
    // As people type them: no scheme, and the address of a page.
    add_service(&mut client, &format!("127.0.0.1:{}", second.port()));
    add_service(
        &mut client,
        &format!(" https://127.0.0.1:{}/some/page/ ", third.port()),
    );
    let list = |c: &Client| -> Vec<String> {
        menu(c)
            .services_for_test()
            .into_iter()
            .map(|entry| entry.address)
            .collect()
    };
    assert_eq!(
        list(&client),
        [built_in.url.clone(), second.url.clone(), third.url.clone()]
    );
    act(
        &mut client,
        Act::Service(ServiceAct::Remove(third.url.clone())),
    );
    act(
        &mut client,
        Act::Service(ServiceAct::Use(second.url.clone())),
    );
    assert_eq!(list(&client), [built_in.url.clone(), second.url.clone()]);
    let kept = client.settings_folder_file("menu.conf");
    assert!(
        kept.contains(&format!("service_added = {}\n", second.url)) && !kept.contains(&third.url),
        "{kept}"
    );
    drop(client);
    drop(guard);
    let (mut again, _guard) = new_player(&built_in.url, &pems, &[("menu.conf", &kept)]);
    assert_eq!(list(&again), [built_in.url.clone(), second.url.clone()]);
    assert_eq!(
        menu(&again).chosen_service_for_test().as_deref(),
        Some(second.url.as_str()),
        "the service chosen is still the one signed in to"
    );
    // A service the player added is known by its host on the card, whatever it calls itself.
    let host = format!("127.0.0.1:{}", second.port());
    wait_for_text(
        &mut again,
        "the chosen service checked at start",
        &format!("{host} — works"),
    );
    assert!(!shows(&mut again, "Second"));
}

#[test]
fn the_built_in_service_cannot_be_removed_and_need_not_be_chosen() {
    let built_in = Bare::start("Built in");
    let other = Bare::start("Other");
    let (mut client, _guard) = new_player(&built_in.url, &[&built_in.pem, &other.pem], &[]);
    add_service(&mut client, &other.url);
    assert!(
        !button_acts(&mut client).contains(&Act::Service(ServiceAct::Remove(built_in.url.clone()))),
        "no REMOVE on the built-in service"
    );
    assert!(
        button_acts(&mut client).contains(&Act::Service(ServiceAct::Remove(other.url.clone()))),
        "REMOVE on one that was added"
    );
    act(
        &mut client,
        Act::Service(ServiceAct::Remove(built_in.url.clone())),
    );
    assert!(shows(
        &mut client,
        "Only a service you added can be removed."
    ));
    let addresses: Vec<String> = menu(&client)
        .services_for_test()
        .into_iter()
        .map(|entry| entry.address)
        .collect();
    assert_eq!(addresses, [built_in.url.clone(), other.url.clone()]);
    assert_eq!(
        menu(&client).chosen_service_for_test().as_deref(),
        Some(other.url.as_str()),
        "another service is chosen, the built-in one stays listed"
    );
}

#[test]
fn an_address_is_made_canonical_as_it_is_typed_and_refused_with_its_reason() {
    let built_in = Bare::start("Built in");
    let (mut client, _guard) = new_player(&built_in.url, &[&built_in.pem], &[]);
    act(&mut client, Act::Go(Page::Service));
    act(&mut client, Act::Focus(0));
    let invalid_host = format!(
        "The host name is not valid. Use letters, digits, dashes and dots, like {BUILT_IN_HOST}."
    );
    for (typed, said) in [
        (
            "  Play.Example.Org/ ",
            "Will be added as wss://play.example.org",
        ),
        (
            "https://example.org/lobby/index.html",
            "Will be added as wss://example.org",
        ),
        (
            "ws://example.org",
            "A service is reached over a secure connection. Use wss://, or leave the start out.",
        ),
        (
            "example.org:99999",
            "The port after the colon must be a number from 1 to 65535.",
        ),
        ("wss://wrong_path", invalid_host.as_str()),
    ] {
        while !menu(&client).typed_for_test().is_empty() {
            act(&mut client, Act::Erase);
        }
        act(&mut client, Act::Type(typed.to_owned()));
        assert!(shows(&mut client, said), "{typed:?} says {said:?}");
    }
    // Pressing ADD on what is no address adds nothing and says why.
    let before = menu(&client).services_for_test().len();
    act(&mut client, Act::Submit);
    assert_eq!(menu(&client).services_for_test().len(), before);
    assert!(shows(&mut client, &invalid_host));
}

#[test]
fn a_settings_file_from_before_the_list_keeps_its_service_and_a_broken_one_starts_the_client() {
    let built_in = Bare::start("Built in");
    let closed = port_lease::PortLease::new();
    let remembered = format!("wss://127.0.0.1:{}", closed.port());
    // The file of an older client: a service typed as it was, with the path.
    let old = format!("service = {remembered}/control\nlogin = alice\nplain_ok = old.example\n");
    let (mut client, guard) = new_player(&built_in.url, &[&built_in.pem], &[("menu.conf", &old)]);
    assert_eq!(page(&client), Page::Login);
    assert_eq!(
        menu(&client).chosen_service_for_test().as_deref(),
        Some(remembered.as_str())
    );
    let entries = menu(&client).services_for_test();
    assert_eq!(
        entries.len(),
        2,
        "the built-in service and the remembered one"
    );
    assert!(
        entries[1].removable(),
        "the remembered service is one the player added"
    );
    wait_for_text(
        &mut client,
        "its check told",
        "Nobody is listening at this address",
    );
    drop(client);
    drop(guard);
    let broken = "service = not a service address\nservice_added = also nonsense\nlogin\n\0\n";
    let (client, _guard) = new_player(&built_in.url, &[&built_in.pem], &[("menu.conf", broken)]);
    assert_eq!(
        menu(&client).chosen_service_for_test().as_deref(),
        Some(built_in.url.as_str()),
        "what is no address is no choice: the built-in service is chosen"
    );
    assert_eq!(menu(&client).services_for_test().len(), 1);
}

// The Service page holds any number of services in any window: the list scrolls, the rest stays.

/// The physical rectangle of the window (the tests' windows are of scale factor one).
fn window_rect(client: &mut Client) -> Rect {
    let world = client.world_mut();
    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window");
    Rect::new(
        0.0,
        0.0,
        window.physical_width() as f32,
        window.physical_height() as f32,
    )
}

/// Where a laid out node is.
fn placed(node: &ComputedNode, at: &UiGlobalTransform) -> Option<Rect> {
    node.size()
        .cmpgt(Vec2::ZERO)
        .all()
        .then(|| Rect::from_center_size(at.translation, node.size()))
}

/// The rectangle of the button of `act` that is on screen.
fn button_rect(client: &mut Client, act: &Act) -> Option<Rect> {
    let world = client.world_mut();
    world
        .query::<(
            &MenuButton,
            &InheritedVisibility,
            &ComputedNode,
            &UiGlobalTransform,
        )>()
        .iter(world)
        .find(|(button, shown, ..)| button.0 == *act && shown.get())
        .and_then(|(_, _, node, at)| placed(node, at))
}

/// The rectangle of the button or node called `name`.
fn named_rect(client: &mut Client, name: &str) -> Option<Rect> {
    let world = client.world_mut();
    world
        .query::<(&Name, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
        .find(|(label, ..)| label.as_str() == name)
        .and_then(|(_, node, at)| placed(node, at))
}

/// Whether `inner` lies in `outer`, to a pixel's rounding.
fn lies_in(outer: Rect, inner: Rect) -> bool {
    inner.min.x >= outer.min.x - 1.0
        && inner.min.y >= outer.min.y - 1.0
        && inner.max.x <= outer.max.x + 1.0
        && inner.max.y <= outer.max.y + 1.0
}

/// How far the service list is scrolled, and how far it can be.
fn list_scroll(client: &mut Client) -> (f32, f32) {
    let world = client.world_mut();
    world
        .query::<(&Name, &ScrollPosition, &ComputedNode)>()
        .iter(world)
        .find(|(name, ..)| name.as_str() == "service list")
        .map(|(_, position, node)| {
            let reach = (node.content_size().y - node.size().y) * node.inverse_scale_factor();
            (position.y, reach.max(0.0))
        })
        .expect("the service list")
}

/// Turns the wheel over `at`: lines are positive upward, as the pointer's wheel reports them.
fn wheel_over(client: &mut Client, at: Vec2, lines: f32) {
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
    let world = client.world_mut();
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window");
    client.mouse(at);
    // The pointer is over the list a frame or two before the wheel turns.
    client.frames(3);
    let wheel = MouseWheel {
        phase: bevy::input::touch::TouchPhase::Moved,
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: lines,
        window,
    };
    client
        .world_mut()
        .write_message(bevy::window::WindowEvent::MouseWheel(wheel));
    client.world_mut().write_message(wheel);
    client.frames(3);
}

/// The settings of a player with `count` services added, `chosen` signed in to if given.
fn added_services(count: usize, chosen: Option<&str>) -> String {
    let mut text = chosen.map_or_else(String::new, |chosen| format!("service = {chosen}\n"));
    for n in 0..count {
        text.push_str(&format!("service_added = arena{n}.example.org\n"));
    }
    text
}

/// The acts of the buttons of every row of the list.
fn row_acts(client: &Client) -> Vec<Act> {
    let chosen = menu(client).chosen_service_for_test();
    menu(client)
        .services_for_test()
        .into_iter()
        .flat_map(|entry| {
            let mut acts = vec![Act::Service(ServiceAct::Recheck(entry.address.clone()))];
            if chosen.as_deref() != Some(entry.address.as_str()) {
                acts.push(Act::Service(ServiceAct::Use(entry.address.clone())));
            }
            if entry.removable() {
                acts.push(Act::Service(ServiceAct::Remove(entry.address)));
            }
            acts
        })
        .collect()
}

/// A dozen services in a window of `size`: the field to add one and the way back are on the
/// screen, and every button of every service can be brought into the list's view by the wheel.
fn a_dozen_services_leave_every_control_in_reach(size: &str) {
    let built_in = Bare::start("Built in");
    let settings = added_services(12, None);
    let (mut client, _guard) = new_player_with(
        &["--window", size],
        &built_in.url,
        &[&built_in.pem],
        &[("menu.conf", &settings)],
    );
    wait_for_text(
        &mut client,
        "the built-in service passed",
        "Public service — works",
    );
    act(&mut client, Act::Go(Page::Service));
    client.frames(10);
    assert_eq!(menu(&client).services_for_test().len(), 13);
    let window = window_rect(&mut client);
    let field = button_rect(&mut client, &Act::Focus(0)).expect("the address field");
    let fixed = [
        ("the address field", Some(field)),
        ("ADD", named_rect(&mut client, "add")),
        ("BACK", named_rect(&mut client, "back")),
        (
            "Play without an account",
            named_rect(&mut client, "play without an account"),
        ),
    ];
    for (what, rect) in fixed {
        let rect = rect.unwrap_or_else(|| panic!("{what} is drawn at {size}"));
        assert!(
            lies_in(window, rect),
            "{what} {rect:?} lies in the window {window:?} at {size}"
        );
    }
    let list = named_rect(&mut client, "service list").expect("the service list");
    assert!(
        lies_in(window, list),
        "the list {list:?} lies in the window {window:?} at {size}"
    );
    let (_, reach) = list_scroll(&mut client);
    assert!(
        reach > 0.0,
        "a dozen services are more than the list shows at {size}"
    );
    // From the top, the wheel brings each button into the list's view in turn.
    let mut pending = row_acts(&client);
    // Over the edge of a row, where no button is.
    let edge = Vec2::new(list.min.x + 4.0, list.center().y);
    wheel_over(&mut client, edge, 1000.0);
    for _ in 0..120 {
        pending.retain(|act| !button_rect(&mut client, act).is_some_and(|at| lies_in(list, at)));
        if pending.is_empty() {
            break;
        }
        wheel_over(&mut client, edge, -3.0);
        eprintln!("DEBUG scroll {:?}", list_scroll(&mut client));
    }
    let (at, reach) = list_scroll(&mut client);
    assert!(
        pending.is_empty(),
        "never in the list's view at {size} (scrolled {at} of {reach}, the list is {list:?}): {pending:?}"
    );
}

#[test]
fn a_dozen_services_leave_every_control_in_reach_at_1280x720() {
    a_dozen_services_leave_every_control_in_reach("1280x720");
}

#[test]
fn a_dozen_services_leave_every_control_in_reach_at_800x600() {
    a_dozen_services_leave_every_control_in_reach("800x600");
}

#[test]
fn a_dozen_services_leave_every_control_in_reach_at_640x480() {
    a_dozen_services_leave_every_control_in_reach("640x480");
}

#[test]
fn the_page_opens_at_the_service_signed_in_to_and_keeps_its_place_as_findings_come_in() {
    let built_in = Bare::start("Built in");
    let closed = port_lease::PortLease::new();
    let chosen = format!("wss://127.0.0.1:{}", closed.port());
    // The service signed in to is the last of the list: a remembered one joins the added ones.
    let settings = added_services(12, Some(&chosen));
    let (mut client, _guard) = new_player_with(
        &["--window", "1280x720"],
        &built_in.url,
        &[&built_in.pem],
        &[("menu.conf", &settings)],
    );
    wait_for_text(
        &mut client,
        "its check told",
        "Nobody is listening at this address",
    );
    act(&mut client, Act::Go(Page::Service));
    client.frames(10);
    let list = named_rect(&mut client, "service list").expect("the service list");
    let own = button_rect(
        &mut client,
        &Act::Service(ServiceAct::Recheck(chosen.clone())),
    )
    .expect("the chosen service's row");
    assert!(
        lies_in(list, own),
        "the chosen service {own:?} is in the list's view {list:?}"
    );
    let (opened_at, _) = list_scroll(&mut client);
    assert!(opened_at > 0.0, "the list was scrolled to it");
    // A little way up, and the chosen service is checked again: the page is drawn anew twice,
    // and the list is where it was left.
    wheel_over(&mut client, list.center(), 2.0);
    let (left_at, _) = list_scroll(&mut client);
    assert!(left_at < opened_at, "the wheel moved the list");
    act(
        &mut client,
        Act::Service(ServiceAct::Recheck(chosen.clone())),
    );
    wait_for_verdict(&mut client, &chosen);
    client.frames(5);
    assert_eq!(
        list_scroll(&mut client).0,
        left_at,
        "the list stays where it was left"
    );
}

#[test]
fn tab_walks_a_long_list_with_each_button_it_stops_on_in_view() {
    let built_in = Bare::start("Built in");
    let settings = added_services(12, None);
    let (mut client, _guard) = new_player_with(
        &["--window", "1280x720"],
        &built_in.url,
        &[&built_in.pem],
        &[("menu.conf", &settings)],
    );
    wait_for_text(
        &mut client,
        "the built-in service passed",
        "Public service — works",
    );
    act(&mut client, Act::Go(Page::Service));
    let window = window_rect(&mut client);
    let in_list = row_acts(&client);
    let mut stops = 0;
    for _ in 0..45 {
        act(&mut client, Act::NextField);
        let Some(focused) = menu(&client).button_focus().cloned() else {
            continue;
        };
        stops += 1;
        let at = button_rect(&mut client, &focused)
            .unwrap_or_else(|| panic!("the button {focused:?} Tab stops on is drawn"));
        assert!(lies_in(window, at), "{focused:?} {at:?} lies in the window");
        if in_list.contains(&focused) {
            let list = named_rect(&mut client, "service list").expect("the service list");
            assert!(
                lies_in(list, at),
                "{focused:?} {at:?} is in the list's view {list:?}"
            );
        }
    }
    assert!(
        stops > 20,
        "Tab stopped on the buttons of the list ({stops} stops)"
    );
}

#[test]
fn a_settings_file_of_thousands_of_services_lists_the_bound_and_adds_no_more() {
    let built_in = Bare::start("Built in");
    // Under the size a file is read to, over any number a player would add.
    let settings = added_services(1500, None);
    assert!(settings.len() < 64 * 1024, "{} bytes", settings.len());
    let (mut client, _guard) =
        new_player(&built_in.url, &[&built_in.pem], &[("menu.conf", &settings)]);
    wait_for_text(
        &mut client,
        "the built-in service passed",
        "Public service — works",
    );
    assert_eq!(
        menu(&client).services_for_test().len(),
        1 + MAX_ADDED_SERVICES,
        "the built-in service and the most that can be added"
    );
    act(&mut client, Act::Go(Page::Service));
    act(&mut client, Act::Focus(0));
    act(&mut client, Act::Type("one-more.example.org".to_owned()));
    act(&mut client, Act::Submit);
    assert!(shows(
        &mut client,
        "The list is full. Remove a service before adding another."
    ));
    assert_eq!(
        menu(&client).services_for_test().len(),
        1 + MAX_ADDED_SERVICES
    );
    // A removal makes room again.
    act(
        &mut client,
        Act::Service(ServiceAct::Remove("wss://arena0.example.org".to_owned())),
    );
    act(&mut client, Act::Submit);
    assert_eq!(
        menu(&client).services_for_test().len(),
        1 + MAX_ADDED_SERVICES
    );
    assert!(
        menu(&client)
            .services_for_test()
            .iter()
            .any(|entry| entry.address == "wss://one-more.example.org")
    );
}

#[test]
fn a_settings_file_too_big_to_be_a_settings_file_is_set_aside_and_the_client_starts() {
    let built_in = Bare::start("Built in");
    let settings = added_services(10_000, None);
    assert!(settings.len() > 64 * 1024);
    let (mut client, _guard) =
        new_player(&built_in.url, &[&built_in.pem], &[("menu.conf", &settings)]);
    wait_for_text(
        &mut client,
        "the built-in service passed",
        "Public service — works",
    );
    assert_eq!(menu(&client).services_for_test().len(), 1);
    client.frames(30);
    assert_eq!(page(&client), Page::Login);
}

// What a service tells of itself is drawn next to the address a person chose.

/// Adds a service that calls itself `server_name` (TOML escapes allowed), waits for its check to
/// pass, and gives `then` the client and the line the Service page says of what it calls itself.
fn with_a_service_called(server_name: &str, then: impl FnOnce(&mut Client, String)) {
    let built_in = Bare::start("Built in");
    let named = Bare::start(server_name);
    let (mut client, _guard) = new_player(&built_in.url, &[&built_in.pem, &named.pem], &[]);
    add_service(&mut client, &named.url);
    wait_for_verdict(&mut client, &named.url);
    let said = client
        .shown_texts()
        .into_iter()
        .find(|text| text.starts_with("It calls itself") && !text.contains("Built in"))
        .expect("what the service calls itself is told");
    then(&mut client, said);
}

/// Characters that are not drawn or that change how the text around them reads.
fn is_hostile(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{AD}' | '\u{300}'..='\u{36F}' | '\u{61C}' | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}'
                | '\u{E0000}'..='\u{E007F}'
        )
}

#[test]
fn a_service_cannot_put_direction_overrides_or_line_breaks_in_the_name_it_is_shown_by() {
    // As the file of a service writes it: an override and an isolate that would turn what
    // follows around, a line break and a paragraph separator, a bell, a zero-width space.
    let name = r"Arena\u202eevil\u2066\n\u2029\u0007\u200b!";
    with_a_service_called(name, |client, said| {
        assert_eq!(said, "It calls itself Arenaevil!. You can sign in.");
        let texts = client.shown_texts();
        let hostile: Vec<&String> = texts.iter().filter(|t| t.chars().any(is_hostile)).collect();
        assert!(hostile.is_empty(), "nothing hostile is drawn: {hostile:?}");
    });
}

#[test]
fn a_service_cannot_make_the_name_it_is_shown_by_longer_than_the_bound() {
    let name = "x".repeat(5000);
    with_a_service_called(&name, |_, said| {
        let shown = said
            .strip_prefix("It calls itself ")
            .and_then(|rest| rest.strip_suffix(". You can sign in."))
            .expect("the usual words");
        assert_eq!(shown.chars().count(), 48);
    });
}

// A service is asked, before the player chose to sign in to it, only whether it is one.

/// What is in a player's settings folder that says who they are.
const MENU_SECRETS: &str =
    "login = secret_login\ndirect = 10.9.8.7:1234\nplain_ok = secret.example\nhost_port = 7777\n";
const SECRETS: [&str; 4] = ["secret_login", "10.9.8.7", "secret.example", "7777"];

/// Waits for the check of a client whose built-in service is an echo server, and gives back what
/// the server was sent, each message as the protocol reads it.
fn what_an_echo_server_was_sent(
    server: &Impostor,
    files: &[(&str, &str)],
) -> Vec<(Vec<u8>, bota_radovid::proto::ClientMsg)> {
    let (mut client, _guard) = new_player(&server.url, &[&server.pem], files);
    wait_for_text(
        &mut client,
        "the check told",
        "Something answers, but it is not this kind of service",
    );
    client.frames(30);
    server
        .received()
        .into_iter()
        .map(|bytes| {
            let payload = bota_radovid::proto::whole_frame(
                &bytes,
                bota_radovid::proto::MAX_CONTROL_PAYLOAD_LEN,
            )
            .expect("a whole frame");
            let frame: bota_radovid::proto::ClientFrame =
                bota_radovid::proto::decode_payload(payload).expect("a message of the protocol");
            (bytes, frame.msg)
        })
        .collect()
}

fn holds(bytes: &[u8], secret: &str) -> bool {
    bytes
        .windows(secret.len())
        .any(|part| part == secret.as_bytes())
}

#[test]
fn the_check_of_the_built_in_service_at_first_start_sends_one_hello_and_nothing_of_the_player() {
    let server = Impostor::start(Behaviour::Echo);
    let said = what_an_echo_server_was_sent(&server, &[("menu.conf", MENU_SECRETS)]);
    let kinds: Vec<_> = said.iter().map(|(_, msg)| msg).collect();
    assert_eq!(
        said.len(),
        1,
        "one message, and nothing signs in on its own: {kinds:?}"
    );
    assert!(
        matches!(said[0].1, bota_radovid::proto::ClientMsg::Hello { .. }),
        "the message is the Hello: {kinds:?}"
    );
    for secret in SECRETS {
        assert!(
            !holds(&said[0].0, secret),
            "the Hello does not hold {secret:?}"
        );
    }
}

#[test]
fn a_saved_session_is_never_sent_to_a_server_that_is_no_service() {
    let server = Impostor::start(Behaviour::Echo);
    // The player signed in here before: the client reaches the service for the session, as it
    // always did. A server that is no service never answers with a Welcome, so it is sent
    // Hellos and nothing else, the token least of all.
    let token = "ab".repeat(32);
    let session = format!("{}\n{token}\n", server.url);
    let said = what_an_echo_server_was_sent(
        &server,
        &[("menu.conf", MENU_SECRETS), ("session", &session)],
    );
    let kinds: Vec<_> = said.iter().map(|(_, msg)| msg).collect();
    assert!(!said.is_empty());
    for (bytes, msg) in &said {
        assert!(
            matches!(msg, bota_radovid::proto::ClientMsg::Hello { .. }),
            "only Hellos: {kinds:?}"
        );
        for secret in SECRETS.iter().copied().chain([token.as_str()]) {
            assert!(!holds(bytes, secret), "no message holds {secret:?}");
        }
    }
}
