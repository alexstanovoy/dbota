//! A separate, bounded visitor over the configured matchmaking client's public read APIs.
use super::{
    account_page::Rating,
    avatar,
    core::{Asked, MenuCore, Page},
    history::{Match, ResultKind},
    text,
    view::PageView,
};
use bevy::{
    platform::time::Instant,
    tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future},
};
use bota_radovid::proto::{
    AccountId, AvatarVersion, ClientMsg, Error, MatchResult, Outcome, PublicAccount, Reply,
};
use std::{sync::Arc, time::Duration};

const MAX_MATCHES: usize = bota_radovid::proto::MAX_RECENT_MATCHES as usize;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub service: String,
    pub account: u64,
}

/// Parsing and identity equality are shared with the matchmaking client.
pub use bota_radovid::matchmaker_identity as service_identity;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Account,
    Matches,
    Avatar,
    Done,
}

#[derive(Default)]
pub struct Reader {
    generation: u64,
    visit: Option<Visit>,
    #[cfg(test)]
    pub(crate) requests: u64,
}
#[derive(Clone)]
pub(super) struct Snapshot {
    name: String,
    ratings: Vec<Rating>,
    matches: Arc<Vec<Match>>,
    results: Arc<Vec<MatchResult>>,
    avatar: avatar::View,
    avatar_version: Option<AvatarVersion>,
}
struct Visit {
    target: Target,
    generation: u64,
    back: Page,
    stage: Part,
    waiting: bool,
    sent: Option<Instant>,
    retry: Option<Instant>,
    tries: u8,
    name: String,
    version: Option<String>,
    ratings: Vec<Rating>,
    matches: Arc<Vec<Match>>,
    results: Arc<Vec<MatchResult>>,
    cached: bool,
    avatar: avatar::View,
    avatar_version: Option<AvatarVersion>,
    decoding: Option<Task<Result<dbota_import::thumbnail::Pixels, String>>>,
    notice: Option<String>,
}
impl Reader {
    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.visit = None;
    }
    pub fn disconnected(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(visit) = &mut self.visit {
            visit.generation = self.generation;
            visit.waiting = false;
            visit.stage = Part::Done;
            visit.sent = None;
            visit.retry = None;
            visit.decoding = None;
            visit.notice = Some("The profile connection was lost. Go back and retry.".into());
        }
    }
    fn begin(&mut self, target: Target, back: Page) {
        self.generation = self.generation.wrapping_add(1);
        self.visit = Some(Visit {
            generation: self.generation,
            name: format!("Player #{}", target.account),
            version: None,
            target,
            back,
            stage: Part::Account,
            waiting: false,
            sent: None,
            retry: None,
            tries: 0,
            ratings: Vec::new(),
            matches: Arc::new(Vec::new()),
            results: Arc::new(Vec::new()),
            cached: false,
            avatar: Default::default(),
            avatar_version: None,
            decoding: None,
            notice: Some("Loading profile…".into()),
        });
    }
    fn restore(&mut self, snapshot: &Snapshot) {
        if let Some(visit) = &mut self.visit {
            visit.name = snapshot.name.clone();
            visit.ratings = snapshot.ratings.clone();
            visit.matches = Arc::clone(&snapshot.matches);
            visit.results = Arc::clone(&snapshot.results);
            visit.avatar = snapshot.avatar.clone();
            visit.avatar_version = snapshot.avatar_version;
            visit.stage = Part::Done;
            visit.notice = None;
            visit.cached = true;
        }
    }
    pub(super) fn take_snapshot(&mut self) -> Option<(u64, Snapshot)> {
        let visit = self.visit.as_mut()?;
        if visit.cached
            || visit.stage != Part::Done
            || visit.waiting
            || visit.decoding.is_some()
            || visit.notice.is_some()
        {
            return None;
        }
        visit.cached = true;
        Some((
            visit.target.account,
            Snapshot {
                name: visit.name.clone(),
                ratings: visit.ratings.clone(),
                matches: Arc::clone(&visit.matches),
                results: Arc::clone(&visit.results),
                avatar: visit.avatar.clone(),
                avatar_version: visit.avatar_version,
            },
        ))
    }
    fn request(&mut self) -> Option<(Asked, ClientMsg)> {
        let visit = self.visit.as_mut()?;
        if let Some(task) = &mut visit.decoding
            && let Some(result) = block_on(future::poll_once(task))
        {
            visit.decoding = None;
            match result {
                Ok(pixels) => visit.avatar.current = Some(Arc::new(pixels)),
                Err(_) => visit.notice = Some("Avatar unavailable".into()),
            }
        }
        if visit.waiting && visit.sent.is_some_and(|sent| sent.elapsed() >= TIMEOUT) {
            visit.waiting = false;
            visit.stage = Part::Done;
            visit.notice = Some("Profile request timed out. Go back and retry.".into());
        }
        if visit.waiting || visit.retry.is_some_and(|at| Instant::now() < at) {
            return None;
        }
        let account = AccountId(visit.target.account);
        let msg = match visit.stage {
            Part::Account => ClientMsg::Account { account },
            Part::Matches => ClientMsg::RecentMatches {
                account,
                limit: MAX_MATCHES as u8,
            },
            Part::Avatar => ClientMsg::GetAvatar { account },
            Part::Done => return None,
        };
        visit.waiting = true;
        visit.sent = Some(Instant::now());
        // Test instrumentation counts attempts before dispatch, including failed sends.
        #[cfg(test)]
        {
            self.requests += 1;
        }
        Some((Asked::Visitor(visit.generation, visit.stage), msg))
    }
    fn receive(&mut self, generation: u64, part: Part, reply: Reply, modes: &[(String, String)]) {
        let Some(visit) = self
            .visit
            .as_mut()
            .filter(|visit| visit.generation == generation && visit.stage == part && visit.waiting)
        else {
            return;
        };
        visit.waiting = false;
        visit.retry = None;
        visit.tries = 0;
        match (part, reply) {
            (Part::Account, Reply::Account { account }) if account.id.0 == visit.target.account => {
                visit.name = text::who(&account.nickname, account.id.0);
                visit.version = account.version.as_deref().map(|version| {
                    crate::player_name::version_text(version, bota_radovid::proto::MAX_VERSION_LEN)
                });
                visit.ratings = public_ratings(&account, modes);
                visit.avatar_version = account.avatar;
                visit.notice = None;
                visit.stage = Part::Matches;
            }
            (Part::Matches, Reply::Matches { list }) => {
                visit.results = Arc::new(
                    list.into_iter()
                        .take(MAX_MATCHES)
                        .filter(|result| {
                            result
                                .seats
                                .iter()
                                .any(|seat| seat.account.0 == visit.target.account)
                        })
                        .collect(),
                );
                visit.matches = Arc::new(
                    visit
                        .results
                        .iter()
                        .cloned()
                        .map(|result| public_match(result, visit.target.account))
                        .collect(),
                );
                visit.stage = if visit.avatar_version.is_some() {
                    Part::Avatar
                } else {
                    Part::Done
                };
            }
            (Part::Avatar, Reply::Avatar { account, avatar })
                if account.0 == visit.target.account =>
            {
                if let Some(avatar) = avatar {
                    visit.decoding = Some(AsyncComputeTaskPool::get().spawn(async move {
                        dbota_import::thumbnail::decode(&avatar.png, 256 * 1024, 128)
                    }));
                }
                visit.stage = Part::Done;
            }
            _ => {
                visit.stage = Part::Done;
                visit.notice = Some("The service returned an invalid profile response".into());
            }
        }
    }
    fn refused(&mut self, generation: u64, part: Part, error: &Error) {
        let Some(visit) = self
            .visit
            .as_mut()
            .filter(|visit| visit.generation == generation && visit.stage == part)
        else {
            return;
        };
        visit.waiting = false;
        if let Error::Throttled { retry_after_ms } | Error::RateLimited { retry_after_ms } = error
            && visit.tries < 3
        {
            visit.tries += 1;
            visit.retry = Some(
                Instant::now() + Duration::from_millis(u64::from(*retry_after_ms).clamp(250, 5000)),
            );
            return;
        }
        visit.notice = Some(refusal_notice(part, error).into());
        visit.stage = if part == Part::Matches && visit.avatar_version.is_some() {
            Part::Avatar
        } else {
            Part::Done
        };
    }
}

