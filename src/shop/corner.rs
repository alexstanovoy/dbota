//! The shop's corner at the bottom right, as Dota's: the stash with GRAB
//! ALL, the quick-buy row with the item kept beside it, and the gold, which
//! opens the shop, with the courier's buttons right of it.

use std::collections::VecDeque;

use bevy::prelude::*;
use bota_client_core::{BAG_SLOTS, Source, TOWN_PORTAL_SLOT};
use bota_proto::{ItemId, ItemSlot, Order};

use super::catalog::Catalog;
use super::cell::{self, CELL, GAP, Place};
use crate::hud::{Blocker, Docks, ItemBox};
use crate::input::Panels;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// The stash's slots.
const STASH_SLOTS: u8 = bota_core::rules::STASH_SLOTS as u8;
/// A stash slot's size.
const STASH_BOX: (f32, f32) = (37.0, 27.0);
/// The corner's width: the stash's row.
const WIDTH: f32 = STASH_SLOTS as f32 * STASH_BOX.0 + (STASH_SLOTS - 1) as f32 * GAP + 12.0;
/// Snapshots between two of GRAB ALL's moves: a seat's orders due on one
/// tick replace each other, so they go a tick apart and a little more.
const GRAB_SPACING: u32 = 2;
/// GRAB ALL's pale blue.
const GRAB_TINT: Color = Color::srgb(0.62, 0.71, 0.86);
/// The item kept beside the quick-buy row, as Dota keeps it by default.
pub const STICKY: ItemId = ItemId::TOWN_PORTAL_SCROLL;

/// The gold, a click on which opens the shop.
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

/// Where the quick-buy row's icons go, and its hint while it is empty.
#[derive(Component)]
struct QuickRow;

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
            (gold, stash, quick_row, grab_step).run_if(super::shop_runs),
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
                width: px(WIDTH),
                margin: UiRect::new(px(0), px(ui::CORNER_MARGIN), px(0), px(6)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(docks.right),
        ))
        .id();
    let grab = spawn_stash(&mut commands, &fonts, &catalog, corner);
    cell::backdrop(
        &mut commands,
        (&manifest, &server),
        grab,
        "shop.grab_all",
        GRAB_TINT,
    );
    let art = (&*fonts, &*manifest, &*server);
    let quick = commands
        .spawn((
            Node {
                column_gap: px(GAP),
                align_items: AlignItems::Center,
                padding: UiRect::all(px(5)),
                ..default()
            },
            BackgroundColor(ui::PANEL),
            Interaction::default(),
            Blocker,
            ChildOf(corner),
        ))
        .id();
    commands.spawn((
        QuickRow,
        Node {
            flex_grow: 1.0,
            min_height: px(CELL.1),
            column_gap: px(GAP),
            align_items: AlignItems::Center,
            ..default()
        },
        ChildOf(quick),
    ));
    cell::spawn(&mut commands, art, quick, STICKY.0, Place::Grid);
    spawn_gold_row(&mut commands, &fonts, &manifest, &server, &docks, corner);
}

