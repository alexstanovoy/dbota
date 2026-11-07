//! What the player may set: hotkeys, sound and which aids are drawn, read
//! from and written to a plain `name = value` file.

use std::path::PathBuf;

use crate::keys::{self, Action, Chord};

/// The aids drawn over the world, each of which may be turned off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aids {
    /// Damage, gold and deny numbers rising off units.
    pub numbers: bool,
    /// Whether the cursor at the window edge pans the camera.
    pub edge_pan: bool,
}

/// The game camera: a fixed yaw (north up), looking down at its pitch through
/// its lens from its distance, which the wheel moves within its range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraSettings {
    /// Degrees below the horizon.
    pub pitch: f32,
    /// The lens: the vertical field of view in degrees.
    pub fov: f32,
    /// Distance from the focus point along the view at the start, in world
    /// units.
    pub distance: f32,
    /// How near the wheel brings the camera.
    pub distance_min: f32,
    /// How far the wheel takes it.
    pub distance_max: f32,
    /// Pan speed in world units a second at the starting distance; nearer, it
    /// pans slower, farther, faster.
    pub pan_speed: f32,
}

impl CameraSettings {
    /// The wheel's range, nearest first, however the two are set.
    pub fn range(&self) -> (f32, f32) {
        (
            self.distance_min.min(self.distance_max),
            self.distance_min.max(self.distance_max),
        )
    }
}

impl Default for CameraSettings {
    /// A long lens from far out, which shows a lane fight whole; panning at
    /// Dota's `dota_camera_speed`.
    fn default() -> CameraSettings {
        CameraSettings {
            pitch: 58.0,
            fov: 25.0,
            distance: 4400.0,
            distance_min: 1600.0,
            distance_max: 6500.0,
            pan_speed: 3031.0,
        }
    }
}

/// How the picture's edges are smoothed. Every choice runs on WebGPU and
/// WebGL2 alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AntiAlias {
    /// Hard edges.
    Off,
    /// FXAA: one cheap pass over the finished picture.
    Fxaa,
    /// SMAA: edge search and blending over the finished picture, with four,
    /// eight or sixteen search steps (the last also finds corners).
    SmaaLow,
    SmaaMedium,
    SmaaHigh,
    /// Multisampling with 2 or 4 samples a pixel: geometry edges only.
    Msaa2,
    Msaa4,
}

impl AntiAlias {
    /// Cheapest to dearest, as the settings list them.
    pub const ALL: [AntiAlias; 7] = [
        AntiAlias::Off,
        AntiAlias::Fxaa,
        AntiAlias::SmaaLow,
        AntiAlias::SmaaMedium,
        AntiAlias::SmaaHigh,
        AntiAlias::Msaa2,
        AntiAlias::Msaa4,
    ];

    /// What a fresh install smooths with: a mild, cheap pass.
    pub const DEFAULT: AntiAlias = AntiAlias::Fxaa;

    /// The name settings files and `--set` use.
    pub fn name(self) -> &'static str {
        match self {
            AntiAlias::Off => "off",
            AntiAlias::Fxaa => "fxaa",
            AntiAlias::SmaaLow => "smaa_low",
            AntiAlias::SmaaMedium => "smaa_medium",
            AntiAlias::SmaaHigh => "smaa_high",
            AntiAlias::Msaa2 => "msaa_2x",
            AntiAlias::Msaa4 => "msaa_4x",
        }
    }

    pub fn named(name: &str) -> Option<AntiAlias> {
        let name = name.to_ascii_lowercase();
        AntiAlias::ALL.into_iter().find(|a| a.name() == name)
    }
}

/// How strongly units and paths are set apart from the ground they stand
/// on: the scenery a little duller and darker, shade around the forest, the
/// units a little more vivid, a dark disc under every unit's feet and a ring
/// at every hero's; see `clarity.rs`, `fog.wgsl` and `overlay.rs`. Off by
/// default, as Dota draws units: no outline, the side's ring under what is
/// selected and a closing ring on what the cursor comes onto (`pick.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clarity {
    Off,
    Low,
    High,
}

impl Clarity {
    /// Weakest to strongest, as the settings list them.
    pub const ALL: [Clarity; 3] = [Clarity::Off, Clarity::Low, Clarity::High];

    /// The name settings files and `--set` use.
    pub fn name(self) -> &'static str {
        match self {
            Clarity::Off => "off",
            Clarity::Low => "low",
            Clarity::High => "high",
        }
    }

    pub fn named(name: &str) -> Option<Clarity> {
        let name = name.to_ascii_lowercase();
        Clarity::ALL.into_iter().find(|c| c.name() == name)
    }

    /// How strongly the aid is drawn, 0 to 1.
    pub fn strength(self) -> f32 {
        match self {
            Clarity::Off => 0.0,
            Clarity::Low => 0.5,
            Clarity::High => 1.0,
        }
    }
}

