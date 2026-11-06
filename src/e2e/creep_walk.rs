//! Every catalogue creep plays its authored locomotion without adding another world trajectory.
use std::time::Duration;

use bevy::{mesh::skinning::SkinnedMesh, prelude::*, time::TimeUpdateStrategy};
use bota_proto::{EntityId, HeroId, NeutralId, ServerMsg, Team, UnitKind, UnitView, Vec2 as Spot};
use heatel::{DirSource, Resource, Vpk, find_game_dir};

use super::harness::{Client, Server, pose_of, subtree};

pub(super) type RootBounds = Vec<(String, Vec2, Vec2, Vec<Quat>)>;

struct Creep {
    name: String,
    kind: UnitKind,
    team: Team,
    neutral: Option<NeutralId>,
    speed: i32,
}

// The match starts on the real server. Controlled visual snapshots must not interleave with
// socket snapshots after the server is paused.
pub(super) struct ControlledFrames;
impl bota_client_core::LiveLink for ControlledFrames {
    fn poll(&mut self) -> Vec<ServerMsg> {
        Vec::new()
    }
    fn send(&mut self, _: &bota_proto::ClientMsg) -> std::io::Result<()> {
        Ok(())
    }
    fn is_closed(&self) -> bool {
        false
    }
}

#[test]
fn every_catalogue_creep_walks_with_its_authored_sequence_and_moving_joints() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            1.0 / 60.0,
        )));
    let source = DirSource::new(find_game_dir(None).expect("local install"));
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("VPK");
    let cases = catalogue();
    assert_eq!(
        cases.len(),
        65,
        "47 neutral kinds and both teams of all nine lane forms"
    );
    let mut failures = Vec::new();
    let mut skipped = 0;
    server.frozen(|| {
        let _live = std::mem::replace(
            &mut client.core_mut().source,
            bota_client_core::Source::Live(Box::new(ControlledFrames)),
        );
        for (index, case) in cases.iter().enumerate() {
            skipped += usize::from(check_creep(
                &mut client,
                case,
                index,
                &source,
                &pak,
                &mut failures,
            ));
            eprintln!("walk checked {}", case.name);
        }
    });
    assert!(
        failures.is_empty(),
        "{} creeps checked: {failures:#?}",
        cases.len()
    );
    assert_eq!(
        skipped, 1,
        "only the model-less Tornado skips a skeleton check"
    );
    eprintln!(
        "catalogue: {} forms, {} drawn, {skipped} explicitly model-less",
        cases.len(),
        cases.len() - skipped
    );
}

fn catalogue() -> Vec<Creep> {
    let mut cases: Vec<_> = bota_core::NEUTRALS
        .iter()
        .map(|neutral| Creep {
            name: neutral.unit.to_owned(),
            kind: UnitKind::CreepNeutral,
            team: Team::Neutral,
            neutral: Some(neutral.kind.id()),
            speed: neutral.def.move_speed,
        })
        .collect();
    for team in [Team::Radiant, Team::Dire] {
        for (index, def) in bota_core::FIXED_UNIT_DEFS
            .iter()
            .enumerate()
            .filter(|(_, def)| bota_core::kind_def(def.kind).lane_creep)
        {
            cases.push(Creep {
                name: format!("{team:?} {:?} form {index}", def.kind),
                kind: def.kind,
                team,
                neutral: None,
                speed: def.move_speed,
            });
        }
    }
    cases
}

fn creep_view(client: &Client, case: &Creep, index: usize) -> UnitView {
    let mut unit = client.me().clone();
    unit.id = EntityId {
        idx: 50_000 + index as u32,
        generation: 1,
    };
    unit.kind = case.kind;
    unit.team = case.team;
    unit.neutral = case.neutral;
    unit.hero = None;
    unit.owner = None;
    unit.cast = None;
    unit.cast_rates = None;
    unit.town_portal = None;
    unit.abilities.clear();
    unit.effects.clear();
    unit.items.clear();
    unit.move_speed = bota_proto::Fixed::from_int(case.speed);
    unit.pos = client.me().pos + Spot::from_ints(100, 0);
    unit
}

