//! The shared production indicator stroke measured in rendered pixels at a fixed camera.
use bevy::{
    app::PluginsState,
    camera::{ImageRenderTarget, RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{PollType, TextureFormat, TextureUsages},
        renderer::RenderDevice,
    },
};
use std::time::Duration;
const SIDE: u32 = 64;
#[derive(Resource, Default)]
struct Pixels(Option<Vec<u8>>);

#[test]
#[ignore = "offscreen renderer; tools/visual_check.sh runs this contract"]
fn indicator_strokes_are_two_pixels_at_a_fixed_camera() {
    let pixels = render_pixels();
    let image = Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        &pixels,
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    image
        .try_into_dynamic()
        .expect("RGBA image")
        .save(crate::test_support::scratch_path("indicator_strokes.png"))
        .expect("stroke evidence saved");
    for row in [16, 32, 48] {
        let width = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .skip(row * SIDE as usize)
            .take(SIDE as usize)
            .filter(|pixel| pixel[0] > 127)
            .count();
        assert_eq!(width, 2, "row {row}: production indicator width in pixels");
    }
}

fn render_pixels() -> Vec<u8> {
    let mut app = render_app();
    let mut patience = crate::e2e::harness::Patience::of(Duration::from_secs(60));
    while app.plugins_state() == PluginsState::Adding {
        patience.pass("the indicator render device");
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::yield_now();
    }
    app.finish();
    app.cleanup();
    while app.world().resource::<Pixels>().0.is_none() {
        patience.pass("the indicator stroke pixels");
        app.update();
    }
    let pixels = app
        .world()
        .resource::<Pixels>()
        .0
        .clone()
        .expect("rendered frame");
    for _ in 0..2 {
        app.update();
        app.world()
            .resource::<RenderDevice>()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })
            .expect("render readbacks drained");
    }
    pixels
}

fn render_app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
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
    app.insert_resource(ClearColor(Color::BLACK))
        .init_resource::<Pixels>()
        .add_systems(Startup, (scene, super::configure))
        .add_systems(Update, strokes);
    app
}

fn scene(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut target = Image::new_target_texture(SIDE, SIDE, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = images.add(target);
    commands.spawn((
        Camera3d::default(),
        RenderTarget::Image(ImageRenderTarget {
            handle: target.clone(),
            scale_factor: 1.0,
        }),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: 4.0,
                height: 4.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(1),
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
    ));
    commands
        .spawn(Readback::texture(target))
        .observe(read_pixels);
}
fn strokes(mut gizmos: Gizmos) {
    gizmos.line(
        Vec3::new(0.0, -1.8, 0.0),
        Vec3::new(0.0, 1.8, 0.0),
        Color::WHITE,
    );
}
fn read_pixels(event: On<ReadbackComplete>, mut pixels: ResMut<Pixels>, mut commands: Commands) {
    let stride = RenderDevice::align_copy_bytes_per_row(SIDE as usize * 4);
    let bytes: Vec<u8> = event
        .data
        .chunks_exact(stride)
        .take(SIDE as usize)
        .flat_map(|row| row[..SIDE as usize * 4].iter().copied())
        .collect();
    assert_eq!(
        bytes.len(),
        SIDE as usize * SIDE as usize * 4,
        "complete RGBA readback"
    );
    if bytes.as_chunks::<4>().0.iter().any(|pixel| pixel[0] > 127) {
        pixels.0 = Some(bytes);
        commands.entity(event.entity).remove::<Readback>();
    }
}
