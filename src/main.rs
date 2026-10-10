//! dbota: Bevy 0.19 client spike for the bota simulator.
//!
//! World mapping (see `coords`): simulator (x, y) with y = north maps to Bevy (x, 0, -y);
//! one simulator unit is one Bevy unit; the ground is the y = 0 plane.

mod audio;
mod camera;
mod capture;
mod coords;
mod fallback;
mod ground;
mod hero;
mod hud;
mod units;

use std::path::PathBuf;

use bevy::{
    asset::{
        UnapprovedPathMode,
        io::{AssetSource, AssetSourceBuilder},
    },
    diagnostic::FrameTimeDiagnosticsPlugin,
    gltf::{GltfPlugin, convert_coordinates::GltfConvertCoordinates},
    prelude::*,
    window::{PresentMode, WindowResolution},
};
use clap::Parser;

/// Name of the asset source rooted at the exported-asset cache (`cache://...`).
pub const CACHE_SOURCE: &str = "cache";

#[derive(Parser, Debug, Clone, Resource)]
#[command(about = "dbota Bevy client spike")]
pub struct Args {
    /// Save a PNG of the primary window after `--frames` frames, then exit.
    #[arg(long, requires = "frames")]
    pub screenshot: Option<PathBuf>,
    /// Number of frames to run before printing frame statistics (and exiting).
    #[arg(long)]
    pub frames: Option<u32>,
    /// Present without vsync, to measure uncapped frame time.
    #[arg(long)]
    pub no_vsync: bool,
    /// Disable directional-light shadow maps.
    #[arg(long)]
    pub no_shadows: bool,
    /// Initial hero animation mode (Space toggles at runtime).
    #[arg(long, value_enum, default_value_t = hero::AnimMode::SimAttack)]
    pub anim: hero::AnimMode,
    /// Initial playback speed of the clip-timed idle loop.
    #[arg(long, default_value_t = 1.0)]
    pub idle_speed: f32,
    /// Do not play any sound.
    #[arg(long)]
    pub mute: bool,
    /// Root of the exported Dota asset cache.
    #[arg(long, default_value_os_t = default_cache_dir())]
    pub cache_dir: PathBuf,
}

fn default_cache_dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME must be set");
    PathBuf::from(home).join(".cache/dbota")
}

fn main() -> AppExit {
    let args = Args::parse();
    if let Some(frames) = args.frames {
        assert!(frames > 0, "--frames must be positive");
    }
    assert!(args.idle_speed.is_finite(), "--idle-speed must be finite");
    let fallback_dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/temp/fallback"));
    let found = fallback::resolve(&args.cache_dir, &fallback_dir);

    let mut app = App::new();
    app.insert_resource(args.clone())
        .insert_resource(found)
        // Must be registered before AssetPlugin (part of DefaultPlugins) builds the sources.
        .register_asset_source(
            CACHE_SOURCE,
            AssetSourceBuilder::new(AssetSource::get_default_reader(
                args.cache_dir.to_string_lossy().into_owned(),
            )),
        )
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "dbota".into(),
                        // Logical size: winit creates the window with LogicalSize, so on a
                        // scale-factor-2 display this becomes 3200x1800 physical pixels.
                        resolution: WindowResolution::new(1600, 900),
                        present_mode: if args.no_vsync {
                            PresentMode::AutoNoVsync
                        } else {
                            PresentMode::AutoVsync
                        },
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    // Default is relative to CARGO_MANIFEST_DIR when run via cargo but to the
                    // executable's directory otherwise; pin it so `target/release/dbota` works.
                    file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
                    // Raw absolute paths are refused unless a load opts in with
                    // `load_builder().override_unapproved()` (see `hud`).
                    unapproved_path_mode: UnapprovedPathMode::Deny,
                    ..default()
                })
                .set(GltfPlugin {
                    // Make `Transform::forward()` of the scene root match the glTF model's front.
                    convert_coordinates: GltfConvertCoordinates {
                        rotate_scene_entity: true,
                        ..default()
                    },
                    ..default()
                }),
        )
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_systems(Startup, fallback::log_found)
        .add_plugins((
            camera::plugin,
            ground::plugin,
            hero::plugin,
            units::plugin,
            hud::plugin,
            audio::plugin,
            capture::plugin,
        ));
    app.run()
}
