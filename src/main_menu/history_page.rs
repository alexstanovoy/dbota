//! Local match history: inspection survives pruning the replay bytes.
use super::{
    core::{Act, MenuCore, Page},
    history::{self, Match, ResultKind},
    kit::{Kit, MenuButton, area, at, body, bold, caption, number, rect, words},
};
use crate::{postgame::graphs, theme};
use bevy::prelude::*;
use bevy::ui_widgets::Button;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Mode,
    Result,
    Hero,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub ratings: Vec<super::account_page::Rating>,
    pub matches: std::sync::Arc<Vec<Match>>,
    pub filter: Selection,
    pub picker: Option<Filter>,
    pub page: usize,
    pub count: usize,
    pub bytes: u64,
    pub limit: u64,
    pub day: String,
    pub avatar: String,
}
impl MenuCore {
    pub(super) fn history_view(&self) -> View {
        let player = self
            .client
            .as_ref()
            .filter(|_| self.logged_in())
            .and_then(|client| client.me())
            .map(|account| account.nickname.clone())
            .unwrap_or_else(|| self.name());
        View {
            ratings: self.profile_ratings(),
            matches: self.local_history.cards(self.library.records()),
            filter: self.history_filter.clone(),
            picker: self.history_picker,
            page: self.history_page,
            count: self.library.records().len(),
            bytes: self
                .library
                .records()
                .iter()
                .map(|record| record.bytes)
                .sum(),
            limit: self.library.policy().max_bytes,
            day: super::results::when(crate::replays::now())
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_owned(),
            avatar: player
                .chars()
                .next()
                .unwrap_or('P')
                .to_uppercase()
                .take(1)
                .collect(),
        }
    }
}
pub fn mode_name(mode: &str) -> String {
    history::mode_name(mode).to_owned()
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub mode: Option<String>,
    pub result: Option<ResultKind>,
    pub hero: Option<bota_proto::HeroId>,
}
impl Selection {
    fn accepts(&self, game: &Match) -> bool {
        self.mode.as_ref().is_none_or(|mode| mode == &game.mode)
            && self.result.is_none_or(|result| result == game.result)
            && self.hero.is_none_or(|hero| Some(hero) == game.hero)
    }
}
impl MenuCore {
    pub(super) fn reset_history_filter(&mut self) {
        self.history_picker = None;
        self.history_page = 0;
    }
}
fn act(
    kit: &mut Kit,
    parent: Entity,
    label: &str,
    node: Node,
    action: Act,
    enabled: bool,
) -> Entity {
    let entity = rect(
        kit,
        parent,
        Node {
            border: UiRect::all(px(1)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..node
        },
        theme::SECONDARY.1,
    );
    kit.commands
        .entity(entity)
        .insert(BorderColor::all(if enabled {
            theme::SECONDARY_EDGE
        } else {
            theme::DIVIDER
        }));
    if enabled {
        kit.commands.entity(entity).insert((
            Pickable::default(),
            Button,
            MenuButton(action),
            Name::new(format!("history {label}")),
        ));
    }
    words(
        kit,
        entity,
        label,
        Node::default(),
        caption(12.0),
        if enabled {
            theme::INK_1
        } else {
            theme::INK_OFF
        },
    );
    entity
}
pub fn draw(kit: &mut Kit, root: Entity, view: &View) {
    rect(
        kit,
        root,
        Node {
            position_type: PositionType::Absolute,
            top: px(54),
            bottom: px(0),
            width: percent(100),
            ..default()
        },
        Color::srgb_u8(13, 18, 24),
    );
    let breadcrumb = rect(
        kit,
        root,
        Node {
            width: percent(100),
            border: UiRect::bottom(px(1)),
            ..area(0.0, 54.0, 0.0, 46.0)
        },
        theme::GROUND,
    );
    kit.commands
        .entity(breadcrumb)
        .insert(BorderColor::all(theme::BAR_EDGE));
    let pane = kit.node(
        root,
        Node {
            left: percent(50),
            margin: UiRect::left(px(-580)),
            ..area(0.0, 54.0, 1160.0, 822.0)
        },
    );
    let avatar = act(
        kit,
        pane,
        &view.avatar,
        area(0.0, 7.0, 32.0, 32.0),
        Act::Go(Page::Profile),
        true,
    );
    kit.commands
        .entity(avatar)
        .insert(BackgroundColor(Color::srgb_u8(33, 52, 73)));
    let profile = kit
        .commands
        .spawn((
            Button,
            MenuButton(Act::Go(Page::Profile)),
            area(50.0, 6.0, 96.0, 40.0),
            ChildOf(pane),
            Name::new("history PROFILE"),
        ))
        .id();
    words(
        kit,
        profile,
        "PROFILE",
        at(0.0, 12.0),
        caption(15.0),
        theme::INK_4,
    );
    words(kit, pane, "/", at(144.0, 18.0), body(15.0), theme::INK_OFF);
    words(
        kit,
        pane,
        "MATCHES",
        at(166.0, 18.0),
        caption(15.0),
        theme::INK_1,
    );
    rect(kit, pane, area(159.0, 44.0, 99.0, 2.0), theme::INK_1);
    history_confidence(kit, pane, &view.ratings);
    let labels = [
        (
            "Mode",
            view.filter
                .mode
                .as_deref()
                .map(mode_name)
                .unwrap_or_else(|| "Any".to_owned()),
            Filter::Mode,
        ),
        (
            "Result",
            view.filter
                .result
                .map(result_label)
                .unwrap_or("Any")
                .to_owned(),
            Filter::Result,
        ),
        (
            "Hero",
            view.filter
                .hero
                .and_then(bota_core::hero_def)
                .map_or("Any", |hero| kit.texts.name(hero.key))
                .to_owned(),
            Filter::Hero,
        ),
    ];
    for (index, (label, value, filter)) in labels.iter().enumerate() {
        let x = index as f32 * 170.0;
        words(kit, pane, *label, at(x, 63.0), body(12.0), theme::INK_4);
        act(
            kit,
            pane,
            &format!("{value}   ⌄"),
            area(x, 82.0, 160.0, 44.0),
            Act::HistoryFilter(*filter),
            true,
        );
    }
    rect(kit, pane, area(0.0, 134.0, 1160.0, 30.0), theme::GROUND);
    for (text, x) in [
        ("DATE", 8.0),
        ("HERO", 184.0),
        ("RESULT", 464.0),
        ("MODE", 548.0),
        ("LENGTH", 678.0),
        ("K / D / A", 762.0),
        ("NET WORTH", 862.0),
    ] {
        words(kit, pane, text, at(x, 144.0), caption(10.0), theme::INK_3);
    }
    let filtered: Vec<_> = view
        .matches
        .iter()
        .filter(|game| view.filter.accepts(game))
        .collect();
    let pages = filtered.len().div_ceil(10).max(1);
    let page = view.page.min(pages - 1);
    for (index, game) in filtered.iter().skip(page * 10).take(10).enumerate() {
        row(kit, pane, game, 172.0 + index as f32 * 58.0, &view.day);
    }
    if filtered.is_empty() {
        words(
            kit,
            pane,
            "No matches recorded on this machine",
            at(20.0, 200.0),
            body(16.0),
            theme::INK_4,
        );
    }
    let foot = rect(kit, pane, area(0.0, 778.0, 1160.0, 44.0), theme::GROUND);
    let limit = if view.limit.is_multiple_of(1 << 30) {
        format!("{} GB", view.limit / (1 << 30))
    } else {
        format!("{} MB", view.limit / (1 << 20))
    };
    words(
        kit,
        foot,
        format!(
            "Replays on this machine: {} · {} MB of {limit}",
            view.count,
            view.bytes / (1 << 20)
        ),
        at(10.0, 16.0),
        body(12.0),
        theme::INK_3,
    );
    rect(
        kit,
        foot,
        area(264.0, 20.0, 160.0, 6.0),
        Color::srgb_u8(35, 42, 45),
    );
    rect(
        kit,
        foot,
        area(
            264.0,
            20.0,
            160.0 * (view.bytes as f64 / view.limit.max(1) as f64).clamp(0.0, 1.0) as f32,
            6.0,
        ),
        theme::SELECT,
    );
    if pages > 1 {
        act(
            kit,
            foot,
            "←",
            area(590.0, 4.0, 40.0, 36.0),
            Act::HistoryPage(-1),
            page > 0,
        );
        words(
            kit,
            foot,
            format!("{} / {pages}", page + 1),
            at(644.0, 15.0),
            body(12.0),
            theme::INK_3,
        );
        act(
            kit,
            foot,
            "→",
            area(710.0, 4.0, 40.0, 36.0),
            Act::HistoryPage(1),
            page + 1 < pages,
        );
    }
    let limits = kit
        .commands
        .spawn((
            Button,
            MenuButton(Act::Go(Page::Settings)),
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                ..area(910.0, 4.0, 240.0, 36.0)
            },
            ChildOf(foot),
            Name::new("history replay limits"),
        ))
        .id();
    words(
        kit,
        limits,
        "Replay limits in Settings",
        Node::default(),
        body(12.0),
        theme::SELECT,
    );
    if let Some(picker) = view.picker {
        dropdown(kit, pane, view, picker);
    }
}
fn row(kit: &mut Kit, parent: Entity, game: &Match, y: f32, day: &str) {
    let colour = result_colour(game.result);
    let row = rect(
        kit,
        parent,
        area(0.0, y, 1160.0, 50.0),
        Color::srgb_u8(9, 14, 17),
    );
    rect(kit, row, area(0.0, 0.0, 3.0, 50.0), colour);
    let (date, time) = date_time(game.when, day);
    words(kit, row, date, at(12.0, 10.0), body(12.0), theme::INK_1);
    words(kit, row, time, at(12.0, 27.0), body(11.0), theme::INK_4);
    if let Some(path) = game.hero.and_then(|hero| kit.manifest.hero_icon(hero.0)) {
        let icon = kit.node(row, area(116.0, 11.0, 50.0, 28.0));
        kit.commands
            .entity(icon)
            .insert(ImageNode::new(kit.server.load(path)));
    }
    if game.players.is_empty() {
        words(
            kit,
            row,
            game.hero_name(kit.texts),
            at(187.0, 20.0),
            bold(12.0),
            theme::INK_1,
        );
    } else {
        words(
            kit,
            row,
            game.hero_name(kit.texts),
            at(187.0, 3.0),
            body(10.0),
            theme::INK_4,
        );
        let names = kit.node(
            row,
            Node {
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..area(187.0, 17.0, 260.0, 32.0)
            },
        );
        for player in game.players.iter().take(2) {
            kit.inline_player_name(
                names,
                (&player.name, player.version.as_deref()),
                bold(11.0),
                theme::INK_1,
                260.0,
            );
        }
    }
    words(
        kit,
        row,
        result_label(game.result),
        at(464.0, 19.0),
        caption(12.0),
        colour,
    );
    words(
        kit,
        row,
        mode_name(&game.mode),
        at(548.0, 20.0),
        body(12.0),
        theme::INK_3,
    );
    words(
        kit,
        row,
        game.length(),
        at(678.0, 20.0),
        number(12.0),
        theme::INK_3,
    );
    words(
        kit,
        row,
        game.kda_text(),
        at(762.0, 20.0),
        number(12.0),
        theme::INK_1,
    );
    words(
        kit,
        row,
        game.net_worth
            .map_or_else(|| "—".to_owned(), |value| graphs::compact(i64::from(value))),
        at(862.0, 20.0),
        number(12.0),
        theme::GOLD,
    );
    act(
        kit,
        row,
        "WATCH",
        area(960.0, 4.0, 92.0, 40.0),
        Act::WatchHistory(game.file.clone()),
        game.watchable,
    );
    act(
        kit,
        row,
        "DETAILS",
        area(1060.0, 4.0, 92.0, 40.0),
        Act::MatchDetails(game.file.clone()),
        game.details_available,
    );
}
fn history_confidence(kit: &mut Kit, parent: Entity, ratings: &[super::account_page::Rating]) {
    let group = kit.node(
        parent,
        Node {
            position_type: PositionType::Absolute,
            left: px(460),
            top: px(3),
            width: px(700),
            height: px(40),
            column_gap: px(20),
            overflow: Overflow::clip(),
            ..default()
        },
    );
    for rating in ratings {
        let Some(confidence) = rating.confidence else {
            continue;
        };
        let column = kit.node(
            group,
            Node {
                width: px(210),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(1),
                ..default()
            },
        );
        kit.commands
            .entity(column)
            .insert(Name::new(format!("rank confidence {}", rating.mode_id)));
        let value = if confidence.calibrated {
            rating.rating.as_deref().unwrap_or("—")
        } else {
            "Uncalibrated"
        };
        kit.typed(
            column,
            format!("{} · {value}", rating.mode),
            theme::CAPTION,
            theme::INK_3,
        );
        super::account_page::confidence_bar(kit, column, rating, confidence);
    }
}

