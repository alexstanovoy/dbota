//! The Lotus Pools and their lotuses as Dota shows them, in a match whose server stands the pools
//! up (`--objectives lotus_pools`) and runs its clock fast enough for them to grow: each pool
//! drawn in the map's skin with as many lotuses in it as it holds, a lotus plucked with its burst
//! and sound and one fewer left, and a lotus or a Cheese eaten with the game's sound; the
//! lotuses, Cheese and Royale with Cheese by their icons and names.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{Intent, Slot};
use bota_proto::{Cheat, EntityId, HeroId, ItemId, UnitKind};

use super::harness::{Client, Server};
use crate::units::{ModelBank, Unit};

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(30);
/// Patience for the pools' first lotuses: three game minutes, at twenty times the pace.
const GROWING: Duration = Duration::from_secs(120);

/// A match of one seat with the Lotus Pools, its clock at twenty times Dota's pace, one's
/// Shadow Fiend on the map and the game begun.
fn lotus_match() -> (Server, Client) {
    let server = Server::start(1, &["--objectives", "lotus_pools", "--tick-rate", "600"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    (server, client)
}

/// The Lotus Pools of the view: each one's handle, where it stands and how many lotuses it holds.
fn pools(client: &Client) -> Vec<(EntityId, (i32, i32), u8)> {
    let view = client.view();
    view.pools
        .iter()
        .filter_map(|pool| {
            let unit = view.unit(pool.id)?;
            (unit.kind == UnitKind::LotusPool).then(|| {
                (
                    pool.id,
                    (unit.pos.x.to_int(), unit.pos.y.to_int()),
                    pool.lotuses,
                )
            })
        })
        .collect()
}

/// The model a unit is drawn with, and how many of its lotuses show, once its model is there.
fn drawn_pool(client: &mut Client, id: EntityId) -> Option<(String, usize)> {
    let body = client.body(id)?;
    let world = client.world_mut();
    let unit = world.get::<Unit>(body)?;
    let model = world.resource::<ModelBank>().0[unit.model?]
        .model
        .logical
        .clone();
    world.get::<crate::lotus::PoolLotuses>(body)?;
    // Both pools are drawn: this one's lotuses alone.
    let lotuses: Vec<Entity> = world
        .query::<(Entity, &Name, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, name, seen)| lotus_part(name.as_str()) && seen.get())
        .map(|(part, ..)| part)
        .collect();
    let shown = lotuses
        .into_iter()
        .filter(|part| descends(world, *part, body))
        .count();
    Some((model, shown))
}

/// Whether a node of a model is one of a pool's lotuses (`<model path>.mango<n>`), not a part
/// of one.
fn lotus_part(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, mesh)| mesh.strip_prefix("mango").is_some_and(|n| n.len() == 1))
}

/// Whether `entity` is `root` or under it.
fn descends(world: &World, mut entity: Entity, root: Entity) -> bool {
    loop {
        if entity == root {
            return true;
        }
        match world.get::<ChildOf>(entity) {
            Some(parent) => entity = parent.parent(),
            None => return false,
        }
    }
}

/// How many times a bound sound of a logical name was heard.
fn heard(client: &Client, logical: &str) -> u32 {
    client
        .world()
        .resource::<crate::fx::Heard>()
        .0
        .get(logical)
        .copied()
        .unwrap_or_default()
}

/// How many times the particle effect of a logical name started.
fn started(client: &Client, effect: &str) -> u32 {
    let world = client.world();
    world
        .get_resource::<crate::particles::Library>()
        .and_then(|library| library.effect(effect))
        .map_or(0, |effect| {
            world
                .resource::<crate::particles::Playing>()
                .started(effect)
        })
}

