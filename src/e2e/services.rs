//! The services of the way in, end to end: the whole client against a real matchmaking service
//! (in this process, with no game server behind it) and against servers that are something else.
//! A service is checked before anyone signs in to it; each outcome is told in its own words and a
//! service that did not pass shows no login form. No test contacts the public service: the menus
//! of a test are the menus of a new player with a service of the test's own as the built-in one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bevy::prelude::*;
use radovid::test_support::impostor::{Behaviour, Impostor, SEND_CAP};

use crate::main_menu::{
    Act, BuiltInGuard, MenuButton, MenuCore, Page, ServiceAct, Verdict, built_in_for_test,
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
    let guard = built_in_for_test(built_in);
    let trust = trusting(pems);
    let client = Client::run_with_files(&["--trust-cert", &trust], files);
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
    wait_for_text(&mut client, "the service passed", "Works");
    assert!(shows(&mut client, "Public service"));
    assert!(shows(&mut client, &service.url));
    assert!(shows(
        &mut client,
        "It calls itself Test arena. You can sign in."
    ));
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
        shows(c, "Works")
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
    wait_for_text(&mut client, "the service passed", "Works");
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
        shows(c, "Works")
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
    wait_for_text(&mut again, "the chosen service checked at start", "Works");
    assert!(shows(
        &mut again,
        "It calls itself Second. You can sign in."
    ));
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
    for (typed, said) in [
        (
            "  Bota.Lubush.Space/ ",
            "Will be added as wss://bota.lubush.space",
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
        (
            "wss://wrong_path",
            "The host name is not valid. Use letters, digits, dashes and dots, like bota.lubush.space.",
        ),
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
    assert!(shows(
        &mut client,
        "The host name is not valid. Use letters, digits, dashes and dots, like bota.lubush.space."
    ));
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
