//! The main menu's backdrop from a real import: the install's painting is 2048 by 1024, which a 4K
//! window magnifies two times over, so the importer softens it once into the cache and the menu
//! draws what it finds there, sampled linearly, without spending a frame on it.

use bevy::image::{ImageFilterMode, ImageSampler};
use bevy::prelude::*;
use dbota_import::catalog::{self, Line, Soften};
use heatel::texture::Texture;
use heatel::{DirSource, Vpk};

use super::harness::{Client, game_dir};
use super::menu::{local_menu, resize_menu_window};
use crate::main_menu::backdrop::Painting;

/// The largest window the menu is laid out for.
const LARGEST_WINDOW: UVec2 = UVec2::new(3840, 2160);
/// Frames the menu stays open after its backdrop is drawn, for what would change it afterwards.
const FRAMES_OPEN: usize = 20;
/// The most a softened painting differs from its original on average, in levels of 255: it is a
/// blur of 1.4 pixels and a grain of 2 levels, not another picture.
const MOST_MEAN_CHANGE: f64 = 6.0;

/// The backdrop as first drawn: its image and its pixels the frame they were available.
struct Backdrop {
    image: Handle<Image>,
    pixels: Vec<u8>,
    width: u32,
}

fn backdrop_at_the_largest_window(client: &mut Client) -> Backdrop {
    resize_menu_window(client, LARGEST_WINDOW);
    let mut found = None;
    client.until("the backdrop drawn and cut to its frame", |c| {
        let world = c.world_mut();
        let image = world
            .query_filtered::<(&ImageNode, &InheritedVisibility), With<Painting>>()
            .iter(world)
            .find(|(picture, shown)| shown.get() && picture.rect.is_some())
            .map(|(picture, _)| picture.image.clone());
        found = image.and_then(|image| {
            let loaded = world.resource::<Assets<Image>>().get(&image)?;
            Some(Backdrop {
                pixels: loaded.data.clone()?,
                width: loaded.texture_descriptor.size.width,
                image,
            })
        });
        found.is_some()
    });
    found.expect("the backdrop")
}

/// The manifest's line that writes the cache file `image` was loaded from.
fn line_of(client: &Client, image: &Handle<Image>) -> catalog::File {
    let path = client
        .world()
        .resource::<AssetServer>()
        .get_path(image)
        .expect("the backdrop is loaded from a path");
    let loaded = path.path().to_string_lossy().into_owned();
    let catalog = catalog::parse(dbota_import::import::MANIFEST, dbota_import::import::IDS)
        .expect("the committed manifest");
    catalog
        .lines
        .into_iter()
        .find_map(|line| match line {
            Line::File(file) if file.out == loaded => Some(file),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no manifest line writes {loaded}"))
}

/// The painting as the install holds it, decoded and not softened.
fn raw_painting(file: &catalog::File) -> Vec<u8> {
    let source = DirSource::new(game_dir());
    let package = Vpk::open(&source, "pak01_dir.vpk").expect("the install's package");
    let bytes = package
        .read_file(&source, &file.src)
        .expect("the painting in the install");
    Texture::parse(&bytes)
        .expect("a texture")
        .to_rgba8()
        .expect("RGBA8 pixels")
}

/// The first colour channel's largest step between horizontal neighbours.
fn sharpest_step(pixels: &[u8], width: usize) -> i32 {
    (0..pixels.len() / 4 - 1)
        .filter(|at| (at + 1) % width != 0)
        .map(|at| (i32::from(pixels[at * 4]) - i32::from(pixels[(at + 1) * 4])).abs())
        .max()
        .expect("pixels")
}

/// What the importer's `soften=` makes of `raw`, which is what a menu on this build's cache holds.
fn softened(raw: &[u8], width: usize, how: Soften) -> Vec<u8> {
    let mut pixels = raw.to_vec();
    dbota_import::soften::apply(&mut pixels, width, raw.len() / 4 / width, how)
        .expect("the install's painting fits the import limit");
    pixels
}

/// The menu draws the painting its real import softened: in the cache the painting differs from
/// the install's own by a blur and a grain and nothing else, and it is the same from the frame it
/// is available to the last, so no frame of the menu does it over.
#[test]
fn the_menu_backdrop_is_the_install_painting_softened_by_the_import_and_never_again() {
    let mut client = local_menu();
    let backdrop = backdrop_at_the_largest_window(&mut client);
    let file = line_of(&client, &backdrop.image);
    let how = file.soften.expect("the backdrop's line asks to soften it");
    let raw = raw_painting(&file);
    let width = backdrop.width as usize;
    assert_eq!(raw.len(), backdrop.pixels.len(), "the same painting's size");

    assert_ne!(
        backdrop.pixels, raw,
        "the menu does not show the raw painting"
    );
    assert_eq!(
        backdrop.pixels,
        softened(&raw, width, how),
        "the menu shows the importer's softening of the install's painting, from its first frame"
    );
    let changed: u64 = backdrop
        .pixels
        .iter()
        .zip(&raw)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    let mean = changed as f64 / raw.len() as f64;
    assert!(
        mean > 0.0 && mean < MOST_MEAN_CHANGE,
        "a blur, not another picture: it changes a level by {mean:.2} on average"
    );
    assert!(
        sharpest_step(&backdrop.pixels, width) < sharpest_step(&raw, width),
        "the softened painting has no edge as hard as the install's"
    );

    client.step_frames(FRAMES_OPEN);
    let images = client.world().resource::<Assets<Image>>();
    let later = images.get(&backdrop.image).expect("the backdrop's image");
    assert_eq!(
        later.data.as_deref(),
        Some(&backdrop.pixels[..]),
        "{FRAMES_OPEN} frames later the painting is as it was"
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