/// What a right click on an allied unit does (Dota's "Right-Click Allies",
/// `dota_force_right_click_attack`; "To Deny" by default: a worn creep is attacked without the attack key, anything else followed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightClickAllies {
    /// Follow the unit.
    Follow,
    /// Attack the unit, as the attack key does: sent only when the game lets
    /// it be denied, else nothing happens but a line saying why.
    Attack,
    /// Deny the unit if it can be denied, else follow it.
    Deny,
}

impl RightClickAllies {
    /// In the order of Dota's list.
    pub const ALL: [RightClickAllies; 3] = [
        RightClickAllies::Follow,
        RightClickAllies::Attack,
        RightClickAllies::Deny,
    ];

    /// The name settings files and `--set` use.
    pub fn name(self) -> &'static str {
        match self {
            RightClickAllies::Follow => "follow",
            RightClickAllies::Attack => "attack",
            RightClickAllies::Deny => "deny",
        }
    }

    pub fn named(name: &str) -> Option<RightClickAllies> {
        let name = name.to_ascii_lowercase();
        RightClickAllies::ALL.into_iter().find(|c| c.name() == name)
    }
}

/// What the picture may cost; see `quality.rs`. The defaults draw it whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    /// The sun's shadows: 0 none, 1 one cascade of 1024 texels, 2 two
    /// cascades of 2048.
    pub shadows: u8,
    /// How edges and shimmer are smoothed; see [`AntiAlias`].
    pub anti_alias: AntiAlias,
    /// The 3D scene's resolution against the window's, 0.25 to 1; the
    /// interface is always drawn whole.
    pub render_scale: f32,
    /// Texels of a model's or the map's texture side at most: larger ones
    /// are halved as they load (takes effect on the next start).
    pub texture_size: u32,
    /// Whether the map's props (walls, statues, rocks, decals) are drawn
    /// (takes effect on the next start).
    pub props: bool,
    /// Frames a second at most; 0 for no cap.
    pub fps_cap: u32,
    /// Whether draws are culled and batched on the GPU with compute shaders
    /// (takes effect on the next start). A software renderer runs those
    /// several times slower than the CPU builds the batches.
    pub gpu_batching: bool,
    /// Whether the settings above are lowered a step at a time while frames
    /// run slower than the cap (or 30 fps) allows.
    pub auto: bool,
}

/// The quality presets by name, best first.
pub const PRESETS: [&str; 4] = ["high", "medium", "low", "potato"];

impl Quality {
    /// A preset by name.
    pub fn preset(name: &str) -> Option<Quality> {
        let high = Quality::default();
        Some(match name.to_ascii_lowercase().as_str() {
            "high" => high,
            "medium" => Quality {
                shadows: 1,
                anti_alias: AntiAlias::Fxaa,
                texture_size: 2048,
                ..high
            },
            "low" => Quality {
                shadows: 0,
                anti_alias: AntiAlias::Off,
                render_scale: 0.75,
                texture_size: 1024,
                ..high
            },
            "potato" => Quality {
                shadows: 0,
                anti_alias: AntiAlias::Off,
                render_scale: 0.5,
                texture_size: 512,
                props: false,
                fps_cap: 30,
                gpu_batching: false,
                ..high
            },
            _ => return None,
        })
    }

    /// The name of the preset these settings are, if any (`auto` aside).
    pub fn preset_name(&self) -> Option<&'static str> {
        PRESETS
            .iter()
            .find(|name| {
                Quality::preset(name).is_some_and(|p| {
                    Quality {
                        auto: self.auto,
                        ..p
                    } == *self
                })
            })
            .copied()
    }

    /// One step cheaper of what can change while the client runs, if any:
    /// multisampling, then shadows, then the scene's resolution.
    pub fn lowered(&self) -> Option<Quality> {
        let mut q = *self;
        if q.anti_alias != AntiAlias::Off {
            q.anti_alias = AntiAlias::Off;
        } else if q.shadows > 0 {
            q.shadows -= 1;
        } else if q.render_scale > 0.5 {
            q.render_scale = (q.render_scale - 0.125).max(0.5);
        } else {
            return None;
        }
        Some(q)
    }
}

impl Default for Quality {
    fn default() -> Quality {
        Quality {
            shadows: 2,
            anti_alias: AntiAlias::DEFAULT,
            render_scale: 1.0,
            texture_size: 16_384,
            props: true,
            fps_cap: 0,
            gpu_batching: true,
            auto: false,
        }
    }
}

/// How the game window sits on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    /// A window with a frame, at the size it was given.
    Windowed,
    /// A frameless window over the whole monitor, which Dota calls
    /// "windowed fullscreen": the desktop's mode is kept, Alt-Tab is quick.
    Borderless,
    /// The monitor's own mode, taken over by the game.
    Exclusive,
}

/// Every display mode with its name in the file, in the order the settings
/// list them.
pub const DISPLAY_MODES: [(DisplayMode, &str); 3] = [
    (DisplayMode::Windowed, "windowed"),
    (DisplayMode::Borderless, "borderless"),
    (DisplayMode::Exclusive, "fullscreen"),
];

