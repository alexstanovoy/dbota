//! Queue admission facts, rendered without exposing service counts.
use super::{
    core::Act,
    kit::Kit,
    view::{MenuView, PageView},
};
use crate::theme;
use bevy::prelude::*;
use bota_radovid::proto::AccountGameLimit;

use bevy::picking::hover::{DirectlyHovered, Hovered};
use bevy::ui_widgets::Button;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Full,
    Cooldown,
    Games,
    Selection,
}
impl Reason {
    pub fn text(self) -> &'static str {
        match self {
            Self::Full => "Matchmaking is full",
            Self::Cooldown => "You declined a match",
            Self::Games => "You are in a game already",
            Self::Selection => "Select fewer game modes",
        }
    }
}

pub fn reason(view: &MenuView) -> Option<Reason> {
    if !view.play.open
        || view.play.lobbies
        || view.account.is_none()
        || view.offer.is_some()
        || !view.finding().0.is_empty()
    {
        return None;
    }
    if view.cooldown_left.is_some() {
        return Some(Reason::Cooldown);
    }
    if view.queue_full {
        return Some(Reason::Full);
    }
    let PageView::Home {
        rulesets,
        in_flight,
        ..
    } = &view.body
    else {
        return None;
    };
    let rows: Vec<_> = rulesets
        .iter()
        .filter(|row| row.rated == view.play.ranked)
        .collect();
    let selected = rows
        .iter()
        .filter(|row| view.play.chosen(row, rows.first().copied()))
        .count();
    match in_flight.1 {
        Some(AccountGameLimit::Limited(maximum))
            if selected > 0 && selected > usize::from(maximum).saturating_sub(in_flight.0) =>
        {
            Some(if in_flight.0 == 0 {
                Reason::Selection
            } else {
                Reason::Games
            })
        }
        _ => None,
    }
}

pub fn return_action(view: &MenuView) -> Option<Act> {
    if view.game.is_some() {
        return Some(Act::ReturnToGame);
    }
    (reason(view) == Some(Reason::Games)).then_some(if view.reconnect {
        Act::Reconnect
    } else {
        Act::Go(super::core::Page::MyGames)
    })
}

pub fn strip(kit: &mut Kit, root: Entity, view: &MenuView, reason: Reason) {
    let strip = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            right: px(theme::CORNER_RIGHT),
            bottom: px(theme::CORNER_BOTTOM + theme::ACTION_HEIGHT + theme::CORNER_GAP),
            width: px(theme::CORNER_WIDTH),
            height: px(theme::FINDING_HEIGHT),
            padding: UiRect::axes(px(14), px(0)),
            align_items: AlignItems::Center,
            column_gap: px(12),
            border: UiRect::axes(px(0), px(1)),
            ..default()
        },
    );
    kit.commands.entity(strip).insert((
        Name::new("matchmaking unavailable"),
        BackgroundColor(theme::FINDING),
        BorderColor::all(theme::SECONDARY_EDGE),
    ));
    if matches!(reason, Reason::Full | Reason::Cooldown) {
        icon(kit, strip, reason);
    }
    kit.typed(strip, reason.text(), theme::BODY, theme::INK_2);
    if reason == Reason::Cooldown {
        kit.node(
            strip,
            Node {
                flex_grow: 1.0,
                ..default()
            },
        );
        if let Some(clock) = &view.cooldown_left {
            kit.typed(strip, clock, theme::FINDING_CLOCK, theme::INK_1);
        }
    }
}

#[derive(Component)]
pub struct ReasonTip(Entity);

pub fn explain(kit: &mut Kit, button: Entity, reason: Reason) {
    let tip = super::play::tooltip(kit, button, reason.text(), false);
    let mut accessible = bevy::a11y::AccessibilityNode::default();
    accessible.set_label("FIND MATCH");
    accessible.set_description(reason.text());
    accessible.set_disabled();
    kit.commands.entity(tip).insert(Visibility::Hidden);
    kit.commands.entity(button).insert((
        Button,
        Hovered::default(),
        DirectlyHovered::default(),
        ReasonTip(tip),
        accessible,
        bevy::input_focus::tab_navigation::TabIndex(0),
        theme::downward(theme::QUEUE_DISABLED),
    ));
}

