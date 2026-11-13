//! The four team-only base exits use the installed force-field effects at their map positions.

use std::time::Duration;

use bevy::prelude::*;
use bota_proto::{Cheat, HeroId, Team};

use super::harness::{Client, Server, scripted_seat};
use crate::particles::{Library, Playing};

// Independent oracle: the four installed-game (build 6975) placements, shifted by 9216.
const GATES: [(f32, f32, &str); 4] = [
    (1523.3955, 6286.6, "base_gate.radiant_ambient"),
    (16576.0, 11648.0, "base_gate.dire_ambient"),
    (5771.0244, 1740.8809, "base_gate.radiant_ambient"),
    (12160.0, 16512.0, "base_gate.dire_ambient"),
];

fn until(client: &mut Client, what: &str, condition: impl Fn(&Client) -> bool) {
    for _ in 0..4096 {
        if condition(client) {
            return;
        }
        client.step_frames(1);
    }
    panic!("frame budget exhausted: {what}");
}

fn instances_near(client: &Client, name: &str, at: Vec2) -> usize {
    let library = client.world().resource::<Library>();
    let effect = library
        .effect(name)
        .expect("the installed gate effect is imported");
    client
        .world()
        .resource::<Playing>()
        .instances_of(effect)
        .filter(|instance| !instance.ending())
        .filter(|instance| {
            instance.nodes[0].control_points[0]
                .position
                .truncate()
                .distance(at)
                < 1.0
        })
        .count()
}

fn look_at(client: &mut Client, at: Vec2) {
    client.core_mut().pinned = None;
    let world = client.world_mut();
    world
        .query::<&mut crate::camera::RtsCamera>()
        .single_mut(world)
        .expect("the camera")
        .focus = Vec3::new(at.x, 0.0, -at.y);
    client.step_frames(3);
}

fn assert_field(client: &Client, name: &str, at: Vec2, vertical: bool) {
    use heatel::particle::Field;
    let effect = client
        .world()
        .resource::<Library>()
        .effect(name)
        .expect("gate effect");
    let playing = client.world().resource::<Playing>();
    let instance = playing
        .instances_of(effect)
        .find(|i| {
            i.nodes[0].control_points[0]
                .position
                .truncate()
                .distance(at)
                < 1.0
        })
        .expect("field at the map placement");
    let node = &instance.nodes[0];
    let source_height = if name.contains("radiant") {
        273.57
    } else {
        276.48
    };
    assert!((node.control_points[0].position.z - source_height - 32.0).abs() < 0.01);
    assert_eq!(node.control_points[2].position, Vec3::splat(10000.0));
    assert_eq!(
        node.parts.len(),
        16,
        "the installed field's sixteen rope points"
    );
    for index in 0..16 {
        let offset = 200.0 - 400.0 * index as f32 / 15.0;
        let expected = Vec3::new(
            at.x + if vertical { 0.0 } else { offset },
            at.y - if vertical { offset } else { 0.0 },
            source_height + 40.0,
        );
        let actual = node.parts.vector_field(Field::POSITION, index);
        assert!(
            actual.distance(expected) < 0.02,
            "rope point {index}: {actual} vs {expected}"
        );
        // The initializer precedes the program's +40Z position offset. Compare in the
        // installed game's frame so a translated host cannot silently change the normals.
        let source_position = expected - Vec3::new(9216.0, 9216.0, 40.0);
        let expected_normal = (source_position - Vec3::new(85.0, 10.0, 0.0)).normalize();
        let actual_normal = node.parts.vector_field(Field::NORMAL, index);
        assert!(
            actual_normal.distance(expected_normal) < 0.0001,
            "normal at point {index} must preserve the source frame: {actual_normal} vs {expected_normal}"
        );
    }
    assert_eq!(
        node.control_points[1].position,
        Vec3::new(9301.0, 9226.0, 0.0)
    );
    let color = node.parts.vector_field(Field::COLOR, 7);
    if name.contains("radiant") {
        assert!(color.z > color.x * 2.0, "blue Radiant field: {color}");
    } else {
        assert!(color.x > color.z * 2.0, "red Dire field: {color}");
    }
}

fn log_gate_extent(client: &Client, gate_index: usize, name: &str, at: Vec2) {
    let ground = client.world().resource::<crate::map::Ground>();
    let effect = client
        .world()
        .resource::<Library>()
        .effect(name)
        .expect("gate effect");
    let frame = client.world().resource::<crate::particles::ParticleFrame>();
    let mut height = (f32::INFINITY, f32::NEG_INFINITY);
    let mut above = (f32::INFINITY, f32::NEG_INFINITY);
    for batch in frame.batches.iter().filter(|batch| {
        effect.defs[0]
            .draws
            .iter()
            .any(|draw| draw.look == batch.look)
    }) {
        for card in &frame.quads[batch.range.start as usize..batch.range.end as usize] {
            if Vec2::new(card.head[0], -card.head[2]).distance(at) > 400.0 {
                continue;
            }
            for (center, side) in [(card.head, card.head_side), (card.tail, card.tail_side)] {
                for sign in [-1.0, 1.0] {
                    let point = Vec3::from(center) + Vec3::from(side) * sign;
                    height = (height.0.min(point.y), height.1.max(point.y));
                    let clearance = point.y - ground.surface_at(point.x, -point.z);
                    above = (above.0.min(clearance), above.1.max(clearance));
                }
            }
        }
    }
    let source_z = if gate_index.is_multiple_of(2) {
        273.57
    } else {
        276.48
    };
    println!(
        "base gate geometry gate={gate_index} source_z={source_z} coarse_z={} surface_z={} card_z={height:?} terrain_clearance={above:?}",
        ground.at(at.x, at.y),
        ground.surface_at(at.x, at.y)
    );
}

