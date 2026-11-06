//! Roshan in a real server's match, as the client draws him.
use super::harness::{Client, Server};
use crate::units::{ModelBank, Unit};
use bota_proto::{Cheat, HeroId, UnitKind};

#[test]
fn roshan_stands_in_the_pit_as_his_imported_model_with_a_health_bar() {
    let server = Server::start(1, &["--objectives", "roshan", "--immortal-heroes"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 100, pit.y.to_int()));
    client.until("Roshan drawn in his pit with a health bar", |client| {
        let Some(roshan) = client
            .view()
            .units
            .iter()
            .find(|unit| unit.kind == UnitKind::Roshan)
        else {
            return false;
        };
        assert!(roshan.pos.within(pit, bota_core::rules::units(300)));
        let Some(body) = client.body(roshan.id) else {
            return false;
        };
        let world = client.world();
        let Some(model) = world.get::<Unit>(body).and_then(|unit| unit.model) else {
            return false;
        };
        world.resource::<ModelBank>().0[model].model.logical == "neutral.roshan.model"
            && world.resource::<crate::bars::Bars>().has(body)
    });
}

/// Slay the real server's Roshan through an attack order. The trusted test setup changes
/// health and attack amplification; combat, loot, visibility and events use normal paths.
fn slain_roshan() -> (Server, Client) {
    slain_roshan_with(&[])
}

fn slain_roshan_with(extra: &[&str]) -> (Server, Client) {
    slain_roshan_observed(extra, |_| {})
}

