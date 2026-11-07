//! Actual production material pixels, without a window or installed assets.
use std::time::Duration;

use bevy::{
    app::PluginsState,
    camera::{ImageRenderTarget, RenderTarget, ScalingMode},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{Extent3d, PollType, TextureDimension, TextureFormat, TextureUsages},
        renderer::RenderDevice,
    },
};

use super::{FogExt, FoggedMaterial, SURFACE_FIGURE, SURFACE_GROUND, VEIL};

const WIDTH: u32 = 32;
const HEIGHT: u32 = 16;

#[derive(Resource, Default)]
struct Pixels(Option<[[u8; 4]; 2]>);

#[test]
#[ignore = "offscreen renderer; tools/visual_check.sh runs this contract"]
fn full_visual_clarity_reaches_the_ground_and_figure_pixels() {
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
    app.insert_resource(ClearColor(Color::BLACK));
    super::register_shader(&mut app);
    app.add_plugins(MaterialPlugin::<FoggedMaterial>::default())
        .init_resource::<Pixels>()
        .add_systems(Startup, scene);
    let mut patience = crate::e2e::harness::Patience::of(Duration::from_secs(60));
    while app.plugins_state() == PluginsState::Adding {
        patience.pass("the offscreen rendering device");
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    while app.world().resource::<Pixels>().0.is_none() {
        patience.pass("the production fog material's pixels");
        app.update();
        std::thread::sleep(Duration::from_millis(1));
    }
    let [ground, figure] = app
        .world()
        .resource::<Pixels>()
        .0
        .expect("both surfaces rendered");
    // Drain the last readbacks before dropping the device, including on a failed assertion.
    for _ in 0..2 {
        app.update();
        app.world()
            .resource::<RenderDevice>()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })
            .expect("the offscreen commands finish");
    }
    // Former High, linear RGB input (0.8,0.2,0.1), no fog or lighting:
    // ground: 20% desaturation then 16% dimming; figure: 20% saturation increase.
    assert_pixel(ground, [0.591_417_1, 0.188_217_1, 0.121_017_1], "ground");
    assert_pixel(figure, [0.895_932, 0.175_932, 0.055_932], "figure");
}

fn scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FoggedMaterial>>,
) {
    let clear = images.add(Image::new_fill(
        Extent3d::default(),
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        default(),
    ));
    let mut target = Image::new_target_texture(WIDTH, HEIGHT, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = images.add(target);
    for (x, surface) in [(-0.5, SURFACE_GROUND), (0.5, SURFACE_FIGURE)] {
        let material = materials.add(FoggedMaterial {
            base: StandardMaterial {
                base_color: Color::linear_rgb(0.8, 0.2, 0.1),
                unlit: true,
                ..default()
            },
            extension: FogExt {
                params: Vec4::new(1024.0, VEIL, f32::from(surface), 0.0),
                veil: clear.clone(),
                cast: clear.clone(),
                paths: clear.clone(),
                field: clear.clone(),
            },
        });
        commands.spawn((
            Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial3d(material),
            Transform::from_xyz(x, 0.0, 0.0),
        ));
    }
    commands.spawn((
        Camera3d::default(),
        RenderTarget::Image(ImageRenderTarget {
            handle: target.clone(),
            scale_factor: 1.0,
        }),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: 2.0,
                height: 1.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
    ));
    commands
        .spawn(Readback::texture(target))
        .observe(read_pixels);
}

fn read_pixels(event: On<ReadbackComplete>, mut pixels: ResMut<Pixels>, mut commands: Commands) {
    let stride = RenderDevice::align_copy_bytes_per_row(WIDTH as usize * 4);
    let pair = [WIDTH / 4, 3 * WIDTH / 4].map(|x| {
        let offset = HEIGHT as usize / 2 * stride + x as usize * 4;
        <[u8; 4]>::try_from(&event.data[offset..offset + 4]).expect("an RGBA pixel")
    });
    // Both quads must have drawn: the render target is initially black while pipelines compile.
    if pair
        .iter()
        .all(|pixel| pixel[..3].iter().any(|channel| *channel != 0))
    {
        pixels.0 = Some(pair);
        commands.entity(event.entity).remove::<Readback>();
    }
}

fn assert_pixel(actual: [u8; 4], linear: [f32; 3], surface: &str) {
    let expected = Color::linear_rgb(linear[0], linear[1], linear[2])
        .to_srgba()
        .to_u8_array();
    for (channel, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
        assert!(
            actual.abs_diff(expected) <= 2,
            "{surface} channel {channel}: rendered {actual}, former High expects {expected}"
        );
    }
}
