//! Popovers (the tooltips of the shop, the inventory and the abilities, the inventory's context menu)
//! are never drawn for a frame at a provisional place or size: the client is stepped one frame at a
//! time while the pointer goes to items and abilities and a menu is opened, and on every frame on
//! which a popover is visible its rectangle is the one it comes to rest at for that content.

use std::collections::HashMap;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, MID, Server};

/// Frames a popover has to hold still for before it counts as at rest.
const CALM: usize = 8;
/// Frames the client may take to show a popover that is wanted (a hang guard, not a duration).
const MOST_FRAMES: usize = 120;
/// Windows: a common one, a small one and one whose interface scale is fractional.
const WINDOWS: [&str; 3] = ["1920x1080", "1280x720", "2877x1642"];

/// What a popover is and where it is drawn on one frame: the words it holds and its rectangle.
#[derive(Clone, PartialEq, Debug)]
struct Seen {
    words: String,
    rect: Rect,
}

/// The popover `T` as drawn on this frame, if it is visible.
fn seen<T: Component>(client: &mut Client) -> Option<Seen> {
    let world = client.world_mut();
    let roots: Vec<(Entity, Rect)> = world
        .query_filtered::<(
            Entity,
            &bevy::ui::UiGlobalTransform,
            &ComputedNode,
            &InheritedVisibility,
        ), With<T>>()
        .iter(world)
        .filter(|(_, _, node, shown)| shown.get() && node.size() != Vec2::ZERO)
        .map(|(entity, at, node, _)| (entity, Rect::from_center_size(at.translation, node.size())))
        .collect();
    let (root, rect) = *roots.first()?;
    let mut words: Vec<(Vec2, String)> = Vec::new();
    for entity in super::harness::subtree(world, root) {
        if let Some(text) = world.get::<Text>(entity)
            && let Some(at) = world.get::<bevy::ui::UiGlobalTransform>(entity)
            && !text.0.is_empty()
        {
            words.push((at.translation, text.0.clone()));
        }
    }
    words.sort_by(|a, b| a.0.y.total_cmp(&b.0.y).then(a.0.x.total_cmp(&b.0.x)));
    let words = words
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join(" | ");
    Some(Seen { words, rect })
}

/// What the popovers drew on each frame of a run.
#[derive(Default)]
struct Watch {
    tips: Vec<Option<Seen>>,
    menus: Vec<Option<Seen>>,
    /// Where each popover came to rest, by its words.
    rests: HashMap<String, Vec<Rect>>,
}

impl Watch {
    fn frame(&mut self, client: &mut Client) {
        client.step_frames(1);
        self.tips.push(seen::<crate::tips::Tip>(client));
        self.menus.push(seen::<crate::item_menu::Root>(client));
    }

    /// Runs frames until the popover `pick` selects holds still, and notes where it rests.
    fn until_calm(
        &mut self,
        client: &mut Client,
        what: &str,
        pick: impl Fn(&Watch) -> &Vec<Option<Seen>>,
    ) {
        let mut calm = 0;
        for _ in 0..MOST_FRAMES {
            self.frame(client);
            let log = pick(self);
            let [.., before, now] = log.as_slice() else {
                continue;
            };
            calm = if now.is_some() && now == before {
                calm + 1
            } else {
                0
            };
            if calm >= CALM {
                let now = now.clone().expect("a popover at rest");
                let rests = self.rests.entry(now.words).or_default();
                if !rests.contains(&now.rect) {
                    rests.push(now.rect);
                }
                return;
            }
        }
        let log = pick(self);
        let last: Vec<&Option<Seen>> = log.iter().rev().take(6).collect();
        let world = client.world_mut();
        let cursor = world
            .query::<&Window>()
            .single(world)
            .ok()
            .and_then(Window::cursor_position);
        let items: Vec<String> = world
            .query::<(
                &bevy::picking::hover::DirectlyHovered,
                &crate::hud::ItemBox,
                &InheritedVisibility,
                &bevy::ui::UiGlobalTransform,
            )>()
            .iter(world)
            .map(|(hovered, slot, shown, at)| {
                format!(
                    "slot {} hovered {} shown {} at {}",
                    slot.0,
                    hovered.get(),
                    shown.get(),
                    at.translation
                )
            })
            .collect();
        panic!(
            "{what}: no popover held still for {CALM} frames within {MOST_FRAMES}; the cursor is at {cursor:?} the item boxes {items:?}; the last frames, latest first: {last:?}"
        );
    }

