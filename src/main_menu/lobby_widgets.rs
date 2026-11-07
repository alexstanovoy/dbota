//! The lobby's fields, choice lists and small controls, using the dashboard kit.
use super::{
    core::Act,
    kit::{Kit, MenuButton, Tone},
    lobby_controls::{Action, Choice},
    view::FieldView,
};
use crate::theme;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use theme::lobby::{RIGHT, WIDTH};
pub const FIELD_HEIGHT: f32 = 44.0;

pub fn action(action: Action) -> Act {
    Act::LobbyUi(action)
}

pub fn column(kit: &mut Kit, parent: Entity, gap: f32) -> Entity {
    kit.node(
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(gap),
            min_width: px(0),
            ..default()
        },
    )
}

pub fn panel(kit: &mut Kit, root: Entity, name: &str) -> Entity {
    let panel = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            right: px(RIGHT),
            top: px(theme::BAR_HEIGHT),
            bottom: px(0),
            width: px(WIDTH),
            padding: UiRect {
                left: px(16),
                right: px(24),
                top: px(14),
                bottom: px(16),
            },
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            border: UiRect::left(px(1)),
            ..default()
        },
    );
    kit.commands.entity(panel).insert((
        Name::new(name.to_owned()),
        BorderColor::all(theme::SECONDARY_EDGE),
        BackgroundGradient::from(LinearGradient {
            angle: std::f32::consts::PI,
            stops: theme::lobby::PANEL
                .into_iter()
                .map(|(color, at)| ColorStop::new(color, percent(at)))
                .collect(),
            ..default()
        }),
    ));
    panel
}

pub fn quiet(kit: &mut Kit, parent: Entity, label: &str, act: Act, ink: Color) -> Entity {
    let button = kit.quiet(
        parent,
        label,
        act,
        theme::Type {
            size: 13.0,
            ..theme::LABEL
        },
        (ink, theme::INK_1),
    );
    kit.commands
        .entity(button)
        .entry::<Node>()
        .and_modify(|mut node| node.min_height = px(32));
    button
}

pub fn rule(kit: &mut Kit, parent: Entity) {
    let line = kit.node(
        parent,
        Node {
            width: percent(100),
            height: px(1),
            flex_shrink: 0.0,
            ..default()
        },
    );
    kit.commands
        .entity(line)
        .insert(BackgroundColor(theme::lobby::RULE));
}

pub fn full_disabled(kit: &mut Kit, parent: Entity, label: &str, height: f32) -> Entity {
    let button = kit.disabled_button(parent, label, height);
    kit.commands
        .entity(button)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.width = percent(100);
            node.min_width = px(0);
        });
    button
}

pub fn full_button(
    kit: &mut Kit,
    parent: Entity,
    label: &str,
    act: Act,
    tone: Tone,
    height: f32,
) -> Entity {
    let button = kit.button(parent, label, act, tone, height);
    kit.commands
        .entity(button)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.width = percent(100);
            node.min_width = px(0);
        });
    button
}

