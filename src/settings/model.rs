//! What the settings screen offers, apart from any drawing: its tabs and
//! every option on them as a toggle, a slider or a choice from a list,
//! each reading and writing one field of [`Settings`].

use crate::config::{AntiAlias, Clarity, DISPLAY_MODES, PRESETS, Quality, Settings};

/// The screen's tabs, as Dota's settings have them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Hotkeys,
    Options,
    Video,
    Audio,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Hotkeys, Tab::Options, Tab::Video, Tab::Audio];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Hotkeys => "HOTKEYS",
            Tab::Options => "OPTIONS",
            Tab::Video => "VIDEO",
            Tab::Audio => "AUDIO",
        }
    }
}

/// An option that is on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    DamageNumbers,
    EdgePan,
    AnimatePortrait,
    ScoreboardItems,
    VoiceResponses,
    Muted,
    AutoQuality,
    Props,
    GpuBatching,
}

impl Toggle {
    pub fn label(self) -> &'static str {
        match self {
            Toggle::DamageNumbers => "Damage numbers",
            Toggle::EdgePan => "Edge pan the camera",
            Toggle::AnimatePortrait => "Animate portrait",
            Toggle::ScoreboardItems => "Scoreboard: show items",
            Toggle::VoiceResponses => "Hero voice responses",
            Toggle::Muted => "Mute all sound",
            Toggle::AutoQuality => "Lower quality while frames run slow",
            Toggle::Props => "Map props",
            Toggle::GpuBatching => "GPU culling and batching",
        }
    }

    pub fn get(self, s: &Settings) -> bool {
        match self {
            Toggle::DamageNumbers => s.aids.numbers,
            Toggle::EdgePan => s.aids.edge_pan,
            Toggle::AnimatePortrait => s.animate_portrait,
            Toggle::ScoreboardItems => s.scoreboard_items,
            Toggle::VoiceResponses => s.voice,
            Toggle::Muted => s.muted,
            Toggle::AutoQuality => s.quality.auto,
            Toggle::Props => s.quality.props,
            Toggle::GpuBatching => s.quality.gpu_batching,
        }
    }

    pub fn set(self, s: &mut Settings, on: bool) {
        let field = match self {
            Toggle::DamageNumbers => &mut s.aids.numbers,
            Toggle::EdgePan => &mut s.aids.edge_pan,
            Toggle::AnimatePortrait => &mut s.animate_portrait,
            Toggle::ScoreboardItems => &mut s.scoreboard_items,
            Toggle::VoiceResponses => &mut s.voice,
            Toggle::Muted => &mut s.muted,
            Toggle::AutoQuality => &mut s.quality.auto,
            Toggle::Props => &mut s.quality.props,
            Toggle::GpuBatching => &mut s.quality.gpu_batching,
        };
        *field = on;
    }
}

/// An option within a range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slider {
    Volume,
    InterfaceScale,
    RenderScale,
    DenyOnRightClick,
}

