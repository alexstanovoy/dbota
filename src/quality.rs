//! What the picture costs, as the settings ask (see [`Quality`]): the
//! sun's shadows, anti-aliasing, the 3D scene drawn at a share of the
//! window's resolution, a cap on texture size, and a frame cap. With
//! `auto_quality`, the settings are lowered a step at a time while frames
//! run slower than the frame cap (or 30 fps) allows.

use bevy::{
    anti_alias::{
        fxaa::Fxaa,
        smaa::{Smaa, SmaaPreset},
    },
    camera::{ImageRenderTarget, RenderTarget},
    light::{CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui::IsDefaultUiCamera,
    window::{PrimaryWindow, WindowRef},
};

use crate::camera::RtsCamera;
use crate::config::{AntiAlias, Settings};

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (shadows, anti_alias).run_if(resource_changed::<Settings>),
    )
    .add_systems(Update, (auto, scale))
    .add_systems(
        PostUpdate,
        shrink_textures.after(bevy::asset::AssetEventSystems),
    );
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Last, cap_frames);
}

/// The sun's cascades at a shadow level: one close to the camera when low.
/// WebGL2 draws one cascade only (Bevy's limit there), so it gets the one
/// that reaches as far as two would; Bevy would draw just the near one.
pub fn cascades(shadows: u8) -> CascadeShadowConfig {
    let wanted = if shadows >= 2 { 2 } else { 1 };
    CascadeShadowConfigBuilder {
        num_cascades: wanted.min(bevy::pbr::MAX_CASCADES_PER_LIGHT),
        first_cascade_far_bound: 3_000.0,
        maximum_distance: 8_000.0,
        ..default()
    }
    .build()
}

/// Texels of a side of each shadow cascade at a shadow level.
fn shadow_map_size(shadows: u8) -> usize {
    if shadows >= 2 { 2048 } else { 1024 }
}

fn shadows(
    settings: Res<Settings>,
    mut map: ResMut<DirectionalLightShadowMap>,
    mut suns: Query<(&mut DirectionalLight, &mut CascadeShadowConfig)>,
) {
    let level = settings.quality.shadows;
    let on = level > 0;
    let size = shadow_map_size(level);
    if map.size != size {
        map.size = size;
    }
    for (mut sun, mut config) in &mut suns {
        if sun.shadow_maps_enabled != on {
            sun.shadow_maps_enabled = on;
        }
        if config.bounds.len() != cascades(level).bounds.len() {
            *config = cascades(level);
        }
    }
}

/// The multisampling the settings ask for; the screen-space filters need
/// none.
pub fn msaa_of(settings: &Settings) -> Msaa {
    match settings.quality.anti_alias {
        AntiAlias::Msaa4 => Msaa::Sample4,
        AntiAlias::Msaa2 => Msaa::Sample2,
        _ => Msaa::Off,
    }
}

/// Sets the scene camera's multisampling and its one screen-space filter, or
/// none, as the settings ask.
fn anti_alias(
    settings: Res<Settings>,
    mut commands: Commands,
    mut camera: Single<(Entity, &mut Msaa), With<RtsCamera>>,
) {
    let (entity, msaa) = &mut *camera;
    msaa.set_if_neq(msaa_of(&settings));
    let mut camera = commands.entity(*entity);
    let smaa = |preset| Smaa { preset };
    match settings.quality.anti_alias {
        AntiAlias::Fxaa => {
            camera.insert(Fxaa::default()).remove::<Smaa>();
        }
        AntiAlias::SmaaLow => {
            camera.insert(smaa(SmaaPreset::Low)).remove::<Fxaa>();
        }
        AntiAlias::SmaaMedium => {
            camera.insert(smaa(SmaaPreset::Medium)).remove::<Fxaa>();
        }
        AntiAlias::SmaaHigh => {
            camera.insert(smaa(SmaaPreset::High)).remove::<Fxaa>();
        }
        AntiAlias::Off | AntiAlias::Msaa2 | AntiAlias::Msaa4 => {
            camera.remove::<(Fxaa, Smaa)>();
        }
    }
}

/// The 3D scene drawn below the window's resolution: the image it is drawn
/// into, the camera that lays the interface over it, and the node showing it.
#[derive(Resource)]
struct Scaled {
    image: Handle<Image>,
    camera: Entity,
    node: Entity,
}

