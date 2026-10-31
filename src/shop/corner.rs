//! The shop's corner at the bottom right, laid out as Dota's
//! `shop_launcher_block` (`hud_reborn`, `dota_hud_quick_buy`,
//! `dota_hud_stash`): the gold and courier block along the foot, with the
//! shop button holding the gold and the courier's buttons right of it; the
//! quick-buy rows over it with the item kept beside them; the stash over
//! those, with its GRAB ALL tab.

use std::collections::VecDeque;

use bevy::prelude::*;
use bota_client_core::{BAG_SLOTS, Source, TOWN_PORTAL_SLOT};
use bota_proto::{ItemId, ItemSlot, Order};

use super::catalog::Catalog;
use super::cell::{self, Place};
use crate::hud::{Blocker, Docks, ItemBox};
use crate::input::Panels;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, DOTA, Fonts, dota};

/// The stash's slots.
const STASH_SLOTS: u8 = bota_core::rules::STASH_SLOTS as u8;
/// The gold and courier block's backdrop (`#shop_launcher_bg`), in Dota's
/// pixels.
const LAUNCHER: (f32, f32) = (309.0, 60.0);
/// The width of the quick-buy rows and of the controls under them
/// (`#quickbuy`).
const QUICK_WIDTH: f32 = 292.0;
/// The quick-buy rows' least height: one row (`#QuickBuyRows`).
const QUICK_HEIGHT: f32 = 44.0;
/// A quick-buy slot (`.Reborn DOTAShopItem`) and the space left of it.
const QUICK_ITEM: (f32, f32) = (38.0, 28.0);
const QUICK_SPACE: f32 = 3.0;
/// Quick-buy slots in a row, and the rows Dota has.
const QUICK_COLUMNS: usize = 4;
const QUICK_SLOTS: usize = 2 * QUICK_COLUMNS;
/// The width of the kept item's column (`#StickyItemSlotContainer`).
const STICKY_WIDTH: f32 = 53.0;
/// The shop button (`#ShopButton`) and its place in the block.
const SHOP_BUTTON: (f32, f32) = (100.0, 36.0);
const SHOP_BUTTON_LEFT: f32 = 8.0;
/// The gold's size and its room from the button's right edge, the coin's
/// size and its room (`.ShopButtonValueLabel`, `#GoldIcon`).
const GOLD_FONT: f32 = 22.0;
const GOLD_RIGHT: f32 = 40.0;
const COIN: f32 = 26.0;
const COIN_RIGHT: f32 = 8.0;
/// Where the courier's buttons begin in the block (`#courier`).
const COURIER_LEFT: f32 = 114.0;
/// The stash's backdrop (`.Reborn #stash_bg`), a slot (`.stash_cell_bg`),
/// the row's place in it (`.Reborn #stash_row`).
const STASH_BG: (f32, f32) = (291.0, 67.0);
const STASH_BOX: (f32, f32) = (40.0, 31.0);
const STASH_ROW: (f32, f32) = (16.0, 31.0);
/// GRAB ALL's tab, drawn at nine tenths (`.Reborn #grab_all_button`), and
/// the room from its right edge to the stash's.
const GRAB: (f32, f32) = (0.9 * 102.0, 0.9 * 26.0);
const GRAB_RIGHT: f32 = 229.0;
/// Snapshots between two of GRAB ALL's moves: a seat's orders due on one
/// tick replace each other, so they go a tick apart and a little more.
const GRAB_SPACING: u32 = 2;
/// The gold's yellow, white within reach of a shop; the stash title's grey;
/// GRAB ALL's ink; the quick-buy hint's.
const GOLD_INK: Color = Color::srgb(1.0, 0.8, 0.2);
const STASH_INK: Color = Color::srgb(0.553, 0.631, 0.694);
const GRAB_INK: Color = Color::srgb(0.133, 0.169, 0.196);
const HINT_INK: Color = Color::srgba(1.0, 1.0, 1.0, 0.68);
/// The quick-buy rows' and the kept item's grounds.
const QUICK_GROUND: Color = Color::srgba(0.145, 0.149, 0.153, 0.675);
const STICKY_GROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.435);
/// The item kept beside the quick-buy row, as Dota keeps it by default.
pub const STICKY: ItemId = ItemId::TOWN_PORTAL_SCROLL;

