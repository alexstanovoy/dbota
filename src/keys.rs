//! Every action a key or a mouse button may do, Dota's default binding of
//! each, and what the bindings fire this frame. One table, [`BINDINGS`],
//! drives the input, the hotkey letters on the HUD's slots and the settings
//! screen; the player may bind any action anew (`key.<name>` in the
//! settings file).
//!
//! The defaults are Dota 2's own, copied once from the install's
//! `cfg/dotakeys.lst` (in `pak01_dir.vpk`), the "ARROW" template (Dota's
//! default hotkeys, version 12), on 2026-10-24: each row names the Dota
//! command it stands for. The few actions Dota has no command for say so.

use bevy::prelude::*;

/// What a binding is pressed with: a key or a mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
}

/// A key or a mouse button, with the modifiers held with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub input: Input,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Chord {
    /// A key alone.
    pub const fn key(key: KeyCode) -> Chord {
        Chord {
            input: Input::Key(key),
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    /// The same chord with Shift held.
    pub const fn shift(self) -> Chord {
        Chord {
            shift: true,
            ..self
        }
    }

    /// How many modifiers it holds: of several chords a press matches, the
    /// one that names the most modifiers fires.
    fn modifiers(self) -> u8 {
        u8::from(self.shift) + u8::from(self.ctrl) + u8::from(self.alt)
    }

    /// The name settings files write and the settings screen shows:
    /// `Shift+Enter`, `F4`, `Mouse3`.
    pub fn name(self) -> String {
        let mut out = String::new();
        for (held, name) in [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
        ] {
            if held {
                out.push_str(name);
            }
        }
        out.push_str(self.input_name());
        out
    }

    /// The input's own name, without the modifiers: the letter a HUD slot
    /// shows.
    pub fn input_name(self) -> &'static str {
        match self.input {
            Input::Key(key) => key_name(key),
            Input::Mouse(button) => mouse_name(button),
        }
    }

    /// A chord by its [`name`](Chord::name), modifiers in any order, case
    /// aside.
    pub fn parse(text: &str) -> Option<Chord> {
        let mut chord = Chord::key(KeyCode::F35);
        let mut parts = text.split('+').map(str::trim).peekable();
        // A trailing `+` names the plus key itself; there is none bound.
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                chord.input = input_named(part)?;
                return Some(chord);
            }
            match part.to_ascii_lowercase().as_str() {
                "shift" => chord.shift = true,
                "ctrl" | "control" => chord.ctrl = true,
                "alt" => chord.alt = true,
                _ => return None,
            }
        }
        None
    }
}

/// Where a binding works: in play, while watching (a replay or as a
/// spectator), or both. Two actions may share a chord when they never work
/// at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Play,
    Watch,
    Both,
}

impl Context {
    fn meets(self, other: Context) -> bool {
        self == Context::Both || other == Context::Both || self == other
    }
}

/// The settings screen's sections of the bindings, in Dota's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Abilities,
    Items,
    Units,
    Shop,
    Camera,
    Interface,
    Watching,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Abilities => "ABILITIES",
            Group::Items => "ITEMS",
            Group::Units => "UNIT ACTIONS",
            Group::Shop => "SHOP",
            Group::Camera => "CAMERA",
            Group::Interface => "INTERFACE",
            Group::Watching => "REPLAYS AND SPECTATING",
        }
    }
}

