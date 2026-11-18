//! The textures a camera draws through (its main textures, their multisampled one, its depth)
//! kept for as long as the camera lives, for the cameras that draw less often than every frame:
//! the portrait's (at most 30 times a second) and the hero stage's. Bevy takes a view's textures
//! from its texture cache each frame the view is drawn and frees those no view took for three
//! frames; at a frame rate over 90 a portrait drawn 30 times a second found its textures freed
//! every time and made them anew: four textures created on its frames, which held a frame up to
//! 150 ms. On the frames such a camera draws nothing, its textures are taken from the cache as
//! the frame that draws them would take them, so they stay.
//!
//! Also counts the textures and buffers the renderer creates (Bevy's ids are handed out by one
//! counter each): `--bench` prints them, and a match's steady frames create none.

use std::sync::atomic::{AtomicU32, Ordering};

use bevy::{
    core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
    platform::collections::HashMap,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_resource::{
            BufferId, TextureDescriptor, TextureDimension, TextureFormat, TextureId, TextureUsages,
        },
        renderer::RenderDevice,
        texture::{TextureCache, update_texture_cache_system},
        view::ViewTarget,
    },
};

/// Marks a camera that draws less often than every frame: its textures are kept between its
/// frames.
#[derive(Component, Clone, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct KeepTargets;

/// Textures and buffers the renderer has created so far.
pub static TEXTURES_CREATED: AtomicU32 = AtomicU32::new(0);
pub static BUFFERS_CREATED: AtomicU32 = AtomicU32::new(0);

pub fn plugin(app: &mut App) {
    app.add_plugins(ExtractComponentPlugin::<KeepTargets>::default());
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render.add_systems(
        Render,
        (
            keep_targets.before(update_texture_cache_system),
            count_created,
        )
            .chain()
            .in_set(RenderSystems::Cleanup),
    );
}

/// The textures each kept camera drew through when it last drew, by its render entity.
#[derive(Default)]
struct Kept(HashMap<Entity, Vec<TextureDescriptor<'static>>>);

/// Records the textures of the kept cameras that drew this frame, and takes those of the ones
/// that did not from the cache, which so keeps them.
fn keep_targets(
    mut kept: Local<Kept>,
    mut cache: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    cameras: Query<
        (
            Entity,
            Has<ExtractedCamera>,
            Option<&ViewTarget>,
            Option<&Msaa>,
            Option<&Camera3d>,
        ),
        With<KeepTargets>,
    >,
) {
    kept.0.retain(|entity, _| cameras.contains(*entity));
    for (entity, drew, target, msaa, camera_3d) in &cameras {
        match (drew, target) {
            (true, Some(target)) => {
                let samples = msaa.map_or(1, Msaa::samples);
                let usage = camera_3d.map(|c| TextureUsages::from(c.depth_texture_usages));
                kept.0.insert(entity, descriptors(target, samples, usage));
            }
            (false, _) => {
                for descriptor in kept.0.get(&entity).into_iter().flatten() {
                    cache.get(&device, descriptor.clone());
                }
            }
            (true, None) => {}
        }
    }
}

/// The descriptors Bevy takes a view's textures from the cache by (`prepare_view_targets` and
/// `prepare_core_3d_depth_textures` of this Bevy version): the cache keys its textures by them.
fn descriptors(
    target: &ViewTarget,
    samples: u32,
    depth_usage: Option<TextureUsages>,
) -> Vec<TextureDescriptor<'static>> {
    let main = target.main_texture();
    let format = main.format();
    let view_formats: &'static [TextureFormat] = match format {
        TextureFormat::Bgra8Unorm => &[TextureFormat::Bgra8UnormSrgb],
        TextureFormat::Rgba8Unorm => &[TextureFormat::Rgba8UnormSrgb],
        _ => &[],
    };
    let plain = TextureDescriptor {
        label: None,
        size: main.size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: main.usage(),
        view_formats,
    };
    let mut all = vec![
        TextureDescriptor {
            label: Some("main_texture_a"),
            ..plain.clone()
        },
        TextureDescriptor {
            label: Some("main_texture_b"),
            ..plain.clone()
        },
    ];
    if samples > 1 {
        all.push(TextureDescriptor {
            label: Some("main_texture_sampled"),
            sample_count: samples,
            usage: TextureUsages::RENDER_ATTACHMENT,
            ..plain.clone()
        });
    }
    if let Some(usage) = depth_usage {
        all.push(TextureDescriptor {
            label: Some("view_depth_texture"),
            sample_count: samples,
            format: CORE_3D_DEPTH_FORMAT,
            usage,
            view_formats: &[],
            ..plain
        });
    }
    all
}

/// Adds what the renderer created since the last frame to the counts: each kind of id comes
/// from one counter, so the gap between two ids taken here (less this one) is what was made.
fn count_created(mut last: Local<Option<(u32, u32)>>) {
    let texture = u32::from(std::num::NonZero::<u32>::from(TextureId::new()));
    let buffer = u32::from(std::num::NonZero::<u32>::from(BufferId::new()));
    if let Some((last_texture, last_buffer)) = *last {
        TEXTURES_CREATED.fetch_add(texture - last_texture - 1, Ordering::Relaxed);
        BUFFERS_CREATED.fetch_add(buffer - last_buffer - 1, Ordering::Relaxed);
    }
    *last = Some((texture, buffer));
}
