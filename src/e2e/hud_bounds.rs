//! One walk over the whole in-match HUD tree: every part that is drawn lies inside the window and
//! inside the part that holds it, and the parts of a row or a column do not lie on one another.
//!
//! It checks no part by name, so a part added tomorrow (or one that slides out of its panel when
//! a layout number changes) is checked as well. The few parts that overhang their panel on
//! purpose carry [`Overhangs`](crate::ui::Overhangs), each with its reason where it is spawned.
//!
//! Three kinds of pairs must not lie on one another (an overlap wider and taller than [`SLACK`]):
//! - siblings that their holder's layout places in flow (a row of slots, a column of bars);
//! - two solid parts (panels that block the pointer, buttons), whichever holders they are in,
//!   unless one holds the other: panels of two docks that met would hide each other's buttons;
//! - an overhanging part and any solid part outside its own holder: it may stand over the part it
//!   decorates (the level ring over the portrait's corner), never over another dock's panel.
//!
//! What is left out, on purpose: a part laid over its holder (fills, badges, overlays) against
//! its siblings, and parts that a cropping holder hides (a scrolled list's rows) on the axis it
//! crops.

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

/// Parts the HUD has at the least: a tree this small is not the HUD (a hidden or empty root).
const MIN_PARTS: usize = 100;
/// Parts that the walk must have reached, by their names.
const ESSENTIAL_PARTS: [&str; 5] = [
    "hud panel",
    "hud level",
    "hud minimap",
    "hud glyph",
    "hud portrait",
];

/// A drawn node of the HUD.
struct Drawn {
    entity: Entity,
    label: String,
    rect: Rect,
    parent: Option<usize>,
    /// The axes (x, y) on which the holder crops what lies outside it.
    cropped: BVec2,
    /// The room that the holders above crop the node to.
    clip: Rect,
    /// What of the node the holders above leave to be seen; None when nothing.
    visible: Option<Rect>,
    /// Whether the node is placed by its parent's layout (not absolutely).
    in_flow: bool,
    /// Whether the node stands outside its holder or over its siblings on purpose.
    overhangs: bool,
    /// Whether the node is a panel that blocks the pointer or a button.
    solid: bool,
    /// Whether the node is turned: the box that holds its corners then reaches past the node's own by
    /// what rounding the drawn corners to pixels can add.
    turned: bool,
}

/// The box a node covers as drawn: the box that holds its corners after the node's own turn.
fn rect_of(world: &World, entity: Entity) -> Option<Rect> {
    let node = world.get::<ComputedNode>(entity)?;
    let at = world.get::<UiGlobalTransform>(entity)?;
    let half = node.size() / 2.0;
    let corners = [
        Vec2::new(-half.x, -half.y),
        Vec2::new(half.x, -half.y),
        Vec2::new(half.x, half.y),
        Vec2::new(-half.x, half.y),
    ]
    .map(|corner| at.translation + at.matrix2 * corner);
    let (low, high) = corners.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(low, high), corner| (low.min(*corner), high.max(*corner)),
    );
    (node.size().min_element() > 0.0).then(|| Rect::from_corners(low, high))
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

