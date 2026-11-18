//! The main menu's backdrop at the largest window the menu supports: the install's painting is
//! 2048 by 1024, so unless it is softened on purpose it must not be magnified past a bound, and it
//! is sampled linearly either way.

use bevy::image::{ImageFilterMode, ImageSampler};
use bevy::prelude::*;

use super::harness::Client;
use super::menu::{local_menu, resize_menu_window};
use crate::main_menu::backdrop::{Soft, Softened};

/// The largest window the menu is laid out for.
const LARGEST_WINDOW: UVec2 = UVec2::new(3840, 2160);
/// The most a source pixel may be magnified (window pixels to a pixel of the picture) before the
/// picture shows its pixels: a quarter more than its own size.
const MAGNIFICATION_BOUND: f32 = 1.25;

struct Backdrop {
    image: Handle<Image>,
    /// Window pixels across a pixel of the picture, as drawn.
    magnification: f32,
}

fn backdrop_at_the_largest_window(client: &mut Client) -> Backdrop {
    resize_menu_window(client, LARGEST_WINDOW);
    let mut found = None;
    client.until("the backdrop drawn and cut to its frame", |c| {
        let world = c.world_mut();
        found = world
            .query_filtered::<(&ImageNode, &ComputedNode, &InheritedVisibility), With<Soft>>()
            .iter(world)
            .find(|(.., shown)| shown.get())
            .and_then(|(picture, node, _)| {
                let source = picture.rect?;
                world
                    .resource::<Assets<Image>>()
                    .contains(&picture.image)
                    .then(|| Backdrop {
                        image: picture.image.clone(),
                        magnification: node.size().x / source.width(),
                    })
            });
        found.is_some()
    });
    found.expect("the backdrop")
}

/// At 4K the painting is either magnified little or softened on purpose.
#[test]
fn the_menu_backdrop_is_not_magnified_past_the_bound_unless_it_is_softened_on_purpose() {
    let mut client = local_menu();
    let backdrop = backdrop_at_the_largest_window(&mut client);
    client.step_frames(3);
    let softened = client
        .world()
        .resource::<Softened>()
        .has(backdrop.image.id());
    assert!(
        backdrop.magnification <= MAGNIFICATION_BOUND || softened,
        "the backdrop is magnified {:.2} times at {LARGEST_WINDOW} and not softened on purpose",
        backdrop.magnification
    );
}

/// Smooth in either direction, with no nearest filter showing the picture's pixels.
#[test]
fn the_menu_backdrop_is_sampled_linearly() {
    let mut client = local_menu();
    let backdrop = backdrop_at_the_largest_window(&mut client);
    client.step_frames(3);
    let image = client
        .world()
        .resource::<Assets<Image>>()
        .get(&backdrop.image)
        .expect("the backdrop's image");
    let ImageSampler::Descriptor(sampler) = &image.sampler else {
        panic!("the backdrop names its sampler: {:?}", image.sampler);
    };
    assert_eq!(sampler.mag_filter, ImageFilterMode::Linear, "magnified");
    assert_eq!(sampler.min_filter, ImageFilterMode::Linear, "minified");
}
