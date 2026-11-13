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
    Body, CardAnchor, CardTab, ChoiceButton, ChoiceOption, CloseButton, KeyField, MuteBox,
    NameField, NavLink, ResetButton, Root, RowReset, ScrollRail, ScrollThumb, SearchClear,
    SearchField, SliderTrack, TabAnchor, TabButton, ToggleBox, ToggleRow, Vertical,
};
use super::model::{Choice, Row, Slider, Tab, TextSetting};
use crate::channels::{Channel, Preview};
use crate::config::Settings;
use crate::keys::{self, Chord, Input};
use crate::theme;
use crate::window::Displays;

/// The screen: whether it is open, the tab in view (the tabs' sections follow one another down a
/// single scrolling body), the key field taking the next key, and the list opened from a choice.
#[derive(Resource, Clone, Debug, PartialEq)]
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

    /// Goes to `tab`, leaving a search and whatever was half done.
    fn show(&mut self, tab: Tab) {
        self.tab = tab;
        self.query.clear();
        self.searching = false;
        self.dropdown = None;
        self.capturing = None;
    }
}

/// Where the navigation asks the body to scroll to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jump {
    /// The start of a tab: its heading, and the slots above its first section.
    Tab(Tab),
    /// A card, by its place in the open view.
    Card(usize),
}

/// How far the body is scrolled, in interface pixels, and where the navigation asked to scroll
/// to. Kept apart from the screen, whose every change builds the layout again.
#[derive(Resource, Default)]
pub struct Scroll {
    pub y: f32,
    pub jump: Option<Jump>,
    /// A control the keyboard moved to, to be brought into view.
    reveal: Option<Entity>,
    /// What the last jump asked for: it counts as in view when the content ended before it
    /// could be carried out, until the wheel moves the body.
    pinned: Option<Jump>,
    /// Frames a jump has waited for its target to be laid out.
    waited: u8,
    /// The scroll the last layout drew the body at: what the places of its nodes are relative to,
    /// while `y` may already be somewhere else.
    laid: f32,
}

/// The control the keyboard stands on, by its name (the controls that show change as the window
/// is used); none until Tab is pressed, and none again after a click.
#[derive(Resource, Default)]
pub struct Focus {
    pub control: Option<String>,
    /// The option of an open list the keyboard stands on.
    pub option: Option<usize>,
}

/// The card in view, by its place in the open view: the navigation marks its link.
#[derive(Resource, Default)]
pub struct InView(pub usize);

/// The space over a card the body keeps when it scrolls to the card.
const CARD_MARGIN: f32 = 24.0;
/// Frames a jump waits for the layout of its target, which a search ended by it builds again.
const JUMP_PATIENCE: u8 = 30;
/// How much a slider moves before another try of its sound: a try per step of a drag would
/// stack the sounds.
const TRY_STEP: f32 = 0.05;
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
                super::draw::layout.in_set(crate::ui::WidgetBuild),
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
                super::look::fader_fills,
                super::look::fader_caps,
                super::look::mutes,
                super::look::slider_values,
                super::look::choices,
                super::look::lists_open_upward,
                super::look::key_fields,
                super::look::key_texts,
                super::look::clash_bar,
                super::look::names,
                super::look::nav,
                super::screen::focus_ring,
            )
                .after(super::draw::layout),
        );
}

