use super::super::{
    graphs,
    summary::{Detail, Row},
    view::{Action, View},
};
use super::{Cell, Portrait, primitives::*, scoreboard};
use crate::{main_menu::kit::Kit, theme};
use bevy::prelude::*;
use bota_proto::Team;

pub struct Superlative {
    pub rank: i32,
    pub value: String,
    pub label: String,
    pub colour: Color,
}

pub fn superlatives(view: &View, row: &Row) -> Vec<Superlative> {
    let mut result = Vec::new();
    if let Some(recording) = &view.summary.recording {
        let streak = recording.killstreak(row.slot);
        if streak >= 3 {
            result.push(Superlative {
                rank: -1,
                value: format!("{streak}×"),
                label: "KILLSTREAK".to_owned(),
                colour: theme::DIRE_TEXT,
            });
        }
    }
    if !view.summary.comparisons_known() {
        return result;
    }
    for (cell, label, colour) in [
        (Cell::Kills, "KILLS", theme::DIRE_TEXT),
        (Cell::Assists, "ASSISTS", theme::DIRE_TEXT),
        (Cell::NetWorth, "NET WORTH", theme::GOLD),
        (Cell::LastHits, "LAST HITS", theme::INK_2),
        (Cell::HeroDamage, "HERO DAMAGE", theme::INK_2),
        (Cell::TowerDamage, "BUILDING DAMAGE", theme::INK_2),
        (Cell::Healing, "HEALING", theme::RADIANT_TEXT),
        (Cell::Deaths, "DEATHS", theme::INK_2),
    ] {
        let Some(value) = scoreboard::value(view, row, cell) else {
            continue;
        };
        if value == 0 && cell != Cell::Deaths {
            continue;
        }
        let values: Option<Vec<_>> = view
            .summary
            .rows
            .iter()
            .map(|row| scoreboard::value(view, row, cell))
            .collect();
        let Some(mut values) = values else {
            continue;
        };
        values.sort_unstable();
        values.dedup();
        if cell != Cell::Deaths {
            values.reverse();
        }
        let rank = values
            .iter()
            .position(|other| *other == value)
            .unwrap_or(values.len());
        if rank >= 3 {
            continue;
        }
        let prefix = if cell == Cell::Deaths {
            ["FEWEST", "SECOND FEWEST", "THIRD FEWEST"][rank]
        } else {
            ["MOST", "SECOND MOST", "THIRD MOST"][rank]
        };
        let text = if matches!(
            cell,
            Cell::NetWorth | Cell::HeroDamage | Cell::TowerDamage | Cell::Healing
        ) {
            graphs::compact(value)
        } else {
            value.to_string()
        };
        result.push(Superlative {
            rank: rank as i32,
            value: text,
            label: format!("{prefix} {label}"),
            colour,
        });
    }
    result.sort_by_key(|entry| entry.rank);
    result
}

pub fn mvp(kit: &mut Kit, parent: Entity, view: &View) {
    let Some(mvp) = view.mvp() else {
        scoreboard::draw(kit, parent, view);
        return;
    };
    let key = view.key_players();
    let width = if key.is_empty() { 1238.0 } else { 760.0 };
    let art_left = if key.is_empty() {
        (width - 420.0) / 2.0
    } else {
        200.0
    };
    let area_node = area(0.0, 0.0, width, 702.0);
    let panel = if view.summary.detail == Detail::Meta {
        button(
            kit,
            parent,
            "results mvp player",
            area_node,
            Action::Player(mvp.slot),
            Color::NONE,
        )
    } else {
        kit.node(parent, area_node)
    };
    let portrait = portrait(kit, panel, mvp, area(art_left, 24.0, 420.0, 556.0), true);
    kit.commands.entity(portrait).insert(Portrait);
    gradient(
        kit,
        panel,
        area(0.0, 362.0, width, 340.0),
        PANEL_DARK.with_alpha(0.0),
        PANEL_DARK.with_alpha(theme::dim(0.98)),
    );
    words(kit, panel, "MVP", at(40.0, 190.0), bold(56.0), theme::GOLD);
    for (index, entry) in superlatives(view, mvp).into_iter().take(5).enumerate() {
        stat_line(
            kit,
            panel,
            &entry,
            area(40.0, 254.0 + index as f32 * 46.0, 320.0, 44.0),
            34.0,
        );
    }
    let identity = kit.node(
        panel,
        Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: px(10),
            overflow: Overflow::clip(),
            ..area(20.0, 578.0, width - 40.0, 60.0)
        },
    );
    profile_name(
        kit,
        identity,
        view,
        mvp,
        Node::default(),
        bold(48.0),
        theme::INK_1,
    );
    if mvp.bot == Some(true) {
        bot(kit, identity, Node::default());
    }
    let hero = words(
        kit,
        panel,
        mvp.hero_name().to_uppercase(),
        area(20.0, 640.0, width - 40.0, 28.0),
        theme::Type {
            size: 18.0,
            tracking: 3.0,
            ..theme::HEADING
        },
        theme::INK_3,
    );
    kit.commands
        .entity(hero)
        .insert(TextLayout::justify(Justify::Center));
    if !key.is_empty() {
        let title = words(
            kit,
            parent,
            "KEY PLAYERS",
            area(780.0, 68.0, 420.0, 32.0),
            theme::Type {
                size: 22.0,
                tracking: 4.0,
                ..theme::HEADING
            },
            theme::INK_3,
        );
        kit.commands
            .entity(title)
            .insert(TextLayout::justify(Justify::Center));
        for (index, row) in key.iter().enumerate() {
            card(
                kit,
                parent,
                view,
                row,
                area(798.0 + index as f32 * 210.0, 112.0, 190.0, 300.0),
                2,
                false,
            );
        }
    }
}