fn slain_roshan_observed(
    extra: &[&str],
    before_attack: impl FnOnce(&mut Client),
) -> (Server, Client) {
    use bota_client_core::Intent;
    use bota_proto::{ModifierSpec, Target};
    let mut args = vec!["--objectives", "roshan"];
    args.extend_from_slice(extra);
    let server = Server::start(1, &args);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);

    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::Levels { count: 29 });
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 200, pit.y.to_int()));
    client.until("Roshan in sight", |c| {
        c.view()
            .units
            .iter()
            .any(|unit| unit.kind == UnitKind::Roshan)
    });

    let boss = client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Roshan)
        .expect("boss")
        .clone();
    client.cheat(Cheat::ApplyModifier {
        target: Target::Unit(boss.id),
        spec: ModifierSpec {
            max_hp: ModifierSpec::MIN_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: 3000,
    });
    client.cheat(Cheat::ApplyModifier {
        target: Target::None,
        spec: ModifierSpec {
            physical_damage: ModifierSpec::MAX_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: 3000,
    });
    before_attack(&mut client);
    client.core_mut().act(Intent::AttackAt {
        at: (boss.pos.x.to_f32(), boss.pos.y.to_f32()),
        unit: Some(boss.id),
    });
    client.until("Roshan killed and the Aegis dropped", |c| {
        c.view()
            .loot
            .iter()
            .any(|drop| drop.item == bota_proto::ItemId::AEGIS)
    });

    (server, client)
}

#[test]
fn a_roshan_kill_draws_the_death_feed_and_ground_drop_then_aegis_reincarnates_its_carrier() {
    use bevy::prelude::*;
    use bota_client_core::{FeedKind, Intent};
    use bota_proto::{ItemId, ObjectiveKind};
    let (_server, mut client) = slain_roshan();
    assert!(client.core().feed.iter().any(|line| matches!(
        line.kind,
        FeedKind::BossFell {
            objective: ObjectiveKind::Roshan,
            ..
        }
    )));
    client.until("Roshan's kill feed text drawn", |c| {
        feed_texts(c).iter().any(|text| text.contains("Roshan"))
    });
    let drop = *client
        .view()
        .loot
        .iter()
        .find(|drop| drop.item == ItemId::AEGIS)
        .expect("drop");
    client.until("the imported Aegis model drawn on the ground", |c| {
        let world = c.world_mut();
        world
            .query::<(&crate::loot::Loot, &Transform)>()
            .iter(world)
            .any(|(loot, at)| {
                loot.model.as_deref() == Some("item.aegis.model")
                    && (at.translation.x - drop.pos.x.to_f32()).abs() < 50.0
                    && (at.translation.z + drop.pos.y.to_f32()).abs() < 50.0
            })
    });
    client.core_mut().act(Intent::Smart {
        at: (drop.pos.x.to_f32(), drop.pos.y.to_f32()),
        unit: None,
    });
    client.until("Aegis in the inventory with its pickup lifetime", |c| {
        c.slot_of(ItemId::AEGIS)
            .and_then(|slot| c.core().item_in(slot))
            .and_then(|item| item.expires_in)
            .is_some_and(|left| left > 0 && left <= 9000)
    });
    let aegis_slot = client.slot_of(ItemId::AEGIS).expect("Aegis slot");
    client.within(
        std::time::Duration::from_secs(30),
        "the Aegis expiry timer drawn in its own slot",
        |c| {
            let Some(root) = c.item_box(aegis_slot) else {
                return false;
            };
            let world = c.world_mut();
            world
                .query::<(Entity, &Text)>()
                .iter(world)
                .any(|(entity, text)| {
                    if !(text.0.starts_with("4:") || text.0 == "5:00") {
                        return false;
                    }
                    super::harness::descends_from(world, entity, root)
                })
        },
    );

    let hero = client.me().id;
    let fountain = bota_core::map_of(bota_proto::MapId(2)).fountains[1];
    client
        .core_mut()
        .act(Intent::Cheat(Cheat::Teleport { at: fountain }));
    let mut death_at = None;
    client.until("reincarnating with no counted death", |c| {
        for event in &c.core().told {
            if let bota_proto::EventKind::ItemUsed {
                item: ItemId::AEGIS,
                at: Some(at),
                ..
            } = event
            {
                death_at = Some(*at);
            }
        }
        let mine = c
            .view()
            .players
            .iter()
            .find(|seat| Some(seat.slot) == c.core().my_slot)
            .expect("seat");
        assert_eq!(
            mine.deaths, 0,
            "the first loss of life must consume Aegis instead of counting a death"
        );
        c.core().my_standing_hero().is_none() && mine.respawn_left > 0
    });
    let death_at = death_at
        .or_else(|| client.core().freshest(hero).map(|unit| unit.pos))
        .expect("the visible death location");

    client.within(std::time::Duration::from_secs(30), "the hero risen", |c| {
        c.core().my_standing_hero().is_some()
    });
    assert_eq!(client.me().id, hero);
    assert!(
        client
            .me()
            .pos
            .within(death_at, bota_core::rules::units(100)),
        "rose at {:?}, death at {:?}",
        client.me().pos,
        death_at
    );
    assert!(client.slot_of(ItemId::AEGIS).is_none());
    assert_eq!(
        client
            .view()
            .players
            .iter()
            .find(|seat| Some(seat.slot) == client.core().my_slot)
            .expect("seat")
            .deaths,
        0
    );
    client.until("the risen hero drawn", |c| c.body(hero).is_some());
}

#[test]
fn an_attack_click_on_ground_aegis_destroys_its_drawn_drop() {
    use bota_client_core::Intent;
    use bota_proto::ItemId;
    let (_server, mut client) = slain_roshan();
    let drop = *client
        .view()
        .loot
        .iter()
        .find(|drop| drop.item == ItemId::AEGIS)
        .expect("drop");
    client.core_mut().act(Intent::AttackAt {
        at: (drop.pos.x.to_f32(), drop.pos.y.to_f32()),
        unit: None,
    });
    assert!(
        client.core().view().is_some(),
        "attack issuance closed the view: {:?}",
        client
            .core()
            .feed
            .iter()
            .map(|line| crate::feed::text(client.core(), &line.kind))
            .collect::<Vec<_>>()
    );
    client.until("the ground Aegis denied by its attack click", |c| {
        let view = c.core().view().unwrap_or_else(|| {
            panic!(
                "denial closed the view: {:?}",
                c.core()
                    .feed
                    .iter()
                    .map(|line| crate::feed::text(c.core(), &line.kind))
                    .collect::<Vec<_>>()
            )
        });
        !view.loot.iter().any(|item| item.id == drop.id)
    });
    assert!(client.core().feed.iter().any(|line| matches!(
        line.kind,
        bota_client_core::FeedKind::Loot {
            item: ItemId::AEGIS,
            destroyed: true,
            ..
        }
    )));
    client.until("the denial message drawn", |c| {
        feed_texts(c).iter().any(|text| text.contains("denied"))
    });
    client.until("the Aegis model gone", |c| {
        let world = c.world_mut();
        !world
            .query::<&crate::loot::Loot>()
            .iter(world)
            .any(|loot| loot.model.as_deref() == Some("item.aegis.model"))
    });
    assert!(client.slot_of(ItemId::AEGIS).is_none());
}

fn feed_texts(client: &mut Client) -> Vec<String> {
    use bevy::prelude::*;
    let world = client.world_mut();
    let Some(root) = crate::feed::root(world) else {
        return Vec::new();
    };
    world
        .query::<(Entity, &Text, &InheritedVisibility)>()
        .iter(world)
        .filter(|(entity, _, shown)| {
            shown.get() && super::harness::descends_from(world, *entity, root)
        })
        .map(|(_, text, _)| text.0.clone())
        .collect()
}

#[test]
fn planting_a_banner_draws_its_aura_preview_model_and_public_message() {
    use bota_client_core::{FeedKind, Intent, Slot};
    use bota_proto::{ItemId, Team};
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.stand_at(super::harness::MID);
    client.cheat(Cheat::Item {
        item: ItemId::ROSHANS_BANNER,
    });
    let slot = client.slot_of(ItemId::ROSHANS_BANNER).expect("banner");
    assert_eq!(client.core().item_in(slot).expect("item").level, 1);
    assert_eq!(client.core().area_of(Slot::Item(slot)), 600);
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    let from = client.me().pos;
    let at = (from.x.to_f32() + 200., from.y.to_f32());
    let screen = client.screen_of(at).expect("on screen");
    client.mouse(screen);
    client.until("Banner aura preview drawn", |c| {
        !super::rings::rings_of(c, crate::overlay::GOOD, 600.).is_empty()
    });
    client.core_mut().act(Intent::Aimed { at, unit: None });
    client.until("planted Banner drawn from the NPC catalogue", |c| {
        let Some(banner) = c.view().units.iter().find(|u| u.kind == UnitKind::Banner) else {
            return false;
        };
        let Some(body) = c.body(banner.id) else {
            return false;
        };
        let Some(model) = c.world().get::<Unit>(body).and_then(|u| u.model) else {
            return false;
        };
        c.world().resource::<ModelBank>().0[model].model.logical == "objective.roshans_banner.model"
            && c.core().feed.iter().any(|line| {
                matches!(
                    line.kind,
                    FeedKind::ItemPlaced {
                        item: ItemId::ROSHANS_BANNER,
                        team: Team::Radiant
                    }
                )
            })
    });
    client.until("Banner public message drawn", |c| {
        feed_texts(c)
            .iter()
            .any(|s| s.to_lowercase().contains("banner"))
    });
    client.cheat(Cheat::CreateCreeps {
        enemy: false,
        at: bota_proto::Vec2::from_ints(at.0 as i32, at.1 as i32),
    });
    client.until("allied lane creeps show the protection effect", |c| {
        c.view().units.iter().any(|u| {
            bota_core::kind_def(u.kind).lane_creep
                && u.team == Team::Radiant
                && u.effects
                    .iter()
                    .any(|e| e.id == bota_proto::EffectId::FORTIFIED)
        })
    });
}

#[test]
fn roshans_npc_level_and_growing_health_reach_the_drawn_panel() {
    use bota_client_core::Intent;
    let server = Server::start(
        1,
        &[
            "--objectives",
            "roshan",
            "--tick-rate",
            "600",
            "--immortal-heroes",
        ],
    );
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 100, pit.y.to_int()));
    client.until("Roshan visible", |c| {
        c.view().units.iter().any(|u| u.kind == UnitKind::Roshan)
    });
    let boss = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Roshan)
        .expect("boss")
        .clone();
    assert_eq!(boss.level, 30);
    assert_eq!(boss.attack_speed, 200);
    client.core_mut().act(Intent::SelectUnit(Some(boss.id)));
    client.until("Roshan's current health in the panel", |c| {
        crate::hud::health_readouts(c.world_mut())
            .iter()
            .any(|s| s == &format!("{} / {}", boss.max_hp, boss.max_hp))
    });
    client.until("growth changes the actual health readout", |c| {
        let Some(current) = c.view().units.iter().find(|u| u.id == boss.id) else {
            return false;
        };
        if current.max_hp <= boss.max_hp {
            return false;
        }
        let expected = format!("{} / {}", current.max_hp, current.max_hp);
        crate::hud::health_readouts(c.world_mut()).contains(&expected)
    });
}

