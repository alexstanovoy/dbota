//! Watcher interactions and the fog drawn from a real match.

use bota_client_core::{Intent, IssuedOrder};
use bota_proto::{Cheat, EffectId, HeroId, Order, Target, UnitKind, Vec2};

use super::harness::{Client, Server, quiet_seat, scripted_seat};
use crate::fog::FogGrid;

#[test]
fn a_captured_watcher_lights_the_ground_after_its_captor_leaves() {
    captured_watcher_lights(1);
}

#[test]
fn a_dire_watcher_lights_the_ground_after_its_captor_leaves() {
    captured_watcher_lights(2);
}

fn captured_watcher_lights(seats: u8) {
    let server = Server::start(seats, &["--map", "0", "--objectives", "watchers"]);
    if seats == 2 {
        quiet_seat(&server, HeroId::PUDGE);
    }
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let at = bota_core::DOTA_WATCHERS[4];
    client.stand_at((at.x.to_int() + 100, at.y.to_int()));
    let watcher = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        .expect("the watcher")
        .id;
    client.core_mut().act(Intent::Smart {
        at: (at.x.to_int() as f32, at.y.to_int() as f32),
        unit: Some(watcher),
    });
    client.until("the watcher captured", |c| {
        c.view()
            .unit(watcher)
            .is_some_and(|u| u.effects.iter().any(|e| e.id == EffectId::VISION_GRANT))
    });
    client.stand_at((4000, 4000));
    let tick = client.tick();
    client.until("fog calculated after the captor leaves", |c| {
        c.world()
            .resource::<FogGrid>()
            .worked_out_at()
            .is_some_and(|t| t >= tick)
    });
    let fog = client.world().resource::<FogGrid>();
    assert!(fog.on, "the player's fog is enabled");
    assert!(
        fog.lit(at.x.to_int() as f32, at.y.to_int() as f32),
        "the captured watcher must light its owner's ground without a hero nearby"
    );
}

fn radiant_capture(server: &Server, at: Vec2) {
    let mut stage = 0;
    scripted_seat(server, HeroId::PUDGE, move |session| {
        let Some(view) = session.current() else {
            return Vec::new();
        };
        let Some(watcher) = view
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        else {
            return Vec::new();
        };
        let order = match stage {
            0 => Order::Cheat {
                cheat: Cheat::Teleport {
                    at: at + Vec2::from_ints(-100, 0),
                },
            },
            1 if view.game_tick() >= session.statics().metadata().pregame_ticks => Order::Attack {
                target: Target::Unit(watcher.id),
            },
            _ => return Vec::new(),
        };
        stage += 1;
        vec![IssuedOrder::hero(order)]
    });
}

#[test]
fn an_enemy_watcher_keeps_the_ground_fogged_and_its_grant_private() {
    let server = Server::start(2, &["--map", "0", "--objectives", "watchers"]);
    let at = bota_core::DOTA_WATCHERS[4];
    radiant_capture(&server, at);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.until("the enemy watcher lit", |c| {
        c.view()
            .units
            .iter()
            .any(|u| u.pos == at && u.effects.iter().any(|e| e.id == EffectId::VISION_GRANT))
    });
    client.stand_at((4000, 4000));
    let tick = client.tick();
    client.until("fog calculated after enemy capture", |c| {
        c.world()
            .resource::<FogGrid>()
            .worked_out_at()
            .is_some_and(|t| t >= tick)
    });
    let fog = client.world().resource::<FogGrid>();
    assert!(fog.on, "the enemy seat has fog");
    assert!(
        !fog.lit(at.x.to_f32(), at.y.to_f32()),
        "the enemy's watcher gives no sight"
    );
    let watcher = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        .expect("the watcher");
    assert_eq!(
        watcher.vision_grant, None,
        "an enemy seat is not told the grant"
    );
}