/// Every node under the HUD's root, and under every panel registered as a [`HudPanel`](crate::ui::HudPanel)
/// (the shop's when it is open: it stands on its own in the interface), that is shown, with its holder. A shown node that has no size (a wrapper that
/// is only a name for its children) passes its children on to its own holder.
fn drawn_parts(client: &Client) -> Vec<Drawn> {
    let world = client.world();
    let root = world
        .try_query_filtered::<Entity, With<crate::hud::HudRoot>>()
        .expect("the HUD's root is queried")
        .single(world)
        .expect("one HUD root");
    let mut found: Vec<Drawn> = Vec::new();
    let mut pending = vec![(root, None::<usize>, String::from("hud"))];
    pending.extend(
        world
            .try_query_filtered::<(Entity, Option<&Name>), With<crate::ui::HudPanel>>()
            .expect("the registered panels are queried")
            .iter(world)
            .map(|(panel, name)| {
                let name = name.map_or("panel", Name::as_str);
                (panel, None, name.to_owned())
            }),
    );
    // A panel that blocks the pointer and stands under neither the HUD's root nor a registered
    // panel is walked too, so that forgetting to register it hides nothing.
    let known: Vec<Entity> = pending.iter().map(|(entity, ..)| *entity).collect();
    let tops: Vec<(Entity, String)> = world
        .try_query_filtered::<Entity, With<crate::hud::Blocker>>()
        .expect("the blockers are queried")
        .iter(world)
        .filter_map(|blocker| {
            let mut top = blocker;
            while let Some(above) = world.get::<ChildOf>(top) {
                top = above.parent();
            }
            (!known.contains(&top)).then(|| {
                let name = world
                    .get::<Name>(top)
                    .map_or("unregistered panel", Name::as_str);
                (top, name.to_owned())
            })
        })
        .collect();
    for (top, name) in tops {
        if !pending.iter().any(|(entity, ..)| *entity == top) {
            pending.push((top, None, name));
        }
    }
    let mut visited = 0;
    while let Some((entity, parent, path)) = pending.pop() {
        visited += 1;
        assert!(visited < MAX_NODES, "a HUD tree of over {MAX_NODES} nodes");
        let shown = world
            .get::<InheritedVisibility>(entity)
            .is_some_and(|seen| seen.get());
        if !shown {
            continue;
        }
        let label = label_of(world, entity, &path);
        let children = |into: &mut Vec<_>, holder: Option<usize>| {
            if let Some(children) = world.get::<Children>(entity) {
                // An unnamed child is told by its place among its siblings.
                into.extend(
                    children
                        .iter()
                        .enumerate()
                        .map(|(n, c)| (c, holder, format!("{label}/{n}"))),
                );
            }
        };
        let Some(rect) = rect_of(world, entity) else {
            children(&mut pending, parent);
            continue;
        };
        let node = world.get::<Node>(entity);
        let cropped = parent
            .and_then(|parent| world.get::<Node>(found[parent].entity))
            .map_or(BVec2::FALSE, |holder| {
                BVec2::new(
                    holder.overflow.x != OverflowAxis::Visible,
                    holder.overflow.y != OverflowAxis::Visible,
                )
            });
        // What the holders above crop away is not drawn: on the axes a holder crops, its box is the room.
        let clip = parent.map_or(EVERYWHERE, |parent| {
            let holder = &found[parent];
            let own = Rect::from_corners(
                Vec2::new(
                    if cropped.x {
                        holder.rect.min.x
                    } else {
                        f32::MIN
                    },
                    if cropped.y {
                        holder.rect.min.y
                    } else {
                        f32::MIN
                    },
                ),
                Vec2::new(
                    if cropped.x {
                        holder.rect.max.x
                    } else {
                        f32::MAX
                    },
                    if cropped.y {
                        holder.rect.max.y
                    } else {
                        f32::MAX
                    },
                ),
            );
            holder.clip.intersect(own)
        });
        let visible = non_empty(rect.intersect(clip));
        let index = found.len();
        found.push(Drawn {
            entity,
            label: label.clone(),
            rect,
            parent,
            cropped,
            clip,
            visible,
            in_flow: node.is_none_or(|node| node.position_type != PositionType::Absolute),
            overhangs: world.get::<crate::ui::Overhangs>(entity).is_some(),
            turned: world
                .get::<UiGlobalTransform>(entity)
                .is_some_and(|at| at.matrix2.x_axis.y.abs() > 1e-3),
            solid: world.get::<crate::hud::Blocker>(entity).is_some()
                || world.get::<bevy::ui_widgets::Button>(entity).is_some(),
        });
        children(&mut pending, Some(index));
    }
    found
}

const EVERYWHERE: Rect = Rect {
    min: Vec2::splat(f32::MIN),
    max: Vec2::splat(f32::MAX),
};

fn non_empty(rect: Rect) -> Option<Rect> {
    (rect.width() > 0.0 && rect.height() > 0.0).then_some(rect)
}

