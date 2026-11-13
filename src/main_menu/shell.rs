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
use bota_client_core::{App as Core, Phase, Source};

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

/// LEAVE telling a game whose seat was left that the match is left for good: a leave of its own,
/// apart from any game session.
#[derive(Default)]
struct Leaver {
    #[cfg(not(target_arch = "wasm32"))]
    op: Option<super::leave::LeaveOp>,
    /// A new local lobby waiting for the stopped leave of the old one to end: the new server is
    /// started only once nothing of the old game's leave can reach it.
    #[cfg(not(target_arch = "wasm32"))]
    waiting_host: Option<Launch>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Leaver {
    /// The launch to open now: a new local lobby waits while a leave is still going (it is the
    /// old lobby's, stopped by the menu), and comes back once it has ended.
    fn hold_host(&mut self, core: &mut MenuCore, launch: Option<Launch>) -> Option<Launch> {
        match launch {
            Some(host @ (Launch::Host { .. } | Launch::LocalDemo { .. })) if self.op.is_some() => {
                core.waiting_for_leave();
                self.waiting_host = Some(host);
                None
            }
            None if self.op.is_none() => self.waiting_host.take(),
            launch => launch,
        }
    }
}

/// A canned view drawn instead of the live one (`--menu-preview`).
#[derive(Resource)]
pub struct Preview(pub MenuView);

/// The certificates the menu's connections trust.
pub struct Trusted(pub Trust);

pub fn plugin(app: &mut App) {
    super::hero_stage::plugin(app);
    super::preview::plugin(app);
    app.init_resource::<super::lobby_scroll::Positions>()
        .init_resource::<GameEpoch>()
        .init_resource::<super::availability::FocusKey>()
        .init_resource::<super::availability::MenuKeys>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<super::top_bar::TabFold>()
        .insert_non_send(Pending::default())
        .insert_non_send(Host::default())
        .insert_non_send(Leaver::default())
        .add_systems(
            PreUpdate,
            (escape, guard)
                .chain()
                .after(InputSystems)
                .after(crate::keys::fire)
                .after(crate::settings::escape)
                .before(crate::session::pump),
        )
        .add_systems(First, super::availability::remember_typing)
        .add_systems(
            PreUpdate,
            super::availability::capture_keys
                .before(crate::postgame::controls::keyboard)
                .after(InputSystems)
                .before(guard),
        )
        .add_systems(Startup, super::screens::spawn_roots)
        .add_systems(
            Update,
            (
                keyboard,
                settings_screen,
                super::top_bar::more_clicks,
                super::availability::remember_focus,
                super::screens::clicks,
                drive,
                demo_start,
                super::screens::draw.in_set(crate::ui::WidgetBuild),
                super::kit::focus_ring,
                super::highlight::highlight,
                super::lobby_widgets::focus_ring,
                super::kit::hover.in_set(crate::ui::WidgetPaint),
                super::availability::navigate,
                super::availability::tips,
                scroll,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                super::top_bar::animate,
                hud_shown.after(drive).in_set(crate::ui::Mode),
            ),
        )
        .add_systems(
            PostUpdate,
            (
                super::top_bar::fit_tabs,
                super::ladder_page::visible_range,
                super::lobby_scroll::scroll,
            )
                .after(bevy::ui::UiSystems::Layout),
        );
}

/// The wheel scrolls a long page; the place is kept while the page is
/// drawn anew (a page is respawned whenever what it shows changes), and
/// another page starts at its top.
fn scroll(
    post: NonSend<crate::postgame::PostGame>,
    core: Option<NonSend<MenuCore>>,
    wheel: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut bodies: Query<(&mut ScrollPosition, &ComputedNode), With<super::pages::MenuBody>>,
    mut at: Local<(f32, Option<Page>)>,
) {
    use bevy::input::mouse::MouseScrollUnit;
    let Some(core) = core else {
        return;
    };
    if post.visible() {
        return;
    }
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
pub(crate) fn escape(
    core: Option<NonSendMut<MenuCore>>,
    post: NonSend<crate::postgame::PostGame>,
    mut fired: ResMut<crate::keys::Fired>,
) {
    let Some(mut core) = core else {
        return;
    };
    if post.visible() {
        return;
    }
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
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut, unused_variables))] mut leaver: NonSendMut<
        Leaver,
    >,
    mut session: NonSendMut<Session>,
    mut post: NonSendMut<crate::postgame::PostGame>,
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
    core.set_hosting(host.0.as_mut().is_some_and(Hosted::running));
    core.set_profile_name(&settings.name);
    core.set_replay_policy(settings.replays);
    core.update();
    #[cfg(not(target_arch = "wasm32"))]
    lead_leave(&mut core, &mut leaver, &trust.0, &args);
    if let Some(fetched) = core.take_meta() {
        post.provide(fetched);
    }
    let launch = core.take_launch();
    if let Some(launch) = &launch {
        post.expect(origin_of(launch, &args));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let launch = leaver.hold_host(&mut core, launch);
    let mut opener = Opener {
        session: &mut session,
        post: &mut post,
        epoch: &mut epoch,
        commands: &mut commands,
        index: &mut index,
    };
    match launch {
        // The command line's game again: as it was opened at the start.
        Some(Launch::Given) => opener.opened(&mut core, crate::session::open(&args)),
        Some(Launch::Replay(path)) => opener.opened(&mut core, open_replay(&path)),
        Some(Launch::Blob(blob)) => opener.opened(&mut core, open_blob(&blob)),
        Some(launch) => {
            if matches!(launch, Launch::Host { .. } | Launch::LocalDemo { .. }) {
                // One lobby hosted at a time: the last one's server ends.
                host.0 = None;
            }
            pending.0 = Some(Opening::start(launch, &trust.0));
        }
        None => {}
    }
    adopt(&mut core, &mut pending, &mut host, &mut opener);
}

