//! A deterministic detector for SF trail excursions, including a fast-to-slow frame transition.
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
            .next()
            .expect("worn trail");
        assert_eq!(
            instance.world_time, 0.0,
            "noise starts at a known world time"
        );
        assert_eq!(
            instance.nodes[0].age, 0.0,
            "the initial effect age is reproducible"
        );
        let looks = ribbon_looks(&client);
        // Reproduce a cadence change explicitly instead of inheriting an arbitrary last
        // loading-frame delta. A 15/s emitter crosses a birth boundary on the first slow step.
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(1)));
        client.frames(50);
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        walk(&mut client, &looks);
    });
}

fn ready(client: &mut Client) -> bool {
    let world = client.world();
    world
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
                    .next()
                    .is_some()
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
    let mut seen = HashSet::new();
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
            seen.insert(batch.look);
            for quad in &frame.quads[batch.range.start as usize..batch.range.end as usize] {
                let head = Vec3::from_array(quad.head);
                let tail = Vec3::from_array(quad.tail);
                worst = worst.max(head.distance(at));
                assert!(
                    head.is_finite() && tail.is_finite(),
                    "finite submitted ribbon geometry"
                );
                assert!(
                    head.distance(at) <= EXCURSION,
                    "ribbon escaped at frame {step}: {head:?}, hero {at:?}, distance {}",
                    head.distance(at)
                );
                assert!(
                    head.distance(tail) < 1000.0,
                    "map-wide ribbon segment at {step}: {quad:?}"
                );
            }
        }
    }
    assert_eq!(&seen, looks, "ribbons were actually submitted");
    eprintln!(
        "SF detector: startup=0, seed=1, cadence=1ms->200ms->30Hz, 1800 idle/walk frames, excursion={worst}"
    );
}
