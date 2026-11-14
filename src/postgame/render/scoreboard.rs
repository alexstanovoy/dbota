use super::super::{
    graphs,
    summary::{Detail, Row},
    view::{Action, View},
};
#[cfg(test)]
use super::SeatRow;
use super::{Cell, primitives::*};
use crate::{main_menu::kit::Kit, theme};
use bevy::prelude::*;
use bota_proto::Team;
use bota_replay::meta::stat;

pub const CELLS: [(Cell, &str, f32); 11] = [
    (Cell::Level, "LVL", 34.0),
    (Cell::NetWorth, "NET WORTH", 74.0),
    (Cell::Kills, "K", 28.0),
    (Cell::Deaths, "D", 28.0),
    (Cell::Assists, "A", 28.0),
    (Cell::LastHits, "LH / DN", 76.0),
    (Cell::Gpm, "GPM", 42.0),
    (Cell::Xpm, "XPM", 42.0),
    (Cell::HeroDamage, "HERO DMG", 66.0),
    (Cell::TowerDamage, "BLD DMG", 60.0),
    (Cell::Healing, "HEALING", 60.0),
];

pub fn value(view: &View, row: &Row, cell: Cell) -> Option<i64> {
    let stat_id = match cell {
        Cell::Level => stat::LEVEL,
        Cell::NetWorth => stat::NET_WORTH,
        Cell::Kills => stat::KILLS,
        Cell::Deaths => stat::DEATHS,
        Cell::Assists => stat::ASSISTS,
        Cell::LastHits => stat::LAST_HITS,
        Cell::Denies => stat::DENIES,
        Cell::HeroDamage => stat::HERO_DAMAGE,
        Cell::TowerDamage => stat::STRUCTURE_DAMAGE,
        Cell::Healing => stat::HEALING,
        Cell::Gpm => stat::GOLD_EARNED,
        Cell::Xpm => stat::XP,
    };
    if let Some(recording) = &view.summary.recording {
        let value = recording.value(row.slot, stat_id, view.summary.seconds)?;
        return Some(if matches!(cell, Cell::Gpm | Cell::Xpm) {
            value.saturating_mul(60) / i64::from(view.summary.seconds.max(1))
        } else {
            value
        });
    }
    match cell {
        Cell::Level => Some(i64::from(row.level)),
        Cell::NetWorth => row.net_worth.map(i64::from),
        Cell::Kills => Some(i64::from(row.kills)),
        Cell::Deaths => Some(i64::from(row.deaths)),
        Cell::Assists => Some(i64::from(row.assists)),
        Cell::LastHits => row.last_hits.map(i64::from),
        Cell::Denies => row.denies.map(i64::from),
        Cell::Gpm => row.gpm.map(i64::from),
        Cell::Xpm => row.xpm.map(i64::from),
        Cell::HeroDamage => row.hero_damage.map(i64::from),
        Cell::TowerDamage => row.tower_damage.map(i64::from),
        Cell::Healing => row.healing.map(i64::from),
    }
}

pub fn formatted(view: &View, row: &Row, cell: Cell) -> String {
    let Some(value) = value(view, row, cell) else {
        return "—".to_owned();
    };
    if cell == Cell::LastHits {
        return format!("{value} / {}", formatted(view, row, Cell::Denies));
    }
    if matches!(
        cell,
        Cell::NetWorth | Cell::HeroDamage | Cell::TowerDamage | Cell::Healing
    ) {
        graphs::compact(value)
    } else {
        value.to_string()
    }
}

pub fn best(view: &View, row: &Row, cell: Cell) -> bool {
    if !view.summary.comparisons_known() || cell == Cell::Level {
        return false;
    }
    let Some(own) = value(view, row, cell) else {
        return false;
    };
    let values: Option<Vec<_>> = view
        .summary
        .rows
        .iter()
        .map(|other| value(view, other, cell))
        .collect();
    let Some(values) = values else {
        return false;
    };
    let target = if cell == Cell::Deaths {
        values.iter().min()
    } else {
        values.iter().max()
    };
    target == Some(&own) && values.iter().filter(|value| **value == own).count() == 1
}