impl DisplayMode {
    /// A mode by the name the file gives it.
    pub fn named(name: &str) -> Option<DisplayMode> {
        let name = name.to_ascii_lowercase();
        DISPLAY_MODES
            .iter()
            .find(|(_, named)| *named == name)
            .map(|(mode, _)| *mode)
    }

    /// The name the file gives this mode.
    pub fn name(self) -> &'static str {
        DISPLAY_MODES
            .iter()
            .find(|(mode, _)| *mode == self)
            .map_or("windowed", |(_, name)| name)
    }
}

impl Default for DisplayMode {
    /// Windowed fullscreen on a desktop; a browser's canvas is a window of
    /// the page until the player picks fullscreen (a browser grants it only
    /// on a gesture).
    fn default() -> DisplayMode {
        if cfg!(target_arch = "wasm32") {
            DisplayMode::Windowed
        } else {
            DisplayMode::Borderless
        }
    }
}

/// Everything the settings file holds.
#[derive(Clone, Debug, PartialEq, bevy::prelude::Resource)]
pub struct Settings {
    /// Master volume, 0 to 1.
    pub volume: f32,
    /// Whether all sound is off.
    pub muted: bool,
    /// Whether one's hero answers orders in its own voice.
    pub voice: bool,
    /// The interface size against the one chosen for the window, 0.5 to 2.
    pub ui_scale: f32,
    /// Which aids are drawn.
    pub aids: Aids,
    /// The chord of every action, in [`keys::BINDINGS`] order; none for one
    /// left unbound.
    pub keys: [Option<Chord>; keys::COUNT],
    /// Whether the portrait plays the unit's idle animation (Dota's "Animate
    /// Portrait"); off, it shows a still picture.
    pub animate_portrait: bool,
    /// Whether the scoreboard shows every player's items beside the rows
    /// (Dota's Show Items).
    pub scoreboard_items: bool,
    /// What the picture may cost.
    pub quality: Quality,
    /// How the window sits on the screen.
    pub display: DisplayMode,
    /// The size of the window, or of the monitor's mode in exclusive fullscreen; none keeps the
    /// monitor's own (a window without a choice opens at the size the command line gives it).
    pub resolution: Option<(u32, u32)>,
    /// How strongly units and paths are set apart from the ground.
    pub clarity: Clarity,
    /// The game camera.
    pub camera: CameraSettings,
    /// The profile name, which games without an account (local lobbies,
    /// direct connects) show this player under, as Dota's local games show
    /// the Steam name. Empty: [`crate::main_menu::DEFAULT_NAME`].
    pub name: String,
    /// Whether an order given while a teleport channels waits for it to land
    /// instead of breaking it (Dota's "Teleport Requires Hold/Cancel to
    /// Stop", `dota_player_teleport_requires_halt`; off in Dota).
    pub teleport_requires_halt: bool,
    /// The same for every other channelled ability and item (Dota's
    /// "Abilities Require Hold/Cancel to Stop",
    /// `dota_player_channels_require_stop`; off in Dota).
    pub channels_require_stop: bool,
    /// What a right click on an allied unit does.
    pub right_click_allies: RightClickAllies,
    /// Whether the client keeps the replays of the player's matches, and how many.
    pub replays: crate::replays::Policy,
}

/// Settings older versions wrote that nothing reads now (a bot command for seats of the local
/// lobby, the bloom switch, the aids that became always-on or went with their feature). A
/// settings file keeps loading with them in it; the next save leaves them out.
pub const RETIRED_KEYS: &[&str] = &[
    "bot_command",
    "bloom",
    "deny_on_right_click_pct",
    "attack_range",
    "raze_marks",
    "last_hit_marks",
    "tower_ranges",
];

/// Characters a profile name has at most.
pub const MAX_NAME: usize = 24;

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            volume: 0.7,
            muted: false,
            voice: true,
            ui_scale: 1.0,
            aids: Aids {
                numbers: true,
                edge_pan: true,
            },
            keys: keys::BINDINGS.map(|b| b.default),
            animate_portrait: true,
            scoreboard_items: true,
            quality: Quality::default(),
            display: DisplayMode::default(),
            resolution: None,
            clarity: Clarity::Off,
            camera: CameraSettings::default(),
            name: String::new(),
            teleport_requires_halt: false,
            channels_require_stop: false,
            right_click_allies: RightClickAllies::Deny,
            replays: crate::replays::Policy::default(),
        }
    }
}

impl Settings {
    /// The chord bound to an action, if any.
    pub fn key(&self, action: Action) -> Option<Chord> {
        self.keys[keys::index_of(action)?]
    }

    /// What a HUD slot shows of an action's binding: its chord's name, or
    /// nothing for an unbound one.
    pub fn label(&self, action: Action) -> String {
        self.key(action).map_or_else(String::new, Chord::name)
    }

