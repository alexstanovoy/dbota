//! A hero's death and rise as the client shows it: the top bar's portrait
//! washed grey while the hero is down, the hero still selected and grouped when it rises, and
//! black while it is untouchable at its fountain.

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{Cheat, EffectId, HeroId, Team};

use super::harness::{Client, Server};

/// Whether the top bar shows one's portrait washed of its colour (the dead seat's cover).
fn grey_portrait(client: &mut Client) -> bool {
    let world = client.world_mut();
    let core = &world.non_send::<crate::session::Session>().0;
    let (Some(slot), Some(view)) = (core.my_slot, core.view()) else {
        return false;
    };
    // The top bar's seats are the view's players in order; a seat wears the hero's wide
    // portrait.
    let Some(seat) = view.players.iter().position(|p| p.slot == slot) else {
        return false;
    };
    let team = view.players[seat].team;
    let place = view.players[..seat]
        .iter()
        .filter(|p| p.team == team)
        .count();
    let manifest = world.resource::<crate::manifest::Manifest>();
    let Some(portrait) = manifest
        .dota_name("hero", view.players[seat].hero.0)
        .and_then(|name| manifest.file(&format!("portrait.hero.{name}")))
    else {
        return false;
    };
    let server = world.resource::<AssetServer>().clone();
    let covers: Vec<(Handle<crate::wash::Washed>, Display)> =
        crate::hud::seat_death_covers(world, team, place as u8)
            .into_iter()
            .filter_map(|cover| {
                let material = world.get::<MaterialNode<crate::wash::Washed>>(cover)?;
                Some((material.0.clone(), world.get::<Node>(cover)?.display))
            })
            .collect();
    let washes = world.resource::<Assets<crate::wash::Washed>>();
    covers.iter().any(|(material, display)| {
        let Some(wash) = washes.get(material) else {
            return false;
        };
        *display != Display::None
            && wash.levels.x == 0.0
            && server
                .get_path(wash.picture.id())
                .is_some_and(|p| p.to_string() == portrait)
    })
}

/// The base colours of the materials one's hero's model is drawn with, part by part; none
/// while the model is not drawn yet (a risen hero is a new unit, and its body and parts spawn
/// frames after the snapshot that tells of it, as many as the machine's load makes them).
fn hero_colours(client: &mut Client) -> Vec<LinearRgba> {
    let id = client.me().id;
    let Some(body) = client.body(id) else {
        return Vec::new();
    };
    let world = client.world_mut();
    let handles: Vec<Handle<crate::fog::FoggedMaterial>> = world
        .query::<(
            &crate::fog::HeroPart,
            &MeshMaterial3d<crate::fog::FoggedMaterial>,
        )>()
        .iter(world)
        .filter(|(part, _)| part.0 == body)
        .map(|(_, material)| material.0.clone())
        .collect();
    let materials = world.resource::<Assets<crate::fog::FoggedMaterial>>();
    handles
        .iter()
        .filter_map(|h| materials.get(h))
        .map(|m| m.base.base_color.to_linear())
        .collect()
}

fn untouchable(client: &Client) -> bool {
    client
        .me()
        .effects
        .iter()
        .any(|e| e.id == EffectId::FOUNTAIN_INVULNERABLE)
}

/// One's hero dies at the enemy's fountain: the top bar shows its portrait washed grey while
/// it is down, and the inventory still shows what it carries (a dead hero may swap it, as in
/// Dota); it rises the same unit, still selected and still in its control group; untouchable
/// at its fountain its model is tinted black, and its colours come back with its first
/// command.
#[test]
fn a_hero_lies_grey_rises_selected_and_is_black_at_its_fountain() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    assert!(!grey_portrait(&mut client), "alive, in colour");
    client.core_mut().act(Intent::SelectHero);
    client.key(KeyCode::ControlLeft, ButtonState::Pressed);
    client.key(KeyCode::Digit1, ButtonState::Pressed);
    client.key(KeyCode::Digit1, ButtonState::Released);
    client.key(KeyCode::ControlLeft, ButtonState::Released);
    client.cheat(Cheat::Item {
        item: bota_proto::ItemId::BOOTS,
    });
    client.until("the boots carried", |c| {
        c.slot_of(bota_proto::ItemId::BOOTS).is_some()
    });
    let boots = client.slot_of(bota_proto::ItemId::BOOTS);
    let fallen = client.me().id;
    let map = bota_core::map_of(bota_core::MAP2_ID);
    let theirs = bota_core::fountain_pos(map, Team::Dire);
    client.cheat(Cheat::Teleport { at: theirs });
    client.until("the hero fallen to the fountain", |c| {
        c.core().my_standing_hero().is_none()
    });
    let patience = std::time::Duration::from_secs(20);
    client.within(patience, "the dead seat shown", grey_portrait);
    assert_eq!(
        client.core().my_hero(),
        Some(fallen),
        "the dead hero's handle"
    );
    assert_eq!(
        client.slot_of(bota_proto::ItemId::BOOTS),
        boots,
        "the dead hero's inventory shown"
    );
    client.cheat(Cheat::Respawn);
    client.until("risen", |c| c.core().my_standing_hero() == Some(fallen));
    client.until("risen, in colour", |c| !grey_portrait(c));
    let risen = client.me().id;
    assert_eq!(risen, fallen, "the same hero rises");
    assert_eq!(
        client.core().selected,
        Some(risen),
        "the risen hero selected"
    );
    client.core_mut().act(Intent::SelectUnit(None));
    client.key(KeyCode::Digit1, ButtonState::Pressed);
    client.key(KeyCode::Digit1, ButtonState::Released);
    assert_eq!(
        client.core().selected,
        Some(risen),
        "the group holds the risen hero"
    );
    assert!(
        untouchable(&client),
        "untouchable at its fountain after rising"
    );
    client.until("its model drawn", |c| !hero_colours(c).is_empty());
    let black = hero_colours(&mut client);
    client.core_mut().act(Intent::MoveTo((1000.0, 1000.0)));
    client.until("touchable again", |c| !untouchable(c));
    client.until("its colours back", |c| {
        let now = hero_colours(c);
        now.len() == black.len() && now != black
    });
    let coloured = hero_colours(&mut client);
    assert_eq!(black.len(), coloured.len());
    for (b, c) in black.iter().zip(&coloured) {
        let tinted = [b.red, b.green, b.blue];
        let full = [c.red, c.green, c.blue];
        for (t, f) in tinted.iter().zip(full) {
            assert!((t - f * 0.08).abs() < 1e-4, "black: {b:?} against {c:?}");
        }
    }
    assert!(
        coloured.iter().any(|c| c.red + c.green + c.blue > 0.3),
        "colours back"
    );
}
