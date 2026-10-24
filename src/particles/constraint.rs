//! The constraints `C_OP_BasicMovement` runs after it moves the particles, as Source 2 runs
//! them: passes in which a constraint runs again only after another one moved particles, then
//! the final ones (the world's) once.

use bevy::math::Vec3;
use heatel::particle::{
    Constraint, DistanceToPath, Field, FloatInput, Function, Planar, WorldTrace,
};

use super::eval::{HeightAt, Scene, float, in_ball};
use super::path::bezier;
use super::store::{self, Particles, slot};

/// Most constraints a system's passes keep track of.
const MAX_TRACKED: usize = 128;

/// Runs `constraints` (at `strengths`) on the particles, up to `passes` passes.
pub fn constrain(
    constraints: &[Function<Constraint>],
    strengths: &[f32],
    scene: &Scene<'_>,
    parts: &mut Particles,
    passes: u8,
) {
    if constraints.is_empty() || parts.is_empty() {
        return;
    }
    let count = constraints.len().min(MAX_TRACKED);
    let mut settled = [false; MAX_TRACKED];
    let is_final = |c: &Constraint| matches!(c, Constraint::WorldTrace(_));
    for _ in 0..passes {
        let mut changed = false;
        for k in 0..count {
            if settled[k] || is_final(&constraints[k].class) {
                continue;
            }
            settled[k] = true;
            if strengths.get(k).is_none_or(|s| *s <= 0.0) {
                continue;
            }
            if apply(&constraints[k].class, scene, parts, k as u32) {
                changed = true;
                settled[..count]
                    .iter_mut()
                    .enumerate()
                    .filter(|(j, _)| *j != k)
                    .for_each(|(_, s)| *s = false);
            }
        }
        if !changed {
            break;
        }
    }
    for (k, c) in constraints.iter().enumerate().take(count) {
        if is_final(&c.class) && strengths.get(k).is_some_and(|s| *s > 0.0) {
            apply(&c.class, scene, parts, k as u32);
        }
    }
}

/// One pass of constraint `c` (number `k` of its system); whether it may have moved particles.
fn apply(c: &Constraint, scene: &Scene<'_>, parts: &mut Particles, k: u32) -> bool {
    let salt = 0xC000 + k * 64;
    match c {
        Constraint::DistanceToPath(d) => distance_to_path(d, scene, parts),
        Constraint::Planar(p) => planar(p, scene, parts, salt),
        Constraint::WorldTrace(w) => match scene.ground {
            Some(ground) if !w.disabled => world_trace(w, scene, parts, ground, salt),
            _ => false,
        },
        _ => false,
    }
}

/// `C_OP_ConstrainDistanceToPath`: each particle kept between the least and the largest
/// distance (the largest running from the path's start through its middle to its end) of a
/// point that travels along the path with its age. As Source's, the particles go in blocks of
/// four: a block one of which is past the largest of the three distances is held at the
/// largest throughout.
fn distance_to_path(d: &DistanceToPath, scene: &Scene<'_>, parts: &mut Particles) -> bool {
    let curve = bezier(
        scene,
        &d.path,
        (d.path.start.index(), d.path.end.index()),
        scene.age,
        scene.seed,
    );
    let [start, mid, end] = d.max_distance;
    let mid = if mid < 0.0 { start } else { mid };
    let end = if end < 0.0 { mid } else { end };
    let uniform = mid == start && end == start;
    let largest = start.max(mid).max(end);
    let least = d.min_distance;
    let mut moved = false;
    let mut limits = [largest; 4];
    for block in (0..parts.len()).step_by(4) {
        let lanes = (block..parts.len().min(block + 4)).collect::<Vec<_>>();
        let mut shares = [0.0; 4];
        let mut targets = [Vec3::ZERO; 4];
        let mut beyond = false;
        for (lane, &i) in lanes.iter().enumerate() {
            shares[lane] = share(d, scene, parts, i);
            targets[lane] = curve.at(shares[lane]);
            beyond |= largest * largest
                < parts.vectors[store::POSITION][i].distance_squared(targets[lane]);
        }
        if !uniform && !beyond {
            for (lane, limit) in limits.iter_mut().enumerate().take(lanes.len()) {
                let t = shares[lane];
                *limit = lerp(lerp(start, mid, t), lerp(mid, end, t), t);
            }
        }
        for (lane, &i) in lanes.iter().enumerate() {
            let position = parts.vectors[store::POSITION][i];
            let squared = position.distance_squared(targets[lane]);
            let limit = if uniform || beyond {
                largest
            } else {
                limits[lane]
            };
            let (over, under) = (limit * limit < squared, squared < least * least);
            if !over && !under {
                continue;
            }
            let direction = if squared > 0.0 {
                (position - targets[lane]) / squared.sqrt()
            } else {
                Vec3::ZERO
            };
            parts.vectors[store::POSITION][i] =
                targets[lane] + direction * if under { least } else { limit };
            moved = true;
        }
    }
    moved
}

