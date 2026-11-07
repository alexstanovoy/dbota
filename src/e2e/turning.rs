//! A unit turns at its hero's turn rate on screen, whatever the clicks: a new order mid-turn
//! neither snaps the drawn model round nor stops it turning.

use bevy::prelude::*;
use bota_client_core::{CueKind, FloaterKind, Intent};
use bota_proto::{Cheat, EntityId, HeroId, UnitKind, UnitView, Vec2 as Spot};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::harness::{Client, MID, Server};

mod creeps;
mod review_edges;

/// Radians of a sim angle in brads.
fn radians(brads: i32) -> f32 {
    brads as f32 / 65536.0 * std::f32::consts::TAU
}

/// The yaw the model's pivot is drawn at, as the facing it shows (radians counter-clockwise
/// from east).
fn drawn_facing(client: &mut Client) -> f32 {
    let id = client.me().id;
    let body = client.body(id).expect("drawn");
    let pivot = client
        .world()
        .get::<crate::units::ModelOf>(body)
        .expect("a model")
        .0;
    let rotation = client
        .world()
        .get::<Transform>(pivot)
        .expect("posed")
        .rotation;
    rotation.to_euler(EulerRot::YXZ).0 + std::f32::consts::FRAC_PI_2
}

/// The shortest signed turn from one angle to another.
fn delta(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

const FRAME: Duration = Duration::from_nanos(33_333_333);

fn player_at_mid(server: &Server, hero: HeroId) -> Client {
    let mut client = Client::join(server, hero, &["--set", "damage_numbers=true"]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(FRAME));
    client.core_mut().act(Intent::Stop);
    client.frames(3);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::All,
    });
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    client
        .core_mut()
        .act(Intent::MoveDirection((MID.0 as f32 + 2000.0, MID.1 as f32)));
    client.until("the hero faces east with an animated model", |c| {
        c.me().facing.brads == 0
            && c.body(c.me().id)
                .is_some_and(|body| c.world().get::<crate::units::Rig>(body).is_some())
    });
    client.frames(6);
    client.core_mut().act(Intent::Stop);
    client.frames(3);
    client
}

fn attack_scene(hero: HeroId, offset: Spot) -> (Server, Client, UnitView) {
    let server = Server::start(1, &[]);
    let mut client = player_at_mid(&server, hero);
    let at = client.me().pos;
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: true,
        at: at + offset,
    });
    let enemy = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.team != client.me().team)
        .expect("enemy")
        .clone();
    // Creating a hero updates the view before its model finishes loading.
    server.frozen(|| {
        client.until(
            "the target's animated model is ready before attacking",
            |c| {
                c.body(enemy.id)
                    .is_some_and(|body| c.world().get::<crate::units::Rig>(body).is_some())
            },
        );
    });
    (server, client, enemy)
}

#[derive(Default)]
struct Sounds {
    cues: Vec<(u32, CueKind)>,
    played: Vec<(u32, String)>,
}

fn attack_sounds(client: &mut Client, hero: HeroId) -> (Arc<Mutex<Sounds>>, Vec<String>) {
    let manifest = client.world().resource::<crate::manifest::Manifest>();
    let name = manifest.dota_name("hero", hero.0).expect("hero name");
    let events: Vec<_> = ["attack", "impact"]
        .into_iter()
        .flat_map(|kind| {
            manifest
                .sounds_with_events(&format!("sound.hero.{name}.{kind}"))
                .into_iter()
                .map(|(_, event)| event)
        })
        .collect();
    let sounds = Arc::new(Mutex::new(Sounds::default()));
    let into = sounds.clone();
    client.app.add_systems(
        PreUpdate,
        (move |session: NonSend<crate::session::Session>| {
            let core = &session.0;
            let tick = core.view().expect("view").tick;
            for cue in core.cues.iter().filter(|cue| {
                cue.hero == Some(hero) && matches!(cue.kind, CueKind::Attack | CueKind::Hit)
            }) {
                into.lock().expect("sounds").cues.push((tick, cue.kind));
            }
        })
        .after(crate::session::pump),
    );
    let into = sounds.clone();
    let names = events.clone();
    client.world_mut().add_observer(
        move |add: On<Add<bevy::audio::AudioPlayer>>,
              mixer: Res<crate::mixer::Mixer>,
              session: NonSend<crate::session::Session>| {
            if let Some((name, _)) = mixer.event_at(add.entity)
                && names.iter().any(|wanted| wanted == name)
            {
                into.lock()
                    .expect("sounds")
                    .played
                    .push((session.0.view().expect("view").tick, name.to_owned()));
            }
        },
    );
    (sounds, events)
}

