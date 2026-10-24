//! The cards a playing effect draws this frame: a sprite, a trail or a rope piece each, written
//! as one instance of the shared quad (see `render.rs`).

use bevy::math::Vec3;
use bytemuck::{Pod, Zeroable};
use heatel::particle::{Blend, FloatInput, Look, Orientation, Renderer, Ropes, Sprites, Trails};

use super::eval::{Scene, float, vector};
use super::library::{Draw, Sequence};
use super::sim::{Instance, Node};
use super::store::{self, Particles};

/// One card: its head edge (centre and half width), its tail edge, the colours there, the
/// sheet frame's and the next frame's texture rectangles (head row first), and the frame
/// blend, whether the colour saturates and the share of what is behind it covers (1 over, 0
/// added).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Quad {
    pub head: [f32; 3],
    pub head_side: [f32; 3],
    pub tail: [f32; 3],
    pub tail_side: [f32; 3],
    pub head_color: [f32; 4],
    pub tail_color: [f32; 4],
    pub uv: [f32; 4],
    pub uv_next: [f32; 4],
    pub extra: [f32; 4],
}

/// The camera in the simulation's frame (Source's: Z up).
#[derive(Clone, Copy)]
pub struct Eye {
    pub position: Vec3,
    pub right: Vec3,
    pub up: Vec3,
}

/// Where the simulation's frame lies in the world's: Bevy's Y up, Z towards the viewer.
pub fn to_world(v: Vec3) -> [f32; 3] {
    [v.x, v.z, -v.y]
}

pub fn from_world(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y)
}

/// Appends the cards of every drawn renderer of `instance`, each to its look's list.
pub fn draw(instance: &Instance, eye: &Eye, out: &mut [Vec<Quad>]) {
    for node in &instance.nodes {
        if node.parts.is_empty() {
            continue;
        }
        let def = &instance.effect.defs[node.def];
        for d in &def.draws {
            let renderer = &def.system.renderers[d.renderer];
            if node.strength_now(&renderer.timing) <= 0.0 {
                continue;
            }
            let Some(list) = out.get_mut(usize::from(d.look)) else {
                continue;
            };
            let first = list.len();
            match &renderer.class {
                Renderer::Sprites(sprites) => draw_sprites(sprites, d, node, eye, list),
                Renderer::Trails(trails) => draw_trails(trails, d, node, eye, list),
                Renderer::Ropes(ropes) => draw_rope(ropes, node, eye, list),
                _ => {}
            }
            if renderer
                .class
                .look()
                .is_some_and(|l| l.blend == Blend::Alpha)
            {
                sort_back_to_front(&mut list[first..], eye);
            }
        }
    }
}

fn sort_back_to_front(quads: &mut [Quad], eye: &Eye) {
    let eye = to_world(eye.position);
    let distance = |q: &Quad| {
        let c = Vec3::from(q.head).lerp(Vec3::from(q.tail), 0.5);
        c.distance_squared(Vec3::from(eye))
    };
    quads.sort_unstable_by(|a, b| distance(b).total_cmp(&distance(a)));
}

/// What a look's scales are this frame, for the system as a whole.
struct Scales {
    radius: f32,
    alpha: f32,
    color: Vec3,
    /// Overbright times one plus the added self.
    gain: f32,
    /// The share of what is behind a card covers.
    cover: f32,
}

fn scales(look: &Look, scene: &Scene<'_>, parts: &Particles) -> Scales {
    let value = |input: &FloatInput, salt: u32| float(input, scene, parts, None, salt);
    Scales {
        radius: value(&look.radius_scale, 1),
        alpha: value(&look.alpha_scale, 2),
        color: vector(&look.color_scale, scene, parts, None, 3),
        gain: value(&look.overbright, 4) * (1.0 + value(&look.add_self, 5)),
        cover: match look.blend {
            Blend::Add | Blend::Mod2x | Blend::Lighten => 0.0,
            Blend::BlendAdd => -1.0,
            Blend::HalfBlendAdd => 0.5,
            Blend::NegHalfBlendAdd => -0.5,
            _ => 1.0,
        },
    }
}

