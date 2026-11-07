//! What the settings screen offers, apart from any drawing: its tabs and
//! every option on them as a toggle, a slider or a choice from a list,
//! each reading and writing one field of [`Settings`].

use crate::config::{
    AntiAlias, Clarity, DISPLAY_MODES, DisplayMode, PRESETS, Quality, RightClickAllies, Settings,
};
use crate::keys::{self, Action, Group};
use crate::window::Displays;

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
    TeleportRequiresHalt,
    ChannelsRequireStop,
    VoiceResponses,
    Muted,
    SaveReplays,
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
            Toggle::TeleportRequiresHalt => "Teleport Requires Hold/Cancel to Stop",
            Toggle::ChannelsRequireStop => "Abilities Require Hold/Cancel to Stop",
            Toggle::VoiceResponses => "Hero voice responses",
            Toggle::Muted => "Mute all sound",
            Toggle::SaveReplays => "Save the replays of my matches",
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
            Toggle::TeleportRequiresHalt => s.teleport_requires_halt,
            Toggle::ChannelsRequireStop => s.channels_require_stop,
            Toggle::VoiceResponses => s.voice,
            Toggle::Muted => s.muted,
            Toggle::SaveReplays => s.replays.keep,
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
            Toggle::TeleportRequiresHalt => &mut s.teleport_requires_halt,
            Toggle::ChannelsRequireStop => &mut s.channels_require_stop,
            Toggle::VoiceResponses => &mut s.voice,
            Toggle::Muted => &mut s.muted,
            Toggle::SaveReplays => &mut s.replays.keep,
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
}

