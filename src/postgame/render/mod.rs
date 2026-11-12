//! One result window for a just-finished game and a recording opened from local history.

mod cards;
pub mod chart;
pub mod hover;
mod player;
pub mod primitives;
mod scoreboard;

#[cfg(test)]
use super::graphs::Metric;
use super::{
    graphs,
    view::{Action, Page, View},
};
use crate::{main_menu::kit::Kit, theme, ui};
use bevy::prelude::*;
use bota_proto::Team;
use primitives::*;

pub const WIDTH: f32 = 1240.0;
pub const HEIGHT: f32 = 820.0;
pub const CONTENT_HEIGHT: f32 = 702.0;

#[derive(Component)]
pub struct Screen;
// These markers expose the actual drawn geometry to headless UI contracts only.
#[cfg(test)]
#[derive(Component, Clone, Copy)]
pub struct SeatRow(pub bota_proto::SlotId);
#[derive(Component)]
pub struct ItemSlot;
#[derive(Component)]
pub struct Banner;
#[derive(Component)]
pub struct Dim;
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Level,
    Kills,
    Deaths,
    Assists,
    LastHits,
    Denies,
    NetWorth,
    Gpm,
    Xpm,
    HeroDamage,
    TowerDamage,
    Healing,
}
#[derive(Component)]
pub struct Portrait;
#[cfg(test)]
#[derive(Component)]
pub struct Dot {
    pub graph: Metric,
}
#[cfg(test)]
#[derive(Component)]
pub struct Segment(pub Metric);

pub fn spawn(kit: &mut Kit, root: Entity, view: &View) {
    let screen = kit.node(root, ui::fill());
    kit.commands
        .entity(screen)
        .insert((
            Screen,
            crate::hud::Blocker,
            bevy::picking::hover::Hovered::default(),
        ))
        .insert(Pickable::default());
    if !view.context.history {
        let background = match view.summary.winner {
            Some(Team::Radiant) => Some("menu.postgame.radiant"),
            Some(Team::Dire) => Some("menu.postgame.dire"),
            _ => None,
        };
        let ground = rect(
            kit,
            screen,
            Node {
                top: px(theme::BAR_HEIGHT),
                ..ui::fill()
            },
            theme::GROUND,
        );
        if let Some(background) = background {
            let art = kit.art(ground, background, ui::fill());
            kit.commands.entity(art).insert(ui::Cover);
        }
    }
    rect(
        kit,
        screen,
        ui::fill(),
        Color::srgba(
            0.0,
            0.0,
            0.0,
            theme::dim(if view.context.history { 0.62 } else { 0.4 }),
        ),
    );
    let window = rect(
        kit,
        screen,
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: px(54),
            margin: UiRect::left(px(-WIDTH / 2.0)),
            width: px(WIDTH),
            height: px(HEIGHT),
            border: UiRect::all(px(1)),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        PANEL,
    );
    kit.commands.entity(window).insert((
        BorderColor::all(EDGE),
        Name::new("match results window"),
        theme::window_shadow(),
    ));
    gradient(kit, window, ui::fill(), PANEL, PANEL_DARK);
    header(kit, window, view);
    let content = kit.node(
        window,
        Node {
            position_type: PositionType::Relative,
            width: percent(100),
            height: px(CONTENT_HEIGHT),
            flex_shrink: 0.0,
            ..default()
        },
    );
    if let Some(row) = view.player.and_then(|slot| view.row(slot)) {
        player::draw(kit, content, view, row);
    } else {
        match view.page {
            Page::Mvp => cards::mvp(kit, content, view),
            Page::Radiant => cards::team(kit, content, view, Team::Radiant),
            Page::Dire => cards::team(kit, content, view, Team::Dire),
            Page::Scoreboard => scoreboard::draw(kit, content, view),
            Page::Graphs => chart::draw(kit, content, view),
        }
    }
    footer(kit, window, view);
}

