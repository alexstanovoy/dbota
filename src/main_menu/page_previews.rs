//! Explicit screenshot fixtures only. Normal menu views never consume these records.
use super::avatar_image::LIST_SIDE;
use super::{
    account_page::Rating,
    avatar,
    core::{DemoOffer, Page},
    history::{Match, ResultKind},
    leaderboard::{Board, Mode, Players, Query, Row},
    page_time::Clock,
    view::{Connection, FieldView, MenuView, PageView},
};
use bota_proto::HeroId;
use dbota_import::thumbnail::Pixels;
use std::sync::Arc;

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
                players: Vec::new(),
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
                players: Vec::new(),
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
        "profile_other" | "profile_other_unavailable" | "profile_other-picture" => {
            Page::PlayerProfile(42)
        }
        "profile"
        | "profile-offline"
        | "profile-picture"
        | "profile-avatar"
        | "profile-avatar-unavailable"
        | "profile-empty"
        | "profile-details-unavailable" => Page::Profile,
        "heroes" => Page::Heroes,
        "hero" => Page::Hero(HeroId::SHADOW_FIEND),
        "hero-pudge" | "hero-demo-signin" | "hero-demo-missing" => Page::Hero(HeroId::PUDGE),
        "demo-hero" | "demo-hero-many" => Page::DemoHero,
        "leaderboard"
        | "leaderboard_solo"
        | "leaderboard-bots"
        | "leaderboard-allpick"
        | "leaderboard-empty"
        | "leaderboard-unavailable"
        | "leaderboard-pictures" => Page::Leaderboard,
        _ => return None,
    };
    Some(page)
}

/// Heroes on the Demo Hero preview that wraps: more cards than a row of the page holds.
const ROSTER_THAT_WRAPS: usize = 8;

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
            demo: match name {
                "hero-demo-signin" => DemoOffer::SignIn,
                "hero-demo-missing" => DemoOffer::NoDemo,
                _ => DemoOffer::Ready,
            },
        },
        Page::DemoHero => PageView::DemoHero {
            heroes: bota_core::HEROES
                .iter()
                // A roster of eight, which the page's width cannot hold in one row.
                .cycle()
                .take(if name == "demo-hero-many" {
                    ROSTER_THAT_WRAPS
                } else {
                    bota_core::HEROES.len()
                })
                .map(|hero| (hero.id.0, hero.key.to_owned()))
                .collect(),
            offer: DemoOffer::Ready,
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
    if name.ends_with("-picture") {
        avatar.current = Some(Arc::new(detailed_picture(0)));
    }
    if name == "profile-avatar-unavailable" {
        avatar.available = false;
        avatar.notice = Some("Avatar changes are not available on this service yet.".to_owned());
    }
    PageView::Profile {
        mode_names: Vec::new(),
        read_only: name.starts_with("profile_other"),
        notice: (name == "profile_other_unavailable")
            .then(|| "This player's profile is unavailable".into()),
        version: None,
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
                    mode_id: "solo_mid".into(),
                    confidence: super::rank_confidence::from_deviation(80.0, 41),
                    mode: "Solo Mid".to_owned(),
                    rating: Some("1,524".to_owned()),
                    rank: Some(12),
                    games: 41,
                },
                Rating {
                    mode_id: "all_pick".into(),
                    confidence: super::rank_confidence::from_deviation(120.0, 9),
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
    if name == "leaderboard_solo" {
        board.modes.retain(|mode| mode.id == "solo_mid");
        for mode in &mut board.modes {
            mode.name = "1x1 Mid Only".into();
        }
    }
    if name == "leaderboard-empty" {
        board.rows.clear();
        board.total = 0;
        board.own = None;
    }
    if name == "leaderboard-unavailable" {
        board.disconnected();
    }
    if name == "leaderboard-pictures" {
        for (at, row) in board
            .rows
            .iter_mut()
            .chain(board.own.iter_mut())
            .enumerate()
        {
            // Two rows in three have an avatar, each its own.
            if at % 3 != 2 {
                row.picture = detailed_picture(at as u8 + 1)
                    .shrunk(LIST_SIDE)
                    .map(Arc::new);
            }
        }
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

/// A picture with all the detail an avatar can have: fine rings, hatching, a checker, a soft
/// edge and a smooth gradient, `seed` rotating its colours. The previews show it at the sizes
/// avatars are drawn at, where a picture sampled badly shimmers or goes grey.
fn detailed_picture(seed: u8) -> Pixels {
    let side = super::avatar_image::SIDE;
    let rgba = (0..side)
        .flat_map(|y| (0..side).map(move |x| (x as f32 - 127.5, y as f32 - 127.5)))
        .flat_map(|(x, y)| {
            let radius = (x * x + y * y).sqrt();
            let angle = y.atan2(x);
            let mut color = [
                60.0 + 0.5 * radius,
                90.0 + 0.3 * (x + 128.0),
                170.0 - 0.4 * radius,
            ];
            if (radius as u32 / 3).is_multiple_of(2) && radius > 40.0 && radius < 100.0 {
                color = [235.0, 225.0, 190.0];
            }
            if ((angle * 24.0 / std::f32::consts::TAU) as i32).rem_euclid(2) == 0 && radius < 38.0 {
                color = [30.0, 30.0, 40.0];
            }
            if x < -40.0 && y > 40.0 && ((x as i32 / 4) + (y as i32 / 4)).rem_euclid(2) == 0 {
                color = [250.0, 120.0, 40.0];
            }
            if (x + y) as i32 % 5 == 0 && x > 40.0 && y < -40.0 {
                color = [20.0, 200.0, 120.0];
            }
            let rotated = [
                color[(seed as usize) % 3],
                color[(seed as usize + 1) % 3],
                color[(seed as usize + 2) % 3],
            ];
            let alpha = ((125.0 - radius) * 12.0).clamp(0.0, 255.0);
            [
                rotated[0] as u8,
                rotated[1] as u8,
                rotated[2] as u8,
                alpha as u8,
            ]
        })
        .collect();
    Pixels::from_rgba(side, side, rgba).expect("exactly the avatar's pixels")
}

fn decorate(view: &mut MenuView, name: &str) {
    if name.ends_with("-picture") {
        view.player_avatar = detailed_picture(0).shrunk(LIST_SIDE).map(Arc::new);
    }
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
            nickname: if bots {
                name.split('#').next().unwrap_or(name).to_owned()
            } else {
                name.to_owned()
            },
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
            avatar: None,
            picture: None,
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
            avatar: None,
            picture: None,
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
            rating: super::leaderboard::rating(f64::from(
                (if bots {
                    1670
                } else if allpick {
                    1650
                } else {
                    1500
                }) - 2 * rank as i32,
            ))
            .expect("a finite fixture rating"),
            avatar: None,
            picture: None,
        });
    }
}