    /// The action that casts ability slot `slot` of a unit holding `count`.
    ///
    /// The last slot of a unit with four or more is its ultimate; the rest
    /// take the ability actions in order.
    pub fn ability_action(slot: u8, count: usize) -> Action {
        if count >= 4 && usize::from(slot) + 1 == count {
            return Action::Ultimate;
        }
        Action::Ability(slot)
    }

    /// Reads settings from text, keeping the defaults for anything absent.
    /// Returns them with one complaint per line that could not be read.
    pub fn parse(text: &str) -> (Settings, Vec<String>) {
        let mut settings = Settings::default();
        let mut complaints = Vec::new();
        for (number, line) in text.lines().enumerate().take(MAX_LINES) {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                complaints.push(format!("line {}: expected name = value", number + 1));
                continue;
            };
            if let Err(why) = settings.set(name.trim(), value.trim()) {
                complaints.push(format!("line {}: {why}", number + 1));
            }
        }
        (settings, complaints)
    }

    /// Sets one named value, as a line `name = value` of the file does.
    pub fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        if let Some(action) = name.strip_prefix("key.") {
            let (at, _) =
                keys::named(action).ok_or_else(|| format!("no action called '{action}'"))?;
            self.keys[at] = match value {
                "none" => None,
                chord => {
                    Some(Chord::parse(chord).ok_or_else(|| format!("no key called '{chord}'"))?)
                }
            };
            return Ok(());
        }
        // A key an older version wrote and no version reads any more: its file still loads.
        if RETIRED_KEYS.contains(&name) {
            return Ok(());
        }
        let flag = || parse_flag(value).ok_or_else(|| format!("'{value}' is not true or false"));
        match name {
            "volume" => self.volume = parse_unit(value, 0.0, 1.0)?,
            "ui_scale" => self.ui_scale = parse_unit(value, 0.5, 2.0)?,
            "muted" => self.muted = flag()?,
            "damage_numbers" => self.aids.numbers = flag()?,
            "voice_responses" => self.voice = flag()?,
            "edge_pan" => self.aids.edge_pan = flag()?,
            "animate_portrait" => self.animate_portrait = flag()?,
            "scoreboard_items" => self.scoreboard_items = flag()?,
            "display_mode" => {
                self.display = DisplayMode::named(value).ok_or_else(|| {
                    let names: Vec<_> = DISPLAY_MODES.iter().map(|(_, n)| *n).collect();
                    format!(
                        "no display mode called '{value}'; one of {}",
                        names.join(", ")
                    )
                })?
            }
            "resolution" => self.resolution = parse_resolution(value)?,
            "quality" => {
                self.quality = Quality::preset(value).ok_or_else(|| {
                    format!("no quality called '{value}'; one of {}", PRESETS.join(", "))
                })?
            }
            "shadows" => {
                self.quality.shadows = match value.to_ascii_lowercase().as_str() {
                    "off" | "none" | "0" => 0,
                    "low" | "1" => 1,
                    "high" | "2" => 2,
                    _ => return Err(format!("'{value}' is not off, low or high")),
                }
            }
            // The key older versions wrote, read as the choice it was.
            "msaa" => {
                self.quality.anti_alias = match value {
                    "1" | "off" => AntiAlias::Off,
                    "2" => AntiAlias::Msaa2,
                    "4" => AntiAlias::Msaa4,
                    _ => return Err(format!("'{value}' is not 1, 2 or 4")),
                }
            }
            "anti_aliasing" => {
                self.quality.anti_alias = AntiAlias::named(value).ok_or_else(|| {
                    let names: Vec<&str> = AntiAlias::ALL.iter().map(|a| a.name()).collect();
                    format!(
                        "no anti-aliasing called '{value}'; one of {}",
                        names.join(", ")
                    )
                })?
            }
            "render_scale" => self.quality.render_scale = parse_unit(value, 0.25, 1.0)?,
            "texture_size" => {
                self.quality.texture_size = parse_unit(value, 64.0, 16_384.0)? as u32;
            }
            "props" => self.quality.props = flag()?,
            "fps_cap" => self.quality.fps_cap = parse_unit(value, 0.0, 1000.0)? as u32,
            "gpu_batching" => self.quality.gpu_batching = flag()?,
            "auto_quality" => self.quality.auto = flag()?,
            "visual_clarity" => {
                self.clarity = Clarity::named(value).ok_or_else(|| {
                    let names: Vec<&str> = Clarity::ALL.iter().map(|c| c.name()).collect();
                    format!(
                        "no visual clarity called '{value}'; one of {}",
                        names.join(", ")
                    )
                })?
            }
            "camera_pitch" => self.camera.pitch = parse_unit(value, 10.0, 89.0)?,
            "camera_fov" => self.camera.fov = parse_unit(value, 5.0, 120.0)?,
            "camera_distance" => self.camera.distance = parse_unit(value, 100.0, 20_000.0)?,
            "camera_distance_min" => {
                self.camera.distance_min = parse_unit(value, 100.0, 20_000.0)?;
            }
            "camera_distance_max" => {
                self.camera.distance_max = parse_unit(value, 100.0, 20_000.0)?;
            }
            "camera_pan_speed" => self.camera.pan_speed = parse_unit(value, 100.0, 20_000.0)?,
            "teleport_requires_halt" => self.teleport_requires_halt = flag()?,
            "channels_require_stop" => self.channels_require_stop = flag()?,
            "right_click_allies" => {
                self.right_click_allies = RightClickAllies::named(value).ok_or_else(|| {
                    let names: Vec<&str> = RightClickAllies::ALL.iter().map(|c| c.name()).collect();
                    format!(
                        "no right click on allies called '{value}'; one of {}",
                        names.join(", ")
                    )
                })?
            }
            "save_replays" => self.replays.keep = flag()?,
            "replays_max_count" => {
                self.replays.max_count = parse_unit(value, 1.0, 1000.0)? as usize;
            }
            "replays_max_mib" => {
                let least = (crate::replays::MAX_BYTES >> 20) as f32;
                self.replays.max_bytes = (parse_unit(value, least, 65_536.0)? as u64) << 20;
            }
            "name" => {
                if value.chars().count() > MAX_NAME || value.chars().any(char::is_control) {
                    return Err(format!("a name is at most {MAX_NAME} printable characters"));
                }
                value.clone_into(&mut self.name);
            }
            other => return Err(format!("no setting called '{other}'")),
        }
        Ok(())
    }

    /// The settings as the text of a file [`Settings::parse`] reads back: only what the player
    /// changed from the defaults. A default is never written, so that a default changed in a
    /// later version reaches a file written before it, instead of the file keeping the old one.
    pub fn render(&self) -> String {
        self.render_against(&Settings::default())
    }

    /// [`Settings::render`] against other defaults than today's: the lines whose value differs
    /// from `defaults`.
    fn render_against(&self, defaults: &Settings) -> String {
        let mut out =
            String::from("# dbota settings. Keys use the names listed in the client README.\n");
        for ((name, value), (_, default)) in self.entries().into_iter().zip(defaults.entries()) {
            if value != default {
                out.push_str(&format!("{name} = {value}\n"));
            }
        }
        out
    }

    /// Every setting as the name and value its line has, in the file's order.
    fn entries(&self) -> Vec<(String, String)> {
        let flag = |on: bool| if on { "true" } else { "false" }.to_owned();
        let q = &self.quality;
        let c = &self.camera;
        let mut out: Vec<(String, String)> = vec![
            ("name".into(), self.name.clone()),
            ("volume".into(), format!("{:.2}", self.volume)),
            ("muted".into(), flag(self.muted)),
            ("voice_responses".into(), flag(self.voice)),
            ("ui_scale".into(), format!("{:.2}", self.ui_scale)),
            ("damage_numbers".into(), flag(self.aids.numbers)),
            ("edge_pan".into(), flag(self.aids.edge_pan)),
            ("animate_portrait".into(), flag(self.animate_portrait)),
            ("scoreboard_items".into(), flag(self.scoreboard_items)),
            (
                "teleport_requires_halt".into(),
                flag(self.teleport_requires_halt),
            ),
            (
                "channels_require_stop".into(),
                flag(self.channels_require_stop),
            ),
            (
                "right_click_allies".into(),
                self.right_click_allies.name().into(),
            ),
            ("save_replays".into(), flag(self.replays.keep)),
            (
                "replays_max_count".into(),
                self.replays.max_count.to_string(),
            ),
            (
                "replays_max_mib".into(),
                (self.replays.max_bytes >> 20).to_string(),
            ),
            ("display_mode".into(), self.display.name().into()),
            (
                "resolution".into(),
                self.resolution
                    .map_or_else(|| "native".to_owned(), |(w, h)| format!("{w}x{h}")),
            ),
            (
                "shadows".into(),
                ["off", "low", "high"][usize::from(q.shadows.min(2))].into(),
            ),
            ("anti_aliasing".into(), q.anti_alias.name().into()),
            ("render_scale".into(), format!("{:.3}", q.render_scale)),
            ("texture_size".into(), q.texture_size.to_string()),
            ("props".into(), flag(q.props)),
            ("fps_cap".into(), q.fps_cap.to_string()),
            ("gpu_batching".into(), flag(q.gpu_batching)),
            ("auto_quality".into(), flag(q.auto)),
            ("visual_clarity".into(), self.clarity.name().into()),
        ];
        for (name, value) in [
            ("camera_pitch", c.pitch),
            ("camera_fov", c.fov),
            ("camera_distance", c.distance),
            ("camera_distance_min", c.distance_min),
            ("camera_distance_max", c.distance_max),
            ("camera_pan_speed", c.pan_speed),
        ] {
            out.push((name.into(), value.to_string()));
        }
        for (binding, chord) in keys::all().zip(self.keys) {
            let chord = chord.map_or_else(|| "none".to_owned(), Chord::name);
            out.push((format!("key.{}", binding.name), chord));
        }
        out
    }
}

