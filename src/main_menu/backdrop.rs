//! The main menu's background: Dota's dashboard background of the player's side, under the
//! pages and darkened at the sides and the foot so the panels over it read.
//!
//! Dota draws it as a small 3D scene (`maps/backgrounds/dashboard_background_crownfall_creeps_
//! radiant` / `_dire`); until dbota renders that, the menu shows the still art the install has of
//! the side's Ancient. [`Backdrop`] is where the scene takes over.

use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::kit::Kit;
use super::view::Side;
use crate::theme;
use crate::ui::Cover;

/// What stands behind the menu's pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// The install's still art of the side's Ancient.
    Still(Side),
}

impl Backdrop {
    /// The background of the side the player last played.
    pub fn of(side: Side) -> Backdrop {
        Backdrop::Still(side)
    }
}

/// How far the still's painting is softened on purpose, in its own pixels (the standard deviation
/// of a Gaussian). The install holds the Ancient's painting at 2048 by 1024, block-compressed, which
/// a 4K window magnifies two times over: unsoftened, its compression blocks show as a pixel grid.
/// Softened, it reads as depth of field behind the hero and the panels, at any window size.
const SOFTNESS: f32 = 1.4;
/// The grain laid over the softened picture, in levels of 255 either way: it keeps the dark
/// gradients (the darkening over them as well) from banding.
const GRAIN: i32 = 2;

/// Marks the backdrop's picture: its image is softened once it has loaded (see [`SOFTNESS`]).
#[derive(Component)]
pub struct Soft;

/// The images already softened: an image is changed in place, once, however often the page that
/// shows it is drawn.
#[derive(Resource, Default)]
pub struct Softened(HashSet<AssetId<Image>>);

