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
/// `dota_force_right_click_attack`; "To Follow" by default).
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
    /// How strongly units and paths are set apart from the ground.
    pub clarity: Clarity,
    /// The game camera.
    pub camera: CameraSettings,
    /// The profile name, which games without an account (local lobbies,
    /// direct connects) show this player under, as Dota's local games show
    /// the Steam name. Empty: [`crate::main_menu::DEFAULT_NAME`].
    pub name: String,
    /// The program a local lobby starts for each seat left to a bot, as a
    /// command line (see `main_menu::launch`). Empty: none set.
    pub bot_command: String,
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

/// Characters a profile name has at most.
pub const MAX_NAME: usize = 24;
/// Characters a bot command has at most.
pub const MAX_BOT_COMMAND: usize = 1024;

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
            clarity: Clarity::Off,
            camera: CameraSettings::default(),
            name: String::new(),
            bot_command: String::new(),
            teleport_requires_halt: false,
            channels_require_stop: false,
            right_click_allies: RightClickAllies::Follow,
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
            "bot_command" => {
                if value.chars().count() > MAX_BOT_COMMAND || value.chars().any(char::is_control) {
                    return Err(format!(
                        "a bot command is at most {MAX_BOT_COMMAND} printable characters"
                    ));
                }
                value.clone_into(&mut self.bot_command);
            }
            other => return Err(format!("no setting called '{other}'")),
        }
        Ok(())
    }

    /// The settings as the text of a file [`Settings::parse`] reads back.
    pub fn render(&self) -> String {
        let flag = |on: bool| if on { "true" } else { "false" };
        let mut out =
            String::from("# dbota settings. Keys use the names listed in the client README.\n");
        if !self.name.is_empty() {
            out.push_str(&format!("name = {}\n", self.name));
        }
        if !self.bot_command.is_empty() {
            out.push_str(&format!("bot_command = {}\n", self.bot_command));
        }
        out.push_str(&format!("volume = {:.2}\n", self.volume));
        out.push_str(&format!("muted = {}\n", flag(self.muted)));
        out.push_str(&format!("voice_responses = {}\n", flag(self.voice)));
        out.push_str(&format!("ui_scale = {:.2}\n", self.ui_scale));
        for (name, on) in [
            ("damage_numbers", self.aids.numbers),
            ("edge_pan", self.aids.edge_pan),
            ("animate_portrait", self.animate_portrait),
            ("scoreboard_items", self.scoreboard_items),
            ("teleport_requires_halt", self.teleport_requires_halt),
            ("channels_require_stop", self.channels_require_stop),
        ] {
            out.push_str(&format!("{name} = {}\n", flag(on)));
        }
        out.push_str(&format!(
            "right_click_allies = {}\n",
            self.right_click_allies.name()
        ));
        out.push_str(&format!("save_replays = {}\n", flag(self.replays.keep)));
        out.push_str(&format!(
            "replays_max_count = {}\nreplays_max_mib = {}\n",
            self.replays.max_count,
            self.replays.max_bytes >> 20
        ));
        out.push_str(&format!("display_mode = {}\n", self.display.name()));
        let q = &self.quality;
        out.push_str(&format!(
            "shadows = {}\n",
            ["off", "low", "high"][usize::from(q.shadows.min(2))]
        ));
        out.push_str(&format!("anti_aliasing = {}\n", q.anti_alias.name()));
        out.push_str(&format!("render_scale = {:.3}\n", q.render_scale));
        out.push_str(&format!("texture_size = {}\n", q.texture_size));
        out.push_str(&format!("props = {}\n", flag(q.props)));
        out.push_str(&format!("fps_cap = {}\n", q.fps_cap));
        out.push_str(&format!("gpu_batching = {}\n", flag(q.gpu_batching)));
        out.push_str(&format!("auto_quality = {}\n", flag(q.auto)));
        out.push_str(&format!("visual_clarity = {}\n", self.clarity.name()));
        let c = &self.camera;
        for (name, value) in [
            ("camera_pitch", c.pitch),
            ("camera_fov", c.fov),
            ("camera_distance", c.distance),
            ("camera_distance_min", c.distance_min),
            ("camera_distance_max", c.distance_max),
            ("camera_pan_speed", c.pan_speed),
        ] {
            out.push_str(&format!("{name} = {value}\n"));
        }
        for (binding, chord) in keys::all().zip(self.keys) {
            let chord = chord.map_or_else(|| "none".to_owned(), Chord::name);
            out.push_str(&format!("key.{} = {chord}\n", binding.name));
        }
        out
    }
}

/// Lines read from a settings file at most.
const MAX_LINES: usize = 512;

/// Where the settings live when no path is given:
/// `$XDG_CONFIG_HOME/dbota/client.conf`, else `~/.config/dbota/client.conf`.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("dbota").join("client.conf"))
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

    /// Dota's defaults: right clicks follow allies, and no channel waits for a stop.
    #[test]
    fn the_right_click_and_channel_options_start_as_dotas_and_survive_the_file() {
        let fresh = Settings::default();
        assert_eq!(fresh.right_click_allies, RightClickAllies::Follow);
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
