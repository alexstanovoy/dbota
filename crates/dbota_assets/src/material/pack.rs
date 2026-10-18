//! Images made from a texture's pixels rather than the texture itself.

/// An image made from a texture's pixels instead of the texture itself, as Source2Viewer 20
/// wrote the materials' inputs: it unpacked each channel a glTF slot wants into an image of
/// its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pack {
    /// One channel (0 red .. 3 alpha) as a grey image.
    Channel(u8),
    /// Occlusion/roughness/metallic of a `g_tMasks1`: no occlusion (red 255), fully rough
    /// (green 255), metallic as the blue channel, opaque. Written to [`orm_uri`] of the
    /// masks' file name.
    Orm,
    /// The colour channels, opaque.
    Rgb,
}

impl Pack {
    /// Rewrites RGBA8 pixels of the texture into the packed image.
    pub fn apply(self, rgba8: &mut [u8]) {
        for px in rgba8.as_chunks_mut::<4>().0 {
            match self {
                Self::Channel(c) => {
                    let v = px[usize::from(c & 3)];
                    *px = [v, v, v, 255];
                }
                Self::Orm => *px = [255, 255, px[2], 255],
                Self::Rgb => px[3] = 255,
            }
        }
    }
}

/// The file name of the occlusion/roughness/metallic image made from the masks written as
/// `masks_uri` (`x.png` becomes `x_orm.png`).
pub fn orm_uri(masks_uri: &str) -> String {
    match masks_uri.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}_orm.{ext}"),
        None => format!("{masks_uri}_orm"),
    }
}
