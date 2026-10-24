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
mod audio_output;
mod bars;
mod buffs;
mod cache_text;
mod cachefs;
mod camera;
mod capture;
mod chat;
mod clarity;
mod config;
mod coords;
mod courier;
mod daylight;
mod dota_text;
mod drill;
mod effects;
mod fog;
mod fortified;
mod framestats;
mod glyph;
mod heap;
mod hud;
#[cfg(not(target_arch = "wasm32"))]
mod import;
mod input;
mod keys;
mod layers;
#[cfg(not(target_arch = "wasm32"))]
mod logging;
mod main_menu;
mod manifest;
mod map;
mod mapfile;
mod minimap;
mod overlay;
mod pick;
mod pings;
mod portrait;
mod quality;
mod scoreboard;
mod select;
mod session;
mod settings;
mod shop;
mod sound;
mod stats;
mod synth;
mod talents;
mod teleport;
mod terrain;
mod texture_file;
mod tip_content;
mod tip_numbers;
mod tips;
mod trees;
mod ui;
mod units;
mod visual;
mod voice;
mod water;
#[cfg(target_arch = "wasm32")]
mod webcache;
mod window;

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
#[command(
    version,
    about,
    after_help = "BOTA_SEAT_KEY, in the environment only (an argument shows in process lists): the \
64-hex-digit key of the seat a server with reserved seats holds for this client; with --spectate \
the client watches through that seat's eyes."
)]
pub struct Args {
    #[cfg(not(target_arch = "wasm32"))]
    #[command(subcommand)]
    pub command: Option<Command>,
    /// The game server to join directly: `host:port` (TCP), `ws://` or
    /// `wss://`. Without it, `--spectate`, a seat key or a replay, the main
    /// menu opens.
    #[arg(long, value_name = "ADDRESS")]
    pub addr: Option<String>,
    /// The matchmaking service the menu signs in to, `wss://host:port`;
    /// remembered for the next run.
    #[arg(long, env = "DBOTA_SERVICE", value_name = "URL")]
    pub service: Option<String>,
    /// Trust exactly the certificates in this PEM file for the service and
    /// `wss://` games, instead of the public web's (a service of one's own).
    #[arg(long, value_name = "FILE")]
    pub trust_cert: Option<PathBuf>,
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
    /// Root of the asset cache `dbota import` makes.
    #[arg(long, global = true, default_value_os_t = default_cache_dir())]
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
    /// Debug: measure each `--screenshot-at` shot against the picture's
    /// requirements (see `visual.rs`), print them, and exit non-zero when one
    /// fails.
    #[arg(long)]
    pub visual_check: bool,
    /// Debug: draw a screen of the main menu with made-up contents, no
    /// service needed (for screenshots: `--frames N --screenshot FILE`).
    #[arg(long, value_name = "NAME", value_parser = clap::builder::PossibleValuesParser::new(main_menu::PREVIEWS))]
    pub menu_preview: Option<String>,
    /// Debug: keep a tooltip open: `item:ID` (id of the item), `shop:ID`,
    /// `ability:ID:LEVEL`, `stats` (the stats tooltip) or `talents` (the
    /// talent tree).
    #[arg(long, value_name = "KIND:ID")]
    pub tip: Option<String>,
    /// Debug: open the settings screen at a tab (`hotkeys`, `options`,
    /// `video`, `audio`) at the start, for screenshots.
    #[arg(long, value_name = "TAB")]
    pub settings: Option<String>,
    /// Debug: keep the scoreboard open from the start, for screenshots.
    #[arg(long)]
    pub scoreboard: bool,
    /// Debug: open the shop at the start: on a tab (`basics`, `upgrades` or
    /// `neutrals`), with an item's build tree shown (`upgrades:51`) or a
    /// search typed (`basics:/tango`).
    #[arg(long, value_name = "TAB[:ITEM|:/SEARCH]")]
    pub shop: Option<String>,
    /// Debug: select the first unit shown with this name (`Tower@dire`: of
    /// that side), as a click would.
    #[arg(long, value_name = "NAME")]
    pub select: Option<String>,
    /// Debug: draw the ranges and camp boxes as if Alt were held.
    #[arg(long)]
    pub show_ranges: bool,
    /// Debug: the camera's focus at the start, in bota world units.
    #[arg(long, value_name = "X,Y", value_delimiter = ',')]
    pub look: Vec<f32>,
    /// Debug: the camera's distance from the ground at the start.
    #[arg(long, value_name = "UNITS")]
    pub distance: Option<f32>,
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
    /// Debug: with `--bench`, also time each render pass on the GPU (where
    /// the adapter has timestamp queries) and print them as `pass:` lines.
    #[arg(long, requires = "bench")]
    pub bench_gpu: bool,
    /// The window's size at the start, in logical pixels, `WIDTHxHEIGHT`.
    #[arg(long, value_name = "WxH", default_value = "1600x900", value_parser = parse_size)]
    pub window: (u32, u32),
    /// How the window sits on the screen, over the settings file's
    /// `display_mode`: `windowed`, `borderless` (windowed fullscreen, the
    /// default on a desktop) or `fullscreen` (exclusive).
    #[arg(long, value_name = "MODE", value_parser = parse_display_mode)]
    pub display_mode: Option<config::DisplayMode>,
    /// Present without vsync.
    #[arg(long)]
    pub no_vsync: bool,
    /// Play no sound.
    #[arg(long)]
    pub mute: bool,
}

