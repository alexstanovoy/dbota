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
