//! The screen's state and what the player does on it: tabs, toggles,
//! sliders dragged, lists opened and picked from, keys captured, Escape,
//! the wheel, and the save when it closes.

use bevy::{
    input::{
        ButtonState,
        keyboard::KeyboardInput,
        mouse::{AccumulatedMouseScroll, MouseButtonInput, MouseScrollUnit},
    },
    prelude::*,
    ui::RelativeCursorPosition,
};

use super::draw::{
    Body, ChoiceButton, ChoiceOption, CloseButton, KeyField, NameField, ResetButton, Root,
    SliderTrack, TabButton, ToggleRow,
};
use super::model::{Choice, Tab, TextSetting};
use crate::config::Settings;
use crate::keys::{self, Chord, Input};

/// The screen: whether it is open, its tab, the key field taking the next
/// key, and the list opened from a choice.
#[derive(Resource, Debug, PartialEq)]
pub struct Screen {
    pub open: bool,
    pub tab: Tab,
    /// The binding, by its place in [`keys::BINDINGS`], whose key is being
    /// taken.
    pub capturing: Option<usize>,
    pub dropdown: Option<Choice>,
    /// The line being typed into (the profile name).
    pub naming: Option<TextSetting>,
    /// A modifier key pressed while capturing and nothing else yet: bound
    /// alone when it is let go first.
    held_modifier: Option<KeyCode>,
}

impl Default for Screen {
    fn default() -> Screen {
        Screen {
            open: false,
            tab: Tab::Hotkeys,
            capturing: None,
            dropdown: None,
            naming: None,
            held_modifier: None,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Screen>()
        .add_systems(Startup, (super::draw::spawn_root, open_at_start))
        .add_systems(
            PreUpdate,
            escape
                .after(crate::keys::fire)
                .after(bevy::input::InputSystems),
        )
        .add_systems(
            Update,
            (
                capture,
                clicks,
                drag,
                scroll,
                save,
                super::draw::layout,
                super::draw::refresh,
            )
                .chain(),
        );
}

/// `--settings TAB` opens the screen at that tab.
fn open_at_start(args: Res<crate::Args>, mut screen: ResMut<Screen>) {
    let Some(name) = args.settings.as_deref() else {
        return;
    };
    match Tab::ALL
        .into_iter()
        .find(|t| t.title().eq_ignore_ascii_case(name))
    {
        Some(tab) => {
            screen.tab = tab;
            screen.open = true;
        }
        None => warn!("--settings {name}: no such tab"),
    }
}

/// Clicks on the tabs, the close and reset buttons, the toggles, the
/// choices and their lists, and the key fields.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn clicks(
    mut screen: ResMut<Screen>,
    mut settings: ResMut<Settings>,
    tabs: Query<(&Interaction, &TabButton), Changed<Interaction>>,
    close: Query<&Interaction, (Changed<Interaction>, With<CloseButton>)>,
    reset: Query<&Interaction, (Changed<Interaction>, With<ResetButton>)>,
    toggles: Query<(&Interaction, &ToggleRow), Changed<Interaction>>,
    choices: Query<(&Interaction, &ChoiceButton), Changed<Interaction>>,
    options: Query<(&Interaction, &ChoiceOption), Changed<Interaction>>,
    fields: Query<(&Interaction, &KeyField), Changed<Interaction>>,
    name: Query<(&Interaction, &NameField), Changed<Interaction>>,
) {
    if !screen.open {
        screen.bypass_change_detection().naming = None;
        return;
    }
    let pressed = |i: &Interaction| *i == Interaction::Pressed;
    if let Some((_, field)) = name.iter().find(|(i, _)| pressed(i)) {
        screen.naming = Some(field.0);
        screen.capturing = None;
    }
    if let Some((_, tab)) = tabs.iter().find(|(i, _)| pressed(i)) {
        screen.tab = tab.0;
        screen.dropdown = None;
        screen.capturing = None;
    }
    if close.iter().any(pressed) {
        screen.open = false;
    }
    if reset.iter().any(pressed) {
        settings.keys = keys::BINDINGS.map(|b| b.default);
    }
    for (_, row) in toggles.iter().filter(|(i, _)| pressed(i)) {
        let on = row.0.get(&settings);
        row.0.set(&mut settings, !on);
    }
    if let Some((_, option)) = options.iter().find(|(i, _)| pressed(i)) {
        option.0.pick(&mut settings, option.1);
        screen.dropdown = None;
    } else if let Some((_, choice)) = choices.iter().find(|(i, _)| pressed(i)) {
        screen.dropdown = (screen.dropdown != Some(choice.0)).then_some(choice.0);
    }
    if let Some((_, field)) = fields.iter().find(|(i, _)| pressed(i)) {
        screen.capturing = Some(field.0);
        screen.held_modifier = None;
    }
}

/// A slider held down follows the cursor along its track.
fn drag(
    screen: Res<Screen>,
    mut settings: ResMut<Settings>,
    tracks: Query<(&Interaction, &SliderTrack, &RelativeCursorPosition)>,
) {
    if !screen.open {
        return;
    }
    for (interaction, track, at) in &tracks {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(at) = at.normalized else {
            continue;
        };
        // The cursor's place is about the track's middle.
        let share = at.x + 0.5;
        let before = track.0.get(&settings);
        let mut changed = settings.clone();
        track.0.set_share(&mut changed, share);
        if track.0.get(&changed) != before {
            track.0.set_share(&mut settings, share);
        }
    }
}

/// Escape, through its binding, closes the open list, else the screen, and
/// goes no further: the menu or the game under the screen never sees it.
pub fn escape(mut screen: ResMut<Screen>, mut fired: ResMut<crate::keys::Fired>) {
    let escape = crate::keys::Action::Escape;
    if !screen.open || !fired.pressed(escape) {
        return;
    }
    fired.pressed.retain(|action| *action != escape);
    if screen.dropdown.is_some() {
        screen.dropdown = None;
    } else {
        screen.open = false;
    }
}

/// The key, button or chord pressed while a key field waits for one binds
/// its action: Escape lets it go unchanged, Backspace unbinds it, a
/// modifier pressed and let go alone binds that key.
fn capture(
    mut screen: ResMut<Screen>,
    mut settings: ResMut<Settings>,
    mut keyboard: MessageReader<KeyboardInput>,
    mut mouse: MessageReader<MouseButtonInput>,
    held: Res<ButtonInput<KeyCode>>,
    mut was_open: Local<bool>,
) {
    // What opened the screen (Escape in a game) is not for it.
    let opened_now = screen.open && !*was_open;
    *was_open = screen.open;
    if !screen.open || opened_now {
        keyboard.clear();
        mouse.clear();
        return;
    }
    if let Some(field) = screen.naming {
        mouse.clear();
        name(&mut screen, &mut settings, field, keyboard.read());
        return;
    }
    let Some(at) = screen.capturing else {
        keyboard.clear();
        mouse.clear();
        return;
    };
    let mods = |chord: Chord| Chord {
        shift: chord.shift || held.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        ctrl: chord.ctrl || held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        alt: chord.alt || held.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        ..chord
    };
    let mut bound: Option<Option<Chord>> = None;
    for event in keyboard.read() {
        match (event.state, event.key_code) {
            (ButtonState::Pressed, KeyCode::Escape) => {
                screen.capturing = None;
                return;
            }
            (ButtonState::Pressed, KeyCode::Backspace | KeyCode::Delete) => bound = Some(None),
            (ButtonState::Pressed, key) if is_modifier(key) => screen.held_modifier = Some(key),
            (ButtonState::Pressed, key) if keys::bindable(key) => {
                bound = Some(Some(mods(Chord::key(key))));
            }
            (ButtonState::Released, key) if screen.held_modifier == Some(key) => {
                bound = Some(Some(Chord::key(key)));
            }
            _ => {}
        }
    }
    for event in mouse.read() {
        let modified = held.any_pressed([
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ]);
        if event.state == ButtonState::Pressed && keys::bindable_mouse(event.button, modified) {
            bound = Some(Some(mods(Chord {
                input: Input::Mouse(event.button),
                shift: false,
                ctrl: false,
                alt: false,
            })));
        }
    }
    if let Some(chord) = bound {
        settings.keys[at] = chord;
        screen.capturing = None;
        screen.held_modifier = None;
    }
}

/// Types into a line (the profile name); Enter or Escape
/// is done with it.
fn name<'a>(
    screen: &mut Screen,
    settings: &mut Settings,
    field: TextSetting,
    keyboard: impl Iterator<Item = &'a KeyboardInput>,
) {
    use bevy::input::keyboard::Key;
    for event in keyboard.filter(|e| e.state == ButtonState::Pressed) {
        let line = field.get_mut(settings);
        match &event.logical_key {
            Key::Enter | Key::Escape | Key::Tab => screen.naming = None,
            Key::Backspace => {
                line.pop();
            }
            Key::Character(typed) => push_name(line, typed, field.max()),
            Key::Space => push_name(line, " ", field.max()),
            _ => {}
        }
    }
}

