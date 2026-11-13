//! What the settings screen's parts look like for the settings' values, kept up to date in place
//! every frame the screen is open (the layout is built only when the screen's state changes, so a
//! slider stays held while it is dragged): texts, marks, fills, and the colours that say changed,
//! disabled, held, hovered, open, capturing and clashing.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;

use super::draw::{
    CAP, CAP_HELD, ChoiceButton, ChoiceOption, ChoiceValue, ClashBar, ClashText, KeyField, KeyText,
    LabelPart, MuteBox, NameCaret, NameCount, NameField, NameText, NavLink, NavText, OptionsList,
    RowBar, RowKey, RowReset, SliderFill, SliderThumb, SliderTrack, SliderValue, SlotBadge,
    SlotKey, THUMB, THUMB_HELD, ToggleBox, Vertical,
};
use super::model::{Row, Tab};
use super::screen::{Focus, InView, Screen};
use crate::config::Settings;
use crate::keys;
use crate::theme;
use crate::window::Displays;

/// Inactive controls share a foreground alpha, distinct from black-scrim darkening.
fn off_alpha() -> f32 {
    theme::DISABLED_FOREGROUND_ALPHA
}
/// The ink on the setting key's badge, over the blue.
const BADGE_SETTING_INK: Color = Color::srgb(0.043, 0.055, 0.067);

/// With the alpha of a control that cannot be used now.
fn dimmed(color: Color, enabled: bool) -> Color {
    if enabled {
        color
    } else {
        color.with_alpha(color.alpha() * off_alpha())
    }
}

/// What a key field or slot shows of its binding.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyState {
    /// Waiting for its key.
    Setting,
    /// Sharing its key with another binding.
    Clash,
    Unbound,
    Bound,
}

fn key_state(screen: &Screen, settings: &Settings, at: usize) -> KeyState {
    if screen.capturing == Some(at) {
        KeyState::Setting
    } else if settings.keys[at].is_none() {
        KeyState::Unbound
    } else if !keys::conflicts(&settings.keys, at).is_empty() {
        KeyState::Clash
    } else {
        KeyState::Bound
    }
}

/// The chord a binding has, as its field says it.
fn chord_name(settings: &Settings, at: usize) -> String {
    settings.keys[at].map_or_else(|| "—".to_owned(), |chord| chord.name())
}

/// Every pair of bindings sharing a chord where both work.
pub fn clash_line(settings: &Settings) -> String {
    let mut pairs = Vec::new();
    for at in 0..keys::COUNT {
        for other in keys::conflicts(&settings.keys, at) {
            let theirs = keys::index_of(other).unwrap_or(at);
            if theirs > at {
                pairs.push(format!(
                    "{} and {} share {}",
                    keys::BINDINGS[at].label,
                    keys::BINDINGS[theirs].label,
                    settings.keys[at].map_or_else(String::new, |c| c.name())
                ));
            }
        }
    }
    pairs.join("   ")
}

/// A changed row's mark and reset arrow, and the colour of the labels.
#[allow(clippy::type_complexity)]
pub fn rows(
    screen: Res<Screen>,
    settings: Res<Settings>,
    displays: Res<Displays>,
    mut bars: Query<(&RowBar, &mut Node), Without<RowReset>>,
    mut resets: Query<(&RowReset, &mut Node), Without<RowBar>>,
    mut labels: Query<(&LabelPart, &mut TextColor)>,
) {
    if !screen.open {
        return;
    }
    let show = |on: bool| if on { Display::Flex } else { Display::None };
    for (bar, mut node) in &mut bars {
        node.display = show(bar.0.changed(&settings, &displays));
    }
    for (reset, mut node) in &mut resets {
        let display = show(reset.0.changed(&settings, &displays) && reset.0.enabled(&settings));
        if node.display != display {
            node.display = display;
        }
    }
    for (part, mut ink) in &mut labels {
        let color = match (part.1, part.0.enabled(&settings)) {
            (true, _) => theme::SELECT,
            (false, true) => theme::ROW_INK,
            (false, false) => theme::INK_OFF,
        };
        ink.set_if_neq(TextColor(color));
    }
}