#[test]
fn a_blocked_dismember_draws_the_spent_cooldown_without_a_channel() {
    use bevy::prelude::*;
    use bota_client_core::{Intent, Slot};
    use bota_proto::{EventKind, StatusFlags};
    let server = Server::start(1, &["--objectives", "roshan"]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::Levels { count: 5 });
    client.cheat(Cheat::Invulnerable { on: true });
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(3),
        ctrl: true,
    });
    client.until("Dismember learned", |c| {
        c.me().abilities.get(3).is_some_and(|a| a.level == 1)
    });
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 100, pit.y.to_int()));
    client.until("Roshan visible", |c| {
        c.view().units.iter().any(|u| u.kind == UnitKind::Roshan)
    });
    let boss = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Roshan)
        .expect("boss")
        .clone();
    let mana = client.me().mana;
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(3),
        ctrl: false,
    });
    client.core_mut().act(Intent::Aimed {
        at: (boss.pos.x.to_f32(), boss.pos.y.to_f32()),
        unit: Some(boss.id),
    });
    let mut blocked = false;
    client.until("spell block and Pudge's spent cooldown",|c| {
        blocked |= c.core().told.iter().any(|event|matches!(event,EventKind::AbilityCast {ability,..} if *ability==bota_core::NeutralAbility::SpellBlock.id()));
        blocked && c.me().abilities.get(3).is_some_and(|a|a.cooldown_left>0) && c.me().mana<mana-90
    });
    assert_eq!(client.me().statuses.bits & StatusFlags::CHANNELLING, 0);
    client.until("Dismember cooldown text actually drawn", |c| {
        let world = c.world_mut();
        let slot = world
            .query::<(Entity, &crate::hud::AbilityBox)>()
            .iter(world)
            .find(|(_, b)| b.0 == 3)
            .map(|(e, _)| e);
        let Some(slot) = slot else {
            return false;
        };
        world
            .query::<(&crate::hud::Part, &crate::hud::PartOf, &Text)>()
            .iter(world)
            .any(|(part, parent, text)| {
                *part == crate::hud::Part::Seconds && parent.0 == slot && !text.0.is_empty()
            })
    });
}