pub fn check(kit: &mut Kit, parent: Entity, label: &str, checked: bool, act: Act) -> Entity {
    let row = kit.node(
        parent,
        Node {
            height: px(32),
            align_items: AlignItems::Center,
            column_gap: px(12),
            ..default()
        },
    );
    kit.commands
        .entity(row)
        .insert((Button, Pickable::default(), MenuButton(act), Name::new(label.to_lowercase())));
    let square = kit.node(
        row,
        Node {
            width: px(20),
            height: px(20),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.commands
        .entity(square)
        .insert(BackgroundColor(if checked {
            theme::lobby::CHECKED
        } else {
            Color::WHITE
        }));
    if checked {
        kit.bold(square, "✓", 18.0, Color::WHITE);
    }
    kit.text(row, label, 14.0, theme::INK_2);
    row
}

pub fn field(
    kit: &mut Kit,
    parent: Entity,
    index: usize,
    field: &FieldView,
    readonly: bool,
    error: bool,
) -> Entity {
    let column = column(kit, parent, 4.0);
    kit.text(column, field.label, 13.0, theme::INK_4);
    let node = input(
        kit,
        column,
        field.label,
        if readonly {
            None
        } else {
            Some(Act::Focus(index))
        },
        if error {
            theme::ERROR_TEXT
        } else if field.focused {
            theme::FOCUS
        } else {
            theme::SECONDARY_EDGE
        },
    );
    kit.text(node, &field.shown, 14.0, theme::INK_1);
    if readonly {
        let mut accessible = bevy::a11y::AccessibilityNode::default();
        accessible.set_label(field.label);
        accessible.set_description("Fixed after creating the lobby");
        accessible.set_read_only();
        kit.commands.entity(node).insert(accessible);
    }
    column
}

pub fn input(kit: &mut Kit, parent: Entity, label: &str, act: Option<Act>, edge: Color) -> Entity {
    let node = kit.node(
        parent,
        Node {
            height: px(FIELD_HEIGHT),
            min_width: px(0),
            width: percent(100),
            padding: UiRect::axes(px(10), px(0)),
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands.entity(node).insert((
        Name::new(label.to_lowercase()),
        BackgroundColor(theme::FIELD_SUNKEN),
        BorderColor::all(edge),
    ));
    if let Some(act) = act {
        kit.commands.entity(node).insert((Button, Pickable::default(), MenuButton(act)));
    }
    node
}

pub struct OptionRow {
    pub name: String,
    pub selected: bool,
    pub act: Act,
}

pub fn select(
    kit: &mut Kit,
    parent: Entity,
    label: &str,
    value: &str,
    choice: Choice,
    options: Option<Vec<OptionRow>>,
    above: bool,
    readonly: bool,
) {
    let column = column(kit, parent, 4.0);
    kit.text(column, label, 13.0, theme::INK_4);
    let selector = input(
        kit,
        column,
        label,
        (!readonly).then(|| action(Action::Open(choice))),
        theme::SECONDARY_EDGE,
    );
    kit.commands
        .entity(selector)
        .insert(BackgroundColor(theme::FIELD));
    let text = kit.text(selector, value, 14.0, theme::INK_1);
    kit.commands.entity(text).insert(Node {
        flex_grow: 1.0,
        ..default()
    });
    if !readonly {
        kit.text(selector, "⌄", 15.0, theme::INK_3);
    }
    if let Some(options) = options {
        choice_list(kit, column, options, above);
    }
}

fn choice_list(kit: &mut Kit, parent: Entity, options: Vec<OptionRow>, above: bool) {
    let list = kit.node(
        parent,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: if above { Val::Auto } else { px(68) },
            bottom: if above { px(48) } else { Val::Auto },
            flex_direction: FlexDirection::Column,
            border: UiRect::all(px(1)),
            max_height: px(264),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    );
    kit.commands.entity(list).insert((
        BackgroundColor(theme::lobby::CHOICES),
        BorderColor::all(theme::SELECT),
        ZIndex(8),
        ScrollPosition::default(),
        Name::new("lobby choices"),
    ));
    for option in options.into_iter().take(32) {
        let row = kit.node(
            list,
            Node {
                height: px(44),
                flex_shrink: 0.0,
                padding: UiRect::axes(px(10), px(0)),
                align_items: AlignItems::Center,
                border: UiRect::left(px(3)),
                ..default()
            },
        );
        kit.commands.entity(row).insert((
            Button,
        Pickable::default(),
            MenuButton(option.act),
            Name::new(option.name.to_lowercase()),
            BorderColor::all(if option.selected {
                theme::SELECT
            } else {
                Color::NONE
            }),
        ));
        if option.selected {
            kit.commands
                .entity(row)
                .insert(theme::rightward(theme::SELECT_FILL));
        }
        kit.text(row, option.name, 14.0, theme::INK_1);
    }
}

pub fn lock(kit: &mut Kit, parent: Entity) {
    let area = kit.node(
        parent,
        Node {
            width: px(16),
            height: px(16),
            flex_shrink: 0.0,
            ..default()
        },
    );
    for (left, top, width, height) in [(4, 7, 8, 7), (6, 3, 4, 6)] {
        let part = kit.node(
            area,
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(width),
                height: px(height),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(1)),
                ..default()
            },
        );
        kit.commands
            .entity(part)
            .insert(BorderColor::all(theme::GOLD));
    }
}