fn drawn_attack(client: &Client) -> Option<(f32, f32)> {
    let body = client.body(client.me().id)?;
    let world = client.world();
    let pose = world.get::<crate::anim::Pose>(body)?;
    if !pose.clip().starts_with("attack") {
        return None;
    }
    let model = world.get::<crate::units::Unit>(body)?.model?;
    let clip = world.resource::<crate::units::ModelBank>().0[model]
        .clips
        .iter()
        .find(|clip| clip.0 == pose.clip())?;
    let rig = world.get::<crate::units::Rig>(body)?.0;
    let active = world.get::<AnimationPlayer>(rig)?.animation(clip.1)?;
    Some((active.seek_time(), clip.3.expect("an attack point")))
}

fn drawn_projectiles(client: &Client, hero: HeroId) -> u32 {
    if hero != HeroId::SHADOW_FIEND {
        return 0;
    }
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect("hero.nevermore.attack")
        .expect("the ranged attack particle");
    client
        .world()
        .resource::<crate::particles::Playing>()
        .started(effect)
}

fn trace_facing(client: &mut Client, frame: u32, enemy: EntityId) -> bool {
    let me = client.me().clone();
    let previous = client
        .core()
        .prev_view()
        .and_then(|v| v.unit(me.id))
        .map_or(me.facing, |u| u.facing);
    let alpha = client.core().smooth.alpha();
    let expected = radians(i32::from(previous.brads))
        + radians(bota_core::angle_delta(previous, me.facing)) * alpha;
    let drawn = drawn_facing(client);
    let target = client
        .body(enemy)
        .and_then(|body| client.world().get::<crate::units::Unit>(body))
        .expect("drawn target")
        .at;
    let body = client.body(me.id).expect("drawn hero");
    let unit = client
        .world()
        .get::<crate::units::Unit>(body)
        .expect("hero");
    let gap = delta(drawn, (target.y - unit.at.y).atan2(target.x - unit.at.x)).abs();
    let attack = drawn_attack(client);
    eprintln!(
        "DRAW {:?} frame={frame} tick={} sim={} prev={} alpha={alpha:.4} yaw={:.3}deg gap={:.3}deg attack={attack:?}",
        me.hero,
        client.tick(),
        me.facing.brads,
        previous.brads,
        drawn.to_degrees(),
        gap.to_degrees()
    );
    assert!(
        delta(drawn, expected).abs() < 0.002,
        "model yaw follows the interpolated sim facing, including wrapping"
    );
    if attack.is_some() {
        assert!(
            gap <= radians(i32::from(bota_core::rules::ATTACK_ANGLE_BRADS)) + 0.002,
            "hero {:?}, target bearing {:.3} degrees: the drawn attack starts {:.3} degrees off target",
            me.hero,
            (target.y - unit.at.y)
                .atan2(target.x - unit.at.x)
                .to_degrees(),
            gap.to_degrees()
        );
    }
    attack.is_some_and(|(time, point)| time + 0.001 >= point)
}

struct Strike {
    hero: HeroId,
    offset: Spot,
    enemy: EntityId,
    shots_before: u32,
    due: Option<u32>,
    reached_point: bool,
    launched: bool,
    numbered: bool,
}

