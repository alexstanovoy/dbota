//! Credit and assist gold are shown for participation even after leaving the fight.
//! https://liquipedia.net/dota2/Gold

use super::harness::{Client, MID, Server, scripted_seat};
use bevy::{input::ButtonState, prelude::*};
use bota_client_core::{Intent, IssuedOrder, Slot};
use bota_proto::{
    Cheat, CreepSide, EntityId, HeroId, ModifierSpec, Order, Target, Team, UnitKind, Vec2 as At,
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

const SAFE: At = At::from_ints(8300, 9600);
const FAR: (i32, i32) = (6000, 9600);

fn wallet(client: &Client) -> i32 {
    client
        .view()
        .player(client.core().my_slot.unwrap())
        .unwrap()
        .gold
        .unwrap()
}

fn passive(client: &Client) -> i32 {
    bota_core::passive_gold_by(
        client
            .view()
            .game_tick()
            .saturating_sub(client.core().pregame_ticks()),
    )
}

fn drawn_gold(client: &mut Client) -> Option<i32> {
    crate::shop::drawn_gold(client.world_mut())?.parse().ok()
}

fn victim(client: &Client) -> EntityId {
    client
        .view()
        .players
        .iter()
        .find(|p| p.team != client.me().team)
        .unwrap()
        .unit
        .unwrap()
}

fn add_victim_peer(server: &Server, phase: Arc<AtomicU8>) {
    let follow = phase;
    let tower = bota_core::rules::DIRE_TOWERS
        .iter()
        .find(|(lane, tier, _)| *lane == bota_core::rules::LANE_MID && *tier == 1)
        .unwrap()
        .2;
    let mut applied = None;
    scripted_seat(server, HeroId::PUDGE, move |session| {
        let next = follow.load(Ordering::SeqCst);
        if applied == Some(next) || session.own_hero().is_none() {
            return Vec::new();
        }
        applied = Some(next);
        if next == 0 {
            return vec![IssuedOrder::hero(Order::Move {
                target: Target::None,
            })];
        }
        let at = if next == 1 {
            SAFE
        } else {
            tower + At::from_ints(-400, 0)
        };
        [
            Order::Cheat {
                cheat: Cheat::Teleport { at },
            },
            Order::Cheat {
                cheat: Cheat::ApplyModifier {
                    target: Target::None,
                    spec: ModifierSpec {
                        max_hp: ModifierSpec::MIN_SCALE,
                        ..ModifierSpec::NOMINAL
                    },
                    ticks: 10_000,
                },
            },
            Order::Move {
                target: Target::None,
            },
        ]
        .into_iter()
        .map(IssuedOrder::hero)
        .collect()
    });
}

fn scene() -> (Server, Client, EntityId, Arc<AtomicU8>) {
    scene_with_enemy_ally(false)
}

fn scene_with_enemy_ally(extra_enemy: bool) -> (Server, Client, EntityId, Arc<AtomicU8>) {
    scene_with_hero(extra_enemy, HeroId::SHADOW_FIEND)
}

fn scene_with_hero(extra_enemy: bool, hero: HeroId) -> (Server, Client, EntityId, Arc<AtomicU8>) {
    let server = Server::start(
        if extra_enemy { 3 } else { 2 },
        &["--map", "0", "--mode", "lockstep", "--runes", "none"],
    );
    let phase = Arc::new(AtomicU8::new(0));
    add_victim_peer(&server, phase.clone());
    let mut client = if extra_enemy {
        let mut client = Client::run(&["--addr", &server.address]);
        client.until("the earning seat assigned", |c| c.core().my_slot.is_some());
        assert_eq!(client.core().my_slot, Some(bota_proto::SlotId(1)));
        client.core_mut().act(Intent::PickHero(hero.0));
        client.core_mut().act(Intent::ToggleReady);
        super::harness::quiet_seat(&server, HeroId::PUDGE);
        client.until("the earning hero on the map", |c| c.core().ears().is_some());
        client
    } else {
        Client::join(&server, hero, &[])
    };
    assert_eq!(client.me().team, Team::Dire);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::AllVision { on: true });
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    client.core_mut().act(Intent::Stop);
    client.cheat(Cheat::Refresh);
    client.until("the wallet label drawn before the fight", |c| {
        let expected = wallet(c);
        drawn_gold(c) == Some(expected)
    });
    let initial_max = client.view().unit(victim(&client)).unwrap().max_hp;
    phase.store(1, Ordering::SeqCst);
    let setup_tick = client.tick();
    client.until("the victim ready outside tower range", |c| {
        let id = victim(c);
        assert!(
            c.tick().saturating_sub(setup_tick) < 60,
            "victim setup state {:?}",
            c.view().unit(id).map(|u| (u.pos, u.hp, u.max_hp))
        );
        c.view().unit(id).is_some_and(|u| {
            u.max_hp < initial_max && u.pos.within(SAFE, bota_proto::Fixed::from_int(100))
        })
    });
    client.cheat(Cheat::Refresh);
    let foe = victim(&client);
    (server, client, foe, phase)
}

