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
        super::models::gather(&instance, &mut placed);
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
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    let server = app.world().resource::<AssetServer>();
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
        .add("budget", &bytes, server)
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

/// Shadowraze in a match, cast by the client as a player casts it (reported: the
/// raze's embers drifted off): every card the raze's effect draws stays about the hero and the
/// spot it lands on while it plays. Its embers' velocity is control point 3,
/// which Dota leaves unplaced; standing at the world's origin it is no velocity, where it stood
/// at the effect's own place (control point 0) they flew off by a fifth of where the raze
/// landed, out of the map's middle toward the Dire corner.
#[test]
fn a_shadowraze_draws_where_it_lands_and_its_embers_do_not_drift() {
    use crate::e2e::harness::{Client, MID, Server};
    use bota_client_core::{Intent, Slot};
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    let core = client.core_mut();
    core.act(Intent::Press {
        slot: Slot::Ability(0),
        ctrl: true,
    });
    client.frames(3);
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(0),
        ctrl: false,
    });
    let hero = client.me().pos;
    let hero = Vec3::new(hero.x.to_f32(), hero.y.to_f32(), 0.0);
    // The looks only the raze draws with: the effects started besides it, before it or while it
    // plays (the hero's ambient glow and trail, the map's ambient effects about him, a rune
    // appearing), share some of its textures.
    let looks = |effect: &super::library::Effect| -> Vec<u16> {
        effect
            .defs
            .iter()
            .flat_map(|def| def.draws.iter().map(|draw| draw.look))
            .collect()
    };
    let raze_looks = |c: &Client| -> Vec<u16> {
        let library = c.world().resource::<super::Library>();
        let playing = c.world().resource::<super::Playing>();
        let others: std::collections::HashSet<u16> = library
            .effects
            .iter()
            .filter(|(name, effect)| *name != "hero.nevermore.raze" && playing.started(effect) > 0)
            .flat_map(|(_, effect)| looks(effect))
            .collect();
        looks(
            library
                .effect("hero.nevermore.raze")
                .expect("the install's Shadowraze effect"),
        )
        .into_iter()
        .filter(|look| !others.contains(look))
        .collect()
    };
    assert!(
        !raze_looks(&client).is_empty(),
        "the raze has looks of its own"
    );
    // The cards of the looks the raze draws with: their heads, in the world's axes (bota's x,
    // the height, bota's y turned).
    let cards = |c: &Client| {
        let raze = raze_looks(c);
        let frame = c.world().resource::<super::render::ParticleFrame>();
        frame
            .batches
            .iter()
            .filter(|batch| raze.contains(&batch.look))
            .flat_map(|batch| {
                frame.quads[batch.range.start as usize..batch.range.end as usize]
                    .iter()
                    .map(|q| (batch.look, Vec3::from(q.head)))
            })
            .collect::<Vec<_>>()
    };
    assert!(
        cards(&client).is_empty(),
        "nothing else draws with the raze's looks"
    );
    let patience = std::time::Duration::from_secs(10);
    client.within(patience, "the raze drawn", |c| !cards(c).is_empty());
    let (mut farthest, mut far_look): (f32, u16) = (0.0, 0);
    for _ in 0..60 {
        client.frames(1);
        for (look, card) in cards(&client) {
            let off = Vec3::new(card.x, -card.z, 0.0).distance(hero);
            if off > farthest {
                (farthest, far_look) = (off, look);
            }
        }
    }
    // A raze lands at most 700 out and bursts 250 wide; its fleks fly a little further (up to
    // some 1400 units). Drifting, the embers went over 7000 units off within the second.
    let library = client.world().resource::<super::Library>();
    let playing = client.world().resource::<super::Playing>();
    let sharing: Vec<String> = library
        .effects
        .iter()
        .filter(|(_, effect)| looks(effect).contains(&far_look))
        .map(|(name, effect)| format!("{name} (started {})", playing.started(effect)))
        .collect();
    assert!(
        farthest < 2000.0,
        "a card {farthest} units off the hero, of look {far_look}, which {sharing:?} draw with"
    );
}