impl Softened {
    pub fn has(&self, image: AssetId<Image>) -> bool {
        self.0.contains(&image)
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Softened>().add_systems(Update, soften);
}

fn soften(
    pictures: Query<&ImageNode, With<Soft>>,
    mut images: ResMut<Assets<Image>>,
    mut done: ResMut<Softened>,
) {
    for picture in &pictures {
        let id = picture.image.id();
        if done.has(id) {
            continue;
        }
        let Some(mut image) = images.get_mut(id) else {
            continue;
        };
        // Sampled linearly whatever the engine's default: a nearest sampler would show the
        // painting's every pixel.
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
        blur(&mut image, SOFTNESS, GRAIN);
        done.0.insert(id);
    }
}

/// Blurs an 8-bit RGBA image by a Gaussian of `sigma` pixels (edges repeat) and lays `grain`
/// levels of fixed noise over its colour. Anything else is left as it is.
fn blur(image: &mut Image, sigma: f32, grain: i32) {
    if !matches!(
        image.texture_descriptor.format,
        TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
    ) {
        return;
    }
    let (width, height) = (
        image.texture_descriptor.size.width as usize,
        image.texture_descriptor.size.height as usize,
    );
    let Some(data) = image
        .data
        .as_mut()
        .filter(|d| d.len() == width * height * 4)
    else {
        return;
    };
    let radius = (sigma * 3.0).ceil() as i32;
    let mut kernel: Vec<f32> = (-radius..=radius)
        .map(|at| (-((at * at) as f32) / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|weight| *weight /= total);
    let mut across = vec![0.0_f32; data.len()];
    for y in 0..height {
        for x in 0..width {
            for channel in 0..4 {
                let sum: f32 = kernel
                    .iter()
                    .zip(-radius..=radius)
                    .map(|(weight, at)| {
                        let sx = (x as i32 + at).clamp(0, width as i32 - 1) as usize;
                        weight * f32::from(data[(y * width + sx) * 4 + channel])
                    })
                    .sum();
                across[(y * width + x) * 4 + channel] = sum;
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            for channel in 0..4 {
                let sum: f32 = kernel
                    .iter()
                    .zip(-radius..=radius)
                    .map(|(weight, at)| {
                        let sy = (y as i32 + at).clamp(0, height as i32 - 1) as usize;
                        weight * across[(sy * width + x) * 4 + channel]
                    })
                    .sum();
                let at = (y * width + x) * 4 + channel;
                let noise = if channel < 3 {
                    grain_at(x, y) * grain
                } else {
                    0
                };
                data[at] = (sum.round() as i32 + noise).clamp(0, 255) as u8;
            }
        }
    }
}

/// Fixed noise of a pixel, -1, 0 or 1: the same picture every run (no random seed to log).
fn grain_at(x: usize, y: usize) -> i32 {
    let mut hash = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x2C1B_3C6D);
    hash ^= hash >> 12;
    (hash % 3) as i32 - 1
}

/// The darkening over the background, left to right and top to bottom: `(alpha, at %)`, the
/// alphas as the mockups give them (see [`theme::dim`]).
const ACROSS: [(f32, f32); 4] = [(0.55, 0.0), (0.05, 38.0), (0.05, 62.0), (0.70, 100.0)];
const DOWN: [(f32, f32); 2] = [(0.10, 0.0), (0.75, 100.0)];

/// The backdrop over the whole of `root` from `top` down, under everything spawned after it.
pub fn draw(kit: &mut Kit, root: Entity, backdrop: Backdrop, top: f32) {
    let Backdrop::Still(side) = backdrop;
    let accent = match side {
        Side::Radiant => theme::RESULT_RADIANT_ROW,
        Side::Dire => theme::RESULT_DIRE_ROW,
    };
    let area = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(top),
                bottom: px(0),
                ..default()
            },
            BackgroundColor(theme::GROUND),
            // A side-coloured backdrop fills the first menu frame while its full-size art decodes.
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::new(accent, percent(0.0)),
                ColorStop::new(theme::GROUND, percent(100.0)),
            ])),
            Pickable::IGNORE,
            Name::new("menu background"),
            ChildOf(root),
        ))
        .id();
    let art = match side {
        Side::Radiant => "menu.background.radiant",
        Side::Dire => "menu.background.dire",
    };
    let picture = kit.art(area, art, crate::ui::fill());
    kit.commands.entity(picture).insert((Cover, Soft));
    for (stops, across) in [(&ACROSS[..], true), (&DOWN[..], false)] {
        let stops = stops
            .iter()
            .map(|(alpha, at)| {
                ColorStop::new(theme::GROUND.with_alpha(theme::dim(*alpha)), percent(*at))
            })
            .collect();
        let gradient = if across {
            LinearGradient::to_right(stops)
        } else {
            LinearGradient::to_bottom(stops)
        };
        kit.commands.spawn((
            crate::ui::fill(),
            BackgroundGradient::from(gradient),
            Pickable::IGNORE,
            ChildOf(area),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension};

    /// The largest step between neighbouring pixels of the first colour channel.
    fn sharpest_step(image: &Image) -> i32 {
        let data = image.data.as_ref().expect("pixels");
        let width = image.texture_descriptor.size.width as usize;
        (0..data.len() / 4 - 1)
            .filter(|at| (at + 1) % width != 0)
            .map(|at| (i32::from(data[at * 4]) - i32::from(data[(at + 1) * 4])).abs())
            .max()
            .expect("pixels")
    }

    /// A checkerboard of the compression's blocks is softened to a gentle ramp, grain included.
    #[test]
    fn softening_turns_the_hard_edges_of_blocks_into_gentle_steps() {
        let size = 16;
        let data: Vec<u8> = (0..size * size)
            .flat_map(|at| {
                let on = (at % size / 4 + at / size / 4) % 2 == 0;
                let level = if on { 200 } else { 40 };
                [level, level, level, 255]
            })
            .collect();
        let mut image = Image::new(
            Extent3d {
                width: size as u32,
                height: size as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        assert_eq!(sharpest_step(&image), 160, "hard edges before");
        blur(&mut image, SOFTNESS, GRAIN);
        assert!(
            sharpest_step(&image) < 80,
            "no edge keeps more than half its step: {}",
            sharpest_step(&image)
        );
    }
}
