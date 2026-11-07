//! Last-frame system bounds used by control-point setters. Source includes particle radii,
//! trail tails and definition padding, then unions finite children's bounds. Empty frames
//! retain the previous box (VRF 2d13e415, ParticleSystemSimulation.Simulation.cs:1157-1229).

use super::{Effect, Node};
use crate::store;
use bevy::math::Vec3;
use heatel::particle::{Renderer, System};

#[derive(Clone, Copy, Default)]
pub(super) struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
    pub valid: bool,
}

pub(super) fn update(effect: &Effect, nodes: &mut [Node]) {
    for index in (0..nodes.len()).rev() {
        let system = &effect.defs[nodes[index].def].system;
        if system.infinite_bounds {
            nodes[index].bounds.valid = true;
            continue;
        }
        let own = own_bounds(system, &nodes[index]);
        let (mut min, mut max) = own.unwrap_or((Vec3::INFINITY, Vec3::NEG_INFINITY));
        let mut valid = own.is_some();
        for &child in &nodes[index].children {
            if nodes[child].bounds.valid && !effect.defs[nodes[child].def].system.infinite_bounds {
                min = min.min(nodes[child].bounds.min);
                max = max.max(nodes[child].bounds.max);
                valid = true;
            }
        }
        nodes[index].bounds.valid = valid;
        if valid {
            nodes[index].bounds.min = min;
            nodes[index].bounds.max = max;
        }
    }
}

fn own_bounds(system: &System, node: &Node) -> Option<(Vec3, Vec3)> {
    if node.parts.is_empty() {
        return None;
    }
    // The last trail renderer decides the tail contribution, independent of its draw frame.
    let trail = system.renderers.iter().rev().find_map(|r| match &r.class {
        Renderer::Trails(t) => Some((t.max_length, !t.ignore_dt)),
        _ => None,
    });
    let (mut min, mut max) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for i in 0..node.parts.len() {
        let position = node.parts.vectors[store::POSITION][i];
        let radius = Vec3::splat(node.parts.scalars[store::RADIUS][i]);
        min = min.min(position - radius);
        max = max.max(position + radius);
        if let Some((most, per_second)) = trail.filter(|(most, _)| *most > 0.0) {
            let delta = node.parts.vectors[store::PREVIOUS][i] - position;
            let distance = delta.length().max(0.001);
            let scale = if per_second && node.last_step != 0.0 {
                node.last_step.recip()
            } else {
                1.0
            };
            let length = (distance * node.parts.scalars[store::TRAIL_LENGTH][i] * scale).min(most);
            let tail = position + delta * (length / distance);
            min = min.min(tail - radius);
            max = max.max(tail + radius);
        }
    }
    Some((
        min + Vec3::from(system.bounds[0]),
        max + Vec3::from(system.bounds[1]),
    ))
}
