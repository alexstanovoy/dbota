//! Dota's context menu of an inventory item (`context_menu_inventory_item`):
//! a right click opens Show in Shop and the item's choices at the cursor, Sell (away from
//! the shop, Mark for Sell, and again Unmark for Sell), Disassemble for a build that
//! may come apart now, Move to Stash where the stash can be reached, and Lock Combining
//! (Unlock Combining once locked) for an item that is part of a recipe. A strategy-phase
//! right click refunds a pending purchase directly. Each choice gives the order the same
//! move gives by drag or key. Dota's Alert Allies is not here: the simulator has no
//! order for it.

use bevy::{
    picking::hover::{DirectlyHovered, Hovered},
    ui_widgets::Button,
};
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
    ShowInShop {
        item: u16,
    },
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
    /// Dota's label key and the plain word where the cache has none.
    fn label(self) -> (&'static str, &'static str) {
        match self {
            Choice::ShowInShop { .. } => ("DOTA_InventoryMenu_ShowInShop", "Show in Shop"),
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

/// The menu open now: its top left corner, placed once when it opened, and the choices.
#[derive(Resource, Default)]
pub(crate) struct Menu {
    open: Option<(Vec2, Vec<Choice>)>,
    /// Set when the menu changed and must be drawn anew.
    stale: bool,
}

impl Menu {
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// The menu's box.
#[derive(Component)]
pub(crate) struct Root;

/// Where a menu of `size` goes for a click at `at` in a viewport of `room`: its corner at the
/// pointer, or, where it does not fit between the pointer and an edge, flipped to the other side of
/// the pointer (the way Dota's context menus open) and then held inside the viewport. The same
/// answer for the same arguments, so a menu is placed once per opening and never moves again.
fn fit(at: Vec2, size: Vec2, room: Vec2) -> Vec2 {
    let flip = |at: f32, size: f32, room: f32| {
        let corner = if at + size <= room { at } else { at - size };
        corner.clamp(0.0, (room - size).max(0.0))
    };
    Vec2::new(flip(at.x, size.x, room.x), flip(at.y, size.y, room.y))
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Menu>()
        .add_systems(First, reset.in_set(crate::loading::Begin))
        .add_systems(
            Update,
            (
                open.in_set(MenuSystems::Open),
                draw,
                choose.in_set(MenuSystems::Choose),
            )
                .chain(),
        );
}

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub(crate) enum MenuSystems {
    Open,
    Choose,
}

/// The choices of the item in a slot.
fn choices(
    core: &bota_client_core::App,
    bag: &TacticalBag,
    settling: &crate::drag::Settling,
    slot: u8,
) -> Vec<Choice> {
    if core.in_tactical_phase() {
        return Vec::new();
    }
    let Some(shown) = crate::shop::displayed_item(core, bag, settling, slot) else {
        return Vec::new();
    };
    let mut out = vec![Choice::ShowInShop { item: shown.id.0 }];
    let own_dead_kit = core
        .my_slot
        .and_then(|slot| core.view()?.player(slot))
        .is_some_and(|player| player.respawn_left > 0 && player.unit == core.commanded());
    if !(core.controls_selection() || own_dead_kit) || !matches!(core.source, Source::Live(_)) {
        return out;
    }
    let Some(item) = core.item_in(slot) else {
        return out;
    };
    let base_shop = core
        .slot_unit()
        .is_some_and(|unit| core.unit_at_base_shop(unit.id));
    if bota_core::item_def(item.id).is_some_and(|def| def.limits.sellable)
        && if own_dead_kit {
            core.dead_sale_allowed(slot) == Some(true)
        } else {
            slot != TOWN_PORTAL_SLOT
        }
    {
        let direct_sale = own_dead_kit
            || base_shop
            || slot >= BAG_SLOTS
            || core
                .slot_unit()
                .is_some_and(|unit| core.unit_at_secret_shop(unit.id));
        out.push(Choice::Sell {
            slot,
            mark: (!direct_sale).then(|| item.selling == Some(true)),
        });
    }
    if core.can_disassemble(slot) {
        out.push(Choice::Disassemble { slot });
    }
    let free = (0..STASH_SLOTS as u8)
        .map(|i| BAG_SLOTS + i)
        .find(|s| core.item_in(*s).is_none());
    if let (true, true, Some(to)) = (slot < BAG_SLOTS, base_shop, free) {
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

/// Refunds preparation items directly; otherwise opens the inventory menu.
fn open(
    mut session: NonSendMut<Session>,
    mut bag: ResMut<TacticalBag>,
    settling: Res<crate::drag::Settling>,
    mouse: Res<ButtonInput<MouseButton>>,
    items: Query<(&DirectlyHovered, &ItemBox)>,
    roots: Query<&Hovered, Or<(With<Root>, With<Choice>)>>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut menu: ResMut<Menu>,
) {
    let pressed = mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right);
    if !pressed {
        return;
    }
    let core = &mut session.0;
    let on_menu = roots.iter().any(|i| i.get());
    let hovered = items.iter().find(|(i, _)| i.get()).map(|(_, b)| b.0);
    let at = window.cursor_position();
    if let (true, Some(slot), Some(at)) = (mouse.just_pressed(MouseButton::Right), hovered, at) {
        if core.in_tactical_phase() {
            bag.refund(core, slot);
            menu.open = None;
            menu.stale = true;
            return;
        }
        let list = choices(core, &bag, &settling, slot);
        if !list.is_empty() {
            let scale = ui_scale.0.max(0.1);
            let room = Vec2::new(window.width(), window.height()) / scale;
            let size = Vec2::new(WIDTH, CHOICE_HEIGHT * list.len() as f32);
            menu.open = Some((fit(at / scale, size, room), list));
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
) {
    if !menu.stale {
        return;
    }
    menu.stale = false;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some((corner, list)) = &menu.open else {
        return;
    };
    let root = commands
        .spawn((
            Root,
            Blocker,
            Hovered::default(),
            Node {
                position_type: PositionType::Absolute,
                left: px(corner.x),
                top: px(corner.y),
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
                Hovered::default(),
                Node {
                    // Exactly the height `fit` placed the menu by.
                    height: px(CHOICE_HEIGHT),
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
    mut choices: Query<(&Hovered, &Choice, &mut BackgroundColor, &Children)>,
    mut inks: Query<&mut TextColor>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut panels: ResMut<crate::input::Panels>,
    mut chosen: ResMut<crate::shop::Chosen>,
) {
    let core = &mut session.0;
    let mut done = false;
    for (interaction, choice, mut ground, children) in &mut choices {
        let lit = interaction.get();
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
                Choice::ShowInShop { item } => {
                    panels.shop = true;
                    chosen.0 = Some(item);
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

fn reset(mut menu: ResMut<Menu>) {
    *menu = Menu {
        stale: true,
        ..default()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click at every pixel of the viewport, the last ones before each edge included, and of
    /// viewports from a phone's to a wide screen's: the menu lies wholly inside, where it does not fit
    /// it lies on the other side of the pointer, and the answer is the same every time it is asked.
    #[test]
    fn a_menu_fits_the_viewport_for_a_click_at_any_pixel_and_flips_to_the_pointers_other_side() {
        for (width, height, choices) in [
            (320.0, 240.0, 5.0),
            (800.0, 500.0, 5.0),
            (1920.0, 1080.0, 1.0),
            (1920.0, 1080.0, 5.0),
            (3440.0, 1440.0, 5.0),
        ] {
            let room = Vec2::new(width, height);
            let size = Vec2::new(WIDTH, CHOICE_HEIGHT * choices);
            for x in (0..width as u32).filter(|x| x % 7 == 0 || *x + 40 >= width as u32) {
                for y in (0..height as u32).filter(|y| y % 7 == 0 || *y + 40 >= height as u32) {
                    let at = Vec2::new(x as f32, y as f32);
                    let corner = fit(at, size, room);
                    assert_eq!(
                        corner,
                        fit(at, size, room),
                        "the same click, the same place"
                    );
                    assert!(
                        corner.x >= 0.0
                            && corner.y >= 0.0
                            && corner.x + size.x <= room.x
                            && corner.y + size.y <= room.y,
                        "{at} in {room}: the menu at {corner} leaves the viewport"
                    );
                    if at.x + size.x <= room.x {
                        assert_eq!(
                            corner.x, at.x,
                            "{at} in {room}: room on the right, so it opens there"
                        );
                    } else if at.x >= size.x {
                        assert_eq!(
                            corner.x,
                            at.x - size.x,
                            "{at} in {room}: no room on the right, so it opens to the left of the pointer"
                        );
                    }
                    if at.y + size.y <= room.y {
                        assert_eq!(corner.y, at.y);
                    } else if at.y >= size.y {
                        assert_eq!(
                            corner.y,
                            at.y - size.y,
                            "{at} in {room}: no room below, so it opens above the pointer"
                        );
                    }
                }
            }
        }
    }
}