impl Slider {
    pub fn label(self) -> &'static str {
        match self {
            Slider::Volume => "Master volume",
            Slider::InterfaceScale => "Interface size",
            Slider::RenderScale => "Render resolution",
            Slider::DenyOnRightClick => "Right click denies an ally below",
        }
    }

    /// The least value, the most, and the step between.
    pub fn range(self) -> (f32, f32, f32) {
        match self {
            Slider::Volume => (0.0, 1.0, 0.01),
            Slider::InterfaceScale => (0.5, 2.0, 0.05),
            Slider::RenderScale => (0.25, 1.0, 0.05),
            Slider::DenyOnRightClick => (0.0, 100.0, 5.0),
        }
    }

    pub fn get(self, s: &Settings) -> f32 {
        match self {
            Slider::Volume => s.volume,
            Slider::InterfaceScale => s.ui_scale,
            Slider::RenderScale => s.quality.render_scale,
            Slider::DenyOnRightClick => f32::from(s.deny_on_right_click_pct),
        }
    }

    /// Sets the value at `share` of the way along the range, on a step.
    pub fn set_share(self, s: &mut Settings, share: f32) {
        let (lo, hi, step) = self.range();
        let raw = lo + share.clamp(0.0, 1.0) * (hi - lo);
        let value = (lo + ((raw - lo) / step).round() * step).clamp(lo, hi);
        match self {
            Slider::Volume => s.volume = value,
            Slider::InterfaceScale => s.ui_scale = value,
            Slider::RenderScale => s.quality.render_scale = value,
            Slider::DenyOnRightClick => s.deny_on_right_click_pct = value.round() as u8,
        }
    }

    /// How far along the range the value stands, 0 to 1.
    pub fn share(self, s: &Settings) -> f32 {
        let (lo, hi, _) = self.range();
        ((self.get(s) - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    /// The value as the screen shows it.
    pub fn shown(self, s: &Settings) -> String {
        match self {
            Slider::DenyOnRightClick if s.deny_on_right_click_pct == 0 => "Off".to_owned(),
            Slider::DenyOnRightClick => format!("{}%", s.deny_on_right_click_pct),
            _ => format!("{:.0}%", self.get(s) * 100.0),
        }
    }
}

/// An option picked from a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    DisplayMode,
    Quality,
    Shadows,
    AntiAliasing,
    VisualClarity,
    FrameCap,
    TextureSize,
}

/// The frame caps offered; 0 for none.
const FRAME_CAPS: [u32; 6] = [0, 30, 60, 120, 144, 240];
/// The texture sizes offered.
const TEXTURE_SIZES: [u32; 6] = [512, 1024, 2048, 4096, 8192, 16_384];

impl Choice {
    pub fn label(self) -> &'static str {
        match self {
            Choice::DisplayMode => "Display mode",
            Choice::Quality => "Quality preset",
            Choice::Shadows => "Shadows",
            Choice::AntiAliasing => "Anti-aliasing",
            Choice::VisualClarity => "Visual clarity",
            Choice::FrameCap => "Frame rate cap",
            Choice::TextureSize => "Largest texture size",
        }
    }

    /// What may be picked, as shown.
    pub fn options(self) -> Vec<String> {
        match self {
            Choice::DisplayMode => vec![
                "Windowed".into(),
                "Windowed fullscreen".into(),
                "Fullscreen".into(),
            ],
            Choice::Quality => PRESETS.iter().map(|p| title(p)).collect(),
            Choice::Shadows => vec!["Off".into(), "Low".into(), "High".into()],
            Choice::AntiAliasing => AntiAlias::ALL.iter().map(|a| anti_alias_name(*a)).collect(),
            Choice::VisualClarity => Clarity::ALL.iter().map(|c| title(c.name())).collect(),
            Choice::FrameCap => FRAME_CAPS
                .iter()
                .map(|c| {
                    if *c == 0 {
                        "None".into()
                    } else {
                        c.to_string()
                    }
                })
                .collect(),
            Choice::TextureSize => TEXTURE_SIZES.iter().map(u32::to_string).collect(),
        }
    }

    /// The option picked now; none for settings no option stands for (a
    /// mix of presets).
    pub fn current(self, s: &Settings) -> Option<usize> {
        let q = &s.quality;
        match self {
            Choice::DisplayMode => DISPLAY_MODES.iter().position(|(m, _)| *m == s.display),
            Choice::Quality => q
                .preset_name()
                .and_then(|n| PRESETS.iter().position(|p| *p == n)),
            Choice::Shadows => Some(usize::from(q.shadows.min(2))),
            Choice::AntiAliasing => AntiAlias::ALL.iter().position(|a| *a == q.anti_alias),
            Choice::VisualClarity => Clarity::ALL.iter().position(|c| *c == s.clarity),
            Choice::FrameCap => FRAME_CAPS.iter().position(|c| *c == q.fps_cap),
            Choice::TextureSize => TEXTURE_SIZES.iter().position(|t| *t == q.texture_size),
        }
    }

    /// Picks option `at`; one past the list picks nothing.
    pub fn pick(self, s: &mut Settings, at: usize) {
        let q = &mut s.quality;
        match self {
            Choice::DisplayMode => {
                if let Some((mode, _)) = DISPLAY_MODES.get(at) {
                    s.display = *mode;
                }
            }
            Choice::Quality => {
                if let Some(preset) = PRESETS.get(at).and_then(|p| Quality::preset(p)) {
                    *q = Quality {
                        auto: q.auto,
                        ..preset
                    };
                }
            }
            Choice::Shadows if at < 3 => q.shadows = at as u8,
            Choice::Shadows => {}
            Choice::AntiAliasing => {
                q.anti_alias = AntiAlias::ALL.get(at).copied().unwrap_or(q.anti_alias)
            }
            Choice::VisualClarity => s.clarity = Clarity::ALL.get(at).copied().unwrap_or(s.clarity),
            Choice::FrameCap => q.fps_cap = FRAME_CAPS.get(at).copied().unwrap_or(q.fps_cap),
            Choice::TextureSize => {
                q.texture_size = TEXTURE_SIZES.get(at).copied().unwrap_or(q.texture_size);
            }
        }
    }

    /// What the choice shows now.
    pub fn shown(self, s: &Settings) -> String {
        self.current(s)
            .and_then(|at| self.options().get(at).cloned())
            .unwrap_or_else(|| "Custom".into())
    }
}

