//! The pre-emission operators a system runs each step before it emits, as Source 2 runs them.

use bevy::math::Vec3;
use heatel::particle::{ControlPointPositions, PreEmission};

use super::eval::{ControlPoint, Scene, float};
use super::ops::Target;
use super::store::Particles;

/// A system's emission stopped by one of its pre-emission operators, with what goes with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop {
    /// Its particles go at once.
    pub destroy: bool,
    /// Its end cap plays.
    pub play_end_cap: bool,
}

/// What one pre-emission operator call has: the system's particles and whether it still
/// emits, whether the operator ran once already, and where it puts the control points it sets.
pub struct PreCall<'a> {
    pub parts: &'a Particles,
    pub emitting: bool,
    pub done: &'a mut bool,
    pub writes: &'a mut Vec<(Target, usize, ControlPoint)>,
}

/// Runs `op`; the stop it asks for, if any.
pub fn pre_emit(op: &PreEmission, scene: &Scene<'_>, call: &mut PreCall<'_>) -> Option<Stop> {
    match op {
        PreEmission::SetControlPointPositions(positions) => {
            set_positions(positions, scene, call);
            None
        }
        PreEmission::StopAfterDuration {
            duration,
            destroy,
            play_end_cap,
        } => (call.emitting && scene.age > float(duration, scene, call.parts, None, 0)).then_some(
            Stop {
                destroy: *destroy,
                play_end_cap: *play_end_cap,
            },
        ),
        PreEmission::HsvShiftToCp {
            color,
            enable,
            output,
            default_color,
        } => {
            let points = scene.control_points;
            let mut out = points[output.index()];
            out.position = if points[enable.index()].position.x <= 0.0 {
                Vec3::ZERO
            } else {
                hsv_shift(
                    points[color.index()].position / 255.0,
                    Vec3::from(*default_color),
                )
            };
            call.writes.push((Target::Own, output.index(), out));
            None
        }
        _ => None,
    }
}

fn set_positions(p: &ControlPointPositions, scene: &Scene<'_>, call: &mut PreCall<'_>) {
    if p.set_once && std::mem::replace(call.done, true) {
        return;
    }
    let head = scene.control_points[p.head.index()];
    for &(cp, place) in &p.points {
        let mut point = scene.control_points[cp.index()];
        point.position = if p.world {
            Vec3::from(place)
        } else {
            head.position + head.rotate(Vec3::from(place))
        };
        if p.orient && !p.world {
            (point.forward, point.left, point.up) = (head.forward, head.left, head.up);
        }
        call.writes.push((Target::Own, cp.index(), point));
    }
}

/// The shift from `default`'s hue, saturation and value to `color`'s: the hue's turn (0..1),
/// and the saturation's and value's ratios less one, as Source makes it; no hue turn when the
/// default has no saturation or value to turn.
fn hsv_shift(color: Vec3, default: Vec3) -> Vec3 {
    let (hsv, base) = (hsv(color), hsv(default));
    let mut hue = (hsv.x - base.x) / 360.0;
    if hue < 0.0 {
        hue += 1.0;
    }
    let gate = if base.y > 0.0 && base.z > 0.0 {
        1.0
    } else {
        0.0
    };
    let ratio = |a: f32, b: f32| if b > 0.0 { a / b } else { 1.0 };
    Vec3::new(
        gate * hue,
        ratio(hsv.y, base.y) - 1.0,
        ratio(hsv.z, base.z) - 1.0,
    )
}

/// A colour's hue in degrees, saturation and value.
fn hsv(rgb: Vec3) -> Vec3 {
    let max = rgb.max_element();
    let min = rgb.min_element();
    let chroma = max - min;
    let hue = if chroma <= 0.0 {
        0.0
    } else if max == rgb.x {
        60.0 * ((rgb.y - rgb.z) / chroma).rem_euclid(6.0)
    } else if max == rgb.y {
        60.0 * ((rgb.z - rgb.x) / chroma + 2.0)
    } else {
        60.0 * ((rgb.x - rgb.y) / chroma + 4.0)
    };
    let saturation = if max > 0.0 { chroma / max } else { 0.0 };
    Vec3::new(hue, saturation, max)
}
