//! dbota: a Dota-looking front end for the bota simulator, on Bevy.
//!
//! The game logic lives in `bota-client-core`; this crate draws it with
//! Dota's own assets from a local cache (see `tools/README.md`) or with
//! stand-ins when the cache is missing. World mapping (see `coords`): bota
//! (x, y) with y north maps to Bevy (x, height, -y).

mod anim;
mod bars;
mod camera;
mod capture;
mod config;
mod coords;
mod effects;
mod fog;
mod hud;
mod input;
mod manifest;
mod map;
mod menu;
mod minimap;
mod overlay;
mod session;
mod shop;
mod sound;
mod synth;
mod terrain;
mod tips;
mod trees;
mod ui;
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

/// dbota: watch or play a bota match with Dota's look.
#[derive(Parser, Debug, Clone, Resource)]
#[command(version, about)]
pub struct Args {
    /// Where the server listens.
    #[arg(long, default_value = "127.0.0.1:4455")]
    pub addr: String,
    /// What the lobby shows.
    #[arg(long, default_value = "player")]
    pub name: String,
    /// Watch instead of playing.
    #[arg(long)]
    pub spectate: bool,
    /// Play this replay file instead of connecting anywhere.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["spectate"])]
    pub replay: Option<PathBuf>,
    /// Watch a replay as this seat sees it: its fog, its panel, its shop.
    #[arg(long, value_name = "SEAT", requires = "replay")]
    pub pov: Option<u8>,
    /// Keep the camera on this seat's hero in a replay.
    #[arg(long, value_name = "SEAT", requires = "replay")]
    pub follow: Option<u8>,
    /// Root of the exported Dota asset cache.
    #[arg(long, default_value_os_t = default_cache_dir())]
    pub cache_dir: PathBuf,
    /// The settings file; `~/.config/dbota/client.conf` when absent.
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// Debug: save the screen once a replay reaches each of these ticks,
    /// then exit after the last.
    #[arg(long, value_name = "TICK", value_delimiter = ',', requires = "replay")]
    pub screenshot_at: Vec<u32>,
    /// Debug: where `--screenshot-at` saves.
    #[arg(long, value_name = "DIR", default_value = "temp/shots")]
    pub screenshot_dir: PathBuf,
    /// Debug: what `--screenshot-at` file names start with.
    #[arg(long, default_value = "shot-")]
    pub screenshot_prefix: String,
    /// Debug: save the screen to this file after `--frames` frames, then exit.
    #[arg(long, requires = "frames")]
    pub screenshot: Option<PathBuf>,
    /// Debug: frames to run before `--screenshot`.
    #[arg(long)]
    pub frames: Option<u32>,
    /// Debug: open the shop at the start.
    #[arg(long)]
    pub open_shop: bool,
    /// Debug: the camera's distance from the ground at the start.
    #[arg(long, value_name = "UNITS")]
    pub distance: Option<f32>,
    /// Debug: leave things out to time the rest: any of `trees`, `shadows`,
    /// `msaa`, `map`, `units`, `ui`.
    #[arg(long, value_delimiter = ',')]
    pub without: Vec<String>,
    /// Present without vsync.
    #[arg(long)]
    pub no_vsync: bool,
    /// Play no sound.
    #[arg(long)]
    pub mute: bool,
}

/// Where the settings are saved; absent when there is nowhere to.
#[derive(Resource, Clone, Debug)]
pub struct SettingsPath(pub Option<PathBuf>);

fn default_cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DBOTA_ASSETS").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".cache/dbota")
}

fn main() -> AppExit {
    let args = Args::parse();
    let session = match session::open(&args) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("dbota: {err}");
            return AppExit::error();
        }
    };
    let manifest = manifest::Manifest::load(&args.cache_dir);
    let settings_path = args.config.clone().or_else(config::default_path);
    let settings = settings_path.as_deref().map(config::load).unwrap_or_default();
    let mut app = App::new();
    app.insert_resource(args.clone())
        .insert_resource(manifest)
        .insert_resource(settings)
        .insert_resource(SettingsPath(settings_path))
        .insert_non_send(session)
        // Must be registered before AssetPlugin (part of DefaultPlugins) builds the sources.
        .register_asset_source(
            CACHE_SOURCE,
            AssetSourceBuilder::new(AssetSource::get_default_reader(
                args.cache_dir.to_string_lossy().into_owned(),
            )),
        )
        .add_plugins(default_plugins(&args))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            session::plugin,
            map::plugin,
            fog::plugin,
            units::plugin,
            anim::plugin,
            camera::plugin,
            capture::plugin,
            trees::plugin,
        ))
        .add_plugins((ui::plugin, bars::plugin, overlay::plugin, hud::plugin, input::plugin, minimap::plugin))
        .add_plugins((sound::plugin, effects::plugin, shop::plugin, menu::plugin, tips::plugin));
    app.run()
}

/// Bevy's plugins as dbota sets them up.
fn default_plugins(args: &Args) -> impl PluginGroup {
    DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "dbota".into(),
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
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
            unapproved_path_mode: UnapprovedPathMode::Deny,
            ..default()
        })
        .set(
            GltfPlugin {
                convert_coordinates: GltfConvertCoordinates {
                    rotate_scene_entity: true,
                    ..default()
                },
                ..default()
            }
            .add_custom_vertex_attribute("TEXCOORD_1", map::ATTRIBUTE_BLEND_WEIGHTS)
            .add_custom_vertex_attribute("TEXCOORD_2", map::ATTRIBUTE_BLEND_ALPHAS)
            .add_custom_vertex_attribute("TEXCOORD_3", map::ATTRIBUTE_PAINTED_TINT),
        )
}
