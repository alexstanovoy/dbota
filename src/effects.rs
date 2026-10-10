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
struct Missile;

/// A burst on the ground, aging to its end.
#[derive(Component)]
struct Burst {
    age: f32,
    material: Handle<StandardMaterial>,
    /// Its full size against a raze's.
    size: f32,
}

/// The ability id of a Blood Grenade in flight.
const GRENADE: u16 = 16;
/// A grenade's burst against a raze's, 300 wide to 250.
const GRENADE_SIZE: f32 = 1.2;

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
    grenade: Handle<StandardMaterial>,
    cracks: Option<Handle<Image>>,
}

/// The missile entities by wire handle, and the raze marks already burst.
#[derive(Resource, Default)]
struct Flying {
    missiles: HashMap<EntityId, Entity>,
    burst: Vec<EntityId>,
    /// Where each grenade in flight was last drawn, to burst where it lands.
    grenades: HashMap<EntityId, (f32, f32)>,
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
        grenade: materials.add(glow(Color::srgb(0.75, 0.04, 0.06))),
        cracks: manifest
            .file("particle.ground_cracks")
            .map(|p| server.load(p)),
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
    mut materials: ResMut<Assets<StandardMaterial>>,
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
        let grenade = ability == Some(GRENADE);
        if ability.is_some() && !soul && !grenade {
            continue;
        }
        if grenade {
            flying.grenades.insert(f.missile.id, (f.x, f.y));
        }
        alive.push(f.missile.id);
        let at = Vec3::new(f.x, ground.at(f.x, f.y) + FLIGHT, -f.y);
        let dir = Vec3::new(f.x - f.tail_x, 0.0, -(f.y - f.tail_y));
        let rotation = if dir.length_squared() > 1e-3 {
            Transform::default()
                .looking_to(dir.normalize(), Vec3::Y)
                .rotation
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
                } else if grenade {
                    (art.bolt.clone(), art.grenade.clone())
                } else if f.missile.team == Team::Dire {
                    (art.bolt.clone(), art.dire.clone())
                } else {
                    (art.bolt.clone(), art.radiant.clone())
                };
                let e = commands
                    .spawn((
                        Missile,
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
    // A grenade gone from the sky has landed: it bursts where it was.
    let landed: Vec<(f32, f32)> = flying
        .grenades
        .iter()
        .filter(|(id, _)| !alive.contains(id))
        .map(|(_, at)| *at)
        .collect();
    flying.grenades.retain(|id, _| alive.contains(id));
    for at in landed {
        burst(
            &mut commands,
            &art,
            &mut materials,
            &ground,
            at,
            GRENADE_SIZE,
            (
                Color::srgba(0.55, 0.02, 0.04, 0.9),
                LinearRgba::rgb(1.2, 0.05, 0.08),
            ),
        );
    }
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
        flying.burst.push(p.id);
        if flying.burst.len() > 64 {
            flying.burst.remove(0);
        }
        let at = (p.pos.x.to_f32(), p.pos.y.to_f32());
        burst(
            &mut commands,
            &art,
            &mut materials,
            &ground,
            at,
            1.0,
            (
                Color::srgba(1.0, 0.35, 0.15, 0.9),
                LinearRgba::rgb(2.0, 0.5, 0.2),
            ),
        );
    }
}

/// Scorches the ground at a spot and rings it with a flash, `size` times a
/// raze's, in a colour.
fn burst(
    commands: &mut Commands,
    art: &Art,
    materials: &mut Assets<StandardMaterial>,
    ground: &Ground,
    (x, y): (f32, f32),
    size: f32,
    (color, glow): (Color, LinearRgba),
) {
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let base = Vec3::new(x, ground.at(x, y) + 6.0, -y);
    let scorch = materials.add(StandardMaterial {
        base_color: color,
        base_color_texture: art.cracks.clone(),
        emissive: glow,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let flash = materials.add(StandardMaterial {
        base_color: color.with_alpha(0.55),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });
    commands.spawn((
        Burst {
            age: 0.0,
            material: scorch.clone(),
            size,
        },
        Mesh3d(art.disc.clone()),
        MeshMaterial3d(scorch),
        Transform::from_translation(base).with_rotation(flat),
        NotShadowCaster,
        NotShadowReceiver,
    ));
    commands.spawn((
        Burst {
            age: 0.0,
            material: flash.clone(),
            size,
        },
        Mesh3d(art.ring.clone()),
        MeshMaterial3d(flash),
        Transform::from_translation(base + Vec3::Y * 4.0).with_rotation(flat),
        NotShadowCaster,
        NotShadowReceiver,
    ));
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
        t.scale = Vec3::splat(b.size * (0.6 + 0.4 * (1.0 - (1.0 - k) * (1.0 - k))));
        if let Some(mut m) = materials.get_mut(&b.material) {
            let a = (1.0 - k) * (1.0 - k);
            m.base_color.set_alpha(a);
        }
    }
}
