//! What the player may set: hotkeys, sound and which aids are drawn, read
//! from and written to a plain `name = value` file.

use std::path::{Path, PathBuf};

use bevy::input::keyboard::KeyCode;

/// Something a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Arms an attack-move for the next left click.
    AttackMove,
    /// Stops whatever the commanded unit does.
    Stop,
    /// Stands still and fights only what comes into reach.
    Hold,
    /// Casts one of the first five ability slots, `0..5`.
    Ability(u8),
    /// Casts the last ability slot of a unit with four or more: the ultimate.
    Ultimate,
    /// Uses one of the six inventory slots, `0..6`.
    Item(u8),
    /// Opens and closes the shop.
    Shop,
    /// Picks one's own hero; twice pins the camera to it.
    SelectHero,
    /// Picks one's own courier; twice pins the camera to it.
    SelectCourier,
    /// Sends the courier to bring what waits in the stash.
    CourierDeliver,
    /// Buys the next missing part of the quick-buy item.
    QuickBuy,
    /// Toggles the camera lock on one's own hero.
    CameraLock,
    /// Holds the camera over one's own hero while pressed.
    CenterHero,
    /// Opens and closes the settings overlay.
    Settings,
    /// Silences and restores all sound.
    Mute,
    /// While held, shows every range aid at full strength.
    ShowRanges,
}

/// Every action with its name in the file and its key out of the box.
pub const BINDINGS: [(Action, &str, KeyCode); 25] = [
    (Action::AttackMove, "attack_move", KeyCode::KeyA),
    (Action::Stop, "stop", KeyCode::KeyS),
    (Action::Hold, "hold", KeyCode::KeyH),
    (Action::Ability(0), "ability_1", KeyCode::KeyQ),
    (Action::Ability(1), "ability_2", KeyCode::KeyW),
    (Action::Ability(2), "ability_3", KeyCode::KeyE),
    (Action::Ability(3), "ability_4", KeyCode::KeyD),
    (Action::Ability(4), "ability_5", KeyCode::KeyF),
    (Action::Ultimate, "ultimate", KeyCode::KeyR),
    (Action::Item(0), "item_1", KeyCode::Digit1),
    (Action::Item(1), "item_2", KeyCode::Digit2),
    (Action::Item(2), "item_3", KeyCode::Digit3),
    (Action::Item(3), "item_4", KeyCode::Digit4),
    (Action::Item(4), "item_5", KeyCode::Digit5),
    (Action::Item(5), "item_6", KeyCode::Digit6),
    (Action::Shop, "shop", KeyCode::KeyB),
    (Action::SelectHero, "select_hero", KeyCode::F1),
    (Action::SelectCourier, "select_courier", KeyCode::F2),
    (Action::CourierDeliver, "courier_deliver", KeyCode::F3),
    (Action::QuickBuy, "quick_buy", KeyCode::F5),
    (Action::CameraLock, "camera_lock", KeyCode::KeyY),
    (Action::CenterHero, "center_hero", KeyCode::Space),
    (Action::Settings, "settings", KeyCode::F10),
    (Action::Mute, "mute", KeyCode::KeyM),
    (Action::ShowRanges, "show_ranges", KeyCode::AltLeft),
];

/// The aids drawn over the world, each of which may be turned off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aids {
    /// Damage, gold and deny numbers rising off units.
    pub numbers: bool,
    /// A dashed ring at one's own hero's attack range.
    pub attack_range: bool,
    /// The three spots one's own razes land on.
    pub raze_marks: bool,
    /// A mark on a creep's health bar where one's next hit kills it.
    pub last_hit_marks: bool,
    /// The reach of an enemy tower while one's hero is near it.
    pub tower_ranges: bool,
    /// Whether the cursor at the window edge pans the camera.
    pub edge_pan: bool,
}