/// What a refused part tells the player. The service answers `UnknownAccount` alike for hidden,
/// banned, deleted and missing accounts, so the text claims none of them.
fn refusal_notice(part: Part, error: &Error) -> &'static str {
    match (part, error) {
        (Part::Matches, Error::UnknownAccount) => "No match list is available for this player",
        (Part::Matches, Error::Throttled { .. } | Error::RateLimited { .. }) => {
            "Recent matches unavailable: too many requests, try again shortly"
        }
        (Part::Matches, Error::Internal) => "Recent matches unavailable: the service failed",
        (Part::Matches, Error::LookupTooExpensive) => "Recent matches unavailable: try again later",
        (Part::Matches, _) => "Recent matches unavailable",
        (Part::Avatar, _) => "Avatar unavailable",
        _ => "This player's profile is unavailable",
    }
}

fn public_ratings(account: &PublicAccount, modes: &[(String, String)]) -> Vec<Rating> {
    account
        .ratings
        .iter()
        .take(bota_radovid::proto::MAX_RATED_MODES)
        .map(|(mode, rating)| Rating {
            mode_id: mode.0.clone(),
            confidence: super::rank_confidence::from_deviation(
                rating.rating.deviation,
                rating.games,
            ),
            mode: modes
                .iter()
                .find(|(id, _)| id == &mode.0)
                .map_or_else(|| text::line(&mode.0, 32), |(_, name)| name.clone()),
            rating: super::leaderboard::rating(rating.rating.rating),
            games: rating.games,
            rank: rating.rank.filter(|rank| *rank > 0),
        })
        .collect()
}
pub(super) fn public_match(result: MatchResult, account: u64) -> Match {
    let seat = result.seats.iter().find(|seat| seat.account.0 == account);
    let outcome = match (result.outcome, seat) {
        (Outcome::Winner { team }, Some(seat)) if team == seat.team => ResultKind::Won,
        (Outcome::Winner { .. }, Some(_)) => ResultKind::Lost,
        (Outcome::Draw, _) => ResultKind::Draw,
        _ => ResultKind::Undecided,
    };
    let players = result
        .seats
        .iter()
        .take(bota_proto::limits::MAX_SEATS)
        .map(|seat| bota_proto::SeatIdentity {
            slot: bota_proto::SlotId(seat.slot),
            name: text::line(&seat.nickname, text::NAME),
            bot: seat.kind == bota_radovid::proto::AccountKind::Bot,
            version: seat.version.clone(),
        })
        .collect();
    Match {
        players,
        file: format!("public-{}", result.match_id.0),
        when: result.finished_at.0 / 1000,
        mode: text::line(&result.ruleset.0, 32),
        hero: None,
        result: outcome,
        seconds: Some(result.duration_ms / 1000),
        kda: None,
        net_worth: None,
        watchable: false,
        details_available: true,
    }
}