/// Something a binding does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Casts one of the first five ability slots, `0..5`.
    Ability(u8),
    /// Casts the last ability slot of a unit with four or more.
    Ultimate,
    /// The next ability key learns a level instead of casting.
    LearnAbilities,
    /// Opens the talent tree, where the Attribute Bonus is learned too.
    LearnStats,
    /// Uses one of the six inventory slots, `0..6`.
    Item(u8),
    /// Uses the town portal slot.
    TownPortal,
    /// Arms an attack-move for the next left click.
    Attack,
    /// Arms a move for the next left click, whatever is there.
    Move,
    /// Held, the hero walks the straight line towards the cursor, round
    /// nothing (Dota's Directional Move); letting go stops it.
    MoveDirection,
    Stop,
    Hold,
    /// Picks one's own hero; twice centres the camera on it, held the camera
    /// follows it.
    SelectHero,
    SelectCourier,
    CourierDeliver,
    CourierBurst,
    CourierShield,
    /// Picks control group `0..10`; with Ctrl, puts the selection in it.
    ControlGroup(u8),
    /// Opens and closes the shop.
    Shop,
    /// Buys the next missing part of the quick-buy item.
    QuickBuy,
    /// Buys the item kept beside the quick-buy row.
    StickyBuy,
    CameraUp,
    CameraDown,
    CameraLeft,
    CameraRight,
    /// Held, the mouse drags the map under the camera.
    CameraGrip,
    /// Keeps the camera on one's own hero until pressed again.
    CameraLock,
    /// Activates the Glyph of Fortification.
    Glyph,
    /// Held, every range aid shows at full strength.
    ShowRanges,
    /// Held, the scoreboard shows.
    Scoreboard,
    /// Opens the chat line to one's allies.
    ChatTeam,
    /// Opens the chat line to everyone.
    ChatAll,
    /// Pings the cursor's spot or unit for one's side.
    Ping,
    /// The same, as a warning.
    PingDanger,
    /// Opens the console line.
    Console,
    /// Lets go of what is half done and closes what is open; in a game with
    /// nothing of either it opens the dashboard, over the menu it backs out
    /// to the top page and there asks to quit.
    Escape,
    /// Pauses and resumes a replay.
    Pause,
    /// Plays a replay faster, or slower.
    Faster,
    Slower,
    /// Steps a paused replay one tick.
    Step,
    /// Watches through the next eyes: everything, then each seat.
    NextEyes,
}

/// One row of the table: the action, its name in the settings file, its
/// label on the settings screen, its section, where it works and its
/// default (the Dota command it is copied from in the row's comment).
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub action: Action,
    pub name: &'static str,
    pub label: &'static str,
    pub group: Group,
    pub context: Context,
    pub default: Option<Chord>,
}

const fn row(
    action: Action,
    name: &'static str,
    label: &'static str,
    group: Group,
    context: Context,
    default: Option<Chord>,
) -> Binding {
    Binding {
        action,
        name,
        label,
        group,
        context,
        default,
    }
}

const fn key(code: KeyCode) -> Option<Chord> {
    Some(Chord::key(code))
}

use Context::{Both, Play, Watch};
use Group::{Abilities, Camera, Interface, Items, Shop as ShopGroup, Units, Watching};

