//! What the menu's screens draw, as plain data: every string in it is safe
//! to draw (bounded, no control or hidden characters) and no password is in
//! it. Built from the [`MenuCore`] every frame, and redrawn when it changes.

use bevy::platform::time::Instant;
use bota_radovid::proto::{
    AccountGameLimit, AccountId, EndReason, LobbyId, MatchId, MatchResult, Outcome, ProposalId,
    QueueEntryId, RulesetId,
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

/// The password dialog of a locked lobby being joined: its name, the field, and what the last
/// try was told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordView {
    pub lobby: String,
    pub field: FieldView,
    pub error: Option<String>,
}

/// A ruleset to search a game in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesetRow {
    pub id: RulesetId,
    pub name: String,
    /// Team sizes, `1v1`.
    pub teams: String,
    pub rated: bool,
    pub description: String,
}

/// A live search; queue position and size stay out of the drawing model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchRow {
    pub entry: QueueEntryId,
    pub ruleset: String,
    pub waited: String,
    /// Every game server busy: nobody is paired until one frees.
    pub overloaded: bool,
    pub games_waiting: u32,
    pub hint: Option<&'static str>,
    /// A match was found for it: the MATCH FOUND prompt has it, not the search box.
    pub offered: bool,
}

/// The MATCH FOUND prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferBox {
    pub proposal: ProposalId,
    pub ruleset: String,
    pub left_ms: u32,
    /// The share of the time to answer that is left, 0 to 1000.
    pub left_permille: Option<u32>,
    pub accepted: u8,
    pub of: u8,
    /// This player accepted; the others have yet to.
    pub answered: bool,
    pub decline_cooldown_ms: Option<u32>,
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
    pub mode: String,
    pub ruleset: RulesetId,
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
    pub bot: bool,
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
    /// The game's cheats: none when the mode has none, else whether they are on.
    pub cheats: Option<bool>,
    /// The owner changes them (the page has a password field then).
    pub editable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyBlock {
    Slot,
    Players(usize),
    Ready(usize),
    Mode,
}
impl LobbyBlock {
    pub fn message(&self) -> String {
        match self {
            Self::Slot => "Join a slot to start".into(),
            Self::Players(count) => format!(
                "Waiting for {count} more {}",
                if *count == 1 { "player" } else { "players" }
            ),
            Self::Ready(count) => format!(
                "Waiting for {count} {} to be ready",
                if *count == 1 { "player" } else { "players" }
            ),
            Self::Mode => "This game mode is unavailable".into(),
        }
    }
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
    pub me_ready: bool,
    /// Why the owner cannot start yet, as Dota's lobby says it.
    pub waiting: Option<LobbyBlock>,
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
    Heroes,
    MatchHistory(super::history_page::View),
    Hero {
        hero: bota_proto::HeroId,
        record: String,
    },
    Leaderboard {
        board: super::leaderboard::Board,
        field: FieldView,
    },
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
        matches: std::sync::Arc<Vec<super::history::Match>>,
        hero: Option<bota_proto::HeroId>,
        rulesets: Vec<RulesetRow>,
        /// Games in flight and the account's limit.
        in_flight: (usize, Option<AccountGameLimit>),
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
        ratings: Vec<super::account_page::Rating>,
        avatar: super::avatar::View,
        matches: std::sync::Arc<Vec<super::history::Match>>,
    },
    /// The open lobbies (all, or those the search found), and the fields to make one and to
    /// search.
    Lobbies {
        list: Vec<LobbyRow>,
        fields: Vec<FieldView>,
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
    /// A game is being opened.
    Joining,
    Loading {
        details: crate::loading::Details,
        progress_permille: u16,
        elapsed_secs: u32,
    },
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
/// The same while the server of the lobby this client hosts runs: quitting ends it, and the game
/// in it for everyone, whoever is still playing.
const QUIT_TEXT_HOSTING: &str = "Quitting ends the game you host for everyone in it.  Do you \
really wish to quit dbota?";
/// Hosting another lobby while this client's lobby server runs ends it.
const REHOST_TITLE: &str = "Confirm New Lobby";
const REHOST_TEXT: &str = "A new lobby ends the game you host now for everyone in it.  Do you \
really wish to host a new lobby?";
/// The same when a seat is left in a game under way (`DOTA_ConfirmQuitDescUnsafe`).
const QUIT_TEXT_UNSAFE: &str = "Leaving a game before it has finished may ruin the game for \
other players.  Do you really wish to quit dbota?";
/// Leaving a match under way for good (worded after `DOTA_ConfirmQuitDescUnsafe`; Dota's own
/// text for LEAVE GAME was not available, so this is an approximation).
const LEAVE_GAME_TITLE: &str = "Confirm Leave Game";
const LEAVE_GAME_TEXT: &str =
    "Leaving the game now ends it, and you will lose it.  Do you really wish to leave the game?";
/// The same while the heroes are being picked: the match is cancelled for everyone.
const LEAVE_PICKS_TEXT: &str = "The heroes are still being picked: leaving now cancels the match \
for everyone, and no result is recorded.  Do you really wish to leave the game?";
/// Dota's popup for leaving a game watched or a replay
/// (`popup_confirm_close_replay`).
const LEAVE_TITLE: &str = "Confirm Disconnect";
const LEAVE_TEXT: &str = "Are you sure you'd like to leave the game?";

/// How the menu stands with the matchmaking service, as the top bar's pill says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Connection {
    /// Signed in to the service of this name.
    Online(String),
    /// Signed in, and the link to the service of this name was lost: it is being opened again.
    Lost(String),
    /// No account: the menu plays without the service.
    Offline,
}

