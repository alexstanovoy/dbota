//! Runes and the Bottle as Dota shows them, in a match whose server spawns runes (`--runes all`)
//! and puts one where a test asks (`dota_spawn_rune`): the rune's model and glow where it lies,
//! the minimap's icon of it kept out of sight, the toast of a river rune, a rune taken by a click
//! with its sound, its hero's line, its buff and its lines in the chat and the feed, the Bottle's
//! icon by its charges and by the rune it keeps, and an invisible hero drawn see-through.

use std::time::Duration;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::{Intent, Slot};
use bota_proto::{Cheat, EffectId, HeroId, ItemId, RuneKind, StatusFlags, Vec2 as Spot};

use super::harness::{Client, Server};
use crate::hud::{Part, PartOf};
use crate::minimap::RuneMark;
use crate::runes::RuneBody;

/// The Dota map's top river rune spot.
fn top_spot() -> (i32, i32) {
    let at = bota_core::DOTA_RUNE_SPOTS[0].at;
    (at.x.to_int(), at.y.to_int())
}

/// Ground far from every rune spot, out of their sight.
const AWAY: (i32, i32) = (4000, 4000);

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(30);

/// A match of one seat that spawns runes, one's Shadow Fiend on the map and the game begun.
fn rune_match() -> (Server, Client) {
    let server = Server::start(1, &["--runes", "all"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    (server, client)
}

/// Puts a rune of `kind` at the rune spot nearest `at`, as `dota_spawn_rune` does.
fn spawn_rune(client: &mut Client, kind: RuneKind, at: (i32, i32)) {
    client.cheat(Cheat::SpawnRune {
        kind,
        at: Spot::from_ints(at.0, at.1),
    });
}

/// The rune of `kind` drawn on the map, if one is: where it stands, and the model it wears (a
/// rune is known by its model).
fn drawn(client: &mut Client, kind: RuneKind) -> Option<(Vec3, String)> {
    let model = format!("rune.{}.model", crate::runes::art(kind));
    let world = client.world_mut();
    world
        .query_filtered::<(&crate::loot::Idle, &Transform), With<RuneBody>>()
        .iter(world)
        .find(|(idle, _)| idle.0 == model)
        .map(|(idle, place)| (place.translation, idle.0.clone()))
}

/// Puts the pointer on a point of the ground, once the camera has settled enough for the
/// ground under the pointer to be that point.
fn point_at(client: &mut Client, (x, y): (f32, f32)) {
    client.within(SOON, "the pointer on the spot", |c| {
        let Some(on) = c.screen_of((x, y)) else {
            return false;
        };
        c.mouse(on);
        c.world()
            .resource::<crate::camera::CursorGround>()
            .0
            .is_some_and(|g| Vec2::new(g.x, -g.z).distance(Vec2::new(x, y)) < 20.0)
    });
}

/// The minimap's rune icons shown: the kind each marks and whether it is seen now.
fn rune_marks(client: &mut Client) -> Vec<(RuneKind, bool)> {
    let world = client.world_mut();
    world
        .query::<(&RuneMark, &Visibility)>()
        .iter(world)
        .filter(|(_, shown)| **shown != Visibility::Hidden)
        .filter_map(|(mark, _)| mark.0.map(|(_, kind, seen)| (kind, seen)))
        .collect()
}

/// How many times the particle effect of a logical name started.
fn started(client: &Client, effect: &str) -> u32 {
    let world = client.world();
    world
        .get_resource::<crate::particles::Library>()
        .and_then(|library| library.effect(effect))
        .map_or(0, |effect| {
            world
                .resource::<crate::particles::Playing>()
                .started(effect)
        })
}

/// How many times a bound sound, or a hero's line, of a logical name was heard.
fn heard(client: &Client, logical: &str) -> u32 {
    client
        .world()
        .resource::<crate::fx::Heard>()
        .0
        .get(logical)
        .copied()
        .unwrap_or_default()
}

/// The runs of text shown (a tooltip's lines, the chat log's), a span each.
fn shown_spans(client: &mut Client) -> Vec<String> {
    let world = client.world_mut();
    let shown: Vec<Entity> = world
        .query::<(Entity, &Text, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, _, seen)| seen.get())
        .map(|(e, ..)| e)
        .collect();
    world
        .query::<(&TextSpan, &ChildOf)>()
        .iter(world)
        .filter(|(_, parent)| shown.contains(&parent.parent()))
        .map(|(span, _)| span.0.clone())
        .collect()
}

/// One of the install's labels.
fn label(client: &Client, key: &str, values: &[(&str, String)]) -> String {
    client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label(key, values)
        .unwrap_or_else(|| panic!("the install's {key}"))
}

/// The asset path of a texture of the cache.
fn file(client: &Client, logical: &str) -> String {
    client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file(logical)
        .unwrap_or_else(|| panic!("{logical} in the cache"))
}

/// Whether one's hero wears an effect.
fn wears(client: &Client, effect: EffectId) -> bool {
    client.me().effects.iter().any(|e| e.id == effect)
}

/// A rune lies drawn with Dota's model of its kind at its spot, wearing its glow, a Bounty Rune
/// appearing in its swirl; the minimap shows its icon, and keeps it, as Dota does, once the hero
/// has gone and the rune is out of sight; taken, it plays its sound and burst, the feed tells
/// it in Dota's words and the minimap lets it go.
#[test]
fn a_rune_lies_drawn_and_the_minimap_keeps_it_until_its_spot_is_seen_without_it() {
    let (_server, mut client) = rune_match();
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Bounty, spot);
    client.within(SOON, "the Bounty Rune drawn", |c| {
        drawn(c, RuneKind::Bounty).is_some()
    });
    let (place, model) = drawn(&mut client, RuneKind::Bounty).expect("drawn");
    assert_eq!(model, "rune.bounty.model");
    let (x, y) = (spot.0 as f32, spot.1 as f32);
    assert!(
        Vec2::new(place.x, -place.z).distance(Vec2::new(x, y)) < 1.0,
        "at its spot: {place}"
    );
    assert!(started(&client, "rune.bounty.prespawn") > 0, "its swirl");
    client.within(SOON, "its glow", |c| started(c, "rune.bounty.glow") > 0);
    client.within(SOON, "its icon on the minimap", |c| {
        rune_marks(c) == [(RuneKind::Bounty, true)]
    });
    client.stand_at(AWAY);
    client.within(SOON, "the rune out of sight", |c| {
        drawn(c, RuneKind::Bounty).is_none()
    });
    client.frames(5);
    assert_eq!(
        rune_marks(&mut client),
        [(RuneKind::Bounty, false)],
        "remembered in the fog"
    );
    client.stand_at(spot);
    client.within(SOON, "the rune taken", |c| {
        heard(c, "sound.rune.bounty") > 0
    });
    client.within(SOON, "the minimap lets it go", |c| rune_marks(c).is_empty());
    assert!(
        started(&client, "rune.bounty.owner") > 0,
        "its burst of gold"
    );
    let rune = label(&client, "DOTA_HUD_Rune_Bounty", &[("gold", String::new())]);
    client.within(SOON, "the feed's line", |c| {
        c.shown_texts().iter().any(|t| t.contains(rune.trim()))
    });
}

