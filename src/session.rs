//! The client core inside the Bevy app: built from the command line, pumped
//! once a frame, and read by every other system.

use bevy::prelude::*;
use bota_client_core::{App as Core, Pin, ReplayPlayer, Source};
use bota_proto::SlotId;

use crate::Args;

/// The client core. Not `Send`: a replay reads through a boxed reader, so
/// the systems that touch it run on the main thread.
pub struct Session(pub Core);

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

/// The environment variable that carries the seat key: an argument would show
/// in every process listing.
#[cfg(not(target_arch = "wasm32"))]
const SEAT_KEY_VAR: &str = "BOTA_SEAT_KEY";

/// The server joined when only `--spectate` or a seat key is given.
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_ADDR: &str = "127.0.0.1:4455";

/// Whether the environment holds a seat key (a browser has no environment).
pub fn seat_key_given() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    return std::env::var(SEAT_KEY_VAR).is_ok_and(|hex| !hex.is_empty());
    #[cfg(target_arch = "wasm32")]
    false
}

/// Joins the server the arguments name, as a player or a spectator.
#[cfg(not(target_arch = "wasm32"))]
fn connect(args: &Args) -> std::io::Result<Session> {
    use bota_client_core::{Net, Transport};
    use bota_proto::{ClientMsg, Codec, Role, SeatKey};
    use std::io::{Error, ErrorKind};
    let addr = args.addr.as_deref().unwrap_or(DEFAULT_ADDR);
    if !stays_private(addr) {
        // Plain TCP is first-class (LAN, local play); the player is told what it means.
        eprintln!(
            "dbota: plain connection to {}: fine on your LAN, not safe across the internet; use \
wss:// where the host has a certificate",
            addr
        );
    }
    let key = std::env::var(SEAT_KEY_VAR)
        .ok()
        .filter(|hex| !hex.is_empty())
        .map(|hex| {
            SeatKey::from_hex(hex.trim()).ok_or_else(|| {
                Error::new(
                    ErrorKind::InvalidInput,
                    format!("{SEAT_KEY_VAR}: a seat key is 64 hex digits"),
                )
            })
        })
        .transpose()?
        // Without one, the seat is claimed with a key of this run's own, so
        // RECONNECT after a disconnect takes the same seat again.
        .or_else(|| (!args.spectate).then(claim).flatten());
    let mut net = Net::connect(addr)?;
    let role = if args.spectate {
        Role::Spectator
    } else {
        Role::Player
    };
    net.send(&ClientMsg::hello(
        role,
        args.name.as_str(),
        key,
        bota_core::WIRE_FINGERPRINT,
    ))?;
    net.send(&ClientMsg::Compress(Codec::Deflate))?;
    Ok(Session(Core::new(Source::Live(Box::new(net)))))
}

/// The key this run claims an unreserved seat with: drawn once, so every
/// connection of the run asks for the same seat.
#[cfg(not(target_arch = "wasm32"))]
fn claim() -> Option<bota_proto::SeatKey> {
    static CLAIM: std::sync::OnceLock<Option<bota_proto::SeatKey>> = std::sync::OnceLock::new();
    *CLAIM.get_or_init(|| crate::main_menu::Claim::draw().map(|c| c.0))
}

/// Whether what is sent to `addr` stays out of other hands: TLS (`wss://`),
/// or a loopback host.
#[cfg(not(target_arch = "wasm32"))]
fn stays_private(addr: &str) -> bool {
    if addr.starts_with("wss://") {
        return true;
    }
    let rest = addr.split_once("://").map_or(addr, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host),
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
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
pub fn pump(
    mut session: NonSendMut<Session>,
    time: Res<Time>,
    mut known: ResMut<MatchKnown>,
    mut exit: MessageWriter<AppExit>,
    mut failed: Local<bool>,
) {
    let core = &mut session.0;
    core.pump(time.delta_secs());
    // A server that cuts a live client off before any match view (a refused seat key, a wrong
    // wire) leaves nothing to look at: say why and quit instead of showing an empty screen.
    if core.lost && core.view().is_none() && matches!(core.source, Source::Live(_)) {
        let why = core.feed.last().map_or_else(
            || "connection to the server lost".to_string(),
            |line| crate::feed::text(core, &line.kind),
        );
        // The line may hold the server's words: no control characters reach the terminal.
        let why: String = why.chars().filter(|c| !c.is_control()).collect();
        eprintln!("dbota: {why}");
        exit.write(AppExit::error());
    }
    // A replay that fails to read says so at once, whatever shows on screen.
    if let Source::Replay(player) = &core.source
        && let Some(err) = player.error()
        && !*failed
    {
        error!("the replay cannot be read: {err}");
        *failed = true;
    }
    let ready = core.terrain_cells() > 0 && core.view().is_some();
    if known.0 != ready {
        known.0 = ready;
        if ready {
            info!("match known: tick {}", core.view().map_or(0, |v| v.tick));
        }
    }
}
