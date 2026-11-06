//! A tower that falls, as Dota shows it: its model is swapped for the baked destruction model,
//! which plays its death clip once from the first frame to the last (the tower collapsing),
//! and only then is gone.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bota_proto::{Cheat, HeroId, Order, Target, Team, UnitKind};

use super::harness::{Client, Server};
use crate::units::{Dying, ModelBank, Unit};

/// What the drawn thing of a unit that is falling shows: its model and its clip.
fn falling(client: &mut Client, id: bota_proto::EntityId) -> Option<(String, &'static str, f32)> {
    let world = client.world_mut();
    let (model, clip, age) = world
        .query_filtered::<(&Unit, &crate::anim::Pose, &Dying), ()>()
        .iter(world)
        .find(|(unit, ..)| unit.id == id)
        .map(|(unit, pose, dying)| (unit.model, pose.clip(), dying.0))?;
    let bank = world.resource::<ModelBank>();
    Some((bank.0[model?].model.logical.clone(), clip, age))
}

#[test]
fn a_fallen_tower_is_swapped_for_its_destruction_model_and_collapses_before_it_is_gone() {
    let server = Server::start(1, &["--map", "0", "--tick-rate", "600"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::Levels { count: 24 });
    let mine = client.me().team;
    let enemy = if mine == Team::Radiant {
        Team::Dire
    } else {
        Team::Radiant
    };
    let tower = client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Tower && u.team == enemy)
        .min_by_key(|u| (u.pos.x.to_int() - 9740).abs() + (u.pos.y.to_int() - 9868).abs())
        .expect("an enemy tower")
        .clone();
    client.stand_at((tower.pos.x.to_int() - 300, tower.pos.y.to_int()));
    client.core_mut().send_order_to(
        None,
        Order::Attack {
            target: Target::Unit(tower.id),
        },
    );
    client.until("the tower falling", |c| falling(c, tower.id).is_some());
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    let (model, clip, _) = falling(&mut client, tower.id).expect("falling");
    let side = if enemy == Team::Dire {
        "dire"
    } else {
        "radiant"
    };
    assert_eq!(
        model,
        format!("tower.{side}.destruction"),
        "the model it falls in"
    );
    assert_eq!(clip, "death", "the clip it plays");
    // Longer than a unit's fall, so the whole collapse (6.7 s) can be seen.
    client.frames(30);
    assert!(
        falling(&mut client, tower.id).is_some(),
        "still collapsing three seconds on"
    );
    client.until("the collapse over and the tower gone", |c| {
        falling(c, tower.id).is_none()
    });
}
