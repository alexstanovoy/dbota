//! The contract a missile's effect keeps with the game, on a definition shaped as Dota's
//! missiles are: one particle (of no lifetime: it lives to the end cap) pulled from the launch control point to the target at the speed
//! control point 2 gives, handing its place to its children, and an impact child that waits for
//! the end cap.

use std::sync::Arc;

use bevy::math::Vec3;
use heatel::particle::{
    Child, Constants, ControlPoint, ControlPointsFromParticles, Emitter, EndCap, Field, FloatInput,
    Force, Function, Initializer, Mapping, Operator, PlaceOnGround, SetMethod, Sphere, System,
    Timing, TraceMiss, Transform, VectorInput,
};

use super::eval::HeightAt;
use super::library::Effect;
use super::sim::Instance;
use super::store;

fn timing(end_cap: EndCap) -> Timing {
    Timing {
        end_cap,
        strength: FloatInput::Literal(1.0),
        fade_in: [0.0; 2],
        fade_out: [0.0; 2],
        oscillate: 0.0,
        run_once: false,
    }
}

fn always<T>(class: T) -> Function<T> {
    Function {
        class,
        timing: timing(EndCap::Always),
    }
}

fn point(index: u8) -> ControlPoint {
    ControlPoint::new(index).unwrap()
}

fn sphere_at(cp: u8) -> Initializer {
    sphere_flying(cp, 0.0)
}

/// Particles at control point `cp` flying out of it at `speed`.
fn sphere_flying(cp: u8, speed: f32) -> Initializer {
    Initializer::CreateWithinSphere(Box::new(Sphere {
        transform: Transform::point(point(cp)),
        radius_min: FloatInput::Literal(0.0),
        radius_max: FloatInput::Literal(0.0),
        speed_min: FloatInput::Literal(speed),
        speed_max: FloatInput::Literal(speed),
        speed_exponent: 1.0,
        local_speed_min: VectorInput::Literal([0.0; 3]),
        local_speed_max: VectorInput::Literal([0.0; 3]),
        distance_bias: VectorInput::Literal([1.0; 3]),
        distance_bias_absolute: [false; 3],
        local: false,
        output: Field::POSITION,
        velocity: Field::POSITION_PREVIOUS,
    }))
}

fn system(
    count: f32,
    operators: Vec<Function<Operator>>,
    forces: Vec<Function<Force>>,
    children: Vec<Child>,
) -> System {
    let lifetime = if children.is_empty() { 0.5 } else { 0.0 };
    let initializers = vec![
        sphere_at(if children.is_empty() { 3 } else { 0 }),
        lifetime_of(lifetime),
    ];
    bare(count, initializers, operators, forces, children)
}

/// A lifetime of `seconds` (0: to the end cap).
fn lifetime_of(seconds: f32) -> Initializer {
    Initializer::InitFloat {
        field: Field::LIFE_DURATION,
        value: FloatInput::Literal(seconds),
        set_method: SetMethod::Replace,
        strength: FloatInput::Literal(1.0),
    }
}

/// A system of one burst of `count` particles and the functions given.
fn bare(
    count: f32,
    initializers: Vec<Initializer>,
    operators: Vec<Function<Operator>>,
    forces: Vec<Function<Force>>,
    children: Vec<Child>,
) -> System {
    System {
        behavior_version: 12,
        first_multiple_override: -1,
        max_particles: 16,
        initial_particles: 0,
        max_time_step: 0.1,
        constants: Constants {
            color: [1.0; 4],
            radius: 5.0,
            lifetime: 0.5,
            rotation: 0.0,
            rotation_speed: 0.0,
            sequence: 0.0,
            normal: [0.0, 0.0, 1.0],
        },
        group: 0,
        pre_emission: Vec::new(),
        emitters: vec![always(Emitter::Instantaneous {
            count: FloatInput::Literal(count),
            start_time: FloatInput::Literal(0.0),
            max_per_frame: -1,
        })],
        initializers: initializers.into_iter().map(always).collect(),
        operators,
        forces,
        constraints: Vec::new(),
        renderers: Vec::new(),
        children,
    }
}

/// A missile: the head, and an impact of five particles where the head is when it ends.
fn missile() -> Arc<Effect> {
    let head = system(
        1.0,
        vec![
            always(Operator::BasicMovement {
                gravity: VectorInput::Literal([0.0; 3]),
                drag: FloatInput::Literal(0.0),
            }),
            always(Operator::MaxVelocity {
                max: FloatInput::ControlPointComponent {
                    control_point: point(2),
                    component: 0,
                    mapping: Mapping::Direct,
                },
                min: FloatInput::Literal(0.0),
            }),
            Function {
                class: Operator::Decay { rope: false },
                timing: timing(EndCap::On),
            },
            always(Operator::SetChildControlPoints(
                ControlPointsFromParticles {
                    group: 0,
                    first_control_point: point(3),
                    count: 1,
                    first_source: FloatInput::Literal(0.0),
                    reverse: false,
                    orientation: true,
                },
            )),
        ],
        vec![always(Force::AttractToControlPoint {
            transform: Transform::point(point(1)),
            amount: FloatInput::Literal(1e6),
            falloff: 0.0,
            min_distance: FloatInput::Literal(0.0),
            component_scale: [1.0; 3],
        })],
        vec![Child {
            path: "impact".to_owned(),
            end_cap: true,
            delay: 0.0,
        }],
    );
    let impact = system(
        5.0,
        vec![always(Operator::Decay { rope: false })],
        Vec::new(),
        Vec::new(),
    );
    Arc::new(Effect::new(&[
        ("head".to_owned(), head),
        ("impact".to_owned(), impact),
    ]))
}

