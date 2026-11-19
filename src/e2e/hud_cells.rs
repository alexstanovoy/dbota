//! The cells of the in-match HUD as a player meets them before they hold anything: the top bar's
//! ten places, the inventory's cells, and the ability row as wide as the hero's abilities need.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bota_proto::HeroId;

use super::harness::{Client, Server, quiet_seat};

/// The client of a hero in a two-seat match, on its screen.
fn hud_of(hero: HeroId) -> (Server, Client) {
    let server = Server::start(2, &[]);
    quiet_seat(
        &server,
        if hero == HeroId::PUDGE {
            HeroId::SHADOW_FIEND
        } else {
            HeroId::PUDGE
        },
    );
    let mut client = Client::join(&server, hero, &[]);
    super::hud_layout::on_screen(&mut client);
    client.frames(20);
    (server, client)
}

fn named(client: &mut Client, name: &str) -> Entity {
    let world = client.world_mut();
    world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, label)| label.as_str() == name)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("{name} is in the HUD"))
}

fn rect(client: &Client, entity: Entity) -> Rect {
    let world = client.world();
    let node = world.get::<ComputedNode>(entity).expect("laid out");
    let at = world.get::<UiGlobalTransform>(entity).expect("placed");
    Rect::from_center_size(at.translation, node.size())
}

fn named_rect(client: &mut Client, name: &str) -> Rect {
    let entity = named(client, name);
    rect(client, entity)
}

/// One of Dota's pixels, in the window's.
fn dota(client: &Client) -> f32 {
    crate::ui::DOTA * client.world().resource::<UiScale>().0
}

/// The path of the picture an image node shows.
fn path_of(client: &Client, image: &ImageNode) -> String {
    client
        .world()
        .resource::<AssetServer>()
        .get_path(image.image.id())
        .map(|path| path.to_string())
        .unwrap_or_default()
}

/// Every place of both teams is drawn in the top bar whoever plays, as a dark box with the light
/// strip along its top (white where nobody holds the place, the player's colour where one does).
#[test]
fn every_place_of_the_top_bar_is_drawn_for_both_teams_with_a_light_strip_along_its_top() {
    let (_server, mut client) = hud_of(HeroId::SHADOW_FIEND);
    let unit = dota(&client);
    for team in ["Radiant", "Dire"] {
        let mut left = f32::MAX;
        for n in 0..5 {
            let place = named(&mut client, &format!("hud seat {team} {n}"));
            let world = client.world();
            assert!(
                world
                    .get::<InheritedVisibility>(place)
                    .is_some_and(|seen| seen.get()),
                "{team} place {n} is drawn"
            );
            assert!(
                world.get::<BackgroundGradient>(place).is_some(),
                "{team} place {n} has the body of a cell"
            );
            let at = rect(&client, place);
            assert!(at.width() > 40.0 * unit, "{team} place {n}: {at:?}");
            left = left.min(at.min.x);
        }
        assert!(left < f32::MAX, "the {team} places stand in the bar");
    }
    let world = client.world_mut();
    let strips: Vec<(Color, ImageNode, bool, Vec2)> = world
        .query::<(
            &crate::hud::SeatColour,
            &BackgroundColor,
            &ImageNode,
            &InheritedVisibility,
            &ComputedNode,
        )>()
        .iter(world)
        .map(|(_, ground, image, seen, node)| (ground.0, image.clone(), seen.get(), node.size()))
        .collect();
    assert_eq!(strips.len(), 10, "a strip on each of the ten places");
    assert!(
        strips
            .iter()
            .all(|(_, _, seen, size)| *seen && size.x > 0.0),
        "every strip is drawn"
    );
    let white = strips
        .iter()
        .filter(|(colour, ..)| *colour == Color::WHITE)
        .count();
    assert_eq!(white, 8, "eight places nobody holds keep the white strip");
    for (_, image, ..) in &strips {
        assert!(
            path_of(&client, image).contains("topbar_playerslot_vignette"),
            "the strip is the game's vignette"
        );
    }
}

/// An empty inventory, backpack and portal cell is a cell: the game's well under it and its bevel
/// over it, not a flat black square.
#[test]
fn every_empty_inventory_and_backpack_cell_is_drawn_with_the_games_well() {
    let (_server, mut client) = hud_of(HeroId::SHADOW_FIEND);
    let world = client.world_mut();
    let boxes: Vec<Entity> = world
        .query_filtered::<Entity, With<crate::hud::ItemBox>>()
        .iter(world)
        .collect();
    assert!(
        boxes.len() >= 9,
        "the cells of the inventory and the backpack: {}",
        boxes.len()
    );
    let mut bare = Vec::new();
    for cell in boxes {
        let world = client.world();
        let arts: Vec<String> = world
            .get::<Children>(cell)
            .into_iter()
            .flat_map(|children| children.iter())
            .filter(|child| world.get::<crate::hud::CellArt>(*child).is_some())
            .filter_map(|child| world.get::<ImageNode>(child))
            .map(|image| path_of(&client, image))
            .collect();
        let has = |word: &str| arts.iter().any(|path| path.contains(word));
        // The stash and the portal slot have their own art; the cells of the panel have the well.
        if world.get::<crate::hud::CellArt>(cell).is_none()
            && !(has("item_well") && has("item_bevel"))
        {
            let slot = world.get::<crate::hud::ItemBox>(cell).expect("a cell").0;
            if slot < bota_client_core::BAG_SLOTS {
                bare.push(slot);
            }
        }
    }
    assert!(
        bare.is_empty(),
        "cells of these slots have no well and bevel: {bare:?}"
    );
}

