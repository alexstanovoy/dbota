//! One walk over the whole in-match HUD tree: every part that is drawn lies inside the window and
//! inside the part that holds it, and the parts of a row or a column do not lie on one another.
//!
//! It checks no part by name, so a part added tomorrow (or one that slides out of its panel when
//! a layout number changes) is checked as well. The few parts that overhang their panel on
//! purpose are [`OVERHANGS`], each with its reason and its measure.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, Server, quiet_seat};

/// What rounding the layout's physical edges to whole pixels may add to an edge.
const SLACK: f32 = 1.0;
/// Nodes the walk visits at most: a HUD of a few thousand, never a runaway tree.
const MAX_NODES: usize = 20_000;

/// What the hero carries, so that every slot of the inventory and the backpack is drawn full, and
/// the Chasm Stone, which Dota files with the neutral items, is one of them.
const CARRIED: [ItemId; 9] = [
    ItemId::CHASM_STONE,
    ItemId::BLINK_DAGGER,
    ItemId::POWER_TREADS,
    ItemId::MAGIC_WAND,
    ItemId::CLARITY,
    ItemId::TANGO,
    ItemId::IRON_BRANCH,
    ItemId::HEALING_SALVE,
    ItemId::OBSERVER_WARD,
];

/// The interface scales the settings allow (`ui_scale` is clamped to 0.5 to 2.0).
const SCALES: [f32; 4] = [0.5, 1.0, 1.5, 2.0];

/// A drawn node of the HUD.
struct Drawn {
    entity: Entity,
    label: String,
    rect: Rect,
    parent: Option<usize>,
    /// Whether the parent crops what lies outside it.
    cropped: bool,
    /// Whether the node is placed by its parent's layout (not absolutely).
    in_flow: bool,
    /// Whether the node stands outside its holder or over its siblings on purpose.
    overhangs: bool,
}

fn rect_of(world: &World, entity: Entity) -> Option<Rect> {
    let node = world.get::<ComputedNode>(entity)?;
    let at = world.get::<UiGlobalTransform>(entity)?;
    (node.size().min_element() > 0.0).then(|| Rect::from_center_size(at.translation, node.size()))
}

fn label_of(world: &World, entity: Entity, path: &str) -> String {
    match world.get::<Name>(entity) {
        Some(name) => name.as_str().to_owned(),
        None => match world.get::<Text>(entity) {
            Some(text) => format!("{path} (text {:?})", text.0),
            None => path.to_owned(),
        },
    }
}

/// Every node under the HUD's root that is shown and laid out, with its holder.
fn drawn_parts(client: &Client) -> Vec<Drawn> {
    let world = client.world();
    let root = world
        .try_query_filtered::<Entity, With<crate::hud::HudRoot>>()
        .expect("the HUD's root is queried")
        .single(world)
        .expect("one HUD root");
    let mut found: Vec<Drawn> = Vec::new();
    let mut pending = vec![(root, None::<usize>, String::from("hud"))];
    while let Some((entity, parent, path)) = pending.pop() {
        assert!(
            found.len() < MAX_NODES,
            "a HUD tree of over {MAX_NODES} nodes"
        );
        let shown = world
            .get::<InheritedVisibility>(entity)
            .is_some_and(|seen| seen.get());
        let Some(rect) = rect_of(world, entity).filter(|_| shown) else {
            continue;
        };
        let node = world.get::<Node>(entity);
        let label = label_of(world, entity, &path);
        let index = found.len();
        found.push(Drawn {
            entity,
            label: label.clone(),
            rect,
            parent,
            cropped: parent.is_some_and(|parent| {
                world
                    .get::<Node>(found[parent].entity)
                    .is_some_and(|holder| holder.overflow != Overflow::visible())
            }),
            in_flow: node.is_none_or(|node| node.position_type != PositionType::Absolute),
            overhangs: world.get::<crate::ui::Overhangs>(entity).is_some(),
        });
        if let Some(children) = world.get::<Children>(entity) {
            // An unnamed child is told by its place among its siblings.
            pending.extend(
                children
                    .iter()
                    .enumerate()
                    .map(|(n, c)| (c, Some(index), format!("{label}/{n}"))),
            );
        }
    }
    found
}

fn contained(inner: Rect, outer: Rect, slack: f32) -> bool {
    inner.min.x >= outer.min.x - slack
        && inner.min.y >= outer.min.y - slack
        && inner.max.x <= outer.max.x + slack
        && inner.max.y <= outer.max.y + slack
}

fn overlap(a: Rect, b: Rect) -> bool {
    let both = a.intersect(b);
    both.width() > SLACK && both.height() > SLACK
}

