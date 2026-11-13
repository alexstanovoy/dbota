//! The main menu's state and logic, apart from any drawing: what the player
//! asks for comes in as [`Act`]s, what the matchmaking service says comes in
//! through the control connection, and out go requests to the service and
//! the games to open ([`Launch`]). The screens draw [`MenuCore::view`].

use std::collections::VecDeque;

use bevy::platform::time::Instant;
use bota_radovid::proto::{
    AccountId, AccountKind, Audience, ClientMsg, Error, Event, Handoff, LobbyId, LobbyState,
    LobbySummary, MatchId, MatchResult, ProposalId, PublicAccount, QueueEntryId, QueueLeftReason,
    Reply, RulesetId, Secret, valid_login, valid_password,
};
use bota_radovid::{Client, ClientError, Incoming, Tls};

use super::form::{Form, FormKind};
use super::keep::Keeper;
use super::prefs::{Prefs, Saved};
use super::store::{Entry, SettingsStore};
use super::text;
use crate::replays::Library;

/// Messages from the service taken in one frame at most.
const MAX_PER_FRAME: usize = 64;
/// Requests whose answers the menu waits for, at most.
const MAX_ASKED: usize = 64;
/// Requests kept for a link being opened.
const MAX_OUTBOX: usize = 16;
/// How long the control link may sit with nothing live on it before it is closed; the next
/// request opens it again.
const IDLE_HANG_UP: std::time::Duration = std::time::Duration::from_secs(10);
/// How long after a link was lost with something live on it the menu reconnects, silently.
const RECONNECT_AFTER: std::time::Duration = std::time::Duration::from_secs(1);
/// Unfinished matches of the account the menu lists, at most.
const MAX_GAMES: usize = 32;
/// Finished matches kept for their result screens.
const MAX_RESULTS: usize = 8;
/// The name announced while the profile has none.
pub const DEFAULT_NAME: &str = "Player";

/// A page of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// Which matchmaking service, and connecting to it.
    Service,
    Login,
    Register,
    /// Play: the rulesets to search a game in.
    Home,
    /// The account's running matches and searches.
    MyGames,
    Lobbies,
    /// The lobby the player sits in.
    Lobby,
    /// A lobby on this machine, others joining by its address.
    LocalLobby,
    /// The replays on this machine.
    Replays,
    MatchHistory,
    /// The account's rating per ruleset and the ladder.
    Profile,
    Heroes,
    Hero(bota_proto::HeroId),
    Leaderboard,
    /// Dota's Demo Hero: a hero to try out alone, with every cheat.
    #[cfg(not(target_arch = "wasm32"))]
    DemoHero,
    /// How a finished match went.
    Result,
    /// A game is on screen, or being opened.
    Game,
    /// The settings screen, Dota's dashboard's: hotkeys, options, video and
    /// audio (drawn by [`crate::settings`]).
    Settings,
    PlayerProfile(u64),
}

impl Page {
    /// Whether the page is the service's, shown only with an account.
    pub fn needs_account(self) -> bool {
        match self {
            Page::MyGames
            | Page::Lobbies
            | Page::Lobby
            | Page::Leaderboard
            | Page::PlayerProfile(_) => true,
            #[cfg(not(target_arch = "wasm32"))]
            Page::DemoHero => true,
            _ => false,
        }
    }
}

mod lobby_data;
mod lobby_requests;
mod lobby_ui;

/// What the player does in the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Go(Page),
    Play(super::PlayAction),
    Avatar(super::avatar::Action),
    InspectHero(bota_proto::HeroId),
    InspectPlayer(super::player_profile::Target),
    MatchDetails(String),
    WatchHistory(String),
    HistoryMode(Option<String>),
    HistoryResult(Option<super::history::ResultKind>),
    HistoryHero(Option<bota_proto::HeroId>),
    HistoryFilter(super::history_page::Filter),
    HistoryPage(i8),
    LeaderboardMode(String),
    LeaderboardPlayers(super::leaderboard::Players),
    /// Puts the keyboard in a field of the page's form.
    Focus(usize),
    Type(String),
    Erase,
    /// Tab: the next field or button of the form.
    NextField,
    /// Shift+Tab.
    PreviousField,
    /// Enter: what the page's form is for.
    Submit,
    Logout,
    /// Searches a game of a ruleset.
    #[cfg(test)]
    Find(RulesetId),
    Cancel(QueueEntryId),
    Accept(ProposalId),
    Decline(ProposalId),
    /// Takes one's seat in a running match again.
    Rejoin(MatchId),
    /// Watches one's own running match.
    Watch(MatchId),
    LobbyUi(super::LobbyAction),
    /// Joins a listed lobby: an open one at once, a locked one through the password dialog.
    JoinLobby(LobbyId),
    /// Lists the open lobbies whose name or owner holds the search typed (all of them without).
    SearchLobbies,
    /// The password dialog's OK (`true`): joins with the password typed; or its Cancel.
    LobbyPassword(bool),
    ReadyLobby(bool),
    LeaveLobby,
    /// Plays (or with `true` watches) the game at the address in the form.
    Direct(bool),
    /// Opens a lobby on this machine, for this machine alone or (`true`)
    /// for the local network too, and takes a seat in it.
    Host(bool),
    /// Turns the local lobby's cheats on or off (Dota's "Enable cheats").
    ToggleCheats,
    /// Starts Dota's Demo Hero with a hero: the service's Demo ruleset, started for this account alone
    /// at once.
    Demo(u16),
    /// Watches a kept replay, by its file name.
    OpenReplay(String),
    /// Deletes a kept replay.
    DeleteReplay(String),
    /// Watches the replay of a finished match of the account's: fetched from the service once.
    WatchReplay(MatchId),
    /// Goes on with plain TCP to a host after the notice.
    PlainAnyway,
    /// Closes the notice line.
    Dismiss,
    /// The connection pill's RECONNECTING: opens the lost link to the service again now.
    RetryService,
    /// The power button: asks whether to quit the client.
    Quit,
    /// OK (`true`) or Cancel on the question asked.
    Answer(bool),
    /// Escape over the menu: closes the question, else backs out to the top
    /// page, else (over a game) goes back to it, else asks to quit.
    Back,
    /// Escape in a game with nothing to close: the dashboard over it.
    Dashboard,
    /// Back to the game under the dashboard.
    ReturnToGame,
    /// Leaves the game under the dashboard: a seat at once (it can be taken
    /// again), watching after asking.
    Disconnect,
    /// The dashboard's LEAVE GAME: asks, then leaves the match under way.
    LeaveGame,
    /// Takes the seat of the game left again.
    Reconnect,
    /// Dota's LEAVE beside RECONNECT: done with the game left, its seat is not taken again from
    /// here (a server that died cannot give it back).
    LeaveSeat,
    /// The rules a new local lobby plays by.
    Rules(LocalRules),
    /// In a lobby: sits in a slot (or none, looking on); the member being moved is put there.
    LobbySlot(u8),
    /// The owner picks a member to move with the next slot clicked.
    MoveMember(AccountId),
    KickMember(AccountId),
}

/// The rules a local lobby plays by. They choose the map and nothing else: the lobby has
/// [`LOCAL_SEATS`] seats under any of them, and the match has the players seated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LocalRules {
    /// The whole map, played to the Ancient.
    #[default]
    Full,
    /// The mid-only map: two lives a side or the first tower, twenty minutes.
    SoloMid,
}

impl LocalRules {
    pub const ALL: [LocalRules; 2] = [LocalRules::Full, LocalRules::SoloMid];

    /// Its name in the menu's remembered choices.
    pub fn name(self) -> &'static str {
        match self {
            LocalRules::Full => "full",
            LocalRules::SoloMid => "solo_mid",
        }
    }

    pub fn named(name: &str) -> Option<LocalRules> {
        LocalRules::ALL.into_iter().find(|r| r.name() == name)
    }

    /// What the menu calls them.
    pub fn label(self) -> &'static str {
        match self {
            LocalRules::Full => "FULL RULES",
            LocalRules::SoloMid => "SOLO MID",
        }
    }

    /// The map of the game server's `--map`.
    pub fn map(self) -> bota_proto::MapId {
        match self {
            LocalRules::Full => bota_proto::MapId(0),
            LocalRules::SoloMid => bota_core::MAP2_ID,
        }
    }
}

/// What the menu asks before it goes on, as Dota's popups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Quit the client.
    Quit,
    /// Stop watching the game or replay under the dashboard.
    Leave,
    /// Leave the match under way for good (Dota's LEAVE GAME): the leaver's team loses it.
    LeaveGame,
    /// Leave for good the match whose seat was left (LEAVE beside RECONNECT): it is told so if
    /// it can be reached.
    LeaveSeat,
    /// Host a new local lobby (on the LAN with `true`) while this client's lobby server runs: it
    /// ends, and the game in it for everyone.
    Rehost(bool),
}

/// How the game on screen is had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameKind {
    /// In a seat.
    Play,
    /// Watching a live game.
    Watch,
    /// A replay.
    Replay,
}

/// A game the shell is to open.
#[derive(Clone, Debug, PartialEq)]
pub enum Launch {
    /// Take one's seat with the handoff's ticket.
    Seat(Handoff),
    /// Watch one's own seat with the ticket.
    Watch(Handoff),
    /// Connect to an address and say Hello as `name`, a seat claimed with
    /// `key` so the same seat can be taken again.
    Direct {
        address: String,
        name: String,
        spectate: bool,
        key: Option<Claim>,
        /// The seat of the local lobby this client hosts, taken again: its game server is this
        /// client's own.
        hosted: bool,
    },
    /// Start a game server on this machine (on the local network too when
    /// `lan`), and take a seat in it as a direct game.
    Host {
        port: u16,
        lan: bool,
        name: String,
        key: Option<Claim>,
        /// Whether its host enabled cheats.
        cheats: bool,
        /// Where the server writes the replay.
        replay: Option<std::path::PathBuf>,
        /// A metadata-only recording when replay saving is off.
        metadata: Option<std::path::PathBuf>,
        /// The server binary, when not looked for (`$BOTA_SERVER`, beside
        /// this client, the `PATH`).
        server: Option<std::path::PathBuf>,
        /// Whether `$BOTA_SERVER` and the `PATH` are looked in for it.
        server_environment: bool,
        /// The rules of the lobby; its seats are [`LOCAL_SEATS`] under any.
        rules: LocalRules,
    },
    /// An account-free, loopback-only sandbox. It has no replay or reconnectable seat.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "A browser cannot start its own game server")
    )]
    LocalDemo {
        hero: bota_proto::HeroId,
        name: String,
        key: Option<Claim>,
        server: Option<std::path::PathBuf>,
        server_environment: bool,
    },
    /// Dota's Demo Hero: the seat of a match of the service's Demo ruleset (cheats on, this
    /// player alone), with the hero chosen to pick.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // only the desktop starts one
    Demo {
        hero: bota_proto::HeroId,
        handoff: Handoff,
    },
    /// Watch a replay file.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // a browser opens no files by path
    Replay(std::path::PathBuf),
    /// Watch a replay held in memory: one a browser read from storage or from a file.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Blob(Blob),
    /// The game the command line named, again.
    Given,
}

/// A replay's bytes, shared.
#[derive(Clone)]
pub struct Blob(pub std::sync::Arc<[u8]>);

impl std::fmt::Debug for Blob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Blob({} bytes)", self.0.len())
    }
}

impl PartialEq for Blob {
    fn eq(&self, other: &Blob) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The key a seat without a reservation is claimed with, which takes it
/// back after the player left (bota_server keeps an unreserved seat for the
/// key its holder came with). Never printed.
#[derive(Clone, Copy)]
pub struct Claim(pub bota_proto::SeatKey);

impl Claim {
    /// A key of the system's randomness; none where there is none to draw.
    pub fn draw() -> Option<Claim> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut key = [0u8; 32];
            getrandom::fill(&mut key)
                .ok()
                .map(|()| Claim(bota_proto::SeatKey(key)))
        }
        #[cfg(target_arch = "wasm32")]
        None
    }
}

impl std::fmt::Debug for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Claim(..)")
    }
}

impl PartialEq for Claim {
    fn eq(&self, other: &Claim) -> bool {
        self.0.0 == other.0.0
    }
}

impl Launch {
    /// How the game it opens is had.
    pub fn kind(&self) -> GameKind {
        match self {
            Launch::Seat(_)
            | Launch::Given
            | Launch::Host { .. }
            | Launch::LocalDemo { .. }
            | Launch::Demo { .. } => GameKind::Play,
            Launch::Watch(_) => GameKind::Watch,
            Launch::Replay(_) | Launch::Blob(_) => GameKind::Replay,
            Launch::Direct { spectate, .. } => {
                if *spectate {
                    GameKind::Watch
                } else {
                    GameKind::Play
                }
            }
        }
    }
}

/// The menu's status overlay: news or a refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub text: String,
    pub error: bool,
}

/// The state of the control connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Conn {
    Offline,
    Connecting,
    Welcomed,
    LoggedIn,
}

/// The password dialog of a locked lobby: which lobby, what the last try was told, and the
/// dialog's own field. Not `Debug`: it holds a password.
pub(super) struct JoinPrompt {
    pub(super) lobby: LobbyId,
    pub(super) name: String,
    pub(super) error: Option<String>,
    pub(super) form: Form,
}

/// A leave under way: its id and deadline, the game it tells, whether that game is the local
/// lobby's own, and whether its seat was left in the picks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Leaving {
    id: u64,
    until: Instant,
    game: SeatOf,
    hosted: bool,
    in_picks: bool,
}

