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
fn demo_level_max_learns_every_ability_and_leaves_talents_to_the_player() {
    let (_server, mut client) = demo();
    let hero = client.me().id;
    let count = bota_core::rules::HERO_MAX_LEVEL;
    let slot = client.core().my_slot.expect("the seat");
    let talents = client
        .core()
        .talents_of(slot)
        .expect("talent alternatives")
        .learned;
    client.click_named("demo levelmax");
    expect_level(&mut client, hero, count);
    let seen = client.tick();
    client.until(
        "the level change and any following order have reached the client",
        |c| c.tick() > seen + 2,
    );
    let abilities = &client.me().abilities;
    assert!(
        abilities
            .iter()
            .filter(|spell| spell.max_level > 0
                && bota_core::ability_def(spell.id).is_some_and(|def| def.granted_at.is_none()))
            .all(|spell| spell.level == spell.max_level),
        "Level Max learns each regular ability: {abilities:?}"
    );
    assert_eq!(
        client.core().talents_of(slot).unwrap().learned,
        talents,
        "Level Max leaves talent choices to the player"
    );
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
    expect_demo_world(&mut client);
}

fn drawn_ground(client: &Client, id: EntityId) -> Option<Vec2> {
    let body = client.body(id)?;
    let place = client.world().get::<GlobalTransform>(body)?.translation();
    Some(Vec2::new(place.x, -place.z))
}

fn expect_demo_world(client: &mut Client) {
    let world = client.world_mut();
    assert!(
        world
            .query::<(&Name, &Mesh3d)>()
            .iter(world)
            .any(|(name, _)| name.as_str() == "sandbox terrain"),
        "the short lane's own terrain mesh is drawn",
    );

    click_cheat(
        client,
        "demo all vision",
        None,
        Cheat::AllVision { on: true },
    );
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::All,
    });
    click_cheat(client, "demo creepwave", None, Cheat::SpawnCreeps);
    use bota_core::rules::{DEMO_DIRE_CREEP_SPAWN, DEMO_RADIANT_CREEP_SPAWN, DEMO_ROSHAN_PIT};
    let near = |client: &Client, id, at: At| {
        let at = Vec2::new(at.x.to_f32(), at.y.to_f32());
        drawn_ground(client, id).is_some_and(|position| position.distance(at) < 450.0)
    };
    client.until("one wave drawn at each end of the short lane", |client| {
        [
            (Team::Radiant, DEMO_RADIANT_CREEP_SPAWN),
            (Team::Dire, DEMO_DIRE_CREEP_SPAWN),
        ]
        .into_iter()
        .all(|(team, at)| {
            client.view().units.iter().any(|unit| {
                unit.kind == UnitKind::CreepMelee && unit.team == team && near(client, unit.id, at)
            })
        })
    });
    client.cheat(Cheat::Teleport {
        at: At::from_ints(5800, 9300),
    });
    client.until("Roshan drawn in the demo pit", |client| {
        client
            .view()
            .units
            .iter()
            .any(|unit| unit.kind == UnitKind::Roshan && near(client, unit.id, DEMO_ROSHAN_PIT))
    });
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
fn demo_controls_are_offered_only_when_the_match_allows_cheats() {
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
fn the_demo_places_allied_creeps_at_the_clicked_point() {
    let (_server, mut client) = demo();
    let before: Vec<_> = client.view().units.iter().map(|unit| unit.id).collect();
    let team = client.me().team;
    let at = nearby(&client, 220, -80);
    client.click_named("demo allied creeps");
    point(&mut client, at, MouseButton::Left);
    assert!(matches!(
        sent(&client),
        (
            None,
            Order::Cheat {
                cheat: Cheat::CreateCreeps { enemy: false, .. }
            }
        )
    ));
    client.until("the allied creep drawn at the placement point", |client| {
        client.view().units.iter().any(|unit| {
            !before.contains(&unit.id)
                && unit.kind == UnitKind::CreepMelee
                && unit.team == team
                && unit.pos.within(at, bota_proto::Fixed::from_int(160))
                && client.body(unit.id).is_some()
        })
    });
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

#[test]
fn clicking_the_ground_after_search_restores_game_hotkeys() {
    let (_server, mut client) = demo();
    client.click_named("demo learn max");
    client.click_named("demo items");
    client.type_text("heart");
    let at = nearby(&client, 200, -80);
    point(&mut client, at, MouseButton::Left);
    client.tap_named(KeyCode::F4, bevy::input::keyboard::Key::F4);
    assert!(
        client.world().resource::<crate::input::Panels>().shop,
        "the shop hotkey works after clicking outside search"
    );
}

#[test]
fn the_hero_controls_explain_when_the_selection_cannot_be_controlled() {
    let (_server, mut client) = demo();
    let unit = client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Tower)
        .expect("tower")
        .id;
    select(&mut client, unit);
    let world = client.world_mut();
    let cell = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "demo levelup")
        .unwrap()
        .0;
    assert!(
        world.get::<bevy::ui::InteractionDisabled>(cell).is_some(),
        "the level button is disabled for a tower"
    );
    assert!(
        world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.0 == "ENEMY CREEPS HERE"),
        "placement identifies the side it spawns"
    );
}

