//! The avatar preview and editor. Pixel buffers are validated before a UI texture is created.
use super::{
    avatar::{Action, View},
    core::Act,
    kit::{Kit, MenuButton, Tone},
};
use crate::theme;
use bevy::ui_widgets::Button;
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use dbota_import::thumbnail::Pixels;
use std::sync::Arc;

pub fn header(kit: &mut Kit, parent: Entity, name: &str, view: &View) {
    let frame = kit.node(
        parent,
        Node {
            width: px(56),
            height: px(56),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands.entity(frame).insert((
        theme::downward(theme::AVATAR),
        BorderColor::all(theme::AVATAR_EDGE),
    ));
    if let Some(pixels) = &view.current
        && view.available
    {
        picture(kit, frame, pixels.clone(), 54.0);
    } else {
        kit.bold(
            frame,
            name.chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default(),
            24.0,
            theme::INK_1,
        );
    }
    if !view.available {
        return;
    }
    let edit = kit.node(
        frame,
        Node {
            position_type: PositionType::Absolute,
            right: px(-6),
            bottom: px(-6),
            width: px(24),
            height: px(24),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(12)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    kit.commands.entity(edit).insert((
        Button,
        Pickable::default(),
        MenuButton(Act::Avatar(Action::Open)),
        Name::new("edit avatar"),
        BackgroundColor(theme::AVATAR.0),
        BorderColor::all(theme::SELECT),
    ));
    kit.art(
        edit,
        "menu.edit",
        Node {
            width: px(12),
            height: px(12),
            ..default()
        },
    );
}

pub fn dialog(kit: &mut Kit, root: Entity, view: &View) {
    if !view.open || !view.available {
        return;
    }
    let scrim = kit.node(root, crate::ui::fill());
    kit.commands.entity(scrim).insert((
        Pickable::default(),
        crate::hud::Blocker,
        bevy::picking::hover::Hovered::default(),
        BackgroundColor(theme::scrim()),
        GlobalZIndex(80),
    ));
    let box_ = kit.node(
        scrim,
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            margin: UiRect::new(px(-240), px(0), px(-146), px(0)),
            width: px(480),
            padding: UiRect::all(px(28)),
            flex_direction: FlexDirection::Column,
            row_gap: px(18),
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands.entity(box_).insert((
        theme::downward(theme::PANEL),
        BorderColor::all(theme::SECONDARY_EDGE),
    ));
    let title = kit.node(
        box_,
        Node {
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    kit.bold(title, "CHANGE AVATAR", 24.0, theme::INK_1);
    let row = kit.node(
        box_,
        Node {
            column_gap: px(20),
            ..default()
        },
    );
    crop_preview(kit, row, view);
    upload_controls(kit, row, view);
    if let Some(notice) = &view.notice {
        kit.text(box_, notice, 13.0, theme::INK_3);
    }
    dialog_actions(kit, box_, view);
}

fn crop_preview(kit: &mut Kit, row: Entity, view: &View) {
    let preview = kit.node(
        row,
        Node {
            width: px(128),
            height: px(128),
            flex_shrink: 0.0,
            border: UiRect::all(px(1)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    kit.commands.entity(preview).insert((
        BackgroundColor(theme::AVATAR.1),
        BorderColor::all(theme::AVATAR_EDGE),
    ));
    if let Some(pixels) = &view.preview {
        picture(kit, preview, pixels.clone(), 128.0);
    } else {
        kit.text(
            preview,
            "Preview of the\nuploaded image\n128 × 128",
            12.0,
            theme::INK_3,
        );
    }
}

fn upload_controls(kit: &mut Kit, row: Entity, view: &View) {
    let controls = kit.node(
        row,
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            ..default()
        },
    );
    let upload = button(
        kit,
        controls,
        "UPLOAD IMAGE",
        Action::Upload,
        view.available && !view.busy,
    );
    kit.commands
        .entity(upload)
        .entry::<Node>()
        .and_modify(|mut node| node.width = percent(100));
    kit.text(
        controls,
        "PNG or JPG, up to 256 KB. Cropped to a square and shown at 128 × 128.",
        13.0,
        theme::INK_3,
    );
    if view.available && !view.busy {
        kit.quiet(
            controls,
            "Remove avatar",
            Act::Avatar(Action::Remove),
            theme::BODY,
            (theme::LOSS, theme::INK_1),
        );
    } else {
        kit.text(controls, "Remove avatar", 14.0, theme::INK_OFF);
    }
}

fn dialog_actions(kit: &mut Kit, box_: Entity, view: &View) {
    let actions = kit.node(
        box_,
        Node {
            justify_content: JustifyContent::Center,
            column_gap: px(16),
            ..default()
        },
    );
    for (label, action, enabled) in [
        ("SAVE", Action::Save, view.available && !view.busy),
        ("CANCEL", Action::Cancel, !view.busy),
    ] {
        let button = button(kit, actions, label, action, enabled);
        kit.commands
            .entity(button)
            .entry::<Node>()
            .and_modify(|mut node| node.width = px(160));
    }
}

fn button(kit: &mut Kit, parent: Entity, label: &str, action: Action, enabled: bool) -> Entity {
    if enabled {
        kit.button(parent, label, Act::Avatar(action), Tone::Secondary, 44.0)
    } else {
        kit.disabled_button(parent, label, 44.0)
    }
}

fn picture(kit: &mut Kit, parent: Entity, pixels: Arc<Pixels>, size: f32) {
    let picture = kit.node(
        parent,
        Node {
            width: px(size),
            height: px(size),
            ..default()
        },
    );
    kit.commands.queue(move |world: &mut World| {
        if world.get_entity(picture).is_err() {
            return;
        }
        let image = Image::new(
            Extent3d {
                width: pixels.width(),
                height: pixels.height(),
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels.rgba().to_vec(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        let handle = world.resource_mut::<Assets<Image>>().add(image);
        world.entity_mut(picture).insert(ImageNode::new(handle));
    });
}
