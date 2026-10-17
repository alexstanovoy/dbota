//! dbota: a Dota-looking front end for the bota simulator, on Bevy.
//!
//! The game logic lives in `bota_client_core`; this crate draws it with
//! Dota's own assets from a local cache (see `tools/README.md`) or with
//! stand-ins when the cache is missing. World mapping (see `coords`): bota
//! (x, y) with y north maps to Bevy (x, height, -y).

// Bevy systems take their data as arguments and queries.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod ability_art;
mod anim;
mod bars;
mod buffs;
mod cachefs;
mod camera;
mod capture;
mod config;
mod coords;
mod drill;
mod effects;
mod fog;
mod framestats;
mod heap;
mod hud;
mod input;
mod manifest;
mod map;
mod mapfile;
mod menu;
mod minimap;
mod overlay;
mod quality;
mod select;
mod session;
mod sheet;
mod shop;
mod sound;
mod synth;
mod talents;
mod teleport;
mod terrain;
mod tips;
mod trees;
mod ui;
mod units;
mod visual;
mod voice;

use std::path::PathBuf;

use bevy::{
    asset::{UnapprovedPathMode, io::AssetSourceBuilder},
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
    /// A setting for this run, as a line of the settings file says it, over
    /// the file's: `--set quality=low`, `--set render_scale=0.6`. Given
    /// again for more.
    #[arg(long, value_name = "NAME=VALUE")]
    pub set: Vec<String>,
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
    /// Debug: measure each `--screenshot-at` shot against the picture's
    /// requirements (see `visual.rs`), print them, and exit non-zero when one
    /// fails.
    #[arg(long)]
    pub visual_check: bool,
    /// Debug: pin the talent tree open at the start.
    #[arg(long)]
    pub open_talents: bool,
    /// Debug: cover the window with every icon of a kind (`items`,
    /// `abilities`, `effects`) and the Dota art it is drawn from.
    #[arg(long, value_name = "KIND")]
    pub sheet: Option<String>,
    /// Debug: hold the camera over this bota world point, `x y`; given
    /// again, each `--screenshot-at` shot in turn is taken over the next.
    #[arg(long, value_name = "X Y", num_args = 2, action = clap::ArgAction::Append)]
    pub look_at: Vec<f32>,
    /// Debug: the camera's distance from the ground at the start.
    #[arg(long, value_name = "UNITS")]
    pub distance: Option<f32>,
    /// Debug: leave things out to time the rest: any of `trees`, `shadows`,
    /// `msaa`, `map`, `props` (the map's), `overlays` (the map's decals).
    #[arg(long, value_delimiter = ',')]
    pub without: Vec<String>,
    /// Debug: start a replay at this tick, read through as fast as the
    /// reader goes.
    #[arg(long, value_name = "TICK", requires = "replay")]
    pub seek: Option<u32>,
    /// Debug: once the scene is whole (and a `--seek` tick reached), time
    /// this many seconds of frames, print them and the startup timings on
    /// one `bench:` line, and exit.
    #[arg(long, value_name = "SECONDS")]
    pub bench: Option<f32>,
    /// Debug: seconds `--bench` lets pass before it starts timing.
    #[arg(long, value_name = "SECONDS", default_value_t = 2.0)]
    pub bench_warmup: f32,
    /// The window's size at the start, in logical pixels, `WIDTHxHEIGHT`.
    #[arg(long, value_name = "WxH", default_value = "1600x900", value_parser = parse_size)]
    pub window: (u32, u32),
    /// Present without vsync.
    #[arg(long)]
    pub no_vsync: bool,
    /// Play no sound.
    #[arg(long)]
    pub mute: bool,
}

/// Debug: a look parameter from `DBOTA_LOOK` (`key=value,...`), else the
/// default, to compare grades without rebuilding.
pub fn look(key: &str, default: f32) -> f32 {
    std::env::var("DBOTA_LOOK")
        .ok()
        .and_then(|all| {
            all.split(',').find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                (k == key).then(|| v.parse().ok()).flatten()
            })
        })
        .unwrap_or(default)
}

/// Where the settings are saved; absent when there is nowhere to.
#[derive(Resource, Clone, Debug)]
pub struct SettingsPath(pub Option<PathBuf>);

/// The export version of the asset cache this client reads, as
/// `tools/import_assets.sh` writes it: each version has a folder of its own
/// under `~/.cache/dbota`, so clients of different versions share a machine.
const ASSET_VERSION: &str = include_str!("../assets-manifest/version");

/// `$DBOTA_ASSETS`, else `~/.cache/dbota/v<ASSET_VERSION>`.
fn default_cache_dir() -> PathBuf {
    // In a browser the cache is served next to the page.
    if cfg!(target_arch = "wasm32") {
        return PathBuf::from("cache");
    }
    if let Some(dir) = std::env::var_os("DBOTA_ASSETS").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".cache/dbota")
        .join(format!("v{}", ASSET_VERSION.trim()))
}

/// When the process started, for the startup timings in the log.
pub static STARTED: std::sync::OnceLock<bevy::platform::time::Instant> = std::sync::OnceLock::new();

/// Seconds since the process started.
pub fn since_start() -> f32 {
    STARTED
        .get_or_init(bevy::platform::time::Instant::now)
        .elapsed()
        .as_secs_f32()
}

