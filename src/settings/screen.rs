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
    Body, CardAnchor, ChoiceButton, ChoiceOption, CloseButton, KeyField, NameField, NavLink,
    ResetButton, Root, RowKey, RowReset, ScrollRail, ScrollThumb, SearchClear, SearchField,
    SliderTrack, TabButton, ToggleBox, ToggleRow,
};
use super::model::{Choice, Row, Tab, TextSetting};
use crate::config::Settings;
use crate::keys::{self, Chord, Input};
use crate::theme;
use crate::window::Displays;

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
    /// What the search field holds; a search shows the rows whose labels hold it, from every tab.
    pub query: String,
    /// Whether the search field is being typed into.
    pub searching: bool,
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
            query: String::new(),
            searching: false,
            held_modifier: None,
        }
    }
}

impl Screen {
    /// Whether the screen shows a search's matches instead of a tab.
    pub fn is_searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Opens `tab`, leaving a search and whatever was half done.
    fn show(&mut self, tab: Tab) {
        self.tab = tab;
        self.query.clear();
        self.searching = false;
        self.dropdown = None;
        self.capturing = None;
    }
}

/// How far the body is scrolled, in interface pixels, and the card the navigation asked to scroll
/// to. Kept apart from the screen, whose every change builds the layout again.
#[derive(Resource, Default)]
pub struct Scroll {
    pub y: f32,
    pub jump: Option<usize>,
}

/// The control the keyboard stands on, by its place in the reading order of the window's controls;
/// none until Tab is pressed, and none again after a click.
#[derive(Resource, Default)]
pub struct Focus(pub Option<usize>);

/// The card in view, by its place in the open view: the navigation marks its link.
#[derive(Resource, Default)]
pub struct InView(pub usize);

/// The space over a card the body keeps when it scrolls to the card.
const CARD_MARGIN: f32 = 24.0;
/// The most characters a search holds.
const MAX_QUERY: usize = 40;

pub fn plugin(app: &mut App) {
    app.init_resource::<Screen>()
        .init_resource::<Scroll>()
        .init_resource::<InView>()
        .init_resource::<Focus>()
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
                focus,
                clicks,
                drag,
                scroll,
                save,
                super::draw::layout,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                super::look::rows,
                super::look::toggles,
                super::look::sliders,
                super::look::choices,
                super::look::key_fields,
                super::look::names,
                super::look::nav,
                super::look::help,
                super::screen::focus_ring,
            )
                .after(super::draw::layout),
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

/// The buttons the player pressed this frame, for [`clicks`].
#[derive(bevy::ecs::system::SystemParam)]
struct Pressed<'w, 's> {
    tabs: Query<'w, 's, (&'static Interaction, &'static TabButton), Changed<Interaction>>,
    links: Query<'w, 's, (&'static Interaction, &'static NavLink), Changed<Interaction>>,
    close: Query<'w, 's, &'static Interaction, (Changed<Interaction>, With<CloseButton>)>,
    reset_all: Query<'w, 's, &'static Interaction, (Changed<Interaction>, With<ResetButton>)>,
    resets: Query<'w, 's, (&'static Interaction, &'static RowReset), Changed<Interaction>>,
    toggles: Query<'w, 's, (&'static Interaction, &'static ToggleRow), Changed<Interaction>>,
    choices: Query<'w, 's, (&'static Interaction, &'static ChoiceButton), Changed<Interaction>>,
    options: Query<'w, 's, (&'static Interaction, &'static ChoiceOption), Changed<Interaction>>,
    fields: Query<'w, 's, (&'static Interaction, &'static KeyField), Changed<Interaction>>,
    name: Query<'w, 's, (&'static Interaction, &'static NameField), Changed<Interaction>>,
    search: Query<'w, 's, &'static Interaction, (Changed<Interaction>, With<SearchField>)>,
    clear: Query<'w, 's, &'static Interaction, (Changed<Interaction>, With<SearchClear>)>,
}

fn pressed(interaction: &Interaction) -> bool {
    *interaction == Interaction::Pressed
}

