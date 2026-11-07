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

    /// Whether a text field (the search, the profile name) has the keyboard.
    pub fn is_typing(&self) -> bool {
        self.searching || self.naming.is_some()
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
pub struct Focus {
    pub control: Option<usize>,
    /// The option of an open list the keyboard stands on.
    pub option: Option<usize>,
}

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
        .add_systems(
            PreUpdate,
            capture
                .after(bevy::input::InputSystems)
                .before(bevy::picking::PickingSystems::Hover),
        )
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
                focus,
                ime_position,
                clicks,
                drag,
                scroll,
                scroll_marks,
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
                super::look::slider_fills,
                super::look::slider_thumbs,
                super::look::slider_values,
                super::look::choices,
                super::look::key_fields,
                super::look::key_texts,
                super::look::clash_bar,
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

/// Controls addressed by the native owning-widget press, consumed by the batched helpers.
#[derive(bevy::ecs::system::SystemParam)]
struct ClickTargets<'w, 's> {
    tabs: Query<'w, 's, &'static TabButton>,
    links: Query<'w, 's, &'static NavLink>,
    close: Query<'w, 's, (), With<CloseButton>>,
    reset_all: Query<'w, 's, (), With<ResetButton>>,
    resets: Query<'w, 's, &'static RowReset>,
    toggles: Query<'w, 's, &'static ToggleRow>,
    choices: Query<'w, 's, &'static ChoiceButton>,
    options: Query<'w, 's, &'static ChoiceOption>,
    fields: Query<'w, 's, &'static KeyField>,
    name: Query<'w, 's, &'static NameField>,
    search: Query<'w, 's, (), With<SearchField>>,
    clear: Query<'w, 's, (), With<SearchClear>>,
}

fn clicks(
    mut screen: ResMut<Screen>,
    mut scroll: ResMut<Scroll>,
    mut settings: ResMut<Settings>,
    displays: Res<Displays>,
    on: ClickTargets,
    press: Res<crate::ui::PrimaryPress>,
) {
    if !screen.open {
        screen.bypass_change_detection().naming = None;
        screen.bypass_change_detection().searching = false;
        return;
    }
    let Some(target) = press.0 else {
        return;
    };
    click_search(&mut screen, &mut scroll, &on, target);
    click_navigation(&mut screen, &mut scroll, &on, target);
    click_resets(&mut settings, &displays, &on, target);
    click_controls(&mut screen, &mut settings, &displays, &on, target);
}

fn click_search(
    screen: &mut ResMut<Screen>,
    scroll: &mut ResMut<Scroll>,
    on: &ClickTargets,
    target: Entity,
) {
    let anything_else = on.name.contains(target)
        || on.tabs.contains(target)
        || on.choices.contains(target)
        || on.toggles.contains(target)
        || on.fields.contains(target)
        || on.clear.contains(target);
    if anything_else && screen.searching {
        screen.searching = false;
    }
    if on.search.contains(target) && !screen.searching {
        screen.searching = true;
        screen.naming = None;
        screen.capturing = None;
        screen.dropdown = None;
    }
    if on.clear.contains(target) {
        screen.query.clear();
        screen.searching = true;
        scroll.y = 0.0;
    }
}

fn click_navigation(
    screen: &mut ResMut<Screen>,
    scroll: &mut ResMut<Scroll>,
    on: &ClickTargets,
    target: Entity,
) {
    if let Ok(tab) = on.tabs.get(target) {
        screen.show(tab.0);
        scroll.y = 0.0;
    }
    if let Ok(link) = on.links.get(target) {
        scroll.jump = Some(link.0);
    }
    if on.close.contains(target) {
        screen.open = false;
    }
}

fn click_resets(
    settings: &mut ResMut<Settings>,
    displays: &Displays,
    on: &ClickTargets,
    target: Entity,
) {
    if on.reset_all.contains(target) {
        settings.keys = keys::BINDINGS.map(|binding| binding.default);
    }
    if let Ok(reset) = on.resets.get(target) {
        reset.0.reset(settings, displays);
    }
}

fn click_controls(
    screen: &mut ResMut<Screen>,
    settings: &mut ResMut<Settings>,
    displays: &Displays,
    on: &ClickTargets,
    target: Entity,
) {
    if let Ok(field) = on.name.get(target) {
        screen.naming = Some(field.0);
        screen.capturing = None;
    }
    if let Ok(row) = on.toggles.get(target)
        && Row::Toggle(row.0).enabled(settings)
    {
        let state = row.0.get(settings);
        row.0.set(settings, !state);
    }
    if let Ok(option) = on.options.get(target) {
        option.0.pick(settings, option.1, displays);
        screen.dropdown = None;
    } else if let Ok(choice) = on.choices.get(target)
        && Row::Choice(choice.0).enabled(settings)
    {
        screen.dropdown = (screen.dropdown != Some(choice.0)).then_some(choice.0);
    }
    if let Ok(field) = on.fields.get(target) {
        screen.capturing = Some(field.0);
        screen.held_modifier = None;
        screen.naming = None;
    }
}

/// A slider held down follows the cursor along its track.
fn drag(
    screen: Res<Screen>,
    mut settings: ResMut<Settings>,
    tracks: Query<(
        Has<bevy::ui::Pressed>,
        &SliderTrack,
        &RelativeCursorPosition,
    )>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    if !screen.open {
        return;
    }
    for (interaction, track, at) in &tracks {
        if !interaction || !mouse.pressed(MouseButton::Left) {
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
    let bound = next_chord(&mut screen, keyboard.read(), mouse.read(), &held);
    if let Some(chord) = bound {
        settings.keys[at] = chord;
        screen.capturing = None;
        screen.held_modifier = None;
    }
}

/// The chord the events bind, if they do: `Some(None)` unbinds. Escape lets go of the capture
/// unchanged.
fn next_chord<'a>(
    screen: &mut ResMut<Screen>,
    keyboard: impl Iterator<Item = &'a KeyboardInput>,
    mouse: impl Iterator<Item = &'a MouseButtonInput>,
    held: &ButtonInput<KeyCode>,
) -> Option<Option<Chord>> {
    let mods = |chord: Chord| Chord {
        shift: chord.shift || held.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        ctrl: chord.ctrl || held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        alt: chord.alt || held.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        ..chord
    };
    let mut bound: Option<Option<Chord>> = None;
    for event in keyboard {
        match (event.state, event.key_code) {
            (ButtonState::Pressed, KeyCode::Escape) => {
                screen.capturing = None;
                return None;
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
    let modified = held.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ]);
    for event in mouse {
        if event.state == ButtonState::Pressed && keys::bindable_mouse(event.button, modified) {
            bound = Some(Some(mods(Chord {
                input: Input::Mouse(event.button),
                shift: false,
                ctrl: false,
                alt: false,
            })));
        }
    }
    bound
}

/// Types into the search field: Enter or Escape is done with it. Typing changes only what is
/// asked, so the screen is marked changed only then.
fn search<'a>(
    screen: &mut ResMut<Screen>,
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
    screen: &mut ResMut<Screen>,
    settings: &mut ResMut<Settings>,
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

/// Where the body and its cards stand, as the last layout put them, in interface pixels.
struct Geometry {
    scale: f32,
    /// How tall the body shows, and how tall its content is.
    view: f32,
    total: f32,
    /// The farthest the body scrolls; unbounded before the first layout.
    reach: f32,
    /// The body's top edge and the scroll that layout shows, in the layout's pixels.
    top: f32,
    applied: f32,
}

impl Geometry {
    fn of(node: &ComputedNode, place: &UiGlobalTransform, applied: f32) -> Geometry {
        let scale = node.inverse_scale_factor();
        let (view, total) = (node.size().y * scale, node.content_size().y * scale);
        Geometry {
            scale,
            view,
            total,
            reach: if view > 0.0 {
                (total - view).max(0.0)
            } else {
                f32::MAX
            },
            top: place.translation.y - node.size().y / 2.0,
            applied,
        }
    }

    /// Whether the body has been laid out; before, it has no size.
    fn laid_out(&self) -> bool {
        self.view > 0.0
    }

    /// Where a card's top stands in the content: what to scroll to to put it at the top.
    fn place_of(&self, card: &ComputedNode, at: &UiGlobalTransform) -> f32 {
        let card_top = at.translation.y - card.size().y / 2.0;
        (card_top - self.top) * self.scale + self.applied - CARD_MARGIN
    }
}

/// The wheel scrolls the screen's body, and the navigation's links scroll it to a card.
#[allow(clippy::type_complexity)]
fn scroll(
    screen: Res<Screen>,
    wheel: Res<AccumulatedMouseScroll>,
    mut scrolled: ResMut<Scroll>,
    mut body: Query<(&mut ScrollPosition, &ComputedNode, &UiGlobalTransform), With<Body>>,
    anchors: Query<(&CardAnchor, &ComputedNode, &UiGlobalTransform)>,
) {
    if !screen.open {
        return;
    }
    let Ok((mut at, node, place)) = body.single_mut() else {
        return;
    };
    let geometry = Geometry::of(node, place, at.y);
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
            .map(|(_, anchor, at_anchor)| geometry.place_of(anchor, at_anchor));
        // The first section of a tab is the top of it, the slots above it included.
        scrolled.y = if card == 0 && !screen.is_searching() {
            0.0
        } else {
            wanted.unwrap_or(scrolled.y)
        };
    }
    scrolled.y = scrolled.y.clamp(0.0, geometry.reach);
    if at.y != scrolled.y {
        at.y = scrolled.y;
    }
}

/// The card in view, whose link the navigation marks, and the scroll bar: shown while the body
/// scrolls, its thumb as long and as far along as the body shows and has scrolled.
#[allow(clippy::type_complexity)]
fn scroll_marks(
    screen: Res<Screen>,
    scrolled: Res<Scroll>,
    mut in_view: ResMut<InView>,
    body: Query<(&ScrollPosition, &ComputedNode, &UiGlobalTransform), With<Body>>,
    anchors: Query<(&CardAnchor, &ComputedNode, &UiGlobalTransform)>,
    mut rail: Query<&mut Node, (With<ScrollRail>, Without<ScrollThumb>)>,
    mut thumb: Query<&mut Node, (With<ScrollThumb>, Without<ScrollRail>)>,
) {
    if !screen.open {
        return;
    }
    let Ok((at, node, place)) = body.single() else {
        return;
    };
    let geometry = Geometry::of(node, place, at.y);
    if !geometry.laid_out() {
        return;
    }
    let seen = if scrolled.y >= geometry.reach - 1.0 && geometry.reach > 0.0 {
        anchors
            .iter()
            .map(|(anchor, ..)| anchor.0)
            .max()
            .unwrap_or(0)
    } else {
        anchors
            .iter()
            .filter(|(_, anchor, at_anchor)| {
                geometry.place_of(anchor, at_anchor) <= scrolled.y + 1.0
            })
            .map(|(anchor, ..)| anchor.0)
            .max()
            .unwrap_or(0)
    };
    if in_view.0 != seen {
        in_view.0 = seen;
    }
    let scrolls = geometry.reach > 0.0;
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
            percent(scrolled.y / geometry.total * 100.0),
            percent(geometry.view / geometry.total * 100.0),
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

/// What the keyboard does to the control it stands on.
#[derive(bevy::ecs::system::SystemParam)]
struct Controls<'w, 's> {
    toggles: Query<'w, 's, &'static ToggleBox>,
    choices: Query<'w, 's, &'static ChoiceButton>,
    keyfields: Query<'w, 's, &'static KeyField, With<RowKey>>,
    names: Query<'w, 's, &'static NameField>,
    sliders: Query<'w, 's, &'static SliderTrack>,
}

impl Controls<'_, '_> {
    /// What a click on `control` does: turns a toggle, opens a list, takes the next key, types.
    fn activate(
        &self,
        control: Entity,
        screen: &mut ResMut<Screen>,
        settings: &mut ResMut<Settings>,
        at: &mut Focus,
        displays: &Displays,
    ) {
        if let Ok(tick) = self.toggles.get(control)
            && Row::Toggle(tick.0).enabled(settings)
        {
            let state = tick.0.get(settings);
            tick.0.set(settings, !state);
        } else if let Ok(choice) = self.choices.get(control)
            && Row::Choice(choice.0).enabled(settings)
        {
            screen.dropdown = (screen.dropdown != Some(choice.0)).then_some(choice.0);
            // The list opens with the picked option highlighted.
            at.option = choice.0.current(settings, displays);
        } else if let Ok(field) = self.keyfields.get(control) {
            screen.capturing = Some(field.0);
            screen.held_modifier = None;
        } else if let Ok(field) = self.names.get(control) {
            screen.naming = Some(field.0);
        }
    }

    /// Moves a slider one step, left (`-1.0`) or right (`1.0`).
    fn nudge(&self, control: Entity, settings: &mut ResMut<Settings>, sign: f32) {
        if let Ok(track) = self.sliders.get(control)
            && Row::Slider(track.0).enabled(settings)
        {
            let (lo, hi, step) = track.0.range();
            let share = track.0.share(settings) + sign * step / (hi - lo);
            track.0.set_share(settings, share);
        }
    }
}

/// Tab and Shift+Tab move the keyboard along the controls; Space or Enter does what a click
/// does; Left and Right move a slider; while a list is open, Up and Down move along its options
/// and Space or Enter picks one. A click gives the focus up; a field edited (or a key set) hands
/// the focus on from where it was.
#[allow(clippy::too_many_arguments)]
fn focus(
    mut screen: ResMut<Screen>,
    mut settings: ResMut<Settings>,
    mut at: ResMut<Focus>,
    displays: Res<Displays>,
    mut keyboard: MessageReader<KeyboardInput>,
    held: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    order: Focusable,
    on: Controls,
    mut was_editing: Local<bool>,
) {
    use bevy::input::keyboard::Key;
    if mouse.just_pressed(MouseButton::Left) && at.control.is_some() {
        at.control = None;
    }
    if !screen.open {
        keyboard.clear();
        at.control = None;
        return;
    }
    if screen.dropdown.is_none() && at.option.is_some() {
        at.option = None;
    }
    let editing = screen.capturing.is_some() || screen.naming.is_some() || screen.searching;
    if editing {
        keyboard.clear();
        *was_editing = true;
        return;
    }
    // The key that ended an edit already did its work there: only Tab goes on from the field.
    let after_edit = std::mem::take(&mut *was_editing);
    let controls = order.in_order();
    let shift = held.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for event in keyboard.read().filter(|e| e.state == ButtonState::Pressed) {
        if let Some(choice) = screen.dropdown {
            pick_option(
                choice,
                &event.logical_key,
                &mut screen,
                &mut settings,
                &mut at,
                &displays,
            );
            continue;
        }
        if controls.is_empty() {
            return;
        }
        let last = controls.len() - 1;
        let here = at.control.map(|i| i.min(last));
        let control = here.map(|i| controls[i]);
        match (&event.logical_key, control) {
            (Key::Tab, _) if shift => {
                at.control = Some(here.map_or(last, |i| i.checked_sub(1).unwrap_or(last)));
            }
            (Key::Tab, _) => {
                at.control = Some(here.map_or(0, |i| if i == last { 0 } else { i + 1 }));
            }
            _ if after_edit => {}
            (Key::Enter | Key::Space, Some(control)) => {
                on.activate(control, &mut screen, &mut settings, &mut at, &displays);
            }
            (Key::ArrowLeft, Some(control)) => on.nudge(control, &mut settings, -1.0),
            (Key::ArrowRight, Some(control)) => on.nudge(control, &mut settings, 1.0),
            _ => {}
        }
    }
}

/// A key while a list is open: Up and Down move along the options that can be picked, Space or
/// Enter picks the one the keyboard stands on. (Escape closes the list, in [`escape`].)
fn pick_option(
    choice: Choice,
    key: &bevy::input::keyboard::Key,
    screen: &mut ResMut<Screen>,
    settings: &mut ResMut<Settings>,
    at: &mut Focus,
    displays: &Displays,
) {
    use bevy::input::keyboard::Key;
    let count = choice.options(settings, displays).len();
    let pickable = |i: &usize| choice.option_enabled(settings, displays, *i);
    let from = at.option.or_else(|| choice.current(settings, displays));
    match key {
        Key::ArrowDown => {
            let after = from.map_or(0, |i| i + 1);
            at.option = (after..count).find(pickable).or(from);
        }
        Key::ArrowUp => {
            let before = from.unwrap_or(count);
            at.option = (0..before).rev().find(pickable).or(from);
        }
        Key::Enter | Key::Space => {
            if let Some(i) = from {
                choice.pick(settings, i, displays);
            }
            screen.dropdown = None;
            at.option = None;
        }
        _ => {}
    }
}

/// The ring round the control the keyboard stands on: 2 px of the focus colour.
pub fn focus_ring(
    screen: Res<Screen>,
    at: Res<Focus>,
    order: Focusable,
    mut rings: Query<
        (Entity, &mut Outline),
        // Only the window's own controls: other outlines (the health bars' frames) are not ours.
        Or<(
            With<ToggleBox>,
            With<ChoiceButton>,
            With<RowKey>,
            With<NameField>,
            With<SliderTrack>,
        )>,
    >,
) {
    let focused = at
        .control
        .filter(|_| screen.open)
        .and_then(|i| order.in_order().get(i).copied());
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

/// Where the input method's candidate window stands while a text field has the keyboard: at the
/// field. Whether it composes at all is `chat::ime`'s, which sees every field of the client.
fn ime_position(
    screen: Res<Screen>,
    mut window: Single<&mut Window, With<bevy::window::PrimaryWindow>>,
    search: Query<&UiGlobalTransform, With<SearchField>>,
    names: Query<&UiGlobalTransform, With<NameField>>,
) {
    if !screen.open || !screen.is_typing() {
        return;
    }
    let field = if screen.searching {
        search.iter().next()
    } else {
        names.iter().next()
    };
    if let Some(at) = field
        && window.ime_position != at.translation
    {
        window.ime_position = at.translation;
    }
}