/// Which game a launch takes a seat of, to tell one game from another: an address by its host
/// and port, `localhost` and `127.0.0.1` alike, with or without its scheme.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SeatOf {
    Match(MatchId),
    Address(String),
    Given,
}

impl SeatOf {
    fn of(launch: &Launch) -> Option<SeatOf> {
        match launch {
            Launch::Seat(h) | Launch::Watch(h) | Launch::Demo { handoff: h, .. } => {
                Some(SeatOf::Match(h.match_id))
            }
            Launch::Direct { address, .. } => Some(SeatOf::Address(same_address(address))),
            Launch::Given => Some(SeatOf::Given),
            Launch::Host { .. }
            | Launch::LocalDemo { .. }
            | Launch::Replay(_)
            | Launch::Blob(_) => None,
        }
    }
}

/// An address as one game: no `tcp://`, `ws://` or `wss://`, the loopback host spelled one way,
/// lower case.
fn same_address(address: &str) -> String {
    let address = address.trim().to_ascii_lowercase();
    let address = ["tcp://", "ws://", "wss://"]
        .iter()
        .find_map(|scheme| address.strip_prefix(scheme))
        .unwrap_or(&address)
        .trim_end_matches('/');
    match address.strip_prefix("localhost:") {
        Some(port) => format!("127.0.0.1:{port}"),
        None => address.to_owned(),
    }
}

/// What a request was, for its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Asked {
    Login,
    Register,
    Resume,
    Logout,
    Queue,
    Accept(ProposalId),
    Rejoin(MatchId),
    Watch(MatchId),
    Lobby,
    LeaveLobby(LobbyId),
    LobbyCatalogue,
    LobbyUi(super::lobby_controls::SaveRequest),
    RestoreLobby(LobbyId),
    /// A join of a listed lobby.
    Join(LobbyId),
    /// The start of a Demo Hero.
    Demo,
    History,
    Profile,
    Visitor(u64, super::player_profile::Part),
    Leaderboard(u64),
    Artifact(MatchId),
    Meta(MatchId),
    Other,
}

/// How long the connection stays open after a seat left the game, for the message to go out.
const LEAVE_GAME_FLUSH: std::time::Duration = std::time::Duration::from_millis(500);
/// How long LEAVE tries to reach the game it tells the match is left; past it, the game is
/// forgotten untold (its server died, or the network is down).
const LEAVE_REACH: std::time::Duration = std::time::Duration::from_secs(10);
/// What a leave that could not tell its game says.
const UNTOLD: &str = "The game could not be told you left it: it is forgotten here";
/// Finished matches listed under MY GAMES.
const HISTORY_LEN: u8 = 20;

/// An unfinished match of the account.
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub match_id: MatchId,
    /// Once the game server is up.
    pub handoff: Option<Handoff>,
}

/// What is on screen instead of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OnScreen {
    Match(MatchId),
    Direct,
    /// A game of a lobby on this machine.
    Local,
    /// Dota's Demo Hero: a match of the service.
    Demo(MatchId),
    Replay,
    /// The game the command line named.
    Given,
}

/// The menu.
pub struct MenuCore {
    pub(super) store: SettingsStore,
    pub(super) prefs: Prefs,
    tls: Tls,
    pub(super) client: Option<Client>,
    #[cfg(test)]
    pause_service_events: bool,
    conn: Conn,
    /// Whether an account is signed in: its session is kept, the link to the service is not
    /// (it is open only while something live is on it, and a request opens it again).
    pub(super) account: bool,
    /// The login or registration to send as soon as the link is open. Held only until then.
    credentials: Option<(ClientMsg, Asked)>,
    /// Requests that wait for the link to open and the session to resume.
    outbox: Vec<(ClientMsg, Asked)>,
    /// Since when nothing live has been on the link.
    idle_since: Option<Instant>,
    /// When to reconnect after a lost link that had something live on it.
    retry_at: Option<Instant>,
    /// Whether a lost link is reconnected once, silently.
    retry: bool,
    /// The signed-in account's link ended without the menu hanging up (an idle link is hung up on
    /// purpose, and is no loss): until it is open again the pill says so.
    link_lost: bool,
    pub(super) page: Page,
    pub(super) play: super::play::PlayState,
    pub(super) accepted: super::accepted::Accepted,
    loading: crate::loading::Details,
    pub(super) form: Form,
    pub(super) status: Option<Status>,
    asked: Vec<(u32, Asked)>,
    pub(super) lobbies: Vec<LobbySummary>,
    pub(super) catalogue: Vec<bota_radovid::proto::RulesetInfo>,
    pub(super) lobby_ui: super::lobby_controls::Controls,
    /// The password dialog of a locked lobby being joined; it has the keyboard while it is up.
    pub(super) join_prompt: Option<JoinPrompt>,
    /// The member the owner is about to move.
    pub(super) moving: Option<AccountId>,
    /// The password the owner set, sent again with each change of the settings (the service never
    /// tells it back).
    lobby_secret: Option<(LobbyId, String)>,
    /// The lobby sat in, and when it was last told.
    pub(super) lobby: Option<(LobbyState, Instant)>,
    pub(super) games: Vec<Game>,
    pub(super) results: VecDeque<MatchResult>,
    /// The result the Result page shows.
    pub(super) shown: Option<MatchId>,
    pub(super) on_screen: Option<OnScreen>,
    launch: Option<Launch>,
    pub(super) launching: bool,
    pub(super) opening_since: Option<Instant>,
    pub(super) queue_full_until: Option<Instant>,
    /// A host waiting for the player's word on plain TCP.
    pub(super) plain_host: Option<String>,
    /// The page the settings were opened from.
    settings_from: Page,
    /// How the game on screen (or under the dashboard) is had.
    pub(super) kind: GameKind,
    /// The question asked over the page.
    pub(super) confirm: Option<Confirm>,
    /// Whether leaving the game now may ruin it for the others: a seat in a
    /// game under way (the shell tells).
    pub(super) game_unsafe: bool,
    /// Whether the live match on screen is over: there is nothing to return to (the shell tells).
    pub(super) game_over: bool,
    /// The seat left, to take again.
    pub(super) reconnect: Option<Launch>,
    /// The seat of the game on screen, for [`Self::reconnect`] once left.
    last_seat: Option<Launch>,
    /// The player asked to quit: the shell ends the client.
    quit: bool,
    /// A leave of the local lobby's own game is to be stopped, once: a new lobby replaces the
    /// server it was telling, and nothing of the old game may reach the new one.
    cancel_leave: bool,
    /// The leave under way: LEAVE telling a game whose seat was left that the match is left for
    /// good (the shell runs it, `super::leave`). An outcome of another id, or one past its
    /// deadline, is never taken.
    leaving: Option<Leaving>,
    /// The leave to start, once: its id, the seat's launch and the deadline.
    leave_order: Option<(u64, Launch, Instant)>,
    /// The id of the last leave started.
    last_leave_id: u64,
    /// The game on screen is in its picks, as the shell last saw it.
    pub(super) game_in_picks: bool,
    /// The seat RECONNECT offers was left in the picks.
    pub(super) left_in_picks: bool,
    /// The game server of the local lobby this client hosts is running, as the shell last saw:
    /// quitting or hosting another lobby ends the game in it for everyone.
    pub(super) hosting: bool,
    /// The player left the game: the shell lets it go.
    leave: bool,
    /// The player confirmed leaving the match: the shell tells the server, then leaves.
    leave_game: bool,
    /// When the connection may be closed after the leave went out.
    leave_at: Option<Instant>,
    /// The name this player goes by where there is no account: the
    /// settings' profile name.
    pub(super) profile: String,
    pub(super) board: super::leaderboard::Board,
    pub(super) list_cache: super::list_cache::Cache,
    pub(super) avatar: super::avatar::Editor,
    pub(super) local_history: super::local_matches::Archive,
    pub(super) local_meta: super::local_meta::Reader,
    pub(super) history_filter: super::history_page::Selection,
    pub(super) history_picker: Option<super::history_page::Filter>,
    pub(super) history_page: usize,
    pub(super) result_request: Option<crate::postgame::view::View>,
    pub(super) pending_result_replay: Option<crate::postgame::view::Replay>,
    pub(super) pending_result_replay_since: Option<Instant>,
    after_results: Option<Page>,
    /// The replays this client keeps.
    pub(super) library: Library,
    /// What a kept replay holds, as read, where the library knows no more.
    pub(super) shelf: super::replay_info::Shelf,
    /// The fetching of the replays of the player's matches.
    pub(super) keeper: Keeper,
    /// The metadata of the match on screen, fetched for the post-game screen.
    pub(super) meta: super::meta_fetch::MetaFetch,
    /// The account's latest finished matches, from the service.
    pub(super) history: Vec<MatchResult>,
    pub(super) account_profile: Option<PublicAccount>,
    pub(super) profiles: super::player_profile::Reader,
    /// The game server a local lobby starts, when not looked for.
    pub(super) server_binary: Option<std::path::PathBuf>,
    /// Whether the server is also looked for in `$BOTA_SERVER` and on the `PATH`.
    server_environment: bool,
    /// The address others type to join the local lobby this client hosts.
    hosted_at: Option<String>,
    /// The hero of the last Demo Hero started.
    demo: Option<bota_proto::HeroId>,
    /// The hero of a Demo Hero being made: started, its match waited for.
    demo_asked: Option<bota_proto::HeroId>,
    /// The last Demo Hero's match: its result is no news.
    demo_match: Option<MatchId>,
    /// Whether the service has been tried: a menu under a game named on the
    /// command line tries it once that game is left.
    tried_service: bool,
}

impl MenuCore {
    /// A menu over `store`, connecting at once when a service is known:
    /// `service` from the command line (or the page's own host), else the
    /// one remembered.
    pub fn new(
        store: SettingsStore,
        tls: Tls,
        service: Option<String>,
        library: Library,
    ) -> MenuCore {
        let mut core = MenuCore::unconnected(store, tls, service, library);
        core.reach_service();
        core
    }

    /// Reaches the service for the session kept for it, if there is one; the way in otherwise
    /// opens no connection, since sign-in is typed offline and connects when it is sent.
    fn reach_service(&mut self) {
        self.tried_service = true;
        if self.saved_session().is_some() {
            self.connect();
        }
    }

    fn saved_session(&self) -> Option<Saved> {
        self.store
            .read(Entry::Session)
            .and_then(|t| Saved::parse(&t))
            .filter(|s| bota_radovid::same_matchmaker(&s.service, &self.prefs.service))
    }

    /// A menu over `store` that has not tried the service yet.
    fn unconnected(
        store: SettingsStore,
        tls: Tls,
        service: Option<String>,
        library: Library,
    ) -> MenuCore {
        let mut prefs = store
            .read(Entry::Menu)
            .map_or_else(Prefs::default, |t| Prefs::parse(&t));
        if let Some(service) = service {
            prefs.service = service;
        }
        let local_first = !cfg!(target_arch = "wasm32") && prefs.service.is_empty();
        let mut core = Self::initial(store, tls, prefs, library);
        // No service to reach: the menu opens on what needs none, and the sign-in shows when
        // the player goes to the online pages.
        if local_first {
            core.settings_from = Page::LocalLobby;
            core.go_form(Page::LocalLobby);
        } else {
            core.settings_from = Page::Login;
            core.go_form(Page::Login);
        }
        core
    }

    fn initial(store: SettingsStore, tls: Tls, prefs: Prefs, library: Library) -> Self {
        let play = super::play::PlayState {
            bots: prefs.allow_matchmaking_bots,
            ..Default::default()
        };
        let local_history = super::local_matches::Archive::load(&store, library.policy().max_count);
        MenuCore {
            local_history,
            local_meta: Default::default(),
            history_filter: Default::default(),
            history_picker: None,
            history_page: 0,
            result_request: None,
            pending_result_replay: None,
            pending_result_replay_since: None,
            after_results: None,
            store,
            form: Form::new(FormKind::Service, &prefs),
            prefs,
            tls,
            client: None,
            #[cfg(test)]
            pause_service_events: false,
            conn: Conn::Offline,
            account: false,
            credentials: None,
            outbox: Vec::new(),
            idle_since: None,
            retry_at: None,
            retry: true,
            link_lost: false,
            page: Page::Service,
            play,
            accepted: Default::default(),
            loading: Default::default(),
            settings_from: Page::Service,
            status: None,
            asked: Vec::new(),
            lobbies: Vec::new(),
            catalogue: Vec::new(),
            lobby_ui: Default::default(),
            join_prompt: None,
            moving: None,
            lobby_secret: None,
            lobby: None,
            games: Vec::new(),
            results: VecDeque::new(),
            shown: None,
            on_screen: None,
            launch: None,
            launching: false,
            opening_since: None,
            queue_full_until: None,
            plain_host: None,
            kind: GameKind::Play,
            confirm: None,
            game_unsafe: false,
            game_over: false,
            reconnect: None,
            last_seat: None,
            quit: false,
            leave: false,
            leave_game: false,
            leave_at: None,
            profile: String::new(),
            board: Default::default(),
            list_cache: Default::default(),
            avatar: Default::default(),
            library,
            shelf: Default::default(),
            keeper: Keeper::default(),
            meta: Default::default(),
            history: Vec::new(),
            account_profile: None,
            profiles: Default::default(),
            server_binary: None,
            server_environment: false,
            hosted_at: None,
            cancel_leave: false,
            leaving: None,
            leave_order: None,
            last_leave_id: 0,
            game_in_picks: false,
            left_in_picks: false,
            hosting: false,
            demo: None,
            demo_asked: None,
            demo_match: None,
            tried_service: false,
        }
    }

