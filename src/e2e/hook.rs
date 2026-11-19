//! Pudge's Meat Hook as a player sees it: the hook's head leaves his hand along the cast line at
//! the ability's speed, turning as it flies, with the chain stretched from his hand to it; a
//! hook that catches something brings it back with the chain shortening, one that catches
//! nothing comes back empty from its full reach; nothing of it stays after it is home, and a
//! second throw is drawn as the first was. Each test records a real match with the hook thrown
//! and plays the record through the real client, whose every frame is sampled at a fixed step.

use std::{fs, path::Path, time::Duration};

use bevy::prelude::*;
use bota_client_core::Source;
use bota_core::rules;
use bota_proto::{
    AbilityId, AbilitySlot, Cheat, HeroId, Inflictor, Order, Target, Team, UnitKind, Vec2 as Spot,
};

use super::fog_pov::{ReplayFile, Spectated};
use super::harness::{Client, MID};

/// How far the thrown-at spot lies from Pudge: inside his cast range, so that he throws from
/// where he stands instead of walking toward it (a hook flies its full reach anyway).
const AIM_DISTANCE: i32 = 800;
/// The enemy a hook catches stands this far off, on the cast line.
const VICTIM_DISTANCE: i32 = 500;
/// Where Pudge throws when the enemy must have no sight of it: Radiant's own ground, far from
/// every tower of the Dire.
const YARD: (i32, i32) = (2790, 2905);
/// How long the client may take to load and play a record.
const SOON: Duration = Duration::from_secs(60);
/// What a replayed frame advances the clock by.
const FRAME: Duration = Duration::from_millis(20);
/// How far a drawn thing may be from where the test expects it: the head trails the hook the
/// server tells of by a few ticks of its flight.
const TRAIL: f32 = 130.0;
/// One replayed frame: the sim's tick and what is drawn of the hook then, in the particles'
/// frame (Z up, Dota units).
#[derive(Clone, Debug)]
struct Frame {
    seconds: f32,
    /// Control point 0 of the hook's particle: the hand the chain starts at.
    hand: Option<Vec3>,
    /// The animated chain socket and the hero's scene scale in Source units.
    socket: Option<Vec3>,
    /// Where the wire's hook is relative to the thrower and the reach of the launch pose.
    wire_reach: f32,
    socket_reach: f32,
    launch_centre: Option<Vec3>,
    hero_scale: Option<f32>,
    weapon_scale: Option<f32>,
    /// The particle that chases the hook and ends the chain.
    parent: Vec<Vec3>,
    /// The chain's particles, in no order.
    chain: Vec<Vec3>,
    chain_drawn: bool,
    /// Each model drawn this frame: where it is and how it is turned.
    models: Vec<Transform>,
    head_sockets: Vec<Vec3>,
    rope: Vec<Vec3>,
    /// Whether the server's view tells of a hook in flight.
    hook_told: bool,
    /// How many frames of the hook's particle have been drawn so far (cards, lights, models).
    drawn: u32,
    drawn_endings: u32,
    /// Pudge's place in the sim's frame, if the view shows him.
    pudge: Option<Vec2>,
    /// The enemy heroes the view shows, in the sim's frame.
    enemies: Vec<Vec2>,
    visible_caster: bool,
    visible_chain_length: f32,
    visible_chain_bounds: Option<(f32, f32)>,
    blur_anchors: Vec<Vec3>,
    wire_head: Option<(Vec2, bool)>,
    blur: Vec<Vec3>,
    target_socket: Option<Vec3>,
    caught: bool,
    returning: bool,
    worn_visible: bool,
    tick: u32,
}

impl Frame {
    /// The head: the model drawn at the end of the chain.
    fn head(&self) -> Option<(Vec3, Quat)> {
        self.models
            .first()
            .map(|t| (source(t.translation), t.rotation))
    }
}

#[derive(Resource, Default)]
struct Frames(Vec<Frame>);