fn dropdown(kit: &mut Kit, parent: Entity, view: &View, filter: Filter) {
    let (x, mut options) = match filter {
        Filter::Mode => (0.0, vec![("Any".to_owned(), Act::HistoryMode(None))]),
        Filter::Result => (170.0, vec![("Any".to_owned(), Act::HistoryResult(None))]),
        Filter::Hero => (340.0, vec![("Any".to_owned(), Act::HistoryHero(None))]),
    };
    match filter {
        Filter::Mode => {
            let mut modes: Vec<_> = view.matches.iter().map(|game| game.mode.clone()).collect();
            modes.sort();
            modes.dedup();
            options.extend(
                modes
                    .into_iter()
                    .map(|mode| (mode_name(&mode), Act::HistoryMode(Some(mode)))),
            );
        }
        Filter::Result => options.extend(
            [
                ResultKind::Won,
                ResultKind::Lost,
                ResultKind::Draw,
                ResultKind::Undecided,
            ]
            .into_iter()
            .map(|kind| {
                (
                    result_label(kind).to_owned(),
                    Act::HistoryResult(Some(kind)),
                )
            }),
        ),
        Filter::Hero => options.extend(bota_core::HEROES.iter().map(|hero| {
            (
                kit.texts.name(hero.key).to_owned(),
                Act::HistoryHero(Some(hero.id)),
            )
        })),
    }
    let dropdown = rect(
        kit,
        parent,
        Node {
            overflow: Overflow::scroll_y(),
            ..area(x, 128.0, 220.0, (options.len() as f32 * 40.0).min(400.0))
        },
        theme::RESULT_PANEL,
    );
    kit.commands.entity(dropdown).insert((
        ZIndex(20),
        ScrollPosition::default(),
        bevy::ui::RelativeCursorPosition::default(),
        crate::ui::ScrollArea,
    ));
    for (index, (label, action)) in options.into_iter().enumerate() {
        act(
            kit,
            dropdown,
            &label,
            area(0.0, index as f32 * 40.0, 220.0, 40.0),
            action,
            true,
        );
    }
}
fn result_label(result: ResultKind) -> &'static str {
    match result {
        ResultKind::Won => "WON",
        ResultKind::Lost => "LOST",
        ResultKind::Draw => "DRAW",
        ResultKind::Undecided => "NO RESULT",
    }
}
fn result_colour(result: ResultKind) -> Color {
    match result {
        ResultKind::Won => theme::RADIANT_TEXT,
        ResultKind::Lost => theme::DIRE_TEXT,
        _ => theme::INK_4,
    }
}