impl Slider {
    pub fn label(self) -> &'static str {
        match self {
            Slider::Volume => "Master volume",
            Slider::InterfaceScale => "Interface size",
            Slider::RenderScale => "Render resolution",
        }
    }

    /// The least value, the most, and the step between.
    pub fn range(self) -> (f32, f32, f32) {
        match self {
            Slider::Volume => (0.0, 1.0, 0.01),
            Slider::InterfaceScale => (0.5, 2.0, 0.05),
            Slider::RenderScale => (0.25, 1.0, 0.05),
        }
    }

    pub fn get(self, s: &Settings) -> f32 {
        match self {
            Slider::Volume => s.volume,
            Slider::InterfaceScale => s.ui_scale,
            Slider::RenderScale => s.quality.render_scale,
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
        }
    }

    /// How far along the range the value stands, 0 to 1.
    pub fn share(self, s: &Settings) -> f32 {
        let (lo, hi, _) = self.range();
        ((self.get(s) - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    /// The value as the screen shows it.
    pub fn shown(self, s: &Settings) -> String {
        format!("{:.0}%", self.get(s) * 100.0)
    }
}

/// An option picked from a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    DisplayMode,
    Resolution,
    Quality,
    Shadows,
    AntiAliasing,
    VisualClarity,
    FrameCap,
    TextureSize,
    RightClickAllies,
    ReplayCount,
    ReplaySize,
}

/// The frame caps offered; 0 for none.
const FRAME_CAPS: [u32; 6] = [0, 30, 60, 120, 144, 240];
/// The texture sizes offered.
const TEXTURE_SIZES: [u32; 6] = [512, 1024, 2048, 4096, 8192, 16_384];
/// The numbers of replays offered to keep.
const REPLAY_COUNTS: [usize; 6] = [10, 25, 50, 100, 250, 500];
/// The sizes, in MiB, all the replays kept may take.
const REPLAY_SIZES: [u64; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];

impl Choice {
    pub fn label(self) -> &'static str {
        match self {
            Choice::DisplayMode => "Display mode",
            Choice::Resolution => "Resolution",
            Choice::Quality => "Quality preset",
            Choice::Shadows => "Shadows",
            Choice::AntiAliasing => "Anti-aliasing",
            Choice::VisualClarity => "Visual clarity",
            Choice::FrameCap => "Frame rate cap",
            Choice::TextureSize => "Largest texture size",
            Choice::RightClickAllies => "Right-Click Allies",
            Choice::ReplayCount => "Replays kept, at most",
            Choice::ReplaySize => "Replays may take, at most",
        }
    }

    /// What may be picked, as shown.
    pub fn options(self, displays: &Displays) -> Vec<String> {
        match self {
            Choice::Resolution => displays
                .sizes()
                .iter()
                .map(|size| size_text(*size))
                .collect(),
            Choice::DisplayMode => ["Windowed", "Windowed fullscreen", "Fullscreen"]
                .iter()
                .take(crate::window::pickable_modes())
                .map(|name| name.to_string())
                .collect(),
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
            Choice::RightClickAllies => ["To Follow", "To Attack", "To Deny"]
                .map(String::from)
                .to_vec(),
            Choice::ReplayCount => REPLAY_COUNTS.iter().map(usize::to_string).collect(),
            Choice::ReplaySize => REPLAY_SIZES.iter().map(|mib| size_name(*mib)).collect(),
        }
    }

    /// The option picked now; none for settings no option stands for (a
    /// mix of presets).
    pub fn current(self, s: &Settings, displays: &Displays) -> Option<usize> {
        let q = &s.quality;
        match self {
            Choice::Resolution => displays
                .sizes()
                .iter()
                .position(|size| *size == displays.chosen(s)),
            Choice::DisplayMode => DISPLAY_MODES.iter().position(|(m, _)| *m == s.display),
            Choice::Quality => q
                .preset_name()
                .and_then(|n| PRESETS.iter().position(|p| *p == n)),
            Choice::Shadows => Some(usize::from(q.shadows.min(2))),
            Choice::AntiAliasing => AntiAlias::ALL.iter().position(|a| *a == q.anti_alias),
            Choice::VisualClarity => Clarity::ALL.iter().position(|c| *c == s.clarity),
            Choice::FrameCap => FRAME_CAPS.iter().position(|c| *c == q.fps_cap),
            Choice::TextureSize => TEXTURE_SIZES.iter().position(|t| *t == q.texture_size),
            Choice::RightClickAllies => RightClickAllies::ALL
                .iter()
                .position(|c| *c == s.right_click_allies),
            Choice::ReplayCount => REPLAY_COUNTS.iter().position(|n| *n == s.replays.max_count),
            Choice::ReplaySize => REPLAY_SIZES
                .iter()
                .position(|mib| mib << 20 == s.replays.max_bytes),
        }
    }

    /// Picks option `at`; one past the list picks nothing.
    pub fn pick(self, s: &mut Settings, at: usize, displays: &Displays) {
        let q = &mut s.quality;
        match self {
            Choice::Resolution => {
                if let Some(size) = displays.sizes().get(at) {
                    s.resolution = (*size != displays.native).then_some(*size);
                }
            }
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
            Choice::RightClickAllies => {
                s.right_click_allies = RightClickAllies::ALL
                    .get(at)
                    .copied()
                    .unwrap_or(s.right_click_allies);
            }
            Choice::ReplayCount => {
                s.replays.max_count = REPLAY_COUNTS
                    .get(at)
                    .copied()
                    .unwrap_or(s.replays.max_count);
            }
            Choice::ReplaySize => {
                s.replays.max_bytes = REPLAY_SIZES
                    .get(at)
                    .map_or(s.replays.max_bytes, |mib| mib << 20);
            }
        }
    }

    /// What the choice shows now.
    pub fn shown(self, s: &Settings, displays: &Displays) -> String {
        // A size the file names that the monitor does not list is still the size.
        if self == Choice::Resolution {
            return size_text(displays.chosen(s));
        }
        self.current(s, displays)
            .and_then(|at| self.options(displays).get(at).cloned())
            .unwrap_or_else(|| "Custom".into())
    }
}

/// A monitor mode as the list shows it.
fn size_text((width, height): (u32, u32)) -> String {
    format!("{width} × {height}")
}

