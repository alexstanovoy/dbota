//! The sun's shadow maps drawn with only the casters whose shadow can fall in a
//! view. Bevy's cascades take every caster in a box around each slice of the
//! camera's frustum stretched toward the sun, and at the RTS camera's angle
//! the far cascade's box holds three times the meshes the camera sees: each
//! one more draw a frame, much of a browser's render work. A caster whose
//! shadow volume, the caster swept along the sunlight down to the lowest
//! ground, misses the view's frustum shades no pixel of it, so leaving it out
//! changes nothing drawn.

use bevy::{
    camera::{
        primitives::{Aabb, Frustum},
        visibility::CascadesVisibleEntities,
    },
    light::SimulationLightSystems,
    math::Vec3A,
    prelude::*,
};

use crate::map::Ground;

/// World units the swept shadow volume is grown by: the texels the shadow
/// filter reads around a pixel and the normal bias, many times over.
const MARGIN: f32 = 128.0;
/// World units below the lowest ground a shadow is still followed: the floor
/// drawn under the map's holes lies 30 under the ground (`map::floor`).
const BELOW_GROUND: f32 = 64.0;

/// The lowest height anything drawn can receive a shadow at, once the ground
/// is known: where a shadow is followed down to.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct ShadowFloor(pub Option<f32>);

pub fn plugin(app: &mut App) {
    app.init_resource::<ShadowFloor>()
        .add_systems(PostUpdate, find_floor.before(bevy::app::AnimationSystems))
        .add_systems(
            PostUpdate,
            cull.after(SimulationLightSystems::CheckLightVisibility),
        );
}

pub(crate) fn find_floor(ground: Res<Ground>, mut floor: ResMut<ShadowFloor>) {
    if ground.is_changed() {
        floor.set_if_neq(ShadowFloor(
            ground.lowest().map(|lowest| lowest - BELOW_GROUND),
        ));
    }
}

/// Drops from each cascade the casters whose shadow cannot reach the view.
fn cull(
    floor: Res<ShadowFloor>,
    mut suns: Query<(&GlobalTransform, &mut CascadesVisibleEntities), With<crate::map::MainSun>>,
    views: Query<&Frustum>,
    casters: Query<(&Aabb, &GlobalTransform)>,
) {
    let Some(floor) = floor.0 else {
        return;
    };
    for (sun, mut cascades) in &mut suns {
        let light = sun.forward().as_vec3();
        for (view, lists) in cascades.entities.iter_mut() {
            let Ok(frustum) = views.get(*view) else {
                continue;
            };
            for list in lists {
                list.entities.retain(|entity| match casters.get(*entity) {
                    Ok((aabb, at)) => {
                        let (low, high) = world_box(aabb, at);
                        shadow_reaches(frustum, (low, high), light, floor)
                    }
                    // Bevy draws a caster without bounds in every cascade.
                    Err(_) => true,
                });
            }
        }
    }
}

/// The corners of a caster's bounds in the world, as an axis-aligned box.
fn world_box(aabb: &Aabb, at: &GlobalTransform) -> (Vec3A, Vec3A) {
    let affine = at.affine();
    let center = affine.transform_point3a(aabb.center);
    let half = affine.matrix3.abs() * aabb.half_extents;
    (center - half, center + half)
}

/// Whether the world box `(low, high)` or its shadow, cast along `light` down
/// to `floor`, may lie in the frustum. A sun at or under the horizon throws
/// shadows without end: everything may.
pub fn shadow_reaches(
    frustum: &Frustum,
    (low, high): (Vec3A, Vec3A),
    light: Vec3,
    floor: f32,
) -> bool {
    light.y > -0.05 || frustum.intersects_obb_identity(&shadow_volume((low, high), light, floor))
}

/// The world box holding a box and its shadow down to `floor` (nothing below
/// it receives one), grown by [`MARGIN`].
fn shadow_volume((low, high): (Vec3A, Vec3A), light: Vec3, floor: f32) -> Aabb {
    // The shadow of its top falls farthest before it meets the floor.
    let reach = ((high.y - floor) / -light.y).max(0.0);
    let shift = Vec3A::from(light) * reach;
    let mut min = low.min(low + shift);
    min.y = min.y.max(floor.min(low.y));
    let grow = Vec3A::splat(MARGIN);
    Aabb::from_min_max((min - grow).into(), (high.max(high + shift) + grow).into())
}

