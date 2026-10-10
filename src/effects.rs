//! What flies and bursts: attack missiles, the souls of a requiem, and the
//! ground a raze burns, drawn simply and bright so they read at a glance.

use std::collections::HashMap;

use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};
use bota_client_core::{RAZE_NEAR, REQUIEM, flights};
use bota_proto::{EntityId, Team};

use crate::manifest::Manifest;
use crate::map::Ground;
use crate::session::Session;

/// How high missiles fly over the ground, in world units.
const FLIGHT: f32 = 110.0;
/// Seconds a raze's burst lasts.
const BURST: f32 = 0.9;

/// A missile on the map, by its wire handle.
#[derive(Component)]
struct Missile(EntityId);

/// A burst on the ground, aging to its end.
#[derive(Component)]
struct Burst {
    age: f32,
    material: Handle<StandardMaterial>,
}

/// The shared meshes and materials of the effects.
#[derive(Resource)]
struct Art {
    bolt: Handle<Mesh>,
    soul: Handle<Mesh>,
    ring: Handle<Mesh>,
    disc: Handle<Mesh>,
    radiant: Handle<StandardMaterial>,
    dire: Handle<StandardMaterial>,
    soul_mat: Handle<StandardMaterial>,
    cracks: Option<Handle<Image>>,
}

/// The missile entities by wire handle, and the raze marks already burst.
#[derive(Resource, Default)]
struct Flying {
    missiles: HashMap<EntityId, Entity>,
    burst: Vec<EntityId>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Flying>()
        .add_systems(Startup, make_art)
        .add_systems(Update, (missiles, bursts, age_bursts));
}

fn glow(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        emissive: color.to_linear() * 3.0,
        unlit: true,
        ..default()
    }
}

fn make_art(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
) {
    commands.insert_resource(Art {
        bolt: meshes.add(Sphere::new(14.0)),
        soul: meshes.add(Cuboid::new(16.0, 16.0, 120.0)),
        ring: meshes.add(Annulus::new(220.0, 250.0)),
        disc: meshes.add(Circle::new(250.0)),
        radiant: materials.add(glow(Color::srgb(0.6, 1.0, 0.5))),
        dire: materials.add(glow(Color::srgb(1.0, 0.45, 0.25))),
        soul_mat: materials.add(glow(Color::srgb(0.75, 0.3, 1.0))),
        cracks: manifest.file("particle.ground_cracks").map(|p| server.load(p)),
    });
}

/// Keeps one entity at every missile in flight, where the frame puts it.
fn missiles(
    mut commands: Commands,
    session: NonSend<Session>,
    art: Option<Res<Art>>,
    ground: Res<Ground>,
    mut flying: ResMut<Flying>,
    mut transforms: Query<&mut Transform, With<Missile>>,
) {
    let (core, Some(art)) = (&session.0, art) else {
        return;
    };
    let Some(view) = &core.view else {
        return;
    };
    let shown = flights(view, core.prev_view.as_ref(), core.smooth.alpha());
    let mut alive = Vec::with_capacity(shown.len());
    for f in &shown {
        let ability = f.missile.ability.map(|a| a.0);
        let soul = ability == Some(REQUIEM);
        if ability.is_some() && !soul {
            continue;
        }
        alive.push(f.missile.id);
        let at = Vec3::new(f.x, ground.at(f.x, f.y) + FLIGHT, -f.y);
        let dir = Vec3::new(f.x - f.tail_x, 0.0, -(f.y - f.tail_y));
        let rotation = if dir.length_squared() > 1e-3 {
            Transform::default().looking_to(dir.normalize(), Vec3::Y).rotation
        } else {
            Quat::IDENTITY
        };
        match flying.missiles.get(&f.missile.id).copied() {
            Some(e) => {
                if let Ok(mut t) = transforms.get_mut(e) {
                    t.translation = at;
                    t.rotation = rotation;
                }
            }
            None => {
                let (mesh, material) = if soul {
                    (art.soul.clone(), art.soul_mat.clone())
                } else if f.missile.team == Team::Dire {
                    (art.bolt.clone(), art.dire.clone())
                } else {
                    (art.bolt.clone(), art.radiant.clone())
                };
                let e = commands
                    .spawn((
                        Missile(f.missile.id),
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::from_translation(at).with_rotation(rotation),
                        NotShadowCaster,
                        NotShadowReceiver,
                    ))
                    .id();
                flying.missiles.insert(f.missile.id, e);
            }
        }
    }
    flying.missiles.retain(|id, e| {
        let keep = alive.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
}

/// Sets off a burst where each new raze mark appears.
fn bursts(
    mut commands: Commands,
    session: NonSend<Session>,
    art: Option<Res<Art>>,
    ground: Res<Ground>,
    mut flying: ResMut<Flying>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (core, Some(art)) = (&session.0, art) else {
        return;
    };
    let Some(view) = &core.view else {
        return;
    };
    for p in &view.projectiles {
        let Some(id) = p.ability.map(|a| a.0) else {
            continue;
        };
        if !(RAZE_NEAR..RAZE_NEAR + 3).contains(&id) || flying.burst.contains(&p.id) {
            continue;
        }
        debug!("raze burst at tick {}", view.tick);
        flying.burst.push(p.id);
        if flying.burst.len() > 64 {
            flying.burst.remove(0);
        }
        let (x, y) = (p.pos.x.to_f32(), p.pos.y.to_f32());
        let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        let base = Vec3::new(x, ground.at(x, y) + 6.0, -y);
        let scorch = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.35, 0.15, 0.9),
            base_color_texture: art.cracks.clone(),
            emissive: LinearRgba::rgb(2.0, 0.5, 0.2),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        let flash = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.25, 0.1, 0.55),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        commands.spawn((
            Burst { age: 0.0, material: scorch.clone() },
            Mesh3d(art.disc.clone()),
            MeshMaterial3d(scorch),
            Transform::from_translation(base).with_rotation(flat),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        commands.spawn((
            Burst { age: 0.0, material: flash.clone() },
            Mesh3d(art.ring.clone()),
            MeshMaterial3d(flash),
            Transform::from_translation(base + Vec3::Y * 4.0).with_rotation(flat),
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

/// Ages every burst: the ring grows and every part fades away.
fn age_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut bursts: Query<(Entity, &mut Burst, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, mut b, mut t) in &mut bursts {
        b.age += time.delta_secs();
        let k = (b.age / BURST).min(1.0);
        if k >= 1.0 {
            materials.remove(&b.material);
            commands.entity(e).despawn();
            continue;
        }
        t.scale = Vec3::splat(0.6 + 0.4 * (1.0 - (1.0 - k) * (1.0 - k)));
        if let Some(mut m) = materials.get_mut(&b.material) {
            let a = (1.0 - k) * (1.0 - k);
            m.base_color.set_alpha(a);
        }
    }
}
