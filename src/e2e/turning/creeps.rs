//! A real lane creep's windup waits for its rendered facing, then releases on time.

use super::*;
use crate::units::{ModelBank, Rig, UnitIndex};

#[derive(Resource)]
struct WindupFacingLag {
    id: EntityId,
    facing: f32,
    began: bool,
    left: u8,
}

fn delay_first_windup(
    session: NonSend<crate::session::Session>,
    mut lag: ResMut<WindupFacingLag>,
    index: Res<UnitIndex>,
    mut units: Query<&mut crate::units::Unit>,
) {
    let winding = session.0.view().is_some_and(|view| {
        view.aggro
            .iter()
            .any(|aggro| aggro.unit == lag.id && aggro.swing.is_some())
    });
    lag.began |= winding;
    if (!lag.began || lag.left > 0)
        && let Some(body) = index.0.get(&lag.id)
        && let Ok(mut unit) = units.get_mut(*body)
    {
        unit.facing = lag.facing;
        if lag.began {
            lag.left = lag.left.saturating_sub(1);
        }
    }
}

fn scene(kind: UnitKind) -> (Server, Client, EntityId) {
    let server = Server::start(1, &[]);
    let mut client = player_at_mid(&server, HeroId::SHADOW_FIEND);
    client.cheat(Cheat::AllVision { on: true });
    client.cheat(Cheat::ApplyModifier {
        target: bota_proto::Target::None,
        spec: bota_proto::ModifierSpec {
            max_hp: bota_proto::ModifierSpec::MAX_SCALE,
            ..bota_proto::ModifierSpec::NOMINAL
        },
        ticks: 10000,
    });
    client.cheat(Cheat::Refresh);
    client.cheat(Cheat::CreateCreeps {
        enemy: true,
        at: client.me().pos + Spot::from_ints(200, 0),
    });
    let creep = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == kind && u.team != client.me().team)
        .expect("the requested lane creep")
        .id;
    server.frozen(|| {
        client.until("the creep's animated model is ready", |c| {
            c.body(creep)
                .is_some_and(|body| c.world().get::<Rig>(body).is_some())
        });
    });
    let unit = client.view().unit(creep).expect("creep");
    let wanted = bota_core::facing_towards(unit.pos, client.me().pos);
    let facing = radians(i32::from(wanted.brads)) + std::f32::consts::PI;
    client.world_mut().insert_resource(WindupFacingLag {
        id: creep,
        facing,
        began: false,
        left: 3,
    });
    client
        .app
        .add_systems(Update, delay_first_windup.after(crate::units::Placed));
    client
        .core_mut()
        .act(Intent::Cheat(Cheat::Invulnerable { on: false }));
    (server, client, creep)
}

fn attack_clip(client: &Client, id: EntityId) -> Option<(f32, f32, f32)> {
    let body = client.body(id)?;
    let pose = client.world().get::<crate::anim::Pose>(body)?;
    if !pose.clip().starts_with("attack") {
        return None;
    }
    let unit = client.world().get::<crate::units::Unit>(body)?;
    let clip = client.world().resource::<ModelBank>().0[unit.model?]
        .clips
        .iter()
        .find(|clip| clip.0 == pose.clip())?;
    let rig = client.world().get::<Rig>(body)?.0;
    let active = client
        .world()
        .get::<AnimationPlayer>(rig)?
        .animation(clip.1)?;
    Some((active.seek_time(), clip.3?, active.weight()))
}

fn note_turn(client: &Client, creep: EntityId, waited: &mut bool, due: &mut Option<u32>) {
    let swing = client
        .view()
        .aggro
        .iter()
        .find(|a| a.unit == creep)
        .and_then(|a| a.swing);
    let Some(swing) = swing else {
        return;
    };
    due.get_or_insert(client.tick() + swing.strike_in);
    let body = client.body(creep).expect("drawn creep");
    let unit = client
        .world()
        .get::<crate::units::Unit>(body)
        .expect("creep placement");
    let wanted = bota_core::facing_towards(
        client.view().unit(creep).expect("creep").pos,
        client.me().pos,
    );
    let gap = delta(unit.facing, radians(i32::from(wanted.brads))).abs();
    let remaining = (swing.strike_in as f32 - client.core().smooth.alpha()) / 30.0;
    if gap > radians(i32::from(bota_core::rules::ATTACK_ANGLE_BRADS)) && remaining > 1.0 / 30.0 {
        *waited = true;
        assert!(
            attack_clip(client, creep).is_none(),
            "creep windup must await its drawn facing"
        );
    }
}

fn released(client: &Client, creep: EntityId) -> bool {
    client
        .view()
        .projectiles
        .iter()
        .any(|p| p.source == Some(creep))
        || client.core().told.iter().any(|event| {
            matches!(event,
            bota_proto::EventKind::Damaged { source: Some(source), target, .. }
            if *source == creep && *target == client.me().id)
        })
}

fn assert_release(client: &Client, creep: EntityId, waited: bool, due: Option<u32>) {
    let unit = client.view().unit(creep).expect("creep");
    let scenario = bota_core::kind_def(unit.kind).name;
    assert!(
        waited,
        "{scenario}: presentation lag must exercise the gate"
    );
    assert_eq!(
        Some(client.tick()),
        due,
        "{scenario}: release stays on the server tick"
    );
    let (time, point, weight) = attack_clip(client, creep).expect("the release is actually drawn");
    assert!(weight > 0.5, "{scenario}: attack clip has visible weight");
    let windup = unit.attack_point as f32 / 1000.0;
    let allowance = FRAME.as_secs_f32() * point / windup + 0.002;
    assert!(
        (time - point).abs() <= allowance,
        "{scenario}: clip point coincides with release"
    );
}

fn check(kind: UnitKind) {
    let (_server, mut client, creep) = scene(kind);
    let (mut waited, mut due) = (false, None);
    for _ in 0..120 {
        client.frames(1);
        note_turn(&client, creep, &mut waited, &mut due);
        if released(&client, creep) {
            assert_release(&client, creep, waited, due);
            return;
        }
    }
    panic!(
        "{}: creep never released its attack",
        bota_core::kind_def(kind).name
    );
}

#[test]
fn melee_creep_waits_for_drawn_facing_and_keeps_its_strike_tick() {
    check(UnitKind::CreepMelee);
}

#[test]
fn ranged_creep_waits_for_drawn_facing_and_keeps_its_launch_tick() {
    check(UnitKind::CreepRanged);
}
