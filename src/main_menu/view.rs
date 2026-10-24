//! What the menu's screens draw, as plain data: every string in it is safe
//! to draw (bounded, no control or hidden characters) and no password is in
//! it. Built from the [`MenuCore`] every frame, and redrawn when it changes.

use bevy::platform::time::Instant;
use bota_radovid::proto::{
    EndReason, LobbyId, MatchId, MatchResult, Outcome, ProposalId, QueueEntryId, RulesetId,
};

use super::core::{Confirm, GameKind, MenuCore, Page, SeatFill, Status, clock};
use super::form::FormKind;
use super::text;

/// One text field as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldView {
    pub label: &'static str,
    /// The text, or dots for a password.
    pub shown: String,
    pub focused: bool,
}

/// A ruleset to search a game in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesetRow {
    pub id: RulesetId,
    pub name: String,
    /// Team sizes, `1v1`.
    pub teams: String,
    pub rated: bool,
}

/// A queue entry and where it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchRow {
    pub entry: QueueEntryId,
    pub ruleset: String,
    pub waited: String,
    pub position: u32,
    pub waiting: u32,
    /// Every game server busy: nobody is paired until one frees.
    pub overloaded: bool,
    pub games_waiting: u32,
}

/// The MATCH FOUND prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferBox {
    pub proposal: ProposalId,
    pub ruleset: String,
    pub left_secs: u32,
    /// The share of the time to answer that is left, 0 to 1000.
    pub left_permille: u32,
    pub accepted: u8,
    pub of: u8,
    /// This player accepted; the others have yet to.
    pub answered: bool,
    /// Which search it is, when there are several: `Search 2 of 3`.
    pub which: Option<String>,
}

/// An accepted match waiting for a game server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitRow {
    pub match_id: MatchId,
    pub ahead: u32,
    pub overloaded: bool,
}

/// An unfinished match of the account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRow {
    pub match_id: MatchId,
    pub label: String,
    /// The game server is up: it can be joined or watched.
    pub ready: bool,
}

/// A finished match of the account in MY GAMES.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishedRow {
    pub match_id: MatchId,
    /// VICTORY, DEFEAT, DRAW or NO RESULT.
    pub headline: String,
    pub won: Option<bool>,
    /// `Match 7 · 4:36 · rated`.
    pub detail: String,
    /// `1662 (+162)` for a rated match with a result.
    pub rating: String,
    /// The game ran, so a replay may be kept.
    pub replay: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyRow {
    pub id: LobbyId,
    pub name: String,
    pub owner: String,
    pub locked: bool,
    pub members: u8,
    pub seats: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyView {
    pub name: String,
    pub locked: bool,
    /// Name, team, ready.
    pub members: Vec<(String, u8, bool)>,
    pub waiting_approval: usize,
    pub me_ready: bool,
    /// Why the owner cannot start yet, as Dota's lobby says it.
    pub waiting: Option<&'static str>,
    pub starting_in_secs: Option<u32>,
}

/// One seat of a finished match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeatRow {
    pub name: String,
    pub team: u8,
    pub hero: String,
    /// `1512 (+12)` for a rated match.
    pub rating: String,
    pub mine: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultView {
    /// VICTORY, DEFEAT, DRAW or NO RESULT.
    pub headline: String,
    pub won: Option<bool>,
    pub detail: String,
    pub seats: Vec<SeatRow>,
}

/// A replay on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayRow {
    pub name: String,
    pub size: String,
    /// When it was played.
    pub when: String,
    /// Who played what and how it ended, as far as read.
    pub summary: super::replay_info::Summary,
}