/// Every action, Dota's default of each (the `dotakeys.lst` command in the
/// comment; `-` for dbota's own), in the settings screen's order.
#[rustfmt::skip]
pub const BINDINGS: [Binding; 58] = [
    row(Action::Ability(0), "ability_1", "Ability 1", Abilities, Play, key(KeyCode::KeyQ)), // AbilityPrimary1
    row(Action::Ability(1), "ability_2", "Ability 2", Abilities, Play, key(KeyCode::KeyW)), // AbilityPrimary2
    row(Action::Ability(2), "ability_3", "Ability 3", Abilities, Play, key(KeyCode::KeyE)), // AbilityPrimary3
    row(Action::Ability(3), "ability_4", "Ability 4", Abilities, Play, key(KeyCode::KeyD)), // AbilitySecondary1
    row(Action::Ability(4), "ability_5", "Ability 5", Abilities, Play, key(KeyCode::KeyF)), // AbilitySecondary2
    row(Action::Ultimate, "ultimate", "Ultimate", Abilities, Play, key(KeyCode::KeyR)), // AbilityUltimate
    row(Action::LearnAbilities, "learn_abilities", "Learn ability", Abilities, Play, key(KeyCode::KeyO)), // LearnAbilities
    row(Action::LearnStats, "learn_stats", "Talent tree", Abilities, Play, key(KeyCode::KeyU)), // LearnStats
    row(Action::Item(0), "item_1", "Item 1", Items, Play, key(KeyCode::KeyZ)), // Inventory1
    row(Action::Item(1), "item_2", "Item 2", Items, Play, key(KeyCode::KeyX)), // Inventory2
    row(Action::Item(2), "item_3", "Item 3", Items, Play, key(KeyCode::KeyC)), // Inventory3
    row(Action::Item(3), "item_4", "Item 4", Items, Play, key(KeyCode::KeyV)), // Inventory4
    row(Action::Item(4), "item_5", "Item 5", Items, Play, key(KeyCode::KeyB)), // Inventory5
    row(Action::Item(5), "item_6", "Item 6", Items, Play, key(KeyCode::KeyN)), // Inventory6
    row(Action::TownPortal, "town_portal", "Town portal scroll", Items, Play, key(KeyCode::KeyT)), // InventoryTP
    row(Action::Attack, "attack", "Attack", Units, Play, key(KeyCode::KeyA)), // HeroAttack
    row(Action::Move, "move", "Move", Units, Play, key(KeyCode::KeyM)), // HeroMove
    row(Action::MoveDirection, "move_direction", "Directional move (held)", Units, Play, None), // HeroMoveDirection (bound in none of the install's templates)
    row(Action::Stop, "stop", "Stop", Units, Play, key(KeyCode::KeyS)), // HeroStop
    row(Action::Hold, "hold", "Hold position", Units, Play, key(KeyCode::KeyH)), // HeroHold
    row(Action::SelectHero, "select_hero", "Select hero", Units, Play, key(KeyCode::F1)), // HeroSelect
    row(Action::SelectCourier, "select_courier", "Select courier", Units, Play, key(KeyCode::F2)), // CourierSelect
    row(Action::CourierDeliver, "courier_deliver", "Courier: deliver items", Units, Play, key(KeyCode::F3)), // CourierDeliver
    row(Action::CourierBurst, "courier_burst", "Courier: speed burst", Units, Play, None), // CourierBurst
    row(Action::CourierShield, "courier_shield", "Courier: shield", Units, Play, None), // CourierShield
    row(Action::ControlGroup(0), "control_group_1", "Control group 1", Units, Play, key(KeyCode::Digit1)), // ControlGroup1
    row(Action::ControlGroup(1), "control_group_2", "Control group 2", Units, Play, key(KeyCode::Digit2)), // ControlGroup2
    row(Action::ControlGroup(2), "control_group_3", "Control group 3", Units, Play, key(KeyCode::Digit3)), // ControlGroup3
    row(Action::ControlGroup(3), "control_group_4", "Control group 4", Units, Play, key(KeyCode::Digit4)), // ControlGroup4
    row(Action::ControlGroup(4), "control_group_5", "Control group 5", Units, Play, key(KeyCode::Digit5)), // ControlGroup5
    row(Action::ControlGroup(5), "control_group_6", "Control group 6", Units, Play, key(KeyCode::Digit6)), // ControlGroup6
    row(Action::ControlGroup(6), "control_group_7", "Control group 7", Units, Play, key(KeyCode::Digit7)), // ControlGroup7
    row(Action::ControlGroup(7), "control_group_8", "Control group 8", Units, Play, key(KeyCode::Digit8)), // ControlGroup8
    row(Action::ControlGroup(8), "control_group_9", "Control group 9", Units, Play, key(KeyCode::Digit9)), // ControlGroup9
    row(Action::ControlGroup(9), "control_group_10", "Control group 10", Units, Play, key(KeyCode::Digit0)), // ControlGroup10
    row(Action::Shop, "shop", "Open shop", ShopGroup, Play, key(KeyCode::F4)), // ShopToggle
    row(Action::QuickBuy, "quick_buy", "Purchase quick buy", ShopGroup, Play, key(KeyCode::F5)), // PurchaseQuickbuy
    row(Action::StickyBuy, "sticky_buy", "Purchase sticky item", ShopGroup, Play, key(KeyCode::F8)), // PurchaseSticky
    row(Action::CameraUp, "camera_up", "Camera up", Camera, Both, key(KeyCode::ArrowUp)), // CameraUp
    row(Action::CameraDown, "camera_down", "Camera down", Camera, Both, key(KeyCode::ArrowDown)), // CameraDown
    row(Action::CameraLeft, "camera_left", "Camera left", Camera, Both, key(KeyCode::ArrowLeft)), // CameraLeft
    row(Action::CameraRight, "camera_right", "Camera right", Camera, Both, key(KeyCode::ArrowRight)), // CameraRight
    row(Action::CameraGrip, "camera_grip", "Camera grip", Camera, Both, Some(Chord { input: Input::Mouse(MouseButton::Middle), shift: false, ctrl: false, alt: false })), // CameraGrip
    // Dota keeps the camera on the hero as an option, with no key; here it
    // is an action too, unbound.
    row(Action::CameraLock, "camera_lock", "Lock camera to hero", Camera, Play, None), // -
    row(Action::Glyph, "glyph", "Glyph of Fortification", Interface, Play, key(KeyCode::KeyJ)), // ActivateGlyph
    // Dota's Alt (the DotaAlt pseudo-modifier) shows what it hides.
    row(Action::ShowRanges, "show_ranges", "Show ranges (held)", Interface, Both, key(KeyCode::AltLeft)), // DotaAlt
    row(Action::Scoreboard, "scoreboard", "Scoreboard (held)", Interface, Both, key(KeyCode::Backquote)), // ScoreboardToggle (SpectatorScoreboard the same)
    row(Action::ChatTeam, "chat_team", "Chat to allies", Interface, Play, key(KeyCode::Enter)), // ChatTeam
    row(Action::ChatAll, "chat_all", "Chat to all", Interface, Play, Some(Chord::key(KeyCode::Enter).shift())), // ChatGlobal (ChatTeam with Shift)
    row(Action::Ping, "ping", "Ping", Interface, Play, Some(Chord { input: Input::Mouse(MouseButton::Left), shift: false, ctrl: false, alt: true })), // - (Dota pings on Alt+click)
    row(Action::PingDanger, "ping_danger", "Ping: danger", Interface, Play, Some(Chord { input: Input::Mouse(MouseButton::Left), shift: false, ctrl: true, alt: true })), // - (Dota warns on Ctrl+Alt+click)
    row(Action::Console, "console", "Console", Interface, Both, key(KeyCode::Backslash)), // Console
    row(Action::Escape, "escape", "Close / menu", Interface, Both, key(KeyCode::Escape)), // Escape
    row(Action::Pause, "pause", "Pause", Watching, Watch, key(KeyCode::F9)), // Pause
    // Step and the next eyes are dbota's own: Dota's spectators pick the fog
    // with F1 to F3.
    row(Action::Faster, "replay_faster", "Replay faster", Watching, Watch, key(KeyCode::Equal)), // SpectatorIncreaseReplaySpeed
    row(Action::Slower, "replay_slower", "Replay slower", Watching, Watch, key(KeyCode::Minus)), // SpectatorDecreaseReplaySpeed
    row(Action::Step, "replay_step", "Replay: one tick", Watching, Watch, key(KeyCode::Period)), // -
    row(Action::NextEyes, "next_eyes", "Watch the next eyes", Watching, Watch, key(KeyCode::Tab)), // -
];

