//! Home's hero and local match cards, independent of the Play/search/accept overlay.
use super::{
    core::{Act, Page},
    history::{Match, ResultKind},
    kit::{Kit, MenuButton, Tone},
};
use crate::theme;
use bevy::prelude::*;
use bota_proto::HeroId;

pub fn draw(
    kit: &mut Kit,
    root: Entity,
    hero: Option<HeroId>,
    matches: &[Match],
    clock: super::page_time::Clock,
) {
    if let Some(hero) = hero {
        super::hero_stage::draw(kit, root, hero);
    }
    let cards = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            right: px(24),
            top: px(78),
            width: px(380),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            ..default()
        },
    );
    last_match(kit, cards, matches.first(), clock);
    recent_matches(kit, cards, matches);
}

fn last_match(kit: &mut Kit, cards: Entity, game: Option<&Match>, clock: super::page_time::Clock) {
    let last = kit.panel(
        cards,
        Node {
            width: percent(100),
            min_height: px(201),
            padding: UiRect::all(px(16)),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            ..default()
        },
    );
    panel_ground(kit, last);
    kit.typed(last, "LAST MATCH", theme::MATCH_CARD_HEADING, theme::INK_3);
    if let Some(game) = game {
        last_content(kit, last, game, clock);
    } else {
        kit.text(last, "No matches on this machine yet.", 15.0, theme::INK_4);
    }
}

fn last_content(kit: &mut Kit, last: Entity, game: &Match, clock: super::page_time::Clock) {
    let row = kit.node(
        last,
        Node {
            column_gap: px(12),
            padding: UiRect::all(px(8)),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    result_tint(kit, row, game);
    if let Some(hero) = game.hero {
        kit.hero_icon(row, hero.0, 112.0);
    }
    last_words(kit, row, game, clock);
    let button = last_details_button(kit, last, game);
    kit.commands
        .entity(button)
        .entry::<Node>()
        .and_modify(|mut node| node.width = percent(100));
}

fn last_details_button(kit: &mut Kit, last: Entity, game: &Match) -> Entity {
    let role = theme::MATCH_DETAILS_ACTION;
    if game.details_available {
        kit.button_role(
            last,
            "MATCH DETAILS",
            Act::MatchDetails(game.file.clone()),
            Tone::Secondary,
            46.0,
            role,
        )
    } else {
        kit.disabled_button_role(last, "MATCH DETAILS", 46.0, role)
    }
}

fn result_tint(kit: &mut Kit, row: Entity, game: &Match) {
    let tint = match game.result {
        ResultKind::Won => Some(Color::srgba(
            91.0 / 255.0,
            177.0 / 255.0,
            11.0 / 255.0,
            0.28,
        )),
        ResultKind::Lost => Some(Color::srgba(
            224.0 / 255.0,
            88.0 / 255.0,
            63.0 / 255.0,
            0.28,
        )),
        ResultKind::Draw | ResultKind::Undecided => None,
    };
    if let Some(tint) = tint {
        kit.commands
            .entity(row)
            .insert(BackgroundGradient::from(LinearGradient::to_right(vec![
                ColorStop::new(tint, percent(0)),
                ColorStop::new(tint.with_alpha(0.0), percent(80)),
            ])));
    }
}

fn last_words(kit: &mut Kit, row: Entity, game: &Match, clock: super::page_time::Clock) {
    let text = kit.node(
        row,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            flex_grow: 1.0,
            ..default()
        },
    );
    kit.typed(text, result(game), theme::MATCH_RESULT, color(game));
    kit.text(
        text,
        format!("{} · {}", game.hero_name(), game.kda_text()),
        14.0,
        theme::INK_2,
    );
    kit.text(
        text,
        format!(
            "{} · {} · {}",
            super::history::mode_name(&game.mode),
            game.length(),
            clock.ago(game.when)
        ),
        13.0,
        theme::INK_4,
    );
}

fn recent_matches(kit: &mut Kit, cards: Entity, matches: &[Match]) {
    let recent = kit.panel(
        cards,
        Node {
            width: percent(100),
            padding: UiRect::all(px(16)),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..default()
        },
    );
    panel_ground(kit, recent);
    let title = kit.node(
        recent,
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.typed(title, "RECENT", theme::MATCH_CARD_HEADING, theme::INK_3);
    let all = kit.quiet(
        title,
        "All matches",
        Act::Go(Page::Replays),
        theme::SMALL_BODY,
        theme::LINK,
    );
    kit.commands
        .entity(all)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.height = px(20);
            node.min_height = px(20);
        });
    for game in matches.iter().take(3) {
        recent_row(kit, recent, game);
    }
}

fn recent_row(kit: &mut Kit, recent: Entity, game: &Match) {
    let row = kit.node(
        recent,
        Node {
            width: percent(100),
            height: px(48),
            padding: UiRect::axes(px(8), px(0)),
            border: UiRect::left(px(4)),
            align_items: AlignItems::Center,
            column_gap: px(10),
            ..default()
        },
    );
    kit.commands.entity(row).insert((
        Name::new(format!("recent {}", game.file)),
        BackgroundColor(theme::MATCH_ROW),
        BorderColor {
            left: color(game),
            ..BorderColor::all(Color::NONE)
        },
    ));
    if game.details_available {
        kit.commands
            .entity(row)
            .insert((Button, MenuButton(Act::MatchDetails(game.file.clone()))));
    } else {
        kit.commands.entity(row).insert(super::kit::Disabled);
    }
    if let Some(hero) = game.hero {
        kit.hero_icon(row, hero.0, 54.0);
    }
    let words = kit.node(
        row,
        Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            row_gap: px(3),
            ..default()
        },
    );
    kit.bold(words, game.hero_name(), 14.0, theme::INK_1);
    kit.text(
        words,
        format!(
            "{} · {} · {}",
            short_result(game),
            super::history::mode_name(&game.mode),
            game.length()
        ),
        12.0,
        theme::INK_4,
    );
    kit.text(row, game.kda_text(), 13.0, theme::INK_3);
}

fn color(game: &Match) -> Color {
    match game.result {
        ResultKind::Won => theme::RADIANT_TEXT,
        ResultKind::Lost => theme::LOSS,
        _ => theme::INK_4,
    }
}
fn result(game: &Match) -> &'static str {
    match game.result {
        ResultKind::Won => "VICTORY",
        ResultKind::Lost => "DEFEAT",
        ResultKind::Draw => "DRAW",
        ResultKind::Undecided => "NO RESULT",
    }
}
fn short_result(game: &Match) -> &'static str {
    match game.result {
        ResultKind::Won => "Won",
        ResultKind::Lost => "Lost",
        ResultKind::Draw => "Draw",
        ResultKind::Undecided => "Unfinished",
    }
}

fn panel_ground(kit: &mut Kit, panel: Entity) {
    kit.commands
        .entity(panel)
        .insert(theme::downward(theme::PANEL));
}