fn forged_cheat(client: &mut Client, unit: EntityId) {
    let seen = client.tick();
    client
        .core_mut()
        .source
        .send(&bota_proto::ClientMsg::Order {
            seq: 1000,
            unit: Some(unit),
            order: Order::Cheat {
                cheat: Cheat::Levels { count: 29 },
            },
            seen: Some(seen),
            queue: false,
        });
}

#[test]
fn a_spectator_of_a_cheat_match_sees_no_demo_panel_and_a_forged_cheat_changes_nothing() {
    let server = Server::start(1, &["--spectators", "open"]);
    let mut player = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let mut watcher = Client::run(&["--addr", &server.address, "--spectate"]);
    watcher.until("spectator watches the match", |watcher| {
        player.frames(1);
        watcher.core().view().is_some()
    });
    let id = player.me().id;
    let tick = watcher.tick();
    assert!(
        !level_plate(&mut watcher, "demo fold"),
        "the spectator's panel"
    );
    forged_cheat(&mut watcher, id);
    watcher.until("both sides saw later snapshots", |watcher| {
        player.frames(1);
        watcher.tick() > tick + 2 && player.tick() > tick + 2
    });
    assert_eq!(player.me().level, 1, "the seatless order had no effect");
}

#[test]
fn a_replay_of_a_cheat_match_shows_no_demo_panel() {
    use bota_proto::{ReplayRecord, ServerMsg};
    let mut reader = bota_proto::FrameReader::new();
    reader.push(&crate::units::tests::replay_of_heroes_on_grid(1, 1));
    let mut replay = Vec::new();
    while let Some(mut record) = reader
        .next_message::<ReplayRecord>()
        .expect("the replay record")
    {
        if let ReplayRecord::Msg(ServerMsg::MatchStart { info }) = &mut record {
            info.cheats = true;
        }
        bota_proto::encode_frame(&record, &mut replay).expect("the replay record encoded");
    }
    let core = bota_client_core::App::new(bota_client_core::Source::Replay(Box::new(
        bota_client_core::ReplayPlayer::from_reader(std::io::Cursor::new(replay)),
    )));
    let mut client = Client::run_with_core(&["--replay", "in_memory.brp"], core);
    client.until("the replay snapshot", |client| {
        client.core().view().is_some()
    });
    super::hud_layout::on_screen(&mut client);
    assert!(
        client
            .core()
            .statics()
            .is_some_and(|statics| statics.metadata().cheats),
        "the recorded match allows cheats"
    );
    assert!(!level_plate(&mut client, "demo fold"), "the replay's panel");
}

#[test]
fn the_ended_demo_match_shows_no_demo_panel() {
    let (mut server, mut client) = demo();
    assert!(
        level_plate(&mut client, "demo fold"),
        "the panel while it runs"
    );
    server.ask_to_finish();
    client.until("the match has ended on the client", |client| {
        client.core().over.is_some()
    });
    server.wait();
    client.frames(2);
    assert!(
        !level_plate(&mut client, "demo fold"),
        "the ended match's panel"
    );
}

