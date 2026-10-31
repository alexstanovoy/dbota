//! The match metadata file of a game of the service, fetched when the match is over so that the
//! post-game screen can read it (see `postgame`). It is an artifact of the match, called `meta`:
//! the service keeps it as bytes and does not read it; the bytes say what they are and which
//! version wrote them. A refusal, a failure and a link lost all end the same way: the screen is
//! told there is none, and shows the final numbers.

use bota_radovid::proto::{ClientMsg, MatchId, MatchResult};

use super::core::{Asked, Launch, MenuCore};
use crate::postgame::Fetched;

/// The name of the artifact.
const NAME: &str = "meta";
/// The most bytes taken: a metadata file is held to a mebibyte (and a reserve to close it).
const MAX_BYTES: u64 =
    (bota_replay::limits::MAX_META_BYTES + bota_replay::limits::END_RESERVE_BYTES) as u64;

/// The file on its way.
struct Fetch {
    match_id: MatchId,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub(super) struct MetaFetch {
    current: Option<Fetch>,
    /// The match whose file was asked for, until its first piece arrives.
    asked: Option<MatchId>,
    /// The match of the service this client last sat down in: the one whose file it wants.
    played: Option<MatchId>,
    ready: Option<Fetched>,
}

impl MetaFetch {
    /// Whether a file is on its way: the link stays open for it.
    pub(super) fn fetching(&self) -> bool {
        self.current.is_some() || self.asked.is_some()
    }
}

impl MenuCore {
    /// A game is about to open: if it is a seat of the service, its match is the one whose
    /// metadata is wanted when it ends.
    pub(super) fn meta_start(&mut self, launch: &Launch) {
        if let Launch::Seat(handoff) = launch {
            self.meta.played = Some(handoff.match_id);
        }
    }

    /// A match of the account ended: the metadata of the one it last played is fetched, whether
    /// it is still on screen or the player left already (the screen it feeds is then gone and
    /// the file is let go).
    pub(super) fn meta_finished(&mut self, result: &MatchResult) {
        if self.meta.played != Some(result.match_id) {
            return;
        }
        self.meta.asked = Some(result.match_id);
        self.ask_meta(result.match_id, 0);
    }

    fn ask_meta(&mut self, match_id: MatchId, offset: u64) {
        let msg = ClientMsg::MatchArtifactNamed {
            match_id,
            name: NAME.to_owned(),
            offset,
        };
        self.ask(msg, Asked::Meta(match_id));
    }

    /// One piece of the file arrived: kept and the next asked for, or the whole handed over.
    pub(super) fn meta_chunk(
        &mut self,
        match_id: MatchId,
        name: &str,
        offset: u64,
        total: u64,
        data: &[u8],
    ) {
        if name != NAME {
            return;
        }
        let mut fetch = match self.meta.current.take() {
            Some(fetch) if fetch.match_id == match_id => fetch,
            None if self.meta.asked.take() == Some(match_id) => Fetch {
                match_id,
                bytes: Vec::new(),
            },
            _ => return,
        };
        let end = offset + data.len() as u64;
        let sound = total <= MAX_BYTES
            && offset == fetch.bytes.len() as u64
            && end <= total
            && (!data.is_empty() || total == 0);
        if !sound {
            self.meta_none(match_id);
            return;
        }
        fetch.bytes.extend_from_slice(data);
        if end < total {
            self.meta.current = Some(fetch);
            self.ask_meta(match_id, end);
            return;
        }
        self.meta.ready = Some(Fetched {
            match_id,
            bytes: Some(fetch.bytes),
        });
    }

    /// The service has no file of the match, or could not give it.
    pub(super) fn meta_none(&mut self, match_id: MatchId) {
        self.meta.current = None;
        self.meta.asked = None;
        self.meta.ready = Some(Fetched {
            match_id,
            bytes: None,
        });
    }

    /// Whether a fetch ended and has not been taken.
    #[cfg(test)]
    pub(super) fn meta_ready(&self) -> bool {
        self.meta.ready.is_some()
    }

    /// What was fetched, once.
    pub fn take_meta(&mut self) -> Option<Fetched> {
        self.meta.ready.take()
    }

    /// The link broke with a file on its way.
    pub(super) fn meta_link_lost(&mut self) {
        if let Some(id) = self
            .meta
            .current
            .as_ref()
            .map(|f| f.match_id)
            .or(self.meta.asked)
        {
            self.meta_none(id);
        }
    }
}
