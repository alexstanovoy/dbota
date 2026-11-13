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
    let _ = matches;
    kit.typed(tabs, "PROFILE", theme::LABEL, theme::INK_1);
}

pub struct Profile<'a> {
    pub name: &'a str,
    pub connected: bool,
    pub read_only: bool,
    pub ratings: &'a [Rating],
    pub matches: &'a [Match],
    pub avatar: &'a super::avatar::View,
    pub notice: Option<&'a str>,
    pub public_page: Option<(usize, usize)>,
}

pub fn profile(kit: &mut Kit, root: Entity, data: Profile<'_>, clock: super::page_time::Clock) {
    let page = profile_page(kit, root);
    tabs(kit, page, false, data.name);
    profile_header(
        kit,
        page,
        data.name,
        data.connected && !data.read_only,
        data.avatar,
        data.read_only,
    );
    if let Some(notice) = data.notice {
        let text=kit.text(page, notice, 14.0, theme::INK_3);
        kit.commands.entity(text).entry::<Node>().and_modify(|mut node|node.margin=UiRect::top(px(12)));
    }
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
            max_height: vh(70),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    );
    kit.commands.entity(left).insert(ScrollPosition::default());
    if data.connected && data.ratings.iter().any(|rating| rating.games > 0) {
        rating_cards(kit, left, data.ratings);
    }
    recent_matches(kit, left, data.matches, clock, data.public_page);
    most_played_column(kit, body, data.matches);
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
    read_only: bool,
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
    if read_only {
        kit.button_role(
            header,
            "BACK",
            Act::Back,
            Tone::Secondary,
            46.0,
            theme::TABLE_ACTION,
        );
    } else if connected {
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

fn recent_matches(
    kit: &mut Kit,
    left: Entity,
    matches: &[Match],
    clock: super::page_time::Clock,
    public_page: Option<(usize, usize)>,
) {
    let title = kit.node(
        left,
        Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    caption(kit, title, "MATCHES");
    if let Some((page, total)) = public_page {
        if page > 0 {
            kit.quiet(
                title,
                "NEWER MATCHES",
                Act::ProfileMatchesPage(-1),
                theme::BODY,
                theme::LINK,
            );
        }
        if (page + 1) * super::player_profile::PAGE_SIZE < total {
            kit.quiet(
                title,
                "OLDER MATCHES",
                Act::ProfileMatchesPage(1),
                theme::BODY,
                theme::LINK,
            );
        }
    }
    if public_page.is_some() && matches.is_empty() {
        kit.text(
            left,
            "No public matches reported by this matchmaker",
            14.0,
            theme::INK_3,
        );
    } else if public_page.is_some() {
        for game in matches {
            match_row(kit, left, game, false, clock, true);
        }
    } else {
        rows(kit, left, matches, false, clock);
    }
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
        match_row(kit, parent, game, full, clock, false);
    }
}

fn match_row(
    kit: &mut Kit,
    parent: Entity,
    game: &Match,
    full: bool,
    clock: super::page_time::Clock,
    public: bool,
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
    if game.details_available {
        use bevy::ui_widgets::Button;
        kit.commands.entity(row).insert((
            Button,
            Pickable::default(),
            super::kit::MenuButton(Act::MatchDetails(game.file.clone())),
            Name::new(format!("profile match {}", game.file)),
            super::kit::Wash { target: row, rest: theme::HISTORY_ROW, hover: theme::SECONDARY.1 },
            bevy::input_focus::tab_navigation::TabIndex::default(),
        ));
    }
    match_numbers(kit, row, game, full, clock, public);
    kit.node(
        row,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    if !public {
        match_actions(kit, row, game);
    }
}

fn match_numbers(
    kit: &mut Kit,
    row: Entity,
    game: &Match,
    full: bool,
    clock: super::page_time::Clock,
    public: bool,
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
    let hero = if public && game.hero.is_none() {
        "—"
    } else {
        game.hero_name()
    };
    cell(kit, row, hero, 136.0, theme::INK_1);
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