fn record(
    session: NonSend<crate::session::Session>,
    library: Option<Res<crate::particles::Library>>,
    playing: Res<crate::particles::Playing>,
    frame: Res<crate::particles::ParticleFrame>,
    units: Res<crate::units::UnitIndex>,
    attachments: Query<(&crate::units::Attachments, &crate::units::Rig)>,
    transforms: Query<&GlobalTransform>,
    wearables: Query<
        (Entity, &GlobalTransform, &InheritedVisibility),
        With<crate::units::Wearable>,
    >,
    children: Query<&Children>,
    pieces: Query<Entity, With<dbota_particles::ModelPiece>>,
    visible: Query<&InheritedVisibility>,
    names: Query<&Name>,
    ground: Res<crate::map::Ground>,
    time: Res<Time>,
    sight: crate::fog::EventSight,
    mut frames: ResMut<Frames>,
) {
    let (Some(view), Some(library)) = (session.0.view(), library) else {
        return;
    };
    let effect = library.effect("hero.pudge.hook").expect("the hook effect");
    let chain_looks: Vec<_> = effect
        .defs
        .iter()
        .filter(|def| def.path.ends_with("/pudge_meathook_chain.vpcf"))
        .flat_map(|def| {
            def.draws.iter().filter_map(|draw| {
                matches!(
                    def.system.renderers[draw.renderer].class,
                    heatel::particle::Renderer::Ropes(_)
                )
                .then_some(draw.look)
            })
        })
        .collect();
    let rope: Vec<_> = frame
        .batches
        .iter()
        .filter(|batch| chain_looks.contains(&batch.look))
        .flat_map(|batch| &frame.quads[batch.range.start as usize..batch.range.end as usize])
        .filter(|quad| quad.head_color[3] > 0.0 || quad.tail_color[3] > 0.0)
        .flat_map(|quad| {
            [
                source(Vec3::from_array(quad.head)),
                source(Vec3::from_array(quad.tail)),
            ]
        })
        .collect();
    let head_sockets = pieces
        .iter()
        .filter(|&piece| visible.get(piece).is_ok_and(|visibility| visibility.get()))
        .flat_map(|piece| children.iter_descendants(piece))
        .filter(|&node| {
            names
                .get(node)
                .is_ok_and(|name| name.as_str() == "attach_chain")
        })
        .filter_map(|node| {
            transforms
                .get(node)
                .ok()
                .map(|frame| source(frame.translation()))
        })
        .collect();
    let mut blur = Vec::new();
    for def in &effect.defs {
        let rope = def.path.ends_with("/pudge_meathook_blur.vpcf");
        if !rope && !def.path.ends_with("/pudge_meathook_blur_b.vpcf") {
            continue;
        }
        for batch in frame
            .batches
            .iter()
            .filter(|batch| def.draws.iter().any(|draw| draw.look == batch.look))
        {
            for quad in &frame.quads[batch.range.start as usize..batch.range.end as usize] {
                if quad.head_color[3] > 0.0 || quad.tail_color[3] > 0.0 {
                    let first = source(Vec3::from_array(quad.head));
                    let last = source(Vec3::from_array(quad.tail));
                    if rope {
                        blur.extend([first, last]);
                    } else {
                        blur.push(first.midpoint(last));
                    }
                }
            }
        }
    }
    let (mut hand, mut parent, mut chain) = (None, Vec::new(), Vec::new());
    let mut blur_anchors = Vec::new();
    for instance in playing.instances_of(effect) {
        // A fading trail keeps its last control points for simulation.
        if !instance.ending() {
            hand = Some(instance.nodes[0].control_points[0].position);
        }
        for node in &instance.nodes {
            let def = &effect.defs[node.def];
            if !instance.shows_system(node.def) || !def.path.ends_with("/pudge_meathook_blur.vpcf")
            {
                continue;
            }
            for initializer in &def.system.initializers {
                if let heatel::particle::Initializer::CreateWithinSphere(sphere) =
                    &initializer.class
                    && let Some(point) = sphere.transform.control_point()
                {
                    blur_anchors.push(node.control_points[point.index()].position);
                }
            }
        }
        for (def, at, _) in instance.particles() {
            match effect.defs[def].system.max_particles {
                16 => chain.push(at),
                _ if def == 0 => parent.push(at),
                _ => {}
            }
        }
    }
    let at = |unit: &bota_proto::UnitView| Vec2::new(unit.pos.x.to_f32(), unit.pos.y.to_f32());
    let hook = Some(Inflictor::Ability(AbilityId::MEAT_HOOK));
    let hero = view
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.team == Team::Radiant);
    let rig = hero
        .and_then(|unit| units.0.get(&unit.id))
        .and_then(|entity| attachments.get(*entity).ok());
    let socket = rig
        .and_then(|(attachments, _)| attachments.node("attach_weapon_chain_rt"))
        .and_then(|node| transforms.get(node).ok())
        .map(|t| source(t.translation()));
    let hero_scale = rig
        .and_then(|(_, rig)| transforms.get(rig.0).ok())
        .map(|t| {
            t.to_scale_rotation_translation().0.abs().max_element() / crate::map::UNITS_PER_METRE
        });
    let worn = wearables.iter().find_map(|(root, transform, visibility)| {
        children
            .iter_descendants(root)
            .any(|entity| {
                names
                    .get(entity)
                    .is_ok_and(|name| name.as_str() == "models/heroes/pudge/righthook.vmdl_c")
            })
            .then(|| {
                (
                    transform
                        .to_scale_rotation_translation()
                        .0
                        .abs()
                        .max_element(),
                    visibility.get(),
                )
            })
    });
    let home = hero.map(at);
    let wire_reach = home.map_or(0.0, |home| {
        view.projectiles
            .iter()
            .filter(|projectile| projectile.inflictor == hook)
            .map(|projectile| {
                Vec2::new(projectile.pos.x.to_f32(), projectile.pos.y.to_f32()).distance(home)
            })
            .fold(0.0, f32::max)
    });
    let launch_centre = home.map(|home| {
        let height = effect.defs[0]
            .system
            .operators
            .iter()
            .find_map(|operator| {
                let heatel::particle::Operator::MovementPlaceOnGround(movement) = &operator.class
                else {
                    return None;
                };
                let heatel::particle::FloatInput::Literal(height) = movement.offset else {
                    return None;
                };
                Some(height)
            })
            .expect("the installed hook carrier height");
        home.extend(ground.at(home.x, home.y) + height)
    });
    let socket_reach = launch_centre
        .zip(socket)
        .map_or(0.0, |(centre, socket)| socket.distance(centre));
    let point = |p: Spot| Vec2::new(p.x.to_f32(), p.y.to_f32());
    let audience = view.viewer.or(sight.fog().team).unwrap_or(Team::Radiant);
    let tether = view
        .tethers
        .iter()
        .find(|t| t.viewer == Team::Radiant || view.viewer.is_some());
    let wire_head = tether
        .and_then(|t| view.projectiles.iter().find(|p| p.id == t.id))
        .map(|p| (point(p.pos), sight.allows_projectile(p)));
    let spans = view
        .tethers
        .iter()
        .filter(|t| t.viewer == audience)
        .flat_map(|t| &t.spans);
    let visible_chain_length = spans
        .clone()
        .map(|s| point(s.start).distance(point(s.end)))
        .sum();
    let visible_chain_bounds = spans
        .clone()
        .map(|s| s.start.x.to_f32().min(s.end.x.to_f32()))
        .min_by(f32::total_cmp)
        .zip(
            spans
                .map(|s| s.start.x.to_f32().max(s.end.x.to_f32()))
                .max_by(f32::total_cmp),
        );
    let caught = tether.and_then(|t| t.head).and_then(|head| head.caught);
    let target_socket = caught
        .and_then(|id| units.0.get(&id))
        .and_then(|entity| attachments.get(*entity).ok())
        .and_then(|(attachments, _)| attachments.node("attach_hitloc"))
        .and_then(|node| transforms.get(node).ok())
        .map(|transform| source(transform.translation()));
    frames.0.push(Frame {
        tick: view.tick,
        target_socket,
        caught: caught.is_some(),
        returning: tether
            .and_then(|tether| tether.head)
            .is_some_and(|head| head.returning),
        worn_visible: worn.is_some_and(|(_, visible)| visible),
        visible_caster: hero.is_some_and(|u| sight.allows_unit(u)),
        visible_chain_length,
        visible_chain_bounds,
        blur_anchors,
        wire_head,
        blur,
        seconds: time.elapsed_secs(),
        hand,
        socket,
        wire_reach,
        socket_reach,
        launch_centre,
        hero_scale,
        weapon_scale: worn.map(|(scale, _)| scale),
        parent,
        chain,
        chain_drawn: !rope.is_empty(),
        rope,
        head_sockets,
        models: frame
            .models()
            .filter(|(model, _)| {
                library.models[usize::from(*model)]
                    .file
                    .ends_with("/righthook.vmdl.skin0.glb")
            })
            .map(|(_, transform)| transform)
            .collect(),
        hook_told: view.projectiles.iter().any(|p| p.inflictor == hook),
        drawn: playing.drawn(effect),
        drawn_endings: playing.drawn_endings(effect),
        pudge: home,
        enemies: view
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Hero && u.team == Team::Dire)
            .map(at)
            .collect(),
    });
}

fn source(world: Vec3) -> Vec3 {
    Vec3::new(world.x, -world.z, world.y)
}

/// Records a match in which Pudge, learned and ready, throws the hook `throws` times toward
/// `AIM_DISTANCE` east of him at `at`, with an enemy hero on the line when `victim` (away from
/// either fountain, whose water kills an enemy hero that stands in it).
fn record_throws(victim: bool, throws: usize, at: (i32, i32)) -> ReplayFile {
    record_throws_with(victim, throws, at, None, AfterLaunch::Wait).0
}

fn record_throws_seen(
    victim: bool,
    throws: usize,
    at: (i32, i32),
    observer: Option<i32>,
) -> (ReplayFile, Vec<Frame>) {
    record_throws_with(victim, throws, at, observer, AfterLaunch::Wait)
}

#[derive(Clone, Copy)]
enum AfterLaunch {
    Wait,
    Move(Spot),
    HideCaster(Spot),
    LoseSharedVision,
    CatchAlly,
    KillCaught,
    KillCaster,
    WitnessLaunch,
}