/// The same reason is available by pointer, Tab and the accessibility description.
pub fn tips(
    focus: Res<bevy::input_focus::InputFocus>,
    buttons: Query<(Entity, &Hovered, &ReasonTip, &Children)>,
    mut tips: Query<&mut Visibility>,
    mut labels: Query<&mut TextColor>,
) {
    for (entity, interaction, tip, children) in &buttons {
        for child in children {
            if let Ok(mut color) = labels.get_mut(*child) {
                color.set_if_neq(TextColor(theme::INK_4));
            }
        }
        if let Ok(mut shown) = tips.get_mut(tip.0) {
            shown.set_if_neq(crate::ui::visible(
                focus.get() == Some(entity) || interaction.get(),
            ));
        }
    }
}

#[derive(Resource, Default)]
pub struct FocusKey(Option<String>);

/// Keep focus on the same named control when a countdown redraws the menu.
pub fn remember_focus(
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    mut key: ResMut<FocusKey>,
    press: Res<crate::ui::PrimaryPress>,
    nodes: Query<
        (Entity, &Name, Option<&super::kit::MenuButton>),
        With<bevy::input_focus::tab_navigation::TabIndex>,
    >,
    groups: Query<Entity, With<bevy::input_focus::tab_navigation::TabGroup>>,
    mut core: Option<NonSendMut<super::core::MenuCore>>,
) {
    if groups.is_empty() {
        return;
    }
    if let Some((entity, name, button)) = press.0.and_then(|target| nodes.get(target).ok()) {
        focus.set(entity, bevy::input_focus::FocusCause::Pressed);
        key.0 = Some(name.to_string());
        if !matches!(button, Some(super::kit::MenuButton(Act::Focus(_))))
            && let Some(core) = core.as_mut()
        {
            core.blur_menu_focus();
        }
    } else if let Some(entity) = focus.get()
        && let Ok((_, name, _)) = nodes.get(entity)
    {
        key.0 = Some(name.to_string());
    }
}

#[derive(Resource, Default)]
pub struct MenuKeys {
    tab: bool,
    activate: bool,
    shift: bool,
    was_typing: bool,
    controller: bool,
}

pub fn remember_typing(typing: Res<crate::input::Typing>, mut keys: ResMut<MenuKeys>) {
    keys.was_typing = *typing != crate::input::Typing::No;
}

/// Capture before the dashboard suppresses the game's raw keys.
pub fn capture_keys(
    input: Res<ButtonInput<KeyCode>>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    mut keys: ResMut<MenuKeys>,
) {
    keys.controller = false;
    keys.tab = input.just_pressed(KeyCode::Tab);
    keys.activate = input.just_pressed(KeyCode::Enter) || input.just_pressed(KeyCode::Space);
    keys.shift = input.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    use bevy::input::gamepad::GamepadButton;
    for pad in pads.iter().take(8) {
        let next =
            pad.just_pressed(GamepadButton::DPadDown) || pad.just_pressed(GamepadButton::DPadRight);
        let previous =
            pad.just_pressed(GamepadButton::DPadUp) || pad.just_pressed(GamepadButton::DPadLeft);
        keys.tab |= next || previous;
        keys.shift |= previous;
        let activate = pad.just_pressed(GamepadButton::South);
        keys.activate |= activate;
        keys.controller |= next || previous || activate;
    }
}

