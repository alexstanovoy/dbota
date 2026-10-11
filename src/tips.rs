//! The hover popup: what an ability, an item or a shop cell does, what it
//! costs and how long it waits.

use bevy::{prelude::*, window::PrimaryWindow};
use bota_client_core::{ability, item};

use crate::hud::{AbilityBox, ItemBox};
use crate::session::Session;
use crate::shop::ShopCell;
use crate::ui::{self, Fonts};

/// The popup box.
#[derive(Component)]
struct Tip;

/// The popup's text.
#[derive(Component)]
struct TipText;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(
        Update,
        show.run_if(|k: Res<crate::session::MatchKnown>| k.0),
    );
}

fn spawn(mut commands: Commands, fonts: Res<Fonts>) {
    let tip = commands
        .spawn((
            Tip,
            Node {
                position_type: PositionType::Absolute,
                max_width: px(360),
                padding: UiRect::all(px(10)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.97)),
            BorderColor::all(ui::EDGE),
            GlobalZIndex(30),
            Pickable::IGNORE,
            Visibility::Hidden,
        ))
        .id();
    commands.spawn((
        TipText,
        ui::text("", &fonts.regular, 14.0, ui::INK),
        Pickable::IGNORE,
        ChildOf(tip),
    ));
}

/// Puts the popup by the cursor over whatever slot or cell it is on.
fn show(
    session: NonSend<Session>,
    abilities: Query<(&Interaction, &AbilityBox)>,
    items: Query<(&Interaction, &ItemBox)>,
    cells: Query<(&Interaction, &ShopCell)>,
    buffs: Query<(&Interaction, &crate::buffs::BuffChip)>,
    talent_cells: Query<(&Interaction, &crate::talents::TalentCell)>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut tip: Single<(&mut Node, &mut Visibility), With<Tip>>,
    mut text: Single<&mut Text, With<TipText>>,
) {
    let core = &session.0;
    let rate = u32::from(core.tick_rate.max(1));
    let unit = core
        .slot_unit()
        .or_else(|| core.ears().and_then(|id| core.known(id)).map(|(u, _)| u));
    let hovered_ability = abilities
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .map(|(_, b)| b.0);
    let hovered_item = items
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .map(|(_, b)| b.0);
    let hovered_cell = cells
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .map(|(_, c)| c.0);
    let hovered_buff = buffs
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .and_then(|(_, b)| b.shown);
    let hovered_talent = talent_cells
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .map(|(_, c)| c.0);
    let body = if let Some(shown) = hovered_buff {
        bota_client_core::effect(shown.id).map(|face| {
            let mut lines = vec![face.name.to_string(), face.blurb.to_string()];
            if let Some(n) = shown.stacks.filter(|n| *n > 0) {
                lines.push(format!("{n} stacks"));
            }
            if let Some(t) = shown.ticks_left {
                lines.push(format!("{:.1} s left", t as f32 / rate as f32));
            }
            lines.join("\n")
        })
    } else if let Some(slot) = hovered_talent {
        crate::hud::subject(core)
            .1
            .and_then(|u| u.hero)
            .and_then(|h| bota_client_core::talents(h.0))
            .map(|tree| {
                let face = &tree[usize::from(slot)];
                let level = bota_client_core::TALENT_LEVELS[usize::from(slot / 2)];
                format!(
                    "{}\n{}\nOpens at level {level}; the other of its tier at {}.",
                    face.name,
                    face.blurb,
                    bota_client_core::TALENT_SECOND_LEVELS[usize::from(slot / 2)]
                )
            })
    } else if let Some(slot) = hovered_ability {
        unit.and_then(|u| u.abilities.get(usize::from(slot)))
            .and_then(|a| {
                let face = ability(a.id.0)?;
                let mut lines = vec![
                    format!("{}  (level {}/{})", face.name, a.level, a.max_level),
                    face.blurb.to_string(),
                ];
                if a.mana_cost > 0 && a.level > 0 {
                    lines.push(format!("Mana {}", a.mana_cost));
                }
                if a.cooldown_left > 0 {
                    lines.push(format!("Ready in {} s", a.cooldown_left / rate + 1));
                }
                if a.can_level {
                    lines.push("Ctrl + its key: learn".to_string());
                }
                Some(lines.join("\n"))
            })
    } else if let Some(slot) = hovered_item {
        core.item_in(slot).and_then(|i| {
            let face = item(i.id.0)?;
            let mut lines = vec![
                face.name.to_string(),
                face.stats.to_string(),
                face.blurb.to_string(),
            ];
            if i.cooldown_left > 0 {
                lines.push(format!("Ready in {} s", i.cooldown_left / rate + 1));
            }
            if slot >= bota_client_core::INVENTORY_SLOTS
                && slot != bota_client_core::TOWN_PORTAL_SLOT
            {
                lines.push("Drag it into the inventory to use it.".to_string());
            }
            Some(lines.join("\n"))
        })
    } else if let Some(id) = hovered_cell {
        item(id).map(|face| {
            let (now, whole) = core.price_of(id);
            let mut lines = vec![
                format!("{}  {} gold", face.name, now),
                face.stats.to_string(),
                face.blurb.to_string(),
            ];
            if now < whole {
                lines.push(format!("{} gold of it is already in hand.", whole - now));
            }
            lines.push("Click: buy   Right-click: quick-buy".to_string());
            lines.join("\n")
        })
    } else {
        None
    };
    let (node, visibility) = &mut *tip;
    match (body, window.cursor_position()) {
        (Some(body), Some(cursor)) => {
            if text.0 != body {
                text.0 = body;
            }
            let scale = ui_scale.0.max(0.1);
            let x = (cursor.x / scale + 16.0).min(window.width() / scale - 370.0);
            node.left = px(x.max(4.0));
            node.bottom = px((window.height() - cursor.y) / scale + 16.0);
            **visibility = Visibility::Inherited;
        }
        _ => {
            if **visibility != Visibility::Hidden {
                **visibility = Visibility::Hidden;
            }
        }
    }
}
