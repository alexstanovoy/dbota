//! Every positional producer observes the same viewer in real full-stream replays.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy::audio::AudioPlayer;
use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{
    AbilityId, AbilitySlot, Cheat, EffectId, HeroId, ItemSlot, MapId, Order, SlotId, Target, Team,
    UnitKind, Vec2 as Spot,
};

use super::fog_pov::{ReplayFile, Spectated, replay_observed};
use super::harness::Client;

fn hero(client: &Client, slot: u8) -> &bota_proto::UnitView {
    client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Hero && unit.owner == Some(SlotId(slot)))
        .expect("the real player's hero")
}

fn counts(client: &Client, logical: &str) -> (u32, u32) {
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect(logical)
        .expect("the imported effect");
    let playing = client.world().resource::<crate::particles::Playing>();
    (playing.started(effect), playing.drawn(effect))
}

fn record_soul() -> (ReplayFile, Spot) {
    let mut game = Spectated::pair("0");
    game.cheat(Cheat::Invulnerable { on: true });
    game.order(Order::Move {
        target: Target::None,
    });
    game.cheat(Cheat::CreepSpawning { on: false });
    game.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::Radiant,
    });
    game.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::Dire,
    });
    game.cheat(Cheat::SpawnNeutrals);
    game.wait("the ranged neutrals have spawned", |_, client| {
        client.view().units.iter().any(|unit| {
            unit.kind == UnitKind::CreepNeutral
                && unit.attack_range.to_int() >= 400
                && unit.pos.x.to_int() > 3500
                && unit.pos.x.to_int() < 13000
        })
    });
    let fountain = bota_core::fountain_pos(bota_core::map_of(MapId(0)), Team::Radiant);
    let victim = game
        .client
        .view()
        .units
        .iter()
        .filter(|unit| {
            unit.kind == UnitKind::CreepNeutral
                && unit.attack_range.to_int() >= 400
                && unit.pos.x.to_int() > 3500
                && unit.pos.x.to_int() < 13000
        })
        .min_by_key(|unit| unit.pos.distance_squared(fountain))
        .expect("a ranged neutral");
    let (raw, at) = (victim.id, victim.pos);
    let killer_at = at + Spot::from_ints(-300, 0);
    game.cheat(Cheat::Teleport { at: killer_at });
    game.wait("the killer reaches the ranged neutral", |_, client| {
        hero(client, 0)
            .pos
            .within(killer_at, bota_core::rules::units(128))
    });
    game.cheat(Cheat::TimeOfDay { night: true });
    let marker_at = hero(&game.client, 0).pos + Spot::from_ints(-650, 0);
    game.other
        .as_mut()
        .unwrap()
        .act(Intent::Cheat(Cheat::Teleport { at: marker_at }));
    game.wait("the marker reaches its viewing point", |_, client| {
        hero(client, 1)
            .pos
            .within(marker_at, bota_core::rules::units(128))
    });
    game.other.as_mut().unwrap().send_order(Order::Move {
        target: Target::None,
    });
    game.wait(
        "the killer visible but its ranged victim hidden from Dire",
        |_, client| {
            hero(client, 0).seen_by.is_some_and(|seen| seen.dire)
                && client
                    .view()
                    .unit(raw)
                    .is_some_and(|unit| unit.seen_by.is_some_and(|seen| seen.radiant && !seen.dire))
        },
    );
    let target = game
        .driver
        .view()
        .unwrap()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::CreepNeutral && unit.pos == at)
        .expect("the same visible neutral in the player's namespace")
        .id;
    game.cheat(Cheat::ApplyModifier {
        target: Target::None,
        spec: bota_proto::ModifierSpec {
            physical_damage: bota_proto::ModifierSpec::MAX_SCALE,
            ..bota_proto::ModifierSpec::NOMINAL
        },
        ticks: 900,
    });
    game.cheat(Cheat::ApplyModifier {
        target: Target::Unit(target),
        spec: bota_proto::ModifierSpec {
            max_hp: bota_proto::ModifierSpec::MIN_SCALE,
            ..bota_proto::ModifierSpec::NOMINAL
        },
        ticks: 900,
    });
    game.order(Order::Attack {
        target: Target::Unit(target),
    });
    game.until_death(raw);
    assert!(
        hero(&game.client, 0).seen_by.unwrap().dire,
        "the killer remains visible"
    );
    (game.save_through_death(raw), at)
}

