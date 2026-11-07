//! Particle effects through the complete game client.

use bevy::math::Vec3;

#[test]
fn particle_model_pools_use_the_clients_pipeline_warmup_and_loading_progress() {
    use crate::e2e::harness::{Client, Server};
    use bevy::{prelude::*, world_serialization::WorldAssetRoot};

    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
    client.until("particle model pools resident", |c| {
        c.world()
            .resource::<dbota_particles::ModelProgress>()
            .0
            .is_some_and(|(done, total)| done == total)
    });
    let world = client.world_mut();
    let models = &world.resource::<super::Library>().models;
    assert!(
        !models.is_empty(),
        "the installed effects include model particles"
    );
    let gltfs = world.resource::<Assets<bevy::gltf::Gltf>>();
    let scenes: Vec<_> = models
        .iter()
        .map(|model| {
            gltfs
                .get(&model.gltf)
                .and_then(|gltf| gltf.scenes.first())
                .expect("the particle model's resident scene")
                .clone()
        })
        .collect();
    let (done, total) = world
        .resource::<dbota_particles::ModelProgress>()
        .0
        .expect("the model progress");
    assert_eq!(
        world.resource::<crate::loading::Parts>().0.get("particles"),
        Some(&(done, total))
    );
    assert_eq!(done, scenes.len());
    let mut pieces = world.query_filtered::<(
        &WorldAssetRoot,
        &Transform,
        Has<bevy::camera::visibility::NoFrustumCulling>,
    ), With<crate::warmup::Warmup>>();
    for scene in scenes {
        assert!(
            pieces.iter(world).any(|(root, transform, no_culling)| {
                root.0 == scene
                    && transform.translation == Vec3::new(0.0, -100_000.0, 0.0)
                    && no_culling
            }),
            "the client prepares each particle scene below the world with its own warm-up marker; scene {scene:?}, matching roots: {:?}",
            pieces
                .iter(world)
                .filter(|(root, ..)| root.0 == scene)
                .map(|(_, transform, no_culling)| (transform.translation, no_culling))
                .collect::<Vec<_>>()
        );
    }
}

/// Shadowraze in a match, cast by the client as a player casts it (reported: the
/// raze's embers drifted off): every card the raze's effect draws stays about the hero and the
/// spot it lands on while it plays. Its embers' velocity is control point 3,
/// which Dota leaves unplaced; standing at the world's origin it is no velocity, where it stood
/// at the effect's own place (control point 0) they flew off by a fifth of where the raze
/// landed, out of the map's middle toward the Dire corner.
#[test]
fn a_shadowraze_draws_where_it_lands_and_its_embers_do_not_drift() {
    use crate::e2e::harness::{Client, MID, Server};
    use bota_client_core::{Intent, Slot};
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    let core = client.core_mut();
    core.act(Intent::Press {
        slot: Slot::Ability(0),
        ctrl: true,
    });
    client.frames(3);
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(0),
        ctrl: false,
    });
    let hero = client.me().pos;
    let hero = Vec3::new(hero.x.to_f32(), hero.y.to_f32(), 0.0);
    // The looks only the raze draws with: the effects started besides it, before it or while it
    // plays (the hero's ambient glow and trail, the map's ambient effects about him, a rune
    // appearing), share some of its textures.
    let looks = |effect: &dbota_particles::Effect| -> Vec<u16> {
        effect
            .defs
            .iter()
            .flat_map(|def| def.draws.iter().map(|draw| draw.look))
            .collect()
    };
    let raze_looks = |c: &Client| -> Vec<u16> {
        let library = c.world().resource::<super::Library>();
        let playing = c.world().resource::<super::Playing>();
        let others: std::collections::HashSet<u16> = library
            .effects
            .iter()
            .filter(|(name, effect)| *name != "hero.nevermore.raze" && playing.started(effect) > 0)
            .flat_map(|(_, effect)| looks(effect))
            .collect();
        looks(
            library
                .effect("hero.nevermore.raze")
                .expect("the install's Shadowraze effect"),
        )
        .into_iter()
        .filter(|look| !others.contains(look))
        .collect()
    };
    assert!(
        !raze_looks(&client).is_empty(),
        "the raze has looks of its own"
    );
    // The cards of the looks the raze draws with: their heads, in the world's axes (bota's x,
    // the height, bota's y turned).
    let cards = |c: &Client| {
        let raze = raze_looks(c);
        let frame = c.world().resource::<super::ParticleFrame>();
        frame
            .batches
            .iter()
            .filter(|batch| raze.contains(&batch.look))
            .flat_map(|batch| {
                frame.quads[batch.range.start as usize..batch.range.end as usize]
                    .iter()
                    .map(|q| (batch.look, Vec3::from(q.head)))
            })
            .collect::<Vec<_>>()
    };
    assert!(
        cards(&client).is_empty(),
        "nothing else draws with the raze's looks"
    );
    let patience = std::time::Duration::from_secs(10);
    client.within(patience, "the raze drawn", |c| !cards(c).is_empty());
    let (mut farthest, mut far_look): (f32, u16) = (0.0, 0);
    for _ in 0..60 {
        client.frames(1);
        for (look, card) in cards(&client) {
            let off = Vec3::new(card.x, -card.z, 0.0).distance(hero);
            if off > farthest {
                (farthest, far_look) = (off, look);
            }
        }
    }
    // A raze lands at most 700 out and bursts 250 wide; its fleks fly a little further (up to
    // some 1400 units). Drifting, the embers went over 7000 units off within the second.
    let library = client.world().resource::<super::Library>();
    let playing = client.world().resource::<super::Playing>();
    let sharing: Vec<String> = library
        .effects
        .iter()
        .filter(|(_, effect)| looks(effect).contains(&far_look))
        .map(|(name, effect)| format!("{name} (started {})", playing.started(effect)))
        .collect();
    assert!(
        farthest < 2000.0,
        "a card {farthest} units off the hero, of look {far_look}, which {sharing:?} draw with"
    );
}