/// Lines read from a settings file at most.
const MAX_LINES: usize = 512;

/// The name of the settings file. Older clients kept every setting, defaults included, in
/// [`OLD_FILE_NAME`]; those pinned defaults a later version changed, so the format has a new name
/// and the old file is not read.
pub const FILE_NAME: &str = "settings.conf";

/// The settings file older clients wrote, which is never read nor deleted.
pub const OLD_FILE_NAME: &str = "client.conf";

/// Why a settings file may not be the retired one: it would be read, and written over.
pub fn refuse_retired(path: &std::path::Path) -> Result<(), String> {
    if path.file_name().is_some_and(|name| name == OLD_FILE_NAME) {
        return Err(format!(
            "{} is the name of an older client's settings file, which is not read; give the settings another name, such as {FILE_NAME}",
            path.display()
        ));
    }
    Ok(())
}

/// The old settings file beside `settings`, when there is one: it is ignored.
pub fn ignored_old_file(settings: &std::path::Path) -> Option<PathBuf> {
    let old = settings.with_file_name(OLD_FILE_NAME);
    (old != settings && old.is_file()).then_some(old)
}

/// Where the settings live when no path is given:
/// `$XDG_CONFIG_HOME/dbota/settings.conf`, else `~/.config/dbota/settings.conf`.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("dbota").join(FILE_NAME))
}

