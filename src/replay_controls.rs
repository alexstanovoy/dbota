//! A replay's playback controls, as Dota's replays have on screen: pause or
//! play, slower and faster with the speed between them, and the vision
//! watched, each with its key (the bindings' own, `F9`, `-`, `=`, `Tab` by
//! default). The keys did all of it before; the strip shows that they did and
//! what they did.

use bevy::prelude::*;
use bota_client_core::{Intent, Source};

use crate::keys::Action;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// The strip.
#[derive(Component)]
struct Strip;

/// A button and what it does.
#[derive(Component, Clone, Copy)]
struct Control(Action);

/// A button's label, said anew each frame.
#[derive(Component)]
struct Label(Action);

/// The speed between the buttons.
#[derive(Component)]
struct Speed;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, (presses, show).chain());
}

/// Below the kill and last-hit box at the top left.
const TOP: f32 = 64.0;

fn spawn(mut commands: Commands, fonts: Res<Fonts>) {
    let strip = commands
        .spawn((
            Strip,
            Node {
                position_type: PositionType::Absolute,
                top: px(TOP),
                left: px(8),
                padding: UiRect::all(px(6)),
                column_gap: px(6),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(ui::PANEL),
            BorderColor::all(ui::EDGE),
            Visibility::Hidden,
        ))
        .id();
    let button = |commands: &mut Commands, action: Action| {
        let button = commands
            .spawn((
                Control(action),
                Button,
                Node {
                    height: px(28),
                    padding: UiRect::axes(px(10), px(0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.13, 0.15, 0.19)),
                ChildOf(strip),
            ))
            .id();
        commands.spawn((
            Label(action),
            ui::text("", &fonts.bold, 13.0, ui::INK),
            Pickable::IGNORE,
            ChildOf(button),
        ));
    };
    button(&mut commands, Action::Pause);
    button(&mut commands, Action::Slower);
    commands.spawn((
        Speed,
        ui::text("", &fonts.bold, 15.0, ui::GOLDEN),
        Node {
            min_width: px(44),
            justify_content: JustifyContent::Center,
            ..default()
        },
        TextLayout::justify(Justify::Center),
        Pickable::IGNORE,
        ChildOf(strip),
    ));
    button(&mut commands, Action::Faster);
    button(&mut commands, Action::NextEyes);
}

/// A click does what the button's key does.
fn presses(
    mut session: NonSendMut<Session>,
    buttons: Query<(&Interaction, &Control), Changed<Interaction>>,
) {
    for (interaction, control) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let intent = match control.0 {
            Action::Pause => Intent::TogglePause,
            Action::Slower => Intent::Slower,
            Action::Faster => Intent::Faster,
            _ => Intent::NextEyes,
        };
        session.0.act(intent);
    }
}

/// What the strip says: shown while a replay plays and the game is on
/// screen.
#[allow(clippy::type_complexity)]
fn show(
    session: NonSend<Session>,
    menu: Option<NonSend<crate::main_menu::MenuCore>>,
    settings: Res<crate::config::Settings>,
    grid: Res<crate::fog::FogGrid>,
    mut strip: Single<&mut Visibility, With<Strip>>,
    mut texts: Query<(&mut Text, Option<&Label>), Or<(With<Label>, With<Speed>)>>,
) {
    let core = &session.0;
    let Source::Replay(player) = &core.source else {
        strip.set_if_neq(Visibility::Hidden);
        return;
    };
    let on_screen = menu.is_none_or(|m| m.shows_game());
    strip.set_if_neq(ui::visible(on_screen));
    if !on_screen {
        return;
    }
    let key = |action: Action| {
        crate::keys::index_of(action)
            .and_then(|at| settings.keys[at])
            .map_or(String::new(), |chord| format!(" [{}]", chord.name()))
    };
    for (mut text, label) in &mut texts {
        let said = match label.map(|l| l.0) {
            Some(Action::Pause) if player.paused => format!("PLAY{}", key(Action::Pause)),
            Some(Action::Pause) => format!("PAUSE{}", key(Action::Pause)),
            Some(Action::Slower) => format!("SLOWER{}", key(Action::Slower)),
            Some(Action::Faster) => format!("FASTER{}", key(Action::Faster)),
            Some(_) => format!("VISION: {}{}", vision(&grid), key(Action::NextEyes)),
            None if player.paused => "PAUSED".to_owned(),
            None => speed(player.speed),
        };
        if text.0 != said {
            text.0 = said;
        }
    }
}

/// Whose eyes the replay is seen with.
fn vision(grid: &crate::fog::FogGrid) -> String {
    match (grid.on, grid.team) {
        (true, None) => "BOTH TEAMS".to_owned(),
        (true, Some(team)) => format!("{team:?}").to_uppercase(),
        (false, _) => "ALL".to_owned(),
    }
}

/// `1x`, `0.25x`, `16x`.
fn speed(speed: f32) -> String {
    let shown = format!("{speed:.2}");
    let shown = shown.trim_end_matches('0').trim_end_matches('.');
    format!("{shown}x")
}
