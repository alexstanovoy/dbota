//! The bar of seats along the top of the pick and the strategy screens: Radiant's seats, the clock,
//! Dire's seats, each seat's hero under the colour of its player and its player's name under it.
//! A seat's picture is whatever the lobby says its seat has picked, so the seats fill as the
//! players lock in.

use bevy::prelude::*;
use bota_client_core::App as Core;
use bota_proto::{SlotId, Team};

use super::reference as r;
use super::{Kit, Shot, Spot};
use crate::manifest::Manifest;
use crate::player_colour::PlayerColours;
use crate::session::Session;
use crate::theme;
use crate::ui::Cover;

/// Which screen a bar is on: the pick screen's seats are empty frames that fill, with the
/// colour under the picture; the strategy screen's carry the colour over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Pick,
    Strategy,
}

impl Style {
    /// The reference the bar is measured in.
    fn shot(self) -> Shot {
        match self {
            Style::Pick => super::PICK,
            Style::Strategy => super::STRATEGY,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Style::Pick => "pick",
            Style::Strategy => "strategy",
        }
    }
}

/// The picture of a seat's hero: the hero the lobby says it holds, or on the pick screen, for
/// one's own seat, the hero chosen and not locked in yet, shown dim (as the original game does:
/// nobody else sees it).
#[derive(Component)]
struct SeatPicture {
    slot: SlotId,
    style: Style,
    shown: Option<(u16, bool)>,
}

/// The tint of a hero that is only chosen.
const PREVIEW: Color = Color::srgba(0.42, 0.47, 0.54, 0.55);

/// The name of a seat's player.
#[derive(Component)]
struct SeatName(SlotId);

/// What a bar is built for: who sits where. A bar is built again when it changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roster(Vec<(u8, Team)>, Option<u8>);

impl Roster {
    pub fn of(core: &Core) -> Roster {
        Roster(
            core.lobby
                .slots
                .iter()
                .map(|s| (s.slot.0, s.team))
                .collect(),
            core.my_slot.map(|s| s.0),
        )
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, refresh);
}

/// Spawns the bar's two rows of seats under `root`; the clock stands between them, which the
/// screen puts there.
pub fn spawn(
    commands: &mut Commands,
    kit: &Kit,
    style: Style,
    core: &Core,
    colours: &PlayerColours,
    root: Entity,
) {
    for (team, from_middle, justify) in [
        (
            Team::Radiant,
            r::RADIANT_SEAT_FROM_MIDDLE,
            JustifyContent::FlexEnd,
        ),
        (
            Team::Dire,
            r::DIRE_SEAT_FROM_MIDDLE,
            JustifyContent::FlexStart,
        ),
    ] {
        let shot = style.shot();
        let row = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(50),
                    margin: UiRect::left(shot.val(from_middle)),
                    top: px(0),
                    width: shot.val(r::SEATS_PER_SIDE as f32 * r::SEAT_PITCH),
                    justify_content: justify,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(root),
            ))
            .id();
        let seats = core.lobby.slots.iter().filter(|s| s.team == team);
        for (rank, seat) in seats.take(r::SEATS_PER_SIDE).enumerate() {
            let local = core.my_slot == Some(seat.slot);
            spawn_seat(
                commands,
                kit,
                style,
                (team, rank, seat.slot),
                (colours.at(team, rank), local),
                row,
            );
        }
    }
}