#[cfg(test)]
mod tests {
    use bevy::shape::ViewFrustum;

    use super::*;

    /// A camera 3000 units over the origin looking straight down with the RTS
    /// camera's 25-degree lens: it sees the ground within ~665 units of it.
    fn view() -> Frustum {
        let projection = bevy::math::proj::perspective(25f32.to_radians(), 1.0, 10.0, 10_000.0);
        let camera = Transform::from_xyz(0.0, 3000.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z);
        let clip_from_world = projection * camera.to_matrix().inverse();
        Frustum(ViewFrustum::from_clip_from_world(&clip_from_world))
    }

    /// A pillar 2000 tall standing on the ground at `x`.
    fn pillar(x: f32) -> (Aabb, GlobalTransform) {
        let aabb = Aabb::from_min_max(Vec3::new(-50.0, 0.0, -50.0), Vec3::new(50.0, 2000.0, 50.0));
        (aabb, GlobalTransform::from_xyz(x, 0.0, 0.0))
    }

    /// Sunlight at 45 degrees, falling toward +x.
    fn light() -> Vec3 {
        Vec3::new(1.0, -1.0, 0.0).normalize()
    }

    #[test]
    fn a_caster_out_of_view_whose_shadow_falls_in_it_is_kept() {
        let (aabb, at) = pillar(-1500.0);
        let frustum = view();
        assert!(!shadow_reaches(
            &frustum,
            world_box(&aabb, &at),
            Vec3::NEG_Y,
            0.0
        ));
        assert!(shadow_reaches(
            &frustum,
            world_box(&aabb, &at),
            light(),
            0.0
        ));
    }

    #[test]
    fn a_caster_whose_shadow_falls_away_from_the_view_is_left_out() {
        let (aabb, at) = pillar(1500.0);
        assert!(!shadow_reaches(
            &view(),
            world_box(&aabb, &at),
            light(),
            0.0
        ));
    }

    #[test]
    fn a_shadow_is_followed_down_to_the_floor() {
        // A post 500 tall on a plateau 1000 high: on the plateau its shadow ends at x = -650,
        // short of the view; down a cliff to a floor at 0 it reaches x = +350, into it.
        let aabb = Aabb::from_min_max(Vec3::new(-50.0, 0.0, -50.0), Vec3::new(50.0, 500.0, 50.0));
        let at = GlobalTransform::from_xyz(-1200.0, 1000.0, 0.0);
        let frustum = view();
        assert!(!shadow_reaches(
            &frustum,
            world_box(&aabb, &at),
            light(),
            1000.0
        ));
        assert!(shadow_reaches(
            &frustum,
            world_box(&aabb, &at),
            light(),
            0.0
        ));
    }

    /// The system over a sun's cascade: the caster out of view whose shadow
    /// falls in it stays, the one whose shadow falls away goes, and one
    /// without bounds stays.
    #[test]
    fn a_cascade_keeps_the_casters_whose_shadow_reaches_the_view() {
        use bevy::camera::visibility::VisibleMeshEntities;
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(ShadowFloor(Some(0.0)));
        let view = world.spawn(view()).id();
        let kept = world.spawn(pillar(-1500.0)).id();
        let dropped = world.spawn(pillar(1500.0)).id();
        let unbounded = world
            .spawn(GlobalTransform::from_xyz(1500.0, 0.0, 0.0))
            .id();
        let mut cascades = CascadesVisibleEntities::default();
        let entities = vec![kept, dropped, unbounded];
        cascades
            .entities
            .insert(view, vec![VisibleMeshEntities { entities }]);
        let sun = Transform::default().looking_to(light(), Vec3::Y);
        let sun = world
            .spawn((crate::map::MainSun, GlobalTransform::from(sun), cascades))
            .id();
        world.run_system_once(cull).expect("the cull runs");
        let cascades = world.get::<CascadesVisibleEntities>(sun).expect("cascades");
        assert_eq!(cascades.entities[&view][0].entities, [kept, unbounded]);
    }
}
