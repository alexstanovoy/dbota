use super::super::{
    graphs::{self, Axis, Curve, Metric},
    summary::MatchSummary,
    view::{Action, View},
};
use super::primitives::*;
#[cfg(test)]
use super::{Dot, Segment};
use crate::{main_menu::kit::Kit, theme};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bota_proto::{SlotId, Team};
use std::sync::Arc;

const PLOT_WIDTH: f32 = 996.0;
const AREA_HEIGHT: f32 = 536.0;

#[derive(Component)]
pub struct PickerRegion;

#[cfg(test)]
#[derive(Component)]
pub struct Grid {
    pub vertical: bool,
    pub major: bool,
}

#[derive(Component)]
pub struct Plot {
    summary: Arc<MatchSummary>,
    curves: Vec<Curve>,
    axis: Axis,
    metric: Metric,
    focus: Option<SlotId>,
    height: f32,
    times: Vec<u32>,
    forced: Option<u32>,
    blocked: bool,
    hover: Option<u32>,
    transient: Vec<Entity>,
}

pub fn draw(kit: &mut Kit, parent: Entity, view: &View) {
    strip(kit, parent, view);
    picker(kit, parent, view);
    let curves = graphs::curves(&view.summary, view.metric);
    let axis = Axis::of(&curves, view.metric == Metric::Advantage);
    let height = AREA_HEIGHT
        - 8.0
        - if view.metric == Metric::Advantage {
            90.0
        } else {
            28.0
        };
    let area_node = kit.node(parent, area(18.0, 156.0, 1202.0, AREA_HEIGHT));
    axes(kit, area_node, view, &axis, height);
    let plot = kit.node(area_node, area(56.0, 8.0, PLOT_WIDTH, height));
    kit.commands.entity(plot).insert(Pickable::default());
    let mut times: Vec<_> = curves
        .iter()
        .flat_map(|curve| curve.values.iter().map(|point| point.second))
        .collect();
    times.sort_unstable();
    times.dedup();
    draw_curves(kit, plot, view, &curves, &axis, height);
    end_labels(kit, area_node, view, &curves, &axis, height);
    if view.metric == Metric::Advantage {
        events(kit, plot, view, height);
    }
    if curves.is_empty() {
        words(
            kit,
            plot,
            "No recorded values",
            at(24.0, 24.0),
            body(16.0),
            theme::INK_4,
        );
    }
    kit.commands.entity(plot).insert((
        RelativeCursorPosition::default(),
        bevy::picking::hover::Hovered::default(),
        Plot {
            summary: view.summary.clone(),
            curves,
            axis,
            metric: view.metric,
            focus: view.focus,
            height,
            times,
            forced: view.hover_second,
            blocked: view.picker,
            hover: None,
            transient: Vec::new(),
        },
        Name::new("results graph plot"),
    ));
}

fn strip(kit: &mut Kit, parent: Entity, view: &View) {
    let strip = kit.node(
        parent,
        Node {
            justify_content: JustifyContent::Center,
            column_gap: px(8),
            ..area(18.0, 14.0, 1202.0, 80.0)
        },
    );
    for team in [Team::Radiant, Team::Dire] {
        if team == Team::Dire {
            team_totals(kit, strip, view);
        }
        for row in view.summary.rows.iter().filter(|row| row.team == team) {
            let holder = button(
                kit,
                strip,
                &format!("results focus {}", row.slot.0),
                Node {
                    width: px(66),
                    height: px(80),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(3),
                    ..default()
                },
                Action::Focus(row.slot),
                Color::NONE,
            );
            let face = round_portrait(
                kit,
                holder,
                row,
                Node {
                    width: px(54),
                    height: px(54),
                    border_radius: BorderRadius::MAX,
                    border: UiRect::all(px(if view.focus == Some(row.slot) { 3 } else { 2 })),
                    overflow: Overflow::clip(),
                    flex_shrink: 0.0,
                    ..default()
                },
            );
            kit.commands
                .entity(face)
                .insert(BorderColor::all(if view.focus == Some(row.slot) {
                    theme::SELECT
                } else {
                    team_colour(team).with_alpha(if view.focus.is_some() {
                        0.55 * 0.55
                    } else {
                        0.55
                    })
                }));
            if view.focus.is_some() && view.focus != Some(row.slot) {
                kit.commands
                    .entity(face)
                    .entry::<ImageNode>()
                    .and_modify(|mut image| {
                        image.color = Color::WHITE.with_alpha(0.55);
                    });
            }
            let badge = rect(
                kit,
                holder,
                Node {
                    padding: UiRect::axes(px(5), px(1)),
                    ..default()
                },
                if team == Team::Radiant {
                    theme::GO.1
                } else {
                    theme::STOP.1
                },
            );
            words(
                kit,
                badge,
                row.net_worth
                    .map_or_else(|| "—".to_owned(), |value| graphs::compact(i64::from(value))),
                Node::default(),
                number(12.0),
                theme::INK_1,
            );
        }
    }
}

