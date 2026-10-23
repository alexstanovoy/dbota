//! One seat's row (Dota's `DOTAScoreboardPlayer`): the player's colour and
//! avatar, the hero with the player's and the hero's names, the talents,
//! the level, the gold and the net worth, kills, deaths and assists, last
//! hits and denies, and the ultimate.

use bevy::prelude::*;

use super::draw::{At, Field, Kit, ROW, SCORE, d, font};
use crate::ui;

/// A column of the rows after the names, as Dota's `*Column` classes size
/// it: width, and the margin left and right, in Dota's pixels.
#[derive(Clone, Copy)]
pub struct Column {
    width: f32,
    left: f32,
    right: f32,
}

impl Column {
    const fn new(width: f32, left: f32, right: f32) -> Column {
        Column { width, left, right }
    }

    pub fn node(self) -> Node {
        Node {
            width: d(self.width),
            margin: UiRect::new(d(self.left), d(self.right), px(0), px(0)),
            ..default()
        }
    }
}

/// The talents, the level, the gold, the net worth, K, D, A, LH, DN and the
/// ultimate: Dota's `TalentColumn`, `LevelColumn`, `GoldColumn`, the net
/// worth wide enough for its name, `KillsColumn` and the other `KDAColumn`s,
/// last hits and denies a little wider for three digits, `UltColumn`.
pub const COLUMNS: [Column; 10] = [
    Column::new(65.0, -10.0, -10.0),
    Column::new(48.0, 2.0, 0.0),
    Column::new(90.0, 13.0, 0.0),
    Column::new(104.0, 13.0, 0.0),
    Column::new(35.0, 13.0, 0.0),
    Column::new(35.0, 0.0, 0.0),
    Column::new(35.0, 0.0, 0.0),
    Column::new(44.0, 0.0, 0.0),
    Column::new(44.0, 0.0, 0.0),
    Column::new(40.0, 0.0, 0.0),
];

pub fn spawn(kit: &mut Kit, parent: Entity, at: At) {
    let row = kit
        .commands
        .spawn((
            at,
            Field::Row,
            Node {
                height: d(68.0),
                margin: UiRect::vertical(d(1.0)),
                border: UiRect::vertical(d(1.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(super::draw::color(ROW)),
            BackgroundGradient::default(),
            BorderColor {
                top: Color::srgba(1.0, 1.0, 1.0, 0.02),
                bottom: Color::srgba(1.0, 1.0, 1.0, 0.012),
                ..BorderColor::DEFAULT
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    player(kit, row, at);
    names(kit, row, at);
    let cells: Vec<Entity> = COLUMNS
        .iter()
        .map(|column| {
            kit.commands
                .spawn((
                    Node {
                        height: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..column.node()
                    },
                    Pickable::IGNORE,
                    ChildOf(row),
                ))
                .id()
        })
        .collect();
    talents(kit, cells[0], at);
    level(kit, cells[1], at);
    let numbers = [
        (Field::Gold, super::draw::GOLD),
        (Field::NetWorth, 0xffffffff),
        (Field::Kills, SCORE),
        (Field::Deaths, SCORE),
        (Field::Assists, SCORE),
        (Field::LastHits, SCORE),
        (Field::Denies, SCORE),
    ];
    for ((field, rgba), cell) in numbers.into_iter().zip(&cells[2..9]) {
        number(kit, *cell, at, field, rgba);
    }
    // Dota shades the kills' column across.
    kit.commands
        .entity(cells[4])
        .insert(BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::new(Color::NONE, percent(0)),
            ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.2), percent(10)),
            ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.2), percent(90)),
            ColorStop::new(Color::NONE, percent(100)),
        ])));
    ult(kit, cells[9], at);
}

/// The player's colour down the edge and the avatar box (Dota's
/// `ScoreColumn`); dbota has no avatars, so the box stays Dota's empty one.
fn player(kit: &mut Kit, row: Entity, at: At) {
    let column = kit
        .commands
        .spawn((
            Node {
                width: d(48.0),
                height: percent(100),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    let vignette = kit.manifest.file("hud.scoreboard_slot");
    kit.commands.spawn((
        at,
        Field::Color,
        Node {
            width: d(4.0),
            height: percent(100),
            ..default()
        },
        vignette.map_or_else(ImageNode::default, |p| ImageNode::new(kit.server.load(p))),
        Pickable::IGNORE,
        ChildOf(column),
    ));
    kit.commands.spawn((
        Node {
            width: d(38.0),
            height: d(38.0),
            margin: UiRect::left(d(6.0)),
            ..default()
        },
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            ColorStop::new(Color::srgba(0.53, 0.53, 0.53, 0.13), percent(0)),
            ColorStop::new(Color::srgba(0.27, 0.27, 0.27, 0.2), percent(100)),
        ])),
        Pickable::IGNORE,
        ChildOf(column),
    ));
}

