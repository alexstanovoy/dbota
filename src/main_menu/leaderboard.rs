//! A bounded, coalesced leaderboard view over the matchmaking SDK. One lookup is in flight;
//! echoed filters and generations keep delayed replies out of a newer selection.
use super::text;
use bevy::platform::time::Instant;
use bota_radovid::proto::{AccountKind, Error, LeaderboardPage, RulesetId};
use std::time::Duration;

pub const TOP: usize = 100;
const EDIT_DELAY: Duration = Duration::from_millis(300);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Players {
    #[default]
    Humans,
    Bots,
}
impl Players {
    pub fn kind(self) -> AccountKind {
        match self {
            Self::Humans => AccountKind::Human,
            Self::Bots => AccountKind::Bot,
        }
    }
    fn from_kind(kind: AccountKind) -> Self {
        match kind {
            AccountKind::Human => Self::Humans,
            AccountKind::Bot => Self::Bots,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub mode: String,
    pub players: Players,
    pub nickname: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mode {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub account: u64,
    pub players: Players,
    pub rank: u32,
    pub nickname: String,
    pub version: Option<String>,
    pub games: u32,
    pub rating: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Loading,
    Ready,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Flight {
    generation: u64,
    query: Query,
    sent: Instant,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub origin: String,
    pub query: Query,
    pub modes: Vec<Mode>,
    pub rows: Vec<Row>,
    pub own: Option<Row>,
    pub status: Status,
    pub total: u32,
    pub notice: Option<String>,
    generation: u64,
    pending: bool,
    due: Option<Instant>,
    last_sent: Option<Instant>,
    flight: Option<Flight>,
    retry_at: Option<Instant>,
}
impl Default for Board {
    fn default() -> Self {
        Self {
            origin: String::new(),
            query: Query {
                mode: "solo_mid".to_owned(),
                ..Default::default()
            },
            modes: Vec::new(),
            rows: Vec::new(),
            own: None,
            status: Status::Unavailable,
            total: 0,
            notice: None,
            generation: 0,
            pending: false,
            due: None,
            last_sent: None,
            flight: None,
            retry_at: None,
        }
    }
}
impl Board {
    fn has_work(&self)->bool {self.pending || self.flight.is_some() || self.retry_at.is_some()}
    fn current_flight(&self,generation:u64)->bool {
        self.generation==generation && self.flight.as_ref().is_some_and(|flight|flight.generation==generation)
    }
    pub fn select(&mut self, query: Query) {
        self.select_at(query, Instant::now());
    }
    fn select_at(&mut self, mut query: Query, now: Instant) {
        if !bota_radovid::proto::valid_nickname_prefix(&query.nickname) {
            self.generation = self.generation.wrapping_add(1);
            self.query = query;
            self.pending = false;
            self.rows.clear();
            self.own = None;
            self.total = 0;
            self.status = Status::Unavailable;
            self.notice = Some(
                "Search the name before #; use up to 20 letters A–Z, digits or underscores."
                    .to_owned(),
            );
            return;
        }
        query.nickname.make_ascii_lowercase();
        if self.query == query && self.status != Status::Unavailable {
            return;
        }
        self.query = query;
        self.rows.clear();
        self.own = None;
        self.total = 0;
        self.status = Status::Loading;
        self.notice = None;
        self.generation = self.generation.wrapping_add(1);
        self.pending = true;
        self.due = Some(if self.last_sent.is_none() {
            now
        } else {
            now + EDIT_DELAY
        });
    }
    pub fn refresh(&mut self) {
        self.status = Status::Unavailable;
        self.select(self.query.clone());
    }
    pub fn request(&mut self) -> Option<(u64, Query)> {
        self.request_at(Instant::now())
    }
    fn request_at(&mut self, now: Instant) -> Option<(u64, Query)> {
        if let Some(flight) = &self.flight
            && now.duration_since(flight.sent) >= REQUEST_TIMEOUT
        {
            self.flight = None;
            if !self.pending {
                self.status = Status::Unavailable;
                self.notice = Some("Leaderboard timed out. Select the mode to retry.".to_owned());
            }
        }
        if self.retry_at.is_some_and(|retry| now >= retry) {
            self.retry_at = None;
        }
        if !self.pending
            || self.flight.is_some()
            || self.due.is_some_and(|at| now < at)
            || self.retry_at.is_some_and(|at| now < at)
            || self
                .last_sent
                .is_some_and(|at| now.duration_since(at) < EDIT_DELAY)
        {
            return None;
        }
        self.pending = false;
        self.last_sent = Some(now);
        self.flight = Some(Flight {
            generation: self.generation,
            query: self.query.clone(),
            sent: now,
        });
        Some((self.generation, self.query.clone()))
    }
    pub fn receive(&mut self, generation: u64, result: Option<(Vec<Row>, Option<Row>)>) {
        if self
            .flight
            .as_ref()
            .is_some_and(|flight| flight.generation == generation)
        {
            self.flight = None;
        }
        if generation != self.generation {
            return;
        }
        let Some((mut rows, own)) = result else {
            self.status = Status::Unavailable;
            self.rows.clear();
            self.own = None;
            self.total = 0;
            return;
        };
        rows.truncate(TOP);
        self.rows = rows;
        self.own = own.filter(|row| row.players == self.query.players);
        self.status = Status::Ready;
        self.notice = None;
    }
    fn page(&mut self, generation: u64, page: LeaderboardPage, account: Option<u64>) {
        let valid_echo = self.flight.as_ref().is_some_and(|flight| {
            flight.generation == generation
                && flight.query.mode == page.ruleset.0
                && flight.query.players.kind() == page.kind
                && flight.query.nickname == page.prefix
        });
        if !valid_echo
            || !(1..=1000).contains(&page.limit)
            || page.rows.len() > usize::from(page.limit)
        {
            self.receive(generation, None);
            return;
        }
        let mut seen = std::collections::HashSet::new();
        let rows = page
            .rows
            .into_iter()
            .filter_map(|row| {
                if row.kind != page.kind {
                    return None;
                }
                let row = Row::from_sdk(row)?;
                seen.insert(row.account).then_some(row)
            })
            .take(TOP)
            .collect();
        let own = page
            .own
            .filter(|row| Some(row.account.0) == account)
            .and_then(Row::from_sdk);
        if generation == self.generation {
            self.total = page.total;
        }
        self.receive(generation, Some((rows, own)));
    }
    fn refused(&mut self, generation: u64, error: &Error) {
        if self
            .flight
            .as_ref()
            .is_some_and(|flight| flight.generation == generation)
        {
            self.flight = None;
        }
        // The server throttles the connection, not a particular set of filters. A delayed
        // rejection still governs the pending edit, while its old error text stays hidden.
        let throttled = if let Error::Throttled { retry_after_ms }
        | Error::RateLimited { retry_after_ms } = error
        {
            self.retry_at = Some(
                Instant::now() + Duration::from_millis(u64::from((*retry_after_ms).min(60_000))),
            );
            true
        } else {
            false
        };
        if generation != self.generation {
            return;
        }
        self.status = Status::Unavailable;
        self.notice = Some(super::core::error_text(error));
        if throttled {
            self.pending = true;
        }
    }
    pub fn disconnected(&mut self) {
        self.flight = None;
        self.pending = false;
        self.status = Status::Unavailable;
        self.rows.clear();
        self.own = None;
        self.total = 0;
    }
}
impl Row {
    fn from_sdk(row: bota_radovid::proto::LadderRow) -> Option<Self> {
        if row.rank == 0 || row.nickname.is_empty() {
            return None;
        }
        Some(Self {
            account: row.account.0,
            players: Players::from_kind(row.kind),
            rank: row.rank,
            nickname: text::who(&row.nickname, row.account.0),
            version: row
                .bot_version
                .as_deref()
                .map(|value| text::line(value, 64)),
            games: row.games,
            rating: rating(row.rating.rating)?,
        })
    }
}
/// Numeric display stays finite and bounded even for a hostile service's extreme float.
pub fn rating(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value.abs() >= 1e9 {
        return Some(format!("{value:.2e}"));
    }
    let integer = format!("{:.0}", value.round());
    let digits = integer.trim_start_matches('-');
    let mut out = if integer.starts_with('-') {
        "-".to_owned()
    } else {
        String::new()
    };
    for (at, ch) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    Some(out)
}

impl super::core::MenuCore {
    pub(super) fn open_leaderboard(&mut self) {
        let query=self.board.query.clone();
        self.form.focus(0);
        self.form.type_in(&query.nickname);
        if self.restore_board(&query) {return;}
        self.board.origin = self.prefs.service.clone();
        if self.board.flight.is_none() {self.board.refresh();}
    }
    pub(super) fn edit_leaderboard(&mut self) {
        if self.page == super::core::Page::Leaderboard {
            let mut query = self.board.query.clone();
            query.nickname = self.form.value(0).to_owned();
            self.select_leaderboard(query);
        }
    }
    pub(super) fn select_leaderboard(&mut self,mut query:Query) {
        query.nickname.make_ascii_lowercase();
        if !self.restore_board(&query) {self.board.select(query);}
    }
    pub(super) fn drive_leaderboard(&mut self) {
        if self.page != super::core::Page::Leaderboard || !self.board.has_work() {
            return;
        }
        if !self.ensure_service_link() {
            return;
        }
        let modes: Vec<_> = self
            .menu_rulesets()
            .filter(|mode| mode.rated)
            .map(|mode| Mode {
                id: mode.id.0.clone(),
                name: text::line(&mode.name, text::NAME),
            })
            .collect();
        if self.board.modes != modes {
            self.board.modes = modes;
            if !self
                .board
                .modes
                .iter()
                .any(|mode| mode.id == self.board.query.mode)
                && let Some(mode) = self.board.modes.first()
            {
                self.board.select(Query {
                    mode: mode.id.clone(),
                    ..self.board.query.clone()
                });
            }
        }
        if self.board.modes.is_empty() {
            self.board.status = Status::Unavailable;
            return;
        }
        let Some((generation, query)) = self.board.request() else {
            return;
        };
        let result = self
            .client
            .as_mut()
            .expect("ready service link")
            .leaderboard(
                &RulesetId(query.mode),
                query.players.kind(),
                &query.nickname,
            );
        match result {
            Ok(sequence) => {
                self.track_request(sequence, super::core::Asked::Leaderboard(generation))
            }
            Err(bota_radovid::ClientError::Refused(error)) => {
                self.board.refused(generation, &error)
            }
            Err(_) => {
                self.board.receive(generation, None);
            }
        }
    }
    pub(super) fn leaderboard_page(&mut self, generation: u64, page: LeaderboardPage) {
        let account = self
            .client
            .as_ref()
            .and_then(|client| client.me())
            .map(|me| me.id.0);
        let current=self.board.current_flight(generation);
        self.board.page(generation, page, account);
        if current && self.board.status==Status::Ready {self.cache_board();}
    }
    pub(super) fn leaderboard_refused(&mut self, generation: u64, error: &Error) {
        self.board.refused(generation, error);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn query(mode: &str) -> Query {
        Query {
            mode: mode.to_owned(),
            ..Default::default()
        }
    }
    #[test]
    fn changing_the_selection_never_starts_a_second_lookup_before_the_first_finishes() {
        let mut board = Board::default();
        board.select(query("first"));
        assert!(board.request().is_some());
        board.select(query("second"));
        assert!(
            board.request().is_none(),
            "coalesce edits while the earlier request is in flight"
        );
    }
    #[test]
    fn the_bots_ladder_does_not_pin_a_human_requesters_rank() {
        let mut board = Board::default();
        board.select(Query {
            players: Players::Bots,
            ..query("solo_mid")
        });
        let (generation, _) = board.request().expect("initial lookup");
        let own = Row {
            account: 7,
            players: Players::Humans,
            rank: 12,
            nickname: "mira".to_owned(),
            version: None,
            games: 41,
            rating: "1,524".to_owned(),
        };
        board.receive(generation, Some((Vec::new(), Some(own))));
        assert!(
            board.own.is_none(),
            "the other category is not on this ladder"
        );
    }
}

#[cfg(test)]
mod delayed_reply_tests {
    use super::*;
    fn first() -> Query {
        Query {
            mode: "solo_mid".to_owned(),
            ..Default::default()
        }
    }
    fn row() -> bota_radovid::proto::LadderRow {
        bota_radovid::proto::LadderRow {
            rank: 1,
            account: bota_radovid::proto::AccountId(3),
            nickname: "ember".to_owned(),
            kind: AccountKind::Human,
            bot_version: None,
            avatar: None,
            rating: bota_radovid::proto::Rating {
                rating: 1811.0,
                deviation: 100.0,
            },
            games: 77,
            wins: 40,
            draws: 0,
            losses: 37,
            last_played: None,
        }
    }
    #[test]
    fn one_malformed_row_does_not_hide_the_other_players() {
        let mut board = Board::default();
        board.select(first());
        let (generation, _) = board.request().unwrap();
        let mut invalid = row();
        invalid.rating.rating = f64::NAN;
        board.page(
            generation,
            LeaderboardPage {
                ruleset: RulesetId("solo_mid".to_owned()),
                kind: AccountKind::Human,
                prefix: String::new(),
                limit: 100,
                total: 2,
                rows: vec![invalid, row()],
                own: None,
            },
            None,
        );
        assert_eq!(board.status, Status::Ready);
        assert_eq!(board.rows.len(), 1);
        assert_eq!(board.rows[0].account, 3);
    }
    #[test]
    fn an_earlier_reply_cannot_replace_the_new_searchs_count() {
        let mut board = Board::default();
        board.select(first());
        let (generation, _) = board.request().unwrap();
        board.select(Query {
            nickname: "mira".to_owned(),
            ..first()
        });
        board.page(
            generation,
            LeaderboardPage {
                ruleset: RulesetId("solo_mid".to_owned()),
                kind: AccountKind::Human,
                prefix: String::new(),
                limit: 1,
                total: 99,
                rows: vec![row()],
                own: None,
            },
            None,
        );
        assert_eq!(board.total, 0);
        assert!(board.rows.is_empty());
        assert_eq!(board.status, Status::Loading);
    }
    #[test]
    fn an_invalid_search_cannot_be_replaced_by_the_previous_valid_reply() {
        let mut board = Board::default();
        board.select(first());
        let (generation, _) = board.request().unwrap();
        board.select(Query {
            nickname: "mira#2207".to_owned(),
            ..first()
        });
        board.receive(
            generation,
            Some((vec![Row::from_sdk(row()).unwrap()], None)),
        );
        assert!(board.rows.is_empty());
        assert_eq!(board.status, Status::Unavailable);
        assert!(board.notice.is_some());
    }
}

#[cfg(test)]
mod pacing_tests {
    use super::*;
    #[test]
    fn edits_are_coalesced_for_300ms_after_a_sent_lookup() {
        let now = Instant::now();
        let mut board = Board::default();
        let query = Query {
            mode: "solo_mid".to_owned(),
            ..Default::default()
        };
        board.select_at(query.clone(), now);
        let (generation, _) = board.request_at(now).unwrap();
        board.receive(generation, Some((vec![], None)));
        board.select_at(
            Query {
                nickname: "a".to_owned(),
                ..query.clone()
            },
            now + Duration::from_millis(10),
        );
        board.select_at(
            Query {
                nickname: "abc".to_owned(),
                ..query
            },
            now + Duration::from_millis(80),
        );
        assert!(board.request_at(now + Duration::from_millis(379)).is_none());
        let (_, query) = board.request_at(now + Duration::from_millis(380)).unwrap();
        assert_eq!(query.nickname, "abc");
        assert!(board.request_at(now + Duration::from_secs(1)).is_none());
    }
    #[test]
    fn even_large_service_pages_show_only_the_first_100_real_rows() {
        let mut board = Board::default();
        board.select(Query {
            mode: "solo_mid".to_owned(),
            ..Default::default()
        });
        let (generation, _) = board.request().unwrap();
        let rows = (1..=120)
            .map(|rank| Row {
                account: rank as u64,
                players: Players::Humans,
                rank,
                nickname: format!("p{rank}"),
                version: None,
                games: 1,
                rating: "1500".to_owned(),
            })
            .collect();
        board.receive(generation, Some((rows, None)));
        assert_eq!(board.rows.len(), 100);
        assert_eq!(board.rows.last().unwrap().rank, 100);
        assert!(board.request().is_none());
    }
    #[test]
    fn a_throttled_old_search_still_delays_the_pending_new_search() {
        let now = Instant::now();
        let mut board = Board::default();
        let query = Query {
            mode: "solo_mid".to_owned(),
            ..Default::default()
        };
        board.select_at(query.clone(), now);
        let (generation, _) = board.request_at(now).unwrap();
        board.select_at(
            Query {
                nickname: "new".to_owned(),
                ..query
            },
            now + Duration::from_millis(1),
        );
        board.refused(
            generation,
            &Error::Throttled {
                retry_after_ms: 1000,
            },
        );
        assert!(
            board.request_at(now + Duration::from_millis(500)).is_none(),
            "throttling applies to the connection, even after filters change"
        );
    }
    #[test]
    fn rating_text_never_renders_non_finite_values() {
        assert!(rating(f64::NAN).is_none());
        assert!(rating(f64::INFINITY).is_none());
        assert_eq!(rating(1523.6).as_deref(), Some("1,524"));
        assert!(rating(f64::MAX).unwrap().len() < 32);
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    // A service-controlled delay must expire within the client's bounded retry window.
    #[test]
    fn a_hostile_service_delay_expires_within_sixty_seconds() {
        let mut board = Board::default();
        let now = Instant::now();
        board.select_at(
            Query {
                mode: "solo_mid".to_owned(),
                ..Default::default()
            },
            now,
        );
        let (generation, _) = board.request_at(now).expect("the first lookup is sent");
        board.refused(
            generation,
            &Error::Throttled {
                retry_after_ms: u32::MAX,
            },
        );
        board.select_at(
            Query {
                mode: "solo_mid".to_owned(),
                ..Default::default()
            },
            now + Duration::from_secs(1),
        );
        assert!(
            board.request_at(now + Duration::from_secs(61)).is_some(),
            "a hostile delay never blocks a retry beyond sixty seconds"
        );
    }
}
