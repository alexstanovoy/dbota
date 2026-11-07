//! Profile and history share their navigation and local match-row renderer.
use super::{
    core::{Act, Page},
    history::{HeroRecord, Match, ResultKind},
    kit::{Kit, Tone},
};
use crate::theme;
use bevy::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rating {
    pub mode: String,
    pub rating: Option<String>,
    pub games: u32,
    pub rank: Option<u32>,
}

pub fn tabs(kit: &mut Kit, root: Entity, matches: bool, player: &str) {
    let tabs = kit.node(
        root,
        Node {
            height: px(46),
            align_items: AlignItems::Center,
            column_gap: px(12),
            ..default()
        },
    );
    let avatar = kit.node(
        tabs,
        Node {
            width: px(32),
            height: px(32),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands.entity(avatar).insert((
        theme::downward(theme::AVATAR),
        BorderColor::all(theme::AVATAR_EDGE),
    ));
    kit.bold(
        avatar,
        player
            .chars()
            .next()
            .map(|ch| ch.to_uppercase().to_string())
            .unwrap_or_default(),
        12.0,
        theme::INK_1,
    );
    for (name, page, selected) in [
        ("PROFILE", Page::Profile, !matches),
        ("MATCHES", Page::Replays, matches),
    ] {
        if name == "MATCHES" {
            kit.text(tabs, "/", 14.0, theme::INK_4);
        }
        let button = kit.quiet(
            tabs,
            name,
            Act::Go(page),
            theme::LABEL,
            (
                if selected { theme::INK_1 } else { theme::INK_4 },
                theme::INK_1,
            ),
        );
        if selected {
            kit.commands
                .entity(button)
                .entry::<Node>()
                .and_modify(|mut node| node.border.bottom = px(2));
            kit.commands.entity(button).insert(BorderColor {
                bottom: theme::INK_1,
                ..BorderColor::all(Color::NONE)
            });
        }
    }
}

pub fn profile(
    kit: &mut Kit,
    root: Entity,
    name: &str,
    connected: bool,
    ratings: &[Rating],
    matches: &[Match],
    avatar: &super::avatar::View,
    clock: super::page_time::Clock,
) {
    let page = profile_page(kit, root);
    tabs(kit, page, false, name);
    profile_header(kit, page, name, connected, avatar);
    let body = kit.node(
        page,
        Node {
            width: percent(100),
            margin: UiRect::top(px(22)),
            column_gap: px(20),
            ..default()
        },
    );
    let left = kit.node(
        body,
        Node {
            width: px(840),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            ..default()
        },
    );
    if connected && ratings.iter().any(|rating| rating.games > 0) {
        rating_cards(kit, left, ratings);
    }
    recent_matches(kit, left, matches, clock);
    most_played_column(kit, body, matches);
}

fn profile_page(kit: &mut Kit, root: Entity) -> Entity {
    let band = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            top: px(54),
            left: px(0),
            right: px(0),
            height: px(46),
            border: UiRect::bottom(px(1)),
            ..default()
        },
    );
    kit.commands.entity(band).insert((
        BackgroundColor(theme::ACCOUNT_BAND),
        BorderColor::all(theme::DIVIDER),
    ));
    kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            margin: UiRect::left(px(-580)),
            top: px(54),
            width: px(1160),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    )
}