impl Strike {
    fn record(&mut self, client: &Client, heard: &Sounds) -> Option<i32> {
        let id = client.me().id;
        if let Some(swing) = client
            .view()
            .aggro
            .iter()
            .find(|a| a.unit == id)
            .and_then(|a| a.swing)
        {
            self.due.get_or_insert(client.tick() + swing.strike_in);
        }
        let shot = client
            .view()
            .projectiles
            .iter()
            .any(|p| p.source == Some(id));
        let number = client
            .core()
            .floaters
            .iter()
            .find(|f| f.unit == self.enemy && f.kind == FloaterKind::Damage)
            .map(|f| f.amount);
        if shot
            || drawn_projectiles(client, self.hero) > self.shots_before
            || number.is_some()
            || !heard.cues.is_empty()
            || !heard.played.is_empty()
        {
            assert!(
                self.reached_point,
                "a projectile, damage number or hit sound ran ahead of the drawn attack point"
            );
        }
        if (shot && !self.launched)
            || (self.hero == HeroId::PUDGE && number.is_some() && !self.numbered)
        {
            self.at_point(client);
        }
        self.launched |= shot;
        self.numbered |= number.is_some();
        number.filter(|_| heard.cues.iter().any(|(_, kind)| *kind == CueKind::Hit))
    }

    fn at_point(&self, client: &Client) {
        assert_eq!(
            Some(client.tick()),
            self.due,
            "the visible strike keeps its authoritative tick"
        );
        let (time, point) = drawn_attack(client).expect("the attack is playing at its strike");
        let allowance =
            FRAME.as_secs_f32() * point / (client.me().attack_point as f32 / 1000.0) + 0.002;
        assert!(
            (time - point).abs() <= allowance,
            "the drawn clip reaches its point at the strike: {time}/{point}"
        );
        eprintln!(
            "STRIKE {:?} offset={:?} tick={} due={:?} clip={time:.6}/{point:.6}",
            self.hero,
            self.offset,
            client.tick(),
            self.due
        );
    }

    fn landed(&self, client: &Client, heard: &Sounds, events: &[String]) {
        assert_eq!(
            self.launched,
            self.hero == HeroId::SHADOW_FIEND,
            "ranged launches a projectile, melee does not"
        );
        assert_eq!(
            drawn_projectiles(client, self.hero) > self.shots_before,
            self.launched,
            "the launched projectile is actually drawn"
        );
        // A hero with no authored audio still has to produce its real Hit cue.
        // Restoring its manifest bindings automatically requires those sounds.
        assert_eq!(
            !heard.played.is_empty(),
            !events.is_empty(),
            "every authored attack sound is observed"
        );
        assert!(
            events
                .iter()
                .all(|event| heard.played.iter().any(|(_, played)| played == event)),
            "each authored launch and hit sound actually played"
        );
        eprintln!(
            "LAND {:?} offset={:?} tick={} cue_ticks={:?} audio_ticks={:?}",
            self.hero,
            self.offset,
            client.tick(),
            heard.cues,
            heard.played
        );
    }
}

fn check_attack(hero: HeroId, offset: Spot) {
    let (server, mut client, enemy) = attack_scene(hero, offset);
    let (sounds, events) = attack_sounds(&mut client, hero);
    let mut strike = Strike {
        hero,
        offset,
        enemy: enemy.id,
        shots_before: drawn_projectiles(&client, hero),
        due: None,
        reached_point: false,
        launched: false,
        numbered: false,
    };
    let (mut tick, mut facing) = (client.tick(), client.me().facing);
    let rate = i32::from(bota_core::hero_def(hero).expect("hero").unit.turn_rate);
    let mut turned = false;
    client.core_mut().act(Intent::AttackAt {
        at: (enemy.pos.x.to_f32(), enemy.pos.y.to_f32()),
        unit: Some(enemy.id),
    });
    for frame in 0..90 {
        client.frames(1);
        strike.reached_point |= trace_facing(&mut client, frame, enemy.id);
        if client.tick() > tick {
            let step = bota_core::angle_delta(facing, client.me().facing).abs();
            assert!(
                step <= rate * (client.tick() - tick) as i32,
                "snapshots respect the turn rate"
            );
            turned |= step > 0;
            (tick, facing) = (client.tick(), client.me().facing);
        }
        let heard = sounds.lock().expect("sounds");
        if let Some(number) = strike.record(&client, &heard) {
            strike.landed(&client, &heard, &events);
            drop(heard);
            assert_number_drawn(&server, &mut client, number);
            assert!(turned, "the attack required a turn");
            return;
        }
    }
    panic!("{hero:?}: the attack did not visibly land and sound its hit cue");
}

