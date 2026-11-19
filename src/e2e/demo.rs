//! The demo's actual buttons, the order they send, and the resulting match and HUD.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{
    Cheat, EntityId, HeroId, ItemId, Order, StatusFlags, Target, Team, UnitKind, UnitView,
    Vec2 as At,
};

use super::harness::{Client, Server, quiet_seat};

fn demo() -> (Server, Client) {
    let map = bota_core::HERO_DEMO_ID.0.to_string();
    let server = Server::for_hud(1, &["--map", &map, "--seated-start"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.ready_with_match_held(&server);
    client.frames(2);
    (server, client)
}

fn sent(client: &Client) -> (Option<EntityId>, Order) {
    let order = client
        .core()
        .shown_orders
        .iter()
        .find(|order| Some(order.slot) == client.core().my_slot)
        .expect("the control sent an order");
    (order.unit, order.order)
}

fn click_cheat(client: &mut Client, button: &str, target: Option<EntityId>, cheat: Cheat) {
    client.click_named(button);
    assert_eq!(sent(client), (target, Order::Cheat { cheat }), "{button}");
}

fn hero_cheat(client: &mut Client, button: &str, cheat: Cheat) {
    let id = client.core().commanded().expect("hero selected");
    click_cheat(client, button, Some(id), cheat);
}

fn named_text(client: &mut Client, label: &str, text: &str) -> bool {
    let world = client.world_mut();
    world
        .query::<(&Name, &Text)>()
        .iter(world)
        .any(|(name, value)| name.as_str() == label && value.0 == text)
}

fn expect_level(client: &mut Client, hero: EntityId, level: u8) {
    client.until("the selected hero's level in the world and HUD", |client| {
        client
            .view()
            .unit(hero)
            .is_some_and(|unit| unit.level == level)
            && named_text(client, "hud level figure", &level.to_string())
    });
}

fn point(client: &mut Client, at: At, button: MouseButton) {
    let screen = client
        .screen_of((at.x.to_f32(), at.y.to_f32()))
        .expect("point in view");
    client.mouse(screen);
    client.click(button);
}

fn nearby(client: &Client, dx: i32, dy: i32) -> At {
    client.me().pos + At::from_ints(dx, dy)
}

fn select(client: &mut Client, hero: EntityId) {
    client.core_mut().act(Intent::SelectUnit(Some(hero)));
    client.frames(2);
}

fn created(client: &Client, team: Team) -> Option<&UnitView> {
    let my = client.core().my_hero();
    client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Hero && unit.team == team && Some(unit.id) != my)
}

fn spawn(client: &mut Client, enemy: bool) -> EntityId {
    let at = nearby(client, 200, -80);
    let team = if enemy { Team::Dire } else { Team::Radiant };
    let button = if enemy {
        "demo spawnenemy"
    } else {
        "demo spawnally"
    };
    client.click_named("demo pudge");
    client.click_named(button);
    assert!(
        created(client, team).is_none(),
        "spawn waits for the ground click"
    );
    point(client, at, MouseButton::Left);
    assert!(matches!(sent(client),
        (None, Order::Cheat { cheat: Cheat::CreateHero { hero: HeroId::PUDGE, enemy: sent, .. } })
        if sent == enemy
    ));
    client.until(
        "the spawned hero's body drawn on the chosen side",
        |client| created(client, team).is_some_and(|unit| client.body(unit.id).is_some()),
    );
    let unit = created(client, team).expect("the spawned hero");
    assert!((unit.pos.x.to_f32() - at.x.to_f32()).abs() < 45.0);
    assert!((unit.pos.y.to_f32() - at.y.to_f32()).abs() < 45.0);
    unit.id
}

#[test]
fn demo_level_up_raises_the_selected_hero_one_level() {
    let (_server, mut client) = demo();
    let hero = spawn(&mut client, false);
    select(&mut client, hero);
    hero_cheat(&mut client, "demo levelup", Cheat::Levels { count: 1 });
    expect_level(&mut client, hero, 2);
    assert_eq!(client.me().level, 1);
}

#[test]
fn demo_level_max_raises_the_selected_hero_to_the_cap() {
    let (_server, mut client) = demo();
    let hero = client.me().id;
    let count = bota_core::rules::HERO_MAX_LEVEL;
    hero_cheat(&mut client, "demo levelmax", Cheat::Levels { count });
    expect_level(&mut client, hero, count);
}

#[test]
fn demo_reset_returns_the_hero_to_level_one_with_its_starting_points() {
    let (_server, mut client) = demo();
    let hero = client.me().id;
    client.cheat(Cheat::Levels { count: 10 });
    client.cheat(Cheat::MaxAbilities);
    assert!(
        client
            .me()
            .abilities
            .iter()
            .any(|ability| ability.level > 0)
    );
    hero_cheat(&mut client, "demo reset", Cheat::ResetHero);
    expect_level(&mut client, hero, 1);
    client.until("the first ability can be learned again", |client| {
        level_plate(client, "hud level up 0")
    });
}

fn level_plate(client: &mut Client, name: &str) -> bool {
    let world = client.world_mut();
    world
        .query::<(&Name, &InheritedVisibility)>()
        .iter(world)
        .any(|(label, visibility)| label.as_str() == name && visibility.get())
}

#[test]
fn demo_learn_max_and_unlearn_change_the_ability_bar_and_return_points() {
    let (_server, mut client) = demo();
    client.cheat(Cheat::Levels { count: 29 });
    hero_cheat(&mut client, "demo learn max", Cheat::MaxAbilities);
    client.until(
        "the first spell at maximum and its learn button gone",
        |client| {
            let spell = &client.me().abilities[0];
            spell.level == spell.max_level && !level_plate(client, "hud level up 0")
        },
    );
    hero_cheat(&mut client, "demo unlearn", Cheat::UnlearnAbilities);
    client.until(
        "the first spell unlearned and its learn button back",
        |client| client.me().abilities[0].level == 0 && level_plate(client, "hud level up 0"),
    );
    assert_eq!(client.me().level, bota_core::rules::HERO_MAX_LEVEL);
}

#[test]
fn demo_gold_orders_credit_the_hero_and_update_the_hud() {
    let (_server, mut client) = demo();
    let hero = spawn(&mut client, false);
    select(&mut client, hero);
    let slot = client
        .view()
        .unit(hero)
        .and_then(|unit| unit.owner)
        .expect("the created seat");
    let balance = |client: &Client| {
        client
            .view()
            .player(slot)
            .and_then(|player| player.gold)
            .expect("own gold")
    };
    for amount in [1000, 10_000] {
        let before = balance(&client);
        hero_cheat(
            &mut client,
            &format!("demo gold({amount})"),
            Cheat::Gold { amount },
        );
        client.until("the credited balance in the HUD", |client| {
            let gold = balance(client);
            gold >= before + amount && client.shown_texts().contains(&gold.to_string())
        });
    }
}

#[test]
fn demo_free_items_go_into_the_selected_enemy_heros_inventory() {
    let (_server, mut client) = demo();
    let hero = spawn(&mut client, true);
    select(&mut client, hero);
    client.click_named("demo items");
    client.type_text("blink dagger");
    let item = ItemId::BLINK_DAGGER;
    hero_cheat(
        &mut client,
        &format!("demo item {}", item.0),
        Cheat::Item { item },
    );
    client.until(
        "the enemy's dagger in the world and HUD inventory",
        |client| {
            client
                .view()
                .unit(hero)
                .is_some_and(|unit| unit.items.iter().flatten().any(|held| held.id == item))
                && client
                    .slot_of(item)
                    .and_then(|slot| client.item_box(slot))
                    .is_some()
        },
    );
    assert!(
        !client
            .me()
            .items
            .iter()
            .flatten()
            .any(|held| held.id == item)
    );
}

#[test]
fn demo_spawn_ally_places_the_chosen_hero_at_the_cursor() {
    let (_server, mut client) = demo();
    spawn(&mut client, false);
}

#[test]
fn demo_spawn_enemy_places_the_chosen_hero_and_remove_despawns_it() {
    let (_server, mut client) = demo();
    let hero = spawn(&mut client, true);
    select(&mut client, hero);
    hero_cheat(&mut client, "demo remove", Cheat::RemoveHero);
    client.until(
        "the former enemy gone from the world and the screen",
        |client| client.view().unit(hero).is_none() && client.body(hero).is_none(),
    );
}

#[test]
fn demo_move_here_teleports_the_selected_enemy_instead_of_the_players_hero() {
    let (_server, mut client) = demo();
    let hero = spawn(&mut client, true);
    select(&mut client, hero);
    let start = client.me().pos;
    let at = nearby(&client, 160, 220);
    client.click_named("demo teleport");
    point(&mut client, at, MouseButton::Left);
    assert!(
        matches!(sent(&client), (Some(id), Order::Cheat { cheat: Cheat::Teleport { .. } }) if id == hero)
    );
    client.until("the teleported enemy drawn at the new point", |client| {
        client.view().unit(hero).is_some_and(|unit| {
            (unit.pos.x.to_f32() - at.x.to_f32()).abs() < 45.0
                && (unit.pos.y.to_f32() - at.y.to_f32()).abs() < 45.0
        }) && client.body(hero).is_some()
    });
    assert_eq!(client.me().pos, start);
}

#[test]
fn demo_respawn_stands_the_fallen_hero_back_up() {
    let (_server, mut client) = demo();
    let hero = client.me().id;
    client.cheat(Cheat::Suicide);
    client.until("the hero dead", |client| {
        client.core().my_standing_hero().is_none()
    });
    hero_cheat(&mut client, "demo respawn", Cheat::Respawn);
    client.until("the hero is standing and drawn", |client| {
        client.core().my_standing_hero() == Some(hero) && client.body(hero).is_some()
    });
}

#[test]
fn demo_refresh_restores_the_pools_drawn_in_the_hud() {
    let (_server, mut client) = demo();
    client.cheat(Cheat::SetPools {
        health: 20,
        mana: 5,
    });
    assert!(client.me().mana < client.me().max_mana);
    click_cheat(&mut client, "demo refresh", None, Cheat::Refresh);
    client.until("full health and mana drawn", |client| {
        let unit = client.me();
        let full = format!("{} / {}", unit.max_mana, unit.max_mana);
        unit.hp == unit.max_hp && unit.mana == unit.max_mana && client.shown_texts().contains(&full)
    });
}

#[test]
fn demo_invulnerability_prevents_damage_until_the_vulnerable_button() {
    let (_server, mut client) = demo();
    let protected = |client: &Client| client.me().statuses.bits & StatusFlags::INVULNERABLE != 0;
    hero_cheat(
        &mut client,
        "demo invulnerable on",
        Cheat::Invulnerable { on: true },
    );
    client.until("the hero reported as invulnerable", |client| {
        protected(client)
    });
    let damage = Cheat::Damage {
        target: Target::None,
        amount: 200,
        kind: bota_proto::DamageKind::Pure,
    };
    client.cheat(damage);
    assert_eq!(client.me().hp, client.me().max_hp);
    hero_cheat(
        &mut client,
        "demo invulnerable off",
        Cheat::Invulnerable { on: false },
    );
    client.until("the hero reported as vulnerable", |client| {
        !protected(client)
    });
    client.cheat(damage);
    client.until("the damage reflected on the HUD", |client| {
        let hero = client.me();
        let hp = format!("{} / {}", hero.hp, hero.max_hp);
        hero.hp < hero.max_hp && client.shown_texts().contains(&hp)
    });
}

#[test]
fn demo_all_vision_shows_an_enemy_outside_the_players_sight() {
    let (_server, mut client) = demo();
    let at = bota_core::rules::DEMO_DIRE_CREEP_SPAWN;
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: true,
        at,
    });
    assert!(created(&client, Team::Dire).is_none());
    click_cheat(
        &mut client,
        "demo all vision",
        None,
        Cheat::AllVision { on: true },
    );
    client.until("the far enemy visible and drawn", |client| {
        created(client, Team::Dire).is_some_and(|unit| client.body(unit.id).is_some())
    });
    click_cheat(
        &mut client,
        "demo normal vision",
        None,
        Cheat::AllVision { on: false },
    );
    client.until("the far enemy hidden again", |client| {
        created(client, Team::Dire).is_none()
    });
}