/// How far along the path the point of particle `i` has travelled, 0..1 (or its manual share).
fn share(d: &DistanceToPath, scene: &Scene<'_>, parts: &Particles, i: usize) -> f32 {
    if d.manual_field != Field::DISABLED {
        return parts.scalar(slot(d.manual_field), i);
    }
    let scale = if d.scale_field != Field::DISABLED {
        parts.scalar(slot(d.scale_field), i)
    } else {
        1.0
    };
    let age = scene.age - parts.scalars[store::CREATION][i];
    (age / (d.travel_time * scale).max(f32::EPSILON)).min(1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `C_OP_PlanarConstraint`: particles that sink behind the plane are pushed back out until
/// their radius rests on it; with a maximum distance only particles that near its point are
/// held (the old code: a block of four wholly, once one of it is).
fn planar(p: &Planar, scene: &Scene<'_>, parts: &mut Particles, salt: u32) -> bool {
    let frame = scene.control_points[p.control_point.index()];
    let origin = if p.global_origin {
        Vec3::from(p.point)
    } else {
        frame.position + frame.rotate(Vec3::from(p.point))
    };
    let normal = if p.global_normal {
        Vec3::from(p.normal)
    } else {
        frame.rotate(Vec3::from(p.normal))
    };
    let Some(normal) = normal.try_normalize() else {
        return false;
    };
    let reach = float(&p.max_distance, scene, parts, None, salt);
    let mut moved = false;
    for block in (0..parts.len()).step_by(4) {
        let end = parts.len().min(block + 4);
        let mut depths = [0.0f32; 4];
        let mut held = false;
        for i in block..end {
            let radius = parts.scalars[store::RADIUS][i]
                * float(&p.radius_scale, scene, parts, Some(i), salt + 1);
            let position = parts.vectors[store::POSITION][i];
            let mut depth = (position - origin).dot(normal) - radius;
            let within = reach <= 0.0 || position.distance_squared(origin) < reach * reach;
            if !p.old_code && !within {
                depth = 0.0;
            }
            depths[i - block] = depth;
            held |= depth < 0.0 && within;
        }
        if !held {
            continue;
        }
        for i in block..end {
            if depths[i - block] < 0.0 {
                parts.vectors[store::POSITION][i] -= normal * depths[i - block];
                moved = true;
            }
        }
    }
    moved
}

/// Units between the samples of a trace against the ground, and most samples of one.
const TRACE_STEP: f32 = 16.0;
const MAX_SAMPLES: usize = 64;

/// Where the segment `from..to` first goes below the ground, as a share of it, and the ground's
/// normal there; none when it stays above. The client's world is its ground: what a particle
/// can meet is the terrain.
fn trace(ground: HeightAt<'_>, from: Vec3, to: Vec3) -> Option<(f32, Vec3)> {
    let above = |p: Vec3| p.z - ground(p.x, p.y);
    if above(from) < 0.0 {
        return None;
    }
    let samples = ((from.distance(to) / TRACE_STEP).ceil() as usize).clamp(1, MAX_SAMPLES);
    let mut low = 0.0;
    let mut hit = None;
    for k in 1..=samples {
        let t = k as f32 / samples as f32;
        if above(from.lerp(to, t)) < 0.0 {
            hit = Some(t);
            break;
        }
        low = t;
    }
    let mut high = hit?;
    for _ in 0..8 {
        let mid = (low + high) * 0.5;
        if above(from.lerp(to, mid)) < 0.0 {
            high = mid;
        } else {
            low = mid;
        }
    }
    let at = from.lerp(to, high);
    let d = 8.0;
    let normal = Vec3::new(
        ground(at.x - d, at.y) - ground(at.x + d, at.y),
        ground(at.x, at.y - d) - ground(at.x, at.y + d),
        2.0 * d,
    )
    .normalize_or(Vec3::Z);
    Some((low, normal))
}

/// `C_OP_WorldTraceConstraint`: a particle whose step (and half its radius on) meets the world
/// stops there, dies there, bounces off or slides along it, or sticks once slow enough.
fn world_trace(
    w: &WorldTrace,
    scene: &Scene<'_>,
    parts: &mut Particles,
    ground: HeightAt<'_>,
    salt: u32,
) -> bool {
    let mut hit_any = false;
    for i in 0..parts.len() {
        let value = |input: &FloatInput, k: u32| float(input, scene, parts, Some(i), salt + k);
        let (bounce, slide) = (value(&w.bounce, 0), value(&w.slide, 1));
        let (scatter, scale) = (
            value(&w.random_direction_scale, 2),
            value(&w.radius_scale, 3),
        );
        let stop = value(&w.stop_speed, 4) * scene.dt;
        if held_stuck(w, parts, i) {
            continue;
        }
        let previous = parts.vectors[store::PREVIOUS][i];
        let step = parts.vectors[store::POSITION][i] - previous;
        let length = step.length();
        if length * length <= f32::EPSILON {
            continue;
        }
        let direction = step / length;
        let half = parts.scalars[store::RADIUS][i] * scale * 0.5;
        let end = parts.vectors[store::POSITION][i] + direction * half;
        let Some((fraction, normal)) = trace(ground, previous, end) else {
            continue;
        };
        hit_any = true;
        let too_slow = w.min_speed >= 0.0 && length < w.min_speed * scene.dt;
        let contact = previous + step * fraction;
        if w.kill_on_contact {
            if w.min_speed < 0.0 || too_slow {
                parts.scalars[store::LIFETIME][i] = 0.0;
            }
            parts.vectors[store::POSITION][i] = contact;
            parts.vectors[store::PREVIOUS][i] = contact;
        } else if too_slow {
            parts.scalars[store::LIFETIME][i] = 0.0;
        }
        let mut write_normal = w.set_normal;
        if !w.kill_on_contact || w.bounce_killed {
            if bounce > 0.0 || slide > 0.0 {
                let seed = parts.seed[i] ^ scene.step_seed.wrapping_mul(0x2545_F491);
                let scattered = in_ball(seed, salt + 8).0 * scatter;
                let outgoing = rebound(w, step, normal, (bounce, slide), scattered);
                parts.vectors[store::POSITION][i] = contact + outgoing * (1.0 - fraction).max(0.0);
                parts.vectors[store::PREVIOUS][i] = parts.vectors[store::POSITION][i] - outgoing;
            } else {
                parts.vectors[store::POSITION][i] = contact;
            }
            if w.stick_field != Field::DISABLED {
                let at = parts.vectors[store::POSITION][i];
                let stopped = !(stop >= 0.0
                    && at.distance_squared(parts.vectors[store::PREVIOUS][i]) >= stop * stop);
                if stopped {
                    parts.set_vector(slot(w.stick_field), i, at);
                }
                write_normal = stopped && w.set_normal;
            }
        }
        if write_normal {
            parts.vectors[store::NORMAL][i] = normal;
        }
    }
    hit_any
}

/// The step a particle takes on after meeting the world: its way reflected (turned by the
/// scatter, of the step's length when the bounce decays) times the bounce, plus its way along
/// the surface times the slide.
fn rebound(
    w: &WorldTrace,
    step: Vec3,
    normal: Vec3,
    (bounce, slide): (f32, f32),
    scattered: Vec3,
) -> Vec3 {
    let length = step.length();
    let direction = step / length;
    let mut reflected = direction - 2.0 * direction.dot(normal) * normal;
    if scattered != Vec3::ZERO {
        reflected = (reflected + scattered).normalize_or_zero();
    }
    if w.decay_bounce {
        reflected *= length;
    }
    let along = step - normal * step.dot(normal);
    reflected * bounce + along * slide
}

/// Whether particle `i` is stuck where it stopped (held there).
fn held_stuck(w: &WorldTrace, parts: &mut Particles, i: usize) -> bool {
    if w.stick_field == Field::DISABLED {
        return false;
    }
    let stuck = parts.vector(slot(w.stick_field), i);
    if stuck.length_squared() <= 0.0 {
        return false;
    }
    parts.vectors[store::POSITION][i] = stuck;
    parts.vectors[store::PREVIOUS][i] = stuck;
    true
}
