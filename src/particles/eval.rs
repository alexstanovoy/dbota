//! Evaluating a function's inputs: constants, draws, ages and attributes through their
//! mappings; and the randomness and noise the functions draw from.

use bevy::math::Vec3;
use heatel::particle::{Bias, Curve, FloatInput, Mapping, Transform, VectorInput};

use super::store::{Particles, slot};

/// The ground's height at a point of the ground plane (Source's x and y).
pub type HeightAt<'a> = &'a dyn Fn(f32, f32) -> f32;

/// What an input may read besides the particle: the system's clock and control points, the
/// ground and the parent system's particles.
pub struct Scene<'a> {
    pub age: f32,
    pub in_end_cap: bool,
    /// How long the end cap has run; 0 before it.
    pub end_cap_age: f32,
    pub control_points: &'a [ControlPoint; 64],
    /// The step's length.
    pub dt: f32,
    /// Changes every step: what varying draws are keyed by.
    pub step_seed: u32,
    /// The system's age when the frame began, and the frame's length: a control point moves
    /// from its previous place to its place over it.
    pub frame_start: f32,
    pub frame_len: f32,
    /// None while the game does not know the ground: what is placed on it stays where it is.
    pub ground: Option<HeightAt<'a>>,
    /// None for a root system.
    pub parent: Option<&'a Particles>,
    /// What the system's own fixed draws are keyed by.
    pub seed: u32,
    /// The world's clock, in seconds: what noise over the world's time reads.
    pub world_time: f32,
    /// Whether the system still emits.
    pub emitting: bool,
}

impl Scene<'_> {
    /// Control point `cp` where it was at system age `time`, turned as it is now.
    pub fn control_point_at(&self, cp: usize, time: f32) -> ControlPoint {
        let mut point = self.control_points[cp];
        if self.frame_len > 0.0 {
            let t = ((time - self.frame_start) / self.frame_len).clamp(0.0, 1.0);
            point.position = point.previous.lerp(point.position, t);
        }
        point
    }

    /// Where `transform` puts a point of its frame at age `time`, and how it turns directions.
    pub fn transform_at(&self, transform: &Transform, time: f32) -> ControlPoint {
        match transform {
            Transform::Identity => ControlPoint::default(),
            Transform::ControlPoint {
                control_point,
                orientation,
                ..
            } => {
                let mut point = self.control_point_at(control_point.index(), time);
                if !orientation {
                    (point.forward, point.left, point.up) = (Vec3::X, Vec3::Y, Vec3::Z);
                }
                point
            }
        }
    }
}

/// A control point: where it is, how it is turned, and where it was a step ago.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlPoint {
    pub position: Vec3,
    pub previous: Vec3,
    pub forward: Vec3,
    pub left: Vec3,
    pub up: Vec3,
}

impl Default for ControlPoint {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            previous: Vec3::ZERO,
            forward: Vec3::X,
            left: Vec3::Y,
            up: Vec3::Z,
        }
    }
}

impl ControlPoint {
    /// A direction in its frame turned into the world's.
    pub fn rotate(&self, v: Vec3) -> Vec3 {
        self.forward * v.x + self.left * v.y + self.up * v.z
    }

    /// A direction in the world's frame turned into its own.
    pub fn unrotate(&self, v: Vec3) -> Vec3 {
        Vec3::new(v.dot(self.forward), v.dot(self.left), v.dot(self.up))
    }

    /// Faces `direction` (unchanged when it has no length).
    pub fn face(&mut self, direction: Vec3) {
        let Some(forward) = direction.try_normalize() else {
            return;
        };
        let (left, up) = basis(forward);
        (self.forward, self.left, self.up) = (forward, left, up);
    }
}

/// Two axes square to `forward`, as Source's `VectorVectors` makes them.
pub fn basis(forward: Vec3) -> (Vec3, Vec3) {
    if forward.x.abs() < 1e-6 && forward.y.abs() < 1e-6 {
        let left = Vec3::NEG_Y;
        return (left, Vec3::new(-forward.z, 0.0, 0.0));
    }
    let right = forward.cross(Vec3::Z).normalize();
    let up = right.cross(forward).normalize();
    (-right, up)
}