/// Every binding, in the table's order.
pub fn all() -> impl Iterator<Item = &'static Binding> {
    BINDINGS.iter()
}

/// How many bindings there are.
pub const COUNT: usize = BINDINGS.len();

/// The place of an action in [`all`].
pub fn index_of(action: Action) -> Option<usize> {
    all().position(|b| b.action == action)
}

/// The binding named `name` in the settings file.
pub fn named(name: &str) -> Option<(usize, &'static Binding)> {
    all().enumerate().find(|(_, b)| b.name == name)
}

/// Every action bound to the same chord as the action at `at`, in a
/// context they share: the settings screen marks them.
pub fn conflicts(keys: &[Option<Chord>; COUNT], at: usize) -> Vec<Action> {
    let Some((mine, me)) = keys[at].zip(all().nth(at)) else {
        return Vec::new();
    };
    all()
        .enumerate()
        .filter(|(other, b)| {
            *other != at && keys[*other] == Some(mine) && b.context.meets(me.context)
        })
        .map(|(_, b)| b.action)
        .collect()
}

/// What the bindings do this frame: the actions whose chord was pressed
/// this frame, and those held.
#[derive(Resource, Default, Debug)]
pub struct Fired {
    pub pressed: Vec<Action>,
    pub held: Vec<Action>,
}

