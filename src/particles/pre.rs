//! The pre-emission operators a system runs each step before it emits, as Source 2 runs them.

use bevy::math::{Mat3, Quat, Vec3};
use heatel::particle::{
    ControlPointPositions, DistanceBetweenCps, FloatInput, PreEmission, ScalarExpressionToCp,
};

use super::eval::{ControlPoint, Scene, basis, float, map, remap_clamped, vector};
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
        _ => {
            pre_emit_more(op, scene, call);
            None
        }
    }
}

/// Runs one of the pre-emission operators of the town portal, the Glyph and the trails.
fn pre_emit_more(op: &PreEmission, scene: &Scene<'_>, call: &mut PreCall<'_>) {
    match op {
        PreEmission::SetSingleControlPointPosition {
            control_point,
            position,
            transform,
            set_once,
        } => {
            let place = vector(position, scene, call.parts, None, 0x0F00);
            if *set_once && std::mem::replace(call.done, true) {
                return;
            }
            let frame = scene.transform_at(transform, scene.age);
            let mut point = scene.control_points[control_point.index()];
            point.position = frame.position + frame.rotate(place);
            call.writes
                .push((Target::Own, control_point.index(), point));
        }
        PreEmission::SetControlPointFieldToScalarExpression(e) => expression(e, scene, call),
        PreEmission::DistanceBetweenCpsToCp(d) => distance_between(d, scene, call),
        PreEmission::SetParentControlPointsToChildCp {
            group,
            child_control_point,
            count,
            first_source,
            orientation,
        } => {
            for k in 0..usize::from(*count) {
                let source = scene.control_points[first_source.index() + k];
                let mut point = ControlPoint {
                    position: source.position,
                    previous: source.position,
                    ..ControlPoint::default()
                };
                if *orientation {
                    (point.forward, point.left, point.up) =
                        (source.forward, source.left, source.up);
                }
                let target = Target::ChildPosition(*group, k, *orientation);
                call.writes
                    .push((target, child_control_point.index(), point));
            }
        }
        PreEmission::SetCpOrientationToPointAtCp { .. } => point_at(op, scene, call),
        _ => {}
    }
}

/// `C_OP_SetControlPointFieldToScalarExpression`: the expression of the two inputs, remapped,
/// moved into the control point's component by the interpolation.
fn expression(e: &ScalarExpressionToCp, scene: &Scene<'_>, call: &mut PreCall<'_>) {
    let value = |input: &FloatInput, salt| float(input, scene, call.parts, None, salt);
    let mut out = e
        .expression
        .evaluate(value(&e.input1, 0x0F10), value(&e.input2, 0x0F11));
    if let Some(remap) = &e.output_remap {
        out = map(remap, out);
    }
    let Some(component) = e.component else {
        return;
    };
    let mut point = scene.control_points[e.control_point.index()];
    let amount = value(&e.interpolation, 0x0F12);
    let current = point.position[usize::from(component)];
    if amount != 1.0 {
        out = current + (out - current) * amount.clamp(0.0, 1.0);
    }
    point.position[usize::from(component)] = out;
    call.writes
        .push((Target::Own, e.control_point.index(), point));
}

/// `C_OP_DistanceBetweenCPsToCP`: the distance between two control points, remapped (clamped)
/// into a component of a third.
fn distance_between(d: &DistanceBetweenCps, scene: &Scene<'_>, call: &mut PreCall<'_>) {
    if d.set_once && std::mem::replace(call.done, true) {
        return;
    }
    let points = scene.control_points;
    let distance = points[d.start.index()]
        .position
        .distance(points[d.end.index()].position);
    let [a, b] = d.input;
    let [c, e] = d.output_range;
    let mut point = points[d.output.index()];
    point.position[usize::from(d.component)] = remap_clamped(distance, a, b, c, e);
    call.writes.push((Target::Own, d.output.index(), point));
}

/// `C_OP_SetCPOrientationToPointAtCP`: the output control point turns (by the interpolation's
/// share each step) to face the input one, or away from it, on the ground plane when flat.
fn point_at(op: &PreEmission, scene: &Scene<'_>, call: &mut PreCall<'_>) {
    let PreEmission::SetCpOrientationToPointAtCp {
        input,
        output,
        interpolation,
        flat,
        avoid_singularity,
        point_away,
    } = op
    else {
        return;
    };
    if input == output {
        return;
    }
    let amount = float(interpolation, scene, call.parts, None, 0x0F20);
    let mut point = scene.control_points[output.index()];
    let mut target = scene.control_points[input.index()].position;
    if *flat {
        target.z = point.position.z;
    }
    let direction = if *point_away {
        point.position - target
    } else {
        target - point.position
    };
    let forward = direction.try_normalize().unwrap_or(Vec3::Z);
    let (left, up) = if *avoid_singularity {
        let side = Vec3::new(
            (1.0 - forward.z) * forward.y * forward.y + forward.z,
            0.0,
            -forward.x,
        )
        .normalize_or_zero();
        let left = (side - forward * side.dot(forward)).normalize_or_zero();
        (left, forward.cross(left))
    } else {
        basis(forward)
    };
    let from = Quat::from_mat3(&Mat3::from_cols(point.forward, point.left, point.up));
    let to = Quat::from_mat3(&Mat3::from_cols(forward, left, up));
    let turn = from.normalize().slerp(to.normalize(), amount);
    (point.forward, point.left, point.up) = (turn * Vec3::X, turn * Vec3::Y, turn * Vec3::Z);
    call.writes.push((Target::Own, output.index(), point));
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