fn verify_gate_scene(
    client: &mut Client,
    gate_index: usize,
    (x, y, name): (f32, f32, &str),
    night: bool,
) {
    let at = Vec2::new(x, y);
    look_at(client, at);
    until(client, "the allied gate's permanent vision", |c| {
        c.world().resource::<crate::fog::FogGrid>().lit(at.x, at.y)
    });
    until(client, "the gate force field", |c| {
        instances_near(c, name, at) == 1
    });
    assert_field(client, name, at, gate_index >= 2);
    until(client, "the gate's visible particle cards", |c| {
        let effect = c
            .world()
            .resource::<Library>()
            .effect(name)
            .expect("gate effect");
        let frame = c.world().resource::<crate::particles::ParticleFrame>();
        frame
            .batches
            .iter()
            .filter(|batch| {
                effect
                    .defs
                    .iter()
                    .any(|def| def.draws.iter().any(|draw| draw.look == batch.look))
            })
            .flat_map(|batch| &frame.quads[batch.range.start as usize..batch.range.end as usize])
            .any(|q| {
                Vec2::new(q.head[0], -q.head[2]).distance(at) < 400.0
                    && q.head_color[3].max(q.tail_color[3]) > 0.05
            })
    });
    log_gate_extent(client, gate_index, name, at);
    println!(
        "base gate scene night={night} gate={gate_index} tick={} at={x},{y} pov={}",
        client.tick(),
        client.core().my_slot.expect("seat").0
    );
}

fn record_owned_gates(side: usize) {
    let replay =
        crate::test_support::scratch_path(&format!("base_gates_{side}_{}.brp", std::process::id()));
    let mut server = Server::start(
        if side == 0 { 1 } else { 2 },
        &[
            "--map",
            "0",
            "--replay",
            replay.to_str().expect("replay path"),
        ],
    );
    if side == 1 {
        scripted_seat(&server, HeroId::PUDGE, |_| Vec::new());
    }
    let mut client = Client::join_with_clock(
        &server,
        HeroId::SHADOW_FIEND,
        &[],
        Duration::from_secs_f32(0.02),
    );
    assert_eq!(
        client.me().team,
        if side == 0 { Team::Radiant } else { Team::Dire }
    );
    for night in [false, true] {
        client.cheat(Cheat::TimeOfDay { night });
        until(&mut client, "the requested time of day", |c| {
            let light = c.world().resource::<crate::daylight::Daylight>().night;
            if night {
                light >= 0.999
            } else {
                light <= 0.001
            }
        });
        for (gate_index, (x, y, name)) in GATES
            .into_iter()
            .enumerate()
            .filter(|(index, _)| index % 2 == side)
        {
            verify_gate_scene(&mut client, gate_index, (x, y, name), night);
        }
    }
    client.core_mut().leave_game();
    client.step_frames(10);
    server.wait();
    println!("base gate scenes replay={}", replay.display());
}

#[test]
fn base_gates_show_the_installed_force_fields_at_all_four_exits_by_day_and_night() {
    for side in 0..2 {
        record_owned_gates(side);
    }
}

#[test]
fn base_gates_are_absent_from_the_solo_mid_map() {
    let server = Server::start(1, &["--map", "1"]);
    let client = Client::join_with_clock(
        &server,
        HeroId::SHADOW_FIEND,
        &[],
        Duration::from_secs_f32(0.02),
    );
    for name in ["base_gate.radiant_ambient", "base_gate.dire_ambient"] {
        let effect = client
            .world()
            .resource::<Library>()
            .effect(name)
            .expect("the installed gate effect is imported");
        assert_eq!(
            client
                .world()
                .resource::<Playing>()
                .instances_of(effect)
                .count(),
            0,
            "solo mid has no base gates"
        );
    }
}

fn vision_ring_at(client: &Client, at: Vec2) -> bool {
    client
        .line_strips()
        .iter()
        .filter(|(points, _)| points.len() > 8)
        .map(|(points, _)| super::harness::ring_of(points))
        .any(|(center, radius)| center.distance(at) < 1.0 && (radius - 700.0).abs() < 1.0)
}

#[test]
fn base_gates_vision_rings_show_only_while_ranges_are_held_on_the_full_map() {
    let server = Server::start(1, &["--map", "0"]);
    let mut client = Client::join_with_clock(
        &server,
        HeroId::SHADOW_FIEND,
        &[],
        Duration::from_secs_f32(0.02),
    );
    for (x, y, _) in GATES {
        let at = Vec2::new(x, y);
        client.stand_at((x as i32 + 300, y as i32 + 300));
        assert!(
            !vision_ring_at(&client, at),
            "no gate vision ring without ALT"
        );
        client.key(KeyCode::AltLeft, bevy::input::ButtonState::Pressed);
        until(&mut client, "the gate's 700-unit vision ring", |c| {
            vision_ring_at(c, at)
        });
        client.key(KeyCode::AltLeft, bevy::input::ButtonState::Released);
        client.step_frames(2);
        assert!(
            !vision_ring_at(&client, at),
            "the gate vision ring ends with ALT"
        );
    }
}
