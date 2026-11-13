//! An item's icon in the shop, as Dota draws one: the picture in a thin
//! frame that is gold for what the seat can buy now, what the side's shop has left of it in the corner, and the
//! wait for the next one over a darkened picture when none is left.

use bevy::prelude::*;
use bevy::{picking::hover::Hovered, ui_widgets::Button};
use bota_client_core::PurchaseAvailability;
use bota_core::item_def;
use bota_proto::ItemId;

use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// The size of an item icon in the shop, Dota's 88 by 64 scaled.
pub const CELL: (f32, f32) = (42.0, 31.0);
/// Space between icons.
pub const GAP: f32 = 4.0;

/// The frame of an icon: one the seat cannot buy, under the cursor, chosen.
const EDGE_UNAVAILABLE: Color = Color::srgb(0.18, 0.18, 0.18);
const EDGE_HOVER: Color = Color::srgb(0.86, 0.72, 0.36);
const EDGE_CHOSEN: Color = Color::srgb(0.92, 0.93, 0.95);

/// An item's icon in the shop, by item id: a right click on it buys it.
#[derive(Component, Clone, Copy)]
#[require(bevy::picking::hover::DirectlyHovered)]
pub struct ShopCell(pub u16);

/// A secret-shop component which is out of reach of both physical carriers.
#[derive(Component)]
pub struct ShopAccessShade;

/// A search result's row, and the frame of the icon in it.
#[derive(Component)]
pub struct Row;
#[derive(Component)]
struct IconFrame;

/// A row's height, its side padding and the space between its parts, in interface pixels; the
/// reference game's rows are 39 high at a 1080-line window.
const ROW_HEIGHT: f32 = 39.0;
const ROW_PAD: f32 = 6.0;
const ROW_GAP: f32 = 12.0;
const ROW_INK: Color = Color::srgb(0.69, 0.73, 0.78);
const ROW_HOVER: Color = Color::srgba(0.56, 0.64, 0.74, 0.35);

/// Where an icon sits.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// A tab's grid or the search results.
    Grid,
    /// The build tree.
    Tree,
    /// The quick-buy row.
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
        .add_systems(
            First,
            crate::loading::reset::<Chosen>.in_set(crate::loading::Begin),
        )
        .add_systems(Update, (stock, edges, shop_access));
}

/// Spawns an item's icon of `size` under `parent`, with its corner count
/// and shade.
pub fn spawn(
    commands: &mut Commands,
    art: (&Fonts, &Manifest, &AssetServer),
    parent: Entity,
    id: u16,
    place: Place,
    (width, height): (f32, f32),
) -> Entity {
    let cell = commands
        .spawn((
            ShopCell(id),
            place,
            Button,
            Hovered::default(),
            Node {
                width: px(width),
                height: px(height),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.06, 0.07, 0.09)),
            BorderColor::all(EDGE_UNAVAILABLE),
            ChildOf(parent),
        ))
        .id();
    decorate(commands, art, cell, id);
    cell
}

/// The picture of an item in the frame `cell`, with its corner count and the shades of an empty
/// shelf and of a secret shop out of reach.
fn decorate(
    commands: &mut Commands,
    art: (&Fonts, &Manifest, &AssetServer),
    cell: Entity,
    id: u16,
) {
    let (fonts, manifest, server) = art;
    if let Some(path) = manifest.item_icon(id) {
        commands.spawn((
            ImageNode::new(server.load(path)),
            ui::fill(),
            Pickable::IGNORE,
            ChildOf(cell),
        ));
    }
    if item_def(ItemId(id)).is_some_and(|def| def.stock.is_some()) {
        spawn_stock(commands, fonts, cell, id);
    }
    if item_def(ItemId(id)).is_some_and(|def| def.secret_shop) {
        commands.spawn((
            ShopAccessShade,
            ui::fill(),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(cell),
        ));
    }
}