/// `--settings TAB` opens the screen at that tab.
fn open_at_start(args: Res<crate::Args>, mut screen: ResMut<Screen>, mut scroll: ResMut<Scroll>) {
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
            scroll.jump = Some(Jump::Tab(tab));
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
        scroll.jump = Some(Jump::Tab(tab.0));
    }
    if let Ok(link) = on.links.get(target) {
        scroll.jump = Some(link.jump);
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

/// A slider held down follows the cursor along its track; the sound of the channel a slider
/// sets is tried as it moves, so that the level is tuned by ear.
fn drag(
    screen: Res<Screen>,
    mut settings: ResMut<Settings>,
    tracks: Query<(
        Has<bevy::ui::Pressed>,
        &SliderTrack,
        &RelativeCursorPosition,
        Has<Vertical>,
    )>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut tries: Local<Tries>,
    mut sounds: MessageWriter<Preview>,
) {
    if !screen.open {
        return;
    }
    let mut held = None;
    for (interaction, track, at, vertical) in &tracks {
        if !interaction || !mouse.pressed(MouseButton::Left) {
            continue;
        }
        let Some(at) = at.normalized else {
            continue;
        };
        held = Some(track.0);
        // The cursor's place is about the track's middle; a fader's top is its most.
        let share = if vertical { 0.5 - at.y } else { at.x + 0.5 };
        let before = track.0.get(&settings);
        let mut changed = settings.clone();
        track.0.set_share(&mut changed, share);
        if track.0.get(&changed) != before {
            track.0.set_share(&mut settings, share);
        }
    }
    tries.follow(held, &settings, &mut sounds);
}

/// The slider being dragged and the value its sound was last tried at.
#[derive(Default)]
struct Tries {
    last: Option<(Slider, f32)>,
}

impl Tries {
    /// Tries the sound at every step of a drag and where it ends.
    fn follow(
        &mut self,
        held: Option<Slider>,
        settings: &Settings,
        sounds: &mut MessageWriter<Preview>,
    ) {
        let Some(slider) = held else {
            if let Some((slider, tried)) = self.last.take()
                && slider.get(settings) != tried
            {
                try_sound(slider, settings, sounds);
            }
            return;
        };
        let value = slider.get(settings);
        let moved = match self.last {
            Some((was, tried)) if was == slider => (value - tried).abs() >= TRY_STEP,
            _ => true,
        };
        if moved {
            try_sound(slider, settings, sounds);
            self.last = Some((slider, value));
        }
    }
}

/// Asks for the sound of the channel the slider sets, if it sets one: the master volume is heard
/// through the first channel that is not muted.
pub fn try_sound(slider: Slider, settings: &Settings, sounds: &mut MessageWriter<Preview>) {
    let channel = match slider {
        Slider::Level(channel) => Some(channel),
        // A channel that has its sound to hand: the hero's voice is read for its own fader.
        Slider::Volume => Channel::ALL
            .into_iter()
            .find(|channel| *channel != Channel::Voices && !settings.mix.muted(*channel)),
        Slider::InterfaceScale | Slider::RenderScale => None,
    };
    if let Some(channel) = channel {
        sounds.write(Preview(channel));
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

    /// Where a node's top stands in the content.
    fn content_top(&self, node: &ComputedNode, at: &UiGlobalTransform) -> f32 {
        let node_top = at.translation.y - node.size().y / 2.0;
        (node_top - self.top) * self.scale + self.applied
    }

    /// Where a card's top stands in the content: what to scroll to to put it at the top.
    fn place_of(&self, card: &ComputedNode, at: &UiGlobalTransform) -> f32 {
        self.content_top(card, at) - CARD_MARGIN
    }
}

/// The nodes the navigation scrolls to: the tabs' starts and the cards.
#[derive(bevy::ecs::system::SystemParam)]
struct Anchors<'w, 's> {
    tabs: Query<
        'w,
        's,
        (
            &'static TabAnchor,
            &'static ComputedNode,
            &'static UiGlobalTransform,
        ),
    >,
    cards: Query<
        'w,
        's,
        (
            &'static CardAnchor,
            &'static CardTab,
            &'static ComputedNode,
            &'static UiGlobalTransform,
        ),
    >,
}

impl Anchors<'_, '_> {
    /// What to scroll to to put the jump's target at the top, once it is laid out.
    fn place_of(&self, jump: Jump, geometry: &Geometry) -> Option<f32> {
        match jump {
            Jump::Tab(tab) => {
                self.tabs
                    .iter()
                    .find(|(anchor, ..)| anchor.0 == tab)
                    .map(|(_, node, at)| {
                        // The first tab starts at the body's top; the others have their heading's
                        // own space over it.
                        if tab == Tab::ALL[0] {
                            geometry.place_of(node, at)
                        } else {
                            geometry.content_top(node, at)
                        }
                    })
            }
            Jump::Card(card) => self
                .cards
                .iter()
                .find(|(anchor, ..)| anchor.0 == card)
                .map(|(_, _, node, at)| geometry.place_of(node, at)),
        }
    }

    /// The tab in view and the card in it: the last of each that has reached the top. At the end
    /// of the content, a jump the content was too short to carry out counts as made.
    fn in_view(&self, geometry: &Geometry, scrolled: &Scroll) -> Option<(Tab, usize)> {
        let reached = |node: &ComputedNode, at: &UiGlobalTransform| {
            geometry.place_of(node, at) <= scrolled.y + 1.0
        };
        let at_end = geometry.reach > 0.0 && scrolled.y >= geometry.reach - 1.0;
        let first_card = |tab: Tab| {
            self.cards
                .iter()
                .filter(|(_, of, ..)| of.0 == tab)
                .map(|(anchor, ..)| anchor.0)
                .min()
        };
        let last_card = || {
            self.cards
                .iter()
                .max_by_key(|(anchor, ..)| anchor.0)
                .map(|(anchor, of, ..)| (of.0, anchor.0))
        };
        match scrolled.pinned.filter(|_| at_end) {
            Some(Jump::Tab(tab)) => return first_card(tab).map(|card| (tab, card)),
            Some(Jump::Card(card)) => {
                return self
                    .cards
                    .iter()
                    .find(|(anchor, ..)| anchor.0 == card)
                    .map(|(_, of, ..)| (of.0, card));
            }
            None => {}
        }
        if at_end {
            return last_card();
        }
        // A search lists cards of every tab and no tab's start: the cards alone decide.
        let tab = if self.tabs.is_empty() {
            None
        } else {
            self.tabs
                .iter()
                .filter(|(_, node, at)| geometry.content_top(node, at) <= scrolled.y + 1.0)
                .map(|(anchor, ..)| anchor.0)
                .max_by_key(|tab| Tab::ALL.iter().position(|t| t == tab))
                .or(Some(Tab::ALL[0]))
        };
        let card = self
            .cards
            .iter()
            .filter(|(_, of, node, at)| tab.is_none_or(|tab| of.0 == tab) && reached(node, at))
            .map(|(anchor, ..)| anchor.0)
            .max()
            .or_else(|| tab.and_then(first_card).or(Some(0)))?;
        let of = self
            .cards
            .iter()
            .find(|(anchor, ..)| anchor.0 == card)
            .map(|(_, of, ..)| of.0)?;
        Some((tab.unwrap_or(of), card))
    }
}

/// The wheel scrolls the screen's body, the navigation's links scroll it to a tab or a card, and
/// the keyboard's control is brought into view.
#[allow(clippy::type_complexity)]
fn scroll(
    screen: Res<Screen>,
    wheel: Res<AccumulatedMouseScroll>,
    mut scrolled: ResMut<Scroll>,
    mut body: Query<(&mut ScrollPosition, &ComputedNode, &UiGlobalTransform), With<Body>>,
    anchors: Anchors,
    controls: Query<(&ComputedNode, &UiGlobalTransform), Without<Body>>,
) {
    if !screen.open {
        return;
    }
    let Ok((mut at, node, place)) = body.single_mut() else {
        return;
    };
    let geometry = Geometry::of(node, place, at.y);
    scrolled.laid = at.y;
    if wheel.delta.y != 0.0 {
        let step = match wheel.unit {
            MouseScrollUnit::Line => wheel.delta.y * 40.0,
            MouseScrollUnit::Pixel => wheel.delta.y,
        };
        scrolled.y -= step;
        scrolled.pinned = None;
    }
    follow_jump(&mut scrolled, &geometry, &anchors);
    if let Some(control) = scrolled.reveal.take()
        && geometry.laid_out()
        && let Ok((node, place)) = controls.get(control)
    {
        let top = geometry.content_top(node, place);
        let height = node.size().y * geometry.scale;
        let room = geometry.view - CARD_MARGIN;
        if top < scrolled.y + CARD_MARGIN {
            scrolled.y = top - CARD_MARGIN;
        } else if top + height > scrolled.y + room {
            scrolled.y = top + height - room;
        }
    }
    scrolled.y = scrolled.y.clamp(0.0, geometry.reach);
    if at.y != scrolled.y {
        at.y = scrolled.y;
    }
}

/// Carries out the jump the navigation asked for once its target is laid out; a target a search
/// just made way for is waited for a while.
fn follow_jump(scrolled: &mut Scroll, geometry: &Geometry, anchors: &Anchors) {
    let Some(jump) = scrolled.jump else {
        return;
    };
    let place = anchors
        .place_of(jump, geometry)
        .filter(|_| geometry.laid_out());
    if let Some(y) = place {
        scrolled.y = y.max(0.0);
        scrolled.pinned = Some(jump);
        scrolled.jump = None;
        scrolled.waited = 0;
    } else if scrolled.waited < JUMP_PATIENCE {
        scrolled.waited += 1;
    } else {
        scrolled.jump = None;
        scrolled.waited = 0;
    }
}

/// The tab and the card in view, whose links the navigation marks, and the scroll bar: shown
/// while the body scrolls, its thumb as long and as far along as the body shows and has scrolled.
#[allow(clippy::type_complexity)]
fn scroll_marks(
    mut screen: ResMut<Screen>,
    scrolled: Res<Scroll>,
    mut in_view: ResMut<InView>,
    body: Query<(&ComputedNode, &UiGlobalTransform), With<Body>>,
    anchors: Anchors,
    mut rail: Query<&mut Node, (With<ScrollRail>, Without<ScrollThumb>)>,
    mut thumb: Query<&mut Node, (With<ScrollThumb>, Without<ScrollRail>)>,
) {
    if !screen.open {
        return;
    }
    let Ok((node, place)) = body.single() else {
        return;
    };
    // The nodes stand where the last layout put them, which is not where the body is scrolled to now.
    let geometry = Geometry::of(node, place, scrolled.laid);
    if !geometry.laid_out() {
        return;
    }
    if let Some((tab, card)) = anchors.in_view(&geometry, &scrolled) {
        if in_view.0 != card {
            in_view.0 = card;
        }
        // A tab the navigation is on its way to is the open one already.
        if !screen.is_searching() && scrolled.jump.is_none() && screen.tab != tab {
            screen.tab = tab;
        }
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

/// How far apart, down the window, two controls of one row can stand.
const ROW_TOLERANCE: f32 = 8.0;

/// The controls the keyboard can stand on, in reading order (top to bottom, then left to right).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Focusable<'w, 's> {
    roots: Query<'w, 's, Entity, With<Root>>,
    parents: Query<'w, 's, &'static ChildOf>,
    nodes: Query<
        'w,
        's,
        (
            Entity,
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static InheritedVisibility,
        ),
        AnyControl,
    >,
    nav: Query<
        'w,
        's,
        (),
        Or<(
            With<TabButton>,
            With<NavLink>,
            With<SearchField>,
            With<SearchClear>,
        )>,
    >,
    header: Query<'w, 's, (), Or<(With<CloseButton>, With<ResetButton>)>>,
    names: Query<'w, 's, &'static Name>,
}

/// The names of every control that shows, for the tests: the set the keyboard has to reach.
#[cfg(test)]
pub fn shown_controls(world: &mut World) -> std::collections::BTreeSet<String> {
    world
        .query_filtered::<(&Name, &ComputedNode, &InheritedVisibility), AnyControl>()
        .iter(world)
        .filter(|(_, node, seen)| seen.get() && node.size() != Vec2::ZERO)
        .map(|(name, ..)| name.to_string())
        .collect()
}

/// Every control the keyboard can stand on: the body's rows (their fields, boxes, sliders, mutes,
/// slots and reset arrows), the list at the left (the search, the tabs, the section links) and the
/// header's buttons.
type AnyControl = Or<(
    With<ToggleBox>,
    With<MuteBox>,
    With<ChoiceButton>,
    With<KeyField>,
    With<NameField>,
    With<SliderTrack>,
    With<RowReset>,
    With<ResetButton>,
    With<SearchField>,
    With<SearchClear>,
    With<TabButton>,
    With<NavLink>,
    With<CloseButton>,
    With<crate::ui::help::Hint>,
)>;

impl Focusable<'_, '_> {
    /// Where a control's middle stands down the window.
    fn height_of(&self, control: Entity) -> Option<f32> {
        self.nodes
            .get(control)
            .ok()
            .map(|(_, _, at, _)| at.translation.y)
    }

    /// Where a control stands in the window, in the order the keyboard walks them: the header,
    /// then the list at the left, then the body. The header comes first because what it holds
    /// follows the tab in view, which the walk through the body changes: the help of HOTKEYS
    /// would be gone by the time the walk came round to it.
    fn region(&self, control: Entity) -> u8 {
        if self.header.contains(control) {
            0
        } else if self.nav.contains(control) {
            1
        } else {
            2
        }
    }

    /// The name of a control, which is how the keyboard knows the one it stands on: the controls
    /// that show change (a reset arrow appears with a change, the window is built again) while
    /// the one the keyboard stands on stays.
    fn name_of(&self, control: Entity) -> Option<String> {
        self.names.get(control).ok().map(|name| name.to_string())
    }

    /// Where the control called `name` stands among `controls`.
    fn position_of(&self, controls: &[Entity], name: &str) -> Option<usize> {
        controls
            .iter()
            .position(|control| self.names.get(*control).is_ok_and(|n| n.as_str() == name))
    }

    /// Whether a control is one of the body's.
    fn in_body(&self, control: Entity) -> bool {
        self.region(control) == 2
    }

    /// The controls that show: the header's, then the list's, then the body's, each in reading
    /// order (top to bottom, then left to right).
    pub fn in_order(&self) -> Vec<Entity> {
        let Ok(root) = self.roots.single() else {
            return Vec::new();
        };
        let mut found: Vec<(u8, Entity, Vec2)> = self
            .nodes
            .iter()
            .filter(|(_, node, _, seen)| seen.get() && node.size() != Vec2::ZERO)
            .filter(|(entity, ..)| {
                self.parents
                    .iter_ancestors(*entity)
                    .take(64)
                    .any(|ancestor| ancestor == root)
            })
            .map(|(entity, _, at, _)| (self.region(entity), entity, at.translation))
            .collect();
        // A row's controls share a line to within a few pixels. The rows are cut from the first
        // control of each, not from a grid, so that scrolling the body (which moves every control
        // by one amount) never moves a control from one row to another and so changes the order.
        found.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.y.total_cmp(&b.2.y)));
        let mut row = 0_u32;
        let mut row_start: Option<(u8, f32)> = None;
        let mut rows: Vec<(u8, u32, f32, Entity)> = Vec::with_capacity(found.len());
        for (region, entity, at) in found {
            if row_start.is_none_or(|(first, y)| first != region || at.y - y > ROW_TOLERANCE) {
                row += 1;
                row_start = Some((region, at.y));
            }
            rows.push((region, row, at.x, entity));
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)));
        rows.into_iter().map(|(.., entity)| entity).collect()
    }
}

