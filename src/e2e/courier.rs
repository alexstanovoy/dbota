//! A courier's owner reaching level four changes the model drawn in the match.

mod return_locked;

use bevy::{ecs::system::SystemState, prelude::*};
use bota_client_core::{Intent, LiveLink, ReplayPlayer, Source};
use bota_proto::{
    Cheat, EffectId, EntityId, HeroId, MatchInfo, ReplayHeader, ReplayRecord, ServerMsg, Team,
    WorldView,
};
use std::{cell::RefCell, io::Cursor, rc::Rc};

use super::harness::{Client, Server, quiet_seat};

fn drawn_model(client: &Client, id: EntityId) -> Option<&str> {
    let body = client.body(id)?;
    let unit = client.world().get::<crate::units::Unit>(body)?;
    let bank = client.world().resource::<crate::units::ModelBank>();
    Some(bank.0[unit.model?].model.logical.as_str())
}

fn drawn_height(client: &Client, id: EntityId) -> f32 {
    let body = client.body(id).expect("courier drawn");
    let place = client
        .world()
        .get::<bevy::prelude::Transform>(body)
        .expect("placed")
        .translation;
    let ground = client
        .world()
        .resource::<crate::map::Ground>()
        .at(place.x, -place.z);
    place.y - ground
}

#[test]
fn courier_draws_wings_when_its_owner_reaches_level_four() {
    // Dota 7.27 reduced the flight unlock from owner level five to four;
    // it remains level four in 7.41f, not a timer-based upgrade.
    for team in [Team::Radiant, Team::Dire] {
        check_courier_flight_for_team(team);
    }
}

fn check_courier_flight_for_team(team: Team) {
    let side = if team == Team::Radiant {
        "radiant"
    } else {
        "dire"
    };
    let server = Server::start(
        if team == Team::Radiant { 1 } else { 2 },
        &["--tick-rate", "600"],
    );
    if team == Team::Dire {
        quiet_seat(&server, HeroId::SHADOW_FIEND);
    }
    let (mut client, start) = join_recording_start(&server);
    assert_eq!(client.me().team, team);
    let courier = client.core().my_courier().expect("the owner's courier");
    let ground_model = format!("courier.{side}.model");
    let flying_model = format!("courier.{side}.flying.model");
    let walking = check_level_three_walker(&mut client, courier, &ground_model);
    client.cheat(Cheat::Levels { count: 1 });
    client.until("the server's level-four flight effect", |c| {
        c.view()
            .unit(courier)
            .is_some_and(|u| u.level == 4 && u.effects.iter().any(|e| e.id == EffectId::FLYING))
    });
    assert_courier_placement(&mut client, courier, &flying_model, 160.0);
    assert_missile_and_effect_heights(&mut client, courier);
    check_death_and_respawn(&mut client, courier, &flying_model);
    server.frozen(|| {
        rewind_to_walking_snapshot(&mut client, start, walking, courier);
        assert_courier_placement(&mut client, courier, &ground_model, 0.0);
    });
}

fn check_level_three_walker(client: &mut Client, courier: EntityId, model: &str) -> WorldView {
    client.cheat(Cheat::Levels { count: 2 });
    client.until("level three and the ground courier drawn", |c| {
        c.me().level == 3 && drawn_model(c, courier).is_some()
    });
    assert_courier_placement(client, courier, model, 0.0);
    client.view().clone()
}

fn assert_courier_placement(client: &mut Client, courier: EntityId, model: &str, height: f32) {
    client.within(
        std::time::Duration::from_secs(30),
        "the expected courier model drawn",
        |c| drawn_model(c, courier) == Some(model),
    );
    assert_eq!(drawn_model(client, courier), Some(model));
    let actual = drawn_height(client, courier);
    assert!(
        (actual - height).abs() < 0.01,
        "courier at {actual}, expected {height}"
    );
    assert_one_courier_body(client, courier);
    assert_pick_height(client, courier, height);
    client.within(
        std::time::Duration::from_secs(30),
        "the bar above the courier",
        |c| bar_matches_height(c, courier, height),
    );
}

