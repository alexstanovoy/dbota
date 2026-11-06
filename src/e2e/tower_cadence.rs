//! Tower projectiles, animation attack points and launch sounds share Dota's firing cadence.
//! https://www.dota2.com/patches/7.27: 0.9 BAT and 110 attack speed, unchanged in 7.41f.

use std::collections::HashSet;
use std::time::Duration;

use bevy::{
    animation::graph::AnimationNodeIndex, audio::PlaybackMode, prelude::*, time::TimeUpdateStrategy,
};
use bota_client_core::Intent;
use bota_proto::{Cheat, CreepSide, EntityId, HeroId, MapId, ModifierSpec, Target, Team, UnitKind};

use super::harness::{Client, Server, quiet_seat};
use crate::units::{ModelBank, ModelOf, Rig, Unit};

const FRAME: Duration = Duration::from_nanos(33_333_333);

fn attack_animation(client: &Client, tower: EntityId) -> Option<(Entity, AnimationNodeIndex, f32)> {
    let body = client.body(tower)?;
    let world = client.world();
    let rig = world.get::<Rig>(body)?.0;
    world.get::<AnimationPlayer>(rig)?;
    let model = world.get::<Unit>(body)?.model?;
    let clip = world.resource::<ModelBank>().0[model]
        .clips
        .iter()
        .find(|clip| clip.0 == "attack")?;
    Some((rig, clip.1, clip.3?))
}

fn model_rotation(client: &Client, unit: EntityId) -> Quat {
    let body = client.body(unit).expect("the drawn tower");
    let pivot = client
        .world()
        .get::<ModelOf>(body)
        .expect("the tower model")
        .0;
    client
        .world()
        .get::<Transform>(pivot)
        .expect("the model pivot")
        .rotation
}

fn assert_attack_point(client: &Client, tower: EntityId) {
    let (rig, node, point) = attack_animation(client, tower).expect("the tower's attack animation");
    let active = client
        .world()
        .get::<AnimationPlayer>(rig)
        .expect("the animation player")
        .animation(node)
        .expect("the attack is playing");
    let windup = client.view().unit(tower).expect("the tower").attack_point as f32 / 1000.0;
    let allowance = FRAME.as_secs_f32() * point / windup + 0.001;
    assert!(
        active.weight() > 0.5,
        "the attack is drawn, not only tracked"
    );
    assert!(
        (point - 0.001..=point + allowance).contains(&active.seek_time()),
        "projectile launched at clip time {}, expected point {point}",
        active.seek_time()
    );
}

fn tower_match(seats: u8) -> (Server, Client, EntityId, bota_proto::Angle, Quat) {
    let server = Server::start(seats, &["--map", "0"]);
    if seats == 2 {
        quiet_seat(&server, HeroId::PUDGE);
    }
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::AllVision { on: true });
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::ApplyModifier {
        target: Target::None,
        spec: ModifierSpec {
            max_hp: ModifierSpec::MAX_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: 10_000,
    });
    client.cheat(Cheat::Refresh);
    let enemy = client.me().team.enemy();
    let map = bota_core::map_of(MapId(0));
    let towers = if enemy == Team::Dire {
        map.dire_towers
    } else {
        map.radiant_towers
    };
    let at = towers
        .iter()
        .find(|(lane, tier, _)| *lane == bota_core::rules::LANE_MID && *tier == 1)
        .expect("the enemy mid tier-one tower")
        .2;
    let tower = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Tower && u.team == enemy && u.pos == at)
        .expect("the tower")
        .id;
    client.until("the tower attack animation loaded", |c| {
        attack_animation(c, tower).is_some()
    });
    let initial_facing = client.view().unit(tower).expect("the tower").facing;
    let authored_rotation = model_rotation(&client, tower);
    // Both sides must turn after the authored orientation was captured.
    let opposite = bota_proto::Angle {
        brads: initial_facing.brads.wrapping_add(32768),
    };
    let bait = bota_core::point_ahead(at, opposite, bota_proto::Fixed::from_int(400));
    client.stand_at((bait.x.to_int(), bait.y.to_int()));
    client.core_mut().act(Intent::Stop);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    (server, client, tower, initial_facing, authored_rotation)
}

fn check_cadence(seats: u8, sound_path: &'static str) {
    let (server, mut client, tower, initial_facing, authored_rotation) = tower_match(seats);
    let starts = super::sound_limits::listen(&mut client, sound_path);
    client.cheat(Cheat::Invulnerable { on: false });
    let mut seen: HashSet<_> = client.view().projectiles.iter().map(|p| p.id).collect();
    let mut launches = Vec::new();
    let mut sound_ticks = Vec::new();
    let mut heard = 0;
    let mut wire_turned = false;
    client.until(
        "twelve tower shots shown and sounded at their launch",
        |c| {
            let facing = c.view().unit(tower).expect("the tower").facing;
            wire_turned |= bota_core::angle_delta(initial_facing, facing).abs()
                > i32::from(bota_core::rules::ATTACK_ANGLE_BRADS);
            for p in c
                .view()
                .projectiles
                .iter()
                .filter(|p| p.source == Some(tower))
            {
                if seen.insert(p.id) {
                    assert!(
                        wire_turned,
                        "the wire tower must turn before its first launch"
                    );
                    launches.push(c.tick());
                    assert!(
                        model_rotation(c, tower).abs_diff_eq(authored_rotation, 1e-5),
                        "the tower keeps its authored map yaw while attacking"
                    );
                    assert_attack_point(c, tower);
                }
            }
            let count = starts.lock().expect("unpoisoned").len();
            sound_ticks.extend(std::iter::repeat_n(c.tick(), count - heard));
            heard = count;
            assert!(
                sound_ticks.len() <= launches.len(),
                "launch sound repeats without a projectile"
            );
            launches.len() >= 12
        },
    );
    for pair in launches.windows(2) {
        assert!(
            (24..=25).contains(&(pair[1] - pair[0])),
            "tower shoots too fast: {pair:?}"
        );
    }
    assert_eq!(
        sound_ticks, launches,
        "one attack sound for each projectile on its creation tick"
    );
    assert!(
        starts
            .lock()
            .expect("unpoisoned")
            .iter()
            .all(|(_, mode)| !matches!(mode, PlaybackMode::Loop))
    );
    server.frozen(|| {
        client.frames(8);
        assert_eq!(
            starts.lock().expect("unpoisoned").len(),
            heard,
            "rendering cannot replay a launch"
        );
    });
}

#[test]
fn a_dire_tower_shoots_animates_and_sounds_at_its_dota_rate() {
    check_cadence(1, "towers/attack_dire");
}

#[test]
fn a_radiant_tower_shoots_animates_and_sounds_at_its_dota_rate() {
    check_cadence(2, "towers/attack_radiant");
}