fn team_totals(kit: &mut Kit, parent: Entity, view: &View) {
    let middle = kit.node(
        parent,
        Node {
            width: px(136),
            margin: UiRect::horizontal(px(10)),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    words(
        kit,
        middle,
        graphs::clock(view.summary.seconds),
        Node::default(),
        number(13.0),
        theme::INK_3,
    );
    let totals = kit.node(
        middle,
        Node {
            margin: UiRect::top(px(3)),
            ..default()
        },
    );
    for team in [Team::Radiant, Team::Dire] {
        let total = view
            .summary
            .team_total(team, bota_replay::meta::stat::NET_WORTH)
            .map_or_else(|| "—".to_owned(), graphs::compact);
        let badge = rect(
            kit,
            totals,
            Node {
                width: px(68),
                height: px(34),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            if team == Team::Radiant {
                theme::GO.1
            } else {
                theme::STOP.1
            },
        );
        words(
            kit,
            badge,
            total,
            Node::default(),
            number(18.0),
            theme::INK_1,
        );
    }
}

fn picker(kit: &mut Kit, parent: Entity, view: &View) {
    let picker = button(
        kit,
        parent,
        "results graph picker",
        Node {
            border: UiRect::all(px(1)),
            ..area(18.0, 98.0, 300.0, 46.0)
        },
        Action::Picker,
        Color::srgb_u8(0x1a, 0x20, 0x26),
    );
    kit.commands.entity(picker).insert((
        BorderColor::all(EDGE),
        ZIndex(4),
        PickerRegion,
        RelativeCursorPosition::default(),
    ));
    words(
        kit,
        picker,
        "GRAPH",
        at(14.0, 6.0),
        caption(11.0),
        theme::INK_4,
    );
    words(
        kit,
        picker,
        view.metric.label(),
        at(14.0, 21.0),
        bold(16.0),
        theme::INK_1,
    );
    line(
        kit,
        picker,
        Vec2::new(276.0, 20.0),
        Vec2::new(281.0, 25.0),
        2.0,
        theme::INK_3,
    );
    line(
        kit,
        picker,
        Vec2::new(281.0, 25.0),
        Vec2::new(286.0, 20.0),
        2.0,
        theme::INK_3,
    );
    if !view.picker {
        return;
    }
    let menu = rect(
        kit,
        parent,
        Node {
            border: UiRect::all(px(1)),
            ..area(58.0, 146.0, 260.0, 538.0)
        },
        Color::srgb_u8(0x16, 0x1b, 0x20),
    );
    kit.commands.entity(menu).insert((
        BorderColor::all(EDGE),
        ZIndex(8),
        Name::new("results graph choices"),
        PickerRegion,
        RelativeCursorPosition::default(),
    ));
    let mut top = 0.0;
    for (index, metric) in Metric::ALL.into_iter().enumerate() {
        if index == 0 || index == 3 {
            let title = rect(kit, menu, area(0.0, top, 258.0, 30.0), PANEL_DARK);
            words(
                kit,
                title,
                if index == 0 { "TEAM" } else { "PER PLAYER" },
                at(14.0, 9.0),
                caption(12.0),
                theme::INK_OFF,
            );
            top += 30.0;
        }
        let selected = metric == view.metric;
        let button = button(
            kit,
            menu,
            &format!("results graph {}", metric.label().to_lowercase()),
            area(0.0, top, 258.0, 34.0),
            Action::Metric(metric),
            if selected {
                Color::srgb_u8(0x2a, 0x3a, 0x47)
            } else {
                Color::NONE
            },
        );
        words(
            kit,
            button,
            metric.label(),
            at(14.0, 10.0),
            body(14.0),
            theme::INK_1,
        );
        if selected {
            rect(kit, button, area(0.0, 0.0, 3.0, 34.0), theme::SELECT);
        }
        top += 34.0;
    }
}

fn axes(kit: &mut Kit, parent: Entity, view: &View, axis: &Axis, height: f32) {
    let advantage = view.metric == Metric::Advantage;
    let ticks = graphs::time_ticks(view.summary.seconds);
    for value in &axis.ticks {
        let y = 8.0 + axis.fraction(*value) * height;
        let strong = *value == 0;
        let _line = rect(
            kit,
            parent,
            area(56.0, y, PLOT_WIDTH, 1.0),
            if strong { GRID_ZERO } else { GRID_HORIZONTAL },
        );
        #[cfg(test)]
        kit.commands.entity(_line).insert(Grid {
            vertical: false,
            major: strong,
        });
        let mut text = graphs::compact(*value).replace(".0k", "k");
        if advantage && *value > 0 {
            text = format!("+{text}");
        }
        let label = words(
            kit,
            parent,
            text,
            area(0.0, y - 8.0, 46.0, 16.0),
            number(12.0),
            theme::INK_5,
        );
        kit.commands
            .entity(label)
            .insert(TextLayout::justify(Justify::Right));
    }
    for x in [56.0, 56.0 + PLOT_WIDTH] {
        let _edge = rect(kit, parent, area(x, 8.0, 1.0, height), GRID_MAJOR);
        #[cfg(test)]
        kit.commands.entity(_edge).insert(Grid {
            vertical: true,
            major: true,
        });
    }
    for minute in 1..=view.summary.seconds / 60 {
        let second = minute * 60;
        if second >= view.summary.seconds {
            break;
        }
        let major = ticks.contains(&second);
        let _line = rect(
            kit,
            parent,
            area(
                56.0 + second as f32 / view.summary.seconds.max(1) as f32 * PLOT_WIDTH,
                8.0,
                1.0,
                height,
            ),
            if major { GRID_MAJOR } else { GRID_MINOR },
        );
        #[cfg(test)]
        kit.commands.entity(_line).insert(Grid {
            vertical: true,
            major,
        });
    }
    let label_y = if advantage {
        AREA_HEIGHT - 18.0
    } else {
        8.0 + height + 12.0
    };
    for (index, second) in ticks.iter().enumerate() {
        let x = 56.0 + *second as f32 / view.summary.seconds.max(1) as f32 * PLOT_WIDTH;
        let width = 60.0;
        let left = if index == 0 {
            x
        } else if *second == view.summary.seconds {
            x - width
        } else {
            x - width / 2.0
        };
        let entity = words(
            kit,
            parent,
            graphs::clock(*second),
            area(left, label_y, width, 18.0),
            number(12.0),
            if *second == view.summary.seconds {
                theme::INK_2
            } else {
                theme::INK_5
            },
        );
        kit.commands
            .entity(entity)
            .insert(TextLayout::justify(if index == 0 {
                Justify::Left
            } else if *second == view.summary.seconds {
                Justify::Right
            } else {
                Justify::Center
            }));
    }
    if advantage {
        words(
            kit,
            parent,
            "RADIANT AHEAD",
            at(66.0, 16.0),
            caption(12.0),
            theme::RADIANT_TEXT,
        );
        words(
            kit,
            parent,
            "DIRE AHEAD",
            at(66.0, height - 10.0),
            caption(12.0),
            theme::DIRE_TEXT,
        );
    }
}

fn colour(curve: &Curve, view: &View) -> Color {
    Color::srgb_u8(curve.colour[0], curve.colour[1], curve.colour[2]).with_alpha(
        if view.metric.team() || view.focus.is_none() || view.focus == curve.slot {
            1.0
        } else {
            0.35
        },
    )
}

fn draw_curves(
    kit: &mut Kit,
    parent: Entity,
    view: &View,
    curves: &[Curve],
    axis: &Axis,
    height: f32,
) {
    for curve in curves {
        let points: Vec<_> = curve
            .values
            .iter()
            .map(|point| {
                Vec2::new(
                    point.second as f32 / view.summary.seconds.max(1) as f32 * PLOT_WIDTH,
                    axis.fraction(point.value) * height,
                )
            })
            .collect();
        let stroke = if matches!(view.metric, Metric::TeamNetWorth | Metric::TeamKills)
            || (view.metric == Metric::Advantage && curve.name == "Net worth")
        {
            3.0
        } else if !view.metric.team() && view.focus == curve.slot && view.focus.is_some() {
            3.5
        } else if !view.metric.team() && view.focus.is_some() {
            1.4
        } else {
            2.0
        };
        for (index, pair) in points.windows(2).enumerate() {
            if curve.values[index + 1].connected {
                let _segment = line(kit, parent, pair[0], pair[1], stroke, colour(curve, view));
                #[cfg(test)]
                kit.commands.entity(_segment).insert(Segment(view.metric));
            }
        }
        if curve.values.len() <= 12 {
            for point in points {
                let _dot = dot(kit, parent, point, 7.0, colour(curve, view));
                #[cfg(test)]
                kit.commands.entity(_dot).insert(Dot { graph: view.metric });
            }
        }
    }
}

fn end_labels(
    kit: &mut Kit,
    parent: Entity,
    view: &View,
    curves: &[Curve],
    axis: &Axis,
    height: f32,
) {
    let fractions: Vec<_> = curves
        .iter()
        .map(|curve| axis.fraction(curve.values.last().map_or(0, |point| point.value)))
        .collect();
    let positions = graphs::label_positions(&fractions, height);
    let list = kit.node(
        parent,
        Node {
            overflow: Overflow::scroll_y(),
            ..area(56.0 + PLOT_WIDTH + 10.0, 0.0, 138.0, height + 16.0)
        },
    );
    kit.commands.entity(list).insert((
        crate::ui::ScrollArea,
        ScrollPosition::default(),
        RelativeCursorPosition::default(),
        bevy::picking::hover::Hovered::default(),
    ));
    let content = kit.node(
        list,
        Node {
            position_type: PositionType::Relative,
            width: percent(100),
            height: px((height + 16.0).max(curves.len() as f32 * 17.0 + 16.0)),
            flex_shrink: 0.0,
            ..default()
        },
    );
    for (curve, y) in curves.iter().zip(positions) {
        let colour = colour(curve, view);
        dot(kit, content, Vec2::new(4.0, y + 8.0), 8.0, colour);
        let end = curve
            .values
            .last()
            .filter(|point| point.second == view.summary.seconds);
        let text = end.map_or_else(
            || "—".to_owned(),
            |point| {
                format!(
                    "{}{}",
                    if view.metric == Metric::Advantage && point.value > 0 {
                        "+"
                    } else {
                        ""
                    },
                    graphs::compact(point.value)
                )
            },
        );
        let label = crate::player_name::spawn(
            &mut kit.commands,
            kit.fonts,
            content,
            &curve.name,
            curve.version.as_deref(),
            if curve.slot.is_some()
                && (curve.slot == view.context.owner || curve.slot == view.focus)
            {
                bold(12.0)
            } else {
                body(12.0)
            },
            theme::INK_2.with_alpha(colour.alpha()),
        );
        kit.commands
            .entity(label.root)
            .insert(area(13.0, y, 85.0, 16.0));
        let value = words(
            kit,
            content,
            text,
            area(96.0, y, 42.0, 16.0),
            number(12.0),
            colour,
        );
        kit.commands
            .entity(value)
            .insert(TextLayout::justify(Justify::Right));
    }
}

fn events(kit: &mut Kit, parent: Entity, view: &View, height: f32) {
    use std::collections::BTreeMap;
    let Some(recording) = &view.summary.recording else {
        return;
    };
    rect(
        kit,
        parent,
        area(0.0, height + 35.0, PLOT_WIDTH, 1.0),
        theme::DIVIDER,
    );
    let mut marks = BTreeMap::new();
    for kill in &recording.kills {
        let team = kill
            .killer
            .and_then(|slot| view.row(slot))
            .map(|row| row.team);
        if let Some(team) = team {
            let x = kill.second.min(view.summary.seconds) as f32
                / view.summary.seconds.max(1) as f32
                * PLOT_WIDTH;
            marks
                .entry((team == Team::Dire, false, (x / 4.0) as u32))
                .or_insert(x);
        }
    }
    for event in &view.summary.structures {
        let second = recording.second(event.tick);
        if second > view.summary.seconds {
            continue;
        }
        let x = second as f32 / view.summary.seconds.max(1) as f32 * PLOT_WIDTH;
        marks
            .entry((event.team == Team::Radiant, true, (x / 4.0) as u32))
            .or_insert(x);
    }
    for ((dire, building, _), x) in marks {
        let colour = if dire {
            theme::DIRE_TEXT
        } else {
            theme::RADIANT_TEXT
        };
        let y = height
            + if building {
                if dire { 54.0 } else { 12.0 }
            } else if dire {
                43.0
            } else {
                26.0
            };
        if building {
            let square = rect(
                kit,
                parent,
                Node {
                    border: UiRect::all(px(2)),
                    ..area(x - 3.0, y - 3.0, 6.0, 6.0)
                },
                Color::NONE,
            );
            kit.commands.entity(square).insert(BorderColor::all(colour));
        } else {
            dot(kit, parent, Vec2::new(x, y), 8.0, colour);
        }
    }
    words(
        kit,
        parent,
        "KILLS, BUILDINGS",
        at(PLOT_WIDTH + 10.0, height + 30.0),
        caption(11.0),
        theme::INK_OFF,
    );
}

pub fn hover(
    commands: Commands,
    fonts: Res<crate::ui::Fonts>,
    manifest: Res<crate::manifest::Manifest>,
    texts: Res<crate::dota_text::Texts>,
    server: Res<AssetServer>,
    mut plots: Query<(Entity, &RelativeCursorPosition, &mut Plot)>,
) {
    let mut kit = Kit {
        commands,
        fonts: &fonts,
        manifest: &manifest,
        texts: &texts,
        server: &server,
    };
    for (entity, cursor, mut plot) in &mut plots {
        let second = if plot.blocked {
            None
        } else {
            plot.forced.or_else(|| {
                cursor
                    .normalized
                    .filter(|_| cursor.cursor_over)
                    .and_then(|position| {
                        nearest(
                            &plot.times,
                            ((position.x + 0.5).clamp(0.0, 1.0) * plot.summary.seconds as f32)
                                as u32,
                        )
                    })
            })
        };
        if second == plot.hover {
            continue;
        }
        for transient in plot.transient.drain(..) {
            kit.commands.entity(transient).despawn();
        }
        plot.hover = second;
        if let Some(second) = second {
            plot.transient = hover_at(&mut kit, entity, &plot, second);
        }
    }
}

fn nearest(times: &[u32], second: u32) -> Option<u32> {
    let index = times.partition_point(|time| *time < second);
    match (
        index.checked_sub(1).and_then(|index| times.get(index)),
        times.get(index),
    ) {
        (Some(before), Some(after)) => Some(if second - before <= after - second {
            *before
        } else {
            *after
        }),
        (Some(before), None) => Some(*before),
        (None, Some(after)) => Some(*after),
        _ => None,
    }
}

fn hover_at(kit: &mut Kit, parent: Entity, plot: &Plot, second: u32) -> Vec<Entity> {
    let x = second as f32 / plot.summary.seconds.max(1) as f32 * PLOT_WIDTH;
    let mut entities = vec![rect(
        kit,
        parent,
        area(
            x,
            0.0,
            1.0,
            plot.height
                + if plot.metric == Metric::Advantage {
                    56.0
                } else {
                    0.0
                },
        ),
        theme::INK_3.with_alpha(0.55),
    )];
    let mut rows: Vec<_> = plot
        .curves
        .iter()
        .map(|curve| {
            (
                curve,
                curve.values.iter().find(|point| point.second == second),
            )
        })
        .collect();
    if !plot.metric.team() {
        rows.sort_by_key(|(_, point)| std::cmp::Reverse(point.map(|point| point.value)));
    }
    for (curve, point) in &rows {
        if let Some(point) = point {
            entities.push(dot(
                kit,
                parent,
                Vec2::new(x, plot.axis.fraction(point.value) * plot.height),
                if curve.slot == plot.focus { 11.0 } else { 8.0 },
                Color::srgb_u8(curve.colour[0], curve.colour[1], curve.colour[2]),
            ));
        }
    }
    let width = if plot.metric == Metric::Advantage {
        240.0
    } else {
        230.0
    };
    let tooltip = rect(
        kit,
        parent,
        Node {
            border: UiRect::all(px(1)),
            ..area(
                if x / PLOT_WIDTH > 0.6 {
                    x - 254.0
                } else {
                    x + 14.0
                },
                12.0,
                width,
                42.0 + rows.len() as f32 * 20.0,
            )
        },
        theme::TOOLTIP,
    );
    kit.commands.entity(tooltip).insert((
        BorderColor::all(theme::TOOLTIP_EDGE),
        ZIndex(6),
        Name::new("results graph tooltip"),
    ));
    entities.push(tooltip);
    words(
        kit,
        tooltip,
        format!(
            "{}{}",
            graphs::clock(second),
            if plot.metric == Metric::Advantage {
                String::new()
            } else {
                format!(" · {}", plot.metric.label().to_uppercase())
            }
        ),
        at(12.0, 10.0),
        bold(14.0),
        theme::INK_1,
    );
    for (index, (curve, point)) in rows.iter().enumerate() {
        let colour = Color::srgb_u8(curve.colour[0], curve.colour[1], curve.colour[2]);
        let y = 32.0 + index as f32 * 20.0;
        dot(kit, tooltip, Vec2::new(16.0, y + 5.0), 7.0, colour);
        let label = crate::player_name::spawn(
            &mut kit.commands,
            kit.fonts,
            tooltip,
            &curve.name,
            curve.version.as_deref(),
            body(13.0),
            theme::INK_3,
        );
        kit.commands
            .entity(label.root)
            .insert(area(28.0, y - 2.0, width - 128.0, 18.0));
        let text = point.map_or_else(
            || "—".to_owned(),
            |point| {
                if plot.metric == Metric::Advantage {
                    format!(
                        "{} +{}",
                        if point.value >= 0 { "Radiant" } else { "Dire" },
                        graphs::compact(point.value.saturating_abs())
                    )
                } else {
                    graphs::compact(point.value)
                }
            },
        );
        let value = words(
            kit,
            tooltip,
            text,
            area(width - 110.0, y - 2.0, 98.0, 18.0),
            number(13.0),
            colour,
        );
        kit.commands
            .entity(value)
            .insert(TextLayout::justify(Justify::Right));
    }
    entities
}