fn particle_started(client: &Client, name: &str) -> u32 {
    let library = client.world().resource::<crate::particles::Library>();
    let effect = library
        .effect(name)
        .unwrap_or_else(|| panic!("missing particle {name}"));
    client
        .world()
        .resource::<crate::particles::Playing>()
        .started(effect)
}

#[test]
fn roshans_slam_draws_its_cast_animation_particle_and_slow_on_the_player() {
    use bota_proto::Vec2;
    let server = Server::start(1, &["--objectives", "roshan", "--immortal-heroes"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 100, pit.y.to_int()));
    client.cheat(Cheat::CreateCreeps {
        enemy: false,
        at: pit + Vec2::from_ints(0, 100),
    });
    let mut casting = false;
    let mut slowed = false;
    client.until(
        "Slam's imported animation, particles and player slow",
        |c| {
            if let Some(boss) = c.view().units.iter().find(|u| u.kind == UnitKind::Roshan) {
                casting |= c
                    .body(boss.id)
                    .and_then(|e| c.world().get::<crate::anim::Pose>(e))
                    .is_some_and(|p| p.clip() == "cast3");
            }
            slowed |= c
                .me()
                .effects
                .iter()
                .any(|e| e.id == bota_proto::EffectId(83));
            casting && slowed && particle_started(c, "neutral.roshan_slam") > 0
        },
    );
}

#[test]
fn a_respawned_roshan_roars_with_animation_particles_global_sound_and_message() {
    use bota_client_core::{CueKind, FeedKind, Intent};
    use bota_proto::Target;
    let (_server, mut client) = slain_roshan_with(&[
        "--mode",
        "realtime",
        "--tick-rate",
        "300",
        "--immortal-heroes",
    ]);
    client.core_mut().act(Intent::Stop);
    client.cheat(Cheat::ClearModifiers {
        target: Target::None,
    });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.until("Roshan's natural 8–11 minute respawn", |c| {
        c.view().units.iter().any(|u| u.kind == UnitKind::Roshan)
    });
    let boss = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Roshan)
        .expect("respawned boss")
        .clone();
    client.core_mut().act(Intent::AttackAt {
        at: (boss.pos.x.to_f32(), boss.pos.y.to_f32()),
        unit: Some(boss.id),
    });
    let mut animation = false;
    let mut sounded = false;
    let mut both_layers = false;
    client.until(
        "Roar's visible and globally announced player effects",
        |c| {
            sounded |= crate::sound::cues_sounded(c.world(), CueKind::BossRoar) > 0;
            let mixer = c.world().resource::<crate::mixer::Mixer>();
            both_layers |= mixer.sounding("Roshan.RevengeRoar") > 0
                && mixer.sounding("Roshan.RevengeRoar.Layer") > 0;
            animation |= c
                .body(boss.id)
                .and_then(|e| c.world().get::<crate::anim::Pose>(e))
                .is_some_and(|p| p.clip() == "cast1");
            sounded
                && animation
                && particle_started(c, "neutral.roshan_roar") > 0
                && c.core()
                    .feed
                    .iter()
                    .any(|line| matches!(line.kind, FeedKind::BossRoared { .. }))
        },
    );
    assert!(feed_texts(&mut client).iter().any(|s| s.contains("roar")));
    assert!(
        both_layers,
        "the global Roar base and layer sound simultaneously"
    );
}

#[test]
fn refresher_shard_displays_its_mana_cost_on_the_item_and_plays_its_refresh_effect() {
    use bevy::prelude::*;
    use bota_client_core::{Intent, Slot};
    use bota_proto::ItemId;
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::Item {
        item: ItemId::REFRESHER_SHARD,
    });
    let slot = client.slot_of(ItemId::REFRESHER_SHARD).expect("Shard");
    client.within(
        std::time::Duration::from_secs(15),
        "Shard's 200 mana cost drawn on its own item icon",
        |c| {
            let world = c.world_mut();
            let Some(parent) = world
                .query::<(Entity, &crate::hud::ItemBox)>()
                .iter(world)
                .find(|(_, b)| b.0 == slot)
                .map(|(e, _)| e)
            else {
                return false;
            };
            world
                .query::<(&crate::hud::Part, &crate::hud::PartOf, &Text)>()
                .iter(world)
                .any(|(part, of, text)| {
                    *part == crate::hud::Part::Cost && of.0 == parent && text.0 == "200"
                })
        },
    );
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.until(
        "Shard consumed and the imported refresh effect started",
        |c| {
            c.slot_of(ItemId::REFRESHER_SHARD).is_none()
                && particle_started(c, "item.refresher") > 0
        },
    );
}

