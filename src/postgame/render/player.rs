use super::super::{
    graphs,
    summary::Row,
    view::{Action, View},
};
use super::{Cell, Portrait, primitives::*, scoreboard};
use crate::{main_menu::kit::Kit, theme};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bota_proto::SlotId;
use bota_replay::meta::stat;

pub fn draw(kit: &mut Kit, parent: Entity, view: &View, row: &Row) {
    word_button(
        kit,
        parent,
        "← SCOREBOARD",
        area(18.0, 12.0, 150.0, 40.0),
        Action::Back,
        false,
    );
    let mut x = 1238.0 - 18.0 - view.summary.rows.len() as f32 * 46.0 + 10.0;
    for other in &view.summary.rows {
        let button = button(
            kit,
            parent,
            &format!("results select player {}", other.slot.0),
            Node {
                border_radius: BorderRadius::MAX,
                border: UiRect::all(px(if other.slot == row.slot { 3 } else { 2 })),
                overflow: Overflow::clip(),
                ..area(x, 14.0, 36.0, 36.0)
            },
            Action::Player(other.slot),
            PANEL_DARK,
        );
        round_portrait(kit, button, other, area(0.0, 0.0, 32.0, 32.0));
        kit.commands
            .entity(button)
            .insert(BorderColor::all(if other.slot == row.slot {
                theme::SELECT
            } else {
                team_colour(other.team).with_alpha(0.5)
            }));
        x += 46.0;
    }
    identity(kit, parent, view, row);
    tiles(kit, parent, view, row);
    values_at(kit, parent, view, row);
    kills(kit, parent, view, row);
    over_match(kit, parent, view, row);
}

fn identity(kit: &mut Kit, parent: Entity, view: &View, row: &Row) {
    let own = view.context.owner == Some(row.slot);
    let card = rect(
        kit,
        parent,
        Node {
            border: UiRect::all(px(2)),
            ..area(18.0, 66.0, 280.0, 404.0)
        },
        PANEL_DARK,
    );
    kit.commands.entity(card).insert(BorderColor::all(if own {
        theme::SELECT
    } else {
        team_colour(row.team)
    }));
    let picture = portrait(kit, card, row, area(0.0, 0.0, 276.0, 400.0), true);
    kit.commands.entity(picture).insert(Portrait);
    gradient(
        kit,
        card,
        area(0.0, 210.0, 276.0, 190.0),
        PANEL_DARK.with_alpha(0.0),
        PANEL_DARK,
    );
    let identity = kit.node(
        card,
        Node {
            align_items: AlignItems::Center,
            column_gap: px(8),
            overflow: Overflow::clip(),
            ..area(14.0, 336.0, 252.0, 36.0)
        },
    );
    words(
        kit,
        identity,
        row.display_name(),
        Node::default(),
        bold(28.0),
        if own { theme::LINK.1 } else { theme::INK_1 },
    );
    if row.bot == Some(true) {
        bot(kit, identity, Node::default());
    }
    words(
        kit,
        card,
        row.hero_name().to_uppercase(),
        at(14.0, 372.0),
        caption(15.0),
        theme::INK_3,
    );
    if view.mvp().is_some_and(|mvp| mvp.slot == row.slot) {
        let badge = rect(
            kit,
            card,
            Node {
                border: UiRect::all(px(1)),
                ..area(10.0, 10.0, 48.0, 22.0)
            },
            Color::BLACK,
        );
        kit.commands
            .entity(badge)
            .insert(BorderColor::all(theme::GOLD));
        words(kit, badge, "MVP", at(7.0, 3.0), caption(12.0), theme::GOLD);
    }
    words(
        kit,
        parent,
        format!("{} / {} / {}", row.kills, row.deaths, row.assists),
        at(18.0, 484.0),
        number(30.0),
        theme::INK_1,
    );
    let level = words(
        kit,
        parent,
        format!("LEVEL {}", row.level),
        area(178.0, 494.0, 120.0, 24.0),
        bold(15.0),
        theme::INK_3,
    );
    kit.commands
        .entity(level)
        .insert(TextLayout::justify(Justify::Right));
    for index in 0..6 {
        super::hover::recorded_item(
            kit,
            parent,
            view,
            row,
            index,
            area(18.0 + index as f32 * 43.0, 527.0, 40.0, 30.0),
        );
    }
    for index in 0..3 {
        super::hover::recorded_item(
            kit,
            parent,
            view,
            row,
            index + 6,
            area(18.0 + index as f32 * 33.0, 569.0, 30.0, 22.0),
        );
    }
    let neutral = rect(
        kit,
        parent,
        Node {
            border_radius: BorderRadius::MAX,
            border: UiRect::all(px(1)),
            ..area(126.0, 566.0, 28.0, 28.0)
        },
        theme::SECONDARY.1,
    );
    kit.commands
        .entity(neutral)
        .insert(BorderColor::all(theme::AVATAR_EDGE));
}