    /// The address others type under LOCAL LOBBY to join the local
    /// lobby this client hosts, while it is the game on screen.
    pub fn hosted_at(&self) -> Option<&str> {
        self.hosted_at
            .as_deref()
            .filter(|_| self.on_screen == Some(OnScreen::Local))
    }

    /// The rules a new local lobby is made for.
    pub(super) fn rules_choice(&self) -> LocalRules {
        self.prefs.host_rules
    }

    /// The settings' profile name, which local and direct games announce.
    pub fn set_profile_name(&mut self, name: &str) {
        if self.profile != name {
            name.clone_into(&mut self.profile);
        }
    }

    /// The name announced where there is no account.
    pub(super) fn name(&self) -> String {
        match self.profile.trim() {
            "" => DEFAULT_NAME.to_owned(),
            name => name.to_owned(),
        }
    }

    /// A menu under the game the command line named: it shows once the game
    /// is left or on Escape, and reaches the service only then.
    pub fn over_given(
        store: SettingsStore,
        tls: Tls,
        service: Option<String>,
        kind: GameKind,
        library: Library,
    ) -> MenuCore {
        let mut core = MenuCore::unconnected(store, tls, service, library);
        core.page = Page::Game;
        core.on_screen = Some(OnScreen::Given);
        core.kind = kind;
        if kind == GameKind::Play {
            core.last_seat = Some(Launch::Given);
        }
        core
    }

    /// A menu that looks for the game server of a local lobby in `$BOTA_SERVER` and on the `PATH`
    /// too, as the client does that takes its configuration from the process environment.
    #[must_use]
    pub fn with_server_from_environment(mut self, yes: bool) -> MenuCore {
        self.server_environment = yes;
        self
    }

    /// Whether the control link to the service is open.
    #[cfg(test)]
    pub(super) fn link_open(&self) -> bool {
        self.client.as_ref().is_some_and(|c| !c.hung_up())
    }

    /// The button of the page's form that has the keyboard, if one has.
    pub fn button_focus(&self) -> Option<&Act> {
        self.active_lobby_form().focused_button()
    }

    /// Whether the page has a text field the player may type into (an input method is then
    /// wanted).
    pub fn wants_text(&self) -> bool {
        !matches!(self.page, Page::Game | Page::Settings)
            && !self.active_lobby_form().fields().is_empty()
    }