#[test]
fn roshans_bash_uses_its_unique_attack_animation_and_stuns_the_player() {
    let server = Server::start(
        1,
        &[
            "--objectives",
            "roshan",
            "--immortal-heroes",
            "--mode",
            "realtime",
            "--tick-rate",
            "300",
        ],
    );
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 100, pit.y.to_int()));
    let mut stunned = false;
    client.within(
        std::time::Duration::from_secs(15),
        "Bash's distinct attack animation and player stun",
        |c| {
            stunned |= c
                .me()
                .effects
                .iter()
                .any(|e| e.id == bota_proto::EffectId(84));
            stunned
                && c.view()
                    .units
                    .iter()
                    .find(|u| u.kind == UnitKind::Roshan)
                    .and_then(|u| c.body(u.id))
                    .and_then(|e| c.world().get::<crate::anim::Pose>(e))
                    .is_some_and(|p| p.clip() == "bash")
        },
    );
}

/// Quiet lockstep ticks still go through the real socket and normal acknowledgements;
/// the renderer resumes before any interaction being asserted.
fn advance_quietly_to(client: &mut Client, elapsed_seconds: u32) {
    use std::time::{Duration, Instant};
    let goal = client.core().pregame_ticks() + elapsed_seconds * bota_core::rules::TICKS_PER_SECOND;
    let deadline = Instant::now() + Duration::from_secs(120);
    for _ in 0..150000 {
        if client.view().game_tick() >= goal {
            client.frames(3);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "real lockstep server did not reach the fixture time"
        );
        client.core_mut().pump(0.0);
        assert!(
            !client.core().lost,
            "the stream remains valid while advancing quietly"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("bounded lockstep advance exhausted");
}

fn migration_with_an_ally_in_roshans_path() -> (Server, Client, bota_proto::EntityId) {
    use bota_client_core::{Intent, Slot};
    use bota_proto::{Team, Vec2};
    let server = Server::start(
        1,
        &[
            "--objectives",
            "roshan",
            "--immortal-heroes",
            "--tick-rate",
            "1200",
        ],
    );
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::Levels { count: 5 });
    client.cheat(Cheat::Invulnerable { on: true });
    for slot in [0, 3] {
        client.core_mut().act(Intent::Press {
            slot: Slot::Ability(slot),
            ctrl: true,
        });
        client.until("fixture ability learned", |c| {
            c.me()
                .abilities
                .get(slot as usize)
                .is_some_and(|a| a.level == 1)
        });
    }
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 260, pit.y.to_int() + 650));
    client.until("the observer's model loaded", |c| {
        c.body(c.me().id)
            .and_then(|e| c.world().get::<Unit>(e))
            .is_some_and(|u| u.model.is_some())
    });
    advance_quietly_to(&mut client, 15 * 60 - 3);
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: false,
        at: pit + Vec2::from_ints(230, 0),
    });
    client.until("the allied hero on screen", |c| {
        c.view()
            .units
            .iter()
            .any(|u| u.kind == UnitKind::Hero && u.id != c.me().id && u.team == Team::Radiant)
    });
    let victim = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.id != client.me().id && u.team == Team::Radiant)
        .expect("ally")
        .id;
    (server, client, victim)
}