fn one_attack(client: &mut Client, foe: EntityId) {
    let source = client.me().id;
    let at = client.view().unit(foe).unwrap().pos;
    client.core_mut().act(Intent::AttackAt {
        at: (at.x.to_f32(), at.y.to_f32()),
        unit: Some(foe),
    });
    client.until("the player's attack damages the victim", |c| {
        c.core().told.iter().any(|event| matches!(event, bota_proto::EventKind::Damaged { source: Some(by), target, amount, .. } if *by == source && *target == foe && *amount > 0))
    });
    client.core_mut().act(Intent::Stop);
}

fn retreat(client: &mut Client) {
    let id = client.me().id;
    client.core_mut().act(Intent::Stop);
    client.cheat(Cheat::Teleport {
        at: At::from_ints(FAR.0, FAR.1),
    });
    let start = client.tick();
    client.until("the retreat teleport observed by the server", |c| {
        assert!(
            c.tick() - start < 60,
            "retreat state {:?}, heroes {:?}",
            c.view().unit(id).map(|u| (u.pos, u.hp)),
            c.view().players
        );
        c.view().unit(id).is_some_and(|u| {
            u.pos
                .within(At::from_ints(FAR.0, FAR.1), bota_proto::Fixed::from_int(50))
        })
    });
}

fn wait_for_death(client: &mut Client) {
    let start = client.tick();
    client.until("the finishing blow within the credit window", |c| {
        assert!(
            c.tick() - start < 15 * bota_core::rules::TICKS_PER_SECOND,
            "the fixture must finish the victim within fifteen seconds"
        );
        c.view()
            .players
            .iter()
            .any(|p| p.team != c.me().team && p.deaths == 1)
    });
}

fn assert_wallet_drawn(server: &Server, client: &mut Client) {
    server.frozen(|| {
        client.until("the credited gold shown in the wallet", |c| {
            let expected = wallet(c);
            drawn_gold(c) == Some(expected)
        })
    });
}