/// A Haste Rune appearing at the river shows Dota's toast; a right click on it walks the hero
/// there and takes it: its sound, the hero's line, the buff's loop on the hero, its chip on the
/// buff bar and the chat line in Dota's words.
#[test]
fn a_rune_clicked_is_taken_with_its_sound_line_buff_and_chat() {
    let (_server, mut client) = rune_match();
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Haste, spot);
    let top = label(&client, "DOTA_HUD_rune_spawned_top", &[]);
    client.within(SOON, "the toast", |c| {
        let world = c.world_mut();
        let toasted = world
            .query::<(&crate::rune_toast::RuneToast, &Visibility)>()
            .iter(world)
            .any(|(t, shown)| {
                *shown != Visibility::Hidden && matches!(t.0, Some((RuneKind::Haste, false, _)))
            });
        toasted && c.shown_texts().contains(&top)
    });
    client.within(SOON, "the Haste Rune drawn", |c| {
        drawn(c, RuneKind::Haste).is_some()
    });
    point_at(&mut client, (spot.0 as f32, spot.1 as f32));
    client.click(MouseButton::Right);
    client.within(SOON, "the haste on the hero", |c| {
        wears(c, EffectId::RUNE_HASTE)
    });
    client.within(SOON, "its sound and the hero's line", |c| {
        heard(c, "sound.rune.haste") > 0 && heard(c, "sound.voice.nevermore.rune.haste") > 0
    });
    client.within(SOON, "the buff's loop", |c| {
        started(c, "rune.haste.owner") > 0
    });
    let me = client.me().id;
    client.core_mut().act(Intent::SelectUnit(Some(me)));
    client.within(SOON, "the buff's chip", |c| {
        let world = c.world_mut();
        world
            .query::<(&crate::buffs::BuffChip, &InheritedVisibility)>()
            .iter(world)
            .any(|(chip, seen)| {
                seen.get() && chip.shown.is_some_and(|s| s.id == EffectId::RUNE_HASTE.0)
            })
    });
    let name = crate::seat_name::seat_name(client.core(), client.core().my_slot.expect("a seat"));
    let line = label(&client, "DOTA_Chat_Rune_Haste", &[]).replace("%s1", &name);
    client.within(SOON, "the chat line", |c| shown_spans(c).contains(&line));
}