/// The Dire tower's shot as Dota draws it (reported: dark squares at the Dire tower's
/// impact): its lava splash is a second texture layer mixed into the first (a texture with no
/// alpha, whose alpha is its luminance, added twice, mirrored), so every card the shot draws
/// with a second texture carries both layers and their mix to the GPU; drawn with its first
/// layer alone and opaque, the splash was the squares. The blend in the shader is not seen
/// here, the cards it is given are.
#[test]
fn the_dire_towers_shot_draws_its_second_texture_layer() {
    use crate::e2e::harness::{Client, MID, Server};
    use bota_proto::{Team, UnitKind};
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    let mid = Vec3::new(MID.0 as f32, MID.1 as f32, 0.0);
    let tower = client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Tower && u.team == Team::Dire)
        .map(|u| Vec3::new(u.pos.x.to_f32(), u.pos.y.to_f32(), 0.0))
        .min_by(|a, b| a.distance(mid).total_cmp(&b.distance(mid)))
        .expect("a Dire tower");
    let under = tower + (mid - tower).normalize() * 350.0;
    client.stand_at((under.x as i32, under.y as i32));
    let library = client.world().resource::<super::Library>();
    let shot = library
        .effect("tower.dire.attack")
        .expect("the install's Dire tower shot");
    // The shot's looks with a second texture.
    let layered: Vec<u16> = shot
        .defs
        .iter()
        .flat_map(|def| def.draws.iter().map(|draw| draw.look))
        .filter(|look| library.looks[usize::from(*look)].second.is_some())
        .collect();
    assert!(
        !layered.is_empty(),
        "the splash's look has its second layer"
    );
    let cards = |c: &Client| {
        let frame = c.world().resource::<super::ParticleFrame>();
        frame
            .batches
            .iter()
            .filter(|batch| layered.contains(&batch.look))
            .flat_map(|batch| &frame.quads[batch.range.start as usize..batch.range.end as usize])
            .map(|q| q.layers)
            .collect::<Vec<_>>()
    };
    let patience = std::time::Duration::from_secs(30);
    client.within(patience, "the tower's shot drawn with its splash", |c| {
        !cards(c).is_empty()
    });
    for layers in cards(&client) {
        assert_eq!(layers[3], 2, "both layers given: {layers:?}");
    }
}

/// The fountain's shots as Dota's (reported: the fountain did not shoot, and then
/// shot with no missile): a hero at the enemy's fountain is shot, and the fountain's own
/// missile effect from the install draws.
#[test]
fn the_fountain_shoots_its_own_missile() {
    use crate::e2e::harness::{Client, Server};
    // The hero takes the shots and stands: no death of its own draws meanwhile.
    let server = Server::start(1, &["--immortal-heroes"]);
    let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
    let map = bota_core::map_of(bota_core::MAP2_ID);
    let theirs = bota_core::fountain_pos(map, bota_proto::Team::Dire);
    let near = bota_proto::Vec2::from_ints(
        theirs.x.to_f32() as i32 - 600,
        theirs.y.to_f32() as i32 - 600,
    );
    let library = client.world().resource::<super::Library>();
    let shot: Vec<u16> = library
        .effect("fountain.dire.attack")
        .expect("the install's fountain missile")
        .defs
        .iter()
        .flat_map(|def| def.draws.iter().map(|draw| draw.look))
        .collect();
    // A card of the missile's looks at the hero: the missile landing on it, not the
    // fountain's own glow, which shares a look and stays at the fountain.
    let at = Vec3::new(near.x.to_f32(), 0.0, -near.y.to_f32());
    let drawn = |c: &Client| {
        let frame = c.world().resource::<super::ParticleFrame>();
        frame
            .batches
            .iter()
            .filter(|batch| shot.contains(&batch.look))
            .flat_map(|batch| &frame.quads[batch.range.start as usize..batch.range.end as usize])
            .any(|q| Vec3::new(q.head[0], 0.0, q.head[2]).distance(at) < 250.0)
    };
    client.cheat(bota_proto::Cheat::Teleport { at: near });
    client.follow_me();
    let patience = std::time::Duration::from_secs(20);
    client.within(patience, "the fountain's missile drawn", |c| drawn(c));
}