fn tiles(kit: &mut Kit, parent: Entity, view: &View, row: &Row) {
    let cells = [
        (Cell::NetWorth, "NET WORTH"),
        (Cell::LastHits, "LH / DN"),
        (Cell::Gpm, "GPM"),
        (Cell::Xpm, "XPM"),
        (Cell::HeroDamage, "HERO DMG"),
        (Cell::TowerDamage, "BLD DMG"),
        (Cell::Healing, "HEALING"),
    ];
    let width = (900.0 - 6.0 * 8.0) / 7.0;
    for (index, (cell, label)) in cells.into_iter().enumerate() {
        let best = scoreboard::best(view, row, cell);
        let tile = rect(
            kit,
            parent,
            Node {
                border: UiRect::all(px(1)),
                ..area(320.0 + index as f32 * (width + 8.0), 66.0, width, 76.0)
            },
            PANEL_DARK,
        );
        kit.commands.entity(tile).insert(BorderColor::all(if best {
            theme::GOLD
        } else {
            theme::DIVIDER
        }));
        words(kit, tile, label, at(10.0, 9.0), caption(11.0), theme::INK_4);
        words(
            kit,
            tile,
            scoreboard::formatted(view, row, cell),
            at(10.0, 27.0),
            number(22.0),
            theme::INK_1,
        );
        let rank = rank(view, row, cell);
        words(
            kit,
            tile,
            if best {
                "BEST".to_owned()
            } else {
                rank.map_or_else(
                    || "Not recorded".to_owned(),
                    |(rank, count)| format!("#{rank} of {count}"),
                )
            },
            at(10.0, 55.0),
            caption(11.0),
            if best { theme::GOLD } else { theme::INK_OFF },
        );
    }
}

fn rank(view: &View, row: &Row, cell: Cell) -> Option<(usize, usize)> {
    if !view.summary.comparisons_known() {
        return None;
    }
    let own = scoreboard::value(view, row, cell)?;
    let values: Option<Vec<_>> = view
        .summary
        .rows
        .iter()
        .map(|row| scoreboard::value(view, row, cell))
        .collect();
    let values = values?;
    Some((
        1 + values.iter().filter(|value| **value > own).count(),
        values.len(),
    ))
}

fn minute_columns(view: &View) -> Vec<u32> {
    let minutes = if view.summary.rows.len() == 2 {
        [300, 600, 900]
    } else {
        [600, 1200, 1800]
    };
    let mut columns: Vec<_> = minutes
        .into_iter()
        .filter(|second| *second < view.summary.seconds)
        .collect();
    columns.push(view.summary.seconds);
    columns
}

fn values_at(kit: &mut Kit, parent: Entity, view: &View, row: &Row) {
    let table = rect(
        kit,
        parent,
        Node {
            border: UiRect::all(px(1)),
            ..area(320.0, 156.0, 442.0, 250.0)
        },
        Color::srgb_u8(0x11, 0x15, 0x1a),
    );
    kit.commands
        .entity(table)
        .insert(BorderColor::all(theme::DIVIDER));
    let columns = minute_columns(view);
    words(
        kit,
        table,
        "AT",
        at(12.0, 11.0),
        caption(11.0),
        theme::INK_4,
    );
    let width = (442.0 - 130.0) / columns.len() as f32;
    for (index, second) in columns.iter().enumerate() {
        let label = if *second == view.summary.seconds {
            "END".to_owned()
        } else {
            graphs::clock(*second)
        };
        let e = words(
            kit,
            table,
            label,
            area(130.0 + index as f32 * width, 11.0, width - 12.0, 18.0),
            caption(11.0),
            theme::INK_4,
        );
        kit.commands
            .entity(e)
            .insert(TextLayout::justify(Justify::Right));
    }
    for (index, (id, label)) in [
        (stat::NET_WORTH, "Net worth"),
        (stat::LAST_HITS, "Last hits"),
        (stat::DENIES, "Denies"),
        (stat::LEVEL, "Level"),
        (stat::XP, "Experience"),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 32.0 + index as f32 * 43.0;
        rect(kit, table, area(0.0, y, 440.0, 1.0), theme::DIVIDER);
        words(
            kit,
            table,
            label,
            at(12.0, y + 14.0),
            body(13.0),
            theme::INK_3,
        );
        for (column, second) in columns.iter().enumerate() {
            let value = view
                .summary
                .recording
                .as_ref()
                .and_then(|recording| recording.value(row.slot, id, *second));
            let text = value.map_or_else(
                || "—".to_owned(),
                |value| {
                    if matches!(id, stat::NET_WORTH | stat::XP) {
                        graphs::compact(value)
                    } else {
                        value.to_string()
                    }
                },
            );
            let e = words(
                kit,
                table,
                text,
                area(130.0 + column as f32 * width, y + 14.0, width - 12.0, 18.0),
                number(13.0),
                theme::INK_1,
            );
            kit.commands
                .entity(e)
                .insert(TextLayout::justify(Justify::Right));
        }
    }
}

