//! 60 creep stand-ins wandering smoothly, each with a billboard health bar.
//!
//! Health bars are world-space quads, not UI nodes: all bars share one mesh and three
//! materials, so they batch into a few draws, need no per-frame CPU projection or UI
//! layout, and are depth-sorted with the world. Because the camera never yaws or pitches,
//! the billboard rotation is a constant set once at spawn.

use bevy::{light::NotShadowCaster, prelude::*};

use crate::{camera::camera_rotation, coords::sim_to_world};

const CREEP_COUNT: usize = 60;
const CREEP_SPEED: f32 = 325.0;
const CREEP_RADIUS: f32 = 16.0;
/// Creeps wander inside this half-extent around the origin (sim units).
const ARENA: f32 = 900.0;
const BAR_SIZE: Vec2 = Vec2::new(64.0, 8.0);
const BAR_HEIGHT: f32 = 90.0;

#[derive(Component)]
struct Creep {
    target: Vec2,
    rng: SplitMix64,
}

#[derive(Component)]
struct BarFill {
    owner_phase: f32,
}

/// Tiny deterministic PRNG (no `rand` dependency for a spike).
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    }

    fn point(&mut self) -> Vec2 {
        Vec2::new(self.next_f32() * 2.0 - 1.0, self.next_f32() * 2.0 - 1.0) * ARENA
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn_creeps)
        .add_systems(Update, (move_creeps, update_bars));
}

fn spawn_creeps(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let body = meshes.add(Capsule3d::new(CREEP_RADIUS, 2.0 * CREEP_RADIUS));
    let team_mats = [Color::srgb(0.3, 0.7, 0.25), Color::srgb(0.75, 0.25, 0.2)]
        .map(|c| materials.add(StandardMaterial::from(c)));
    let bar = meshes.add(Rectangle::from_size(BAR_SIZE));
    let unlit = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        ..default()
    };
    let bar_bg = materials.add(unlit(Color::srgb(0.05, 0.05, 0.05)));
    let fill_mats =
        [Color::srgb(0.2, 0.85, 0.2), Color::srgb(0.9, 0.2, 0.15)].map(|c| materials.add(unlit(c)));

    for i in 0..CREEP_COUNT {
        let team = i % 2;
        let mut rng = SplitMix64(i as u64 + 1);
        let start = rng.point();
        let phase = rng.next_f32() * 10.0;
        commands
            .spawn((
                Creep {
                    target: rng.point(),
                    rng,
                },
                Transform::from_translation(sim_to_world(start)),
                Visibility::default(),
            ))
            .with_children(|unit| {
                unit.spawn((
                    Mesh3d(body.clone()),
                    MeshMaterial3d(team_mats[team].clone()),
                    Transform::from_xyz(0.0, 2.0 * CREEP_RADIUS, 0.0),
                ));
                unit.spawn((
                    Mesh3d(bar.clone()),
                    MeshMaterial3d(bar_bg.clone()),
                    Transform::from_xyz(0.0, BAR_HEIGHT, 0.0).with_rotation(camera_rotation()),
                    NotShadowCaster,
                ))
                .with_child((
                    Mesh3d(bar.clone()),
                    MeshMaterial3d(fill_mats[team].clone()),
                    // Slightly toward the camera so it wins the depth test against the bg.
                    Transform::from_xyz(0.0, 0.0, 0.5),
                    NotShadowCaster,
                    BarFill { owner_phase: phase },
                ));
            });
    }
}

fn move_creeps(time: Res<Time>, mut q: Query<(&mut Creep, &mut Transform)>) {
    let step = CREEP_SPEED * time.delta_secs();
    for (mut creep, mut transform) in &mut q {
        let pos = Vec2::new(transform.translation.x, -transform.translation.z);
        let to = creep.target - pos;
        if to.length() <= step {
            creep.target = creep.rng.point();
            continue;
        }
        transform.translation = sim_to_world(pos + to.normalize() * step);
    }
}

fn update_bars(time: Res<Time>, mut q: Query<(&BarFill, &mut Transform)>) {
    for (fill, mut transform) in &mut q {
        // Fake health in [0.1, 1]; left-anchored by shifting the scaled quad.
        let hp = 0.55 + 0.45 * (time.elapsed_secs() * 0.7 + fill.owner_phase).sin();
        transform.scale.x = hp;
        transform.translation.x = -0.5 * BAR_SIZE.x * (1.0 - hp);
    }
}