/// The side of the player's last match, whose main-menu background the menu shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    #[default]
    Radiant,
    Dire,
}

/// Everything the menu draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuView {
    pub lobby_ui: super::lobby_controls::ControlsView,
    pub page: Page,
    /// The page drawn: the one the settings were opened from while they are open over it.
    pub beneath: Page,
    pub body: PageView,
    pub play: super::play::PlayState,
    pub queue_full: bool,
    pub cooldown_left: Option<String>,
    /// The account's nickname once logged in: no account number (the owner's rule).
    pub account: Option<String>,
    /// The name the player goes by: the account's, else the settings' profile name.
    pub player: String,
    pub connection: Connection,
    pub side: Side,
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
    pub password: Option<PasswordView>,
    /// Whether there is a client to quit: a browser's page has none.
    pub can_quit: bool,
    /// The platform can start a game server on this machine.
    pub can_host: bool,
    /// Whether the local lobby's host enables cheats.
    pub host_cheats: bool,
}

impl MenuView {
    pub fn loading(&self) -> bool {
        matches!(self.body, PageView::Loading { .. })
    }

    /// What the FINDING MATCH box shows: the searches no match was found for yet, and the
    /// matches kept waiting for a game server behind others. Dota swaps the box for the accept
    /// prompt as soon as a match is found, and for the game once it starts.
    pub fn finding(&self) -> (Vec<&SearchRow>, Vec<&WaitRow>) {
        if self.offer.is_some() {
            return (Vec::new(), Vec::new());
        }
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
        #[cfg(test)]
        self.lobby_ui
            .view_builds
            .set(self.lobby_ui.view_builds.get() + 1);
        let client = self.client.as_ref().filter(|_| self.logged_in());
        let account = client
            .and_then(|c| c.me())
            .map(|me| text::line(&me.nickname, text::NAME));
        let (searches, waits, offer) = client.map_or_else(Default::default, |c| {
            let names = |id: &RulesetId| ruleset_name(&self.catalogue, id);
            (
                search_rows(c, &names, &self.catalogue),
                wait_rows(c),
                offer_box(c, &names, &self.accepted, &self.catalogue),
            )
        });
        let service = text::line(service_name(&self.prefs.service), text::NAME);
        let connection = match (&account, self.reconnecting()) {
            (None, _) => Connection::Offline,
            (Some(_), false) => Connection::Online(service),
            (Some(_), true) => Connection::Lost(service),
        };
        MenuView {
            lobby_ui: if matches!(self.page_beneath(), Page::Lobbies | Page::Lobby) {
                self.lobby_menu_view(&connection)
            } else {
                Default::default()
            },
            page: self.page,
            beneath: self.page_beneath(),
            body: self.body_at(Instant::now()),
            play: self.play.clone(),
            queue_full: self
                .queue_full_until
                .is_some_and(|until| until > Instant::now()),
            cooldown_left: client.and_then(|c| {
                let left = c.searches().cooldown_left_ms();
                (left > 0).then(|| clock(left.div_ceil(1000)))
            }),
            player: account
                .clone()
                .unwrap_or_else(|| text::line(&self.name(), text::NAME)),
            connection,
            side: self.last_side(),
            account,
            status: self.status.clone(),
            searches,
            waits,
            offer,
            game: self.in_game().then_some(self.kind),
            can_leave_game: self.in_game() && self.kind == GameKind::Play && self.game_unsafe,
            reconnect: self.reconnect.is_some() && !self.in_game() && !self.launching,
            confirm: self.confirm.map(|choice| self.confirm_view(choice)),
            password: self.join_prompt.as_ref().map(|prompt| PasswordView {
                lobby: text::line(&prompt.name, text::NAME),
                field: fields_of(&prompt.form)
                    .into_iter()
                    .next()
                    .expect("the dialog has its password field"),
                error: prompt.error.clone(),
            }),
            can_quit: !cfg!(target_arch = "wasm32"),
            can_host: !cfg!(target_arch = "wasm32"),
            host_cheats: self.prefs.host_cheats,
        }
    }

