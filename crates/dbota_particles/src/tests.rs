//! The contract a missile's effect keeps with the game, on a definition shaped as Dota's
//! missiles are: one particle (of no lifetime: it lives to the end cap) pulled from the launch control point to the target at the speed
//! control point 2 gives, handing its place to its children, and an impact child that waits for
//! the end cap.

use std::sync::Arc;

use bevy::math::Vec3;
use heatel::particle::{
    Child, Constants, ControlPoint, ControlPointsFromParticles, Emitter, EndCap, Field, FloatInput,
    Force, Function, Initializer, Mapping, Operator, PlaceOnGround, PreEmission, SetMethod, Sphere,
    System, Timing, TraceMiss, Transform, VectorInput,
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

#[test]
fn model_particles_follow_their_skin_control_point_and_fall_back_to_default_materials() {
    let mut instance = Instance::new(model_skin_effect(), 1);
    instance.advance(0.02, None);
    for (skin, expected) in [
        (5.0, Some(42)),
        (0.0, Some(7)),
        (2.0, Some(7)),
        (64.0, Some(7)),
        (-1.0, Some(7)),
        (f32::NAN, Some(7)),
    ] {
        instance.set_control_point(2, Vec3::new(skin, 0.0, 0.0), Vec3::ZERO, false);
        let mut placed = Vec::new();
        super::models::gather(&instance, &mut placed, 100.0);
        assert_eq!(
            placed.iter().map(|p| p.model).collect::<Vec<_>>(),
            expected.into_iter().collect::<Vec<_>>(),
            "skin {skin}"
        );
    }
}

fn model_skin_effect() -> Arc<Effect> {
    use heatel::particle::{Models, Renderer};
    let renderer = Models {
        models: vec![("models/example.vmdl".to_owned(), 1.0)],
        model_input: None,
        original_model: false,
        ignore_normal: true,
        orient_z: false,
        ignore_radius: false,
        local_scale: false,
        suppress_tint: false,
        center_offset: false,
        scale_control_point: None,
        local_offset: VectorInput::Literal([0.0; 3]),
        local_rotation: VectorInput::Literal([0.0; 3]),
        component_scale: VectorInput::Literal([1.0; 3]),
        manual_selection: FloatInput::Literal(-1.0),
        skin: FloatInput::ControlPointComponent {
            control_point: point(2),
            component: 0,
            mapping: Mapping::Direct,
        },
        override_material: None,
        animated: false,
        radius_scale: FloatInput::Literal(1.0),
        roll_scale: FloatInput::Literal(1.0),
    };
    let mut definition = system(1.0, Vec::new(), Vec::new(), Vec::new());
    definition.renderers = vec![always(Renderer::Models(Box::new(renderer)))];
    let mut effect = Effect::new(&[("example".to_owned(), definition)]);
    let loaded = std::collections::HashMap::from([
        (("models/example.vmdl".to_owned(), 0), 7),
        (("models/example.vmdl".to_owned(), 5), 42),
    ]);
    effect.defs[0].model_draws = super::library::model_draws(&effect.defs[0].system, &loaded);
    Arc::new(effect)
}

#[test]
fn model_draws_map_heatel_groups_to_loaded_geometry_and_cache_their_weights() {
    let mut effect = model_skin_effect();
    let def = &mut Arc::get_mut(&mut effect).expect("unique effect").defs[0];
    let heatel::particle::Renderer::Models(model) = &mut def.system.renderers[0].class else {
        panic!("models");
    };
    model.models = vec![
        ("a".to_owned(), 2.0),
        ("b".to_owned(), 3.0),
        ("missing".to_owned(), 100.0),
    ];
    let loaded = std::collections::HashMap::from([
        (("a".to_owned(), 0), 7),
        (("b".to_owned(), 0), 8),
        (("a".to_owned(), 5), 42),
    ]);
    let draws = super::library::model_draws(&def.system, &loaded);
    assert_eq!(draws.len(), 1);
    assert_eq!(draws[0].skins.len(), 2);
    assert_eq!(
        draws[0].skins[0].models,
        vec![(Some(7), 2.0), (Some(8), 3.0), (None, 100.0)]
    );
    assert_eq!(draws[0].skins[0].total_weight, 5.0);
    assert_eq!(
        draws[0].skins[1].models,
        vec![(Some(42), 2.0), (None, 3.0), (None, 100.0)]
    );
    assert_eq!(draws[0].skins[1].total_weight, 2.0);
}

#[test]
fn a_full_model_budget_drops_only_the_new_model_and_keeps_the_effect() {
    use super::library::{Library, MAX_MODELS, ParticleModel};
    use bevy::prelude::*;
    struct FullPool;
    impl crate::AssetSource for FullPool {
        fn accepts_image(&self, _: &str) -> bool {
            false
        }
        fn accepts_model(&self, file: &str) -> bool {
            file.ends_with(".glb")
        }
        fn image(&self, _: &str) -> Handle<Image> {
            panic!("the fixture has no images")
        }
        fn model(&self, _: &str) -> Handle<bevy::gltf::Gltf> {
            panic!("a full model pool requests no new assets")
        }
    }
    let mut library = Library {
        models: (0..MAX_MODELS)
            .map(|i| ParticleModel {
                file: format!("kept_{i}.glb"),
                gltf: Handle::default(),
            })
            .collect(),
        ..Library::default()
    };
    let system = model_skin_effect().defs[0].system.clone();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "version": 3, "systems": [{"path": "example", "system": system}],
        "models": [
            {"vmdl": "models/overflow.vmdl", "file": "overflow.glb", "skin": 0},
            {"vmdl": "models/example.vmdl", "file": "kept_0.glb", "skin": 0}
        ]
    }))
    .expect("particle file");
    library
        .add("budget", &bytes, &FullPool)
        .expect("one omitted model does not drop the effect");
    assert_eq!(library.models.len(), MAX_MODELS);
    let draw = &library.effect("budget").expect("effect kept").defs[0].model_draws[0];
    assert_eq!(draw.skins[0].models, vec![(Some(0), 1.0)]);
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
        snapshot_file: None,
        snapshot: None,
        snapshot_control_point: point(0),
        first_multiple_override: -1,
        max_particles: 16,
        initial_particles: 0,
        max_time_step: 0.1,
        sleep_after: heatel::particle::SLEEP_AFTER,
        bounds: heatel::particle::BOUNDS,
        infinite_bounds: false,
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
                max_constraint_passes: 3,
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

fn center_setter(average: bool, target: heatel::particle::ParentSet) -> PreEmission {
    PreEmission::SetControlPointToCenter {
        control_point: point(2),
        offset: [1.0, 2.0, 3.0],
        average,
        target,
    }
}

#[test]
fn center_control_points_use_last_bounds_or_the_live_mean_and_retain_empty_bounds() {
    use heatel::particle::ParentSet;
    for average in [false, true] {
        let mut system = system(3.0, Vec::new(), Vec::new(), Vec::new());
        system.bounds = [[-2.0, -4.0, -6.0], [4.0, 6.0, 8.0]];
        system.pre_emission = vec![always(center_setter(average, ParentSet::Own))];
        let mut instance =
            Instance::new(Arc::new(Effect::new(&[("center".to_owned(), system)])), 7);
        instance.advance(1.0 / 60.0, None);
        instance.nodes[0].parts.vectors[store::POSITION].copy_from_slice(&[
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(14.0, 6.0, 0.0),
            Vec3::new(100.0, 0.0, 0.0),
        ]);
        instance.nodes[0].parts.scalars[store::RADIUS].copy_from_slice(&[1.0, 2.0, 3.0]);
        instance.advance(1.0 / 60.0, None);
        instance.advance(1.0 / 60.0, None);
        // Extrema include the unequal radii and the asymmetric definition padding.
        let box_center = Vec3::new(58.0, 5.5, 4.0);
        let expected = if average {
            Vec3::new(1.0 + 124.0 / 3.0, 4.0, 3.0)
        } else {
            box_center
        };
        assert!(
            instance.nodes[0].control_points[2]
                .position
                .distance(expected)
                < 0.001,
            "average={average}: expected {expected:?}, got {:?}",
            instance.nodes[0].control_points[2].position
        );
        instance.nodes[0].parts.clear();
        instance.set_control_point(0, Vec3::splat(1000.0), Vec3::ZERO, true);
        instance.advance(1.0 / 60.0, None);
        assert!(
            instance.nodes[0].control_points[2]
                .position
                .distance(box_center)
                < 0.001,
            "with no live positions to average, the last bounds remain the reference"
        );
    }
}