impl MenuCore {
    pub fn inspect_player(&mut self, target: Target) -> Result<(), String> {
        if target.account == 0 || service_identity(&target.service).is_none() {
            return Err("Profile information is unavailable".into());
        }
        let same = bota_radovid::same_matchmaker(&target.service, &self.prefs.service);
        if !same {
            return Err(format!(
                "The file claims this profile is at {}. This claim is not verified. Select that matchmaker explicitly to view it.",
                target.service
            ));
        }
        if !self.logged_in() {
            return Err("Sign in to the configured matchmaker to view this profile".into());
        }
        if self.page == Page::PlayerProfile(target.account)
            && self
                .profiles
                .visit
                .as_ref()
                .is_some_and(|visit| visit.target == target)
        {
            return Ok(());
        }
        let back = self
            .profiles
            .visit
            .as_ref()
            .filter(|_| matches!(self.page, Page::PlayerProfile(_)))
            .map_or(self.page, |visit| visit.back);
        let account = target.account;
        let snapshot = self.cached_profile(account);
        self.profiles.begin(target, back);
        if let Some(snapshot) = snapshot {
            self.profiles.restore(&snapshot);
        }
        self.go_form(Page::PlayerProfile(account));
        Ok(())
    }
    pub(super) fn drive_profile(&mut self) {
        if !matches!(self.page, Page::PlayerProfile(_)) {
            return;
        }
        let allowed = self.logged_in()
            && self.profiles.visit.as_ref().is_some_and(|visit| {
                bota_radovid::same_matchmaker(&visit.target.service, &self.prefs.service)
            });
        if !allowed {
            self.profiles.clear();
            return;
        }
        let request = self.profiles.request();
        self.cache_completed_profile();
        if let Some((asked, msg)) = request {
            self.ask(msg, asked);
        }
    }
    pub(super) fn profile_reply(&mut self, generation: u64, part: Part, reply: Reply) {
        let modes = self.client.as_ref().map_or_else(Vec::new, |client| {
            client
                .rulesets()
                .take(64)
                .map(|mode| (mode.id.0.clone(), text::line(&mode.name, 64)))
                .collect()
        });
        self.profiles.receive(generation, part, reply, &modes);
        self.cache_completed_profile();
    }
    pub(super) fn profile_refused(&mut self, generation: u64, part: Part, error: &Error) {
        self.profiles.refused(generation, part, error);
    }
    pub(super) fn back_profile(&mut self) {
        let back = self
            .profiles
            .visit
            .as_ref()
            .map_or(Page::Profile, |visit| visit.back);
        self.profiles.clear();
        self.go_form(back);
        if back == Page::Leaderboard {
            self.open_leaderboard();
        }
    }
    pub(super) fn public_profile_result(&self, key: &str) -> Option<&MatchResult> {
        let id = key.strip_prefix("public-")?.parse::<u64>().ok()?;
        self.profiles
            .visit
            .as_ref()
            .into_iter()
            .flat_map(|visit| visit.results.iter())
            .chain(&self.history)
            .find(|result| result.match_id.0 == id)
    }
    pub(super) fn visitor_view(&self) -> PageView {
        let visit = self.profiles.visit.as_ref();
        let matches =
            visit.map_or_else(|| Arc::new(Vec::new()), |visit| Arc::clone(&visit.matches));
        PageView::Profile {
            mode_names: self.profile_mode_names(),
            name: visit.map_or_else(|| "Profile unavailable".into(), |visit| visit.name.clone()),
            version: visit.and_then(|visit| visit.version.clone()),
            ratings: visit.map_or_else(Vec::new, |visit| visit.ratings.clone()),
            avatar: visit.map_or_else(Default::default, |visit| visit.avatar.clone()),
            matches,
            read_only: true,
            notice: visit.and_then(|visit| visit.notice.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_identity_preserves_namespaces_and_rejects_nonpublic_addresses() {
        assert_eq!(
            service_identity("wss://EXAMPLE.org:443"),
            Some("wss://example.org/control".into())
        );
        assert_eq!(
            service_identity("wss://example.org/notcontrol"),
            Some("wss://example.org/notcontrol/control".into())
        );
        assert_ne!(
            service_identity("wss://example.org/a"),
            service_identity("wss://example.org/b")
        );
        assert_ne!(
            service_identity("wss://example.org"),
            service_identity("ws://example.org")
        );
        for value in [
            "wss://user:secret@example.org",
            "wss://example.org/?token=secret",
            "wss://example.org/#part",
            "wss://ex\u{202e}ample.org",
        ] {
            assert_eq!(service_identity(value), None, "public identity only");
        }
        assert!(service_identity(&format!("wss://example.org/{}", "a".repeat(512))).is_none());
    }
    #[test]
    fn an_old_account_reply_cannot_replace_the_selected_profile() {
        use bota_radovid::proto::{AccountKind, PublicAccount};
        let target = |account| Target {
            service: "wss://example.org".into(),
            account,
        };
        let mut reader = Reader::default();
        reader.begin(target(1), Page::Leaderboard);
        let old = reader.generation;
        reader.begin(target(2), Page::Leaderboard);
        reader.visit.as_mut().expect("visit").waiting = true;
        reader.receive(
            old,
            Part::Account,
            Reply::Account {
                account: PublicAccount {
                    id: AccountId(1),
                    nickname: "old".into(),
                    kind: AccountKind::Human,
                    online: false,
                    retired_bot_version: None,
                    version: None,
                    main_account: None,
                    ratings: Vec::new(),
                    avatar: None,
                },
            },
            &[],
        );
        let visit = reader.visit.expect("current");
        assert_eq!(visit.name, "Player #2");
        assert_eq!(visit.target.account, 2);
        assert!(visit.waiting);
        assert_eq!(visit.stage, Part::Account);
    }
}

#[cfg(test)]
impl MenuCore {
    pub(crate) fn hold_profile_account_request(&mut self) -> u64 {
        let request = self.profiles.request().expect("new visitor request");
        let Asked::Visitor(generation, Part::Account) = request.0 else {
            panic!("account request")
        };
        generation
    }
    pub(crate) fn expire_profile_account_request(&mut self) {
        self.profiles.visit.as_mut().expect("visit").sent =
            Some(Instant::now() - TIMEOUT - Duration::from_secs(1));
    }
    pub(crate) fn retry_profile_account_request(&mut self, due: bool) {
        self.profiles.visit.as_mut().expect("visit").retry = Some(if due {
            Instant::now() - Duration::from_secs(1)
        } else {
            Instant::now() + Duration::from_secs(3600)
        });
    }
    pub(crate) fn inject_profile_account_reply(&mut self, generation: u64, reply: Reply) {
        self.profile_reply(generation, Part::Account, reply);
    }
    pub(crate) fn throttle_profile_account_request(&mut self, generation: u64) {
        let before = Instant::now();
        self.profile_refused(
            generation,
            Part::Account,
            &Error::Throttled { retry_after_ms: 0 },
        );
        let after = Instant::now();
        let retry = self
            .profiles
            .visit
            .as_ref()
            .expect("visit")
            .retry
            .expect("retry deadline");
        assert!(
            retry >= before + Duration::from_millis(250),
            "a zero service delay is bounded below"
        );
        assert!(
            retry <= after + Duration::from_secs(5),
            "service delays are bounded above"
        );
    }
}

#[cfg(test)]
mod served_matches_tests {
    use super::*;
    #[test]
    fn a_client_never_stamps_local_matchmaker_addresses_from_a_served_result() {
        let service = crate::main_menu::tests::service();
        let played = crate::main_menu::tests::served_postmatch(&service);
        let result = played.menu.results.front().expect("served result");
        for address in [
            "ws://127.0.0.1:7000",
            "ws://127.1",
            "ws://0x7f.0.0.1",
            "ws://192.168.1.5:7000",
            "ws://169.254.2.3",
            "ws://[::1]",
            "ws://[fe80::1]",
            "ws://[fd00::1]",
            "ws://lanserver",
            "ws://host.local",
        ] {
            let profiles = crate::main_menu::profile_metadata::from_result(address, result)
                .expect("explicit no matchmaker");
            assert_eq!(
                profiles,
                bota_replay::meta::Profiles::default(),
                "local addresses and their account mappings never enter a file"
            );
        }
    }
    #[test]
    fn a_served_match_list_keeps_only_matches_containing_the_selected_player() {
        let service = crate::main_menu::tests::service();
        let played = crate::main_menu::tests::served_postmatch(&service);
        let result = played
            .menu
            .results
            .iter()
            .find(|result| result.match_id.0 == played.id)
            .expect("served result")
            .clone();
        let mut unrelated = result.clone();
        unrelated.match_id.0 += 1000;
        for seat in &mut unrelated.seats {
            seat.account.0 += 10000;
        }
        let mut reader = Reader::default();
        reader.begin(
            Target {
                service: service.url.clone(),
                account: played.other,
            },
            Page::Leaderboard,
        );
        let generation = reader.generation;
        let visit = reader.visit.as_mut().expect("visit");
        visit.stage = Part::Matches;
        visit.waiting = true;
        reader.receive(
            generation,
            Part::Matches,
            Reply::Matches {
                list: vec![unrelated, result],
            },
            &[],
        );
        let rows = &reader.visit.as_ref().expect("visit").matches;
        assert_eq!(rows.len(), 1, "unrelated server data is excluded");
        assert_eq!(rows[0].file, format!("public-{}", played.id));
        assert!(rows[0].hero.is_none());
        assert!(rows[0].kda.is_none());
        assert!(rows[0].net_worth.is_none());
    }
    fn matches_notice_after(error: &Error, earlier_tries: u8) -> String {
        let mut reader = Reader::default();
        reader.begin(
            Target {
                service: "ws://example.test".into(),
                account: 2,
            },
            Page::Leaderboard,
        );
        let generation = reader.generation;
        let visit = reader.visit.as_mut().expect("visit");
        visit.stage = Part::Matches;
        visit.waiting = true;
        visit.tries = earlier_tries;
        reader.refused(generation, Part::Matches, error);
        reader
            .visit
            .as_ref()
            .and_then(|visit| visit.notice.clone())
            .expect("a refused match list explains itself")
    }
    #[test]
    fn an_unknown_account_match_list_claims_neither_hidden_nor_missing() {
        assert_eq!(
            matches_notice_after(&Error::UnknownAccount, 0),
            "No match list is available for this player"
        );
    }
    #[test]
    fn an_exhausted_throttle_on_the_match_list_asks_to_wait() {
        assert_eq!(
            matches_notice_after(&Error::Throttled { retry_after_ms: 0 }, 3),
            "Recent matches unavailable: too many requests, try again shortly"
        );
    }
    #[test]
    fn a_service_failure_on_the_match_list_is_named_as_one() {
        assert_eq!(
            matches_notice_after(&Error::Internal, 0),
            "Recent matches unavailable: the service failed"
        );
    }
    #[test]
    fn a_refused_expensive_match_list_asks_for_a_later_retry() {
        assert_eq!(
            matches_notice_after(&Error::LookupTooExpensive, 0),
            "Recent matches unavailable: try again later"
        );
    }
}
