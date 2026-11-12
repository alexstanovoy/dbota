//! Results-specific geometry built with the menu's typefaces and design tokens.

use super::super::{
    summary::Row,
    view::{Action, Button},
};
use crate::{main_menu::kit::Kit, theme, ui};
use bevy::prelude::*;

pub use crate::main_menu::kit::{area, at, body, bold, caption, number, rect, words};
pub use crate::theme::{
    RESULT_EDGE as EDGE, RESULT_GRID_HORIZONTAL as GRID_HORIZONTAL,
    RESULT_GRID_MAJOR as GRID_MAJOR, RESULT_GRID_MINOR as GRID_MINOR,
    RESULT_GRID_ZERO as GRID_ZERO, RESULT_PANEL as PANEL, RESULT_PANEL_DARK as PANEL_DARK,
};

#[derive(Component, Clone, Copy)]
pub struct Hover {
    pub rest: Color,
    pub over: Color,
}

pub fn button(
    kit: &mut Kit,
    parent: Entity,
    name: &str,
    node: Node,
    action: Action,
    colour: Color,
) -> Entity {
    kit.commands
        .spawn((
            bevy::ui_widgets::Button,
            Pickable::default(),
            Button(action),
            node,
            BackgroundColor(colour),
            Hover {
                rest: colour,
                over: if colour == Color::NONE {
                    Color::NONE
                } else {
                    Color::srgb_u8(0x29, 0x35, 0x3f)
                },
            },
            Name::new(name.to_owned()),
            ChildOf(parent),
        ))
        .id()
}

pub fn word_button(
    kit: &mut Kit,
    parent: Entity,
    label: &str,
    node: Node,
    action: Action,
    primary: bool,
) -> Entity {
    let entity = button(
        kit,
        parent,
        &format!("results {}", label.to_lowercase()),
        Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            ..node
        },
        action,
        if primary {
            theme::GO.1
        } else {
            theme::SECONDARY.1
        },
    );
    kit.commands
        .entity(entity)
        .insert(BorderColor::all(theme::SECONDARY_EDGE));
    let text = kit.typed(entity, label, theme::button(44.0), theme::INK_1);
    kit.commands.entity(text).insert(TextLayout::no_wrap());
    entity
}

pub fn portrait(kit: &mut Kit, parent: Entity, row: &Row, node: Node, tall: bool) -> Entity {
    let path = if tall {
        kit.manifest.portrait(row.hero.0)
    } else {
        kit.manifest.hero_icon(row.hero.0)
    };
    let entity = kit.node(parent, node);
    if let Some(path) = path {
        kit.commands
            .entity(entity)
            .insert((ImageNode::new(kit.server.load(path)), ui::Cover));
    }
    entity
}

pub fn gradient(kit: &mut Kit, parent: Entity, node: Node, from: Color, to: Color) {
    let entity = kit.node(parent, node);
    kit.commands
        .entity(entity)
        .insert(BackgroundGradient::from(LinearGradient::to_bottom(vec![
            from.into(),
            to.into(),
        ])));
}

pub fn team_colour(team: bota_proto::Team) -> Color {
    if team == bota_proto::Team::Dire {
        theme::DIRE_TEXT
    } else {
        theme::RADIANT_TEXT
    }
}

pub fn bot(kit: &mut Kit, parent: Entity, node: Node) {
    let badge = rect(
        kit,
        parent,
        Node {
            padding: UiRect::axes(px(4), px(0)),
            border: UiRect::all(px(1)),
            ..node
        },
        Color::NONE,
    );
    kit.commands
        .entity(badge)
        .insert(BorderColor::all(theme::INK_OFF));
    words(
        kit,
        badge,
        "BOT",
        Node::default(),
        caption(10.0),
        theme::INK_3,
    );
}

pub fn item(kit: &mut Kit, parent: Entity, item: Option<bota_proto::ItemId>, node: Node) -> Entity {
    let entity = rect(
        kit,
        parent,
        Node {
            border: UiRect::all(px(1)),
            ..node
        },
        theme::SECONDARY.1,
    );
    kit.commands
        .entity(entity)
        .insert((BorderColor::all(theme::DIVIDER), super::ItemSlot));
    if let Some(path) = item.and_then(|id| kit.manifest.item_icon(id.0)) {
        kit.commands
            .entity(entity)
            .insert(ImageNode::new(kit.server.load(path)));
    } else {
        kit.commands
            .entity(entity)
            .insert(ImageNode::default().with_color(Color::NONE));
    }
    entity
}

pub fn line(
    kit: &mut Kit,
    parent: Entity,
    from: Vec2,
    to: Vec2,
    width: f32,
    colour: Color,
) -> Entity {
    let centre = (from + to) / 2.0;
    let length = from.distance(to);
    let entity = rect(
        kit,
        parent,
        area(
            centre.x - length / 2.0,
            centre.y - width / 2.0,
            length,
            width,
        ),
        colour,
    );
    let direction = to - from;
    kit.commands
        .entity(entity)
        .insert(UiTransform::from_rotation(Rot2::radians(
            direction.y.atan2(direction.x),
        )));
    entity
}

pub fn dot(kit: &mut Kit, parent: Entity, point: Vec2, size: f32, colour: Color) -> Entity {
    rect(
        kit,
        parent,
        Node {
            border_radius: BorderRadius::MAX,
            ..area(point.x - size / 2.0, point.y - size / 2.0, size, size)
        },
        colour,
    )
}

pub fn round_portrait(kit: &mut Kit, parent: Entity, row: &Row, node: Node) -> Entity {
    let path = kit
        .manifest
        .dota_name("hero", row.hero.0)
        .and_then(|name| kit.manifest.file(&format!("minimap.hero.{name}")))
        .or_else(|| kit.manifest.hero_icon(row.hero.0));
    let entity = kit.node(parent, node);
    if let Some(path) = path {
        kit.commands
            .entity(entity)
            .insert(ImageNode::new(kit.server.load(path)));
    }
    entity
}
