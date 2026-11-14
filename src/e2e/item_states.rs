//! Items drawn by their state, as Dota draws them, on a real server's match: Power Treads by the
//! attribute they are set to (Strength, Intelligence, Agility, as a press turns them), on one's
//! own hero and on another hero whose inventory is inspected, with the attribute in the install's
//! words in the tooltip; and a Ward Dispenser by the ward it plants.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{Intent, IssuedOrder, Slot};
use bota_proto::{Cheat, HeroId, ItemId, ItemSlot, Order, Target, UnitKind, Vec2 as Spot};

use super::harness::{Client, MID, Server, scripted_seat};
use crate::hud::{Part, PartOf};

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(30);

/// The asset path of a texture of the cache.
fn file(client: &Client, logical: &str) -> String {
    client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file(logical)
        .unwrap_or_else(|| panic!("{logical} in the cache"))
}

/// One of the install's labels.
fn label(client: &Client, key: &str) -> String {
    client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label(key, &[])
        .unwrap_or_else(|| panic!("the install's {key}"))
}

/// The asset path of the icon an item slot of the bottom panel draws.
fn icon_of(client: &mut Client, slot: u8) -> Option<String> {
    let item_box = client.item_box(slot)?;
    let world = client.world_mut();
    let server = world.resource::<AssetServer>().clone();
    let mut parts = world.query::<(&Part, &PartOf, &ImageNode)>();
    parts
        .iter(world)
        .find(|(part, of, _)| of.0 == item_box && matches!(part, Part::Icon))
        .and_then(|(_, _, image)| server.get_path(image.image.id()))
        .map(|path| path.to_string())
}

/// Every run of text shown, a node's or a span's.
fn shown(client: &mut Client) -> Vec<String> {
    let mut texts = client.shown_texts();
    let world = client.world_mut();
    let visible: Vec<Entity> = world
        .query::<(Entity, &Text, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, _, seen)| seen.get())
        .map(|(e, ..)| e)
        .collect();
    texts.extend(
        world
            .query::<(&TextSpan, &ChildOf)>()
            .iter(world)
            .filter(|(_, parent)| visible.contains(&parent.parent()))
            .map(|(span, _)| span.0.clone()),
    );
    texts
}

/// Whether the slot shows the icon `icon` and, hovered, a tooltip naming `words`.
fn shows(client: &mut Client, slot: u8, icon: &str, words: &str) -> bool {
    if icon_of(client, slot).as_deref() != Some(icon) {
        return false;
    }
    let Some(on) = client.item_box(slot).and_then(|b| client.centre_of(b)) else {
        return false;
    };
    client.mouse(on);
    shown(client).iter().any(|t| t.contains(words))
}

/// The three states of Power Treads in the order a press turns them: icon and words.
fn treads(client: &Client) -> [(String, String); 3] {
    ["str", "int", "agi"].map(|a| {
        (
            file(client, &format!("icon.item.power_treads_{a}")),
            label(
                client,
                &format!("DOTA_Tooltip_Ability_item_power_treads_{a}"),
            ),
        )
    })
}

/// One's Power Treads are drawn set to Strength, and each press turns them, to Intelligence and
/// then Agility, the slot's icon and the tooltip's words following; a Ward Dispenser switched to
/// its Sentry Ward is drawn as Dota draws it.
#[test]
fn power_treads_and_a_ward_dispenser_are_drawn_by_their_state() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::Item {
        item: ItemId::POWER_TREADS,
    });
    let slot = client.slot_of(ItemId::POWER_TREADS).expect("the Treads");
    let states = treads(&client);
    for (n, (icon, words)) in states.iter().enumerate() {
        if n > 0 {
            client.within(SOON, "the Treads ready", |c| {
                c.core().item_in(slot).is_some_and(|i| i.cooldown_left == 0)
            });
            client.core_mut().act(Intent::Press {
                slot: Slot::Item(slot),
                ctrl: false,
            });
        }
        client.within(SOON, &format!("the Treads drawn set to {words}"), |c| {
            shows(c, slot, icon, words)
        });
    }
    // An Observer and a Sentry Ward together are a Ward Dispenser, as in Dota.
    for item in [ItemId::OBSERVER_WARD, ItemId::SENTRY_WARD] {
        client.cheat(Cheat::Item { item });
    }
    client.within(SOON, "the dispenser", |c| {
        c.slot_of(ItemId::WARD_DISPENSER).is_some()
    });
    let slot = client
        .slot_of(ItemId::WARD_DISPENSER)
        .expect("the dispenser");
    let observer = file(&client, "icon.item.ward_dispenser");
    client.within(SOON, "the dispenser planting Observers", |c| {
        icon_of(c, slot).as_deref() == Some(observer.as_str())
    });
    // Taken up and pressed again, it switches: Dota's double tap.
    for _ in 0..2 {
        client.core_mut().act(Intent::Press {
            slot: Slot::Item(slot),
            ctrl: false,
        });
        client.frames(2);
    }
    let sentry = file(&client, "icon.item.ward_dispenser_sentry");
    client.within(SOON, "the dispenser planting Sentries", |c| {
        icon_of(c, slot).as_deref() == Some(sentry.as_str())
    });
}