/// A word with its first letter in capitals.
fn title(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn anti_alias_name(a: AntiAlias) -> String {
    match a {
        AntiAlias::Off => "Off",
        AntiAlias::Fxaa => "FXAA",
        AntiAlias::SmaaLow => "SMAA low",
        AntiAlias::SmaaMedium => "SMAA medium",
        AntiAlias::SmaaHigh => "SMAA high",
        AntiAlias::Msaa2 => "MSAA 2x",
        AntiAlias::Msaa4 => "MSAA 4x",
    }
    .into()
}

/// A setting typed in as a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSetting {
    /// The profile name.
    Name,
    /// The program a local lobby runs for a seat left to a bot.
    BotCommand,
}

impl TextSetting {
    pub fn label(self) -> &'static str {
        match self {
            TextSetting::Name => "Name in local and direct games",
            TextSetting::BotCommand => "Bot command",
        }
    }

    /// What the field says while it is empty.
    pub fn placeholder(self) -> &'static str {
        match self {
            TextSetting::Name => crate::main_menu::DEFAULT_NAME,
            TextSetting::BotCommand => "none: empty seats stay open",
        }
    }

    pub fn get(self, settings: &Settings) -> &String {
        match self {
            TextSetting::Name => &settings.name,
            TextSetting::BotCommand => &settings.bot_command,
        }
    }

    pub fn get_mut(self, settings: &mut Settings) -> &mut String {
        match self {
            TextSetting::Name => &mut settings.name,
            TextSetting::BotCommand => &mut settings.bot_command,
        }
    }

    /// Characters it holds at most.
    pub fn max(self) -> usize {
        match self {
            TextSetting::Name => crate::config::MAX_NAME,
            TextSetting::BotCommand => crate::config::MAX_BOT_COMMAND,
        }
    }
}

/// One row of a tab other than the hotkeys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Heading(&'static str),
    /// A line typed in.
    Text(TextSetting),
    Toggle(Toggle),
    Slider(Slider),
    Choice(Choice),
}

/// The rows of a tab other than the hotkeys, as Dota's tabs group them. A
/// browser's page owns the canvas' size: no display mode there.
pub fn controls(tab: Tab) -> Vec<Control> {
    use Control::{Choice as C, Heading as H, Slider as S, Toggle as T};
    match tab {
        Tab::Hotkeys => Vec::new(),
        Tab::Options => vec![
            H("PROFILE"),
            Control::Text(TextSetting::Name),
            H("INTERFACE"),
            S(Slider::InterfaceScale),
            T(Toggle::DamageNumbers),
            T(Toggle::AnimatePortrait),
            T(Toggle::ScoreboardItems),
            H("CONTROLS"),
            S(Slider::DenyOnRightClick),
            H("CAMERA"),
            T(Toggle::EdgePan),
            H("LOCAL LOBBY"),
            Control::Text(TextSetting::BotCommand),
        ],
        Tab::Video => {
            let mut rows = vec![H("DISPLAY")];
            if !cfg!(target_arch = "wasm32") {
                rows.push(C(Choice::DisplayMode));
            }
            rows.extend([
                C(Choice::FrameCap),
                S(Slider::RenderScale),
                H("QUALITY"),
                C(Choice::Quality),
                C(Choice::Shadows),
                C(Choice::AntiAliasing),
                C(Choice::VisualClarity),
                T(Toggle::AutoQuality),
                H("ON THE NEXT START"),
                C(Choice::TextureSize),
                T(Toggle::Props),
                T(Toggle::GpuBatching),
            ]);
            rows
        }
        Tab::Audio => vec![
            H("SOUND"),
            S(Slider::Volume),
            T(Toggle::Muted),
            T(Toggle::VoiceResponses),
        ],
    }
}