    fn until_tip_calm(&mut self, client: &mut Client, what: &str) {
        self.until_calm(client, what, |watch| &watch.tips);
    }

    /// The frames, as complaints: a visible popover whose rectangle is not one it rests at for its
    /// words, or that leaves the window.
    fn complaints(&self, window: Rect) -> Vec<String> {
        let mut all = Vec::new();
        for (kind, log) in [("tooltip", &self.tips), ("menu", &self.menus)] {
            for (frame, now) in log.iter().enumerate() {
                let Some(now) = now else {
                    continue;
                };
                let rests = self.rests.get(&now.words).map_or(&[][..], Vec::as_slice);
                if !rests.contains(&now.rect) {
                    all.push(format!(
                        "frame {frame}: the {kind} {:?} is drawn at {:?}, where it never rests (it rests at {rests:?})",
                        now.words, now.rect
                    ));
                }
                if now.rect.min.cmplt(window.min).any() || now.rect.max.cmpgt(window.max).any() {
                    all.push(format!(
                        "frame {frame}: the {kind} {:?} at {:?} leaves the window {window:?}",
                        now.words, now.rect
                    ));
                }
            }
        }
        all
    }
}

fn window_rect(client: &mut Client) -> Rect {
    let world = client.world_mut();
    let window = world.query::<&Window>().single(world).expect("window");
    Rect::new(
        0.0,
        0.0,
        window.physical_width() as f32,
        window.physical_height() as f32,
    )
}

/// A client in the game with the shop open on its basics and the hero at the fountain's shop, holding
/// items of different tooltips in its bag.
fn shopper(window: &str) -> (Server, Client) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--shop", "basics", "--window", window],
    );
    // The interface's scale is the game's only once its loading is over.
    super::hud_layout::on_screen(&mut client);
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Gold { amount: 10_000 });
    client.stand_at(MID);
    for item in [
        ItemId::CLARITY,
        ItemId::BLINK_DAGGER,
        ItemId::CHASM_STONE,
        ItemId::IRON_BRANCH,
    ] {
        client.cheat(Cheat::Item { item });
    }
    client.until("the carried items drawn", |c| {
        [ItemId::CLARITY, ItemId::BLINK_DAGGER, ItemId::CHASM_STONE]
            .iter()
            .all(|item| c.slot_of(*item).is_some())
    });
    client.frames(30);
    (server, client)
}

/// The shop cells of these items in the grid, as a pointer place on each (the physical pixel of its middle).
fn grid_cells(client: &mut Client, items: &[ItemId]) -> Vec<Vec2> {
    let world = client.world_mut();
    items
        .iter()
        .filter_map(|item| {
            world
                .query::<(
                    &crate::shop::ShopCell,
                    &crate::shop::Place,
                    &ComputedNode,
                    &bevy::ui::UiGlobalTransform,
                    &InheritedVisibility,
                )>()
                .iter(world)
                .find(|(cell, place, node, _, seen)| {
                    cell.0 == item.0
                        && **place == crate::shop::Place::Grid
                        && seen.get()
                        && node.size().min_element() > 0.0
                })
                .map(|(_, _, _, at, _)| at.translation)
        })
        .collect()
}

