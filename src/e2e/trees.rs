//! A Tango on a tree as Dota shows it: the tree under the cursor ringed in
//! the hover yellow and named by the click; felled, it tips over and goes; regrown, it grows
//! up over its stump; and every tree the match places stands in the forest as the model the map
//! gives it.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{Intent, Slot};
use bota_proto::{Cheat, HeroId, ItemId};

use super::harness::{Client, MID, Server, ring_of};

/// The tree drawn alone at a spot while it falls or grows: its turn and its size.
fn alone_at(client: &mut Client, at: Vec2) -> Option<(Quat, f32)> {
    let world = client.world_mut();
    world
        .query_filtered::<&Transform, (With<crate::fog::Fogged>, With<Mesh3d>)>()
        .iter(world)
        .find(|t| Vec2::new(t.translation.x, -t.translation.z).distance(at) < 1.0)
        .map(|t| (t.rotation, t.scale.x))
}

/// A tree of the match near the middle of the mid lane that the map's file plants as a
/// tree_pine_01, a model the cache has.
fn pine_near_mid(client: &Client) -> Vec2 {
    let root = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .root
        .clone()
        .expect("the asset cache");
    let text = std::fs::read_to_string(root.join("map/mid/trees.txt")).expect("the map's trees");
    let mid = Vec2::new(MID.0 as f32, MID.1 as f32);
    // Dota's coordinates, bota's less 9216 on each axis.
    let pine = text
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next()? == "tree_pine_01").then_some(())?;
            let x: f32 = words.next()?.parse().ok()?;
            let y: f32 = words.next()?.parse().ok()?;
            Some(Vec2::new(x + 9216.0, y + 9216.0))
        })
        .min_by(|a, b| a.distance(mid).total_cmp(&b.distance(mid)))
        .expect("a pine");
    client
        .core()
        .trees()
        .iter()
        .map(|at| Vec2::new(at.x.to_f32(), at.y.to_f32()))
        .find(|at| at.distance(pine) < 48.0)
        .expect("the pine is a tree of the match")
}

/// With a Tango taken up, the pointer on a tree rings that tree in the hover yellow and the
/// click names it: the hero eats it, and the tree tips over on its own and goes; three minutes
/// on it grows back up from nothing over its stump.
#[test]
fn a_tango_rings_its_tree_and_the_tree_falls_and_grows_back() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let pointed = pine_near_mid(&client);
    client.stand_at((pointed.x as i32 + 250, pointed.y as i32));
    client.cheat(Cheat::Item {
        item: ItemId::TANGO,
    });
    let slot = client.slot_of(ItemId::TANGO).expect("the tango in the bag");
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    let pixel = client
        .screen_of((pointed.x, pointed.y))
        .expect("the tree on screen");
    client.mouse(pixel);
    let patience = Duration::from_secs(10);
    let target = |c: &Client| c.world().resource::<crate::trees::TreeTarget>().0;
    client.within(patience, "a tree under the cursor", |c| target(c).is_some());
    // The tree in front of the one pointed at may be the one the cursor is on.
    let (index, tree) = target(&client)
        .map(|(i, at)| (i, Vec2::new(at.x.to_f32(), at.y.to_f32())))
        .expect("a tree");
    assert!(
        tree.distance(pointed) < 150.0,
        "the tree pointed at, or one in front of it: {index} at {tree}"
    );
    client.frames(2);
    let ringed = client
        .line_strips()
        .into_iter()
        .filter(|(p, c)| *c == crate::pick::HOVER_RING.to_linear() && p.len() > 8)
        .map(|(p, _)| ring_of(&p))
        .any(|(middle, _)| middle.distance(tree) < 2.0);
    assert!(ringed, "the tree ringed in the hover yellow");
    client.click(MouseButton::Left);
    client.within(patience, "the tree tipping over", |c| {
        alone_at(c, tree).is_some_and(|(turn, _)| turn.angle_between(Quat::IDENTITY) > 0.0)
    });
    let first = alone_at(&mut client, tree).expect("falling").0;
    client.frames(10);
    if let Some((later, _)) = alone_at(&mut client, tree) {
        assert!(later != first, "it tips on");
    }
    client.within(patience, "the fallen tree gone", |c| {
        alone_at(c, tree).is_none()
    });
    // The match's clock on three minutes: the tree grows back.
    let mut sizes = Vec::new();
    client.until("the tree growing back", |c| {
        let size = alone_at(c, tree).map(|(_, size)| size);
        sizes.extend(size);
        sizes.len() > 3
    });
    assert!(
        sizes.windows(2).all(|w| w[1] >= w[0]),
        "it grows up: {sizes:?}"
    );
    assert!(sizes[0] < sizes[sizes.len() - 1], "from small: {sizes:?}");
}

/// The forest draws every tree of the match, and each of the map's tree models stands in it: the
/// bamboo, `tree_pine_03b` and `tree_cine_02_low` the importer once left out were never drawn
/// (168 of the map's 2307 trees), though they block paths and a Tango can name them.
#[test]
fn every_tree_the_match_places_is_drawn_as_the_model_of_the_map() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.until("the forest built", |c| {
        c.world().resource::<crate::trees::SceneryReady>().0
    });
    let root = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .root
        .clone()
        .expect("the asset cache");
    let text = std::fs::read_to_string(root.join("map/mid/trees.txt")).expect("the map's trees");
    let mapped: std::collections::BTreeSet<&str> = text
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    let drawn = crate::trees::drawn(client.world());
    let placed = client.core().trees().len();
    let missing: Vec<usize> = (0..placed)
        .filter(|i| {
            drawn
                .binary_search_by_key(&(*i as u32), |(index, _)| *index)
                .is_err()
        })
        .collect();
    assert!(
        missing.is_empty(),
        "{} of {placed} trees have no model drawn, such as the tree at {:?}",
        missing.len(),
        missing.first().map(|i| client.core().trees()[*i])
    );
    let models: std::collections::BTreeSet<&str> =
        drawn.iter().map(|(_, model)| model.as_str()).collect();
    let absent: Vec<_> = mapped.difference(&models).collect();
    assert!(absent.is_empty(), "map models never drawn: {absent:?}");
}
