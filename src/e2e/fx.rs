//! The effects the manifest's `fx` lines bind to the game's facts, in a real match: a hook thrown
//! (a cast, a missile in flight, a blow, a stun worn, a death) and items used (a use, an effect
//! put on, an item carried). Each asserts that the particle effects started and the sounds
//! sounded, which are what the client would draw and play.

use bota_client_core::{Intent, Slot};
use bota_proto::{AbilityId, Cheat, EntityId, HeroId, ItemId, ModifierSpec, Target, UnitKind};

use super::harness::{Client, MID, Server};
use crate::fx::Heard;
use crate::particles::{Library, Playing};

/// How many times the particle effect of a logical name was started.
fn started(client: &Client, logical: &str) -> u32 {
    let library = client.world().resource::<Library>();
    let effect = library
        .effect(logical)
        .unwrap_or_else(|| panic!("{logical} is not in the particle library"));
    client.world().resource::<Playing>().started(effect)
}

/// How many times a sound of the logical prefix was sounded by a bound line.
fn heard(client: &Client, prefix: &str) -> u32 {
    let heard = client.world().resource::<Heard>();
    heard.0.get(prefix).copied().unwrap_or(0)
}

/// The slot of one's ability.
fn slot_of(client: &Client, ability: AbilityId) -> u8 {
    client
        .me()
        .abilities
        .iter()
        .position(|a| a.id == ability)
        .expect("the ability") as u8
}

/// The enemy hero beside, as the view tells it.
fn enemy(client: &Client) -> (EntityId, (f32, f32)) {
    let mine = client.me().team;
    let hero = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.team != mine)
        .expect("the enemy hero");
    (hero.id, (hero.pos.x.to_f32(), hero.pos.y.to_f32()))
}

/// Throws the hook at a spot.
fn hook(client: &mut Client, slot: u8, at: (f32, f32)) {
    let core = client.core_mut();
    core.act(Intent::Press {
        slot: Slot::Ability(slot),
        ctrl: false,
    });
    core.act(Intent::Aimed { at, unit: None });
}

/// Pudge hooks an enemy hero and then a frail one: the throw sounds, the chain flies as Dota's
/// particle, the hit bursts and sounds on the one caught, the stun of the drag shows over it,
/// and the death of the second is sounded as Dota's Pudge dies.
#[test]
fn a_meat_hook_is_thrown_flies_hits_stuns_and_kills_as_dota_shows_it() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Levels { count: 5 });
    client.cheat(Cheat::Wtf { on: true });
    let slot = slot_of(&client, AbilityId::MEAT_HOOK);
    let core = client.core_mut();
    core.act(Intent::Press {
        slot: Slot::Ability(slot),
        ctrl: true,
    });
    client.frames(3);
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: true,
        at: bota_proto::Vec2::from_ints(MID.0 + 500, MID.1),
    });
    let (_, at) = enemy(&client);
    hook(&mut client, slot, at);
    client.until("the hook thrown and sounded", |c| {
        heard(c, "sound.pudge.hook_throw") > 0
    });
    client.until("the chain flying", |c| started(c, "hero.pudge.hook") > 0);
    client.until("the hit shown and sounded", |c| {
        started(c, "hero.pudge.hook_impact") > 0 && heard(c, "sound.pudge.hook_impact") > 0
    });
    client.until("the stun of the drag shown", |c| {
        started(c, "status.stunned") > 0
    });
    // The hook is back; the enemy is beside Pudge, frail, and the next hook strikes four times as hard.
    client.until("the hook back", |c| {
        !c.view()
            .projectiles
            .iter()
            .any(|p| p.inflictor == Some(bota_proto::Inflictor::Ability(AbilityId::MEAT_HOOK)))
    });
    let (victim, at) = enemy(&client);
    let frail = ModifierSpec {
        max_hp: ModifierSpec::MIN_SCALE,
        ..ModifierSpec::NOMINAL
    };
    let strong = ModifierSpec {
        pure_damage: ModifierSpec::MAX_SCALE,
        ..ModifierSpec::NOMINAL
    };
    let me = client.me().id;
    for (target, spec) in [(victim, frail), (me, strong)] {
        client.core_mut().act(Intent::Cheat(Cheat::ApplyModifier {
            target: Target::Unit(target),
            spec,
            ticks: 3000,
        }));
    }
    client.frames(5);
    hook(&mut client, slot, at);
    // The fall of a Pudge sounds Pudge's own and no other hero's.
    client.until("the death sounded", |c| {
        crate::sound::deaths_sounded(c.world(), HeroId::PUDGE) > 0
    });
    assert_eq!(
        crate::sound::deaths_sounded(client.world(), HeroId::SHADOW_FIEND),
        0,
        "Shadow Fiend's death sounded for a Pudge's"
    );
}

