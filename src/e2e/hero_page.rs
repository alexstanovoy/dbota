//! The Heroes pages as a player meets them: the grid's cards answering the pointer and the
//! keyboard, and a hero's page with the hero standing large in the middle, the attributes' gains on
//! their values' line, the abilities' tooltips, the innate's mark, the way back to the grid, and
//! the Demo Hero it starts.

use bevy::input::keyboard::Key;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::TextLayoutInfo;
use bevy::ui_widgets::Button;
use bevy::window::PrimaryWindow;
use bota_proto::HeroId;

use super::harness::Client;
use super::menu::{act, local_menu};
use crate::main_menu::catalogue::HeroCard;
use crate::main_menu::hero_stage::{HeroImage, HeroStage};
use crate::main_menu::{Act, MenuCore, Page};

const HERO: HeroId = HeroId::SHADOW_FIEND;

fn grid() -> Client {
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Heroes));
    client.until("the cards drawn", |c| {
        cards(c).len() >= bota_core::HEROES.len()
    });
    client
}

fn hero_page() -> Client {
    let mut client = local_menu();
    act(&mut client, Act::InspectHero(HERO));
    client.until("the hero's page drawn", |c| {
        named(c, "‹ all heroes").is_some()
    });
    client
}

/// The cards of the grid by hero name.
fn cards(client: &mut Client) -> Vec<(String, Entity)> {
    let world = client.world_mut();
    world
        .query_filtered::<(Entity, &Name), With<HeroCard>>()
        .iter(world)
        .map(|(entity, name)| (name.to_string(), entity))
        .collect()
}

fn card(client: &mut Client, hero: &str) -> Entity {
    cards(client)
        .into_iter()
        .find(|(name, _)| name == hero)
        .unwrap_or_else(|| panic!("a card of {hero}"))
        .1
}

fn named(client: &mut Client, name: &str) -> Option<Entity> {
    let world = client.world_mut();
    world
        .query::<(Entity, &Name, &InheritedVisibility)>()
        .iter(world)
        .find(|(_, named, shown)| shown.get() && named.as_str() == name)
        .map(|(entity, ..)| entity)
}

/// The box a node is drawn in, in the window's pixels.
fn rect(client: &Client, node: Entity) -> Rect {
    let world = client.world();
    let laid = world.get::<ComputedNode>(node).expect("laid out");
    let at = world.get::<UiGlobalTransform>(node).expect("placed");
    Rect::from_center_size(at.translation, laid.size())
}

/// The scale a card is drawn at.
fn scale_of(client: &Client, node: Entity) -> Vec2 {
    client
        .world()
        .get::<UiTransform>(node)
        .expect("transformed")
        .scale
}

fn hovered(client: &Client, node: Entity) -> bool {
    client
        .world()
        .get::<Hovered>(node)
        .expect("hoverable")
        .get()
}

fn window_width(client: &mut Client) -> f32 {
    let world = client.world_mut();
    world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .expect("the window")
        .width()
}

/// The card under the pointer is lit and enlarged about its middle, and no card beside it moves.
#[test]
fn the_card_under_the_mouse_is_raised_and_no_neighbour_moves() {
    let mut client = grid();
    let target = card(&mut client, "Shadow Fiend");
    let others: Vec<(Entity, Rect)> = cards(&mut client)
        .into_iter()
        .filter(|(_, entity)| *entity != target)
        .map(|(_, entity)| (entity, rect(&client, entity)))
        .collect();
    let resting = rect(&client, target);
    assert_eq!(scale_of(&client, target), Vec2::ONE, "at rest");
    client.mouse(resting.center());
    assert!(hovered(&client, target), "the pointer is on the card");
    client.until("the card grown", |c| scale_of(c, target).x > 1.085);
    let raised = scale_of(&client, target);
    assert!(
        raised.x > 1.04 && raised.x < 1.15 && raised.x == raised.y,
        "slightly larger: {raised}"
    );
    assert_eq!(
        rect(&client, target).center(),
        resting.center(),
        "about its middle"
    );
    let outline = client
        .world()
        .get::<Outline>(target)
        .expect("an outline")
        .color;
    assert_ne!(outline, Color::NONE, "lit");
    for (entity, before) in others {
        assert_eq!(
            rect(&client, entity),
            before,
            "{entity:?} stays where it was"
        );
        assert_eq!(
            scale_of(&client, entity),
            Vec2::ONE,
            "{entity:?} is not raised"
        );
    }
    client.mouse(Vec2::new(5.0, 5.0));
    client.until("the card lowered", |c| scale_of(c, target) == Vec2::ONE);
}