struct RecordStart {
    inner: Box<dyn LiveLink>,
    start: Rc<RefCell<Option<MatchInfo>>>,
}

impl LiveLink for RecordStart {
    fn poll(&mut self) -> Vec<ServerMsg> {
        let messages = self.inner.poll();
        for message in &messages {
            if let ServerMsg::MatchStart { info } = message {
                *self.start.borrow_mut() = Some(info.clone());
            }
        }
        messages
    }
    fn send(&mut self, message: &bota_proto::ClientMsg) -> std::io::Result<()> {
        self.inner.send(message)
    }
    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

fn join_recording_start(server: &Server) -> (Client, MatchInfo) {
    let mut client = Client::run(&["--addr", &server.address]);
    let start = Rc::new(RefCell::new(None));
    let empty = Source::Replay(Box::new(ReplayPlayer::from_reader(Cursor::new(
        Vec::<u8>::new(),
    ))));
    let Source::Live(inner) = std::mem::replace(&mut client.core_mut().source, empty) else {
        panic!("live match")
    };
    client.core_mut().source = Source::Live(Box::new(RecordStart {
        inner,
        start: start.clone(),
    }));
    client.until("the hero picked and on the map", |c| {
        let core = c.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(Intent::ToggleReady);
        }
        core.ears().is_some()
    });
    let info = start
        .borrow_mut()
        .take()
        .expect("the real server's match start");
    (client, info)
}

fn check_death_and_respawn(client: &mut Client, courier: EntityId, flying_model: &str) {
    let enemy = if client.me().team == Team::Radiant {
        Team::Dire
    } else {
        Team::Radiant
    };
    let fountain = bota_core::fountain_pos(bota_core::map_of(bota_core::MAP2_ID), enemy);
    client.core_mut().act(Intent::SelectCourier);
    client.core_mut().pinned = Some(bota_client_core::Pin::Unit(courier));
    client.cheat(Cheat::Teleport { at: fountain });
    client.until("the courier killed at the enemy fountain", |c| {
        c.view().unit(courier).is_none()
    });
    client.until("the fallen courier's body removed", |c| {
        c.body(courier).is_none()
    });
    let world = client.world_mut();
    assert_eq!(
        world
            .query::<&crate::units::Unit>()
            .iter(world)
            .filter(|u| u.id == courier)
            .count(),
        0
    );
    // The respawn timer is simulator time. Pump the real connection without
    // rendering thousands of empty frames while this courier is dead.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
    while client.view().unit(courier).is_none() {
        assert!(std::time::Instant::now() < deadline, "courier respawns");
        client.core_mut().pump(0.0);
        std::thread::yield_now();
    }
    client.until("the same courier risen with wings", |c| {
        c.view().unit(courier).is_some_and(|u| u.level == 4)
            && drawn_model(c, courier) == Some(flying_model)
    });
    assert_eq!(client.core().my_courier(), Some(courier));
    assert_courier_placement(client, courier, flying_model, 160.0);
}

/// Reopen real server records at the earlier snapshot, while keeping the rendered scene.
/// This ends flight on the same courier id, as rewinding a replay does.
fn rewind_to_walking_snapshot(
    client: &mut Client,
    start: MatchInfo,
    view: WorldView,
    courier: EntityId,
) {
    let mut bytes = Vec::new();
    for record in [
        ReplayRecord::Header(ReplayHeader::new(bota_core::WIRE_FINGERPRINT)),
        ReplayRecord::Msg(ServerMsg::MatchStart { info: start }),
        ReplayRecord::Msg(ServerMsg::Snapshot { view }),
    ] {
        bota_proto::encode_frame(&record, &mut bytes).expect("record the server's messages");
    }
    let mut player = ReplayPlayer::from_reader(Cursor::new(bytes));
    player.paused = true;
    let slot = client.core().my_slot;
    *client.core_mut() = bota_client_core::App::new(Source::Replay(Box::new(player)));
    client.core_mut().my_slot = slot;
    client.core_mut().pinned = Some(bota_client_core::Pin::Unit(courier));
}