fn assert_number_drawn(server: &Server, client: &mut Client, number: i32) {
    // Visibility/layout may settle a frame after the floater is populated.
    // Hold its lifetime and the match still while requiring the rendered text;
    // Strike::record already checked that its creation did not precede the point.
    server.frozen(|| {
        client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::ZERO,
            ));
        client.until("the damage number is actually drawn", |c| {
            c.shown_texts()
                .iter()
                .any(|text| text == &number.to_string())
        });
    });
}

#[test]
fn c_turn_shadow_fiend_180_degrees_faces_before_windup_and_strikes_on_time() {
    check_attack(HeroId::SHADOW_FIEND, Spot::from_ints(-120, 0));
}

#[test]
fn c_turn_shadow_fiend_90_degrees_faces_before_windup_and_strikes_on_time() {
    check_attack(HeroId::SHADOW_FIEND, Spot::from_ints(0, -120));
}

#[test]
fn c_turn_pudge_180_degrees_faces_before_windup_and_strikes_on_time() {
    check_attack(HeroId::PUDGE, Spot::from_ints(-120, 0));
}

#[test]
fn c_turn_pudge_90_degrees_faces_before_windup_and_strikes_on_time() {
    check_attack(HeroId::PUDGE, Spot::from_ints(0, -120));
}

/// A click on alternating sides every other frame, as fast as a player can: on no frame does
/// the drawn model turn by more than its hero's turn rate allows for the ticks that passed
/// (plus the one tick the drawing trails the server by), and over the clicks it does turn
/// round, frame after frame.
#[test]
fn rapid_clicks_on_alternating_sides_turn_the_model_at_the_heros_rate() {
    for hero in [HeroId::SHADOW_FIEND, HeroId::PUDGE] {
        let server = Server::start(1, &[]);
        let mut client = Client::join(&server, hero, &[]);
        client.stand_at(MID);
        let rate = radians(i32::from(
            bota_core::hero_def(hero).expect("a hero").unit.turn_rate,
        ));
        let (mut before, mut tick) = (drawn_facing(&mut client), client.tick());
        let (mut turning_frames, mut widest) = (0, 0.0_f32);
        for frame in 0..160_u32 {
            if frame.is_multiple_of(2) {
                let side = if (frame / 2).is_multiple_of(2) {
                    2000.0
                } else {
                    -2000.0
                };
                client
                    .core_mut()
                    .act(Intent::MoveTo((MID.0 as f32 + side, MID.1 as f32)));
            }
            client.frames(1);
            let now = drawn_facing(&mut client);
            let passed = client.tick().saturating_sub(tick);
            let step = delta(before, now).abs();
            let allowed = rate * (passed + 1) as f32 + 1e-3;
            assert!(
                step <= allowed,
                "{hero:?}: frame {frame} drew a turn of {step} rad, more than {allowed} ({passed} ticks)"
            );
            turning_frames += usize::from(step > 1e-4);
            widest = widest.max(step);
            (before, tick) = (now, client.tick());
        }
        assert!(
            turning_frames > 20,
            "{hero:?}: the model turned on {turning_frames} frames only"
        );
        assert!(widest > 0.0);
    }
}