/// Clicks on the tabs and the navigation's links, the search field, the close and reset buttons,
/// the toggles, the choices and their lists, and the key fields and slots.
fn clicks(
    mut screen: ResMut<Screen>,
    mut scroll: ResMut<Scroll>,
    mut settings: ResMut<Settings>,
    displays: Res<Displays>,
    on: Pressed,
) {
    if !screen.open {
        screen.bypass_change_detection().naming = None;
        screen.bypass_change_detection().searching = false;
        return;
    }
    // The search field keeps the keyboard until the player clicks something else.
    let anything_else = on.name.iter().any(|(i, _)| pressed(i))
        || on.tabs.iter().any(|(i, _)| pressed(i))
        || on.choices.iter().any(|(i, _)| pressed(i))
        || on.toggles.iter().any(|(i, _)| pressed(i))
        || on.fields.iter().any(|(i, _)| pressed(i))
        || on.clear.iter().any(pressed);
    if anything_else && screen.searching {
        screen.searching = false;
    }
    if on.search.iter().any(pressed) && !screen.searching {
        let screen = &mut *screen;
        screen.searching = true;
        screen.naming = None;
        screen.capturing = None;
        screen.dropdown = None;
    }
    if on.clear.iter().any(pressed) {
        let screen = &mut *screen;
        screen.query.clear();
        screen.searching = true;
        scroll.y = 0.0;
    }
    if let Some((_, field)) = on.name.iter().find(|(i, _)| pressed(i)) {
        screen.naming = Some(field.0);
        screen.capturing = None;
    }
    if let Some((_, tab)) = on.tabs.iter().find(|(i, _)| pressed(i)) {
        screen.show(tab.0);
        scroll.y = 0.0;
    }
    if let Some((_, link)) = on.links.iter().find(|(i, _)| pressed(i)) {
        scroll.jump = Some(link.0);
    }
    if on.close.iter().any(pressed) {
        screen.open = false;
    }
    if on.reset_all.iter().any(pressed) {
        settings.keys = keys::BINDINGS.map(|b| b.default);
    }
    if let Some((_, reset)) = on.resets.iter().find(|(i, _)| pressed(i)) {
        reset.0.reset(&mut settings, &displays);
    }
    for (_, row) in on.toggles.iter().filter(|(i, _)| pressed(i)) {
        if Row::Toggle(row.0).enabled(&settings) {
            let state = row.0.get(&settings);
            row.0.set(&mut settings, !state);
        }
    }
    if let Some((_, option)) = on.options.iter().find(|(i, _)| pressed(i)) {
        option.0.pick(&mut settings, option.1, &displays);
        screen.dropdown = None;
    } else if let Some((_, choice)) = on.choices.iter().find(|(i, _)| pressed(i))
        && Row::Choice(choice.0).enabled(&settings)
    {
        screen.dropdown = (screen.dropdown != Some(choice.0)).then_some(choice.0);
    }
    if let Some((_, field)) = on.fields.iter().find(|(i, _)| pressed(i)) {
        screen.capturing = Some(field.0);
        screen.held_modifier = None;
        screen.naming = None;
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
    } else if screen.searching {
        screen.searching = false;
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
    mut composing: MessageReader<bevy::window::Ime>,
    held: Res<ButtonInput<KeyCode>>,
    mut was_open: Local<bool>,
) {
    // What opened the screen (Escape in a game) is not for it.
    let opened_now = screen.open && !*was_open;
    *was_open = screen.open;
    if !screen.open || opened_now {
        keyboard.clear();
        mouse.clear();
        composing.clear();
        return;
    }
    if screen.searching {
        mouse.clear();
        search(&mut screen, keyboard.read(), composing.read());
        return;
    }
    if let Some(field) = screen.naming {
        mouse.clear();
        name(
            &mut screen,
            &mut settings,
            field,
            keyboard.read(),
            composing.read(),
        );
        return;
    }
    let Some(at) = screen.capturing else {
        keyboard.clear();
        mouse.clear();
        composing.clear();
        return;
    };
    composing.clear();
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

/// Types into the search field: Enter or Escape is done with it. Typing changes only what is
/// asked, so the screen is marked changed only then.
fn search<'a>(
    screen: &mut Screen,
    keyboard: impl Iterator<Item = &'a KeyboardInput>,
    composing: impl Iterator<Item = &'a bevy::window::Ime>,
) {
    use bevy::input::keyboard::Key;
    let mut query = screen.query.clone();
    let mut done = false;
    for event in keyboard.filter(|e| e.state == ButtonState::Pressed) {
        match &event.logical_key {
            Key::Enter | Key::Escape | Key::Tab => done = true,
            Key::Backspace => {
                query.pop();
            }
            Key::Character(typed) => push_name(&mut query, typed, MAX_QUERY),
            Key::Space => push_name(&mut query, " ", MAX_QUERY),
            _ => {}
        }
    }
    for event in composing {
        if let bevy::window::Ime::Commit { value, .. } = event {
            push_name(&mut query, value, MAX_QUERY);
        }
    }
    if query != screen.query {
        screen.query = query;
    }
    if done {
        screen.searching = false;
    }
}

/// Types into a line (the profile name); Enter or Escape
/// is done with it.
fn name<'a>(
    screen: &mut Screen,
    settings: &mut Settings,
    field: TextSetting,
    keyboard: impl Iterator<Item = &'a KeyboardInput>,
    composing: impl Iterator<Item = &'a bevy::window::Ime>,
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
    for event in composing {
        if let bevy::window::Ime::Commit { value, .. } = event {
            push_name(field.get_mut(settings), value, field.max());
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

/// The wheel scrolls the screen's body, the navigation's links scroll it to a card, the card in
/// view is the one whose link is marked, and the scroll bar follows.
#[allow(clippy::type_complexity)]
fn scroll(
    screen: Res<Screen>,
    wheel: Res<AccumulatedMouseScroll>,
    mut scrolled: ResMut<Scroll>,
    mut in_view: ResMut<InView>,
    mut body: Query<(&mut ScrollPosition, &ComputedNode, &UiGlobalTransform), With<Body>>,
    anchors: Query<(&CardAnchor, &ComputedNode, &UiGlobalTransform)>,
    mut rail: Query<&mut Node, (With<ScrollRail>, Without<ScrollThumb>)>,
    mut thumb: Query<&mut Node, (With<ScrollThumb>, Without<ScrollRail>)>,
) {
    if !screen.open {
        return;
    }
    let Ok((mut at, node, place)) = body.single_mut() else {
        return;
    };
    let scale = node.inverse_scale_factor();
    let (view, total) = (node.size().y * scale, node.content_size().y * scale);
    // Before the first layout the body has no size: keep what was asked.
    let laid_out = view > 0.0;
    let reach = if laid_out {
        (total - view).max(0.0)
    } else {
        f32::MAX
    };
    let top = place.translation.y - node.size().y / 2.0;
    // The cards stand where the layout put them, which shows the scroll applied so far.
    let applied = at.y;
    let place_of = move |anchor: &ComputedNode, at_anchor: &UiGlobalTransform| {
        let anchor_top = at_anchor.translation.y - anchor.size().y / 2.0;
        (anchor_top - top) * scale + applied
    };
    if wheel.delta.y != 0.0 {
        let step = match wheel.unit {
            MouseScrollUnit::Line => wheel.delta.y * 40.0,
            MouseScrollUnit::Pixel => wheel.delta.y,
        };
        scrolled.y -= step;
    }
    if let Some(card) = scrolled.jump.take() {
        let wanted = anchors
            .iter()
            .find(|(anchor, ..)| anchor.0 == card)
            .map(|(_, anchor, at_anchor)| place_of(anchor, at_anchor) - CARD_MARGIN);
        // The first section of a tab is the top of it, the slots above it included.
        scrolled.y = if card == 0 && !screen.is_searching() {
            0.0
        } else {
            wanted.unwrap_or(scrolled.y)
        };
    }
    scrolled.y = scrolled.y.clamp(0.0, reach);
    if at.y != scrolled.y {
        at.y = scrolled.y;
    }
    if !laid_out {
        return;
    }
    let seen = if scrolled.y >= reach - 1.0 && reach > 0.0 {
        anchors
            .iter()
            .map(|(anchor, ..)| anchor.0)
            .max()
            .unwrap_or(0)
    } else {
        anchors
            .iter()
            .filter(|(_, anchor, at_anchor)| {
                place_of(anchor, at_anchor) - CARD_MARGIN <= scrolled.y + 1.0
            })
            .map(|(anchor, ..)| anchor.0)
            .max()
            .unwrap_or(0)
    };
    if in_view.0 != seen {
        in_view.0 = seen;
    }
    let scrolls = reach > 0.0;
    for mut node in &mut rail {
        let display = if scrolls {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut node in &mut thumb {
        let (top, height) = (
            percent(scrolled.y / total * 100.0),
            percent(view / total * 100.0),
        );
        if node.top != top || node.height != height {
            node.top = top;
            node.height = height;
        }
    }
}

/// Saves the settings when the screen closes, if they changed while it was
/// open.
fn save(
    mut screen: ResMut<Screen>,
    settings: Res<Settings>,
    store: Res<crate::main_menu::SettingsStore>,
    mut opened_with: Local<Option<Settings>>,
    mut scrolled: ResMut<Scroll>,
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
            if screen.capturing.is_some()
                || screen.dropdown.is_some()
                || !screen.query.is_empty()
                || screen.searching
            {
                screen.capturing = None;
                screen.dropdown = None;
                screen.query.clear();
                screen.searching = false;
            }
            scrolled.y = 0.0;
        }
        _ => {}
    }
    for mut visibility in &mut root {
        visibility.set_if_neq(crate::ui::visible(screen.open));
    }
}

/// The controls the keyboard can stand on, in reading order (top to bottom, then left to right).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Focusable<'w, 's> {
    nodes: Query<
        'w,
        's,
        (
            Entity,
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static InheritedVisibility,
        ),
        Or<(
            With<ToggleBox>,
            With<ChoiceButton>,
            With<RowKey>,
            With<NameField>,
            With<SliderTrack>,
        )>,
    >,
}

impl Focusable<'_, '_> {
    /// The controls that show, in reading order.
    pub fn in_order(&self) -> Vec<Entity> {
        let mut found: Vec<(Entity, Vec2)> = self
            .nodes
            .iter()
            .filter(|(_, node, _, seen)| seen.get() && node.size() != Vec2::ZERO)
            .map(|(entity, _, at, _)| (entity, at.translation))
            .collect();
        found.sort_by(|a, b| {
            // A row's controls share a line to within a pixel or two.
            let (ay, by) = ((a.1.y / 8.0).round(), (b.1.y / 8.0).round());
            ay.total_cmp(&by).then(a.1.x.total_cmp(&b.1.x))
        });
        found.into_iter().map(|(entity, _)| entity).collect()
    }
}

/// Tab and Shift+Tab move the keyboard along the controls; Space or Enter does what a click
/// does; Left and Right move a slider. A click gives the focus up.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn focus(
    mut screen: ResMut<Screen>,
    mut settings: ResMut<Settings>,
    mut at: ResMut<Focus>,
    mut keyboard: MessageReader<KeyboardInput>,
    held: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    order: Focusable,
    toggles: Query<&ToggleBox>,
    choices: Query<&ChoiceButton>,
    keyfields: Query<&KeyField, With<RowKey>>,
    names: Query<&NameField>,
    sliders: Query<&SliderTrack>,
) {
    use bevy::input::keyboard::Key;
    if !screen.open || screen.capturing.is_some() || screen.naming.is_some() || screen.searching {
        keyboard.clear();
        at.0 = None;
        return;
    }
    if mouse.just_pressed(MouseButton::Left) && at.0.is_some() {
        at.0 = None;
    }
    let controls = order.in_order();
    let shift = held.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for event in keyboard.read().filter(|e| e.state == ButtonState::Pressed) {
        if controls.is_empty() {
            return;
        }
        let last = controls.len() - 1;
        let here = at.0.map(|i| i.min(last));
        match &event.logical_key {
            Key::Tab if shift => {
                at.0 = Some(here.map_or(last, |i| i.checked_sub(1).unwrap_or(last)))
            }
            Key::Tab => at.0 = Some(here.map_or(0, |i| if i == last { 0 } else { i + 1 })),
            Key::Enter | Key::Space => {
                let Some(control) = here.map(|i| controls[i]) else {
                    continue;
                };
                if let Ok(tick) = toggles.get(control)
                    && Row::Toggle(tick.0).enabled(&settings)
                {
                    let state = tick.0.get(&settings);
                    tick.0.set(&mut settings, !state);
                } else if let Ok(choice) = choices.get(control)
                    && Row::Choice(choice.0).enabled(&settings)
                {
                    screen.dropdown = (screen.dropdown != Some(choice.0)).then_some(choice.0);
                } else if let Ok(field) = keyfields.get(control) {
                    screen.capturing = Some(field.0);
                    screen.held_modifier = None;
                } else if let Ok(field) = names.get(control) {
                    screen.naming = Some(field.0);
                }
            }
            Key::ArrowLeft | Key::ArrowRight => {
                let Some(control) = here.map(|i| controls[i]) else {
                    continue;
                };
                if let Ok(track) = sliders.get(control)
                    && Row::Slider(track.0).enabled(&settings)
                {
                    let (lo, hi, step) = track.0.range();
                    let sign = if event.logical_key == Key::ArrowLeft {
                        -1.0
                    } else {
                        1.0
                    };
                    let share = track.0.share(&settings) + sign * step / (hi - lo);
                    track.0.set_share(&mut settings, share);
                }
            }
            _ => {}
        }
    }
}

/// The ring round the control the keyboard stands on: 2 px of the focus colour.
pub fn focus_ring(
    screen: Res<Screen>,
    at: Res<Focus>,
    order: Focusable,
    mut rings: Query<(Entity, &mut Outline)>,
) {
    if !screen.open {
        return;
    }
    let focused = at.0.and_then(|i| order.in_order().get(i).copied());
    for (entity, mut outline) in &mut rings {
        let color = if Some(entity) == focused {
            theme::FOCUS
        } else {
            Color::NONE
        };
        if outline.color != color {
            outline.color = color;
        }
    }
}