#[test]
fn demo_uses_the_short_lane_and_draws_roshan() {
    let (_server, mut client) = demo();
    click_cheat(
        &mut client,
        "demo all vision",
        None,
        Cheat::AllVision { on: true },
    );
    client.within(
        std::time::Duration::from_secs(25),
        "both waves drawn",
        |client| {
            [Team::Radiant, Team::Dire].into_iter().all(|team| {
                client.view().units.iter().any(|unit| {
                    unit.kind == UnitKind::CreepMelee
                        && unit.team == team
                        && client.body(unit.id).is_some()
                })
            })
        },
    );
    client.cheat(Cheat::Teleport {
        at: At::from_ints(5800, 9300),
    });
    client.within(
        std::time::Duration::from_secs(25),
        "Roshan drawn in his pit",
        |client| {
            client
                .view()
                .units
                .iter()
                .any(|unit| unit.kind == UnitKind::Roshan && client.body(unit.id).is_some())
        },
    );
    let world = client.world_mut();
    assert!(
        world
            .query::<&Name>()
            .iter(world)
            .any(|name| name.as_str() == "sandbox terrain"),
        "the short lane's own terrain is drawn",
    );
}

#[test]
fn demo_a_spawned_enemy_takes_the_players_normal_move_and_ability_orders() {
    let (_server, mut client) = demo();
    let hero = spawn(&mut client, true);
    let body_at = client.view().unit(hero).expect("spawned hero").pos;
    point(&mut client, body_at, MouseButton::Left);
    assert_eq!(
        client.core().selected,
        Some(hero),
        "clicking the enemy selects it"
    );

    let own = client.me();
    let (own_at, own_id) = (own.pos, own.id);
    point(&mut client, own_at, MouseButton::Right);
    assert_eq!(
        sent(&client),
        (
            Some(hero),
            Order::Attack {
                target: Target::Unit(own_id)
            }
        )
    );

    let walk = nearby(&client, 270, 120);
    point(&mut client, walk, MouseButton::Right);
    assert!(matches!(sent(&client), (Some(id), Order::Move { .. }) if id == hero));
    client.until("the selected enemy walks toward the order", |client| {
        client.view().unit(hero).is_some_and(|unit| {
            (unit.pos.x.to_f32() - walk.x.to_f32()).abs() < 45.0
                && (unit.pos.y.to_f32() - walk.y.to_f32()).abs() < 45.0
        })
    });

    hero_cheat(&mut client, "demo learn max", Cheat::MaxAbilities);
    client.until("Meat Hook learned", |client| {
        client
            .view()
            .unit(hero)
            .is_some_and(|unit| unit.abilities[0].level > 0)
    });
    client.tap_named(
        KeyCode::KeyQ,
        bevy::input::keyboard::Key::Character("q".into()),
    );
    let aim = nearby(&client, 0, -180);
    point(&mut client, aim, MouseButton::Left);
    assert!(matches!(sent(&client), (Some(id), Order::Cast { .. }) if id == hero));
    client.until(
        "the enemy used mana and put Meat Hook on cooldown",
        |client| {
            client.view().unit(hero).is_some_and(|unit| {
                unit.abilities[0].cooldown_left > 0 && unit.mana < unit.max_mana
            })
        },
    );
}

