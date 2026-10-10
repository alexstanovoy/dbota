//! Simulator <-> Bevy coordinate mapping.
//!
//! Simulator: X east, Y north, units where a hero is ~48 wide. Bevy: right-handed, Y up,
//! -Z forward. Mapping: sim (x, y) -> Bevy (x, 0, -y), 1 sim unit = 1 Bevy unit, ground at
//! y = 0. Sim north is Bevy -Z, i.e. "up" on screen for the fixed-yaw camera. Models face
//! Bevy forward (-Z) at yaw 0 (`GltfConvertCoordinates::rotate_scene_entity` makes glTF +Z
//! fronts do so), and a sim facing angle (radians, CCW from east) maps to yaw `a - pi/2`.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

pub fn world_to_sim(world: Vec3) -> Vec2 {
    Vec2::new(world.x, -world.z)
}

/// Rotation for a model whose front is Bevy forward (-Z) to face sim angle `facing`.
pub fn sim_facing_to_rotation(facing: f32) -> Quat {
    Quat::from_rotation_y(facing - FRAC_PI_2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_facing() {
        let p = Vec2::new(123.0, -456.0);
        assert_eq!(world_to_sim(sim_to_world(p)), p);
        // Sim east (facing 0) must point along Bevy +X.
        let east = sim_facing_to_rotation(0.0) * Vec3::NEG_Z;
        assert!((east - Vec3::X).length() < 1e-6);
        // Sim north (facing pi/2) must point along Bevy -Z.
        let north = sim_facing_to_rotation(FRAC_PI_2) * Vec3::NEG_Z;
        assert!((north - Vec3::NEG_Z).length() < 1e-6);
    }
}
