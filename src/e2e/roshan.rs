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
    client.until(
        "Roar's visible and globally announced player effects",
        |c| {
            sounded |= crate::sound::cues_sounded(c.world(), CueKind::BossRoar) > 0;
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
