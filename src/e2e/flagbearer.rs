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

#[test]
fn inspiration_draws_above_the_terrain_for_both_teams_and_viewers() {
    for viewer in [Team::Radiant, Team::Dire] {
        check_inspiration_for_viewer(viewer);
    }
}

fn check_inspiration_for_viewer(viewer: Team) {
    let server = Server::start(if viewer == Team::Dire { 2 } else { 1 }, &["--seed", "5"]);
    if viewer == Team::Dire {
        super::harness::quiet_seat(&server, HeroId::PUDGE);
    }
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    assert_eq!(client.me().team, viewer);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::AllVision { on: true });
    client.cheat(Cheat::StartGame);
    advance_to_flag_wave(&mut client);
    client.until("the hero body available", |c| c.body(c.me().id).is_some());
    // Recreate the cold-load interleaving with initial camera placement still pending.
    client.core_mut().found_my_hero = false;
    client.until("the initial hero camera placement", |c| {
        c.core().found_my_hero
    });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f32(1.0 / 60.0),
        ));
    let flags: Vec<_> = client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::CreepFlagbearer)
        .map(|u| (u.id, u.team))
        .collect();
    assert_eq!(flags.len(), 2);
    server.frozen(|| {
        for (id, team) in flags {
            check_inspiration_circle(&mut client, id, team, viewer);
        }
    });
    client.cheat(Cheat::AllVision { on: false });
    let own = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::CreepFlagbearer && u.team == viewer)
        .expect("own flagbearer")
        .id;
    server.frozen(|| {
        check_inspiration_circle(&mut client, own, viewer, viewer);
    });
}

fn check_inspiration_circle(client: &mut Client, id: EntityId, team: Team, viewer: Team) {
    centre_camera_on(client, id);
    client.until("this flagbearer's scene ready", |c| {
        c.body(id).is_some_and(|body| {
            c.world()
                .get::<crate::units::Attachments>(body)
                .is_some_and(|a| a.node("attach_banner").is_some())
        })
    });
    client.frames(30);
    assert_inspiration_circle(client, id, team, viewer);
}

fn inspiration_circle(client: &Client, team: Team) -> (Vec3, Vec<u16>) {
    use heatel::particle::Renderer;
    let side = if team == Team::Radiant {
        "radiant"
    } else {
        "dire"
    };
    let world = client.world();
    let effect = world
        .resource::<crate::particles::Library>()
        .effect(&format!("creep.{side}.inspiration"))
        .expect("imported Inspiration");
    let mut instances = world
        .resource::<crate::particles::Playing>()
        .instances_of(effect);
    let instance = instances.next().expect("Inspiration playing");
    assert!(
        instances.next().is_none(),
        "one flagbearer per team in this wave"
    );
    let node = instance.nodes.iter().find(|n| effect.defs[n.def].system.renderers.iter().any(|r|
        matches!(&r.class, Renderer::Sprites(s) if s.look.texture.as_ref().is_some_and(|t| t.contains("ping_circle")))))
        .expect("the authored circle renderer");
    assert_eq!(node.parts.len(), 1, "one persistent circle for {team:?}");
    let position = node.parts.vectors[0][0];
    let center = Vec3::new(position.x, position.z, -position.y);
    let looks: Vec<_> = effect.defs[node.def].draws.iter().map(|d| d.look).collect();
    (center, looks)
}

fn assert_inspiration_circle(client: &Client, id: EntityId, team: Team, viewer: Team) {
    let world = client.world();
    let (center, looks) = inspiration_circle(client, team);
    let frame = world.resource::<crate::particles::ParticleFrame>();

    let cards: Vec<_> = frame
        .batches
        .iter()
        .filter(|b| looks.contains(&b.look))
        .flat_map(|b| &frame.quads[b.range.start as usize..b.range.end as usize])
        .filter(|q| ((Vec3::from(q.head) + Vec3::from(q.tail)) * 0.5).distance(center) < 0.01)
        .collect();
    assert_eq!(
        cards.len(),
        1,
        "the actual card for {id:?}, {team:?} viewed by {viewer:?}; center={center:?}, looks={looks:?}, batches={:?}",
        frame.batches
    );
    let q = cards[0];
    let alpha = if team == Team::Radiant { 0.3 } else { 1.0 };
    assert!(
        (q.head_color[3] - alpha).abs() < 0.001,
        "source alpha: {:?}",
        q.head_color
    );
    let ground = world.resource::<crate::map::Ground>();
    let clearance = center.y - ground.surface_at(center.x, -center.z);
    assert!(
        (clearance - 18.0).abs() < 2.0,
        "the circle clears drawn terrain: {team:?} viewed by {viewer:?}, clearance {clearance}, center {center:?}"
    );
    let normal = Vec3::from(q.head_side)
        .cross(Vec3::from(q.tail) - Vec3::from(q.head))
        .normalize();
    assert!(normal.y.abs() > 0.9, "the circle faces up: {normal:?}");
    eprintln!(
        "Inspiration {team:?} viewed by {viewer:?}: card alpha {}, clearance {clearance}",
        q.head_color[3]
    );
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
    // Joint and attachment local axes stay in Source's frame below the converted skeleton
    // root. The exported model's pole is glTF Y, authored Source Z.
    let pole = banner.rotation * Vec3::Y;
    let forward = banner.rotation * Vec3::NEG_Z;
    assert!(
        forward.dot(attachment.rotation * Vec3::X) > 0.9999,
        "the drawn banner's forward follows the attachment's authored X: {forward:?}, {attachment:?}"
    );
    assert!(
        pole.dot(attachment.rotation * Vec3::Z) > 0.9999,
        "the drawn pole follows the attachment's authored up axis: {pole:?}, {attachment:?}"
    );
    assert!(
        pole.dot(Vec3::Y) > 0.85,
        "the flag stands up instead of sideways: {pole:?}"
    );
    let playing = world.resource::<crate::particles::Playing>();
    let instance = playing.instances_of(effect).next().expect("banner playing");
    assert!(
        (instance.nodes[0].control_points[7].position.x - 0.93).abs() < 0.001,
        "the particle reads the actual lane model scale"
    );
}

