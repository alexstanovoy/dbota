//! The client core inside the Bevy app: built from the command line, pumped
//! once a frame, and read by every other system.

use bevy::prelude::*;
use bota_client_core::{App as Core, Pin, ReplayPlayer, Source};
use bota_proto::{EntityId, SlotId, UnitView, WorldView};

use crate::Args;

/// The client core. Not `Send`: a replay reads through a boxed reader, so
/// the systems that touch it run on the main thread.
pub struct Session(pub Core);

/// A unit of a view by its id: a view keeps its units sorted by id.
pub fn unit_of(view: &WorldView, id: EntityId) -> Option<&UnitView> {
    let at = view.units.binary_search_by_key(&id, |u| u.id).ok()?;
    view.units.get(at)
}

/// Whether a match is under way: set once the match's terrain is known.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchKnown(pub bool);

pub fn plugin(app: &mut App) {
    app.init_resource::<MatchKnown>()
        .add_systems(PreUpdate, (hold_replay, pump).chain());
}

/// Connects or opens the replay the arguments ask for.
pub fn open(args: &Args) -> std::io::Result<Session> {
    // A browser build with `web-replay` plays the replay named by
    // DBOTA_WEB_REPLAY when it was built: a page has no file to open yet.
    #[cfg(all(target_arch = "wasm32", feature = "web-replay"))]
    let replay: Option<Box<ReplayPlayer>> = Some(Box::new(ReplayPlayer::from_reader(
        std::io::Cursor::new(include_bytes!(env!("DBOTA_WEB_REPLAY")).as_slice()),
    )));
    #[cfg(not(all(target_arch = "wasm32", feature = "web-replay")))]
    let replay = match &args.replay {
        Some(path) => Some(Box::new(ReplayPlayer::load(path)?)),
        None => None,
    };
    if let Some(player) = replay {
        let mut core = Core::new(Source::Replay(player));
        core.pov = args.pov.map(SlotId);
        core.my_slot = core.pov;
        core.pinned = args.follow.map(|seat| Pin::Hero(SlotId(seat)));
        return Ok(Session(core));
    }
    connect(args)
}

/// Joins the server the arguments name, as a player or a spectator.
#[cfg(not(target_arch = "wasm32"))]
fn connect(args: &Args) -> std::io::Result<Session> {
    use bota_client_core::{Net, Transport};
    use bota_proto::{ClientMsg, Role};
    let mut net = Net::connect(&args.addr)?;
    let role = if args.spectate {
        Role::Spectator
    } else {
        Role::Player
    };
    net.send(&ClientMsg::hello(role, args.name.as_str(), None))?;
    Ok(Session(Core::new(Source::Live(Box::new(net)))))
}

/// A browser has no sockets; live play there needs a link of its own.
#[cfg(target_arch = "wasm32")]
fn connect(_args: &Args) -> std::io::Result<Session> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "live play is not available in a browser yet; build with --features web-replay",
    ))
}

/// Holds a replay still until the scenery is in place, so nothing of its
/// start plays to a half-built map.
fn hold_replay(
    mut session: NonSendMut<Session>,
    ready: Res<crate::trees::SceneryReady>,
    mut held: Local<bool>,
    time: Res<Time>,
) {
    let started = session.0.view().is_some();
    let bota_client_core::Source::Replay(player) = &mut session.0.source else {
        return;
    };
    // A replay without a match start never gets scenery; give up after a while.
    let waiting = !ready.0 && time.elapsed_secs() < 60.0;
    if waiting && !*held && started {
        *held = true;
        player.paused = true;
    } else if !waiting && *held {
        *held = false;
        player.paused = false;
    }
}

/// Takes in whatever arrived and moves the client's clocks on.
pub fn pump(mut session: NonSendMut<Session>, time: Res<Time>, mut known: ResMut<MatchKnown>) {
    let core = &mut session.0;
    core.pump(time.delta_secs());
    let ready = core.terrain_cells() > 0 && core.view().is_some();
    if known.0 != ready {
        known.0 = ready;
    }
}