/// A number within `lo..=hi`.
fn parse_unit(value: &str, lo: f32, hi: f32) -> Result<f32, String> {
    let v: f32 = value
        .parse()
        .map_err(|_| format!("'{value}' is not a number"))?;
    if !(lo..=hi).contains(&v) {
        return Err(format!("{v} is outside {lo}..{hi}"));
    }
    Ok(v)
}

/// The smallest and the largest side of a resolution a file may name.
const RESOLUTION_SIDES: std::ops::RangeInclusive<u32> = 320..=16_384;

/// `native`, or a size as `1920x1080`.
fn parse_resolution(value: &str) -> Result<Option<(u32, u32)>, String> {
    if value.eq_ignore_ascii_case("native") {
        return Ok(None);
    }
    let wrong = || format!("'{value}' is neither native nor a size like 1920x1080");
    let (width, height) = value.split_once(['x', 'X']).ok_or_else(wrong)?;
    let width: u32 = width.trim().parse().map_err(|_| wrong())?;
    let height: u32 = height.trim().parse().map_err(|_| wrong())?;
    if !RESOLUTION_SIDES.contains(&width) || !RESOLUTION_SIDES.contains(&height) {
        return Err(format!(
            "{width}x{height} has a side outside {}..{}",
            RESOLUTION_SIDES.start(),
            RESOLUTION_SIDES.end()
        ));
    }
    Ok(Some((width, height)))
}

