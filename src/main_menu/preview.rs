//! Debug: every screen of the menu with made-up contents, drawn without a
//! service (`--menu-preview NAME`), for screenshots of how it looks.

use bota_radovid::proto::{LobbyId, MatchId, ProposalId, QueueEntryId, RulesetId};

use super::core::{GameKind, Page, SeatFill, Status};
use super::form::FormKind;
use super::view::{
    ConfirmView, FieldView, FinishedRow, GameRow, LobbyRow, LobbyView, MenuView, ModeRow, OfferBox,
    PageView, ResultView, RulesetRow, SearchRow, SeatRow, WaitRow,
};

/// The names `--menu-preview` takes.
pub const NAMES: [&str; 18] = [
    "service",
    "login",
    "register",
    "home",
    "searching",
    "overloaded",
    "match-found",
    "accepted",
    "waiting",
    "my-games",
    "lobbies",
    "lobby",
    "result",
    "direct",
    "quit",
    "dashboard",
    "dashboard-replay",
    "reconnect",
];

fn field(label: &'static str, shown: &str, focused: bool) -> FieldView {
    FieldView {
        label,
        shown: shown.to_owned(),
        focused,
    }
}

fn form(page: Page, kind: FormKind, fields: Vec<FieldView>) -> MenuView {
    MenuView {
        page,
        body: PageView::Form {
            kind,
            fields,
            busy: false,
            plain_host: None,
            fill: (page == Page::LocalLobby).then_some((SeatFill::Open, false)),
            hosted_at: None,
        },
        account: None,
        status: None,
        searches: Vec::new(),
        waits: Vec::new(),
        offer: None,
        game: None,
        reconnect: false,
        can_leave_game: false,
        confirm: None,
        can_quit: true,
        host_cheats: false,
    }
}

fn logged_in(page: Page, body: PageView) -> MenuView {
    MenuView {
        page,
        body,
        account: Some("alice#12".to_owned()),
        status: None,
        searches: Vec::new(),
        waits: Vec::new(),
        offer: None,
        game: None,
        reconnect: false,
        can_leave_game: false,
        confirm: None,
        can_quit: true,
        host_cheats: false,
    }
}

fn home() -> PageView {
    PageView::Home {
        rulesets: vec![RulesetRow {
            id: RulesetId("solo_mid".to_owned()),
            name: "1x1 Mid Only".to_owned(),
            teams: "1v1".to_owned(),
            rated: true,
            audience: "Humans and Bots",
        }],
        in_flight: (1, 2),
        cooldown: None,
    }
}

fn search(overloaded: bool) -> SearchRow {
    SearchRow {
        entry: QueueEntryId(3),
        ruleset: "1x1 Mid Only".to_owned(),
        waited: "1:07".to_owned(),
        position: 2,
        waiting: 5,
        overloaded,
        games_waiting: if overloaded { 3 } else { 0 },
        offered: false,
    }
}

fn offer(answered: bool) -> OfferBox {
    OfferBox {
        proposal: ProposalId(7),
        ruleset: "1x1 Mid Only".to_owned(),
        left_secs: if answered { 17 } else { 24 },
        left_permille: if answered { 566 } else { 800 },
        accepted: u8::from(answered),
        of: 2,
        answered,
        which: Some("Search 1 of 2".to_owned()),
    }
}

