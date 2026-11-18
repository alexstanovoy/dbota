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

/// How far (physical pixels) the context help may be from the place it comes to rest at while the
/// page under it still settles after a resize: its anchor row moves a pixel or two over the next
/// frame. A popup at the place of the old size would be hundreds of pixels off.
const SETTLING: f32 = 4.0;
/// Frames a popover has to hold still for before it counts as at rest.
const CALM: usize = 8;
/// Frames the client may take to show a popover that is wanted (a hang guard, not a duration).
const MOST_FRAMES: usize = 120;
/// Windows: a common one, a small one and one whose interface scale is fractional.
const WINDOWS: [&str; 3] = ["1920x1080", "1280x720", "2877x1642"];

/// What a popover is and where it is drawn on one frame: the words it holds and its rectangle.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Seen {
    words: String,
    rect: Rect,
}

/// The popover `T` as drawn on this frame, if it is visible.
pub(super) fn seen<T: Component>(client: &mut Client) -> Option<Seen> {
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
    shopper_scaled(window, 1.0, "basics")
}

/// [`shopper`] in a window at an interface scale of the settings, with the shop on a tab.
fn shopper_scaled(window: &str, scale: f32, tab: &str) -> (Server, Client) {
    let server = Server::start(1, &[]);
    let setting = format!("ui_scale={scale}");
    let mut client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--shop", tab, "--window", window, "--set", &setting],
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

/// The hero's items and its first four abilities, as a pointer place on each (the physical pixel of its middle).
fn hero_places(client: &mut Client) -> Vec<(String, Vec2)> {
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
    places
}

/// The tooltips of the hero's items and abilities, moving from one straight to the next.
#[test]
fn an_item_or_ability_tooltip_is_never_visible_anywhere_but_where_it_rests() {
    for window in WINDOWS {
        let (_server, mut client) = shopper(window);
        let room = window_rect(&mut client);
        let mut watch = Watch::default();
        let places = hero_places(&mut client);
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

/// A client that started at the menu, joined a match and holds the pointer over its Clarity, the
/// tooltip of which is up.
fn tooltip_in_a_match_from_the_menu() -> (Server, Client, Vec2) {
    let server = Server::start(1, &[]);
    let mut client = Client::run(&[]);
    super::regame::connect(&mut client, &server);
    super::regame::pick(&mut client);
    client.until("the loading over", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    super::hud_layout::on_screen(&mut client);
    client.cheat(Cheat::Item {
        item: ItemId::CLARITY,
    });
    client.until("the Clarity drawn", |c| {
        c.slot_of(ItemId::CLARITY).is_some()
    });
    client.frames(30);
    let slot = client.slot_of(ItemId::CLARITY).expect("the Clarity's slot");
    let boxed = client.item_box(slot).expect("the item's box");
    let place = client.centre_of(boxed).expect("placed");
    client.point(place);
    Watch::default().until_tip_calm(&mut client, "the Clarity's tooltip");
    (server, client, place)
}

fn leave_the_match(client: &mut Client) {
    super::regame::act(client, crate::main_menu::Act::Disconnect);
    super::regame::act(client, crate::main_menu::Act::Answer(true));
}

/// What the tooltip does on each of the next `frames` frames of a client that has left its match:
/// whether the match is gone, and whether a tooltip is drawn.
fn after_leaving(client: &mut Client, frames: usize) -> Vec<(bool, bool)> {
    (0..frames)
        .map(|_| {
            client.step_frames(1);
            let left = !client
                .world()
                .non_send::<crate::main_menu::MenuCore>()
                .in_game();
            (left, seen::<crate::tips::Tip>(client).is_some())
        })
        .collect()
}

/// A tooltip never outlives the screen it belongs to: the one of a match item is gone on the frame
/// the menu takes the match's place.
#[test]
fn a_match_tooltip_is_gone_on_every_frame_after_the_client_leaves_the_match_for_the_menu() {
    let (_server, mut client, _) = tooltip_in_a_match_from_the_menu();
    leave_the_match(&mut client);
    let frames = after_leaving(&mut client, 60);
    assert!(
        frames.iter().any(|(left, _)| *left),
        "the client never reached the menu"
    );
    let stale: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter(|(_, (left, drawn))| *left && *drawn)
        .map(|(frame, _)| frame)
        .collect();
    assert!(
        stale.is_empty(),
        "a tooltip of the match is drawn over the menu on frames {stale:?}"
    );
}

/// ... and over the loading screen of the next match, with the pointer where the old item stood.
#[test]
fn a_match_tooltip_is_not_drawn_while_the_next_match_loads() {
    let (_server, mut client, place) = tooltip_in_a_match_from_the_menu();
    leave_the_match(&mut client);
    client.until("back at the menu", |c| {
        !c.world().non_send::<crate::main_menu::MenuCore>().in_game()
    });
    client.point(place);
    let next = Server::start(1, &[]);
    super::regame::connect(&mut client, &next);
    let mut stale = Vec::new();
    for frame in 0..400 {
        client.step_frames(1);
        let loading = client.world().resource::<crate::loading::Loading>().shown;
        if loading && seen::<crate::tips::Tip>(&mut client).is_some() {
            stale.push(frame);
        }
        // The hero is picked once the seat is told to.
        let core = client.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(Intent::ToggleReady);
        }
        if frame > 20 && client.world().resource::<crate::loading::Loading>().done() {
            break;
        }
    }
    assert!(
        stale.is_empty(),
        "a tooltip of the last match is drawn over the loading screen on frames {stale:?}"
    );
}

/// The tooltips of the hero's items and abilities, moving from one straight to the next: the old
/// one stays until the next is ready at its place, so no frame between them is blank.
#[test]
fn moving_from_one_slot_to_the_next_never_leaves_a_frame_without_a_tooltip() {
    for window in WINDOWS {
        let (_server, mut client) = shopper(window);
        let room = window_rect(&mut client);
        let mut watch = Watch::default();
        let places = hero_places(&mut client);
        assert!(places.len() >= 6, "{window}: {} places", places.len());
        let (first, rest) = places.split_first().expect("places");
        client.point(first.1);
        watch.until_tip_calm(
            &mut client,
            &format!("{window}: {} at {}", first.0, first.1),
        );
        let from = watch.tips.len();
        for (label, place) in rest {
            client.point(*place);
            watch.until_tip_calm(&mut client, &format!("{window}: {label} at {place}"));
        }
        let blank: Vec<usize> = watch.tips[from..]
            .iter()
            .enumerate()
            .filter(|(_, tip)| tip.is_none())
            .map(|(frame, _)| from + frame)
            .collect();
        assert!(
            blank.is_empty(),
            "{window}: no tooltip is drawn on frames {blank:?} between the slots"
        );
        let complaints = watch.complaints(room);
        assert!(
            complaints.is_empty(),
            "{window}:\n{}",
            complaints.join("\n")
        );
    }
}

/// Windows and interface scales at which a tooltip may be taller than the room beside its owner.
const CRAMPED: [(&str, f32); 7] = [
    ("640x360", 1.0),
    ("800x450", 2.0),
    ("1024x600", 1.0),
    ("1024x600", 2.0),
    ("1280x720", 1.5),
    ("1280x720", 2.0),
    ("1920x1080", 2.0),
];

/// The shop's cells on show, every few, as a pointer place on each.
fn some_shop_cells(client: &mut Client) -> Vec<(String, Vec2)> {
    let world = client.world_mut();
    let mut cells: Vec<(u16, Vec2)> = world
        .query::<(
            &crate::shop::ShopCell,
            &crate::shop::Place,
            &ComputedNode,
            &bevy::ui::UiGlobalTransform,
            &InheritedVisibility,
        )>()
        .iter(world)
        .filter(|(_, place, node, _, seen)| {
            **place == crate::shop::Place::Grid && seen.get() && node.size().min_element() > 0.0
        })
        .map(|(cell, _, _, at, _)| (cell.0, at.translation))
        .collect();
    cells.sort_by(|a, b| a.0.cmp(&b.0));
    cells
        .into_iter()
        .step_by(3)
        .take(8)
        .map(|(item, at)| (format!("shop item {item}"), at))
        .collect()
}

/// A tooltip is shown whenever its owner is hovered: on the side with the most room, and then
/// inside the window, however little room that is, and however large the tooltip.
#[test]
fn a_tooltip_is_shown_inside_the_window_wherever_little_room_is_left_for_it() {
    for (window, scale) in CRAMPED {
        let (_server, mut client) = shopper_scaled(window, scale, "upgrades");
        let room = window_rect(&mut client);
        let mut watch = Watch::default();
        let mut places = hero_places(&mut client);
        places.extend(some_shop_cells(&mut client));
        assert!(
            places.len() >= 10,
            "{window} x{scale}: {} places",
            places.len()
        );
        let mut complaints = Vec::new();
        for (label, place) in &places {
            // Off every owner first, so that each case starts without a tooltip.
            client.point(Vec2::new(room.width() * 0.5, room.height() * 0.2));
            for _ in 0..4 {
                watch.frame(&mut client);
            }
            client.point(*place);
            for _ in 0..30 {
                watch.frame(&mut client);
            }
            let Some(Some(now)) = watch.tips.last() else {
                complaints.push(format!("{label} at {place}: no tooltip is shown"));
                continue;
            };
            let (rect, over) = (now.rect, now.rect.size().cmpgt(room.size()));
            // Larger than the window in an axis: the top left corner is inside, the rest is cut.
            let inside = |low: f32, high: f32, room_high: f32, over: bool| {
                low >= 0.0 && (over || high <= room_high)
            };
            if !inside(rect.min.x, rect.max.x, room.max.x, over.x)
                || !inside(rect.min.y, rect.max.y, room.max.y, over.y)
            {
                complaints.push(format!(
                    "{label} at {place}: the tooltip {rect:?} is not inside the window {room:?}"
                ));
            }
        }
        assert!(
            complaints.is_empty(),
            "{window} x{scale}:\n{}",
            complaints.join("\n")
        );
    }
}

/// Resizes the primary window to `width` by `height` physical pixels, as its user does.
fn resize(client: &mut Client, width: u32, height: u32) {
    let world = client.world_mut();
    let id = {
        let (id, mut window) = world
            .query_filtered::<(Entity, &mut Window), With<bevy::window::PrimaryWindow>>()
            .single_mut(world)
            .expect("the window");
        window.resolution.set_physical_resolution(width, height);
        id
    };
    world.write_message(bevy::window::WindowResized {
        window: id,
        width: width as f32,
        height: height as f32,
    });
}

/// The menu's context help, held open by the keyboard, while the window is resized: the popup is
/// drawn on every frame at the place it comes to rest at for the new size, never for a frame at
/// the place it had for the old one.
#[test]
fn context_help_is_never_drawn_at_its_old_place_when_the_window_is_resized() {
    use super::help_widgets::{fonts_ready, tab_to};
    let mut client = Client::run(&["--menu-preview", "play"]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(10),
        ));
    fonts_ready(&mut client);
    client.mouse(Vec2::ZERO);
    tab_to(&mut client, "about solo mid");
    let help = |client: &mut Client| seen::<crate::ui::help::placement::Anchor>(client);
    for _ in 0..60 {
        client.step_frames(1);
    }
    assert!(
        help(&mut client).is_some(),
        "the help is open before the resize"
    );
    let mut complaints = Vec::new();
    for (width, height) in [(1280, 720), (1920, 1080), (1024, 600), (1920, 1080)] {
        resize(&mut client, width, height);
        let frames: Vec<Option<Seen>> = (0..40)
            .map(|_| {
                client.step_frames(1);
                help(&mut client)
            })
            .collect();
        let Some(Some(rest)) = frames.last() else {
            complaints.push(format!(
                "{width}x{height}: the help is gone after the resize"
            ));
            continue;
        };
        for (frame, now) in frames.iter().enumerate() {
            if let Some(now) = now
                && ((now.rect.min - rest.rect.min).abs().max_element() > SETTLING
                    || (now.rect.max - rest.rect.max).abs().max_element() > SETTLING)
            {
                complaints.push(format!(
                    "{width}x{height} frame {frame}: the help is drawn at {:?}, it rests at {:?}",
                    now.rect, rest.rect
                ));
            }
        }
    }
    assert!(complaints.is_empty(), "{}", complaints.join("\n"));
}

/// Whether the pointer is over a thing that has a tooltip: an item, an ability, a shop item, a buff,
/// a talent and the like.
pub(super) fn owner_hovered(client: &mut Client) -> bool {
    use bevy::picking::hover::DirectlyHovered;
    let world = client.world_mut();
    world
        .query_filtered::<&DirectlyHovered, Or<(
            With<crate::hud::ItemBox>,
            With<crate::hud::AbilityBox>,
            With<crate::hud::InnateButton>,
            With<crate::scan::ScanButton>,
            With<crate::tips::AbilityTile>,
            With<crate::tips::TalentTile>,
            With<crate::shop::ShopCell>,
            With<crate::buffs::BuffChip>,
            With<crate::talents::TalentCell>,
            With<crate::talents::AttributeRow>,
        )>>()
        .iter(world)
        .any(|hovered| hovered.get())
}