fn record_throws_with(
    victim: bool,
    throws: usize,
    at: (i32, i32),
    observer: Option<i32>,
    after_launch: AfterLaunch,
) -> (ReplayFile, Vec<Frame>) {
    let mut game = Spectated::hero("2", HeroId::PUDGE);
    game.next_side(Team::Radiant);
    game.cheat(Cheat::CreepSpawning { on: false });
    game.cheat(Cheat::Teleport {
        at: Spot::from_ints(at.0, at.1),
    });
    game.cheat(Cheat::Levels { count: 1 });
    game.order(Order::Move {
        target: Target::None,
    });
    let hero = game.driver.my_hero().expect("Pudge");
    let at = game.driver.view().unwrap().unit(hero).unwrap().pos;
    if victim {
        game.cheat(Cheat::CreateHero {
            hero: HeroId::SHADOW_FIEND,
            enemy: !matches!(after_launch, AfterLaunch::CatchAlly),
            at: at + Spot::from_ints(VICTIM_DISTANCE, 0),
        });
    }
    let slot = AbilitySlot(
        game.driver
            .view()
            .unwrap()
            .unit(hero)
            .unwrap()
            .abilities
            .iter()
            .position(|ability| ability.id == AbilityId::MEAT_HOOK)
            .expect("the hook is in his kit") as u8,
    );
    game.order(Order::Learn { slot });
    if let Some(distance) = observer {
        if !matches!(after_launch, AfterLaunch::HideCaster(_)) {
            game.cheat(Cheat::TimeOfDay { night: true });
        }
        let offset = if matches!(after_launch, AfterLaunch::LoseSharedVision) {
            game.cheat(Cheat::AllVision { on: true });
            450
        } else {
            0
        };
        game.cheat(Cheat::CreateHero {
            hero: HeroId::SHADOW_FIEND,
            enemy: true,
            at: at + Spot::from_ints(distance, offset),
        });
        game.next_side(Team::Dire);
        game.client.core_mut().pinned = None;
        let world = game.client.world_mut();
        let mut camera = world.query::<&mut crate::camera::RtsCamera>();
        let mut camera = camera.single_mut(world).expect("the live camera");
        camera.focus = Vec3::new(
            at.x.to_f32() + AIM_DISTANCE as f32 / 2.0,
            0.0,
            -at.y.to_f32(),
        );
        game.client.app.init_resource::<Frames>().add_systems(
            PostUpdate,
            record
                .after(crate::particles::gather_frame)
                .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
    }
    if matches!(after_launch, AfterLaunch::WitnessLaunch) {
        game.client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(FRAME));
        game.wait(
            "the opponent can already draw the caster's hand",
            |_, client| {
                client
                    .world()
                    .resource::<Frames>()
                    .0
                    .last()
                    .is_some_and(|frame| frame.visible_caster && frame.socket.is_some())
            },
        );
    }
    for _ in 0..throws {
        game.cheat(Cheat::Refresh);
        game.order(Order::Cast {
            slot,
            target: Target::Pos(at + Spot::from_ints(AIM_DISTANCE, 0)),
        });
        match after_launch {
            AfterLaunch::Wait | AfterLaunch::CatchAlly | AfterLaunch::WitnessLaunch => {}
            AfterLaunch::KillCaught | AfterLaunch::KillCaster => {
                let mut target = None;
                game.wait("a moving hook for the death scenario", |driver, _| {
                    let view = driver.view().unwrap();
                    target = view.tethers.iter().find_map(|tether| {
                        let head = tether.head?;
                        match after_launch {
                            AfterLaunch::KillCaught => head.caught,
                            _ => view
                                .projectile(tether.id)
                                .filter(|shot| shot.pos != at)
                                .map(|_| hero),
                        }
                    });
                    target.is_some()
                });
                game.cheat(Cheat::Damage {
                    target: Target::Unit(target.unwrap()),
                    amount: 1000,
                    kind: bota_proto::DamageKind::Pure,
                });
            }
            AfterLaunch::Move(spot) => {
                game.wait("the head starts coming back", |driver, _| {
                    driver
                        .view()
                        .unwrap()
                        .tethers
                        .iter()
                        .any(|t| t.head.is_some_and(|h| h.returning))
                });
                game.cheat(Cheat::Teleport { at: spot });
            }
            AfterLaunch::LoseSharedVision => {
                game.wait(
                    "the opponent sees the start and head of the return",
                    |_, client| {
                        client.view().tethers.iter().any(|tether| {
                            tether
                                .head
                                .is_some_and(|head| head.returning && head.origin.is_some())
                        })
                    },
                );
                game.cheat(Cheat::AllVision { on: false });
            }
            AfterLaunch::HideCaster(spot) => {
                game.wait("the launch is seen", |_, client| {
                    client.view().tethers.iter().any(|t| {
                        t.source.is_some()
                            && t.head.is_some()
                            && t.spans
                                .iter()
                                .any(|s| !s.start.within(s.end, bota_proto::Fixed::from_int(100)))
                    })
                });
                game.cheat(Cheat::Teleport { at: spot });
            }
        }
        game.wait("the hook has flown and come back", |driver, _| {
            let view = driver.view().unwrap();
            view.tethers.is_empty()
                && (matches!(after_launch, AfterLaunch::KillCaster)
                    || view
                        .unit(hero)
                        .is_some_and(|unit| unit.abilities[slot.0 as usize].cooldown_left > 0))
        });
    }
    if observer.is_some() {
        game.wait(
            "the side plays to the end of the flight",
            |driver, client| client.tick() >= driver.view().expect("driving view").tick,
        );
    }
    let frames = game
        .client
        .world()
        .get_resource::<Frames>()
        .map_or_else(Vec::new, |frames| frames.0.clone());
    let file = game.save();
    // A person looking at the renders keeps the record: HOOK_EVIDENCE names a folder for it.
    if let Some(dir) = std::env::var_os("HOOK_EVIDENCE") {
        let name = match (observer, after_launch) {
            (_, AfterLaunch::HideCaster(_)) => "hook-caster-leaves-fog.brp".into(),
            (_, AfterLaunch::Move(_)) => "hook-moving-caster.brp".into(),
            (_, AfterLaunch::CatchAlly) => "hook-ally.brp".into(),
            (_, AfterLaunch::KillCaught) => "hook-victim-dies.brp".into(),
            (_, AfterLaunch::KillCaster) => "hook-caster-dies.brp".into(),
            (Some(distance), _) => format!("hook-fog-{distance}.brp"),
            (None, _) => format!("hook-{}{throws}.brp", if victim { "hit" } else { "miss" }),
        };
        std::fs::copy(file.path(), std::path::Path::new(&dir).join(name)).expect("the record kept");
    }
    (file, frames)
}

/// Plays a record from the side `pov` sees it, a `FRAME` at a time, and returns every frame.
fn play(file: &ReplayFile, pov: u8, at: (i32, i32)) -> Vec<Frame> {
    play_recording(file, pov, at, false)
}

fn play_recording(file: &ReplayFile, pov: u8, at: (i32, i32), isolate: bool) -> Vec<Frame> {
    let mut client = replay_client(file, pov, at);
    if isolate {
        let effect = client
            .world()
            .resource::<crate::particles::Library>()
            .effect("hero.pudge.hook")
            .unwrap()
            .clone();
        client
            .world_mut()
            .resource_mut::<crate::particles::Playing>()
            .isolate_effect(effect);
    }
    client.within(SOON, "the recorded match played", |c| {
        c.core().view().is_some_and(|view| view.tick >= file.tick())
    });
    if let Source::Replay(player) = &mut client.core_mut().source {
        player.paused = true;
    }
    client.step_frames(2);
    client.world().resource::<Frames>().0.clone()
}

fn replay_client(file: &ReplayFile, pov: u8, at: (i32, i32)) -> Client {
    let pov = pov.to_string();
    let mut client = Client::run(&[
        "--replay",
        file.path().to_str().expect("a UTF-8 path"),
        "--pov",
        &pov,
        "--look",
        &format!("{},{}", at.0 + AIM_DISTANCE / 2, at.1),
    ]);
    client.app.init_resource::<Frames>().add_systems(
        PostUpdate,
        record
            .after(crate::particles::gather_frame)
            .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
    );
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(FRAME));
    client.within(SOON, "the particle library ready", |c| {
        c.world()
            .get_resource::<crate::particles::Library>()
            .is_some()
    });
    client
}

/// The frames of each flight (the server tells of a hook flying), in order.
fn flights(frames: &[Frame]) -> Vec<&[Frame]> {
    frames
        .chunk_by(|a, b| a.hook_told == b.hook_told)
        .filter(|run| run[0].hook_told)
        .collect()
}

/// The distances of the head from where Pudge stood, one for each frame of a flight.
fn reach_of(flight: &[Frame], home: Vec2) -> Vec<f32> {
    flight
        .iter()
        .map(|f| {
            let head = f.head().unwrap_or_else(|| {
                panic!(
                    "no head drawn at the end of the chain at {:.2} s ({} models, parent {:?})",
                    f.seconds,
                    f.models.len(),
                    f.parent
                )
            });
            Vec2::new(head.0.x, head.0.y).distance(home)
        })
        .collect()
}

/// The roll of the head about the line it flies along, relative to its first frame, in degrees.
fn turns_of(flight: &[Frame]) -> Vec<f32> {
    let first = flight.iter().find_map(Frame::head).expect("a head").1;
    flight
        .iter()
        .filter_map(Frame::head)
        .map(|(_, turn)| {
            let twist = turn * first.inverse();
            2.0 * twist.x.atan2(twist.w).to_degrees()
        })
        .collect()
}

/// Every frame where the chain's far end is the head and its near end the hand, within a few
/// units: the chain is drawn from the hand to the hook.
fn assert_chain_spans(flight: &[Frame]) {
    for f in flight {
        let (hand, head) = (f.hand.expect("alive"), f.head().expect("a head").0);
        let near = f
            .chain
            .iter()
            .map(|c| c.distance(hand))
            .fold(f32::MAX, f32::min);
        let far = f
            .chain
            .iter()
            .max_by(|a, b| a.distance(hand).total_cmp(&b.distance(hand)))
            .expect("a chain");
        assert!(
            near < 25.0,
            "at {:.2} s the chain starts {near:.0} from the hand",
            f.seconds
        );
        assert!(
            far.distance(head) < 25.0,
            "at {:.2} s the chain ends {:.0} from the head",
            f.seconds,
            far.distance(head)
        );
        assert_eq!(f.chain.len(), 16, "a chain of sixteen links");
    }
}

#[test]
fn a_flying_hook_head_uses_the_weapon_scale_of_the_drawn_hero() {
    let file = record_throws(true, 1, MID);
    assert_flying_weapon_scale(&play(&file, 0, MID));
}