pub fn team(kit: &mut Kit, parent: Entity, view: &View, team: Team) {
    let colour = team_colour(team);
    let title = words(
        kit,
        parent,
        if team == Team::Dire {
            "DIRE"
        } else {
            "RADIANT"
        },
        area(0.0, 22.0, 1238.0, 54.0),
        theme::Type {
            size: 44.0,
            tracking: 6.0,
            ..theme::HEADING
        },
        colour,
    );
    kit.commands
        .entity(title)
        .insert(TextLayout::justify(Justify::Center));
    let outcome = match view.summary.winner {
        Some(winner) if winner == team => "VICTORY",
        Some(Team::Neutral) => "DRAW",
        None => "NO RESULT",
        Some(_) => "DEFEAT",
    };
    let subtitle = words(
        kit,
        parent,
        outcome,
        area(0.0, 80.0, 1238.0, 32.0),
        theme::Type {
            size: 22.0,
            tracking: 3.0,
            ..theme::HEADING
        },
        theme::INK_3,
    );
    kit.commands
        .entity(subtitle)
        .insert(TextLayout::justify(Justify::Center));
    let rows: Vec<_> = view
        .summary
        .rows
        .iter()
        .filter(|row| row.team == team)
        .collect();
    let widths: Vec<_> = rows
        .iter()
        .map(|row| {
            if view.context.owner == Some(row.slot) {
                200.0
            } else {
                198.0
            }
        })
        .collect();
    let total = widths.iter().sum::<f32>() + rows.len().saturating_sub(1) as f32 * 16.0;
    let mut left = (1238.0 - total) / 2.0;
    for (row, width) in rows.iter().zip(widths) {
        let height = if view.context.owner == Some(row.slot) {
            424.0
        } else {
            422.0
        };
        card(
            kit,
            parent,
            view,
            row,
            area(left, 119.0, width, height),
            3,
            view.mvp().is_some_and(|mvp| mvp.slot == row.slot),
        );
        left += width + 16.0;
    }
    if rows.is_empty() {
        let text = words(
            kit,
            parent,
            "No players on this side",
            area(0.0, 240.0, 1238.0, 32.0),
            body(18.0),
            theme::INK_4,
        );
        kit.commands
            .entity(text)
            .insert(TextLayout::justify(Justify::Center));
    }
}

