//! What the menu's screens draw, as plain data: every string in it is safe
//! to draw (bounded, no control or hidden characters) and no password is in
//! it. Built from the [`MenuCore`] every frame, and redrawn when it changes.

use bevy::platform::time::Instant;
use bota_radovid::proto::{
    AccountId, EndReason, LobbyId, MatchId, MatchResult, Outcome, ProposalId, QueueEntryId,
    RulesetId,
};

use super::core::{Confirm, GameKind, LocalRules, MenuCore, Page, Status, clock};
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
    /// Who may play it: `Humans and Bots`, `Humans` or `Bots`.
    pub audience: &'static str,
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
    /// A match was found for it: the MATCH FOUND prompt has it, not the search box.
    pub offered: bool,
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

/// A ruleset a lobby may be made for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeRow {
    pub id: RulesetId,
    pub name: String,
    pub chosen: bool,
}

/// One member of a lobby, in a slot or looking on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberRow {
    pub account: AccountId,
    pub name: String,
    pub ready: bool,
    pub me: bool,
}

/// A slot of a team: free, or held by a member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotRow {
    pub slot: u8,
    pub who: Option<MemberRow>,
}

/// A side's slots, as the ruleset numbers them team by team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamRow {
    pub name: String,
    pub slots: Vec<SlotRow>,
}

/// Who may sit in a lobby, as its settings say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudienceRow {
    pub audience: bota_radovid::proto::Audience,
    pub name: &'static str,
    pub chosen: bool,
}

/// The settings of a lobby: read by everyone, changed by its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsView {
    /// The game mode's name.
    pub mode: String,
    pub audience: &'static str,
    pub locked: bool,
    /// The owner's choices; empty for the others.
    pub modes: Vec<ModeRow>,
    pub audiences: Vec<AudienceRow>,
    /// The owner changes them (the page has a password field then).
    pub editable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyView {
    pub name: String,
    pub locked: bool,
    pub teams: Vec<TeamRow>,
    /// Members without a slot: they look on and are not part of the match.
    pub spectators: Vec<MemberRow>,
    pub owner: bool,
    /// The member the owner is moving: their next click on a slot puts them there.
    pub moving: Option<String>,
    pub settings: SettingsView,
    /// The owner's password field.
    pub fields: Vec<FieldView>,
    pub waiting_approval: usize,
    pub me_ready: bool,
    /// Why the owner cannot start yet, as Dota's lobby says it.
    pub waiting: Option<&'static str>,
    /// The owner may start with the lobby below its seats: the empty ones stay empty.
    pub starts_short: bool,
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

/// A replay this client keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayRow {
    /// Its file name, which the buttons of its row name it by.
    pub name: String,
    pub size: String,
    /// When it was played.
    pub when: String,
    /// Where it was played: a ruleset's id, `local`, `demo`, `file`.
    pub mode: String,
    /// Who played what and how it ended, as far as known.
    pub summary: super::replay_info::Summary,
}

/// What the REPLAYS page says of the place replays are kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayShelf {
    /// The folder, on a desktop; empty in a browser, which keeps them in its own storage.
    pub folder: String,
    /// Whether replays are kept at all now (the setting, and a store to keep them in).
    pub keeping: bool,
    /// Whether there is a store to keep them in.
    pub store: bool,
    /// The service said it keeps no replays of its matches, so none of them is fetched.
    pub service_keeps_none: bool,
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
        /// A local lobby's: the rules it is to play by.
        rules: Option<LocalRules>,
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
        /// The game modes the service offers this account, and the one a new lobby is of.
        modes: Vec<ModeRow>,
    },
    Lobby(LobbyView),
    Result(ResultView),
    /// Dota's Demo Hero: every hero to try, by id and name.
    #[cfg(not(target_arch = "wasm32"))]
    DemoHero {
        heroes: Vec<(u16, String)>,
        /// The service has a Demo Hero to start.
        offered: bool,
    },
    /// The replays on this machine, and a field for any replay file.
    Replays {
        list: Vec<ReplayRow>,
        fields: Vec<FieldView>,
        shelf: ReplayShelf,
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
const LEAVE_GAME_TITLE: &str = "Confirm Leave Game";
const LEAVE_GAME_TEXT: &str =
    "Leaving the game now ends it, and you will lose it.  Do you really wish to leave the game?";
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
    /// A match under way is played from this seat: LEAVE GAME leaves it for good.
    pub can_leave_game: bool,
    pub confirm: Option<ConfirmView>,
    /// Whether there is a client to quit: a browser's page has none.
    pub can_quit: bool,
    /// Whether the local lobby's host enables cheats.
    pub host_cheats: bool,
}