/// The tooltips of the shop's items, one after another and after the pointer has been off every
/// item: none is ever drawn anywhere but where it comes to rest.
#[test]
fn a_shop_tooltip_is_never_visible_anywhere_but_where_it_rests() {
    for window in WINDOWS {
        let (_server, mut client) = shopper(window);
        let room = window_rect(&mut client);
        let mut watch = Watch::default();
        let cells = grid_cells(
            &mut client,
            &[
                ItemId::CLARITY,
                ItemId::IRON_BRANCH,
                ItemId::TANGO,
                ItemId::HEALING_SALVE,
                ItemId::CIRCLET,
                ItemId::GAUNTLETS,
            ],
        );
        assert!(cells.len() >= 4, "{window}: {} shop cells", cells.len());
        // Off every item: the pointer over the world, away from the HUD.
        let away = Vec2::new(room.width() * 0.5, room.height() * 0.3);
        let mut hovered = 0;
        for (at, place) in cells.iter().enumerate() {
            // From the pointer off every item (a popover beside a cell may lie over the next cell).
            client.point(away);
            for _ in 0..4 {
                watch.frame(&mut client);
            }
            client.point(*place);
            // A cell the grid's clipping hides from the pointer shows no tooltip: it is not a case.
            let mut shown = false;
            for _ in 0..15 {
                watch.frame(&mut client);
                shown |= watch.tips.last().is_some_and(Option::is_some);
            }
            if !shown {
                continue;
            }
            watch.until_tip_calm(&mut client, &format!("{window}: shop cell {at} at {place}"));
            hovered += 1;
        }
        assert!(
            hovered >= 2,
            "{window}: only {hovered} shop cells showed a tooltip"
        );
        let complaints = watch.complaints(room);
        assert!(
            complaints.is_empty(),
            "{window}:\n{}",
            complaints.join("\n")
        );
    }
}

/// The tooltips of the hero's items and abilities, moving from one straight to the next.
#[test]
fn an_item_or_ability_tooltip_is_never_visible_anywhere_but_where_it_rests() {
    for window in WINDOWS {
        let (_server, mut client) = shopper(window);
        let room = window_rect(&mut client);
        let mut watch = Watch::default();
        let mut places: Vec<(String, Vec2)> = Vec::new();
        for slot in 0..bota_client_core::BAG_SLOTS {
            if client.core().item_in(slot).is_none() {
                continue;
            }
            let boxed = client.item_box(slot).expect("the item's box");
            places.push((
                format!("item slot {slot}"),
                client.centre_of(boxed).expect("placed"),
            ));
        }
        let world = client.world_mut();
        let mut abilities: Vec<(u8, Entity)> = world
            .query::<(Entity, &crate::hud::AbilityBox)>()
            .iter(world)
            .map(|(entity, slot)| (slot.0, entity))
            .collect();
        abilities.sort();
        for (slot, entity) in abilities.into_iter().take(4) {
            places.push((
                format!("ability {slot}"),
                client.centre_of(entity).expect("placed"),
            ));
        }
        assert!(places.len() >= 6, "{window}: {} places", places.len());
        for (label, place) in &places {
            client.point(*place);
            watch.until_tip_calm(&mut client, &format!("{window}: {label} at {place}"));
        }
        let complaints = watch.complaints(room);
        assert!(
            complaints.is_empty(),
            "{window}:\n{}",
            complaints.join("\n")
        );
    }
}

/// The inventory's context menu, opened at each carried item.
#[test]
fn the_inventory_menu_is_never_visible_anywhere_but_where_it_rests() {
    for window in WINDOWS {
        let (_server, mut client) = shopper(window);
        let room = window_rect(&mut client);
        let mut watch = Watch::default();
        for slot in 0..bota_client_core::BAG_SLOTS {
            if client.core().item_in(slot).is_none() {
                continue;
            }
            let boxed = client.item_box(slot).expect("the item's box");
            client.point(client.centre_of(boxed).expect("placed"));
            watch.until_tip_calm(&mut client, &format!("{window}: tooltip at slot {slot}"));
            for state in [ButtonState::Pressed, ButtonState::Released] {
                client.button(MouseButton::Right, state);
                watch
                    .menus
                    .push(seen::<crate::item_menu::Root>(&mut client));
                watch.tips.push(seen::<crate::tips::Tip>(&mut client));
            }
            watch.until_calm(
                &mut client,
                &format!("{window}: menu at slot {slot}"),
                |w| &w.menus,
            );
            // Close it by a click on the world.
            client.point(Vec2::new(2.0, 2.0));
            client.button(MouseButton::Left, ButtonState::Pressed);
            client.button(MouseButton::Left, ButtonState::Released);
            for _ in 0..3 {
                watch.frame(&mut client);
            }
        }
        let complaints = watch.complaints(room);
        assert!(
            complaints.is_empty(),
            "{window}:\n{}",
            complaints.join("\n")
        );
    }
}