#[test]
fn escape_hides_the_search_caret_and_restores_hotkeys() {
    let (_server, mut client) = demo();
    client.click_named("demo items");
    client.type_text("heart");
    assert!(
        client.shown_texts().contains(&"heart|".to_owned()),
        "the focused field shows its caret"
    );
    client.tap_named(KeyCode::Escape, bevy::input::keyboard::Key::Escape);
    assert!(client.shown_texts().contains(&"heart".to_owned()));
    assert!(!client.shown_texts().contains(&"heart|".to_owned()));
    client.tap_named(KeyCode::F4, bevy::input::keyboard::Key::F4);
    assert!(client.world().resource::<crate::input::Panels>().shop);
}

#[test]
fn the_panel_takes_its_top_edge_from_the_tally() {
    let (_server, mut client) = demo();
    let world = client.world_mut();
    let named = |world: &mut World, label| {
        world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, name)| name.as_str() == label)
            .unwrap()
            .0
    };
    let (panel, tally) = (named(world, "demo controls"), named(world, "hud tally"));
    let Val::Px(top) = world.get::<Node>(panel).unwrap().top else {
        panic!("panel top")
    };
    let mut tally = world.get_mut::<Node>(tally).unwrap();
    let Val::Px(height) = tally.height else {
        panic!("tally height")
    };
    tally.height = px(height + 24.0);
    client.frames(2);
    assert_eq!(
        client.world().get::<Node>(panel).unwrap().top,
        px(top + 24.0)
    );
}

#[test]
fn an_armed_teleport_gives_a_short_instruction_and_escape_cancels_it() {
    let (_server, mut client) = demo();
    client.click_named("demo teleport");
    assert!(
        client.shown_texts().contains(&"PICK SPOT".to_owned()),
        "the armed button uses a caption that fits its cell"
    );
    assert!(
        client
            .shown_texts()
            .contains(&"CLICK GROUND · ESC CANCELS".to_owned())
    );
    client.tap_named(KeyCode::Escape, bevy::input::keyboard::Key::Escape);
    assert!(!client.shown_texts().contains(&"PICK SPOT".to_owned()));
}

fn launch_demo_from_example() -> (crate::main_menu::tests::Service, Client) {
    use crate::main_menu::{Act, MenuCore};
    let service = crate::main_menu::tests::service_from_demo_example();
    let mut client = super::menu::signed_up(&service, "demoplayer");
    super::menu::act(&mut client, Act::InspectHero(HeroId::SHADOW_FIEND));
    client.click_named("demo hero");
    client.until("Demo Hero seated by the matchmaking service", |c| {
        c.core().my_standing_hero().is_some()
    });
    assert_eq!(
        client.world().non_send::<MenuCore>().demo_hero(),
        Some(HeroId::SHADOW_FIEND)
    );
    super::hud_layout::on_screen(&mut client);
    (service, client)
}

// Guards the shipped example's map selection. A deployment supplies its own launcher arguments.
#[test]
fn the_demo_button_draws_the_short_lane_and_roshan_from_the_shipped_launcher() {
    let (_service, mut client) = launch_demo_from_example();
    expect_demo_world(&mut client);
    assert_eq!(
        client.core().session().expect("service match").map().id,
        bota_core::HERO_DEMO_ID
    );
}

#[test]
fn an_enemy_spawned_from_the_demo_button_rises_at_its_drawn_death_spot() {
    let (_service, mut client) = launch_demo_from_example();
    client.cheat(Cheat::AllVision { on: true });
    let id = spawn(&mut client, true);
    select(&mut client, id);
    let slot = client
        .view()
        .unit(id)
        .expect("created enemy")
        .owner
        .unwrap();
    let alive = |client: &Client| client.view().player(slot).unwrap().respawn_left == 0;
    let fell = drawn_ground(&client, id).expect("the enemy is drawn before death");
    client.cheat(Cheat::Suicide);
    client.until(
        "the selected enemy died and its body stopped drawing",
        |client| !alive(client) && client.body(id).is_none(),
    );
    client.until("the same enemy is alive and drawn again", |client| {
        alive(client) && drawn_ground(client, id).is_some()
    });
    let risen = drawn_ground(&client, id).unwrap();
    assert!(
        risen.distance(fell) < 2.0,
        "the revived enemy is drawn where it died: {fell:?} -> {risen:?}"
    );
}
