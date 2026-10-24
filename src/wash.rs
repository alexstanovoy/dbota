//! A picture drawn through Dota's panel filters (`saturation`, `brightness`,
//! `wash-color`), as Dota draws a dead hero's portrait (`.Dead
//! DOTAHeroImage`): an interface material over a cache image.

use bevy::{
    asset::embedded_asset, prelude::*, render::render_resource::AsBindGroup, shader::ShaderRef,
};

/// A picture through Dota's filters.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct Washed {
    /// x: the saturation kept, 0 grey to 1 as drawn; y: the brightness
    /// factor.
    #[uniform(0)]
    pub levels: Vec4,
    /// The colour the filtered picture is multiplied by.
    #[uniform(0)]
    pub wash: LinearRgba,
    /// The part of the picture drawn, in its texture coordinates: the
    /// corner's x and y, then the width and the height.
    #[uniform(0)]
    pub cut: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub picture: Handle<Image>,
}

impl UiMaterial for Washed {
    fn fragment_shader() -> ShaderRef {
        "embedded://dbota/wash.wgsl".into()
    }
}

impl Washed {
    /// The whole picture.
    pub const WHOLE: Vec4 = Vec4::new(0.0, 0.0, 1.0, 1.0);

    /// Dota's dead hero: no colour, washed #898989.
    pub fn dead(picture: Handle<Image>) -> Washed {
        Washed {
            levels: Vec4::new(0.0, 1.0, 0.0, 0.0),
            wash: Color::srgb_u8(0x89, 0x89, 0x89).to_linear(),
            cut: Washed::WHOLE,
            picture,
        }
    }
}

pub fn plugin(app: &mut App) {
    embedded_asset!(app, "wash.wgsl");
    app.add_plugins(UiMaterialPlugin::<Washed>::default());
}