/// A draw in 0..1 keyed by `seed` and `salt`: the same pair draws the same.
pub fn draw(seed: u32, salt: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// A draw between `min` and `max`.
pub fn between(seed: u32, salt: u32, min: f32, max: f32) -> f32 {
    min + (max - min) * draw(seed, salt)
}

/// A draw between `min` and `max` raised to `exponent` (counted in quarter steps).
pub fn between_exp(seed: u32, salt: u32, exponent: f32, min: f32, max: f32) -> f32 {
    let x = draw(seed, salt);
    let x = if exponent == 1.0 {
        x
    } else {
        x.powf((exponent * 4.0).trunc() * 0.25)
    };
    min + (max - min) * x
}

/// A point in the unit ball and its distance from the centre, as a share of the radius.
pub fn in_ball(seed: u32, salt: u32) -> (Vec3, f32) {
    for attempt in 0..16 {
        let salt = salt.wrapping_add(attempt * 3);
        let v = Vec3::new(draw(seed, salt), draw(seed, salt + 1), draw(seed, salt + 2)) * 2.0 - 1.0;
        let length = v.length();
        if length <= 1.0 && length > 1e-4 {
            return (v, length);
        }
    }
    (Vec3::X, 1.0)
}

/// Smooth value noise in -1..1.
pub fn noise(p: Vec3) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let s = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let corner = |dx: f32, dy: f32, dz: f32| {
        let c = cell + Vec3::new(dx, dy, dz);
        let key = (c.x as i32 as u32).wrapping_mul(73_856_093)
            ^ (c.y as i32 as u32).wrapping_mul(19_349_663)
            ^ (c.z as i32 as u32).wrapping_mul(83_492_791);
        draw(key, 0x51ED) * 2.0 - 1.0
    };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(corner(0.0, 0.0, 0.0), corner(1.0, 0.0, 0.0), s.x);
    let x10 = lerp(corner(0.0, 1.0, 0.0), corner(1.0, 1.0, 0.0), s.x);
    let x01 = lerp(corner(0.0, 0.0, 1.0), corner(1.0, 0.0, 1.0), s.x);
    let x11 = lerp(corner(0.0, 1.0, 1.0), corner(1.0, 1.0, 1.0), s.x);
    lerp(lerp(x00, x10, s.y), lerp(x01, x11, s.y), s.z)
}

/// Three noise channels at `p`, offset from each other as Source offsets them.
pub fn noise3(p: Vec3) -> Vec3 {
    let second = p + Vec3::new(100_000.5, 300_000.25, 9_000_001.0);
    let third = second + Vec3::new(110_000.25, 310_000.75, 9_100_000.0);
    Vec3::new(noise(p), noise(second), noise(third))
}

/// The curl of vector noise at `p`: a flow without sources or sinks, from three samples
/// offset as Source offsets them.
pub fn curl3(p: Vec3) -> Vec3 {
    let first = noise3(p);
    let at = p + Vec3::new(43.256, -67.89, 1338.2);
    let second = noise3(at);
    let third = noise3(at + Vec3::new(-129.856, -967.23, 2338.98));
    Vec3::new(third.y - second.z, first.z - third.x, second.x - first.y)
}

/// The offset from `p` to the nearest point of a jittered unit cell grid (each cell's point up
/// to `jitter` from its corner, the grid keyed by `seed`), searched in the 27 cells about it.
pub fn worley_offset(p: Vec3, jitter: f32, seed: u32) -> Vec3 {
    let cell = p.floor();
    let f = p - cell;
    let mut best = (f32::MAX, Vec3::ZERO);
    for i in -1..=1 {
        for j in -1..=1 {
            for k in -1..=1 {
                let c = cell + Vec3::new(i as f32, j as f32, k as f32);
                let key = (c.x as i32 as u32).wrapping_mul(73_856_093)
                    ^ (c.y as i32 as u32).wrapping_mul(19_349_663)
                    ^ (c.z as i32 as u32).wrapping_mul(83_492_791);
                let point = Vec3::new(draw(key, seed), draw(key, seed ^ 1), draw(key, seed ^ 2));
                let offset = point * jitter + Vec3::new(i as f32, j as f32, k as f32) - f;
                let distance = offset.length_squared();
                if distance < best.0 {
                    best = (distance, offset);
                }
            }
        }
    }
    best.1
}

