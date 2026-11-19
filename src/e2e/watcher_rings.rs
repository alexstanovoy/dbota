//! The ring under a selected Watcher is marked by whose the Watcher is for the player now: a
//! side's own colour once it is lit for that side, the neutral colour while it is lit for none,
//! and it changes with the Watcher's owner while the Watcher stays selected.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{EffectId, EntityId, HeroId, Team, UnitKind, Vec2 as Spot};

use super::harness::{Client, Server, ring_of};
use super::watcher_colours::radiant_lights;
use crate::overlay::team_color;

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(60);

/// The ring colour of a selected unit of a side.
fn selection(side: Team) -> Color {
    team_color(side).with_alpha(0.9)
}

/// The centres of the rings drawn this frame in a colour.
fn rings(client: &Client, colour: Color) -> Vec<Vec2> {
    let colour = colour.to_linear();
    client
        .line_strips()
        .into_iter()
        .filter(|(points, c)| *c == colour && points.len() > 8)
        .map(|(points, _)| ring_of(&points).0)
        .collect()
}

/// Whether a ring of the colour is drawn round the Watcher.
fn ringed(client: &Client, colour: Color, watcher: &bota_proto::UnitView) -> bool {
    let at = Vec2::new(watcher.pos.x.to_f32(), watcher.pos.y.to_f32());
    rings(client, colour).iter().any(|c| c.distance(at) < 5.0)
}

fn watcher_at(client: &Client, at: Spot) -> bota_proto::UnitView {
    client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        .expect("the watcher in the view")
        .clone()
}

/// Stands beside a Watcher and clicks on it, as a player selects it.
fn select_by_click(client: &mut Client, at: Spot) -> EntityId {
    client.stand_at((at.x.to_int() + 100, at.y.to_int()));
    let watcher = watcher_at(client, at);
    let on = client
        .screen_of((at.x.to_f32(), at.y.to_f32()))
        .expect("the watcher on the screen");
    client.mouse(on);
    client.click(MouseButton::Left);
    client.within(SOON, "the watcher selected", |c| {
        c.core().selected == Some(watcher.id)
    });
    client.frames(2);
    watcher.id
}

fn lit(client: &Client, id: EntityId) -> bool {
    client
        .view()
        .unit(id)
        .is_some_and(|u| u.effects.iter().any(|e| e.id == EffectId::VISION_GRANT))
}

/// Three Watchers, clicked in turn by a Dire player: one lit for no side, one the Radiant lit
/// and one the player's own hero captures while it is selected. Each ring is its owner's colour,
/// the last changing from the neutral one to the player's own as the capture begins.
#[test]
fn a_selected_watcher_is_ringed_as_it_stands_to_the_player_and_follows_its_owner() {
    let server = Server::start(2, &["--map", "0", "--objectives", "watchers"]);
    let theirs = bota_core::DOTA_WATCHERS[4];
    let ours = bota_core::DOTA_WATCHERS[3];
    let nobodys = bota_core::DOTA_WATCHERS[0];
    radiant_lights(&server, theirs);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    assert_eq!(client.core().my_team(), Some(Team::Dire), "one's side");
    client.cheat(bota_proto::Cheat::StartGame);

    let id = select_by_click(&mut client, nobodys);
    let watcher = client.view().unit(id).expect("seen").clone();
    assert!(
        ringed(&client, selection(Team::Neutral), &watcher),
        "a Watcher lit for none is ringed in the neutral colour"
    );

    let id = select_by_click(&mut client, theirs);
    client.until("the Radiant lights its watcher", |c| lit(c, id));
    client.frames(2);
    let watcher = client.view().unit(id).expect("seen").clone();
    assert!(
        ringed(&client, selection(Team::Radiant), &watcher),
        "a Watcher the enemy lit is ringed as an enemy"
    );

    let id = select_by_click(&mut client, ours);
    let watcher = client.view().unit(id).expect("seen").clone();
    assert!(
        ringed(&client, selection(Team::Neutral), &watcher),
        "one's Watcher not yet lit is neutral"
    );
    client.core_mut().act(Intent::Smart {
        at: (ours.x.to_f32(), ours.y.to_f32()),
        unit: Some(id),
    });
    client.within(
        SOON,
        "the ring turning to one's own while the lamp is lit",
        |c| {
            let watcher = c.view().unit(id).expect("seen").clone();
            ringed(c, selection(Team::Dire), &watcher)
        },
    );
    assert!(
        !lit(&client, id),
        "the ring is one's own while the Watcher is being captured, before it is lit"
    );
    client.until("one's own hero lights it", |c| lit(c, id));
    client.frames(2);
    let watcher = client.view().unit(id).expect("seen").clone();
    assert!(
        ringed(&client, selection(Team::Dire), &watcher),
        "a Watcher one's side lit is ringed as one's own creep is"
    );
    assert!(
        !ringed(&client, selection(Team::Neutral), &watcher),
        "no neutral ring is left under it"
    );
    assert_eq!(client.core().selected, Some(id), "it stayed selected");
}