#[test]
fn a_tower_finish_credits_the_distant_damager_in_the_drawn_score_and_wallet() {
    let (server, mut client, foe, phase) = scene();
    let souls = client
        .me()
        .effects
        .iter()
        .find(|effect| effect.id == bota_proto::EffectId::SOULS)
        .and_then(|effect| effect.stacks)
        .unwrap_or(0);
    one_attack(&mut client, foe);
    retreat(&mut client);
    let before = wallet(&client) - passive(&client);
    phase.store(2, Ordering::SeqCst);
    wait_for_death(&mut client);
    let me = client
        .view()
        .player(client.core().my_slot.unwrap())
        .unwrap();
    assert_eq!(
        me.kills, 1,
        "the tower's finish credits the sole recent enemy damager"
    );
    assert_eq!(me.assists, 0, "a credited killer is not its own assister");
    assert!(
        wallet(&client) - passive(&client) - before > 268,
        "kill bounty and assist gold both reach the distant killer"
    );
    assert_eq!(
        client
            .me()
            .effects
            .iter()
            .find(|effect| effect.id == bota_proto::EffectId::SOULS)
            .and_then(|effect| effect.stacks)
            .unwrap_or(0),
        souls,
        "a credited tower finish grants no Necromastery souls"
    );
    let credit = client
        .core()
        .feed
        .iter()
        .find_map(|line| match &line.told {
            Some(bota_proto::EventKind::Died {
                unit,
                killer,
                credited_hero,
                ..
            }) if *unit == foe => Some((*killer, credited_hero.as_deref().copied())),
            _ => None,
        })
        .expect("the hero death is reported");
    assert_eq!(
        credit.1,
        Some(client.me().id),
        "the wire names the credited hero separately"
    );
    let physical = credit.0.expect("AllVision reveals the finishing tower");
    assert_eq!(
        client.view().unit(physical).unwrap().kind,
        UnitKind::Tower,
        "the physical killing blow remains the tower's"
    );
    assert!(client.core().feed.iter().any(|line| matches!(line.kind, bota_client_core::FeedKind::HeroDied { killer: Some(bota_client_core::Killer::Seat(slot)), .. } if Some(slot) == client.core().my_slot)), "the drawn feed names the hero receiving credit");
    assert_wallet_drawn(&server, &mut client);
    client.key(KeyCode::Backquote, ButtonState::Pressed);
    server.frozen(|| {
        client.until("the credited kill drawn on the scoreboard", |c| {
            let team = c.me().team;
            let world = c.world_mut();
            world
                .query::<(&crate::scoreboard::TeamScore, &Text, &InheritedVisibility)>()
                .iter(world)
                .any(|(score, text, shown)| score.0 == team && shown.get() && text.0 == "1")
        })
    });
    client.key(KeyCode::Backquote, ButtonState::Released);
}

#[derive(Clone, Copy)]
enum Participation {
    Damage,
    Debuff,
    None,
}

fn participate(client: &mut Client, foe: EntityId, kind: Participation) {
    match kind {
        Participation::Damage => one_attack(client, foe),
        Participation::Debuff => {
            client.core_mut().act(Intent::Press {
                slot: Slot::Ability(4),
                ctrl: true,
            });
            client.until(
                "Presence applies its armor debuff without damaging the victim",
                |c| {
                    let target = c.view().unit(foe).unwrap();
                    let applied = target.effects.iter().any(|e| {
                        e.caster == Some(c.me().id)
                            && bota_core::effect_def(e.id).is_some_and(|def| def.harmful)
                    });
                    assert_eq!(
                        target.hp, target.max_hp,
                        "the participant supplies only a debuff"
                    );
                    applied
                },
            );
        }
        Participation::None => {}
    }
}

fn finishing_ally(client: &mut Client, foe: EntityId) -> At {
    allied_hero(client, foe, ModifierSpec::MAX_SCALE).0
}

fn allied_hero(client: &mut Client, foe: EntityId, damage_scale: i32) -> (At, EntityId) {
    let at = client.view().unit(foe).unwrap().pos + At::from_ints(0, 100);
    assert!(
        !client
            .me()
            .pos
            .within(at, bota_proto::Fixed::from_int(1500)),
        "the participant starts outside assist proximity"
    );
    client.core_mut().act(Intent::Cheat(Cheat::CreateHero {
        hero: HeroId::SHADOW_FIEND,
        enemy: false,
        at,
    }));
    client.until("the finishing ally stands up", |c| {
        c.view()
            .units
            .iter()
            .any(|u| u.kind == UnitKind::Hero && u.team == c.me().team && u.id != c.me().id)
    });
    let killer = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.team == client.me().team && u.id != client.me().id)
        .unwrap()
        .id;
    client.cheat(Cheat::ApplyModifier {
        target: Target::Unit(killer),
        spec: ModifierSpec {
            physical_damage: damage_scale,
            move_speed: ModifierSpec::MIN_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: 10_000,
    });
    (at, killer)
}

