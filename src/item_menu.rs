//! Dota's context menu of an inventory item (`context_menu_inventory_item`):
//! a right click on an item opens its choices at the cursor, Sell (away from
//! the shop, Mark for Sell, and again Unmark for Sell), Disassemble for a build that
//! may come apart now, Move to Stash where the stash can be reached, and Lock Combining
//! (Unlock Combining once locked) for an item that is part of a recipe; in the strategy
//! phase, Sell takes the item off what is chosen. Each choice gives the order the same
//! move gives by drag or key. Dota's Alert Allies is not here: the simulator has no
//! order for it.

use bevy::{prelude::*, window::PrimaryWindow};
use bota_client_core::{BAG_SLOTS, Intent, Source, TOWN_PORTAL_SLOT};
use bota_core::rules::STASH_SLOTS;

use crate::bag::TacticalBag;
use crate::dota_text::Texts;
use crate::hud::{Blocker, ItemBox};
use crate::session::Session;
use crate::ui::{self, Fonts};

/// The menu's width and the least height of a choice, in Dota's pixels at
/// 1080 lines (`Button`: 200 by 40) turned into interface pixels.
const WIDTH: f32 = 200.0 * ui::DOTA;
const CHOICE_HEIGHT: f32 = 40.0 * ui::DOTA;
/// The choice's grounds and inks: `#182128`, and its hover `#8ea3c922`;
/// `#acaaaa` and, hovered, `#eae9e9`.
const GROUND: Color = Color::srgb(0.094, 0.129, 0.157);
const HOVERED: Color = Color::srgba(0.557, 0.639, 0.788, 0.133);
const INK: Color = Color::srgb(0.675, 0.667, 0.667);
const INK_HOVERED: Color = Color::srgb(0.918, 0.914, 0.914);

/// What a choice does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Sell {
        slot: u8,
        mark: Option<bool>,
    },
    ToStash {
        slot: u8,
        to: u8,
    },
    Disassemble {
        slot: u8,
    },
    /// Sets the lock to `lock`: the choice offers the state the item is not in.
    CombineLock {
        slot: u8,
        lock: bool,
    },
}

impl Choice {
    /// Dota's label key and its plain stand-in.
    fn label(self) -> (&'static str, &'static str) {
        match self {
            Choice::Sell { mark: None, .. } => ("DOTA_InventoryMenu_Sell", "Sell"),
            Choice::Sell {
                mark: Some(false), ..
            } => ("DOTA_InventoryMenu_MarkForSell", "Mark for Sell"),
            Choice::Sell {
                mark: Some(true), ..
            } => ("DOTA_InventoryMenu_UnmarkForSell", "Unmark for Sell"),
            Choice::ToStash { .. } => ("DOTA_InventoryMenu_MoveToStash", "Move to Stash"),
            Choice::Disassemble { .. } => ("DOTA_InventoryMenu_Disassemble", "Disassemble"),
            Choice::CombineLock { lock: true, .. } => {
                ("DOTA_InventoryMenu_LockCombine", "Lock Combining")
            }
            Choice::CombineLock { lock: false, .. } => {
                ("DOTA_InventoryMenu_UnlockCombine", "Unlock Combining")
            }
        }
    }
}

/// The menu open now: where, and the choices.
#[derive(Resource, Default)]
struct Menu {
    open: Option<(Vec2, Vec<Choice>)>,
    /// Set when the menu changed and must be drawn anew.
    stale: bool,
}

/// The menu's box.
#[derive(Component)]
struct Root;

pub fn plugin(app: &mut App) {
    app.init_resource::<Menu>()
        .add_systems(Update, (open, draw, choose).chain());
}

/// The choices of the item in a slot.
fn choices(core: &bota_client_core::App, bag: &TacticalBag, slot: u8) -> Vec<Choice> {
    if core.in_tactical_phase() {
        return bag
            .id_at(slot)
            .map(|_| Choice::Sell { slot, mark: None })
            .into_iter()
            .collect();
    }
    let Some(item) = core.item_in(slot) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if slot != TOWN_PORTAL_SLOT {
        let at_shop = core.at_home_shop() || slot >= BAG_SLOTS;
        out.push(Choice::Sell {
            slot,
            mark: (!at_shop).then(|| item.selling == Some(true)),
        });
    }
    if core.can_disassemble(slot) {
        out.push(Choice::Disassemble { slot });
    }
    let free = (0..STASH_SLOTS as u8)
        .map(|i| BAG_SLOTS + i)
        .find(|s| core.item_in(*s).is_none());
    if let (true, true, Some(to)) = (slot < BAG_SLOTS, core.at_home_shop(), free) {
        out.push(Choice::ToStash { slot, to });
    }
    if let Some(locked) = core.combine_lock_of(slot) {
        out.push(Choice::CombineLock {
            slot,
            lock: !locked,
        });
    }
    out
}

