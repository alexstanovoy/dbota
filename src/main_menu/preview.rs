//! Debug: every screen of the menu with made-up contents, drawn without a
//! service (`--menu-preview NAME`), for screenshots of how it looks.

use bota_radovid::proto::{AccountId, LobbyId, MatchId, ProposalId, QueueEntryId, RulesetId};

use super::core::{GameKind, LocalRules, Page, Status};
use super::form::FormKind;
use super::view::{
    AudienceRow, ConfirmView, Connection, FieldView, FinishedRow, GameRow, LobbyRow, LobbyView,
    MemberRow, MenuView, ModeRow, OfferBox, PageView, PasswordView, ResultView, RulesetRow,
    SearchRow, SeatRow, SettingsView, Side, SlotRow, TeamRow, WaitRow,
};

/// The names `--menu-preview` takes.
pub const NAMES: [&str; 51] = [
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
    "lobby-password",
    "lobby",
    "lobby-owner",
    "result",
    "direct",
    "quit",
    "dashboard",
    "dashboard-replay",
    "reconnect",
    "lost",
    "dire",
    "play",
    "play-bots",
    "play-searching",
    "play-tooltip",
    "play-offline",
    "main-searching",
    "match-decline",
    "loading",
    "play-full",
    "play-cooldown",
    "play-searching-bots-overloaded",
    "profile",
    "profile-offline",
    "profile-details-unavailable",
    "profile-avatar",
    "profile-avatar-unavailable",
    "profile-empty",
    "heroes",
    "hero",
    "hero-pudge",
    "leaderboard",
    "leaderboard-bots",
    "leaderboard-allpick",
    "leaderboard-empty",
    "leaderboard-unavailable",
    "home-local",
    "home-defeat",
    "home-empty",
    "home-offline",
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
        beneath: page,
        body: PageView::Form {
            kind,
            fields,
            busy: false,
            plain_host: None,
            rules: (page == Page::LocalLobby).then_some(LocalRules::Full),
            hosted_at: None,
        },
        play: Default::default(),
        queue_full: false,
        cooldown_left: None,
        account: None,
        player: "mira".to_owned(),
        connection: Connection::Offline,
        side: Side::Radiant,
        status: None,
        searches: Vec::new(),
        waits: Vec::new(),
        offer: None,
        game: None,
        reconnect: false,
        can_leave_game: false,
        confirm: None,
        password: None,
        can_quit: true,
        can_host: !cfg!(target_arch = "wasm32"),
        host_cheats: false,
    }
}

pub(super) fn logged_in(page: Page, body: PageView) -> MenuView {
    MenuView {
        page,
        beneath: page,
        body,
        play: Default::default(),
        queue_full: false,
        cooldown_left: None,
        account: Some("alice".to_owned()),
        player: "alice".to_owned(),
        connection: Connection::Online("play.example.org".to_owned()),
        side: Side::Radiant,
        status: None,
        searches: Vec::new(),
        waits: Vec::new(),
        offer: None,
        game: None,
        reconnect: false,
        can_leave_game: false,
        confirm: None,
        password: None,
        can_quit: true,
        can_host: !cfg!(target_arch = "wasm32"),
        host_cheats: false,
    }
}

fn home() -> PageView {
    PageView::Home {
        matches: super::page_previews::matches(false).into(),
        hero: Some(bota_proto::HeroId::SHADOW_FIEND),
        rulesets: vec![
            RulesetRow {
                id: RulesetId("solo_mid".to_owned()),
                name: "Solo Mid".to_owned(),
                teams: "1v1".to_owned(),
                rated: true,
                description: super::play::description(&RulesetId("solo_mid".into())),
            },
            RulesetRow {
                id: RulesetId("all_pick".to_owned()),
                name: "All Pick".to_owned(),
                teams: "5v5".to_owned(),
                rated: true,
                description: super::play::description(&RulesetId("all_pick".into())),
            },
        ],
        in_flight: (1, Some(bota_radovid::proto::AccountGameLimit::Limited(2))),
        cooldown: None,
    }
}