/// The page under the top bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageView {
    /// A page of fields: the service, login, registration, direct connect.
    Form {
        kind: FormKind,
        fields: Vec<FieldView>,
        /// Shown while the service is being reached.
        busy: bool,
        /// Plain TCP to this host: the notice, and a button to go on.
        plain_host: Option<String>,
        /// A local lobby's: what its empty seats take, and whether a bot
        /// command is set.
        fill: Option<(SeatFill, bool)>,
        /// The address of the local lobby this client hosts now.
        hosted_at: Option<String>,
    },
    Home {
        rulesets: Vec<RulesetRow>,
        /// Games in flight and the account's limit.
        in_flight: (usize, u8),
        cooldown: Option<String>,
    },
    MyGames {
        games: Vec<GameRow>,
        /// The account's latest finished matches, newest first.
        finished: Vec<FinishedRow>,
        cooldown: Option<String>,
    },
    /// Ratings per ruleset and the ladder's head.
    Profile {
        name: String,
        ratings: Vec<String>,
        ladder: Vec<String>,
    },
    Lobbies {
        list: Vec<LobbyRow>,
        fields: Vec<FieldView>,
    },
    Lobby(LobbyView),
    Result(ResultView),
    /// The replays on this machine, and a field for any replay file.
    Replays {
        list: Vec<ReplayRow>,
        fields: Vec<FieldView>,
    },
    /// The settings screen, which draws itself over the menu.
    Settings,
    /// A game is being opened.
    Joining,
    /// A game is on screen: only the overlays show.
    Hidden,
}

/// A question over the page, as Dota's popups ask it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmView {
    pub title: &'static str,
    pub text: &'static str,
}

/// Dota's quit popup (`DOTA_ConfirmQuit`), with this client's name.
const QUIT_TITLE: &str = "Confirm Quit";
const QUIT_TEXT: &str = "Do you really wish to quit dbota?";
/// The same when a seat is left in a game under way (`DOTA_ConfirmQuitDescUnsafe`).
const QUIT_TEXT_UNSAFE: &str = "Leaving a game before it has finished may ruin the game for \
other players.  Do you really wish to quit dbota?";
/// Leaving a match under way for good (worded after `DOTA_ConfirmQuitDescUnsafe`; Dota's own
/// text for LEAVE GAME was not available, so this is an approximation).
const ABANDON_TITLE: &str = "Confirm Leave Game";
const ABANDON_TEXT: &str = "Leaving the game now abandons it, and you will lose it.  Do you really wish to leave the game?";
/// Dota's popup for leaving a game watched or a replay
/// (`popup_confirm_close_replay`).
const LEAVE_TITLE: &str = "Confirm Disconnect";
const LEAVE_TEXT: &str = "Are you sure you'd like to leave the game?";

/// Everything the menu draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuView {
    pub page: Page,
    pub body: PageView,
    /// `nickname#id` once logged in.
    pub account: Option<String>,
    pub status: Option<Status>,
    pub searches: Vec<SearchRow>,
    pub waits: Vec<WaitRow>,
    pub offer: Option<OfferBox>,
    /// The game under the dashboard, and how it is had.
    pub game: Option<GameKind>,
    /// A seat left that can be taken again (Dota's RECONNECT).
    pub reconnect: bool,
    /// A match under way is played from this seat: LEAVE GAME abandons it.
    pub can_abandon: bool,
    pub confirm: Option<ConfirmView>,
    /// Whether there is a client to quit: a browser's page has none.
    pub can_quit: bool,
}

impl MenuView {
    /// Whether the menu covers the screen.
    pub fn covers(&self) -> bool {
        self.body != PageView::Hidden
    }
}

impl MenuCore {
    /// What the screens draw now.
    pub fn view(&self) -> MenuView {
        let client = self.client.as_ref().filter(|_| self.logged_in());
        let account = client
            .and_then(|c| c.me())
            .map(|me| text::who(&me.nickname, me.id.0));
        let (searches, waits, offer) = client.map_or_else(Default::default, |c| {
            let names = |id: &RulesetId| ruleset_name(c, id);
            (search_rows(c, &names), wait_rows(c), offer_box(c, &names))
        });
        MenuView {
            page: self.page,
            body: self.body(),
            account,
            status: self.status.clone(),
            searches,
            waits,
            offer,
            game: self.in_game().then_some(self.kind),
            can_abandon: self.in_game() && self.kind == GameKind::Play && self.game_unsafe,
            reconnect: self.reconnect.is_some() && !self.in_game() && !self.launching,
            confirm: self.confirm.map(|c| match c {
                Confirm::Quit => ConfirmView {
                    title: QUIT_TITLE,
                    text: if self.game_unsafe {
                        QUIT_TEXT_UNSAFE
                    } else {
                        QUIT_TEXT
                    },
                },
                Confirm::Abandon => ConfirmView {
                    title: ABANDON_TITLE,
                    text: ABANDON_TEXT,
                },
                Confirm::Leave => ConfirmView {
                    title: LEAVE_TITLE,
                    text: LEAVE_TEXT,
                },
            }),
            can_quit: !cfg!(target_arch = "wasm32"),
        }
    }

