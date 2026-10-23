//! An item's icon in the shop, as Dota draws one: the picture in a thin
//! bronze frame, what the side's shop has left of it in the corner, and the
//! wait for the next one over a darkened picture when none is left.

use bevy::prelude::*;
use bota_core::item_def;
use bota_proto::ItemId;

use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// The size of an item icon in the shop, Dota's 88 by 64 scaled.
pub const CELL: (f32, f32) = (42.0, 31.0);
/// Space between icons.
pub const GAP: f32 = 4.0;

/// The frame of an icon: as it stands, under the cursor, chosen.
const EDGE: Color = Color::srgb(0.42, 0.34, 0.16);
const EDGE_HOVER: Color = Color::srgb(0.86, 0.72, 0.36);
const EDGE_CHOSEN: Color = Color::srgb(0.92, 0.93, 0.95);

/// An item's icon in the shop, by item id: a click on it buys it.
#[derive(Component, Clone, Copy)]
pub struct ShopCell(pub u16);

/// Where an icon sits, for what a click on it does besides buying.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// A tab's grid or the search results.
    Grid,
    /// The build tree: a click shows its own tree.
    Tree,
    /// The quick-buy row: a right click empties it.
    Quick,
}

/// What the side's shop has left of an item, in an icon's corner.
#[derive(Component)]
struct Stock(u16);

/// The shade over an icon whose item the side's shop has none left of.
#[derive(Component)]
struct Restock(u16);

/// The seconds until the side's shop has one more, over the shade.
#[derive(Component)]
struct RestockSeconds(u16);

/// Dota's art for a part of the shop, under what the part holds, tinted,
/// in place of its plain colour; the plain colour stands in without the
/// cache.
pub fn backdrop(
    commands: &mut Commands,
    art: (&Manifest, &AssetServer),
    at: Entity,
    logical: &str,
    tint: Color,
) {
    if let Some(path) = art.0.file(logical) {
        commands.entity(at).insert((
            ImageNode::new(art.1.load(path))
                .with_color(tint)
                .with_mode(NodeImageMode::Stretch),
            BackgroundColor(Color::NONE),
        ));
    }
}

/// The item whose build tree the shop shows, if any.
#[derive(Resource, Default)]
pub struct Chosen(pub Option<u16>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Chosen>()
        .add_systems(Update, (stock, edges));
}

/// Spawns an item's icon under `parent`, with its corner count and shade.
pub fn spawn(
    commands: &mut Commands,
    art: (&Fonts, &Manifest, &AssetServer),
    parent: Entity,
    id: u16,
    place: Place,
) -> Entity {
    let (fonts, manifest, server) = art;
    let cell = commands
        .spawn((
            ShopCell(id),
            place,
            Button,
            Node {
                width: px(CELL.0),
                height: px(CELL.1),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.06, 0.07, 0.09)),
            BorderColor::all(EDGE),
            ChildOf(parent),
        ))
        .id();
    match manifest.item_icon(id) {
        Some(path) => {
            commands.spawn((
                ImageNode::new(server.load(path)),
                ui::fill(),
                Pickable::IGNORE,
                ChildOf(cell),
            ));
        }
        // Without the cache: the simulator's short name on the dark ground.
        None => {
            let name = bota_client_core::ITEMS
                .iter()
                .find(|face| face.id == id)
                .map_or("?", |face| face.name);
            commands.spawn((
                ui::text(name, &fonts.regular, 9.0, ui::INK_DIM),
                Pickable::IGNORE,
                ChildOf(cell),
            ));
        }
    }
    if item_def(ItemId(id)).is_some_and(|def| def.stock.is_some()) {
        spawn_stock(commands, fonts, cell, id);
    }
    cell
}

/// The corner count and the empty shop's shade of an item sold from a
/// limited stock.
fn spawn_stock(commands: &mut Commands, fonts: &Fonts, cell: Entity, id: u16) {
    let shade = commands
        .spawn((
            Restock(id),
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..ui::fill()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(cell),
        ))
        .id();
    commands.spawn((
        RestockSeconds(id),
        ui::text("", &fonts.bold, 12.0, ui::INK),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(shade),
    ));
    commands.spawn((
        Stock(id),
        Node {
            position_type: PositionType::Absolute,
            right: px(2),
            bottom: px(0),
            ..default()
        },
        ui::text("", &fonts.bold, 12.0, ui::INK),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(cell),
    ));
}

/// What the watched side's shop has left of each limited item, and the wait
/// for the next where it has none.
fn stock(
    session: NonSend<Session>,
    mut counts: Query<(&Stock, &mut Text), Without<RestockSeconds>>,
    mut shades: Query<(&Restock, &mut Visibility)>,
    mut seconds: Query<(&RestockSeconds, &mut Text), Without<Stock>>,
) {
    let core = &session.0;
    let rate = u32::from(core.tick_rate().max(1));
    for (Stock(id), mut text) in &mut counts {
        let left = core.stock_of(*id).map(|s| s.left);
        let shown = left
            .filter(|n| *n > 0)
            .map_or(String::new(), |n| n.to_string());
        text.set_if_neq(Text(shown));
    }
    for (Restock(id), mut shown) in &mut shades {
        let empty = core.stock_of(*id).is_some_and(|s| s.left == 0);
        shown.set_if_neq(ui::visible(empty));
    }
    for (RestockSeconds(id), mut text) in &mut seconds {
        let wait = core
            .stock_of(*id)
            .filter(|s| s.left == 0)
            .map_or(String::new(), |s| s.restock_left.div_ceil(rate).to_string());
        text.set_if_neq(Text(wait));
    }
}

/// The frame of each icon: brighter under the cursor, white for the item
/// whose tree is shown.
fn edges(
    chosen: Res<Chosen>,
    mut cells: Query<(&ShopCell, &Place, &Interaction, &mut BorderColor)>,
) {
    for (cell, place, interaction, mut border) in &mut cells {
        let edge = if *interaction != Interaction::None {
            EDGE_HOVER
        } else if chosen.0 == Some(cell.0) && *place != Place::Tree {
            EDGE_CHOSEN
        } else {
            EDGE
        };
        border.set_if_neq(BorderColor::all(edge));
    }
}