#[test]
fn a_natural_pit_migration_draws_grab_flight_and_landing_even_when_spell_block_fires_during_the_channel()
 {
    use bota_client_core::{Intent, Slot};
    use bota_proto::{EventKind, ForcedPhase, Vec2};
    let (_server, mut client, victim) = migration_with_an_ally_in_roshans_path();
    client.until("the ally grabbed during the natural pit change", |c| {
        c.view()
            .unit(victim)
            .is_some_and(|u| u.forced.is_some_and(|f| f.phase == ForcedPhase::Held))
    });
    let boss = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Roshan)
        .expect("boss")
        .clone();
    assert_eq!(
        boss.channel.expect("grab channel").ability,
        bota_core::NeutralAbility::GrabAndThrow.id()
    );
    client.cheat(Cheat::Teleport {
        at: boss.pos + Vec2::from_ints(-150, 0),
    });
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(3),
        ctrl: false,
    });
    client.core_mut().act(Intent::Aimed {
        at: (boss.pos.x.to_f32(), boss.pos.y.to_f32()),
        unit: Some(boss.id),
    });
    let mut blocked = false;
    let mut kept_channel = false;
    let mut airborne = false;
    let mut raised = false;
    client.until("a full grab, visible arc and ordinary landing",|c|{
        blocked|=c.core().told.iter().any(|e|matches!(e,EventKind::AbilityCast{caster,ability} if *caster==boss.id&&*ability==bota_core::NeutralAbility::SpellBlock.id()));
        if blocked {kept_channel|=c.view().unit(boss.id).is_some_and(|u|u.channel.is_some_and(|ch|ch.ability==bota_core::NeutralAbility::GrabAndThrow.id()))&&c.body(boss.id).and_then(|e|c.world().get::<crate::anim::Pose>(e)).is_some_and(|p|p.clip()=="cast2");}
        let Some(hero)=c.view().unit(victim) else{return false;};
        airborne|=hero.forced.is_some_and(|f|f.phase==ForcedPhase::Airborne);
        raised|=c.body(victim).and_then(|e|c.world().get::<Unit>(e)).is_some_and(|u|crate::units::flight_height(u)>10.0);
        airborne&&raised&&hero.forced.is_none()
    });
    assert!(blocked);
    assert!(kept_channel);
    client.until("the landed model is back on the ground", |c| {
        c.body(victim)
            .and_then(|e| c.world().get::<Unit>(e))
            .is_some_and(|u| crate::units::flight_height(u) < 1.0)
    });
}

#[test]
fn interrupting_roshans_grab_with_hook_releases_the_allied_model_position_and_facing() {
    use bevy::prelude::*;
    use bota_client_core::{Intent, Slot};
    use bota_proto::ForcedPhase;
    let (_server, mut client, victim) = migration_with_an_ally_in_roshans_path();
    client.until("the ally held by Roshan", |c| {
        c.view()
            .unit(victim)
            .is_some_and(|u| u.forced.is_some_and(|f| f.phase == ForcedPhase::Held))
    });
    let at = client.view().unit(victim).expect("held ally").pos;
    let boss = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Roshan)
        .expect("boss")
        .pos;
    let outward = bota_core::point_along(at, at + (at - boss), bota_core::rules::units(600));
    client.cheat(Cheat::Teleport { at: outward });
    client.core_mut().act(Intent::Press {
        slot: Slot::Ability(0),
        ctrl: false,
    });
    client.core_mut().act(Intent::Aimed {
        at: (at.x.to_f32(), at.y.to_f32()),
        unit: Some(victim),
    });
    client.until("the hook pulls the ally clear of the grab", |c| {
        c.view().unit(victim).is_some_and(|u| {
            u.forced.is_none()
                && u.pos.distance_squared(at) > bota_core::rules::units(200).squared_raw()
        })
    });
    let released = client.view().unit(victim).expect("released ally").clone();
    client.until("the recovered model moves and turns normally", |c| {
        let Some(ally) = c.view().unit(victim) else {
            return false;
        };
        if ally.forced.is_some()
            || ally.pos.distance_squared(released.pos) < bota_core::rules::units(30).squared_raw()
            || ally.facing == released.facing
        {
            return false;
        }
        let Some(body) = c.body(victim) else {
            return false;
        };
        let world = c.world();
        let Some(unit) = world.get::<Unit>(body) else {
            return false;
        };
        let Some(model) = world.get::<crate::units::ModelOf>(body) else {
            return false;
        };
        world.get::<Transform>(model.0).is_some_and(|t| {
            t.rotation
                .abs_diff_eq(crate::coords::sim_facing_to_rotation(unit.facing), 0.001)
        })
    });
}

#[test]
fn pit_placement_previews_mark_wards_banners_and_branches_as_forbidden() {
    use bota_client_core::{Intent, Slot};
    use bota_proto::ItemId;
    let server = Server::start(1, &["--objectives", "roshan"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::Invulnerable { on: true });
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 120, pit.y.to_int()));
    for item in [
        ItemId::OBSERVER_WARD,
        ItemId::SENTRY_WARD,
        ItemId::WARD_DISPENSER,
        ItemId::ROSHANS_BANNER,
        ItemId::IRON_BRANCH,
    ] {
        client.cheat(Cheat::Item { item });
        let slot = client
            .slot_of(item)
            .unwrap_or_else(|| panic!("placement item {item:?} is held"));
        client.core_mut().act(Intent::Press {
            slot: Slot::Item(slot),
            ctrl: false,
        });
        let radius = client.core().area_of(Slot::Item(slot)).max(24) as f32;
        let screen = client
            .screen_of((pit.x.to_f32() + 180., pit.y.to_f32()))
            .expect("pit on screen");
        client.mouse(screen);
        client.until("the placement preview has been drawn", |c| {
            !super::rings::rings_of(c, crate::overlay::GOOD, radius).is_empty()
                || !super::rings::rings_of(c, crate::overlay::BAD, radius).is_empty()
        });
        assert!(
            !super::rings::rings_of(&client, crate::overlay::BAD, radius).is_empty(),
            "{item:?} is forbidden inside the pit"
        );
        client.stand_at(super::harness::MID);
        let here = client.me().pos;
        let screen = client
            .screen_of((here.x.to_f32() + 180., here.y.to_f32()))
            .expect("ordinary ground on screen");
        client.mouse(screen);
        client.until(
            "the same placement preview is allowed on ordinary ground",
            |c| !super::rings::rings_of(c, crate::overlay::GOOD, radius).is_empty(),
        );
        client.core_mut().act(Intent::CancelAim);
        client.core_mut().act(Intent::PutAt {
            slot,
            at: (here.x.to_f32() + 80., here.y.to_f32()),
            unit: None,
        });
        client.until("the previous item was put on the ground", |c| {
            c.slot_of(item).is_none()
        });
        client.stand_at((pit.x.to_int() + 120, pit.y.to_int()));
    }
}

