//! The buffs and debuffs of the unit the bottom panel is about, in a row
//! over its abilities as Dota draws them: round icons, buffs first with a
//! green rim, debuffs after with a red one, a dark sweep for the time gone
//! and the stack count in the corner. The hover popup tells what each does.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::prelude::*;
use bota_client_core::effect;
use bota_proto::{EffectId, EntityId};

use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// Chips in the row at most.
const CHIPS: usize = 12;
/// A chip's side in interface pixels.
const CHIP: f32 = 30.0;
/// Effects never listed: the rot shows on its ability.
const UNLISTED: [u16; 1] = [16];

const BUFF_RIM: Color = Color::srgb(0.35, 0.8, 0.4);
const DEBUFF_RIM: Color = Color::srgb(0.9, 0.25, 0.2);

/// One chip of the row and the effect it shows, for the hover popup.
#[derive(Component, Default)]
pub struct BuffChip {
    pub shown: Option<Shown>,
}

/// What a chip shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub id: u16,
    pub ticks_left: Option<u32>,
    pub stacks: Option<u32>,
}

#[derive(Component)]
struct Chips(Vec<Entity>);

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        update.run_if(|k: Res<crate::session::MatchKnown>| k.0),
    );
}

/// Lays the row in `parent`, the ability column, over the abilities and
/// inside the panel: in the flow, so it never reaches the panel's edge.
pub fn spawn_row(commands: &mut Commands, fonts: &Fonts, parent: Entity) {
    let row = commands
        .spawn((
            Node {
                column_gap: px(4),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    let mut chips = Vec::with_capacity(CHIPS);
    for _ in 0..CHIPS {
        let chip = commands
            .spawn((
                BuffChip::default(),
                Button,
                Node {
                    width: px(CHIP),
                    height: px(CHIP),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(CHIP / 2.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BorderColor::all(BUFF_RIM),
                BackgroundColor(ui::PANEL_DEEP),
                Visibility::Hidden,
                ChildOf(row),
            ))
            .id();
        let full = || Node {
            border_radius: BorderRadius::MAX,
            ..ui::fill()
        };
        commands.spawn((
            full(),
            ImageNode::default(),
            Pickable::IGNORE,
            ChildOf(chip),
        ));
        commands.spawn((full(), sweep(1.0), Pickable::IGNORE, ChildOf(chip)));
        // The count in the middle of the circle: a centring box, the text
        // in it nudged down a pixel to sit on the digits' visual middle
        // (the font's ascent is taller than its descent).
        let middle = commands
            .spawn((
                Node {
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..ui::fill()
                },
                Pickable::IGNORE,
                ChildOf(chip),
            ))
            .id();
        commands.spawn((
            ui::text("", &fonts.bold, 13.0, ui::INK),
            ui::shadow(),
            Node {
                top: px(1),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(middle),
        ));
        chips.push(chip);
    }
    commands.entity(row).insert(Chips(chips));
}

/// A status icon: its square art and the same cut to a circle.
struct Art {
    source: Handle<Image>,
    round: Option<Handle<Image>>,
}

/// An RGBA8 image with everything outside its inscribed circle made
/// transparent, the edge softened over a pixel.
pub fn round_cut(image: &Image) -> Option<Image> {
    let mut out = image.clone();
    let format = out.texture_descriptor.format;
    if format.block_copy_size(None) != Some(4) {
        return None;
    }
    let (w, h) = (out.width() as usize, out.height() as usize);
    let data = out.data.as_mut()?;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let r = cx.min(cy);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let keep = (r - d).clamp(0.0, 1.0);
            let a = &mut data[(y * w + x) * 4 + 3];
            *a = (f32::from(*a) * keep) as u8;
        }
    }
    Some(out)
}

/// The dark part of a chip for the share of its time gone.
fn sweep(left: f32) -> BackgroundGradient {
    let dark = Color::srgba(0.0, 0.0, 0.0, 0.6);
    ui::split_disc((1.0 - left.clamp(0.0, 1.0)) * TAU, dark, Color::NONE)
}

#[allow(clippy::type_complexity)]
fn update(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    rows: Query<&Chips>,
    mut chips: Query<(&mut BuffChip, &mut Visibility, &mut BorderColor, &Children)>,
    mut icons: Query<&mut ImageNode>,
    mut sweeps: Query<&mut BackgroundGradient>,
    mut texts: Query<&mut Text>,
    kids: Query<&Children, Without<BuffChip>>,
    // The longest time left seen of each effect on each unit: its whole
    // length, for the sweep.
    mut lengths: Local<HashMap<(EntityId, u16), u32>>,
    mut art: Local<HashMap<u16, Art>>,
    mut images: ResMut<Assets<Image>>,
) {
    let core = &session.0;
    let unit = crate::hud::subject(core).1;
    let mut shown: Vec<Shown> = unit
        .map(|u| {
            u.effects
                .iter()
                .filter(|e| !UNLISTED.contains(&e.id.0) && effect(e.id.0).is_some())
                .map(|e| Shown {
                    id: e.id.0,
                    ticks_left: e.ticks_left,
                    stacks: e.stacks,
                })
                .collect()
        })
        .unwrap_or_default();
    // Buffs first, debuffs after, as Dota lays them.
    shown.sort_by_key(|s| harmful(s.id));
    if lengths.len() > 512 {
        lengths.clear();
    }
    let Ok(row) = rows.single() else {
        return;
    };
    for (i, entity) in row.0.iter().enumerate() {
        let Ok((mut chip, mut visibility, mut rim, children)) = chips.get_mut(*entity) else {
            continue;
        };
        let now = shown.get(i).copied();
        visibility.set_if_neq(ui::visible(now.is_some()));
        if chip.shown != now {
            chip.shown = now;
        }
        let (Some(s), Some(u)) = (now, unit) else {
            continue;
        };
        rim.set_if_neq(BorderColor::all(if harmful(s.id) {
            DEBUFF_RIM
        } else {
            BUFF_RIM
        }));
        if let Some(mut image) = children.first().and_then(|c| icons.get_mut(*c).ok()) {
            let art = art.entry(s.id).or_insert_with(|| Art {
                source: manifest
                    .effect_icon(s.id)
                    .map(|p| server.load(p))
                    .unwrap_or_default(),
                round: None,
            });
            // The icon cut to the circle once it has loaded, as Dota's
            // status icons are: nothing square inside the rim.
            if art.round.is_none()
                && let Some(round) = images.get(&art.source).and_then(round_cut)
            {
                art.round = Some(images.add(round));
            }
            let handle = art.round.clone().unwrap_or_default();
            if image.image != handle {
                image.image = handle;
            }
        }
        let left = match s.ticks_left {
            Some(t) => {
                let whole = lengths.entry((u.id, s.id)).or_insert(t);
                if t > *whole {
                    *whole = t;
                }
                t as f32 / (*whole).max(1) as f32
            }
            None => 1.0,
        };
        if let Some(mut g) = children.get(1).and_then(|c| sweeps.get_mut(*c).ok()) {
            g.set_if_neq(sweep(left));
        }
        let count = children
            .get(2)
            .and_then(|c| kids.get(*c).ok())
            .and_then(|k| k.first().copied());
        if let Some(mut text) = count.and_then(|c| texts.get_mut(c).ok()) {
            let s = s
                .stacks
                .filter(|n| *n > 0)
                .map(|n| n.to_string())
                .unwrap_or_default();
            text.set_if_neq(Text(s));
        }
    }
}

/// Whether an effect hurts whoever wears it.
fn harmful(id: u16) -> bool {
    bota_core::effect_def(EffectId(id)).is_some_and(|def| def.harmful)
}