fn assert_flying_weapon_scale(frames: &[Frame]) {
    use heatel::particle::{Field, FloatInput, Initializer, Mapping, Renderer, SetMethod, System};

    let cache = std::path::PathBuf::from(std::env::var_os("DBOTA_ASSETS").expect("test cache"));
    let particle: serde_json::Value = serde_json::from_slice(
        &fs::read(cache.join("particles/pudge_meathook.json")).expect("the imported hook"),
    )
    .expect("particle graph");
    let systems = particle["systems"].as_array().expect("particle systems");
    let (head_system, renderer) = systems
        .iter()
        .find_map(|entry| {
            let system: System = serde_json::from_value(entry["system"].clone()).expect("system");
            let renderer = system.renderers.iter().find_map(|renderer| {
                let Renderer::Models(models) = &renderer.class else {
                    return None;
                };
                models.model_input.map(|_| models.clone())
            })?;
            Some((system, renderer))
        })
        .expect("the flying weapon renderer");
    let (input, output) = head_system
        .initializers
        .iter()
        .find_map(|initializer| match initializer.class {
            Initializer::InitFloat {
                value:
                    FloatInput::ParticleFloat {
                        field: Field::RADIUS,
                        mapping: Mapping::Remap { input, output, .. },
                        ..
                    },
                field: Field::RADIUS,
                set_method: SetMethod::ScaleInitial,
                ..
            } => Some((input, output)),
            _ => None,
        })
        .expect("the installed radius remap");
    let FloatInput::Literal(radius_scale) = renderer.radius_scale else {
        panic!("a literal scale for the flying weapon");
    };
    let flying_file = particle["models"][0]["file"]
        .as_str()
        .expect("the imported weapon");
    let flying_span = mesh_span(&cache.join(flying_file));
    let held_span = mesh_span(&cache.join("models/hero/pudge_weapon/pudge_weapon.glb"));
    assert_eq!(
        flying_span, held_span,
        "the same weapon geometry flies and is worn"
    );
    let flight = flights(frames)[0];
    for frame in flight {
        let hero = frame.hero_scale.expect("the drawn hero scale");
        let [head] = frame.models.as_slice() else {
            panic!("one hook head must be drawn at {:.2} s", frame.seconds);
        };
        let [low, high] = input;
        let [small, large] = output;
        let t = ((hero - low) / (high - low)).clamp(0.0, 1.0);
        let remapped = small + t * (large - small);
        let held_world = held_span
            * frame
                .weapon_scale
                .expect("the mounted weapon's drawn scale");
        let expected = (flying_span / held_span) * remapped * radius_scale;
        let actual = flying_span * head.scale.abs().max_element() / held_world;
        assert!(
            (actual - expected).abs() < 0.002,
            "flying-to-held weapon extent {actual:.4} disagrees with the authored ratio {expected:.4}"
        );
    }
}

/// The widest rest axis of a cached glTF's rendered primitives, in metres.
fn mesh_span(path: &Path) -> f32 {
    let bytes = fs::read(path).expect("an imported glTF");
    assert_eq!(&bytes[..4], b"glTF");
    let length = u32::from_le_bytes(bytes[12..16].try_into().expect("the JSON length")) as usize;
    let gltf: serde_json::Value =
        serde_json::from_slice(&bytes[20..20 + length]).expect("the glTF JSON");
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for mesh in gltf["meshes"].as_array().expect("glTF meshes") {
        for primitive in mesh["primitives"].as_array().expect("glTF primitives") {
            let index = primitive["attributes"]["POSITION"]
                .as_u64()
                .expect("POSITION") as usize;
            let accessor = &gltf["accessors"][index];
            let bound = |name: &str| {
                Vec3::from(
                    serde_json::from_value::<[f32; 3]>(accessor[name].clone())
                        .expect("a position bound"),
                )
            };
            low = low.min(bound("min"));
            high = high.max(bound("max"));
        }
    }
    (high - low).max_element()
}

#[test]
fn a_hook_launch_starts_at_the_animated_weapon_socket() {
    let file = record_throws(true, 1, MID);
    let frames = play(&file, 0, MID);
    let launch = &flights(&frames)[0][0];
    let socket = launch.socket.expect("the weapon's chain socket");
    let head = launch.head().expect("the hook head at launch").0;
    assert!(
        head.distance(socket) < 0.05,
        "the hook starts {:.1} units from the weapon socket",
        head.distance(socket)
    );
}

#[test]
fn the_chain_stays_on_the_animated_weapon_socket_through_flight_and_return() {
    let file = record_throws(true, 1, MID);
    let frames = play(&file, 0, MID);
    for frame in flights(&frames)[0] {
        let socket = frame.socket.expect("the thrower's weapon chain socket");
        assert!(
            frame
                .chain
                .iter()
                .any(|point| point.distance(socket) < 12.0),
            "chain missed the animated weapon socket at {:.2} s",
            frame.seconds
        );
    }
}

#[test]
fn the_chain_stays_on_the_hook_head_socket_through_flight_and_return() {
    let file = record_throws(true, 1, MID);
    let frames = play(&file, 0, MID);
    for frame in flights(&frames)[0] {
        assert!(
            frame.chain_drawn,
            "the active hook draws no chain at {:.2} s",
            frame.seconds
        );
        let [socket] = frame.head_sockets.as_slice() else {
            panic!(
                "the imported head socket must be drawn at {:.2} s, sockets {:?}",
                frame.seconds, frame.head_sockets
            );
        };
        assert!(
            frame
                .rope
                .iter()
                .any(|point| point.distance(*socket) < 0.05),
            "chain missed the hook head socket at {:.2} s",
            frame.seconds
        );
    }
}

#[test]
fn the_hook_leaves_no_flight_effect_on_the_first_frame_home() {
    let file = record_throws(false, 1, MID);
    let frames = play(&file, 0, MID);
    let first_home = frames
        .windows(2)
        .find(|frames| frames[0].hook_told && !frames[1].hook_told)
        .expect("the first frame after the hook returned");
    assert!(
        first_home[1].hand.is_none()
            && !first_home[1].chain_drawn
            && first_home[1].models.is_empty(),
        "the flight effect survives the projectile's return"
    );
}

/// A hook that catches nothing: its head leaves along the cast line at the ability's speed,
/// turning, with the chain from the hand to it, reaches the full range and returns, and
/// nothing of it is left.
#[test]
fn a_thrown_hook_flies_out_turning_on_a_chain_to_full_range_and_comes_back_empty() {
    let file = record_throws(false, 1, YARD);
    let frames = play(&file, 0, YARD);
    let flights = flights(&frames);
    assert_eq!(flights.len(), 1, "one throw is one flight");
    let flight = flights[0];
    let home = flight[0]
        .pudge
        .expect("Pudge is in the view of his own side");
    let reach = reach_of(flight, home);
    let farthest = reach.iter().copied().fold(0.0, f32::max);
    let full = rules::HOOK_RANGE as f32;
    assert!(
        (full - TRAIL..=full + TRAIL).contains(&farthest),
        "the head reaches {farthest:.0}, not the hook's range {full}"
    );
    let top = reach.iter().position(|r| *r == farthest).unwrap();
    let out = &flight[..=top];
    // The speed: the head covers the flight's middle at the ability's speed.
    let middle: Vec<(f32, f32)> = out
        .iter()
        .zip(&reach)
        .filter(|(_, r)| (300.0..=1000.0).contains(*r))
        .map(|(f, r)| (f.seconds, *r))
        .collect();
    let (first, last) = (middle[0], *middle.last().unwrap());
    let speed = (last.1 - first.1) / (last.0 - first.0);
    let expected = rules::HOOK_SPEED as f32;
    assert!(
        (speed - expected).abs() < expected * 0.1,
        "the head flies at {speed:.0} units a second, the ability's speed is {expected}"
    );
    // The wire's position can be one tick ahead of the interpolated projectile. Once clear
    // of the launch socket the head stays on the cast line. The launch itself leaves the
    // animated socket and closes its measured lateral offset.
    let one_tick = rules::HOOK_SPEED as f32 / rules::TICKS_PER_SECOND as f32;
    for f in out {
        let head = f.head().expect("a head").0;
        let lateral_limit = if f.wire_reach > f.socket_reach + one_tick {
            30.0
        } else {
            let socket = f.socket.expect("the launch socket");
            (socket.y - home.y).abs() + 0.05
        };
        assert!(
            (head.y - home.y).abs() < lateral_limit && head.x >= home.x - 30.0,
            "the head at ({:.0}, {:.0}) is off the cast line from {home:?}",
            head.x,
            head.y
        );
    }
    let turns = turns_of(out);
    let spread = turns.iter().copied().fold(f32::MIN, f32::max)
        - turns.iter().copied().fold(f32::MAX, f32::min);
    assert!(
        spread > 25.0,
        "the head turns {spread:.0} degrees over its flight"
    );
    assert_chain_spans(flight);
    // It comes back: the reach falls to nothing and no enemy was dragged.
    assert!(
        reach.last().unwrap() < &300.0,
        "the hook comes home: {reach:?}"
    );
    assert!(
        flight
            .iter()
            .all(|f| f.enemies.iter().all(|e| e.distance(home) > 1000.0)),
        "a missed hook brings nothing back"
    );
    // And nothing of it is left once it is home: no head at once, no particle a moment later.
    let home_at = flight.last().expect("a flight").seconds;
    for f in frames.iter().filter(|f| f.seconds > home_at) {
        assert!(
            f.head().is_none(),
            "the head stays at {:.2} s after the hook is home",
            f.seconds
        );
        if f.seconds > home_at + 0.5 {
            assert!(
                f.hand.is_none() && f.chain.is_empty(),
                "the chain stays at {:.2} s after the hook is home",
                f.seconds
            );
        }
    }
}

