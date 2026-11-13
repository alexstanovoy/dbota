//! Local-history inspection and the handoff from results to replay playback.
use super::{
    core::{Launch, MenuCore, OnScreen, Page},
    local_matches::{Saved, service_key},
};
use crate::postgame::{
    summary::{Detail, MatchSummary},
    view::{Context, Kind, Replay, View},
};
use bota_proto::{SlotId, Team};
use bota_radovid::proto::{EndReason, MatchResult, Outcome};
use std::sync::Arc;

#[cfg(test)]
thread_local! { pub(crate) static OUTCOME_PROJECTIONS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

impl MenuCore {
    pub(super) fn open_result(&mut self, key: &str) {
        if let Some(result) = self.public_profile_result(key) {
            let mut context = self.event_context(result, None);
            context.history = true;
            context.replay = self.result_replay(result.match_id.0);
            let summary = MatchSummary {
                detail: Detail::Stats, winner: winner(result.outcome), seconds: result.duration_ms / 1000,
                rows: Vec::new(), points: Vec::new(), structures: Vec::new(), recording: None,
            };
            self.result_request = Some(View::new(Arc::new(summary), context));
            return;
        }
        let Some(entry) = self.local_history.find(key) else {
            self.tell("Match details are unavailable.".to_owned(), false);
            return;
        };
        let Some(mut summary) = entry.details() else {
            self.tell("Match details are unavailable.".to_owned(), false);
            return;
        };
        let mut context = self.saved_context(entry);
        if let Some(result) = &entry.result {
            summary.winner = winner(result.outcome);
            context.reason = reason(result.reason, summary.winner).to_owned();
        }
        self.result_request = Some(View::new(Arc::new(summary), context));
    }

    fn saved_context(&self, entry: &Saved) -> Context {
        let mut context = entry
            .result
            .as_ref()
            .map_or_else(Context::default, |result| {
                self.event_context(result, entry.owner)
            });
        context.match_id = context.match_id.or_else(|| {
            entry
                .summary
                .as_ref()?
                .recording
                .as_ref()
                .map(|recording| recording.match_id)
        });
        context.mode = super::history::mode_name(&entry.mode).to_owned();
        context.when = when(entry.when);
        context.owner = entry.owner;
        context.history = true;
        context.replay = entry
            .file
            .as_ref()
            .filter(|file| self.has_replay(file))
            .cloned()
            .map(Replay::Saved);
        context
    }

    pub fn take_result_request(&mut self, history_open: bool) -> Option<View> {
        if history_open
            && self
                .result_request
                .as_ref()
                .is_some_and(|view| !view.context.history)
        {
            return None;
        }
        self.result_request.take()
    }

    pub fn result_context(&self, summary: &MatchSummary, owner: Option<SlotId>) -> Context {
        let service = match self.on_screen {
            Some(OnScreen::Match(id)) => Some(id),
            Some(OnScreen::Replay) => None,
            _ if self.page == Page::Result => self.shown,
            _ => None,
        };
        if let Some(result) =
            service.and_then(|id| self.results.iter().find(|result| result.match_id == id))
        {
            let mut context = self.event_context(result, owner);
            context.replay = self.result_replay(result.match_id.0);
            return context;
        }
        Context {
            match_id: service.map(|id| id.0).or_else(|| {
                summary
                    .recording
                    .as_ref()
                    .map(|recording| recording.match_id)
                    .filter(|id| *id != 0)
            }),
            owner,
            kind: if service.is_some() {
                Kind::Unknown
            } else if summary.recording.is_some() {
                Kind::Local
            } else {
                Kind::Direct
            },
            mode: match summary.recording.as_ref().map(|recording| recording.map) {
                Some(2) => "Solo Mid",
                Some(0) => "All Pick",
                _ => "Local game",
            }
            .to_owned(),
            ..Default::default()
        }
    }

    fn event_context(&self, result: &MatchResult, owner: Option<SlotId>) -> Context {
        let account = self
            .client
            .as_ref()
            .and_then(|client| client.me())
            .map(|account| account.id);
        let seat = owner
            .and_then(|slot| result.seats.iter().find(|seat| seat.slot == slot.0))
            .or_else(|| {
                result
                    .seats
                    .iter()
                    .find(|seat| Some(seat.account) == account)
            });
        let rating =
            seat.and_then(|seat| Some((rating(seat.before?.rating)?, rating(seat.after?.rating)?)));
        Context {
            live_service: bota_radovid::matchmaker_identity(&self.prefs.service),
            live_accounts: result
                .seats
                .iter()
                .take(bota_proto::limits::MAX_SEATS)
                .filter(|seat| seat.account.0 != 0)
                .map(|seat| (SlotId(seat.slot), seat.account.0))
                .collect(),
            match_id: Some(result.match_id.0),
            mode: super::history::mode_name(&result.ruleset.0).to_owned(),
            when: when(result.finished_at.0 / 1000),
            kind: if result.rated {
                Kind::Ranked
            } else {
                Kind::Unranked
            },
            rating,
            reason: reason(result.reason, winner(result.outcome)).to_owned(),
            owner: owner.or_else(|| seat.map(|seat| SlotId(seat.slot))),
            ..Default::default()
        }
    }

    pub fn outcome_revision(&self) -> Option<(bota_radovid::proto::MatchId, u64)> {
        (self.page == Page::Result)
            .then_some(self.shown)
            .flatten()
            .map(|id| (id, self.local_history.revision()))
    }

    pub fn outcome_result(&self) -> Option<View> {
        // Count this projection boundary in headless tests; no counter is compiled into the product.
        #[cfg(test)]
        OUTCOME_PROJECTIONS.with(|count| count.set(count.get() + 1));
        if self.page != Page::Result {
            return None;
        }
        let result = self
            .results
            .iter()
            .find(|result| Some(result.match_id) == self.shown)?;
        let entry = self
            .local_history
            .find(&service_key(&self.prefs.service, result.match_id.0));
        let mut summary = entry
            .and_then(Saved::details)
            .unwrap_or_else(|| MatchSummary {
                detail: Detail::Stats,
                winner: winner(result.outcome),
                seconds: result.duration_ms / 1000,
                rows: Vec::new(),
                points: Vec::new(),
                structures: Vec::new(),
                recording: None,
            });
        summary.winner = winner(result.outcome);
        let mut context = self.event_context(result, entry.and_then(|entry| entry.owner));
        context.replay = self.result_replay(result.match_id.0);
        Some(View::new(Arc::new(summary), context))
    }

    fn has_replay(&self, file: &str) -> bool {
        self.library
            .records()
            .iter()
            .any(|record| record.name == file && record.bytes >= crate::replays::MIN_BYTES)
    }

    fn result_replay(&self, id: u64) -> Option<Replay> {
        self.local_history
            .find(&service_key(&self.prefs.service, id))
            .and_then(|entry| entry.file.as_ref())
            .filter(|file| self.has_replay(file))
            .cloned()
            .map(Replay::Saved)
    }

    pub fn watch_result(&mut self, replay: Replay) -> Result<(), WatchFailure> {
        if self.in_game() && self.game_unsafe {
            return Err(WatchFailure::ActiveGame);
        }
        if let Replay::Saved(file) = &replay
            && !self.has_replay(file)
        {
            return Err(WatchFailure::MissingFile);
        }
        if self.in_game() {
            self.pending_result_replay = Some(replay);
            self.pending_result_replay_since = Some(bevy::platform::time::Instant::now());
            self.return_from_results();
            return Ok(());
        }
        match replay {
            Replay::File(path) => self.start(Launch::Replay(path)),
            Replay::Blob(bytes) => self.start(Launch::Blob(super::core::Blob(bytes))),
            Replay::Saved(file) => self.open_listed(&file),
        }
        Ok(())
    }

    pub(super) fn resume_result_replay(&mut self) {
        if self.in_game() {
            return;
        }
        let Some(replay) = self.pending_result_replay.take() else {
            return;
        };
        if let Replay::Saved(file) = &replay
            && !self.has_replay(file)
        {
            if self
                .pending_result_replay_since
                .is_some_and(|since| since.elapsed() < std::time::Duration::from_secs(10))
            {
                self.pending_result_replay = Some(replay);
            } else {
                self.pending_result_replay_since = None;
                self.tell("That replay could not be kept".to_owned(), true);
            }
            return;
        }
        self.pending_result_replay_since = None;
        if let Err(failure) = self.watch_result(replay) {
            self.tell(failure.message().to_owned(), true);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn adopted_result_replay(&mut self, from: &std::path::Path, to: Option<String>) {
        if matches!(&self.pending_result_replay,Some(Replay::File(path))if path==from) {
            self.pending_result_replay = to.map(Replay::Saved);
        }
    }
}

pub fn when(seconds: u64) -> String {
    if seconds > 253_402_300_799 {
        return "—".to_owned();
    }
    std::time::UNIX_EPOCH
        .checked_add(std::time::Duration::from_secs(seconds))
        .map_or_else(|| "—".to_owned(), super::replay_info::when)
}
fn rating(value: f64) -> Option<i32> {
    let value = value.round();
    (value.is_finite() && value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX))
        .then_some(value as i32)
}
fn winner(outcome: Outcome) -> Option<Team> {
    match outcome {
        Outcome::Winner { team: 0 } => Some(Team::Radiant),
        Outcome::Winner { team: 1 } => Some(Team::Dire),
        Outcome::Draw => Some(Team::Neutral),
        _ => None,
    }
}
fn reason(reason: EndReason, winner: Option<Team>) -> &'static str {
    match reason {
        EndReason::Decided | EndReason::Draw => "",
        EndReason::Abandoned => "ABANDONED",
        EndReason::Crashed => "NO RESULT",
        EndReason::Stopped => "STOPPED",
        EndReason::Left => match winner {
            Some(Team::Radiant) => "DIRE PLAYER LEFT",
            Some(Team::Dire) => "RADIANT PLAYER LEFT",
            _ => "PLAYER LEFT",
        },
    }
}
pub(crate) fn shell(kit: &mut super::kit::Kit, root: bevy::prelude::Entity, core: &MenuCore) {
    super::top_bar::draw(kit, root, &core.view(), Default::default());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchFailure {
    ActiveGame,
    MissingFile,
}
impl WatchFailure {
    pub fn message(self) -> &'static str {
        match self {
            Self::ActiveGame => "Leave the current game before watching this replay",
            Self::MissingFile => "That replay is no longer available",
        }
    }
}