/// Everything the settings file holds.
#[derive(Clone, Debug, PartialEq, bevy::prelude::Resource)]
pub struct Settings {
    /// Master volume, 0 to 1.
    pub volume: f32,
    /// Whether all sound is off.
    pub muted: bool,
    /// The interface size against the one chosen for the window, 0.5 to 2.
    pub ui_scale: f32,
    /// Which aids are drawn.
    pub aids: Aids,
    /// The key of every action, in [`BINDINGS`] order.
    pub keys: [KeyCode; BINDINGS.len()],
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            volume: 0.7,
            muted: false,
            ui_scale: 1.0,
            aids: Aids {
                numbers: true,
                attack_range: false,
                raze_marks: true,
                last_hit_marks: true,
                tower_ranges: true,
                edge_pan: true,
            },
            keys: BINDINGS.map(|(_, _, key)| key),
        }
    }
}

impl Settings {
    /// The key bound to an action.
    pub fn key(&self, action: Action) -> KeyCode {
        BINDINGS
            .iter()
            .position(|(bound, _, _)| *bound == action)
            .map_or(KeyCode::F35, |at| self.keys[at])
    }

    /// The key that casts ability slot `slot` of a unit holding `count`.
    ///
    /// The last slot of a unit with four or more is its ultimate; the rest
    /// take the ability keys in order.
    pub fn ability_key(&self, slot: u8, count: usize) -> KeyCode {
        if count >= 4 && usize::from(slot) + 1 == count {
            return self.key(Action::Ultimate);
        }
        self.key(Action::Ability(slot))
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

    /// Sets one named value.
    fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        if let Some(action) = name.strip_prefix("key.") {
            let at = BINDINGS
                .iter()
                .position(|(_, named, _)| *named == action)
                .ok_or_else(|| format!("no action called '{action}'"))?;
            self.keys[at] = key_named(value).ok_or_else(|| format!("no key called '{value}'"))?;
            return Ok(());
        }
        let flag = || parse_flag(value).ok_or_else(|| format!("'{value}' is not true or false"));
        match name {
            "volume" => self.volume = parse_unit(value, 0.0, 1.0)?,
            "ui_scale" => self.ui_scale = parse_unit(value, 0.5, 2.0)?,
            "muted" => self.muted = flag()?,
            "damage_numbers" => self.aids.numbers = flag()?,
            "attack_range" => self.aids.attack_range = flag()?,
            "raze_marks" => self.aids.raze_marks = flag()?,
            "last_hit_marks" => self.aids.last_hit_marks = flag()?,
            "tower_ranges" => self.aids.tower_ranges = flag()?,
            "edge_pan" => self.aids.edge_pan = flag()?,
            other => return Err(format!("no setting called '{other}'")),
        }
        Ok(())
    }

    /// The settings as the text of a file [`Settings::parse`] reads back.
    pub fn render(&self) -> String {
        let flag = |on: bool| if on { "true" } else { "false" };
        let mut out =
            String::from("# dbota settings. Keys use the names listed in the client README.\n");
        out.push_str(&format!("volume = {:.2}\n", self.volume));
        out.push_str(&format!("muted = {}\n", flag(self.muted)));
        out.push_str(&format!("ui_scale = {:.2}\n", self.ui_scale));
        for (name, on) in [
            ("damage_numbers", self.aids.numbers),
            ("attack_range", self.aids.attack_range),
            ("raze_marks", self.aids.raze_marks),
            ("last_hit_marks", self.aids.last_hit_marks),
            ("tower_ranges", self.aids.tower_ranges),
            ("edge_pan", self.aids.edge_pan),
        ] {
            out.push_str(&format!("{name} = {}\n", flag(on)));
        }
        for ((_, name, _), key) in BINDINGS.iter().zip(self.keys) {
            out.push_str(&format!("key.{name} = {}\n", key_name(key)));
        }
        out
    }
}

/// Lines read from a settings file at most.
const MAX_LINES: usize = 512;
/// Bytes read from a settings file at most.
const MAX_BYTES: u64 = 64 * 1024;

/// Where the settings live when no path is given:
/// `$XDG_CONFIG_HOME/dbota/client.conf`, else `~/.config/dbota/client.conf`.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("dbota").join("client.conf"))
}

