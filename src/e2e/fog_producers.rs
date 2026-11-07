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
        move |add: On<Add<AudioPlayer>>, players: Query<&AudioPlayer>, server: Res<AssetServer>| {
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

fn record_teleport(lose_origin: bool) -> (ReplayFile, Spot, Spot) {
    let mut game = if lose_origin {
        Spectated::pair("2")
    } else {
        Spectated::start("2")
    };
    let from = bota_core::DOTA_RUNE_SPOTS[0].at;
    game.cheat(Cheat::Teleport { at: from });
    if lose_origin {
        game.other
            .as_mut()
            .unwrap()
            .act(Intent::Cheat(Cheat::Teleport {
                at: from + Spot::from_ints(200, 0),
            }));
        game.wait("Dire can see the origin before the channel", |_, client| {
            hero(client, 0).seen_by.is_some_and(|seen| seen.dire)
        });
        game.other.as_mut().unwrap().send_order(Order::Move {
            target: Target::None,
        });
        game.cheat(Cheat::Invulnerable { on: true });
    }
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
    if lose_origin {
        game.wait("the real channel begins", |_, client| {
            !client.view().teleports.is_empty()
        });
        let away = hero(&game.client, 1).pos + Spot::from_ints(4000, 0);
        game.other
            .as_mut()
            .unwrap()
            .act(Intent::Cheat(Cheat::Teleport { at: away }));
        game.wait(
            "Dire loses current sight while the channel continues",
            |_, client| {
                !client.view().teleports.is_empty() && !hero(client, 0).seen_by.unwrap().dire
            },
        );
    }
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

/// Public destination is a guard; the hidden origin is the reproduced privacy failure.
#[test]
fn a_public_teleport_destination_does_not_disclose_a_hidden_channel_origin() {
    let (file, from, to) = record_teleport(false);
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

#[derive(Resource, Default)]
struct PingFrames(Vec<Vec<Vec2>>, Vec<Vec<Vec2>>, Vec<Vec<Vec2>>);

fn observe_ping_frames(
    sight: crate::fog::EventSight,
    colours: Res<crate::player_colour::PlayerColours>,
    drawn: Res<crate::pings::DrawnPings>,
    rings: Query<(&Node, &Visibility), With<crate::minimap::PingRing>>,
    mut frames: ResMut<PingFrames>,
) {
    frames.1.push(drawn.0.clone());
    frames.2.push(
        rings
            .iter()
            .filter(|(_, visibility)| **visibility != Visibility::Hidden)
            .filter_map(|(node, _)| {
                let (Val::Px(left), Val::Px(top), Val::Px(width), Val::Px(height)) =
                    (node.left, node.top, node.width, node.height)
                else {
                    return None;
                };
                Some(Vec2::new(left + width / 2.0, top + height / 2.0))
            })
            .collect(),
    );
    let marks = crate::pings::marks(&sight, &colours, |_| None);
    if !marks.is_empty() {
        frames
            .0
            .push(marks.into_iter().map(|mark| mark.at).collect());
    }
}

/// Guard: core POV filtering already hides enemy team chat and pings. The regression
/// reproduced here is an allied world ring drawn through fog; union intentionally hears both.
#[test]
fn team_pings_and_team_chat_follow_the_replay_pov() {
    use bota_proto::{Audience, Ping, PingAt, PingKind};
    let mut game = Spectated::pair("2");
    let radiant = hero(&game.client, 0).pos;
    let dire = hero(&game.client, 1).pos;
    let hidden = Spot::from_ints(10000, 13000);
    game.driver.ping(Ping {
        kind: PingKind::Regular,
        at: PingAt::Spot(radiant),
    });
    game.driver.ping(Ping {
        kind: PingKind::Regular,
        at: PingAt::Spot(hidden),
    });
    game.other.as_mut().unwrap().ping(Ping {
        kind: PingKind::Danger,
        at: PingAt::Spot(dire),
    });
    game.driver
        .say(Audience::Team, "radiant-private-marker")
        .expect("Radiant chat");
    game.other
        .as_mut()
        .unwrap()
        .say(Audience::Team, "dire-private-marker")
        .expect("Dire chat");
    game.cheat(Cheat::Gold { amount: 1 });
    game.record_team_lines(5);
    let file = game.save();
    for (pov, own, enemy) in [
        (Some(0), radiant, Some(dire)),
        (Some(1), dire, Some(radiant)),
        (None, radiant, None),
    ] {
        let sounds = Sounds::default();
        let into = sounds.clone();
        let mut client = replay_observed(&file, pov, &[], |client| {
            listen(client, into);
            client
                .app
                .init_resource::<PingFrames>()
                .add_systems(PostUpdate, observe_ping_frames);
        });
        let frames = &client.world().resource::<PingFrames>().0;
        let point = |at: Spot| Vec2::new(at.x.to_f32(), at.y.to_f32());
        assert!(
            frames.iter().flatten().any(|at| *at == point(own)),
            "POV {pov:?} keeps its team's ping"
        );
        if let Some(enemy) = enemy {
            assert!(
                !frames.iter().flatten().any(|at| *at == point(enemy)),
                "POV {pov:?} exposed the enemy team ping"
            );
        }
        let rings = &client.world().resource::<PingFrames>().2;
        let minimap_at = |at: Spot| crate::minimap::on_map(at.x.to_f32(), at.y.to_f32());
        assert!(
            rings
                .iter()
                .flatten()
                .any(|at| at.distance(minimap_at(own)) < 0.01),
            "the allied minimap ring is laid out"
        );
        if let Some(enemy) = enemy {
            assert!(
                !rings
                    .iter()
                    .flatten()
                    .any(|at| at.distance(minimap_at(enemy)) < 0.01),
                "an enemy ping was laid out on the minimap"
            );
        }
        assert_eq!(
            heard(&client, &sounds, "sound.ping.regular"),
            pov != Some(1)
        );
        assert_eq!(heard(&client, &sounds, "sound.ping.danger"), pov != Some(0));
        let drawn = &client.world().resource::<PingFrames>().1;
        if pov == Some(0) {
            assert!(
                !drawn.iter().flatten().any(|at| *at == point(hidden)),
                "an allied ping in fog must not draw a world ring"
            );
        }
        let world = client.world_mut();
        let shown = world
            .query::<(&TextSpan, &ChildOf)>()
            .iter(world)
            .filter(|(_, parent)| {
                world
                    .get::<Node>(parent.parent())
                    .is_some_and(|node| node.display != Display::None)
            })
            .map(|(span, _)| span.0.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(shown.contains("radiant-private-marker"), pov != Some(1));
        assert_eq!(shown.contains("dire-private-marker"), pov != Some(0));
    }
}

#[test]
fn a_visible_hook_retracts_with_its_end_cap() {
    let (file, at) = record_hook();
    let look = format!("{},{}", at.x.to_int(), at.y.to_int());
    let client = replay_observed(&file, Some(0), &["--look", &look], |_| {});
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect("hero.pudge.hook")
        .unwrap();
    let playing = client.world().resource::<crate::particles::Playing>();
    assert!(playing.drawn(effect) > 0, "the real hook flew visibly");
    assert!(
        playing.drawn_endings(effect) > 0,
        "a visible retraction draws its ending rather than being cancelled"
    );
}

#[test]
fn a_witnessed_teleport_departure_keeps_the_last_seen_origin() {
    let (file, from, _) = record_teleport(true);
    let sounds = Sounds::default();
    let into = sounds.clone();
    let look = format!("{},{}", from.x.to_int(), from.y.to_int());
    let client = replay_observed(&file, Some(1), &["--look", &look], |client| {
        listen(client, into)
    });
    assert!(
        heard(&client, &sounds, "sound.portal.loop_disappear"),
        "Dire witnessed the beginning"
    );
    assert!(
        heard(&client, &sounds, "sound.portal.hero_disappear"),
        "the last witnessed origin keeps its departure sound"
    );
}

#[derive(Resource, Default)]
struct GrenadeLandings(Vec<(Spot, usize, Option<bota_proto::EntityId>)>);

fn observe_grenade_landings(
    session: NonSend<crate::session::Session>,
    library: Option<Res<crate::particles::Library>>,
    playing: Res<crate::particles::Playing>,
    mut landings: ResMut<GrenadeLandings>,
) {
    let Some(effect) = library
        .as_ref()
        .and_then(|library| library.effect("item.blood_grenade"))
    else {
        return;
    };
    for event in &session.0.told {
        let bota_proto::EventKind::Landed {
            inflictor: bota_proto::Inflictor::Item(bota_proto::ItemId::BLOOD_GRENADE),
            at,
            missile,
            ..
        } = event
        else {
            continue;
        };
        let endings = playing
            .instances_of(effect)
            .filter(|instance| {
                instance.ending()
                    && instance.flight_id() == *missile
                    && instance.nodes.first().is_some_and(|node| {
                        let point = node.control_points[0].position;
                        Vec2::new(point.x, point.y)
                            .distance(Vec2::new(at.x.to_f32(), at.y.to_f32()))
                            < 1.0
                    })
            })
            .count();
        landings.0.push((*at, endings, *missile));
    }
}

fn record_two_grenades() -> (ReplayFile, Spot) {
    use bota_proto::ItemId;
    let mut game = Spectated::start("2");
    for _ in 0..2 {
        game.cheat(Cheat::Item {
            item: ItemId::BLOOD_GRENADE,
        });
    }
    let caster = game.driver.my_hero().unwrap();
    let from = game.driver.view().unwrap().unit(caster).unwrap().pos;
    for distance in [850, 900] {
        game.cheat(Cheat::Refresh);
        let unit = game.driver.view().unwrap().unit(caster).unwrap();
        let slot = ItemSlot(
            unit.items
                .iter()
                .position(|item| item.is_some_and(|item| item.id == ItemId::BLOOD_GRENADE))
                .expect("the next grenade") as u8,
        );
        game.order(Order::Use {
            slot,
            target: Target::Pos(from + Spot::from_ints(distance, 0)),
        });
        if distance == 850 {
            game.wait(
                "the first throw has completed its cast point",
                |_, client| {
                    client.view().projectiles.iter().any(|shot| {
                        shot.inflictor == Some(bota_proto::Inflictor::Item(ItemId::BLOOD_GRENADE))
                    })
                },
            );
        }
    }
    game.wait("both real grenades fly together", |_, client| {
        client
            .view()
            .projectiles
            .iter()
            .filter(|shot| {
                shot.inflictor == Some(bota_proto::Inflictor::Item(ItemId::BLOOD_GRENADE))
            })
            .count()
            == 2
    });
    game.wait("both real grenades have landed", |_, client| {
        client.view().projectiles.is_empty()
    });
    let settled = game.driver.view().unwrap().tick + u32::from(game.driver.tick_rate());
    game.wait("both grenade endings recorded", |driver, _| {
        driver.view().unwrap().tick >= settled
    });
    (game.save(), from)
}

#[test]
fn two_grenades_keep_their_own_flights_and_each_land_once() {
    let (file, from) = record_two_grenades();
    let look = format!("{},{}", from.x.to_int() + 450, from.y.to_int());
    let client = replay_observed(&file, Some(0), &["--look", &look], |client| {
        client.app.init_resource::<GrenadeLandings>().add_systems(
            Update,
            observe_grenade_landings.after(crate::effects::Flights),
        );
    });
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect("item.blood_grenade")
        .unwrap();
    let playing = client.world().resource::<crate::particles::Playing>();
    let landings = &client.world().resource::<GrenadeLandings>().0;
    assert_eq!(
        landings.len(),
        2,
        "both actual landing facts reached the replay"
    );
    assert!(
        landings
            .iter()
            .all(|(_, endings, missile)| *endings == 1 && missile.is_some()),
        "each landing ends exactly its own particle: {landings:?}"
    );
    assert_eq!(
        playing.started(effect),
        2,
        "landing A must not steal B's still-flying body and restart B"
    );
    assert!(
        playing.drawn_endings(effect) >= 2,
        "both visible endings contribute rendered output"
    );
}