/// The shop button: it holds the gold, and a click on it opens the shop.
#[derive(Component)]
pub struct ShopButton;

/// The gold's figure.
#[derive(Component)]
struct Gold;

/// The stash's block, its GRAB ALL tab.
#[derive(Component)]
struct StashBlock;
#[derive(Component)]
pub struct GrabAll;

/// Where the quick-buy rows' icons go.
#[derive(Component)]
struct QuickRow;

/// The quick-buy rows' hint while they are empty.
#[derive(Component)]
struct QuickHint;

/// GRAB ALL's moves still to send: stash slot, bag slot.
#[derive(Resource, Default)]
pub struct Grab {
    moves: VecDeque<(u8, u8)>,
    /// The snapshot tick the last move went out on.
    sent: Option<u32>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Grab>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (gold, shop_button, stash, quick_row, grab_step).run_if(super::match_or_tactical),
        );
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    catalog: Res<Catalog>,
    docks: Res<Docks>,
) {
    let corner = commands
        .spawn((
            Node {
                width: dota(LAUNCHER.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(docks.right),
        ))
        .id();
    let art = (&*fonts, &*manifest, &*server);
    spawn_stash(&mut commands, art, &catalog, corner);
    let hint = catalog.word(
        "dota_quick_buy_hint",
        "Shift-click items to add to Quickbuy.",
    );
    spawn_quick(&mut commands, art, hint, corner);
    spawn_launcher(&mut commands, art, &docks, corner);
}

/// Dota's art at `logical` stretched over a node, over a ground of
/// `ground` (none where the cache has no art: the ground alone).
fn backdrop(art: (&Manifest, &AssetServer), logical: &str, ground: Color) -> impl Bundle {
    let image = art.0.file(logical).map_or_else(
        || ImageNode::default().with_color(Color::NONE),
        |path| ImageNode::new(art.1.load(path)).with_mode(NodeImageMode::Stretch),
    );
    (image, BackgroundColor(ground))
}

/// The stash: its backdrop, its title, its six slots, and GRAB ALL's tab
/// out past its left edge.
fn spawn_stash(
    commands: &mut Commands,
    (fonts, manifest, server): (&Fonts, &Manifest, &AssetServer),
    catalog: &Catalog,
    corner: Entity,
) {
    let block = commands
        .spawn((
            StashBlock,
            Node {
                width: dota(STASH_BG.0),
                height: dota(STASH_BG.1),
                justify_content: JustifyContent::Center,
                padding: UiRect::top(dota(9.0)),
                ..default()
            },
            backdrop((manifest, server), "shop.stash", ui::PANEL_DEEP),
            Interaction::default(),
            Blocker,
            ChildOf(corner),
        ))
        .id();
    commands.spawn((
        ui::text(
            catalog.word("dota_inventorystash", "STASH").to_uppercase(),
            &fonts.bold,
            14.0 * DOTA,
            STASH_INK,
        ),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(block),
    ));
    let row = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: dota(STASH_ROW.0),
                top: dota(STASH_ROW.1),
                column_gap: dota(3.0),
                ..default()
            },
            ChildOf(block),
        ))
        .id();
    for slot in BAG_SLOTS..BAG_SLOTS + STASH_SLOTS {
        let size = (STASH_BOX.0 * DOTA, STASH_BOX.1 * DOTA);
        crate::hud::spawn_slot_box(commands, fonts, row, size, ItemBox(slot), false, false);
    }
    spawn_grab_all(commands, (fonts, manifest, server), catalog, block);
}

