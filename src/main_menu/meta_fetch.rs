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
    profiles: Option<(MatchId, bota_replay::meta::Profiles)>,
    service: Option<String>,
    /// Attribution captured when the participant receives its outcome, before a later sign-out.
    history: Option<(MatchId, super::local_matches::Saved)>,
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
        self.meta.service = Some(self.prefs.service.clone());
        self.meta.profiles = super::profile_metadata::from_result(&self.prefs.service, result)
            .map(|profiles| (result.match_id, profiles));
        let owner = self.owner_in_result(result);
        self.meta.history = owner.map(|owner| {
            (
                result.match_id,
                super::local_matches::Saved {
                    key: super::local_matches::service_key(&self.prefs.service, result.match_id.0),
                    file: None,
                    when: result.finished_at.0 / 1000,
                    mode: result.ruleset.0.clone(),
                    owner: Some(owner),
                    metadata: Vec::new(),
                    metadata_key: None,
                    cached: None,
                    result: Some(result.clone()),
                    summary: None,
                    temporary: None,
                },
            )
        });
        self.meta.asked = Some(result.match_id);
        self.ask_meta(result.match_id, 0);
    }

    pub(super) fn owner_in_result(&self, result: &MatchResult) -> Option<bota_proto::SlotId> {
        let account = self
            .client
            .as_ref()
            .and_then(|client| client.me())
            .map(|me| me.id);
        result
            .seats
            .iter()
            .find(|seat| Some(seat.account) == account)
            .map(|seat| bota_proto::SlotId(seat.slot))
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
        if self.meta.service.as_ref().is_some_and(|service| {
            !super::profile_metadata::same_service(service, &self.prefs.service)
        }) {
            self.meta_none(match_id);
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
        let Some(end) = offset.checked_add(data.len() as u64) else {
            self.meta_none(match_id);
            return;
        };
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
        if let Some((id, profiles)) = self.meta.profiles.take()
            && id == match_id
            && let Some(bytes) = super::profile_metadata::stamp_meta(&fetch.bytes, id.0, &profiles)
        {
            fetch.bytes = bytes;
        }
        if let Some((id, mut entry)) = self.meta.history.take()
            && id == match_id
        {
            entry.metadata = fetch.bytes.clone();
            self.local_history
                .insert(entry, self.library.policy().max_count);
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
        self.meta.history = None;
        self.meta.profiles = None;
        self.meta.service = None;
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

#[cfg(test)]
mod profile_tests {
    use super::*;
    use bota_radovid::proto::{
        AccountId, AccountKind, EndReason, Outcome, RulesetId, SeatResult, UnixMs,
    };
    #[test]
    fn metadata_profile_capture_refuses_chunks_after_the_configured_matchmaker_changes() {
        let (_, file) = crate::postgame::recorded_match_for_menu();
        let mut bytes = std::fs::read(file).expect("real metadata");
        let match_id = MatchId(
            bota_replay::meta::decode(&bytes)
                .expect("metadata")
                .header
                .match_id,
        );
        bytes.extend(
            bota_proto::encode_frame_to_vec(&(
                13_u8,
                Some("wss://match.example/control"),
                vec![(0_u8, Some(42_u64))],
            ))
            .expect("publisher snapshot"),
        );
        for changed in [false, true] {
            let mut player = super::super::tests::Player::new(None);
            player.menu.prefs.service = "wss://match.example/control".into();
            player.menu.meta.played = Some(match_id);
            let result = MatchResult {
                match_id,
                ruleset: RulesetId("solo_mid".into()),
                rated: false,
                outcome: Outcome::Winner { team: 0 },
                reason: EndReason::Decided,
                duration_ms: 1000,
                game_report: Vec::new(),
                finished_at: UnixMs(1000),
                seats: vec![SeatResult {
                    slot: 0,
                    team: 0,
                    account: AccountId(42),
                    nickname: "public_player".into(),
                    kind: AccountKind::Human,
                    before: None,
                    after: None,
                    version: None,
                }],
            };
            player.menu.meta_finished(&result);
            if changed {
                player.menu.prefs.service = "wss://other.example/control".into();
            }
            player
                .menu
                .meta_chunk(match_id, "meta", 0, bytes.len() as u64, &bytes);
            let fetched = player.menu.take_meta().expect("settled metadata delivery");
            if changed {
                assert!(
                    fetched.bytes.is_none(),
                    "an old request must not adopt the newly selected service's identity"
                );
            } else {
                let profiles = bota_replay::meta::decode(&fetched.bytes.expect("metadata"))
                    .expect("read")
                    .profiles()
                    .expect("public context");
                assert_eq!(profiles.matchmaker(), Some("wss://match.example/control"));
                assert_eq!(profiles.account(0), Some(42));
            }
        }
    }
}
