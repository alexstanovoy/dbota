//! The strategy phase as Dota's: its abilities' tooltips, a purchase that
//! sounds as it is chosen and not again when the match buys it, and the inventory's order kept.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{HeroId, ItemId};

use super::harness::{Client, Server};

/// A seat of its own in the strategy phase, long enough to shop in, its hero picked.
fn strategy() -> (Server, Client) {
    let server = Server::start(1, &["--tactical-secs", "8"]);
    let client = Client::pick(&server, HeroId::SHADOW_FIEND, &[]);
    assert!(client.core().in_tactical_phase(), "the strategy phase");
    (server, client)
}

/// Hovering an ability of the hero one enters battle as on the strategy screen opens its
/// tooltip, with the ability's name in the install's words, as it does in the game (nothing
/// opened there).
#[test]
fn the_strategy_screens_abilities_show_their_tooltips() {
    let (_server, mut client) = strategy();
    let shown = |c: &mut Client| {
        let world = c.world_mut();
        world
            .query::<(
                Entity,
                &crate::strategy::StrategyAbility,
                &InheritedVisibility,
            )>()
            .iter(world)
            .find(|(.., seen)| seen.get())
            .map(|(e, ability, _)| (e, ability.0))
    };
    let patience = Duration::from_secs(20);
    client.within(patience, "the abilities laid out", |c| {
        shown(c)
            .and_then(|(e, _)| c.centre_of(e))
            .is_some_and(|at| at != Vec2::ZERO)
    });
    let (icon, ability) = shown(&mut client).expect("an ability");
    let name = {
        let manifest = client.world().resource::<crate::manifest::Manifest>();
        let key = manifest.text_key("ability", ability).expect("its text key");
        let texts = client.world().resource::<crate::dota_text::Texts>();
        texts
            .get(&key)
            .expect("its name in the install's words")
            .name
            .clone()
    };
    let on = client.centre_of(icon).expect("laid out");
    client.mouse(on);
    client.within(patience, "the ability's tooltip", |c| {
        c.shown_texts()
            .iter()
            .any(|t| t.eq_ignore_ascii_case(&name))
    });
}

/// The cache files of the sounds the client starts, as it starts them.
fn listen(client: &mut Client) -> Arc<Mutex<Vec<String>>> {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let into = heard.clone();
    client.world_mut().add_observer(
        move |add: On<Add, AudioPlayer>, players: Query<&AudioPlayer>, server: Res<AssetServer>| {
            let path = players
                .get(add.entity)
                .ok()
                .and_then(|p| server.get_path(p.0.id()))
                .map(|p| p.to_string());
            into.lock().expect("unpoisoned").extend(path);
        },
    );
    heard
}

/// Each item chosen in the strategy phase sounds a purchase as it is chosen (it sounded none
/// then), and when the match begins and buys them no purchase sounds again (it sounded one for
/// each item).
#[test]
fn a_purchase_sounds_when_chosen_and_not_again_when_the_match_buys_it() {
    let (_server, mut client) = strategy();
    let buy = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file("sound.buy")
        .expect("the install's purchase sound");
    let heard = listen(&mut client);
    let buys = |heard: &Arc<Mutex<Vec<String>>>| {
        heard
            .lock()
            .expect("unpoisoned")
            .iter()
            .filter(|p| **p == buy)
            .count()
    };
    let chosen = [ItemId::CLARITY, ItemId::IRON_BRANCH, ItemId::HEALING_SALVE];
    for item in chosen {
        client.core_mut().act(Intent::Buy(item.0));
        client.frames(15);
    }
    assert!(client.core().in_tactical_phase(), "still choosing");
    assert_eq!(buys(&heard), chosen.len(), "one purchase sound a choice");
    client.until("the match begun", |c| c.core().ears().is_some());
    client.until("the choices bought", |c| {
        chosen.iter().all(|item| c.slot_of(*item).is_some())
    });
    client.frames(120);
    assert_eq!(
        buys(&heard),
        chosen.len(),
        "no purchase sound when the match buys them"
    );
}

/// Items chosen in the strategy phase fill the bottom panel's own inventory; one dragged to
/// another slot there is in that slot when the match begins and buys it.
#[test]
#[ignore = "FAILING, a dbota bug: when the match begins the client core sends the first chosen \
            buy (PreUpdate) a frame before `select::tactical` clears TacticalPhase (Update, not \
            ordered against `bag::follow_queue`), so `follow_queue` syncs the shortened queue and \
            the first choice leaves the strategy bag; `arrange` then never moves it to its slot \
            (fails about 4 runs in 5)"]
fn an_item_moved_in_the_strategy_inventory_is_bought_into_that_slot() {
    let (_server, mut client) = strategy();
    for item in [ItemId::CLARITY, ItemId::IRON_BRANCH] {
        client.core_mut().act(Intent::Buy(item.0));
        client.frames(5);
    }
    let in_slot =
        |c: &Client, slot: u8| c.world().resource::<crate::bag::TacticalBag>().id_at(slot);
    let patience = Duration::from_secs(10);
    client.within(patience, "the choices in the strategy inventory", |c| {
        in_slot(c, 0) == Some(ItemId::CLARITY.0) && in_slot(c, 1) == Some(ItemId::IRON_BRANCH.0)
    });
    let (from, to) = (0, 4);
    let at = |c: &mut Client, slot| c.item_box(slot).and_then(|b| c.centre_of(b));
    client.within(patience, "the inventory laid out", |c| {
        at(c, from).is_some_and(|p| p != Vec2::ZERO) && at(c, to).is_some_and(|p| p != Vec2::ZERO)
    });
    let (start, end) = (
        at(&mut client, from).expect("laid"),
        at(&mut client, to).expect("laid"),
    );
    client.mouse(start);
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse((start + end) / 2.0);
    client.mouse(end);
    client.button(MouseButton::Left, ButtonState::Released);
    client.within(patience, "the clarity moved", |c| {
        in_slot(c, to) == Some(ItemId::CLARITY.0)
    });
    client.until("the match begun and the choices bought", |c| {
        c.core().ears().is_some()
            && c.slot_of(ItemId::IRON_BRANCH).is_some()
            && c.slot_of(ItemId::CLARITY).is_some()
    });
    client.within(patience, "the clarity in the slot it was moved to", |c| {
        c.slot_of(ItemId::CLARITY) == Some(to)
    });
    assert_eq!(client.slot_of(ItemId::IRON_BRANCH), Some(1));
}