fn card(
    kit: &mut Kit,
    parent: Entity,
    view: &View,
    row: &Row,
    node: Node,
    lines: usize,
    mvp: bool,
) {
    let width = if let Val::Px(width) = node.width {
        width
    } else {
        190.0
    };
    let height = if let Val::Px(height) = node.height {
        height
    } else {
        300.0
    };
    let own = view.context.owner == Some(row.slot);
    let node = Node {
        height: px(height + 64.0),
        ..node
    };
    let shell = if view.summary.detail == Detail::Meta {
        button(
            kit,
            parent,
            &format!("results card {}", row.slot.0),
            node,
            Action::Player(row.slot),
            Color::NONE,
        )
    } else {
        kit.node(parent, node)
    };
    let picture = portrait(
        kit,
        shell,
        row,
        Node {
            border: UiRect::all(px(if own { 2 } else { 1 })),
            overflow: Overflow::clip(),
            ..area(0.0, 0.0, width, height)
        },
        true,
    );
    kit.commands.entity(picture).insert((
        Portrait,
        BorderColor::all(if own {
            theme::SELECT
        } else {
            team_colour(row.team).with_alpha(0.55)
        }),
    ));
    gradient(
        kit,
        picture,
        area(0.0, height - 170.0, width, 170.0),
        Color::BLACK.with_alpha(0.0),
        Color::BLACK.with_alpha(theme::dim(0.88)),
    );
    if mvp {
        let badge = rect(
            kit,
            picture,
            Node {
                border: UiRect::all(px(1)),
                ..area(8.0, 8.0, 48.0, 22.0)
            },
            Color::BLACK.with_alpha(0.75),
        );
        kit.commands
            .entity(badge)
            .insert(BorderColor::all(theme::GOLD));
        words(kit, badge, "MVP", at(7.0, 3.0), caption(12.0), theme::GOLD);
    }
    if !mvp
        && matches!(
            view.page,
            super::super::view::Page::Radiant | super::super::view::Page::Dire
        )
        && view
            .key_players()
            .iter()
            .any(|player| player.slot == row.slot)
    {
        let badge = rect(
            kit,
            picture,
            Node {
                border: UiRect::all(px(1)),
                ..area(8.0, 8.0, 94.0, 22.0)
            },
            Color::BLACK,
        );
        kit.commands
            .entity(badge)
            .insert(BorderColor::all(theme::GOLD));
        words(
            kit,
            badge,
            "KEY PLAYER",
            at(6.0, 4.0),
            caption(10.0),
            theme::GOLD,
        );
    }
    let stats = superlatives(view, row);
    let count = stats.len().min(lines);
    for (index, entry) in stats.iter().take(lines).enumerate() {
        stat_line(
            kit,
            picture,
            entry,
            area(
                12.0,
                height - 12.0 - (count - index) as f32 * 36.0,
                width - 20.0,
                34.0,
            ),
            26.0,
        );
    }
    let identity = kit.node(
        shell,
        Node {
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: px(6),
            overflow: Overflow::clip(),
            ..area(-8.0, height + 12.0, width + 16.0, 24.0)
        },
    );
    profile_name(
        kit,
        identity,
        view,
        row,
        Node::default(),
        bold(16.0),
        if own { theme::LINK.1 } else { theme::INK_1 },
    );
    if row.bot == Some(true) {
        bot(kit, identity, Node::default());
    }
    let hero = words(
        kit,
        shell,
        row.hero_name(),
        area(0.0, height + 38.0, width, 20.0),
        body(13.0),
        theme::INK_5,
    );
    kit.commands
        .entity(hero)
        .insert(TextLayout::justify(Justify::Center));
    if view.summary.detail == Detail::Meta {
        let chip = rect(
            kit,
            picture,
            area(width - 76.0, 8.0, 68.0, 24.0),
            theme::INK_2,
        );
        kit.commands.entity(chip).insert((
            super::hover::DetailsHint {
                button: shell,
                frame: picture,
                rest: if own {
                    theme::SELECT
                } else {
                    team_colour(row.team).with_alpha(0.55)
                },
                forced: view.hover_player == Some(row.slot),
            },
            crate::ui::visible(view.hover_player == Some(row.slot)),
        ));
        words(
            kit,
            chip,
            "DETAILS",
            at(7.0, 5.0),
            caption(11.0),
            PANEL_DARK,
        );
        if view.hover_player == Some(row.slot) {
            kit.commands
                .entity(picture)
                .insert(BorderColor::all(theme::INK_2));
        }
    }
}

fn stat_line(kit: &mut Kit, parent: Entity, entry: &Superlative, node: Node, size: f32) {
    let row = kit.node(
        parent,
        Node {
            align_items: AlignItems::Baseline,
            column_gap: px(8),
            ..node
        },
    );
    words(
        kit,
        row,
        entry.value.clone(),
        Node::default(),
        bold(size),
        entry.colour,
    );
    let label = words(
        kit,
        row,
        entry.label.clone(),
        Node {
            max_width: px(if size > 30.0 { 130.0 } else { 110.0 }),
            ..default()
        },
        caption(if size > 30.0 { 12.0 } else { 11.0 }),
        theme::INK_2,
    );
    kit.commands.entity(label).insert(TextLayout::default());
}