fn search(overloaded: bool) -> SearchRow {
    SearchRow {
        entry: QueueEntryId(3),
        ruleset: "Solo Mid · ranked".to_owned(),
        waited: "0:42".to_owned(),
        overloaded,
        games_waiting: if overloaded { 3 } else { 0 },
        hint: None,
        offered: false,
    }
}

fn offer(answered: bool) -> OfferBox {
    OfferBox {
        proposal: ProposalId(7),
        ruleset: "Solo Mid · ranked".to_owned(),
        left_ms: if answered { 17_000 } else { 24_000 },
        left_permille: Some(700),
        accepted: if answered { 7 } else { 0 },
        of: 10,
        answered,
        decline_cooldown_ms: Some(300_000),
    }
}

/// The view named `name`, if it is one.
pub fn view(name: &str) -> Option<MenuView> {
    if crate::postgame::preview::NAMES.contains(&name) {
        return Some(logged_in(
            Page::MatchHistory,
            PageView::MatchHistory(super::history_page::preview()),
        ));
    }

    if let Some(view) = super::page_previews::view(name) {
        return Some(view);
    }
    Some(match name {
        "service" => preview_service(),
        "login" => preview_login(),
        "register" => preview_register(),
        "home" => preview_home(),
        "searching" | "main-searching" | "overloaded" => preview_searching(name),
        "match-found" | "accepted" | "match-decline" => preview_match_found(name),
        "play"
        | "play-bots"
        | "play-searching"
        | "play-tooltip"
        | "play-offline"
        | "play-full"
        | "play-cooldown"
        | "play-searching-bots-overloaded" => preview_play(name),
        "loading" => preview_loading(),
        "waiting" => preview_waiting(),
        "my-games" => preview_my_games(),
        "lobbies" => preview_lobbies(),
        "lobby-password" => preview_lobby_password(),
        "lobby" => preview_lobby(),
        "lobby-owner" => preview_lobby_owner(),
        "result" => preview_result(),
        "direct" => preview_direct(),
        "quit" => preview_quit(),
        "dashboard" | "dashboard-replay" => preview_dashboard(name),
        "reconnect" => preview_reconnect(),
        "lost" => preview_lost(),
        "dire" => preview_dire(),
        _ => return None,
    })
}

fn loading() -> crate::loading::Details {
    let names = [
        "mira#2207",
        "kestrel#118",
        "Vesper",
        "Kiln",
        "owlbear#90",
        "tamsin#3311",
        "Marrow",
        "Thistle",
        "lowtide#7",
        "Cinder",
    ];
    crate::loading::Details {
        match_id: Some(10432),
        mode: "All Pick · Unranked".into(),
        seats: names
            .into_iter()
            .enumerate()
            .map(|(at, name)| crate::loading::Row {
                slot: at as u8,
                team: if at < 5 {
                    bota_proto::Team::Radiant
                } else {
                    bota_proto::Team::Dire
                },
                name: name.into(),
                bot: false,
                connected: at != 2 && at != 7,
                me: at == 0,
            })
            .collect(),
    }
}

/// The loading preview uses the same chat renderer as a live connection.
pub(crate) fn loading_chat() -> [(&'static str, bevy::prelude::Color, &'static str); 2] {
    [
        ("kestrel#118", crate::theme::RADIANT_TEXT, "gl hf"),
        ("tamsin#3311", crate::theme::DIRE_TEXT, "glhf"),
    ]
}

fn preview_service() -> MenuView {
    form(
        Page::Service,
        FormKind::Service,
        vec![field("Service", "wss://play.example.org:4460", true)],
    )
}

fn preview_login() -> MenuView {
    form(
        Page::Login,
        FormKind::Login,
        vec![
            field("Login", "alice", false),
            field("Password", "••••••••••", true),
        ],
    )
}