/// At the very edge of a card, to and fro, the card does not flicker: growing keeps the pointer
/// inside it, and the next card is reached only past the gap.
#[test]
fn the_pointer_on_a_cards_border_raises_it_steadily() {
    let mut client = grid();
    let target = card(&mut client, "Shadow Fiend");
    let resting = rect(&client, target);
    let edge = Vec2::new(resting.min.x + 0.5, resting.center().y);
    client.mouse(edge);
    client.until("the card grown", |c| scale_of(c, target).x > 1.0899);
    let mut states = Vec::new();
    for _ in 0..6 {
        // Inside the border, then a pixel outside it (in the gap): both belong to the card once
        // it is raised, and neither lets it fall.
        client.mouse(edge);
        states.push(scale_of(&client, target).x);
        client.mouse(Vec2::new(resting.min.x - 1.0, resting.center().y));
        states.push(scale_of(&client, target).x);
    }
    assert!(
        states
            .iter()
            .all(|scale| (*scale - states[0]).abs() < 1e-4 && *scale > 1.085),
        "raised all along: {states:?}"
    );
}

/// The keyboard's focus raises a card as the pointer does.
#[test]
fn the_card_with_the_keyboards_focus_is_raised() {
    let mut client = grid();
    for _ in 0..80 {
        client.tap_named(KeyCode::Tab, Key::Tab);
        let focused = client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get();
        let Some(card) = focused.filter(|entity| client.world().get::<HeroCard>(*entity).is_some())
        else {
            continue;
        };
        client.until("the focused card grown", |c| scale_of(c, card).x > 1.085);
        let raised: Vec<String> = cards(&mut client)
            .into_iter()
            .filter(|(_, entity)| scale_of(&client, *entity).x > 1.0)
            .map(|(name, _)| name)
            .collect();
        assert_eq!(raised.len(), 1, "one card at a time: {raised:?}");
        return;
    }
    panic!("Tab never reached a hero card");
}

/// The hero stands large in the middle of the scene, as the model the game draws and not as a
/// picture of it.
#[test]
fn the_hero_page_draws_the_hero_in_the_middle_of_the_scene() {
    let mut client = hero_page();
    client.until("the hero standing on the stage", |c| {
        let stage = c.world().resource::<HeroStage>();
        stage.hero == Some(HERO) && stage.ready()
    });
    client.frames(3);
    let width = window_width(&mut client);
    let world = client.world_mut();
    let (shown, node, at, image) = world
        .query::<(
            &InheritedVisibility,
            &ComputedNode,
            &UiGlobalTransform,
            &ImageNode,
        )>()
        .iter(world)
        .filter(|(.., image)| image.image == *world.resource::<HeroStage>().image_handle())
        .map(|(a, b, c, d)| (a.get(), *b, *c, d.image.clone()))
        .next()
        .expect("the stage's picture on the page");
    assert!(shown, "the hero shows");
    assert_ne!(image, Handle::default(), "a rendered image");
    let middle = at.translation.x / width;
    assert!(
        (middle - 0.485).abs() < 0.01,
        "large in the middle of the scene, a little left of the screen's: {middle} of the width, {node:?}"
    );
    let portrait = world
        .resource::<crate::manifest::Manifest>()
        .portrait(HERO.0);
    let pictures = crate::main_menu::tests::page_pictures(world, 100.0);
    assert!(
        !pictures.iter().any(|path| Some(path) == portrait.as_ref()),
        "no large picture of the hero's portrait is on the page: {pictures:?}"
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<HeroImage>>()
            .iter(world)
            .count(),
        1
    );
}

/// The lowest edge of the glyphs of each span of the text, in the window's pixels: for figures,
/// the line they stand on.
fn span_bottoms(world: &World, text: Entity) -> Vec<f32> {
    let (Some(layout), Some(node), Some(at)) = (
        world.get::<TextLayoutInfo>(text),
        world.get::<ComputedNode>(text),
        world.get::<UiGlobalTransform>(text),
    ) else {
        return Vec::new();
    };
    let top = at.translation.y - node.size().y / 2.0;
    let mut lowest: std::collections::BTreeMap<u32, f32> = std::collections::BTreeMap::new();
    for glyph in &layout.glyphs {
        let bottom = top + glyph.position.y + glyph.atlas_info.rect.height() / 2.0;
        let slot = lowest.entry(glyph.section_index).or_insert(f32::MIN);
        *slot = slot.max(bottom);
    }
    lowest.into_values().collect()
}

