//! The header bands of a tooltip (Active, Passive, Use), in the real tooltip of the client: each is
//! the band of `tip_band`, spanning the tooltip's text blocks, its art laid on the band's left end
//! at the art's own size. (`tip_band::render` measures the same on a GPU.)

use bevy::prelude::*;
use bota_proto::{HeroId, ItemId};

use super::harness::{Client, Server};
use crate::tip_band::{ART, Art, Band};

/// A node's rectangle in the window's physical pixels.
fn rect_of(world: &World, node: Entity) -> Rect {
    let size = world.get::<ComputedNode>(node).expect("laid out").size();
    let centre = world
        .get::<bevy::ui::UiGlobalTransform>(node)
        .expect("placed")
        .translation;
    Rect::from_center_size(centre, size)
}

/// The bands of the popup now drawn, each with its art's rectangle if it has art.
fn bands(client: &mut Client) -> Vec<(Rect, Option<Rect>)> {
    let world = client.world_mut();
    let found: Vec<(Entity, Option<Entity>)> = world
        .query_filtered::<(Entity, &InheritedVisibility, &ComputedNode), With<Band>>()
        .iter(world)
        .filter(|(_, seen, node)| seen.get() && node.size() != Vec2::ZERO)
        .map(|(band, ..)| (band, None))
        .collect();
    found
        .into_iter()
        .map(|(band, _)| {
            let art = world.get::<Children>(band).and_then(|children| {
                children
                    .iter()
                    .find(|child| world.get::<Art>(*child).is_some())
            });
            (rect_of(world, band), art.map(|art| rect_of(world, art)))
        })
        .collect()
}

/// Items of each header, and the abilities of a hero: every band is as wide as the text blocks
/// under it, 28 high at the window's scale, and its art starts at the band's left edge at the art's
/// own size.
#[test]
fn every_header_band_of_a_tooltip_carries_its_art_from_the_bands_left_end_at_its_own_size() {
    let hero = bota_core::hero_def(HeroId::SHADOW_FIEND).expect("a hero");
    let mut specs: Vec<String> = [ItemId::MANGO, ItemId::BLINK_DAGGER, ItemId::WRAITH_BAND]
        .iter()
        .map(|item| format!("shop:{}", item.0))
        .collect();
    specs.extend(
        hero.abilities
            .iter()
            .map(|id| format!("ability:{}:1", id.0)),
    );
    for window in ["1920x1080", "2877x1642", "1280x720"] {
        let server = Server::start(1, &[]);
        let mut client = Client::join(
            &server,
            HeroId::SHADOW_FIEND,
            &["--tip", "shop:42", "--window", window],
        );
        let mut seen = 0;
        for spec in &specs {
            client.world_mut().resource_mut::<crate::Args>().tip = None;
            client.frames(2);
            client.world_mut().resource_mut::<crate::Args>().tip = Some(spec.clone());
            client.frames(4);
            let drawn = bands(&mut client);
            // A Use item and an Active one have their header.
            if matches!(spec.as_str(), "shop:42" | "shop:31") {
                assert!(!drawn.is_empty(), "{window} {spec}: no header band drawn");
            }
            for (band, art) in drawn {
                seen += 1;
                let art = art.unwrap_or_else(|| panic!("{window} {spec}: a band with no art"));
                // The window's scale as the art's width gives it (heights round to whole pixels).
                let scale = art.width() / ART.x;
                assert!(
                    (art.min.x - band.min.x).abs() <= 1.0,
                    "{window} {spec}: the art starts at {} and the band at {}",
                    art.min.x,
                    band.min.x
                );
                assert!(
                    (art.min.y - band.min.y).abs() <= 1.0,
                    "{window} {spec}: the art's top"
                );
                assert!(
                    (art.size() - ART * scale).abs().max_element() <= 1.5,
                    "{window} {spec}: the art's size {:?}, not {:?}",
                    art.size(),
                    ART * scale
                );
                assert!(
                    (band.height() - art.height()).abs() <= 1.0,
                    "{window} {spec}: a band {} high for art {} high",
                    band.height(),
                    art.height()
                );
                assert!(
                    band.width() <= art.width() + 1.0 && band.width() >= art.width() - 6.0 * scale,
                    "{window} {spec}: a band of {} for art of {}",
                    band.width(),
                    art.width()
                );
            }
        }
        assert!(
            seen >= 2,
            "{window}: bands were drawn for the items and abilities ({seen})"
        );
    }
}
