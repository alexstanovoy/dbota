//! Debug: `--sheet items|abilities|effects` covers the window with every
//! icon of that kind the client can show, each with its bota id and name and
//! the Dota art it is drawn from, to check the mapping by eye (with
//! `--screenshot` to keep it).

use bevy::prelude::*;
use bota_client_core::{ABILITIES, EFFECTS, ITEMS};

use crate::manifest::Manifest;
use crate::ui::{self, Fonts};

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn);
}

fn spawn(
    mut commands: Commands,
    args: Res<crate::Args>,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
) {
    let Some(kind) = args.sheet.as_deref() else {
        return;
    };
    // (id, name, Dota art, icon path)
    let entries: Vec<(u16, &str, String, Option<String>)> = match kind {
        "items" => ITEMS
            .iter()
            .map(|f| {
                let dota = manifest.dota_name("item", f.id).unwrap_or("-").to_string();
                (f.id, f.name, dota, manifest.item_icon(f.id))
            })
            .collect(),
        "abilities" => ABILITIES
            .iter()
            .map(|f| {
                let dota = manifest
                    .dota_name("ability", f.id)
                    .unwrap_or("-")
                    .to_string();
                (f.id, f.name, dota, manifest.ability_icon(f.id))
            })
            .collect(),
        "effects" => EFFECTS
            .iter()
            .map(|f| {
                let dota = manifest
                    .dota_name("effect", f.id)
                    .unwrap_or("-")
                    .to_string();
                let dota = dota.trim_start_matches("icon.").to_string();
                (f.id, f.name, dota, manifest.effect_icon(f.id))
            })
            .collect(),
        other => {
            warn!("--sheet: no such kind {other}; items, abilities or effects");
            return;
        }
    };
    let (w, h) = if kind == "items" {
        (66.0, 48.0)
    } else {
        (52.0, 52.0)
    };
    let sheet = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                padding: UiRect::all(px(10)),
                column_gap: px(6),
                row_gap: px(6),
                ..default()
            },
            BackgroundColor(Color::srgb(0.06, 0.07, 0.09)),
            GlobalZIndex(100),
        ))
        .id();
    for (id, name, dota, icon) in entries {
        let cell = commands
            .spawn((
                Node {
                    width: px(150),
                    column_gap: px(6),
                    align_items: AlignItems::Center,
                    ..default()
                },
                ChildOf(sheet),
            ))
            .id();
        let missing = icon.is_none();
        commands.spawn((
            Node {
                width: px(w),
                height: px(h),
                flex_shrink: 0.0,
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(if missing {
                Color::srgb(0.9, 0.2, 0.2)
            } else {
                ui::EDGE
            }),
            ImageNode::new(icon.map(|p| server.load(p)).unwrap_or_default()),
            ChildOf(cell),
        ));
        commands.spawn((
            ui::text(
                format!("{id} {name}\n{dota}"),
                &fonts.regular,
                11.0,
                ui::INK,
            ),
            ChildOf(cell),
        ));
    }
}