#[test]
fn center_control_point_parent_scopes_write_positions_without_overwriting_frames() {
    use heatel::particle::ParentSet;
    for (target, first) in [
        (ParentSet::Own, 2),
        (ParentSet::Parent, 1),
        (ParentSet::Root, 0),
    ] {
        let child = |name: &str| Child {
            path: name.to_owned(),
            end_cap: false,
            delay: 0.0,
        };
        let root = system(0.0, Vec::new(), Vec::new(), vec![child("parent")]);
        let parent = system(0.0, Vec::new(), Vec::new(), vec![child("emitter")]);
        let mut emitter = system(3.0, Vec::new(), Vec::new(), vec![child("descendant")]);
        emitter.pre_emission = vec![always(center_setter(true, target))];
        let descendant = system(0.0, Vec::new(), Vec::new(), Vec::new());
        let mut instance = Instance::new(
            Arc::new(Effect::new(&[
                ("root".to_owned(), root),
                ("parent".to_owned(), parent),
                ("emitter".to_owned(), emitter),
                ("descendant".to_owned(), descendant),
            ])),
            8,
        );
        instance.advance(1.0 / 60.0, None);
        instance.nodes[2].parts.vectors[store::POSITION].copy_from_slice(&[Vec3::splat(10.0); 3]);
        for (i, node) in instance.nodes.iter_mut().enumerate() {
            let frame = &mut node.control_points[2];
            frame.position = Vec3::ZERO;
            let rotation = bevy::math::Quat::from_rotation_z(i as f32 * 0.2);
            (frame.forward, frame.left, frame.up) =
                (rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z);
            frame.object_scale = i as f32 + 0.5;
        }
        let frames: Vec<_> = instance.nodes.iter().map(|n| n.control_points[2]).collect();
        instance.advance(1.0 / 60.0, None);
        for (i, node) in instance.nodes.iter().enumerate() {
            let frame = node.control_points[2];
            assert_eq!(
                frame.position,
                if i >= first {
                    Vec3::new(11.0, 12.0, 13.0)
                } else {
                    Vec3::ZERO
                }
            );
            assert_eq!(
                frame.forward, frames[i].forward,
                "a position setter preserves each recipient's basis"
            );
            assert_eq!(frame.left, frames[i].left);
            assert_eq!(frame.up, frames[i].up);
            assert_eq!(
                frame.object_scale, frames[i].object_scale,
                "bound objects retain their scale"
            );
        }
    }
}

#[test]
fn a_center_includes_finite_children_and_omits_infinite_children() {
    use heatel::particle::ParentSet;
    for infinite in [false, true] {
        let mut root = system(
            0.0,
            Vec::new(),
            Vec::new(),
            vec![Child {
                path: "child".to_owned(),
                end_cap: false,
                delay: 0.0,
            }],
        );
        root.pre_emission = vec![always(center_setter(false, ParentSet::Own))];
        let mut child = system(1.0, Vec::new(), Vec::new(), Vec::new());
        child.infinite_bounds = infinite;
        let mut instance = Instance::new(
            Arc::new(Effect::new(&[
                ("root".to_owned(), root),
                ("child".to_owned(), child),
            ])),
            9,
        );
        instance.advance(1.0 / 60.0, None);
        instance.nodes[1].parts.vectors[store::POSITION][0] = Vec3::splat(100.0);
        instance.advance(1.0 / 60.0, None);
        instance.advance(1.0 / 60.0, None);
        let expected = Vec3::new(1.0, 2.0, 3.0)
            + if infinite {
                Vec3::ZERO
            } else {
                Vec3::splat(100.0)
            };
        assert_eq!(instance.nodes[0].control_points[2].position, expected);
    }
}

#[test]
fn distance_constraints_keep_both_sides_of_the_shell_in_local_and_global_frames() {
    use heatel::particle::{Constraint, Distance};
    for global_center in [false, true] {
        let offset = Vec3::new(5.0, -7.0, 50.0);
        let mut system = system(
            4.0,
            vec![always(Operator::BasicMovement {
                gravity: VectorInput::Literal([0.0; 3]),
                drag: FloatInput::Literal(0.0),
                max_constraint_passes: 3,
            })],
            Vec::new(),
            Vec::new(),
        );
        system.constraints = vec![always(Constraint::Distance(Box::new(Distance {
            min_distance: FloatInput::Literal(5.0),
            max_distance: FloatInput::ControlPointComponent {
                control_point: point(2),
                component: 0,
                mapping: Mapping::Direct,
            },
            transform: Transform::point(point(7)),
            center_offset: VectorInput::Literal(offset.to_array()),
            global_center,
        })))];
        let mut instance =
            Instance::new(Arc::new(Effect::new(&[("shell".to_owned(), system)])), 11);
        instance.set_control_point(2, Vec3::new(75.0, 0.0, 0.0), Vec3::ZERO, false);
        instance.advance(1.0 / 60.0, None);
        let frame = &mut instance.nodes[0].control_points[7];
        frame.position = Vec3::new(100.0, 200.0, 300.0);
        frame.previous = frame.position;
        frame.forward = Vec3::Y;
        frame.left = -Vec3::X;
        let center = if global_center {
            offset
        } else {
            frame.position + frame.rotate(offset)
        };
        let positions = [
            center + Vec3::X * 2.0,
            center + Vec3::Y * 100.0,
            center + Vec3::Z * 30.0,
            center + Vec3::Y * 10.0,
        ];
        instance.nodes[0].parts.vectors[store::POSITION].copy_from_slice(&positions);
        instance.nodes[0].parts.vectors[store::PREVIOUS].copy_from_slice(&positions);
        instance.advance(1.0 / 60.0, None);
        let parts = &instance.nodes[0].parts;
        for (position, expected) in parts.vectors[store::POSITION]
            .iter()
            .zip([5.0, 75.0, 30.0, 10.0])
        {
            assert!(
                (position.distance(center) - expected).abs() < 0.001,
                "global={global_center}, {position:?} must be {expected} from {center:?}"
            );
        }
        assert_eq!(
            parts.vectors[store::PREVIOUS],
            positions,
            "the projection preserves the previous positions used by Verlet movement"
        );
    }
}

fn constrained_positions(minimum: f32, maximum: f32, positions: &[Vec3]) -> Vec<Vec3> {
    use heatel::particle::{Constraint, Distance};
    let mut system = system(
        positions.len() as f32,
        vec![always(Operator::BasicMovement {
            gravity: VectorInput::Literal([0.0; 3]),
            drag: FloatInput::Literal(0.0),
            max_constraint_passes: 1,
        })],
        Vec::new(),
        Vec::new(),
    );
    system.constraints = vec![always(Constraint::Distance(Box::new(Distance {
        min_distance: FloatInput::Literal(minimum),
        max_distance: FloatInput::Literal(maximum),
        transform: Transform::Identity,
        center_offset: VectorInput::Literal([0.0; 3]),
        global_center: true,
    })))];
    let mut instance = Instance::new(Arc::new(Effect::new(&[("shell".to_owned(), system)])), 11);
    instance.advance(1.0 / 60.0, None);
    instance.nodes[0].parts.vectors[store::POSITION].copy_from_slice(positions);
    instance.nodes[0].parts.vectors[store::PREVIOUS].copy_from_slice(positions);
    instance.advance(1.0 / 60.0, None);
    instance.nodes[0].parts.vectors[store::POSITION].clone()
}

#[test]
fn distance_constraints_keep_sources_negative_limit_semantics() {
    let positions = [Vec3::X * 2.0, Vec3::Y * 100.0, Vec3::Z * 30.0, Vec3::ZERO];
    assert_eq!(
        constrained_positions(0.0, -1.0, &positions),
        positions,
        "a negative maximum disables the outer shell"
    );
    let constrained = constrained_positions(-5.0, 75.0, &positions);
    for (position, radius) in constrained.iter().zip([75.0, 75.0, 30.0, 0.0]) {
        assert!(
            (position.length() - radius).abs() < 0.001,
            "a negative minimum pushes undersized non-central particles to the outer shell"
        );
    }
}

