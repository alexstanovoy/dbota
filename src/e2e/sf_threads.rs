//! Deterministic detectors for SF trail excursions across both directions of frame-time changes.
use std::{collections::HashSet, time::Duration};

use bevy::{prelude::*, time::TimeUpdateStrategy};
use bota_proto::HeroId;
use heatel::particle::Renderer;

use super::{
    creep_walk::{ControlledFrames, tell},
    harness::{Client, Server},
};
use crate::particles::{Library, ParticleFrame, Playing};

const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 30);
const TRAIL: &str = "hero.nevermore.trail";
// The three installed ribbon systems individually bound ring + speed*life + gravity*life²
// + card half-width at 395, 570 and 570 units. Use the conservative full gravity term,
// not a half term; full CP0 position lock cancels walking displacement. Catmull-Rom's
// absolute coefficient sum is at most 1.25, so round 570*1.25=712.5 up to 750.
const EXCURSION: f32 = 750.0;

#[test]
fn shadow_fiends_ribbons_stay_local_while_idle_and_walking() {
    with_trail(|client, looks| {
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(1)));
        client.frames(50);
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        walk(client, looks);
    });
}

fn with_trail(check: impl FnOnce(&mut Client, &HashSet<u16>)) {
    let server = Server::start(1, &["--seed", "1"]);
    let mut client = Client::join_with_clock(&server, HeroId::SHADOW_FIEND, &[], Duration::ZERO);
    client.follow_me();
    server.frozen(|| {
        let _live = std::mem::replace(
            &mut client.core_mut().source,
            bota_client_core::Source::Live(Box::new(ControlledFrames)),
        );
        client.until(
            "resident models and worn trail bound without advancing time",
            ready,
        );
        assert_eq!(
            client.world().resource::<Time>().elapsed(),
            Duration::ZERO,
            "particle startup must not sample the wall-clock time spent joining/loading"
        );
        let world = client.world();
        let effect = world.resource::<Library>().effect(TRAIL).expect("trail");
        let instance = world
            .resource::<Playing>()
            .instances_of(effect)
            .find(|instance| !instance.portrait)
            .expect("world trail");
        assert_eq!(
            instance.world_time, 0.0,
            "noise starts at a known world time"
        );
        assert_eq!(
            instance.nodes[0].age, 0.0,
            "the initial effect age is reproducible"
        );
        let looks = ribbon_looks(&client);
        check(&mut client, &looks);
    });
}

fn ready(client: &mut Client) -> bool {
    let world = client.world();
    world.contains_resource::<crate::map::TerrainMaterial>()
        && world
            .resource::<crate::units::ModelBank>()
            .0
            .iter()
            .all(|model| model.loaded)
        && world
            .resource::<Library>()
            .effect(TRAIL)
            .is_some_and(|effect| {
                world
                    .resource::<Playing>()
                    .instances_of(effect)
                    .any(|instance| !instance.portrait)
            })
}

fn ribbon_looks(client: &Client) -> HashSet<u16> {
    let library = client.world().resource::<Library>();
    let effect = library.effect(TRAIL).expect("trail");
    let renderers = effect
        .defs
        .iter()
        .flat_map(|definition| &definition.system.renderers)
        .filter(|renderer| matches!(renderer.class, Renderer::Ropes(_)))
        .count();
    let looks: HashSet<_> = effect
        .defs
        .iter()
        .flat_map(|definition| {
            definition.draws.iter().filter_map(|draw| {
                let rope = matches!(
                    definition.system.renderers[draw.renderer].class,
                    Renderer::Ropes(_)
                );
                rope.then_some(draw.look)
            })
        })
        .collect();
    assert_eq!(renderers, 3, "all three authored ribbon renderers");
    assert_attribution(client, &looks);
    looks
}

fn assert_attribution(client: &Client, looks: &HashSet<u16>) {
    let library = client.world().resource::<Library>();
    let effect = library.effect(TRAIL).expect("trail");
    let playing = client.world().resource::<Playing>();
    let mut others: Vec<_> = library
        .effects
        .iter()
        .filter(|(_, other)| !std::sync::Arc::ptr_eq(effect, other))
        .filter(|(_, other)| {
            other.defs.iter().any(|definition| {
                definition
                    .draws
                    .iter()
                    .any(|draw| looks.contains(&draw.look))
            })
        })
        .filter(|(_, other)| playing.instances_of(other).next().is_some())
        .map(|(name, _)| name.as_str())
        .collect();
    others.sort_unstable();
    assert!(
        others.is_empty(),
        "look attribution must not count another effect's cards: {others:?}"
    );
}