/// `true` or `false`, also as `on`/`off`, `yes`/`no` and `1`/`0`.
fn parse_flag(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dota's defaults hold no two actions on one chord where both work,
    /// and every binding written to the file reads back, a rebound and an
    /// unbound one too.
    #[test]
    fn the_defaults_clash_nowhere_and_every_binding_survives_the_file() {
        let settings = Settings::default();
        for at in 0..keys::COUNT {
            assert_eq!(
                keys::conflicts(&settings.keys, at),
                [],
                "{}",
                keys::BINDINGS[at].name
            );
        }
        let mut changed = settings.clone();
        changed.keys[0] = Chord::parse("Ctrl+Alt+K");
        changed.keys[1] = None;
        let (read, complaints) = Settings::parse(&changed.render());
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(read, changed);
    }

    /// A settings file with every key some version of the client wrote loads without a word of
    /// complaint, those no version reads now included: an update never breaks a user's file.
    #[test]
    fn a_file_with_every_key_an_older_version_wrote_loads_cleanly() {
        let current = [
            ("volume", "0.50"),
            ("muted", "false"),
            ("ui_scale", "1.00"),
            ("name", "alice"),
            ("damage_numbers", "true"),
            ("voice_responses", "true"),
            ("edge_pan", "true"),
            ("animate_portrait", "true"),
            ("scoreboard_items", "true"),
            ("teleport_requires_halt", "false"),
            ("channels_require_stop", "false"),
            ("right_click_allies", "deny"),
            ("display_mode", "windowed"),
            ("resolution", "1280x720"),
            ("quality", "high"),
            ("shadows", "high"),
            ("msaa", "4"),
            ("anti_aliasing", "off"),
            ("texture_size", "1024"),
            ("props", "true"),
            ("fps_cap", "144"),
            ("gpu_batching", "true"),
            ("auto_quality", "false"),
            ("visual_clarity", "low"),
            ("render_scale", "1.00"),
            ("camera_distance", "1134"),
            ("save_replays", "true"),
            ("replays_max_count", "50"),
            ("replays_max_mib", "512"),
        ];
        let retired = [
            ("bot_command", "/path/to/mybot --server {address}"),
            ("bloom", "true"),
            ("deny_on_right_click_pct", "50"),
            ("attack_range", "true"),
            ("raze_marks", "true"),
            ("last_hit_marks", "true"),
            ("tower_ranges", "true"),
        ];
        let text: String = current
            .iter()
            .chain(&retired)
            .map(|(key, value)| format!("{key} = {value}\n"))
            .collect();
        let (settings, complaints) = Settings::parse(&text);
        assert!(complaints.is_empty(), "{complaints:?}");
        assert!(
            !settings.render().contains("bot_command"),
            "a retired key is not written back"
        );
    }

    /// The settings file has a name of its own, apart from the older clients' `client.conf`, which
    /// is found beside it and ignored, never read; and that name is refused as a settings path.
    #[test]
    fn the_settings_file_is_not_the_old_clients_and_the_old_one_beside_it_is_found_to_be_ignored() {
        let dir =
            crate::test_support::scratch_path(&format!("config_names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        let settings = dir.join(FILE_NAME);
        assert_ne!(FILE_NAME, OLD_FILE_NAME);
        assert_eq!(ignored_old_file(&settings), None, "none beside it yet");
        std::fs::write(dir.join(OLD_FILE_NAME), "right_click_allies = follow\n").expect("written");
        assert_eq!(ignored_old_file(&settings), Some(dir.join(OLD_FILE_NAME)));
        assert_eq!(refuse_retired(&settings), Ok(()));
        let refused = refuse_retired(&dir.join(OLD_FILE_NAME)).expect_err("the retired name");
        assert!(
            refused.contains("older client's settings file"),
            "{refused}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every kind of setting reaches the file and reads back: a setting `entries` leaves out would
    /// never be saved. Each field here differs from its default, so each must be written.
    #[test]
    fn every_setting_survives_the_file() {
        let default = Settings::default();
        let settings = Settings {
            volume: 0.31,
            muted: !default.muted,
            voice: !default.voice,
            ui_scale: 1.5,
            aids: Aids {
                numbers: !default.aids.numbers,
                edge_pan: !default.aids.edge_pan,
            },
            keys: std::array::from_fn(|_| Chord::parse("Ctrl+Alt+K")),
            animate_portrait: !default.animate_portrait,
            scoreboard_items: !default.scoreboard_items,
            quality: Quality {
                shadows: 0,
                anti_alias: AntiAlias::Msaa2,
                render_scale: 0.5,
                texture_size: 512,
                props: !default.quality.props,
                fps_cap: 77,
                gpu_batching: !default.quality.gpu_batching,
                auto: !default.quality.auto,
            },
            display: DisplayMode::Windowed,
            resolution: Some((1280, 720)),
            clarity: Clarity::High,
            camera: CameraSettings {
                pitch: default.camera.pitch + 1.0,
                fov: default.camera.fov + 1.0,
                distance: default.camera.distance + 1.0,
                distance_min: default.camera.distance_min + 1.0,
                distance_max: default.camera.distance_max + 1.0,
                pan_speed: default.camera.pan_speed + 1.0,
            },
            name: "alice".to_owned(),
            teleport_requires_halt: !default.teleport_requires_halt,
            channels_require_stop: !default.channels_require_stop,
            right_click_allies: RightClickAllies::Follow,
            replays: crate::replays::Policy {
                keep: !default.replays.keep,
                max_count: 7,
                max_bytes: 128 << 20,
            },
        };
        let text = settings.render();
        let unwritten: Vec<String> = settings
            .entries()
            .into_iter()
            .zip(default.entries())
            .filter(|((name, _), _)| !text.lines().any(|l| l.starts_with(&format!("{name} = "))))
            .map(|((name, _), _)| name)
            .collect();
        assert!(
            unwritten.is_empty(),
            "never written though changed: {unwritten:?}"
        );
        let (read, complaints) = Settings::parse(&text);
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(read, settings);
    }

    /// A default is not a choice: nothing of the defaults is written.
    #[test]
    fn a_settings_file_of_the_defaults_holds_no_setting() {
        let text = Settings::default().render();
        assert!(
            text.lines().all(|line| line.starts_with('#')),
            "only the comment: {text}"
        );
    }

    /// The root of the bugs where a default changed and a file kept the old one: a file is
    /// written against the defaults of its day, so what the player never changed is not in it,
    /// and a default flipped in code takes effect for a file written before.
    #[test]
    fn a_default_changed_in_code_takes_effect_for_a_file_written_before() {
        let mut old_defaults = Settings {
            right_click_allies: RightClickAllies::Follow,
            ..Settings::default()
        };
        old_defaults.keys[keys::index_of(Action::Item(0)).expect("the first item key")] =
            Chord::parse("1");
        let mut played = old_defaults.clone();
        played.volume = 0.3;
        let file = played.render_against(&old_defaults);
        assert!(!file.contains("right_click_allies"), "{file}");
        assert!(!file.contains("key.item_1"), "{file}");
        let (read, complaints) = Settings::parse(&file);
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(
            read.right_click_allies,
            Settings::default().right_click_allies
        );
        assert_eq!(
            read.key(Action::Item(0)),
            Settings::default().key(Action::Item(0))
        );
        assert!(
            (read.volume - 0.3).abs() < 1e-6,
            "what the player changed stays"
        );
    }

    /// Settings written to the file read back the same, whatever quality
    /// they hold.
    #[test]
    fn every_quality_preset_survives_the_file() {
        for name in PRESETS {
            let settings = Settings {
                quality: Quality::preset(name).expect("a preset"),
                ..Settings::default()
            };
            let (read, complaints) = Settings::parse(&settings.render());
            assert!(complaints.is_empty(), "{name}: {complaints:?}");
            assert_eq!(read, settings, "{name}");
            assert_eq!(read.quality.preset_name(), Some(name));
        }
    }

    /// A file an older version wrote keeps its multisampling choice, and is
    /// written back under the new key.
    #[test]
    fn an_old_msaa_line_is_read_as_the_new_setting() {
        for (old, new) in [
            ("1", AntiAlias::Off),
            ("off", AntiAlias::Off),
            ("2", AntiAlias::Msaa2),
            ("4", AntiAlias::Msaa4),
        ] {
            let (read, complaints) = Settings::parse(&format!("volume = 0.5\nmsaa = {old}\n"));
            assert!(complaints.is_empty(), "{old}: {complaints:?}");
            assert_eq!(read.quality.anti_alias, new, "{old}");
            let written = read.render();
            assert!(!written.contains("msaa ="), "{old}");
            assert!(
                written.contains(&format!("anti_aliasing = {}", new.name())),
                "{old}"
            );
        }
    }

    /// A display mode is written and read back by its name, and a wrong
    /// name is refused with the names that exist.
    #[test]
    fn every_display_mode_survives_the_file() {
        for (mode, _) in DISPLAY_MODES {
            let settings = Settings {
                display: mode,
                ..Settings::default()
            };
            let (read, complaints) = Settings::parse(&settings.render());
            assert!(complaints.is_empty(), "{mode:?}: {complaints:?}");
            assert_eq!(read.display, mode);
        }
        let (_, complaints) = Settings::parse("display_mode = tiny");
        assert!(complaints[0].contains("borderless"), "{complaints:?}");
    }

    /// A resolution is written as native or WxH and read back; nonsense is refused with what
    /// is expected.
    #[test]
    fn a_resolution_survives_the_file_and_nonsense_is_refused() {
        for resolution in [None, Some((1920, 1080)), Some((2560, 1440))] {
            let settings = Settings {
                resolution,
                ..Settings::default()
            };
            let (read, complaints) = Settings::parse(&settings.render());
            assert!(complaints.is_empty(), "{resolution:?}: {complaints:?}");
            assert_eq!(read.resolution, resolution);
        }
        let (_, complaints) = Settings::parse("resolution = wide");
        assert!(
            complaints[0].contains("neither native nor a size"),
            "{complaints:?}"
        );
        let (_, complaints) = Settings::parse("resolution = 10x99999");
        assert!(complaints[0].contains("outside"), "{complaints:?}");
    }

    /// Replays are kept by default, within caps, and the caps survive the file.
    #[test]
    fn replays_are_saved_by_default_within_caps_that_survive_the_file() {
        let fresh = Settings::default();
        assert!(fresh.replays.keep);
        let capped = Settings {
            replays: crate::replays::Policy {
                keep: false,
                max_count: 7,
                max_bytes: 300 << 20,
            },
            ..Settings::default()
        };
        let (read, complaints) = Settings::parse(&capped.render());
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(read.replays, capped.replays);
        let (_, complaints) = Settings::parse("replays_max_mib = 1\nreplays_max_count = 0");
        assert_eq!(complaints.len(), 2, "caps below one replay are refused");
    }

    /// Dota's defaults: a right click denies a worn ally and follows the rest, and no channel waits for a stop.
    #[test]
    fn the_right_click_and_channel_options_start_as_dotas_and_survive_the_file() {
        let fresh = Settings::default();
        assert_eq!(fresh.right_click_allies, RightClickAllies::Deny);
        assert!(!fresh.teleport_requires_halt && !fresh.channels_require_stop);
        for mode in RightClickAllies::ALL {
            let settings = Settings {
                right_click_allies: mode,
                teleport_requires_halt: true,
                channels_require_stop: true,
                ..Settings::default()
            };
            let (read, complaints) = Settings::parse(&settings.render());
            assert!(complaints.is_empty(), "{mode:?}: {complaints:?}");
            assert_eq!(read.right_click_allies, mode);
            assert!(read.teleport_requires_halt && read.channels_require_stop);
        }
    }
}