/// The view named `name`, if it is one.
pub fn view(name: &str) -> Option<MenuView> {
    Some(match name {
        "service" => form(
            Page::Service,
            FormKind::Service,
            vec![field("Service", "wss://play.example.org:4460", true)],
        ),
        "login" => form(
            Page::Login,
            FormKind::Login,
            vec![
                field("Login", "alice", false),
                field("Password", "••••••••••", true),
            ],
        ),
        "register" => {
            let mut view = form(
                Page::Register,
                FormKind::Register,
                vec![
                    field("Login", "alice", false),
                    field("Password", "•••••••••", false),
                    field("Password again", "••••", true),
                ],
            );
            view.status = Some(Status {
                text: "The two passwords differ".to_owned(),
                error: true,
            });
            view
        }
        "home" => logged_in(Page::Home, home()),
        "searching" | "overloaded" => {
            let mut view = logged_in(Page::Home, home());
            view.searches = vec![search(name == "overloaded")];
            view
        }
        "match-found" | "accepted" => {
            let mut view = logged_in(Page::Home, home());
            view.searches = vec![
                SearchRow {
                    offered: true,
                    ..search(false)
                },
                SearchRow {
                    entry: QueueEntryId(4),
                    waited: "0:12".to_owned(),
                    ..search(false)
                },
            ];
            view.offer = Some(offer(name == "accepted"));
            view
        }
        "waiting" => {
            let mut view = logged_in(Page::Home, home());
            view.waits = vec![WaitRow {
                match_id: MatchId(41),
                ahead: 2,
                overloaded: true,
            }];
            view
        }
        "my-games" => {
            let mut view = logged_in(
                Page::MyGames,
                PageView::MyGames {
                    games: vec![GameRow {
                        match_id: MatchId(40),
                        label: "Match 40".to_owned(),
                        ready: true,
                    }],
                    finished: vec![FinishedRow {
                        match_id: MatchId(39),
                        headline: "VICTORY".to_owned(),
                        won: Some(true),
                        detail: "Match 39 · 14:21 · rated".to_owned(),
                        rating: "1662 (+162)".to_owned(),
                        replay: true,
                    }],
                    cooldown: Some("You can search again in 1:42".to_owned()),
                },
            );
            view.searches = vec![search(false)];
            view
        }
        "lobbies" => logged_in(
            Page::Lobbies,
            PageView::Lobbies {
                modes: vec![ModeRow {
                    id: RulesetId("solo_mid".to_owned()),
                    name: "1x1 Mid Only".to_owned(),
                    chosen: true,
                }],
                list: vec![
                    LobbyRow {
                        id: LobbyId(1),
                        name: "mid practice".to_owned(),
                        owner: "bob#7".to_owned(),
                        locked: false,
                        members: 1,
                        seats: 2,
                    },
                    LobbyRow {
                        id: LobbyId(2),
                        name: "friends only".to_owned(),
                        owner: "carol#9".to_owned(),
                        locked: true,
                        members: 1,
                        seats: 2,
                    },
                ],
                fields: vec![
                    field("Lobby name", "friends only", false),
                    field("Password (optional)", "••••••", true),
                ],
            },
        ),
        "lobby" => logged_in(
            Page::Lobby,
            PageView::Lobby(LobbyView {
                name: "friends only".to_owned(),
                locked: true,
                members: vec![
                    ("carol#9".to_owned(), 0, true),
                    ("alice#12".to_owned(), 1, true),
                ],
                waiting_approval: 0,
                me_ready: true,
                waiting: None,
                starts_short: false,
                starting_in_secs: Some(3),
            }),
        ),
        "result" => logged_in(
            Page::Result,
            PageView::Result(ResultView {
                headline: "VICTORY".to_owned(),
                won: Some(true),
                detail: "Match 40 · 14:21 · rated".to_owned(),
                seats: vec![
                    SeatRow {
                        name: "alice#12".to_owned(),
                        team: 0,
                        hero: "Shadow Fiend".to_owned(),
                        rating: "1512 (+12)".to_owned(),
                        mine: true,
                    },
                    SeatRow {
                        name: "bob#7".to_owned(),
                        team: 1,
                        hero: "Pudge".to_owned(),
                        rating: "1488 (-12)".to_owned(),
                        mine: false,
                    },
                ],
            }),
        ),
        "direct" => {
            let mut view = form(
                Page::LocalLobby,
                FormKind::Host,
                vec![
                    field("Port", "4455", false),
                    field("Players", "2", false),
                    field("Address", "192.168.1.20:4455", true),
                ],
            );
            if let PageView::Form { plain_host, .. } = &mut view.body {
                *plain_host = Some("192.168.1.20".to_owned());
            }
            view
        }
        "quit" => {
            let mut view = logged_in(Page::Home, home());
            view.confirm = Some(ConfirmView {
                title: "Confirm Quit",
                text: "Do you really wish to quit dbota?",
            });
            view
        }
        "dashboard" | "dashboard-replay" => {
            let mut view = logged_in(Page::Home, home());
            view.game = Some(if name == "dashboard" {
                GameKind::Play
            } else {
                GameKind::Replay
            });
            view
        }
        "reconnect" => {
            let mut view = logged_in(Page::Home, home());
            view.reconnect = true;
            view
        }
        _ => return None,
    })
}
