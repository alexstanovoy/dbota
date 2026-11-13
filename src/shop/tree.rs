//! The build tree at the panel's foot, as Dota draws it: the item chosen
//! in the middle, what it builds into along the top, its parts and recipe
//! along the foot, a line from it to each. Parts in hand are shaded.

use bevy::prelude::*;

use super::catalog::{Catalog, builds_into, parts_of};
use super::cell::{self, CELL, Chosen, Place};
use super::panel::INNER;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::Fonts;

/// Space between the tree's rows, when it has all three.
const ROW_GAP: f32 = 7.0;
/// Space above and below the tree.
const PAD: f32 = 6.0;
/// The tree's height: three rows of icons.
pub(crate) const HEIGHT: f32 = 3.0 * CELL.1 + 2.0 * ROW_GAP + 2.0 * PAD;
/// The lines' thickness and colour.
const LINE: f32 = 2.0;
const LINE_COLOR: Color = Color::srgba(0.74, 0.64, 0.26, 0.85);

/// Where the tree's icons and lines go.
#[derive(Component)]
pub(crate) struct Tree;

/// The shade over a part of the chosen item: shown while it is in hand. The
/// part, and how many of it come before it among the parts.
#[derive(Component)]
struct Owned {
    part: u16,
    nth: usize,
}

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (draw, owned).chain().run_if(super::match_or_tactical),
    );
}

/// The tree's band, empty until an item is chosen.
pub fn spawn(commands: &mut Commands, panel: Entity) {
    commands.spawn((
        Tree,
        Node {
            width: percent(100),
            height: px(HEIGHT),
            // The icons are placed by their offsets in a band of this height: a band squeezed by a
            // short panel would leave them hanging below it.
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::srgba(0.03, 0.035, 0.045, 0.98)),
        ChildOf(panel),
    ));
}

/// The tops of `n` rows of icons spread over the band, from its top padding to its foot's: one row
/// alone is in the middle.
fn rows(n: usize) -> Vec<f32> {
    if n == 1 {
        return vec![(HEIGHT - CELL.1) / 2.0];
    }
    let step = (HEIGHT - 2.0 * PAD - CELL.1) / (n - 1) as f32;
    (0..n).map(|k| PAD + k as f32 * step).collect()
}

/// The left edge of the `i`th of `n` icons spread across a row.
fn spread(i: usize, n: usize) -> f32 {
    let width = INNER + 2.0 * super::panel::PAD;
    let centre = (i as f32 + 0.5) / n as f32 * width;
    (centre - CELL.0 / 2.0).clamp(0.0, width - CELL.0)
}

/// Draws the tree anew when the chosen item changes.
fn draw(
    mut commands: Commands,
    chosen: Res<Chosen>,
    mut last: Local<Option<Option<u16>>>,
    (fonts, manifest, server): (Res<Fonts>, Res<Manifest>, Res<AssetServer>),
    catalog: Res<Catalog>,
    tree: Single<Entity, With<Tree>>,
) {
    if *last == Some(chosen.0) && !catalog.is_changed() {
        return;
    }
    *last = Some(chosen.0);
    commands.entity(*tree).despawn_children();
    let Some(id) = chosen.0 else {
        return;
    };
    let art = (&*fonts, &*manifest, &*server);
    let into = builds_into(id, catalog.offered);
    let parts = parts_of(id);
    // Only the rows the item has: what it builds into above it, its parts below it, spread over
    // the band from its top to its foot (as Dota draws a Divine Rapier, which builds into nothing,
    // at the band's top with its two parts at its foot), and one row alone in the middle.
    let rows = rows(usize::from(!into.is_empty()) + 1 + usize::from(!parts.is_empty()));
    let (into_row, item_row, parts_row) = match (into.is_empty(), parts.is_empty()) {
        (false, false) => (rows[0], rows[1], rows[2]),
        (false, true) => (rows[0], rows[1], 0.0),
        (true, false) => (0.0, rows[0], rows[1]),
        (true, true) => (0.0, rows[0], 0.0),
    };
    let middle = Vec2::new(spread(0, 1), item_row);
    let tops: Vec<Vec2> = (0..into.len())
        .map(|i| Vec2::new(spread(i, into.len()), into_row))
        .collect();
    let feet: Vec<Vec2> = (0..parts.len())
        .map(|i| Vec2::new(spread(i, parts.len()), parts_row))
        .collect();
    let half = Vec2::new(CELL.0, CELL.1) / 2.0;
    for at in tops.iter().chain(&feet) {
        line(&mut commands, *tree, middle + half, *at + half);
    }
    let place = |commands: &mut Commands, item: u16, at: Vec2| {
        let cell = cell::spawn(commands, art, *tree, item, Place::Tree, cell::CELL);
        commands.entity(cell).insert(Node {
            position_type: PositionType::Absolute,
            left: px(at.x),
            top: px(at.y),
            width: px(CELL.0),
            height: px(CELL.1),
            border: UiRect::all(px(1)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        });
        cell
    };
    place(&mut commands, id, middle);
    for (item, at) in into.iter().zip(&tops) {
        place(&mut commands, *item, *at);
    }
    for (i, (part, at)) in parts.iter().zip(&feet).enumerate() {
        let cell = place(&mut commands, part.0, *at);
        let nth = parts[..i].iter().filter(|p| **p == *part).count();
        commands.spawn((
            Owned { part: part.0, nth },
            crate::ui::fill(),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(cell),
        ));
    }
}

/// A line between two points of the tree, beneath its icons.
fn line(commands: &mut Commands, tree: Entity, from: Vec2, to: Vec2) {
    let span = to - from;
    let length = span.length();
    let centre = (from + to) / 2.0;
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(centre.x - length / 2.0),
            top: px(centre.y - LINE / 2.0),
            width: px(length),
            height: px(LINE),
            ..default()
        },
        UiTransform::from_rotation(Rot2::radians(span.y.atan2(span.x))),
        BackgroundColor(LINE_COLOR),
        Pickable::IGNORE,
        ChildOf(tree),
    ));
}

/// Shades the parts of the chosen item this seat holds, or has queued.
fn owned(
    session: NonSend<Session>,
    chosen: Res<Chosen>,
    mut shades: Query<(&Owned, &mut Visibility)>,
) {
    let Some(id) = chosen.0 else {
        return;
    };
    if shades.is_empty() {
        return;
    }
    let core = &session.0;
    let missing = core.missing_for(id);
    let parts = parts_of(id);
    for (owned, mut shown) in &mut shades {
        let of = |list: &[bota_proto::ItemId]| list.iter().filter(|p| p.0 == owned.part).count();
        let held = of(parts).saturating_sub(of(&missing));
        shown.set_if_neq(crate::ui::visible(owned.nth < held));
    }
}