/// What else than playing dbota can do.
#[cfg(not(target_arch = "wasm32"))]
#[derive(clap::Subcommand, Debug, Clone)]
pub enum Command {
    /// Make the asset cache from your Dota 2 install: its models, map, textures, sounds and
    /// texts, converted on this machine for your own use (one to two minutes; nothing is
    /// downloaded). Run again after a Dota update.
    Import {
        /// The `dota 2 beta` folder; by default the usual Steam places are searched.
        #[arg(long, value_name = "DIR")]
        dota_path: Option<PathBuf>,
        /// Import even when the cache was made from the same install already.
        #[arg(long)]
        force: bool,
    },
}

impl Args {
    /// Whether the main menu opens: nothing on the command line names a game.
    pub fn wants_menu(&self) -> bool {
        !cfg!(feature = "web-replay")
            && self.replay.is_none()
            && self.addr.is_none()
            && !self.spectate
            && !crate::session::seat_key_given()
    }
}

/// Where the settings are saved; absent when there is nowhere to.
#[derive(Resource, Clone, Debug)]
pub struct SettingsPath(pub Option<PathBuf>);

/// The version of the asset cache this client reads, as `dbota import`
/// writes it: each version has a folder of its own under `~/.cache/dbota`,
/// so clients of different versions share a machine.
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
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(Command::Import { dota_path, force }) = &args.command {
        return match import::run(dota_path.as_deref(), &args.cache_dir, *force) {
            Ok(true) => AppExit::Success,
            Ok(false) => AppExit::from_code(3),
            Err(why) => {
                eprintln!("dbota import: {why}");
                AppExit::error()
            }
        };
    }
    // Before the first thread starts (see `ui::font_cx`).
    #[cfg(not(target_arch = "wasm32"))]
    let font_cx = ui::font_cx();
    let session = if args.wants_menu() {
        main_menu::idle()
    } else {
        match session::open(&args) {
            Ok(session) => session,
            Err(err) => {
                eprintln!("dbota: {err}");
                return AppExit::error();
            }
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    import::warn_if_stale(&args.cache_dir);
    let manifest = manifest::Manifest::load(&args.cache_dir);
    let texts = dota_text::Texts::load(manifest.root.as_deref());
    let catalog = shop::Catalog::load(&manifest);
    let settings_path = args.config.clone().or_else(config::default_path);
    let store = main_menu::SettingsStore::new(settings_path.clone());
    let mut settings = store.load_settings();
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
    if let Some(mode) = args.display_mode {
        settings.display = mode;
    }
    let mut app = App::new();
    #[cfg(not(target_arch = "wasm32"))]
    app.insert_resource(font_cx);
    let source = cache_source(&mut app, &args.cache_dir);
    app.insert_resource(args.clone())
        .insert_resource(manifest)
        .insert_resource(texts)
        .insert_resource(catalog)
        .insert_resource(settings.clone())
        .insert_resource(SettingsPath(settings_path))
        .insert_resource(store.clone())
        .insert_non_send(session)
        // Must be registered before AssetPlugin (part of DefaultPlugins) builds the sources.
        .register_asset_source(CACHE_SOURCE, source)
        .add_plugins(default_plugins(&args, &settings))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            session::plugin,
            window::plugin,
            map::plugin,
            layers::plugin,
            clarity::plugin,
            daylight::plugin,
            fog::plugin,
            water::plugin,
            units::plugin,
            anim::plugin,
            camera::plugin,
            capture::plugin,
            trees::plugin,
            audio_output::plugin,
        ))
        .add_plugins((
            ui::plugin,
            bars::plugin,
            overlay::plugin,
            hud::plugin,
            chat::plugin,
            input::plugin,
            pick::plugin,
            pings::plugin,
            minimap::plugin,
            portrait::plugin,
            stats::plugin,
        ))
        .add_plugins((
            sound::plugin,
            effects::plugin,
            teleport::plugin,
            shop::plugin,
            glyph::plugin,
            tips::plugin,
            drill::plugin,
            buffs::plugin,
            talents::plugin,
            voice::plugin,
            select::plugin,
            framestats::plugin,
            quality::plugin,
            heap::plugin,
        ))
        .add_plugins((
            fortified::plugin,
            courier::plugin,
            settings::plugin,
            keys::plugin,
            scoreboard::plugin,
        ))
        .add_plugins(main_menu::plugin);
    if let Err(err) = menu(&mut app, &args, store) {
        eprintln!("dbota: {err}");
        return AppExit::error();
    }
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
    if args.bench_gpu {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    }
    app.run()
}

