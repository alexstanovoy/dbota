//! The screen a match ends on: Dota's post-game scoreboard, with a row for every seat and two
//! graphs, worked out by [`summary`] from the match's metadata file.
//!
//! Where the file comes from depends on the match: a local game's server writes it beside its
//! replay; a replay file carries it at its end, or the recorder works it out from the replay's
//! records; a replay held in memory likewise; a game of the matchmaking service has it fetched
//! by the menu as an artifact of the service (`meta`). A match with none of these (a direct
//! game, a watched one) tells what every client is sent, the final numbers. Demo Hero, a
//! sandbox with no end, shows nothing.

mod draw;
pub mod summary;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};
use bota_radovid::proto::MatchId;
use summary::MatchSummary;

use crate::session::Session;

/// Where a match's metadata file is had from.
#[derive(Clone, Debug, Default)]
pub enum Origin {
    /// None to be had: the final numbers alone.
    #[default]
    Nothing,
    /// A sandbox with no end: no screen.
    Hidden,
    /// A replay file, with its metadata file beside it, in it, or to be worked out of it.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // a browser opens no files by path
    File(std::path::PathBuf),
    /// A replay held in memory.
    Blob(Arc<[u8]>),
    /// The service's artifact of this match, which the menu fetches.
    Service(MatchId),
}

/// What the menu fetched of a match's metadata.
pub struct Fetched {
    pub match_id: MatchId,
    /// The file; none when the service had none to give.
    pub bytes: Option<Vec<u8>>,
}

type Loading = Task<Option<Vec<u8>>>;

enum Stage {
    /// The match is not over.
    Idle,
    /// The file is being had, off the frame.
    Loading(Loading),
    /// Waiting for the menu's fetch to come.
    Waiting(Instant),
    /// A file that was not finished when the match ended is had again soon.
    Retry(Instant),
    /// Settled: shown, or nothing to show.
    Done,
}

/// The summary of the game on screen, once it is over.
pub struct PostGame {
    origin: Origin,
    /// The origin of the game being opened.
    next: Origin,
    stage: Stage,
    /// What the menu fetched, until the screen takes it.
    fetched: Option<Fetched>,
    /// How many times the files of a local game were had again.
    tries: u8,
    /// What is to be shown, and how many times it was set.
    shown: Option<MatchSummary>,
    version: u32,
}

impl Default for PostGame {
    fn default() -> Self {
        PostGame {
            origin: Origin::Nothing,
            next: Origin::Nothing,
            stage: Stage::Idle,
            fetched: None,
            tries: 0,
            shown: None,
            version: 0,
        }
    }
}

/// How often and how many times a local game's files are looked at again while the server
/// finishes them.
/// How long a game of the service waits for its file before the final numbers are shown.
const SERVICE_PATIENCE: Duration = Duration::from_secs(8);
const RETRY_EVERY: Duration = Duration::from_millis(500);
const RETRIES: u8 = 20;

impl PostGame {
    /// The game about to open will have its summary from `origin`.
    pub fn expect(&mut self, origin: Origin) {
        self.next = origin;
    }

    /// The game expected opened: whatever was shown is gone.
    pub fn begin(&mut self) {
        self.origin = std::mem::take(&mut self.next);
        self.reset();
    }

    fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.fetched = None;
        self.tries = 0;
        if self.shown.take().is_some() {
            self.version += 1;
        }
    }

    /// The file the menu fetched. A late one replaces the final numbers shown meanwhile.
    pub fn provide(&mut self, fetched: Fetched) {
        if matches!(&self.origin, Origin::Service(id) if *id == fetched.match_id) {
            self.fetched = Some(fetched);
        }
    }

    fn show(&mut self, summary: MatchSummary) {
        self.shown = Some(summary);
        self.version += 1;
        self.stage = Stage::Done;
    }

    /// What is on screen.
    pub fn summary(&self) -> Option<&MatchSummary> {
        self.shown.as_ref()
    }
}

pub fn plugin(app: &mut App) {
    app.insert_non_send(PostGame::default())
        .add_systems(Startup, from_args)
        .add_systems(Update, (drive, redraw).chain());
}

/// A replay the command line named has its own summary read from the file.
fn from_args(args: Res<crate::Args>, mut post: NonSendMut<PostGame>) {
    if let Some(path) = args.replay.clone() {
        post.expect(Origin::File(path));
        post.begin();
    }
}

/// The final numbers the match sent, as a summary.
fn from_stats(session: &Session) -> Option<MatchSummary> {
    let (winner, stats) = session.0.over.as_ref()?;
    let players = session
        .0
        .view()
        .map(|v| v.players.as_slice())
        .unwrap_or(&[]);
    Some(MatchSummary::from_stats(
        *winner,
        stats,
        players,
        session.0.pregame_ticks(),
    ))
}

