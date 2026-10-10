//! Ground plane (tiled texture from the cache, or a procedural checker) and lighting.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor},
    light::CascadeShadowConfigBuilder,
    math::Affine2,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::{Args, fallback::FoundAssets};

const GROUND_SIZE: f32 = 10_000.0;
/// World units covered by one texture repeat.
const TILE_WORLD: f32 = 256.0;
/// Procedural texture: one texel per world unit at TILE_WORLD, i.e. ~1.6 physical px per
/// texel at the default zoom, so the missing mipmaps barely alias.
const CHECKER_TEXELS: u32 = 256;
const CHECKER_CELL: u32 = 32;

pub fn plugin(app: &mut App) {
    app.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
        ..default()
    })
    .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)))
    .add_systems(Startup, spawn_ground_and_light);
}

fn repeat_sampler() -> ImageSamplerDescriptor {
    ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 8,
        ..ImageSamplerDescriptor::linear()
    }
}

fn spawn_ground_and_light(
    mut commands: Commands,
    args: Res<Args>,
    found: Res<FoundAssets>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let texture = match &found.ground {
        // Sampler settings for a file-backed image are loader settings, not post-load edits.
        Some(path) => asset_server
            .load_builder()
            .with_settings(|s: &mut ImageLoaderSettings| {
                s.sampler = ImageSampler::Descriptor(repeat_sampler());
            })
            .load(path.clone()),
        None => images.add(checker_image()),
    };
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(GROUND_SIZE, GROUND_SIZE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(texture),
            uv_transform: Affine2::from_scale(Vec2::splat(GROUND_SIZE / TILE_WORLD)),
            perceptual_roughness: 0.95,
            reflectance: 0.1,
            ..default()
        })),
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 6_000.0,
            shadow_maps_enabled: !args.no_shadows,
            ..default()
        },
        // Cascade bounds are view-space depths: the ground sits ~2000-4000 units away.
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            minimum_distance: 500.0,
            first_cascade_far_bound: 3_000.0,
            maximum_distance: 7_000.0,
            ..default()
        }
        .build(),
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -0.9, 0.0)),
    ));
}

fn checker_image() -> Image {
    let n = CHECKER_TEXELS;
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let dark = ((x / CHECKER_CELL) + (y / CHECKER_CELL)).is_multiple_of(2);
            // Cheap hash noise so the tiling is not perfectly flat.
            let h = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 13;
            let base: u8 = if dark { 52 } else { 66 };
            let v = base + h as u8;
            data.extend_from_slice(&[v / 2 + 10, v + 18, v / 2 + 4, 255]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: n,
            height: n,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(repeat_sampler());
    image
}
