//! The menu inside the Bevy app: the [`MenuCore`] pumped once a frame, its
//! games opened and put on screen in place of the idle session, the
//! keyboard routed to its fields, and the way back when a game ends.

use bevy::{
    input::{
        ButtonState, InputSystems,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
};
use bota_client_core::{App as Core, Phase, Source};

use super::core::{Act, MenuCore, Page};
use super::launch::{Idle, Opening, Trust};
use super::view::MenuView;
use crate::session::Session;

/// Bumped whenever another game takes the screen, so what was drawn of the
/// last one can be let go.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameEpoch(pub u32);

/// A game being opened.
#[derive(Default)]
struct Pending(Option<Opening>);

/// A canned view drawn instead of the live one (`--menu-preview`).
#[derive(Resource)]
pub struct Preview(pub MenuView);

/// The certificates the menu's connections trust.
pub struct Trusted(pub Trust);

pub fn plugin(app: &mut App) {
    app.init_resource::<GameEpoch>()
        .insert_non_send(Pending::default())
        .add_systems(
            PreUpdate,
            guard.after(InputSystems).before(crate::session::pump),
        )
        .add_systems(Startup, super::screens::spawn_roots)
        .add_systems(
            Update,
            (
                keyboard,
                super::screens::clicks,
                drive,
                super::screens::draw,
                super::screens::hover,
            )
                .chain(),
        );
}

/// Lets go of every unit drawn of the last game: the next numbers its units
/// afresh.
fn forget_units(commands: &mut Commands, index: &mut crate::units::UnitIndex) {
    for (_, entity) in index.0.drain() {
        commands.entity(entity).despawn();
    }
}

/// A session with nothing on it, for while the menu is up.
pub fn idle() -> Session {
    Session(Core::new(Source::Live(Box::new(Idle))))
}

/// Takes in the service's news and opens the games the menu asks for.
fn drive(
    core: Option<NonSendMut<MenuCore>>,
    trust: Option<NonSend<Trusted>>,
    mut pending: NonSendMut<Pending>,
    mut session: NonSendMut<Session>,
    mut epoch: ResMut<GameEpoch>,
    mut commands: Commands,
    mut index: ResMut<crate::units::UnitIndex>,
) {
    let (Some(mut core), Some(trust)) = (core, trust) else {
        return;
    };
    core.update();
    if let Some(launch) = core.take_launch() {
        pending.0 = Some(Opening::start(launch, &trust.0));
    }
    let Some(opened) = pending.0.as_mut().and_then(Opening::poll) else {
        return;
    };
    pending.0 = None;
    match opened {
        Ok(link) => {
            *session = Session(Core::new(Source::Live(link)));
            epoch.0 += 1;
            forget_units(&mut commands, &mut index);
            core.game_started();
        }
        Err(err) => core.launch_failed(&err.to_string()),
    }
}

/// Keeps the game's keys quiet while the menu covers the screen, and brings
/// the menu back when the game on screen is lost or the player leaves it.
fn guard(
    core: Option<NonSendMut<MenuCore>>,
    mut session: NonSendMut<Session>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    panels: Res<crate::input::Panels>,
    mut epoch: ResMut<GameEpoch>,
    mut commands: Commands,
    mut index: ResMut<crate::units::UnitIndex>,
) {
    let Some(mut core) = core else {
        return;
    };
    if core.page() != Page::Game || core.launching {
        keys.reset_all();
        return;
    }
    let game = &session.0;
    let lost = game.lost && !game.source.is_redialing();
    let why = lost.then(|| {
        game.feed.last().filter(|_| game.view().is_none()).map_or(
            "the connection to the game server was lost".to_owned(),
            |l| l.text.clone(),
        )
    });
    let done = lost || game.over.is_some() || (game.phase == Phase::Lobby && game.view().is_none());
    let leave = keys.just_pressed(KeyCode::Escape) && done && !panels.settings && !panels.shop;
    if leave || (lost && game.over.is_none()) {
        keys.clear_just_pressed(KeyCode::Escape);
        *session = idle();
        epoch.0 += 1;
        forget_units(&mut commands, &mut index);
        core.left_game(if leave { None } else { why.as_deref() });
    }
}

/// Types into the menu's fields while it covers the screen.
fn keyboard(core: Option<NonSendMut<MenuCore>>, mut events: MessageReader<KeyboardInput>) {
    let Some(mut core) = core else {
        events.clear();
        return;
    };
    if core.page() == Page::Game {
        events.clear();
        return;
    }
    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let act = match &event.logical_key {
            Key::Enter => Act::Submit,
            Key::Tab => Act::NextField,
            Key::Backspace => Act::Erase,
            Key::Escape => Act::Dismiss,
            Key::Space => Act::Type(" ".to_owned()),
            Key::Character(typed) => Act::Type(typed.to_string()),
            _ => continue,
        };
        core.act(act);
    }
}