pub fn preview() -> View {
    let heroes = [bota_proto::HeroId(1), bota_proto::HeroId(0)];
    let times = [2292, 2497, 708, 1330, 860, 595, 2645, 751, 2169, 1720];
    let worth = [
        24800, 9600, 8100, 11200, 6400, 5900, 15300, 7200, 14000, 13100,
    ];
    let kda = [
        (11, 3, 9),
        (3, 9, 12),
        (9, 1, 2),
        (5, 4, 8),
        (1, 2, 0),
        (2, 1, 0),
        (7, 6, 19),
        (2, 0, 1),
        (6, 8, 4),
        (9, 5, 13),
    ];
    View {
        ratings: vec![super::account_page::Rating {
            mode_id: "solo_mid".into(),
            mode: "Solo Mid".into(),
            rating: Some("1,524".into()),
            games: 41,
            rank: Some(12),
            confidence: super::rank_confidence::from_deviation(80.0, 41),
        }],
        matches: std::sync::Arc::new(
            (0..10)
                .map(|index| Match {
                    players: Vec::new(),
                    file: format!("preview-{index}.brp"),
                    when: [
                        1793976000, 1793962920, 1793898900, 1793808120, 1793729400, 1793640600,
                        1793548860, 1793459520, 1793386020, 1793381100,
                    ][index],
                    mode: match index {
                        2 | 4 | 5 | 7 => "solo_mid",
                        3 | 9 => "local",
                        _ => "all_pick",
                    }
                    .to_owned(),
                    hero: Some(heroes[usize::from(matches!(index, 1 | 3 | 6 | 9))]),
                    result: if matches!(index, 1 | 4 | 8) {
                        ResultKind::Lost
                    } else {
                        ResultKind::Won
                    },
                    seconds: Some(times[index]),
                    kda: Some(kda[index]),
                    net_worth: Some(worth[index]),
                    watchable: index != 4,
                    details_available: true,
                })
                .collect(),
        ),
        filter: Default::default(),
        picker: None,
        page: 0,
        count: 12,
        bytes: 184 << 20,
        limit: 1 << 30,
        day: "2026-11-06".to_owned(),
        avatar: "M".to_owned(),
    }
}