/// The ability row is as wide as its abilities need: a hero of four abilities has a narrower panel
/// than one of six, the bars run under the whole row and end where the room kept for the Aghanim's
/// marks ends, and no empty ability cell takes room before them.
#[test]
fn the_ability_row_is_as_wide_as_the_heros_abilities_need_and_the_bars_run_under_all_of_it() {
    let mut panels = Vec::new();
    for hero in [HeroId::PUDGE, HeroId::SHADOW_FIEND] {
        let (_server, mut client) = hud_of(hero);
        let unit = dota(&client);
        let count = bota_core::hero_def(hero).expect("a hero").abilities.len();
        let world = client.world_mut();
        let shown: Vec<(u8, Rect)> = world
            .query::<(
                &crate::hud::AbilityBox,
                &InheritedVisibility,
                &ComputedNode,
                &UiGlobalTransform,
            )>()
            .iter(world)
            .filter(|(_, seen, node, _)| seen.get() && node.size().min_element() > 0.0)
            .map(|(slot, _, node, at)| {
                (slot.0, Rect::from_center_size(at.translation, node.size()))
            })
            .collect();
        assert_eq!(
            shown.len(),
            count,
            "{hero:?}: a box for each of its {count} abilities, no more"
        );
        let last = shown
            .iter()
            .map(|(_, at)| at.max.x)
            .fold(f32::MIN, f32::max);
        let health = named_rect(&mut client, "hud health");
        let mark = named_rect(&mut client, "hud aghs");
        let reach = (health.max.x - last) / unit;
        // The bars end 50 of Dota's pixels past the last ability: the Aghanim's marks' room, and no more.
        assert!(
            (reach - 50.0).abs() < 4.0,
            "{hero:?}: the bars run {reach:.1} of Dota's pixels past the last ability"
        );
        assert!(
            (mark.max.x - health.max.x).abs() <= 2.0,
            "{hero:?}: the Aghanim's marks end where the bars do: {mark:?} {health:?}"
        );
        assert!(
            mark.min.x - last <= 20.0 * unit,
            "{hero:?}: the marks follow the last ability closely: {mark:?} after {last}"
        );
        let panel = named_rect(&mut client, "hud panel");
        panels.push((hero, panel.width()));
    }
    assert!(
        panels[0].1 < panels[1].1 - 20.0,
        "a hero of four abilities has a narrower panel than one of six: {panels:?}"
    );
}

/// The level badge is the game's: a copper rim, the gold ring of the experience's progress inside it,
/// a black disc and the level in gold, not a plain ring round a number.
#[test]
fn the_level_badge_is_a_copper_rim_a_gold_progress_ring_a_black_disc_and_a_gold_figure() {
    let (_server, mut client) = hud_of(HeroId::SHADOW_FIEND);
    let badge = named(&mut client, "hud level");
    let disc = named(&mut client, "hud level disc");
    let figure = named(&mut client, "hud level figure");
    let world = client.world();
    let rim = world.get::<ComputedNode>(badge).expect("laid out").border();
    assert!(
        rim.min_inset.x > 0.0 && rim.max_inset.y > 0.0,
        "a rim all round the ring: {rim:?}"
    );
    let colour = world.get::<BorderColor>(badge).expect("a rim").top;
    assert!(
        colour.to_srgba().red > colour.to_srgba().blue * 2.0,
        "the rim is copper: {colour:?}"
    );
    assert_eq!(
        world.get::<BackgroundColor>(disc).expect("a disc").0,
        Color::BLACK,
        "a black disc under the figure"
    );
    let gold = world
        .get::<TextColor>(figure)
        .expect("a figure")
        .0
        .to_srgba();
    assert!(
        (gold.red - 0.906).abs() < 0.01 && (gold.green - 0.82).abs() < 0.01,
        "the figure is the game's gold: {gold:?}"
    );
    assert!(
        world
            .get::<TextShadow>(figure)
            .is_some_and(|glow| glow.offset == Vec2::ZERO),
        "the figure glows"
    );
    let inner = world.get::<ComputedNode>(disc).expect("laid out").size();
    let outer = world.get::<ComputedNode>(badge).expect("laid out").size();
    assert!(
        outer.x - inner.x > 2.0 * (rim.min_inset.x + 1.0),
        "a ring of progress between the rim and the disc: {outer:?} {inner:?}"
    );
}

/// The level badge on a top-bar portrait lets the pointer through to the portrait: over the badge
/// the portrait is the node directly hovered, so a click selects its hero and an item dropped
/// there is handed over.
#[test]
fn the_pointer_over_a_top_bar_level_badge_is_over_the_portrait() {
    use bevy::picking::hover::DirectlyHovered;
    let (_server, mut client) = hud_of(HeroId::SHADOW_FIEND);
    let mut checked = 0;
    for team in ["Radiant", "Dire"] {
        for n in 0..5 {
            let badge = format!("hud seat level {team} {n}");
            let world = client.world_mut();
            let found = world
                .query::<(Entity, &Name)>()
                .iter(world)
                .find(|(_, name)| name.as_str() == badge)
                .map(|(entity, _)| entity);
            let Some(ring) = found else { continue };
            let laid = world
                .get::<ComputedNode>(ring)
                .is_some_and(|node| node.size().min_element() > 0.0);
            if !laid {
                continue;
            }
            let at = client.centre_of(ring).expect("placed");
            client.mouse(at);
            let world = client.world_mut();
            let hovered: Vec<bool> = world
                .query::<(&crate::hud::SeatBox, &DirectlyHovered)>()
                .iter(world)
                .filter(|(_, hovered)| hovered.get())
                .map(|_| true)
                .collect();
            assert_eq!(
                hovered.len(),
                1,
                "{team} {n}: the pointer over the level badge at {at} is directly over one portrait"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 2,
        "the badges of both seats were tried: {checked}"
    );
}