/// Settings supplies values; the shared widget owns the checkbox's appearance.
pub fn toggles(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut boxes: Query<(&ToggleBox, &mut crate::ui::checkbox::State)>,
) {
    if !screen.open {
        return;
    }
    for (tick, mut state) in &mut boxes {
        state.set_if_neq(crate::ui::checkbox::State {
            checked: tick.0.get(&settings),
            enabled: Row::Toggle(tick.0).enabled(&settings),
        });
    }
}

/// Slider fills: to the value, grey when the slider is disabled.
pub fn slider_fills(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut fills: Query<(&SliderFill, &mut Node, &mut BackgroundColor), Without<Vertical>>,
) {
    if !screen.open {
        return;
    }
    for (fill, mut node, mut ground) in &mut fills {
        let enabled = Row::Slider(fill.0).enabled(&settings);
        let width = percent(fill.0.share(&settings) * 100.0);
        if node.width != width {
            node.width = width;
        }
        let color = if enabled { theme::FOCUS } else { theme::INK_6 };
        ground.set_if_neq(BackgroundColor(color));
    }
}

/// Slider thumbs: on the value, larger and ringed while held.
pub fn slider_thumbs(
    screen: Res<Screen>,
    settings: Res<Settings>,
    tracks: Query<(&SliderTrack, Has<bevy::ui::Pressed>)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut thumbs: Query<
        (
            &SliderThumb,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Without<Vertical>,
    >,
) {
    if !screen.open {
        return;
    }
    let held = |slider| {
        tracks
            .iter()
            .any(|(track, i)| track.0 == slider && i && mouse.pressed(MouseButton::Left))
    };
    for (thumb, mut node, mut ground, mut edge) in &mut thumbs {
        let slider = thumb.0;
        let enabled = Row::Slider(slider).enabled(&settings);
        let grip = held(slider) && enabled;
        let (size, top, ring) = if grip {
            (THUMB_HELD, 1.0, 3.0)
        } else {
            (THUMB, 4.0, 0.0)
        };
        let left = percent(slider.share(&settings) * 100.0);
        let (width, height, top, ring, margin) =
            (px(size), px(size), px(top), px(ring), px(-size / 2.0));
        if node.left != left
            || node.width != width
            || node.top != top
            || node.border.left != ring
            || node.margin.left != margin
        {
            node.left = left;
            node.width = width;
            node.height = height;
            node.top = top;
            node.border = UiRect::all(ring);
            node.margin = UiRect::left(margin);
        }
        let ink = if enabled { theme::INK_1 } else { theme::INK_5 };
        ground.set_if_neq(BackgroundColor(dimmed(ink, enabled)));
        edge.set_if_neq(BorderColor::all(theme::FOCUS));
    }
}

/// Fader fills: to the level, grey when the fader is disabled.
pub fn fader_fills(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut fills: Query<(&SliderFill, &mut Node, &mut BackgroundColor), With<Vertical>>,
) {
    if !screen.open {
        return;
    }
    for (fill, mut node, mut ground) in &mut fills {
        let height = percent(fill.0.share(&settings) * 100.0);
        if node.height != height {
            node.height = height;
        }
        let color = if Row::Slider(fill.0).enabled(&settings) {
            theme::FOCUS
        } else {
            theme::INK_6
        };
        ground.set_if_neq(BackgroundColor(color));
    }
}

/// Fader caps: on the level, larger and ringed while held.
#[allow(clippy::type_complexity)]
pub fn fader_caps(
    screen: Res<Screen>,
    settings: Res<Settings>,
    tracks: Query<(&SliderTrack, Has<bevy::ui::Pressed>)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut caps: Query<
        (
            &SliderThumb,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Vertical>,
    >,
) {
    if !screen.open {
        return;
    }
    for (cap, mut node, mut ground, mut edge) in &mut caps {
        let enabled = Row::Slider(cap.0).enabled(&settings);
        let held = enabled
            && mouse.pressed(MouseButton::Left)
            && tracks
                .iter()
                .any(|(track, pressed)| track.0 == cap.0 && pressed);
        let (width, height) = if held { CAP_HELD } else { CAP };
        let ring = if held { 2.0 } else { 0.0 };
        let wanted = (
            percent(cap.0.share(&settings) * 100.0),
            px(width),
            px(height),
            px(ring),
            px(-height / 2.0),
        );
        if (
            node.bottom,
            node.width,
            node.height,
            node.border.left,
            node.margin.bottom,
        ) != wanted
        {
            node.bottom = wanted.0;
            node.width = wanted.1;
            node.height = wanted.2;
            node.border = UiRect::all(wanted.3);
            node.margin = UiRect::bottom(wanted.4);
            // The cap stays centred on its rail however wide it is.
            node.left = px((super::draw::FADER_WIDTH - width) / 2.0);
        }
        let ink = if enabled { theme::INK_1 } else { theme::INK_5 };
        ground.set_if_neq(BackgroundColor(dimmed(ink, enabled)));
        edge.set_if_neq(BorderColor::all(theme::FOCUS));
    }
}

/// Mute buttons: red while their channel is silenced.
pub fn mutes(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut buttons: Query<(&MuteBox, &mut BackgroundColor, &mut BorderColor)>,
) {
    if !screen.open {
        return;
    }
    for (mute, mut ground, mut edge) in &mut buttons {
        let (fill, line) = if mute.0.get(&settings) {
            theme::STOP
        } else {
            (theme::FIELD, theme::FIELD_EDGE)
        };
        ground.set_if_neq(BackgroundColor(fill));
        edge.set_if_neq(BorderColor::all(line));
    }
}

/// The value beside a slider.
pub fn slider_values(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut values: Query<(&SliderValue, &mut Text, &mut TextColor)>,
) {
    if !screen.open {
        return;
    }
    for (value, mut text, mut ink) in &mut values {
        text.set_if_neq(Text(value.0.shown(&settings)));
        let color = if Row::Slider(value.0).enabled(&settings) {
            theme::INK_1
        } else {
            theme::INK_OFF
        };
        ink.set_if_neq(TextColor(color));
    }
}

/// Choices: the picked option's name; the field open, hovered or disabled; the list's options.
#[allow(clippy::type_complexity)]
pub fn choices(
    screen: Res<Screen>,
    settings: Res<Settings>,
    displays: Res<Displays>,
    mut buttons: Query<(
        &ChoiceButton,
        &Hovered,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut values: Query<(&ChoiceValue, &mut Text, &mut TextColor)>,
    mut options: Query<(&ChoiceOption, &Hovered, &mut BackgroundColor), Without<ChoiceButton>>,
    focus: Res<Focus>,
) {
    if !screen.open {
        return;
    }
    for (button, interaction, mut ground, mut edge) in &mut buttons {
        let choice = button.0;
        let (fill, line) = if !Row::Choice(choice).enabled(&settings) {
            theme::FIELD_OFF
        } else if screen.dropdown == Some(choice) {
            (theme::FIELD, theme::FOCUS)
        } else if interaction.get() {
            theme::FIELD_HOVER
        } else {
            (theme::FIELD, theme::FIELD_EDGE)
        };
        ground.set_if_neq(BackgroundColor(fill));
        edge.set_if_neq(BorderColor::all(line));
    }
    for (value, mut text, mut ink) in &mut values {
        text.set_if_neq(Text(value.0.shown(&settings, &displays)));
        let color = if Row::Choice(value.0).enabled(&settings) {
            theme::INK_1
        } else {
            theme::INK_OFF
        };
        ink.set_if_neq(TextColor(color));
    }
    for (option, interaction, mut ground) in &mut options {
        let picked = option.0.current(&settings, &displays) == Some(option.1);
        let enabled = option.0.option_enabled(&settings, &displays, option.1);
        let on_it = interaction.get() || focus.option == Some(option.1);
        let color = match (on_it, picked, enabled) {
            (false, true, _) => theme::LIST_PICKED,
            (false, false, _) | (_, _, false) => Color::NONE,
            _ => theme::LIST_HOVER,
        };
        ground.set_if_neq(BackgroundColor(color));
    }
}

/// An open list stands above its button when the window has no room for it below, so that every
/// option can be reached.
pub fn lists_open_upward(
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    mut lists: Query<(&ChildOf, &mut Node, &ComputedNode), With<OptionsList>>,
    anchors: Query<(&ComputedNode, &UiGlobalTransform), Without<OptionsList>>,
) {
    for (parent, mut node, list) in &mut lists {
        let Ok((anchor, at)) = anchors.get(parent.parent()) else {
            continue;
        };
        let below = at.translation.y + anchor.size().y / 2.0 + list.size().y;
        let upward = below > window.physical_height() as f32;
        let (top, bottom) = if upward {
            (Val::Auto, px(42))
        } else {
            (px(42), Val::Auto)
        };
        if node.top != top || node.bottom != bottom {
            node.top = top;
            node.bottom = bottom;
        }
    }
}

/// Key fields and HUD slots: a field's ground and edge, a slot's edge, for a key being set, a
/// clash, an unbound key.
#[allow(clippy::type_complexity)]
pub fn key_fields(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut fields: Query<(
        &KeyField,
        &mut BackgroundColor,
        &mut BorderColor,
        &mut Node,
        Has<RowKey>,
    )>,
) {
    if !screen.open {
        return;
    }
    for (field, mut ground, mut edge, mut node, in_row) in &mut fields {
        let state = key_state(&screen, &settings, field.0);
        if in_row {
            let (fill, line, _) = match state {
                KeyState::Setting => theme::KEY_SETTING,
                KeyState::Clash => theme::KEY_CLASH,
                KeyState::Unbound => (theme::KEY_UNBOUND.0, theme::KEY_UNBOUND.1, theme::INK_6),
                KeyState::Bound => (theme::FIELD, theme::FIELD_EDGE, theme::INK_1),
            };
            ground.set_if_neq(BackgroundColor(fill));
            edge.set_if_neq(BorderColor::all(line));
            continue;
        }
        let (line, width) = match state {
            KeyState::Setting => (theme::FOCUS, 2.0),
            KeyState::Clash => (theme::DIRE, 2.0),
            _ => (theme::TOOLTIP_EDGE, 1.0),
        };
        edge.set_if_neq(BorderColor::all(line));
        let border = UiRect::all(px(width));
        if node.border != border {
            node.border = border;
        }
    }
}

/// What a field and a slot's badge say of their binding, and in which ink.
#[allow(clippy::type_complexity)]
pub fn key_texts(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut texts: Query<(&KeyText, &mut Text, &mut TextColor), Without<SlotKey>>,
    mut badges: Query<(&SlotBadge, &mut BackgroundColor)>,
    mut slot_texts: Query<(&SlotKey, &mut Text, &mut TextColor), Without<KeyText>>,
) {
    if !screen.open {
        return;
    }
    for (key, mut text, mut ink) in &mut texts {
        let state = key_state(&screen, &settings, key.0);
        let (words, color) = match state {
            KeyState::Setting => ("Press a key…".to_owned(), theme::KEY_SETTING.2),
            KeyState::Clash => (chord_name(&settings, key.0), theme::KEY_CLASH.2),
            KeyState::Unbound => ("—".to_owned(), theme::INK_6),
            KeyState::Bound => (chord_name(&settings, key.0), theme::INK_1),
        };
        text.set_if_neq(Text(words));
        ink.set_if_neq(TextColor(color));
    }
    for (badge, mut ground) in &mut badges {
        let color = match key_state(&screen, &settings, badge.0) {
            KeyState::Setting => theme::FOCUS,
            KeyState::Clash => theme::KEY_CLASH.1,
            KeyState::Unbound => Color::srgba(0.0, 0.0, 0.0, 0.75),
            KeyState::Bound => Color::srgba(0.0, 0.0, 0.0, 0.8),
        };
        ground.set_if_neq(BackgroundColor(color));
    }
    for (key, mut text, mut ink) in &mut slot_texts {
        let (words, color) = match key_state(&screen, &settings, key.0) {
            KeyState::Setting => ("…".to_owned(), BADGE_SETTING_INK),
            KeyState::Clash => (chord_name(&settings, key.0), theme::INK_1),
            KeyState::Unbound => ("—".to_owned(), theme::INK_6),
            KeyState::Bound => (chord_name(&settings, key.0), theme::INK_1),
        };
        text.set_if_neq(Text(words));
        ink.set_if_neq(TextColor(color));
    }
}

/// The bar of clashes: shown, with what clashes, while some bindings share a key.
#[allow(clippy::type_complexity)]
pub fn clash_bar(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut bar: Query<&mut Node, With<ClashBar>>,
    mut bar_text: Query<&mut Text, With<ClashText>>,
) {
    if !screen.open {
        return;
    }
    // Told where the bindings are in view, not under the video options.
    let line = if screen.is_searching() || screen.tab == Tab::Hotkeys {
        clash_line(&settings)
    } else {
        String::new()
    };
    for mut node in &mut bar {
        let display = if line.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut text in &mut bar_text {
        text.set_if_neq(Text(line.clone()));
    }
}

/// Typed lines: the line, a caret and a counter while typed into, what holds while it is empty.
#[allow(clippy::type_complexity)]
pub fn names(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mut fields: Query<(&NameField, &mut BorderColor)>,
    mut texts: Query<
        (&NameText, &mut Text, &mut TextColor),
        (Without<NameCount>, Without<NameCaret>),
    >,
    mut carets: Query<(&NameCaret, &mut Node)>,
    mut counts: Query<(&NameCount, &mut Text), Without<NameText>>,
) {
    if !screen.open {
        return;
    }
    for (field, mut edge) in &mut fields {
        let typing = screen.naming == Some(field.0);
        edge.set_if_neq(BorderColor::all(if typing {
            theme::FOCUS
        } else {
            theme::FIELD_EDGE
        }));
    }
    for (name, mut text, mut ink) in &mut texts {
        let line = name.0.get(&settings);
        let (words, color) = if screen.naming == Some(name.0) || !line.is_empty() {
            (line.clone(), theme::INK_1)
        } else {
            (name.0.placeholder().to_owned(), theme::INK_5)
        };
        text.set_if_neq(Text(words));
        ink.set_if_neq(TextColor(color));
    }
    for (caret, mut node) in &mut carets {
        let display = if screen.naming == Some(caret.0) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (count, mut text) in &mut counts {
        let words = if screen.naming == Some(count.0) {
            format!(
                "{} / {}",
                count.0.get(&settings).chars().count(),
                count.0.max()
            )
        } else {
            String::new()
        };
        text.set_if_neq(Text(words));
    }
}

/// The links of the navigation: lit while their section is the one in view, or listed by a search.
pub fn nav(
    screen: Res<Screen>,
    in_view: Res<InView>,
    mut links: Query<(&NavLink, &mut BorderColor)>,
    mut words: Query<(&NavText, &mut TextColor)>,
) {
    if !screen.open {
        return;
    }
    let lit = |at: usize| screen.is_searching() || in_view.0 == at;
    for (link, mut edge) in &mut links {
        let color = if lit(link.card) {
            theme::SELECT
        } else {
            theme::NAV_RULE
        };
        edge.set_if_neq(BorderColor::all(color));
    }
    for (text, mut ink) in &mut words {
        let color = if lit(text.0) {
            theme::INK_1
        } else {
            theme::NAV_INK
        };
        ink.set_if_neq(TextColor(color));
    }
}