/// The main menu, when the command line names no game: its core connected
/// to the service, or a canned screen of it (`--menu-preview`).
fn menu(app: &mut App, args: &Args, store: main_menu::SettingsStore) -> std::io::Result<()> {
    if let Some(name) = &args.menu_preview {
        let view =
            main_menu::preview(name).ok_or_else(|| std::io::Error::other("no such screen"))?;
        app.insert_resource(main_menu::Preview(view));
        return Ok(());
    }
    if !args.wants_menu() {
        return Ok(());
    }
    #[cfg(not(target_arch = "wasm32"))]
    let pem = args
        .trust_cert
        .as_deref()
        .map(std::fs::read_to_string)
        .transpose()?;
    #[cfg(target_arch = "wasm32")]
    let pem: Option<String> = None;
    let trust = main_menu::Trust::new(pem.as_deref())?;
    let service = args.service.clone().or_else(main_menu::page_service);
    let core = main_menu::MenuCore::new(store, trust.radovid.clone(), service);
    app.insert_non_send(core)
        .insert_non_send(main_menu::Trusted(trust));
    Ok(())
}

/// A display mode by the name the settings file gives it.
fn parse_display_mode(text: &str) -> Result<config::DisplayMode, String> {
    config::DisplayMode::named(text).ok_or_else(|| {
        let names: Vec<_> = config::DISPLAY_MODES.iter().map(|(_, n)| *n).collect();
        format!("'{text}' is not one of {}", names.join(", "))
    })
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
    #[cfg(not(target_arch = "wasm32"))]
    let log = bevy::log::LogPlugin {
        fmt_layer: logging::throttled,
        ..default()
    };
    #[cfg(target_arch = "wasm32")]
    let log = bevy::log::LogPlugin::default();
    DefaultPlugins
        .build()
        // The sound device is dbota's own, see `audio_output`.
        .disable::<bevy::audio::AudioPlugin>()
        .set(log)
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "dbota".into(),
                // In a browser: the page's canvas, sized to the page.
                canvas: Some("#dbota".into()),
                fit_canvas_to_parent: true,
                resolution: WindowResolution::new(args.window.0, args.window.1),
                present_mode: if args.no_vsync {
                    PresentMode::AutoNoVsync
                } else {
                    PresentMode::AutoVsync
                },
                resizable: true,
                mode: window::window_mode(settings.display, true),
                ..default()
            }),
            ..default()
        })
        .set(bevy::render::RenderPlugin {
            render_creation: render_creation(args),
            // Pipelines compile on the render thread, which the app waits for when it ends,
            // never on a pool thread: a process that ends while a pool thread is inside the
            // driver creating one (an unreadable replay quits in the first frames) has the
            // driver's exit handlers free what that thread uses, a crash. A browser and macOS
            // compile this way always.
            synchronous_pipeline_compilation: true,
            ..default()
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
        .set(GltfPlugin {
            convert_coordinates: GltfConvertCoordinates {
                rotate_scene_entity: true,
                ..default()
            },
            ..default()
        })
}

/// The GPU as Bevy asks for it; `--bench-gpu` adds the timestamp queries its
/// pass timings need.
fn render_creation(args: &Args) -> bevy::render::settings::RenderCreation {
    use bevy::render::settings::{WgpuFeatures, WgpuSettings};
    let mut settings = WgpuSettings::default();
    if args.bench_gpu {
        settings.features |= WgpuFeatures::TIMESTAMP_QUERY
            | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_ENCODERS
            | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_PASSES;
    }
    settings.into()
}