/// Every text under `root`.
fn texts_under(world: &World, root: Entity) -> Vec<Entity> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if world.get::<Text>(entity).is_some() {
            found.push(entity);
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    found
}

/// Each attribute's gain stands on the same line as its value: the lowest edge of the figures of
/// the value and of the gain is one.
#[test]
fn an_attributes_gain_stands_on_the_baseline_of_its_value() {
    let mut client = hero_page();
    client.frames(5);
    let tile_titles = ["STRENGTH", "AGILITY", "INTELLIGENCE"];
    let page = named(&mut client, "hero page").expect("the hero's page");
    let world = client.world_mut();
    let mut checked = 0;
    // The tile is the parent of the caption naming the attribute.
    let captions: Vec<(Entity, String)> = texts_under(world, page)
        .into_iter()
        .filter_map(|entity| Some((entity, world.get::<Text>(entity)?.0.clone())))
        .filter(|(_, title)| tile_titles.contains(&title.as_str()))
        .collect();
    assert_eq!(captions.len(), 3, "the three attribute tiles: {captions:?}");
    for (caption, title) in captions {
        let tile = world.get::<ChildOf>(caption).expect("in a tile").parent();
        let texts = texts_under(world, tile);
        let figures: Vec<f32> = texts
            .into_iter()
            .filter(|text| *text != caption)
            .flat_map(|text| span_bottoms(world, text))
            .collect();
        assert!(figures.len() >= 2, "{title}: a value and a gain were set");
        let (low, high) = figures
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), b| (l.min(*b), h.max(*b)));
        assert!(
            high - low < 1.5,
            "{title}: the figures end on one line, not {low}..{high}"
        );
        checked += 1;
    }
    assert_eq!(checked, 3);
}

/// Under the pointer, an ability shows the tooltip of the game: its name, its numbers from the
/// simulator's data.
#[test]
fn an_ability_under_the_pointer_shows_the_games_tooltip() {
    let mut client = hero_page();
    let def = bota_core::hero_def(HERO).expect("the hero");
    let ability = def
        .abilities
        .iter()
        .copied()
        .map(bota_core::learn_group)
        .find(|id| Some(*id) != def.innate)
        .and_then(bota_core::ability_def)
        .expect("an ability that is not the innate");
    let name = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .name(ability.key)
        .to_lowercase();
    let tile = named(&mut client, &format!("ability {name}"))
        .unwrap_or_else(|| panic!("a tile for {name}"));
    let expected = expected_tooltip(&mut client, ability.id.0, ability.max_level);
    assert!(
        tooltip_words(&mut client).is_empty(),
        "no tooltip before the pointer comes"
    );
    let at = client.centre_of(tile).expect("placed");
    client.mouse(at);
    client.frames(3);
    assert_tooltip(&mut client, &expected);
}

/// The innate is the round mark of the game's interface, and the pointer on it shows its tooltip.
#[test]
fn the_innate_is_the_games_round_mark_and_shows_its_tooltip() {
    let mut client = hero_page();
    let def = bota_core::hero_def(HERO).expect("the hero");
    let innate = def
        .innate
        .and_then(bota_core::ability_def)
        .expect("a hero with an innate");
    let name = client
        .world()
        .resource::<crate::dota_text::Texts>()
        .name(innate.key)
        .to_lowercase();
    let tile = named(&mut client, &format!("ability {name}")).expect("the innate's tile");
    let round = {
        let world = client.world();
        world
            .get::<Children>(tile)
            .expect("a mark in the tile")
            .iter()
            .filter_map(|child| world.get::<Node>(child))
            .any(|node| node.border_radius == BorderRadius::MAX)
    };
    assert!(round, "a round mark in the innate's place");
    let expected = expected_tooltip(&mut client, innate.id.0, 1);
    let at = client.centre_of(tile).expect("placed");
    client.mouse(at);
    client.frames(3);
    assert_tooltip(&mut client, &expected);
}