    fn fields(&self) -> Vec<FieldView> {
        self.form
            .fields()
            .iter()
            .enumerate()
            .map(|(at, f)| FieldView {
                label: f.label,
                shown: f.shown(),
                focused: at == self.form.focused(),
            })
            .collect()
    }

    fn cooldown(&self) -> Option<String> {
        let left = self.client.as_ref()?.searches().cooldown_left_ms();
        (left > 0).then(|| format!("You can search again in {}", clock(left.div_ceil(1000))))
    }

    fn body(&self) -> PageView {
        match self.page {
            Page::Service | Page::Login | Page::Register | Page::Direct | Page::LocalLobby => {
                PageView::Form {
                    kind: self.form.kind,
                    fields: self.fields(),
                    busy: self.connecting() && self.page != Page::Direct,
                    plain_host: self.plain_host.as_ref().map(|h| text::line(h, text::NAME)),
                    fill: (self.page == Page::LocalLobby).then(|| self.fill_choice()),
                    hosted_at: self.hosted_at().map(str::to_owned),
                }
            }
            Page::Home => self.home(),
            Page::MyGames => PageView::MyGames {
                games: self
                    .games
                    .iter()
                    .map(|g| game_row(g.match_id, g.handoff.is_some()))
                    .collect(),
                finished: self.history.iter().map(|r| self.finished_row(r)).collect(),
                cooldown: self.cooldown(),
            },
            Page::Profile => self.profile_view(),
            Page::Lobbies => PageView::Lobbies {
                list: self
                    .lobbies
                    .iter()
                    .take(bota_radovid::proto::MAX_LOBBY_LIST)
                    .map(lobby_row)
                    .collect(),
                fields: self.fields(),
            },
            Page::Lobby => self
                .lobby
                .as_ref()
                .map_or(PageView::Joining, |(state, at)| {
                    PageView::Lobby(lobby_view(state, *at, self.client.as_ref()))
                }),
            Page::Result => self
                .shown
                .and_then(|id| self.results.iter().find(|r| r.match_id == id))
                .map_or(PageView::Joining, |r| PageView::Result(self.result_view(r))),
            Page::Replays => PageView::Replays {
                list: self
                    .replays
                    .iter()
                    .map(|r| ReplayRow {
                        name: r.name.clone(),
                        size: r.size.clone(),
                        when: r.when.clone(),
                        summary: self.shelf.peek(&r.path, r.stamp),
                    })
                    .collect(),
                fields: self.fields(),
            },
            Page::Settings => PageView::Settings,
            Page::Game if self.launching => PageView::Joining,
            Page::Game => PageView::Hidden,
        }
    }

    fn home(&self) -> PageView {
        let Some(client) = &self.client else {
            return PageView::Joining;
        };
        let searches = client.searches();
        let in_flight = searches.entries().len() + searches.slot_waits().len() + self.games.len();
        PageView::Home {
            rulesets: client
                .rulesets()
                .map(|r| RulesetRow {
                    id: r.id.clone(),
                    name: text::line(&r.name, text::NAME),
                    teams: r
                        .teams
                        .iter()
                        .map(u8::to_string)
                        .collect::<Vec<_>>()
                        .join("v"),
                    rated: r.rated,
                })
                .collect(),
            in_flight: (
                in_flight,
                client.limits(None).map_or(0, |l| l.games_per_account),
            ),
            cooldown: self.cooldown(),
        }
    }