/// Particle `i`'s colour as the card draws it: linear, scaled, its alpha last.
fn color(look: &Look, s: &Scales, parts: &Particles, i: usize) -> [f32; 4] {
    let mut rgb = parts.vectors[store::COLOR][i];
    if look.gamma_vertex_colors {
        rgb = Vec3::new(to_linear(rgb.x), to_linear(rgb.y), to_linear(rgb.z));
    }
    let rgb = rgb * s.color * s.gain;
    [
        rgb.x,
        rgb.y,
        rgb.z,
        (parts.scalars[store::ALPHA][i] * s.alpha).clamp(0.0, 1.0),
    ]
}

fn to_linear(c: f32) -> f32 {
    let c = c.max(0.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The frame of a sheet a particle shows: its index, the next one and the blend between.
fn sheet_frame(look: &Look, sequence: &Sequence, age: f32) -> (usize, usize, f32) {
    let count = sequence.frames.len();
    if count < 2 {
        return (0, 0, 0.0);
    }
    let passes = if look.animate_in_fps {
        age * look.animation_rate / sequence.total.max(1e-6)
    } else {
        age * look.animation_rate
    };
    let position = if sequence.clamp {
        passes.clamp(0.0, 1.0)
    } else {
        passes.rem_euclid(1.0)
    };
    let frame = sequence
        .frames
        .partition_point(|f| f.start <= position)
        .saturating_sub(1)
        .min(count - 1);
    let next = if frame + 1 < count {
        frame + 1
    } else if sequence.clamp {
        frame
    } else {
        0
    };
    let start = sequence.frames[frame].start;
    let end = if frame + 1 < count {
        sequence.frames[frame + 1].start
    } else {
        1.0
    };
    let blend = if end > start {
        ((position - start) / (end - start)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (frame, next, blend)
}

/// A card's texture rectangles and the share of the card the frames' content covers: the
/// window `[x0, y0, x1, y1]` in card coordinates (y down).
struct Frames {
    uv: [f32; 4],
    uv_next: [f32; 4],
    window: [f32; 4],
    blend: f32,
}

fn frames(look: &Look, d: &Draw, parts: &Particles, i: usize, age: f32) -> Frames {
    let whole = Frames {
        uv: [0.0, 0.0, 1.0, 1.0],
        uv_next: [0.0, 0.0, 1.0, 1.0],
        window: [0.0, 0.0, 1.0, 1.0],
        blend: 0.0,
    };
    let Some(sheet) = d.sheet.as_ref().filter(|s| !s.is_empty()) else {
        return transformed(look, whole);
    };
    let number = parts.scalars[store::SEQUENCE][i] as usize;
    let sequence = sheet.get(number).unwrap_or(&sheet[0]);
    if sequence.frames.is_empty() {
        return transformed(look, whole);
    }
    let (frame, next, blend) = sheet_frame(look, sequence, age);
    let (a, b) = (&sequence.frames[frame], &sequence.frames[next]);
    let tile = |f: &super::library::Frame| {
        let [u0, v0, u1, v1] = f.uncropped;
        let (w, h) = ((u1 - u0).max(1e-6), (v1 - v0).max(1e-6));
        [
            (f.cropped[0] - u0) / w,
            (f.cropped[1] - v0) / h,
            (f.cropped[2] - u0) / w,
            (f.cropped[3] - v0) / h,
        ]
    };
    let (ta, tb) = (tile(a), tile(b));
    let window = [
        ta[0].min(tb[0]),
        ta[1].min(tb[1]),
        ta[2].max(tb[2]),
        ta[3].max(tb[3]),
    ];
    let rect = |f: &super::library::Frame| {
        let [u0, v0, u1, v1] = f.uncropped;
        let (w, h) = (u1 - u0, v1 - v0);
        [
            u0 + window[0] * w,
            v0 + window[1] * h,
            u0 + window[2] * w,
            v0 + window[3] * h,
        ]
    };
    let blend = if look.blend_frames { blend } else { 0.0 };
    transformed(
        look,
        Frames {
            uv: rect(a),
            uv_next: rect(b),
            window,
            blend,
        },
    )
}

/// The rectangles under the texture's scale and offset, as the card's corners place them.
fn transformed(look: &Look, mut f: Frames) -> Frames {
    let [scale_v, offset_v, scale_u, offset_u] = look.texture_transform;
    let place = |c: f32, scale: f32, offset: f32| {
        let origin = offset + 0.5;
        (c - 0.5) / if scale == 0.0 { 1.0 } else { scale } + (origin - origin.floor())
    };
    let apply = |r: [f32; 4]| {
        let (w, h) = (r[2] - r[0], r[3] - r[1]);
        [
            r[0] + place(0.0, scale_u, offset_u) * w,
            r[1] + place(0.0, scale_v, offset_v) * h,
            r[0] + place(1.0, scale_u, offset_u) * w,
            r[1] + place(1.0, scale_v, offset_v) * h,
        ]
    };
    f.uv = apply(f.uv);
    f.uv_next = apply(f.uv_next);
    f
}

fn quad(
    head: Vec3,
    head_side: Vec3,
    tail: Vec3,
    tail_side: Vec3,
    colors: ([f32; 4], [f32; 4]),
    f: &Frames,
    look: &Look,
    s: &Scales,
) -> Quad {
    Quad {
        head: to_world(head),
        head_side: to_world(head_side),
        tail: to_world(tail),
        tail_side: to_world(tail_side),
        head_color: colors.0,
        tail_color: colors.1,
        uv: f.uv,
        uv_next: f.uv_next,
        extra: [f.blend, f32::from(u8::from(look.saturate)), s.cover, 0.0],
    }
}

fn draw_sprites(sprites: &Sprites, d: &Draw, node: &Node, eye: &Eye, out: &mut Vec<Quad>) {
    let (scene, parts) = (node.scene_now(), &node.parts);
    let s = scales(&sprites.look, &scene, parts);
    if s.radius == 0.0 || s.alpha == 0.0 {
        return;
    }
    let value = |input: &FloatInput, salt: u32| float(input, &scene, parts, None, salt);
    let (min_size, max_size) = (value(&sprites.min_size, 6), value(&sprites.max_size, 7));
    let (fade_start, fade_end) = (
        value(&sprites.start_fade_size, 8),
        value(&sprites.end_fade_size, 9),
    );
    for i in 0..parts.len() {
        let position = parts.vectors[store::POSITION][i];
        let distance = eye.position.distance(position);
        let radius = parts.scalars[store::RADIUS][i] * s.radius;
        let mut c = color(&sprites.look, &s, parts, i);
        if radius > fade_start * distance {
            if radius >= fade_end * distance {
                continue;
            }
            let fade = 1.0
                - (radius - fade_start * distance) / ((fade_end - fade_start) * distance).max(1e-6);
            c = [c[0] * fade, c[1] * fade, c[2] * fade, c[3]];
        }
        let half = radius.max(min_size * distance).min(max_size * distance);
        if half <= 0.0 || c[3] < 1.0 / 255.0 {
            continue;
        }
        let (right, up) = card_axes(sprites.look.orientation, eye, parts, i);
        let age = scene.age - parts.scalars[store::CREATION][i];
        let f = frames(&sprites.look, d, parts, i, age);
        let (right, up) = (right * half, up * half);
        let [x0, y0, x1, y1] = f.window;
        let origin = position + (x1 + x0 - 1.0) * right + (1.0 - y0 - y1) * up;
        let (right, up) = (right * (x1 - x0), up * (y1 - y0));
        out.push(quad(
            origin + up,
            right,
            origin - up,
            right,
            (c, c),
            &f,
            &sprites.look,
            &s,
        ));
    }
}

/// A card's right and up axes by its orientation, the particle's roll turning them.
fn card_axes(orientation: Orientation, eye: &Eye, parts: &Particles, i: usize) -> (Vec3, Vec3) {
    let (base_right, base_up) = match orientation {
        Orientation::WorldZ => (Vec3::X, Vec3::Y),
        Orientation::ParticleNormal => {
            let normal = parts.vectors[store::NORMAL][i].normalize_or(Vec3::Z);
            let reference = if normal.z.abs() > 0.999 {
                Vec3::X
            } else {
                Vec3::Z
            };
            let up = normal.cross(reference).normalize();
            (up.cross(normal), up)
        }
        Orientation::ScreenZ => (eye.right, Vec3::Z),
        _ => (eye.right, eye.up),
    };
    let roll = parts.scalars[store::ROLL][i];
    let (sin, cos) = roll.sin_cos();
    (
        base_right * cos + base_up * sin,
        base_up * cos - base_right * sin,
    )
}

fn draw_trails(trails: &Trails, d: &Draw, node: &Node, eye: &Eye, out: &mut Vec<Quad>) {
    let (scene, parts) = (node.scene_now(), &node.parts);
    let s = scales(&trails.look, &scene, parts);
    let per_second = if trails.ignore_dt || node.last_step() <= 0.0 {
        1.0
    } else {
        1.0 / node.last_step()
    };
    for i in 0..parts.len() {
        let position = parts.vectors[store::POSITION][i];
        let difference = parts.vectors[store::PREVIOUS][i] - position;
        let distance = difference.length().max(f32::EPSILON);
        let direction = difference / distance;
        let age = scene.age - parts.scalars[store::CREATION][i];
        let mut length =
            trails.length_scale * parts.scalars[store::TRAIL_LENGTH][i] * distance * per_second;
        length *= (age / trails.length_fade_in.max(1e-9)).min(1.0);
        let length = length.min(trails.max_length).max(trails.min_length);
        let radius = (parts.scalars[store::RADIUS][i] * s.radius.max(0.0))
            .min(trails.radius_to_length * length);
        let c = color(&trails.look, &s, parts, i);
        if radius <= 0.0 || c[3] < 1.0 / 255.0 {
            continue;
        }
        let plane = match trails.look.orientation {
            Orientation::ParticleNormal => parts.vectors[store::NORMAL][i],
            Orientation::WorldZ => Vec3::Z,
            _ => eye.position - position,
        };
        let side = direction
            .cross(plane)
            .try_normalize()
            .unwrap_or_else(|| direction.any_orthonormal_vector());
        let center = position + direction * (length * (0.5 - trails.forward_shift));
        let half = direction * (length * 0.5);
        let head_width = radius * float(&trails.head_radius_scale, &scene, parts, Some(i), 10);
        let tail_width = radius * float(&trails.tail_radius_scale, &scene, parts, Some(i), 11);
        let tail_alpha = float(&trails.tail_alpha_scale, &scene, parts, Some(i), 12);
        let tail = [c[0], c[1], c[2], (c[3] * tail_alpha).clamp(0.0, 1.0)];
        let f = frames(&trails.look, d, parts, i, age);
        out.push(quad(
            center - half,
            side * head_width,
            center + half,
            side * tail_width,
            (c, tail),
            &f,
            &trails.look,
            &s,
        ));
    }
}

fn draw_rope(ropes: &Ropes, node: &Node, eye: &Eye, out: &mut Vec<Quad>) {
    let (scene, parts) = (node.scene_now(), &node.parts);
    if parts.len() < 2 {
        return;
    }
    let s = scales(&ropes.look, &scene, parts);
    let world_size = float(&ropes.texture_v_world_size, &scene, parts, None, 13);
    let world_size = if world_size.abs() < 1e-3 {
        1.0
    } else {
        world_size
    };
    let scroll = float(&ropes.texture_v_scroll_rate, &scene, parts, None, 14) * scene.age;
    let mut v = (float(&ropes.texture_v_offset, &scene, parts, None, 15) + scroll) / world_size;
    let pieces = usize::from(ropes.min_tessellation.max(1))
        .next_power_of_two()
        .min(128);
    let point = |k: usize| parts.vectors[store::POSITION][k.min(parts.len() - 1)];
    let mut previous: Option<(Vec3, Vec3, [f32; 4], f32)> = None;
    for span in 0..parts.len() - 1 {
        let (p0, p1, p2, p3) = (
            point(span.saturating_sub(1)),
            point(span),
            point(span + 1),
            point(span + 2),
        );
        for piece in 0..=pieces {
            if span > 0 && piece == 0 {
                continue;
            }
            let t = piece as f32 / pieces as f32;
            let (at, side, c) = rope_point(ropes, &s, parts, eye, [p0, p1, p2, p3], span, t);
            if let Some((last, last_side, last_color, last_v)) = previous {
                v += last.distance(at) / world_size;
                let f = Frames {
                    uv: [0.0, last_v, 1.0, v],
                    uv_next: [0.0, last_v, 1.0, v],
                    window: [0.0, 0.0, 1.0, 1.0],
                    blend: 0.0,
                };
                let f = rope_transform(&ropes.look, f);
                out.push(quad(
                    last,
                    last_side,
                    at,
                    side,
                    (last_color, c),
                    &f,
                    &ropes.look,
                    &s,
                ));
            }
            previous = Some((at, side, c, v));
        }
    }
}

/// A point `t` of the way along span `span` of a rope through `points` (the span's ends and
/// those beside them): where it is, its half width across, and its colour.
fn rope_point(
    ropes: &Ropes,
    s: &Scales,
    parts: &Particles,
    eye: &Eye,
    [p0, p1, p2, p3]: [Vec3; 4],
    span: usize,
    t: f32,
) -> (Vec3, Vec3, [f32; 4]) {
    let at = catmull_rom(p0, p1, p2, p3, t);
    let tangent = catmull_rom_tangent(p0, p1, p2, p3, t).normalize_or(Vec3::X);
    let k = if t < 0.5 { span } else { span + 1 };
    let plane = match ropes.look.orientation {
        Orientation::ParticleNormal => parts.vectors[store::NORMAL][k],
        Orientation::WorldZ => Vec3::Z,
        _ => eye.position - at,
    };
    let side = tangent
        .cross(plane)
        .try_normalize()
        .unwrap_or_else(|| tangent.any_orthonormal_vector());
    let radius = lerp(
        parts.scalars[store::RADIUS][span],
        parts.scalars[store::RADIUS][span + 1],
        t,
    ) * s.radius;
    let (ca, cb) = (
        color(&ropes.look, s, parts, span),
        color(&ropes.look, s, parts, span + 1),
    );
    let c = [
        lerp(ca[0], cb[0], t),
        lerp(ca[1], cb[1], t),
        lerp(ca[2], cb[2], t),
        lerp(ca[3], cb[3], t),
    ];
    (at, side * radius, c)
}

/// A rope's texture runs along it unbounded: only the scale and offset apply, without the
/// card's wrap.
fn rope_transform(look: &Look, mut f: Frames) -> Frames {
    let [scale_v, offset_v, scale_u, offset_u] = look.texture_transform;
    let map = |r: [f32; 4]| {
        [
            r[0] * scale_u + offset_u,
            r[1] * scale_v + offset_v,
            r[2] * scale_u + offset_u,
            r[3] * scale_v + offset_v,
        ]
    };
    f.uv = map(f.uv);
    f.uv_next = map(f.uv_next);
    f
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let (t2, t3) = (t * t, t * t * t);
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
}

fn catmull_rom_tangent(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    0.5 * ((p2 - p0)
        + 2.0 * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t
        + 3.0 * (3.0 * p1 - p0 - 3.0 * p2 + p3) * t2)
}