/// Printable characters, up to the line's length.
fn push_name(line: &mut String, typed: &str, max: usize) {
    for c in typed.chars().filter(|c| !c.is_control()) {
        if line.chars().count() >= max {
            break;
        }
        line.push(c);
    }
}

fn is_modifier(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
    )
}

/// The wheel scrolls the screen's body.
fn scroll(
    screen: Res<Screen>,
    wheel: Res<AccumulatedMouseScroll>,
    mut body: Query<&mut ScrollPosition, With<Body>>,
) {
    if !screen.open || wheel.delta.y == 0.0 {
        return;
    }
    let step = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * 40.0,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    for mut at in &mut body {
        at.y = (at.y - step).max(0.0);
    }
}

/// Saves the settings when the screen closes, if they changed while it was
/// open.
fn save(
    mut screen: ResMut<Screen>,
    settings: Res<Settings>,
    store: Res<crate::main_menu::SettingsStore>,
    mut opened_with: Local<Option<Settings>>,
    mut root: Query<&mut Visibility, With<Root>>,
) {
    match (&*opened_with, screen.open) {
        (None, true) => *opened_with = Some(settings.clone()),
        (Some(before), false) => {
            if *before != *settings
                && let Err(err) = store.save_settings(&settings)
            {
                warn!("settings not saved: {err}");
            }
            *opened_with = None;
            if screen.capturing.is_some() || screen.dropdown.is_some() {
                screen.capturing = None;
                screen.dropdown = None;
            }
        }
        _ => {}
    }
    for mut visibility in &mut root {
        visibility.set_if_neq(crate::ui::visible(screen.open));
    }
}