fn public_roshan_message(client: &mut Client, needle: &str) -> bool {
    let world = client.world_mut();
    world
        .query::<&bevy::prelude::Text>()
        .iter(world)
        .any(|text| text.0.contains(needle))
}

#[test]
fn the_public_roshan_status_shows_dead_then_possible_then_alive_from_the_public_window() {
    use bota_client_core::Intent;
    let (_server, mut client) = slain_roshan_with(&["--tick-rate", "1200"]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.core_mut().act(Intent::Hold);
    client.frames(6);
    assert!(
        public_roshan_message(&mut client, "Roshan is dead"),
        "the public death window is drawn near the minimap"
    );
    let fall = client.view().lairs[0].fell_at.expect("public fall");
    let elapsed =
        fall.saturating_sub(client.core().pregame_ticks()) / bota_core::rules::TICKS_PER_SECOND;
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() - 700, pit.y.to_int()));
    advance_quietly_to(&mut client, elapsed + 8 * 60 + 1);
    assert!(
        !client
            .view()
            .units
            .iter()
            .any(|u| u.kind == UnitKind::Roshan)
    );
    assert!(public_roshan_message(&mut client, "Roshan may be alive"));
    advance_quietly_to(&mut client, elapsed + 11 * 60 + 1);
    assert!(public_roshan_message(&mut client, "Roshan is alive"));
    assert!(
        !client.view().lairs[0].alive_known_by.radiant,
        "the guaranteed deadline is public inference, not a fabricated sighting"
    );
}

#[test]
fn roshans_death_roar_and_stinger_are_global_even_with_the_camera_across_the_map() {
    use bevy::prelude::*;
    let (_server, mut client) = slain_roshan_observed(&[], |c| {
        c.core_mut().pinned = None;
        let world = c.world_mut();
        let mut camera = world
            .query::<&mut crate::camera::RtsCamera>()
            .single_mut(world)
            .expect("camera");
        camera.focus = Vec3::new(16000., 0., -2000.);
    });
    client.frames(2);
    let mixer = client.world().resource::<crate::mixer::Mixer>();
    assert!(
        mixer.sounding("Roshan.Death") > 0,
        "the non-positional death roar is heard"
    );
    assert!(
        mixer.sounding("valve_dota_001.music.roshan_end") > 0,
        "the default global stinger is heard"
    );
    assert_eq!(
        mixer.loop_gain("valve_dota_001.music.roshan_end"),
        None,
        "the music asset is a one-shot cue, not an owned loop"
    );
}

fn banner_instances(client: &Client) -> usize {
    let library = client.world().resource::<crate::particles::Library>();
    let effect = library
        .effect("objective.banner")
        .expect("imported Banner particle");
    client
        .world()
        .resource::<crate::particles::Playing>()
        .instances_of(effect)
        .count()
}

fn banner_with_its_ambient_effect() -> (Server, Client, bota_proto::EntityId, bota_proto::Vec2, u32)
{
    use bota_client_core::{Intent, Slot};
    use bota_proto::{ItemId, ModifierSpec, Target, Vec2};
    let server = Server::start(1, &["--tick-rate", "1200"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::ApplyModifier {
        target: Target::None,
        spec: ModifierSpec {
            physical_damage: ModifierSpec::MIN_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: 10000,
    });
    client.stand_at(super::harness::MID);
    client.cheat(Cheat::Item {
        item: ItemId::ROSHANS_BANNER,
    });
    let slot = client.slot_of(ItemId::ROSHANS_BANNER).expect("Banner");
    let at = client.me().pos + Vec2::from_ints(200, 0);
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.core_mut().act(Intent::Aimed {
        at: (at.x.to_f32(), at.y.to_f32()),
        unit: None,
    });
    client.until("the Banner model and its owned looping particle", |c| {
        c.view()
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Banner)
            .and_then(|u| c.body(u.id))
            .and_then(|e| c.world().get::<Unit>(e))
            .is_some_and(|u| u.model.is_some())
            && banner_instances(c) > 0
    });
    let banner = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Banner)
        .expect("Banner")
        .id;
    let elapsed = client
        .view()
        .game_tick()
        .saturating_sub(client.core().pregame_ticks())
        / bota_core::rules::TICKS_PER_SECOND;
    (server, client, banner, at, elapsed)
}