#[test]
fn demo_controls_are_offered_only_in_live_cheat_matches() {
    for cheats in [false, true] {
        let server = Server::for_hud_with_cheats(2, &[], cheats);
        quiet_seat(&server, HeroId::PUDGE);
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
        client.ready_with_match_held(&server);
    client.frames(2);
        assert_eq!(level_plate(&mut client, "demo fold"), cheats);
    }
}

#[test]
fn demo_a_creep_wave_and_creeps_here_draw_new_units() {
    let (_server, mut client) = demo();
    client.cheat(Cheat::AllVision { on: true });
    click_cheat(
        &mut client,
        "demo creeps off",
        None,
        Cheat::CreepSpawning { on: false },
    );
    client.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::All,
    });
    let creeps = |client: &Client| {
        client
            .view()
            .units
            .iter()
            .filter(|unit| unit.kind == UnitKind::CreepMelee && client.body(unit.id).is_some())
            .count()
    };
    client.until("the lane emptied", |client| creeps(client) == 0);
    click_cheat(&mut client, "demo creepwave", None, Cheat::SpawnCreeps);
    client.until("the new lane wave drawn", |client| creeps(client) > 0);
    let count = creeps(&client);
    let at = nearby(&client, 200, -80);
    client.click_named("demo creepsat");
    point(&mut client, at, MouseButton::Left);
    assert!(matches!(
        sent(&client),
        (
            None,
            Order::Cheat {
                cheat: Cheat::CreateCreeps { enemy: true, .. }
            }
        )
    ));
    client.until("the new creep group drawn", |client| creeps(client) > count);
    click_cheat(
        &mut client,
        "demo creeps on",
        None,
        Cheat::CreepSpawning { on: true },
    );
}