/// A size in MiB as the list shows it.
fn size_name(mib: u64) -> String {
    if mib >= 1024 {
        format!("{} GB", mib / 1024)
    } else {
        format!("{mib} MB")
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
}

impl TextSetting {
    pub fn label(self) -> &'static str {
        match self {
            TextSetting::Name => "Name in local and direct games",
        }
    }

    /// What the field says while it is empty.
    pub fn placeholder(self) -> &'static str {
        match self {
            TextSetting::Name => crate::main_menu::DEFAULT_NAME,
        }
    }

    pub fn get(self, settings: &Settings) -> &String {
        match self {
            TextSetting::Name => &settings.name,
        }
    }

    pub fn get_mut(self, settings: &mut Settings) -> &mut String {
        match self {
            TextSetting::Name => &mut settings.name,
        }
    }

    /// Characters it holds at most.
    pub fn max(self) -> usize {
        match self {
            TextSetting::Name => crate::config::MAX_NAME,
        }
    }
}

/// One row of a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A line typed in.
    Text(TextSetting),
    Toggle(Toggle),
    Slider(Slider),
    Choice(Choice),
    /// A binding, by its place in [`keys::BINDINGS`].
    Key(usize),
    /// The line telling where the art and the sound come from.
    Census,
}

impl Row {
    /// What the row says and what a search looks in.
    pub fn label(self) -> &'static str {
        match self {
            Row::Text(setting) => setting.label(),
            Row::Toggle(toggle) => toggle.label(),
            Row::Slider(slider) => slider.label(),
            Row::Choice(choice) => choice.label(),
            Row::Key(at) => keys::BINDINGS[at].label,
            Row::Census => "",
        }
    }

    /// Whether the player can change it now: the replay caps wait on saving, the sound on the
    /// mute, a resolution on a window or exclusive mode (windowed fullscreen keeps the desktop's).
    pub fn enabled(self, s: &Settings) -> bool {
        match self {
            Row::Choice(Choice::ReplayCount | Choice::ReplaySize) => s.replays.keep,
            Row::Choice(Choice::Resolution) => s.display != DisplayMode::Borderless,
            Row::Slider(Slider::Volume) | Row::Toggle(Toggle::VoiceResponses) => !s.muted,
            _ => true,
        }
    }

    /// Whether it differs from its default.
    pub fn changed(self, s: &Settings, displays: &Displays) -> bool {
        let fresh = Settings::default();
        match self {
            Row::Text(setting) => setting.get(s) != setting.get(&fresh),
            Row::Toggle(toggle) => toggle.get(s) != toggle.get(&fresh),
            Row::Slider(slider) => (slider.get(s) - slider.get(&fresh)).abs() > 1e-4,
            Row::Choice(choice) => choice.current(s, displays) != choice.current(&fresh, displays),
            Row::Key(at) => s.keys[at] != fresh.keys[at],
            Row::Census => false,
        }
    }

    /// Puts it back to its default.
    pub fn reset(self, s: &mut Settings, displays: &Displays) {
        let fresh = Settings::default();
        match self {
            Row::Text(setting) => setting.get_mut(s).clone_from(setting.get(&fresh)),
            Row::Toggle(toggle) => toggle.set(s, toggle.get(&fresh)),
            Row::Slider(slider) => slider.set_share(s, slider.share(&fresh)),
            Row::Choice(choice) => {
                if let Some(at) = choice.current(&fresh, displays) {
                    choice.pick(s, at, displays);
                }
            }
            Row::Key(at) => s.keys[at] = fresh.keys[at],
            Row::Census => {}
        }
    }
}

/// A titled group of rows: a card of the screen and a link of the navigation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub title: &'static str,
    /// What the navigation calls it.
    pub link: &'static str,
    pub rows: Vec<Row>,
}

fn section(title: &'static str, link: &'static str, rows: Vec<Row>) -> Section {
    Section { title, link, rows }
}

/// Whether the HUD slots drawn at the top of the hotkeys stand for the binding (they are not
/// repeated as rows, but a search still finds them).
pub fn is_slot(action: Action) -> bool {
    matches!(
        action,
        Action::Ability(_) | Action::Ultimate | Action::Item(_) | Action::TownPortal
    )
}