/// A hook that catches an enemy: the hook and its victim travel back together with the chain
/// shortening.
#[test]
fn a_hook_that_catches_an_enemy_brings_it_back_with_the_chain_shortening() {
    let file = record_throws(true, 1, MID);
    let frames = play(&file, 0, MID);
    let flights = flights(&frames);
    assert_eq!(flights.len(), 1);
    let flight = flights[0];
    let home = flight[0]
        .pudge
        .expect("Pudge is in the view of his own side");
    let reach = reach_of(flight, home);
    let farthest = reach.iter().copied().fold(0.0, f32::max);
    let top = reach.iter().position(|r| *r == farthest).unwrap();
    assert!(
        farthest < VICTIM_DISTANCE as f32 + 250.0,
        "the hook turned back at {farthest:.0}, where it caught the enemy {VICTIM_DISTANCE} away, not at its full range"
    );
    // The chain shortens on the way back, never growing by more than the head's own jitter.
    for pair in reach[top..].windows(2) {
        assert!(
            pair[1] <= pair[0] + 15.0,
            "the chain grows on the way back: {reach:?}"
        );
    }
    // The victim is drawn where the hook is, the whole way.
    for f in &flight[top + 2..] {
        let head = f.head().expect("a head").0;
        let victim = f
            .enemies
            .iter()
            .min_by(|a, b| a.distance(home).total_cmp(&b.distance(home)))
            .expect("the enemy is in the view");
        assert!(
            Vec2::new(head.x, head.y).distance(*victim) < TRAIL,
            "the victim at {victim:?} is not with the head at {head:?}"
        );
    }
    assert_chain_spans(flight);
}

/// A second hook thrown right after the first is drawn like the first.
#[test]
fn a_second_hook_thrown_right_after_the_first_is_drawn_as_the_first_was() {
    let file = record_throws(false, 2, YARD);
    let frames = play(&file, 0, YARD);
    let flights = flights(&frames);
    assert_eq!(flights.len(), 2, "two throws are two flights");
    for flight in flights {
        let home = flight[0]
            .pudge
            .expect("Pudge is in the view of his own side");
        let farthest = reach_of(flight, home).into_iter().fold(0.0, f32::max);
        assert!(
            farthest > rules::HOOK_RANGE as f32 - TRAIL,
            "a throw reaches {farthest:.0}"
        );
        assert_chain_spans(flight);
    }
}

/// A hook thrown where the enemy has no sight is not drawn for them.
#[test]
fn a_hook_thrown_out_of_the_enemys_sight_is_not_drawn_for_the_enemy() {
    let file = record_throws(false, 1, YARD);
    let seen = play(&file, 0, YARD);
    assert!(
        seen.iter().any(|f| f.head().is_some()) && seen.iter().any(|f| f.drawn > 0),
        "the thrower's side sees the head and the chain"
    );
    let unseen = play(&file, 1, YARD);
    assert!(
        unseen.iter().any(|f| f.hook_told),
        "the record holds the throw, so that its absence is the fog's doing"
    );
    let shown = unseen.iter().find(|f| f.drawn > 0 || f.head().is_some());
    assert!(
        shown.is_none(),
        "the other side is shown a hook it cannot see: at {:?} s, drawn {:?}, head {:?}",
        shown.map(|f| f.seconds),
        shown.map(|f| f.drawn),
        shown.map(|f| f.head())
    );
}

#[test]
fn review_flying_head_and_chain_emerge_from_fog_on_the_victims_stream_and_replay() {
    let (file, live) = record_throws_seen(false, 1, YARD, Some(1100));
    let replay = play(&file, 1, YARD);
    for (name, frames) in [("side stream", &live), ("side replay", &replay)] {
        let visible = frames
            .iter()
            .filter(|f| {
                !f.visible_caster
                    && f.wire_head.is_some_and(|(_, visible)| visible)
                    && f.visible_chain_length >= 64.0
            })
            .collect::<Vec<_>>();
        assert!(
            visible.len() >= 2,
            "{name} needs several frames with a hidden caster and visible flight"
        );
        let drawn: Vec<_> = visible
            .iter()
            .filter_map(|f| {
                f.head()
                    .filter(|(at, _)| at.truncate().distance(f.wire_head.unwrap().0) <= 70.0)
                    .map(|_| f.chain_drawn)
            })
            .collect();
        assert!(
            drawn.len() * 2 > visible.len(),
            "{name} presented {} heads over {} visible frames",
            drawn.len(),
            visible.len()
        );
        assert!(
            drawn.iter().any(|chain| *chain),
            "{name} drew no visible chain"
        );
        for frame in visible.iter().filter(|frame| frame.chain_drawn) {
            let (left, _) = frame
                .visible_chain_bounds
                .expect("the visible chain has bounds");
            // A visible weapon can itself extend across the ground sight edge.
            let left = frame
                .head_sockets
                .iter()
                .fold(left, |edge, socket| edge.min(socket.x));
            let min_drawn = frame
                .rope
                .iter()
                .map(|point| point.x)
                .fold(f32::INFINITY, f32::min);
            assert!(
                min_drawn >= left - 1.0,
                "at tick {} the {name} chain draws behind the visibility boundary at {left}: {min_drawn}",
                frame.tick
            );
        }
    }
}

#[test]
fn review_the_head_disappears_into_fog_while_the_visible_chain_stays() {
    let (file, _) = record_throws_seen(false, 1, YARD, Some(-450));
    let replay = play(&file, 1, YARD);
    let visible = replay
        .iter()
        .filter(|f| {
            f.visible_caster
                && f.wire_head.is_some_and(|(_, visible)| !visible)
                && f.visible_chain_length >= 64.0
        })
        .collect::<Vec<_>>();
    assert!(
        visible.len() >= 2,
        "the hook needs time beyond the line of sight with its chain still visible"
    );
    assert!(
        visible.iter().any(|f| f.chain_drawn),
        "the visible chain went away with the head"
    );
    let snapped: Vec<_> = visible
        .iter()
        .filter_map(|f| f.head().map(|h| h.0))
        .collect();
    assert!(
        snapped.is_empty(),
        "visible heads while the actual head was hidden: {snapped:?}"
    );
}

#[test]
fn review_the_hook_faces_the_cast_line_from_the_hand_through_return() {
    let file = record_throws(false, 1, MID);
    let frames = play(&file, 0, MID);
    for frame in flights(&frames)[0] {
        let (_, rotation) = frame.head().unwrap_or_else(|| {
            panic!(
                "head missing at tick {}, at {:?}; carrier {:?}, chain count {}",
                frame.tick,
                frame.wire_head,
                frame.parent,
                frame.chain.len()
            )
        });
        let direction = source(rotation * Vec3::NEG_Z);
        let angle = direction.angle_between(Vec3::X).to_degrees();
        assert!(
            angle < 3.0,
            "at {:.3} s, the blade is {angle:.1} degrees off the cast line",
            frame.seconds
        );
    }
}

#[test]
fn review_the_blur_draws_near_the_visible_head_as_it_launches_and_returns() {
    let file = record_throws(true, 1, MID);
    let frames = play_recording(&file, 0, MID, true);
    let flight = flights(&frames)[0];
    let count = flight.iter().filter(|f| !f.blur.is_empty()).count();
    assert!(
        count > flight.len() / 2,
        "visible blur on {count} of {} flight frames",
        flight.len()
    );
    let oldest = 0.35;
    let speed = rules::HOOK_SPEED as f32;
    for frame in flight {
        let (head, _) = frame.head().expect("the head is drawn");
        let [anchor] = frame.blur_anchors.as_slice() else {
            panic!("a visible blur emitter has one carrier position");
        };
        assert!(
            anchor.distance(head) < 0.1,
            "tick {}: blur anchor {anchor:?} trails the drawn head at {head:?}",
            frame.tick
        );
        let max_distance = (frame.seconds - flight[0].seconds + 2.0 * FRAME.as_secs_f32())
            .clamp(0.0, oldest)
            * speed
            + 96.0;
        for point in &frame.blur {
            assert!(
                point.distance(head) <= max_distance,
                "at {:.3} s the blur at {point:?} is {} units from {head:?}; tick {}, at {:?}; launch {}",
                frame.seconds,
                point.distance(head),
                frame.tick,
                frame.wire_head,
                flight[0].seconds
            );
        }
    }
}

