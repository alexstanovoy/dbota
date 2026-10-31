//! The buffs and debuffs of the unit the bottom panel is about, in a row
//! over its abilities as Dota draws them (`dota_hud_buff`): round icons in
//! full colour, the art of the ability or item each came from, framed in
//! black; buffs first in Dota's green ring, debuffs after in its red one; a
//! dark sweep clockwise over the share of the time gone; the stack count in
//! the middle. The hover popup tells what each does.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::prelude::*;
use bota_client_core::effect_listed;
use bota_proto::{EffectId, Inflictor};

use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// Chips in the row at most.
const CHIPS: usize = 12;
/// The hero panel's pixels to one of Dota's (`hud.rs`: its portrait is
/// Dota's 159 wide).
const PANEL_PER_DOTA: f32 = crate::hud::PORTRAIT.0 / 159.0;
/// A chip's side (`DOTABuff`), the room either side of it, and the icon's
/// inset in it (`#BuffImage`), in interface pixels.
const CHIP: f32 = 40.0 * PANEL_PER_DOTA;
const CHIP_SPACE: f32 = 2.0 * PANEL_PER_DOTA;
const ICON_INSET: f32 = 2.0 * PANEL_PER_DOTA;
/// The stack count's size (`#StackCount`).
const STACKS_SIZE: f32 = 20.0 * PANEL_PER_DOTA;
/// The ring's wash, a buff's and a debuff's (`#CircularDuration`).
const BUFF_RIM: Color = Color::srgb(0.545, 0.867, 0.31);
const DEBUFF_RIM: Color = Color::srgb(0.894, 0.235, 0.063);
/// The darkness over the time gone, deep enough to read on red and dark
/// art: a gradient's stop darkens less than a flat colour of its alpha, and
/// 0.93 here reads as about 0.7 would.
const SWEEP: Color = Color::srgba(0.0, 0.0, 0.0, 0.93);

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
    /// What `ticks_left` counts down from.
    pub ticks_total: Option<u32>,
    pub stacks: Option<u32>,
    /// The ability or item it came from, whose art it wears.
    pub inflictor: Option<Inflictor>,
}

#[derive(Component)]
struct Chips(Vec<Entity>);

/// A chip's parts: its icon, the sweep over it, its stack count, its ring.
#[derive(Component, Clone, Copy)]
struct ChipParts {
    icon: Entity,
    sweep: Entity,
    count: Entity,
    ring: Entity,
}

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        update.run_if(|k: Res<crate::session::MatchKnown>| k.0),
    );
}

/// Lays the row in `parent`, the ability column, over the abilities and
/// inside the panel: in the flow, so it never reaches the panel's edge.
pub fn spawn_row(
    commands: &mut Commands,
    fonts: &Fonts,
    art: (&Manifest, &AssetServer),
    parent: Entity,
) {
    let row = commands
        .spawn((Node::default(), Pickable::IGNORE, ChildOf(parent)))
        .id();
    let chips = (0..CHIPS)
        .map(|_| spawn_chip(commands, fonts, art, row))
        .collect();
    commands.entity(row).insert(Chips(chips));
}

/// One chip: the icon framed in black, the sweep over it, the stack count
/// in its middle, Dota's ring round it.
fn spawn_chip(
    commands: &mut Commands,
    fonts: &Fonts,
    (manifest, server): (&Manifest, &AssetServer),
    row: Entity,
) -> Entity {
    let chip = commands
        .spawn((
            BuffChip::default(),
            Button,
            Node {
                width: px(CHIP),
                height: px(CHIP),
                margin: UiRect::horizontal(px(CHIP_SPACE)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), px(0), px(0), px(0), px(4)),
            Visibility::Hidden,
            ChildOf(row),
        ))
        .id();
    let (icon, sweep) = spawn_icon(commands, chip);
    let count = spawn_count(commands, fonts, chip);
    let ring = commands
        .spawn((ring(manifest, server), Pickable::IGNORE, ChildOf(chip)))
        .id();
    commands.entity(chip).insert(ChipParts {
        icon,
        sweep,
        count,
        ring,
    });
    chip
}

/// A chip's icon framed in black, and the sweep over it.
fn spawn_icon(commands: &mut Commands, chip: Entity) -> (Entity, Entity) {
    let inset = || Node {
        position_type: PositionType::Absolute,
        left: px(ICON_INSET),
        top: px(ICON_INSET),
        right: px(ICON_INSET),
        bottom: px(ICON_INSET),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::MAX,
        ..default()
    };
    let icon = commands
        .spawn((
            inset(),
            ImageNode::default(),
            BorderColor::all(Color::BLACK),
            BackgroundColor(ui::PANEL_DEEP),
            Pickable::IGNORE,
            ChildOf(chip),
        ))
        .id();
    // Inside the icon, so it is drawn over the picture.
    let shade = commands
        .spawn((
            Node {
                border_radius: BorderRadius::MAX,
                ..ui::fill()
            },
            sweep(1.0),
            Pickable::IGNORE,
            ChildOf(icon),
        ))
        .id();
    (icon, shade)
}

/// A chip's stack count, in its middle.
fn spawn_count(commands: &mut Commands, fonts: &Fonts, chip: Entity) -> Entity {
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
    commands
        .spawn((
            ui::text("", &fonts.bold, STACKS_SIZE, Color::WHITE),
            TextShadow {
                offset: Vec2::ZERO,
                color: Color::BLACK,
            },
            Pickable::IGNORE,
            ChildOf(middle),
        ))
        .id()
}