fn run(instance: &mut Instance, seconds: f32) {
    run_on(instance, seconds, None);
}

fn run_on(instance: &mut Instance, seconds: f32, ground: Option<HeightAt<'_>>) {
    for _ in 0..(seconds * 60.0).round() as usize {
        instance.advance(1.0 / 60.0, ground);
    }
}

#[test]
fn a_missile_flies_at_its_speed_to_its_target_and_its_impact_plays_where_it_ends() {
    let mut instance = Instance::new(missile(), 7);
    instance.set_control_point(0, Vec3::ZERO, Vec3::ZERO, true);
    instance.set_control_point(1, Vec3::new(2000.0, 0.0, 0.0), Vec3::ZERO, true);
    instance.set_control_point(2, Vec3::new(1000.0, 0.0, 0.0), Vec3::ZERO, true);

    run(&mut instance, 0.5);
    let head = instance.nodes[0].parts.vectors[0][0];
    assert!(
        (head.x - 500.0).abs() < 25.0,
        "the head is at {head:?} after half a second"
    );
    assert!(
        instance.nodes[1].parts.is_empty(),
        "the impact waits for the end cap"
    );

    instance.end();
    run(&mut instance, 0.1);
    assert!(
        instance.nodes[0].parts.is_empty(),
        "the end cap kills the head"
    );
    let impact = &instance.nodes[1].parts;
    assert_eq!(impact.len(), 5);
    assert!(
        impact.vectors[0].iter().all(|p| p.distance(head) < 150.0),
        "the impact plays at the head"
    );
    assert!(!instance.finished(), "the impact still plays");

    run(&mut instance, 1.0);
    assert!(instance.finished());
}

/// Souls and sparks: a child's particles made where its parent's are, which keep taking their
/// parent's places as those fly.
#[test]
fn a_child_made_from_its_parents_particles_follows_them() {
    let parent = bare(
        3.0,
        vec![sphere_flying(0, 200.0), lifetime_of(0.0)],
        vec![always(Operator::BasicMovement {
            gravity: VectorInput::Literal([0.0; 3]),
            drag: FloatInput::Literal(0.0),
        })],
        Vec::new(),
        vec![Child {
            path: "child".to_owned(),
            end_cap: false,
            delay: 0.0,
        }],
    );
    let created = Initializer::CreateFromParentParticles {
        velocity_scale: 0.0,
        increment: 1.0,
        random: false,
        random_seed: 0,
        sub_frame: true,
    };
    let inherit = Operator::InheritFromParentParticles {
        scale: 1.0,
        field: Field::POSITION,
        increment: 1,
        random: false,
    };
    let child = bare(
        3.0,
        vec![created, lifetime_of(0.0)],
        vec![always(inherit)],
        Vec::new(),
        Vec::new(),
    );
    let effect = Effect::new(&[("parent".to_owned(), parent), ("child".to_owned(), child)]);
    let mut instance = Instance::new(Arc::new(effect), 11);
    instance.set_control_point(0, Vec3::new(100.0, 100.0, 0.0), Vec3::ZERO, true);

    run(&mut instance, 0.5);

    let (parents, children) = (&instance.nodes[0].parts, &instance.nodes[1].parts);
    assert_eq!(children.len(), 3);
    let flown = parents.vectors[store::POSITION][0].distance(Vec3::new(100.0, 100.0, 0.0));
    assert!(flown > 50.0, "the parent's particles fly ({flown})");
    for (k, at) in children.vectors[store::POSITION].iter().enumerate() {
        let to = parents.vectors[store::POSITION][k];
        assert!(
            at.distance(to) < 1e-3,
            "child {k} at {at:?}, its parent at {to:?}"
        );
    }
}

/// The scorch and cracks of a raze: a particle made in the air lands on the ground the game
/// gives, its offset above it, facing up the slope's normal.
#[test]
fn a_particle_placed_on_the_ground_sits_on_the_sampled_ground() {
    let ground = PlaceOnGround {
        offset: FloatInput::Literal(4.0),
        max_trace_length: FloatInput::Literal(2048.0),
        direction: VectorInput::Literal([0.0, 0.0, -1.0]),
        miss: TraceMiss::TraceEnd,
        field: Field::POSITION,
        previous_only: false,
        set_normal: true,
        normal: Field::NORMAL,
        offset_by_radius: 0.0,
        preserve_offset: None,
    };
    let initializers = vec![
        sphere_at(0),
        lifetime_of(1.0),
        Initializer::PositionPlaceOnGround(Box::new(ground)),
    ];
    let system = bare(1.0, initializers, Vec::new(), Vec::new(), Vec::new());
    let mut instance = Instance::new(Arc::new(Effect::new(&[("cracks".to_owned(), system)])), 3);
    instance.set_control_point(0, Vec3::new(500.0, 200.0, 300.0), Vec3::ZERO, true);
    let slope = |x: f32, _: f32| 0.1 * x + 50.0;

    run_on(&mut instance, 0.1, Some(&slope));

    let parts = &instance.nodes[0].parts;
    let at = parts.vectors[store::POSITION][0];
    assert!((at.z - (slope(at.x, at.y) + 4.0)).abs() < 1e-3, "at {at:?}");
    let normal = parts.vectors[store::NORMAL][0];
    let up_the_slope = Vec3::new(-0.1, 0.0, 1.0).normalize();
    assert!(normal.distance(up_the_slope) < 1e-3, "normal {normal:?}");
}