fn walk(client: &mut Client, looks: &HashSet<u16>) {
    let mut hero = client.me().clone();
    let origin = hero.pos;
    let mut seen_world = HashSet::new();
    let mut seen_portrait = HashSet::new();
    let mut worst = 0.0_f32;
    for step in 0..1800 {
        let phase = step % 1200;
        let walked = match phase {
            0..300 => 0,
            300..600 => phase - 300,
            600..900 => 900 - phase,
            _ => 0,
        };
        hero.pos = origin + bota_proto::Vec2::from_ints(walked * hero.move_speed.to_int() / 30, 0);
        tell(client, hero.clone());
        let frame = match step {
            0 => Duration::from_millis(1),
            1 | 2 => Duration::from_millis(200),
            _ => FRAME,
        };
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        client.frames(1);
        assert_attribution(client, looks);
        assert_hero_placement(client, hero.id);
        let body = client.body(hero.id).expect("hero drawn");
        let at = client
            .world()
            .get::<GlobalTransform>(body)
            .expect("hero transform")
            .translation();
        let frame = client.world().resource::<ParticleFrame>();
        for batch in &frame.batches {
            if !looks.contains(&batch.look) {
                continue;
            }
            let anchor = if batch.stage {
                seen_portrait.insert(batch.look);
                portrait_origin(client).expect("a portrait batch has a live subject pivot")
            } else {
                seen_world.insert(batch.look);
                at
            };
            for quad in &frame.quads[batch.range.start as usize..batch.range.end as usize] {
                let head = Vec3::from_array(quad.head);
                let tail = Vec3::from_array(quad.tail);
                worst = worst.max(head.distance(anchor));
                assert!(
                    head.is_finite() && tail.is_finite(),
                    "finite submitted ribbon geometry"
                );
                assert!(
                    head.distance(anchor) <= EXCURSION,
                    "ribbon head escaped at frame {step}: {head:?}, anchor={anchor:?}, stage={}",
                    batch.stage
                );
                assert!(
                    tail.distance(anchor) <= EXCURSION,
                    "ribbon tail escaped at frame {step}: {tail:?}, anchor={anchor:?}, stage={}",
                    batch.stage
                );
                assert!(
                    head.distance(tail) < 1000.0,
                    "map-wide ribbon segment at {step}: {quad:?}"
                );
            }
        }
    }
    assert_eq!(&seen_world, looks, "world ribbons were actually submitted");
    assert_eq!(
        &seen_portrait, looks,
        "portrait ribbons were actually submitted"
    );
    eprintln!(
        "SF detector: startup=0, seed=1, cadence=1ms->200ms->30Hz, 1800 idle/walk frames, excursion={worst}"
    );
}

#[test]
fn steady_to_millisecond_frames_keep_sf_heads_and_hero_placement_bounded() {
    with_trail(|client, looks| {
        for walking in [false, true] {
            let mut hero = client.me().clone();
            let origin = hero.pos;
            let mut elapsed = 0;
            client
                .world_mut()
                .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
            for _ in 0..50 {
                move_hero(client, &mut hero, origin, &mut elapsed, FRAME, walking);
            }
            let tracked = tracked_heads(client);
            let mut previous = submitted_heads(client, &tracked);
            client
                .world_mut()
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(1)));
            for frame in 0..50 {
                move_hero(
                    client,
                    &mut hero,
                    origin,
                    &mut elapsed,
                    Duration::from_millis(1),
                    walking,
                );
                assert_attribution(client, looks);
                let current = submitted_heads(client, &tracked);
                for (before, after) in previous.iter().zip(&current) {
                    // Authored velocities plus gravity stay below 1000 u/s; 1 ms may
                    // move a submitted head at most 1 unit, including the 150 u/s walk.
                    assert!(
                        after.distance(*before) < 1.0,
                        "1 ms head motion is not rescaled: walking={walking}, frame={frame}, before={before:?}, after={after:?}, distance={}",
                        after.distance(*before)
                    );
                }
                previous = current;
            }
        }
    });
}