fn kills(kit: &mut Kit, parent: Entity, view: &View, row: &Row) {
    let panel = rect(
        kit,
        parent,
        Node {
            border: UiRect::all(px(1)),
            ..area(778.0, 156.0, 442.0, 250.0)
        },
        Color::srgb_u8(0x11, 0x15, 0x1a),
    );
    kit.commands
        .entity(panel)
        .insert(BorderColor::all(theme::DIVIDER));
    words(
        kit,
        panel,
        "KILLS AND DEATHS",
        at(12.0, 10.0),
        caption(11.0),
        theme::INK_4,
    );
    let list = kit.node(
        panel,
        Node {
            overflow: Overflow::scroll_y(),
            ..area(0.0, 32.0, 440.0, 216.0)
        },
    );
    kit.commands.entity(list).insert((
        crate::ui::ScrollArea,
        ScrollPosition::default(),
        RelativeCursorPosition::default(),
        bevy::picking::hover::Hovered::default(),
    ));
    let Some(recording) = &view.summary.recording else {
        return;
    };
    let events: Vec<_> = recording
        .kills
        .iter()
        .filter(|kill| kill.killer == Some(row.slot) || kill.victim == Some(row.slot))
        .take(1024)
        .collect();
    let content = kit.node(
        list,
        Node {
            position_type: PositionType::Relative,
            width: percent(100),
            height: px(events.len() as f32 * 30.0),
            flex_shrink: 0.0,
            ..default()
        },
    );
    for (index, event) in events.iter().enumerate() {
        let y = index as f32 * 30.0;
        rect(kit, content, area(0.0, y, 440.0, 1.0), theme::DIVIDER);
        words(
            kit,
            content,
            graphs::clock(event.second),
            at(12.0, y + 8.0),
            number(13.0),
            theme::INK_4,
        );
        let killed = event.killer == Some(row.slot);
        words(
            kit,
            content,
            if killed { "●" } else { "×" },
            at(66.0, y + 7.0),
            body(15.0),
            if killed {
                theme::RADIANT_TEXT
            } else {
                theme::DIRE_TEXT
            },
        );
        let other =
            if killed { event.victim } else { event.killer }.and_then(|slot| view.row(slot));
        let who = other.map_or_else(
            || "Other unit".to_owned(),
            |other| format!("{} · {}", other.hero_name(), other.display_name()),
        );
        let text = words(
            kit,
            content,
            format!("{} {who}", if killed { "Killed" } else { "Died to" }),
            Node {
                overflow: Overflow::clip(),
                ..area(96.0, y + 8.0, 275.0, 18.0)
            },
            body(13.0),
            theme::INK_1,
        );
        kit.commands.entity(text).insert(TextLayout::no_wrap());
        if killed && event.gold > 0 {
            let value = words(
                kit,
                content,
                format!("+{}", event.gold),
                area(366.0, y + 8.0, 62.0, 18.0),
                number(13.0),
                theme::GOLD,
            );
            kit.commands
                .entity(value)
                .insert(TextLayout::justify(Justify::Right));
        }
    }
}