/// What the game's tooltip of the ability says, made from the same sources.
fn expected_tooltip(client: &mut Client, id: u16, max_level: u8) -> crate::tip_content::Content {
    let world = client.world();
    let look = crate::tip_content::Lookup {
        texts: world.resource::<crate::dota_text::Texts>(),
        manifest: world.resource::<crate::manifest::Manifest>(),
        rates: Default::default(),
    };
    look.ability(id, 1, max_level).expect("a tooltip")
}

/// The words of the tooltip: its texts and the spans of them, which hold the figures.
fn tooltip_words(client: &mut Client) -> Vec<String> {
    let world = client.world_mut();
    let spans: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<Text>, With<TextSpan>)>>()
        .iter(world)
        .collect();
    spans
        .into_iter()
        .filter(|entity| {
            let mut at = Some(*entity);
            while let Some(node) = at {
                if world
                    .get::<GlobalZIndex>(node)
                    .is_some_and(|z| z.0 == crate::tips::TIP_Z)
                {
                    return true;
                }
                at = world.get::<ChildOf>(node).map(|parent| parent.parent());
            }
            false
        })
        .filter_map(|entity| {
            world
                .get::<Text>(entity)
                .map(|text| text.0.clone())
                .or_else(|| world.get::<TextSpan>(entity).map(|span| span.0.clone()))
        })
        .collect()
}

fn assert_tooltip(client: &mut Client, expected: &crate::tip_content::Content) {
    let shown = tooltip_words(client);
    assert!(
        shown
            .iter()
            .any(|text| text.eq_ignore_ascii_case(&expected.title)),
        "the tooltip names the ability {:?}: {shown:?}",
        expected.title
    );
    let figures: Vec<&str> = expected
        .costs
        .iter()
        .flat_map(|cost| cost.figures.iter().map(|run| run.text.as_str()))
        .filter(|figure| !figure.trim().is_empty() && *figure != "/")
        .collect();
    for figure in figures {
        assert!(
            shown.iter().any(|text| text.contains(figure)),
            "the tooltip carries the figure {figure:?} of the simulator: {shown:?}"
        );
    }
}

/// The way back to the grid is a button at the page's left, and it leads there.
#[test]
fn the_way_back_to_the_heroes_is_a_visible_button_at_the_left() {
    let mut client = hero_page();
    let back = named(&mut client, "‹ all heroes").expect("the way back");
    assert!(
        client.world().get::<Button>(back).is_some()
            && client
                .world()
                .get::<crate::main_menu::Shade>(back)
                .is_some(),
        "a button of the menu's kit, with a ground and an edge"
    );
    let width = window_width(&mut client);
    let place = rect(&client, back);
    assert!(
        place.max.x < width * 0.5,
        "in the left half, not in the middle: {place:?}"
    );
    assert!(place.min.x < width * 0.12, "at the left edge: {place:?}");
    client.mouse(place.center());
    client.click(MouseButton::Left);
    client.frames(5);
    assert_eq!(
        client.world().non_send::<MenuCore>().page(),
        Page::Heroes,
        "back at the grid"
    );
}

/// A hero's page offers DEMO, and a press on it starts Dota's Demo Hero with the hero.
#[test]
fn the_demo_button_of_a_heros_page_starts_the_demo_hero() {
    let mut client = hero_page();
    crate::main_menu::tests::use_matching_local_server(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
    );
    let demo = named(&mut client, "demo hero").expect("a DEMO HERO button on the hero's page");
    let place = rect(&client, demo);
    client.mouse(place.center());
    client.click(MouseButton::Left);
    client.until("the Demo Hero on screen", |c| {
        c.world().non_send::<MenuCore>().shows_game()
    });
    assert_eq!(
        client.world().non_send::<MenuCore>().demo_hero(),
        Some(HERO),
        "the demo is of the hero whose page it was"
    );
}

/// A browser starts no game of its own, so it offers DEMO only where the service has a Demo Hero;
/// a desktop always can.
#[test]
fn a_demo_is_offered_where_the_client_can_host_or_the_service_has_one() {
    for (can_host, service_offers, offered) in [
        (true, false, true),
        (true, true, true),
        (false, true, true),
        (false, false, false),
    ] {
        assert_eq!(
            MenuCore::demo_policy(can_host, service_offers),
            offered,
            "host {can_host}, service {service_offers}"
        );
    }
}