/// GRAB ALL's tab, at the stash's top left.
fn spawn_grab_all(
    commands: &mut Commands,
    (fonts, manifest, server): (&Fonts, &Manifest, &AssetServer),
    catalog: &Catalog,
    block: Entity,
) {
    let grab = commands
        .spawn((
            GrabAll,
            Button,
            Node {
                position_type: PositionType::Absolute,
                right: dota(GRAB_RIGHT),
                top: dota(5.0),
                width: dota(GRAB.0),
                height: dota(GRAB.1),
                padding: UiRect::left(dota(4.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            backdrop((manifest, server), "shop.grab_all", ui::INK_DIM),
            ChildOf(block),
        ))
        .id();
    commands.spawn((
        ui::text(
            catalog.word("dota_inventory_grab_all", "GRAB ALL"),
            &fonts.bold,
            0.9 * 14.0 * DOTA,
            GRAB_INK,
        ),
        Pickable::IGNORE,
        ChildOf(grab),
    ));
}

/// The quick-buy rows: the icons from the left, the hint while there are
/// none, the kept item at the right.
fn spawn_quick(
    commands: &mut Commands,
    (fonts, manifest, server): (&Fonts, &Manifest, &AssetServer),
    hint: &str,
    corner: Entity,
) {
    let rows = commands
        .spawn((
            Node {
                width: dota(QUICK_WIDTH),
                min_height: dota(QUICK_HEIGHT),
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            backdrop((manifest, server), "shop.quickbuy", QUICK_GROUND),
            Interaction::default(),
            Blocker,
            ChildOf(corner),
        ))
        .id();
    commands.spawn((
        QuickRow,
        Node {
            width: dota(4.0 + QUICK_COLUMNS as f32 * (QUICK_SPACE + QUICK_ITEM.0)),
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            align_content: AlignContent::FlexStart,
            padding: UiRect::new(dota(4.0), px(0), dota(8.0), dota(8.0)),
            row_gap: dota(4.0),
            ..default()
        },
        ChildOf(rows),
    ));
    commands.spawn((
        QuickHint,
        Node {
            position_type: PositionType::Absolute,
            left: dota(8.0),
            right: dota(85.0),
            height: percent(100),
            align_items: AlignItems::Center,
            ..default()
        },
        children![(
            ui::text(hint, &fonts.regular, 13.0 * DOTA, HINT_INK),
            ui::shadow(),
        )],
        Pickable::IGNORE,
        ChildOf(rows),
    ));
    spawn_sticky(commands, (fonts, manifest, server), rows);
}

/// The kept item in its own darker column at the quick-buy rows' right.
fn spawn_sticky(commands: &mut Commands, art: (&Fonts, &Manifest, &AssetServer), rows: Entity) {
    let sticky = commands
        .spawn((
            Node {
                width: dota(STICKY_WIDTH),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(STICKY_GROUND),
            ChildOf(rows),
        ))
        .id();
    let size = (QUICK_ITEM.0 * DOTA, QUICK_ITEM.1 * DOTA);
    cell::spawn(commands, art, sticky, STICKY.0, Place::Grid, size);
}

/// The gold and courier block: its backdrop, the shop button with the gold
/// and its coin, and the courier's buttons.
fn spawn_launcher(
    commands: &mut Commands,
    (fonts, manifest, server): (&Fonts, &Manifest, &AssetServer),
    docks: &Docks,
    corner: Entity,
) {
    let block = commands
        .spawn((
            Node {
                width: dota(LAUNCHER.0),
                height: dota(LAUNCHER.1),
                ..default()
            },
            backdrop((manifest, server), "shop.launcher", ui::PANEL),
            Interaction::default(),
            Blocker,
            ChildOf(corner),
        ))
        .id();
    let controls = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                width: dota(QUICK_WIDTH),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(block),
        ))
        .id();
    spawn_shop_button(commands, (fonts, manifest, server), controls);
    commands.entity(docks.courier).insert((
        Node {
            position_type: PositionType::Absolute,
            left: dota(COURIER_LEFT),
            height: percent(100),
            padding: UiRect::left(dota(4.0)),
            align_items: AlignItems::Center,
            ..default()
        },
        ChildOf(controls),
    ));
}

/// The shop button, of a fixed size: the gold right-aligned before the coin
/// at its right end, shrunk as Dota's when Dota's most, 99999, would not fit.
fn spawn_shop_button(
    commands: &mut Commands,
    (fonts, manifest, server): (&Fonts, &Manifest, &AssetServer),
    controls: Entity,
) {
    let button = commands
        .spawn((
            ShopButton,
            Button,
            Blocker,
            Node {
                position_type: PositionType::Absolute,
                left: dota(SHOP_BUTTON_LEFT),
                top: dota((LAUNCHER.1 - SHOP_BUTTON.1) / 2.0),
                width: dota(SHOP_BUTTON.0),
                height: dota(SHOP_BUTTON.1),
                ..default()
            },
            backdrop((manifest, server), "shop.button", ui::PANEL_DEEP),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.33),
                px(0),
                px(0),
                dota(2.0),
                dota(2.0),
            ),
            ChildOf(controls),
        ))
        .id();
    let label = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: dota(GOLD_RIGHT),
                height: percent(100),
                justify_content: JustifyContent::FlexEnd,
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    commands.spawn((
        Gold,
        ui::text("0", &fonts.bold, GOLD_FONT * DOTA, GOLD_INK),
        ui::Shrink::default(),
        ui::OwnSize(GOLD_FONT * DOTA),
        ui::shadow(),
        TextLayout::no_wrap(),
        Pickable::IGNORE,
        ChildOf(label),
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: dota(COIN_RIGHT),
            top: dota((SHOP_BUTTON.1 - COIN) / 2.0 - 1.0),
            width: dota(COIN),
            height: dota(COIN),
            ..default()
        },
        ui::art(manifest, server, "hud.gold_small"),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// One's own gold when playing; in a replay or as a spectator, that of the
/// hero watched (the stream tells all); in the tactical phase, what the
/// queued items leave. Yellow, white within reach of a shop
/// (`.ShopInRange .ShopButtonValueLabel`).
fn gold(session: NonSend<Session>, mut text: Single<(&mut Text, &mut TextColor), With<Gold>>) {
    let core = &session.0;
    let shown = if core.in_tactical_phase() {
        core.gold_after_queue().to_string()
    } else {
        let playing = matches!(core.source, Source::Live(_)) && core.my_slot.is_some();
        let seat = if playing {
            core.my_slot
        } else {
            core.panel_slot()
        };
        core.view()
            .and_then(|v| v.player(seat?))
            .and_then(|p| p.gold)
            .map_or("-".to_string(), |g| g.to_string())
    };
    text.0.set_if_neq(Text(shown));
    let ink = if core.at_home_shop() {
        Color::WHITE
    } else {
        GOLD_INK
    };
    text.1.set_if_neq(TextColor(ink));
}

/// The shop button's art: lit within reach of a shop, brighter under the
/// cursor, as Dota swaps its pictures.
fn shop_button(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut button: Single<(&Interaction, &mut ImageNode), With<ShopButton>>,
    mut shown: Local<&'static str>,
) {
    let hovered = *button.0 != Interaction::None;
    let logical = match (session.0.at_home_shop(), hovered) {
        (false, false) => "shop.button",
        (false, true) => "shop.button_hover",
        (true, false) => "shop.button_inrange",
        (true, true) => "shop.button_inrange_hover",
    };
    if *shown == logical {
        return;
    }
    *shown = logical;
    if let Some(path) = manifest.file(logical) {
        button.1.image = server.load(path);
    }
}

/// The stash while the shop is open, at home, while an item is held for
/// moving, or while one's own hero lies dead, as Dota shows it then (its
/// HUD's `StashVisible` without `StashActive`: what was bought is seen, and
/// nothing is taken out); GRAB ALL while what is in it can be taken.
fn stash(
    session: NonSend<Session>,
    panels: Res<Panels>,
    mut block: Single<&mut Node, With<StashBlock>>,
    mut grab: Single<&mut Visibility, With<GrabAll>>,
) {
    let core = &session.0;
    let dead = core.my_slot.is_some() && core.view().is_some() && core.my_standing_hero().is_none();
    let shown = panels.shop || core.at_home_shop() || core.held_item.is_some() || dead;
    let display = if shown { Display::Flex } else { Display::None };
    if block.display != display {
        block.display = display;
    }
    grab.set_if_neq(ui::visible(!grab_moves(core).is_empty()));
}

/// The quick-buy rows, rebuilt as what is left to buy of the quick-buy item
/// changes: its missing parts in build order, or the item itself when none
/// is missing, as many as Dota's two rows hold and then how many more; a
/// hint while there is none.
fn quick_row(
    mut commands: Commands,
    session: NonSend<Session>,
    mut last: Local<Option<Vec<u16>>>,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    row: Single<Entity, With<QuickRow>>,
    mut hint: Single<&mut Visibility, With<QuickHint>>,
) {
    let core = &session.0;
    let items: Vec<u16> = core.quick_buy.map_or_else(Vec::new, |id| {
        let missing: Vec<u16> = core.missing_for(id).iter().map(|p| p.0).collect();
        if missing.is_empty() {
            vec![id]
        } else {
            missing
        }
    });
    if last.as_ref() == Some(&items) {
        return;
    }
    hint.set_if_neq(ui::visible(items.is_empty()));
    commands.entity(*row).despawn_children();
    let size = (QUICK_ITEM.0 * DOTA, QUICK_ITEM.1 * DOTA);
    for id in items.iter().take(QUICK_SLOTS) {
        let cell = cell::spawn(
            &mut commands,
            (&fonts, &manifest, &server),
            *row,
            *id,
            Place::Quick,
            size,
        );
        commands
            .entity(cell)
            .entry::<Node>()
            .and_modify(|mut node| {
                node.margin.left = dota(QUICK_SPACE);
            });
    }
    if items.len() > QUICK_SLOTS {
        commands.spawn((
            ui::text(
                format!("+{}", items.len() - QUICK_SLOTS),
                &fonts.bold,
                11.0 * DOTA,
                STASH_INK,
            ),
            ui::shadow(),
            Node {
                margin: UiRect::left(dota(QUICK_SPACE)),
                ..default()
            },
            ChildOf(*row),
        ));
    }
    *last = Some(items);
}

/// The moves that take what the stash holds into the hero's bag at home:
/// each stash slot into the first free slot, the inventory before the
/// backpack, and a scroll onto the scrolls the hero already carries.
pub fn grab_moves(core: &bota_client_core::App) -> Vec<(u8, u8)> {
    let plan = || {
        if !matches!(core.source, Source::Live(_)) || !core.at_home_shop() {
            return None;
        }
        let view = core.view()?;
        let player = view.player(core.my_slot?)?;
        let hero = view.unit(player.unit?)?;
        let mut free: VecDeque<u8> = (0..BAG_SLOTS)
            .filter(|slot| {
                hero.items
                    .get(usize::from(*slot))
                    .is_some_and(Option::is_none)
            })
            .collect();
        let mut moves = Vec::new();
        for (at, item) in player
            .stash
            .as_ref()?
            .iter()
            .enumerate()
            .take(usize::from(STASH_SLOTS))
        {
            let Some(item) = item else {
                continue;
            };
            let from = BAG_SLOTS + at as u8;
            if item.id == ItemId::TOWN_PORTAL_SCROLL && hero.town_portal.is_some() {
                moves.push((from, TOWN_PORTAL_SLOT));
            } else if let Some(to) = free.pop_front() {
                moves.push((from, to));
            }
        }
        Some(moves)
    };
    plan().unwrap_or_default()
}

/// Sends GRAB ALL's moves one at a time, a few snapshots apart, each only
/// while its stash slot still holds something and its bag slot is free.
fn grab_step(mut session: NonSendMut<Session>, mut grab: ResMut<Grab>) {
    if grab.moves.is_empty() {
        return;
    }
    let core = &mut session.0;
    let Some(tick) = core.view().map(|v| v.tick) else {
        return;
    };
    if !core.at_home_shop() {
        grab.moves.clear();
        return;
    }
    if grab.sent.is_some_and(|sent| tick < sent + GRAB_SPACING) {
        return;
    }
    let still = grab_moves(core);
    while let Some((from, to)) = grab.moves.pop_front() {
        // Still in the stash, and still the place GRAB ALL would put it.
        if still.contains(&(from, to)) {
            core.send_order_to(
                None,
                Order::Swap {
                    from: ItemSlot(from),
                    to: ItemSlot(to),
                },
            );
            grab.sent = Some(tick);
            return;
        }
    }
}

impl Grab {
    /// Starts taking everything out of the stash.
    pub fn start(&mut self, core: &bota_client_core::App) {
        self.moves = grab_moves(core).into();
        self.sent = None;
    }
}