#[test]
fn demo_day_and_night_switch_the_game_clock_and_its_light() {
    let (_server, mut client) = demo();
    for (button, night) in [("demo night", true), ("demo day", false)] {
        click_cheat(&mut client, button, None, Cheat::TimeOfDay { night });
        client.until("the daylight state updated", |client| {
            crate::daylight::night(client.core()) == Some(night)
        });
    }
}

#[test]
fn demo_the_normal_shop_buys_for_the_selected_enemy() {
    let (_server, mut client) = demo();
    let hero = spawn(&mut client, true);
    client.cheat(Cheat::AllVision { on: true });
    let map = bota_core::map_of(bota_core::HERO_DEMO_ID);
    let home = bota_core::hero_spawn_pos(map, Team::Dire);
    client.core_mut().send_order_to(
        Some(hero),
        Order::Cheat {
            cheat: Cheat::Teleport { at: home },
        },
    );
    client.within(
        std::time::Duration::from_secs(20),
        "the enemy reaches its shop",
        |client| client.core().unit_at_home_shop(hero),
    );
    select(&mut client, hero);
    client.tap_named(KeyCode::F4, bevy::input::keyboard::Key::F4);
    client.within(
        std::time::Duration::from_secs(20),
        "the shop drawn",
        |client| level_plate(client, "shop panel"),
    );
    let item = ItemId::BLINK_DAGGER;
    client.core_mut().act(Intent::Buy(item.0));
    assert_eq!(sent(&client), (Some(hero), Order::Buy { item }));
    client.within(
        std::time::Duration::from_secs(20),
        "the selected hero's shop purchase in the inventory",
        |client| {
            client
                .view()
                .unit(hero)
                .is_some_and(|unit| unit.items.iter().flatten().any(|held| held.id == item))
                && client.slot_of(item).is_some()
        },
    );
}

