//! The post-game screen: the winner's banner, a table for each side, and the two graphs.

use bevy::prelude::*;
use bota_proto::Team;

use super::summary::{BAG, Detail, MatchSummary, Point, Row};
use crate::manifest::Manifest;
use crate::ui::{self, Fonts};

/// Over the scoreboard and the notice, which are up while a match is over.
pub const Z: i32 = crate::scoreboard::BOARD_Z + 2;

const RADIANT: Color = Color::srgb(0.45, 0.85, 0.35);
const DIRE: Color = Color::srgb(0.92, 0.32, 0.26);
const NET_WORTH: Color = ui::GOLDEN;
const XP: Color = Color::srgb(0.45, 0.65, 1.0);

const GRAPH_W: f32 = 520.0;
const GRAPH_H: f32 = 120.0;
const ROW_H: f32 = 34.0;
const ITEM: f32 = 30.0;

/// The whole screen.
#[derive(Component)]
pub struct Screen;

/// A seat's row.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatRow(pub bota_proto::SlotId);

/// What a cell of a row says.
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

/// A seat's hero portrait.
#[cfg_attr(not(test), allow(dead_code))] // read by the tests
#[derive(Component)]
pub struct Portrait;

/// One of a seat's item slots.
#[cfg_attr(not(test), allow(dead_code))] // read by the tests
#[derive(Component)]
pub struct ItemSlot;

/// The banner's words.
#[derive(Component)]
pub struct Banner;

/// Which graph a part belongs to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Graph {
    NetWorth,
    Xp,
}

/// A graph's point for one game minute.
#[cfg_attr(not(test), allow(dead_code))] // read by the tests
#[derive(Component)]
pub struct Dot {
    pub graph: Graph,
}

/// A graph's line between two minutes.
#[cfg_attr(not(test), allow(dead_code))] // read by the tests
#[derive(Component)]
pub struct Segment(pub Graph);

pub struct Kit<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub fonts: &'a Fonts,
    pub manifest: &'a Manifest,
    pub server: &'a AssetServer,
}

fn side_colour(team: Team) -> Color {
    if team == Team::Dire { DIRE } else { RADIANT }
}

fn minutes_and_seconds(seconds: u32) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn number(value: Option<impl std::fmt::Display>) -> String {
    value.map_or_else(|| "-".to_owned(), |v| v.to_string())
}

