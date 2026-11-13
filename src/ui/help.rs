//! Hover-owned context help, shared by menu and settings controls.
pub(crate) mod placement;
use crate::{theme, ui::Draw as Kit};
use bevy::{
    picking::hover::{DirectlyHovered, Hovered},
    prelude::*,
    text::TextLayoutInfo,
    ui::Val2,
};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Side {
    Left,
    Right,
    Below,
    /// Above the box, for a popup that hangs over what it is about.
    Above,
}
impl Side {
    fn width(self) -> f32 {
        match self {
            Self::Left => 286.0,
            Self::Right => 266.0,
            Self::Below | Self::Above => 300.0,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Style {
    pub diameter: f32,
    pub glyph: theme::Type,
    pub side: Side,
}
impl Style {
    pub const COMPACT: Self = Self {
        diameter: 15.0,
        glyph: theme::HELP_GLYPH,
        side: Side::Left,
    };
}

#[derive(Component)]
pub(crate) struct Hint {
    glyph: Entity,
    rim: Entity,
    tip: Entity,
}
#[derive(Component)]
#[require(UiTransform)]
struct CenterInk;

pub(crate) fn draw(kit: &mut Kit, parent: Entity, name: &str, text: &str, style: Style) -> Entity {
    let control = kit.node(
        parent,
        Node {
            width: px(22),
            height: px(22),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::MAX,
            ..default()
        },
    );
    kit.commands.entity(control).insert((
        bevy::ui_widgets::Button,
        bevy::input_focus::tab_navigation::TabIndex::default(),
        Outline::new(px(1), px(1), Color::NONE),
        super::effects::Motion::default(),
        Hovered::default(),
        DirectlyHovered::default(),
        Pickable::default(),
        Name::new(name.to_lowercase()),
    ));
    let (rim, glyph) = mark(kit, control, style);
    let tip = animated_tip(kit, control, text, style.side);
    kit.commands.entity(tip).insert(placement::Anchor {
        row: parent,
        side: style.side,
    });
    let mut accessible = bevy::a11y::AccessibilityNode::default();
    accessible.set_label(name);
    accessible.set_description(text);
    kit.commands
        .entity(control)
        .insert((Hint { glyph, rim, tip }, accessible));
    control
}

fn animated_tip(kit: &mut Kit, control: Entity, text: &str, side: Side) -> Entity {
    let tip = tooltip(kit, control, text, side);
    let offset = match side {
        Side::Left => Vec2::new(-4.0, 0.0),
        Side::Right => Vec2::new(4.0, 0.0),
        Side::Below => Vec2::new(0.0, 4.0),
        Side::Above => Vec2::new(0.0, -4.0),
    };
    kit.commands.entity(tip).insert((
        super::effects::PanelMotion::hidden(offset),
        Visibility::Hidden,
        Node {
            display: Display::None,
            ..tip_node(side)
        },
    ));
    tip
}

fn mark(kit: &mut Kit, control: Entity, style: Style) -> (Entity, Entity) {
    let rim = kit.node(
        control,
        Node {
            width: px(style.diameter),
            height: px(style.diameter),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::MAX,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.commands
        .entity(rim)
        .insert(BorderColor::all(theme::INK_4));
    let glyph = kit.typed(rim, "?", style.glyph, theme::INK_3);
    kit.commands.entity(glyph).insert(CenterInk);
    (rim, glyph)
}

fn tip_node(side: Side) -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: px(side.width()),
        left: px(0),
        top: px(0),
        padding: UiRect::axes(px(12), px(10)),
        border: UiRect::all(px(1)),
        ..default()
    }
}

pub(crate) fn tooltip(kit: &mut Kit, parent: Entity, text: &str, side: Side) -> Entity {
    let tip = kit.node(parent, tip_node(side));
    kit.commands.entity(tip).insert((
        placement::Anchor { row: parent, side },
        Name::new("context help"),
        BackgroundColor(theme::TOOLTIP),
        BorderColor::all(theme::TOOLTIP_EDGE),
        GlobalZIndex(95),
        Pickable::IGNORE,
    ));
    kit.commands.entity(tip).insert(BoxShadow::new(
        Color::BLACK.with_alpha(0.35),
        px(0),
        px(6),
        px(0),
        px(18),
    ));
    let words = kit.typed(tip, text, theme::SMALL_BODY, theme::INK_2);
    kit.commands
        .entity(words)
        .insert(bevy::text::LineHeight::RelativeToFont(1.45));
    tip
}

fn hover(
    hints: Query<(Entity, &Hint, &Hovered, &super::effects::Motion)>,
    focus: Res<bevy::input_focus::InputFocus>,
    visible: Res<bevy::input_focus::InputFocusVisible>,
    mut tips: Query<&mut super::effects::PanelMotion>,
    mut glyphs: Query<&mut TextColor>,
    mut rims: Query<&mut BorderColor>,
    mut outlines: Query<&mut Outline>,
) {
    for (entity, hint, hovered, motion) in &hints {
        let focused = visible.0 && focus.get() == Some(entity);
        if let Ok(mut panel) = tips.get_mut(hint.tip) {
            panel.open = hovered.get() || focused;
        }
        if let Ok(mut outline) = outlines.get_mut(entity) {
            let ink = if focused { theme::FOCUS } else { Color::NONE };
            if outline.color != ink {
                outline.color = ink;
            }
        }
        let fraction = super::effects::amount(motion.hover);
        if let Ok(mut ink) = glyphs.get_mut(hint.glyph) {
            ink.set_if_neq(TextColor(super::effects::tint(
                theme::INK_3,
                theme::INK_2,
                fraction,
            )));
        }
        if let Ok(mut edge) = rims.get_mut(hint.rim) {
            edge.set_if_neq(BorderColor::all(super::effects::tint(
                theme::INK_4,
                theme::INK_2,
                fraction,
            )));
        }
    }
}

/// Glyph bitmap bounds define optical placement; font line boxes include ascender/descender space.
fn center_ink(
    mut marks: Query<(&TextLayoutInfo, &ComputedNode, &mut UiTransform), With<CenterInk>>,
) {
    for (layout, node, mut transform) in &mut marks {
        let Some(first) = layout.glyphs.first() else {
            continue;
        };
        let mut bounds = Rect::from_center_size(first.position, first.atlas_info.rect.size());
        for glyph in layout.glyphs.iter().skip(1) {
            let rect = Rect::from_center_size(glyph.position, glyph.atlas_info.rect.size());
            bounds.min = bounds.min.min(rect.min);
            bounds.max = bounds.max.max(rect.max);
        }
        let shift = -(node.content_box().min + bounds.center()) * node.inverse_scale_factor();
        if shift.is_finite() {
            let desired = UiTransform::from_translation(Val2::px(shift.x, shift.y));
            transform.set_if_neq(desired);
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        hover.in_set(super::WidgetPaint).after(super::WidgetBuild),
    )
    .add_systems(
        PostUpdate,
        center_ink.after(bevy::ui::UiSystems::PostLayout),
    )
    .add_systems(
        PostUpdate,
        placement::place
            .after(bevy::ui::UiSystems::Layout)
            .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
    );
}
