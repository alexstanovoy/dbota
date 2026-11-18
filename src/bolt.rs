//! The small lightning mark beside the number of a critical strike: Dota's overhead alert for one
//! draws a thin outlined bolt next to the digits (`materials/particle/msg/msg_01`, tinted by the
//! alert's colour). Ours is rasterised from its outline at start, white inside a black stroke, so
//! a tint colours the inside only.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

/// The picture's size in pixels: as drawn it is no larger than a digit's height.
pub const WIDTH: u32 = 24;
pub const HEIGHT: u32 = 48;
/// Samples a side of a pixel, for the edges' smoothness.
const SAMPLES: u32 = 4;
/// The stroke's width in pixels.
const STROKE: f32 = 1.4;
/// The bolt's corners in the unit square, y down: its top right, down to the left, across, down to
/// the point and back up the right side.
const CORNERS: [(f32, f32); 7] = [
    (0.80, 0.04),
    (0.16, 0.58),
    (0.46, 0.58),
    (0.28, 0.96),
    (0.90, 0.36),
    (0.58, 0.36),
    (0.96, 0.04),
];

/// Whether a point (unit square) is inside the bolt (even-odd rule).
fn inside((x, y): (f32, f32)) -> bool {
    let mut inside = false;
    for (i, &(x0, y0)) in CORNERS.iter().enumerate() {
        let (x1, y1) = CORNERS[(i + 1) % CORNERS.len()];
        if (y0 > y) != (y1 > y) && x < x0 + (y - y0) / (y1 - y0) * (x1 - x0) {
            inside = !inside;
        }
    }
    inside
}

/// How far a point (unit square) is from the nearest side, in pixels.
fn edge_distance((x, y): (f32, f32)) -> f32 {
    let scale = Vec2::new(WIDTH as f32, HEIGHT as f32);
    let p = Vec2::new(x, y) * scale;
    CORNERS
        .iter()
        .enumerate()
        .map(|(i, &(x0, y0))| {
            let a = Vec2::new(x0, y0) * scale;
            let b = Vec2::from(CORNERS[(i + 1) % CORNERS.len()]) * scale;
            let along = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
            p.distance(a + (b - a) * along)
        })
        .fold(f32::MAX, f32::min)
}

/// The bolt as a picture: white inside, black round it, clear beyond.
pub fn image() -> Image {
    let mut data = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for row in 0..HEIGHT {
        for column in 0..WIDTH {
            let (mut white, mut solid) = (0, 0);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let at = (
                        (column as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / WIDTH as f32,
                        (row as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / HEIGHT as f32,
                    );
                    let within = inside(at);
                    white += u32::from(within && edge_distance(at) >= STROKE * 0.5);
                    solid += u32::from(within || edge_distance(at) < STROKE * 0.5);
                }
            }
            let samples = (SAMPLES * SAMPLES) as f32;
            let shade = if solid == 0 {
                0.0
            } else {
                white as f32 / solid as f32
            };
            let byte = |share: f32| (share * 255.0).round() as u8;
            data.extend([
                byte(shade),
                byte(shade),
                byte(shade),
                byte(solid as f32 / samples),
            ]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}