fn header(kit: &mut Kit, parent: Entity, view: &View) {
    let head = rect(
        kit,
        parent,
        Node {
            position_type: PositionType::Relative,
            width: percent(100),
            height: px(52),
            flex_shrink: 0.0,
            border: UiRect::bottom(px(1)),
            ..default()
        },
        Color::NONE,
    );
    kit.commands
        .entity(head)
        .insert(BorderColor::all(theme::DIVIDER));
    let (title, colour) = match view.summary.winner {
        Some(Team::Radiant) => ("RADIANT VICTORY", theme::RADIANT_TEXT),
        Some(Team::Dire) => ("DIRE VICTORY", theme::DIRE_TEXT),
        Some(Team::Neutral) => ("DRAW", theme::INK_3),
        None => ("NO RESULT", theme::INK_3),
    };
    let title_node = words(
        kit,
        head,
        title,
        at(20.0, 16.0),
        theme::Type {
            size: 18.0,
            tracking: 2.0,
            ..theme::HEADING
        },
        colour,
    );
    kit.commands.entity(title_node).insert(Banner);
    let score = |team| {
        view.summary
            .team_total(team, bota_replay::meta::stat::KILLS)
            .map_or_else(|| "—".to_owned(), |value| value.to_string())
    };
    let x = match title {
        "RADIANT VICTORY" => 225.0,
        "DIRE VICTORY" => 190.0,
        "NO RESULT" => 150.0,
        _ => 100.0,
    };
    let radiant = score(Team::Radiant);
    let dire = score(Team::Dire);
    let radiant_width = radiant.chars().count() as f32 * 10.0;
    let dire_left = x + radiant_width + 16.0;
    let score_text = words(
        kit,
        head,
        radiant,
        at(x, 16.0),
        number(18.0),
        theme::RADIANT_TEXT,
    );
    kit.commands
        .entity(score_text)
        .insert(Name::new("results radiant score"));
    words(
        kit,
        head,
        "–",
        at(x + radiant_width + 3.0, 16.0),
        number(18.0),
        theme::INK_OFF,
    );
    words(
        kit,
        head,
        dire.clone(),
        at(dire_left, 16.0),
        number(18.0),
        theme::DIRE_TEXT,
    );
    let mut next = dire_left + dire.chars().count() as f32 * 10.0 + 16.0;
    if !view.context.reason.is_empty() {
        let width = (view.context.reason.chars().count() as f32 * 7.0 + 18.0).min(200.0);
        let badge = rect(
            kit,
            head,
            Node {
                border: UiRect::all(px(1)),
                ..area(next, 13.0, width, 24.0)
            },
            Color::NONE,
        );
        kit.commands
            .entity(badge)
            .insert(BorderColor::all(theme::TOOLTIP_EDGE));
        words(
            kit,
            badge,
            view.context.reason.clone(),
            at(8.0, 5.0),
            caption(11.0),
            theme::INK_3,
        );
        next += width + 14.0;
    }
    if let Some((before, after)) = view.context.rating {
        let delta = after.saturating_sub(before);
        let text = if delta == 0 {
            "Rating unchanged".to_owned()
        } else {
            format!("Rating {delta:+} → {}", grouped(after))
        };
        words(
            kit,
            head,
            text,
            at(next, 19.0),
            bold(13.0),
            if delta > 0 {
                theme::RADIANT_TEXT
            } else if delta < 0 {
                theme::DIRE_TEXT
            } else {
                theme::INK_4
            },
        );
    }
    let mode = format!(
        "{}{}",
        view.context.mode.to_uppercase(),
        view.context.kind.suffix()
    );
    let meta = kit.node(
        head,
        Node {
            position_type: PositionType::Absolute,
            right: px(65),
            top: px(10),
            max_width: px(410),
            overflow: Overflow::clip(),
            align_items: AlignItems::FlexEnd,
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    words(
        kit,
        meta,
        mode,
        Node::default(),
        caption(13.0),
        theme::INK_3,
    );
    let id = view
        .context
        .match_id
        .filter(|id| *id != 0)
        .map_or(String::new(), |id| format!("Match {id} · "));
    words(
        kit,
        meta,
        format!(
            "{id}{}{}{}",
            view.context.when,
            if view.context.when.is_empty() {
                ""
            } else {
                " · "
            },
            graphs::clock(view.summary.seconds)
        ),
        Node::default(),
        body(12.0),
        theme::INK_5,
    );
    let close = button(
        kit,
        head,
        "results close",
        area(WIDTH - 54.0, 3.0, 44.0, 44.0),
        Action::Close,
        Color::NONE,
    );
    let art = kit.art(close, "menu.window_close", area(12.0, 12.0, 20.0, 20.0));
    kit.commands
        .entity(art)
        .insert(Name::new("results close icon"));
}

fn footer(kit: &mut Kit, parent: Entity, view: &View) {
    let foot = rect(
        kit,
        parent,
        Node {
            position_type: PositionType::Relative,
            width: percent(100),
            height: px(64),
            flex_shrink: 0.0,
            border: UiRect::top(px(1)),
            ..default()
        },
        Color::NONE,
    );
    kit.commands
        .entity(foot)
        .insert(BorderColor::all(theme::DIVIDER));
    if !view.context.history {
        word_button(
            kit,
            foot,
            "CONTINUE",
            area(16.0, 10.0, 146.0, 44.0),
            Action::Continue,
            true,
        );
    }
    if let Some(notice) = &view.notice {
        words(
            kit,
            foot,
            notice.clone(),
            at(190.0, -26.0),
            body(13.0),
            theme::DIRE_TEXT,
        );
    }
    if view.context.replay.is_some() {
        word_button(
            kit,
            foot,
            "WATCH REPLAY",
            area(WIDTH - 198.0, 10.0, 180.0, 44.0),
            Action::Watch,
            false,
        );
    }
    let pages = view.pages();
    let width = 88.0 + (pages.len() + 1) as f32 * 8.0 + pages.len() as f32 * 48.0;
    let mut x = (WIDTH - width) / 2.0;
    pager(
        kit,
        foot,
        area(x, 10.0, 44.0, 44.0),
        "results previous",
        "←",
        Action::Previous,
        false,
        theme::INK_3,
    );
    x += 52.0;
    for page in pages {
        let (icon, colour) = match page {
            Page::Mvp => ("star", theme::INK_2),
            Page::Radiant => ("shield", theme::RADIANT_TEXT),
            Page::Dire => ("shield", theme::DIRE_TEXT),
            Page::Scoreboard => ("board", theme::INK_2),
            Page::Graphs => ("graph", theme::INK_2),
        };
        pager(
            kit,
            foot,
            area(x, 8.0, 48.0, 48.0),
            &format!("results {}", page.name()),
            icon,
            Action::Page(page),
            page == view.page,
            colour,
        );
        x += 56.0;
    }
    pager(
        kit,
        foot,
        area(x, 10.0, 44.0, 44.0),
        "results next",
        "→",
        Action::Next,
        false,
        theme::INK_3,
    );
}

#[allow(clippy::too_many_arguments)]
fn pager(
    kit: &mut Kit,
    parent: Entity,
    node: Node,
    name: &str,
    icon: &str,
    action: Action,
    selected: bool,
    colour: Color,
) {
    let entity = button(
        kit,
        parent,
        name,
        Node {
            border_radius: BorderRadius::MAX,
            border: UiRect::all(px(if selected { 2 } else { 1 })),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..node
        },
        action,
        if selected {
            Color::srgb_u8(0x3a, 0x4a, 0x57)
        } else {
            Color::srgb_u8(0x16, 0x1b, 0x20)
        },
    );
    kit.commands
        .entity(entity)
        .insert(BorderColor::all(if selected { theme::INK_3 } else { EDGE }));
    draw_icon(kit, entity, icon, colour);
}

fn draw_icon(kit: &mut Kit, parent: Entity, icon: &str, colour: Color) {
    let middle = if icon == "←" || icon == "→" {
        22.0
    } else {
        24.0
    };
    let points: Vec<Vec2> = match icon {
        "star" => (0..10)
            .map(|index| {
                let angle =
                    -std::f32::consts::FRAC_PI_2 + index as f32 * std::f32::consts::PI / 5.0;
                let radius = if index % 2 == 0 { 9.0 } else { 4.0 };
                Vec2::new(angle.cos() * radius, angle.sin() * radius)
            })
            .collect(),
        "shield" => vec![
            Vec2::new(-7.0, -7.0),
            Vec2::new(0.0, -10.0),
            Vec2::new(7.0, -7.0),
            Vec2::new(7.0, 1.0),
            Vec2::new(4.0, 6.0),
            Vec2::new(0.0, 9.0),
            Vec2::new(-4.0, 6.0),
            Vec2::new(-7.0, 1.0),
        ],
        "graph" => vec![
            Vec2::new(-7.0, 3.0),
            Vec2::new(-2.0, -2.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(7.0, -7.0),
        ],
        _ => Vec::new(),
    };
    let centre = Vec2::splat(middle);
    if !points.is_empty() {
        for pair in points.windows(2) {
            line(kit, parent, centre + pair[0], centre + pair[1], 1.7, colour);
        }
        if matches!(icon, "star" | "shield") {
            line(
                kit,
                parent,
                centre + points[0],
                centre + points[points.len() - 1],
                1.7,
                colour,
            );
        }
    }
    if icon == "board" {
        for y in [-6.0, 0.0, 6.0] {
            dot(kit, parent, centre + Vec2::new(-7.0, y), 2.0, colour);
            line(
                kit,
                parent,
                centre + Vec2::new(-2.0, y),
                centre + Vec2::new(7.0, y),
                1.7,
                colour,
            );
        }
    }
    if icon == "graph" {
        line(
            kit,
            parent,
            centre + Vec2::new(-8.0, 7.0),
            centre + Vec2::new(8.0, 7.0),
            1.7,
            colour,
        );
    }
    if icon == "←" || icon == "→" {
        let sign = if icon == "←" { -1.0 } else { 1.0 };
        line(
            kit,
            parent,
            centre + Vec2::new(-6.0, 0.0),
            centre + Vec2::new(6.0, 0.0),
            1.7,
            colour,
        );
        for y in [-5.0, 5.0] {
            line(
                kit,
                parent,
                centre + Vec2::new(6.0 * sign, 0.0),
                centre + Vec2::new(sign, y),
                1.7,
                colour,
            );
        }
    }
}

pub fn grouped(value: i32) -> String {
    let magnitude = value.unsigned_abs().to_string();
    let mut text = String::new();
    if value < 0 {
        text.push('−');
    }
    for (index, ch) in magnitude.chars().enumerate() {
        if index > 0 && (magnitude.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(ch);
    }
    text
}