/// The icon and the two counts of an item slot of one's hero, as drawn.
fn slot_face(client: &mut Client, slot: u8) -> (Option<String>, String, String) {
    let item_box = client.item_box(slot).expect("the slot's box");
    let world = client.world_mut();
    let server = world.resource::<AssetServer>().clone();
    let mut face = (None, String::new(), String::new());
    let mut parts = world.query::<(&Part, &PartOf, Option<&ImageNode>, Option<&Text>)>();
    for (part, _, image, text) in parts.iter(world).filter(|p| p.1.0 == item_box) {
        match (part, image, text) {
            (Part::Icon, Some(image), _) => {
                face.0 = server.get_path(image.image.id()).map(|p| p.to_string());
            }
            (Part::Charges, _, Some(text)) => face.1 = text.0.clone(),
            (Part::OtherCharges, _, Some(text)) => face.2 = text.0.clone(),
            _ => {}
        }
    }
    face
}

/// The Bottle as Dota draws it: full with its three charges, the medium icon once one is
/// drunk (its pour heard, its swirl on the drinker); a rune walked over goes into it, corked,
/// with the hero's line, its icon then the Bottle holding that rune, the seconds before it goes
/// off on the slot and the rune in Dota's words in the tooltip; used, the rune takes effect.
#[test]
fn the_bottle_shows_its_charges_and_the_rune_it_keeps() {
    let (_server, mut client) = rune_match();
    client.cheat(Cheat::Item {
        item: ItemId::BOTTLE,
    });
    let slot = client
        .slot_of(ItemId::BOTTLE)
        .expect("the Bottle in the bag");
    let full = file(&client, "icon.item.bottle");
    client.within(SOON, "the full Bottle", |c| {
        let face = slot_face(c, slot);
        face.0.as_deref() == Some(full.as_str()) && face.1 == "3"
    });
    // The pour is worn by the hero's drawn body: the drink (2.7 s) must not run out before the
    // hero's model has loaded on a busy machine.
    client.within(SOON, "one's hero drawn", |c| c.body(c.me().id).is_some());
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    let medium = file(&client, "icon.item.bottle_medium");
    client.within(SOON, "a charge drunk", |c| {
        let face = slot_face(c, slot);
        face.0.as_deref() == Some(medium.as_str()) && face.1 == "2"
    });
    client.within(SOON, "the pour", |c| {
        heard(c, "sound.bottle.drink") > 0 && started(c, "item.bottle") > 0
    });
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Arcane, spot);
    client.within(SOON, "the Arcane Rune drawn", |c| {
        drawn(c, RuneKind::Arcane).is_some()
    });
    client.stand_at(spot);
    let arcane = file(&client, "icon.item.bottle_arcane");
    client.within(SOON, "the rune in the Bottle", |c| {
        slot_face(c, slot).0.as_deref() == Some(arcane.as_str())
    });
    let seconds: u32 = slot_face(&mut client, slot)
        .2
        .parse()
        .expect("the seconds it keeps the rune");
    assert!((80..=90).contains(&seconds), "{seconds} s left");
    client.within(SOON, "the cork and the hero's line", |c| {
        heard(c, "sound.bottle.cork") > 0 && heard(c, "sound.voice.nevermore.rune.bottle") > 0
    });
    let item_box = client.item_box(slot).expect("its box");
    let on = client.centre_of(item_box).expect("laid out");
    client.mouse(on);
    let kept = label(&client, "DOTA_Tooltip_Ability_item_bottle_arcane", &[]);
    client.within(SOON, "the kept rune in the tooltip", |c| {
        shown_spans(c).iter().any(|t| t.contains(&kept))
    });
    // Used again once its half second's cooldown since the drink has run out, as a player can.
    client.within(SOON, "the Bottle ready", |c| {
        c.core().item_in(slot).is_some_and(|i| i.cooldown_left == 0)
    });
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.within(SOON, "the rune let out", |c| {
        wears(c, EffectId::RUNE_ARCANE) && heard(c, "sound.rune.arcane") > 0
    });
    client.within(SOON, "the Bottle refilled", |c| {
        slot_face(c, slot).0.as_deref() == Some(full.as_str())
    });
}

