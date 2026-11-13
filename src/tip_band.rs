//! The header band of a tooltip section (Active, Passive, Use): the one place that draws it.
//!
//! Dota's style (`#AbilityDescriptionContainer Label.Header` in `ability_details.vcss`) gives the
//! label a height of 28 and its band as a background image with `background-repeat: no-repeat`,
//! `background-position: top` and no `background-size`: the art is drawn at its own size,
//! unscaled, from the label's left edge, and the label's edge cuts off what is past it. An
//! `ImageNode` of the label would not do the same: in `Auto` mode Bevy fits the art, keeping its
//! proportions, inside the label's padding box and centres it, which left the band's left end bare
//! and shrank the art (reported again and again, each time fixed by moving the label's padding or
//! its width, never the draw mode). So the art is a node of its own, of the art's size, laid on
//! the band's top left corner and cut off by the band.

use bevy::prelude::*;

/// The band's height, Dota's `height: 28px`.
pub(crate) const HEIGHT: f32 = 28.0;
/// The art's own size (`item_tooltip_*_psd`, 334 by 28), drawn unscaled.
pub(crate) const ART: Vec2 = Vec2::new(334.0, HEIGHT);

/// A band.
#[derive(Component)]
pub(crate) struct Band;

/// The band's art, laid on its top left corner.
#[derive(Component)]
pub(crate) struct Art;

