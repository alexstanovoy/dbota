//! Screenshot fixtures of the services: the list, and the sign-in card for every outcome of a
//! check. Explicit fixtures only: the menu never draws them.

use bota_radovid::{ServiceFault, ServiceFound};

use super::core::Page;
use super::form::FormKind;
use super::preview::{field, form};
use super::service_words;
use super::services::{BUILT_IN_ADDRESS, BUILT_IN_NAME, Verdict};
use super::view::{MenuView, PageView, ServiceBanner, ServiceRow, ServicesView, TypedService};

/// The names `--menu-preview` takes for the services.
pub const NAMES: [&str; 18] = [
    "service",
    "service-typing",
    "service-refused",
    "login-checking",
    "login-works",
    "login-host-not-found",
    "login-refused",
    "login-timed-out",
    "login-insecure",
    "login-not-service",
    "login-hangup",
    "login-newer",
    "login-older",
    "login-incompatible",
    "login-busy",
    "login-bad-address",
    "login-cannot-connect",
    "register-works",
];

const ADDED: &str = "wss://arena.example.org:4460";

fn verdict_of(name: &str) -> Option<Verdict> {
    Some(match name {
        "login-checking" => Verdict::Checking,
        "login-works" | "register-works" => Verdict::Works(ServiceFound {
            name: "Dota arena".to_owned(),
        }),
        "login-host-not-found" => Verdict::Failed(ServiceFault::HostNotFound),
        "login-refused" => Verdict::Failed(ServiceFault::Refused),
        "login-timed-out" => Verdict::Failed(ServiceFault::TimedOut),
        "login-insecure" => Verdict::Failed(ServiceFault::Insecure),
        "login-not-service" => Verdict::Failed(ServiceFault::NotService),
        "login-hangup" => Verdict::Failed(ServiceFault::Hangup),
        "login-newer" => Verdict::Failed(ServiceFault::Newer { server: 25 }),
        "login-older" => Verdict::Failed(ServiceFault::Older { server: 20 }),
        "login-incompatible" => Verdict::Failed(ServiceFault::Incompatible),
        "login-busy" => Verdict::Failed(ServiceFault::Busy),
        "login-bad-address" => Verdict::Failed(ServiceFault::BadAddress),
        "login-cannot-connect" => Verdict::Failed(ServiceFault::CannotConnect),
        _ => return None,
    })
}

pub fn view(name: &str) -> Option<MenuView> {
    match name {
        "service" | "service-typing" | "service-refused" => Some(list(name)),
        "register-works" => Some(way_in(name, Page::Register)),
        _ => verdict_of(name).map(|_| way_in(name, Page::Login)),
    }
}

fn way_in(name: &str, page: Page) -> MenuView {
    let verdict = verdict_of(name).expect("a fixture with an outcome");
    let words = service_words::verdict(Some(&verdict));
    let open = matches!(verdict, Verdict::Works(_));
    let banner = ServiceBanner {
        name: BUILT_IN_NAME.to_owned(),
        address: BUILT_IN_ADDRESS.to_owned(),
        checking: matches!(verdict, Verdict::Checking),
        words,
        open,
    };
    let (kind, fields) = if page == Page::Login {
        (
            FormKind::Login,
            vec![
                field("Login", "alice", false),
                field("Password", "••••••••••", true),
            ],
        )
    } else {
        (
            FormKind::Register,
            vec![
                field("Login", "alice", false),
                field("Password", "•••••••••", false),
                field("Password again", "•••••••••", true),
            ],
        )
    };
    let mut view = form(page, kind, fields);
    if let PageView::Form { service, .. } = &mut view.body {
        *service = Some(banner);
    }
    view
}

fn list(name: &str) -> MenuView {
    let row =
        |name: &str, address: &str, selected: bool, removable: bool, verdict: Verdict| ServiceRow {
            name: name.to_owned(),
            address: address.to_owned(),
            selected,
            removable,
            checking: matches!(verdict, Verdict::Checking),
            words: service_words::verdict(Some(&verdict)),
        };
    let rows = vec![
        row(
            BUILT_IN_NAME,
            BUILT_IN_ADDRESS,
            true,
            false,
            Verdict::Works(ServiceFound {
                name: "Dota arena".to_owned(),
            }),
        ),
        row(
            "arena.example.org:4460",
            ADDED,
            false,
            true,
            Verdict::Failed(ServiceFault::HostNotFound),
        ),
        row(
            "match.example.net",
            "wss://match.example.net",
            false,
            true,
            Verdict::Checking,
        ),
    ];
    let (typed, shown) = match name {
        "service-typing" => (
            TypedService::Address("wss://play.example.org".to_owned()),
            "https://play.example.org/lobby/",
        ),
        "service-refused" => (
            TypedService::Refused(
                service_words::address_fault(bota_radovid::AddressFault::Plain).to_owned(),
            ),
            "ws://play.example.org",
        ),
        _ => (TypedService::Nothing, ""),
    };
    let mut view = form(
        Page::Service,
        FormKind::Service,
        vec![field("Address", shown, true)],
    );
    let field = field("Address", shown, true);
    view.body = PageView::Services(Box::new(ServicesView { rows, field, typed }));
    view
}