    /// The player confirmed quitting, once.
    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit)
    }

    /// The leave of the local lobby's own game is to be stopped, once: a new lobby replaces its
    /// server (the shell starts the new one only once the leave has stopped).
    pub fn take_cancel_leave(&mut self) -> bool {
        std::mem::take(&mut self.cancel_leave)
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// The leave the shell is to start, once: its id, the seat's launch and its deadline.
    pub fn take_leave_order(&mut self) -> Option<(u64, Launch, Instant)> {
        self.leave_order.take()
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// The outcome of the leave `id`: taken only for the leave under way, so a leave given up at
    /// its deadline, or replaced, never comes back.
    pub fn leave_done(&mut self, id: u64, outcome: super::leave::Outcome) {
        use super::leave::Outcome;
        let Some(leaving) = self.leaving.take_if(|leaving| leaving.id == id) else {
            return;
        };
        let told = match outcome {
            Outcome::Left => "You left the game",
            Outcome::Refused(_) => "The game holds no seat of yours any more",
            Outcome::Unreachable(_) | Outcome::Cancelled => UNTOLD,
        };
        self.leave_finished(&leaving, told);
    }

    /// The player left the game on screen or under the dashboard, once.
    pub fn take_leave(&mut self) -> bool {
        if self.leave_at.is_some_and(|at| Instant::now() >= at) {
            self.leave_at = None;
            return true;
        }
        std::mem::take(&mut self.leave)
    }

    /// The player confirmed leaving the match, once; the connection closes
    /// [`LEAVE_GAME_FLUSH`] later.
    pub fn take_leave_game(&mut self) -> bool {
        let left = std::mem::take(&mut self.leave_game);
        if left {
            self.leave_at = Some(Instant::now() + LEAVE_GAME_FLUSH);
        }
        left
    }

    /// Whether a game is on screen or under the dashboard.
    pub fn in_game(&self) -> bool {
        self.on_screen.is_some() && !self.launching
    }

    /// The hero of the Demo Hero on screen, if one is.
    pub fn demo_hero(&self) -> Option<bota_proto::HeroId> {
        self.demo
            .filter(|_| matches!(self.on_screen, Some(OnScreen::Demo(_) | OnScreen::Local)))
    }

    /// Whether the game on screen is a replay opened from the menu.
    pub fn replay_from_menu(&self) -> bool {
        self.on_screen == Some(OnScreen::Replay)
    }

    /// Whether leaving the game on screen now may ruin it for the others.
    pub fn set_game_unsafe(&mut self, game_unsafe: bool) {
        self.game_unsafe = game_unsafe;
    }

    /// Whether the live match on screen is over, so that there is nothing to return to.
    pub fn game_is_over(&self) -> bool {
        self.in_game() && self.game_over
    }

    /// Tells whether the live match on screen is over.
    pub fn set_game_over(&mut self, game_over: bool) {
        self.game_over = game_over;
    }

    pub fn page(&self) -> Page {
        self.page
    }

    /// Whether a game is on screen, the menu out of its way.
    pub fn shows_game(&self) -> bool {
        self.page == Page::Game && !self.launching
    }

    /// Leaves the settings screen for the page it was opened from.
    pub fn leave_settings(&mut self) {
        if self.page == Page::Settings {
            self.page = self.settings_from;
        }
    }

    pub fn logged_in(&self) -> bool {
        self.account
    }

    /// The page drawn under the settings while they are open, else the page itself.
    pub(super) fn page_beneath(&self) -> Page {
        if self.page == Page::Settings {
            self.settings_from
        } else {
            self.page
        }
    }

    /// Whether the signed-in account's link to the service was lost and is not open again yet,
    /// whether or not a reconnection is due.
    pub(super) fn reconnecting(&self) -> bool {
        self.account && (self.link_lost || self.retry_at.is_some())
    }

    /// The account's matches not yet over, as far as the menu knows.
    #[cfg(test)]
    pub fn games_running(&self) -> usize {
        self.games.len()
    }

    /// Whether a link is being opened: the Hello or the sign-in is not yet answered.
    pub(super) fn connecting(&self) -> bool {
        matches!(self.conn, Conn::Connecting | Conn::Welcomed)
    }

    /// The game the shell is to open next, once.
    pub fn take_launch(&mut self) -> Option<Launch> {
        self.launch.take()
    }

    /// Whether the game server of the local lobby this client hosts is running.
    pub fn set_hosting(&mut self, running: bool) {
        self.hosting = running;
    }

    /// A new local lobby waits until the stopped leave of the old one has ended.
    pub fn waiting_for_leave(&mut self) {
        self.tell(
            "Your last lobby is still being told you left it: the new one opens in a moment"
                .to_owned(),
            false,
        );
    }

    /// What the shell sees of the game on screen: whether it is in its picks.
    pub fn set_game_in_picks(&mut self, in_picks: bool) {
        self.game_in_picks = in_picks;
    }

    /// The shell has the game on screen.
    pub fn game_started(&mut self) {
        self.launching = false;
    }

    /// The shell could not open the game.
    pub fn launch_failed(&mut self, why: &str) {
        self.launching = false;
        let left = self.on_screen.take();
        self.reconnect = self.last_seat.take();
        self.page = self.rest_page(left);
        self.form = Form::new(self.form_kind(), &self.prefs);
        self.lobby_browser_controls();
        self.tell(
            format!(
                "Could not join the game: {}",
                text::line(why, text::MESSAGE)
            ),
            true,
        );
    }

    /// The player left the game on screen, or it ended (`why` says how when
    /// it was not the player's doing); `over` when the match is over, so
    /// there is no seat to take again.
    pub fn left_game(&mut self, why: Option<&str>, over: bool) {
        let left = self.on_screen.take();
        self.keep_left(left);
        self.launching = false;
        self.status = None;
        self.confirm = None;
        self.game_unsafe = false;
        self.game_over = false;
        self.reconnect = self.last_seat.take().filter(|_| !over);
        self.left_in_picks = self.game_in_picks;
        // A menu that came up under a game reaches the service now.
        if !self.tried_service {
            self.reach_service();
        }
        if let Some(why) = why {
            self.tell(text::line(why, text::MESSAGE), true);
        }
        match left {
            Some(OnScreen::Match(id)) if self.results.iter().any(|r| r.match_id == id) => {
                self.shown = Some(id);
                self.page = Page::Result;
            }
            Some(OnScreen::Match(id)) if self.games.iter().any(|g| g.match_id == id) => {
                self.go(Page::MyGames);
            }
            left => self.page = self.rest_page(left),
        }
        if let Some(page) = self.after_results.take() {
            self.page = page;
        }
        self.form = Form::new(self.form_kind(), &self.prefs);
        self.lobby_browser_controls();
    }

    pub fn return_from_results(&mut self) {
        let page = if self.logged_in() {
            Page::Home
        } else {
            self.rest_page(self.on_screen)
        };
        self.shown = None;
        if self.in_game() {
            self.leave = true;
            self.after_results = Some(page);
        } else {
            self.go_form(page);
        }
    }

    /// The page to fall back to: the page a game left was opened from
    /// (Direct, the local lobby, the replays), else Play once logged in,
    /// else the way in.
    fn rest_page(&self, left: Option<OnScreen>) -> Page {
        match left {
            #[cfg(not(target_arch = "wasm32"))]
            Some(OnScreen::Direct | OnScreen::Local) => Page::LocalLobby,
            #[cfg(not(target_arch = "wasm32"))]
            Some(OnScreen::Demo(_)) => Page::DemoHero,
            Some(OnScreen::Replay) => Page::Replays,
            _ if self.account => Page::Home,
            #[cfg(not(target_arch = "wasm32"))]
            _ if self.prefs.service.is_empty() => Page::LocalLobby,
            _ => Page::Login,
        }
    }

    fn form_kind(&self) -> FormKind {
        match self.page {
            Page::Service => FormKind::Service,
            Page::Login => FormKind::Login,
            Page::Register => FormKind::Register,
            Page::Lobbies => FormKind::Lobby,

            Page::LocalLobby => FormKind::Host,
            Page::Replays => FormKind::Replay,
            Page::Leaderboard => FormKind::Leaderboard,
            _ => FormKind::None,
        }
    }

    pub(super) fn tell(&mut self, text: String, error: bool) {
        self.status = Some(Status { text, error });
    }

    fn save_prefs(&self) {
        if let Err(err) = self.store.write(Entry::Menu, &self.prefs.render()) {
            bevy::log::warn!("menu choices not saved: {err}");
        }
    }

    /// Opens the control connection to the service in the prefs.
    fn connect(&mut self) {
        self.tried_service = true;
        self.asked.clear();
        self.retry_at = None;
        match Client::open(&self.prefs.service, &self.tls) {
            Ok(client) => {
                self.client = Some(client);
                self.conn = Conn::Connecting;
                self.status = None;
            }
            Err(err) => {
                self.conn = Conn::Offline;
                self.credentials = None;
                self.outbox.clear();
                self.tell(format!("Cannot reach the service: {err}"), true);
            }
        }
    }

    /// Whether the link is open and signed in.
    pub(super) fn link_ready(&self) -> bool {
        self.conn == Conn::LoggedIn && self.client.as_ref().is_some_and(|c| !c.hung_up())
    }

    /// Sends a request, noting what it was for its answer. With no link open it waits for one
    /// that is opened now and signed in again with the kept session.
    pub(super) fn ask(&mut self, msg: ClientMsg, asked: Asked) {
        if let Some(read) = self.cached_read(&msg, asked) {
            if let super::list_cache::Read::Reply(reply) = read {
                self.replied((*reply).clone(), asked);
            }
            return;
        }
        if self.link_ready() {
            self.send(msg, asked);
        } else if !self.account {
            self.tell("Sign in first".to_owned(), true);
        } else {
            if self.outbox.len() < MAX_OUTBOX {
                if lobby_requests::is_lobby_request(&msg) {
                    self.lobby_ui
                        .queued_until
                        .get_or_insert_with(|| Instant::now() + lobby_requests::REQUEST_REACH);
                }
                self.outbox.push((msg, asked));
            } else if lobby_requests::is_lobby_request(&msg) {
                self.fail_lobby_request(
                    asked,
                    "Could not queue the lobby request. Try again after reconnecting.",
                );
            }
            if self.conn == Conn::Offline {
                self.connect();
            }
        }
    }

    /// Sends a login or registration: the link opens for it now, the typed values stay in the
    /// form, and nothing is held open before.
    fn sign_in(&mut self, msg: ClientMsg, asked: Asked) {
        self.accepted.clear();
        self.status = None;
        self.credentials = Some((msg, asked));
        if self.conn == Conn::Offline {
            self.connect();
        }
    }

    fn send(&mut self, mut msg: ClientMsg, asked: Asked) {
        let is_lobby = lobby_requests::is_lobby_request(&msg);
        if is_lobby && self.lobby_ui.deadlines.len() == MAX_ASKED {
            if matches!(asked, Asked::LobbyUi(request) if request.step == super::lobby_controls::Saving::Create)
            {
                self.lobby_ui.creation_unconfirmed = false;
            }
            self.fail_lobby_request(
                asked,
                "Too many lobby requests. Wait for the current requests to finish.",
            );
            return;
        }
        #[cfg(test)]
        if let Some(held) = &mut self.lobby_ui.held {
            let seq = u32::MAX - held.len() as u32;
            assert!(held.len() < MAX_ASKED * 2, "bounded test transport");
            held.push(msg);
            self.track_request(seq, asked);
            if is_lobby {
                self.lobby_request_sent(seq, asked);
            }
            return;
        }
        let list_key = self.list_request_key(&msg, asked);
        let lobby_changes = is_lobby
            && !matches!(
                msg,
                ClientMsg::Rulesets | ClientMsg::LobbyList | ClientMsg::LobbySearch { .. }
            );
        let Some(client) = &mut self.client else {
            self.tell("Not connected to the service".to_owned(), true);
            return;
        };
        // Resolve this after login/resume too: queued requests may outlive a connection.
        if let ClientMsg::QueueJoinAudiences { audiences, .. } = &mut msg
            && client
                .me()
                .is_some_and(|account| account.kind == AccountKind::Bot)
        {
            audiences.clear();
            audiences.push(Audience::Everyone);
        }
        match client.request(msg) {
            Ok(seq) => {
                if lobby_changes {
                    self.lobby_list_written();
                }
                if let Some(key) = list_key {
                    self.list_cache.sent(seq, key);
                }
                self.track_request(seq, asked);
                if is_lobby {
                    self.lobby_request_sent(seq, asked);
                }
            }
            Err(err) => self.lost(err),
        }
    }

    /// Closes the link, keeping what the view shows of a signed-in account.
    fn drop_link(&mut self) {
        self.list_cache.disconnected();
        match (&mut self.client, self.account) {
            (Some(client), true) => client.hang_up(),
            _ => self.client = None,
        }
        self.conn = Conn::Offline;
        self.asked.clear();
        self.idle_since = None;
    }

    /// Whether nothing live is on the link: no search or proposal, no lobby, no request or
    /// download in flight, no match of the account on screen or waited for.
    pub(super) fn idle(&self) -> bool {
        let quiet = self.client.as_ref().is_some_and(|c| {
            c.searches().entries().is_empty() && c.searches().slot_waits().is_empty()
        });
        quiet
            && !self.avatar.view.busy
            && self.asked.is_empty()
            && self.lobby_ui.deadlines.is_empty()
            && self.outbox.is_empty()
            && self.credentials.is_none()
            && self.lobby.is_none()
            && !self.keeper.fetching()
            && !self.meta.fetching()
            && !self.launching
            && !matches!(self.on_screen, Some(OnScreen::Match(_) | OnScreen::Demo(_)))
            && !(self.page == Page::MyGames && !self.games.is_empty())
    }

    /// Closes a link that has had nothing live on it for a while, and reopens one that was lost
    /// with something live on it.
    fn keep_link(&mut self) {
        if self.retry_at.is_some_and(|at| Instant::now() >= at) && self.account {
            self.connect();
        }
        if !self.link_ready() || !self.idle() {
            self.idle_since = None;
            return;
        }
        let since = *self.idle_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= IDLE_HANG_UP {
            self.drop_link();
        }
    }

    pub(super) fn ensure_service_link(&mut self) -> bool {
        if self.link_ready() {
            return true;
        }
        if self.account && !self.connecting() {
            self.connect();
        }
        false
    }
    pub(super) fn track_request(&mut self, sequence: u32, asked: Asked) {
        if self.asked.len() >= MAX_ASKED {
            self.asked.remove(0);
        }
        self.asked.push((sequence, asked));
    }

    /// Takes in what the service said since the last frame.
    pub fn update(&mut self) {
        if self.link_ready()
            && let Some(client) = &self.client
        {
            self.accepted.update(client);
        }
        self.poll_lobby_ui();
        self.shelf.take();
        if let Some(leaving) = self
            .leaving
            .take_if(|leaving| Instant::now() >= leaving.until)
        {
            // The game LEAVE was telling did not answer in time: it is forgotten untold, and the
            // leave's own outcome, should it come, is not taken.
            self.leave_finished(&leaving, UNTOLD);
        }
        self.keep_update();
        self.update_local_history();
        self.resume_result_replay();
        self.update_avatar();
        self.drive_leaderboard();
        self.drive_profile();
        if self.page == Page::Replays {
            self.read_summaries();
        }
        self.keep_link();
        #[cfg(test)]
        if self.pause_service_events {
            return;
        }
        for _ in 0..MAX_PER_FRAME {
            let Some(client) = &mut self.client else {
                return;
            };
            match client.poll() {
                Ok(Some(incoming)) => self.take(incoming),
                Ok(None) => return,
                Err(err) => {
                    self.lost(err);
                    return;
                }
            }
        }
    }

    pub(super) fn lost(&mut self, err: ClientError) {
        let creation_unconfirmed = self.lobby_ui.creation_unconfirmed;
        let was_signed_in = self.conn == Conn::LoggedIn;
        self.keep_link_lost();
        self.board.disconnected();
        self.profiles.disconnected();
        self.meta_link_lost();
        let live = was_signed_in && !self.idle();
        self.link_lost = self.account;
        self.credentials = None;
        self.outbox.clear();
        self.lobby = None;
        self.moving = None;
        self.lobby_ui.deadlines.clear();
        self.lobby_ui.uncertain_join = None;
        self.lobby_ui.dialog = None;
        self.lobby_ui.editor = None;
        self.lobby_ui.open = None;
        self.lobby_ui.chat = Default::default();
        self.lobby_secret = None;
        self.demo_asked = None;
        self.drop_link();
        if !self.account && !matches!(self.page, Page::Game | Page::LocalLobby) {
            // Not signed in: the pages of the account have nothing to show.
            if self.page.needs_account() {
                let rest = self.rest_page(None);
                self.go_form(rest);
            }
        }
        // Something live on the link is picked up again at once, once; the rest waits for the
        // next request.
        if creation_unconfirmed {
            self.lobby_ui.creation_unconfirmed = false;
            if matches!(self.page, Page::Lobby | Page::Lobbies) {
                self.go_form(Page::Lobbies);
            }
            self.tell(lobby_requests::CREATE_UNCONFIRMED.into(), true);
        }
        if live && self.retry {
            self.retry = false;
            self.retry_at = Some(Instant::now() + RECONNECT_AFTER);
            return;
        }
        let why = match err {
            ClientError::Refused(error) => error_text(&error),
            other => text::line(&other.to_string(), text::MESSAGE),
        };
        let what = if was_signed_in {
            "Disconnected from the service"
        } else {
            "Cannot reach the service"
        };
        if !creation_unconfirmed {
            self.tell(format!("{what}: {why}"), true);
        }
    }

    fn take(&mut self, incoming: Incoming) {
        match incoming {
            Incoming::Reply { seq, result } => self.take_reply(seq, result),
            Incoming::Event(event) => self.event(event),
        }
    }

    fn take_reply(&mut self, seq: u32, result: Result<Reply, Error>) {
        self.remember_list_reply(seq, &result);
        let remembered = self
            .lobby_ui
            .deadlines
            .iter()
            .find(|(known, _, _)| *known == seq)
            .map(|(_, _, asked)| *asked);
        self.lobby_ui
            .deadlines
            .retain(|(known, _, _)| *known != seq);
        let asked = self
            .asked
            .iter()
            .position(|(s, _)| *s == seq)
            .map(|at| self.asked.swap_remove(at).1)
            .or(remembered);
        match (result, asked) {
            (_, Some(Asked::Logout)) => {
                self.drop_link();
                // A sign-in typed while the old session was being ended goes now.
                if self.credentials.is_some() {
                    self.connect();
                }
            }
            (Ok(reply), Some(Asked::RestoreLobby(lobby))) => self.lobby_resumed(lobby, reply),
            (Err(error), Some(Asked::RestoreLobby(lobby))) => {
                self.lobby_resume_failed(lobby, error_text(&error))
            }
            (Ok(reply), Some(Asked::LobbyUi(request))) => self.lobby_save_replied(reply, request),
            (Ok(Reply::Welcome { rulesets, .. }), _) => {
                self.receive_lobby_catalogue(&rulesets);
                self.welcomed();
            }
            (Ok(reply), asked) => self.replied(reply, asked.unwrap_or(Asked::Other)),
            (Err(error), asked) => self.refused_request(&error, asked.unwrap_or(Asked::Other)),
        }
    }

    fn welcomed(&mut self) {
        self.conn = Conn::Welcomed;
        if let Some((msg, asked)) = self.credentials.take() {
            self.send(msg, asked);
        } else if let Some(saved) = self.saved_session() {
            self.send(
                ClientMsg::Resume {
                    session: saved.token,
                    client: Default::default(),
                },
                Asked::Resume,
            );
        } else {
            // Nothing to say to the service: no link is kept for nothing.
            self.drop_link();
        }
    }

    fn replied(&mut self, reply: Reply, asked: Asked) {
        if let Asked::Visitor(generation, part) = asked {
            self.profile_reply(generation, part, reply);
            return;
        }
        match reply {
            Reply::Done if matches!(asked, Asked::LeaveLobby(_)) => {
                if let Asked::LeaveLobby(id) = asked {
                    self.lobby_left(id);
                }
            }
            Reply::Done if matches!(asked, Asked::Accept(_)) => {
                if let Asked::Accept(proposal) = asked
                    && let Some(client) = &self.client
                {
                    self.accepted.remember(client, proposal);
                }
            }
            Reply::LoggedIn {
                session,
                activities,
                ..
            } => {
                self.received_login(session, activities);
            }
            Reply::Handoffs { list } => {
                for handoff in list.into_iter().take(MAX_GAMES) {
                    self.note_game(handoff);
                }
            }
            Reply::Handoff { handoff } => {
                self.note_game(handoff.clone());
                match asked {
                    Asked::Rejoin(_) => self.start(Launch::Seat(handoff)),
                    Asked::Watch(_) => self.start(Launch::Watch(handoff)),
                    _ => {}
                }
            }
            Reply::Matches { list } if matches!(asked, Asked::History) => {
                self.history = list.into_iter().take(usize::from(HISTORY_LEN)).collect();
            }
            Reply::Account { account }
                if matches!(asked, Asked::Profile) && self.my_id() == Some(account.id) =>
            {
                self.account_profile = Some(account)
            }
            Reply::LeaderboardPage { page } => {
                if let Asked::Leaderboard(generation) = asked {
                    self.leaderboard_page(generation, page);
                }
            }
            Reply::Artifact {
                match_id,
                offset,
                total,
                data,
            } => self.keep_chunk(match_id, offset, total, &data),
            Reply::ArtifactNamed {
                match_id,
                name,
                offset,
                total,
                data,
            } => self.meta_chunk(match_id, &name, offset, total, &data),
            Reply::Lobbies { list } => self.receive_lobby_list(list),
            Reply::Rulesets { list } => self.receive_lobby_catalogue(&list),
            Reply::Lobby { state } if matches!(asked, Asked::Join(id) if id == state.id) => {
                self.join_prompt = None;
                if self.sit(state) {
                    self.status = None;
                }
            }
            Reply::Queued { .. } | Reply::QueuesJoined { .. } => self.status = None,
            _ => {}
        }
    }

    fn refused_request(&mut self, error: &Error, asked: Asked) {
        if let Asked::Join(lobby) = asked
            && self.join_prompt.as_ref().is_some_and(|p| p.lobby != lobby)
        {
            return;
        }
        if let Asked::LobbyUi(request) = asked {
            self.lobby_save_refused(request, error);
            return;
        }
        if let (Asked::Accept(proposal), Error::UnknownProposal) = (asked, error) {
            self.accepted.remove(proposal);
        }
        if let Error::QueueFull { retry_after_ms } = error {
            self.play.open = true;
            self.queue_full_until =
                Some(Instant::now() + std::time::Duration::from_millis(u64::from(*retry_after_ms)));
            self.status = None;
        } else {
            self.refused(error, asked);
        }
    }

    fn received_login(
        &mut self,
        session: bota_radovid::proto::SessionToken,
        activities: bota_radovid::proto::Activities,
    ) {
        self.accepted.resumed(&activities);
        self.conn = Conn::LoggedIn;
        let first = !self.account;
        self.account = true;
        if self.page == Page::Leaderboard {
            self.board.refresh();
        }
        self.retry = true;
        self.link_lost = false;
        let saved = Saved {
            service: self.prefs.service.clone(),
            token: session,
        };
        if let Err(err) = self.store.write(Entry::Session, &saved.render()) {
            bevy::log::warn!("login not kept for the next run: {err}");
        }
        let queued_until = self.lobby_ui.queued_until.take();
        self.ask(ClientMsg::Rulesets, Asked::LobbyCatalogue);
        self.restore_lobby_membership(&activities.lobbies);
        self.games = activities
            .matches
            .iter()
            .take(MAX_GAMES)
            .map(|&match_id| Game {
                match_id,
                handoff: None,
            })
            .collect();
        self.send(ClientMsg::MyMatches, Asked::Other);
        self.keep_signed_in();
        self.lobby_ui.queued_until = queued_until;
        for (msg, asked) in std::mem::take(&mut self.outbox) {
            self.send(msg, asked);
        }
        self.lobby_ui.queued_until = None;
        if first {
            self.status = None;
            if !matches!(self.page, Page::Game) {
                self.go(Page::Home);
            }
        }
    }

    fn refused(&mut self, error: &Error, asked: Asked) {
        if let Asked::Visitor(generation, part) = asked {
            self.profile_refused(generation, part, error);
            return;
        }
        // A join refused for whatever reason may be a list out of date (the lobby full, locked,
        // started or gone since): the list is asked again, as the search typed asks it.
        if matches!(asked, Asked::Join(_)) && self.page == Page::Lobbies {
            self.search_lobbies();
        }
        match (asked, error) {
            (Asked::Leaderboard(generation), error) => self.leaderboard_refused(generation, error),
            (Asked::Resume, _) => {
                self.accepted.clear();
                self.store.remove(Entry::Session);
                self.account = false;
                self.outbox.clear();
                self.drop_link();
                self.go_form(Page::Login);
                if *error != Error::BadSession {
                    self.tell(error_text(error), true);
                }
            }
            (Asked::Login | Asked::Register, _) => {
                // The form stays as typed for another try; no link is held for it.
                self.drop_link();
                self.tell(error_text(error), true);
            }
            (Asked::Rejoin(id) | Asked::Watch(id), Error::UnknownMatch) => {
                self.games.retain(|g| g.match_id != id);
                self.tell("That match is over".to_owned(), true);
            }
            (Asked::Artifact(id), error) => self.keep_refused(id, error),
            (Asked::Demo, _) => {
                self.demo_asked = None;
                self.tell(error_text(error), true);
            }
            (Asked::Meta(id), _) => self.meta_none(id),
            (Asked::LeaveLobby(lobby), Error::NotInLobby { .. } | Error::UnknownLobby) => {
                self.lobby_left(lobby);
            }
            (Asked::Lobby, Error::NotInLobby { .. }) => {
                self.lobby = None;
                self.go(Page::Lobbies);
            }
            (Asked::Join(lobby), Error::LobbyPasswordRequired) => {
                // Locked since it was listed.
                let name = self
                    .lobbies
                    .iter()
                    .find(|l| l.id == lobby)
                    .map_or_else(String::new, |l| l.name.clone());
                self.ask_password(lobby, name);
            }
            (Asked::Join(lobby), Error::BadLobbyPassword | Error::Throttled { .. })
                if self.join_prompt.as_ref().is_some_and(|p| p.lobby == lobby) =>
            {
                // The dialog stays for another try.
                let why = match error {
                    Error::BadLobbyPassword => "Wrong password.".to_owned(),
                    other => error_text(other),
                };
                if let Some(prompt) = &mut self.join_prompt {
                    prompt.error = Some(why);
                }
            }
            (Asked::Join(_), _) => {
                self.join_prompt = None;
                self.tell(error_text(error), true);
            }
            _ => self.tell(error_text(error), true),
        }
    }

    fn event(&mut self, event: Event) {
        self.accepted.event(&event);
        match event {
            Event::MatchFound { handoff } => {
                self.note_game(handoff.clone());
                let demo = self
                    .demo_asked
                    .filter(|_| self.is_demo_ruleset(&handoff.ruleset));
                if demo.is_some() {
                    self.demo_asked = None;
                }
                if self.on_screen.is_none() && !self.launching {
                    match demo {
                        Some(hero) => self.start(Launch::Demo { hero, handoff }),
                        None => self.start(Launch::Seat(handoff)),
                    }
                } else {
                    self.tell(
                        "Another game of yours is ready: join it from MY GAMES".to_owned(),
                        false,
                    );
                }
            }
            Event::MatchResult { result } => self.finished(result),
            Event::LobbyState { state } => {
                if self.lobby_ui.active == Some(state.id) {
                    self.update_lobby_state(state);
                }
            }
            Event::LobbyStarting { lobby, in_ms } => {
                if let Some((state, at)) = &mut self.lobby
                    && state.id == lobby
                {
                    state.starting_in_ms = Some(in_ms);
                    *at = Instant::now();
                }
            }
            Event::LobbyKicked { lobby } => self.unseat(lobby, "You were removed from the lobby"),
            Event::LobbyClosed { lobby } => self.unseat(lobby, "The lobby was closed"),
            Event::ProposalFailed {
                back_in_queue: true,
                ..
            } => self.tell(
                "The match did not start: you are back in the queue".to_owned(),
                false,
            ),
            Event::QueueLeft { reason, .. } => {
                let (message, error) = match reason {
                    QueueLeftReason::Matched => return,
                    QueueLeftReason::Cancelled => ("Search cancelled", false),
                    QueueLeftReason::TimedOut => (
                        "The match was not accepted in time: the search stopped",
                        true,
                    ),
                    QueueLeftReason::Declined => {
                        ("You declined the match: the search stopped", true)
                    }
                };
                self.tell(message.into(), error);
            }
            Event::Bye { error } => self.tell(error_text(&error), true),
            _ => {}
        }
    }

    fn finished(&mut self, result: MatchResult) {
        let id = result.match_id;
        if self.page == Page::MyGames {
            self.ask_history();
        }
        if matches!(&self.reconnect, Some(Launch::Seat(h)) if h.match_id == id) {
            self.reconnect = None;
        }
        self.games.retain(|g| g.match_id != id);
        // A Demo Hero ends when its player quits it: no result to read, no replay to keep.
        if self.demo_match == Some(id) {
            return;
        }
        self.keep_finished(&result);
        self.meta_finished(&result);
        if self.results.len() >= MAX_RESULTS {
            self.results.pop_front();
        }
        self.results.push_back(result);
        if self.on_screen.is_none() && !self.launching {
            self.shown = Some(id);
            self.page = Page::Result;
            self.result_request = self.outcome_result();
        }
    }

    pub(super) fn my_id(&self) -> Option<AccountId> {
        self.client.as_ref().and_then(|c| c.me()).map(|me| me.id)
    }

    fn ask_history(&mut self) {
        if let Some(account) = self.my_id() {
            let msg = ClientMsg::RecentMatches {
                account,
                limit: HISTORY_LEN,
            };
            self.ask(msg, Asked::History);
        }
    }

    /// The account's public per-mode ratings and ranks.
    fn ask_profile(&mut self) {
        let Some(account) = self.my_id() else {
            return;
        };
        self.ask(ClientMsg::Account { account }, Asked::Profile);
    }

    /// Whether a Demo Hero can be started from here: a desktop always can (the service's, else
    /// a game of its own), a browser only when the service lists one.
    pub(super) fn demo_offered(&self) -> bool {
        Self::demo_policy(
            cfg!(not(target_arch = "wasm32")),
            self.service_offers_demo(),
        )
    }

    /// The rule of [`Self::demo_offered`], for a client that can start its own game or not.
    pub(crate) fn demo_policy(can_host: bool, service_offers: bool) -> bool {
        can_host || service_offers
    }

    fn service_offers_demo(&self) -> bool {
        self.menu_rulesets().any(bota_radovid::is_demo)
    }

    /// Whether the service lists `ruleset` as a Demo Hero.
    pub(super) fn is_demo_ruleset(&self, ruleset: &RulesetId) -> bool {
        self.menu_rulesets()
            .any(|r| &r.id == ruleset && bota_radovid::is_demo(r))
    }

    /// Dota's Demo Hero with `hero`: the service's Demo ruleset started for this account alone, at
    /// once (no queue, lobby or countdown); its match comes as any match does ([`Self::event`]).
    /// Without such a ruleset a desktop starts a game of its own; a browser says so.
    fn start_demo(&mut self, hero: bota_proto::HeroId) {
        if bota_core::hero_def(hero).is_none() || self.demo_asked.is_some() {
            return;
        }
        let ruleset = self
            .menu_rulesets()
            .find(|r| bota_radovid::is_demo(r))
            .map(|r| r.id.clone());
        let Some(ruleset) = ruleset else {
            #[cfg(not(target_arch = "wasm32"))]
            self.start(Launch::LocalDemo {
                hero,
                name: self.name(),
                key: Claim::draw(),
                server: self.server_binary.clone(),
                server_environment: self.server_environment,
            });
            #[cfg(target_arch = "wasm32")]
            self.tell("This service offers no Demo Hero".to_owned(), true);
            return;
        };
        self.demo_asked = Some(hero);
        self.tell("Starting Demo Hero".to_owned(), false);
        self.ask(ClientMsg::StartSolo { ruleset }, Asked::Demo);
    }

    fn sit(&mut self, state: LobbyState) -> bool {
        let id = state.id;
        if !self.update_lobby_state(state) {
            return false;
        }
        self.lobby_ui.active = Some(id);
        self.lobby_ui.selected = Some(id);
        if self.page == Page::Lobbies {
            self.go_form(Page::Lobby);
        }
        true
    }

    fn update_lobby_state(&mut self, state: LobbyState) -> bool {
        let Some(state) = lobby_data::sanitise_state(state) else {
            self.tell("Invalid lobby information from the service.".into(), true);
            return false;
        };
        self.lobby_ui.controls_dirty = true;
        self.lobby_ui.state_dirty = true;
        if self
            .lobby
            .as_ref()
            .is_none_or(|(old, _)| old.id != state.id)
            || self
                .moving
                .is_some_and(|id| !state.members.iter().any(|member| member.account == id))
        {
            self.moving = None;
        }
        self.lobby_ui.server = super::lobby_controls::Server::Service;
        let locked = state.locked;
        self.lobby = Some((state, Instant::now()));
        self.invalidate_lobby_password(locked);
        true
    }

    fn unseat(&mut self, lobby: LobbyId, why: &str) {
        if self.lobby.as_ref().is_some_and(|(s, _)| s.id == lobby) {
            self.lobby = None;
            self.lobby_secret = None;
            self.moving = None;
            self.lobby_ui.active = None;
            self.lobby_ui.editor = None;
            if self.page == Page::Lobby {
                self.go(Page::Lobbies);
            }
        }
        self.tell(why.to_owned(), false);
    }

    fn note_game(&mut self, handoff: Handoff) {
        if let Some(game) = self
            .games
            .iter_mut()
            .find(|g| g.match_id == handoff.match_id)
        {
            game.handoff = Some(handoff);
        } else if self.games.len() < MAX_GAMES {
            self.games.push(Game {
                match_id: handoff.match_id,
                handoff: Some(handoff),
            });
        }
    }

    pub(super) fn start(&mut self, launch: Launch) {
        if matches!(launch, Launch::Host { .. } | Launch::LocalDemo { .. })
            && self.leaving.as_ref().is_some_and(|leaving| leaving.hosted)
        {
            // The new lobby replaces the server the leave was telling: the leave stops first
            // (or, not started yet, never starts), and its outcome is never taken.
            self.leaving = None;
            self.leave_order = None;
            self.cancel_leave = true;
        }
        if self
            .leaving
            .as_ref()
            .is_some_and(|leaving| SeatOf::of(&launch).as_ref() == Some(&leaving.game))
        {
            self.tell(
                "That game is being told you left it: wait until it is".to_owned(),
                true,
            );
            return;
        }
        if self.on_screen.is_some() {
            self.tell("Disconnect from the game you are in first".to_owned(), true);
            return;
        }
        self.launch_context(&launch);
        self.reconnect = None;
        self.confirm = None;
        self.play.open = false;
        self.launch = Some(launch);
        self.launching = true;
        self.opening_since = Some(Instant::now());
        self.page = Page::Game;
        self.status = None;
    }

    /// Moves to `page` with its form fresh.
    pub(super) fn go_form(&mut self, page: Page) {
        if page != Page::Home && page != Page::Settings {
            self.play.open = false;
        }
        self.lobby_ui.controls_dirty = true;
        self.lobby_ui.search_due = None;
        self.lobby_ui.uncertain_join = None;
        if self.lobby_ui.editor.as_ref().is_some_and(|editor| {
            page != Page::Lobby || editor.saving != Some(super::lobby_controls::Saving::Create)
        }) {
            self.cancel_lobby_editor();
        }
        self.lobby_ui.open = None;
        self.lobby_ui.chat.focused = false;
        self.page = page;
        self.join_prompt = None;
        self.form = Form::new(self.form_kind(), &self.prefs);
        self.lobby_browser_controls();
    }

    pub(super) fn go(&mut self, page: Page) {
        // The settings come back to where they were opened from.
        if page == Page::Settings {
            if self.page != Page::Settings && self.page != Page::Game {
                self.settings_from = self.page;
                self.page = Page::Settings;
            }
            return;
        }
        // Signing in starts with the service's address, until there is one.
        let page = match page {
            Page::Login | Page::Register if self.prefs.service.is_empty() => Page::Service,
            page => page,
        };
        if page.needs_account() && !self.logged_in() {
            let rest = self.rest_page(None);
            self.go_form(rest);
            return;
        }
        if self.page == Page::Game && !self.launching && self.on_screen.is_some() {
            return;
        }
        self.go_form(page);
        self.plain_host = None;
        match page {
            Page::MyGames => {
                self.ask(ClientMsg::MyMatches, Asked::Other);
                self.ask_history();
            }
            Page::Profile | Page::MatchHistory => {
                self.ask_profile();
                self.ask_history();
            }
            Page::Leaderboard => self.open_leaderboard(),
            Page::Lobbies => self.ask(ClientMsg::LobbyList, Asked::Other),
            Page::Replays => self.read_summaries(),
            _ => {}
        }
    }

    fn launch_context(&mut self, launch: &Launch) {
        self.loading = self.details_of(launch);
        self.on_screen = Some(match launch {
            Launch::Seat(h) | Launch::Watch(h) => OnScreen::Match(h.match_id),
            Launch::Direct { .. } => OnScreen::Direct,
            Launch::Host { .. } | Launch::LocalDemo { .. } => OnScreen::Local,
            Launch::Demo { handoff, .. } => OnScreen::Demo(handoff.match_id),
            Launch::Replay(_) | Launch::Blob(_) => OnScreen::Replay,
            Launch::Given => OnScreen::Given,
        });
        self.keep_start(launch);
        self.hosted_at = match launch {
            Launch::Host {
                port, lan: true, ..
            } => Some(match super::launch::lan_address() {
                Some(ip) => std::net::SocketAddr::new(ip, *port).to_string(),
                None => format!("this machine's address, port {port}"),
            }),
            Launch::Host { port, .. } => Some(format!("127.0.0.1:{port} (this machine only)")),
            _ => None,
        };
        self.kind = launch.kind();
        // A seat is taken again on the same server: a local lobby's by its
        // address, the server already up.
        self.last_seat = match launch {
            Launch::Host {
                port, name, key, ..
            } => Some(Launch::Direct {
                address: format!("127.0.0.1:{port}"),
                name: name.clone(),
                spectate: false,
                key: *key,
                hosted: true,
            }),
            // Leaving a Demo Hero ends it: there is no seat to take again.
            Launch::Demo { .. } | Launch::LocalDemo { .. } => None,
            launch => (self.kind == GameKind::Play).then(|| launch.clone()),
        };
        self.demo = match launch {
            Launch::Demo { hero, .. } | Launch::LocalDemo { hero, .. } => Some(*hero),
            _ => None,
        };
        if let Launch::Demo { handoff, .. } = launch {
            self.demo_match = Some(handoff.match_id);
        }
    }

    /// Acts on what the player did.
    pub fn act(&mut self, act: Act) {
        if self.route_lobby_overlay(&act) {
            return;
        }
        // A match offer takes priority over this editor. Otherwise its modal owns menu input,
        // including keyboard navigation, until cancelled or its save completes.
        if self.avatar.view.open && self.view().offer.is_none() {
            match act {
                Act::Back => {
                    self.avatar.act(super::avatar::Action::Cancel);
                    return;
                }
                Act::Avatar(_) => {}
                _ => return,
            }
        }
        assert!(self.dispatch(act).is_none(), "menu action has no handler");
    }

    fn dispatch(&mut self, act: Act) -> Option<Act> {
        match act {
            Act::Go(_)
            | Act::Play(_)
            | Act::Avatar(_)
            | Act::InspectHero(_)
            | Act::InspectPlayer(_)
            | Act::MatchDetails(_)
            | Act::WatchHistory(_)
            | Act::HistoryMode(_)
            | Act::HistoryResult(_)
            | Act::HistoryHero(_)
            | Act::HistoryFilter(_)
            | Act::HistoryPage(_)
            | Act::LeaderboardMode(_)
            | Act::LeaderboardPlayers(_) => self.act_pages(act),
            Act::Focus(_)
            | Act::Type(_)
            | Act::Erase
            | Act::NextField
            | Act::PreviousField
            | Act::Submit
            | Act::Logout => self.act_input(act),
            Act::Cancel(_)
            | Act::Accept(_)
            | Act::Decline(_)
            | Act::Rejoin(_)
            | Act::Watch(_)
            | Act::LobbyUi(_)
            | Act::JoinLobby(_)
            | Act::SearchLobbies
            | Act::LobbyPassword(_)
            | Act::ReadyLobby(_)
            | Act::LeaveLobby
            | Act::Direct(_)
            | Act::Host(_)
            | Act::ToggleCheats
            | Act::Rules(_)
            | Act::LobbySlot(_)
            | Act::MoveMember(_)
            | Act::KickMember(_) => self.act_lobby(act),
            Act::OpenReplay(_)
            | Act::DeleteReplay(_)
            | Act::WatchReplay(_)
            | Act::PlainAnyway
            | Act::Dismiss
            | Act::RetryService
            | Act::Quit
            | Act::Answer(_)
            | Act::Back => self.act_library(act),
            Act::Dashboard
            | Act::ReturnToGame
            | Act::Disconnect
            | Act::LeaveGame
            | Act::Reconnect
            | Act::LeaveSeat => self.act_game(act),
            #[cfg(test)]
            Act::Find(_) => self.act_lobby(act),
            Act::Demo(_) => self.act_library(act),
        }
    }

    fn act_pages(&mut self, act: Act) -> Option<Act> {
        match act {
            Act::Go(Page::Home) => {
                self.play.open = false;
                self.go(Page::Home);
            }
            Act::Go(page) => self.go(page),
            Act::Play(action) => self.play_action(action),
            Act::MatchDetails(key) => self.open_result(&key),
            Act::WatchHistory(key) => {
                if let Some(file) = self
                    .local_history
                    .find(&key)
                    .and_then(|entry| entry.file.clone())
                    && let Err(failure) =
                        self.watch_result(crate::postgame::view::Replay::Saved(file))
                {
                    self.tell(failure.message().to_owned(), true);
                }
            }
            Act::HistoryMode(mode) => {
                self.history_filter.mode = mode;
                self.reset_history_filter();
            }
            Act::HistoryResult(result) => {
                self.history_filter.result = result;
                self.reset_history_filter();
            }
            Act::HistoryHero(hero) => {
                self.history_filter.hero = hero;
                self.reset_history_filter();
            }
            Act::HistoryFilter(filter) => {
                self.history_picker = (self.history_picker != Some(filter)).then_some(filter);
            }
            Act::HistoryPage(delta) => {
                self.history_page = self.history_page.saturating_add_signed(isize::from(delta));
            }
            Act::Avatar(action) => self.avatar.act(action),
            Act::InspectPlayer(target) => {
                if let Err(message) = self.inspect_player(target) {
                    self.tell(message, true);
                }
            }
            Act::InspectHero(hero) => {
                if bota_core::hero_def(hero).is_some() {
                    self.go(Page::Hero(hero));
                }
            }
            Act::LeaderboardMode(mode) => {
                let mut query = self.board.query.clone();
                query.mode = mode;
                self.select_leaderboard(query);
            }
            Act::LeaderboardPlayers(players) => {
                let mut query = self.board.query.clone();
                query.players = players;
                self.select_leaderboard(query);
            }
            other => return Some(other),
        }
        None
    }

    fn act_input(&mut self, act: Act) -> Option<Act> {
        match act {
            Act::Focus(at) => self.keyboard().focus(at),
            Act::Type(typed) => {
                self.lobby_type(Some(&typed));
                self.edit_leaderboard();
            }
            Act::Erase => {
                self.lobby_type(None);
                self.edit_leaderboard();
            }
            Act::NextField => {
                self.keyboard().next();
                self.follow_lobby_choice_focus();
            }
            Act::PreviousField => {
                self.keyboard().previous();
                self.follow_lobby_choice_focus();
            }
            Act::Submit => self.press(),
            Act::Logout => self.logout(),
            other => return Some(other),
        }
        None
    }

    fn act_lobby(&mut self, act: Act) -> Option<Act> {
        match act {
            #[cfg(test)]
            Act::Find(ruleset) => self.queue(ruleset),
            Act::Cancel(entry) => self.ask(ClientMsg::QueueLeave { entry }, Asked::Other),
            Act::Accept(proposal) => {
                self.ask(ClientMsg::MatchAccept { proposal }, Asked::Accept(proposal))
            }
            Act::Decline(proposal) => self.ask(ClientMsg::MatchDecline { proposal }, Asked::Other),
            Act::Rejoin(id) => self.enter(id, false),
            Act::Watch(id) => self.enter(id, true),
            Act::LobbyUi(action) => self.lobby_action(action),
            Act::JoinLobby(lobby) => self.join_lobby(lobby),
            Act::SearchLobbies => self.refresh_lobbies(),
            Act::LobbyPassword(true) => self.send_lobby_password(),
            Act::LobbyPassword(false) => {
                self.cancel_lobby_password();
            }
            Act::ReadyLobby(ready) => {
                self.lobby_ask(|lobby| ClientMsg::LobbyReady { lobby, ready })
            }
            Act::LeaveLobby => {
                if let Some((state, _)) = &self.lobby {
                    let lobby = state.id;
                    self.ask(ClientMsg::LobbyLeave { lobby }, Asked::LeaveLobby(lobby));
                }
            }
            Act::Direct(spectate) => self.direct(spectate, false),
            // The client does not know who plays in the lobby it hosts: it says what ending its
            // server means, and ends it only when told to.
            Act::Host(lan) if self.hosting => self.confirm = Some(Confirm::Rehost(lan)),
            Act::Host(lan) => self.host(lan),
            Act::LobbySlot(slot) => {
                let account = self.moving.take();
                self.lobby_ask(|lobby| ClientMsg::LobbySlot {
                    lobby,
                    account,
                    slot,
                });
            }
            Act::MoveMember(id) => self.moving = (self.moving != Some(id)).then_some(id),
            Act::KickMember(account) => {
                self.lobby_ask(|lobby| ClientMsg::LobbyKick { lobby, account })
            }
            Act::Rules(rules) => {
                self.prefs.host_rules = rules;
                self.save_prefs();
            }
            Act::ToggleCheats => {
                self.prefs.host_cheats = !self.prefs.host_cheats;
                self.save_prefs();
            }
            other => return Some(other),
        }
        None
    }

    fn act_library(&mut self, act: Act) -> Option<Act> {
        match act {
            Act::Demo(hero) => self.start_demo(bota_proto::HeroId(hero)),
            Act::OpenReplay(name) => self.open_listed(&name),
            Act::DeleteReplay(name) => self.library.delete(&name),
            Act::WatchReplay(match_id) => self.watch_replay(match_id),
            Act::PlainAnyway => self.direct(false, true),
            Act::Dismiss => {
                self.status = None;
                if let Some(editor) = &mut self.lobby_ui.editor {
                    editor.error = None;
                }
            }
            Act::RetryService => {
                if self.reconnecting() {
                    self.retry_at = Some(Instant::now());
                }
            }
            Act::Quit if !cfg!(target_arch = "wasm32") => self.confirm = Some(Confirm::Quit),
            Act::Quit => {}
            Act::Answer(yes) => match self.confirm.take() {
                Some(Confirm::Quit) if yes => self.quit = true,
                Some(Confirm::Leave) if yes => self.leave = true,
                Some(Confirm::LeaveGame) if yes => {
                    self.last_seat = None;
                    self.leave_game = true;
                }
                Some(Confirm::LeaveSeat) if yes => self.leave_seat(),
                Some(Confirm::Rehost(lan)) if yes => self.host(lan),
                _ => {}
            },
            Act::Back => self.back(),
            other => return Some(other),
        }
        None
    }

    fn act_game(&mut self, act: Act) -> Option<Act> {
        match act {
            Act::Dashboard => {
                if self.page == Page::Game && self.in_game() {
                    let top = self.rest_page(None);
                    self.go_form(top);
                }
            }
            Act::ReturnToGame => {
                if self.in_game() {
                    self.confirm = None;
                    self.plain_host = None;
                    self.page = Page::Game;
                }
            }
            Act::Disconnect => {
                if self.in_game() {
                    match self.kind {
                        // A Demo Hero is nobody else's to keep: leaving it ends it.
                        GameKind::Play if self.demo_hero().is_some() => self.leave_game = true,
                        GameKind::Play => self.leave = true,
                        GameKind::Watch | GameKind::Replay => self.confirm = Some(Confirm::Leave),
                    }
                }
            }
            Act::LeaveGame => {
                if self.in_game() && self.kind == GameKind::Play && self.game_unsafe {
                    self.confirm = Some(Confirm::LeaveGame);
                }
            }
            Act::Reconnect => {
                if let Some(launch) = self.reconnect.clone() {
                    self.start(launch);
                }
            }
            Act::LeaveSeat => {
                if self.reconnect.is_some() {
                    self.confirm = Some(Confirm::LeaveSeat);
                }
            }
            other => return Some(other),
        }
        None
    }

    /// Escape over the menu, as Dota's: the question first, then the page
    /// back to the top one, then the game under the dashboard, and on the
    /// top page with nothing under it, whether to quit.
    fn back(&mut self) {
        if self.dismiss_lobby_ui() {
            return;
        }
        if self.play.open {
            self.play.open = false;
            return;
        }
        if self.confirm.take().is_some()
            || self.plain_host.take().is_some()
            || self.cancel_lobby_password()
        {
            return;
        }
        let top = self.rest_page(None);
        match self.page {
            // The game and the settings take their own Escape; a game being
            // opened is waited for.
            Page::Game | Page::Settings => {}
            Page::PlayerProfile(_) => self.back_profile(),
            Page::Register => self.go_form(Page::Login),
            Page::Lobby => self.go(Page::Lobbies),
            page if page != top => self.go(top),
            _ if self.in_game() => self.act(Act::ReturnToGame),
            // A browser's page is closed by the browser.
            _ if cfg!(target_arch = "wasm32") => {}
            _ => self.confirm = Some(Confirm::Quit),
        }
    }

    /// Enter: presses the button that has the keyboard, else submits the form.
    pub(super) fn blur_menu_focus(&mut self) {
        self.form.blur();
    }

    fn press(&mut self) {
        match self.keyboard().focused_button().cloned() {
            Some(Act::Host(_)) if self.hosted_at.is_some() => {}
            Some(Act::Submit) | None => self.submit(),
            Some(button) => self.act(button),
        }
    }

    fn submit(&mut self) {
        if self.join_prompt.is_some() {
            self.send_lobby_password();
            return;
        }
        if self.lobby_ui.editor.is_some() {
            self.save_lobby_editor();
            return;
        }
        if self.lobby_ui.chat.focused {
            self.lobby_ui.chat.send();
            return;
        }
        if self.join_prompt.is_some() {
            self.send_lobby_password();
            return;
        }
        match self.page {
            Page::Service => {
                let service = self.form.value(0).trim().to_owned();
                if let Err(why) = service_address(&service) {
                    self.tell(why, true);
                    return;
                }
                if !bota_radovid::same_matchmaker(&self.prefs.service, &service) {
                    self.keep_link_lost();
                    self.meta_link_lost();
                    self.logout();
                    self.drop_link();
                    self.results.clear();
                    self.history.clear();
                    self.shown = None;
                    self.result_request = None;
                }
                self.prefs.service = service;
                self.save_prefs();
                self.status = None;
                self.go_form(Page::Login);
            }
            Page::Login => self.login(),
            Page::Register => self.register(),
            Page::LocalLobby if self.form.focused_field() == Some(2) => self.direct(false, false),
            Page::LocalLobby => self.host(false),
            Page::Lobbies if self.form.focused_field() == Some(SEARCH_FIELD) => {
                self.search_lobbies();
            }
            Page::Lobbies => self.lobby_action(super::LobbyAction::Create),
            _ => {}
        }
    }

    fn login(&mut self) {
        let login = self.form.value(0).trim().to_owned();
        let password = Secret(self.form.value(1).to_owned());
        if login.is_empty() || password.0.is_empty() {
            self.tell("Type your login and password".to_owned(), true);
            return;
        }
        self.prefs.login = login.clone();
        self.save_prefs();
        let client = Default::default();
        self.sign_in(
            ClientMsg::Login {
                login,
                password,
                client,
            },
            Asked::Login,
        );
    }

    fn register(&mut self) {
        let login = self.form.value(0).trim().to_owned();
        let (password, again) = (self.form.value(1).to_owned(), self.form.value(2).to_owned());
        let complaint = if !valid_login(&login) {
            Some("A login is 3 to 20 characters: a-z, 0-9 and _")
        } else if !valid_password(&password) {
            Some("A password is 8 to 128 characters")
        } else if password != again {
            Some("The two passwords differ")
        } else {
            None
        };
        if let Some(complaint) = complaint {
            self.tell(complaint.to_owned(), true);
            return;
        }
        self.prefs.login = login.clone();
        self.save_prefs();
        let msg = ClientMsg::Register {
            login,
            password: Secret(password),
            kind: AccountKind::Human,
            client: Default::default(),
        };
        self.sign_in(msg, Asked::Register);
    }

    fn logout(&mut self) {
        self.list_cache.clear();
        self.accepted.clear();
        self.lobby_ui = Default::default();
        self.catalogue.clear();
        self.moving = None;
        self.lobby_secret = None;
        // The session is forgotten here at once; the service is told when a link is open (one
        // is not opened for it: a session left there expires by itself).
        let live = self.link_ready();
        self.store.remove(Entry::Session);
        self.account = false;
        self.account_profile = None;
        self.board.disconnected();
        self.profiles.clear();
        self.avatar.backend(None, false);
        self.games.clear();
        self.lobby = None;
        self.outbox.clear();
        self.credentials = None;
        self.retry_at = None;
        self.link_lost = false;
        if live {
            self.send(ClientMsg::Logout, Asked::Logout);
        } else {
            self.drop_link();
        }
        self.go_form(Page::Login);
    }

    fn enter(&mut self, id: MatchId, watch: bool) {
        let handoff = self
            .games
            .iter()
            .find(|g| g.match_id == id)
            .and_then(|g| g.handoff.clone());
        match (handoff, watch) {
            (Some(handoff), false) => self.start(Launch::Seat(handoff)),
            (Some(handoff), true) => self.start(Launch::Watch(handoff)),
            (None, false) => self.ask(ClientMsg::MatchHandoff { match_id: id }, Asked::Rejoin(id)),
            (None, true) => self.ask(ClientMsg::MatchHandoff { match_id: id }, Asked::Watch(id)),
        }
    }

    /// The form with the keyboard: the password dialog's while it is up, else the page's.
    pub(super) fn keyboard(&mut self) -> &mut Form {
        if self.lobby_ui.dialog.is_some() {
            return &mut self.lobby_ui.dialog_form;
        }
        if let Some(prompt) = &mut self.join_prompt {
            return &mut prompt.form;
        }
        if let Some(editor) = &mut self.lobby_ui.editor {
            return &mut editor.form;
        }
        if self.lobby_ui.chat.focused {
            return &mut self.lobby_ui.chat.input;
        }
        &mut self.form
    }

    /// Joins a listed lobby: an open one at once, a locked one once its password is typed into
    /// the dialog.
    fn join_lobby(&mut self, lobby: LobbyId) {
        self.lobby_ui.uncertain_join = None;
        self.lobby_ui.selected = Some(lobby);
        match self.lobbies.iter().find(|l| l.id == lobby) {
            Some(listed) if listed.locked && Some(listed.owner) != self.my_id() => {
                self.ask_password(lobby, listed.name.clone())
            }
            _ => self.ask(
                ClientMsg::LobbyJoin {
                    lobby,
                    password: None,
                },
                Asked::Join(lobby),
            ),
        }
    }

    /// Opens the password dialog of a locked lobby.
    fn ask_password(&mut self, lobby: LobbyId, name: String) {
        self.lobby_ui.hide_join_password = true;
        self.lobby_ui.selected = Some(lobby);
        self.join_prompt = Some(JoinPrompt {
            lobby,
            name,
            error: None,
            form: Form::new(FormKind::LobbyPassword, &self.prefs),
        });
    }

    /// The password dialog's OK: the join with the password typed, which the field forgets.
    fn send_lobby_password(&mut self) {
        let Some(prompt) = &mut self.join_prompt else {
            return;
        };
        let password = prompt.form.take_secret(0);
        if password.is_empty() {
            prompt.error = Some("Type the lobby's password".to_owned());
            return;
        }
        prompt.error = None;
        let lobby = prompt.lobby;
        let password = Some(Secret(password));
        self.ask(ClientMsg::LobbyJoin { lobby, password }, Asked::Join(lobby));
    }

    fn cancel_lobby_password(&mut self) -> bool {
        let closed = self.join_prompt.take().is_some();
        if closed {
            self.lobby_ui.uncertain_join = None;
        }
        closed
    }

    fn refresh_lobbies(&mut self) {
        if let Some(lobby) = self.lobby_ui.uncertain_join.take()
            && self.page == Page::Lobbies
            && self.lobby_ui.selected == Some(lobby)
        {
            // An existing membership is returned before the server checks a password.
            self.ask(
                ClientMsg::LobbyJoin {
                    lobby,
                    password: None,
                },
                Asked::Join(lobby),
            );
        }
        self.search_lobbies();
    }

    /// Lists the open lobbies whose name or owner holds the search typed.
    fn search_lobbies(&mut self) {
        self.lobby_ui.search_due = None;
        let query = self.form.value(SEARCH_FIELD).trim().to_owned();
        self.ask(ClientMsg::LobbySearch { query }, Asked::Other);
    }

    fn lobby_ask(&mut self, msg: impl FnOnce(LobbyId) -> ClientMsg) {
        if let Some((state, _)) = &self.lobby {
            let msg = msg(state.id);
            self.ask(msg, Asked::Lobby);
        }
    }

    /// LEAVE, confirmed: the client is done with the game RECONNECT would go back to. The game is
    /// told the match is left for good, as LEAVE GAME tells it (`super::leave`: the seat taken
    /// once more out of sight, the surrender once admitted), within [`LEAVE_REACH`]; one that
    /// cannot be reached is forgotten untold. The local lobby's own game is told the same over
    /// loopback, and its server runs on for whoever plays. A service's match stays the
    /// service's: MY GAMES lists it while the service says it runs.
    fn leave_seat(&mut self) {
        let Some(left) = self.reconnect.take() else {
            return;
        };
        self.last_seat = None;
        self.status = None;
        let hosted = matches!(left, Launch::Direct { hosted: true, .. });
        if cfg!(target_arch = "wasm32") {
            // A browser opens no connection out of sight: the game is forgotten here.
            self.tell("The game is forgotten here".to_owned(), false);
            return;
        }
        let Some(game) = SeatOf::of(&left) else {
            return;
        };
        self.last_leave_id += 1;
        let until = Instant::now() + LEAVE_REACH;
        self.leaving = Some(Leaving {
            id: self.last_leave_id,
            until,
            game,
            hosted,
            in_picks: self.left_in_picks,
        });
        self.leave_order = Some((self.last_leave_id, left, until));
        self.tell("Leaving the game…".to_owned(), false);
    }

    /// A leave is over, `told` saying how. The local lobby's server is never stopped here: this
    /// client cannot know who plays on (a seat may have been taken since it last saw the lobby),
    /// so the server ends by itself once its match is over or empty (`--empty-after-secs`).
    fn leave_finished(&mut self, leaving: &Leaving, told: &str) {
        let told = match (leaving.hosted, leaving.in_picks) {
            // A seat left in the picks cancels the match, the local lobby's too.
            (_, true) => format!("{told}: the match is cancelled"),
            (true, false) => format!("{told}: the match goes on for whoever plays"),
            (false, false) => told.to_owned(),
        };
        self.tell(told, false);
    }

    /// The leave under way runs out of time now, as if its deadline had passed.
    #[cfg(test)]
    pub(super) fn leave_times_out(&mut self) {
        if let Some(leaving) = &mut self.leaving {
            leaving.until = Instant::now();
        }
    }

    /// A game by address: TCP (`host:port`, `tcp://`) or a WebSocket URL.
    /// Plain TCP to another machine carries everything in clear, so the
    /// player is told once per host before it goes.
    fn direct(&mut self, spectate: bool, anyway: bool) {
        let address = self.form.value(1).trim().to_owned();
        let name = self.name();
        let host = match super::address::check(&address) {
            Ok(host) => host,
            Err(why) => {
                self.tell(why.to_owned(), true);
                return;
            }
        };
        if let Some(host) = host {
            if anyway {
                self.prefs.allow_plain(&host);
            } else if !self.prefs.plain_ok.contains(&host) {
                self.plain_host = Some(host);
                return;
            }
        }
        self.plain_host = None;
        self.prefs.direct = address.clone();
        self.save_prefs();
        self.start(Launch::Direct {
            hosted: false,
            address,
            name,
            spectate,
            key: if spectate { None } else { Claim::draw() },
        });
    }

    /// A lobby on this machine, as Dota's local lobby: a game server started
    /// here for so many players, this player seated first and the others
    /// joining by this machine's address (the same page). Named by the
    /// profile; no account.
    fn host(&mut self, lan: bool) {
        let port = self
            .form
            .value(0)
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|p| *p >= 1024);
        let Some(port) = port else {
            self.tell("A port is 1024 to 65535".to_owned(), true);
            return;
        };
        self.prefs.host_port = port.to_string();
        self.save_prefs();
        let recording = self.recording_path("local");
        let replay = recording.clone().filter(|_| self.library.keeping());
        let metadata = recording
            .filter(|_| replay.is_none())
            .map(|path| path.with_extension("meta"));
        self.start(Launch::Host {
            port,
            lan,
            name: self.name(),
            key: Claim::draw(),
            cheats: self.prefs.host_cheats,
            replay,
            metadata,
            server: self.server_binary.clone(),
            server_environment: self.server_environment,
            rules: self.prefs.host_rules,
        });
    }
}

