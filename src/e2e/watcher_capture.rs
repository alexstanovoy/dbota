//! A Watcher captured and disabled through the actual client order path.

use bota_client_core::{Intent, IssuedOrder};
use bota_proto::{Cheat, EffectId, HeroId, Order, StatusFlags, Target, UnitKind, Vec2};

use super::harness::{Client, Server, scripted_seat};

fn radiant_captor(server: &Server, at: Vec2) {
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
            2 if watcher
                .effects
                .iter()
                .any(|e| e.id == EffectId::VISION_GRANT) =>
            {
                Order::Attack {
                    target: Target::None,
                }
            }
            _ => return Vec::new(),
        };
        stage += 1;
        vec![IssuedOrder::hero(order)]
    });
}

fn cancel_channel(client: &mut Client, interact: Intent) {
    client.core_mut().act(interact);
    client.until("Dire channels the enemy watcher", |c| {
        c.me().statuses.bits & StatusFlags::CHANNELLING != 0
    });
    client.core_mut().act(Intent::Stop);
    client.until("Stop interrupts the channel", |c| {
        c.me().statuses.bits & StatusFlags::CHANNELLING == 0
    });
    let tick = client.tick();
    client.until("the cancelled channel's completion time passes", |c| {
        c.tick() > tick + bota_core::rules::TICKS_PER_SECOND
    });
}

#[test]
fn an_opponent_can_cancel_and_restart_a_watcher_channel_then_disable_it() {
    let server = Server::start(2, &["--map", "0", "--objectives", "watchers"]);
    let at = bota_core::DOTA_WATCHERS[4];
    radiant_captor(&server, at);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.until("Radiant captures the watcher", |c| {
        c.view()
            .units
            .iter()
            .any(|u| u.pos == at && u.effects.iter().any(|e| e.id == EffectId::VISION_GRANT))
    });
    client.stand_at((at.x.to_int() + 100, at.y.to_int()));
    let watcher = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        .expect("the watcher")
        .id;
    let interact = Intent::Smart {
        at: (at.x.to_f32(), at.y.to_f32()),
        unit: Some(watcher),
    };
    cancel_channel(&mut client, interact.clone());
    assert!(
        client
            .view()
            .unit(watcher)
            .expect("the watcher")
            .effects
            .iter()
            .any(|e| e.id == EffectId::VISION_GRANT),
        "an interrupted capture leaves the owner unchanged"
    );
    client.core_mut().act(interact);
    client.until("the opposing channel disables the watcher", |c| {
        c.view()
            .unit(watcher)
            .is_some_and(|u| u.effects.iter().any(|e| e.id == EffectId::LAMP_OFF))
    });
    let effects = &client.view().unit(watcher).expect("the watcher").effects;
    assert!(
        !effects.iter().any(|e| e.id == EffectId::VISION_GRANT),
        "the enemy's vision ends"
    );
    assert_eq!(
        effects
            .iter()
            .find(|e| e.id == EffectId::LAMP_OFF)
            .expect("disabled")
            .ticks_total,
        Some(120 * bota_core::rules::TICKS_PER_SECOND)
    );
}
