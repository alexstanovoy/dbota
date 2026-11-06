//! The Watchers as Dota shows whom they light for: lit, each wears the colours of that side,
//! the Radiant's teal or the Dire's red, on its lamp and its eyes, and is marked in that side's
//! colour on the minimap, to both sides alike; and a hero channelling at one plays the channel's
//! effect.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{Intent, IssuedOrder};
use bota_proto::{Cheat, EffectId, EntityId, HeroId, Order, Target, Team, UnitKind, Vec2};

use super::harness::{Client, Server, scripted_seat};
use crate::particles::{Library, Playing};
use crate::units::{ModelBank, Unit};

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(60);

/// A Radiant seat that lights the Watcher at `at` once the game begins.
fn radiant_lights(server: &Server, at: Vec2) {
    let mut stage = 0;
    scripted_seat(server, HeroId::PUDGE, move |session| {
        let Some(view) = session.current() else {
            return Vec::new();
        };
        let Some(watcher) = view
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        else {
            return Vec::new();
        };
        let order = match stage {
            0 => Order::Cheat {
                cheat: Cheat::Teleport {
                    at: at + Vec2::from_ints(-100, 0),
                },
            },
            1 if view.game_tick() >= session.statics().metadata().pregame_ticks => Order::Attack {
                target: Target::Unit(watcher.id),
            },
            _ => return Vec::new(),
        };
        stage += 1;
        vec![IssuedOrder::hero(order)]
    });
}

/// The Watcher of the view standing at a place of the map.
fn watcher_at(client: &Client, at: Vec2) -> EntityId {
    client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        .expect("the watcher in the view")
        .id
}

/// Whether the Watcher is lit.
fn lit(client: &Client, id: EntityId) -> bool {
    client
        .view()
        .unit(id)
        .is_some_and(|u| u.effects.iter().any(|e| e.id == EffectId::VISION_GRANT))
}

/// The model a unit is drawn with, once it is drawn.
pub(super) fn drawn_model(client: &Client, id: EntityId) -> Option<String> {
    let unit = client.world().get::<Unit>(client.body(id)?)?;
    Some(
        client.world().resource::<ModelBank>().0[unit.model?]
            .model
            .logical
            .clone(),
    )
}

/// The side each playing instance of the lit lamp's effect is coloured for: its control
/// point 12, 0 the Radiant's teal, 1 the Dire's orange.
pub(super) fn lamp_colours(client: &Client) -> Vec<f32> {
    lamp_fed(client, 12)
}

/// The radius each playing instance of the lit lamp's effect draws its sight ring at: its
/// control point 11.
fn lamp_sights(client: &Client) -> Vec<f32> {
    lamp_fed(client, 11)
}

/// What control point `cp` of each playing instance of the lit lamp's effect is fed, in x,
/// sorted.
fn lamp_fed(client: &Client, cp: usize) -> Vec<f32> {
    let world = client.world();
    let effect = world
        .resource::<Library>()
        .effect("watcher.controlled")
        .expect("watcher.controlled in the asset cache");
    let mut colours: Vec<f32> = world
        .resource::<Playing>()
        .instances_of(effect)
        .filter(|instance| !instance.ending())
        .map(|instance| instance.nodes[0].control_points[cp].position.x)
        .collect();
    colours.sort_by(f32::total_cmp);
    colours
}

