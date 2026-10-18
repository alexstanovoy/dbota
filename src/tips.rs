//! The hover popup: what an ability, an item or a shop cell does, what it
//! costs and how long it waits.

use bevy::{prelude::*, window::PrimaryWindow};
use bota_client_core::{ability, item};
use bota_proto::TalentSlot;

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
    attribute: Query<&Interaction, With<crate::talents::AttributeButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut tip: Single<(&mut Node, &mut Visibility), With<Tip>>,
    mut text: Single<&mut Text, With<TipText>>,
) {
    let core = &session.0;
    let rate = u32::from(core.tick_rate().max(1));
    let buff = buffs
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .and_then(|(_, b)| b.shown);
    let body = if attribute.iter().any(|i| *i != Interaction::None) {
        Some(attribute_tip(core))
    } else if let Some(shown) = buff {
        buff_tip(shown, rate)
    } else if let Some(cell) = hovered(&talent_cells) {
        talent_tip(core, cell.0)
    } else if let Some(slot) = hovered(&abilities) {
        ability_tip(core, slot.0, rate)
    } else if let Some(slot) = hovered(&items) {
        item_tip(core, slot.0, rate)
    } else if let Some(cell) = hovered(&cells) {
        shop_tip(core, cell.0)
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
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

/// What of a query's boxes the cursor is over, if any.
fn hovered<'a, T: Component>(boxes: &'a Query<(&Interaction, &T)>) -> Option<&'a T> {
    boxes
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .map(|(_, b)| b)
}

fn attribute_tip(core: &bota_client_core::App) -> String {
    let level = crate::hud::subject(core)
        .0
        .and_then(|p| core.talents_of(p.slot))
        .map_or(0, |t| t.attribute_bonus);
    format!(
        "Attribute Bonus  (level {level}/{})\n+{} to all attributes per level.\nClick: spend a skill point on it",
        bota_core::rules::ATTRIBUTE_BONUS_MAX_LEVEL,
        bota_core::rules::ATTRIBUTE_BONUS_POINTS,
    )
}

fn buff_tip(shown: crate::buffs::Shown, rate: u32) -> Option<String> {
    bota_client_core::effect(shown.id).map(|face| {
        let mut lines = vec![face.name().to_string(), face.blurb()];
        if let Some(n) = shown.stacks.filter(|n| *n > 0) {
            lines.push(format!("{n} stacks"));
        }
        if let Some(t) = shown.ticks_left {
            lines.push(format!("{:.1} s left", t as f32 / rate as f32));
        }
        lines.join("\n")
    })
}

fn talent_tip(core: &bota_client_core::App, slot: u8) -> Option<String> {
    crate::hud::subject(core)
        .1
        .and_then(|u| u.hero)
        .and_then(|h| bota_client_core::talents(h.0))
        .map(|tree| {
            let face = &tree[usize::from(slot)];
            let (level, second) = bota_client_core::talent_levels(TalentSlot(slot));
            format!(
                "{}\n{}\nOpens at level {level}; the other of its tier at {second}.",
                face.name, face.blurb
            )
        })
}

/// An ability of the unit one commands, else of the unit one watches.
fn ability_tip(core: &bota_client_core::App, slot: u8, rate: u32) -> Option<String> {
    let unit = core
        .slot_unit()
        .or_else(|| core.ears().and_then(|id| core.known(id)).map(|(u, _)| u))?;
    let a = unit.abilities.get(usize::from(slot))?;
    let face = ability(a.id.0)?;
    let mut lines = vec![
        format!("{}  (level {}/{})", face.name, a.level, a.max_level),
        face.blurb(),
    ];
    if a.mana_cost > 0 && a.level > 0 {
        lines.push(format!("Mana {}", a.mana_cost));
    }
    if a.cooldown_left > 0 {
        lines.push(format!("Ready in {} s", a.cooldown_left.div_ceil(rate)));
    }
    if a.can_level {
        lines.push("Ctrl + its key: learn".to_string());
    }
    Some(lines.join("\n"))
}

fn item_tip(core: &bota_client_core::App, slot: u8, rate: u32) -> Option<String> {
    let i = core.item_in(slot)?;
    let face = item(i.id.0)?;
    let mut lines = vec![face.name.to_string(), face.stats(), face.blurb()];
    if i.cooldown_left > 0 {
        lines.push(format!("Ready in {} s", i.cooldown_left.div_ceil(rate)));
    }
    if slot >= bota_client_core::INVENTORY_SLOTS && slot != bota_client_core::TOWN_PORTAL_SLOT {
        lines.push("Drag it into the inventory to use it.".to_string());
    }
    Some(lines.join("\n"))
}

fn shop_tip(core: &bota_client_core::App, id: u16) -> Option<String> {
    item(id).map(|face| {
        let (now, whole) = core.price_of(id);
        let mut lines = vec![
            format!("{}  {} gold", face.name, now),
            face.stats(),
            face.blurb(),
        ];
        if now < whole {
            lines.push(format!("{} gold of it is already in hand.", whole - now));
        }
        lines.push("Click: buy   Right-click: quick-buy".to_string());
        lines.join("\n")
    })
}