/// What the keyboard does to the control it stands on.
#[derive(bevy::ecs::system::SystemParam)]
struct Controls<'w, 's> {
    toggles: Query<'w, 's, &'static ToggleBox>,
    mutes: Query<'w, 's, &'static MuteBox>,
    choices: Query<'w, 's, &'static ChoiceButton>,
    keyfields: Query<'w, 's, &'static KeyField>,
    names: Query<'w, 's, &'static NameField>,
    sliders: Query<'w, 's, &'static SliderTrack>,
}

impl Controls<'_, '_> {
    /// What a click on `control` does: turns a toggle, opens a list, takes the next key, types;
    /// false for a control that is a button, which the press it makes is for.
    fn activate(
        &self,
        control: Entity,
        screen: &mut ResMut<Screen>,
        settings: &mut ResMut<Settings>,
        at: &mut Focus,
        displays: &Displays,
    ) -> bool {
        if let Some(toggle) = self
            .toggles
            .get(control)
            .map(|tick| tick.0)
            .ok()
            .or_else(|| self.mutes.get(control).map(|mute| mute.0).ok())
            && Row::Toggle(toggle).enabled(settings)
        {
            let state = toggle.get(settings);
            toggle.set(settings, !state);
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
        } else if !self.toggles.contains(control)
            && !self.mutes.contains(control)
            && !self.choices.contains(control)
            && !self.sliders.contains(control)
        {
            return false;
        }
        true
    }

