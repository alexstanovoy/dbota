//! The contract a missile's effect keeps with the game, on a definition shaped as Dota's
//! missiles are: one particle (of no lifetime: it lives to the end cap) pulled from the launch control point to the target at the speed
//! control point 2 gives, handing its place to its children, and an impact child that waits for
//! the end cap.

use std::sync::Arc;

use bevy::math::Vec3;
use heatel::particle::{
    Child, Constants, ControlPoint, ControlPointsFromParticles, Emitter, EndCap, Field, FloatInput,
    Force, Function, Initializer, Mapping, Operator, SetMethod, Sphere, System, Timing, Transform,
    VectorInput,
};

use super::library::Effect;
use super::sim::Instance;

fn timing(end_cap: EndCap) -> Timing {
    Timing {
        end_cap,
        strength: FloatInput::Literal(1.0),
        fade_in: [0.0; 2],
        fade_out: [0.0; 2],
        oscillate: 0.0,
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
    Initializer::CreateWithinSphere(Box::new(Sphere {
        transform: Transform::point(point(cp)),
        radius_min: FloatInput::Literal(0.0),
        radius_max: FloatInput::Literal(0.0),
        speed_min: FloatInput::Literal(0.0),
        speed_max: FloatInput::Literal(0.0),
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
        emitters: vec![always(Emitter::Instantaneous {
            count: FloatInput::Literal(count),
            start_time: FloatInput::Literal(0.0),
            max_per_frame: -1,
        })],
        initializers: vec![
            always(sphere_at(if children.is_empty() { 3 } else { 0 })),
            always(Initializer::InitFloat {
                field: Field::LIFE_DURATION,
                value: FloatInput::Literal(if children.is_empty() { 0.5 } else { 0.0 }),
                set_method: SetMethod::Replace,
                strength: FloatInput::Literal(1.0),
            }),
        ],
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
    for _ in 0..(seconds * 60.0).round() as usize {
        instance.advance(1.0 / 60.0);
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