/// Whether `part` is `ancestor` or lies below it.
fn under(parts: &[Drawn], part: usize, ancestor: usize) -> bool {
    let mut at = Some(part);
    for _ in 0..=parts.len() {
        match at {
            Some(index) if index == ancestor => return true,
            Some(index) => at = parts[index].parent,
            None => return false,
        }
    }
    unreachable!("a HUD tree has no cycle")
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

/// Whether the drawn parts `a` and `b`, neither of which holds the other, lie on one another.
fn lie_on_one_another(parts: &[Drawn], a: usize, b: usize) -> bool {
    let (Some(a_seen), Some(b_seen)) = (parts[a].visible, parts[b].visible) else {
        return false;
    };
    !under(parts, a, b) && !under(parts, b, a) && overlap(a_seen, b_seen)
}

/// What is wrong with the HUD as drawn in a window.
fn complaints(client: &Client, window: Rect, essential: &[&str]) -> Vec<String> {
    let parts = drawn_parts(client);
    assert!(
        parts.len() >= MIN_PARTS,
        "the walk found {} parts in the HUD, where it has hundreds: {:?}",
        parts.len(),
        parts.iter().map(|part| &part.label).collect::<Vec<_>>()
    );
    for essential in ESSENTIAL_PARTS.iter().chain(essential) {
        assert!(
            parts.iter().any(|part| part.label == *essential),
            "the walk never reached the part {essential:?}: the tree it walks is not the HUD"
        );
    }
    let mut all = Vec::new();
    for (at, part) in parts.iter().enumerate() {
        let Some(seen) = part.visible else {
            continue;
        };
        if !contained(seen, window, SLACK) {
            all.push(format!(
                "{} {:?} leaves the window {:?}",
                part.label, part.rect, window
            ));
        }
        let Some(holder) = part.parent.map(|parent| &parts[parent]) else {
            continue;
        };
        // Only the axes the holder does not crop: a scrolled list's rows lie outside it by design.
        let slack = if part.turned { SLACK + 1.5 } else { SLACK };
        let inside = |axis: fn(Vec2) -> f32, cropped: bool| {
            cropped
                || (axis(part.rect.min) >= axis(holder.rect.min) - slack
                    && axis(part.rect.max) <= axis(holder.rect.max) + slack)
        };
        if !part.overhangs && !(inside(|v| v.x, part.cropped.x) && inside(|v| v.y, part.cropped.y))
        {
            all.push(format!(
                "{} {:?} leaves its holder {} {:?}",
                part.label, part.rect, holder.label, holder.rect
            ));
        }
        // Only parts the holder's own layout places side by side can lie on one another by a
        // wrong number; fills, overlays and badges are laid over their holder on purpose.
        for (other_at, other) in parts[..at].iter().enumerate().filter(|(_, other)| {
            other.parent == part.parent
                && other.in_flow
                && part.in_flow
                && !other.overhangs
                && !part.overhangs
        }) {
            if lie_on_one_another(&parts, at, other_at) {
                all.push(format!(
                    "{} {:?} lies on its sibling {} {:?}",
                    part.label, part.rect, other.label, other.rect
                ));
            }
        }
        // Two solid parts, and a part marked as standing over others (a badge, a drawn edge: as
        // turned) over a solid part outside its holder: the docks and their buttons stay apart
        // however the window is cut.
        let stranger = |drawn: usize, solid: usize| {
            parts[drawn].overhangs
                && parts[solid].solid
                && parts[drawn]
                    .parent
                    .is_some_and(|holder| !under(&parts, solid, holder))
        };
        for (other_at, other) in parts[..at].iter().enumerate() {
            if ((part.solid && other.solid) || stranger(at, other_at) || stranger(other_at, at))
                && lie_on_one_another(&parts, at, other_at)
            {
                all.push(format!(
                    "{} {:?} lies on {} {:?}, a solid part it is not placed with",
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
    hud_with(width, height, scale, &[])
}

/// [`hud_in`] with more command-line arguments.
fn hud_with(width: u32, height: u32, scale: f32, extra: &[&str]) -> (Server, Client) {
    let server = Server::for_hud(2, &[]);
    quiet_seat(&server, HeroId::SHADOW_FIEND);
    let size = format!("{width}x{height}");
    let setting = format!("ui_scale={scale}");
    let mut arguments = vec!["--window", &size, "--set", &setting];
    arguments.extend_from_slice(extra);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &arguments);
    client.ready_with_match_held(&server);
    for item in CARRIED {
        client.cheat(Cheat::Item { item });
    }
    client.until("the carried items drawn", |c| {
        CARRIED.iter().all(|item| c.slot_of(*item).is_some())
    });
    // The Glyph and the Scan each hold a charge: their badges are drawn too.
    client.cheat(Cheat::Refresh);
    client.until("a charge of the Glyph and of the Scan", |c| {
        c.core()
            .glyph()
            .and_then(|glyph| glyph.charges)
            .is_some_and(|n| n > 0)
            && c.core().scan().is_some_and(|side| side.ledger.charges > 0)
    });
    client.frames(30);
    (server, client)
}

/// Every drawn part of the HUD lies inside the window and inside its holder, and parts side by side
/// are apart, at every interface scale the settings allow.
fn hud_holds_together_in(width: u32, height: u32) {
    hud_holds_together_with(width, height, &[]);
}

/// [`hud_holds_together_in`] with more command-line arguments (`--shop basics` opens the shop).
fn hud_holds_together_with(width: u32, height: u32, extra: &[&str]) {
    // The shop's panel is a part of the walk when it is open: an empty walk of it would pass.
    let essential: &[&str] = if extra.contains(&"--shop") {
        &["shop panel"]
    } else {
        &[]
    };
    let mut all = Vec::new();
    for scale in SCALES {
        let (_server, mut client) = hud_with(width, height, scale, extra);
        let window = window_rect(&mut client);
        all.extend(
            complaints(&client, window, essential)
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
fn the_hud_holds_together_at_3840x2160() {
    hud_holds_together_in(3840, 2160);
}

#[test]
fn the_hud_holds_together_in_an_ultrawide_window() {
    hud_holds_together_in(3440, 1440);
}

#[test]
fn the_hud_holds_together_in_a_small_window() {
    hud_holds_together_in(1024, 600);
}

/// The shop open besides the HUD: its panel stands clear of the other panels and buttons, as every
/// part of it lies inside its holder.
#[test]
fn the_hud_holds_together_with_the_shop_open() {
    for (width, height) in [
        (1920, 1080),
        (1280, 720),
        (1024, 600),
        (2877, 1620),
        (3440, 1440),
    ] {
        hud_holds_together_with(width, height, &["--shop", "basics"]);
    }
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