/// Dota's buff ring at 95% of the chip, washed later; a plain round
/// border where the cache has none.
fn ring(manifest: &Manifest, server: &AssetServer) -> impl Bundle {
    let node = |border: f32| Node {
        width: percent(95),
        height: percent(95),
        border: UiRect::all(px(border)),
        border_radius: BorderRadius::MAX,
        ..default()
    };
    match manifest.file("hud.buff_outline") {
        Some(path) => (
            node(0.0),
            ImageNode::new(server.load(path)),
            BorderColor::all(Color::NONE),
        ),
        None => (
            node(2.0),
            ImageNode::default().with_color(Color::NONE),
            BorderColor::all(BUFF_RIM),
        ),
    }
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

/// The dark part of a chip for the share of its time gone, clockwise from
/// twelve o'clock.
fn sweep(left: f32) -> BackgroundGradient {
    ui::split_disc((1.0 - left.clamp(0.0, 1.0)) * TAU, SWEEP, Color::NONE)
}

/// The parts of the chips the update dresses.
#[derive(bevy::ecs::system::SystemParam)]
struct Faces<'w, 's> {
    manifest: Res<'w, Manifest>,
    server: Res<'w, AssetServer>,
    images: ResMut<'w, Assets<Image>>,
    icons: Query<'w, 's, &'static mut ImageNode>,
    borders: Query<'w, 's, &'static mut BorderColor>,
    sweeps: Query<'w, 's, &'static mut BackgroundGradient>,
    texts: Query<'w, 's, &'static mut Text>,
    art: Local<'s, HashMap<(u16, Option<Inflictor>), Art>>,
}

fn update(
    session: NonSend<Session>,
    rows: Query<&Chips>,
    mut chips: Query<(&mut BuffChip, &mut Visibility, &ChipParts)>,
    mut faces: Faces,
) {
    let core = &session.0;
    let unit = crate::hud::subject(core).1;
    let mut shown: Vec<Shown> = unit
        .map(|u| {
            u.effects
                .iter()
                .filter(|e| effect_listed(e.id.0))
                .map(|e| Shown {
                    id: e.id.0,
                    ticks_left: e.ticks_left,
                    ticks_total: e.ticks_total,
                    stacks: e.stacks,
                    inflictor: e.inflictor,
                })
                .collect()
        })
        .unwrap_or_default();
    // Buffs first, debuffs after, as Dota lays them.
    shown.sort_by_key(|s| harmful(s.id));
    let Ok(row) = rows.single() else {
        return;
    };
    for (i, entity) in row.0.iter().enumerate() {
        let Ok((mut chip, mut visibility, parts)) = chips.get_mut(*entity) else {
            continue;
        };
        let now = shown.get(i).copied();
        visibility.set_if_neq(ui::visible(now.is_some()));
        if chip.shown != now {
            chip.shown = now;
        }
        if let Some(s) = now {
            dress(&mut faces, *parts, s);
        }
    }
}

/// Dresses a chip's parts for what it shows.
fn dress(faces: &mut Faces, parts: ChipParts, s: Shown) {
    let rim = if harmful(s.id) { DEBUFF_RIM } else { BUFF_RIM };
    wash_ring(parts.ring, rim, &mut faces.icons, &mut faces.borders);
    let handle = icon_art(faces, s);
    if let Ok(mut image) = faces.icons.get_mut(parts.icon)
        && image.image != handle
    {
        image.image = handle;
    }
    let left = match (s.ticks_left, s.ticks_total) {
        (Some(t), Some(whole)) => t as f32 / whole.max(1) as f32,
        _ => 1.0,
    };
    if let Ok(mut g) = faces.sweeps.get_mut(parts.sweep) {
        g.set_if_neq(sweep(left));
    }
    if let Ok(mut text) = faces.texts.get_mut(parts.count) {
        let count = s
            .stacks
            .filter(|n| *n > 0)
            .map(|n| n.to_string())
            .unwrap_or_default();
        text.set_if_neq(Text(count));
    }
}

/// As Dota's status icons: the art of the ability or item the effect came
/// from, the effect's own for one from neither, cut to the circle once it
/// has loaded (nothing square inside the ring).
fn icon_art(faces: &mut Faces, s: Shown) -> Handle<Image> {
    let (manifest, server) = (&faces.manifest, &faces.server);
    let art = faces.art.entry((s.id, s.inflictor)).or_insert_with(|| Art {
        source: s
            .inflictor
            .and_then(|by| match by {
                Inflictor::Ability(id) => manifest.ability_icon(id.0),
                Inflictor::Item(id) => manifest.item_icon(id.0),
            })
            .or_else(|| manifest.effect_icon(s.id))
            .map(|p| server.load(p))
            .unwrap_or_default(),
        round: None,
    });
    if art.round.is_none()
        && let Some(round) = faces.images.get(&art.source).and_then(round_cut)
    {
        art.round = Some(faces.images.add(round));
    }
    art.round.clone().unwrap_or_default()
}

/// Washes a chip's ring a buff's or a debuff's colour: Dota's ring art, or
/// the plain border where it has none.
fn wash_ring(
    ring: Entity,
    rim: Color,
    icons: &mut Query<&mut ImageNode>,
    borders: &mut Query<&mut BorderColor>,
) {
    if let Ok(mut image) = icons.get_mut(ring)
        && image.color != Color::NONE
    {
        if image.color != rim {
            image.color = rim;
        }
    } else if let Ok(mut border) = borders.get_mut(ring) {
        border.set_if_neq(BorderColor::all(rim));
    }
}

/// Whether an effect hurts whoever wears it.
fn harmful(id: u16) -> bool {
    bota_core::effect_def(EffectId(id)).is_some_and(|def| def.harmful)
}