fn over_match(kit: &mut Kit, parent: Entity, view: &View, row: &Row) {
    let panel = rect(
        kit,
        parent,
        Node {
            border: UiRect::all(px(1)),
            ..area(320.0, 420.0, 900.0, 266.0)
        },
        Color::srgb_u8(0x11, 0x15, 0x1a),
    );
    kit.commands
        .entity(panel)
        .insert(BorderColor::all(theme::DIVIDER));
    words(
        kit,
        panel,
        "OVER THE MATCH",
        at(12.0, 10.0),
        caption(11.0),
        theme::INK_4,
    );
    let plot = kit.node(panel, area(96.0, 32.0, 780.0, 208.0));
    let duration = view.summary.seconds.max(1) as f32;
    for minute in 0..view.summary.seconds / 60 + 1 {
        let second = minute * 60;
        rect(
            kit,
            plot,
            area(second as f32 / duration * 780.0, 0.0, 1.0, 208.0),
            if graphs::time_ticks(view.summary.seconds).contains(&second) {
                GRID_MAJOR
            } else {
                GRID_MINOR
            },
        );
    }
    for (label, y) in [("ITEMS", 22.0), ("LEVELS", 110.0), ("KILLS", 150.0)] {
        words(kit, plot, label, at(-84.0, y), caption(11.0), theme::INK_4);
    }
    for y in [96.0, 140.0] {
        rect(kit, plot, area(0.0, y, 780.0, 1.0), theme::DIVIDER);
    }
    let Some(recording) = &view.summary.recording else {
        return;
    };
    let purchases: Vec<_> = if let Some(inventory) = view
        .illustrated_items
        .iter()
        .find(|inventory| inventory.slot == row.slot)
    {
        inventory
            .purchases
            .iter()
            .map(|(key, second)| (Some(key.clone()), None, *second))
            .collect()
    } else {
        recording
            .purchases
            .iter()
            .filter(|purchase| purchase.slot == row.slot)
            .map(|purchase| (None, Some(purchase.item), purchase.second))
            .collect()
    };
    for (index, (key, id, second)) in purchases.iter().take(256).enumerate() {
        let x = ((*second).max(0) as f32 / duration * 780.0).clamp(0.0, 780.0);
        let y = if index % 2 == 0 { 6.0 } else { 50.0 };
        let icon = item(kit, plot, *id, area(x - 17.0, y, 34.0, 25.0));
        if let Some(path) = key
            .as_ref()
            .and_then(|key| kit.manifest.file(&format!("icon.item.{key}")))
        {
            kit.commands
                .entity(icon)
                .insert(ImageNode::new(kit.server.load(path)));
        }
        let text = format!(
            "{}{}",
            if *second < 0 { "−" } else { "" },
            graphs::clock(second.unsigned_abs().min(u64::from(u32::MAX)) as u32)
        );
        words(
            kit,
            plot,
            text,
            at(x - 17.0, y + 27.0),
            number(11.0),
            theme::INK_4,
        );
    }
    levels(kit, plot, view, row.slot, 780.0, duration);
    for kill in recording
        .kills
        .iter()
        .filter(|kill| kill.killer == Some(row.slot) || kill.victim == Some(row.slot))
        .take(512)
    {
        let x = kill.second as f32 / duration * 780.0;
        let killed = kill.killer == Some(row.slot);
        if killed {
            dot(kit, plot, Vec2::new(x, 155.0), 9.0, theme::RADIANT_TEXT);
        } else {
            words(
                kit,
                plot,
                "×",
                at(x - 5.0, 165.0),
                bold(20.0),
                theme::DIRE_TEXT,
            );
        }
    }
    for second in graphs::time_ticks(view.summary.seconds) {
        let x = second as f32 / duration * 780.0;
        words(
            kit,
            panel,
            graphs::clock(second),
            at((96.0 + x - 20.0).clamp(96.0, 840.0), 244.0),
            number(12.0),
            if second == view.summary.seconds {
                theme::INK_2
            } else {
                theme::INK_5
            },
        );
    }
}

fn levels(kit: &mut Kit, parent: Entity, view: &View, slot: SlotId, width: f32, duration: f32) {
    let Some(recording) = &view.summary.recording else {
        return;
    };
    for level in recording.levels.iter().filter(|level| level.slot == slot) {
        let x = level.second as f32 / duration * width;
        if level.level % 5 == 0 || level.level == row_level(view, slot) {
            let badge = rect(
                kit,
                parent,
                Node {
                    border_radius: BorderRadius::MAX,
                    border: UiRect::all(px(1)),
                    ..area(x - 11.0, 104.0, 22.0, 22.0)
                },
                theme::SECONDARY.1,
            );
            kit.commands
                .entity(badge)
                .insert(BorderColor::all(theme::AVATAR_EDGE));
            let text = words(
                kit,
                badge,
                level.level.to_string(),
                Node {
                    width: percent(100),
                    margin: UiRect::top(px(4)),
                    ..default()
                },
                number(11.0),
                theme::INK_1,
            );
            kit.commands
                .entity(text)
                .insert(TextLayout::justify(Justify::Center));
        } else {
            dot(kit, parent, Vec2::new(x, 115.0), 4.0, theme::INK_OFF);
        }
    }
}

fn row_level(view: &View, slot: SlotId) -> u8 {
    view.row(slot).map_or(1, |row| row.level)
}