/// The radical inverse of `index` in `base`: the `index`-th point of a Halton sequence, 0..1.
pub fn radical_inverse(mut index: u32, base: u32) -> f32 {
    let (mut result, mut fraction) = (0.0f64, 1.0 / f64::from(base));
    while index > 0 {
        result += f64::from(index % base) * fraction;
        index /= base;
        fraction /= f64::from(base);
    }
    result as f32
}

/// Schlick's bias of `x` (0.5 linear).
pub fn bias(x: f32, bias: f32) -> f32 {
    if bias <= 0.0 {
        return 0.0;
    }
    if bias >= 1.0 {
        return 1.0;
    }
    x / ((1.0 - x) * (1.0 / bias - 2.0) + 1.0)
}

fn biased(x: f32, b: &Bias) -> f32 {
    if b.exponential {
        let exponent = if b.parameter >= 0.0 {
            1.0 - b.parameter.clamp(0.0, 1.0)
        } else {
            20.0 - (b.parameter + 1.0).clamp(0.0, 1.0) * 19.0
        };
        if exponent <= 0.0 {
            return 1.0;
        }
        return x.clamp(0.0, 1.0).powf(exponent.min(20.0));
    }
    // A gain or a standard bias: the parameter -1..1 as Schlick's 0..1.
    bias(x, (b.parameter * 0.5 + 0.5).clamp(0.0, 1.0))
}

/// `x` of `a..b` mapped onto `c..d`, clamped to it (a range of no length: `d` from `b` on).
pub fn remap_clamped(x: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    if a == b {
        return if x >= b { d } else { c };
    }
    c + (d - c) * ((x - a) / (b - a)).clamp(0.0, 1.0)
}

pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge0 == edge1 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `x` mapped as the input's mapping says.
pub fn map(mapping: &Mapping, x: f32) -> f32 {
    match mapping {
        Mapping::Direct => x,
        Mapping::Mult(factor) => x * factor,
        Mapping::Remap {
            input,
            output,
            looped,
            bias: b,
        } => {
            let x = if *looped && input[1] != 0.0 {
                input[1] * (x / input[1]).fract()
            } else {
                x
            };
            let range = input[1] - input[0];
            let t = if range == 0.0 {
                f32::from(u8::from(input[0] <= x))
            } else {
                (x - input[0]) / range
            };
            let t = b.as_ref().map_or(t, |b| biased(t.min(1.0), b));
            let value = output[0] + (output[1] - output[0]) * t;
            value.clamp(output[0].min(output[1]), output[0].max(output[1]))
        }
        Mapping::Curve(curve) => evaluate_curve(curve, x),
        Mapping::Notched {
            range,
            inside,
            outside,
        } => {
            if (range[0]..=range[1]).contains(&x) {
                *inside
            } else {
                *outside
            }
        }
        Mapping::Round(kind) => match kind {
            -1 => x.floor(),
            1 => x.ceil(),
            _ => x.round(),
        },
        Mapping::Min(v) => x.min(*v),
        Mapping::Max(v) => x.max(*v),
        Mapping::Mod(v) if *v != 0.0 => x - (x / v).trunc() * v,
        _ => x,
    }
}