/// Phase Boots and Power Treads used: the boots' sound and the trail they put on, the burst of the
/// treads' swap, and the glow of the treads while they are carried; and Shadow Fiend's eyes burn.
#[test]
fn items_used_and_carried_show_the_effects_and_sounds_dota_gives_them() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    for item in [ItemId::PHASE_BOOTS, ItemId::POWER_TREADS] {
        client.cheat(Cheat::Item { item });
    }
    client.until("the eyes burning", |c| {
        started(c, "hero.nevermore.ambient_eyes") > 0
    });
    client.until("the treads' glow", |c| {
        started(c, "item.power_treads.buff") > 0
    });
    let press = |client: &mut Client, item| {
        let slot = client.slot_of(item).expect("the item carried");
        client.core_mut().act(Intent::Press {
            slot: Slot::Item(slot),
            ctrl: false,
        });
    };
    // One order at a time: a second press before the first takes effect replaces it.
    press(&mut client, ItemId::PHASE_BOOTS);
    client.until("the boots' sound", |c| {
        heard(c, "sound.item.phase_boots") > 0
    });
    client.until("the boots' trail", |c| started(c, "item.phase_boots") > 0);
    press(&mut client, ItemId::POWER_TREADS);
    client.until("the treads' burst", |c| {
        started(c, "item.power_treads.swap") > 0
    });
}

/// Pudge carrying a Maelstrom and a Skull Basher strikes a sturdy enemy hero: each unit a Chain
/// Lightning bolt strikes flashes and crackles as a Maelstrom's does in Dota, and a bash sounds as
/// a Skull Basher's on the one it stuns, under the stun's own mark.
#[test]
fn a_maelstroms_lightning_and_a_skull_bashers_bash_show_as_dota_shows_them() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::PUDGE, &[]);
    client.stand_at(MID);
    for item in [ItemId::MAELSTROM, ItemId::SKULL_BASHER] {
        client.cheat(Cheat::Item { item });
    }
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: true,
        at: bota_proto::Vec2::from_ints(MID.0 + 150, MID.1),
    });
    let (victim, _) = enemy(&client);
    // Many blows land before it falls.
    client.cheat(Cheat::ApplyModifier {
        target: Target::Unit(victim),
        spec: ModifierSpec {
            max_hp: ModifierSpec::MAX_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: 3000,
    });
    client.core_mut().send_order(bota_proto::Order::Attack {
        target: Target::Unit(victim),
    });
    client.until("a Chain Lightning bolt's flash and crackle", |c| {
        started(c, "item.maelstrom.hit") > 0 && heard(c, "sound.item.maelstrom") > 0
    });
    client.until("a bash sounded under the stun's mark", |c| {
        heard(c, "sound.item.skull_basher") > 0 && started(c, "status.stunned") > 0
    });
}

/// A Blink Dagger used by a client run with `--mute` (as the visual checks run it): its start and
/// end effects, and the dagger's sound, which the mixer takes though the master volume is nil.
#[test]
fn a_blink_sounds_the_daggers_activation_in_a_muted_client() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    client.stand_at(MID);
    client.cheat(Cheat::Item {
        item: ItemId::BLINK_DAGGER,
    });
    client.until("the dagger carried", |c| {
        c.slot_of(ItemId::BLINK_DAGGER).is_some()
    });
    let slot = client
        .slot_of(ItemId::BLINK_DAGGER)
        .expect("the dagger carried");
    let from = client.me().pos;
    client.core_mut().send_order(bota_proto::Order::Use {
        slot: bota_proto::ItemSlot(slot),
        target: bota_proto::Target::Pos(from + bota_proto::Vec2::from_ints(600, 0)),
    });
    client.until("the start effect", |c| started(c, "item.blink_start") > 0);
    client.until("the end effect", |c| started(c, "item.blink_end") > 0);
    client.until("the dagger's sound", |c| {
        heard(c, "sound.blink.activate") > 0
    });
}

#[test]
fn support_item_particles_follow_the_imported_wave_data() {
    use bevy::prelude::*;
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f32(1.0 / 30.0),
        ));
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    let shiva = bota_core::ITEMS
        .iter()
        .find(|item| item.key == "item_shivas_guard")
        .expect("Shiva");
    client.cheat(Cheat::Item { item: shiva.id });
    let slot = client.slot_of(shiva.id).expect("Shiva held");
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.until("arctic blast effect and sound", |c| {
        started(c, "item.shivas_guard.wave") > 0 && heard(c, "sound.item.shivas_guard") > 0
    });
    let expected = Vec3::new(
        bota_core::widened(
            shiva.value("blast_radius").expect("radius")[0].int(),
            shiva.value("bonus_aoe").expect("area bonus")[0].int(),
        ) as f32,
        0.0,
        shiva.value("blast_speed").expect("speed")[0].int() as f32,
    );
    let origin = client.me().pos;
    client
        .core_mut()
        .act(Intent::MoveTo((MID.0 as f32 + 200.0, MID.1 as f32)));
    let mut samples = 0;
    for _ in 0..10 {
        client.step_frames(1);
        let body = client.body(client.me().id).expect("drawn hero");
        let drawn = client
            .world()
            .get::<crate::units::Unit>(body)
            .expect("unit")
            .at;
        let library = client.world().resource::<Library>();
        let effect = library
            .effect("item.shivas_guard.wave")
            .expect("blast effect");
        for instance in client.world().resource::<Playing>().instances_of(effect) {
            assert!(
                instance.nodes[0].control_points[1]
                    .position
                    .distance(expected)
                    < 0.1,
                "imported area and speed feed the wave"
            );
            let origin = instance.nodes[0].control_points[0].position;
            assert!(
                Vec2::new(origin.x, origin.y).distance(drawn) < 0.5,
                "the wave follows the drawn caster"
            );
            samples += 1;
        }
    }
    assert!(samples > 0, "the moving wave was inspected");
    assert!(
        !client
            .me()
            .pos
            .within(origin, bota_proto::Fixed::from_int(10)),
        "the caster moved while the wave was inspected"
    );
}

