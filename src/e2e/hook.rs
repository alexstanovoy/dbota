//! Pudge's Meat Hook as a player sees it: the hook's head leaves his hand along the cast line at
//! the ability's speed, turning as it flies, with the chain stretched from his hand to it; a
//! hook that catches something brings it back with the chain shortening, one that catches
//! nothing comes back empty from its full reach; nothing of it stays after it is home, and a
//! second throw is drawn as the first was. Each test records a real match with the hook thrown
//! and plays the record through the real client, whose every frame is sampled at a fixed step.

use std::time::Duration;

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
/// The rest position of `attach_chain` in righthook.vmdl, in the exported model's metres.
const HEAD_SOCKET: Vec3 = Vec3::new(-0.012_288_09, 0.005_571_05, -0.553_935_05);

/// One replayed frame: the sim's tick and what is drawn of the hook then, in the particles'
/// frame (Z up, Dota units).
#[derive(Clone, Debug)]
struct Frame {
    seconds: f32,
    /// Control point 0 of the hook's particle: the hand the chain starts at.
    hand: Option<Vec3>,
    /// The animated chain socket and the hero's scene scale in Source units.
    socket: Option<Vec3>,
    hero_scale: Option<f32>,
    /// The particle that chases the hook and ends the chain.
    parent: Vec<Vec3>,
    /// The chain's particles, in no order.
    chain: Vec<Vec3>,
    chain_drawn: bool,
    /// Each model drawn this frame: where it is and how it is turned.
    models: Vec<Transform>,
    /// Whether the server's view tells of a hook in flight.
    hook_told: bool,
    /// How many frames of the hook's particle have been drawn so far (cards, lights, models).
    drawn: u32,
    /// Pudge's place in the sim's frame, if the view shows him.
    pudge: Option<Vec2>,
    /// The enemy heroes the view shows, in the sim's frame.
    enemies: Vec<Vec2>,
}

impl Frame {
    /// The head: the model drawn at the end of the chain.
    fn head(&self) -> Option<(Vec3, Quat)> {
        self.models
            .first()
            .map(|t| (source(t.transform_point(HEAD_SOCKET)), t.rotation))
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
    time: Res<Time>,
    mut frames: ResMut<Frames>,
) {
    let (Some(view), Some(library)) = (session.0.view(), library) else {
        return;
    };
    let effect = library.effect("hero.pudge.hook").expect("the hook effect");
    let chain_looks: Vec<_> = effect
        .defs
        .iter()
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
    let (mut hand, mut parent, mut chain) = (None, Vec::new(), Vec::new());
    for instance in playing.instances_of(effect) {
        hand = Some(instance.nodes[0].control_points[0].position);
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
    frames.0.push(Frame {
        seconds: time.elapsed_secs(),
        hand,
        socket,
        hero_scale,
        parent,
        chain,
        chain_drawn: frame.batches.iter().any(|batch| {
            chain_looks.contains(&batch.look)
                && frame.quads[batch.range.start as usize..batch.range.end as usize]
                    .iter()
                    .any(|quad| quad.head_color[3] > 0.0 || quad.tail_color[3] > 0.0)
        }),
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
        pudge: hero.map(at),
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
            enemy: true,
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
    for _ in 0..throws {
        game.cheat(Cheat::Refresh);
        game.order(Order::Cast {
            slot,
            target: Target::Pos(at + Spot::from_ints(AIM_DISTANCE, 0)),
        });
        game.wait("the hook has flown and come back", |driver, _| {
            driver.view().unwrap().unit(hero).unwrap().abilities[slot.0 as usize].cooldown_left > 0
                && driver.view().unwrap().projectiles.is_empty()
        });
    }
    let file = game.save();
    // A person looking at the renders keeps the record: HOOK_EVIDENCE names a folder for it.
    if let Some(dir) = std::env::var_os("HOOK_EVIDENCE") {
        let name = format!("hook-{}{throws}.brp", if victim { "hit" } else { "miss" });
        std::fs::copy(file.path(), std::path::Path::new(&dir).join(name)).expect("the record kept");
    }
    file
}

/// Plays a record from the side `pov` sees it, a `FRAME` at a time, and returns every frame.
fn play(file: &ReplayFile, pov: u8, at: (i32, i32)) -> Vec<Frame> {
    let pov = pov.to_string();
    let mut client = Client::run(&[
        "--replay",
        file.path().to_str().expect("a UTF-8 path"),
        "--pov",
        &pov,
        "--look",
        &format!("{},{}", at.0 + AIM_DISTANCE / 2, at.1),
    ]);
    client
        .app
        .init_resource::<Frames>()
        .add_systems(PostUpdate, record.after(crate::particles::gather_frame));
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(FRAME));
    client.within(SOON, "the particle library ready", |c| {
        c.world()
            .get_resource::<crate::particles::Library>()
            .is_some()
    });
    client.within(SOON, "the recorded match played", |c| {
        c.core().view().is_some_and(|view| view.tick >= file.tick())
    });
    if let Source::Replay(player) = &mut client.core_mut().source {
        player.paused = true;
    }
    client.step_frames(2);
    client.world().resource::<Frames>().0.clone()
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
    let frames = play(&file, 0, MID);
    let flight = flights(&frames)[0];
    for frame in flight {
        let hero = frame.hero_scale.expect("the drawn hero scale");
        let [head] = frame.models.as_slice() else {
            panic!("one hook head must be drawn at {:.2} s", frame.seconds);
        };
        // The installed head initializes its radius from the CP7 object, then multiplies it
        // by its current radius remapped from 0..10 to 0..15.
        let expected = 1.5 * hero * hero;
        let actual = head.scale.abs() / crate::map::UNITS_PER_METRE;
        assert!(
            (actual - Vec3::splat(expected)).abs().max_element() < 0.002,
            "hook head scale {actual:?} disagrees with authored scale {expected:.4} for hero scale {hero:.4}"
        );
    }
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
        let [head] = frame.models.as_slice() else {
            panic!("one hook head must be drawn at {:.2} s", frame.seconds);
        };
        let socket = source(head.transform_point(HEAD_SOCKET));
        assert!(
            frame
                .chain
                .iter()
                .any(|point| point.distance(socket) < 0.05),
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
    // Along the cast line, which runs east from where Pudge stood.
    for f in out {
        let head = f.head().expect("a head").0;
        assert!(
            (head.y - home.y).abs() < 30.0 && head.x >= home.x - 30.0,
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