#[test]
fn a_disabled_outer_shell_never_projects_held_particles_to_a_float_sentinel() {
    // The independent review's negative-minimum/negative-maximum reproducer.
    let positions = [Vec3::X * 2.0, Vec3::Y * 100.0, Vec3::Z * 30.0, Vec3::ZERO];
    let constrained = constrained_positions(-5.0, -1.0, &positions);
    for position in &constrained {
        assert!(position.is_finite());
        assert!(
            position.length() < 1.0e6,
            "held particle flung to {position:?}"
        );
    }
    assert_eq!(
        constrained, positions,
        "a negative minimum requests outer projection, which is inactive with no outer surface"
    );
    let inner_only = constrained_positions(5.0, -1.0, &positions[..3]);
    assert!((inner_only[0].length() - 5.0).abs() < 0.001);
    assert_eq!(inner_only[1..], positions[1..3]);
}

#[test]
fn distance_shell_outer_limit_is_never_inside_the_inner_cutoff() {
    let positions = [
        Vec3::X * 2.0,
        Vec3::Y * 7.0,
        Vec3::Z * 100.0,
        Vec3::X * 30.0,
    ];
    for minimum in [10.0, -10.0] {
        for position in constrained_positions(minimum, 5.0, &positions) {
            assert!(
                (position.length() - 10.0).abs() < 0.001,
                "inverted shell produced {position:?}"
            );
        }
    }
}

#[test]
fn distance_constraints_reseed_the_center_and_keep_sources_four_lane_offset_rule() {
    let positions = [
        Vec3::ZERO,
        Vec3::X * 2.0,
        Vec3::Y * 100.0,
        Vec3::new(6.0, 8.0, 0.0),
    ];
    let constrained = constrained_positions(5.0, 75.0, &positions);
    assert!(
        (constrained[0].length() - 5.0).abs() < 0.001,
        "the centre gets a finite radial direction"
    );
    assert!(constrained[1].distance(Vec3::splat(5.0 / 3.0f32.sqrt())) < 0.001);
    assert_eq!(
        constrained[2], positions[2],
        "zero X offset cannot be normalized in the reseeded block"
    );
    assert_eq!(
        constrained[3],
        Vec3::splat(6.0),
        "the non-central lane replicates its X offset"
    );
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

/// A frame that hitches (a long first frame, then short ones) leaves the particles at the speed
/// they were made with: the displacement a long step took is not carried into each short one.
#[test]
fn a_long_frame_followed_by_short_ones_does_not_fling_the_particles() {
    let flown = |frames: &[f32]| {
        let system = bare(
            1.0,
            vec![sphere_flying(0, 200.0), lifetime_of(0.0)],
            vec![always(Operator::BasicMovement {
                gravity: VectorInput::Literal([0.0; 3]),
                drag: FloatInput::Literal(0.0),
                max_constraint_passes: 3,
            })],
            Vec::new(),
            Vec::new(),
        );
        let mut instance = Instance::new(Arc::new(Effect::new(&[("fly".to_owned(), system)])), 5);
        instance.set_control_point(0, Vec3::ZERO, Vec3::ZERO, true);
        let mut time = 0.0;
        for dt in frames {
            instance.advance(*dt, None);
            time += dt;
        }
        let at = instance.nodes[0].parts.vectors[store::POSITION][0];
        (at.length(), time)
    };
    let (steady, _) = flown(&[1.0 / 250.0; 250]);
    let mut hitch = vec![0.25];
    hitch.extend([1.0 / 250.0; 250]);
    let (hitched, time) = flown(&hitch);
    // 200 units a second, over the whole time it flew.
    assert!((steady - 200.0).abs() < 10.0, "steady flight {steady}");
    assert!(
        (hitched - 200.0 * time).abs() < 20.0,
        "after a long first frame {hitched} units in {time} s"
    );
}

/// A particle born at the very end of a step keeps the speed it was born with through a short
/// step and a long one after it (a frame hitch): its displacement spans the step it was born in,
/// not the next one.
#[test]
fn a_particle_born_at_a_steps_end_keeps_its_speed_through_a_hitch() {
    let mut system = bare(
        1.0,
        vec![sphere_flying(0, 200.0), lifetime_of(0.0)],
        vec![always(Operator::BasicMovement {
            gravity: VectorInput::Literal([0.0; 3]),
            drag: FloatInput::Literal(0.0),
            max_constraint_passes: 3,
        })],
        Vec::new(),
        Vec::new(),
    );
    system.emitters = vec![always(Emitter::Instantaneous {
        count: FloatInput::Literal(1.0),
        start_time: FloatInput::Literal(0.1),
        max_per_frame: -1,
    })];
    let mut instance = Instance::new(Arc::new(Effect::new(&[("fly".to_owned(), system)])), 5);
    instance.set_control_point(0, Vec3::ZERO, Vec3::ZERO, true);
    let mut flown = Vec::new();
    for dt in [0.1, 0.001, 0.2] {
        instance.advance(dt, None);
        let parts = &instance.nodes[0].parts;
        flown.push(parts.vectors[store::POSITION].first().map(|p| p.length()));
    }
    // Born at 0.1 s, the end of the first step (which moves it a step ahead, as every particle
    // born in a step is), then 200 units a second: 0.2 units in the millisecond, 40 in the 0.2 s.
    let [Some(born), Some(short), Some(long)] = flown[..] else {
        panic!("one particle each frame: {flown:?}");
    };
    assert!(
        (short - born - 0.2).abs() < 0.01,
        "the 1 ms step: {flown:?}"
    );
    assert!(
        (long - short - 40.0).abs() < 0.5,
        "the 0.2 s step: {flown:?}"
    );
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
            max_constraint_passes: 3,
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

#[test]
fn flagbearer_banner_particles_copy_their_attachment_orientation_and_object_scale() {
    let effect = flagbearer_banner_definition();
    let mut instance = Instance::new(effect, 1);
    instance.set_control_point(0, Vec3::new(100.0, 200.0, 50.0), Vec3::Y, true);
    instance.advance(0.02, None);
    let node = &instance.nodes[0];
    assert_eq!(
        node.control_points[7].position,
        Vec3::ONE,
        "an unbound object has unit scale"
    );
    assert!(!node.parts.is_empty());
    assert!((node.parts.scalars[store::YAW][0] - std::f32::consts::FRAC_PI_2).abs() < 0.00001);
    assert_eq!(
        node.parts.vectors[store::POSITION][0],
        Vec3::new(80.0, 207.0, 50.0)
    );
    instance.nodes[0].parts.vectors[store::NORMAL][0] = Vec3::X;
    instance.bind_control_point(
        0,
        Vec3::new(300.0, 400.0, 75.0),
        bevy::math::Mat3::from_rotation_z(std::f32::consts::PI),
        0.93,
    );
    instance.advance(0.02, None);
    let node = &instance.nodes[0];
    assert_eq!(node.control_points[7].position, Vec3::splat(0.93));
    assert_eq!(
        node.parts.vectors[store::POSITION][0],
        Vec3::new(280.0, 407.0, 75.0)
    );
    assert_eq!(
        node.parts.vectors[store::PREVIOUS][0],
        node.parts.vectors[store::POSITION][0]
    );
    assert!((node.parts.scalars[store::YAW][0].abs() - std::f32::consts::PI).abs() < 0.00001);
    assert!((node.parts.vectors[store::NORMAL][0] - Vec3::Y).length() < 0.00001);
}

fn flagbearer_banner_definition() -> Arc<Effect> {
    let mut definition = system(
        1.0,
        vec![
            always(Operator::RemapOrientationToRotations {
                transform: Transform::point(point(0)),
                rotation_offset: [0.0; 3],
                use_quaternion: false,
                write_normal: false,
            }),
            always(Operator::SetToControlPoint {
                control_point: point(0),
                offset: [-20.0, 7.0, 0.0],
                local: false,
            }),
            always(Operator::NormalLock {
                control_point: point(0),
            }),
        ],
        Vec::new(),
        Vec::new(),
    );
    definition.initializers = vec![always(sphere_at(0))];
    definition.pre_emission = vec![always(
        heatel::particle::PreEmission::SetControlPointFromObjectScale {
            input: point(0),
            output: point(7),
        },
    )];
    Arc::new(Effect::new(&[("banner".to_owned(), definition)]))
}

#[test]
fn flagbearer_angle_offsets_compose_locally_at_both_pitch_gimbal_locks() {
    for pitch in [-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2] {
        let instance = flagbearer_particle_instance(
            vec![always(Operator::RemapOrientationToRotations {
                transform: Transform::point(point(0)),
                rotation_offset: [0.0, 90.0, 0.0],
                use_quaternion: false,
                write_normal: true,
            })],
            bevy::math::Quat::from_rotation_y(pitch),
        );
        // A local quarter-turn about Z sends forward to Y, which either pitch leaves at Y.
        let normal = instance.nodes[0].parts.vectors[store::NORMAL][0];
        assert!(
            (normal - Vec3::Y).length() < 0.00001,
            "pitch {pitch}: {normal:?}"
        );
    }
}

#[test]
fn flagbearer_quaternion_offsets_read_roll_pitch_yaw_and_write_all_rotation_columns() {
    for (offset, forward, up) in [
        ([90.0, 0.0, 0.0], Vec3::X, Vec3::NEG_Y),
        ([0.0, 90.0, 0.0], Vec3::NEG_Z, Vec3::X),
        ([0.0, 0.0, 90.0], Vec3::Y, Vec3::Z),
    ] {
        let instance = flagbearer_particle_instance(
            vec![always(Operator::RemapOrientationToRotations {
                transform: Transform::point(point(0)),
                rotation_offset: offset,
                use_quaternion: true,
                write_normal: false,
            })],
            bevy::math::Quat::IDENTITY,
        );
        let parts = &instance.nodes[0].parts;
        let rotation = bevy::math::Quat::from_euler(
            bevy::math::EulerRot::ZYX,
            parts.scalar(store::slot(Field::YAW), 0),
            parts.scalar(store::slot(Field::PITCH), 0),
            parts.scalar(store::slot(Field::ROLL), 0),
        );
        assert!(
            (rotation * Vec3::X - forward).length() < 0.00001,
            "{offset:?}"
        );
        assert!((rotation * Vec3::Z - up).length() < 0.00001, "{offset:?}");
    }
}

#[test]
fn flagbearer_local_control_point_offsets_snap_position_and_previous_position() {
    let instance = flagbearer_particle_instance(
        vec![always(Operator::SetToControlPoint {
            control_point: point(0),
            offset: [-20.0, 7.0, 0.0],
            local: true,
        })],
        bevy::math::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
    );
    let parts = &instance.nodes[0].parts;
    assert!((parts.vectors[store::POSITION][0] - Vec3::new(3.0, 0.0, 30.0)).length() < 0.00001);
    assert_eq!(
        parts.vectors[store::PREVIOUS][0],
        parts.vectors[store::POSITION][0]
    );
}

#[test]
fn flagbearer_normal_lock_keeps_a_unit_normal_for_a_particle_born_halfway_through_the_step() {
    let mut instance = flagbearer_particle_instance(
        vec![always(Operator::NormalLock {
            control_point: point(0),
        })],
        bevy::math::Quat::IDENTITY,
    );
    let node = &mut instance.nodes[0];
    node.parts.vectors[store::NORMAL][0] = Vec3::X;
    node.parts.scalars[store::CREATION][0] = node.age + 0.01;
    instance.bind_control_point(
        0,
        Vec3::new(10.0, 20.0, 30.0),
        bevy::math::Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2),
        1.0,
    );
    instance.advance(0.02, None);
    let normal = instance.nodes[0].parts.vectors[store::NORMAL][0];
    assert!(
        (normal.length() - 1.0).abs() < 0.00001,
        "normal length: {normal:?}"
    );
    let diagonal = Vec3::new(
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
        0.0,
    );
    assert!(
        (normal - diagonal).length() < 0.00001,
        "half turn: {normal:?}"
    );
}

#[test]
fn flagbearer_absolute_normal_and_delta_lock_obey_the_definition_order() {
    let mut instance = flagbearer_particle_instance(
        vec![
            always(Operator::RemapOrientationToRotations {
                transform: Transform::point(point(0)),
                rotation_offset: [0.0; 3],
                use_quaternion: true,
                write_normal: true,
            }),
            always(Operator::NormalLock {
                control_point: point(0),
            }),
        ],
        bevy::math::Quat::IDENTITY,
    );
    instance.bind_control_point(
        0,
        Vec3::new(10.0, 20.0, 30.0),
        bevy::math::Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2),
        1.0,
    );
    instance.advance(0.02, None);
    // The writer sets Y, then the lock applies a further quarter-turn: combining the two
    // operations is permitted, and order matters. Neither silently disables the other.
    assert!((instance.nodes[0].parts.vectors[store::NORMAL][0] - Vec3::NEG_X).length() < 0.00001);
}