/// Draws the 3D scene at `render_scale` of the window's pixels: into an
/// image a second camera stretches over the window under the interface.
/// The image keeps the window's logical size (its scale factor is scaled
/// too), so picking and the bars read the camera as before.
fn scale(
    mut commands: Commands,
    settings: Res<Settings>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut target: Single<&mut RenderTarget, With<RtsCamera>>,
    mut images: ResMut<Assets<Image>>,
    scaled: Option<Res<Scaled>>,
) {
    let k = settings.quality.render_scale;
    if k >= 1.0 {
        if let Some(scaled) = scaled {
            **target = RenderTarget::Window(WindowRef::Primary);
            commands.entity(scaled.camera).despawn();
            commands.entity(scaled.node).despawn();
            images.remove(&scaled.image);
            commands.remove_resource::<Scaled>();
        }
        return;
    }
    let size = UVec2::new(
        ((window.physical_width() as f32 * k).round() as u32).max(1),
        ((window.physical_height() as f32 * k).round() as u32).max(1),
    );
    let scale_factor = window.scale_factor() * k;
    let image = match &scaled {
        Some(scaled) => scaled.image.clone(),
        None => {
            // RGBA: WebGL2 takes no BGRA pixels, and a target starts as uploaded zeros.
            let image = images.add(Image::new_target_texture(
                size.x,
                size.y,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            let ui = commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: 1,
                        ..default()
                    },
                    IsDefaultUiCamera,
                    Msaa::Off,
                ))
                .id();
            let node = commands
                .spawn((
                    crate::ui::fill(),
                    ImageNode::new(image.clone()),
                    Pickable::IGNORE,
                    GlobalZIndex(i32::MIN),
                ))
                .id();
            commands.insert_resource(Scaled {
                image: image.clone(),
                camera: ui,
                node,
            });
            image
        }
    };
    if images.get(&image).is_some_and(|i| i.size() != size)
        && let Some(mut i) = images.get_mut(&image)
    {
        i.resize(Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
    let want = ImageRenderTarget {
        handle: image,
        scale_factor,
    };
    if !matches!(&**target, RenderTarget::Image(now) if *now == want) {
        **target = RenderTarget::Image(want);
    }
}

/// Halves every texture of a model or the map larger than `texture_size`
/// as it loads, before it reaches the GPU.
fn shrink_textures(
    mut events: MessageReader<AssetEvent<Image>>,
    mut images: ResMut<Assets<Image>>,
    server: Res<AssetServer>,
    settings: Res<Settings>,
) {
    let cap = settings.quality.texture_size.max(1);
    for event in events.read() {
        let AssetEvent::Added { id } = event else {
            continue;
        };
        let from_scene = server.get_path(*id).is_some_and(|path| {
            path.source().as_str() == Some(crate::CACHE_SOURCE)
                && ["models/", "map/", "textures/"]
                    .iter()
                    .any(|dir| path.path().starts_with(dir))
        });
        if !from_scene
            || images
                .get(*id)
                .is_none_or(|i| i.width().max(i.height()) <= cap)
        {
            continue;
        }
        if let Some(image) = images.get_mut_untracked(*id) {
            shrink(image, cap);
        }
    }
}

/// Brings an image within `cap` texels a side: a mipmapped one (the cache's
/// textures, block-compressed or not) loses its largest levels, a single
/// level of 8-bit channels is halved with a 2×2 box; any other is left as it is.
fn shrink(image: &mut Image, cap: u32) {
    if image.texture_descriptor.mip_level_count > 1 {
        drop_levels(image, cap);
        return;
    }
    let channels = match image.texture_descriptor.format {
        TextureFormat::R8Unorm => 1,
        TextureFormat::Rg8Unorm => 2,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => 4,
        _ => return,
    };
    let d = &image.texture_descriptor;
    if d.mip_level_count != 1
        || d.size.depth_or_array_layers != 1
        || d.dimension != TextureDimension::D2
    {
        return;
    }
    let Some(mut data) = image.data.take() else {
        return;
    };
    let (mut w, mut h) = (d.size.width as usize, d.size.height as usize);
    while w.max(h) > cap as usize && w > 1 && h > 1 {
        let (hw, hh) = (w / 2, h / 2);
        let mut half = vec![0u8; hw * hh * channels];
        for y in 0..hh {
            let (top, bottom) = (2 * y * w, (2 * y + 1) * w);
            for x in 0..hw {
                for c in 0..channels {
                    let at = |row: usize, col: usize| u16::from(data[(row + col) * channels + c]);
                    let sum = at(top, 2 * x)
                        + at(top, 2 * x + 1)
                        + at(bottom, 2 * x)
                        + at(bottom, 2 * x + 1);
                    half[(y * hw + x) * channels + c] = ((sum + 2) / 4) as u8;
                }
            }
        }
        data = half;
        (w, h) = (hw, hh);
    }
    image.texture_descriptor.size.width = w as u32;
    image.texture_descriptor.size.height = h as u32;
    image.data = Some(data);
}

/// Drops a mipmapped image's largest levels until no side is over `cap` (its
/// smallest level stays); its levels lie one after another, largest first.
fn drop_levels(image: &mut Image, cap: u32) {
    let d = &image.texture_descriptor;
    if d.size.depth_or_array_layers != 1 || d.dimension != TextureDimension::D2 {
        return;
    }
    let Some(block_bytes) = d.format.block_copy_size(None) else {
        return;
    };
    let (bw, bh) = d.format.block_dimensions();
    let (width, height, levels) = (d.size.width, d.size.height, d.mip_level_count);
    let side = |level: u32| ((width >> level).max(1), (height >> level).max(1));
    let level_bytes = |level: u32| {
        let (w, h) = side(level);
        w.div_ceil(bw) as usize * h.div_ceil(bh) as usize * block_bytes as usize
    };
    let mut dropped = 0;
    while dropped + 1 < levels && side(dropped).0.max(side(dropped).1) > cap {
        dropped += 1;
    }
    let skip: usize = (0..dropped).map(level_bytes).sum();
    let Some(data) = image
        .data
        .as_mut()
        .filter(|data| dropped > 0 && data.len() > skip)
    else {
        return;
    };
    data.drain(..skip);
    let (w, h) = side(dropped);
    let d = &mut image.texture_descriptor;
    d.size.width = w;
    d.size.height = h;
    d.mip_level_count = levels - dropped;
}

/// Seconds of frames each judgement of `auto_quality` reads.
const AUTO_WINDOW: f32 = 3.0;

/// The frames `auto_quality` is reading.
#[derive(Default)]
struct Judge {
    seconds: f32,
    frames: u32,
}

/// Lowers the settings a step while frames run slower than the frame cap
/// (or 30 fps) allows, once the scene is whole.
fn auto(
    mut settings: ResMut<Settings>,
    time: Res<Time<bevy::time::Real>>,
    photo: Res<crate::minimap::PhotoTaken>,
    mut judge: Local<Judge>,
) {
    if !settings.quality.auto || !photo.0 {
        return;
    }
    judge.seconds += time.delta_secs();
    judge.frames += 1;
    if judge.seconds < AUTO_WINDOW {
        return;
    }
    let mean_ms = judge.seconds * 1000.0 / judge.frames as f32;
    *judge = Judge::default();
    let target = if settings.quality.fps_cap > 0 {
        settings.quality.fps_cap
    } else {
        30
    };
    let budget_ms = 1000.0 / target as f32 * 1.1;
    if mean_ms <= budget_ms {
        return;
    }
    if let Some(lower) = settings.quality.lowered() {
        info!("quality: frames take {mean_ms:.1} ms against {budget_ms:.1}: lowered to {lower:?}");
        settings.quality = lower;
    }
}

/// Holds each frame to `fps_cap`: sleeps out what is left of its share of
/// the second.
#[cfg(not(target_arch = "wasm32"))]
fn cap_frames(settings: Res<Settings>, mut due: Local<Option<std::time::Instant>>) {
    let fps = settings.quality.fps_cap;
    if fps == 0 {
        *due = None;
        return;
    }
    let period = std::time::Duration::from_secs_f64(1.0 / f64::from(fps));
    let now = std::time::Instant::now();
    let next = due.unwrap_or(now);
    if next > now {
        std::thread::sleep(next - now);
    }
    // A frame far behind starts the count again rather than racing to catch up.
    let base = if now > next + period { now } else { next };
    *due = Some(base + period);
}
