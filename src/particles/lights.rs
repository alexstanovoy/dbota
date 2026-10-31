//! The light particle systems cast on what is about them (`C_OP_RenderDeferredLight`): each
//! frame the lights the drawn systems ask for, the strongest at the camera first, go to a pool
//! of point lights spawned once, so a light appearing changes no entity, no component and no
//! pipeline.

use std::f32::consts::PI;

use bevy::light::PointLight;
use bevy::prelude::*;
use heatel::particle::{ColorBlend, DeferredLight, Renderer};

use super::draw::{from_world, to_world};
use super::eval::{float, vector};
use super::plugin::{Playing, tick};
use super::sim::{Instance, Node};
use super::store;
use crate::camera::RtsCamera;

/// Point lights in the pool: the lights shining at once.
const POOL: usize = 16;
/// Most lights gathered in a frame, before the strongest are picked.
const MAX_GATHERED: usize = 1024;
/// Lux one unit of Source's light gives. Dota's sun is 5 × (208, 201, 228) (the map's
/// `lighting.txt`), its brightest channel 5 × 0.776 linear, and this client lights it with
/// 13 000 lux (`map::spawn_sun`): a unit is 13 000 / 3.88 lux.
const LUX_PER_UNIT: f32 = 3350.0;
/// Where a light of the pool waits unused: far under the map, so no view's clusters hold it.
const PARKED: Vec3 = Vec3::new(0.0, -1.0e6, 0.0);
/// How far from their middle, as a part of the shortest reach among them, the lights of one
/// renderer may lie and still shine as one (see [`merge_close`]).
const MERGE_SPREAD: f32 = 0.25;

/// A light one particle asks for: where (the simulation's frame), its colour, its intensity in
/// lumens (negative: a dark light) and its reach.
#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub position: Vec3,
    pub color: LinearRgba,
    pub lumens: f32,
    pub range: f32,
}

/// The pool's point lights.
#[derive(Component)]
struct Pooled;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(PostUpdate, shine.after(tick));
}