fn flagbearer_particle_instance(
    operators: Vec<Function<Operator>>,
    rotation: bevy::math::Quat,
) -> Instance {
    let definition = bare(1.0, vec![sphere_at(0)], operators, Vec::new(), Vec::new());
    let effect = Arc::new(Effect::new(&[("banner".to_owned(), definition)]));
    let mut instance = Instance::new(effect, 1);
    instance.bind_control_point(
        0,
        Vec3::new(10.0, 20.0, 30.0),
        bevy::math::Mat3::from_quat(rotation),
        1.0,
    );
    instance.advance(0.02, None);
    instance
}

/// A system that keeps `count` times control point 1's x particles alive, each living
/// `lifetime` seconds: all it lacks at once, or `rate` a second.
fn maintained(count: f32, lifetime: f32, rate: Option<f32>) -> Arc<Effect> {
    let mut system = bare(
        count,
        vec![sphere_at(0), lifetime_of(lifetime)],
        vec![always(Operator::Decay { rope: false })],
        Vec::new(),
        Vec::new(),
    );
    system.max_particles = 64;
    system.emitters = vec![always(Emitter::Maintain {
        count: FloatInput::Literal(count),
        scale: FloatInput::ControlPointComponent {
            control_point: point(1),
            component: 0,
            mapping: Mapping::Direct,
        },
        start_time: FloatInput::Literal(0.0),
        duration: FloatInput::Literal(0.0),
        rate: rate.map(FloatInput::Literal),
    })];
    Arc::new(Effect::new(&[("maintained".to_owned(), system)]))
}

/// C_OP_MaintainEmitter (Dota's progress circle): as many alive as its count scaled by what the
/// control point gives, made up again as they die, more as the scale rises.
#[test]
fn a_maintain_emitter_keeps_its_scaled_count_alive() {
    let mut instance = Instance::new(maintained(40.0, 0.3, None), 3);
    instance.set_control_point(0, Vec3::ZERO, Vec3::ZERO, true);
    instance.set_control_point(1, Vec3::new(0.25, 0.0, 0.0), Vec3::ZERO, true);
    run(&mut instance, 1.0);
    assert_eq!(
        instance.nodes[0].parts.len(),
        10,
        "a quarter of 40, kept past their lives"
    );
    instance.set_control_point(1, Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, true);
    run(&mut instance, 0.1);
    assert_eq!(
        instance.nodes[0].parts.len(),
        40,
        "all 40 once the scale is whole"
    );
}

/// One not instantaneous makes up what it lacks at its rate, no faster.
#[test]
fn a_maintain_emitter_with_a_rate_makes_up_its_count_at_that_rate() {
    let mut instance = Instance::new(maintained(40.0, 10.0, Some(20.0)), 3);
    instance.set_control_point(0, Vec3::ZERO, Vec3::ZERO, true);
    instance.set_control_point(1, Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, true);
    run(&mut instance, 1.0);
    let made = instance.nodes[0].parts.len();
    assert!((19..=21).contains(&made), "20 a second: {made} after one");
    run(&mut instance, 2.0);
    assert_eq!(instance.nodes[0].parts.len(), 40, "no more than it keeps");
}