fn spawn_seat(
    commands: &mut Commands,
    kit: &Kit,
    style: Style,
    (team, rank, slot): (Team, usize, SlotId),
    (colour, local): (Color, bool),
    row: Entity,
) {
    let shot = style.shot();
    let column = commands
        .spawn((
            Node {
                width: shot.val(r::SEAT_SIZE[0]),
                margin: UiRect::right(shot.val(r::SEAT_PITCH - r::SEAT_SIZE[0])),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    let height = match style {
        Style::Pick => r::SEAT_SIZE[1],
        Style::Strategy => r::STRATEGY_SEAT_HEIGHT,
    };
    let team_word = if team == Team::Radiant {
        "radiant"
    } else {
        "dire"
    };
    let frame = commands
        .spawn((
            Name::new(format!("{} seat {team_word} {rank}", style.word())),
            Spot(match (style, team, rank) {
                (Style::Pick, Team::Radiant, 0) => "pick seat radiant 0",
                (Style::Pick, _, 0) => "pick seat dire 0",
                (Style::Strategy, Team::Radiant, 0) => "strategy seat radiant 0",
                (Style::Strategy, _, 0) => "strategy seat dire 0",
                _ => "",
            }),
            Node {
                width: percent(100),
                height: shot.val(height),
                ..default()
            },
            ground(style),
            Pickable::IGNORE,
            ChildOf(column),
        ))
        .id();
    let (bar_top, bar_height) = match style {
        Style::Pick => (r::SEAT_BAR[0], r::SEAT_BAR[1]),
        Style::Strategy => (0.0, r::STRATEGY_SEAT_BAR_HEIGHT),
    };
    commands.spawn((
        SeatPicture {
            slot,
            style,
            shown: None,
        },
        Cover,
        ImageNode::default().with_color(Color::NONE),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: match style {
                Style::Pick => px(0),
                Style::Strategy => shot.val(r::STRATEGY_SEAT_BAR_HEIGHT),
            },
            bottom: match style {
                Style::Pick => shot.val(height - bar_top),
                Style::Strategy => px(0),
            },
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(frame),
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: shot.val(bar_top),
            height: shot.val(bar_height),
            ..default()
        },
        BackgroundColor(colour),
        Pickable::IGNORE,
        ChildOf(frame),
    ));
    if local && style == Style::Pick {
        // The glow the original game puts under one's own name.
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: shot.val(-30.0),
                right: shot.val(-30.0),
                top: shot.val(r::SEAT_NAME[0] - 16.0),
                height: shot.val(110.0),
                ..default()
            },
            BackgroundGradient::from(RadialGradient::new(
                UiPosition::CENTER,
                RadialGradientShape::Ellipse(percent(50), percent(50)),
                vec![
                    ColorStop::new(Color::srgba(0.45, 0.62, 0.9, 0.38), percent(0)),
                    ColorStop::new(Color::NONE, percent(100)),
                ],
            )),
            Pickable::IGNORE,
            ChildOf(column),
        ));
    }
    let ink = if local { theme::INK_1 } else { theme::INK_4 };
    commands.spawn((
        SeatName(slot),
        shot.text(
            kit,
            "",
            (
                if local {
                    theme::Face::Bold
                } else {
                    theme::Face::Regular
                },
                r::SEAT_NAME[1],
                0.0,
            ),
            ink,
        ),
        Node {
            margin: UiRect::top(shot.val(r::SEAT_NAME[0] - height)),
            ..default()
        },
        TextShadow {
            offset: Vec2::new(0.0, 1.0),
            color: Color::BLACK,
        },
        Pickable::IGNORE,
        ChildOf(column),
    ));
}

/// The ground of a seat before its hero is known.
fn ground(style: Style) -> impl Bundle {
    let (top, bottom) = match style {
        Style::Pick => (
            Color::srgba_u8(0x3A, 0x41, 0x4A, 0xD8),
            Color::srgba_u8(0x1E, 0x23, 0x29, 0xD8),
        ),
        Style::Strategy => (
            Color::srgba_u8(0x0E, 0x10, 0x14, 0xE8),
            Color::srgba_u8(0x0E, 0x10, 0x14, 0xE8),
        ),
    };
    theme::downward((top, bottom))
}

/// Keeps every seat's picture and name what the lobby says.
fn refresh(
    session: NonSend<Session>,
    choice: Res<crate::select::Choice>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut pictures: Query<(&mut SeatPicture, &mut ImageNode)>,
    mut names: Query<(&SeatName, &mut Text)>,
) {
    let core = &session.0;
    for (mut seat, mut image) in &mut pictures {
        let locked = core
            .lobby
            .slots
            .iter()
            .find(|s| s.slot == seat.slot)
            .and_then(|s| s.hero)
            .map(|h| (h.0, true));
        let preview = (seat.style == Style::Pick && core.my_slot == Some(seat.slot))
            .then_some(choice.chosen)
            .flatten()
            .map(|hero| (hero, false));
        let shown = locked.or(preview);
        if seat.shown == shown {
            continue;
        }
        seat.shown = shown;
        *image = match shown.and_then(|(hero, _)| manifest.hero_icon(hero).map(|p| (p, shown))) {
            Some((path, Some((_, true)))) => ImageNode::new(server.load(path)),
            Some((path, _)) => ImageNode::new(server.load(path)).with_color(PREVIEW),
            None => ImageNode::default().with_color(Color::NONE),
        };
    }
    for (SeatName(slot), mut text) in &mut names {
        let name = core
            .lobby
            .slots
            .iter()
            .find(|s| s.slot == *slot)
            .map(|s| s.name.as_str())
            .unwrap_or_default();
        text.set_if_neq(Text::new(name));
    }
}