#[test]
fn review_a_caster_leaving_sight_does_not_take_the_visible_head_or_chain_with_him() {
    let (file, live) = record_throws_with(
        false,
        1,
        MID,
        Some(550),
        AfterLaunch::HideCaster(Spot::from_ints(MID.0 - 3300, MID.1)),
    );
    let replay = play(&file, 1, MID);
    for (name, frames) in [("side stream", &live), ("side replay", &replay)] {
        let start = frames
            .iter()
            .position(|f| f.visible_caster && f.wire_head.is_some())
            .expect("launch seen");
        let segment = frames[start..]
            .iter()
            .filter(|f| {
                !f.visible_caster
                    && f.wire_head.is_some_and(|(_, seen)| seen)
                    && f.visible_chain_length > 20.0
            })
            .collect::<Vec<_>>();
        assert!(
            !segment.is_empty(),
            "{name} requires a visible head and chain after the caster vanishes"
        );
        for frame in segment {
            let (head, _) = frame.head().expect("a visible head keeps drawing");
            let at = frame
                .target_socket
                .map_or(frame.wire_head.unwrap().0, Vec3::truncate);
            assert!(
                head.truncate().distance(at)
                    < if frame.target_socket.is_some() {
                        0.1
                    } else {
                        rules::HOOK_SPEED as f32 / rules::TICKS_PER_SECOND as f32 + 0.1
                    },
                "visible head is at {head:?}, expected {at:?} at tick {}",
                frame.tick
            );
            assert!(
                frame.chain_drawn,
                "visible chain missing at tick {}",
                frame.tick
            );
        }
    }
}

#[test]
fn a_return_seen_without_the_hand_has_no_ending_in_fog_live_or_in_replay() {
    let (file, live) =
        record_throws_with(false, 1, YARD, Some(1_100), AfterLaunch::LoseSharedVision);
    let replay = play(&file, 1, YARD);
    for (name, frames) in [("side stream", &live), ("side replay", &replay)] {
        assert!(
            frames
                .iter()
                .any(|frame| frame.visible_caster && frame.returning && frame.head().is_some()),
            "{name}: the observer first sees the caster and returning hook"
        );
        assert!(
            frames
                .iter()
                .any(|frame| !frame.visible_caster && frame.returning && frame.head().is_some()),
            "{name}: the returning head stays drawn after the caster enters fog"
        );
    }
    let ending_count = |frames: &[Frame]| frames.last().expect("a drawn return").drawn_endings;
    assert_eq!(
        [ending_count(&live), ending_count(&replay)],
        [0, 0],
        "the hidden hand left a visible return trail on the side stream or replay"
    );
}

#[test]
fn review_the_caught_hook_moves_with_the_animated_victim_and_stops_spinning() {
    let file = record_throws(true, 1, MID);
    let frames = play(&file, 0, MID);
    let mut previous: Option<Quat> = None;
    let mut compared = 0;
    for frame in frames.iter().filter(|frame| frame.caught) {
        let socket = frame
            .target_socket
            .expect("the victim's visible hit attachment");
        let head = frame.models.first().expect("the caught hook is drawn");
        let error = source(head.translation).distance(socket);
        assert!(
            error < 0.1,
            "{} units from the victim attachment at tick {}",
            error,
            frame.tick
        );
        if let Some(rotation) = previous {
            assert!(
                head.rotation.angle_between(rotation).to_degrees() < 0.2,
                "the latched head still rolls at tick {}",
                frame.tick
            );
            compared += 1;
        }
        previous = Some(head.rotation);
    }
    assert!(
        compared >= 3,
        "the catch must be visible over several frames"
    );
}

#[test]
fn review_the_worn_hook_is_hidden_while_the_projectile_is_drawn_and_restored_after() {
    let file = record_throws(true, 1, MID);
    let frames = play(&file, 0, MID);
    let start = frames
        .iter()
        .position(|f| f.head().is_some())
        .expect("the launch");
    let end = frames
        .iter()
        .rposition(|f| f.head().is_some())
        .expect("the return");
    assert!(
        frames[..start].iter().any(|f| f.worn_visible),
        "the hero holds a hook before casting"
    );
    for frame in &frames[start..=end] {
        assert!(
            !frame.worn_visible,
            "the worn blade is still drawn during flight at tick {}",
            frame.tick
        );
    }
    assert!(
        frames[end + 1..].iter().any(|f| f.worn_visible),
        "the hero holds the hook again after it comes back"
    );
}

#[test]
fn review_moving_the_thrower_beside_the_return_path_does_not_redraw_the_tip_at_his_hand() {
    let moved = Spot::from_ints(YARD.0 + 500, YARD.1 + 60);
    let (file, _) = record_throws_with(false, 1, YARD, None, AfterLaunch::Move(moved));
    let frames = play(&file, 0, YARD);
    let moved = Vec2::new(moved.x.to_f32(), moved.y.to_f32());
    let mut measured = 0;
    for frame in frames
        .iter()
        .filter(|f| f.pudge == Some(moved) && f.returning)
    {
        let head = frame.head().expect("the returning hook keeps drawing").0;
        let wire = frame.wire_head.expect("the return path").0;
        if wire.distance(moved) < 110.0 {
            assert!(
                (head.y - YARD.1 as f32).abs() < 0.1,
                "the head moved off the cast line at tick {}: {head:?}",
                frame.tick
            );
            measured += 1;
        }
    }
    assert!(
        measured >= 2,
        "the moved caster and returning head must be close for multiple frames"
    );
}

#[test]
fn review_two_overlapping_throws_follow_their_own_paths() {
    let file = record_throws(false, 1, YARD);
    let mut client = replay_client(&file, 0, YARD);
    let mut snapshots = Vec::new();
    client.within(SOON, "consecutive travelling hook snapshots", |client| {
        let Some(view) = client.core().view() else {
            return false;
        };
        if view
            .tethers
            .iter()
            .any(|t| t.viewer == Team::Radiant && t.head.is_some())
            && snapshots
                .last()
                .is_none_or(|previous: &bota_proto::WorldView| previous.tick < view.tick)
        {
            snapshots.push(view.clone());
        }
        snapshots.len() == 12
    });
    let mut combined = Vec::new();
    for (older, newer) in snapshots[..5].iter().zip(&snapshots[7..]) {
        let mut view = newer.clone();
        let older_tether = older
            .tethers
            .iter()
            .find(|t| t.viewer == Team::Radiant)
            .unwrap();
        let mut second = older_tether.clone();
        let mut shot = *older.projectile(second.id).unwrap();
        shot.id.generation += 1;
        second.id = shot.id;
        view.projectiles.push(shot);
        view.projectiles.sort_unstable_by_key(|shot| shot.id);
        view.tethers.push(second);
        combined.push(view);
    }
    let begin = combined.first().unwrap().tick;
    let end = combined.last().unwrap().tick;
    let old = client.world().resource::<Frames>().0.len();
    replace_replay(&mut client, &file, combined, false);
    client.within(SOON, "the overlapping trajectories were played", |c| {
        c.core().view().is_some_and(|view| view.tick >= end)
    });
    let frames = &client.world().resource::<Frames>().0[old..];
    let home = YARD.0 as f32;
    let both = frames
        .iter()
        .filter(|frame| {
            (begin..=end).contains(&frame.tick) && {
                let xs = frame.models.iter().map(|head| head.translation.x);
                xs.clone().any(|x| {
                    x > home + 120.0
                        && xs
                            .clone()
                            .any(|other| other >= home && (x - other).abs() > 120.0)
                })
            }
        })
        .count();
    assert!(
        both >= 3,
        "only {both} frames drew both distinct head positions from the same caster"
    );
}