fn main() -> AppExit {
    STARTED.get_or_init(bevy::platform::time::Instant::now);
    // A browser build with `web-replay` takes its arguments from
    // DBOTA_WEB_ARGS when it was built: a page has no command line.
    #[cfg(all(target_arch = "wasm32", feature = "web-replay"))]
    let args = Args::parse_from(
        std::iter::once("dbota").chain(
            option_env!("DBOTA_WEB_ARGS")
                .unwrap_or("")
                .split_whitespace(),
        ),
    );
    #[cfg(not(all(target_arch = "wasm32", feature = "web-replay")))]
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
    let mut settings = settings_path
        .as_deref()
        .map(config::load)
        .unwrap_or_default();
    for line in &args.set {
        let set = line
            .split_once('=')
            .ok_or_else(|| "expected NAME=VALUE".to_string())
            .and_then(|(name, value)| settings.set(name.trim(), value.trim()));
        if let Err(why) = set {
            eprintln!("dbota: --set {line}: {why}");
            return AppExit::error();
        }
    }
    let mut app = App::new();
    let source = cache_source(&mut app, &args.cache_dir);
    app.insert_resource(args.clone())
        .insert_resource(manifest)
        .insert_resource(settings.clone())
        .insert_resource(SettingsPath(settings_path))
        .insert_non_send(session)
        // Must be registered before AssetPlugin (part of DefaultPlugins) builds the sources.
        .register_asset_source(CACHE_SOURCE, source)
        .add_plugins(default_plugins(&args, &settings))
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
        .add_plugins((
            ui::plugin,
            bars::plugin,
            overlay::plugin,
            hud::plugin,
            input::plugin,
            minimap::plugin,
        ))
        .add_plugins((
            sound::plugin,
            effects::plugin,
            teleport::plugin,
            shop::plugin,
            menu::plugin,
            tips::plugin,
            drill::plugin,
            buffs::plugin,
            talents::plugin,
            sheet::plugin,
            voice::plugin,
            select::plugin,
            framestats::plugin,
            quality::plugin,
            heap::plugin,
        ));
    // GPU buffers of at most 128 MiB: a browser's WebGPU refuses ones over
    // 256 MiB, and Bevy's mesh slabs grow to 512.
    app.sub_app_mut(bevy::render::RenderApp).insert_resource(
        bevy::render::mesh::allocator::MeshAllocatorSettings {
            slab_allocator_settings: bevy::render::slab_allocator::SlabAllocatorSettings {
                max_slab_size: 128 << 20,
                large_threshold: 64 << 20,
                ..default()
            },
            ..default()
        },
    );
    app.run()
}

/// A `WIDTHxHEIGHT` size, each side 64 to 16384.
fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (w, h) = text
        .split_once('x')
        .ok_or_else(|| format!("'{text}' is not WIDTHxHEIGHT"))?;
    let side = |s: &str| {
        s.parse::<u32>()
            .ok()
            .filter(|v| (64..=16_384).contains(v))
            .ok_or_else(|| format!("'{s}' is not a side of 64 to 16384 pixels"))
    };
    Ok((side(w)?, side(h)?))
}

/// The asset source over the cache folder: files read as `cachefs` reads
/// them, textures within a decode budget.
fn cache_source(app: &mut App, root: &std::path::Path) -> AssetSourceBuilder {
    let budget = cachefs::DecodeBudget::default();
    app.insert_resource(budget.clone())
        .add_plugins(cachefs::plugin);
    let root = root.to_path_buf();
    AssetSourceBuilder::new(move || {
        Box::new(cachefs::CacheReader::new(root.clone(), budget.clone()))
    })
}

/// Bevy's plugins as dbota sets them up.
fn default_plugins(args: &Args, settings: &config::Settings) -> impl PluginGroup {
    DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "dbota".into(),
                // In a browser: the page's canvas, sized to the page.
                canvas: Some("#dbota".into()),
                fit_canvas_to_parent: true,
                resolution: WindowResolution::new(args.window.0, args.window.1),
                // `DBOTA_NO_VSYNC` for runs started by a launcher, as the
                // drill is: vsync stalls while the screen is locked.
                present_mode: if args.no_vsync || std::env::var_os("DBOTA_NO_VSYNC").is_some() {
                    PresentMode::AutoNoVsync
                } else {
                    PresentMode::AutoVsync
                },
                resizable: true,
                ..default()
            }),
            ..default()
        })
        // DBOTA_THREADS caps the threads Bevy's pools take (all cores by
        // default), to time a busy machine.
        .set(TaskPoolPlugin {
            task_pool_options: match std::env::var("DBOTA_THREADS")
                .ok()
                .and_then(|t| t.parse().ok())
            {
                Some(max) => TaskPoolOptions {
                    max_total_threads: max,
                    ..default()
                },
                None => TaskPoolOptions::default(),
            },
        })
        .set(bevy::pbr::PbrPlugin {
            use_gpu_instance_buffer_builder: settings.quality.gpu_batching,
            ..default()
        })
        .set(AssetPlugin {
            // A browser fetches them over HTTP, next to the page.
            file_path: if cfg!(target_arch = "wasm32") {
                "assets".into()
            } else {
                concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into()
            },
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
