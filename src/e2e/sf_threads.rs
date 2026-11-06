//! SF's authored ribbons remain local while idle and walking; they never become map-wide lines.
use std::{collections::HashSet, time::Duration};

use bevy::{prelude::*, time::TimeUpdateStrategy};
use bota_proto::HeroId;
use heatel::particle::Renderer;

use super::{
    creep_walk::{ControlledFrames, tell},
    harness::{Client, Server},
};
use crate::particles::{Library, ParticleFrame};

#[test]
fn shadow_fiends_ribbons_stay_local_while_idle_and_walking() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            1.0 / 30.0,
        )));
    let looks: HashSet<_> = client
        .world()
        .resource::<Library>()
        .effect("hero.nevermore.trail")
        .expect("trail")
        .defs
        .iter()
        .flat_map(|definition| {
            definition.draws.iter().filter_map(|draw| {
                matches!(
                    definition.system.renderers[draw.renderer].class,
                    Renderer::Ropes(_)
                )
                .then_some(draw.look)
            })
        })
        .collect();
    assert_eq!(
        client
            .world()
            .resource::<Library>()
            .effect("hero.nevermore.trail")
            .expect("trail")
            .defs
            .iter()
            .flat_map(|d| &d.system.renderers)
            .filter(|r| matches!(r.class, Renderer::Ropes(_)))
            .count(),
        3,
        "all three authored ribbon renderers"
    );
    assert!(!looks.is_empty());
    client.until("ribbons submitted", |c| {
        c.world()
            .resource::<ParticleFrame>()
            .batches
            .iter()
            .any(|b| looks.contains(&b.look))
    });
    let mut seen = HashSet::new();
    let mut longest = 0.0_f32;
    server.frozen(|| {
        let _live = std::mem::replace(&mut client.core_mut().source, bota_client_core::Source::Live(Box::new(ControlledFrames)));
        let mut hero = client.me().clone();
        let origin = hero.pos;
        for step in 0..1800 {
            // Alternate standing and normal walking, with a turn every ten seconds.
            let phase = step % 1200;
            let walked = match phase { 0..300 => 0, 300..600 => phase - 300, 600..900 => 900 - phase, _ => 0 };
            let distance = walked * hero.move_speed.to_int() / 30;
            hero.pos = origin + bota_proto::Vec2::from_ints(distance, 0);
            tell(&mut client, hero.clone());
            client.frames(1);
            let body = client.body(hero.id).expect("hero drawn");
            let at = client.world().get::<GlobalTransform>(body).expect("hero transform").translation();
            let frame = client.world().resource::<ParticleFrame>();
            for batch in &frame.batches {
                if !looks.contains(&batch.look) { continue; }
                seen.insert(batch.look);
                for quad in &frame.quads[batch.range.start as usize..batch.range.end as usize] {
                    let head = Vec3::from_array(quad.head);
                    let tail = Vec3::from_array(quad.tail);
                    // The source uses lifetimes <=1.5s, speed <=150, gravity <=300,
                    // ring radius <=20 and card radius <=60. Even their sum of maximum
                    // excursions plus 300-unit/s walking stays inside this conservative bound.
                    assert!(head.distance(at) < 1000.0, "ribbon escaped SF at frame {step}: {head:?}, hero {at:?}");
                    let length = head.distance(tail);
                    longest = longest.max(length);
                    assert!(length.is_finite() && length < 1000.0,
                        "a ribbon becomes a flying thread at frame {step}, look {}: {length}, {quad:?}", batch.look);
                }
            }
        }
    });
    assert_eq!(seen, looks, "each authored ribbon was actually submitted");
    eprintln!("SF ribbons: 60 seconds idle/walk, three renderers, longest segment {longest}");
}