fn preview_register() -> MenuView {
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

fn preview_home() -> MenuView {
    logged_in(Page::Home, home())
}

fn preview_searching(name: &str) -> MenuView {
    let mut view = logged_in(Page::Home, home());
    view.searches = vec![SearchRow {
        ruleset: "All Pick".into(),
        ..search(name == "overloaded")
    }];
    view
}

fn preview_match_found(name: &str) -> MenuView {
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
    view.offer = Some(OfferBox {
        ruleset: "All Pick · Ranked".to_owned(),
        ..offer(name == "accepted")
    });
    if name == "match-decline" {
        view.play.info = Some(super::play::PlayInfo::Decline);
    }
    view
}

fn preview_play(name: &str) -> MenuView {
    let mut view = logged_in(Page::Home, home());
    view.play.open = true;
    view.play.bots = name == "play-bots" || name == "play-searching-bots-overloaded";
    if name == "play-searching" || name == "play-searching-bots-overloaded" {
        view.searches = vec![SearchRow {
            ruleset: if view.play.bots {
                "Solo Mid · ranked · with bots".into()
            } else {
                "Solo Mid · ranked".into()
            },
            ..search(name == "play-searching-bots-overloaded")
        }];
    }
    if name == "play-tooltip" {
        view.play.info = Some(super::play::PlayInfo::Queue(RulesetId("solo_mid".into())));
    }
    if name == "play-offline" {
        view.account = None;
        view.player = super::DEFAULT_NAME.into();
        view.connection = Connection::Offline;
    }
    if name == "play-full" {
        view.queue_full = true;
    }
    if name == "play-cooldown" {
        view.cooldown_left = Some("3:42".into());
        if let PageView::Home { cooldown, .. } = &mut view.body {
            *cooldown = Some("3:42".into());
        }
    }
    view
}

fn preview_loading() -> MenuView {
    let mut view = logged_in(
        Page::Game,
        PageView::Loading {
            details: loading(),
            progress_permille: 640,
            elapsed_secs: 18,
        },
    );
    view.can_quit = false;
    view
}

fn preview_waiting() -> MenuView {
    let mut view = logged_in(Page::Home, home());
    view.waits = vec![WaitRow {
        match_id: MatchId(41),
        ahead: 2,
        overloaded: true,
    }];
    view
}

fn preview_my_games() -> MenuView {
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

fn preview_lobbies() -> MenuView {
    logged_in(
        Page::Lobbies,
        PageView::Lobbies {
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
                field("Lobby name", "", false),
                field("Password (optional)", "", false),
                field("Search lobbies", "friends", true),
            ],
        },
    )
}

fn preview_lobby_password() -> MenuView {
    let mut view = preview_lobbies();
    view.password = Some(PasswordView {
        lobby: "friends only".to_owned(),
        field: field("Lobby password", "", true),
        error: Some("Wrong password: try again".to_owned()),
    });
    view
}

fn preview_lobby() -> MenuView {
    logged_in(
        Page::Lobby,
        PageView::Lobby(LobbyView {
            name: "friends only".to_owned(),
            locked: true,
            teams: vec![preview_lobby_radiant(), preview_lobby_dire()],
            spectators: Vec::new(),
            owner: false,
            moving: None,
            settings: preview_lobby_settings(),
            fields: Vec::new(),
            me_ready: false,
            waiting: None,
            starts_short: false,
            starting_in_secs: Some(3),
        }),
    )
}

fn preview_lobby_owner() -> MenuView {
    logged_in(
        Page::Lobby,
        PageView::Lobby(LobbyView {
            name: "friends only".to_owned(),
            locked: true,
            teams: vec![preview_lobby_owner_radiant(), preview_lobby_owner_dire()],
            spectators: Vec::new(),
            owner: true,
            moving: Some("alice".to_owned()),
            settings: preview_lobby_owner_settings(),
            fields: vec![field("Password (empty removes it)", "", false)],
            me_ready: false,
            waiting: None,
            starts_short: false,
            starting_in_secs: Some(3),
        }),
    )
}

fn preview_result() -> MenuView {
    logged_in(
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
    )
}

fn preview_direct() -> MenuView {
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

fn preview_quit() -> MenuView {
    let mut view = logged_in(Page::Home, home());
    view.confirm = Some(ConfirmView {
        title: "Confirm Quit",
        text: "Do you really wish to quit dbota?",
    });
    view
}

fn preview_dashboard(name: &str) -> MenuView {
    let mut view = logged_in(Page::Home, home());
    view.game = Some(if name == "dashboard" {
        GameKind::Play
    } else {
        GameKind::Replay
    });
    view.can_leave_game = name == "dashboard";
    view
}

fn preview_reconnect() -> MenuView {
    let mut view = logged_in(Page::Home, home());
    view.reconnect = true;
    view
}

fn preview_lost() -> MenuView {
    let mut view = logged_in(Page::Home, home());
    view.connection = Connection::Lost("play.example.org".to_owned());
    view
}

fn preview_dire() -> MenuView {
    let mut view = logged_in(Page::Home, home());
    view.side = Side::Dire;
    view
}

fn preview_lobby_owner_dire() -> TeamRow {
    TeamRow {
        name: "DIRE".to_owned(),
        slots: vec![
            SlotRow {
                slot: 2,
                who: Some(MemberRow {
                    account: AccountId(12),
                    name: "alice".to_owned(),
                    ready: false,
                    me: false,
                }),
            },
            SlotRow { slot: 3, who: None },
        ],
    }
}

fn preview_lobby_owner_radiant() -> TeamRow {
    TeamRow {
        name: "RADIANT".to_owned(),
        slots: vec![
            SlotRow {
                slot: 0,
                who: Some(MemberRow {
                    account: AccountId(9),
                    name: "carol".to_owned(),
                    ready: true,
                    me: true,
                }),
            },
            SlotRow { slot: 1, who: None },
        ],
    }
}

fn preview_lobby_dire() -> TeamRow {
    TeamRow {
        name: "DIRE".to_owned(),
        slots: vec![
            SlotRow {
                slot: 2,
                who: Some(MemberRow {
                    account: AccountId(12),
                    name: "alice".to_owned(),
                    ready: false,
                    me: true,
                }),
            },
            SlotRow { slot: 3, who: None },
        ],
    }
}

fn preview_lobby_radiant() -> TeamRow {
    TeamRow {
        name: "RADIANT".to_owned(),
        slots: vec![
            SlotRow {
                slot: 0,
                who: Some(MemberRow {
                    account: AccountId(9),
                    name: "carol".to_owned(),
                    ready: true,
                    me: false,
                }),
            },
            SlotRow { slot: 1, who: None },
        ],
    }
}

fn preview_lobby_owner_settings() -> SettingsView {
    SettingsView {
        mode: "Practice".to_owned(),
        audience: "Humans and Bots",
        locked: true,
        modes: vec![
            ModeRow {
                id: RulesetId("squad".to_owned()),
                name: "Squad".to_owned(),
                chosen: false,
            },
            ModeRow {
                id: RulesetId("practice".to_owned()),
                name: "Practice".to_owned(),
                chosen: true,
            },
        ],
        audiences: vec![
            AudienceRow {
                audience: bota_radovid::proto::Audience::Everyone,
                name: "Humans and Bots",
                chosen: true,
            },
            AudienceRow {
                audience: bota_radovid::proto::Audience::HumansOnly,
                name: "Humans",
                chosen: false,
            },
        ],
        cheats: Some(false),
        editable: true,
    }
}

fn preview_lobby_settings() -> SettingsView {
    SettingsView {
        mode: "Practice".to_owned(),
        audience: "Humans and Bots",
        locked: true,
        modes: Vec::new(),
        audiences: Vec::new(),
        cheats: Some(true),
        editable: false,
    }
}