/// A handle from the previous match cannot name a newly started generation.
#[test]
fn resetting_the_simulation_retires_previous_match_handles() {
    let effect = Arc::new(Effect::new(&[(
        "reset".to_owned(),
        system(1.0, vec![], vec![], vec![]),
    )]));
    let mut simulation = super::Simulation::default();
    let previous = simulation.start(&effect, 1).expect("the first effect");
    simulation.reset();
    assert!(
        !simulation.contains(previous),
        "the previous match's effect ended"
    );
    let current = simulation
        .start(&effect, 2)
        .expect("the next match's effect");
    assert_ne!(previous, current, "retired generations cannot be reused");
    assert!(
        !simulation.contains(previous),
        "the stale handle stays retired"
    );
    assert!(
        simulation.contains(current),
        "the new match's effect is live"
    );
}

/// The portrait's effects step on the frames the portrait is drawn, by the time since; a
/// portrait not drawn for long (another unit shown, or none) owes them no more than
/// `STAGE_OWED_MAX`, stepped as the world's effects are, by the portrait's frame at most, so
/// their clock moves no further than their particles do.
#[test]
fn a_portrait_effect_long_undrawn_steps_no_more_than_it_may_owe() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let mut playing = super::Simulation::default();
    let handle = playing.hold_on_stage(&missile(), 1).expect("started");
    playing.advance(1.0 / 60.0, Some(5.0), None);
    let instance = playing.get_mut(handle).expect("still playing");
    assert!(
        (instance.world_time - super::plugin::STAGE_OWED_MAX).abs() < 1e-4,
        "stepped {} s",
        instance.world_time
    );
}

/// An effect played on the portrait's stage and ended goes to the spares; started again in the
/// world it is the world's, and each start is counted where it was made.
#[test]
fn a_spare_of_the_stage_started_in_the_world_plays_in_the_world() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let effect = missile();
    let mut playing = super::Simulation::default();
    let staged = playing.hold_on_stage(&effect, 1).expect("started");
    playing.get_mut(staged).expect("playing").end();
    for _ in 0..600 {
        playing.advance(1.0 / 60.0, Some(1.0 / 60.0), None);
    }
    assert!(
        playing.get_mut(staged).is_none(),
        "the stage's effect finished"
    );
    let world = playing.start(&effect, 2).expect("started");
    assert!(
        !playing.get_mut(world).expect("playing").portrait,
        "the world's"
    );
    assert_eq!(playing.started(&effect), 1);
    assert_eq!(playing.started_on_stage(&effect), 1);
}