impl MenuView {
    /// What the FINDING MATCH box shows: the searches no match was found for yet, and the
    /// matches kept waiting for a game server behind others. Dota swaps the box for the accept
    /// prompt as soon as a match is found, and for the game once it starts.
    pub fn finding(&self) -> (Vec<&SearchRow>, Vec<&WaitRow>) {
        (
            self.searches.iter().filter(|s| !s.offered).collect(),
            self.waits
                .iter()
                .filter(|w| w.ahead > 0 || w.overloaded)
                .collect(),
        )
    }

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
            .map(|me| text::line(&me.nickname, text::NAME));
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
            can_leave_game: self.in_game() && self.kind == GameKind::Play && self.game_unsafe,
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
                Confirm::LeaveGame => ConfirmView {
                    title: LEAVE_GAME_TITLE,
                    text: LEAVE_GAME_TEXT,
                },
                Confirm::Leave => ConfirmView {
                    title: LEAVE_TITLE,
                    text: LEAVE_TEXT,
                },
            }),
            can_quit: !cfg!(target_arch = "wasm32"),
            host_cheats: self.prefs.host_cheats,
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
                focused: Some(at) == self.form.focused_field(),
            })
            .collect()
    }

    /// The game modes a new lobby may be of: what the service offers the account, the chosen one
    /// marked (the first until one is).
    fn modes(&self) -> Vec<ModeRow> {
        let Some(client) = &self.client else {
            return Vec::new();
        };
        let chosen = self.lobby_mode(client);
        super::core::lobby_rulesets(client)
            .map(|r| ModeRow {
                id: r.id.clone(),
                name: text::line(&r.name, text::NAME),
                chosen: Some(&r.id) == chosen.as_ref(),
            })
            .collect()
    }

    fn cooldown(&self) -> Option<String> {
        let left = self.client.as_ref()?.searches().cooldown_left_ms();
        (left > 0).then(|| format!("You can search again in {}", clock(left.div_ceil(1000))))
    }

    fn body(&self) -> PageView {
        match self.page {
            Page::Service | Page::Login | Page::Register | Page::LocalLobby => PageView::Form {
                kind: self.form.kind,
                fields: self.fields(),
                busy: self.connecting() && self.page != Page::LocalLobby,
                plain_host: self.plain_host.as_ref().map(|h| text::line(h, text::NAME)),
                rules: (self.page == Page::LocalLobby).then(|| self.rules_choice()),
                hosted_at: self.hosted_at().map(str::to_owned),
            },
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
                modes: self.modes(),
            },
            Page::Lobby => self
                .lobby
                .as_ref()
                .map_or(PageView::Joining, |(state, at)| {
                    let mut view = lobby_view(state, *at, self.client.as_ref(), self.moving);
                    view.fields = self.fields();
                    PageView::Lobby(view)
                }),
            Page::Result => self
                .shown
                .and_then(|id| self.results.iter().find(|r| r.match_id == id))
                .map_or(PageView::Joining, |r| PageView::Result(self.result_view(r))),
            Page::Replays => PageView::Replays {
                list: self
                    .library
                    .records()
                    .iter()
                    .map(|r| ReplayRow {
                        name: r.name.clone(),
                        size: if r.bytes < 1_000_000 {
                            format!("{} KB", r.bytes.div_ceil(1000))
                        } else {
                            format!("{:.1} MB", r.bytes as f64 / 1e6)
                        },
                        when: super::replay_info::when(
                            std::time::UNIX_EPOCH + std::time::Duration::from_secs(r.when),
                        ),
                        mode: r.mode.clone(),
                        summary: self.summary_of(r),
                    })
                    .collect(),
                fields: self.fields(),
                shelf: ReplayShelf {
                    folder: self
                        .library
                        .dir()
                        .map(|d| text::line(&d.display().to_string(), text::MESSAGE))
                        .unwrap_or_default(),
                    keeping: self.library.keeping(),
                    store: self.library.standing() != crate::replays::Standing::Unavailable,
                    service_keeps_none: self.service_keeps_none(),
                },
            },
            #[cfg(not(target_arch = "wasm32"))]
            Page::DemoHero => PageView::DemoHero {
                offered: self
                    .client
                    .as_ref()
                    .is_some_and(|c| c.rulesets().any(bota_radovid::is_demo)),
                heroes: bota_core::HEROES
                    .iter()
                    .enumerate()
                    .filter_map(|(id, hero)| Some((u16::try_from(id).ok()?, hero.name.to_owned())))
                    .collect(),
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
                // Play lists what Find Match queues (radovid: a ruleset with a queue is rated);
                // the others are for lobbies, picked where one is made.
                .filter(|r| r.rated)
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
                    audience: audience_name(r.audience),
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
            // A match that ran leaves a replay, a lost one too; one nobody came to leaves none.
            replay: result.reason != EndReason::Abandoned && !self.service_keeps_none(),
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
            EndReason::Abandoned => " · nobody played",
            EndReason::Crashed => " · the game server failed",
            EndReason::Stopped => " · stopped by the service",
            EndReason::Left => " · a player left",
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

/// Who a ruleset is for, as a subtitle says.
fn audience_name(audience: bota_radovid::proto::Audience) -> &'static str {
    use bota_radovid::proto::Audience;
    match audience {
        Audience::Everyone => "Humans and Bots",
        Audience::HumansOnly => "Humans",
        Audience::BotsOnly => "Bots",
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
            offered: s.offer.is_some(),
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
        owner: text::line(&l.owner_nickname, text::NAME),
        locked: l.locked,
        members: l.members,
        seats: l.seats,
    }
}

/// What a lobby page shows: the slot grid by team, those looking on, and the settings.
fn lobby_view(
    state: &bota_radovid::proto::LobbyState,
    at: Instant,
    client: Option<&bota_radovid::Client>,
    moving: Option<AccountId>,
) -> LobbyView {
    use bota_radovid::proto::{Audience, LOBBY_NO_SLOT};
    let me = client.and_then(|c| c.me()).map(|me| me.id);
    let elapsed = u32::try_from(at.elapsed().as_millis()).unwrap_or(u32::MAX);
    let rules = client.and_then(|c| c.rulesets().find(|r| r.id == state.ruleset));
    let sizes: Vec<u8> = state.teams.clone();
    let needed = rules.map_or(usize::MAX, |r| usize::from(r.lobby_min_players));
    let owner = Some(state.owner) == me;
    let row = |m: &bota_radovid::proto::LobbyMember| MemberRow {
        account: m.account,
        name: text::line(&m.nickname, text::NAME),
        ready: m.ready,
        me: Some(m.account) == me,
    };
    let mut first = 0u8;
    let teams: Vec<TeamRow> = sizes
        .iter()
        .enumerate()
        .map(|(team, size)| {
            let slots = (first..first.saturating_add(*size))
                .map(|slot| SlotRow {
                    slot,
                    who: state.members.iter().find(|m| m.slot == slot).map(row),
                })
                .collect();
            first = first.saturating_add(*size);
            TeamRow {
                name: match team {
                    0 => "RADIANT".to_owned(),
                    1 => "DIRE".to_owned(),
                    n => format!("TEAM {}", n + 1),
                },
                slots,
            }
        })
        .collect();
    let spectators: Vec<MemberRow> = state
        .members
        .iter()
        .filter(|m| m.slot == LOBBY_NO_SLOT)
        .map(row)
        .collect();
    let seated = state.members.len() - spectators.len();
    let seats: usize = sizes.iter().map(|s| usize::from(*s)).sum();
    let others_unready = state
        .members
        .iter()
        .any(|m| Some(m.account) != me && m.slot != LOBBY_NO_SLOT && !m.ready);
    let i_am_looking = spectators.iter().any(|m| m.me);
    let waiting = if !owner {
        None
    } else if i_am_looking || seated < needed {
        Some("Waiting for more players")
    } else if others_unready {
        Some("Waiting for the other players to be ready")
    } else {
        None
    };
    let audience_name = |audience: Audience| match audience {
        Audience::Everyone => "Humans and Bots",
        Audience::HumansOnly => "Humans",
        Audience::BotsOnly => "Bots",
    };
    let settings = SettingsView {
        mode: rules.map_or_else(
            || text::line(&state.ruleset.0, text::NAME),
            |r| text::line(&r.name, text::NAME),
        ),
        audience: audience_name(state.audience),
        locked: state.locked,
        modes: if owner {
            client.map_or_else(Vec::new, |c| {
                super::core::lobby_rulesets(c)
                    .map(|r| ModeRow {
                        id: r.id.clone(),
                        name: text::line(&r.name, text::NAME),
                        chosen: r.id == state.ruleset,
                    })
                    .collect()
            })
        } else {
            Vec::new()
        },
        audiences: if owner {
            [Audience::Everyone, Audience::HumansOnly, Audience::BotsOnly]
                .into_iter()
                .map(|audience| AudienceRow {
                    audience,
                    name: audience_name(audience),
                    chosen: audience == state.audience,
                })
                .collect()
        } else {
            Vec::new()
        },
        editable: owner,
    };
    LobbyView {
        name: text::line(&state.name, text::NAME),
        locked: state.locked,
        teams,
        owner,
        moving: moving
            .and_then(|id| state.members.iter().find(|m| m.account == id))
            .map(|m| text::line(&m.nickname, text::NAME)),
        spectators,
        settings,
        fields: Vec::new(),
        waiting_approval: state.pending.len(),
        me_ready: state
            .members
            .iter()
            .any(|m| Some(m.account) == me && m.ready),
        waiting,
        starts_short: owner && waiting.is_none() && seated < seats,
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