    /// Moves a slider one step, towards less (`-1.0`) or more (`1.0`), and tries its sound.
    fn nudge(
        &self,
        control: Entity,
        settings: &mut ResMut<Settings>,
        sign: f32,
        sounds: &mut MessageWriter<Preview>,
    ) {
        if let Ok(track) = self.sliders.get(control)
            && Row::Slider(track.0).enabled(settings)
        {
            let (lo, hi, step) = track.0.range();
            let share = track.0.share(settings) + sign * step / (hi - lo);
            track.0.set_share(settings, share);
            try_sound(track.0, settings, sounds);
        }
    }
}

/// Tab and Shift+Tab move the keyboard along the controls, scrolling the body to the one it
/// stands on; Space or Enter does what a click does; Left and Down, Right and Up move a slider; while a list is open, Up and Down move along its options
/// and Space or Enter picks one. A click gives the focus up; a field edited (or a key set) hands
/// the focus on from where it was.
#[allow(clippy::too_many_arguments)]
fn focus(
    mut screen: ResMut<Screen>,
    mut settings: ResMut<Settings>,
    mut at: ResMut<Focus>,
    mut native: ResMut<bevy::input_focus::InputFocus>,
    displays: Res<Displays>,
    mut keyboard: MessageReader<KeyboardInput>,
    held: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    order: Focusable,
    on: Controls,
    body: Query<(&ComputedNode, &UiGlobalTransform), With<Body>>,
    tab_starts: Query<(&TabAnchor, &ComputedNode, &UiGlobalTransform), Without<Body>>,
    mut scroll: ResMut<Scroll>,
    mut sounds: MessageWriter<Preview>,
    mut press: ResMut<crate::ui::PrimaryPress>,
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
    // Tab, from no control yet, begins at the first control of the tab in view that the body
    // shows, not at the top of the settings' long list.
    let tab_top = tab_starts
        .iter()
        .find(|(start, ..)| start.0 == screen.tab && !screen.is_searching())
        .map(|(_, node, at)| at.translation.y - node.size().y / 2.0);
    let shown = body.single().ok().map(|(node, at)| {
        let half = node.size().y / 2.0;
        let top = at.translation.y - half;
        tab_top.map_or(top, |start| start.max(top))..=(at.translation.y + half)
    });
    let in_view = |control: &&Entity| {
        shown
            .as_ref()
            .zip(order.height_of(**control))
            .is_some_and(|(shown, y)| shown.contains(&y))
    };
    let seen_in_body = |control: &&Entity| order.in_body(**control) && in_view(control);
    let first_seen = controls.iter().position(|c| seen_in_body(&c));
    let last_seen = controls.iter().rposition(|c| seen_in_body(&c));
    let shift = held.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for event in keyboard.read().filter(|e| e.state == ButtonState::Pressed) {
        if let Some(choice) = screen.dropdown {
            if event.logical_key == Key::Tab {
                // Tab leaves an open list as it leaves a dialog's field: the list closes with the
                // choice as it was, and the walk goes on from the button that opened it.
                screen.dropdown = None;
                at.option = None;
            } else {
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
        }
        if controls.is_empty() {
            return;
        }
        let last = controls.len() - 1;
        let here = at
            .control
            .as_deref()
            .and_then(|name| order.position_of(&controls, name));
        let control = here.map(|i| controls[i]);
        match (&event.logical_key, control) {
            (Key::Tab, _) if shift => {
                let to = here.map_or(last_seen.unwrap_or(last), |i| {
                    i.checked_sub(1).unwrap_or(last)
                });
                at.control = order.name_of(controls[to]);
                scroll.reveal = Some(controls[to]).filter(|control| order.in_body(*control));
            }
            (Key::Tab, _) => {
                let to = here.map_or(
                    first_seen.unwrap_or(0),
                    |i| {
                        if i == last { 0 } else { i + 1 }
                    },
                );
                at.control = order.name_of(controls[to]);
                scroll.reveal = Some(controls[to]).filter(|control| order.in_body(*control));
            }
            _ if after_edit => {}
            (Key::Enter | Key::Space, Some(control)) => {
                if !on.activate(control, &mut screen, &mut settings, &mut at, &displays) {
                    // The rest are buttons: the press a click on them would be.
                    press.0 = Some(control);
                }
            }
            (Key::ArrowLeft | Key::ArrowDown, Some(control)) => {
                on.nudge(control, &mut settings, -1.0, &mut sounds);
            }
            (Key::ArrowRight | Key::ArrowUp, Some(control)) => {
                on.nudge(control, &mut settings, 1.0, &mut sounds);
            }
            _ => {}
        }
    }
    if let Some(control) = at
        .control
        .as_deref()
        .and_then(|name| order.position_of(&controls, name))
        .map(|index| controls[index])
        && native.get() != Some(control)
    {
        native.set(control, bevy::input_focus::FocusCause::Navigated);
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
        AnyControl,
    >,
) {
    let focused = at
        .control
        .as_deref()
        .filter(|_| screen.open)
        .and_then(|name| order.position_of(&order.in_order(), name))
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