fn move_hero(
    client: &mut Client,
    hero: &mut bota_proto::UnitView,
    origin: bota_proto::Vec2,
    elapsed: &mut i32,
    frame: Duration,
    walking: bool,
) {
    *elapsed += frame.as_micros() as i32;
    if walking {
        hero.pos.x = origin.x + bota_proto::Fixed::from_ratio(*elapsed * 150, 1_000_000);
    }
    tell(client, hero.clone());
    client.frames(1);
    assert_hero_placement(client, hero.id);
}

fn assert_hero_placement(client: &Client, id: bota_proto::EntityId) {
    let body = client.body(id).expect("hero drawn");
    let actual = client
        .world()
        .get::<GlobalTransform>(body)
        .expect("hero transform")
        .translation();
    let shown = bota_client_core::bodies(
        client.view(),
        client.core().prev_view(),
        client.core().smooth.alpha(),
    )
    .into_iter()
    .find(|shown| shown.unit.id == id)
    .expect("hero projection");
    let expected = Vec3::new(
        shown.x,
        client
            .world()
            .resource::<crate::map::Ground>()
            .at(shown.x, shown.y)
            + shown.height,
        -shown.y,
    );
    assert!(
        actual.distance(expected) < 0.01,
        "hero placement: drawn {actual:?}, expected {expected:?}"
    );
}

struct TrackedHead {
    node: usize,
    particle: u32,
    look: u16,
}

fn tracked_heads(client: &Client) -> Vec<TrackedHead> {
    use heatel::particle::Field;
    let library = client.world().resource::<Library>();
    let effect = library.effect(TRAIL).expect("trail");
    let instance = client
        .world()
        .resource::<Playing>()
        .instances_of(effect)
        .find(|instance| !instance.portrait)
        .expect("world trail");
    let tracked: Vec<_> = instance
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let definition = &effect.defs[node.def];
            let draw = definition.draws.iter().find(|draw| {
                matches!(
                    definition.system.renderers[draw.renderer].class,
                    Renderer::Ropes(_)
                )
            })?;
            let particle = (0..node.parts.len().saturating_sub(1)).find(|&i| {
                let age = node.age - node.parts.scalar_field(Field::CREATION_TIME, i);
                age > 0.05 && node.parts.scalar_field(Field::LIFE_DURATION, i) - age > 0.1
            })?;
            Some(TrackedHead {
                node: index,
                particle: node.parts.number[particle],
                look: draw.look,
            })
        })
        .collect();
    assert_eq!(
        tracked.len(),
        3,
        "one persistent head from each authored ribbon"
    );
    tracked
}

fn submitted_heads(client: &Client, tracked: &[TrackedHead]) -> Vec<Vec3> {
    use heatel::particle::Field;
    let world = client.world();
    let effect = world.resource::<Library>().effect(TRAIL).expect("trail");
    let instance = world
        .resource::<Playing>()
        .instances_of(effect)
        .find(|instance| !instance.portrait)
        .expect("world trail");
    let frame = world.resource::<ParticleFrame>();
    tracked
        .iter()
        .map(|tracked| {
            let node = &instance.nodes[tracked.node];
            let index = node
                .parts
                .number
                .iter()
                .position(|number| *number == tracked.particle)
                .expect("tracked head survives the 50 ms interval");
            let position = node.parts.vector_field(Field::POSITION, index);
            let expected = Vec3::new(position.x, position.z, -position.y);
            frame
                .batches
                .iter()
                .filter(|batch| !batch.stage && batch.look == tracked.look)
                .flat_map(|batch| {
                    &frame.quads[batch.range.start as usize..batch.range.end as usize]
                })
                .map(|quad| Vec3::from_array(quad.head))
                .find(|head| head.distance(expected) < 0.01)
                .expect("the tracked particle is a submitted rope head")
        })
        .collect()
}

/// The actual subject pivot of the separate portrait view, independent of particle inputs.
fn portrait_origin(client: &Client) -> Option<Vec3> {
    let world = client.world();
    let actor = world.resource::<crate::portrait::Stage>().actor.as_ref()?;
    world
        .get::<GlobalTransform>(actor.pivot)
        .map(GlobalTransform::translation)
}