fn profile_header(
    kit: &mut Kit,
    page: Entity,
    name: &str,
    connected: bool,
    avatar: &super::avatar::View,
) {
    let header = kit.node(
        page,
        Node {
            height: px(72),
            margin: UiRect::top(px(20)),
            padding: UiRect::all(px(12)),
            align_items: AlignItems::Center,
            column_gap: px(14),
            ..default()
        },
    );
    kit.commands
        .entity(header)
        .insert(BackgroundColor(Color::BLACK));
    super::avatar_draw::header(kit, header, name, avatar);
    kit.bold(header, name, 28.0, theme::INK_1);
    kit.node(
        header,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    if connected {
        let sign_out = kit.button_role(
            header,
            "SIGN OUT",
            Act::Logout,
            Tone::Secondary,
            46.0,
            theme::TABLE_ACTION,
        );
        let change = kit.button_role(
            header,
            "CHANGE SERVICE",
            Act::Go(Page::Service),
            Tone::Secondary,
            46.0,
            theme::TABLE_ACTION,
        );
        kit.commands
            .entity(sign_out)
            .entry::<Node>()
            .and_modify(|mut node| node.width = px(100));
        kit.commands
            .entity(change)
            .entry::<Node>()
            .and_modify(|mut node| node.width = px(156));
    }
}

fn rating_cards(kit: &mut Kit, left: Entity, ratings: &[Rating]) {
    caption(kit, left, "RATINGS · PER MODE");
    let row = kit.node(
        left,
        Node {
            width: percent(100),
            column_gap: px(10),
            margin: UiRect::bottom(px(10)),
            ..default()
        },
    );
    for rating in ratings {
        rating_card(kit, row, rating);
    }
}

fn rating_card(kit: &mut Kit, row: Entity, rating: &Rating) {
    let card = kit.node(
        row,
        Node {
            flex_grow: 1.0,
            height: px(96),
            padding: UiRect::all(px(16)),
            align_items: AlignItems::Center,
            column_gap: px(16),
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands.entity(card).insert((
        theme::downward(theme::PANEL),
        BorderColor::all(theme::PANEL_EDGE),
    ));
    rating_badge(kit, card, rating);
    let label = kit.node(
        card,
        Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            ..default()
        },
    );
    kit.typed(
        label,
        rating.mode.to_uppercase(),
        theme::HEADING,
        theme::INK_1,
    );
    kit.text(
        label,
        format!("{} rated games", rating.games),
        13.0,
        theme::INK_3,
    );
    kit.bold(
        card,
        rating.rating.as_deref().unwrap_or("UNRANKED"),
        if rating.rating.is_some() { 36.0 } else { 20.0 },
        theme::INK_1,
    );
}

fn rating_badge(kit: &mut Kit, card: Entity, rating: &Rating) {
    let badge = kit.node(
        card,
        Node {
            width: px(64),
            height: px(64),
            flex_shrink: 0.0,
            border_radius: BorderRadius::all(px(32)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    let fill = if rating
        .rank
        .is_some_and(|rank| rank <= super::leaderboard::TOP as u32)
    {
        theme::GOLD
    } else {
        theme::RANK_BADGE
    };
    kit.commands.entity(badge).insert(BackgroundColor(fill));
    kit.bold(
        badge,
        rating
            .rank
            .map_or_else(|| "—".to_owned(), |r| format!("#{r}")),
        15.0,
        Color::BLACK,
    );
}

fn recent_matches(kit: &mut Kit, left: Entity, matches: &[Match], clock: super::page_time::Clock) {
    let title = kit.node(
        left,
        Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    caption(kit, title, "RECENT MATCHES");
    let all = kit.quiet(
        title,
        "All matches",
        Act::Go(Page::Replays),
        theme::BODY,
        theme::LINK,
    );
    kit.commands
        .entity(all)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.min_height = px(20);
            node.height = px(20);
        });
    rows(kit, left, &matches[..matches.len().min(6)], false, clock);
}

fn most_played_column(kit: &mut Kit, body: Entity, matches: &[Match]) {
    let right = kit.node(
        body,
        Node {
            width: px(300),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            ..default()
        },
    );
    let heroes = super::history::heroes(matches);
    if let Some(hero) = heroes.first() {
        super::catalogue::tall(
            kit,
            right,
            hero.hero,
            Node {
                width: px(300),
                height: px(400),
                ..default()
            },
        );
        caption(kit, right, "MOST PLAYED");
        let played = kit.node(
            right,
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
        );
        for (index, hero) in heroes.iter().enumerate() {
            most_played(kit, played, hero, index);
        }
    }
}

fn most_played(kit: &mut Kit, parent: Entity, hero: &HeroRecord, index: usize) {
    let row = kit.node(
        parent,
        Node {
            width: percent(100),
            height: px(52),
            align_items: AlignItems::Center,
            column_gap: px(10),
            ..default()
        },
    );
    kit.commands.entity(row).insert((
        Name::new("most played row"),
        BackgroundColor(theme::HISTORY_ROW),
        BorderColor::all(if index.is_multiple_of(2) {
            theme::LOSS
        } else {
            theme::RADIANT_TEXT
        }),
    ));
    kit.commands
        .entity(row)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.padding = UiRect::axes(px(8), px(0));
            node.border.left = px(3);
        });
    kit.hero_icon(row, hero.hero.0, 64.0);
    let name = kit.node(
        row,
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            overflow: Overflow::clip(),
            ..default()
        },
    );
    kit.text(
        name,
        bota_core::hero_def(hero.hero).map_or("", |hero| hero.name),
        14.0,
        theme::INK_1,
    );
    let stats = kit.node(
        row,
        Node {
            width: px(66),
            flex_shrink: 0.0,
            justify_content: JustifyContent::FlexEnd,
            ..default()
        },
    );
    kit.commands
        .entity(stats)
        .insert(Name::new("most played stats"));
    let words = kit.text(
        stats,
        format!("{} games\n{}% won", hero.games, hero.percentage()),
        12.0,
        theme::INK_3,
    );
    kit.commands
        .entity(words)
        .insert(TextLayout::justify(Justify::Right));
}

pub fn rows(
    kit: &mut Kit,
    parent: Entity,
    matches: &[Match],
    full: bool,
    clock: super::page_time::Clock,
) {
    if matches.is_empty() {
        kit.text(
            parent,
            "No matches on this machine yet.",
            15.0,
            theme::INK_4,
        );
    }
    for game in matches {
        match_row(kit, parent, game, full, clock);
    }
}

fn match_row(
    kit: &mut Kit,
    parent: Entity,
    game: &Match,
    full: bool,
    clock: super::page_time::Clock,
) {
    let color = match game.result {
        ResultKind::Won => theme::RADIANT_TEXT,
        ResultKind::Lost => theme::LOSS,
        _ => theme::INK_4,
    };
    let row = kit.node(
        parent,
        Node {
            width: percent(100),
            min_height: px(if full { 50 } else { 44 }),
            align_items: AlignItems::Center,
            column_gap: px(12),
            padding: UiRect::axes(px(8), px(0)),
            border: UiRect::left(px(3)),
            ..default()
        },
    );
    kit.commands.entity(row).insert((
        BackgroundColor(theme::HISTORY_ROW),
        BorderColor {
            left: color,
            ..BorderColor::all(Color::NONE)
        },
    ));
    match_numbers(kit, row, game, full, clock);
    kit.node(
        row,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    match_actions(kit, row, game);
}

fn match_numbers(
    kit: &mut Kit,
    row: Entity,
    game: &Match,
    full: bool,
    clock: super::page_time::Clock,
) {
    let color = match game.result {
        ResultKind::Won => theme::RADIANT_TEXT,
        ResultKind::Lost => theme::LOSS,
        _ => theme::INK_4,
    };
    let date = clock.date(game.when);
    cell(kit, row, &date, 92.0, theme::INK_3);
    if let Some(hero) = game.hero {
        kit.hero_icon(row, hero.0, 50.0);
    }
    cell(kit, row, game.hero_name(), 136.0, theme::INK_1);
    cell(
        kit,
        row,
        match game.result {
            ResultKind::Won => "WON",
            ResultKind::Lost => "LOST",
            ResultKind::Draw => "DRAW",
            ResultKind::Undecided => "—",
        },
        70.0,
        color,
    );
    cell(kit, row, &game.length(), 68.0, theme::INK_3);
    cell(
        kit,
        row,
        super::history::mode_name(&game.mode),
        110.0,
        theme::INK_3,
    );
    if full {
        cell(kit, row, &game.kda_text(), 104.0, theme::INK_1);
        cell(
            kit,
            row,
            &game
                .net_worth
                .map_or_else(|| "—".to_owned(), |n| format!("{:.1}k", n as f64 / 1000.0)),
            80.0,
            theme::GOLD,
        );
    }
}

fn match_actions(kit: &mut Kit, row: Entity, game: &Match) {
    for (label, action, available) in [
        (
            "WATCH",
            Act::WatchHistory(game.file.clone()),
            game.watchable,
        ),
        (
            "DETAILS",
            Act::MatchDetails(game.file.clone()),
            game.details_available,
        ),
    ] {
        let button = if available {
            kit.button_role(
                row,
                label,
                action,
                Tone::Secondary,
                40.0,
                theme::TABLE_ACTION,
            )
        } else {
            let disabled = kit.disabled_button_role(row, label, 40.0, theme::TABLE_ACTION);
            kit.commands
                .entity(disabled)
                .remove::<BackgroundGradient>()
                .insert(BackgroundColor(theme::REPLAY_ACTION_OFF));
            disabled
        };
        kit.commands
            .entity(button)
            .entry::<Node>()
            .and_modify(|mut node| node.width = px(92));
    }
}

fn cell(kit: &mut Kit, parent: Entity, text: &str, width: f32, color: Color) {
    let cell = kit.node(
        parent,
        Node {
            width: px(width),
            flex_shrink: 0.0,
            ..default()
        },
    );
    kit.text(cell, text, 13.0, color);
}

/// The existing results renderer consumes the local metadata without opening a replay.
pub fn details(kit: &mut Kit, root: Entity, summary: &crate::postgame::summary::MatchSummary) {
    let mut renderer = crate::postgame::draw::Kit {
        commands: &mut kit.commands,
        fonts: kit.fonts,
        manifest: kit.manifest,
        server: kit.server,
    };
    let screen = crate::postgame::draw::spawn(&mut renderer, root, summary);
    // The menu owns this instance; an update of the game HUD's result must not despawn it.
    kit.commands
        .entity(screen)
        .remove::<crate::postgame::draw::Screen>()
        .insert(GlobalZIndex(62));
    kit.commands
        .entity(screen)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.top = px(54);
            node.height = auto();
            node.bottom = px(0);
        });
    let close = kit.button(root, "CLOSE", Act::Go(Page::Replays), Tone::Secondary, 40.0);
    kit.commands
        .entity(close)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.position_type = PositionType::Absolute;
            node.right = px(24);
            node.top = px(64);
        });
    kit.commands.entity(close).insert(GlobalZIndex(63));
}

fn caption(kit: &mut Kit, parent: Entity, label: &str) {
    let frame = kit.node(
        parent,
        Node {
            height: px(18),
            align_self: AlignSelf::FlexStart,
            padding: UiRect::axes(px(8), px(0)),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.commands
        .entity(frame)
        .insert(BackgroundColor(Color::BLACK));
    kit.typed(frame, label, theme::CAPTION, theme::INK_4);
}
