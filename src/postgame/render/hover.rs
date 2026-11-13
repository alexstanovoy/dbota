//! Compact result hints use recorded purchase times and the imported item names.
use super::super::{
    graphs,
    summary::Row,
    view::{Page, View},
};
use super::primitives::*;
use crate::{main_menu::kit::Kit, theme};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bota_proto::ItemId;

#[derive(Component)]
pub struct ItemHint {
    pub item: Option<ItemId>,
    pub key: String,
    pub bought: Option<i64>,
    pub forced: bool,
    pub left: f32,
}
#[derive(Component)]
pub struct ItemTooltip;
#[derive(Component)]
pub struct DetailsHint {
    pub button: Entity,
    pub frame: Entity,
    pub rest: Color,
    pub forced: bool,
}

pub fn recorded_item(
    kit: &mut Kit,
    parent: Entity,
    view: &View,
    row: &Row,
    index: usize,
    node: Node,
) -> Entity {
    let id = row.items.get(index).copied().flatten();
    let illustration = view
        .illustrated_items
        .iter()
        .find(|inventory| inventory.slot == row.slot);
    let key = illustration
        .and_then(|inventory| inventory.items.get(index))
        .cloned()
        .flatten()
        .or_else(|| {
            id.and_then(bota_core::item_def)
                .map(|item| item.key.trim_start_matches("item_").to_owned())
        });
    let bought = if let Some(inventory) = illustration {
        key.as_ref().and_then(|key| {
            inventory
                .purchases
                .iter()
                .filter(|(item, _)| item == key)
                .map(|(_, second)| *second)
                .max()
        })
    } else {
        view.summary.recording.as_ref().and_then(|recording| {
            recording
                .purchases
                .iter()
                .filter(|purchase| purchase.slot == row.slot && Some(purchase.item) == id)
                .map(|purchase| purchase.second)
                .max()
        })
    };
    let icon = item(kit, parent, id, node);
    if let Some(key) = key {
        if let Some(path) = kit.manifest.file(&format!("icon.item.{key}")) {
            kit.commands
                .entity(icon)
                .insert(ImageNode::new(kit.server.load(path)));
        }
        let forced = view.player.is_none()
            && view.page == Page::Scoreboard
            && view.hover_player == Some(row.slot)
            && key == "desolator";
        kit.commands.entity(icon).insert((
            RelativeCursorPosition::default(),
            ItemHint {
                item: id,
                key,
                bought,
                forced,
                left: if view.player.is_some() { 0.0 } else { -176.0 },
            },
        ));
    }
    icon
}

#[allow(clippy::too_many_arguments)]
pub fn items(
    commands: Commands,
    fonts: Res<crate::ui::Fonts>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
    texts: Res<crate::dota_text::Texts>,
    icons: Query<(Entity, &RelativeCursorPosition, &ItemHint)>,
    tips: Query<Entity, With<ItemTooltip>>,
    mut shown: Local<Option<Entity>>,
) {
    let selected = icons
        .iter()
        .find(|(_, cursor, hint)| hint.forced || cursor.cursor_over);
    let next = selected.map(|(entity, _, _)| entity);
    if *shown == next {
        return;
    }
    *shown = next;
    let mut kit = Kit {
        commands,
        fonts: &fonts,
        manifest: &manifest,
        texts: &texts,
        server: &server,
    };
    for tip in &tips {
        kit.commands.entity(tip).despawn();
    }
    let Some((icon, _, hint)) = selected else {
        return;
    };
    let lookup = crate::tip_content::Lookup {
        texts: &texts,
        manifest: &manifest,
        rates: Default::default(),
    };
    let tip = rect(
        &mut kit,
        icon,
        Node {
            border: UiRect::all(px(1)),
            ..area(hint.left, 32.0, 220.0, 58.0)
        },
        theme::TOOLTIP,
    );
    kit.commands.entity(tip).insert((
        ItemTooltip,
        GlobalZIndex(100),
        BorderColor::all(theme::TOOLTIP_EDGE),
        Name::new("results item tooltip"),
    ));
    let picture = item(&mut kit, tip, hint.item, area(12.0, 10.0, 48.0, 36.0));
    if let Some(path) = manifest.file(&format!("icon.item.{}", hint.key)) {
        kit.commands
            .entity(picture)
            .insert(ImageNode::new(server.load(path)));
    }
    words(
        &mut kit,
        tip,
        texts
            .get(&format!("item_{}", hint.key))
            .map(|entry| entry.name.clone())
            .or_else(|| hint.item.and_then(|item| lookup.item_name(item.0)))
            .unwrap_or_else(|| "Unknown item".to_owned()),
        at(70.0, 12.0),
        bold(14.0),
        theme::INK_1,
    );
    let time = hint.bought.map_or_else(
        || "Purchase time not recorded".to_owned(),
        |second| {
            format!(
                "Bought {}{}",
                if second < 0 { "−" } else { "" },
                graphs::clock(second.unsigned_abs().min(u64::from(u32::MAX)) as u32)
            )
        },
    );
    words(
        &mut kit,
        tip,
        time,
        at(70.0, 32.0),
        body(12.0),
        theme::INK_3,
    );
}
pub fn details(
    buttons: Query<&Hovered>,
    mut frames: Query<&mut BorderColor>,
    mut hints: Query<(&DetailsHint, &mut Visibility)>,
) {
    for (hint, mut visibility) in &mut hints {
        let active = hint.forced || buttons.get(hint.button).is_ok_and(Hovered::get);
        visibility.set_if_neq(crate::ui::visible(active));
        if let Ok(mut border) = frames.get_mut(hint.frame) {
            border.set_if_neq(BorderColor::all(if active {
                theme::INK_2
            } else {
                hint.rest
            }));
        }
    }
}
