//! The popups that open at the pointer in the shop's corner, where the screen's right and bottom
//! edges are near: the inventory's context menu and the item's tooltip. Each lies wholly inside the
//! window and keeps one place for as long as the pointer rests.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{BAG_SLOTS, Intent};
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, MID, Server};

/// Frames a popup must hold still for.
const FRAMES: usize = 12;

/// Windows of the sizes people play in, down to one where the menu is wider than the room left of
/// the stash.
const WINDOWS: [&str; 5] = ["1920x1080", "2877x1642", "1280x720", "1024x600", "800x500"];

/// A node's rectangle in the window's physical pixels, if it is drawn.
fn drawn<T: Component>(client: &mut Client) -> Option<Rect> {
    let world = client.world_mut();
    world
        .query_filtered::<(
            &bevy::ui::UiGlobalTransform,
            &ComputedNode,
            &InheritedVisibility,
        ), With<T>>()
        .iter(world)
        .find(|(_, node, seen)| seen.get() && node.size() != Vec2::ZERO)
        .map(|(at, node, _)| Rect::from_center_size(at.translation, node.size()))
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

/// Runs frames with the pointer at rest and returns what each popup drew on every one of them.
fn rest(client: &mut Client) -> Vec<(Option<Rect>, Option<Rect>)> {
    (0..FRAMES)
        .map(|_| {
            client.frames(1);
            (
                drawn::<crate::item_menu::Root>(client),
                drawn::<crate::tips::Tip>(client),
            )
        })
        .collect()
}

fn inside(room: Rect, rect: Rect) -> bool {
    room.contains(rect.min) && room.contains(rect.max)
}

fn stashed(window: &str) -> (Server, Client) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--shop", "basics", "--window", window],
    );
    client.core_mut().act(Intent::SelectHero);
    client.cheat(Cheat::Gold { amount: 10_000 });
    client.stand_at(MID);
    for count in 1..=bota_core::rules::STASH_SLOTS {
        client.core_mut().act(Intent::Buy(ItemId::IRON_BRANCH.0));
        client.until("another branch in the stash", |c| {
            (0..bota_core::rules::STASH_SLOTS as u8)
                .filter(|at| c.core().item_in(BAG_SLOTS + at).is_some())
                .count()
                == count
        });
    }
    (server, client)
}

/// The rectangle of a node drawn, in the window's physical pixels.
fn rect_of(client: &Client, node: Entity) -> Rect {
    let world = client.world();
    let size = world.get::<ComputedNode>(node).expect("laid out").size();
    let at = world
        .get::<bevy::ui::UiGlobalTransform>(node)
        .expect("placed")
        .translation;
    Rect::from_center_size(at, size)
}

fn overlap(a: Rect, b: Rect) -> bool {
    let both = a.intersect(b);
    both.width() > 0.0 && both.height() > 0.0
}

/// Holds the pointer on a node, at the middle and at the pixel nearest the window's bottom right corner,
/// and checks the popup the item shows: present on every frame (a popup that is absent is no popup
/// that held still), inside the window, off the node it is about, and in the same place on every frame.
fn tooltip_holds_still(client: &mut Client, what: &str, node: Entity, room: Rect) {
    let box_ = rect_of(client, node);
    for at in [box_.center(), box_.max - Vec2::splat(1.0)] {
        client.mouse(at);
        let tips: Vec<Option<Rect>> = rest(client).into_iter().map(|(_, tip)| tip).collect();
        let first = tips[0].unwrap_or_else(|| panic!("{what} {at}: no tooltip is shown"));
        assert!(
            first.size().min_element() > 0.0,
            "{what} {at}: an empty tooltip"
        );
        for (n, tip) in tips.iter().enumerate() {
            assert_eq!(
                *tip,
                Some(first),
                "{what} {at}: the tooltip is gone or moved on frame {n}"
            );
        }
        assert!(
            inside(room, first),
            "{what} {at}: tooltip {first:?} leaves {room:?}"
        );
        assert!(
            !overlap(first, box_),
            "{what} {at}: tooltip {first:?} covers the item {box_:?}"
        );
    }
}

/// The item tooltips of the stash, the bag and the shop, at every window size: visible, inside, off their
/// item and still, at the fractional interface scales (1.2 at 1920 by 1080, 1.52 at 2877 by 1642) where the
/// popup's measured height rounds up and down.
#[test]
fn the_tooltip_of_a_hovered_item_is_shown_inside_the_window_off_its_item_and_holds_still() {
    for window in WINDOWS {
        let (_server, mut client) = stashed(window);
        let room = window_rect(&mut client);
        for slot in (0..1).chain(BAG_SLOTS..BAG_SLOTS + bota_core::rules::STASH_SLOTS as u8) {
            let Some(boxed) = client.item_box(slot) else {
                continue;
            };
            if client.core().item_in(slot).is_none() {
                continue;
            }
            client.hover_ui(boxed, Duration::from_secs(20));
            tooltip_holds_still(&mut client, &format!("{window} slot {slot}"), boxed, room);
        }
        // A shop item, in the grid the shop opens on.
        let world = client.world_mut();
        let cell = world
            .query::<(
                Entity,
                &crate::shop::ShopCell,
                &crate::shop::Place,
                &ComputedNode,
                &InheritedVisibility,
            )>()
            .iter(world)
            .find(|(_, cell, place, node, seen)| {
                cell.0 == ItemId::CLARITY.0
                    && **place == crate::shop::Place::Grid
                    && seen.get()
                    && node.size().min_element() > 0.0
            })
            .map(|(entity, ..)| entity)
            .expect("Clarity in the shop's grid");
        client.hover_ui(cell, Duration::from_secs(20));
        tooltip_holds_still(
            &mut client,
            &format!("{window} Clarity in the shop"),
            cell,
            room,
        );
    }
}

/// Right-clicks the stash's slots at their corners nearest the screen's corner and holds still: the
/// menu is inside the window on every frame and does not move between frames.
#[test]
fn the_context_menu_by_the_stash_stays_inside_the_window_and_holds_still() {
    for window in WINDOWS {
        let (_server, mut client) = stashed(window);
        let room = window_rect(&mut client);
        for slot in BAG_SLOTS..BAG_SLOTS + bota_core::rules::STASH_SLOTS as u8 {
            let boxed = client.item_box(slot).expect("the stash slot is drawn");
            client.hover_ui(boxed, Duration::from_secs(20));
            let centre = client.centre_of(boxed).expect("placed");
            let half = client
                .world()
                .get::<ComputedNode>(boxed)
                .expect("laid out")
                .size()
                / 2.0;
            for at in [centre, centre + half - Vec2::splat(1.0)] {
                client.mouse(at);
                client.click(MouseButton::Right);
                let frames = rest(&mut client);
                let menu = frames[0]
                    .0
                    .unwrap_or_else(|| panic!("{window} slot {slot} {at}: no menu"));
                for (n, (root, _)) in frames.iter().enumerate() {
                    assert_eq!(
                        *root,
                        Some(menu),
                        "{window} slot {slot} {at}: the menu moved on frame {n}"
                    );
                }
                assert!(
                    inside(room, menu),
                    "{window} slot {slot} {at}: menu {menu:?} leaves {room:?}"
                );
                client.mouse(Vec2::new(2.0, 2.0));
                client.click(MouseButton::Left);
                client.frames(2);
            }
        }
    }
}