/// The stash: GRAB ALL, its title and its six slots.
fn spawn_stash(
    commands: &mut Commands,
    fonts: &Fonts,
    catalog: &Catalog,
    corner: Entity,
) -> Entity {
    let block = commands
        .spawn((
            StashBlock,
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6)),
                row_gap: px(4),
                ..default()
            },
            BackgroundColor(ui::PANEL_DEEP),
            Interaction::default(),
            Blocker,
            ChildOf(corner),
        ))
        .id();
    let head = commands
        .spawn((
            Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
            ChildOf(block),
        ))
        .id();
    let grab = commands
        .spawn((
            GrabAll,
            Button,
            Node {
                width: px(96),
                height: px(24),
                padding: UiRect::left(px(18)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(GRAB_TINT),
            ChildOf(head),
        ))
        .id();
    commands.spawn((
        ui::text(
            catalog.word("dota_inventory_grab_all", "GRAB ALL"),
            &fonts.bold,
            12.0,
            Color::srgb(0.1, 0.13, 0.17),
        ),
        Pickable::IGNORE,
        ChildOf(grab),
    ));
    commands.spawn((
        ui::text(
            catalog.word("dota_inventorystash", "STASH"),
            &fonts.bold,
            12.0,
            ui::INK_DIM,
        ),
        Node {
            margin: UiRect::right(px(40)),
            ..default()
        },
        ChildOf(head),
    ));
    let row = commands
        .spawn((
            Node {
                column_gap: px(GAP),
                ..default()
            },
            ChildOf(block),
        ))
        .id();
    for slot in BAG_SLOTS..BAG_SLOTS + STASH_SLOTS {
        crate::hud::spawn_slot_box(commands, fonts, row, STASH_BOX, ItemBox(slot), false, false);
    }
    grab
}

/// The gold, which opens the shop, and the courier's delivery button.
fn spawn_gold_row(
    commands: &mut Commands,
    fonts: &Fonts,
    manifest: &Manifest,
    server: &AssetServer,
    docks: &Docks,
    corner: Entity,
) {
    let row = commands
        .spawn((
            Node {
                column_gap: px(8),
                justify_content: JustifyContent::End,
                align_items: AlignItems::Center,
                ..default()
            },
            ChildOf(corner),
        ))
        .id();
    let button = commands
        .spawn((
            ShopButton,
            Button,
            Blocker,
            Node {
                flex_grow: 1.0,
                height: px(44),
                padding: UiRect::horizontal(px(14)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::auto(Color::srgb(0.36, 0.29, 0.12)),
                ColorStop::auto(Color::srgb(0.2, 0.15, 0.05)),
            ])),
            BorderColor::all(Color::srgb(0.62, 0.5, 0.22)),
            ChildOf(row),
        ))
        .id();
    cell::backdrop(
        commands,
        (manifest, server),
        button,
        "shop.button",
        Color::srgb(1.0, 0.82, 0.42),
    );
    commands.spawn((
        Gold,
        ui::text("0", &fonts.bold, 24.0, ui::GOLDEN),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    if let Some(coin) = manifest.file("hud.gold") {
        commands.spawn((
            ImageNode::new(server.load(coin)),
            Node {
                width: px(28),
                height: px(28),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(button),
        ));
    }
    commands.entity(row).add_child(docks.courier);
}

/// One's own gold when playing; in a replay or as a spectator, that of the
/// hero watched (the stream tells all); in the tactical phase, what the
/// queued items leave.
fn gold(session: NonSend<Session>, mut text: Single<&mut Text, With<Gold>>) {
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
    text.set_if_neq(Text(shown));
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
    let dead = core.my_slot.is_some() && core.view().is_some() && core.my_hero().is_none();
    let shown = panels.shop || core.at_home_shop() || core.held_item.is_some() || dead;
    let display = if shown { Display::Flex } else { Display::None };
    if block.display != display {
        block.display = display;
    }
    grab.set_if_neq(ui::visible(!grab_moves(core).is_empty()));
}

/// The quick-buy row, rebuilt as what is left to buy of the quick-buy item
/// changes: its missing parts in build order, or the item itself when none
/// is missing; a hint while there is none.
fn quick_row(
    mut commands: Commands,
    session: NonSend<Session>,
    mut last: Local<Option<Vec<u16>>>,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    row: Single<Entity, With<QuickRow>>,
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
    commands.entity(*row).despawn_children();
    if items.is_empty() {
        commands.spawn((
            ui::text(
                "Right-click or shift-click items to add to Quickbuy.",
                &fonts.regular,
                12.0,
                ui::INK_DIM,
            ),
            Node {
                max_width: px(WIDTH - CELL.0 - 20.0),
                ..default()
            },
            ChildOf(*row),
        ));
    }
    for id in &items {
        cell::spawn(
            &mut commands,
            (&fonts, &manifest, &server),
            *row,
            *id,
            Place::Quick,
        );
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