fn check_creep(
    client: &mut Client,
    case: &Creep,
    index: usize,
    source: &DirSource,
    pak: &Vpk,
    failures: &mut Vec<String>,
) -> bool {
    let mut unit = creep_view(client, case, index);
    tell(client, unit.clone());
    let invisible = case.kind == UnitKind::CreepNeutral
        && client
            .world()
            .resource::<crate::manifest::Manifest>()
            .unit_art(&case.name)
            .expect("unit art")
            .model
            .is_none();
    if invisible {
        client.frames(2);
        assert!(client.body(unit.id).is_none_or(|body| {
            client
                .world()
                .get::<crate::units::Unit>(body)
                .is_some_and(|drawn| drawn.model.is_none())
        }));
        eprintln!(
            "explicitly skipped skeleton check: {} has no model",
            case.name
        );
        return true;
    }
    client.until("the creep's model and rig are drawn", |c| {
        c.body(unit.id)
            .is_some_and(|body| c.world().get::<crate::units::Rig>(body).is_some())
    });
    let body = client.body(unit.id).expect("creep body");
    let roots = root_joints(client, body);
    assert!(
        !roots.is_empty(),
        "{}: drawn mesh must be skinned",
        case.name
    );
    let (error, bounds) = authored_walk(client, body, source, pak);
    if let Some(error) = error {
        failures.push(format!("{}: {error}", case.name));
    }
    let origin = unit.pos;
    let mut samples = Vec::new();
    let mut positions = Vec::new();
    for step in 1..=30 {
        unit.pos = origin + Spot::from_ints(case.speed * step / 30, 0);
        tell(client, unit.clone());
        client.frames(2);
        if [10, 20, 30].contains(&step) {
            check_sample(client, body, &roots, &bounds, &case.name, failures);
            positions.push(
                client
                    .world()
                    .get::<GlobalTransform>(body)
                    .expect("drawn body transform")
                    .translation(),
            );
            samples.push(
                pose_of(client, body)
                    .into_iter()
                    .filter(|(entity, _)| *entity != body)
                    .collect::<Vec<_>>(),
            );
        }
    }
    assert!(
        positions
            .windows(2)
            .all(|pair| pair[0].distance(pair[1]) > 1.0),
        "{}: the drawn body actually walks",
        case.name
    );
    if !samples.windows(2).all(|pair| {
        pair[0].iter().any(|(entity, before)| {
            pair[1].iter().any(|(other, after)| {
                entity == other
                    && (before.rotation.angle_between(after.rotation).abs() > 1e-4
                        || before.translation.distance(after.translation) > 1e-4)
            })
        })
    }) {
        failures.push(format!(
            "{}: joints are frozen while the body moves",
            case.name
        ));
    }
    false
}

pub(super) fn check_sample(
    client: &Client,
    body: Entity,
    roots: &[Entity],
    bounds: &RootBounds,
    name: &str,
    failures: &mut Vec<String>,
) {
    let world = client.world();
    let pose = world.get::<crate::anim::Pose>(body).expect("pose");
    let drawn = world.get::<crate::units::Unit>(body).expect("drawn unit");
    let bank = world.resource::<crate::units::ModelBank>();
    let run = bank.0[drawn.model.expect("model")]
        .clips
        .iter()
        .find(|clip| clip.0 == "run")
        .expect("loaded run graph node")
        .1;
    let rig = world.get::<crate::units::Rig>(body).expect("rig").0;
    let player = world.get::<AnimationPlayer>(rig).expect("animation player");
    if !player
        .playing_animations()
        .any(|(node, active)| *node == run && active.weight() > 0.99)
    {
        failures.push(format!(
            "{name}: authored run graph node is not actually playing at full weight"
        ));
    }
    if pose.clip() != "run" {
        failures.push(format!("{name}: moving but playing {}", pose.clip()));
    }
    for root in roots {
        let root_name = world.get::<Name>(*root).expect("root name").as_str();
        let bound = bounds
            .iter()
            .find(|bound| bound.0 == root_name)
            .expect("source root");
        let at = world
            .get::<Transform>(*root)
            .expect("root transform")
            .translation
            .xz();
        let rotation = world
            .get::<Transform>(*root)
            .expect("root transform")
            .rotation;
        if !orientation_in_clip(&bound.3, rotation) {
            failures.push(format!(
                "{name}: {root_name} adds yaw/rotation outside the authored run"
            ));
        }
        if at.x < bound.1.x - 1e-3
            || at.y < bound.1.y - 1e-3
            || at.x > bound.2.x + 1e-3
            || at.y > bound.2.y + 1e-3
        {
            failures.push(format!("{name}: {root_name} adds trajectory outside authored pose bounds {:?}..{:?}: {at:?}", bound.1, bound.2));
        }
    }
}

