//! Match results from local recording metadata, shared by post-game and local history.

pub(crate) mod controls;
pub mod data;
pub mod graphs;
pub(crate) mod preview;
mod recording;
pub(crate) mod render;
pub mod summary;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::recorded_match_for_menu;
pub(crate) mod view;

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
    /// An online match with a known handoff: only matching local metadata can enrich it.
    Service(MatchId),
}

/// A local metadata lookup for a known online match.
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
    overlay: Option<view::View>,
    profile_return: Option<(crate::main_menu::Page, view::View)>,
    presented: u32,
    active_version: u32,
    closed: bool,
    outcome_revision: Option<(MatchId, u64)>,
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
            overlay: None,
            profile_return: None,
            presented: 0,
            active_version: 0,
            closed: false,
            outcome_revision: None,
        }
    }
}

/// How often and how many times a local game's files are looked at again while the server
/// finishes them.
const RETRY_EVERY: Duration = Duration::from_millis(500);
const RETRIES: u8 = 20;

impl PostGame {
    /// The game about to open will have its summary from `origin`.
    pub fn expect(&mut self, origin: Origin) {
        self.next = origin;
    }

    /// The game expected opened: whatever was shown is gone.
    pub fn begin(&mut self) {
        self.outcome_revision = None;
        self.origin = std::mem::take(&mut self.next);
        self.reset();
        self.overlay = None;
        self.profile_return = None;
        self.closed = false;
        self.presented += 1;
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

    pub fn visible(&self) -> bool {
        self.overlay.is_some()
    }

    /// What is on screen.
    #[cfg(test)]
    pub fn summary(&self) -> Option<&MatchSummary> {
        self.shown.as_ref()
    }
}

pub fn plugin(app: &mut App) {
    app.insert_non_send(PostGame::default())
        .add_systems(Startup, from_args)
        .add_systems(
            Update,
            (
                drive,
                controls::present,
                controls::clicks,
                controls::redraw,
                controls::dim,
                controls::hover,
                controls::focus_outline,
                render::chart::hover,
                controls::scroll,
                render::hover::items,
                render::hover::details,
            )
                .chain()
                .after(crate::main_menu::availability::navigate),
        )
        .add_systems(
            PreUpdate,
            controls::keyboard
                .after(crate::keys::fire)
                .before(crate::main_menu::shell::escape),
        );
}

/// A replay the command line named has its own summary read from the file.
fn from_args(args: Res<crate::Args>, mut post: NonSendMut<PostGame>) {
    if let Some(name) = args.menu_preview.as_deref() {
        post.overlay = preview::view(name);
        if post.overlay.is_some() {
            post.presented = 1;
            post.active_version = post.version;
        }
    }
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
    use std::io::Read;
    let limit =
        (bota_replay::limits::MAX_META_BYTES + bota_replay::limits::END_RESERVE_BYTES) as u64;
    let sidecar = std::fs::File::open(path.with_extension("meta"))
        .ok()
        .and_then(|file| {
            let mut bytes = Vec::new();
            file.take(limit + 1).read_to_end(&mut bytes).ok()?;
            (bytes.len() as u64 <= limit).then_some(bytes)
        });
    if let Some(bytes) = sidecar
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
    if matches!(post.stage, Stage::Done)
        && matches!(post.origin, Origin::Service(_))
        && let Some(Fetched {
            bytes: Some(bytes), ..
        }) = post.fetched.take()
    {
        settle(post, &session, Some(bytes), false);
        return;
    }
    if matches!(post.stage, Stage::Idle) && matches!(post.origin, Origin::Service(_)) {
        if let Some(Fetched { bytes, .. }) = post.fetched.take() {
            settle(post, &session, bytes, false);
        } else if let Some(summary) = from_stats(&session) {
            post.show(summary);
        }
        return;
    }
    match &mut post.stage {
        Stage::Idle => {
            post.stage = match &post.origin {
                Origin::Hidden => Stage::Done,
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