/// What is wrong with the HUD as drawn in a window.
fn complaints(client: &Client, window: Rect) -> Vec<String> {
    let parts = drawn_parts(client);
    let mut all = Vec::new();
    for (at, part) in parts.iter().enumerate() {
        if !contained(part.rect, window, SLACK) {
            all.push(format!(
                "{} {:?} leaves the window {:?}",
                part.label, part.rect, window
            ));
        }
        let Some(holder) = part.parent.map(|parent| &parts[parent]) else {
            continue;
        };
        if !part.cropped && !part.overhangs && !contained(part.rect, holder.rect, SLACK) {
            all.push(format!(
                "{} {:?} leaves its holder {} {:?}",
                part.label, part.rect, holder.label, holder.rect
            ));
        }
        // Only parts the holder's own layout places side by side can lie on one another by a
        // wrong number; fills, overlays and badges are laid over their holder on purpose.
        for other in parts[..at].iter().filter(|other| {
            other.parent == part.parent
                && other.in_flow
                && part.in_flow
                && !other.overhangs
                && !part.overhangs
        }) {
            if overlap(part.rect, other.rect) {
                all.push(format!(
                    "{} {:?} lies on its sibling {} {:?}",
                    part.label, part.rect, other.label, other.rect
                ));
            }
        }
    }
    all
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

/// The HUD of a hero that carries [`CARRIED`], in a window of `width` by `height` at an interface scale.
fn hud_in(width: u32, height: u32, scale: f32) -> (Server, Client) {
    let server = Server::start(2, &[]);
    quiet_seat(&server, HeroId::SHADOW_FIEND);
    let size = format!("{width}x{height}");
    let setting = format!("ui_scale={scale}");
    let mut client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--window", &size, "--set", &setting],
    );
    super::hud_layout::on_screen(&mut client);
    for item in CARRIED {
        client.cheat(Cheat::Item { item });
    }
    client.until("the carried items drawn", |c| {
        CARRIED.iter().all(|item| c.slot_of(*item).is_some())
    });
    client.frames(30);
    (server, client)
}

/// Every drawn part of the HUD lies inside the window and inside its holder, and parts side by side
/// are apart, at every interface scale the settings allow.
fn hud_holds_together_in(width: u32, height: u32) {
    let mut all = Vec::new();
    for scale in SCALES {
        let (_server, mut client) = hud_in(width, height, scale);
        let window = window_rect(&mut client);
        all.extend(
            complaints(&client, window)
                .into_iter()
                .map(|line| format!("at scale {scale}: {line}")),
        );
    }
    assert!(
        all.is_empty(),
        "in a {width}x{height} window, {} complaints:\n{}",
        all.len(),
        all.join("\n")
    );
}

#[test]
fn the_hud_holds_together_at_1280x720() {
    hud_holds_together_in(1280, 720);
}

#[test]
fn the_hud_holds_together_at_1920x1080() {
    hud_holds_together_in(1920, 1080);
}

#[test]
fn the_hud_holds_together_at_2877x1620() {
    hud_holds_together_in(2877, 1620);
}

#[test]
fn the_hud_holds_together_in_an_ultrawide_window() {
    hud_holds_together_in(3440, 1440);
}

#[test]
fn the_hud_holds_together_in_a_small_window() {
    hud_holds_together_in(1024, 600);
}

/// The neutral item's slot stands inside the hero panel, level with the inventory's first row: it
/// stood out of the interface, over the panel's top edge.
#[test]
fn the_neutral_item_slot_stands_inside_the_hero_panel_level_with_the_first_row_of_items() {
    for (width, height) in [(1920, 1080), (1280, 720)] {
        let (_server, mut client) = hud_in(width, height, 1.0);
        let world = client.world_mut();
        let named = |world: &mut World, name: &str| {
            world
                .query::<(Entity, &Name)>()
                .iter(world)
                .find(|(_, n)| n.as_str() == name)
                .map(|(e, _)| e)
                .unwrap_or_else(|| panic!("{name} is in the HUD"))
        };
        let (panel, neutral) = (named(world, "hud panel"), named(world, "hud neutral"));
        let first = world
            .query::<(Entity, &crate::hud::ItemBox)>()
            .iter(world)
            .find(|(_, slot)| slot.0 == 0)
            .map(|(e, _)| e)
            .expect("the first item box");
        let rect = |entity| rect_of(client.world(), entity).expect("laid out");
        let (panel, neutral, first) = (rect(panel), rect(neutral), rect(first));
        assert!(
            contained(neutral, panel, SLACK),
            "{width}x{height}: the neutral slot {neutral:?} is not inside the hero panel {panel:?}"
        );
        assert!(
            (neutral.min.y - first.min.y).abs() <= 2.0,
            "{width}x{height}: the neutral slot {neutral:?} is not level with the first item {first:?}"
        );
    }
}