#[test]
fn a_visible_killer_does_not_reveal_a_hidden_necromastery_victim() {
    let (file, at) = record_soul();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    for (pov, visible) in [(Some(0), true), (Some(1), false), (None, true)] {
        let client = replay_observed(&file, pov, &["--look", &look], |_| {});
        let (started, drawn) = counts(&client, "hero.nevermore.necro_soul");
        assert_eq!(
            (started > 0, drawn > 0),
            (visible, visible),
            "POV {pov:?}: the victim, not just its killer, must authorize emission and rendering"
        );
    }
}

type Sounds = Arc<Mutex<Vec<PathBuf>>>;

fn listen(client: &mut Client, heard: Sounds) {
    client.world_mut().add_observer(
        move |add: On<Add, AudioPlayer>, players: Query<&AudioPlayer>, server: Res<AssetServer>| {
            if let Ok(player) = players.get(add.entity)
                && let Some(path) = server.get_path(player.0.id())
            {
                heard.lock().unwrap().push(path.path().to_path_buf());
            }
        },
    );
}

fn heard(client: &Client, sounds: &Sounds, logical: &str) -> bool {
    let manifest = client.world().resource::<crate::manifest::Manifest>();
    let sources: Vec<_> = manifest.sound_files(logical).collect();
    sounds
        .lock()
        .unwrap()
        .iter()
        .any(|path| sources.iter().any(|source| path.ends_with(source)))
}

fn record_teleport() -> (ReplayFile, Spot, Spot) {
    let mut game = Spectated::start("2");
    let from = bota_core::DOTA_RUNE_SPOTS[0].at;
    game.cheat(Cheat::Teleport { at: from });
    game.cheat(Cheat::Refresh);
    let fountain = game
        .driver
        .view()
        .unwrap()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Fountain && unit.team == Team::Radiant)
        .expect("the public destination");
    let (target, to) = (fountain.id, fountain.pos);
    game.order(Order::Use {
        slot: ItemSlot(bota_client_core::TOWN_PORTAL_SLOT),
        target: Target::Unit(target),
    });
    game.wait("the real teleport lands", |driver, _| {
        driver.told.iter().any(|event| {
            matches!(
                event,
                bota_proto::EventKind::Teleported { landed: true, .. }
            )
        })
    });
    game.cheat(Cheat::Gold { amount: 1 });
    (game.save(), from, to)
}

#[test]
fn a_public_teleport_destination_does_not_disclose_a_hidden_channel_origin() {
    let (file, from, to) = record_teleport();
    for (pov, origin_seen) in [(Some(0), true), (Some(1), false), (None, true)] {
        let sounds = Sounds::default();
        let into = sounds.clone();
        let look = format!("{},{}", from.x.to_int(), from.y.to_int());
        let client = replay_observed(&file, pov, &["--look", &look], |client| {
            listen(client, into)
        });
        assert_eq!(
            counts(&client, "item.teleport_start").0 > 0,
            origin_seen,
            "POV {pov:?}: private origin particle"
        );
        assert!(
            counts(&client, "item.teleport_end").0 > 0,
            "the destination is public"
        );
        assert_eq!(
            heard(&client, &sounds, "sound.portal.loop_disappear"),
            origin_seen,
            "POV {pov:?}: private origin hum"
        );
        assert_eq!(
            heard(&client, &sounds, "sound.portal.hero_disappear"),
            origin_seen,
            "POV {pov:?}: private departure sound"
        );
    }
    let sounds = Sounds::default();
    let into = sounds.clone();
    let look = format!("{},{}", to.x.to_int(), to.y.to_int());
    let client = replay_observed(&file, Some(1), &["--look", &look], |client| {
        listen(client, into)
    });
    assert!(
        heard(&client, &sounds, "sound.portal.loop_appear"),
        "public destination hum"
    );
    assert!(
        heard(&client, &sounds, "sound.portal.hero_appear"),
        "public arrival"
    );
}

fn particle_at(client: &Client, logical: &str, at: Spot) -> bool {
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect(logical)
        .unwrap();
    client
        .world()
        .resource::<crate::particles::Playing>()
        .instances_of(effect)
        .any(|instance| {
            instance.nodes.first().is_some_and(|node| {
                let point = node.control_points[0].position;
                Vec2::new(point.x, point.y).distance(Vec2::new(at.x.to_f32(), at.y.to_f32())) < 1.0
            })
        })
}