fn assert_missile_and_effect_heights(client: &mut Client, courier: EntityId) {
    let body = client.body(courier).expect("drawn");
    client.until("the courier's imported hit attachment", |c| {
        c.world()
            .get::<crate::units::Attachments>(body)
            .is_some_and(|a| a.hitloc.is_some())
    });
    // The imported GLBs have attach_hitloc; exercise the fallback too, before the scene is ready.
    let attachments = client
        .world_mut()
        .entity_mut(body)
        .take::<crate::units::Attachments>()
        .expect("attachments");
    let view = client.view().unit(courier).expect("visible").clone();
    let mut state: SystemState<(crate::effects::Shooters, Res<crate::map::Ground>)> =
        SystemState::new(client.world_mut());
    let (bodies, ground) = state.get(client.world()).expect("effect inputs");
    let hit = bodies.hit(courier).expect("missile fallback");
    assert!(
        (hit.y - ground.at(hit.x, -hit.z) - 160.0).abs() < 0.01,
        "fallback aims at the flyer, without another 110 units"
    );
    for (attach, above) in [
        (crate::fx::line::Attach::Origin, 0.0),
        (crate::fx::line::Attach::Overhead, 165.0),
    ] {
        let at = crate::fx::attach_point(&ground, &bodies, &view, &attach);
        assert!((at.y - ground.at(at.x, -at.z) - 160.0 - above).abs() < 0.01);
    }
    client.world_mut().entity_mut(body).insert(attachments);
}

fn assert_one_courier_body(client: &mut Client, id: EntityId) {
    let world = client.world_mut();
    let count = world
        .query::<&crate::units::Unit>()
        .iter(world)
        .filter(|u| u.id == id)
        .count();
    assert_eq!(count, 1, "one drawn entity for courier {id:?}");
}

fn assert_pick_height(client: &mut Client, id: EntityId, height: f32) {
    let view = client.view().clone();
    let team = client.core().my_team();
    let mut state: SystemState<(
        Res<crate::units::UnitIndex>,
        Query<&crate::units::Unit>,
        Res<crate::map::Ground>,
    )> = SystemState::new(client.world_mut());
    let (index, units, ground) = state.get(client.world()).expect("pick inputs");
    let (candidate, _) = crate::pick::stands(&view, &index, &units, &ground, team)
        .find(|(candidate, _)| candidate.id == id)
        .expect("a pick candidate");
    let actual = candidate.base.y - ground.at(candidate.base.x, -candidate.base.z);
    assert!(
        (actual - height).abs() < 0.01,
        "courier click hitbox at {actual}, expected {height}"
    );
}

fn bar_matches_height(client: &mut Client, id: EntityId, height: f32) -> bool {
    let Some(body) = client.body(id) else {
        return false;
    };
    let world = client.world_mut();
    let unit = world.get::<crate::units::Unit>(body).expect("drawn");
    let at = unit.at;
    let y = world.resource::<crate::map::Ground>().at(at.x, at.y)
        + height
        + crate::bars::bar_height(unit.kind);
    let Some(bar) = world.resource::<crate::bars::Bars>().root(body) else {
        return false;
    };
    let node = world.get::<Node>(bar).expect("bar node");
    let (Val::Px(left), Val::Px(top)) = (node.left, node.top) else {
        return false;
    };
    let scale = world.resource::<UiScale>().0;
    let (camera, transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<crate::camera::RtsCamera>>()
        .single(world)
        .expect("camera");
    let Ok(screen) = camera.world_to_viewport(transform, Vec3::new(at.x, y, -at.y)) else {
        return false;
    };
    let expected = screen / scale - Vec2::new(25.0, 6.0) * crate::ui::DOTA;
    Vec2::new(left, top).distance(expected) < 0.5
}