pub fn draw(kit: &mut Kit, parent: Entity, view: &View) {
    if view.summary.rows.is_empty() {
        words(
            kit,
            parent,
            "No local recording is available for this match",
            at(32.0, 40.0),
            body(18.0),
            theme::INK_2,
        );
        words(
            kit,
            parent,
            "The outcome is shown above. Recorded statistics are unavailable.",
            at(32.0, 74.0),
            body(14.0),
            theme::INK_4,
        );
        return;
    }
    let mut y = 12.0;
    for team in [Team::Radiant, Team::Dire] {
        let rows: Vec<_> = view
            .summary
            .rows
            .iter()
            .filter(|row| row.team == team)
            .collect();
        team_header(kit, parent, view, team, y);
        y += 32.0;
        for row in rows {
            row_line(kit, parent, view, row, y);
            y += 48.0;
        }
        totals(kit, parent, view, team, y);
        y += 38.0;
    }
}

fn columns(view: &View) -> Vec<(Cell, &'static str, f32)> {
    if view.summary.detail != Detail::Stats {
        return CELLS.to_vec();
    }
    [
        (Cell::Level, "LVL", 44.0),
        (Cell::Kills, "K", 44.0),
        (Cell::Deaths, "D", 44.0),
        (Cell::Assists, "A", 44.0),
        (Cell::LastHits, "LH / DN", 84.0),
        (Cell::Gpm, "GPM", 64.0),
        (Cell::Xpm, "XPM", 64.0),
        (Cell::HeroDamage, "HERO DMG", 96.0),
        (Cell::TowerDamage, "BLD DMG", 96.0),
    ]
    .to_vec()
}