/// Whether every mesh of one's hero is drawn see-through.
fn see_through(client: &mut Client) -> bool {
    let Some(body) = client.body(client.me().id) else {
        return false;
    };
    let world = client.world_mut();
    let parts: Vec<Handle<crate::fog::FoggedMaterial>> = world
        .query::<(
            &crate::fog::HeroPart,
            &MeshMaterial3d<crate::fog::FoggedMaterial>,
        )>()
        .iter(world)
        .filter(|(part, _)| part.0 == body)
        .map(|(_, material)| material.0.clone())
        .collect();
    let materials = world.resource::<Assets<crate::fog::FoggedMaterial>>();
    !parts.is_empty()
        && parts.iter().all(|handle| {
            materials
                .get(handle)
                .is_some_and(|m| m.base.alpha_mode == AlphaMode::Blend)
        })
}

/// Under an Invisibility Rune one's hero is drawn see-through, as Dota shows an invisible hero
/// to its own side; an armed attack clicked on a rune destroys it, which the feed tells.
#[test]
fn an_invisible_hero_is_see_through_and_an_attack_click_destroys_a_rune() {
    let (_server, mut client) = rune_match();
    let spot = top_spot();
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Invisibility, spot);
    client.within(SOON, "the Invisibility Rune drawn", |c| {
        drawn(c, RuneKind::Invisibility).is_some()
    });
    client.stand_at(spot);
    client.within(SOON, "the hero invisible", |c| {
        c.me().statuses.bits & StatusFlags::INVISIBLE != 0
    });
    client.within(SOON, "drawn see-through", see_through);
    client.stand_at((spot.0 + 400, spot.1));
    spawn_rune(&mut client, RuneKind::Shield, spot);
    client.within(SOON, "the Shield Rune drawn", |c| {
        drawn(c, RuneKind::Shield).is_some()
    });
    client.key(KeyCode::KeyA, ButtonState::Pressed);
    client.key(KeyCode::KeyA, ButtonState::Released);
    point_at(&mut client, (spot.0 as f32, spot.1 as f32));
    client.click(MouseButton::Left);
    let denied = label(&client, "DOTA_HUD_Rune_DenyShield", &[]);
    client.within(SOON, "the rune destroyed, told in the feed", |c| {
        drawn(c, RuneKind::Shield).is_none()
            && c.shown_texts().iter().any(|t| t.contains(denied.trim()))
    });
    assert!(
        !wears(&client, EffectId::RUNE_SHIELD),
        "destroyed, not taken"
    );
}
