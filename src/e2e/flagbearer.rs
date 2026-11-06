//! The live lane flagbearer carries its team's banner and wears Inspiration's ground effect.

use std::time::{Duration, Instant};

use bevy::{gltf::Gltf, prelude::*, world_serialization::WorldAssetRoot};
use bota_proto::{Cheat, EntityId, HeroId, Team, UnitKind};

use super::harness::{Client, Server};

#[test]
fn flagbearers_carry_their_team_banner_and_the_inspiration_aura() {
    let server = Server::start(1, &["--seed", "5"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::AllVision { on: true });
    client.cheat(Cheat::StartGame);
    advance_to_flag_wave(&mut client);
    let flags: Vec<_> = client
        .view()
        .units
        .iter()
        .filter(|unit| unit.kind == UnitKind::CreepFlagbearer)
        .map(|unit| (unit.id, unit.team))
        .collect();
    server.frozen(|| {
        client.until("the two flagbearer bodies drawn", |c| {
            flags.iter().all(|(id, _)| c.body(*id).is_some())
        });
        for (id, team) in flags {
            check_flag(&mut client, id, team);
        }
    });
}

fn advance_to_flag_wave(client: &mut Client) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while client
        .view()
        .units
        .iter()
        .filter(|unit| unit.kind == UnitKind::CreepFlagbearer)
        .count()
        < 2
    {
        assert!(
            Instant::now() < deadline,
            "the first flagbearer wave never arrived"
        );
        client.core_mut().pump(0.0);
        std::thread::yield_now();
    }
}

fn check_flag(client: &mut Client, id: EntityId, team: Team) {
    let side = match team {
        Team::Radiant => "radiant",
        Team::Dire => "dire",
        _ => panic!("lane team"),
    };
    assert!(
        client.view().units.iter().any(|unit| unit.team == team
            && unit
                .effects
                .iter()
                .any(|effect| effect.id == bota_proto::EffectId(14))
            && unit.hp_regen >= bota_proto::Fixed::from_int(3)),
        "the server grants the allied wave Inspiration and its regeneration"
    );
    let (banner, model, looks) = flag_art(client, side);
    centre_camera_on(client, id);
    client.until("the banner's mesh and Inspiration cards drawn", |c| {
        !visible_banner_roots(c, &model).is_empty()
            && c.world()
                .resource::<crate::particles::Playing>()
                .instances_of(&banner)
                .any(|instance| instance.nodes.iter().any(|node| !node.parts.is_empty()))
            && c.world()
                .resource::<crate::particles::ParticleFrame>()
                .batches
                .iter()
                .any(|batch| looks.contains(&batch.look) && !batch.range.is_empty())
    });
    let roots = visible_banner_roots(client, &model);
    assert_eq!(roots.len(), 1, "one rendered flag for this lane creep");
    assert_banner_frame(client, id, roots[0], &banner);
}

fn flag_art(
    client: &Client,
    side: &str,
) -> (
    std::sync::Arc<crate::particles::Effect>,
    Handle<Gltf>,
    Vec<u16>,
) {
    let library = client.world().resource::<crate::particles::Library>();
    let banner = library
        .effect(&format!("creep.{side}.banner"))
        .expect("the team's banner particle is imported")
        .clone();
    let aura = library
        .effect(&format!("creep.{side}.inspiration"))
        .expect("the team's Inspiration particle is imported")
        .clone();
    let model = library
        .models
        .iter()
        .find(|model| model.file.ends_with(&format!("creep_banner_{side}.glb")))
        .expect("the authored banner model")
        .gltf
        .clone();
    let looks: Vec<_> = aura
        .defs
        .iter()
        .flat_map(|def| def.draws.iter().map(|draw| draw.look))
        .collect();
    (banner, model, looks)
}

fn centre_camera_on(client: &mut Client, id: EntityId) {
    let at = client.view().unit(id).expect("flagbearer").pos;
    client.core_mut().pinned = None;
    let world = client.world_mut();
    let height = world
        .resource::<crate::map::Ground>()
        .at(at.x.to_f32(), at.y.to_f32());
    world
        .query::<&mut crate::camera::RtsCamera>()
        .single_mut(world)
        .expect("camera")
        .focus = Vec3::new(at.x.to_f32(), height, -at.y.to_f32());
}

fn visible_banner_roots(client: &mut Client, model: &Handle<Gltf>) -> Vec<Entity> {
    let world = client.world_mut();
    let Some(scene) = world
        .resource::<Assets<Gltf>>()
        .get(model)
        .and_then(|gltf| gltf.scenes.first())
        .cloned()
    else {
        return Vec::new();
    };
    let roots: Vec<_> = world
        .query::<(
            Entity,
            &WorldAssetRoot,
            &Visibility,
            Option<&crate::warmup::Warmup>,
        )>()
        .iter(world)
        .filter(|(_, asset, shown, warmup)| {
            asset.0 == scene && **shown != Visibility::Hidden && warmup.is_none()
        })
        .map(|(entity, ..)| entity)
        .collect();
    roots
        .into_iter()
        .filter(|root| {
            let mut stack = vec![*root];
            for _ in 0..256 {
                let Some(entity) = stack.pop() else {
                    return false;
                };
                if world.get::<Mesh3d>(entity).is_some()
                    && world
                        .get::<InheritedVisibility>(entity)
                        .is_some_and(|shown| shown.get())
                {
                    return true;
                }
                if let Some(children) = world.get::<Children>(entity) {
                    stack.extend(children.iter());
                }
            }
            panic!("banner model exceeded the bounded scene traversal");
        })
        .collect()
}

fn assert_banner_frame(
    client: &Client,
    id: EntityId,
    rendered: Entity,
    effect: &std::sync::Arc<crate::particles::Effect>,
) {
    let world = client.world();
    let body = client.body(id).expect("drawn");
    let node = world
        .get::<crate::units::Attachments>(body)
        .expect("attachments")
        .node("attach_banner")
        .expect("the authored attachment");
    let attachment = world
        .get::<GlobalTransform>(node)
        .expect("attachment frame")
        .compute_transform();
    let banner = world
        .get::<GlobalTransform>(rendered)
        .expect("banner frame")
        .compute_transform();
    // The install's C_OP_SetToCP uses world offset (-20, 7, 0), not a local offset.
    assert!(
        banner
            .translation
            .distance(attachment.translation + Vec3::new(-20.0, 0.0, -7.0))
            < 0.01,
        "the flag stays at its current drawn attachment: {banner:?}, {attachment:?}"
    );
    assert!(
        banner.rotation.dot(attachment.rotation).abs() > 0.9999,
        "the flag follows its current drawn attachment's turn"
    );
    let playing = world.resource::<crate::particles::Playing>();
    let instance = playing.instances_of(effect).next().expect("banner playing");
    assert!(
        (instance.nodes[0].control_points[7].position.x - 0.93).abs() < 0.001,
        "the particle reads the actual lane model scale"
    );
}