/// Another hero's Power Treads, its inventory inspected, are drawn by the attribute they are
/// set to as its player turns them, Strength to Intelligence to Agility, with the words.
#[test]
fn another_heros_power_treads_are_drawn_by_their_state() {
    let server = Server::start(2, &[]);
    let near = Spot::from_ints(MID.0 + 300, MID.1);
    // The other seat takes the Treads, comes to mid and turns them every two seconds.
    let mut began = None;
    scripted_seat(&server, HeroId::SHADOW_FIEND, move |session| {
        let Some(hero) = session.own_hero() else {
            return Vec::new();
        };
        let tick = session.current().map_or(0, |v| v.tick);
        let start = *began.get_or_insert(tick);
        let held = hero
            .items
            .iter()
            .position(|i| i.is_some_and(|i| i.id == ItemId::POWER_TREADS));
        match held {
            // Asked again until it holds them: a cheat before the game begins is refused.
            None if (tick - start).is_multiple_of(30) => vec![
                IssuedOrder::hero(Order::Cheat {
                    cheat: Cheat::Item {
                        item: ItemId::POWER_TREADS,
                    },
                }),
                IssuedOrder::hero(Order::Cheat {
                    cheat: Cheat::Teleport { at: near },
                }),
            ],
            Some(at) if tick > start && (tick - start).is_multiple_of(60) => {
                vec![IssuedOrder::hero(Order::Use {
                    slot: ItemSlot(at as u8),
                    target: Target::None,
                })]
            }
            _ => Vec::new(),
        }
    });
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.stand_at(MID);
    let me = client.me().id;
    let other = |c: &Client| {
        c.view()
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Hero && u.id != me)
            .filter(|u| {
                u.items
                    .iter()
                    .any(|i| i.is_some_and(|i| i.id == ItemId::POWER_TREADS))
            })
            .map(|u| u.id)
    };
    client.within(SOON, "the other hero in sight with its Treads", |c| {
        other(c).is_some()
    });
    let them = other(&client).expect("the other hero");
    client.core_mut().act(Intent::SelectUnit(Some(them)));
    let slot = |c: &Client| {
        c.view().unit(them).and_then(|u| {
            u.items
                .iter()
                .position(|i| i.is_some_and(|i| i.id == ItemId::POWER_TREADS))
                .map(|at| at as u8)
        })
    };
    let states = treads(&client);
    for (icon, words) in &states {
        client.within(SOON, &format!("its Treads drawn set to {words}"), |c| {
            slot(c).is_some_and(|at| shows(c, at, icon, words))
        });
    }
}

#[test]
fn support_items_draw_their_installed_icons_and_radiance_switches_its_face() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::StartGame);
    for key in [
        "headdress",
        "ring_of_basilius",
        "buckler",
        "mekansm",
        "arcane_boots",
        "ancient_janggo",
        "pipe",
        "guardian_greaves",
        "assault",
        "vladmir",
        "radiance",
        "shivas_guard",
        "tranquil_boots",
        "boots_of_bearing",
        "crellas_crozier",
    ] {
        client.cheat(Cheat::ResetHero);
        let id = bota_core::ITEMS
            .iter()
            .find(|item| item.key.strip_prefix("item_") == Some(key))
            .expect("catalogue item")
            .id;
        client.cheat(Cheat::Item { item: id });
        let slot = client.slot_of(id).expect("held item");
        client.step_frames(2);
        let installed = file(&client, &format!("icon.item.{key}"));
        assert_eq!(
            icon_of(&mut client, slot),
            Some(installed.clone()),
            "{key}: installed item icon"
        );
        if key == "tranquil_boots" {
            client.stand_at(super::harness::MID);
            let at = client.me().pos + Spot::from_ints(400, 0);
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
                .expect("created enemy")
                .clone();
            client.core_mut().act(Intent::AttackAt {
                at: (enemy.pos.x.to_f32(), enemy.pos.y.to_f32()),
                unit: Some(enemy.id),
            });
            client.until("landed attack breaks boots", |c| {
                !c.core()
                    .session()
                    .expect("session")
                    .item_active(c.me().id, ItemSlot(slot), id)
            });
            client.core_mut().act(Intent::Stop);
            client.step_frames(2);
            let broken = file(&client, "icon.item.tranquil_boots_active");
            assert_eq!(
                icon_of(&mut client, slot),
                Some(broken.clone()),
                "broken boots use their installed state icon"
            );
            let passive = bota_core::item_def(id)
                .expect("boots data")
                .attack_break
                .as_ref()
                .expect("breakable passive");
            let carried = client.core().carried_of(client.me());
            assert_eq!(
                carried.move_speed, passive.broken.move_speed,
                "client reads the broken movement bonus"
            );
            assert_eq!(
                carried.hp_regen, passive.broken.hp_regen,
                "client reads the broken regeneration bonus"
            );
            client.cheat(Cheat::Refresh);
            client.step_frames(2);
            assert_eq!(
                icon_of(&mut client, slot),
                Some(broken),
                "refresh does not repair the passive icon"
            );
        }
        if key == "radiance" {
            client.core_mut().act(Intent::Press {
                slot: Slot::Item(slot),
                ctrl: false,
            });
            client.until("burn disabled", |c| {
                !c.core()
                    .session()
                    .expect("session")
                    .item_active(c.me().id, ItemSlot(slot), id)
            });
            client.step_frames(2);
            let disabled = file(&client, "icon.item.radiance_inactive");
            assert_eq!(
                icon_of(&mut client, slot),
                Some(disabled),
                "the disabled icon is drawn"
            );
            client.core_mut().act(Intent::Press {
                slot: Slot::Item(slot),
                ctrl: false,
            });
            client.until("burn enabled", |c| {
                c.core()
                    .session()
                    .expect("session")
                    .item_active(c.me().id, ItemSlot(slot), id)
            });
            client.step_frames(2);
            assert_eq!(
                icon_of(&mut client, slot),
                Some(installed),
                "the enabled icon returns"
            );
        }
    }
}