fn replace_replay(
    client: &mut Client,
    file: &ReplayFile,
    views: Vec<bota_proto::WorldView>,
    paused: bool,
) {
    use bota_client_core::ReplayPlayer;
    use bota_proto::{ReplayHeader, ReplayRecord, ServerMsg};

    let mut recording = Vec::new();
    let mut original = ReplayPlayer::load(file.path()).expect("the recorded match");
    let start = (0..10)
        .find_map(|_| {
            original.poll(0.1).into_iter().find_map(|message| {
                if let ServerMsg::MatchStart { info } = message {
                    Some(info)
                } else {
                    None
                }
            })
        })
        .expect("the original MatchStart reaches the replay");
    let initial = [
        ReplayRecord::Header(ReplayHeader::new(bota_core::WIRE_FINGERPRINT)),
        ReplayRecord::Msg(ServerMsg::MatchStart { info: start }),
    ];
    let snapshots = views
        .into_iter()
        .map(|view| ReplayRecord::Msg(ServerMsg::Snapshot { view }));
    for record in initial.into_iter().chain(snapshots) {
        bota_proto::encode_frame(&record, &mut recording).expect("the replay can be read again");
    }
    let mut player = ReplayPlayer::from_reader(std::io::Cursor::new(recording));
    player.paused = paused;
    let mut rewound = bota_client_core::App::new(Source::Replay(Box::new(player)));
    rewound.pov = Some(bota_proto::SlotId(0));
    rewound.my_slot = rewound.pov;
    *client.core_mut() = rewound;
}

#[test]
fn review_rewinding_and_pausing_recreates_the_launch_pose_without_return_trails() {
    let file = record_throws(true, 1, MID);
    let mut client = replay_client(&file, 0, MID);
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect("hero.pudge.hook")
        .expect("the hook")
        .clone();
    let caught_roll = effect
        .defs
        .iter()
        .flat_map(|def| &def.system.operators)
        .find_map(|op| {
            let heatel::particle::Operator::RemapSpeed {
                field,
                output: [first, last],
                ..
            } = op.class
            else {
                return None;
            };
            (field == heatel::particle::Field::ROLL && first == last).then_some(first)
        })
        .expect("authored caught roll");
    client
        .world_mut()
        .resource_mut::<crate::particles::Playing>()
        .isolate_effect(effect);
    client.within(SOON, "the launch was drawn", |client| {
        client
            .world()
            .resource::<Frames>()
            .0
            .last()
            .is_some_and(|frame| frame.head().is_some() && !frame.caught)
    });
    let launch = client.view().clone();
    client.within(SOON, "the return was drawn", |client| {
        client
            .world()
            .resource::<Frames>()
            .0
            .last()
            .is_some_and(|frame| frame.head().is_some() && frame.caught)
    });

    let returning_rotation = client
        .world()
        .resource::<Frames>()
        .0
        .last()
        .unwrap()
        .head()
        .unwrap()
        .1;
    replace_replay(&mut client, &file, vec![launch.clone()], true);
    client.within(SOON, "the paused launch snapshot", |client| {
        client
            .core()
            .view()
            .is_some_and(|view| view.tick == launch.tick)
    });
    client.step_frames(2);
    let frames = client.world().resource::<Frames>();
    let frame = frames.0.last().expect("the paused pose was sampled");
    let (head, rotation) = frame.head().expect("launch head drawn while paused");
    let socket = frame.socket.expect("the animated launch attachment");
    assert!(
        head.distance(socket) < 0.1,
        "rewind left the head at {head:?}, with the launch attachment at {socket:?}"
    );
    let discarded_roll = rotation.angle_between(returning_rotation);
    assert!(
        (discarded_roll - caught_roll.abs()).abs() < 0.01,
        "after rewind the pose is {discarded_roll} radians from catch, expected the authored {caught_roll}"
    );
    assert!(
        frame.chain_drawn && frame.rope.iter().all(|point| point.is_finite()),
        "the rewound chain has no finite submitted rope"
    );
    assert!(
        frame.blur.iter().all(|point| point.distance(head) < 70.0),
        "rewind kept the return's blur"
    );
}

#[test]
fn rewinding_a_completed_hook_leaves_no_return_trail_in_the_paused_view() {
    let file = record_throws(false, 1, MID);
    let mut client = replay_client(&file, 0, MID);
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect("hero.pudge.hook")
        .expect("the hook")
        .clone();
    let endings = |client: &Client| {
        client
            .world()
            .resource::<crate::particles::Playing>()
            .endings_this_frame(&effect)
    };
    client.within(SOON, "a visible launch", |client| {
        client
            .world()
            .resource::<Frames>()
            .0
            .last()
            .is_some_and(|frame| frame.head().is_some())
    });
    let launch = client.view().clone();
    client.within(
        SOON,
        "a completed hook draws its returning trail",
        |client| endings(client) > 0,
    );

    replace_replay(&mut client, &file, vec![launch.clone()], true);
    client.within(SOON, "the paused launch and its vision", |client| {
        client
            .core()
            .view()
            .is_some_and(|view| view.tick == launch.tick)
            && client
                .world()
                .resource::<crate::fog::FogGrid>()
                .worked_out_at()
                == Some(launch.tick)
            && client
                .world()
                .resource::<Frames>()
                .0
                .last()
                .is_some_and(|frame| frame.tick == launch.tick && frame.head().is_some())
    });
    for frame in 0..4 {
        assert_eq!(
            endings(&client),
            0,
            "the paused launch still draws the completed return on frame {frame}"
        );
        client.frames(1);
    }
}

#[test]
fn an_opponent_who_sees_the_throw_sees_the_head_leave_the_hand_live_and_in_replay() {
    let (file, live) = record_throws_with(
        false,
        1,
        MID,
        Some(VICTIM_DISTANCE),
        AfterLaunch::WitnessLaunch,
    );
    let replay = play(&file, 1, MID);
    let errors = [("live", &live), ("replay", &replay)].map(|(stream, frames)| {
        let (launch, centre, wire) = frames
            .iter()
            .find_map(|frame| {
                let (wire, seen) = frame.wire_head?;
                let centre = frame.launch_centre?;
                (seen
                    && frame.visible_caster
                    && !frame.returning
                    && wire.distance(centre.truncate()) < frame.socket_reach
                    && (stream == "live" || wire == centre.truncate()))
                .then_some((frame, centre, wire))
            })
            .unwrap_or_else(|| panic!("{stream} missed the visible launch"));
        let head = launch.head().expect("the launch head is drawn").0;
        let hand = launch.socket.expect("the visible caster's hand socket");
        // A live render may first sample the next network snapshot. Until the projectile
        // clears the hand, its visible trajectory lies on the segment from the hand to
        // the pose at that observation, even when interpolated within the interval.
        let travel = wire - centre.truncate();
        let last = centre
            + travel.extend(0.0)
            + (hand - centre) * (1.0 - travel.length() / launch.socket_reach);
        let axis = last - hand;
        let offset = if axis.length_squared() > 0.0 {
            axis * ((head - hand).dot(axis) / axis.length_squared()).clamp(0.0, 1.0)
        } else {
            Vec3::ZERO
        };
        (stream, head.distance(hand + offset))
    });
    assert!(
        errors.iter().all(|(_, error)| *error < 0.1),
        "launch distances from the hand trajectory: {errors:?}"
    );
}

#[test]
fn an_opponents_visible_hook_advances_on_each_frame_between_snapshots_at_240_hz() {
    let (file, _) = record_throws_seen(false, 1, MID, Some(VICTIM_DISTANCE));
    let mut client = replay_client(&file, 1, MID);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 240.0),
        ));
    client.within(SOON, "the enemy hook replay finished", |c| {
        c.tick() >= file.tick()
    });
    let frames = &client.world().resource::<Frames>().0;
    let steps: Vec<_> = frames
        .windows(2)
        .filter_map(|pair| {
            let [before, now] = pair else { unreachable!() };
            let (wire, visible) = now.wire_head?;
            if !visible
                || now.returning
                || wire.x < MID.0 as f32 + 180.0
                || wire.x > MID.0 as f32 + 340.0
                || before.tick != now.tick
            {
                return None;
            }
            Some(now.head()?.0.x - before.head()?.0.x)
        })
        .collect();
    assert!(
        steps.len() >= 12,
        "several visible snapshots should each contain multiple rendered frames"
    );
    assert!(
        steps.iter().all(|step| *step > 0.0 && *step < 15.0),
        "visible outbound motion per rendered frame: {steps:?}"
    );
}