fn date_time(seconds: u64, today: &str) -> (String, String) {
    let stamp = super::results::when(seconds);
    let Some((date, time)) = stamp.split_once(' ') else {
        return ("Unknown date".to_owned(), String::new());
    };
    let label = if date == today {
        "Today".to_owned()
    } else if previous_day(today).as_deref() == Some(date) {
        "Yesterday".to_owned()
    } else {
        let parts: Vec<_> = date.split('-').collect();
        let month = parts
            .get(1)
            .and_then(|month| month.parse::<usize>().ok())
            .and_then(|month| month.checked_sub(1))
            .and_then(|month| {
                [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ]
                .get(month)
            })
            .copied();
        match (parts.get(2).and_then(|day| day.parse::<u8>().ok()), month) {
            (Some(day), Some(month)) => format!("{day} {month}"),
            _ => "Unknown date".to_owned(),
        }
    };
    (label, time.to_owned())
}
#[cfg(not(target_arch = "wasm32"))]
fn previous_day(date: &str) -> Option<String> {
    date.parse::<jiff::civil::Date>()
        .ok()?
        .yesterday()
        .ok()
        .map(|date| date.to_string())
}
#[cfg(target_arch = "wasm32")]
fn previous_day(date: &str) -> Option<String> {
    let date = js_sys::Date::new(&format!("{date}T12:00:00").into());
    if !date.get_time().is_finite() {
        return None;
    }
    date.set_date(date.get_date().saturating_sub(1));
    Some(format!(
        "{:04}-{:02}-{:02}",
        date.get_full_year(),
        date.get_month() + 1,
        date.get_date()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        e2e::harness::{Client, Server},
        postgame::{
            self, PostGame,
            summary::MatchSummary,
            view::{Context, View as ResultView},
        },
        replays::Record,
    };
    use bevy::input::ButtonState;
    use bota_radovid::proto::{
        AccountKind, EndReason, MatchId, MatchResult, Outcome, Rating, RulesetId, SeatResult,
        UnixMs,
    };

    fn saved(
        record: &Record,
        file: Option<String>,
        metadata: Vec<u8>,
    ) -> Option<super::super::local_matches::Saved> {
        let summary = MatchSummary::from_meta(&bota_replay::meta::decode(&metadata).ok()?)?;
        Some(super::super::local_matches::Saved {
            key: record.name.clone(),
            file,
            when: record.when,
            mode: record.mode.clone(),
            owner: record.owner,
            metadata,
            metadata_key: None,
            cached: None,
            result: None,
            summary: Some(std::sync::Arc::new(summary)),
            temporary: None,
        })
    }

    #[test]
    fn match_details_survive_replay_pruning_and_close_back_to_the_filtered_history() {
        let mut client = Client::run(&[]);
        client.frames(4);
        let record = Record {
            name: "recorded.brp".to_owned(),
            when: 1794062400,
            mode: "all_pick".to_owned(),
            bytes: 2048,
            match_id: Some(10432),
            players: String::new(),
            outcome: String::new(),
            owner: Some(bota_proto::SlotId(0)),
        };
        let metadata = postgame::preview::metadata("match_details");
        {
            let mut core = client.world_mut().non_send_mut::<MenuCore>();
            let mut policy = core.library.policy();
            policy.keep = false;
            core.library.set_policy(policy);
            core.local_history.insert(
                saved(&record, Some(record.name.clone()), metadata.clone()).expect("metadata"),
                50,
            );
            let mut foreign = record.clone();
            foreign.name = "imported.brp".to_owned();
            foreign.when = 1;
            foreign.owner = None;
            core.local_history.insert(
                saved(&foreign, None, metadata).expect("foreign metadata"),
                50,
            );
            let store = core.store.clone();
            core.local_history.save(&store).expect("persist history");
            core.act(Act::Go(Page::MatchHistory));
        }
        client.frames(5);
        let world = client.world_mut();
        assert_eq!(
            world
                .query_filtered::<&Name, With<Button>>()
                .iter(world)
                .filter(|name| name.as_str() == "history DETAILS")
                .count(),
            2
        );
        assert!(
            !world
                .query_filtered::<&Name, With<Button>>()
                .iter(world)
                .any(|name| name.as_str() == "history WATCH"),
            "pruned replay has no active watch button"
        );
        client.click_named("history DETAILS");
        assert!(client.world().non_send::<PostGame>().visible());
        let world = client.world_mut();
        let covered = world
            .query::<(&Name, &UiGlobalTransform)>()
            .iter(world)
            .find(|(name, _)| name.as_str() == "history Any   ⌄")
            .map(|(_, place)| place.translation)
            .expect("background filter");
        client.mouse(covered);
        client.click(MouseButton::Left);
        client.frames(3);
        assert_eq!(
            client.world().non_send::<MenuCore>().history_picker,
            None,
            "modal consumes background clicks"
        );
        client.click_named("results scoreboard");
        assert!(client.shown_texts().iter().any(|text| text == "24.8k"));
        client.click_named("results graphs");
        assert!(
            client
                .shown_texts()
                .iter()
                .any(|text| text == "Team advantage")
        );
        client.click_named("results close");
        assert!(!client.world().non_send::<PostGame>().visible());
        assert_eq!(
            client.world().non_send::<MenuCore>().page(),
            Page::MatchHistory
        );
        client.click_named("history Any   ⌄");
        client.click_named("history All Pick");
        assert_eq!(
            client
                .world()
                .non_send::<MenuCore>()
                .history_filter
                .mode
                .as_deref(),
            Some("all_pick")
        );
        client.until("history snapshot persisted", |client| {
            client
                .world()
                .non_send::<MenuCore>()
                .store
                .read(super::super::store::Entry::MatchHistory)
                .is_some()
        });
        let store = client.world().non_send::<MenuCore>().store.clone();
        client.world_mut().non_send_mut::<MenuCore>().local_history =
            super::super::local_matches::Archive::load(&store, 50);
        client.until("metadata returns after restart", |client| {
            client
                .world()
                .non_send::<MenuCore>()
                .local_history
                .entries()
                .len()
                == 2
        });
    }

    #[test]
    fn inspecting_history_blocks_game_orders_and_cannot_replace_a_live_session() {
        let server = Server::start(1, &["--map", "1"]);
        let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .library
            .save(crate::replays::Saving {
                explicit: true,
                mode: "file".to_owned(),
                match_id: None,
                owner: None,
                players: String::new(),
                outcome: String::new(),
                bytes: vec![b'x'; 2048],
            });
        client.until("test replay listed", |client| {
            !client
                .world()
                .non_send::<MenuCore>()
                .library
                .records()
                .is_empty()
        });
        {
            let mut core = client.world_mut().non_send_mut::<MenuCore>();
            let record = core.library.records()[0].clone();
            let entry = saved(
                &record,
                Some(record.name.clone()),
                crate::postgame::preview::metadata("match_details"),
            )
            .expect("old match metadata");
            core.local_history.insert(entry, 50);
            core.act(Act::Dashboard);
            core.act(Act::Go(Page::MatchHistory));
        }
        client.frames(4);
        client.click_named("history DETAILS");
        client.click_named("results graphs");
        let world = client.world_mut();
        let centre = world
            .query::<(&Name, &UiGlobalTransform)>()
            .iter(world)
            .find(|(name, _)| name.as_str() == "results graph plot")
            .map(|(_, place)| place.translation)
            .expect("plot");
        client.mouse(centre);
        assert!(
            client.world().resource::<crate::input::OverHud>().0,
            "even the plot consumes world input"
        );
        let orders = client.core().shown_orders.len();
        client.click(MouseButton::Right);
        client.key(KeyCode::KeyS, ButtonState::Pressed);
        client.key(KeyCode::KeyS, ButtonState::Released);
        client.frames(4);
        assert_eq!(
            client.core().shown_orders.len(),
            orders,
            "inspection emits no game orders"
        );
        client.click_named("results watch replay");
        assert!(matches!(
            client.core().source,
            bota_client_core::Source::Live(_)
        ));
        assert!(client.world().non_send::<MenuCore>().in_game());
        assert!(client.world().non_send::<PostGame>().visible());
        assert!(
            client
                .shown_texts()
                .iter()
                .any(|text| text == "Leave the current game before watching this replay")
        );
        client.click_named("results dismiss");
        assert!(client.world().non_send::<PostGame>().visible());
        assert!(matches!(
            client.core().source,
            bota_client_core::Source::Live(_)
        ));
    }

    #[test]
    fn a_late_outcome_enriches_the_drawn_header_without_replacing_recorded_statistics() {
        let service = super::super::tests::service();
        let mut player = super::super::tests::Player::new(Some(&service));
        player.register("mira");
        let account = player
            .menu
            .client
            .as_ref()
            .and_then(|client| client.me())
            .expect("signed in")
            .id;
        let mut client = Client::run(&[]);
        client.world_mut().insert_non_send(player.menu);
        let metadata = crate::postgame::preview::metadata("match_details");
        let summary = std::sync::Arc::new(
            MatchSummary::from_meta(&bota_replay::meta::decode(&metadata).expect("meta"))
                .expect("summary"),
        );
        {
            let mut core = client.world_mut().non_send_mut::<MenuCore>();
            core.shown = Some(MatchId(10432));
            core.act(Act::Go(Page::Result));
            core.result_request = Some(ResultView::new(
                summary,
                Context {
                    match_id: Some(10432),
                    mode: "All Pick".to_owned(),
                    owner: Some(bota_proto::SlotId(0)),
                    ..Default::default()
                },
            ));
        }
        client.frames(5);
        assert!(
            !client
                .shown_texts()
                .iter()
                .any(|text| text.starts_with("Rating +24"))
        );
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .results
            .push_back(MatchResult {
                match_id: MatchId(10432),
                ruleset: RulesetId("all_pick".to_owned()),
                rated: true,
                outcome: Outcome::Winner { team: 0 },
                reason: EndReason::Left,
                duration_ms: 2292000,
                game_report: vec![255],
                seats: vec![SeatResult {
                    version: None,
                    slot: 0,
                    team: 0,
                    account,
                    nickname: "mira".to_owned(),
                    kind: AccountKind::Human,
                    before: Some(Rating {
                        rating: 1302.0,
                        deviation: 100.0,
                    }),
                    after: Some(Rating {
                        rating: 1326.0,
                        deviation: 100.0,
                    }),
                }],
                finished_at: UnixMs(1793976000000),
            });
        client.frames(5);
        let texts = client.shown_texts();
        assert!(texts.iter().any(|text| text == "Rating +24 → 1,326"));
        assert!(texts.iter().any(|text| text == "DIRE PLAYER LEFT"));
        client.click_named("results scoreboard");
        assert!(
            client.shown_texts().iter().any(|text| text == "24.8k"),
            "statistics still come from the local recording"
        );
    }
    #[test]
    fn a_replay_pruned_while_details_are_open_reports_the_missing_file() {
        let mut client = Client::run(&[]);
        client.frames(4);
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .library
            .save(crate::replays::Saving {
                explicit: true,
                mode: "file".into(),
                match_id: None,
                owner: None,
                players: String::new(),
                outcome: String::new(),
                bytes: vec![b'x'; 2048],
            });
        client.until("replay saved", |client| {
            !client
                .world()
                .non_send::<MenuCore>()
                .library
                .records()
                .is_empty()
        });
        let file = {
            let mut core = client.world_mut().non_send_mut::<MenuCore>();
            let record = core.library.records()[0].clone();
            core.local_history.insert(
                saved(
                    &record,
                    Some(record.name.clone()),
                    postgame::preview::metadata("match_details"),
                )
                .expect("metadata"),
                50,
            );
            core.act(Act::MatchDetails(record.name.clone()));
            record.name
        };
        client.frames(4);
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .library
            .delete(&file);
        client.frames(4);
        client.click_named("results watch replay");
        assert!(client.world().non_send::<PostGame>().visible());
        assert!(
            client
                .shown_texts()
                .iter()
                .any(|text| text == "That replay is no longer available"),
            "the failure describes the missing file"
        );
        assert!(
            !client
                .shown_texts()
                .iter()
                .any(|text| text == "Leave the current game before watching this replay")
        );
        client.click_named("results dismiss");
        assert!(client.world().non_send::<PostGame>().visible());
        assert!(
            !client
                .shown_texts()
                .iter()
                .any(|text| text == "That replay is no longer available")
        );
    }
}
