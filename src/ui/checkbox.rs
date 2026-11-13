//! A checkbox has a legible interior in either state, independent of imported icons.
use super::{Draw, effects};
use crate::theme;
use bevy::picking::hover::{DirectlyHovered, Hovered};
use bevy::prelude::*;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) struct State {
    pub checked: bool,
    pub enabled: bool,
}

#[derive(Component)]
struct Region(Entity);

#[derive(Clone, Copy)]
enum Kind {
    Empty,
    Check,
    Stroke,
}
#[derive(Component)]
struct Piece {
    owner: Entity,
    kind: Kind,
}

pub(crate) fn draw(
    kit: &mut Draw,
    parent: Entity,
    size: f32,
    corner_radius: f32,
    state: State,
) -> Entity {
    kit.commands.entity(parent).insert((
        effects::Motion::default(),
        Hovered::default(),
        DirectlyHovered::default(),
    ));
    let outer = kit.node(
        parent,
        Node {
            width: px(size),
            height: px(size),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(corner_radius)),
            ..default()
        },
    );
    kit.commands.entity(outer).insert((
        state,
        Region(parent),
        BackgroundColor::default(),
        BorderColor::default(),
    ));
    let empty = piece(
        kit,
        outer,
        outer,
        Kind::Empty,
        Node {
            width: px(size * 0.28),
            height: px(size * 0.28),
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands
        .entity(empty)
        .insert(Name::new("unchecked indicator"));
    let tick = piece(
        kit,
        outer,
        outer,
        Kind::Check,
        Node {
            width: px(size * 0.64),
            height: px(size * 0.5),
            ..default()
        },
    );
    for (left, top, width, angle) in [(0.03, 0.29, 0.29, 45.0), (0.22, 0.20, 0.47, -45.0)] {
        let stroke = piece(
            kit,
            tick,
            outer,
            Kind::Stroke,
            Node {
                position_type: PositionType::Absolute,
                left: px(size * left),
                top: px(size * top),
                width: px(size * width),
                height: px((size * 0.10).max(1.5)),
                ..default()
            },
        );
        kit.commands.entity(stroke).insert(UiTransform {
            rotation: Rot2::degrees(angle),
            ..default()
        });
    }
    outer
}

fn piece(kit: &mut Draw, parent: Entity, owner: Entity, kind: Kind, node: Node) -> Entity {
    let entity = kit.node(parent, node);
    kit.commands.entity(entity).insert((
        Piece { owner, kind },
        BackgroundColor::default(),
        BorderColor::default(),
    ));
    entity
}

fn dim(color: Color, enabled: bool) -> Color {
    if enabled {
        color
    } else {
        color.with_alpha(color.alpha() * theme::DISABLED_FOREGROUND_ALPHA)
    }
}

fn paint(
    mut boxes: Query<(&State, &Region, &mut BackgroundColor, &mut BorderColor)>,
    regions: Query<&effects::Motion>,
    states: Query<&State>,
    mut pieces: Query<(&Piece, &mut Node, &mut BackgroundColor, &mut BorderColor), Without<State>>,
) {
    for (state, region, mut fill, mut edge) in &mut boxes {
        let motion = regions.get(region.0).copied().unwrap_or_default();
        let hover = if state.enabled {
            effects::amount(motion.hover)
        } else {
            0.0
        };
        let press = if state.enabled {
            effects::amount(motion.pressed)
        } else {
            0.0
        };
        let ground = if state.checked {
            theme::TICK.0
        } else {
            theme::FIELD_TYPED
        };
        let line = if state.checked {
            theme::TICK.1
        } else {
            theme::TICK_EDGE_OFF
        };
        let ground = effects::tint(
            effects::tint(ground, line, hover * 0.12),
            Color::BLACK,
            press * 0.12,
        );
        let line = effects::tint(line, theme::INK_1, hover * 0.25);
        fill.set_if_neq(BackgroundColor(dim(ground, state.enabled)));
        edge.set_if_neq(BorderColor::all(dim(line, state.enabled)));
    }
    for (piece, mut node, mut fill, mut edge) in &mut pieces {
        let Ok(state) = states.get(piece.owner) else {
            continue;
        };
        let shown = match piece.kind {
            Kind::Empty => !state.checked,
            Kind::Check => state.checked,
            Kind::Stroke => true,
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let (ground, border) = match piece.kind {
            Kind::Empty => (Color::NONE, theme::INK_3),
            Kind::Check => (Color::NONE, Color::NONE),
            Kind::Stroke => (theme::INK_1, Color::NONE),
        };
        fill.set_if_neq(BackgroundColor(dim(ground, state.enabled)));
        edge.set_if_neq(BorderColor::all(dim(border, state.enabled)));
    }
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, paint.before(bevy::ui::UiSystems::Layout));
}
