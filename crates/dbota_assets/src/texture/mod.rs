//! Textures: Valve's `vtex_c` decoded to the pixel data they hold, and written as KTX2.

mod convert;
mod format;
mod ktx2;
mod rgba;
mod sheet;
mod vtex;

pub use format::PixelFormat;
pub use ktx2::{Encoding, to_plain as ktx2_to_plain};
pub use vtex::Texture;

/// What a material reads a texture as, which decides its encoding and the block format an image
/// made from it is compressed to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// Colours on the sRGB curve: base colour, emission.
    Colour,
    /// A tangent-space normal.
    Normal,
    /// Plain numbers: masks, occlusion, roughness, metalness.
    Data,
}

impl Role {
    pub fn encoding(self) -> Encoding {
        match self {
            Self::Colour => Encoding::Srgb,
            Self::Normal | Self::Data => Encoding::Linear,
        }
    }
}