/// Reads the settings at `path`. A missing file gives the defaults; a file
/// that cannot be read, or lines that cannot, are reported on stderr.
pub fn load(path: &Path) -> Settings {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(err) => {
            eprintln!("bota-client: settings {}: {err}", path.display());
            return Settings::default();
        }
    };
    let mut text = String::new();
    if let Err(err) = file.take(MAX_BYTES).read_to_string(&mut text) {
        eprintln!("bota-client: settings {}: {err}", path.display());
        return Settings::default();
    }
    let (settings, complaints) = Settings::parse(&text);
    for complaint in complaints {
        eprintln!("bota-client: settings {}: {complaint}", path.display());
    }
    settings
}

/// Writes the settings to `path`, making its folder if needed.
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, settings.render())
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

/// Every key a binding may name, by the name it is written with.
const KEY_NAMES: [(&str, KeyCode); 68] = [
    ("A", KeyCode::KeyA),
    ("B", KeyCode::KeyB),
    ("C", KeyCode::KeyC),
    ("D", KeyCode::KeyD),
    ("E", KeyCode::KeyE),
    ("F", KeyCode::KeyF),
    ("G", KeyCode::KeyG),
    ("H", KeyCode::KeyH),
    ("I", KeyCode::KeyI),
    ("J", KeyCode::KeyJ),
    ("K", KeyCode::KeyK),
    ("L", KeyCode::KeyL),
    ("M", KeyCode::KeyM),
    ("N", KeyCode::KeyN),
    ("O", KeyCode::KeyO),
    ("P", KeyCode::KeyP),
    ("Q", KeyCode::KeyQ),
    ("R", KeyCode::KeyR),
    ("S", KeyCode::KeyS),
    ("T", KeyCode::KeyT),
    ("U", KeyCode::KeyU),
    ("V", KeyCode::KeyV),
    ("W", KeyCode::KeyW),
    ("X", KeyCode::KeyX),
    ("Y", KeyCode::KeyY),
    ("Z", KeyCode::KeyZ),
    ("0", KeyCode::Digit0),
    ("1", KeyCode::Digit1),
    ("2", KeyCode::Digit2),
    ("3", KeyCode::Digit3),
    ("4", KeyCode::Digit4),
    ("5", KeyCode::Digit5),
    ("6", KeyCode::Digit6),
    ("7", KeyCode::Digit7),
    ("8", KeyCode::Digit8),
    ("9", KeyCode::Digit9),
    ("F1", KeyCode::F1),
    ("F2", KeyCode::F2),
    ("F3", KeyCode::F3),
    ("F4", KeyCode::F4),
    ("F5", KeyCode::F5),
    ("F6", KeyCode::F6),
    ("F7", KeyCode::F7),
    ("F8", KeyCode::F8),
    ("F9", KeyCode::F9),
    ("F10", KeyCode::F10),
    ("F11", KeyCode::F11),
    ("F12", KeyCode::F12),
    ("Space", KeyCode::Space),
    ("Tab", KeyCode::Tab),
    ("Backquote", KeyCode::Backquote),
    ("Minus", KeyCode::Minus),
    ("Equal", KeyCode::Equal),
    ("LeftBracket", KeyCode::BracketLeft),
    ("RightBracket", KeyCode::BracketRight),
    ("Semicolon", KeyCode::Semicolon),
    ("Apostrophe", KeyCode::Quote),
    ("Comma", KeyCode::Comma),
    ("Period", KeyCode::Period),
    ("Slash", KeyCode::Slash),
    ("Backslash", KeyCode::Backslash),
    ("LeftShift", KeyCode::ShiftLeft),
    ("RightShift", KeyCode::ShiftRight),
    ("LeftAlt", KeyCode::AltLeft),
    ("RightAlt", KeyCode::AltRight),
    ("CapsLock", KeyCode::CapsLock),
    ("Insert", KeyCode::Insert),
    ("Delete", KeyCode::Delete),
];

/// The key of a name, case aside.
pub fn key_named(name: &str) -> Option<KeyCode> {
    KEY_NAMES
        .iter()
        .find(|(named, _)| named.eq_ignore_ascii_case(name))
        .map(|(_, key)| *key)
}

/// The name a key is written and shown with.
pub fn key_name(key: KeyCode) -> &'static str {
    KEY_NAMES
        .iter()
        .find(|(_, named)| *named == key)
        .map_or("?", |(name, _)| name)
}