    fn finished_row(&self, result: &MatchResult) -> FinishedRow {
        let view = self.result_view(result);
        FinishedRow {
            match_id: result.match_id,
            rating: view
                .seats
                .iter()
                .find(|s| s.mine)
                .map(|s| s.rating.clone())
                .unwrap_or_default(),
            headline: view.headline,
            won: view.won,
            detail: view.detail,
            replay: matches!(
                result.reason,
                EndReason::Decided | EndReason::Draw | EndReason::Abandon
            ),
        }
    }

    fn profile_view(&self) -> PageView {
        let name = self
            .account_profile
            .as_ref()
            .map_or_else(String::new, |p| text::who(&p.nickname, p.id.0));
        let ratings = self.client.as_ref().map_or_else(Vec::new, |client| {
            self.account_profile
                .iter()
                .flat_map(|p| &p.ratings)
                .map(|(id, r)| {
                    format!(
                        "{}: {} ± {} · {} games, {} won, {} drawn, {} lost",
                        ruleset_name(client, id),
                        r.rating.rating.round() as i64,
                        r.rating.deviation.round() as i64,
                        r.games,
                        r.wins,
                        r.draws,
                        r.losses
                    )
                })
                .collect()
        });
        let ladder = self
            .ladder
            .iter()
            .map(|row| {
                format!(
                    "{:>3}  {}  {}  ({} games)",
                    row.rank,
                    text::who(&row.nickname, row.account.0),
                    row.rating.rating.round() as i64,
                    row.games
                )
            })
            .collect();
        PageView::Profile {
            name,
            ratings,
            ladder,
        }
    }

    fn result_view(&self, result: &MatchResult) -> ResultView {
        let me = self.client.as_ref().and_then(|c| c.me()).map(|me| me.id);
        let mine = result.seats.iter().find(|s| Some(s.account) == me);
        let won = match result.outcome {
            Outcome::Winner { team } => mine.map(|s| s.team == team),
            _ => None,
        };
        let headline = match (result.outcome, won) {
            (_, Some(true)) => "VICTORY",
            (_, Some(false)) => "DEFEAT",
            (Outcome::Draw, _) => "DRAW",
            (Outcome::Winner { .. }, None) => "MATCH OVER",
            (Outcome::Null, _) => "NO RESULT",
        };
        let reason = match result.reason {
            EndReason::Decided | EndReason::Draw => "",
            EndReason::Abandoned => " · abandoned: nobody stayed",
            EndReason::Crashed => " · the game server failed",
            EndReason::Stopped => " · stopped by the service",
            EndReason::Abandon => " · a player left",
        };
        let report = bota_radovid::decode_report(result);
        let hero = |slot: u8| {
            report
                .as_ref()
                .and_then(|r| r.picks.iter().find(|p| p.slot.0 == slot))
                .and_then(|p| bota_core::HEROES.get(usize::from(p.hero.0)))
                .map_or("—", |h| h.name)
                .to_owned()
        };
        let seats = result
            .seats
            .iter()
            .map(|s| SeatRow {
                name: text::who(&s.nickname, s.account.0),
                team: s.team,
                hero: hero(s.slot),
                rating: rating_change(s.before, s.after),
                mine: Some(s.account) == me,
            })
            .collect();
        ResultView {
            headline: headline.to_owned(),
            won,
            detail: format!(
                "Match {} · {}{}{reason}",
                result.match_id.0,
                clock(result.duration_ms / 1000),
                if result.rated { " · rated" } else { "" },
            ),
            seats,
        }
    }
}

fn ruleset_name(client: &bota_radovid::Client, id: &RulesetId) -> String {
    client.rulesets().find(|r| &r.id == id).map_or_else(
        || text::line(&id.0, text::NAME),
        |r| text::line(&r.name, text::NAME),
    )
}