/// The other side lights one Watcher and one's own Dire hero another: each is drawn in its
/// side's material group, and its lamp effect is coloured for that side, the other side's as
/// well as its own; one's hero's channel plays the Watcher's channel effect.
#[test]
fn a_lit_watcher_wears_the_colours_of_the_side_it_lights_for() {
    let server = Server::start(2, &["--map", "0", "--objectives", "watchers"]);
    let theirs = bota_core::DOTA_WATCHERS[4];
    let ours = bota_core::DOTA_WATCHERS[3];
    radiant_lights(&server, theirs);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    assert_eq!(client.core().my_team(), Some(Team::Dire), "one's side");
    client.cheat(Cheat::StartGame);
    client.stand_at((theirs.x.to_int() + 300, theirs.y.to_int()));
    let id = watcher_at(&client, theirs);
    client.until("the Radiant lights its watcher", |c| lit(c, id));
    client.within(SOON, "their watcher drawn in the Radiant's skin", |c| {
        drawn_model(c, id).as_deref() == Some("objective.lantern_of_sight.skin1.model")
    });
    client.within(SOON, "their lamp coloured for the Radiant", |c| {
        lamp_colours(c) == [0.0]
    });
    // By day, the hours after the horn: the Watcher's day sight.
    assert_eq!(
        lamp_sights(&client),
        [bota_core::WATCHER_DAY_VISION as f32],
        "the sight ring at the Watcher's sight"
    );
    client.within(SOON, "their watcher marked in the Radiant's colour", |c| {
        marked_for(c, Team::Radiant) == 1
    });
    client.stand_at((ours.x.to_int() + 100, ours.y.to_int()));
    let mine = watcher_at(&client, ours);
    client.within(SOON, "our watcher drawn at rest", |c| {
        drawn_model(c, mine).as_deref() == Some("objective.lantern_of_sight.model")
    });
    client.core_mut().act(Intent::Smart {
        at: (ours.x.to_f32(), ours.y.to_f32()),
        unit: Some(mine),
    });
    client.within(SOON, "the channel's effect", |c| {
        started(c, "watcher.channel") > 0
    });
    client.within(SOON, "our watcher lit", |c| lit(c, mine));
    client.within(SOON, "our watcher drawn in the Dire's skin", |c| {
        drawn_model(c, mine).as_deref() == Some("objective.lantern_of_sight.skin2.model")
    });
    // Their Watcher may be out of sight from here: ours is coloured for the Dire.
    client.within(SOON, "our lamp coloured for the Dire", |c| {
        lamp_colours(c).last() == Some(&1.0)
    });
    client.within(SOON, "our watcher marked in the Dire's colour", |c| {
        marked_for(c, Team::Dire) == 1
    });
}

/// How many Watchers the minimap marks in a side's colour.
pub(super) fn marked_for(client: &mut Client, side: Team) -> usize {
    let tint = crate::overlay::team_color(side);
    let watcher = Rect::new(128.0, 0.0, 192.0, 64.0);
    let world = client.world_mut();
    world
        .query::<(&ImageNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(image, shown)| shown.get() && image.rect == Some(watcher) && image.color == tint)
        .count()
}

/// How many times the particle effect of a logical name started.
fn started(client: &Client, effect: &str) -> u32 {
    let world = client.world();
    let effect = world
        .resource::<Library>()
        .effect(effect)
        .unwrap_or_else(|| panic!("{effect} in the asset cache"));
    world.resource::<Playing>().started(effect)
}

/// One player lights a Watcher: a spectator of the whole map, the same spectator through the
/// other side's eyes, and a replay of the match from either side and of the whole map all draw
/// it in the lighter's colours, which the wire tells every view (it was a guess from whose
/// grant a view lacked).
#[test]
fn a_spectator_the_other_sides_eyes_and_a_replay_see_whom_a_watcher_lights_for() {
    let mut game = super::fog_pov::Spectated::with_options("0", &["--objectives", "watchers"]);
    let at = bota_core::DOTA_WATCHERS[4];
    let watcher = watcher_at(&game.client, at);
    assert_eq!(
        game.driver.my_team(),
        Some(Team::Radiant),
        "the driving seat lights it"
    );
    game.cheat(Cheat::Teleport {
        at: at + Vec2::from_ints(-100, 0),
    });
    // The driving seat's own handle of it.
    let lamp = game
        .driver
        .view()
        .and_then(|view| {
            view.units
                .iter()
                .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        })
        .expect("the watcher in the driving seat's view")
        .id;
    game.order(Order::Attack {
        target: Target::Unit(lamp),
    });
    game.wait("the watcher lit, as the spectator sees it", |_, c| {
        lit(c, watcher)
    });
    // The replay ends with the Watcher lit: what the match plays after need not fill it.
    game.stop_recording();
    let radiant_skin = Some("objective.lantern_of_sight.skin1.model");
    game.wait(
        "the whole map's spectator draws the Radiant's colours",
        |_, c| {
            drawn_model(c, watcher).as_deref() == radiant_skin
                && lamp_colours(c) == [0.0]
                && marked_for(c, Team::Radiant) == 1
        },
    );
    game.next_side(Team::Dire);
    game.wait("through the Dire's eyes, the Radiant's colours", |_, c| {
        let watcher = watcher_at(c, at);
        c.view()
            .unit(watcher)
            .is_some_and(|u| u.lit_for == Some(Team::Radiant))
            && drawn_model(c, watcher).as_deref() == radiant_skin
            && marked_for(c, Team::Radiant) == 1
    });
    let file = game.save();
    for slot in [Some(0), Some(1), None] {
        let mut replay = super::fog_pov::replay_observed(&file, slot, &[], |_| {});
        replay.within(SOON, "the replay draws the Radiant's colours", |c| {
            let watcher = watcher_at(c, at);
            drawn_model(c, watcher).as_deref() == radiant_skin && marked_for(c, Team::Radiant) == 1
        });
    }
}