#[test]
fn support_activations_show_their_imported_particles_and_sounds_at_full_pools() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::StartGame);
    for (key, particle, sound) in [
        ("item_mekansm", "item.mekansm.cast", "sound.item.mekansm"),
        (
            "item_arcane_boots",
            "item.arcane_boots.cast",
            "sound.item.arcane_boots",
        ),
        (
            "item_ancient_janggo",
            "item.ancient_janggo.buff",
            "sound.item.ancient_janggo",
        ),
        ("item_pipe", "item.pipe.barrier", "sound.item.pipe"),
        (
            "item_guardian_greaves",
            "item.guardian_greaves.cast",
            "sound.item.guardian_greaves",
        ),
        (
            "item_boots_of_bearing",
            "item.boots_of_bearing.cast",
            "sound.item.ancient_janggo",
        ),
    ] {
        client.cheat(Cheat::ResetHero);
        client.stand_at(MID);
        let id = bota_core::ITEMS
            .iter()
            .find(|item| item.key == key)
            .expect("support item")
            .id;
        client.cheat(Cheat::Item { item: id });
        let slot = client.slot_of(id).expect("held item");
        let shown = started(&client, particle);
        let sounded = heard(&client, sound);
        client.core_mut().act(Intent::Press {
            slot: Slot::Item(slot),
            ctrl: false,
        });
        client.until(&format!("{key} activation particle and sound"), |c| {
            started(c, particle) > shown && heard(c, sound) > sounded
        });
        if let bota_core::UseSpec::Support(support) =
            bota_core::item_def(id).expect("support data").effect
            && let Some(index) = support.lockout
        {
            let effect =
                bota_core::item_def(id).expect("support data").modifiers[usize::from(index)].effect;
            client.until("the receiver timer is drawn in the buff row", |c| {
                let world = c.world_mut();
                world
                    .query::<(&crate::buffs::BuffChip, &bevy::prelude::InheritedVisibility)>()
                    .iter(world)
                    .any(|(chip, seen)| {
                        seen.get()
                            && chip.shown.is_some_and(|shown| {
                                shown.id == effect.0
                                    && shown.ticks_left.is_some_and(|ticks| ticks > 0)
                            })
                    })
            });
        }
        if key == "item_mekansm" || key == "item_guardian_greaves" {
            let target = if key == "item_mekansm" {
                "item.mekansm.target"
            } else {
                "item.guardian_greaves.target"
            };
            assert!(
                started(&client, target) > 0,
                "full-health recipient still shows its recovery effect"
            );
        }
    }
}

#[test]
fn crozier_shows_its_shared_ghost_and_movement_effects_and_collection_heal() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    let at = client.me().pos + bota_proto::Vec2::from_ints(400, 0);
    client.cheat(Cheat::CreateHero {
        hero: HeroId::SHADOW_FIEND,
        enemy: true,
        at,
    });
    let enemy = client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Hero && unit.team != client.me().team)
        .expect("enemy")
        .id;
    client.cheat(Cheat::Damage {
        target: Target::Unit(enemy),
        amount: 100,
        kind: bota_proto::DamageKind::Pure,
    });
    let item = bota_core::ITEMS
        .iter()
        .find(|item| item.key == "item_crellas_crozier")
        .expect("item")
        .id;
    client.cheat(Cheat::Item { item });
    client.cheat(Cheat::SetPools {
        health: 25,
        mana: 100,
    });
    let slot = client.slot_of(item).expect("held item");
    let ghost = started(&client, "item.ghost.form");
    let source = started(&client, "item.crellas_crozier.buff");
    let target = started(&client, "item.crellas_crozier.debuff");
    let healed = started(&client, "item.magic_stick");
    let cast = heard(&client, "sound.item.crellas_crozier.cast");
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.until(
        "the shared ghost and movement effects are shown with the cast sound",
        |c| {
            started(c, "item.ghost.form") > ghost
                && started(c, "item.crellas_crozier.buff") > source
                && started(c, "item.crellas_crozier.debuff") > target
                && heard(c, "sound.item.crellas_crozier.cast") > cast
        },
    );
    client.until("positive collection shows the native heal particle", |c| {
        started(c, "item.magic_stick") > healed
    });
}
