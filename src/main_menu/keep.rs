//! Keeping the replays of the player's matches, by default: a match entered is noted, and when
//! it ends its replay is fetched from the service (the participants' own copy of what the game
//! server wrote) and handed to the library; a local lobby's or Demo Hero's is the server's own
//! file, adopted when the game is left. A fetch that fails is tried again in the background, on
//! the next start too, and never holds the menu.

use std::collections::VecDeque;
#[cfg(target_arch = "wasm32")]
use std::sync::Arc;

use bevy::platform::time::Instant;
use bota_radovid::proto::{ClientMsg, Error, MatchId, MatchResult, Outcome};

#[cfg(target_arch = "wasm32")]
use super::core::Blob;
use super::core::{Asked, Launch, MenuCore, OnScreen};
use crate::replays::{self, MAX_BYTES, Notice, Saving, Wanted};

/// Waits before the first, second and third try of a fetch that failed. After the last the
/// replay is left to the next start.
const RETRY_AFTER: [std::time::Duration; 3] = [
    std::time::Duration::from_secs(5),
    std::time::Duration::from_secs(30),
    std::time::Duration::from_secs(120),
];
/// Matches waiting to be fetched, at most.
const MAX_QUEUE: usize = 16;

/// A replay to fetch from the service.
struct Want {
    match_id: MatchId,
    /// The ruleset of the match.
    mode: String,
    /// How it ended, once the service said so.
    result: Option<MatchResult>,
    /// Opens the replay when it is kept: the player asked for it.
    open: bool,
    tries: usize,
    due: Instant,
}

/// A replay on its way: the bytes arrived so far.
struct Fetch {
    want: Want,
    bytes: Vec<u8>,
}

/// The file a local game's server writes its replay into.
#[cfg(not(target_arch = "wasm32"))]
struct Recording {
    path: std::path::PathBuf,
    mode: &'static str,
}

#[derive(Default)]
pub(super) struct Keeper {
    queue: VecDeque<Want>,
    current: Option<Fetch>,
    #[cfg(not(target_arch = "wasm32"))]
    recording: Option<Recording>,
    /// The match whose replay is opened once it is kept (the player asked for it).
    open_match: Option<u64>,
    /// A kept replay asked for whose bytes are to be opened when read (a browser's).
    #[cfg(target_arch = "wasm32")]
    open_name: Option<String>,
    /// Whether the matches waited for were looked through since the last sign-in.
    swept: bool,
    /// The service that said it keeps no replays of its matches.
    not_kept: Option<String>,
}

impl Keeper {
    /// Whether a replay is on its way: the link stays open for it.
    pub(super) fn fetching(&self) -> bool {
        self.current.is_some()
    }
}

impl MenuCore {
    /// What the player set about keeping replays.
    pub fn set_replay_policy(&mut self, policy: replays::Policy) {
        if self.library.policy() != policy {
            self.library.set_policy(policy);
        }
    }

    /// A game is about to open: a match entered is noted, a local game's recording named.
    pub(super) fn keep_start(&mut self, launch: &Launch) {
        match launch {
            Launch::Seat(handoff) if !self.service_keeps_none() => self.library.want(Wanted {
                match_id: handoff.match_id.0,
                since: replays::now(),
                mode: handoff.ruleset.0.clone(),
            }),
            #[cfg(not(target_arch = "wasm32"))]
            Launch::Host { replay, .. } => self.record(replay, "local"),
            #[cfg(not(target_arch = "wasm32"))]
            Launch::Demo { replay, .. } => self.record(replay, "demo"),
            _ => {}
        }
    }

