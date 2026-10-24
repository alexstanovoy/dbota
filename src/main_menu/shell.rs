//! The menu inside the Bevy app: the [`MenuCore`] pumped once a frame, its
//! games opened and put on screen in place of the idle session, the
//! keyboard routed to its fields, and the way back when a game ends.

use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::{
        ButtonState, InputSystems,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    window::Ime,
};
use bota_client_core::{App as Core, Source};

use super::core::{Act, Launch, MenuCore, Page};
use super::launch::{Hosted, Idle, Opening, Trust};
use super::view::MenuView;
use crate::session::Session;

/// Bumped whenever another game takes the screen, so what was drawn of the
/// last one can be let go.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameEpoch(pub u32);

/// A game being opened.
#[derive(Default)]
struct Pending(Option<Opening>);

/// The game server of the local lobby this client started, ended with it
/// (or when another is started).
#[derive(Default)]
struct Host(Option<Hosted>);

/// A canned view drawn instead of the live one (`--menu-preview`).
#[derive(Resource)]
pub struct Preview(pub MenuView);

/// The certificates the menu's connections trust.
pub struct Trusted(pub Trust);

pub fn plugin(app: &mut App) {
    app.init_resource::<GameEpoch>()
        .insert_non_send(Pending::default())
        .insert_non_send(Host::default())
        .add_systems(
            PreUpdate,
            (escape, guard)
                .chain()
                .after(InputSystems)
                .after(crate::keys::fire)
                .after(crate::settings::escape)
                .before(crate::session::pump),
        )
        .add_systems(Startup, super::screens::spawn_roots)
        .add_systems(
            Update,
            (
                keyboard,
                settings_screen,
                super::screens::clicks,
                drive,
                super::screens::draw,
                super::screens::hover,
                scroll,
            )
                .chain(),
        );
}

/// The wheel scrolls a long page; the place is kept while the page is
/// drawn anew (a page is respawned whenever what it shows changes), and
/// another page starts at its top.
fn scroll(
    core: Option<NonSend<MenuCore>>,
    wheel: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut bodies: Query<(&mut ScrollPosition, &ComputedNode), With<super::pages::MenuBody>>,
    mut at: Local<(f32, Option<Page>)>,
) {
    use bevy::input::mouse::MouseScrollUnit;
    let Some(core) = core else {
        return;
    };
    let page = core.page();
    if at.1 != Some(page) {
        *at = (0.0, Some(page));
    }
    let step = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * 40.0,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    at.0 = (at.0 - step).max(0.0);
    for (mut scrolled, node) in &mut bodies {
        // As far as the page reaches past the screen, once laid out.
        let reach = (node.content_size().y - node.size().y) * node.inverse_scale_factor();
        if node.size().y > 0.0 {
            at.0 = at.0.min(reach.max(0.0));
        }
        if scrolled.y != at.0 {
            scrolled.y = at.0;
        }
    }
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

/// Escape over the menu backs out of it, through its binding; the menu
/// takes every Escape while it covers the game, so none reaches the game.
fn escape(core: Option<NonSendMut<MenuCore>>, mut fired: ResMut<crate::keys::Fired>) {
    let Some(mut core) = core else {
        return;
    };
    let escape = crate::keys::Action::Escape;
    if core.shows_game() || !fired.pressed(escape) {
        return;
    }
    fired.pressed.retain(|action| *action != escape);
    core.act(Act::Back);
}

/// Takes in the service's news, opens the games the menu asks for, and ends
/// the client when the player said to quit: through `AppExit`, the way a
/// refused replay ends it, so the app winds down its render thread first.
#[allow(clippy::too_many_arguments)]
fn drive(
    core: Option<NonSendMut<MenuCore>>,
    trust: Option<NonSend<Trusted>>,
    mut pending: NonSendMut<Pending>,
    mut host: NonSendMut<Host>,
    mut session: NonSendMut<Session>,
    mut epoch: ResMut<GameEpoch>,
    mut commands: Commands,
    mut index: ResMut<crate::units::UnitIndex>,
    args: Res<crate::Args>,
    settings: Res<crate::config::Settings>,
    mut exit: MessageWriter<AppExit>,
) {
    let (Some(mut core), Some(trust)) = (core, trust) else {
        return;
    };
    if core.take_quit() {
        // The lobby this client hosts ends with it.
        host.0 = None;
        exit.write(AppExit::Success);
        return;
    }
    core.set_profile_name(&settings.name);
    core.set_bot_command(&settings.bot_command);
    core.update();
    let mut opened_here = |core: &mut MenuCore, opened: std::io::Result<Session>| match opened {
        Ok(opened) => {
            *session = opened;
            epoch.0 += 1;
            forget_units(&mut commands, &mut index);
            core.game_started();
        }
        Err(err) => core.launch_failed(&err.to_string()),
    };
    match core.take_launch() {
        // The command line's game again: as it was opened at the start.
        Some(Launch::Given) => opened_here(&mut core, crate::session::open(&args)),
        Some(Launch::Replay(path)) => opened_here(&mut core, open_replay(&path)),
        Some(launch) => {
            if matches!(launch, Launch::Host { .. }) {
                // One lobby hosted at a time: the last one's server ends.
                host.0 = None;
            }
            pending.0 = Some(Opening::start(launch, &trust.0));
        }
        None => {}
    }
    let Some(opened) = pending.0.as_mut().and_then(Opening::poll) else {
        return;
    };
    pending.0 = None;
    match opened {
        Ok(opened) => {
            if opened.server.is_some() {
                host.0 = opened.server;
            }
            opened_here(&mut core, Ok(Session(Core::new(Source::Live(opened.link)))));
        }
        Err(err) => core.launch_failed(&err.to_string()),
    }
}

/// A replay file opened from the menu, watched through every eyes.
fn open_replay(path: &std::path::Path) -> std::io::Result<Session> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let player = bota_client_core::ReplayPlayer::load(path)?;
        Ok(Session(Core::new(Source::Replay(Box::new(player)))))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        Err(std::io::Error::other("a browser opens no files"))
    }
}

