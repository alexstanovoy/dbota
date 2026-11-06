//! How far a sound carries.
//!
//! Dota's gain graph (`soundstacks_dota_core`) hands the distance to the source and the sound
//! level of the event to `calc_falloff`, an operator of the engine whose curve the install does
//! not hold. This is **our approximation of it**, and the one place that approximates:
//!
//! - `distance_max`, when the event has one, is where the sound stops being heard
//!   (`core_distance_limiter`), and the gain falls linearly from 1 beside the listener to 0
//!   there;
//! - an event without one carries as far as its sound level does by Source 1's ratio, a sound
//!   reaching `36 · 10^((level − 60) / 20)` units, scaled so that 81 dB reaches 2500 units
//!   (`DOTA_Item.BlinkDagger.Activate` states both).
//!
//! If the real curve is found, it replaces [`falloff`] and the constants below, and nothing else.

/// The sound level (dB) whose reach is known: Blink Dagger's activation.
pub const REFERENCE_LEVEL_DB: f32 = 81.0;
/// The reach (units) of [`REFERENCE_LEVEL_DB`], which `DOTA_Item.BlinkDagger.Activate` states as
/// its `distance_max`.
pub const REFERENCE_REACH: f32 = 2500.0;
/// The sound level of an event the cache does not know: Dota's default for its sound events.
pub const DEFAULT_LEVEL_DB: f32 = 70.0;
/// The longest reach: a sound of the loudest levels is not heard past the map.
pub const LONGEST_REACH: f32 = 100_000.0;

/// How far (units) a sound of `level_db` decibels carries when its event states no
/// `distance_max`.
pub fn reach_of_level(level_db: f32) -> f32 {
    let reach = REFERENCE_REACH * 10f32.powf((level_db - REFERENCE_LEVEL_DB) / 20.0);
    if reach.is_finite() {
        reach.clamp(0.0, LONGEST_REACH)
    } else {
        0.0
    }
}

/// The share of its volume a sound keeps `distance` units from the listener: 1 beside it, 0 at
/// its reach and beyond. `level_factor` multiplies the sound level (the mix group's `lvl`, which
/// a layer may take to 0 to make the group carry nowhere).
pub fn falloff(distance: f32, level_db: f32, distance_max: Option<f32>, level_factor: f32) -> f32 {
    // A level factor of nothing makes a group carry nowhere, beside its source too.
    if level_factor <= 0.0 {
        return 0.0;
    }
    let level = level_db * level_factor;
    let reach = match distance_max {
        // A group's level factor stretches or shrinks the reach by the same ratio it changes the
        // level by.
        Some(max) => max * 10f32.powf((level - level_db) / 20.0),
        None => reach_of_level(level),
    };
    if !distance.is_finite() || reach <= 0.0 {
        return 0.0;
    }
    (1.0 - distance / reach).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_level_reaches_the_reference_distance_and_louder_levels_reach_further() {
        assert!((reach_of_level(81.0) - 2500.0).abs() < 0.01);
        assert!((reach_of_level(75.0) - 2500.0 / 10f32.powf(0.3)).abs() < 0.5);
        assert!(reach_of_level(105.0) > reach_of_level(81.0) * 10.0);
        assert!(reach_of_level(f32::NAN) == 0.0);
    }

    #[test]
    fn a_sound_falls_linearly_to_nothing_at_its_distance_max() {
        for (distance, expected) in [
            (0.0, 1.0),
            (500.0, 0.8),
            (1250.0, 0.5),
            (2500.0, 0.0),
            (9000.0, 0.0),
        ] {
            let got = falloff(distance, 81.0, Some(2500.0), 1.0);
            assert!((got - expected).abs() < 1e-4, "{distance}: {got}");
        }
    }

    #[test]
    fn a_sound_without_a_distance_max_carries_as_its_level_does() {
        let reach = reach_of_level(75.0);
        assert!((falloff(reach / 2.0, 75.0, None, 1.0) - 0.5).abs() < 1e-3);
        assert_eq!(falloff(reach * 1.01, 75.0, None, 1.0), 0.0);
    }

    #[test]
    fn a_group_whose_level_factor_is_zero_carries_nowhere_and_a_bad_distance_is_silent() {
        assert_eq!(falloff(300.0, 75.0, None, 0.0), 0.0);
        for distance in [0.0, 0.1, 300.0] {
            assert_eq!(
                falloff(distance, 81.0, Some(2500.0), 0.0),
                0.0,
                "{distance}"
            );
        }
        assert_eq!(falloff(f32::NAN, 75.0, Some(2500.0), 1.0), 0.0);
    }
}