/// Opens the menu on a right click on an item box, and closes it on any click
/// elsewhere.
fn open(
    session: NonSend<Session>,
    bag: Res<TacticalBag>,
    mouse: Res<ButtonInput<MouseButton>>,
    items: Query<(&Interaction, &ItemBox)>,
    roots: Query<&Interaction, Or<(With<Root>, With<Choice>)>>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut menu: ResMut<Menu>,
) {
    let pressed = mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right);
    if !pressed {
        return;
    }
    let core = &session.0;
    let on_menu = roots.iter().any(|i| *i != Interaction::None);
    let hovered = items
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .map(|(_, b)| b.0);
    let may = core.in_tactical_phase()
        || (core.my_hero().is_some()
            && core.controls_selection()
            && matches!(core.source, Source::Live(_)));
    let at = window.cursor_position();
    if let (true, true, Some(slot), Some(at)) =
        (mouse.just_pressed(MouseButton::Right), may, hovered, at)
    {
        let list = choices(core, &bag, slot);
        if !list.is_empty() {
            menu.open = Some((at / ui_scale.0.max(0.1), list));
            menu.stale = true;
            return;
        }
    }
    if menu.open.is_some() && !on_menu {
        menu.open = None;
        menu.stale = true;
    }
}

/// Draws the menu anew when it changed.
fn draw(
    mut commands: Commands,
    mut menu: ResMut<Menu>,
    roots: Query<Entity, With<Root>>,
    fonts: Res<Fonts>,
    texts: Res<Texts>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
) {
    if !menu.stale {
        return;
    }
    menu.stale = false;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some((at, list)) = &menu.open else { return };
    let scale = ui_scale.0.max(0.1);
    let room = Vec2::new(window.width(), window.height()) / scale;
    let height = CHOICE_HEIGHT * list.len() as f32;
    let left = at.x.min(room.x - WIDTH - 2.0).max(0.0);
    let top = at.y.min(room.y - height - 2.0).max(0.0);
    let root = commands
        .spawn((
            Root,
            Blocker,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            GlobalZIndex(45),
        ))
        .id();
    for choice in list {
        let (key, plain) = choice.label();
        let word = texts.label(key, &[]).unwrap_or_else(|| plain.to_owned());
        commands
            .spawn((
                *choice,
                Button,
                Node {
                    min_height: px(CHOICE_HEIGHT),
                    padding: UiRect::new(
                        px(8.0 * ui::DOTA),
                        px(4.0 * ui::DOTA),
                        px(4.0 * ui::DOTA),
                        px(4.0 * ui::DOTA),
                    ),
                    border: UiRect::all(px(1)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                BorderColor::all(Color::BLACK),
                BackgroundColor(GROUND),
                ChildOf(root),
            ))
            .with_child(ui::text(word, &fonts.regular, 16.0 * ui::DOTA, INK));
    }
}

/// Lights the choice under the cursor, and does what a pressed one says.
fn choose(
    mut session: NonSendMut<Session>,
    mut menu: ResMut<Menu>,
    mut choices: Query<(&Interaction, &Choice, &mut BackgroundColor, &Children)>,
    mut inks: Query<&mut TextColor>,
    mouse: Res<ButtonInput<MouseButton>>,
    bag: Res<TacticalBag>,
) {
    let core = &mut session.0;
    let mut done = false;
    for (interaction, choice, mut ground, children) in &mut choices {
        let lit = *interaction != Interaction::None;
        let (g, ink) = if lit {
            (HOVERED, INK_HOVERED)
        } else {
            (GROUND, INK)
        };
        ground.set_if_neq(BackgroundColor(g));
        for child in children {
            if let Ok(mut text) = inks.get_mut(*child) {
                text.set_if_neq(TextColor(ink));
            }
        }
        if lit && mouse.just_pressed(MouseButton::Left) {
            match *choice {
                Choice::Sell { slot, .. } if core.in_tactical_phase() => {
                    // Strategy phase: the item leaves what is chosen.
                    if let Some(id) = bag.id_at(slot) {
                        core.act(Intent::Unqueue(id));
                    }
                }
                Choice::Sell { slot, .. } => core.act(Intent::Sell(slot)),
                Choice::ToStash { slot, to } => core.act(Intent::Swap { from: slot, to }),
                Choice::Disassemble { slot } => core.act(Intent::Disassemble(slot)),
                Choice::CombineLock { slot, lock } => {
                    core.act(Intent::SetCombineLock { slot, locked: lock })
                }
            }
            done = true;
        }
    }
    if done {
        menu.open = None;
        menu.stale = true;
    }
}
