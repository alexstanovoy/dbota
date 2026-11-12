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

#[derive(Component)]
pub struct Control;

pub fn focus_ring(
    core: Option<NonSend<crate::main_menu::MenuCore>>,
    mut commands: Commands,
    controls: Query<(Entity, &MenuButton, Has<Outline>), With<Control>>,
) {
    let focused = core.as_ref().and_then(|core| core.button_focus());
    for (entity, button, has_outline) in &controls {
        let wanted = focused == Some(&button.0);
        if wanted && !has_outline {
            commands.entity(entity).insert(Outline {
                width: px(1),
                offset: px(-1),
                color: theme::FOCUS,
            });
        } else if !wanted && has_outline {
            commands.entity(entity).remove::<Outline>();
        }
    }
}

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
                left: px(15),
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
    let button = kit.quiet(parent, label, act, theme::SMALL_BODY, (ink, theme::INK_1));
    kit.commands
        .entity(button)
        .entry::<Node>()
        .and_modify(|mut node| node.min_height = px(32));
    kit.commands.entity(button).insert(Control);
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
    let button = kit.disabled_button_role(parent, label, height, theme::lobby::ROOM_ACTION);
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
    role: theme::Type,
) -> Entity {
    let button = kit.button_role(parent, label, act, tone, height, role);
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
    kit.commands.entity(row).insert((
        Button,
        Control,
        Pickable::default(),
        MenuButton(act),
        Name::new(label.to_lowercase()),
    ));
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
        kit.typed(square, "✓", theme::lobby::CHECK_MARK, Color::WHITE);
    }
    kit.typed(row, label, theme::BODY, theme::INK_2);
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
    kit.typed(column, field.label, theme::SMALL_BODY, theme::INK_4);
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
    kit.typed(node, &field.shown, theme::BODY, theme::INK_1);
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
        kit.commands
            .entity(node)
            .insert((Button, Control, Pickable::default(), MenuButton(act)));
    }
    node
}

use super::lobby_controls::Choices;

pub fn select(
    kit: &mut Kit,
    parent: Entity,
    label: &str,
    value: &str,
    choice: Choice,
    options: Option<Choices>,
    above: bool,
    readonly: bool,
) {
    let column = column(kit, parent, 4.0);
    kit.typed(column, label, theme::SMALL_BODY, theme::INK_4);
    let selector = input(
        kit,
        column,
        label,
        (!readonly).then(|| action(Action::Open(choice))),
        theme::SECONDARY_EDGE,
    );
    kit.commands.entity(selector).insert((
        BackgroundColor(theme::FIELD),
        Name::new(match choice {
            Choice::Filter => "filter game mode",
            Choice::Mode => "new lobby game mode",
            Choice::EditMode => "lobby game mode",
            Choice::Server => "server",
            Choice::Audience => "open to",
        }),
    ));
    let text = kit.typed(selector, value, theme::BODY, theme::INK_1);
    kit.commands.entity(text).insert(Node {
        flex_grow: 1.0,
        ..default()
    });
    if !readonly {
        kit.typed(selector, "⌄", theme::LABEL, theme::INK_3);
    }
    if let Some(options) = options {
        choice_list(kit, column, options, above);
    }
}

fn choice_list(kit: &mut Kit, parent: Entity, options: Choices, above: bool) {
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
            max_height: px(theme::lobby::CHOICE_HEIGHT),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    );
    kit.commands.entity(list).insert((
        BackgroundColor(theme::lobby::CHOICES),
        BorderColor::all(theme::SELECT),
        ZIndex(8),
        ScrollPosition::default(),
        super::lobby_scroll::Area::Choices,
        Pickable::default(),
        bevy::picking::hover::Hovered::default(),
        crate::hud::Blocker,
        Name::new("lobby choices"),
    ));
    let first = options.first.min(options.rows.len());
    let shown = theme::lobby::CHOICE_RENDER_ROWS.min(options.rows.len() - first);
    choice_space(kit, list, first);
    let remaining = options.rows.len() - first - shown;
    for option in options.rows.into_iter().skip(first).take(shown) {
        choice_row(kit, list, option);
    }
    choice_space(kit, list, remaining);
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

fn choice_row(kit: &mut Kit, list: Entity, option: super::lobby_controls::ChoiceRow) {
    let row = kit.node(
        list,
        Node {
            height: px(theme::lobby::CHOICE_ROW_HEIGHT),
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
        MenuButton(action(option.action)),
        Control,
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
            .insert(theme::rightward(theme::lobby::SELECTION));
    }
    kit.typed(row, option.name, theme::BODY, theme::INK_1);
}

fn choice_space(kit: &mut Kit, parent: Entity, rows: usize) {
    if rows > 0 {
        kit.node(
            parent,
            Node {
                height: px(rows as f32 * theme::lobby::CHOICE_ROW_HEIGHT),
                flex_shrink: 0.0,
                ..default()
            },
        );
    }
}