#[test]
fn the_visible_part_of_an_enemy_tether_meets_the_heads_socket_and_keeps_its_blur() {
    let (file, _) = record_throws_seen(false, 1, YARD, Some(1100));
    let frames = play_recording(&file, 1, YARD, true);
    let first = frames
        .iter()
        .find(|frame| !frame.visible_caster && frame.wire_head.is_some_and(|(_, visible)| visible))
        .expect("the first witnessed enemy head");
    let (head, _) = first.head().expect("the first visible sample is drawn");
    assert!(
        head.truncate().distance(first.wire_head.unwrap().0) < 0.1,
        "the first visible frame used a previous position in fog"
    );
    let exposed: Vec<_> = frames
        .iter()
        .filter(|frame| {
            !frame.visible_caster
                && !frame.returning
                && frame.visible_chain_length > 120.0
                && frame.head().is_some()
        })
        .collect();
    assert!(exposed.len() >= 3, "several partial flight frames");
    let edges: Vec<_> = exposed
        .iter()
        .map(|frame| {
            let [socket] = frame.head_sockets.as_slice() else {
                panic!("one flown socket")
            };
            frame
                .rope
                .iter()
                .map(|point| point.distance(*socket))
                .fold(f32::INFINITY, f32::min)
        })
        .collect();
    let blurred = exposed
        .iter()
        .filter(|frame| !frame.blur.is_empty())
        .count();
    assert!(
        edges.iter().all(|error| *error < 0.1) && blurred > 0,
        "distances from exposed rope to socket {edges:?}, frames with visible blur {blurred}"
    );
}

#[test]
fn an_allied_catch_uses_the_ally_attachment_during_the_pull() {
    let (file, _) = record_throws_with(true, 1, MID, None, AfterLaunch::CatchAlly);
    let frames = play(&file, 0, MID);
    let bound: Vec<_> = frames
        .iter()
        .filter_map(|frame| Some((frame.head()?.0, frame.target_socket?)))
        .collect();
    assert!(
        bound.len() > 3,
        "the ally is carried for several rendered frames"
    );
    assert!(bound.first().unwrap().0.x - bound.last().unwrap().0.x > 100.0);
    assert!(
        bound
            .iter()
            .all(|(head, socket)| head.distance(*socket) < 0.1),
        "the visible head follows the ally's animated hit socket"
    );
}

#[test]
fn a_victim_dying_during_the_pull_releases_the_attachment_and_the_head_returns() {
    let (file, _) = record_throws_with(true, 1, MID, None, AfterLaunch::KillCaught);
    let frames = play(&file, 0, MID);
    let latched = frames
        .iter()
        .position(|frame| frame.target_socket.is_some())
        .expect("the victim is caught");
    let released: Vec<_> = frames[latched..]
        .iter()
        .filter(|frame| frame.returning && frame.target_socket.is_none())
        .filter_map(|frame| frame.head().map(|head| head.0))
        .collect();
    assert!(
        released.len() >= 2,
        "the head keeps returning after the victim dies"
    );
    assert!(released.first().unwrap().x - released.last().unwrap().x > 60.0);
}

#[test]
fn the_throwers_death_takes_the_rope_and_head_out_of_the_next_frame() {
    let (file, _) = record_throws_with(false, 1, MID, None, AfterLaunch::KillCaster);
    let frames = play(&file, 0, MID);
    let flying = frames
        .iter()
        .position(|frame| frame.pudge.is_some() && frame.chain_drawn && frame.head().is_some())
        .expect("a visible outbound cast");
    let dead: Vec<_> = frames[flying..]
        .iter()
        .filter(|frame| frame.pudge.is_none())
        .collect();
    assert!(
        !dead.is_empty(),
        "the recording shows the caster's death during the throw"
    );
    assert!(
        dead.iter()
            .all(|frame| frame.head().is_none() && !frame.chain_drawn),
        "a displayed head or rope survived the caster's death"
    );
}

#[test]
fn switching_the_replay_pov_mid_flight_hides_the_previous_sides_hook_immediately() {
    let file = record_throws(false, 1, YARD);
    let mut client = replay_client(&file, 0, YARD);
    client.within(SOON, "the caster sees a moving hook", |client| {
        client
            .world()
            .resource::<Frames>()
            .0
            .last()
            .is_some_and(|frame| frame.head().is_some() && frame.chain_drawn)
    });
    if let Source::Replay(player) = &mut client.core_mut().source {
        player.paused = true;
    }
    let at = client.tick();
    client.core_mut().act(bota_client_core::Intent::NextEyes);
    client.step_frames(1);
    assert_eq!(client.core().my_slot, Some(bota_proto::SlotId(1)));
    assert_eq!(client.tick(), at);
    let frames = client.world().resource::<Frames>();
    let first_enemy_frame = frames.0.last().unwrap();
    assert!(
        first_enemy_frame.head().is_none() && first_enemy_frame.rope.is_empty(),
        "the same tick's hidden head and chain leaked into the new view"
    );
}

#[test]
fn a_caught_unit_becoming_invisible_does_not_leave_the_head_attached_to_hidden_geometry() {
    let file = record_throws(true, 1, MID);
    let mut client = replay_client(&file, 0, MID);
    let mut views = Vec::new();
    client.within(SOON, "the caught victim is in several snapshots", |c| {
        let view = c.view();
        let caught = view
            .tethers
            .iter()
            .any(|t| t.viewer == Team::Radiant && t.head.is_some_and(|h| h.caught.is_some()));
        if caught
            && views
                .last()
                .is_none_or(|old: &bota_proto::WorldView| old.tick < view.tick)
        {
            views.push(view.clone());
        }
        views.len() == 6
    });
    for view in &mut views[2..] {
        let id = view
            .tethers
            .iter_mut()
            .find(|t| t.viewer == Team::Radiant)
            .unwrap()
            .head
            .as_mut()
            .unwrap()
            .caught
            .take()
            .unwrap();
        let obscured = view
            .units
            .iter_mut()
            .find(|unit| unit.id == id)
            .expect("unit is recorded");
        obscured.seen_by.as_mut().unwrap().radiant = false;
        obscured.pos.y += bota_proto::Fixed::from_int(1200);
    }
    let hidden_tick = views[2].tick;
    let last_tick = views.last().unwrap().tick;
    let before = client.world().resource::<Frames>().0.len();
    replace_replay(&mut client, &file, views, false);
    client.within(SOON, "the invisible-victim replay played", |c| {
        c.tick() >= last_tick
    });
    let frames = &client.world().resource::<Frames>().0[before..];
    assert!(
        frames
            .iter()
            .any(|frame| frame.tick < hidden_tick && frame.target_socket.is_some())
    );
    let hidden: Vec<_> = frames
        .iter()
        .filter(|frame| frame.tick >= hidden_tick)
        .filter_map(|frame| frame.head())
        .collect();
    assert!(
        hidden.len() >= 2,
        "the visible head has measurements during the hidden interval"
    );
    assert!(
        hidden
            .iter()
            .all(|(head, _)| (head.y - MID.1 as f32).abs() < 1.0),
        "the head must follow its flight line when its caught unit disappears"
    );
}

#[test]
fn a_rescaled_live_rig_drives_the_flying_weapon_scale() {
    let file = record_throws(true, 1, MID);
    let mut client = replay_client(&file, 0, MID);
    client.within(SOON, "the visible worn hook before the cast", |c| {
        c.world()
            .resource::<Frames>()
            .0
            .last()
            .is_some_and(|f| f.worn_visible && !f.hook_told)
    });
    let original = client
        .world()
        .resource::<Frames>()
        .0
        .last()
        .unwrap()
        .hero_scale
        .unwrap();
    let world = client.world_mut();
    let (wearable, body) = world
        .query::<(Entity, &crate::units::Wearable)>()
        .iter(world)
        .find_map(|(entity, worn)| {
            worn.world_model()
                .filter(|(_, model)| model.ends_with("pudge_weapon.glb"))
                .map(|_| (entity, worn.wearer()))
        })
        .expect("the visible rig and mounted blade");
    let multiplier = 1.35;
    for root in [wearable, body] {
        world.get_mut::<Transform>(root).unwrap().scale *= multiplier;
    }
    client.within(SOON, "the rescaled hero finishes his throw", |c| {
        c.tick() >= file.tick()
    });
    let frames = &client.world().resource::<Frames>().0;
    let flight = flights(frames)[0];
    assert!(
        flight
            .iter()
            .all(|f| (f.hero_scale.unwrap() - original * multiplier).abs() < 0.001),
        "the flight is rendered with the rescaled rig"
    );
    assert_flying_weapon_scale(frames);
}