/// The search field of the LOBBIES page's form.
const SEARCH_FIELD: usize = 0;

/// What a refusal means to the player.
pub fn error_text(error: &Error) -> String {
    let secs = |ms: &u32| ms.div_ceil(1000);
    match error {
        Error::BadCredentials => "Wrong login or password".to_owned(),
        Error::LoginTaken => "That login is taken".to_owned(),
        Error::BadLogin => "A login is 3 to 20 characters: a-z, 0-9 and _".to_owned(),
        Error::WeakPassword => "A password is 8 to 128 characters".to_owned(),
        Error::BadSession => "Your login has expired: log in again".to_owned(),
        Error::TooManySessions => "This account is logged in in too many places".to_owned(),
        Error::Throttled { retry_after_ms } | Error::RateLimited { retry_after_ms } => {
            format!("Too many attempts: try again in {} s", secs(retry_after_ms))
        }
        Error::Banned { reason, .. } => {
            format!(
                "This account is banned: {}",
                text::line(reason, text::MESSAGE)
            )
        }
        Error::TooManyGames { .. } => "You are in a game already".into(),
        Error::TooManyGamesFromSource { .. } => "Matchmaking is full".into(),
        Error::QueueFull { retry_after_ms } => format!(
            "The queues are full right now: try again in {} s",
            secs(retry_after_ms)
        ),
        Error::QueueCooldown { retry_after_ms } => format!(
            "After a declined match you can search again in {}",
            clock(secs(retry_after_ms))
        ),
        Error::UnknownProposal => "This match offer is no longer available".to_owned(),
        Error::NotInQueue => "This search has already stopped".to_owned(),
        Error::ProposalExpired => "Too late: the match was not accepted in time".to_owned(),
        Error::GameVersionMismatch { .. } => {
            "The service's game servers run another version of the game".to_owned()
        }
        Error::ProtoMismatch { .. } => {
            "The service speaks another version of the protocol".to_owned()
        }
        Error::BadLobbyPassword => "Wrong lobby password".to_owned(),
        Error::LobbyNameTaken => "A lobby of that name is open already".to_owned(),
        Error::BadLobbyName => "A lobby name is 1 to 32 characters".to_owned(),
        Error::UnknownLobby => "That lobby is gone".to_owned(),
        Error::LobbyPasswordRequired => "This lobby has a password".to_owned(),
        Error::NoCheats => "This game mode has no cheats".to_owned(),
        Error::LobbyFull => "The lobby is full".to_owned(),
        Error::LobbyNotFull => "Waiting for more players".to_owned(),
        Error::NoSoloStart => "This service does not start a Demo Hero".to_owned(),
        Error::NotAllReady => "Waiting for the other players to be ready".to_owned(),
        Error::ServerStopping => "The service is shutting down".to_owned(),
        other => text::line(&format!("Refused: {other:?}"), text::MESSAGE),
    }
}

