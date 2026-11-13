//! Physical minimap strokes through the production UI scale and indicator constructor.
use std::time::Duration;

use bevy::{
    app::PluginsState,
    camera::{ImageRenderTarget, RenderTarget},
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{TextureFormat, TextureUsages},
        renderer::RenderDevice,
    },
    window::PrimaryWindow,
};

#[derive(Resource, Default)]
struct Pixels {
    pending: Option<Entity>,
    bytes: Option<Vec<u8>>,
}
#[derive(Resource)]
struct Resolution(UVec2);
#[derive(Component)]
struct Sample {
    name: &'static str,
    legacy: bool,
}
#[derive(Resource, Default)]
struct CaptureTarget(Option<Handle<Image>>);
const KINDS: [(&str, bool, f32, f32); 3] = [
    ("camera", false, 120.0, 80.0),
    ("ping", true, super::PING_SIZES.1, super::PING_SIZES.1),
    ("teleport", true, super::PING_SIZES.0, super::PING_SIZES.0),
];

#[test]
#[ignore = "offscreen renderer; visual_check runs this contract"]
fn minimap_camera_ping_and_teleport_strokes_halve_physical_widths_above_one_pixel() {
    let mut failures = Vec::new();
    for (width, height) in [(1280, 720), (1920, 1080), (2877, 1642), (3840, 2160)] {
        let mut app = make_app(width, height);
        let mut wait = crate::e2e::harness::Patience::of(Duration::from_secs(60));
        while app.plugins_state() == PluginsState::Adding {
            wait.pass("the minimap UI render device");
            bevy::tasks::tick_global_task_pools_on_main_thread();
            std::thread::yield_now();
        }
        app.finish();
        app.cleanup();
        for _ in 0..4 {
            app.update();
        }
        let rows = measure_pair(&mut app, width, height);
        failures.extend(width_failures(&rows, width, height));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn width_failures(rows: &[(&str, bool, f32)], width: u32, height: u32) -> Vec<String> {
    let mut failures = Vec::new();
    for (name, _, _, _) in KINDS {
        let old = rows
            .iter()
            .find(|row| row.0 == name && row.1)
            .expect("legacy")
            .2;
        let actual = rows
            .iter()
            .find(|row| row.0 == name && !row.1)
            .expect("current")
            .2;
        assert!(
            old > 0.85,
            "{width}x{height} {name}: legacy stroke must be visible"
        );
        let wanted = if old >= 1.5 { old / 2.0 } else { old };
        println!(
            "minimap stroke {width}x{height} {name}: legacy={old:.3} current={actual:.3} expected={wanted:.3}"
        );
        if (actual - wanted).abs() >= 0.15 {
            failures.push(format!(
                "{width}x{height} {name}: rendered width {actual} must be {wanted}, from {old}"
            ));
        }
    }
    failures
}

fn measure_pair(app: &mut App, width: u32, height: u32) -> Vec<(&'static str, bool, f32)> {
    let mut rows = Vec::new();
    for legacy in [true, false] {
        let world = app.world_mut();
        let mut query = world.query::<(&Sample, &mut Visibility)>();
        for (sample, mut shown) in query.iter_mut(world) {
            *shown = if sample.legacy == legacy {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        for _ in 0..3 {
            app.update();
        }
        let bytes = capture(app);
        save(width, height, legacy, &bytes);
        let world = app.world_mut();
        let mut query = world.query::<(&Sample, &ComputedNode, &UiGlobalTransform)>();
        for (sample, node, transform) in query.iter(world).filter(|(s, _, _)| s.legacy == legacy) {
            let centre = transform.translation;
            let size = node.size();
            let ink = stroke(
                &bytes,
                width,
                height,
                centre.x,
                centre.y - size.y / 2.0,
                size.y / 2.0,
            );
            println!(
                "node {width}x{height} {} legacy={legacy}: border={:?} scale={}",
                sample.name,
                node.border(),
                node.inverse_scale_factor()
            );
            rows.push((sample.name, legacy, ink));
        }
    }
    rows
}

fn capture(app: &mut App) -> Vec<u8> {
    let target = app
        .world()
        .resource::<CaptureTarget>()
        .0
        .clone()
        .expect("target");
    let entity = app
        .world_mut()
        .spawn(Readback::texture(target))
        .observe(received)
        .id();
    *app.world_mut().resource_mut::<Pixels>() = Pixels {
        pending: Some(entity),
        bytes: None,
    };
    let mut wait = crate::e2e::harness::Patience::of(Duration::from_secs(60));
    while app.world().resource::<Pixels>().bytes.is_none() {
        wait.pass("minimap border pixels");
        app.update();
    }
    app.world_mut()
        .resource_mut::<Pixels>()
        .bytes
        .take()
        .expect("pixels")
}

fn received(
    event: On<ReadbackComplete>,
    resolution: Res<Resolution>,
    mut pixels: ResMut<Pixels>,
    mut commands: Commands,
) {
    if pixels.pending != Some(event.entity) {
        return;
    }
    let size = resolution.0;
    let stride = RenderDevice::align_copy_bytes_per_row(size.x as usize * 4);
    pixels.bytes = Some(
        event
            .data
            .chunks_exact(stride)
            .take(size.y as usize)
            .flat_map(|row| row[..size.x as usize * 4].iter().copied())
            .collect(),
    );
    pixels.pending = None;
    commands.entity(event.entity).despawn();
}

fn linear(value: u8) -> f32 {
    let value = value as f32 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn stroke(bytes: &[u8], width: u32, height: u32, x: f32, top: f32, half_height: f32) -> f32 {
    let mut total = 0.0;
    let end = ((top + 9.0).min(top + half_height).ceil() as u32).min(height);
    for column in [x.floor() as u32 - 1, x.floor() as u32, x.floor() as u32 + 1] {
        for row in (top.floor() as i32 - 2).max(0) as u32..end {
            total += linear(bytes[((row * width + column) * 4) as usize]);
        }
    }
    total / 3.0
}

fn save(width: u32, height: u32, legacy: bool, pixels: &[u8]) {
    let image = Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    let directory = crate::test_support::scratch_path("minimap_strokes");
    std::fs::create_dir_all(&directory).expect("evidence");
    let suffix = if legacy { "before" } else { "after" };
    image
        .try_into_dynamic()
        .expect("RGBA")
        .save(directory.join(format!("{width}x{height}_{suffix}.png")))
        .expect("image");
}

fn make_app(width: u32, height: u32) -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
            .disable::<bevy::log::LogPlugin>()
            .disable::<PipelinedRenderingPlugin>()
            .set(bevy::render::RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            }),
    );
    super::outline::plugin(&mut app);
    app.insert_resource(ClearColor(Color::BLACK))
        .insert_resource(crate::config::Settings::default())
        .insert_resource(crate::ui::HudShown(true))
        .insert_resource(Resolution(UVec2::new(width, height)))
        .init_resource::<Pixels>()
        .init_resource::<CaptureTarget>()
        .add_systems(Update, crate::ui::scale_ui)
        .add_systems(Startup, scene);
    app
}

fn scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut target: ResMut<CaptureTarget>,
    mut materials: ResMut<Assets<super::outline::Outline>>,
    resolution: Res<Resolution>,
) {
    let size = resolution.0;
    commands.spawn((
        Window {
            resolution: bevy::window::WindowResolution::new(size.x, size.y),
            ..default()
        },
        PrimaryWindow,
    ));
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let handle = images.add(image);
    target.0 = Some(handle.clone());
    let camera = commands
        .spawn((
            Camera2d,
            RenderTarget::Image(ImageRenderTarget {
                handle,
                scale_factor: 1.0,
            }),
            Msaa::Off,
        ))
        .id();
    sample_nodes(&mut commands, &mut materials, camera);
}

fn sample_nodes(
    commands: &mut Commands,
    materials: &mut Assets<super::outline::Outline>,
    camera: Entity,
) {
    for legacy in [true, false] {
        for (index, (name, round, width, height)) in KINDS.into_iter().enumerate() {
            let mut node = super::indicator_node(round);
            if legacy {
                node.border = UiRect::all(px(if round { 2.0 } else { 1.0 }));
            }
            node.left = px(40.0 + index as f32 * 180.0);
            node.top = px(60);
            node.width = px(width);
            node.height = px(height);
            let mut entity = commands.spawn((
                Sample { name, legacy },
                UiTargetCamera(camera),
                if legacy {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            ));
            if legacy {
                entity.insert((node, BorderColor::all(Color::WHITE)));
            } else {
                entity.insert(super::outline::bundle(round, materials));
                entity.insert(node);
            }
        }
    }
}