/// The game being opened, once it is: on screen, its server (a local lobby's) kept.
fn adopt(core: &mut MenuCore, pending: &mut Pending, host: &mut Host, opener: &mut Opener) {
    let Some(opened) = pending.0.as_mut().and_then(Opening::poll) else {
        return;
    };
    pending.0 = None;
    match opened {
        Ok(opened) => {
            if opened.server.is_some() {
                host.0 = opened.server;
            }
            opener.opened(core, Ok(Session(Core::new(Source::Live(opened.link)))));
        }
        Err(err) => core.launch_failed(&err.to_string()),
    }
}

/// What a game opened takes over: the session, the summary to come, the epoch and the units of
/// the game before.
struct Opener<'a, 'w, 's> {
    session: &'a mut Session,
    post: &'a mut crate::postgame::PostGame,
    epoch: &'a mut GameEpoch,
    commands: &'a mut Commands<'w, 's>,
    index: &'a mut crate::units::UnitIndex,
}

impl Opener<'_, '_, '_> {
    /// The game is on screen, or the menu says why it could not be opened.
    fn opened(&mut self, core: &mut MenuCore, opened: std::io::Result<Session>) {
        match opened {
            Ok(opened) => {
                *self.session = opened;
                self.post.begin();
                self.epoch.0 += 1;
                forget_units(self.commands, self.index);
                core.game_started();
            }
            Err(err) => core.launch_failed(&err.to_string()),
        }
    }
}

/// Starts the leave the menu asks for, and hands it its outcome: the menu takes it only for the
/// leave under way, so one given up at its deadline is never taken.
#[cfg(not(target_arch = "wasm32"))]
fn lead_leave(core: &mut MenuCore, leaver: &mut Leaver, trust: &Trust, args: &crate::Args) {
    use super::leave::{LeaveOp, Outcome};
    if core.take_cancel_leave()
        && let Some(op) = &leaver.op
    {
        op.cancel();
    }
    if let Some((id, launch, until)) = core.take_leave_order() {
        match super::launch::leave_connector(&launch, trust, args) {
            Ok((connect, hello)) => leaver.op = Some(LeaveOp::start(id, connect, hello, until)),
            Err(error) => core.leave_done(id, Outcome::Unreachable(error.to_string())),
        }
    }
    if let Some(op) = &mut leaver.op
        && let Some(outcome) = op.poll()
    {
        core.leave_done(op.id, outcome);
        leaver.op = None;
    }
}

/// Where the summary of the game a launch opens is read from.
fn origin_of(launch: &Launch, args: &crate::Args) -> crate::postgame::Origin {
    use crate::postgame::Origin;
    match launch {
        Launch::Replay(path) => Origin::File(path.clone()),
        Launch::Blob(blob) => Origin::Blob(blob.0.clone()),
        Launch::Given => args.replay.clone().map_or(Origin::Nothing, Origin::File),
        Launch::Host {
            replay: Some(path), ..
        } => Origin::File(path.clone()),
        Launch::Demo { .. } | Launch::LocalDemo { .. } => Origin::Hidden,
        // The service's artifact of the match, which the menu fetches.
        Launch::Seat(handoff) => Origin::Service(handoff.match_id),
        Launch::Host { .. } | Launch::Watch(_) | Launch::Direct { .. } => Origin::Nothing,
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
        Err(std::io::Error::other("a browser opens no files by path"))
    }
}