/// Each Lotus Pool is drawn as the map draws it (the Dire's corner's in its own skin) with as
/// many lotuses floating in it as it holds; a hero standing in one plucks a lotus, with the
/// pool's burst and Dota's sound, one fewer floats there, and the lotus it carries, eaten, mends
/// it with the game's sound.
#[test]
fn a_pool_shows_its_lotuses_and_one_plucked_and_eaten_plays_its_effects() {
    let (_server, mut client) = lotus_match();
    client.within(GROWING, "the pools' first lotuses", |c| {
        let pools = pools(c);
        pools.len() == 2 && pools.iter().all(|p| p.2 > 0)
    });
    for (id, at, _) in pools(&client) {
        // The pool of the Dire's corner, top left, is the one the map draws in skin 1.
        let skin = if at.1 > at.0 {
            "objective.lotus_pool.skin1.model"
        } else {
            "objective.lotus_pool.model"
        };
        client.within(SOON, "the pool drawn with its lotuses", |c| {
            let held = pools(c)
                .iter()
                .find(|p| p.0 == id)
                .map(|p| usize::from(p.2));
            drawn_pool(c, id).is_some_and(|(model, shown)| model == skin && Some(shown) == held)
        });
    }
    let (pool, at, _) = pools(&client)[0];
    let before = heard(&client, "sound.lotus_pool.grant");
    client.stand_at(at);
    client.within(SOON, "a lotus plucked", |c| {
        c.slot_of(ItemId::HEALING_LOTUS).is_some()
            && heard(c, "sound.lotus_pool.grant") > before
            && started(c, "lotus_pool.pluck") > 0
    });
    // One fewer floats there, whatever grows meanwhile: the drawn count follows the pool's.
    client.within(SOON, "the pool drawn with what it holds", |c| {
        let held = pools(c)
            .iter()
            .find(|p| p.0 == pool)
            .map(|p| usize::from(p.2));
        drawn_pool(c, pool).is_some_and(|(_, shown)| Some(shown) == held)
    });
    client.stand_at(super::harness::MID);
    let slot = client.slot_of(ItemId::HEALING_LOTUS).expect("the lotus");
    client.within(SOON, "the lotus ready", |c| {
        c.core().item_in(slot).is_some_and(|i| i.cooldown_left == 0)
    });
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.within(SOON, "the lotus eaten", |c| {
        heard(c, "sound.item.mango") > 0 && started(c, "item.mango.active") > 0
    });
}

/// The asset path of an item's icon as the bag draws it.
fn icon_of(client: &mut Client, slot: u8) -> Option<String> {
    let item_box = client.item_box(slot)?;
    let world = client.world_mut();
    let server = world.resource::<AssetServer>().clone();
    let mut parts = world.query::<(&crate::hud::Part, &crate::hud::PartOf, &ImageNode)>();
    parts
        .iter(world)
        .find(|(part, of, _)| of.0 == item_box && matches!(part, crate::hud::Part::Icon))
        .and_then(|(_, _, image)| server.get_path(image.image.id()))
        .map(|path| path.to_string())
}

/// The lotuses, Cheese and Royale with Cheese a cheat hands out are drawn with Dota's icons and
/// named as Dota names them; a Cheese eaten sounds as in Dota.
#[test]
fn lotuses_and_cheese_show_their_icons_and_names_and_cheese_eaten_sounds() {
    let server = Server::start(1, &["--objectives", "lotus_pools"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let items = [
        (ItemId::GREAT_HEALING_LOTUS, "great_famango"),
        (ItemId::GREATER_HEALING_LOTUS, "greater_famango"),
        (ItemId::ROYALE_WITH_CHEESE, "royale_with_cheese"),
        (ItemId::CHEESE, "cheese"),
    ];
    for (item, _) in items {
        client.cheat(Cheat::Item { item });
    }
    for (item, name) in items {
        let slot = client.slot_of(item).expect("the item in the bag");
        let icon = client
            .world()
            .resource::<crate::manifest::Manifest>()
            .file(&format!("icon.item.{name}"))
            .expect("its icon in the cache");
        client.within(SOON, "the item's icon", |c| {
            icon_of(c, slot).as_deref() == Some(icon.as_str())
        });
        let named = client
            .world()
            .resource::<crate::dota_text::Texts>()
            .get(&format!("item_{name}"))
            .map(|entry| entry.name.clone())
            .expect("its name in the cache");
        let item_box = client.item_box(slot).expect("its box");
        let on = client.centre_of(item_box).expect("laid out");
        client.mouse(on);
        // The tooltip's title is the name in capitals, as Dota's is.
        let title = named.to_uppercase();
        client.within(SOON, "its name in the tooltip", |c| {
            c.shown_texts().contains(&title)
        });
    }
    let slot = client.slot_of(ItemId::CHEESE).expect("the Cheese");
    client.within(SOON, "one's hero drawn", |c| c.body(c.me().id).is_some());
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.within(SOON, "the Cheese eaten", |c| {
        heard(c, "sound.item.cheese") > 0
    });
}