/// A search result as Dota lists it: a row of the panel's width with the item's icon, its name in
/// capitals and, at the right, its price over a coin. The row is the item's [`ShopCell`], so the
/// clicks of an icon are the row's; the frame is the icon's.
pub fn spawn_row(
    commands: &mut Commands,
    art: (&Fonts, &Manifest, &AssetServer),
    parent: Entity,
    id: u16,
    name: &str,
    price: i32,
) -> Entity {
    let (fonts, manifest, server) = art;
    let row = commands
        .spawn((
            ShopCell(id),
            Place::Grid,
            Row,
            Button,
            Hovered::default(),
            Node {
                width: percent(100),
                height: px(ROW_HEIGHT),
                padding: UiRect::horizontal(px(ROW_PAD)),
                column_gap: px(ROW_GAP),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
            ChildOf(parent),
        ))
        .id();
    let icon = commands
        .spawn((
            IconFrame,
            Node {
                width: px(CELL.0),
                height: px(CELL.1),
                border: UiRect::all(px(1)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgb(0.06, 0.07, 0.09)),
            BorderColor::all(EDGE_UNAVAILABLE),
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    decorate(commands, art, icon, id);
    commands.spawn((
        ui::text(name.to_uppercase(), &fonts.regular, 15.0, ROW_INK),
        Pickable::IGNORE,
        ChildOf(row),
    ));
    let cost = commands
        .spawn((
            Node {
                margin: UiRect::left(Val::Auto),
                column_gap: px(4),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    commands.spawn((
        ui::text(price.to_string(), &fonts.bold, 15.0, ui::GOLDEN),
        Pickable::IGNORE,
        ChildOf(cost),
    ));
    if let Some(path) = manifest.file("hud.gold") {
        commands.spawn((
            ImageNode::new(server.load(path)),
            Node {
                width: px(16),
                height: px(16),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(cost),
        ));
    }
    row
}

fn shop_access(
    session: NonSend<Session>,
    mut shades: Query<&mut Visibility, With<ShopAccessShade>>,
) {
    let shown = ui::visible(!session.0.secret_shop_accessible());
    for mut shade in &mut shades {
        shade.set_if_neq(shown);
    }
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

/// Which items the seat can buy right now, worked out once for each view and queue the
/// session shows rather than once for each of the hundreds of icons drawn.
#[derive(Default)]
struct Buyable {
    stamp: Option<(u32, usize, i32, bool)>,
    items: Vec<(u16, bool)>,
}

impl Buyable {
    /// Whether the seat could buy `item` now, by the one prediction the client core makes.
    fn can_buy(&mut self, core: &bota_client_core::App, item: u16) -> bool {
        let stamp = (
            core.view().map_or(0, |view| view.tick),
            core.queued_buys.len(),
            core.spendable_gold(),
            core.in_tactical_phase(),
        );
        if self.stamp != Some(stamp) {
            self.stamp = Some(stamp);
            self.items.clear();
        }
        if let Some((_, buyable)) = self.items.iter().find(|(id, _)| *id == item) {
            return *buyable;
        }
        let buyable = core.purchase_availability(item) == PurchaseAvailability::Available;
        self.items.push((item, buyable));
        buyable
    }
}

/// The frame of each icon: brighter under the cursor, white for the item
/// whose tree is shown, and gold only for an item the seat can buy now (Dota's
/// `CanPurchase` border), plain for the rest.
fn edges(
    session: NonSend<Session>,
    chosen: Res<Chosen>,
    mut buyable: Local<Buyable>,
    mut cells: Query<(
        &ShopCell,
        &Place,
        &Hovered,
        &mut BorderColor,
        Option<&Children>,
        Option<&Row>,
        &mut BackgroundColor,
    )>,
    mut frames: Query<&mut BorderColor, (With<IconFrame>, Without<ShopCell>)>,
) {
    for (cell, place, interaction, mut border, children, row, mut ground) in &mut cells {
        let edge = if interaction.get() {
            EDGE_HOVER
        } else if chosen.0 == Some(cell.0) && *place != Place::Tree {
            EDGE_CHOSEN
        } else if buyable.can_buy(&session.0, cell.0) {
            ui::GOLDEN
        } else {
            EDGE_UNAVAILABLE
        };
        border.set_if_neq(BorderColor::all(edge));
        if row.is_some() {
            ground.set_if_neq(BackgroundColor(if interaction.get() {
                ROW_HOVER
            } else {
                Color::NONE
            }));
            for child in children.into_iter().flatten() {
                if let Ok(mut frame) = frames.get_mut(*child) {
                    frame.set_if_neq(BorderColor::all(edge));
                }
            }
        }
    }
}
