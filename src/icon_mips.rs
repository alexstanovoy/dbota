//! Crisp icons: the cache's icons and interface art are single-level PNGs,
//! and an icon drawn smaller than it is (an ability's 128 texels in a buff
//! chip or a 70-pixel slot) was sampled from the top level alone, which skips
//! texels and shimmers into jagged, pixelated edges. As each such image
//! loads it is given its mip chain (each level a coverage-weighted 2×2 box of
//! the one above, so a transparent edge keeps its colour), and the sampler
//! blends levels, so a downscale averages the texels it covers, as the
//! install's own interface does. Done on the CPU, so it is the same on the
//! desktop and in both browser backends.

use bevy::{
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{TextureDimension, TextureFormat},
};

/// The folders of the cache whose images are drawn as icons and interface art.
const FOLDERS: [&str; 3] = ["icons/", "hud/", "minimap/"];
/// Anisotropic samples for a slot drawn at a slant: none are, but a node
/// turned on the minimap is.
const ANISOTROPY: u16 = 4;

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, mip_icons.after(bevy::asset::AssetEventSystems));
}

fn mip_icons(
    mut events: MessageReader<AssetEvent<Image>>,
    mut images: ResMut<Assets<Image>>,
    server: Res<AssetServer>,
) {
    for event in events.read() {
        let AssetEvent::Added { id } = event else {
            continue;
        };
        let icon = server.get_path(*id).is_some_and(|path| {
            path.source().as_str() == Some(crate::CACHE_SOURCE)
                && FOLDERS.iter().any(|dir| path.path().starts_with(dir))
        });
        if !icon {
            continue;
        }
        if let Some(image) = images.get_mut_untracked(*id) {
            add_mips(image);
        }
    }
}

/// Gives a single-level 8-bit RGBA image its whole chain of levels, largest
/// first, and a sampler that blends them; any other image is left as it is.
fn add_mips(image: &mut Image) {
    let d = &image.texture_descriptor;
    let plain = matches!(
        d.format,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
    );
    if !plain
        || d.mip_level_count != 1
        || d.dimension != TextureDimension::D2
        || d.size.depth_or_array_layers != 1
    {
        return;
    }
    let (mut w, mut h) = (d.size.width as usize, d.size.height as usize);
    let Some(mut data) = image.data.take() else {
        return;
    };
    if data.len() != w * h * 4 {
        image.data = Some(data);
        return;
    }
    let mut levels = 1;
    let mut from = 0;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let next = halve(&data[from..from + w * h * 4], (w, h), (nw, nh));
        from = data.len();
        data.extend_from_slice(&next);
        (w, h) = (nw, nh);
        levels += 1;
    }
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: ANISOTROPY,
        ..ImageSamplerDescriptor::linear()
    });
}

/// The next level: each texel the box of the texels it covers, weighted by
/// their opacity so what is clear does not tint what is not.
fn halve(level: &[u8], (w, h): (usize, usize), (nw, nh): (usize, usize)) -> Vec<u8> {
    let mut out = Vec::with_capacity(nw * nh * 4);
    for y in 0..nh {
        for x in 0..nw {
            let (x0, y0) = (2 * x, 2 * y);
            let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
            let texels = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
            let (mut sum, mut alpha) = ([0u32; 3], 0u32);
            for (tx, ty) in texels {
                let t = &level[(ty * w + tx) * 4..(ty * w + tx) * 4 + 4];
                let a = u32::from(t[3]);
                alpha += a;
                for c in 0..3 {
                    sum[c] += u32::from(t[c]) * a;
                }
            }
            for c in sum {
                out.push(if alpha == 0 { 0 } else { (c / alpha) as u8 });
            }
            out.push(((alpha + 2) / 4) as u8);
        }
    }
    out
}