/// The link a binding group has in the navigation.
fn group_link(group: Group) -> &'static str {
    match group {
        Group::Abilities => "Abilities and items",
        Group::Items => "Items",
        Group::Units => "Unit actions",
        Group::Shop => "Shop",
        Group::Camera => "Camera",
        Group::Interface => "Interface",
        Group::Watching => "Replays and spectating",
    }
}

/// The sections of a tab, as Dota's tabs group them. The hotkeys leave out the bindings the
/// slots show, unless `every_row`: a search looks in them all.
pub fn sections(tab: Tab, every_row: bool) -> Vec<Section> {
    use Row::{Choice as C, Slider as S, Toggle as T};
    match tab {
        Tab::Hotkeys => [
            Group::Abilities,
            Group::Items,
            Group::Units,
            Group::Shop,
            Group::Camera,
            Group::Interface,
            Group::Watching,
        ]
        .into_iter()
        .map(|group| {
            let rows: Vec<Row> = keys::all()
                .enumerate()
                .filter(|(_, b)| b.group == group && (every_row || !is_slot(b.action)))
                .map(|(at, _)| Row::Key(at))
                .collect();
            section(group.title(), group_link(group), rows)
        })
        .filter(|section| !section.rows.is_empty())
        .collect(),
        Tab::Options => vec![
            section("PROFILE", "Profile", vec![Row::Text(TextSetting::Name)]),
            section(
                "INTERFACE",
                "Interface",
                vec![
                    S(Slider::InterfaceScale),
                    T(Toggle::DamageNumbers),
                    T(Toggle::AnimatePortrait),
                    T(Toggle::ScoreboardItems),
                ],
            ),
            section(
                "CONTROLS",
                "Controls",
                vec![
                    C(Choice::RightClickAllies),
                    T(Toggle::TeleportRequiresHalt),
                    T(Toggle::ChannelsRequireStop),
                ],
            ),
            section("CAMERA", "Camera", vec![T(Toggle::EdgePan)]),
            section(
                "REPLAYS",
                "Replays",
                vec![
                    T(Toggle::SaveReplays),
                    C(Choice::ReplayCount),
                    C(Choice::ReplaySize),
                ],
            ),
            section("ASSETS", "Assets", vec![Row::Census]),
        ],
        Tab::Video => {
            // A browser's canvas follows the page: no monitor mode to pick.
            let mut display = vec![C(Choice::DisplayMode)];
            if !cfg!(target_arch = "wasm32") {
                display.push(C(Choice::Resolution));
            }
            display.extend([C(Choice::FrameCap), S(Slider::RenderScale)]);
            vec![
                section("DISPLAY", "Display", display),
                section(
                    "QUALITY",
                    "Quality",
                    vec![
                        C(Choice::Quality),
                        C(Choice::Shadows),
                        C(Choice::AntiAliasing),
                        C(Choice::VisualClarity),
                        T(Toggle::AutoQuality),
                    ],
                ),
                section(
                    "ON THE NEXT START",
                    "On the next start",
                    vec![
                        C(Choice::TextureSize),
                        T(Toggle::Props),
                        T(Toggle::GpuBatching),
                    ],
                ),
            ]
        }
        Tab::Audio => vec![section(
            "SOUND",
            "Sound",
            vec![
                S(Slider::Volume),
                T(Toggle::Muted),
                T(Toggle::VoiceResponses),
            ],
        )],
    }
}

/// Where in a label a search matches: the text before, the match, the text after.
pub fn split_match<'a>(label: &'a str, query: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    // ASCII lowercase keeps every byte where it was.
    let at = label
        .to_ascii_lowercase()
        .find(&query.to_ascii_lowercase())?;
    let end = at + query.len();
    label
        .is_char_boundary(end)
        .then(|| (&label[..at], &label[at..end], &label[end..]))
}

/// A tab's sections that hold a match of `query`, cut down to the rows that match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub tab: Tab,
    pub section: Section,
}