/// Keeps the game's keys quiet while the menu covers the screen, and brings
/// the menu back when the game on screen is lost or the player leaves it
/// (Escape in a game that is over, DISCONNECT on the dashboard).
#[allow(clippy::too_many_arguments)]
fn guard(
    core: Option<NonSendMut<MenuCore>>,
    mut session: NonSendMut<Session>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut fired: ResMut<crate::keys::Fired>,
    panels: Res<crate::input::Panels>,
    screen: Res<crate::settings::Screen>,
    mut epoch: ResMut<GameEpoch>,
    mut commands: Commands,
    mut index: ResMut<crate::units::UnitIndex>,
) {
    let Some(mut core) = core else {
        return;
    };
    if core.take_abandon() {
        session.0.leave_game();
    }
    let game = &session.0;
    let seated = game.my_slot.is_some() && matches!(game.source, Source::Live(_));
    core.set_game_unsafe(seated && game.over.is_none() && !game.lost);
    let disconnect = core.take_leave();
    let shown = core.shows_game();
    if !shown {
        keys.reset_all();
    }
    // A game under the dashboard is let go when lost too.
    if !core.in_game() {
        return;
    }
    // A replay opened from the menu that cannot be read is left, saying why
    // (one named on the command line says so and stays, as it always has).
    let unreadable = match &game.source {
        Source::Replay(player) if core.replay_from_menu() => player.error().map(|e| e.to_string()),
        _ => None,
    };
    let lost = (game.lost && !game.source.is_redialing()) || unreadable.is_some();
    let why = lost.then(|| match &unreadable {
        Some(err) => format!("The replay cannot be read: {err}"),
        None => game.feed.last().filter(|_| game.view().is_none()).map_or(
            "the connection to the game server was lost".to_owned(),
            |l| l.text.clone(),
        ),
    });
    let over = game.over.is_some();
    let done = lost || over;
    let escape = crate::keys::Action::Escape;
    let leave =
        disconnect || (shown && fired.pressed(escape) && done && !screen.open && !panels.shop);
    if leave || (lost && !over) {
        // Escape leaves the game here, not the client too.
        fired.pressed.retain(|action| *action != escape);
        *session = idle();
        epoch.0 += 1;
        forget_units(&mut commands, &mut index);
        core.left_game(if leave { None } else { why.as_deref() }, over);
    }
}

/// Opens the settings screen when the menu goes to its page, and goes back
/// to the page it came from when the screen closes.
fn settings_screen(
    core: Option<NonSendMut<MenuCore>>,
    mut screen: ResMut<crate::settings::Screen>,
    mut was_open: Local<bool>,
) {
    let Some(mut core) = core else {
        return;
    };
    if core.page() == Page::Settings {
        if *was_open && !screen.open {
            core.leave_settings();
        } else if !screen.open {
            screen.open = true;
        }
    }
    *was_open = screen.open;
}

/// The most of a paste taken into a field, in characters; the field cuts it to its own length.
const MAX_PASTE: usize = 4096;

/// Types into the menu's fields while it covers the screen: keys, text an input method or a
/// script commits, and a paste (Ctrl+V, or Shift+Insert).
#[allow(clippy::too_many_arguments)]
fn keyboard(
    core: Option<NonSendMut<MenuCore>>,
    mut events: MessageReader<KeyboardInput>,
    mut composing: MessageReader<Ime>,
    held: Res<ButtonInput<KeyCode>>,
    mut clipboard: ResMut<Clipboard>,
    mut pasting: Local<Option<ClipboardRead>>,
) {
    let Some(mut core) = core else {
        events.clear();
        composing.clear();
        *pasting = None;
        return;
    };
    if matches!(core.page(), Page::Game | Page::Settings) {
        events.clear();
        composing.clear();
        *pasting = None;
        return;
    }
    let control = held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = held.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let paste = (control && event.key_code == KeyCode::KeyV)
            || (shift && event.key_code == KeyCode::Insert);
        if paste {
            *pasting = Some(clipboard.fetch_text());
            continue;
        }
        let act = match &event.logical_key {
            Key::Enter => Act::Submit,
            Key::Tab => Act::NextField,
            Key::Backspace => Act::Erase,
            Key::Space => Act::Type(" ".to_owned()),
            Key::Character(_) if control => continue,
            Key::Character(typed) => Act::Type(typed.to_string()),
            _ => continue,
        };
        core.act(act);
    }
    for event in composing.read() {
        if let Ime::Commit { value, .. } = event {
            core.act(Act::Type(value.chars().take(MAX_PASTE).collect()));
        }
    }
    if let Some(read) = pasting.as_mut()
        && let Some(result) = read.poll_result()
    {
        *pasting = None;
        if let Ok(text) = result {
            core.act(Act::Type(text.chars().take(MAX_PASTE).collect()));
        }
    }
}