/// The Dire tower's shot as Dota draws it (reported: dark squares at the Dire tower's
/// impact): its lava splash is a second texture layer mixed into the first (a texture with no
/// alpha, whose alpha is its luminance, added twice, mirrored), so every card the shot draws
/// with a second texture carries both layers and their mix to the GPU; drawn with its first
/// layer alone and opaque, the splash was the squares. The blend in the shader is not seen
/// here, the cards it is given are.
#[test]
fn the_dire_towers_shot_draws_its_second_texture_layer() {
    use crate::e2e::harness::{Client, MID, Server};
    use bota_proto::{Team, UnitKind};
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    let mid = Vec3::new(MID.0 as f32, MID.1 as f32, 0.0);
    let tower = client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Tower && u.team == Team::Dire)
        .map(|u| Vec3::new(u.pos.x.to_f32(), u.pos.y.to_f32(), 0.0))
        .min_by(|a, b| a.distance(mid).total_cmp(&b.distance(mid)))
        .expect("a Dire tower");
    let under = tower + (mid - tower).normalize() * 350.0;
    client.stand_at((under.x as i32, under.y as i32));
    let library = client.world().resource::<super::Library>();
    let shot = library
        .effect("tower.dire.attack")
        .expect("the install's Dire tower shot");
    // The shot's looks with a second texture.
    let layered: Vec<u16> = shot
        .defs
        .iter()
        .flat_map(|def| def.draws.iter().map(|draw| draw.look))
        .filter(|look| library.looks[usize::from(*look)].second.is_some())
        .collect();
    assert!(
        !layered.is_empty(),
        "the splash's look has its second layer"
    );
    let cards = |c: &Client| {
        let frame = c.world().resource::<super::render::ParticleFrame>();
        frame
            .batches
            .iter()
            .filter(|batch| layered.contains(&batch.look))
            .flat_map(|batch| &frame.quads[batch.range.start as usize..batch.range.end as usize])
            .map(|q| q.layers)
            .collect::<Vec<_>>()
    };
    let patience = std::time::Duration::from_secs(30);
    client.within(patience, "the tower's shot drawn with its splash", |c| {
        !cards(c).is_empty()
    });
    for layers in cards(&client) {
        assert_eq!(layers[3], 2, "both layers given: {layers:?}");
    }
}

/// The fountain's shots as Dota's (reported: the fountain did not shoot, and then
/// shot with no missile): a hero at the enemy's fountain is shot, and the fountain's own
/// missile effect from the install draws.
#[test]
fn the_fountain_shoots_its_own_missile() {
    use crate::e2e::harness::{Client, Server};
    // The hero takes the shots and stands: no death of its own draws meanwhile.
    let server = Server::start(1, &["--immortal-heroes"]);
    let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
    let map = bota_core::map_of(bota_core::MAP2_ID);
    let theirs = bota_core::fountain_pos(map, bota_proto::Team::Dire);
    let near = bota_proto::Vec2::from_ints(
        theirs.x.to_f32() as i32 - 600,
        theirs.y.to_f32() as i32 - 600,
    );
    let library = client.world().resource::<super::Library>();
    let shot: Vec<u16> = library
        .effect("fountain.dire.attack")
        .expect("the install's fountain missile")
        .defs
        .iter()
        .flat_map(|def| def.draws.iter().map(|draw| draw.look))
        .collect();
    // A card of the missile's looks at the hero: the missile landing on it, not the
    // fountain's own glow, which shares a look and stays at the fountain.
    let at = Vec3::new(near.x.to_f32(), 0.0, -near.y.to_f32());
    let drawn = |c: &Client| {
        let frame = c.world().resource::<super::render::ParticleFrame>();
        frame
            .batches
            .iter()
            .filter(|batch| shot.contains(&batch.look))
            .flat_map(|batch| &frame.quads[batch.range.start as usize..batch.range.end as usize])
            .any(|q| Vec3::new(q.head[0], 0.0, q.head[2]).distance(at) < 250.0)
    };
    client.cheat(bota_proto::Cheat::Teleport { at: near });
    client.follow_me();
    let patience = std::time::Duration::from_secs(20);
    client.within(patience, "the fountain's missile drawn", |c| drawn(c));
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