/// The popup that shows, if one does.
/// The ability tiles of the hero's page (the pick screen under it has its own, which do not show).
fn ability_tiles(client: &mut Client) -> Vec<Entity> {
    let page = named(client, "hero page").expect("the hero's page");
    let world = client.world_mut();
    super::harness::subtree(world, page)
        .into_iter()
        .filter(|entity| world.get::<crate::tips::AbilityTile>(*entity).is_some())
        .collect()
}

fn tooltip(client: &mut Client) -> Option<Entity> {
    let world = client.world_mut();
    world
        .query::<(Entity, &GlobalZIndex, &InheritedVisibility)>()
        .iter(world)
        .find(|(_, z, shown)| z.0 == crate::tips::TIP_Z && shown.get())
        .map(|(entity, ..)| entity)
}

/// Whichever ability or the innate is hovered, its popup stands beside the tile, inside the window,
/// and covers neither the tile nor the rest of the row of abilities nor the hero's title over it,
/// in a small window and a large one.
#[test]
fn an_abilitys_tooltip_stands_beside_its_row_and_never_over_it() {
    let mut client = hero_page();
    for (width, height) in [(1280.0, 720.0), (1920.0, 1080.0)] {
        {
            let world = client.world_mut();
            let mut window = world
                .query_filtered::<&mut Window, With<PrimaryWindow>>()
                .single_mut(world)
                .expect("the window");
            window.resolution.set(width, height);
        }
        client.frames(10);
        for hero in [HeroId::PUDGE, HeroId::SHADOW_FIEND] {
            act(&mut client, Act::InspectHero(hero));
            client.frames(8);
            let tiles = ability_tiles(&mut client);
            assert!(tiles.len() >= 4, "the innate and the abilities");
            let protected = [
                named(&mut client, "hero abilities").expect("the row of abilities"),
                named(&mut client, "hero title").expect("the hero's title"),
            ];
            for tile in tiles {
                let anchor = rect(&client, tile);
                client.mouse(anchor.center());
                client.frames(8);
                let name = client.world().get::<Name>(tile).map(|n| n.to_string());
                let popup = tooltip(&mut client).unwrap_or_else(|| {
                    panic!("{width}x{height} {hero:?}: no popup over {name:?} at {anchor:?}")
                });
                let popup = rect(&client, popup);
                let in_window = popup.min.cmpge(Vec2::ZERO).all()
                    && popup.max.x <= width
                    && popup.max.y <= height;
                assert!(
                    in_window,
                    "{width}x{height}: the popup {popup:?} is inside the window"
                );
                for covered in [tile].into_iter().chain(protected) {
                    let part = rect(&client, covered);
                    assert!(
                        popup.intersect(part).size().cmple(Vec2::ZERO).any(),
                        "{width}x{height} {hero:?} {name:?}: the popup {popup:?} covers {part:?}"
                    );
                }
                client.mouse(Vec2::new(width - 2.0, height - 2.0));
                client.frames(3);
            }
        }
    }
}

/// The popup is set in the game's faces (the shipped ones only where the cache has none).
#[test]
fn the_tooltip_is_set_in_the_games_faces() {
    let mut client = hero_page();
    let tile = { ability_tiles(&mut client).last().copied().expect("a tile") };
    let at = client.centre_of(tile).expect("placed");
    client.mouse(at);
    client.frames(6);
    let fonts = client.world().resource::<crate::ui::Fonts>().clone();
    let (bold, regular) = (
        fonts.face(crate::theme::Face::Bold).id(),
        fonts.face(crate::theme::Face::Regular).id(),
    );
    let world = client.world_mut();
    let used: Vec<bevy::asset::AssetId<Font>> = world
        .query::<(Entity, &TextFont)>()
        .iter(world)
        .filter(|(entity, _)| {
            let mut at = Some(*entity);
            while let Some(node) = at {
                if world
                    .get::<GlobalZIndex>(node)
                    .is_some_and(|z| z.0 == crate::tips::TIP_Z)
                {
                    return true;
                }
                at = world.get::<ChildOf>(node).map(|parent| parent.parent());
            }
            false
        })
        .filter_map(|(_, font)| match &font.font {
            bevy::text::FontSource::Handle(handle) => Some(handle.id()),
            _ => None,
        })
        .collect();
    assert!(used.len() > 5, "the popup's texts: {}", used.len());
    assert!(
        used.iter().all(|id| *id == bold || *id == regular),
        "every text of the popup is in the game's bold or regular face"
    );
}
