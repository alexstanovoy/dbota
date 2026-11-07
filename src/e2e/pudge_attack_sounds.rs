//! Pudge's own sound-event rows: the blade whooshes during windup and hits
//! on the authoritative melee strike, with no invented projectile impact.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::{audio::PlaybackMode, prelude::*, time::TimeUpdateStrategy};
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, UnitKind, UnitView, Vec2 as Spot};

use super::harness::{Client, MID, Server};

#[derive(Debug)]
struct SoundStart {
    event: String,
    tick: u32,
    path: String,
    mode: PlaybackMode,
}

fn listen(client: &mut Client) -> Arc<Mutex<Vec<SoundStart>>> {
    let starts = Arc::new(Mutex::new(Vec::new()));
    let into = starts.clone();
    client.world_mut().add_observer(
        move |add: On<Add<AudioPlayer>>,
              players: Query<(&AudioPlayer, &PlaybackSettings)>,
              mixer: Res<crate::mixer::Mixer>,
              session: NonSend<crate::session::Session>| {
            let Some((event, _)) = mixer.event_at(add.entity) else {
                return;
            };
            if !matches!(event, "Hero_Pudge.PreAttack" | "Hero_Pudge.Attack") {
                return;
            }
            let (player, settings) = players.get(add.entity).expect("a sound player");
            let path = player.0.path().expect("a real imported sound").to_string();
            into.lock().expect("sound starts").push(SoundStart {
                event: event.to_owned(),
                tick: session.0.view().expect("match").tick,
                path,
                mode: settings.mode,
            });
        },
    );
    starts
}

fn scene() -> (Server, Client, UnitView) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_nanos(
            33_333_333,
        )));
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::All,
    });
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    client.core_mut().act(Intent::Stop);
    client.frames(3);
    let at = client.me().pos;
    client.cheat(Cheat::CreateHero {
        hero: HeroId::SHADOW_FIEND,
        enemy: true,
        at: at + Spot::from_ints(120, 0),
    });
    let enemy = client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Hero && unit.team != client.me().team)
        .expect("enemy")
        .clone();
    (server, client, enemy)
}

fn hit_once(client: &mut Client, enemy: &UnitView) -> (u32, u32) {
    let id = client.me().id;
    let mut windup = None;
    let mut strike = None;
    client.core_mut().act(Intent::AttackAt {
        at: (enemy.pos.x.to_f32(), enemy.pos.y.to_f32()),
        unit: Some(enemy.id),
    });
    client.until("Pudge's first melee hit deals damage", |c| {
        if let Some(swing) = c
            .view()
            .aggro
            .iter()
            .find(|aggro| aggro.unit == id)
            .and_then(|aggro| aggro.swing)
        {
            windup.get_or_insert(c.tick());
            strike.get_or_insert(c.tick() + swing.strike_in);
        }
        c.view()
            .unit(enemy.id)
            .is_some_and(|unit| unit.hp < enemy.hp)
    });
    assert_eq!(
        Some(client.tick()),
        strike,
        "a real hit landed on the swing's strike tick"
    );
    (windup.expect("a real windup"), client.tick())
}

fn assert_sounds(heard: &[SoundStart], windup: u32, strike: u32) {
    assert_eq!(
        heard.len(),
        2,
        "one whoosh and one hit, not silence or duplicate attacks: {heard:?}"
    );
    assert_eq!(heard[0].event, "Hero_Pudge.PreAttack");
    assert_eq!(heard[0].tick, windup);
    assert!(heard[0].path.contains("hero/shared/small_blade/whoosh"));
    assert_eq!(heard[1].event, "Hero_Pudge.Attack");
    assert_eq!(heard[1].tick, strike, "the hit sound waits for damage");
    assert!(heard[1].path.contains("hero/pudge/attack"));
    assert!(
        heard
            .iter()
            .all(|sound| !matches!(sound.mode, PlaybackMode::Loop))
    );
    assert!(
        heard[0].tick < heard[1].tick,
        "the whoosh precedes the blow"
    );
}

#[test]
fn a_pudge_hit_plays_its_own_windup_and_melee_attack_sounds_once() {
    let (server, mut client, enemy) = scene();
    let starts = listen(&mut client);
    let (windup, strike) = hit_once(&mut client, &enemy);
    let heard = starts.lock().expect("sound starts");
    assert_sounds(&heard, windup, strike);
    drop(heard);
    server.frozen(|| {
        client.frames(4);
        assert_eq!(
            starts.lock().expect("sound starts").len(),
            2,
            "rendering cannot replay the attack"
        );
    });
}