impl Fired {
    pub fn pressed(&self, action: Action) -> bool {
        self.pressed.contains(&action)
    }

    pub fn held(&self, action: Action) -> bool {
        self.held.contains(&action)
    }
}

/// How much of the keyboard is the bindings' this frame.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Muted {
    /// Every binding fires.
    #[default]
    No,
    /// The menu or the settings screen covers the game: only the
    /// interface's own keys fire (see [`over_menus`]).
    Game,
    /// A line or a box takes what is typed, or a key is being bound: none.
    All,
}

/// Whether an action works over the menu and the settings screen too, as
/// Dota's Escape backs out of them.
pub fn over_menus(action: Action) -> bool {
    action == Action::Escape
}

/// A text that shows an action's binding, kept up to date as it is bound
/// anew.
#[derive(Component, Clone, Copy, Debug)]
pub struct KeyLabel(pub Action);

pub fn plugin(app: &mut App) {
    app.init_resource::<Fired>()
        .init_resource::<Muted>()
        .add_systems(PreUpdate, deafen.before(bevy::input::InputSystems))
        .add_systems(PreUpdate, fire.after(bevy::input::InputSystems))
        .add_systems(Update, labels);
}

/// An unattended run (a capture, a visual check, a benchmark) takes no keys, buttons, wheel or
/// motion from the desktop it runs on: a key held there (an Alt that shows the ranges) changes
/// nothing it draws or checks.
fn deafen(
    args: Res<crate::Args>,
    mut keys: ResMut<Messages<bevy::input::keyboard::KeyboardInput>>,
    mut buttons: ResMut<Messages<bevy::input::mouse::MouseButtonInput>>,
    mut wheel: ResMut<Messages<bevy::input::mouse::MouseWheel>>,
    mut motion: ResMut<Messages<bevy::input::mouse::MouseMotion>>,
) {
    if args.unattended() {
        keys.clear();
        buttons.clear();
        wheel.clear();
        motion.clear();
    }
}

/// Every [`KeyLabel`]'s text, from the settings.
fn labels(settings: Res<crate::config::Settings>, mut texts: Query<(Ref<KeyLabel>, &mut Text)>) {
    for (label, mut text) in &mut texts {
        if settings.is_changed() || label.is_added() {
            text.set_if_neq(Text(settings.label(label.0)));
        }
    }
}

/// The modifiers held now.
fn held_modifiers(keys: &ButtonInput<KeyCode>) -> (bool, bool, bool) {
    (
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
    )
}

/// Whether `chord`'s modifiers are held: those it names must be, others may
/// be (Shift queues and Ctrl learns, whatever the key).
fn modifiers_held(chord: Chord, (shift, ctrl, alt): (bool, bool, bool)) -> bool {
    (!chord.shift || shift) && (!chord.ctrl || ctrl) && (!chord.alt || alt)
}

/// Works out what the bindings fire this frame from the keys and buttons.
pub fn fire(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    settings: Res<crate::config::Settings>,
    muted: Res<Muted>,
    mut fired: ResMut<Fired>,
) {
    fired.pressed.clear();
    fired.held.clear();
    if *muted == Muted::All {
        return;
    }
    let mods = held_modifiers(&keys);
    let down = |input: Input| match input {
        Input::Key(k) => keys.pressed(k),
        Input::Mouse(b) => mouse.pressed(b),
    };
    let just = |input: Input| match input {
        Input::Key(k) => keys.just_pressed(k),
        Input::Mouse(b) => mouse.just_pressed(b),
    };
    let bound = || {
        all()
            .zip(settings.keys)
            .filter_map(|(b, chord)| Some((b.action, chord?)))
            .filter(|(action, _)| *muted == Muted::No || over_menus(*action))
            .filter(|(_, chord)| modifiers_held(*chord, mods))
    };
    for (action, chord) in bound() {
        if down(chord.input) {
            fired.held.push(action);
        }
        // Of the chords a press matches, those naming the most modifiers
        // fire: Shift+Enter opens the chat to all, not to the allies.
        let most = bound()
            .filter(|(_, other)| other.input == chord.input)
            .map(|(_, other)| other.modifiers())
            .max()
            .unwrap_or(0);
        if just(chord.input) && chord.modifiers() == most {
            fired.pressed.push(action);
        }
    }
}