    fn confirm_view(&self, c: Confirm) -> ConfirmView {
        match c {
            Confirm::Quit => ConfirmView {
                title: QUIT_TITLE,
                text: if self.hosting {
                    QUIT_TEXT_HOSTING
                } else if self.game_unsafe {
                    QUIT_TEXT_UNSAFE
                } else {
                    QUIT_TEXT
                },
            },
            Confirm::Rehost(_) => ConfirmView {
                title: REHOST_TITLE,
                text: REHOST_TEXT,
            },
            Confirm::LeaveGame | Confirm::LeaveSeat => ConfirmView {
                title: LEAVE_GAME_TITLE,
                // In the picks the server cancels the match instead of giving it away.
                text: match (c, self.game_in_picks, self.left_in_picks) {
                    (Confirm::LeaveGame, true, _) | (Confirm::LeaveSeat, _, true) => {
                        LEAVE_PICKS_TEXT
                    }
                    _ => LEAVE_GAME_TEXT,
                },
            },
            Confirm::Leave => ConfirmView {
                title: LEAVE_TITLE,
                text: LEAVE_TEXT,
            },
        }
    }

    /// The latest completed, explicitly owned recording on this machine supplies Home's side.
    fn last_side(&self) -> Side {
        self.local_history
            .entries()
            .iter()
            .find_map(|entry| entry.completed_team())
            .map_or(Side::Radiant, |team| {
                if team == bota_proto::Team::Dire {
                    Side::Dire
                } else {
                    Side::Radiant
                }
            })
    }

    fn fields(&self) -> Vec<FieldView> {
        fields_of(&self.form)
    }

    fn cooldown(&self) -> Option<String> {
        let left = self.client.as_ref()?.searches().cooldown_left_ms();
        (left > 0).then(|| format!("You can search again in {}", clock(left.div_ceil(1000))))
    }