/// Spawns the pool once, dark and parked: the views count the same lights from the start.
fn spawn(mut commands: Commands) {
    for _ in 0..POOL {
        commands.spawn((
            Pooled,
            PointLight {
                intensity: 0.0,
                range: 1.0,
                radius: 0.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(PARKED),
        ));
    }
}

/// Appends the lights of every drawn deferred light renderer of `instance`.
pub fn gather(instance: &Instance, out: &mut Vec<Light>) {
    for node in &instance.nodes {
        if node.parts.is_empty() {
            continue;
        }
        for renderer in &instance.effect.defs[node.def].system.renderers {
            let Renderer::DeferredLight(light) = &renderer.class else {
                continue;
            };
            if node.strength_now(&renderer.timing) > 0.0 {
                lights_of(light, node, out);
            }
        }
    }
}

/// The lights of one renderer's particles. As Source makes them: the colour is the particle's
/// (whole bytes) blended with the colour scale, read as sRGB; the reach is the radius times
/// the radius scale; the intensity is the alpha times the alpha scale, a negative one taking
/// light away (the shadow raze's dark light, alpha scale -4, darkens the ground about it; what
/// it lights stops at black, as the tonemapper clamps); the light reaches its intensity at half
/// its reach and falls off with the square of the distance.
fn lights_of(light: &DeferredLight, node: &Node, out: &mut Vec<Light>) {
    let (scene, parts) = (node.scene_now(), &node.parts);
    let radius_scale = float(&light.radius_scale, &scene, parts, None, 1);
    let alpha_scale = float(&light.alpha_scale, &scene, parts, None, 2);
    let scale = convert(
        light.color_blend,
        vector(&light.color_scale, &scene, parts, None, 3),
    );
    let start = out.len();
    for i in 0..parts.len() {
        if out.len() == MAX_GATHERED {
            break;
        }
        let range = parts.scalars[store::RADIUS][i] * radius_scale;
        let intensity = parts.scalars[store::ALPHA][i] * alpha_scale;
        if range <= 0.0 || intensity == 0.0 {
            continue;
        }
        let bytes = (parts.vectors[store::COLOR][i].clamp(Vec3::ZERO, Vec3::ONE) * 255.0).trunc();
        let rgb = blend(light.color_blend, bytes, scale) / 255.0;
        // Bevy's point light gives intensity / (4π d²) lux at d; Source's gives the intensity at
        // half the reach, falling with the square of the distance: lumens = 4π × lux × (r/2)².
        let half = range * 0.5;
        out.push(Light {
            position: parts.vectors[store::POSITION][i],
            color: Color::srgb(rgb.x, rgb.y, rgb.z).to_linear(),
            lumens: 4.0 * PI * LUX_PER_UNIT * intensity * half * half,
            range,
        });
    }
    merge_close(out, start);
}

/// The lights from `start` on, one renderer's, shine as one when they lie close together for
/// their reach (all within [`MERGE_SPREAD`] of the shortest reach from their middle) and all
/// light or all darken: their lumens summed, from their middle weighted by lumens, in their
/// colours weighted alike, as far as the farthest reaches. A lamp's flame keeps a few lights
/// flickering a few tens of units apart, each reaching hundreds: what they light is the same
/// as from one, flickering as their sum does, and every light costs each pixel it reaches.
fn merge_close(out: &mut Vec<Light>, start: usize) {
    let lights = &out[start..];
    if lights.len() < 2 {
        return;
    }
    let dark = lights[0].lumens < 0.0;
    let weight: f32 = lights.iter().map(|l| l.lumens.abs()).sum();
    if weight <= 0.0 || lights.iter().any(|l| (l.lumens < 0.0) != dark) {
        return;
    }
    let middle = lights
        .iter()
        .map(|l| l.position * l.lumens.abs())
        .sum::<Vec3>()
        / weight;
    let shortest = lights.iter().map(|l| l.range).fold(f32::INFINITY, f32::min);
    if lights
        .iter()
        .any(|l| l.position.distance(middle) > MERGE_SPREAD * shortest)
    {
        return;
    }
    let color = lights
        .iter()
        .map(|l| l.color.to_vec4() * l.lumens.abs())
        .sum::<Vec4>()
        / weight;
    let one = Light {
        position: middle,
        color: LinearRgba::from_vec4(color),
        lumens: lights.iter().map(|l| l.lumens).sum(),
        range: lights
            .iter()
            .map(|l| l.range + l.position.distance(middle))
            .fold(0.0, f32::max),
    };
    out.truncate(start);
    out.push(one);
}

/// The colour scale as the blend reads it, in 0..255 or as a factor.
fn convert(kind: ColorBlend, scale: Vec3) -> Vec3 {
    match kind {
        ColorBlend::Multiply2x | ColorBlend::Mod2x => scale * 2.0,
        ColorBlend::Screen => Vec3::ONE - scale,
        ColorBlend::Multiply | ColorBlend::Divide | ColorBlend::Negate | ColorBlend::Luminance => {
            scale
        }
        _ => scale * 255.0,
    }
}

/// A colour of 0..255 channels blended with a converted scale, kept within 0..255.
fn blend(kind: ColorBlend, color: Vec3, scale: Vec3) -> Vec3 {
    let white = Vec3::splat(255.0);
    let rgb = match kind {
        ColorBlend::Divide => {
            let q = color / scale;
            Vec3::select(q.is_finite_mask(), q, Vec3::ZERO)
        }
        ColorBlend::Add => color + scale,
        ColorBlend::Subtract => color - scale,
        ColorBlend::Screen => white - (white - color) * scale,
        ColorBlend::Max => color.max(scale),
        ColorBlend::Min => color.min(scale),
        ColorBlend::Replace => scale,
        ColorBlend::Average => (color + scale) * 0.5,
        ColorBlend::Negate => white - color * scale,
        ColorBlend::Luminance => color * scale.dot(Vec3::new(0.2125, 0.7154, 0.0721)),
        _ => color * scale,
    };
    rgb.clamp(Vec3::ZERO, white)
}

/// Gives the pool the lights gathered this frame, the strongest at the camera first (a dark
/// light by how much it takes away); the rest
/// of the pool goes dark and parks. The particles were stepped after the transforms were
/// propagated, so each light's global transform is set with its transform: it shines where its
/// particle is this frame, not the next.
fn shine(
    mut playing: ResMut<Playing>,
    camera: Query<&GlobalTransform, (With<RtsCamera>, Without<Pooled>)>,
    mut pool: Query<(&mut PointLight, &mut Transform, &mut GlobalTransform), With<Pooled>>,
) {
    let lights = &mut playing.lights[..];
    if let Ok(camera) = camera.single()
        && lights.len() > POOL
    {
        let eye = from_world(camera.translation());
        let strength = |l: &Light| l.lumens.abs() / l.position.distance_squared(eye).max(1.0);
        lights.select_nth_unstable_by(POOL, |a, b| strength(b).total_cmp(&strength(a)));
    }
    for (k, (mut point, mut transform, mut global)) in pool.iter_mut().enumerate() {
        let Some(light) = lights.get(k) else {
            if point.intensity != 0.0 {
                (point.intensity, point.range) = (0.0, 1.0);
                transform.translation = PARKED;
                *global = GlobalTransform::from(*transform);
            }
            continue;
        };
        point.color = light.color.into();
        (point.intensity, point.range) = (light.lumens, light.range);
        transform.translation = Vec3::from(to_world(light.position));
        *global = GlobalTransform::from(*transform);
    }
}