fn sequential_path_instance(count: FloatInput, distance: FloatInput, continuous: bool) -> Instance {
    use heatel::particle::{Path, SequentialPath};
    let path = SequentialPath {
        max_distance: distance,
        count,
        looped: true,
        pairs: false,
        save_offset: false,
        path: Path {
            start: point(3),
            mid: None,
            end: point(4),
            bulge_control: 0,
            bulge: 0.0,
            mid_point: 0.5,
            start_offset: [0.0; 3],
            mid_offset: [0.0; 3],
            end_offset: [0.0; 3],
        },
    };
    let mut definition = bare(
        16.0,
        vec![
            Initializer::CreateSequentialPath(Box::new(path)),
            lifetime_of(10.0),
        ],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    if continuous {
        definition.emitters = vec![always(Emitter::Continuous {
            rate: FloatInput::Literal(20.0),
            start_time: FloatInput::Literal(0.0),
            duration: FloatInput::Literal(0.0),
        })];
    }
    let effect = Arc::new(Effect::new(&[("sequential".to_owned(), definition)]));
    let mut instance = Instance::new(effect, 42);
    instance.set_control_point(3, Vec3::ZERO, Vec3::X, true);
    instance.set_control_point(4, Vec3::X * 100.0, Vec3::X, true);
    instance
}

fn control_point_input(index: u8) -> FloatInput {
    FloatInput::ControlPointComponent {
        control_point: point(index),
        component: 0,
        mapping: Mapping::Direct,
    }
}

#[test]
fn sequential_path_evaluates_literal_and_control_point_counts() {
    for count in [4, 16] {
        let input = if count == 16 {
            FloatInput::Literal(16.0)
        } else {
            control_point_input(5)
        };
        let mut instance = sequential_path_instance(input, FloatInput::Literal(0.0), false);
        instance.set_control_point(5, Vec3::X * count as f32, Vec3::X, true);
        instance.advance(0.02, None);
        let parts = &instance.nodes[0].parts;
        for index in 0..count {
            let expected = Vec3::X * (100.0 * index as f32 / (count - 1) as f32);
            assert!(parts.vectors[store::POSITION][index].distance(expected) < 0.001);
        }
    }
}

#[test]
fn sequential_path_count_inputs_use_whole_points_within_the_particle_limit() {
    for (input, expected_second) in [
        (-5.0, 0.0),
        (0.0, 0.0),
        (0.5, 0.0),
        (1.0, 0.0),
        (1.9, 0.0),
        (2.75, 100.0),
        (1e30, 100.0 / 4999.0),
        (f32::INFINITY, 100.0 / 4999.0),
        (f32::NAN, 0.0),
    ] {
        let mut instance =
            sequential_path_instance(FloatInput::Literal(input), FloatInput::Literal(0.0), false);
        instance.advance(0.02, None);
        assert_eq!(
            instance.nodes[0].parts.len(),
            16,
            "layout count does not control emission"
        );
        let position = instance.nodes[0].parts.vectors[store::POSITION][1];
        assert!(
            (position.x - expected_second).abs() < 1e-5,
            "count {input}: second point {position}, expected x={expected_second}"
        );
    }
}

#[test]
fn sequential_path_negative_distance_places_points_on_the_path() {
    for distance in [-2.0, f32::NEG_INFINITY, f32::NAN] {
        let mut instance = sequential_path_instance(
            FloatInput::Literal(1.0),
            FloatInput::Literal(distance),
            false,
        );
        instance.advance(0.02, None);
        assert_eq!(
            instance.nodes[0].parts.len(),
            16,
            "negative distance keeps the burst"
        );
        assert!(
            instance.nodes[0].parts.vectors[store::POSITION]
                .iter()
                .all(|p| *p == Vec3::ZERO)
        );
    }
}

#[test]
fn sequential_path_fractional_distance_keeps_subunit_jitter() {
    let mut instance =
        sequential_path_instance(FloatInput::Literal(1.0), FloatInput::Literal(0.25), false);
    instance.advance(0.02, None);
    assert_eq!(
        instance.nodes[0].parts.len(),
        16,
        "fractional distance keeps the burst"
    );
    let positions = &instance.nodes[0].parts.vectors[store::POSITION];
    assert!(positions.iter().all(|p| p.abs().max_element() <= 0.25));
    assert!(positions.iter().any(|p| p.length_squared() > 0.01));
}

#[test]
fn sequential_path_huge_distance_keeps_positions_and_squared_lengths_finite() {
    for distance in [f32::MAX, f32::INFINITY, f32::NAN] {
        let mut instance = sequential_path_instance(
            FloatInput::Literal(1.0),
            FloatInput::Literal(distance),
            false,
        );
        instance.advance(0.02, None);
        assert_eq!(
            instance.nodes[0].parts.len(),
            16,
            "huge distance keeps the burst"
        );
        for position in &instance.nodes[0].parts.vectors[store::POSITION] {
            assert!(position.is_finite(), "finite position: {position}");
            assert!(
                position.length_squared().is_finite(),
                "finite squared length: {position}"
            );
        }
    }
}

#[test]
fn sequential_path_refreshes_count_when_control_points_change_between_batches() {
    let mut instance =
        sequential_path_instance(control_point_input(5), FloatInput::Literal(0.0), true);
    instance.set_control_point(5, Vec3::X * 3.0, Vec3::X, true);
    instance.advance(0.1, None);
    assert_eq!(instance.nodes[0].parts.len(), 2);
    instance.set_control_point(5, Vec3::X * 5.0, Vec3::X, true);
    instance.advance(0.1, None);
    let positions = &instance.nodes[0].parts.vectors[store::POSITION];
    assert_eq!(positions.len(), 4);
    assert!((positions[2].x - 100.0).abs() < 0.001);
    assert!(
        (positions[3].x - 25.0).abs() < 0.001,
        "new input changes spacing: {:?}",
        positions[3]
    );
    instance.set_control_point(5, Vec3::X * 0.5, Vec3::X, true);
    instance.advance(0.1, None);
    assert_eq!(
        &instance.nodes[0].parts.vectors[store::POSITION][4..],
        &[Vec3::ZERO; 2]
    );
}

#[test]
fn sequential_path_refreshes_distance_when_control_points_change_between_batches() {
    let mut instance =
        sequential_path_instance(FloatInput::Literal(1.0), control_point_input(6), true);
    instance.set_control_point(6, Vec3::ZERO, Vec3::X, true);
    instance.advance(0.1, None);
    assert_eq!(
        &instance.nodes[0].parts.vectors[store::POSITION][..2],
        &[Vec3::ZERO; 2]
    );
    instance.set_control_point(6, Vec3::X * 0.25, Vec3::X, true);
    instance.advance(0.1, None);
    let positions = &instance.nodes[0].parts.vectors[store::POSITION];
    assert!(positions[2..].iter().all(|p| p.abs().max_element() <= 0.25));
    assert!(positions[2..].iter().any(|p| p.length_squared() > 0.001));
}

#[test]
fn sequential_path_random_distance_has_a_separate_draw_from_x_jitter() {
    let template = sequential_path_instance(
        FloatInput::Literal(1.0),
        FloatInput::Random {
            min: 0.0,
            max: 1.0,
            varying: false,
            bias: None,
            sign_flip: false,
        },
        false,
    );
    let mut minimum_x = 0.0f32;
    for seed in 1..=8 {
        let mut instance = Instance::new(template.effect.clone(), seed);
        instance.advance(0.02, None);
        assert_eq!(
            instance.nodes[0].parts.len(),
            16,
            "the full burst for seed {seed}"
        );
        for position in &instance.nodes[0].parts.vectors[store::POSITION] {
            minimum_x = minimum_x.min(position.x);
        }
    }
    // Reusing u for both distance=u and x direction=2u-1 gives x=2u²-u,
    // whose minimum is -1/8. Independent draws can reach below that artificial cutoff.
    assert!(
        minimum_x < -0.2,
        "seeds 1..=8 should not share the distance/x draw: {minimum_x}"
    );
}

fn radius_attribute(initial: bool) -> FloatInput {
    FloatInput::ParticleFloat {
        field: Field::RADIUS,
        initial,
        mapping: Mapping::Direct,
    }
}

fn radius_from_particle_index(min: f32, max: f32) -> Initializer {
    Initializer::InitFloat {
        field: Field::RADIUS,
        value: FloatInput::ParticleNumberNormalized(Mapping::Remap {
            input: [0.0, 1.0],
            output: [min, max],
            looped: false,
            bias: None,
        }),
        set_method: SetMethod::Replace,
        strength: FloatInput::Literal(1.0),
    }
}

fn birth_attribute_instance(initial: bool) -> Instance {
    let template =
        sequential_path_instance(radius_attribute(initial), FloatInput::Literal(0.0), true);
    let mut definition = template.effect.defs[0].system.clone();
    definition
        .initializers
        .insert(0, always(radius_from_particle_index(3.0, 7.0)));
    let effect = Arc::new(Effect::new(&[("birth_attributes".to_owned(), definition)]));
    let mut instance = Instance::new(effect, 42);
    instance.set_control_point(4, Vec3::X * 100.0, Vec3::X, true);
    instance
}

#[test]
fn sequential_path_constant_random_count_is_shared_within_a_birth_batch() {
    let template = sequential_path_instance(
        FloatInput::Random {
            min: 8.0,
            max: 16.0,
            varying: false,
            bias: None,
            sign_flip: false,
        },
        FloatInput::Literal(0.0),
        false,
    );
    for seed in 1..=8 {
        let mut instance = Instance::new(template.effect.clone(), seed);
        instance.set_control_point(4, Vec3::X * 100.0, Vec3::X, true);
        instance.advance(0.02, None);
        let positions = &instance.nodes[0].parts.vectors[store::POSITION];
        assert_eq!(positions.len(), 16);
        let first = positions[1].x - positions[0].x;
        let second = positions[2].x - positions[1].x;
        assert!(
            (first - second).abs() < 0.001,
            "one count for seed {seed}: {first} vs {second}"
        );
    }
}

#[test]
fn particle_collection_random_uses_its_system_seed_and_is_stable_for_the_same_seed() {
    let mut definition = bare(
        1.0,
        vec![lifetime_of(10.0)],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    definition.max_particles = 64;
    definition.emitters = vec![always(Emitter::Instantaneous {
        count: FloatInput::Random {
            min: 4.0,
            max: 64.0,
            varying: false,
            bias: None,
            sign_flip: false,
        },
        start_time: FloatInput::Literal(0.0),
        max_per_frame: -1,
    })];
    let effect = Arc::new(Effect::new(&[("seeded_burst".to_owned(), definition)]));
    let mut counts = Vec::new();
    for seed in 1..=8 {
        let mut first = Instance::new(effect.clone(), seed);
        let mut again = Instance::new(effect.clone(), seed);
        first.advance(0.02, None);
        again.advance(0.02, None);
        let count = first.nodes[0].parts.len();
        assert!((4..=64).contains(&count));
        assert_eq!(
            count,
            again.nodes[0].parts.len(),
            "stable system seed {seed}"
        );
        counts.push(count);
    }
    assert!(
        counts.iter().any(|count| *count != counts[0]),
        "distinct system seeds: {counts:?}"
    );
}

#[test]
fn sequential_path_collection_count_reads_system_zero_in_later_birth_batches() {
    let mut instance = birth_attribute_instance(false);
    instance.advance(0.1, None);
    instance.advance(0.1, None);
    let positions = &instance.nodes[0].parts.vectors[store::POSITION];
    assert_eq!(positions.len(), 4);
    for (index, x) in [0.0, 50.0, 100.0, 50.0].into_iter().enumerate() {
        assert!(
            (positions[index].x - x).abs() < 0.001,
            "point {index}: {:?} vs {x}",
            positions[index]
        );
    }
}

#[test]
fn sequential_path_initial_attribute_count_reads_current_values_while_initializing() {
    let mut instance = birth_attribute_instance(true);
    instance.advance(0.1, None);
    assert_eq!(instance.nodes[0].parts.len(), 2);
    assert!((instance.nodes[0].parts.vectors[store::POSITION][1].x - 50.0).abs() < 0.001);
    instance.nodes[0].parts.scalars[store::RADIUS][0] = 9.0;
    instance.advance(0.1, None);
    let positions = &instance.nodes[0].parts.vectors[store::POSITION];
    assert_eq!(positions.len(), 4);
    assert!((positions[2].x - 100.0).abs() < 0.001);
    assert!(
        (positions[3].x - 12.5).abs() < 0.001,
        "current system0 value: {:?}",
        positions[3]
    );
}

fn maintained_path_instance(
    count: FloatInput,
    maximum: FloatInput,
    cohesion: FloatInput,
    radius: Initializer,
    looped: bool,
) -> Instance {
    use heatel::particle::{MaintainPath, Path};
    let path = Path {
        start: point(3),
        mid: None,
        end: point(4),
        bulge_control: 0,
        bulge: 0.0,
        mid_point: 0.5,
        start_offset: [0.0; 3],
        mid_offset: [0.0; 3],
        end_offset: [0.0; 3],
    };
    let definition = bare(
        4.0,
        vec![radius, lifetime_of(10.0)],
        vec![always(Operator::MaintainSequentialPath(Box::new(
            MaintainPath {
                max_distance: maximum,
                count,
                cohesion,
                tolerance: 0.0,
                looped,
                use_particle_count: false,
                path,
            },
        )))],
        Vec::new(),
        Vec::new(),
    );
    let effect = Arc::new(Effect::new(&[("maintained_path".to_owned(), definition)]));
    let mut instance = Instance::new(effect, 42);
    instance.set_control_point(4, Vec3::X * 100.0, Vec3::X, true);
    instance
}

#[test]
fn maintained_path_count_is_shared_from_system_zero_instead_of_each_particle() {
    let mut instance = maintained_path_instance(
        radius_attribute(false),
        FloatInput::Literal(0.0),
        FloatInput::Literal(1.0),
        radius_from_particle_index(3.0, 7.0),
        true,
    );
    instance.advance(0.02, None);
    let positions = &instance.nodes[0].parts.vectors[store::POSITION];
    assert_eq!(positions.len(), 4);
    for (index, x) in [0.0, 50.0, 100.0, 0.0].into_iter().enumerate() {
        assert!(
            (positions[index].x - x).abs() < 0.001,
            "point {index}: {:?} vs {x}",
            positions[index]
        );
    }
}

#[test]
fn maintained_path_distance_and_cohesion_read_system_zero_once() {
    let mut distance = maintained_path_instance(
        FloatInput::Literal(4.0),
        radius_attribute(false),
        FloatInput::Literal(0.0),
        radius_from_particle_index(3.0, 7.0),
        true,
    );
    distance.advance(0.02, None);
    let positions = &distance.nodes[0].parts.vectors[store::POSITION];
    assert_eq!(positions.len(), 4);
    for (index, x) in [0.0, 100.0 / 3.0 - 3.0, 200.0 / 3.0 - 3.0, 97.0]
        .into_iter()
        .enumerate()
    {
        assert!(
            (positions[index].x - x).abs() < 0.001,
            "shared distance at {index}: {:?}",
            positions[index]
        );
    }
    let mut cohesion = maintained_path_instance(
        FloatInput::Literal(4.0),
        FloatInput::Literal(1000.0),
        radius_attribute(false),
        radius_from_particle_index(0.25, 1.25),
        true,
    );
    cohesion.advance(0.02, None);
    let positions = &cohesion.nodes[0].parts.vectors[store::POSITION];
    for (index, x) in [0.0, 25.0 / 3.0, 50.0 / 3.0, 25.0].into_iter().enumerate() {
        assert!(
            (positions[index].x - x).abs() < 0.001,
            "shared cohesion at {index}: {:?}",
            positions[index]
        );
    }
}

#[test]
fn maintained_path_initial_attribute_count_retains_the_initial_buffer() {
    let mut instance = maintained_path_instance(
        radius_attribute(true),
        FloatInput::Literal(0.0),
        FloatInput::Literal(1.0),
        radius_from_particle_index(3.0, 7.0),
        true,
    );
    instance.advance(0.02, None);
    instance.nodes[0].parts.scalars[store::RADIUS][0] = 9.0;
    instance.advance(0.02, None);
    let positions = &instance.nodes[0].parts.vectors[store::POSITION];
    assert_eq!(positions.len(), 4);
    assert!(
        (positions[1].x - 50.0).abs() < 0.001,
        "initial scalar preserved: {:?}",
        positions[1]
    );
}

#[test]
fn maintained_path_attribute_counts_cannot_overflow_the_bounce_index() {
    for (value, second) in [(-f32::MAX, 0.0), (f32::MAX, 100.0 / 4999.0), (2.75, 100.0)] {
        let radius = Initializer::InitFloat {
            field: Field::RADIUS,
            value: FloatInput::Literal(value),
            set_method: SetMethod::Replace,
            strength: FloatInput::Literal(1.0),
        };
        let mut instance = maintained_path_instance(
            radius_attribute(false),
            FloatInput::Literal(0.0),
            FloatInput::Literal(1.0),
            radius,
            false,
        );
        instance.advance(0.02, None);
        let positions = &instance.nodes[0].parts.vectors[store::POSITION];
        assert_eq!(positions.len(), 4);
        assert!(
            (positions[1].x - second).abs() < 1e-5,
            "count {value}: {:?} vs {second}",
            positions[1]
        );
    }
}

#[test]
fn maintained_path_collection_number_uses_active_zero_and_active_count() {
    for (reverse, mapping, expected) in [
        (
            false,
            Mapping::Remap {
                input: [0.0, 1.0],
                output: [3.0, 4.0],
                looped: false,
                bias: None,
            },
            [0.0, 50.0, 100.0, 0.0],
        ),
        (
            true,
            Mapping::Direct,
            [0.0, 100.0 / 3.0, 200.0 / 3.0, 100.0],
        ),
    ] {
        let mut instance = maintained_path_instance(
            FloatInput::ParticleNumber { reverse, mapping },
            FloatInput::Literal(0.0),
            FloatInput::Literal(1.0),
            lifetime_of(10.0),
            true,
        );
        instance.advance(0.02, None);
        instance.nodes[0].parts.number[0] = 100;
        instance.advance(0.02, None);
        let positions = &instance.nodes[0].parts.vectors[store::POSITION];
        assert_eq!(positions.len(), 4);
        for (index, x) in expected.into_iter().enumerate() {
            assert!(
                (positions[index].x - x).abs() < 0.001,
                "active numbering reverse={reverse}: {:?}",
                positions[index]
            );
        }
    }
}

#[test]
fn particle_collection_attribute_input_is_safe_before_any_particle_exists() {
    let mut definition = bare(
        1.0,
        vec![lifetime_of(10.0)],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    definition.emitters = vec![always(Emitter::Instantaneous {
        count: radius_attribute(false),
        start_time: FloatInput::Literal(0.0),
        max_per_frame: -1,
    })];
    let effect = Arc::new(Effect::new(&[("empty_collection".to_owned(), definition)]));
    let mut instance = Instance::new(effect, 42);
    instance.advance(0.02, None);
    assert_eq!(instance.nodes[0].parts.len(), 0);
}

fn normal_rope_look() -> heatel::particle::Look {
    use heatel::particle::{Blend, Look, Orientation, TextureChannels};
    Look {
        texture: None,
        texture_transform: [1.0, 0.0, 1.0, 0.0],
        channels: TextureChannels::Rgba,
        layers: Vec::new(),
        blend: Blend::Add,
        overbright: FloatInput::Literal(1.0),
        add_self: FloatInput::Literal(0.0),
        saturate: false,
        gamma_vertex_colors: false,
        alpha_scale: FloatInput::Literal(1.0),
        radius_scale: FloatInput::Literal(1.0),
        color_scale: VectorInput::Literal([1.0; 3]),
        no_depth_test: false,
        animation_rate: 0.0,
        animate_in_fps: false,
        blend_frames: false,
        orientation: Orientation::ParticleNormal,
        refract: false,
        depth_bias: FloatInput::Literal(0.0),
    }
}

#[test]
fn a_normal_oriented_rope_interpolates_its_drawn_width_between_endpoint_normals() {
    use heatel::particle::{Blend, Renderer, Ropes};
    let mut definition = bare(
        2.0,
        vec![lifetime_of(10.0)],
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    definition.renderers = vec![always(Renderer::Ropes(Box::new(Ropes {
        look: normal_rope_look(),
        min_tessellation: 2,
        max_tessellation: 2,
        texture_v_world_size: FloatInput::Literal(1.0),
        texture_v_scroll_rate: FloatInput::Literal(0.0),
        texture_v_offset: FloatInput::Literal(0.0),
    })))];
    let mut effect = Effect::new(&[("normal_rope".to_owned(), definition)]);
    effect.defs[0].draws.push(super::library::Draw {
        renderer: 0,
        look: 0,
        sheet: None,
        blend: Blend::Add,
        material: 0,
    });
    let mut instance = Instance::new(Arc::new(effect), 42);
    instance.advance(0.02, None);
    let parts = &mut instance.nodes[0].parts;
    assert_eq!(parts.len(), 2);
    parts.vectors[store::POSITION][0] = Vec3::ZERO;
    parts.vectors[store::POSITION][1] = Vec3::X * 10.0;
    parts.vectors[store::NORMAL][0] = Vec3::Y;
    parts.vectors[store::NORMAL][1] = Vec3::Z;
    parts.scalars[store::RADIUS][..2].fill(1.0);
    let eye = super::draw::Eye {
        position: Vec3::new(0.0, -10.0, 10.0),
        right: Vec3::X,
        up: Vec3::Z,
    };
    let mut cards = [Vec::new()];
    super::draw::draw(&instance, &eye, None, &mut cards);
    assert_eq!(cards[0].len(), 2, "two tessellated rope cards");
    // At the common midpoint N=(Y+Z)/sqrt(2), T=X. Source Z-up is converted to drawn Y-up.
    let expected = Vec3::new(
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    );
    assert!(
        Vec3::from(cards[0][0].tail_side).distance(expected) < 0.0001,
        "midpoint width must interpolate: {:?}",
        cards[0][0].tail_side
    );
    assert!(
        Vec3::from(cards[0][1].head_side).distance(expected) < 0.0001,
        "adjacent cards share their interpolated midpoint width"
    );
}

#[test]
fn review_residual_maintenance_bounds_distance_and_cohesion() {
    for distance in [f32::NAN, -1.0, f32::INFINITY, f32::MAX] {
        for cohesion in [f32::NAN, -1.0, f32::INFINITY, f32::NEG_INFINITY, 0.5] {
            let mut instance = maintained_path_instance(
                FloatInput::Literal(4.0),
                FloatInput::Literal(distance),
                FloatInput::Literal(cohesion),
                radius_from_particle_index(1.0, 1.0),
                true,
            );
            instance.advance(0.02, None);
            let parts = &instance.nodes[0].parts;
            assert_eq!(parts.len(), 4);
            for field in [store::POSITION, store::PREVIOUS] {
                for position in &parts.vectors[field] {
                    assert!(position.is_finite(), "{distance} {cohesion}: {position}");
                    assert!((-0.001..=100.001).contains(&position.x));
                }
            }
        }
    }
}

#[test]
fn review_residual_collection_speed_and_normalized_number_apply_mappings() {
    let mapped = |input, output| Mapping::Remap {
        input,
        output,
        looped: false,
        bias: None,
    };
    for count in [
        FloatInput::ParticleSpeed(mapped([0.0, 200.0], [2.0, 6.0])),
        FloatInput::ParticleNumberNormalized(mapped([0.0, 1.0], [3.0, 7.0])),
    ] {
        let mut instance = maintained_path_instance(
            count,
            FloatInput::Literal(0.0),
            FloatInput::Literal(1.0),
            radius_from_particle_index(1.0, 1.0),
            true,
        );
        instance.advance(0.02, None);
        instance.nodes[0].parts.vectors[store::POSITION][0] = Vec3::X;
        instance.nodes[0].parts.vectors[store::PREVIOUS][0] = Vec3::ZERO;
        instance.advance(0.02, None);
        assert_eq!(instance.nodes[0].parts.len(), 4);
        assert!((instance.nodes[0].parts.vectors[store::POSITION][1].x - 50.0).abs() < 0.001);
    }
}

fn review_random_input() -> FloatInput {
    FloatInput::Random {
        min: 4.0,
        max: 64.0,
        varying: false,
        bias: None,
        sign_flip: false,
    }
}

#[test]
fn review_residual_initializer_random_uses_system_seed() {
    let template = sequential_path_instance(review_random_input(), FloatInput::Literal(0.0), false);
    let mut results = Vec::new();
    for seed in 1..=8 {
        let run = || {
            let mut instance = Instance::new(template.effect.clone(), seed);
            instance.set_control_point(4, Vec3::X * 100.0, Vec3::X, true);
            instance.advance(0.02, None);
            assert_eq!(instance.nodes[0].parts.len(), 16);
            instance.nodes[0].parts.vectors[store::POSITION][1].x
        };
        let value = run();
        assert_eq!(value, run());
        results.push(value);
    }
    assert!(results.iter().any(|value| *value != results[0]));
}

#[test]
fn review_residual_constraint_random_uses_system_seed() {
    use heatel::particle::{Constraint, Distance};
    let mut definition = system(
        4.0,
        vec![always(Operator::BasicMovement {
            gravity: VectorInput::Literal([0.0; 3]),
            drag: FloatInput::Literal(0.0),
            max_constraint_passes: 1,
        })],
        Vec::new(),
        Vec::new(),
    );
    definition.constraints = vec![always(Constraint::Distance(Box::new(Distance {
        min_distance: FloatInput::Literal(0.0),
        max_distance: review_random_input(),
        transform: Transform::Identity,
        center_offset: VectorInput::Literal([0.0; 3]),
        global_center: true,
    })))];
    let effect = Arc::new(Effect::new(&[("seeded_constraint".to_owned(), definition)]));
    let mut results = Vec::new();
    for seed in 1..=8 {
        let run = || {
            let mut instance = Instance::new(effect.clone(), seed);
            instance.set_control_point(3, Vec3::X * 400.0, Vec3::X, true);
            instance.advance(0.02, None);
            assert_eq!(instance.nodes[0].parts.len(), 4);
            instance.nodes[0].parts.vectors[store::POSITION][0].length()
        };
        let value = run();
        assert!((4.0..=64.001).contains(&value));
        assert_eq!(value, run());
        results.push(value);
    }
    assert!(results.iter().any(|value| *value != results[0]));
}

#[test]
fn imported_control_point_remap_uses_the_input_component_for_smoothing() {
    use heatel::particle::{ControlPointRemap, VectorComponent};
    let mut definition = bare(0.0, vec![], vec![], vec![], vec![]);
    definition.pre_emission = vec![always(PreEmission::RemapControlPoint(Box::new(
        ControlPointRemap {
            input: point(3),
            output: point(9),
            input_component: VectorComponent::Z,
            output_component: VectorComponent::X,
            input_range: [-4.0, 12.0],
            output_range: [20.0, 100.0],
            interpolation: 0.25,
            derivative: false,
        },
    )))];
    let mut instance = Instance::new(Arc::new(Effect::new(&[("remap".into(), definition)])), 1);
    instance.set_control_point(3, Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, false);
    instance.set_control_point(9, Vec3::new(0.0, 77.0, 40.0), Vec3::ZERO, false);
    instance.advance(0.0, None);
    assert_eq!(
        instance.nodes[0].control_points[9].position,
        Vec3::new(45.0, 77.0, 40.0)
    );
}

#[test]
fn imported_particle_noise_retains_nonzero_authored_radius_bounds() {
    use heatel::particle::ParticleNoise;
    for (output, scale) in [([12.0, 24.0], 0.5), ([-f32::MAX, f32::MAX], f32::MAX)] {
        let definition = bare(
            1.0,
            vec![Initializer::InitFloat {
                field: Field::RADIUS,
                value: FloatInput::ParticleNoise(Box::new(ParticleNoise {
                    field: Field::POSITION,
                    output,
                    scale,
                    offset_rate: [1.0, 1.0, 4.0],
                    offset: 0.0,
                    octaves: 1,
                    mapping: Mapping::Direct,
                })),
                set_method: SetMethod::Replace,
                strength: FloatInput::Literal(1.0),
            }],
            vec![],
            vec![],
            vec![],
        );
        let mut instance = Instance::new(Arc::new(Effect::new(&[("noise".into(), definition)])), 1);
        instance.advance(0.01, None);
        let radii = &instance.nodes[0].parts.scalars[store::RADIUS];
        assert_eq!(radii.len(), 1);
        assert!(
            radii[0].is_finite(),
            "non-finite radius from finite inputs: {}",
            radii[0]
        );
        assert!(
            (output[0]..=output[1]).contains(&radii[0]),
            "radius: {}",
            radii[0]
        );
    }
}

#[test]
fn imported_snapshot_positions_use_the_spatial_frame_and_authored_start_index() {
    use heatel::particle::Snapshot;
    let mut definition = bare(
        2.0,
        vec![Initializer::SkinnedSnapshot {
            control_point: point(0),
            snapshot_control_point: point(6),
            start: 1,
            random: false,
            set_normal: true,
        }],
        vec![],
        vec![],
        vec![],
    );
    definition.snapshot_control_point = point(6);
    definition.snapshot = Some(Snapshot::Points {
        positions: vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
        normals: Some(vec![[0.0, 0.0, 1.0]; 2]),
    });
    let mut instance = Instance::new(Arc::new(Effect::new(&[("snapshot".into(), definition)])), 1);
    instance.set_control_point(0, Vec3::new(10.0, 20.0, 30.0), Vec3::ZERO, false);
    instance.advance(0.01, None);
    assert_eq!(
        instance.nodes[0].parts.vectors[store::POSITION],
        vec![Vec3::new(14.0, 25.0, 36.0), Vec3::new(11.0, 22.0, 33.0)]
    );
}