    /// What the page under the top bar shows: under the settings, the page they were opened from.
    pub(super) fn body_at(&self, now: Instant) -> PageView {
        let page = self.page_beneath();
        match page {
            Page::Service | Page::Login | Page::Register | Page::LocalLobby => PageView::Form {
                kind: self.form.kind,
                fields: self.fields(),
                busy: self.connecting() && page != Page::LocalLobby,
                plain_host: self.plain_host.as_ref().map(|h| text::line(h, text::NAME)),
                rules: (page == Page::LocalLobby).then(|| self.rules_choice()),
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
            Page::Heroes => PageView::Heroes,
            Page::Hero(hero) => self.hero_page(hero),
            Page::Leaderboard => self.leaderboard_view(),
            Page::Lobbies => PageView::Lobbies {
                list: self
                    .lobbies
                    .iter()
                    .take(bota_radovid::proto::MAX_LOBBY_LIST)
                    .map(|row| lobby_row(row, &self.catalogue))
                    .collect(),
                fields: self.fields(),
            },
            Page::Lobby => self
                .lobby_projection()
                .map_or(PageView::Joining, PageView::Lobby),
            Page::Result => self
                .shown
                .and_then(|id| self.results.iter().find(|r| r.match_id == id))
                .map_or(PageView::Joining, |r| PageView::Result(self.result_view(r))),
            Page::Replays => self.replay_page(),
            Page::MatchHistory => PageView::MatchHistory(self.history_view()),
            #[cfg(not(target_arch = "wasm32"))]
            Page::DemoHero => PageView::DemoHero {
                offered: self.menu_rulesets().any(bota_radovid::is_demo),
                heroes: bota_core::HEROES
                    .iter()
                    .enumerate()
                    .filter_map(|(id, hero)| Some((u16::try_from(id).ok()?, hero.name.to_owned())))
                    .collect(),
            },
            // Never beneath: the settings open over another page.
            Page::Settings => PageView::Hidden,
            Page::Game if self.launching => PageView::Loading {
                details: self.loading_details(),
                progress_permille: 0,
                elapsed_secs: self.opening_since.map_or(0, |at| {
                    u32::try_from(now.saturating_duration_since(at).as_secs()).unwrap_or(u32::MAX)
                }),
            },
            Page::Game => PageView::Hidden,
        }
    }

    fn hero_page(&self, hero: bota_proto::HeroId) -> PageView {
        let cards = self.local_history.cards(self.library.records());
        let records = super::history::heroes(&cards);
        let record = records.iter().find(|row| row.hero == hero).map_or_else(
            || "No attributed matches on this machine.".to_owned(),
            |row| format!("Your matches: {} · {}% won", row.games, row.percentage()),
        );
        PageView::Hero { hero, record }
    }

    fn leaderboard_view(&self) -> PageView {
        PageView::Leaderboard {
            board: self.board.clone(),
            field: FieldView {
                label: "Find a nickname",
                shown: self.form.value(0).to_owned(),
                focused: self.form.focused_field() == Some(0),
            },
        }
    }

    fn replay_page(&self) -> PageView {
        PageView::Replays {
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
        }
    }

    pub(super) fn lobby_projection(&self) -> Option<LobbyView> {
        self.lobby.as_ref().map(|(state, at)| {
            lobby_view(
                state,
                *at,
                self.client.as_ref(),
                self.moving,
                &self.catalogue,
            )
        })
    }

    fn home(&self) -> PageView {
        let Some(client) = &self.client else {
            return PageView::Home {
                matches: self.local_history.cards(self.library.records()),
                hero: self.last_played_hero(),
                rulesets: Vec::new(),
                in_flight: (0, None),
                cooldown: None,
            };
        };
        let searches = client.searches();
        let in_flight = searches.groups().len() + searches.slot_waits().len() + self.games.len();
        PageView::Home {
            matches: self.local_history.cards(self.library.records()),
            hero: self.last_played_hero(),
            rulesets: self
                .menu_rulesets()
                // Lobbies and solo starts without a queue have no matchmaking policy.
                .filter(|r| r.rating_window.is_some())
                .map(|r| RulesetRow {
                    id: r.id.clone(),
                    name: text::line(&r.name, text::NAME),
                    teams: r
                        .teams
                        .iter()
                        .take(2)
                        .map(u8::to_string)
                        .collect::<Vec<_>>()
                        .join("v"),
                    rated: r.rated,
                    description: super::play::description(&r.id),
                })
                .collect(),
            in_flight: (
                in_flight,
                client.limits(None).map(|limits| limits.games_per_account),
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
            .map_or_else(|| self.name(), |p| text::who(&p.nickname, p.id.0));
        let ratings = self.client.as_ref().map_or_else(Vec::new, |_| {
            self.menu_rulesets()
                .filter(|mode| mode.rated)
                .filter_map(|mode| {
                    let rated = self
                        .account_profile
                        .as_ref()
                        .and_then(|profile| profile.ratings.iter().find(|(id, _)| *id == mode.id))
                        .map(|(_, rating)| rating)?;
                    Some(super::account_page::Rating {
                        mode: ruleset_name(&self.catalogue, &mode.id),
                        rating: super::leaderboard::rating(rated.rating.rating),
                        games: rated.games,
                        rank: rated.rank,
                    })
                })
                .collect()
        });
        PageView::Profile {
            name,
            ratings,
            avatar: self.avatar.view.clone(),
            matches: self.local_history.cards(self.library.records()),
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
        let (report, report_note) = match bota_radovid::decode_report(result) {
            Ok(report) => (report, ""),
            Err(bota_proto::MatchReportError::UnsupportedVersion(_)) => {
                (None, " · match details need a newer client")
            }
            Err(_) => (None, " · match details could not be read"),
        };
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
                "Match {} · {}{}{reason}{report_note}",
                result.match_id.0,
                clock(result.duration_ms / 1000),
                if result.rated { " · rated" } else { "" },
            ),
            seats,
        }
    }
}

/// What the pill calls a service: its host, without the scheme and the port
/// (`wss://play.example.org:4460` is `play.example.org`).
fn service_name(address: &str) -> &str {
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    match host.rsplit_once(':') {
        // An IPv6 address keeps its brackets and colons.
        Some((name, port)) if !name.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    }
}

fn ruleset_name(catalogue: &[bota_radovid::proto::RulesetInfo], id: &RulesetId) -> String {
    catalogue.iter().find(|r| &r.id == id).map_or_else(
        || text::line(&id.0, text::NAME),
        |r| text::line(&r.name, text::NAME),
    )
}

fn search_rows(
    client: &bota_radovid::Client,
    names: &dyn Fn(&RulesetId) -> String,
    catalogue: &[bota_radovid::proto::RulesetInfo],
) -> Vec<SearchRow> {
    client
        .searches()
        .groups()
        .into_iter()
        .map(|s| SearchRow {
            entry: s.entry,
            ruleset: catalogue.iter().find(|r| r.id == s.ruleset).map_or_else(
                || names(&s.ruleset),
                |r| {
                    format!(
                        "{} · {}{}",
                        names(&s.ruleset),
                        if r.rated { "ranked" } else { "unranked" },
                        if client.searches().entries().iter().any(|entry| {
                            entry.search == s.search
                                && entry.audience == bota_radovid::proto::Audience::Everyone
                        }) {
                            " · with bots"
                        } else {
                            ""
                        }
                    )
                },
            ),
            waited: clock(s.waited_ms() / 1000),
            overloaded: s.overloaded,
            games_waiting: s.games_waiting,
            hint: (s.window_capped
                && catalogue
                    .iter()
                    .any(|rule| rule.id == s.ruleset && rule.rated)
                && catalogue.iter().any(|rule| {
                    !rule.rated
                        && rule.rating_window.is_some()
                        && rule.rating_ruleset.as_ref() == Some(&s.ruleset)
                }))
            .then_some("Rating window at its maximum. You can also search in Unranked."),
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

/// Unanswered proposals first, ordered by the time left to answer.
fn offer_box(
    client: &bota_radovid::Client,
    names: &dyn Fn(&RulesetId) -> String,
    accepted: &super::accepted::Accepted,
    catalogue: &[bota_radovid::proto::RulesetInfo],
) -> Option<OfferBox> {
    let searches = client.searches();
    choose_offer(searches.entries().iter().filter_map(|search| {
        let offer = search.offer.as_ref()?;
        let mut projected = project_offer(client, names, search, offer, catalogue);
        projected.answered |= accepted.contains(search.entry, offer.proposal);
        Some(projected)
    }))
}

fn choose_offer(offers: impl Iterator<Item = OfferBox>) -> Option<OfferBox> {
    offers
        .filter(|offer| offer.left_ms > 0)
        .min_by_key(|offer| (offer.answered, offer.left_ms))
}

fn project_offer(
    client: &bota_radovid::Client,
    names: &dyn Fn(&RulesetId) -> String,
    search: &bota_radovid::Search,
    offer: &bota_radovid::Offer,
    catalogue: &[bota_radovid::proto::RulesetInfo],
) -> OfferBox {
    let total = client
        .limits(None)
        .map(|l| l.accept_timeout_ms)
        .filter(|total| *total > 0);
    let left = offer.left_ms();
    OfferBox {
        proposal: offer.proposal,
        ruleset: format!(
            "{} · {}",
            names(&search.ruleset),
            if catalogue
                .iter()
                .find(|r| r.id == search.ruleset)
                .is_some_and(|r| r.rated)
            {
                "Ranked"
            } else {
                "Unranked"
            }
        ),
        left_ms: left,
        left_permille: total
            .map(|total| (u64::from(left.min(total)) * 1000 / u64::from(total)) as u32),
        accepted: offer.accepted,
        of: offer.of,
        answered: offer.answered,
        decline_cooldown_ms: client.limits(None).map(|l| l.decline_cooldown_ms),
    }
}

fn game_row(match_id: MatchId, ready: bool) -> GameRow {
    GameRow {
        match_id,
        label: format!("Match {}", match_id.0),
        ready,
    }
}

fn lobby_row(
    l: &bota_radovid::proto::LobbySummary,
    catalogue: &[bota_radovid::proto::RulesetInfo],
) -> LobbyRow {
    LobbyRow {
        id: l.id,
        name: text::line(&l.name, text::NAME),
        owner: text::line(&l.owner_nickname, text::NAME),
        ruleset: l.ruleset.clone(),
        mode: catalogue
            .iter()
            .find(|rules| rules.id == l.ruleset)
            .map_or_else(
                || text::line(&l.ruleset.0, text::NAME),
                |rules| text::line(&rules.name, text::NAME),
            ),
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
    catalogue: &[bota_radovid::proto::RulesetInfo],
) -> LobbyView {
    let me = client.and_then(|c| c.me()).map(|me| me.id);
    let elapsed = u32::try_from(at.elapsed().as_millis()).unwrap_or(u32::MAX);
    let owner = Some(state.owner) == me;
    let teams = lobby_teams(state, me);
    let spectators = state
        .members
        .iter()
        .take(256)
        .filter(|m| m.slot == bota_radovid::proto::LOBBY_NO_SLOT)
        .map(|member| lobby_member(member, me))
        .collect();
    let seated: Vec<_> = teams
        .iter()
        .flat_map(|team| &team.slots)
        .filter_map(|slot| slot.who.as_ref())
        .collect();
    let seats: usize = teams.iter().map(|team| team.slots.len()).sum();
    let waiting = owner
        .then(|| {
            lobby_start_block(
                &seated,
                catalogue
                    .iter()
                    .find(|rules| rules.id == state.ruleset)
                    .map(|rules| usize::from(rules.lobby_min_players)),
            )
        })
        .flatten();
    LobbyView {
        name: text::line(&state.name, text::NAME),
        locked: state.locked,
        starts_short: owner && waiting.is_none() && seated.len() < seats,
        me_ready: seated.iter().any(|member| member.me && member.ready),
        waiting,
        owner,
        settings: lobby_settings(state, catalogue, owner),
        moving: moving
            .and_then(|id| state.members.iter().take(256).find(|m| m.account == id))
            .map(|m| text::line(&m.nickname, text::NAME)),
        spectators,
        teams,
        starting_in_secs: state
            .starting_in_ms
            .map(|ms| ms.saturating_sub(elapsed).div_ceil(1000)),
    }
}

fn lobby_member(member: &bota_radovid::proto::LobbyMember, me: Option<AccountId>) -> MemberRow {
    MemberRow {
        account: member.account,
        bot: member.kind == bota_radovid::proto::AccountKind::Bot,
        name: text::line(&member.nickname, text::NAME),
        ready: member.ready,
        me: Some(member.account) == me,
    }
}

fn lobby_teams(state: &bota_radovid::proto::LobbyState, me: Option<AccountId>) -> Vec<TeamRow> {
    let mut first = 0u8;
    state
        .teams
        .iter()
        .take(2)
        .enumerate()
        .map(|(team, size)| {
            let slots = (first..first.saturating_add(*size))
                .map(|slot| SlotRow {
                    slot,
                    who: state
                        .members
                        .iter()
                        .take(256)
                        .find(|member| member.slot == slot)
                        .map(|member| lobby_member(member, me)),
                })
                .collect();
            first = first.saturating_add(*size);
            TeamRow {
                name: if team == 0 { "RADIANT" } else { "DIRE" }.into(),
                slots,
            }
        })
        .collect()
}

fn lobby_start_block(seated: &[&MemberRow], needed: Option<usize>) -> Option<LobbyBlock> {
    if !seated.iter().any(|member| member.me) {
        return Some(LobbyBlock::Slot);
    }
    let Some(needed) = needed else {
        return Some(LobbyBlock::Mode);
    };
    if seated.len() < needed {
        return Some(LobbyBlock::Players(needed - seated.len()));
    }
    let unready = seated
        .iter()
        .filter(|member| !member.me && !member.ready)
        .count();
    (unready > 0).then_some(LobbyBlock::Ready(unready))
}

fn lobby_settings(
    state: &bota_radovid::proto::LobbyState,
    catalogue: &[bota_radovid::proto::RulesetInfo],
    owner: bool,
) -> SettingsView {
    use bota_radovid::proto::Audience;
    let rules = catalogue.iter().find(|rules| rules.id == state.ruleset);
    SettingsView {
        mode: rules.map_or_else(
            || text::line(&state.ruleset.0, text::NAME),
            |rules| text::line(&rules.name, text::NAME),
        ),
        audience: super::lobby_controls::audience_name(state.audience),
        locked: state.locked,
        modes: if owner {
            catalogue
                .iter()
                .filter(|r| !bota_radovid::is_demo(r))
                .map(|r| ModeRow {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    chosen: r.id == state.ruleset,
                })
                .collect()
        } else {
            Vec::new()
        },
        audiences: if owner {
            [Audience::Everyone, Audience::HumansOnly, Audience::BotsOnly]
                .into_iter()
                .map(|audience| AudienceRow {
                    audience,
                    name: super::lobby_controls::audience_name(audience),
                    chosen: audience == state.audience,
                })
                .collect()
        } else {
            Vec::new()
        },
        cheats: state.cheats,
        editable: owner,
    }
}

/// A form's fields as drawn.
pub(super) fn fields_of(form: &super::form::Form) -> Vec<FieldView> {
    form.fields()
        .iter()
        .enumerate()
        .map(|(at, f)| FieldView {
            label: f.label,
            shown: f.shown(),
            focused: Some(at) == form.focused_field(),
        })
        .collect()
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

#[cfg(test)]
mod proposal_selection_tests {
    use super::*;
    #[test]
    fn the_prompt_uses_the_earliest_live_unanswered_deadline() {
        let sample = super::super::preview::view("match-found")
            .expect("preview")
            .offer
            .expect("offer");
        let later = OfferBox {
            proposal: ProposalId(1),
            left_ms: 20_000,
            ..sample.clone()
        };
        let urgent = OfferBox {
            proposal: ProposalId(2),
            left_ms: 2_000,
            ..sample.clone()
        };
        let answered = OfferBox {
            proposal: ProposalId(3),
            left_ms: 1_000,
            answered: true,
            ..sample
        };
        assert_eq!(
            choose_offer([later, answered, urgent].into_iter())
                .expect("prompt")
                .proposal,
            ProposalId(2)
        );
    }
    #[test]
    fn expired_unanswered_offer_never_takes_the_prompt_from_a_live_one() {
        let sample = super::super::preview::view("match-found")
            .expect("preview")
            .offer
            .expect("offer");
        let expired = OfferBox {
            proposal: ProposalId(1),
            left_ms: 0,
            left_permille: Some(0),
            answered: false,
            ..sample.clone()
        };
        let live = OfferBox {
            proposal: ProposalId(2),
            left_ms: 9_000,
            left_permille: Some(300),
            answered: false,
            ..sample
        };
        assert_eq!(
            choose_offer([expired, live].into_iter())
                .expect("prompt")
                .proposal,
            ProposalId(2)
        );
    }
}

#[cfg(test)]
mod report_tests {
    use super::*;

    #[test]
    fn a_future_report_keeps_the_match_result_and_explains_why_details_are_missing() {
        let player = crate::main_menu::tests::Player::new(None);
        let result = MatchResult {
            match_id: MatchId(99),
            ruleset: RulesetId("solo_mid".to_owned()),
            rated: false,
            outcome: Outcome::Draw,
            reason: EndReason::Draw,
            duration_ms: 1000,
            game_report: b"BMRP\x02".to_vec(),
            seats: Vec::new(),
            finished_at: bota_radovid::proto::UnixMs(0),
        };
        let view = player.menu.result_view(&result);
        assert_eq!(view.headline, "DRAW");
        assert!(
            view.detail.contains("match details need a newer client"),
            "the result must explain an unsupported report: {}",
            view.detail
        );
    }
}