fn assist_gold(kind: Participation) {
    assist_gold_with_enemy_ally(kind, false, 0)
}

fn assist_gold_with_enemy_ally(kind: Participation, extra_enemy: bool, minimum_reward: i32) {
    let (server, mut client, foe, _phase) = scene_with_enemy_ally(extra_enemy);
    participate(&mut client, foe, kind);
    retreat(&mut client);
    client.core_mut().act(Intent::Stop);
    let before = wallet(&client) - passive(&client);
    let at = finishing_ally(&mut client, foe);
    wait_for_death(&mut client);
    assert_eq!(
        client
            .view()
            .player(client.core().my_slot.unwrap())
            .unwrap()
            .kills,
        0,
        "the participant must not land the finishing blow"
    );
    assert!(
        !client
            .me()
            .pos
            .within(at, bota_proto::Fixed::from_int(1500)),
        "the participant remains outside assist proximity: {:?} versus {:?}",
        client.me().pos,
        at
    );
    let extra = wallet(&client) - passive(&client) - before;
    match kind {
        Participation::None => assert_eq!(
            extra, 0,
            "an uninvolved hero outside1500 gets no assist gold"
        ),
        _ => assert!(
            extra > minimum_reward,
            "a distant damage or debuff contributor receives assist gold: {extra}, minimum {minimum_reward}"
        ),
    }
    assert_wallet_drawn(&server, &mut client);
    server.frozen(|| match kind {
        Participation::None => assert!(
            crate::bars::drawn_gold_popups(client.world_mut()).is_empty(),
            "a nonparticipant gets no gold caption"
        ),
        _ => client.until(
            "the earned assist gold caption drawn over its recipient",
            |c| crate::bars::drawn_gold_popups(c.world_mut()) == vec![extra],
        ),
    });
}

#[test]
fn a_distant_damager_receives_assist_gold_in_the_drawn_wallet() {
    assist_gold(Participation::Damage);
}
#[test]
fn a_distant_debuffer_receives_assist_gold_in_the_drawn_wallet_without_dealing_damage() {
    assist_gold(Participation::Debuff);
}
#[test]
fn a_distant_nonparticipant_receives_only_passive_gold() {
    assist_gold(Participation::None);
}

#[test]
fn a_trailing_team_receives_ratio_scaled_assist_gold_in_the_drawn_wallet() {
    // Two real opposing seats have roughly twice the earning team's worth.
    // The published ratio pays about 102; the old bounded factor paid about 76.
    assist_gold_with_enemy_ally(Participation::Damage, true, 90);
}