/// Every key a binding may name, by the name it is written with.
const KEY_NAMES: [(&str, KeyCode); 88] = [
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
    ("Enter", KeyCode::Enter),
    ("Escape", KeyCode::Escape),
    ("Backspace", KeyCode::Backspace),
    ("`", KeyCode::Backquote),
    ("-", KeyCode::Minus),
    ("=", KeyCode::Equal),
    ("[", KeyCode::BracketLeft),
    ("]", KeyCode::BracketRight),
    (";", KeyCode::Semicolon),
    ("'", KeyCode::Quote),
    (",", KeyCode::Comma),
    (".", KeyCode::Period),
    ("/", KeyCode::Slash),
    ("\\", KeyCode::Backslash),
    ("Up", KeyCode::ArrowUp),
    ("Down", KeyCode::ArrowDown),
    ("Left", KeyCode::ArrowLeft),
    ("Right", KeyCode::ArrowRight),
    ("LeftShift", KeyCode::ShiftLeft),
    ("RightShift", KeyCode::ShiftRight),
    ("LeftCtrl", KeyCode::ControlLeft),
    ("RightCtrl", KeyCode::ControlRight),
    ("LeftAlt", KeyCode::AltLeft),
    ("RightAlt", KeyCode::AltRight),
    ("CapsLock", KeyCode::CapsLock),
    ("Insert", KeyCode::Insert),
    ("Delete", KeyCode::Delete),
    ("Home", KeyCode::Home),
    ("End", KeyCode::End),
    ("PageUp", KeyCode::PageUp),
    ("PageDown", KeyCode::PageDown),
    ("Keypad0", KeyCode::Numpad0),
    ("Keypad1", KeyCode::Numpad1),
    ("Keypad2", KeyCode::Numpad2),
    ("Keypad3", KeyCode::Numpad3),
    ("KeypadEnter", KeyCode::NumpadEnter),
    ("KeypadPlus", KeyCode::NumpadAdd),
    ("KeypadMinus", KeyCode::NumpadSubtract),
];

/// The mouse buttons a binding may name: Dota's MOUSE3 is the middle one.
const MOUSE_NAMES: [(&str, MouseButton); 5] = [
    ("Mouse1", MouseButton::Left),
    ("Mouse2", MouseButton::Right),
    ("Mouse3", MouseButton::Middle),
    ("Mouse4", MouseButton::Back),
    ("Mouse5", MouseButton::Forward),
];

/// Whether a key may be bound at all: a modifier alone only as itself.
pub fn bindable(key: KeyCode) -> bool {
    KEY_NAMES.iter().any(|(_, k)| *k == key)
}

/// The name a key is written and shown with.
pub fn key_name(key: KeyCode) -> &'static str {
    KEY_NAMES
        .iter()
        .find(|(_, named)| *named == key)
        .map_or("?", |(name, _)| name)
}

fn mouse_name(button: MouseButton) -> &'static str {
    MOUSE_NAMES
        .iter()
        .find(|(_, named)| *named == button)
        .map_or("?", |(name, _)| name)
}

/// The key of a name, case aside.
pub fn key_named(name: &str) -> Option<KeyCode> {
    match input_named(name)? {
        Input::Key(key) => Some(key),
        Input::Mouse(_) => None,
    }
}