/// Seconds as `m:ss`.
pub fn clock(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The seats of a local lobby, whatever its rules: five a side, as the map's.
pub const LOCAL_SEATS: u8 = 10;

/// Whether a typed service address can be tried at all, or what to type
/// instead: an empty line or a game server's `host:port` would only fail
/// later with the connection's own words.
fn service_address(service: &str) -> Result<(), String> {
    let host = service
        .strip_prefix("wss://")
        .or_else(|| service.strip_prefix("ws://"));
    match host {
        _ if service.is_empty() => {
            Err("Type the service's address first: wss://host:port".to_owned())
        }
        Some(host) if !host.trim_end_matches('/').is_empty() => Ok(()),
        Some(_) => Err(format!("'{service}' names no host: wss://host:port")),
        None => Err(format!(
            "'{service}' is not a service address: it starts with wss:// (ws:// on your own network). A game server's address goes under LOCAL LOBBY"
        )),
    }
}

impl MenuCore {
    fn queue(&mut self, ruleset: RulesetId) {
        let mut audiences = vec![Audience::HumansOnly];
        if self.play.bots {
            audiences.push(Audience::Everyone);
        }
        self.ask(
            ClientMsg::QueueJoinAudiences { ruleset, audiences },
            Asked::Queue,
        );
    }

    fn play_action(&mut self, action: super::play::PlayAction) {
        use super::play::PlayAction;
        match action {
            PlayAction::Open(open) => {
                self.play.open = open;
                if open {
                    self.go(Page::Home);
                }
            }
            PlayAction::Section(section) => {
                if self.play.section != section {
                    self.play.section = section;
                    self.play.selected = None;
                }
            }
            PlayAction::Bots => {
                self.play.bots = !self.play.bots;
                self.prefs.allow_matchmaking_bots = self.play.bots;
                self.save_prefs();
            }
            PlayAction::Select(id) => self.select_queue(id),
            PlayAction::Find => self.find_selected(),
            PlayAction::LobbyLink(create) => {
                let online = self.logged_in();
                self.go(if online {
                    Page::Lobbies
                } else {
                    Page::LocalLobby
                });
                if online && create {
                    self.lobby_ui.server = super::lobby_controls::Server::Service;
                    self.lobby_action(super::LobbyAction::Create);
                } else {
                    self.form.focus(if !online && !create { 1 } else { 0 });
                }
            }
        }
    }

    fn select_queue(&mut self, id: RulesetId) {
        if self.client.is_none() {
            return;
        }
        let available: Vec<_> = self
            .menu_rulesets()
            .filter(|r| self.play.lists(r.rated) && r.rating_window.is_some())
            .map(|r| r.id.clone())
            .collect();
        if !available.contains(&id) {
            return;
        }
        let selected = self
            .play
            .selected
            .get_or_insert_with(|| available.into_iter().take(1).collect());
        if selected.contains(&id) {
            selected.retain(|r| *r != id);
        } else {
            selected.push(id);
        }
    }

    fn find_selected(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        let available: Vec<_> = self
            .menu_rulesets()
            .filter(|r| self.play.lists(r.rated) && r.rating_window.is_some())
            .collect();
        let chosen: Vec<_> = available
            .iter()
            .enumerate()
            .filter(|(at, r)| {
                self.play
                    .selected
                    .as_ref()
                    .map_or(*at == 0, |ids| ids.contains(&r.id))
            })
            .map(|(_, row)| *row)
            .collect();
        if chosen.is_empty() {
            self.tell("Select at least one queue.".to_owned(), false);
            return;
        }
        let in_flight = client.searches().groups().len()
            + client.searches().slot_waits().len()
            + self.games.len();
        let cooldown = client.searches().cooldown_left_ms();
        if cooldown > 0 {
            self.tell(
                format!("You can search again in {}", clock(cooldown.div_ceil(1000))),
                false,
            );
            return;
        }
        if !super::play::can_search(
            (in_flight, client.limits(None).map(|l| l.games_per_account)),
            chosen.len(),
            false,
        ) {
            self.tell(
                "Not enough game slots for the selected queues.".to_owned(),
                true,
            );
            return;
        }
        let ids: Vec<_> = chosen.iter().map(|r| r.id.clone()).collect();
        for ruleset in ids {
            self.queue(ruleset);
        }
        self.play.open = false;
    }
}

impl MenuCore {
    pub(crate) fn loading_details(&self) -> crate::loading::Details {
        self.loading.clone()
    }

    fn details_of(&self, launch: &Launch) -> crate::loading::Details {
        let handoff = match launch {
            Launch::Seat(h) | Launch::Watch(h) | Launch::Demo { handoff: h, .. } => h,
            _ => return crate::loading::Details::default(),
        };
        let name = self
            .menu_rulesets()
            .find(|r| r.id == handoff.ruleset)
            .map_or(&handoff.ruleset.0, |r| &r.name);
        crate::loading::Details {
            match_id: Some(handoff.match_id.0),
            mode: format!(
                "{} · {}",
                text::line(name, text::NAME),
                if handoff.rated { "Ranked" } else { "Unranked" }
            ),
            seats: handoff
                .seats
                .iter()
                .take(10)
                .map(|seat| crate::loading::Row {
                    slot: seat.slot,
                    team: if seat.team == 0 {
                        bota_proto::Team::Radiant
                    } else {
                        bota_proto::Team::Dire
                    },
                    name: text::line(&seat.nickname, text::NAME),
                    bot: seat.kind == AccountKind::Bot,
                    connected: false,
                    me: seat.slot == handoff.slot,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    #[test]
    fn a_matched_search_preserves_the_notice_without_reporting_cancellation() {
        let mut client = crate::e2e::harness::Client::run(&[]);
        client.frames(8);
        {
            let mut menu = client.world_mut().non_send_mut::<MenuCore>();
            menu.tell("Another game is ready".into(), false);
            menu.event(Event::QueueLeft {
                entry: QueueEntryId(1),
                reason: QueueLeftReason::Matched,
                cooldown_ms: 0,
            });
        }
        client.frames(4);
        let shown = client.shown_texts();
        assert!(shown.iter().any(|text| text == "Another game is ready"));
        assert!(!shown.iter().any(|text| text == "Search cancelled"));
    }

    #[test]
    fn queue_capacity_refusal_reopens_play_and_expires_at_the_retry_advice() {
        let mut player = super::super::tests::Player::new(None);
        player.menu.refused_request(
            &Error::QueueFull {
                retry_after_ms: 1_000,
            },
            Asked::Queue,
        );
        assert!(player.menu.view().queue_full);
        assert!(player.menu.play.open);
        assert!(player.menu.status.is_none());
        player.menu.queue_full_until = Some(Instant::now() - std::time::Duration::from_secs(1));
        assert!(!player.menu.view().queue_full);
    }
}

#[cfg(test)]
mod result_overlay_tests {
    use super::*;
    use crate::{e2e::harness::Client, postgame::PostGame};
    use bota_radovid::proto::{EndReason, Outcome, UnixMs};

    fn deliver(client: &mut Client, id: u64, team: u8) {
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .finished(MatchResult {
                match_id: MatchId(id),
                ruleset: RulesetId("solo_mid".into()),
                rated: false,
                outcome: Outcome::Winner { team },
                reason: EndReason::Decided,
                duration_ms: 1000,
                game_report: Vec::new(),
                seats: Vec::new(),
                finished_at: UnixMs(0),
            });
        client.frames(5);
    }
    #[test]
    fn an_unchanged_result_does_not_rebuild_its_recording_each_frame() {
        use crate::main_menu::{
            local_matches::{Saved, service_key},
            results::OUTCOME_PROJECTIONS,
        };
        use bota_replay::meta::{MetaRecord, Writer};
        let mut client = Client::run(&[]);
        client.frames(4);
        let mut meta =
            bota_replay::meta::decode(&crate::postgame::preview::metadata("match_details"))
                .expect("fixture");
        meta.header.match_id = 7;
        let mut writer = Writer::default();
        assert!(writer.push(&MetaRecord::Header(meta.header), false));
        for record in meta.records {
            if !matches!(record, MetaRecord::End { .. }) {
                assert!(writer.push(&record, false));
            }
        }
        let bytes = writer.into_bytes();
        {
            let mut core = client.world_mut().non_send_mut::<MenuCore>();
            core.local_history = Default::default();
            let key = service_key(&core.prefs.service, 7);
            core.local_history.insert(
                Saved {
                    key,
                    file: None,
                    when: 0,
                    mode: "solo_mid".into(),
                    owner: Some(bota_proto::SlotId(0)),
                    metadata: bytes,
                    metadata_key: None,
                    cached: None,
                    result: None,
                    summary: None,
                    temporary: None,
                },
                50,
            );
        }
        let before = OUTCOME_PROJECTIONS.with(std::cell::Cell::get);
        deliver(&mut client, 7, 0);
        client.frames(30);
        assert!(client.world().non_send::<PostGame>().visible());
        assert_eq!(
            OUTCOME_PROJECTIONS.with(std::cell::Cell::get) - before,
            1,
            "one projection for an unchanged result/archive version"
        );
        let mut complete =
            bota_replay::meta::decode(&crate::postgame::preview::metadata("match_details"))
                .expect("complete fixture");
        complete.header.match_id = 7;
        let mut writer = Writer::default();
        assert!(writer.push(&MetaRecord::Header(complete.header), false));
        for record in complete.records {
            assert!(writer.push(&record, true));
        }
        {
            let mut core = client.world_mut().non_send_mut::<MenuCore>();
            let key = service_key(&core.prefs.service, 7);
            let mut entry = core.local_history.find(&key).expect("saved match").clone();
            entry.metadata = writer.into_bytes();
            entry.summary = None;
            core.local_history.insert(entry, 50);
        }
        client.frames(5);
        assert_eq!(
            OUTCOME_PROJECTIONS.with(std::cell::Cell::get) - before,
            2,
            "changed metadata is projected once"
        );
        let world = client.world_mut();
        let rows = world
            .query::<&crate::postgame::render::SeatRow>()
            .iter(world)
            .count();
        assert_eq!(
            rows, 10,
            "completed details replace the private projection on screen"
        );
    }

    #[test]
    fn dismissing_a_background_result_does_not_dismiss_the_next_match() {
        let mut client = Client::run(&[]);
        client.frames(4);
        deliver(&mut client, 1, 0);
        assert!(client.world().non_send::<PostGame>().visible());
        client.click_named("results close");
        assert!(!client.world().non_send::<PostGame>().visible());
        deliver(&mut client, 2, 1);
        assert!(
            client.world().non_send::<PostGame>().visible(),
            "a new match needs its own dismissal"
        );
        assert!(
            client
                .shown_texts()
                .iter()
                .any(|text| text == "DIRE VICTORY")
        );
    }
    #[test]
    fn a_background_result_waits_for_the_inspected_history_match_to_close() {
        use crate::postgame::{
            summary::MatchSummary,
            view::{Context, View},
        };
        let mut client = Client::run(&[]);
        client.frames(4);
        deliver(&mut client, 1, 0);
        client.click_named("results close");
        let meta = bota_replay::meta::decode(&crate::postgame::preview::metadata("match_details"))
            .expect("fixture");
        let summary = std::sync::Arc::new(MatchSummary::from_meta(&meta).expect("summary"));
        client.world_mut().non_send_mut::<MenuCore>().result_request = Some(View::new(
            summary,
            Context {
                history: true,
                match_id: Some(99),
                ..Default::default()
            },
        ));
        client.frames(4);
        deliver(&mut client, 2, 1);
        assert!(
            client
                .shown_texts()
                .iter()
                .any(|text| text.contains("Match 99")),
            "the inspected recording stays on screen"
        );
        client.click_named("results close");
        client.frames(4);
        assert!(client.world().non_send::<PostGame>().visible());
        assert!(
            client
                .shown_texts()
                .iter()
                .any(|text| text == "DIRE VICTORY")
        );
    }
}

#[cfg(test)]
mod identity_alias_tests {
    use super::*;
    #[test]
    fn an_uncorrelated_public_reply_cannot_overwrite_the_signed_in_account() {
        let service = crate::main_menu::tests::service();
        let mut player = crate::main_menu::tests::Player::new(Some(&service));
        player.register("reply_viewer");
        let original = player.menu.account_profile.clone();
        let stranger = bota_radovid::proto::PublicAccount {
            id: AccountId(player.menu.my_id().expect("logged in").0 + 1000),
            nickname: "stranger".into(),
            kind: bota_radovid::proto::AccountKind::Human,
            online: false,
            bot_version: None,
            ratings: Vec::new(),
            avatar: None,
        };
        player
            .menu
            .take_reply(u32::MAX, Ok(Reply::Account { account: stranger }));
        assert_eq!(
            player.menu.account_profile, original,
            "unsolicited visitor data never becomes the own profile"
        );
    }
    #[test]
    fn a_canonical_service_alias_keeps_the_login_and_its_resumable_session() {
        let service = crate::main_menu::tests::service();
        let mut player = crate::main_menu::tests::Player::new(Some(&service));
        player.register("alias_viewer");
        let id = player.menu.my_id();
        player.menu.act(Act::Go(Page::Service));
        player.fill(&[&format!("{}/control/", service.url)]);
        player.menu.act(Act::Submit);
        assert!(
            player.menu.logged_in(),
            "an endpoint spelling change is not a service switch"
        );
        assert_eq!(player.menu.my_id(), id);
        assert!(
            player.menu.saved_session().is_some(),
            "the existing credential remains scoped to the same canonical service"
        );
    }
}

#[cfg(test)]
impl MenuCore {
    pub(crate) fn list_requests_finished_for_test(&self) -> bool {
        self.asked.is_empty() && self.outbox.is_empty()
    }
    pub(crate) fn list_clock_for_test(&mut self, now: Instant) {
        self.list_cache.now = Some(now);
    }
}
