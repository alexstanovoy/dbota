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
    use bota_client_core::Intent;
    use bota_proto::{ModifierSpec, Target};
    let server = Server::start(1, &["--objectives", "roshan"]);
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