fn search_rows(
    client: &bota_radovid::Client,
    names: &dyn Fn(&RulesetId) -> String,
) -> Vec<SearchRow> {
    client
        .searches()
        .entries()
        .iter()
        .map(|s| SearchRow {
            entry: s.entry,
            ruleset: names(&s.ruleset),
            waited: clock(s.waited_ms() / 1000),
            position: s.position,
            waiting: s.waiting,
            overloaded: s.overloaded,
            games_waiting: s.games_waiting,
        })
        .collect()
}

fn wait_rows(client: &bota_radovid::Client) -> Vec<WaitRow> {
    client
        .searches()
        .slot_waits()
        .iter()
        .map(|w| WaitRow {
            match_id: w.match_id,
            ahead: w.ahead,
            overloaded: w.overloaded,
        })
        .collect()
}

/// The oldest open proposal: unanswered ones first.
fn offer_box(
    client: &bota_radovid::Client,
    names: &dyn Fn(&RulesetId) -> String,
) -> Option<OfferBox> {
    let searches = client.searches();
    let count = searches.entries().len();
    let (at, search, offer) = searches
        .entries()
        .iter()
        .enumerate()
        .filter_map(|(at, s)| Some((at, s, s.offer.as_ref()?)))
        .min_by_key(|(_, _, o)| o.answered)?;
    let total = client
        .limits(None)
        .map_or(30_000, |l| l.accept_timeout_ms)
        .max(1);
    let left = offer.left_ms();
    Some(OfferBox {
        proposal: offer.proposal,
        ruleset: names(&search.ruleset),
        left_secs: left.div_ceil(1000),
        left_permille: (u64::from(left.min(total)) * 1000 / u64::from(total)) as u32,
        accepted: offer.accepted,
        of: offer.of,
        answered: offer.answered,
        which: (count > 1).then(|| format!("Search {} of {count}", at + 1)),
    })
}

fn game_row(match_id: MatchId, ready: bool) -> GameRow {
    GameRow {
        match_id,
        label: format!("Match {}", match_id.0),
        ready,
    }
}

fn lobby_row(l: &bota_radovid::proto::LobbySummary) -> LobbyRow {
    LobbyRow {
        id: l.id,
        name: text::line(&l.name, text::NAME),
        owner: text::who(&l.owner_nickname, l.owner.0),
        locked: l.locked,
        members: l.members,
        seats: l.seats,
    }
}

fn lobby_view(
    state: &bota_radovid::proto::LobbyState,
    at: Instant,
    client: Option<&bota_radovid::Client>,
) -> LobbyView {
    let me = client.and_then(|c| c.me()).map(|me| me.id);
    let elapsed = u32::try_from(at.elapsed().as_millis()).unwrap_or(u32::MAX);
    let seats = client
        .and_then(|c| c.rulesets().find(|r| r.id == state.ruleset))
        .map_or(0, |r| r.teams.iter().map(|t| usize::from(*t)).sum());
    let others_unready = state
        .members
        .iter()
        .any(|m| Some(m.account) != me && !m.ready);
    let waiting = if Some(state.owner) != me {
        None
    } else if state.members.len() < seats {
        Some("Waiting for more players")
    } else if others_unready {
        Some("Waiting for the other players to be ready")
    } else {
        None
    };
    LobbyView {
        name: text::line(&state.name, text::NAME),
        locked: state.locked,
        members: state
            .members
            .iter()
            .take(bota_radovid::proto::MAX_SEATS)
            .map(|m| (text::who(&m.nickname, m.account.0), m.team, m.ready))
            .collect(),
        waiting_approval: state.pending.len(),
        me_ready: state
            .members
            .iter()
            .any(|m| Some(m.account) == me && m.ready),
        waiting,
        starting_in_secs: state
            .starting_in_ms
            .map(|ms| ms.saturating_sub(elapsed).div_ceil(1000)),
    }
}

fn rating_change(
    before: Option<bota_radovid::proto::Rating>,
    after: Option<bota_radovid::proto::Rating>,
) -> String {
    match (before, after) {
        (Some(b), Some(a)) => {
            let (b, a) = (b.rating.round() as i64, a.rating.round() as i64);
            format!("{a} ({:+})", a - b)
        }
        _ => String::new(),
    }
}
