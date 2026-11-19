//! The two clipped halves of a team ability in an all-teams view.

use std::f32::consts::TAU;

use bevy::{
    ecs::system::SystemParam,
    picking::hover::{DirectlyHovered, Hovered},
    prelude::*,
    ui_widgets::Button,
};
use bota_client_core::App;
use bota_proto::Team;

use super::{dota, fill, split_disc, visible};
use crate::glyph::BUTTON;

pub(crate) const SIDES: [Team; 2] = [Team::Radiant, Team::Dire];

pub(crate) fn all_teams(core: &App) -> bool {
    core.my_slot.is_none() && core.view().is_some_and(|view| view.viewer.is_none())
}

pub(crate) fn team_label(team: Team) -> Option<&'static str> {
    match team {
        Team::Radiant => Some("DOTA_Tooltip_Glyph_Radiant"),
        Team::Dire => Some("DOTA_Tooltip_Glyph_Dire"),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Ability {
    Glyph,
    Scan,
}

#[derive(Component)]
pub(crate) struct TeamButton {
    pub(crate) ability: Ability,
    pub(crate) team: Team,
}

#[derive(Component)]
struct Pair;
#[derive(Component)]
struct Icon;
#[derive(Component)]
struct Sweep;

#[derive(SystemParam)]
pub(crate) struct Paint<'w, 's> {
    pairs: Query<'w, 's, &'static mut Visibility, With<Pair>>,
    icons: Query<'w, 's, &'static mut ImageNode, With<Icon>>,
    sweeps: Query<'w, 's, &'static mut BackgroundGradient, With<Sweep>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct FaceState {
    waiting: bool,
    angle: u16,
}

impl FaceState {
    pub(crate) fn new(waiting: bool, left: f32) -> Self {
        Self {
            waiting,
            angle: if waiting {
                ((1.0 - left.clamp(0.0, 1.0)) * 720.0) as u16
            } else {
                720
            },
        }
    }
}

struct Face {
    icon: Entity,
    sweep: Entity,
    ready: Option<Handle<Image>>,
    last: Option<FaceState>,
}

pub(crate) struct TeamFaces {
    pair: Entity,
    sides: [Face; 2],
    dimmed: Option<Handle<Image>>,
}

impl TeamFaces {
    pub(crate) fn spawn(
        commands: &mut Commands,
        parent: Entity,
        ability: Ability,
        ready: [Option<Handle<Image>>; 2],
        dimmed: Option<Handle<Image>>,
    ) -> Self {
        let pair = commands
            .spawn((
                Pair,
                Node {
                    left: px(0),
                    top: px(0),
                    width: dota(BUTTON),
                    height: dota(BUTTON),
                    ..fill()
                },
                Visibility::Hidden,
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id();
        let sides = std::array::from_fn(|index| {
            let team = SIDES[index];
            let name = match (ability, team) {
                (Ability::Glyph, Team::Radiant) => "hud glyph radiant",
                (Ability::Glyph, _) => "hud glyph dire",
                (Ability::Scan, Team::Radiant) => "hud scan radiant",
                (Ability::Scan, _) => "hud scan dire",
            };
            spawn_face(
                commands,
                pair,
                name,
                TeamButton { ability, team },
                ready[index].clone(),
            )
        });
        commands.spawn((
            Node {
                left: dota(BUTTON / 2.0 - 1.0),
                top: percent(2),
                width: dota(2.0),
                height: percent(96),
                ..fill()
            },
            BackgroundColor(Color::BLACK),
            Pickable::IGNORE,
            ChildOf(pair),
        ));
        Self {
            pair,
            sides,
            dimmed,
        }
    }

    pub(crate) fn paint(&mut self, states: Option<[FaceState; 2]>, paint: &mut Paint) {
        if let Ok(mut pair) = paint.pairs.get_mut(self.pair) {
            pair.set_if_neq(visible(states.is_some()));
        }
        let Some(states) = states else { return };
        for (face, state) in self.sides.iter_mut().zip(states) {
            let previous = face.last.replace(state);
            if previous == Some(state) {
                continue;
            }
            if previous.is_none_or(|previous| previous.waiting != state.waiting)
                && let Ok(mut icon) = paint.icons.get_mut(face.icon)
            {
                let art = if state.waiting {
                    self.dimmed.as_ref()
                } else {
                    face.ready.as_ref()
                };
                icon.color = if art.is_some() {
                    Color::WHITE
                } else {
                    Color::NONE
                };
                icon.image = art.cloned().unwrap_or_default();
            }
            if let Ok(mut sweep) = paint.sweeps.get_mut(face.sweep) {
                let alpha = if state.waiting { 0.87 } else { 0.0 };
                *sweep = split_disc(
                    f32::from(state.angle) * TAU / 720.0,
                    Color::NONE,
                    Color::srgba(0.0, 0.0, 0.0, alpha),
                );
            }
        }
    }
}

fn spawn_face(
    commands: &mut Commands,
    parent: Entity,
    name: &'static str,
    button: TeamButton,
    ready: Option<Handle<Image>>,
) -> Face {
    let left = button.team == Team::Radiant;
    let half = commands
        .spawn((
            button,
            Name::new(name),
            Button,
            Hovered::default(),
            DirectlyHovered::default(),
            Node {
                position_type: PositionType::Absolute,
                left: if left { px(0) } else { percent(50) },
                top: px(0),
                width: percent(50),
                height: percent(100),
                overflow: Overflow::clip(),
                ..default()
            },
            ChildOf(parent),
        ))
        .id();
    // The texture is a full disc. Both panels show its left half, mirrored on the Dire side.
    let paint = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: if left { px(0) } else { Val::Auto },
                right: if left { Val::Auto } else { px(0) },
                top: px(0),
                width: dota(BUTTON),
                height: dota(BUTTON),
                ..default()
            },
            UiTransform::from_scale(Vec2::new(if left { 1.0 } else { -1.0 }, 1.0)),
            Pickable::IGNORE,
            ChildOf(half),
        ))
        .id();
    let disc = Node {
        border_radius: BorderRadius::MAX,
        ..fill()
    };
    let icon = commands
        .spawn((
            Icon,
            disc.clone(),
            ImageNode::default().with_color(Color::NONE),
            Pickable::IGNORE,
            ChildOf(paint),
        ))
        .id();
    let sweep = commands
        .spawn((
            Sweep,
            disc,
            split_disc(TAU, Color::NONE, Color::NONE),
            Pickable::IGNORE,
            ChildOf(paint),
        ))
        .id();
    Face {
        icon,
        sweep,
        ready,
        last: None,
    }
}