/// Spawns a band of the parent's whole width, `art` (when the cache has it) over `ground`; the
/// caller adds the label to the band after this, so that the label is over the art.
pub(crate) fn spawn(
    commands: &mut Commands,
    parent: Entity,
    art: Option<Handle<Image>>,
    ground: Color,
) -> Entity {
    let band = commands
        .spawn((
            Band,
            Node {
                width: percent(100),
                height: px(HEIGHT),
                padding: UiRect::new(px(8), px(8), px(4), px(0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(ground),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    if let Some(art) = art {
        commands.spawn((
            Art,
            ImageNode::new(art).with_mode(NodeImageMode::Stretch),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: px(ART.x),
                height: px(ART.y),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(band),
        ));
    }
    band
}

/// The band as drawn on a GPU: no window and no other part of the client, the art from the asset
/// cache. It measures what the picture shows, which a node's layout rectangle does not tell.
#[cfg(test)]
mod render {
    use std::path::PathBuf;
    use std::time::Duration;

    use bevy::{
        app::PluginsState,
        asset::RenderAssetUsages,
        camera::{ImageRenderTarget, RenderTarget},
        image::{CompressedImageFormats, ImageSampler, ImageType},
        render::{
            gpu_readback::{Readback, ReadbackComplete},
            pipelined_rendering::PipelinedRenderingPlugin,
            render_resource::{PollType, TextureFormat, TextureUsages},
            renderer::RenderDevice,
        },
    };

    use super::*;

    /// Where the band's top left corner is, in logical pixels, and the band's width: Dota's
    /// `#AbilityDescriptionContainer` of 330.
    const CORNER: Vec2 = Vec2::new(10.0, 10.0);
    const BAND_WIDTH: f32 = 330.0;
    /// What the target is cleared to: nothing the art is near.
    const BARE: [u8; 3] = [255, 0, 255];

    /// Space between the stacked bands.
    const STRIDE: f32 = HEIGHT + 12.0;

    /// The bands drawn, one over the other, each the art it names and drawn the former way or not.
    #[derive(Resource)]
    struct Setup {
        bands: Vec<(Handle<Image>, bool)>,
        scale: f32,
        size: UVec2,
    }

    #[derive(Resource, Default)]
    struct Seen(Option<(Vec<u8>, usize)>);

    /// The art's pixels, from the asset cache the tests of the client read.
    fn load(name: &str) -> Image {
        let root = PathBuf::from(
            std::env::var_os("DBOTA_ASSETS").expect("DBOTA_ASSETS names the asset cache"),
        );
        let bytes = std::fs::read(root.join("hud").join(name)).expect("the art in the cache");
        assert!(
            matches!(crate::texture_file::decoded_size(&bytes), Some(Ok(_))),
            "{name}: a PNG within the decoder's caps"
        );
        Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::Default,
            RenderAssetUsages::default(),
        )
        .expect("a PNG")
    }

    /// The target's pixels as RGBA rows, and a row's stride in bytes, once the band is drawn.
    fn draw(bands: &[(&Image, bool)], scale: f32) -> (Vec<u8>, usize) {
        let height = CORNER.y * 2.0 + STRIDE * (bands.len() - 1) as f32 + HEIGHT;
        let size = (Vec2::new(CORNER.x * 2.0 + BAND_WIDTH, height) * scale)
            .ceil()
            .as_uvec2();
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
        app.insert_resource(ClearColor(Color::srgb_u8(BARE[0], BARE[1], BARE[2])));
        let bands = bands
            .iter()
            .map(|(art, former)| {
                let handle = app
                    .world_mut()
                    .resource_mut::<Assets<Image>>()
                    .add((*art).clone());
                (handle, *former)
            })
            .collect();
        app.insert_resource(Setup { bands, scale, size })
            .init_resource::<Seen>()
            .add_systems(Startup, scene);
        let mut patience = crate::e2e::harness::Patience::of(Duration::from_secs(60));
        while app.plugins_state() == PluginsState::Adding {
            patience.pass("the offscreen rendering device");
            bevy::tasks::tick_global_task_pools_on_main_thread();
            std::thread::sleep(Duration::from_millis(1));
        }
        app.finish();
        app.cleanup();
        while app.world().resource::<Seen>().0.is_none() {
            patience.pass("the band's pixels");
            app.update();
            std::thread::sleep(Duration::from_millis(1));
        }
        let seen = app
            .world()
            .resource::<Seen>()
            .0
            .clone()
            .expect("the band drawn");
        // Drain the last readbacks before the device goes, including on a failed assertion.
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
        seen
    }

    fn scene(mut commands: Commands, mut images: ResMut<Assets<Image>>, setup: Res<Setup>) {
        let mut target = Image::new_target_texture(
            setup.size.x,
            setup.size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        );
        target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let target = images.add(target);
        let camera = commands
            .spawn((
                Camera2d,
                RenderTarget::Image(ImageRenderTarget {
                    handle: target.clone(),
                    scale_factor: setup.scale,
                }),
            ))
            .id();
        for (at, (art, former)) in setup.bands.iter().enumerate() {
            let root = commands
                .spawn((
                    UiTargetCamera(camera),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(CORNER.x),
                        top: px(CORNER.y + STRIDE * at as f32),
                        width: px(BAND_WIDTH),
                        ..default()
                    },
                ))
                .id();
            if *former {
                former_spawn(&mut commands, root, art.clone());
            } else {
                spawn(&mut commands, root, Some(art.clone()), Color::NONE);
            }
        }
        commands
            .spawn(Readback::texture(target))
            .observe(read_pixels);
    }

    fn read_pixels(
        event: On<ReadbackComplete>,
        setup: Res<Setup>,
        mut seen: ResMut<Seen>,
        mut commands: Commands,
    ) {
        let stride = RenderDevice::align_copy_bytes_per_row(setup.size.x as usize * 4);
        // The band is drawn once the pixel in its middle is no longer the clear colour, nor the
        // black the target holds before anything is drawn.
        let middle = ((CORNER + Vec2::new(BAND_WIDTH, HEIGHT) / 2.0) * setup.scale).as_uvec2();
        let at = middle.y as usize * stride + middle.x as usize * 4;
        if event.data[at..at + 3] != BARE && event.data[at..at + 3] != [0, 0, 0] {
            seen.0 = Some((event.data.clone(), stride));
            commands.entity(event.entity).remove::<Readback>();
        }
    }

    /// The pixel of the render at a physical column and row.
    fn pixel(seen: &(Vec<u8>, usize), x: usize, y: usize) -> [u8; 3] {
        let at = y * seen.1 + x * 4;
        [seen.0[at], seen.0[at + 1], seen.0[at + 2]]
    }

    fn distance(a: [u8; 3], b: [u8; 3]) -> u8 {
        a.iter()
            .zip(b)
            .map(|(a, b)| a.abs_diff(b))
            .max()
            .unwrap_or(0)
    }

    /// How the band was drawn until it was measured: the art as the `ImageNode` of the padded label,
    /// which Bevy fits inside the padding box and centres.
    fn former_spawn(commands: &mut Commands, parent: Entity, art: Handle<Image>) {
        commands.spawn((
            Node {
                width: percent(100),
                height: px(26),
                padding: UiRect::new(px(8), px(8), px(4), px(0)),
                ..default()
            },
            ImageNode::new(art),
            ChildOf(parent),
        ));
    }

    /// What is wrong with a drawn band, if anything: the first drawn column of the band is the
    /// band's own left edge, every column shows the art's column at that place (so the art is
    /// unscaled and starts at the left end) and nothing of the art shows past the band.
    fn measure(art: &Image, seen: &(Vec<u8>, usize), scale: f32, at: usize) -> Result<(), String> {
        let left = (CORNER.x * scale) as usize;
        let top = ((CORNER.y + STRIDE * at as f32) * scale) as usize;
        let rows = (HEIGHT * scale) as usize;
        let columns = (BAND_WIDTH * scale) as usize;
        let data = art.data.as_ref().expect("the art's pixels");
        for row in [rows / 8 + 1, rows / 2, rows - 1 - rows / 8] {
            let first = (0..columns)
                .find(|x| distance(pixel(seen, left + x, top + row), BARE) > 40)
                .ok_or("the band is not drawn")?;
            if first > 1 {
                return Err(format!("row {row}: the band starts {first} columns in"));
            }
            for x in 0..columns {
                let shown = pixel(seen, left + x, top + row);
                if distance(shown, BARE) <= 40 {
                    return Err(format!("row {row}: column {x} of the band is bare"));
                }
                let source_x = ((x as f32 + 0.5) / scale) as usize;
                let source_y = ((row as f32 + 0.5) / scale) as usize;
                let at = (source_y * ART.x as usize + source_x) * 4;
                let wanted = [data[at], data[at + 1], data[at + 2]];
                // At twice the size the art is filtered between its pixels.
                let tolerance = if scale == 1.0 { 3 } else { 24 };
                if distance(shown, wanted) > tolerance {
                    return Err(format!(
                        "row {row} column {x}: drawn {shown:?}, the art has {wanted:?} there"
                    ));
                }
            }
            if pixel(seen, left + columns + 2, top + row) != BARE {
                return Err(format!("row {row}: the art runs past the band"));
            }
        }
        Ok(())
    }

    /// For each of Dota's three bands, at the scales a window of 1080 lines and of twice as many
    /// give: the band is drawn from its left end with the art unscaled. The same measure fails the
    /// way the band was drawn before: a test that cannot fail says nothing.
    #[test]
    #[ignore = "offscreen renderer and the asset cache; tools/visual_check.sh runs this contract"]
    fn each_band_draws_its_art_from_the_left_end_unscaled_at_every_scale() {
        let names = [
            "item_tooltip_active_psd.png",
            "item_tooltip_passive_psd.png",
            "item_tooltip_use_psd.png",
        ];
        let arts: Vec<Image> = names.iter().map(|name| load(name)).collect();
        for (name, art) in names.iter().zip(&arts) {
            assert_eq!(art.size(), ART.as_uvec2(), "{name}: the art's own size");
        }
        // The three bands as they are drawn, and the first's art the former way.
        let mut bands: Vec<(&Image, bool)> = arts.iter().map(|art| (art, false)).collect();
        bands.push((&arts[0], true));
        for scale in [1.0, 2.0] {
            let seen = draw(&bands, scale);
            for (at, name) in names.iter().enumerate() {
                if let Err(wrong) = measure(&arts[at], &seen, scale, at) {
                    panic!("{name} at {scale}x: {wrong}");
                }
            }
            if scale == 1.0 {
                let wrong = measure(&arts[0], &seen, scale, names.len())
                    .expect_err("the measure finds what was wrong with the former band");
                eprintln!("the former band: {wrong}");
            }
        }
    }
}