#[test]
fn glyph_particles_do_not_cover_hidden_structures() {
    let mut game = Spectated::start("2");
    game.cheat(Cheat::CreepSpawning { on: false });
    game.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::All,
    });
    let at = game
        .client
        .view()
        .units
        .iter()
        .filter(|unit| unit.kind == UnitKind::Tower && unit.team == Team::Radiant)
        .min_by_key(|unit| unit.pos.distance_squared(hero(&game.client, 0).pos))
        .unwrap()
        .pos;
    game.cheat(Cheat::Refresh);
    game.order(Order::Glyph);
    game.wait("the real Glyph covers the structure", |_, client| {
        client.view().units.iter().any(|unit| {
            unit.pos == at
                && unit
                    .effects
                    .iter()
                    .any(|effect| effect.id == EffectId::FORTIFIED)
        })
    });
    let file = game.save();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    for (pov, visible) in [(Some(0), true), (Some(1), false), (None, true)] {
        let mut client = replay_observed(&file, pov, &["--look", &look], |_| {});
        client.until("the selected vision is applied", |client| {
            client
                .world()
                .resource::<crate::fog::FogGrid>()
                .worked_out_at()
                .is_some_and(|tick| tick == client.tick())
        });
        assert_eq!(
            particle_at(&client, "glyph.structure", at),
            visible,
            "POV {pov:?}: Glyph follows unit sight"
        );
    }
}

fn record_hook() -> (ReplayFile, Spot) {
    let mut game = Spectated::hero("2", HeroId::PUDGE);
    game.next_side(Team::Dire);
    game.cheat(Cheat::Levels { count: 1 });
    let hero = game.driver.my_hero().unwrap();
    let unit = game.driver.view().unwrap().unit(hero).unwrap();
    let at = unit.pos;
    let slot = AbilitySlot(
        unit.abilities
            .iter()
            .position(|ability| ability.id == AbilityId::MEAT_HOOK)
            .unwrap() as u8,
    );
    game.order(Order::Learn { slot });
    game.order(Order::Cast {
        slot,
        target: Target::Pos(at + Spot::from_ints(900, 0)),
    });
    game.wait("the hook returns", |driver, _| {
        driver.view().unwrap().unit(hero).unwrap().abilities[slot.0 as usize].cooldown_left > 0
            && driver.view().unwrap().projectiles.is_empty()
    });
    game.cheat(Cheat::Gold { amount: 1 });
    assert_eq!(
        counts(&game.client, "hero.pudge.hook"),
        (0, 0),
        "the real side-filtered live client receives no hidden flight"
    );
    (game.save(), at)
}

#[test]
fn hidden_hook_flights_never_reach_the_particle_renderer() {
    let (file, at) = record_hook();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    for (pov, visible) in [(Some(0), true), (Some(1), false), (None, true)] {
        let client = replay_observed(&file, pov, &["--look", &look], |_| {});
        let (started, drawn) = counts(&client, "hero.pudge.hook");
        assert_eq!(
            (started > 0, drawn > 0),
            (visible, visible),
            "POV {pov:?}: flight selection and final cards, lights and models"
        );
    }
}

#[derive(Resource, Default)]
struct FlightSwitch {
    switched: bool,
    checked: bool,
    drawn: u32,
}

fn switch_hook_pov(
    mut session: NonSendMut<crate::session::Session>,
    library: Option<Res<crate::particles::Library>>,
    playing: Res<crate::particles::Playing>,
    fog: Res<crate::fog::FogGrid>,
    mut check: ResMut<FlightSwitch>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    if !view
        .projectiles
        .iter()
        .any(|p| p.inflictor == Some(bota_proto::Inflictor::Ability(AbilityId::MEAT_HOOK)))
    {
        return;
    }
    let Some(library) = library else {
        return;
    };
    let effect = library.effect("hero.pudge.hook").unwrap();
    let held = playing.instances_of(effect).next().is_some();
    if !check.switched && held && playing.drawn(effect) > 0 {
        check.drawn = playing.drawn(effect);
        check.switched = true;
        session.0.act(Intent::NextEyes);
        session.0.pinned = None;
    } else if check.switched && !check.checked && fog.team == Some(Team::Dire) {
        assert!(
            !held && playing.drawn(effect) == check.drawn,
            "a hidden ongoing flight or tail survived the POV switch: held={held}, private drawn frames={}",
            playing.drawn(effect) - check.drawn
        );
        check.checked = true;
    }
}

#[test]
fn switching_pov_cancels_hidden_flights_and_their_ongoing_tails() {
    let (file, at) = record_hook();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    let client = replay_observed(&file, Some(0), &["--look", &look], |client| {
        client.app.init_resource::<FlightSwitch>().add_systems(
            PostUpdate,
            switch_hook_pov.after(crate::particles::gather_frame),
        );
    });
    let check = client.world().resource::<FlightSwitch>();
    assert!(check.switched, "a real visible flight was drawn first");
    assert!(
        check.checked,
        "the same projectile was still in flight after the switch"
    );
}