/// Dismember's blur aligns its particle normals to CP0's X. Its victim's hit-location
/// attachment has the same Source-local axes as a flag socket, below the converted root.
#[test]
fn dismember_blur_follows_the_victims_authored_attachment_frame() {
    let server = Server::start(1, &[]);
    let mut client = cast_dismember(&server);
    let victim = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.team != client.me().team)
        .expect("victim")
        .id;
    client.until("the Dismember blur particles", |c| {
        dismember_normals(c).is_some()
    });
    server.frozen(|| {
        let world = client.world();
        let body = client.body(victim).expect("victim drawn");
        let socket = world
            .get::<crate::units::Attachments>(body)
            .expect("attachments")
            .node("attach_hitloc")
            .expect("hit location");
        let rotation = world
            .get::<GlobalTransform>(socket)
            .expect("socket frame")
            .to_scale_rotation_translation()
            .1;
        let to_particle = |v: Vec3| Vec3::new(v.x, -v.z, v.y);
        let (basis, normals) = dismember_normals(&client).expect("live blur");
        for (actual, axis) in basis.into_iter().zip([Vec3::X, Vec3::Y, Vec3::Z]) {
            assert!(
                actual.dot(to_particle(rotation * axis)) > 0.9999,
                "Dismember CP0 follows the victim's authored {axis:?}: {actual:?}, {rotation:?}"
            );
        }
        for normal in normals {
            assert!(
                normal.dot(to_particle(rotation * Vec3::X)) > 0.999,
                "the rendered blur normal follows the victim's Source X: {normal:?}"
            );
        }
    });
}

fn dismember_normals(client: &Client) -> Option<([Vec3; 3], Vec<Vec3>)> {
    let world = client.world();
    let effect = world
        .resource::<crate::particles::Library>()
        .effect("hero.pudge.dismember")?;
    let instance = world
        .resource::<crate::particles::Playing>()
        .instances_of(effect)
        .next()?;
    let blur = instance.nodes.iter().find(|node| {
        effect.defs[node.def]
            .system
            .initializers
            .iter()
            .any(|init| {
                matches!(
                    init.class,
                    heatel::particle::Initializer::NormalAlignToCp { axis: 0, .. }
                )
            })
    })?;
    // Field 21 (normal) is the particle store's vector column 4.
    let normals = blur.parts.vectors[4].clone();
    if normals.is_empty() {
        return None;
    }
    let cp = blur.control_points[0];
    Some(([cp.forward, cp.left, cp.up], normals))
}

fn cast_dismember(server: &Server) -> Client {
    use bota_client_core::{Intent, Slot};
    use bota_proto::{AbilityId, Vec2 as Spot};

    let mut client = Client::join(server, HeroId::PUDGE, &[]);
    let mid = super::harness::MID;
    client.stand_at(mid);
    client.cheat(Cheat::Levels { count: 5 });
    let slot = client
        .me()
        .abilities
        .iter()
        .position(|a| a.id == AbilityId::DISMEMBER)
        .expect("Dismember") as u8;
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(slot),
        ctrl: true,
    });
    client.frames(3);
    client.cheat(Cheat::CreateHero {
        hero: HeroId::SHADOW_FIEND,
        enemy: true,
        at: Spot::from_ints(mid.0 + 120, mid.1),
    });
    let victim = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.team != client.me().team)
        .map(|u| (u.id, u.pos))
        .expect("victim");
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(slot),
        ctrl: false,
    });
    client.core_mut().act(Intent::Aimed {
        at: (victim.1.x.to_f32(), victim.1.y.to_f32()),
        unit: Some(victim.0),
    });
    client
}