fn widths(view: &View) -> (f32, f32, Vec<(Cell, &'static str, f32)>) {
    let columns = columns(view);
    let gap = if view.summary.detail == Detail::Stats {
        10.0
    } else {
        8.0
    };
    let items = if view.summary.detail == Detail::Stats {
        0.0
    } else {
        330.0 + gap
    };
    let name = 1202.0 - 16.0 - columns.iter().map(|(_, _, width)| width + gap).sum::<f32>() - items;
    (name, gap, columns)
}

fn team_header(kit: &mut Kit, parent: Entity, view: &View, team: Team, y: f32) {
    let (name, gap, columns) = widths(view);
    let side = if team == Team::Dire {
        "DIRE"
    } else {
        "RADIANT"
    };
    let outcome = match view.summary.winner {
        Some(winner) if winner == team => " VICTORY",
        Some(Team::Neutral) | None => "",
        Some(_) => " DEFEAT",
    };
    words(
        kit,
        parent,
        format!("{side}{outcome}"),
        at(26.0, y + 8.0),
        theme::Type {
            size: 16.0,
            tracking: 2.0,
            ..theme::HEADING
        },
        team_colour(team),
    );
    let mut x = 26.0 + name + gap;
    for (_, label, width) in columns {
        let e = words(
            kit,
            parent,
            label,
            area(x, y + 8.0, width, 16.0),
            caption(11.0),
            theme::INK_4,
        );
        kit.commands
            .entity(e)
            .insert(TextLayout::justify(Justify::Right));
        x += width + gap;
    }
    if view.summary.detail != Detail::Stats {
        words(
            kit,
            parent,
            "ITEMS",
            at(x + 12.0, y + 8.0),
            caption(11.0),
            theme::INK_4,
        );
    }
}

fn row_line(kit: &mut Kit, parent: Entity, view: &View, row: &Row, y: f32) {
    let selected = view.context.owner == Some(row.slot);
    let bg = if view.hover_player == Some(row.slot) {
        theme::RESULT_HOVER_ROW
    } else if selected {
        theme::RESULT_SELF_ROW
    } else if row.team == Team::Radiant {
        theme::RESULT_RADIANT_ROW
    } else {
        theme::RESULT_DIRE_ROW
    };
    let node = area(18.0, y, 1202.0, 46.0);
    let line = if view.summary.detail == Detail::Meta {
        button(
            kit,
            parent,
            &format!("results player {}", row.slot.0),
            node,
            Action::Player(row.slot),
            bg,
        )
    } else {
        rect(kit, parent, node, bg)
    };
    #[cfg(test)]
    kit.commands.entity(line).insert(SeatRow(row.slot));
    if row.left_at.is_some() {
        kit.commands.entity(line).insert(super::Dim);
    }
    portrait(kit, line, row, area(8.0, 7.0, 54.0, 30.0), false);
    let (name, gap, columns) = widths(view);
    let identity = kit.node(
        line,
        Node {
            overflow: Overflow::clip(),
            ..area(70.0, 6.0, (name - 62.0).max(60.0), 36.0)
        },
    );
    let name_line = kit.node(
        identity,
        Node {
            height: px(17),
            align_items: AlignItems::Center,
            column_gap: px(6),
            ..default()
        },
    );
    profile_name(
        kit,
        name_line,
        view,
        row,
        Node::default(),
        bold(14.0),
        if selected {
            theme::LINK.1
        } else {
            theme::INK_1
        },
        SCOREBOARD_NAME_WIDTH,
    );
    if row.bot == Some(true) {
        bot(kit, name_line, Node::default());
    }
    if let (Some(tick), Some(recording)) = (row.left_at, &view.summary.recording) {
        words(
            kit,
            name_line,
            format!("LEFT {}", graphs::clock(recording.second(tick))),
            Node::default(),
            caption(10.0),
            theme::DIRE_TEXT,
        );
    }
    words(
        kit,
        identity,
        row.hero_name(kit.texts),
        at(0.0, 17.0),
        body(12.0),
        theme::INK_4,
    );
    let mut x = 8.0 + name + gap;
    for (cell, _, width) in columns {
        cell_value(kit, line, view, row, cell, area(x, 0.0, width, 46.0));
        x += width + gap;
    }
    if view.summary.detail != Detail::Stats {
        items(
            kit,
            line,
            view,
            row,
            Vec2::new(x + 12.0, 11.0),
            Vec2::new(30.0, 22.0),
            24.0,
        );
    }
}

pub fn cell_value(kit: &mut Kit, parent: Entity, view: &View, row: &Row, cell: Cell, node: Node) {
    let holder = kit.node(
        parent,
        Node {
            justify_content: JustifyContent::FlexEnd,
            align_items: AlignItems::Center,
            ..node
        },
    );
    let highlight = best(view, row, cell);
    let value_box = kit.node(
        holder,
        Node {
            border: UiRect::all(px(if highlight { 1 } else { 0 })),
            padding: UiRect::axes(px(if highlight { 5 } else { 0 }), px(1)),
            ..default()
        },
    );
    if highlight {
        kit.commands
            .entity(value_box)
            .insert(BorderColor::all(theme::GOLD));
    }
    let text = words(
        kit,
        value_box,
        formatted(view, row, cell),
        Node::default(),
        if highlight { bold(14.0) } else { number(14.0) },
        if highlight { theme::GOLD } else { theme::INK_1 },
    );
    kit.commands.entity(text).insert(cell);
}

pub fn items(
    kit: &mut Kit,
    parent: Entity,
    view: &View,
    row: &Row,
    origin: Vec2,
    size: Vec2,
    backpack_width: f32,
) {
    let (x, y, width, height) = (origin.x, origin.y, size.x, size.y);
    let mut left = x;
    for slot in 0..6 {
        super::hover::recorded_item(kit, parent, view, row, slot, area(left, y, width, height));
        left += width + 3.0;
    }
    left += 8.0;
    for slot in 6..9 {
        super::hover::recorded_item(
            kit,
            parent,
            view,
            row,
            slot,
            area(left, y + 2.0, backpack_width, height - 4.0),
        );
        left += backpack_width + 3.0;
    }
    let neutral = rect(
        kit,
        parent,
        Node {
            border_radius: BorderRadius::MAX,
            border: UiRect::all(px(1)),
            ..area(left + 8.0, y, 26.0, 26.0)
        },
        theme::SECONDARY.1,
    );
    kit.commands.entity(neutral).insert((
        BorderColor::all(theme::AVATAR_EDGE),
        Name::new("results neutral slot"),
    ));
}

fn totals(kit: &mut Kit, parent: Entity, view: &View, team: Team, y: f32) {
    let (name, gap, columns) = widths(view);
    let mut x = 26.0 + name + gap;
    for (cell, _, width) in columns {
        if matches!(cell, Cell::NetWorth | Cell::Kills) {
            let id = if cell == Cell::Kills {
                stat::KILLS
            } else {
                stat::NET_WORTH
            };
            let text = view
                .summary
                .team_total(team, id)
                .map_or_else(|| "—".to_owned(), graphs::compact);
            let e = words(
                kit,
                parent,
                text,
                area(x, y + 7.0, width, 20.0),
                number(14.0),
                if cell == Cell::NetWorth {
                    theme::GOLD
                } else {
                    theme::INK_3
                },
            );
            kit.commands
                .entity(e)
                .insert(TextLayout::justify(Justify::Right));
        }
        x += width + gap;
    }
}

/// The widest a player's name may draw in a scoreboard row.
const SCOREBOARD_NAME_WIDTH: f32 = 200.0;