#[test]
fn an_expiring_banner_removes_its_model_and_every_owned_particle_instance() {
    let (_server, mut client, banner, _at, elapsed) = banner_with_its_ambient_effect();
    advance_quietly_to(&mut client, elapsed + 121);
    assert!(client.view().unit(banner).is_none());
    client.until(
        "the expired Banner's model and particles have finished",
        |c| c.body(banner).is_none() && banner_instances(c) == 0,
    );
}

#[test]
fn a_banner_destroyed_by_real_enemy_attacks_removes_its_model_and_every_owned_particle_instance() {
    let (_server, mut client, banner, at, _elapsed) = banner_with_its_ambient_effect();
    for offset in [-40, 40] {
        client.cheat(Cheat::CreateCreeps {
            enemy: true,
            at: at + bota_proto::Vec2::from_ints(offset, 50),
        });
    }
    client.until("enemy creep attacks destroy the Banner", |c| {
        c.view().unit(banner).is_none()
    });
    client.until(
        "the destroyed Banner's model and particles have finished",
        |c| c.body(banner).is_none() && banner_instances(c) == 0,
    );
}

#[test]
fn an_illusion_cannot_drop_a_copied_banner_as_real_ground_loot() {
    use bota_client_core::{Intent, Refused};
    use bota_proto::{ItemId, RejectReason, RuneKind};
    let server = Server::start(1, &["--runes", "illusion", "--tick-rate", "1200"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::StartGame);
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    client.stand_at((at.x.to_int() + 300, at.y.to_int()));
    client.cheat(Cheat::Item {
        item: ItemId::ROSHANS_BANNER,
    });
    let slot = client.slot_of(ItemId::ROSHANS_BANNER).expect("real Banner");
    let real = client.me().id;
    client.core_mut().act(Intent::Cheat(Cheat::SpawnRune {
        kind: RuneKind::Illusion,
        at,
    }));
    let tick = client.tick();
    client.within(
        std::time::Duration::from_secs(30),
        "illusion rune cheat answered",
        |c| {
            assert!(
                !c.core().lost,
                "stream failed: {}",
                c.core()
                    .feed
                    .iter()
                    .find_map(|f| match &f.kind {
                        bota_client_core::FeedKind::ProtocolBroken { why } => Some(why.as_str()),
                        _ => None,
                    })
                    .unwrap_or("connection lost")
            );
            c.tick() > tick + 1
        },
    );
    let rune = client
        .view()
        .runes
        .iter()
        .find(|r| r.kind == RuneKind::Illusion)
        .expect("visible rune")
        .clone();
    client.core_mut().send_order(bota_proto::Order::Take {
        target: bota_proto::Target::Unit(rune.id),
    });
    client.until("the rune creates a controlled illusion", |c| {
        c.view()
            .units
            .iter()
            .any(|u| u.kind == UnitKind::Hero && u.id != real && u.team == c.me().team)
    });
    let copy = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.id != real && u.team == client.me().team)
        .expect("illusion")
        .clone();
    client.core_mut().act(Intent::SelectUnit(Some(copy.id)));
    assert!(client.core().controls_selection());
    assert_eq!(
        client.core().item_in(slot).expect("copied inventory").id,
        ItemId::ROSHANS_BANNER
    );
    client.core_mut().reject = None;
    client.core_mut().act(Intent::PutAt {
        slot,
        at: (copy.pos.x.to_f32(), copy.pos.y.to_f32()),
        unit: None,
    });
    client.until("the copied-item drop is answered", |c| {
        c.core().reject.is_some() || c.core().item_in(slot).is_none()
    });
    assert!(matches!(
        client.core().reject,
        Some((Refused::Rule(RejectReason::ItemRestricted), _))
    ));
    assert!(
        client
            .view()
            .loot
            .iter()
            .all(|l| l.item != ItemId::ROSHANS_BANNER)
    );
    assert_eq!(
        client.core().item_in(slot).expect("copy retained").id,
        ItemId::ROSHANS_BANNER
    );
    client.core_mut().act(Intent::SelectUnit(Some(real)));
    let here = client.me().pos;
    client.core_mut().act(Intent::PutAt {
        slot,
        at: (here.x.to_f32(), here.y.to_f32()),
        unit: None,
    });
    client.until("the real hero can drop the real Banner", |c| {
        c.view()
            .loot
            .iter()
            .any(|l| l.item == ItemId::ROSHANS_BANNER)
    });
}