    /// Whether the service this client uses said it keeps no replays of its matches.
    pub(super) fn service_keeps_none(&self) -> bool {
        self.keeper.not_kept.as_deref() == Some(self.prefs.service.as_str())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn record(&mut self, replay: &Option<std::path::PathBuf>, mode: &'static str) {
        self.keeper.recording = replay.clone().map(|path| Recording { path, mode });
    }

    /// Where a local game's server is to write its replay, if replays are kept.
    pub(super) fn recording_path(&self, mode: &str) -> Option<std::path::PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = self.library.dir().filter(|_| self.library.keeping())?;
            Some(replays::recording_path(&dir, mode, replays::now()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = mode;
            None
        }
    }

    /// The game on screen was left: a local game's recording becomes a kept replay.
    pub(super) fn keep_left(&mut self, left: Option<OnScreen>) {
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(left, Some(OnScreen::Local | OnScreen::Demo))
            && let Some(recording) = self.keeper.recording.take()
        {
            self.library.adopt(recording.path, recording.mode);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = left;
    }

    /// A match of the account ended: its replay is fetched if it is waited for.
    pub(super) fn keep_finished(&mut self, result: &MatchResult) {
        if self
            .library
            .wanted()
            .iter()
            .any(|w| w.match_id == result.match_id.0)
        {
            self.queue_fetch(result.match_id, Some(result.clone()), false);
        }
    }

    /// Signed in: the matches that ended while the client was away are fetched, once the
    /// library has been read.
    pub(super) fn keep_signed_in(&mut self) {
        self.keeper.swept = false;
    }

    fn keep_sweep(&mut self) {
        if self.keeper.swept
            || !self.link_ready()
            || self.library.standing() != crate::replays::Standing::Ready
        {
            return;
        }
        self.keeper.swept = true;
        let ended: Vec<MatchId> = self
            .library
            .wanted()
            .iter()
            .map(|w| MatchId(w.match_id))
            .filter(|id| !self.games.iter().any(|g| g.match_id == *id))
            .collect();
        for id in ended {
            self.queue_fetch(id, None, false);
        }
    }

    fn queue_fetch(&mut self, match_id: MatchId, result: Option<MatchResult>, open: bool) {
        if self.service_keeps_none() {
            self.library.unwant(match_id.0);
            if open {
                self.tell(
                    "The service does not keep replays of its matches".to_owned(),
                    true,
                );
            }
            return;
        }
        let keeper = &mut self.keeper;
        let known = |w: &Want| w.match_id == match_id;
        if keeper.current.as_ref().is_some_and(|f| known(&f.want)) {
            return;
        }
        if let Some(queued) = keeper.queue.iter_mut().find(|w| known(w)) {
            queued.open |= open;
            queued.result = queued.result.take().or(result);
            return;
        }
        if keeper.queue.len() >= MAX_QUEUE {
            return;
        }
        let mode = self
            .library
            .wanted()
            .iter()
            .find(|w| w.match_id == match_id.0)
            .map(|w| w.mode.clone())
            .or_else(|| result.as_ref().map(|r| r.ruleset.0.clone()))
            .unwrap_or_default();
        keeper.queue.push_back(Want {
            match_id,
            mode,
            result,
            open,
            tries: 0,
            due: Instant::now(),
        });
    }

    /// Every frame: what the library finished, and the next fetch when its time has come.
    pub(super) fn keep_update(&mut self) {
        for notice in self.library.poll() {
            self.kept(notice);
        }
        self.keep_sweep();
        // Signed out, nothing is fetched; the matches stay wanted for the next sign-in.
        if self.account
            && self.keeper.current.is_none()
            && let Some(at) = self
                .keeper
                .queue
                .iter()
                .position(|w| w.due <= Instant::now())
            && let Some(want) = self.keeper.queue.remove(at)
        {
            let match_id = want.match_id;
            self.keeper.current = Some(Fetch {
                want,
                bytes: Vec::new(),
            });
            self.request_chunk(match_id, 0);
        }
    }

    fn request_chunk(&mut self, match_id: MatchId, offset: u64) {
        let msg = ClientMsg::MatchArtifact { match_id, offset };
        self.ask(msg, Asked::Artifact(match_id));
    }

    fn kept(&mut self, notice: Notice) {
        match notice {
            Notice::Saved(record) => {
                if let Some(id) = record.match_id {
                    self.library.unwant(id);
                }
                if self
                    .keeper
                    .open_match
                    .take_if(|m| Some(*m) == record.match_id)
                    .is_some()
                {
                    self.open_kept(&record.name);
                }
            }
            Notice::NotSaved { match_id, why } => {
                bevy::log::info!("replay not kept: {why}");
                if let Some(id) = match_id {
                    // A replay of no match, or one the player does not want, is not fetched again.
                    self.library.unwant(id);
                    if self.keeper.open_match.take_if(|m| *m == id).is_some() {
                        self.tell(format!("The replay cannot be kept: {why}"), true);
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            Notice::Loaded { name, bytes } => {
                if self.keeper.open_name.take_if(|n| *n == name).is_some() {
                    self.start(Launch::Blob(Blob(Arc::from(bytes))));
                }
            }
            #[cfg(target_arch = "wasm32")]
            Notice::Picked { name, bytes } => {
                if bytes.len() as u64 > MAX_BYTES {
                    self.tell(format!("{name} is longer than a replay may be"), true);
                } else {
                    self.start(Launch::Blob(Blob(Arc::from(bytes))));
                }
            }
            #[cfg(target_arch = "wasm32")]
            Notice::PickFailed(why) => {
                if !why.is_empty() {
                    self.tell(format!("The file cannot be opened: {why}"), true);
                }
            }
            #[cfg(target_arch = "wasm32")]
            Notice::LoadFailed { name, why } => {
                if self.keeper.open_name.take_if(|n| *n == name).is_some() {
                    self.tell(format!("The replay cannot be opened: {why}"), true);
                }
            }
        }
    }

    /// One piece of a replay arrived: kept and the next asked for, or the whole handed over.
    pub(super) fn keep_chunk(&mut self, match_id: MatchId, offset: u64, total: u64, data: &[u8]) {
        let Some(mut fetch) = self.keeper.current.take_if(|f| f.want.match_id == match_id) else {
            return;
        };
        let end = offset + data.len() as u64;
        let sound = total <= MAX_BYTES
            && offset == fetch.bytes.len() as u64
            && !data.is_empty()
            && end <= total;
        if !sound {
            self.fetch_failed(fetch.want, "an unsound piece");
            return;
        }
        fetch.bytes.extend_from_slice(data);
        if end < total {
            if fetch.want.open {
                self.tell(
                    format!("Fetching the replay: {}%", end * 100 / total),
                    false,
                );
            }
            self.keeper.current = Some(fetch);
            self.request_chunk(match_id, end);
            return;
        }
        if fetch.want.open {
            self.status = None;
        }
        let Fetch { want, bytes } = fetch;
        let (players, outcome) = want.result.as_ref().map(sum_up).unwrap_or_default();
        if want.open {
            self.keeper.open_match = Some(match_id.0);
        }
        self.library.save(Saving {
            mode: want.mode,
            match_id: Some(match_id.0),
            players,
            outcome,
            bytes,
            explicit: want.open,
        });
    }

    /// The service has no replay of the match, or refused to give it now.
    pub(super) fn keep_refused(&mut self, match_id: MatchId, error: &Error) {
        let Some(fetch) = self.keeper.current.take_if(|f| f.want.match_id == match_id) else {
            return;
        };
        match error {
            Error::ArtifactsNotKept => {
                self.keeper.not_kept = Some(self.prefs.service.clone());
                self.library.unwant(match_id.0);
                if fetch.want.open {
                    self.tell(
                        "The service does not keep replays of its matches".to_owned(),
                        true,
                    );
                }
            }
            Error::NoArtifact => {
                if fetch.want.open {
                    self.tell("The service keeps no replay of that match".to_owned(), true);
                }
                self.library.unwant(match_id.0);
            }
            other => self.fetch_failed(fetch.want, &format!("{other:?}")),
        }
    }

    /// The link broke with a replay on its way.
    pub(super) fn keep_link_lost(&mut self) {
        if let Some(fetch) = self.keeper.current.take() {
            self.fetch_failed(fetch.want, "the link was lost");
        }
    }

    /// Tries again later, a few times; then the match stays wanted for the next start.
    fn fetch_failed(&mut self, mut want: Want, why: &str) {
        bevy::log::info!("replay of match {} not fetched: {why}", want.match_id.0);
        if let Some(wait) = RETRY_AFTER.get(want.tries) {
            want.tries += 1;
            want.due = Instant::now() + *wait;
            self.keeper.queue.push_back(want);
        } else if want.open {
            self.tell("The replay could not be fetched".to_owned(), true);
        }
    }

    /// Opens a kept replay.
    pub(super) fn open_kept(&mut self, name: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(path) = self.library.path(name) {
                self.start(Launch::Replay(path));
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.keeper.open_name = Some(name.to_owned());
            self.library.fetch(name);
        }
    }

    /// WATCH REPLAY of a finished match: the kept replay, else fetched from the service now.
    pub(super) fn watch_replay(&mut self, match_id: MatchId) {
        if let Some(name) = self.library.kept(match_id.0).map(|r| r.name.clone()) {
            self.open_kept(&name);
            return;
        }
        if self.library.standing() == replays::Standing::Unavailable {
            self.tell("This client keeps no replays".to_owned(), true);
            return;
        }
        let result = self
            .results
            .iter()
            .find(|r| r.match_id == match_id)
            .cloned();
        self.queue_fetch(match_id, result, true);
    }

    /// Opens the replay listed under `name`.
    pub(super) fn open_listed(&mut self, name: &str) {
        let Some(record) = self
            .library
            .records()
            .iter()
            .find(|r| r.name == name)
            .cloned()
        else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let super::replay_info::Summary::Broken(why) = self.summary_of(&record) {
            self.tell(format!("That replay cannot be watched: {why}"), true);
            return;
        }
        self.open_kept(&record.name);
    }

    /// The replays page: its keyboard visits the listed replays, and what is not known of them
    /// is read.
    pub(super) fn read_summaries(&mut self) {
        let names: Vec<String> = self
            .library
            .records()
            .iter()
            .map(|r| r.name.clone())
            .collect();
        self.form.set_replay_rows(&names);
        #[cfg(not(target_arch = "wasm32"))]
        for record in self.library.records() {
            if record.players.is_empty()
                && let Some(path) = self.library.path(&record.name)
            {
                self.shelf.summary(&path, stamp_of(record));
            }
        }
    }

    /// The summary of a listed replay.
    pub(super) fn summary_of(&self, record: &replays::Record) -> super::replay_info::Summary {
        use super::replay_info::Summary;
        if !record.players.is_empty() {
            return Summary::Read {
                sides: record.players.clone(),
                outcome: record.outcome.clone(),
            };
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = self.library.path(&record.name) {
            return self.shelf.peek(&path, stamp_of(record));
        }
        Summary::Read {
            sides: String::new(),
            outcome: record.outcome.clone(),
        }
    }
}

/// The identity of a listed file, which a change of reads anew.
#[cfg(not(target_arch = "wasm32"))]
fn stamp_of(record: &replays::Record) -> super::replay_info::Stamp {
    super::replay_info::Stamp {
        len: record.bytes,
        modified: std::time::UNIX_EPOCH + std::time::Duration::from_secs(record.when),
    }
}

/// Who played and how it ended, as the match's result tells it.
fn sum_up(result: &MatchResult) -> (String, String) {
    use super::text;
    let team = |team: u8| {
        let names: Vec<String> = result
            .seats
            .iter()
            .filter(|s| s.team == team)
            .map(|s| text::line(&s.nickname, text::NAME))
            .collect();
        if names.is_empty() {
            "nobody".to_owned()
        } else {
            names.join(", ")
        }
    };
    let players = format!("{} vs {}", team(0), team(1));
    let clock = super::core::clock(result.duration_ms / 1000);
    let outcome = match result.outcome {
        Outcome::Winner { team: won } => format!("{} won, {clock}", team(won)),
        Outcome::Draw => format!("draw, {clock}"),
        Outcome::Null => "no result".to_owned(),
    };
    (players, outcome)
}