fn orientation_in_clip(rotations: &[Quat], actual: Quat) -> bool {
    rotations.windows(2).any(|pair| {
        let tolerance = pair[0].angle_between(pair[1]) / 16.0 + 1e-3;
        (0..=8)
            .any(|step| actual.angle_between(pair[0].slerp(pair[1], step as f32 / 8.0)) < tolerance)
    }) || rotations
        .first()
        .is_some_and(|only| only.angle_between(actual) < 1e-3)
}

pub(super) fn root_joints(client: &Client, body: Entity) -> Vec<Entity> {
    let world = client.world();
    let joints: std::collections::HashSet<_> = subtree(world, body)
        .into_iter()
        .filter_map(|entity| world.get::<SkinnedMesh>(entity))
        .flat_map(|skin| skin.joints.iter().copied())
        .collect();
    let mut roots: Vec<_> = joints
        .iter()
        .copied()
        .filter(|joint| {
            world
                .get::<ChildOf>(*joint)
                .is_none_or(|parent| !joints.contains(&parent.parent()))
        })
        .collect();
    roots.sort();
    roots
}

pub(super) fn tell(client: &mut Client, unit: UnitView) {
    let mut view = client.view().clone();
    view.tick += 1;
    view.units
        .retain(|old| old.id != unit.id && !(50_000..51_000).contains(&old.id.idx));
    view.units.push(unit);
    view.units.sort_by_key(|unit| unit.id);
    view.aggro.clear();
    let tick = view.tick;
    client.core_mut().handle(ServerMsg::Snapshot { view });
    client.core_mut().handle(ServerMsg::Events {
        tick,
        events: Vec::new(),
    });
    let errors: Vec<_> = client
        .core()
        .feed
        .iter()
        .filter_map(|line| match &line.kind {
            bota_client_core::FeedKind::ProtocolBroken { why } => Some(why.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        client.tick(),
        tick,
        "the stream must accept the creep snapshot: {errors:?}"
    );
}

pub(super) fn authored_walk(
    client: &Client,
    body: Entity,
    source: &DirSource,
    pak: &Vpk,
) -> (Option<String>, RootBounds) {
    let drawn = client
        .world()
        .get::<crate::units::Unit>(body)
        .expect("unit");
    let bank = client.world().resource::<crate::units::ModelBank>();
    let model = &bank.0[drawn.model.expect("model")];
    let clip = model
        .model
        .clips
        .iter()
        .find(|clip| clip.logical == "run")
        .expect("run clip");
    let path = include_str!("../../assets-manifest/manifest.txt")
        .lines()
        .find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("model") && words.next() == Some(&model.model.logical))
                .then(|| words.next().expect("model source"))
        })
        .expect("model source");
    let bytes = pak.read_file(source, path).expect("model bytes");
    let resource = Resource::parse(&bytes).expect("model resource");
    let sequences = heatel::anim::read_sequences(&resource).expect("model sequences");
    let expected: Vec<_> = sequences
        .iter()
        .filter(|sequence| {
            sequence.activities.iter().any(|activity| {
                activity.name == "ACT_DOTA_RUN"
                    && activity.modifiers.is_empty()
                    && activity.weight > 0
            })
        })
        .map(|sequence| sequence.name.as_str())
        .collect();
    let movement = super::animation_reference::Trajectory::read(&resource, &clip.name);
    let matches_activity = expected.iter().any(|name| {
        super::animation_reference::Trajectory::read(&resource, name).animation
            == movement.animation
    });
    let error = (!matches_activity).then(|| {
        format!(
            "run clip {} differs from installed ACT_DOTA_RUN {expected:?}",
            clip.name
        )
    });
    let skeleton = heatel::Model::read(&resource).expect("source skeleton");
    let reference = heatel::anim::read_clips(&resource, &[&clip.name]).expect("raw poses");
    let bounds = skeleton
        .bones
        .iter()
        .enumerate()
        .filter(|(_, bone)| bone.parent.is_none())
        .map(|(index, bone)| {
            let mut minimum = Vec2::splat(f32::INFINITY);
            let mut maximum = Vec2::splat(f32::NEG_INFINITY);
            let mut rotations = Vec::new();
            for time in movement.times() {
                let (key, rotation) = movement.pose(&reference[0], index, time);
                rotations.push(Quat::from_array(rotation).normalize());
                let at = Vec2::new(key[0], key[2]);
                minimum = minimum.min(at);
                maximum = maximum.max(at);
            }
            (bone.name.clone(), minimum, maximum, rotations)
        })
        .collect();
    (error, bounds)
}