/// The metadata file of the match a replay holds, had in the order of what it costs: the file
/// the game server wrote beside the replay, the one the replay carries at its end, the one the
/// recorder works out of the replay's records.
#[cfg(not(target_arch = "wasm32"))]
fn meta_of_file(path: &std::path::Path) -> Option<Vec<u8>> {
    if let Ok(bytes) = std::fs::read(path.with_extension("meta"))
        && bota_replay::meta::decode(&bytes).is_ok()
    {
        return Some(bytes);
    }
    let mut file = std::fs::File::open(path).ok()?;
    if let Ok(Some(carried)) = bota_client_core::meta_in_replay(&mut file) {
        return Some(carried);
    }
    bota_client_core::meta_of_replay(std::fs::File::open(path).ok()?).ok()
}

fn meta_of_blob(bytes: Arc<[u8]>) -> Option<Vec<u8>> {
    let mut reader = std::io::Cursor::new(bytes);
    if let Ok(Some(carried)) = bota_client_core::meta_in_replay(&mut reader) {
        return Some(carried);
    }
    reader.set_position(0);
    bota_client_core::meta_of_replay(reader).ok()
}

/// Starts having the file of an origin.
fn load(origin: &Origin) -> Option<Loading> {
    let pool = AsyncComputeTaskPool::get();
    match origin {
        #[cfg(not(target_arch = "wasm32"))]
        Origin::File(path) => {
            let path = path.clone();
            Some(pool.spawn(async move { meta_of_file(&path) }))
        }
        Origin::Blob(bytes) => {
            let bytes = bytes.clone();
            Some(pool.spawn(async move { meta_of_blob(bytes) }))
        }
        _ => None,
    }
}

/// Has the summary of a match that is over.
fn drive(session: NonSend<Session>, mut post: NonSendMut<PostGame>) {
    if session.0.over.is_none() {
        if !matches!(post.stage, Stage::Idle) || post.shown.is_some() {
            post.reset();
        }
        return;
    }
    let post = &mut *post;
    match &mut post.stage {
        Stage::Idle => {
            post.stage = match &post.origin {
                Origin::Hidden => Stage::Done,
                Origin::Service(_) => Stage::Waiting(Instant::now()),
                origin => match load(origin) {
                    Some(loading) => Stage::Loading(loading),
                    None => {
                        if let Some(stats) = from_stats(&session) {
                            post.show(stats);
                        }
                        return;
                    }
                },
            };
        }
        Stage::Waiting(since) => {
            let waited = since.elapsed();
            match post.fetched.take() {
                Some(Fetched { bytes, .. }) => settle(post, &session, bytes, false),
                // The file may still come and replace the numbers shown meanwhile.
                None if waited >= SERVICE_PATIENCE && post.shown.is_none() => {
                    if let Some(stats) = from_stats(&session) {
                        post.shown = Some(stats);
                        post.version += 1;
                    }
                }
                None => {}
            }
        }
        Stage::Retry(at) if Instant::now() >= *at => {
            post.stage = match load(&post.origin) {
                Some(loading) => Stage::Loading(loading),
                None => Stage::Done,
            };
        }
        Stage::Loading(loading) => {
            if let Some(done) = block_on(future::poll_once(loading)) {
                let live = matches!(post.origin, Origin::File(_))
                    && matches!(session.0.source, bota_client_core::Source::Live(_));
                settle(post, &session, done, live);
            }
        }
        _ => {}
    }
}

/// The file was had, or not: shown as a summary, else as the final numbers. A local game's file
/// may be had before its server finished it, which shows as a match not decided in it while the
/// client knows it is: it is had again soon, a few times.
fn settle(post: &mut PostGame, session: &Session, bytes: Option<Vec<u8>>, live: bool) {
    let summary = bytes
        .and_then(|bytes| bota_replay::meta::decode(&bytes).ok())
        .and_then(|meta| MatchSummary::from_meta(&meta));
    let over = session.0.over.as_ref().map(|(winner, _)| *winner);
    let unfinished = summary
        .as_ref()
        .is_none_or(|summary| summary.winner.is_none() && over.is_some());
    if live && unfinished && post.tries < RETRIES {
        post.tries += 1;
        post.stage = Stage::Retry(Instant::now() + RETRY_EVERY);
        return;
    }
    match summary {
        Some(mut summary) => {
            summary.winner = summary.winner.or(over);
            post.show(summary);
        }
        None => {
            if let Some(stats) = from_stats(session) {
                post.show(stats);
            }
        }
    }
}

/// Draws the screen anew whenever the summary on show changed.
#[allow(clippy::too_many_arguments)]
fn redraw(
    mut commands: Commands,
    post: NonSend<PostGame>,
    fonts: Res<crate::ui::Fonts>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
    root: Single<Entity, With<crate::hud::HudRoot>>,
    screens: Query<Entity, With<draw::Screen>>,
    mut drawn: Local<u32>,
) {
    if *drawn == post.version {
        return;
    }
    *drawn = post.version;
    for screen in &screens {
        commands.entity(screen).despawn();
    }
    if let Some(summary) = post.summary() {
        let mut kit = draw::Kit {
            commands: &mut commands,
            fonts: &fonts,
            manifest: &manifest,
            server: &server,
        };
        draw::spawn(&mut kit, *root, summary);
    }
}