/// Every row of every tab whose label holds `query`, under its tab and section, in the order
/// the tabs and sections have.
pub fn search(query: &str) -> Vec<Found> {
    let mut found = Vec::new();
    for tab in Tab::ALL {
        for section in sections(tab, true) {
            let rows: Vec<Row> = section
                .rows
                .iter()
                .copied()
                .filter(|row| split_match(row.label(), query).is_some())
                .collect();
            if !rows.is_empty() {
                found.push(Found {
                    tab,
                    section: Section { rows, ..section },
                });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A search is not case sensitive, finds a word anywhere in a label, and tells where.
    #[test]
    fn a_match_is_found_anywhere_in_a_label_whatever_its_case() {
        assert_eq!(
            split_match("Save the replays of my matches", "REPLAY"),
            Some(("Save the ", "replay", "s of my matches"))
        );
        assert_eq!(split_match("Shadows", "  "), None, "blank asks for nothing");
        assert_eq!(split_match("Shadows", "high"), None);
    }

    /// The hotkeys' tab leaves out the bindings its panel of slots shows; a search finds them.
    #[test]
    fn the_hotkeys_list_leaves_out_the_slots_but_a_search_finds_them() {
        let listed: Vec<&str> = sections(Tab::Hotkeys, false)
            .iter()
            .flat_map(|s| s.rows.iter().map(|r| r.label()))
            .collect();
        for slot in ["Ability 1", "Ultimate", "Item 6", "Town portal scroll"] {
            assert!(!listed.contains(&slot), "{slot} is drawn as a slot");
        }
        assert!(listed.contains(&"Learn ability"));
        let found = search("item 6");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].section.title, "ITEMS");
    }

    /// Every setting of every tab is reachable by its label, and each row of a tab is once.
    #[test]
    fn every_binding_is_a_row_of_the_search_and_no_row_repeats() {
        let all: Vec<Row> = Tab::ALL
            .into_iter()
            .flat_map(|tab| sections(tab, true))
            .flat_map(|s| s.rows)
            .collect();
        for at in 0..keys::COUNT {
            assert!(all.contains(&Row::Key(at)), "binding {at}");
        }
        for (i, row) in all.iter().enumerate() {
            assert!(!all[..i].contains(row), "{row:?} twice");
        }
    }

    /// Changed rows are those that differ from the defaults, and a reset puts each back.
    #[test]
    fn a_reset_puts_every_changed_row_back_to_its_default() {
        let displays = Displays::default();
        let mut s = Settings {
            volume: 0.2,
            muted: true,
            name: "mira".to_owned(),
            display: DisplayMode::Windowed,
            resolution: Some((1280, 720)),
            ..Settings::default()
        };
        s.keys[0] = None;
        let rows: Vec<Row> = Tab::ALL
            .into_iter()
            .flat_map(|tab| sections(tab, true))
            .flat_map(|section| section.rows)
            .collect();
        let changed: Vec<Row> = rows
            .iter()
            .copied()
            .filter(|row| row.changed(&s, &displays))
            .collect();
        assert!(changed.contains(&Row::Slider(Slider::Volume)));
        assert!(changed.contains(&Row::Choice(Choice::Resolution)));
        assert!(changed.contains(&Row::Key(0)));
        assert!(!changed.contains(&Row::Toggle(Toggle::EdgePan)));
        for row in changed {
            row.reset(&mut s, &displays);
            assert!(!row.changed(&s, &displays), "{row:?} still changed");
        }
    }

    /// The resolution row waits for a window or exclusive fullscreen.
    #[test]
    fn the_resolution_is_enabled_everywhere_but_in_windowed_fullscreen() {
        let row = Row::Choice(Choice::Resolution);
        let with = |display| Settings {
            display,
            ..Settings::default()
        };
        assert!(!row.enabled(&with(DisplayMode::Borderless)));
        assert!(row.enabled(&with(DisplayMode::Windowed)));
        assert!(row.enabled(&with(DisplayMode::Exclusive)));
    }
}