/// The key or button of a name, case aside.
fn input_named(name: &str) -> Option<Input> {
    let key = KEY_NAMES
        .iter()
        .find(|(named, _)| named.eq_ignore_ascii_case(name))
        .map(|(_, key)| Input::Key(*key));
    key.or_else(|| {
        MOUSE_NAMES
            .iter()
            .find(|(named, _)| named.eq_ignore_ascii_case(name))
            .map(|(_, button)| Input::Mouse(*button))
    })
}

/// Whether a mouse button may be bound: the left and right ones only with a
/// modifier, since a click alone is the game's own and is also what starts
/// binding.
pub fn bindable_mouse(button: MouseButton, modified: bool) -> bool {
    let plain = matches!(button, MouseButton::Left | MouseButton::Right);
    MOUSE_NAMES.iter().any(|(_, b)| *b == button) && (modified || !plain)
}

#[cfg(test)]
mod tests {
    use bevy::input::{ButtonState, InputPlugin, keyboard::KeyboardInput};

    use super::*;
    use crate::config::Settings;

    /// An app with Bevy's input and the bindings, as the client runs them.
    fn app(settings: Settings) -> App {
        app_run_as(settings, &["dbota"])
    }

    /// The same, run with command line `args`.
    fn app_run_as(settings: Settings, args: &[&str]) -> App {
        use clap::Parser;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin))
            .insert_resource(settings)
            .insert_resource(crate::Args::parse_from(args))
            .add_plugins(plugin);
        app.update();
        app
    }

    /// A capture or a visual check takes no key from the desktop it runs on: the owner's Alt
    /// held there shows no ranges in its frames.
    #[test]
    fn an_unattended_run_takes_no_key_from_the_desktop() {
        for (args, shown) in [
            (&["dbota"][..], true),
            (
                &["dbota", "--replay", "x.brp", "--screenshot-at", "10"][..],
                false,
            ),
        ] {
            let mut app = app_run_as(Settings::default(), args);
            key(&mut app, KeyCode::AltLeft, ButtonState::Pressed);
            assert_eq!(fired(&app).held(Action::ShowRanges), shown, "{args:?}");
        }
    }

    /// Presses (or lets go of) `key` as the window would report it, and
    /// runs a frame.
    fn key(app: &mut App, key: KeyCode, state: ButtonState) {
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: bevy::input::keyboard::Key::Unidentified(
                bevy::input::keyboard::NativeKey::Unidentified,
            ),
            state,
            text: None,
            repeat: false,
            window,
        });
        app.update();
    }

    /// Presses `button` as the window would report it, and runs a frame.
    fn click(app: &mut App, button: MouseButton) {
        let window = app.world_mut().spawn_empty().id();
        app.world_mut()
            .write_message(bevy::input::mouse::MouseButtonInput {
                button,
                state: ButtonState::Pressed,
                window,
            });
        app.update();
    }

    fn fired(app: &App) -> &Fired {
        app.world().resource::<Fired>()
    }

    /// A key bound anew drives its action and the old one no longer does;
    /// Shift with Enter opens the chat to all, not to the allies; an ability
    /// key with Ctrl held (to learn) still fires the ability.
    #[test]
    fn a_rebound_key_drives_its_action() {
        let mut settings = Settings::default();
        let shop = index_of(Action::Shop).expect("the shop is bound");
        settings.keys[shop] = Chord::parse("K");
        let mut app = app(settings);
        key(&mut app, KeyCode::F4, ButtonState::Pressed);
        assert!(
            !fired(&app).pressed(Action::Shop),
            "F4 is the shop's no more"
        );
        key(&mut app, KeyCode::F4, ButtonState::Released);
        key(&mut app, KeyCode::KeyK, ButtonState::Pressed);
        assert!(fired(&app).pressed(Action::Shop));
        key(&mut app, KeyCode::KeyK, ButtonState::Released);
        key(&mut app, KeyCode::ShiftLeft, ButtonState::Pressed);
        key(&mut app, KeyCode::Enter, ButtonState::Pressed);
        assert!(fired(&app).pressed(Action::ChatAll));
        assert!(!fired(&app).pressed(Action::ChatTeam));
        key(&mut app, KeyCode::Enter, ButtonState::Released);
        key(&mut app, KeyCode::ShiftLeft, ButtonState::Released);
        key(&mut app, KeyCode::ControlLeft, ButtonState::Pressed);
        key(&mut app, KeyCode::KeyQ, ButtonState::Pressed);
        assert!(fired(&app).pressed(Action::Ability(0)));
        *app.world_mut().resource_mut::<Muted>() = Muted::All;
        key(&mut app, KeyCode::KeyW, ButtonState::Pressed);
        assert!(fired(&app).pressed.is_empty(), "a line has the keyboard");
    }

    /// Over the menu only Escape fires, through its binding: bound anew, the
    /// new key backs out and Escape no longer does; with a line typed into,
    /// not even that.
    #[test]
    fn over_the_menu_only_escape_fires() {
        let mut settings = Settings::default();
        let escape = index_of(Action::Escape).expect("Escape is bound");
        settings.keys[escape] = Chord::parse("F10");
        let mut app = app(settings);
        *app.world_mut().resource_mut::<Muted>() = Muted::Game;
        key(&mut app, KeyCode::F4, ButtonState::Pressed);
        assert!(
            fired(&app).pressed.is_empty(),
            "the shop's key is the game's"
        );
        key(&mut app, KeyCode::Escape, ButtonState::Pressed);
        assert!(fired(&app).pressed.is_empty(), "Escape is bound elsewhere");
        key(&mut app, KeyCode::F10, ButtonState::Pressed);
        assert_eq!(fired(&app).pressed, [Action::Escape]);
        key(&mut app, KeyCode::F10, ButtonState::Released);
        *app.world_mut().resource_mut::<Muted>() = Muted::All;
        key(&mut app, KeyCode::F10, ButtonState::Pressed);
        assert!(fired(&app).pressed.is_empty(), "a field has the keyboard");
    }

    /// Alt+click pings, Ctrl+Alt+click warns, and neither fires the other.
    #[test]
    fn a_ping_click_is_told_from_a_danger_click() {
        let mut app = app(Settings::default());
        key(&mut app, KeyCode::AltLeft, ButtonState::Pressed);
        click(&mut app, MouseButton::Left);
        assert!(fired(&app).pressed(Action::Ping));
        assert!(!fired(&app).pressed(Action::PingDanger));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        key(&mut app, KeyCode::ControlLeft, ButtonState::Pressed);
        click(&mut app, MouseButton::Left);
        assert!(fired(&app).pressed(Action::PingDanger));
        assert!(!fired(&app).pressed(Action::Ping));
    }

    /// The defaults are the table's, Dota's hotkeys: the ability keys, the
    /// inventory on Z to N, the control groups on the digits, the shop on
    /// F4, the console on the backslash.
    #[test]
    fn the_defaults_are_dotas() {
        let settings = Settings::default();
        assert_eq!(settings.keys, BINDINGS.map(|b| b.default));
        let names: Vec<String> = BINDINGS
            .iter()
            .filter(|b| matches!(b.group, Group::Abilities | Group::Items))
            .map(|b| settings.label(b.action))
            .collect();
        assert_eq!(
            names,
            [
                "Q", "W", "E", "D", "F", "R", "O", "U", "Z", "X", "C", "V", "B", "N", "T"
            ]
        );
        for (action, chord) in [
            (Action::Shop, "F4"),
            (Action::ControlGroup(9), "0"),
            (Action::Console, "\\"),
            (Action::ChatAll, "Shift+Enter"),
            (Action::CameraGrip, "Mouse3"),
            (Action::Scoreboard, "`"),
            (Action::Ping, "Alt+Mouse1"),
            (Action::PingDanger, "Ctrl+Alt+Mouse1"),
        ] {
            assert_eq!(settings.label(action), chord, "{action:?}");
        }
        // The install's templates bind no key to the Directional Move.
        assert_eq!(
            settings.keys[index_of(Action::MoveDirection).unwrap()],
            None
        );
    }
}