impl Kit<'_, '_, '_> {
    fn text(&mut self, parent: Entity, s: impl Into<String>, size: f32, color: Color) -> Entity {
        self.commands
            .spawn((
                ui::text(s, &self.fonts.bold, size, color),
                ui::shadow(),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id()
    }

    fn node(&mut self, parent: Entity, node: Node, color: Color) -> Entity {
        self.commands
            .spawn((
                node,
                BackgroundColor(color),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id()
    }

    /// A fixed-width cell with centred text.
    fn cell(&mut self, parent: Entity, width: f32, s: String, color: Color) -> Entity {
        let cell = self.node(
            parent,
            Node {
                width: px(width),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Color::NONE,
        );
        self.text(cell, s, 14.0, color)
    }
}

/// The screen for a summary, over the whole window.
pub fn spawn(kit: &mut Kit, root: Entity, summary: &MatchSummary) {
    let screen = kit
        .commands
        .spawn((
            Screen,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(10),
                ..default()
            },
            BackgroundColor(Color::srgba(0.01, 0.015, 0.02, 0.94)),
            GlobalZIndex(Z),
            ChildOf(root),
        ))
        .id();
    banner(kit, screen, summary);
    for team in [Team::Radiant, Team::Dire] {
        table(kit, screen, summary, team);
    }
    if summary.detail == Detail::Meta {
        graphs(kit, screen, &summary.points);
    } else {
        kit.text(
            screen,
            "No replay of this match: the totals only",
            14.0,
            ui::INK_DIM,
        );
    }
    kit.text(screen, "Escape: back to the menu", 15.0, ui::INK_DIM);
}

fn banner(kit: &mut Kit, parent: Entity, summary: &MatchSummary) {
    let (words, colour) = match summary.winner {
        Some(Team::Radiant) => ("RADIANT VICTORY", RADIANT),
        Some(Team::Dire) => ("DIRE VICTORY", DIRE),
        Some(Team::Neutral) => ("DRAW", ui::INK),
        None => ("MATCH ENDED", ui::INK),
    };
    let banner = kit.text(parent, words, 40.0, colour);
    kit.commands.entity(banner).insert(Banner);
    kit.text(
        parent,
        format!("Duration {}", minutes_and_seconds(summary.seconds)),
        16.0,
        ui::INK_DIM,
    );
}

const COLUMNS: [(&str, f32); 14] = [
    ("", 58.0),
    ("LVL", 36.0),
    ("K", 38.0),
    ("D", 38.0),
    ("A", 38.0),
    ("LH", 46.0),
    ("DN", 42.0),
    ("NET", 66.0),
    ("GPM", 50.0),
    ("XPM", 50.0),
    ("HERO DMG", 76.0),
    ("TOWER", 60.0),
    ("HEAL", 56.0),
    ("ITEMS", 0.0),
];

fn table(kit: &mut Kit, parent: Entity, summary: &MatchSummary, team: Team) {
    let table = kit.node(
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(6)),
            ..default()
        },
        ui::PANEL,
    );
    let head = kit.node(
        table,
        Node {
            height: px(22),
            align_items: AlignItems::Center,
            ..default()
        },
        Color::NONE,
    );
    let name = if team == Team::Dire {
        "DIRE"
    } else {
        "RADIANT"
    };
    let title = kit.node(
        head,
        Node {
            width: px(COLUMNS[0].1),
            ..default()
        },
        Color::NONE,
    );
    kit.text(title, name, 13.0, side_colour(team));
    for (label, width) in &COLUMNS[1..COLUMNS.len() - 1] {
        kit.cell(head, *width, (*label).to_owned(), ui::INK_DIM);
    }
    for row in summary.rows.iter().filter(|r| r.team == team) {
        seat(kit, table, row, summary.detail);
    }
}

fn seat(kit: &mut Kit, table: Entity, row: &Row, detail: Detail) {
    let line = kit
        .commands
        .spawn((
            SeatRow(row.slot),
            Node {
                height: px(ROW_H),
                align_items: AlignItems::Center,
                margin: UiRect::top(px(2)),
                border: UiRect::left(px(3)),
                ..default()
            },
            BorderColor::all(side_colour(row.team)),
            BackgroundColor(Color::srgba(0.07, 0.09, 0.11, 0.9)),
            Pickable::IGNORE,
            ChildOf(table),
        ))
        .id();
    kit.commands.spawn((
        Portrait,
        Node {
            width: px(COLUMNS[0].1 - 4.0),
            height: px(ROW_H - 4.0),
            margin: UiRect::right(px(4)),
            ..default()
        },
        match kit.manifest.portrait(row.hero.0) {
            Some(path) => ImageNode::new(kit.server.load(path)),
            None => ImageNode::default().with_color(Color::NONE),
        },
        ui::Cover,
        Pickable::IGNORE,
        ChildOf(line),
    ));
    let width = |i: usize| COLUMNS[i].1;
    let cells = [
        (Cell::Level, 1, row.level.to_string(), ui::INK),
        (Cell::Kills, 2, row.kills.to_string(), ui::INK),
        (Cell::Deaths, 3, row.deaths.to_string(), ui::INK),
        (Cell::Assists, 4, row.assists.to_string(), ui::INK),
        (Cell::LastHits, 5, number(row.last_hits), ui::INK),
        (Cell::Denies, 6, number(row.denies), ui::INK),
        (Cell::NetWorth, 7, number(row.net_worth), ui::GOLDEN),
        (Cell::Gpm, 8, row.gpm.to_string(), ui::INK),
        (Cell::Xpm, 9, row.xpm.to_string(), ui::INK),
        (Cell::HeroDamage, 10, number(row.hero_damage), ui::INK),
        (Cell::TowerDamage, 11, number(row.tower_damage), ui::INK),
        (Cell::Healing, 12, number(row.healing), ui::INK),
    ];
    for (cell, column, text, colour) in cells {
        let at = kit.cell(line, width(column), text, colour);
        kit.commands.entity(at).insert(cell);
    }
    if detail == Detail::Meta {
        for item in row.items.iter().take(BAG) {
            let art = item.and_then(|item| kit.manifest.item_icon(item.0));
            kit.commands.spawn((
                ItemSlot,
                Node {
                    width: px(ITEM),
                    height: px(ITEM * 0.75),
                    margin: UiRect::left(px(2)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.9)),
                match art {
                    Some(path) => ImageNode::new(kit.server.load(path)),
                    None => ImageNode::default().with_color(Color::NONE),
                },
                ui::Cover,
                Pickable::IGNORE,
                ChildOf(line),
            ));
        }
    }
}

fn graphs(kit: &mut Kit, parent: Entity, points: &[Point]) {
    let both = kit.node(
        parent,
        Node {
            column_gap: px(16),
            ..default()
        },
        Color::NONE,
    );
    let series = |pick: fn(&Point) -> i32| points.iter().map(pick).collect::<Vec<_>>();
    graph(
        kit,
        both,
        Graph::NetWorth,
        "NET WORTH ADVANTAGE",
        &series(|p| p.net_worth),
        NET_WORTH,
    );
    graph(
        kit,
        both,
        Graph::Xp,
        "EXPERIENCE ADVANTAGE",
        &series(|p| p.xp),
        XP,
    );
}

/// One graph: a point for every minute, joined by a line, the Radiant's lead above the middle.
fn graph(kit: &mut Kit, parent: Entity, which: Graph, title: &str, values: &[i32], colour: Color) {
    let panel = kit.node(
        parent,
        Node {
            width: px(GRAPH_W + 24.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(8)),
            ..default()
        },
        ui::PANEL,
    );
    let reach = values
        .iter()
        .map(|v| v.unsigned_abs())
        .max()
        .unwrap_or(0)
        .max(1);
    kit.text(
        panel,
        format!("{title}  (+{reach} Radiant ahead, -{reach} Dire ahead)"),
        12.0,
        ui::INK_DIM,
    );
    let area = kit.node(
        panel,
        Node {
            width: px(GRAPH_W),
            height: px(GRAPH_H),
            margin: UiRect::top(px(4)),
            ..default()
        },
        Color::srgba(0.0, 0.0, 0.0, 0.35),
    );
    kit.commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(GRAPH_H / 2.0),
            width: px(GRAPH_W),
            height: px(1),
            ..default()
        },
        BackgroundColor(ui::EDGE),
        Pickable::IGNORE,
        ChildOf(area),
    ));
    let at = |i: usize| {
        let x = if values.len() < 2 {
            0.0
        } else {
            GRAPH_W * i as f32 / (values.len() - 1) as f32
        };
        let y = GRAPH_H / 2.0 - values[i] as f32 / reach as f32 * (GRAPH_H / 2.0 - 3.0);
        Vec2::new(x, y)
    };
    for i in 1..values.len() {
        let (from, to) = (at(i - 1), at(i));
        let middle = (from + to) / 2.0;
        let length = from.distance(to);
        let turn = to - from;
        kit.commands.spawn((
            Segment(which),
            Node {
                position_type: PositionType::Absolute,
                left: px(middle.x - length / 2.0),
                top: px(middle.y - 1.0),
                width: px(length),
                height: px(2),
                ..default()
            },
            UiTransform::from_rotation(Rot2::radians(turn.y.atan2(turn.x))),
            BackgroundColor(colour),
            Pickable::IGNORE,
            ChildOf(area),
        ));
    }
    for minute in 0..values.len() {
        let p = at(minute);
        kit.commands.spawn((
            Dot { graph: which },
            Node {
                position_type: PositionType::Absolute,
                left: px(p.x - 2.5),
                top: px(p.y - 2.5),
                width: px(5),
                height: px(5),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(colour),
            Pickable::IGNORE,
            ChildOf(area),
        ));
    }
    let last = values.len().saturating_sub(1);
    kit.text(panel, format!("0 to {last} min"), 12.0, ui::INK_DIM);
}
