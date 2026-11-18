//! A texture softened on purpose, at import: a Gaussian blur of its 8-bit RGBA pixels with a
//! fixed grain over its colour. The menu's backdrop is such a painting; made once here, no client
//! spends a frame on it, in a browser or natively.

use crate::catalog::Soften;

/// Blurs `pixels` (RGBA8, `width` by `height`, rows from the top) by a Gaussian of `soften.sigma`
/// pixels, edges repeating, and lays `soften.grain` levels of fixed noise over the colour (not the
/// alpha). Panics when `pixels` are not `width` by `height` RGBA8 pixels, or `soften` is beyond
/// what a manifest line may ask for.
pub fn apply(pixels: &mut [u8], width: usize, height: usize, soften: Soften) {
    assert_eq!(pixels.len(), width * height * 4, "RGBA8 pixels");
    assert!(
        soften.sigma > 0.0 && soften.sigma <= Soften::MAX_SIGMA,
        "a Gaussian within the manifest's bound"
    );
    assert!(soften.grain <= Soften::MAX_GRAIN, "grain within the bound");
    let radius = (soften.sigma * 3.0).ceil() as i32;
    let kernel = kernel(soften.sigma, radius);
    let mut across = vec![0.0_f32; pixels.len()];
    for y in 0..height {
        for x in 0..width {
            for channel in 0..4 {
                across[(y * width + x) * 4 + channel] = taps(&kernel, radius, x, width, |at| {
                    f32::from(pixels[(y * width + at) * 4 + channel])
                });
            }
        }
    }
    let grain = i32::from(soften.grain);
    for y in 0..height {
        for x in 0..width {
            let noise = grain_at(x, y) * grain;
            for channel in 0..4 {
                let sum = taps(&kernel, radius, y, height, |at| {
                    across[(at * width + x) * 4 + channel]
                });
                let noise = if channel < 3 { noise } else { 0 };
                pixels[(y * width + x) * 4 + channel] =
                    (sum.round() as i32 + noise).clamp(0, 255) as u8;
            }
        }
    }
}

/// The Gaussian's weights over `-radius..=radius`, summing to one.
fn kernel(sigma: f32, radius: i32) -> Vec<f32> {
    let mut kernel: Vec<f32> = (-radius..=radius)
        .map(|at| (-((at * at) as f32) / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|weight| *weight /= total);
    kernel
}

/// The weighted sum of the values `value` reads around `centre` of a line of `length`, the line's
/// ends repeating.
fn taps(
    kernel: &[f32],
    radius: i32,
    centre: usize,
    length: usize,
    value: impl Fn(usize) -> f32,
) -> f32 {
    kernel
        .iter()
        .zip(-radius..=radius)
        .map(|(weight, at)| {
            let at = (centre as i32 + at).clamp(0, length as i32 - 1) as usize;
            weight * value(at)
        })
        .sum()
}

/// Fixed noise of a pixel, -1, 0 or 1: the same picture every import (no random seed to log).
fn grain_at(x: usize, y: usize) -> i32 {
    let mut hash = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x2C1B_3C6D);
    hash ^= hash >> 12;
    (hash % 3) as i32 - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first colour channel's largest step between horizontal neighbours.
    fn sharpest_step(pixels: &[u8], width: usize) -> i32 {
        (0..pixels.len() / 4 - 1)
            .filter(|at| (at + 1) % width != 0)
            .map(|at| (i32::from(pixels[at * 4]) - i32::from(pixels[(at + 1) * 4])).abs())
            .max()
            .expect("pixels")
    }

    /// A checkerboard of 4 pixel blocks, as compression leaves a flat painting.
    fn blocks(size: usize) -> Vec<u8> {
        (0..size * size)
            .flat_map(|at| {
                let on = (at % size / 4 + at / size / 4).is_multiple_of(2);
                let level = if on { 200 } else { 40 };
                [level, level, level, 255]
            })
            .collect()
    }

    const SOFTEN: Soften = Soften {
        sigma: 1.4,
        grain: 2,
    };

    #[test]
    fn softening_turns_the_hard_edges_of_blocks_into_gentle_steps() {
        let mut pixels = blocks(16);
        assert_eq!(sharpest_step(&pixels, 16), 160, "hard edges before");
        apply(&mut pixels, 16, 16, SOFTEN);
        assert!(
            sharpest_step(&pixels, 16) < 80,
            "no edge keeps more than half its step: {}",
            sharpest_step(&pixels, 16)
        );
    }

    #[test]
    fn softening_leaves_an_opaque_picture_opaque_and_the_grain_in_the_colour_only() {
        let mut pixels = blocks(16);
        apply(&mut pixels, 16, 16, SOFTEN);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
        let mut plain = blocks(16);
        apply(
            &mut plain,
            16,
            16,
            Soften {
                sigma: 1.4,
                grain: 0,
            },
        );
        let most = pixels
            .iter()
            .zip(&plain)
            .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
            .max()
            .expect("pixels");
        assert_eq!(most, 2, "the grain moves a level by at most its amplitude");
    }

    #[test]
    fn softening_makes_the_same_picture_every_time() {
        let (mut first, mut second) = (blocks(16), blocks(16));
        apply(&mut first, 16, 16, SOFTEN);
        apply(&mut second, 16, 16, SOFTEN);
        assert_eq!(first, second);
    }

    #[test]
    fn softening_keeps_a_flat_picture_flat_but_for_the_grain() {
        let mut pixels: Vec<u8> = [90, 90, 90, 255].repeat(8 * 8);
        apply(
            &mut pixels,
            8,
            8,
            Soften {
                sigma: 2.0,
                grain: 0,
            },
        );
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [90, 90, 90, 255])
        );
    }
}
