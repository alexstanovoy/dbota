//! The sound of a creep's blow, in a real match: Dota sounds the blows taken as damage for the
//! hero one plays only; creeps fighting another hero (an ally's or an enemy's) are not heard
//! taking it.

use bota_proto::{Cheat, EntityId, HeroId, UnitKind, Vec2 as At};

use super::harness::{Client, MID, Server};
use super::sound_limits::listen;

const BLOW: &str = "melee_hit";
/// How far from one's own hero the fight is: out of the creeps' reach, so none beats
/// one's own hero.
const APART: i32 = 3500;

/// Puts the camera on a point of the world, let go of the hero it followed: the blows are heard
/// from where the player looks, one's own hero or not.
fn look_at(client: &mut Client, (x, y): (i32, i32)) {
    client.core_mut().pinned = None;
    let world = client.world_mut();
    let mut camera = world
        .query::<&mut crate::camera::RtsCamera>()
        .single_mut(world)
        .expect("the game's camera");
    camera.focus = bevy::prelude::Vec3::new(x as f32, 0.0, -(y as f32));
}

/// The other hero of the match: its id, health and full health.
fn other_hero(client: &Client, me: EntityId) -> Option<(EntityId, i32, i32)> {
    client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.id != me)
        .map(|u| (u.id, u.hp, u.max_hp))
}

/// Creeps beat a hero other than one's own until it is hurt: the number of their blow sounds.
fn blows_heard_on_another_hero(enemy_hero: bool) -> usize {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Wtf { on: true });
    let me = client.me().id;
    // The creeps of the side opposed to the hero's.
    for dy in [-100, 100] {
        client.cheat(Cheat::CreateCreeps {
            enemy: !enemy_hero,
            at: At::from_ints(MID.0 + APART + 300, MID.1 + dy),
        });
    }
    let stand = At::from_ints(MID.0 + APART, MID.1);
    look_at(&mut client, (MID.0 + APART, MID.1));
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: enemy_hero,
        at: stand,
    });
    client.until("the other hero stands", |c| other_hero(c, me).is_some());
    let starts = listen(&mut client, BLOW);
    client.until("the creeps hurt the other hero", |c| {
        other_hero(c, me).is_some_and(|(_, hp, full)| hp < full)
    });
    client.frames(120);
    starts.lock().expect("unpoisoned").len()
}

#[test]
fn creeps_beating_an_allied_hero_are_not_heard_taking_the_blows() {
    assert_eq!(blows_heard_on_another_hero(false), 0);
}

#[test]
fn creeps_beating_an_enemy_hero_are_not_heard_taking_the_blows() {
    assert_eq!(blows_heard_on_another_hero(true), 0);
}

#[test]
fn creeps_beating_ones_own_hero_are_heard_taking_the_blows() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Wtf { on: true });
    let starts = listen(&mut client, BLOW);
    client.cheat(Cheat::CreateCreeps {
        enemy: true,
        at: At::from_ints(MID.0, MID.1 + 150),
    });
    client.until("the blows on one's own hero sounded", |_| {
        !starts.lock().expect("unpoisoned").is_empty()
    });
}