/// A replay held in memory, watched through every eyes.
fn open_blob(blob: &super::core::Blob) -> std::io::Result<Session> {
    let player = bota_client_core::ReplayPlayer::from_reader(std::io::Cursor::new(blob.0.clone()));
    Ok(Session(Core::new(Source::Replay(Box::new(player)))))
}

/// Keeps the game's keys quiet while the menu covers the screen, and brings
/// the menu back when the game on screen is lost or the player leaves it
/// (Escape in a game that is over, DISCONNECT on the dashboard).
#[allow(clippy::too_many_arguments)]
fn guard(
    post: NonSend<crate::postgame::PostGame>,
    core: Option<NonSendMut<MenuCore>>,
    mut session: NonSendMut<Session>,
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
    if core.take_leave_game() {
        session.0.leave_game();
    }
    let game = &session.0;
    let seated = game.my_slot.is_some() && matches!(game.source, Source::Live(_));
    if seated && let Some(slot) = game.my_slot {
        core.set_local_recording_owner(slot);
    }
    core.set_game_unsafe(seated && game.over.is_none() && !game.lost);
    core.set_game_in_picks(seated && game.phase == bota_client_core::Phase::Lobby);
    let disconnect = core.take_leave();
    let shown = core.shows_game();
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
            |l| crate::feed::text(game, &l.kind),
        ),
    });
    let over = game.over.is_some();
    let done = lost || over;
    let escape = crate::keys::Action::Escape;
    let leave = disconnect
        || (shown
            && !post.visible()
            && fired.pressed(escape)
            && done
            && !screen.open
            && !panels.shop);
    if leave || (lost && !over) {
        // Escape leaves the game here, not the client too.
        fired.pressed.retain(|action| *action != escape);
        *session = idle();
        epoch.0 += 1;
        forget_units(&mut commands, &mut index);
        core.left_game(if leave { None } else { why.as_deref() }, over);
    }
}

/// Tells the interface's scale whether the game's HUD is on screen or the menu covers it.
fn hud_shown(
    post: NonSend<crate::postgame::PostGame>,
    core: Option<NonSend<MenuCore>>,
    preview: Option<Res<Preview>>,
    loading: Res<crate::loading::Loading>,
    mut hud: ResMut<crate::ui::HudShown>,
) {
    let shown = preview.map_or_else(
        || core.is_none_or(|c| c.shows_game()) && !loading.shown,
        |p| !p.0.covers(),
    );
    hud.set_if_neq(crate::ui::HudShown(shown && !post.visible()));
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
    settings_screen: Res<crate::settings::Screen>,
) {
    let Some(mut core) = core else {
        events.clear();
        composing.clear();
        *pasting = None;
        return;
    };
    // The settings window over a page has the keyboard.
    if matches!(core.page(), Page::Game | Page::Settings) || settings_screen.open {
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
        if let Some(action) = core.lobby_key(&event.logical_key) {
            core.act(action);
            continue;
        }
        let act = match &event.logical_key {
            Key::Enter => Act::Submit,
            Key::Tab if shift => Act::PreviousField,
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

/// Where Demo Hero's opening stands, for the game on screen.
#[derive(Default)]
struct DemoStart {
    epoch: u32,
    picked: bool,
    begun: bool,
}

/// Dota's Demo Hero starts at once: the hero chosen is picked and readied,
/// and once the match stands the game clock goes to the horn and the lane
/// creeps wait for the panel's Creeps (as Dota's demo has them off).
fn demo_start(
    core: Option<NonSend<MenuCore>>,
    mut session: NonSendMut<Session>,
    epoch: Res<GameEpoch>,
    mut state: Local<DemoStart>,
) {
    let Some(hero) = core.and_then(|core| core.demo_hero()) else {
        return;
    };
    if state.epoch != epoch.0 {
        *state = DemoStart {
            epoch: epoch.0,
            ..default()
        };
    }
    let game = &mut session.0;
    if !state.picked && game.phase == Phase::Lobby && game.my_slot.is_some() {
        state.picked = true;
        game.act(bota_client_core::Intent::PickHero(hero.0));
        if !game.ready {
            game.act(bota_client_core::Intent::ToggleReady);
        }
    }
    if !state.begun && game.my_standing_hero().is_some() {
        state.begun = true;
        for cheat in [
            bota_proto::Cheat::StartGame,
            bota_proto::Cheat::CreepSpawning { on: false },
        ] {
            game.act(bota_client_core::Intent::Cheat(cheat));
        }
    }
}