#[test]
fn demo_respawns_a_fallen_hero_where_the_hud_last_showed_it() {
    let (_server, mut client) = demo();
    let at = client.me().pos;
    let id = client.me().id;
    client.cheat(Cheat::Suicide);
    client.until("the hero down", |client| {
        client.core().my_standing_hero().is_none()
    });
    client.until("the same hero drawn again where it died", |client| {
        client.view().unit(id).is_some_and(|unit| unit.pos == at) && client.body(id).is_some()
    });
}

#[test]
fn demo_neutrals_fills_an_empty_camp_and_draws_it() {
    let (_server, mut client) = demo();
    client.cheat(Cheat::Teleport {
        at: At::from_ints(8350, 9952),
    });
    client.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::Neutral,
    });
    let drawn = |client: &Client| {
        client
            .view()
            .units
            .iter()
            .any(|unit| unit.kind == UnitKind::CreepNeutral && client.body(unit.id).is_some())
    };
    client.until("the neutral camp cleared", |client| !drawn(client));
    click_cheat(&mut client, "demo neutrals", None, Cheat::SpawnNeutrals);
    client.within(
        std::time::Duration::from_secs(25),
        "the neutral camp drawn",
        |client| drawn(client),
    );
}

#[test]
fn demo_free_spells_and_normal_spells_change_the_cooldown_shown() {
    let (_server, mut client) = demo();
    client.cheat(Cheat::MaxAbilities);
    click_cheat(
        &mut client,
        "demo free spells",
        None,
        Cheat::Wtf { on: true },
    );
    let mana = client.me().mana;
    let cast = |client: &mut Client| {
        client.tap_named(
            KeyCode::KeyQ,
            bevy::input::keyboard::Key::Character("q".into()),
        );
        assert!(matches!(sent(client), (None, Order::Cast { .. })));
        let tick = client.tick();
        client.until("the cast finishes", |client| {
            client.tick() > tick && client.me().cast.is_none()
        });
    };
    cast(&mut client);
    assert_eq!(client.me().mana, mana);
    assert_eq!(client.me().abilities[0].cooldown_left, 0);
    click_cheat(
        &mut client,
        "demo normal spells",
        None,
        Cheat::Wtf { on: false },
    );
    cast(&mut client);
    client.until(
        "the normal spell spent mana and started its cooldown",
        |client| client.me().mana < mana && client.me().abilities[0].cooldown_left > 0,
    );
}