fn shared_tower_finish(multiple: bool) {
    let (server, mut client, foe, phase) = scene();
    retreat(&mut client);
    if multiple {
        let (_, source) = allied_hero(&mut client, foe, 10_000);
        client.until("the second hero's real damage event", |c| c.core().told.iter().any(|event|
            matches!(event, bota_proto::EventKind::Damaged { source: Some(by), target, amount, .. } if *by == source && *target == foe && *amount > 0)));
        client.cheat(Cheat::Teleport {
            at: At::from_ints(MID.0, MID.1),
        });
        client.until("back in attack range", |c| {
            c.me()
                .pos
                .within(At::from_ints(MID.0, MID.1), bota_proto::Fixed::from_int(50))
        });
        one_attack(&mut client, foe);
        retreat(&mut client);
    }
    let before = wallet(&client) - passive(&client);
    let contact = client.tick();
    let tower_at = bota_core::rules::DIRE_TOWERS
        .iter()
        .find(|(lane, tier, _)| *lane == bota_core::rules::LANE_MID && *tier == 1)
        .unwrap()
        .2;
    let tower = client
        .view()
        .units
        .iter()
        .find(|unit| {
            unit.kind == UnitKind::Tower && unit.team == Team::Dire && unit.pos == tower_at
        })
        .expect("the finishing tower")
        .id;
    client.core_mut().pinned = Some(bota_client_core::Pin::Unit(tower));
    // Move the camera before the death. The match is held so this takes no
    // contribution time and the kill-bounty caption starts on screen.
    server.frozen(|| {
        client.until("the camera on the finishing tower", |c| {
            let world = c.world_mut();
            let camera = world
                .query::<&crate::camera::RtsCamera>()
                .single(world)
                .unwrap();
            Vec2::new(camera.focus.x, -camera.focus.z)
                .distance(Vec2::new(tower_at.x.to_f32(), tower_at.y.to_f32()))
                < 50.0
        })
    });
    phase.store(2, Ordering::SeqCst);
    wait_for_death(&mut client);
    let killer = client
        .core()
        .feed
        .iter()
        .find_map(|line| match &line.told {
            Some(bota_proto::EventKind::Died { unit, killer, .. }) if *unit == foe => *killer,
            _ => None,
        })
        .expect("the physical finisher is reported");
    assert_eq!(
        client.view().unit(killer).unwrap().kind,
        UnitKind::Tower,
        "the fixture is a tower finish, not a third hero blow"
    );
    assert!(
        client.tick() - contact < 600,
        "the fixture finishes inside the contribution window"
    );
    let extra = wallet(&client) - passive(&client) - before;
    assert_eq!(
        client
            .view()
            .player(client.core().my_slot.unwrap())
            .unwrap()
            .kills,
        0
    );
    if multiple {
        assert!(
            extra > 134,
            "a shared bounty and assist gold reach each damager: {extra}"
        );
    } else {
        assert_eq!(
            extra, 133,
            "an undamaged tower kill pays the only enemy its base bounty, without First Blood"
        );
    }
    assert_wallet_drawn(&server, &mut client);
    let bounty = if multiple { 134 } else { 133 };
    server.frozen(|| {
        client.until(
            "the shared bounty caption drawn over the visible victim",
            |c| crate::bars::drawn_gold_popups(c.world_mut()).contains(&bounty),
        )
    });
}

#[test]
fn an_undamaged_tower_finish_shares_gold_in_the_drawn_wallet() {
    shared_tower_finish(false);
}
#[test]
fn a_tower_finish_after_two_hero_damagers_shares_bounty_and_assist_gold() {
    shared_tower_finish(true);
}

#[test]
fn a_pudge_credited_tower_finish_gives_a_drawn_flesh_heap_stack_from_far_away() {
    let (server, mut client, foe, phase) = scene_with_hero(false, HeroId::PUDGE);
    one_attack(&mut client, foe);
    retreat(&mut client);
    phase.store(2, Ordering::SeqCst);
    wait_for_death(&mut client);
    assert_eq!(
        client
            .view()
            .player(client.core().my_slot.unwrap())
            .unwrap()
            .kills,
        1
    );
    assert!(client.me().effects.iter().any(|effect| effect.id == bota_proto::EffectId::FLESH_HEAP && effect.stacks == Some(1)), "credited Pudge receives Flesh Heap outside 450 on the death snapshot");
    server.frozen(|| {
        client.until("the Flesh Heap stack drawn in the HUD", |c| {
            let world = c.world_mut();
            world
                .query::<(&crate::buffs::BuffChip, &InheritedVisibility)>()
                .iter(world)
                .any(|(chip, visible)| {
                    visible.get()
                        && chip.shown.is_some_and(|effect| {
                            effect.id == bota_proto::EffectId::FLESH_HEAP.0
                                && effect.stacks == Some(1)
                        })
                })
        })
    });
}