/// The hero's picture, greyed with the seconds to its return while it is
/// down, and the player's and the hero's names (Dota's `NameColumn`).
fn names(kit: &mut Kit, row: Entity, at: At) {
    let column = kit.commands.spawn((
        Node {
            width: d(288.0),
            margin: UiRect::left(d(8.0)),
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(row),
    ));
    let column = column.id();
    let hero = kit
        .commands
        .spawn((
            at,
            Field::Hero,
            Node {
                width: d(88.0),
                height: d(54.0),
                border: UiRect::all(d(1.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(Color::BLACK),
            ImageNode::default(),
            ui::Cover,
            Pickable::IGNORE,
            ChildOf(column),
        ))
        .id();
    kit.commands.spawn((
        at,
        Field::Respawn,
        ui::text("", &kit.fonts.bold, font(22.0), ui::INK),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(hero),
    ));
    let both = kit.commands.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            margin: UiRect::left(d(8.0)),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(column),
    ));
    let both = both.id();
    for (field, size) in [(Field::Name, 19.0), (Field::HeroName, 14.0)] {
        kit.commands.spawn((
            at,
            field,
            ui::text(
                "",
                &kit.fonts.bold,
                font(size),
                super::draw::name_color(field == Field::HeroName),
            ),
            ui::shadow(),
            Pickable::IGNORE,
            ChildOf(both),
        ));
    }
}

/// The talent tree in small: four tiers of two pips, the level-25 tier on
/// top, lit as they are learned (Dota's `DOTAHudTalentDisplay` at 75%).
fn talents(kit: &mut Kit, cell: Entity, at: At) {
    let tree = kit.commands.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: d(4.0),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(cell),
    ));
    let tree = tree.id();
    for tier in (0..4u8).rev() {
        let line = kit.commands.spawn((
            Node {
                column_gap: d(6.0),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(tree),
        ));
        let line = line.id();
        for side in 0..2u8 {
            kit.commands.spawn((
                at,
                Field::Pip(tier * 2 + side),
                Node {
                    width: d(11.0),
                    height: d(6.0),
                    border_radius: BorderRadius::all(d(2.0)),
                    ..default()
                },
                BackgroundColor(super::PIP_SHUT),
                Pickable::IGNORE,
                ChildOf(line),
            ));
        }
    }
}

/// An ally's level in its experience ring; a foe's plainly, in Dota's pale
/// gold.
fn level(kit: &mut Kit, cell: Entity, at: At) {
    let ring = kit
        .commands
        .spawn((
            at,
            Field::Ring,
            Node {
                width: d(36.0),
                height: d(36.0),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundGradient::default(),
            Pickable::IGNORE,
            ChildOf(cell),
        ))
        .id();
    let well = kit.commands.spawn((
        Node {
            width: d(28.0),
            height: d(28.0),
            border_radius: BorderRadius::MAX,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.06, 0.07, 0.08)),
        Pickable::IGNORE,
        ChildOf(ring),
    ));
    let well = well.id();
    kit.commands.spawn((
        at,
        Field::RingLevel,
        ui::text("", &kit.fonts.bold, font(16.0), ui::INK),
        Pickable::IGNORE,
        ChildOf(well),
    ));
    kit.commands.spawn((
        at,
        Field::Level,
        ui::text(
            "",
            &kit.fonts.regular,
            font(18.0),
            super::draw::color(super::draw::LEVEL),
        ),
        Pickable::IGNORE,
        ChildOf(cell),
    ));
}

fn number(kit: &mut Kit, cell: Entity, at: At, field: Field, rgba: u32) {
    let font_of = if matches!(field, Field::Gold | Field::Kills) {
        &kit.fonts.bold
    } else {
        &kit.fonts.regular
    };
    kit.commands.spawn((
        at,
        field,
        ui::text("", font_of, font(18.0), super::draw::color(rgba)),
        Pickable::IGNORE,
        ChildOf(cell),
    ));
}

/// The ultimate's diamond and its cooldown under it (Dota's `UltColumn`).
fn ult(kit: &mut Kit, cell: Entity, at: At) {
    kit.commands.spawn((
        at,
        Field::Ult,
        Node {
            width: d(20.0),
            height: d(20.0),
            ..default()
        },
        ImageNode::default(),
        Pickable::IGNORE,
        ChildOf(cell),
    ));
    kit.commands.spawn((
        at,
        Field::UltSeconds,
        ui::text(
            "",
            &kit.fonts.bold,
            font(14.0),
            super::draw::color(super::draw::SECONDS),
        ),
        Node {
            position_type: PositionType::Absolute,
            top: d(44.0),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(cell),
    ));
}