/// A Hermite curve at `x`, clamped to its domain's output.
pub fn evaluate_curve(curve: &Curve, x: f32) -> f32 {
    let keys = &curve.keys;
    if keys.len() < 2 {
        return 0.0;
    }
    let x = if curve.looped && curve.domain_max[0] != 0.0 {
        (x / curve.domain_max[0]).fract().abs() * curve.domain_max[0]
    } else {
        x
    };
    let end = keys[1..].partition_point(|k| k[0] < x).min(keys.len() - 2) + 1;
    let (a, b) = (keys[end - 1], keys[end]);
    let dx = b[0] - a[0];
    let t = if dx != 0.0 {
        ((x - a[0]) / dx).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (t2, t3) = (t * t, t * t * t);
    let y = (2.0 * t3 - 3.0 * t2 + 1.0) * a[1]
        + (t3 - 2.0 * t2 + t) * a[3] * dx
        + (-2.0 * t3 + 3.0 * t2) * b[1]
        + (t3 - t2) * b[2] * dx;
    y.max(curve.domain_min[1]).min(curve.domain_max[1])
}

/// A float input for particle `i` (none: the system as a whole), its constant draws keyed by
/// `salt`.
pub fn float(
    input: &FloatInput,
    scene: &Scene<'_>,
    parts: &Particles,
    i: Option<usize>,
    salt: u32,
) -> f32 {
    let seed = i.map_or(0x5EED, |i| parts.seed[i]);
    match input {
        FloatInput::Literal(v) => *v,
        FloatInput::Random { .. } => random(input, scene, seed, salt),
        FloatInput::CollectionAge(m) => map(m, scene.age),
        FloatInput::EndCapAge(m) => map(m, scene.end_cap_age),
        FloatInput::ControlPointComponent {
            control_point,
            component,
            mapping,
        } => {
            let p = scene.control_points[control_point.index()].position;
            map(mapping, p[usize::from(*component).min(2)])
        }
        FloatInput::Unsupported { literal, .. } => *literal,
        other => i.map_or(0.0, |i| particle_float(other, scene, parts, i)),
    }
}

/// A random draw: the same for a particle while it lives, or anew each step when varying.
fn random(input: &FloatInput, scene: &Scene<'_>, seed: u32, salt: u32) -> f32 {
    let FloatInput::Random {
        min,
        max,
        varying,
        bias: b,
        sign_flip,
    } = input
    else {
        return 0.0;
    };
    let key = if *varying {
        seed ^ scene.step_seed.wrapping_mul(0x2545_F491)
    } else {
        seed
    };
    let mut t = draw(key, salt);
    if let Some(b) = b {
        t = biased(t.min(1.0), b);
    }
    let value = (min + (max - min) * t).clamp(min.min(*max), min.max(*max));
    if *sign_flip && draw(key, salt + 37) < 0.5 {
        -value
    } else {
        value
    }
}

/// An input that reads particle `i`'s age or attributes.
fn particle_float(input: &FloatInput, scene: &Scene<'_>, parts: &Particles, i: usize) -> f32 {
    match input {
        FloatInput::ParticleAge(m) => map(m, scene.age - parts.scalars[super::store::CREATION][i]),
        FloatInput::ParticleAgeNormalized(m) => map(m, normalized_age(scene, parts, i)),
        FloatInput::ParticleFloat {
            field,
            initial,
            mapping,
        } => {
            let slot = slot(*field);
            let v = if *initial {
                parts.initial_scalar(slot, i)
            } else {
                parts.scalar(slot, i)
            };
            map(mapping, if field.is_angle() { v.to_degrees() } else { v })
        }
        FloatInput::ParticleSpeed(m) => {
            let step =
                parts.vectors[super::store::POSITION][i] - parts.vectors[super::store::PREVIOUS][i];
            map(
                m,
                if scene.dt > 0.0 {
                    step.length() / scene.dt
                } else {
                    0.0
                },
            )
        }
        FloatInput::ParticleNumber { reverse, mapping } => {
            let n = if *reverse {
                (parts.len() - i) as f32
            } else {
                parts.number[i] as f32
            };
            map(mapping, n)
        }
        FloatInput::ParticleNumberNormalized(m) => map(m, i as f32 / parts.len().max(1) as f32),
        _ => 0.0,
    }
}

pub fn normalized_age(scene: &Scene<'_>, parts: &Particles, i: usize) -> f32 {
    let lifetime = parts.scalars[super::store::LIFETIME][i];
    if lifetime <= 0.0 {
        0.0
    } else {
        (scene.age - parts.scalars[super::store::CREATION][i]) / lifetime
    }
}

/// A vector input for particle `i`, its draws keyed by `salt`.
pub fn vector(
    input: &VectorInput,
    scene: &Scene<'_>,
    parts: &Particles,
    i: Option<usize>,
    salt: u32,
) -> Vec3 {
    let seed = i.map_or(0x5EED, |i| parts.seed[i]);
    match input {
        VectorInput::Literal(v) => Vec3::from(*v),
        VectorInput::Random { min, max } => between3(seed, salt, min, max),
        VectorInput::RandomOffset {
            min,
            max,
            field,
            scale,
        } => {
            let attribute = i.map_or(Vec3::ZERO, |i| parts.vector(slot(*field), i));
            attribute * Vec3::from(*scale) + between3(seed, salt, min, max)
        }
        VectorInput::ControlPointDelta {
            control_point,
            delta,
        } => {
            scene.control_points[control_point.index()].position
                - scene.control_points[delta.index()].position
        }
        VectorInput::ParticleVector {
            field,
            initial,
            scale,
        } => i.map_or(Vec3::ZERO, |i| {
            let slot = slot(*field);
            let v = if *initial {
                parts.initial_vector(slot, i)
            } else {
                parts.vector(slot, i)
            };
            v * Vec3::from(*scale)
        }),
        VectorInput::ParticleVelocity => i.map_or(Vec3::ZERO, |i| {
            let step =
                parts.vectors[super::store::POSITION][i] - parts.vectors[super::store::PREVIOUS][i];
            if scene.dt > 0.0 {
                step / scene.dt
            } else {
                Vec3::ZERO
            }
        }),
        VectorInput::ControlPointValue {
            control_point,
            scale,
        } => scene.control_points[control_point.index()].position * Vec3::from(*scale),
        VectorInput::FloatInterp { .. } | VectorInput::Gradient { .. } => {
            interpolated(input, scene, parts, i, salt)
        }
        VectorInput::Components(c) => Vec3::new(
            float(&c[0], scene, parts, i, salt),
            float(&c[1], scene, parts, i, salt + 1),
            float(&c[2], scene, parts, i, salt + 2),
        ),
        VectorInput::Unsupported { literal, .. } => Vec3::from(*literal),
        _ => Vec3::ZERO,
    }
}

/// A draw per component between `min` and `max`.
fn between3(seed: u32, salt: u32, min: &[f32; 3], max: &[f32; 3]) -> Vec3 {
    Vec3::new(
        between(seed, salt, min[0], max[0]),
        between(seed, salt + 1, min[1], max[1]),
        between(seed, salt + 2, min[2], max[2]),
    )
}

/// A float input's interpolation between two vectors or through a gradient.
fn interpolated(
    input: &VectorInput,
    scene: &Scene<'_>,
    parts: &Particles,
    i: Option<usize>,
    salt: u32,
) -> Vec3 {
    match input {
        VectorInput::FloatInterp {
            input,
            range,
            output,
            clamped,
        } => {
            let t = interp_fraction(float(input, scene, parts, i, salt), *range);
            let t = if *clamped { t.clamp(0.0, 1.0) } else { t };
            Vec3::from(output[0]).lerp(Vec3::from(output[1]), t)
        }
        VectorInput::Gradient {
            input,
            range,
            stops,
        } => gradient(
            stops,
            interp_fraction(float(input, scene, parts, i, salt), *range),
        ),
        _ => Vec3::ZERO,
    }
}

fn interp_fraction(value: f32, range: [f32; 2]) -> f32 {
    let span = range[1] - range[0];
    if span == 0.0 {
        0.0
    } else {
        (value - range[0]) / span
    }
}

/// A colour gradient at `t`, its blends in whole steps of 1/255 as Source's.
fn gradient(stops: &[[f32; 4]], t: f32) -> Vec3 {
    let color = |s: &[f32; 4]| Vec3::new(s[1], s[2], s[3]);
    let Some(first) = stops.first() else {
        return Vec3::ONE;
    };
    let after = stops.partition_point(|s| s[0] <= t);
    if after == 0 {
        return color(first);
    }
    let a = &stops[after - 1];
    let Some(b) = stops.get(after) else {
        return color(a);
    };
    let span = b[0] - a[0];
    if span < 1e-5 {
        return color(a);
    }
    let blended = color(a).lerp(color(b), (t - a[0]) / span) * 255.0;
    blended.trunc() / 255.0
}