pub fn navigate(
    keys: Res<MenuKeys>,
    typing: Res<crate::input::Typing>,
    navigation: bevy::input_focus::tab_navigation::TabNavigation,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    mut key: ResMut<FocusKey>,
    nodes: Query<
        (Entity, &Name, Option<&super::kit::MenuButton>),
        With<bevy::input_focus::tab_navigation::TabIndex>,
    >,
    groups: Query<(Entity, &bevy::input_focus::tab_navigation::TabGroup)>,
    parents: Query<&ChildOf>,
    mut press: ResMut<crate::ui::PrimaryPress>,
    result_buttons: Query<(), With<crate::postgame::view::Button>>,
    mut core: Option<NonSendMut<super::core::MenuCore>>,
) {
    if groups.is_empty() {
        // Lobby forms and dialogs keep menu F's existing ownership and focus reducer.
        if keys.controller
            && !keys.was_typing
            && *typing == crate::input::Typing::No
            && let Some(core) = core.as_mut()
            && matches!(
                core.page(),
                super::core::Page::Lobby | super::core::Page::Lobbies
            )
        {
            if keys.tab {
                core.act(if keys.shift {
                    Act::PreviousField
                } else {
                    Act::NextField
                });
            } else if keys.activate {
                core.act(Act::Submit);
            }
        }
        if key.0.take().is_some() {
            focus.clear();
        }
        return;
    }
    if focus.get().is_some_and(|entity| nodes.get(entity).is_err())
        && let Some(name) = &key.0
        && let Some((entity, _, _)) = nodes
            .iter()
            .find(|(_, candidate, _)| candidate.as_str() == name)
    {
        focus.set(entity, bevy::input_focus::FocusCause::Navigated);
    }
    if !keys.tab && (keys.was_typing || *typing != crate::input::Typing::No) {
        return;
    }
    if keys.tab {
        use bevy::input_focus::tab_navigation::NavAction;
        let action = if keys.shift {
            NavAction::Previous
        } else {
            NavAction::Next
        };
        let modal = groups
            .iter()
            .filter(|(_, group)| group.modal)
            .max_by_key(|(_, group)| group.order)
            .map(|(entity, _)| entity);
        let initialize = modal.filter(|modal| {
            !focus.get().is_some_and(|entity| {
                parents
                    .iter_ancestors(entity)
                    .take(64)
                    .any(|parent| parent == *modal)
            })
        });
        let navigation = match initialize {
            Some(modal) => navigation.initialize(modal, action),
            None => navigation.navigate(&focus, action),
        };
        let next = match navigation {
            Ok(next)
            | Err(
                bevy::input_focus::tab_navigation::TabNavigationError::NoTabGroupForCurrentFocus {
                    new_focus: next,
                    ..
                },
            ) => Some(next),
            _ => None,
        };
        if let Some(next) = next {
            focus.set(next, bevy::input_focus::FocusCause::Navigated);
            key.0 = nodes.get(next).ok().map(|(_, name, _)| name.to_string());
            if let Some(core) = core.as_mut() {
                match nodes.get(next).ok().and_then(|(_, _, button)| button) {
                    Some(super::kit::MenuButton(Act::Focus(at))) => core.act(Act::Focus(*at)),
                    _ => core.blur_menu_focus(),
                }
            }
        }
    } else if keys.activate
        && let Some(entity) = focus.get()
    {
        if result_buttons.contains(entity) {
            press.0 = Some(entity);
        } else if let Ok((_, _, Some(button))) = nodes.get(entity)
            && let Some(mut core) = core
        {
            core.act(button.0.clone());
        }
    }
}

fn icon(kit: &mut Kit, parent: Entity, reason: Reason) {
    let circle = kit.node(
        parent,
        Node {
            width: px(16),
            height: px(16),
            flex_shrink: 0.0,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
    );
    kit.commands
        .entity(circle)
        .insert(BorderColor::all(theme::INK_3));
    let strokes = if reason == Reason::Full {
        [(4.0, 4.0, 2.0, 6.0), (8.0, 4.0, 2.0, 6.0)]
    } else {
        [(6.0, 3.0, 1.0, 5.0), (6.0, 7.0, 4.0, 1.0)]
    };
    for (left, top, width, height) in strokes {
        let stroke = kit.node(
            circle,
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(width),
                height: px(height),
                ..default()
            },
        );
        kit.commands
            .entity(stroke)
            .insert(BackgroundColor(theme::INK_3));
    }
}
