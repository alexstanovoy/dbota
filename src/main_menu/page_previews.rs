//! Explicit screenshot fixtures only. Normal menu views never consume these records.
use super::{
    account_page::Rating,
    avatar,
    core::Page,
    history::{Match, ResultKind},
    leaderboard::{Board, Mode, Players, Query, Row},
    page_time::Clock,
    view::{Connection, FieldView, MenuView, PageView},
};
use bota_proto::HeroId;

type MatchSeed = (HeroId, bool, u32, (u16, u16, u16), i32, &'static str, u64);
const OFFLINE_MATCHES: &[MatchSeed] = &[
    (
        HeroId::SHADOW_FIEND,
        true,
        2292,
        (11, 3, 9),
        24800,
        "all_pick",
        7200,
    ),
    (
        HeroId::SHADOW_FIEND,
        true,
        708,
        (9, 1, 2),
        8100,
        "solo_mid",
        84300,
    ),
    (
        HeroId::PUDGE,
        true,
        1330,
        (5, 4, 8),
        11200,
        "local",
        2 * 86400 + 2280,
    ),
    (
        HeroId::SHADOW_FIEND,
        true,
        595,
        (2, 1, 0),
        5900,
        "solo_mid",
        4 * 86400 - 5400,
    ),
];
const SERVICE_MATCHES: &[MatchSeed] = &[
    (
        HeroId::SHADOW_FIEND,
        true,
        2292,
        (11, 3, 9),
        24800,
        "all_pick",
        7200,
    ),
    (
        HeroId::PUDGE,
        false,
        2497,
        (3, 9, 12),
        9600,
        "all_pick",
        20280,
    ),
    (
        HeroId::SHADOW_FIEND,
        true,
        708,
        (9, 1, 2),
        8100,
        "solo_mid",
        84300,
    ),
    (
        HeroId::PUDGE,
        true,
        1330,
        (5, 4, 8),
        11200,
        "local",
        2 * 86400 + 2280,
    ),
    (
        HeroId::SHADOW_FIEND,
        false,
        860,
        (1, 2, 0),
        6400,
        "solo_mid",
        3 * 86400 + 4200,
    ),
    (
        HeroId::SHADOW_FIEND,
        true,
        595,
        (2, 1, 0),
        5900,
        "solo_mid",
        4 * 86400 - 5400,
    ),
];

pub fn matches(offline: bool) -> Vec<Match> {
    let now = Clock::PREVIEW;
    let entries = if offline {
        OFFLINE_MATCHES
    } else {
        SERVICE_MATCHES
    };
    let mut matches: Vec<_> = entries
        .iter()
        .copied()
        .enumerate()
        .map(
            |(index, (hero, won, seconds, kda, worth, mode, age))| Match {
                file: format!("preview-{index}.brp"),
                when: now - age,
                mode: mode.to_owned(),
                hero: Some(hero),
                result: if won {
                    ResultKind::Won
                } else {
                    ResultKind::Lost
                },
                seconds: Some(seconds),
                kda: Some(kda),
                net_worth: Some(worth),
                watchable: index != 4,
                details_available: true,
            },
        )
        .collect();
    let targets = if offline {
        [(HeroId::SHADOW_FIEND, 21, 14), (HeroId::PUDGE, 11, 6)]
    } else {
        [(HeroId::SHADOW_FIEND, 29, 18), (HeroId::PUDGE, 18, 9)]
    };
    for (hero, games, wins) in targets {
        let existing = matches
            .iter()
            .filter(|game| game.hero == Some(hero))
            .count();
        let existing_wins = matches
            .iter()
            .filter(|game| game.hero == Some(hero) && game.result == ResultKind::Won)
            .count();
        for index in 0..games - existing {
            matches.push(Match {
                file: format!("preview-{}-{index}.brp", hero.0),
                when: now - (5 + index as u64) * 86400,
                mode: "solo_mid".to_owned(),
                hero: Some(hero),
                result: if index < wins - existing_wins {
                    ResultKind::Won
                } else {
                    ResultKind::Lost
                },
                seconds: Some(600),
                kda: Some((2, 1, 0)),
                net_worth: Some(5000),
                watchable: true,
                details_available: true,
            });
        }
    }
    matches
}

pub fn view(name: &str) -> Option<MenuView> {
    let page = page_of(name)?;
    let mut view = super::preview::logged_in(page, body(page, name));
    decorate(&mut view, name);
    Some(view)
}

fn page_of(name: &str) -> Option<Page> {
    let page = match name {
        "home-local" | "home-offline" | "home-defeat" | "home-empty" => Page::Home,
        "profile_other" | "profile_other_unavailable" => Page::PlayerProfile(42),
        "profile"
        | "profile-offline"
        | "profile-avatar"
        | "profile-avatar-unavailable"
        | "profile-empty"
        | "profile-details-unavailable" => Page::Profile,
        "heroes" => Page::Heroes,
        "hero" => Page::Hero(HeroId::SHADOW_FIEND),
        "hero-pudge" => Page::Hero(HeroId::PUDGE),
        "leaderboard"
        | "leaderboard-bots"
        | "leaderboard-allpick"
        | "leaderboard-empty"
        | "leaderboard-unavailable" => Page::Leaderboard,
        _ => return None,
    };
    Some(page)
}

fn body(page: Page, name: &str) -> PageView {
    match page {
        Page::Home => PageView::Home {
            matches: matches(true).into(),
            hero: Some(HeroId::SHADOW_FIEND),
            rulesets: Vec::new(),
            in_flight: (0, None),
            cooldown: None,
        },
        Page::Profile | Page::PlayerProfile(_) => profile_fixture(name),
        Page::Heroes => PageView::Heroes,
        Page::Hero(hero) => PageView::Hero {
            hero,
            record: if hero == HeroId::SHADOW_FIEND {
                "Your matches: 29 · 62% won"
            } else {
                "Your matches: 18 · 50% won"
            }
            .to_owned(),
            demo: true,
        },
        Page::Leaderboard => ladder_fixture(name),
        _ => PageView::Hidden,
    }
}

fn profile_fixture(name: &str) -> PageView {
    let mut avatar = avatar::View {
        open: name.starts_with("profile-avatar"),
        available: name == "profile-avatar",
        ..Default::default()
    };
    if name == "profile-avatar-unavailable" {
        avatar.available = false;
        avatar.notice = Some("Avatar changes are not available on this service yet.".to_owned());
    }
    PageView::Profile {
        mode_names: Vec::new(),
        read_only: name.starts_with("profile_other"),
        notice: (name == "profile_other_unavailable")
            .then(|| "This player's profile is unavailable".into()),
        name: if name == "profile_other_unavailable" {
            "Player #42"
        } else {
            "mira#2207"
        }
        .to_owned(),
        ratings: if name == "profile-empty" || name == "profile_other_unavailable" {
            Vec::new()
        } else {
            vec![
                Rating {
                    mode: "Solo Mid".to_owned(),
                    rating: Some("1,524".to_owned()),
                    rank: Some(12),
                    games: 41,
                },
                Rating {
                    mode: "All Pick".to_owned(),
                    rating: Some("1,302".to_owned()),
                    rank: Some(412),
                    games: 9,
                },
            ]
        },
        matches: if name == "profile-empty" || name == "profile_other_unavailable" {
            Vec::new()
        } else if name == "profile_other" {
            matches(false)
                .into_iter()
                .take(6)
                .map(|mut row| {
                    row.hero = None;
                    row.kda = None;
                    row.net_worth = None;
                    row.watchable = false;
                    row.details_available = false;
                    row
                })
                .collect()
        } else {
            matches(name == "profile-offline")
        }
        .into(),
        avatar,
    }
}

fn ladder_fixture(name: &str) -> PageView {
    let mut board = board(name);
    if name == "leaderboard-empty" {
        board.rows.clear();
        board.total = 0;
        board.own = None;
    }
    if name == "leaderboard-unavailable" {
        board.disconnected();
    }
    PageView::Leaderboard {
        board: Box::new(board),
        field: FieldView {
            label: "Find a nickname",
            shown: String::new(),
            focused: false,
        },
    }
}

fn decorate(view: &mut MenuView, name: &str) {
    if name == "profile-details-unavailable"
        && let PageView::Profile { matches, .. } = &mut view.body
    {
        for row in std::sync::Arc::make_mut(matches) {
            row.details_available = false;
            row.watchable = false;
        }
    }
    if name == "home-empty"
        && let PageView::Home { matches, hero, .. } = &mut view.body
    {
        std::sync::Arc::make_mut(matches).clear();
        *hero = None;
        view.account = None;
        view.player = "mira".to_owned();
        view.connection = Connection::Offline;
    }
    if name == "home-defeat"
        && let PageView::Home { matches, .. } = &mut view.body
    {
        std::sync::Arc::make_mut(matches)[0].result = ResultKind::Lost;
    }
    if name == "home-local" {
        view.account = None;
        view.player = "mira".to_owned();
        view.connection = Connection::Offline;
    }
    if name == "profile-offline" || name == "home-offline" {
        view.connection = Connection::Lost("[SERVICE]".to_owned());
    }
}

fn board(name: &str) -> Board {
    let bots = name == "leaderboard-bots";
    let allpick = name == "leaderboard-allpick";
    let kind = if bots { Players::Bots } else { Players::Humans };
    let mut board = Board::default();
    board.modes = vec![
        Mode {
            id: "solo_mid".to_owned(),
            name: "Solo Mid".to_owned(),
        },
        Mode {
            id: "all_pick".to_owned(),
            name: "All Pick".to_owned(),
        },
    ];
    board.select(Query {
        mode: if allpick { "all_pick" } else { "solo_mid" }.to_owned(),
        players: kind,
        nickname: String::new(),
    });
    let (generation, _) = board.request().expect("first fixture request");
    let mut rows = ladder_rows(bots, allpick, kind);
    let own = own_row(&mut rows, bots, allpick);
    pad_rows(&mut rows, bots, allpick, kind);
    board.receive(generation, Some((rows, own)));
    board.total = 300;
    board
}

fn ladder_rows(bots: bool, allpick: bool, kind: Players) -> Vec<Row> {
    let names = if bots {
        vec![
            "Ironbark#902",
            "Ironbark#903",
            "lantern#77",
            "Ironbark#899",
            "moth#310",
            "lantern#76",
            "sable#12",
            "moth#309",
            "quill#5",
            "Ironbark#880",
            "sable#11",
            "lantern#70",
            "quill#4",
        ]
    } else {
        vec![
            "ember#3",
            "tamsin#3311",
            "quartz#55",
            "lowtide#7",
            "kestrel#118",
            "hookline#4",
            "owlbear#90",
            "vale#201",
            "brine#62",
            "sorrel#9",
            "nlm#771",
            "mira#2207",
            "pike#88",
        ]
    };
    let human = [
        1811, 1760, 1702, 1655, 1610, 1588, 1571, 1560, 1541, 1533, 1529, 1524, 1519,
    ];
    let bot = [
        2104, 2031, 1987, 1950, 1902, 1874, 1840, 1801, 1777, 1750, 1726, 1700, 1688,
    ];
    let games = [77, 64, 90, 51, 38, 120, 44, 29, 33, 70, 26, 41, 63];
    names
        .into_iter()
        .enumerate()
        .map(|(index, name)| Row {
            account: if name == "mira#2207" {
                2207
            } else {
                index as u64 + 1
            },
            players: kind,
            rank: index as u32 + 1,
            nickname: name.to_owned(),
            version: bots.then(|| {
                [
                    "v12", "v11", "v4.2", "v10", "v0.9", "v4.1", "v2", "v0.8", "v1", "v9", "v1.4",
                    "v3", "v0.7",
                ][index]
                    .to_owned()
            }),
            games: games[index],
            rating: super::leaderboard::rating(f64::from(if bots {
                bot[index]
            } else {
                human[index] + if allpick { 140 } else { 0 }
            }))
            .unwrap(),
        })
        .collect()
}

fn own_row(rows: &mut Vec<Row>, bots: bool, allpick: bool) -> Option<Row> {
    if bots {
        None
    } else if allpick {
        rows.retain(|row| row.account != 2207);
        for (at, row) in rows.iter_mut().enumerate() {
            row.rank = at as u32 + 1;
        }
        Some(Row {
            account: 2207,
            players: Players::Humans,
            rank: 412,
            nickname: "mira#2207".to_owned(),
            version: None,
            games: 9,
            rating: "1,302".to_owned(),
        })
    } else {
        rows.iter().find(|row| row.account == 2207).cloned()
    }
}

fn pad_rows(rows: &mut Vec<Row>, bots: bool, allpick: bool, kind: Players) {
    // A full loaded ladder makes the preview exercise clipping and scrolling, not just
    // a short fixture whose entire contents happen to fit the window.
    for rank in rows.len() + 1..=super::leaderboard::TOP {
        rows.push(Row {
            account: 10000 + rank as u64,
            players: kind,
            rank: rank as u32,
            nickname: format!("player{rank}"),
            version: bots.then(|| "v1".to_owned()),
            games: 20,
            rating: (if bots {
                1670
            } else if allpick {
                1650
            } else {
                1500
            } - 2 * rank as i32)
                .to_string(),
        });
    }
}
