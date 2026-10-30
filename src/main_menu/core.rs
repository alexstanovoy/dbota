//! The main menu's state and logic, apart from any drawing: what the player
//! asks for comes in as [`Act`]s, what the matchmaking service says comes in
//! through the control connection, and out go requests to the service and
//! the games to open ([`Launch`]). The screens draw [`MenuCore::view`].

use std::collections::VecDeque;

use bevy::platform::time::Instant;
use bota_radovid::proto::{
    AccountId, AccountKind, Audience, ClientMsg, Error, Event, Handoff, LadderRow, LobbyId,
    LobbyState, LobbySummary, MatchId, MatchResult, ProposalId, PublicAccount, QueueEntryId,
    QueueLeftReason, Reply, RulesetId, Secret, valid_login, valid_password,
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
    /// The account's rating per ruleset and the ladder.
    Profile,
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
}

/// What the player does in the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Go(Page),
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
    Find(RulesetId),
    Cancel(QueueEntryId),
    Accept(ProposalId),
    Decline(ProposalId),
    /// Takes one's seat in a running match again.
    Rejoin(MatchId),
    /// Watches one's own running match.
    Watch(MatchId),
    CreateLobby,
    /// Joins a listed lobby: at once with its password (in the form), else
    /// by asking its owner.
    AskLobby(LobbyId),
    ReadyLobby(bool),
    LeaveLobby,
    /// Plays (or with `true` watches) the game at the address in the form.
    Direct(bool),
    /// Opens a lobby on this machine, for this machine alone or (`true`)
    /// for the local network too, and takes a seat in it.
    Host(bool),
    /// Turns the local lobby's cheats on or off (Dota's "Enable cheats").
    ToggleCheats,
    /// Starts Dota's Demo Hero with a hero.
    #[cfg(not(target_arch = "wasm32"))]
    Demo(u16),
    /// Watches a kept replay, by its file name.
    OpenReplay(String),
    /// Deletes a kept replay.
    DeleteReplay(String),
    /// Saves a kept replay to a file of the player's own (a browser's download).
    #[cfg(target_arch = "wasm32")]
    DownloadReplay(String),
    /// Opens a replay file the player picks (a browser's file chooser).
    #[cfg(target_arch = "wasm32")]
    OpenReplayFile,
    /// Watches the replay of a finished match of the account's: fetched from the service once.
    WatchReplay(MatchId),
    /// Goes on with plain TCP to a host after the notice.
    PlainAnyway,
    /// Closes the notice line.
    Dismiss,
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
    /// What a local lobby's empty seats take.
    Fill(SeatFill),
    /// The game mode a new lobby is of.
    LobbyMode(RulesetId),
    /// In a lobby: sits in a slot (or none, looking on); the member being moved is put there.
    LobbySlot(u8),
    /// The owner picks a member to move with the next slot clicked.
    MoveMember(AccountId),
    KickMember(AccountId),
    /// The owner's settings of the lobby one sits in.
    ConfigureMode(RulesetId),
    ConfigureAudience(Audience),
    /// Sets the password typed (empty removes it).
    ConfigurePassword,
}

/// What a local lobby's seats other than one's own take, as Dota's lobby
/// leaves a slot open or puts a bot in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SeatFill {
    /// Left to whoever joins by the address.
    #[default]
    Open,
    /// The bot command of the settings, once for each seat.
    Bot,
}

impl SeatFill {
    pub const ALL: [SeatFill; 2] = [SeatFill::Open, SeatFill::Bot];

    /// Its name in the menu's remembered choices.
    pub fn name(self) -> &'static str {
        match self {
            SeatFill::Open => "open",
            SeatFill::Bot => "bot",
        }
    }

    pub fn named(name: &str) -> Option<SeatFill> {
        SeatFill::ALL.into_iter().find(|f| f.name() == name)
    }
}

/// How a local lobby's game server fills the seats left: as [`SeatFill`],
/// with the bot's command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fill {
    Open,
    Bot(String),
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
    },
    /// Start a game server on this machine (on the local network too when
    /// `lan`), and take a seat in it as a direct game.
    Host {
        port: u16,
        players: u8,
        lan: bool,
        name: String,
        key: Option<Claim>,
        /// Whether its host enabled cheats.
        cheats: bool,
        /// Where the server writes the replay.
        replay: Option<std::path::PathBuf>,
        /// The server binary, when not looked for (`$BOTA_SERVER`, beside
        /// this client, the `PATH`).
        server: Option<std::path::PathBuf>,
        /// What the seats other than one's own take.
        fill: Fill,
    },
    /// Dota's Demo Hero: a game server started on this machine alone with
    /// cheats on, seating this player alone with the hero chosen.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // only the desktop starts one
    Demo {
        hero: bota_proto::HeroId,
        name: String,
        key: Option<Claim>,
        /// The server binary, when not looked for.
        server: Option<std::path::PathBuf>,
        /// Where the server writes the replay.
        replay: Option<std::path::PathBuf>,
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
            Launch::Seat(_) | Launch::Given | Launch::Host { .. } | Launch::Demo { .. } => {
                GameKind::Play
            }
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

/// The line under the top bar: news or a refusal.
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

/// What a request was, for its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Asked {
    Login,
    Register,
    Resume,
    Logout,
    Queue,
    Accept,
    Rejoin(MatchId),
    Watch(MatchId),
    Lobby,
    History,
    Profile,
    Ladder,
    Artifact(MatchId),
    Other,
}

/// How long the connection stays open after a seat left the game, for the message to go out.
const LEAVE_GAME_FLUSH: std::time::Duration = std::time::Duration::from_millis(500);
/// Finished matches listed under MY GAMES.
const HISTORY_LEN: u8 = 20;
/// Ladder rows shown on the profile.
const LADDER_LEN: u8 = 10;

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
    /// Dota's Demo Hero.
    Demo,
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
    pub(super) page: Page,
    pub(super) form: Form,
    pub(super) status: Option<Status>,
    asked: Vec<(u32, Asked)>,
    pub(super) lobbies: Vec<LobbySummary>,
    /// The ruleset a new lobby is made for; the service's first until one is chosen.
    lobby_mode: Option<RulesetId>,
    /// The member the owner is about to move.
    pub(super) moving: Option<AccountId>,
    /// The password the owner set, sent again with each change of the settings (the service never
    /// tells it back).
    lobby_secret: Option<String>,
    /// The lobby sat in, and when it was last told.
    pub(super) lobby: Option<(LobbyState, Instant)>,
    pub(super) games: Vec<Game>,
    pub(super) results: VecDeque<MatchResult>,
    /// The result the Result page shows.
    pub(super) shown: Option<MatchId>,
    on_screen: Option<OnScreen>,
    launch: Option<Launch>,
    pub(super) launching: bool,
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
    /// The seat left, to take again.
    pub(super) reconnect: Option<Launch>,
    /// The seat of the game on screen, for [`Self::reconnect`] once left.
    last_seat: Option<Launch>,
    /// The player asked to quit: the shell ends the client.
    quit: bool,
    /// The player left the game: the shell lets it go.
    leave: bool,
    /// The player confirmed leaving the match: the shell tells the server, then leaves.
    leave_game: bool,
    /// When the connection may be closed after the leave went out.
    leave_at: Option<Instant>,
    /// The name this player goes by where there is no account: the
    /// settings' profile name.
    profile: String,
    /// The replays this client keeps.
    pub(super) library: Library,
    /// What a kept replay holds, as read, where the library knows no more.
    pub(super) shelf: super::replay_info::Shelf,
    /// The fetching of the replays of the player's matches.
    pub(super) keeper: Keeper,
    /// The account's latest finished matches, from the service.
    pub(super) history: Vec<MatchResult>,
    pub(super) account_profile: Option<PublicAccount>,
    pub(super) ladder: Vec<LadderRow>,
    /// The game server a local lobby starts, when not looked for.
    pub(super) server_binary: Option<std::path::PathBuf>,
    /// The settings' bot command, which a local lobby's bot seats run.
    bot_command: String,
    /// The address others type to join the local lobby this client hosts.
    hosted_at: Option<String>,
    /// The hero of the last Demo Hero started.
    demo: Option<bota_proto::HeroId>,
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
            .filter(|s| s.service == self.prefs.service)
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
        let mut core = MenuCore {
            store,
            form: Form::new(FormKind::Service, &prefs),
            prefs,
            tls,
            client: None,
            conn: Conn::Offline,
            account: false,
            credentials: None,
            outbox: Vec::new(),
            idle_since: None,
            retry_at: None,
            retry: true,
            page: Page::Service,
            settings_from: Page::Service,
            status: None,
            asked: Vec::new(),
            lobbies: Vec::new(),
            lobby_mode: None,
            moving: None,
            lobby_secret: None,
            lobby: None,
            games: Vec::new(),
            results: VecDeque::new(),
            shown: None,
            on_screen: None,
            launch: None,
            launching: false,
            plain_host: None,
            kind: GameKind::Play,
            confirm: None,
            game_unsafe: false,
            reconnect: None,
            last_seat: None,
            quit: false,
            leave: false,
            leave_game: false,
            leave_at: None,
            profile: String::new(),
            library,
            shelf: Default::default(),
            keeper: Keeper::default(),
            history: Vec::new(),
            account_profile: None,
            ladder: Vec::new(),
            server_binary: None,
            bot_command: String::new(),
            hosted_at: None,
            demo: None,
            tried_service: false,
        };
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

    /// The settings' bot command, which a local lobby runs for its bot seats.
    pub fn set_bot_command(&mut self, command: &str) {
        if self.bot_command != command {
            command.clone_into(&mut self.bot_command);
        }
    }

    /// The address others type under LOCAL LOBBY to join the local
    /// lobby this client hosts, while it is the game on screen.
    pub fn hosted_at(&self) -> Option<&str> {
        self.hosted_at
            .as_deref()
            .filter(|_| self.on_screen == Some(OnScreen::Local))
    }

    /// What a local lobby's empty seats take, and whether a bot command is
    /// set for them to take a bot.
    pub(super) fn fill_choice(&self) -> (SeatFill, bool) {
        (self.prefs.host_fill, !self.bot_command.trim().is_empty())
    }

    /// The settings' profile name, which local and direct games announce.
    pub fn set_profile_name(&mut self, name: &str) {
        if self.profile != name {
            name.clone_into(&mut self.profile);
        }
    }

    /// The name announced where there is no account.
    fn name(&self) -> String {
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

    /// Whether the control link to the service is open.
    #[cfg(test)]
    pub(super) fn link_open(&self) -> bool {
        self.client.as_ref().is_some_and(|c| !c.hung_up())
    }

    /// The button of the page's form that has the keyboard, if one has.
    pub fn button_focus(&self) -> Option<&Act> {
        self.form.focused_button()
    }

    /// Whether the page has a text field the player may type into (an input method is then
    /// wanted).
    pub fn wants_text(&self) -> bool {
        !matches!(self.page, Page::Game | Page::Settings) && !self.form.fields().is_empty()
    }

    /// The player confirmed quitting, once.
    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit)
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
        self.demo.filter(|_| self.on_screen == Some(OnScreen::Demo))
    }

    /// Whether the game on screen is a replay opened from the menu.
    pub fn replay_from_menu(&self) -> bool {
        self.on_screen == Some(OnScreen::Replay)
    }

    /// Whether leaving the game on screen now may ruin it for the others.
    pub fn set_game_unsafe(&mut self, game_unsafe: bool) {
        self.game_unsafe = game_unsafe;
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

    /// Whether a link is being opened: the Hello or the sign-in is not yet answered.
    pub(super) fn connecting(&self) -> bool {
        matches!(self.conn, Conn::Connecting | Conn::Welcomed)
    }

    /// The game the shell is to open next, once.
    pub fn take_launch(&mut self) -> Option<Launch> {
        self.launch.take()
    }

    /// The shell has the game on screen.
    pub fn game_started(&mut self) {
        self.launching = false;
    }

    /// The shell could not open the game.
    pub fn launch_failed(&mut self, why: &str) {
        self.launching = false;
        self.reconnect = self.last_seat.take();
        let left = self.on_screen.take();
        self.page = self.rest_page(left);
        self.form = Form::new(self.form_kind(), &self.prefs);
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
        self.reconnect = self.last_seat.take().filter(|_| !over);
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
        self.form = Form::new(self.form_kind(), &self.prefs);
    }

    /// The page to fall back to: the page a game left was opened from
    /// (Direct, the local lobby, the replays), else Play once logged in,
    /// else the way in.
    fn rest_page(&self, left: Option<OnScreen>) -> Page {
        match left {
            #[cfg(not(target_arch = "wasm32"))]
            Some(OnScreen::Direct | OnScreen::Local) => Page::LocalLobby,
            #[cfg(not(target_arch = "wasm32"))]
            Some(OnScreen::Demo) => Page::DemoHero,
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
            Page::Lobby => FormKind::Room,
            Page::LocalLobby => FormKind::Host,
            Page::Replays => FormKind::Replay,
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
        if self.link_ready() {
            self.send(msg, asked);
        } else if !self.account {
            self.tell("Sign in first".to_owned(), true);
        } else {
            if self.outbox.len() < MAX_OUTBOX {
                self.outbox.push((msg, asked));
            }
            if self.conn == Conn::Offline {
                self.connect();
            }
        }
    }

    /// Sends a login or registration: the link opens for it now, the typed values stay in the
    /// form, and nothing is held open before.
    fn sign_in(&mut self, msg: ClientMsg, asked: Asked) {
        self.status = None;
        self.credentials = Some((msg, asked));
        if self.conn == Conn::Offline {
            self.connect();
        }
    }

    fn send(&mut self, msg: ClientMsg, asked: Asked) {
        let Some(client) = &mut self.client else {
            self.tell("Not connected to the service".to_owned(), true);
            return;
        };
        match client.request(msg) {
            Ok(seq) => {
                if self.asked.len() >= MAX_ASKED {
                    self.asked.remove(0);
                }
                self.asked.push((seq, asked));
            }
            Err(err) => self.lost(err),
        }
    }

    /// Closes the link, keeping what the view shows of a signed-in account.
    fn drop_link(&mut self) {
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
    fn idle(&self) -> bool {
        let quiet = self.client.as_ref().is_some_and(|c| {
            c.searches().entries().is_empty() && c.searches().slot_waits().is_empty()
        });
        quiet
            && self.asked.is_empty()
            && self.outbox.is_empty()
            && self.credentials.is_none()
            && self.lobby.is_none()
            && !self.keeper.fetching()
            && !self.launching
            && !matches!(self.on_screen, Some(OnScreen::Match(_)))
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

    /// Takes in what the service said since the last frame.
    pub fn update(&mut self) {
        self.shelf.take();
        self.keep_update();
        if self.page == Page::Replays {
            self.read_summaries();
        }
        self.keep_link();
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

    fn lost(&mut self, err: ClientError) {
        let was_signed_in = self.conn == Conn::LoggedIn;
        self.keep_link_lost();
        let live = was_signed_in && !self.idle();
        self.credentials = None;
        self.outbox.clear();
        self.lobby = None;
        self.drop_link();
        if !self.account && !matches!(self.page, Page::Game | Page::LocalLobby) {
            // Not signed in: the pages of the account have nothing to show.
            if matches!(
                self.page,
                Page::Home | Page::MyGames | Page::Lobbies | Page::Lobby | Page::Profile
            ) {
                let rest = self.rest_page(None);
                self.go_form(rest);
            }
        }
        // Something live on the link is picked up again at once, once; the rest waits for the
        // next request.
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
        self.tell(format!("{what}: {why}"), true);
    }

    fn take(&mut self, incoming: Incoming) {
        match incoming {
            Incoming::Reply { seq, result } => {
                let asked = self
                    .asked
                    .iter()
                    .position(|(s, _)| *s == seq)
                    .map(|at| self.asked.swap_remove(at).1);
                match (result, asked) {
                    (_, Some(Asked::Logout)) => {
                        self.drop_link();
                        // A sign-in typed while the old session was being ended goes now.
                        if self.credentials.is_some() {
                            self.connect();
                        }
                    }
                    (Ok(Reply::Welcome { .. }), _) => self.welcomed(),
                    (Ok(reply), asked) => self.replied(reply, asked.unwrap_or(Asked::Other)),
                    (Err(error), asked) => self.refused(&error, asked.unwrap_or(Asked::Other)),
                }
            }
            Incoming::Event(event) => self.event(event),
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
        match reply {
            Reply::LoggedIn {
                session,
                activities,
                ..
            } => {
                self.conn = Conn::LoggedIn;
                let first = !self.account;
                self.account = true;
                self.retry = true;
                let saved = Saved {
                    service: self.prefs.service.clone(),
                    token: session,
                };
                if let Err(err) = self.store.write(Entry::Session, &saved.render()) {
                    bevy::log::warn!("login not kept for the next run: {err}");
                }
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
                for (msg, asked) in std::mem::take(&mut self.outbox) {
                    self.send(msg, asked);
                }
                if first {
                    self.status = None;
                    if !matches!(self.page, Page::Game) {
                        self.go(Page::Home);
                    }
                }
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
            Reply::Matches { list } => {
                self.history = list.into_iter().take(usize::from(HISTORY_LEN)).collect();
            }
            Reply::Account { account } => self.account_profile = Some(account),
            Reply::LadderPage { rows, .. } => {
                self.ladder = rows.into_iter().take(usize::from(LADDER_LEN)).collect();
            }
            Reply::Artifact {
                match_id,
                offset,
                total,
                data,
            } => self.keep_chunk(match_id, offset, total, &data),
            Reply::Lobbies { list } => self.lobbies = list,
            Reply::Lobby { state } => self.sit(state),
            Reply::Queued { .. } => self.status = None,
            _ => {}
        }
    }

    fn refused(&mut self, error: &Error, asked: Asked) {
        match (asked, error) {
            (Asked::Resume, _) => {
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
            (Asked::Lobby, Error::NotInLobby { .. }) => {
                self.lobby = None;
                self.go(Page::Lobbies);
            }
            _ => self.tell(error_text(error), true),
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::MatchFound { handoff } => {
                self.note_game(handoff.clone());
                if self.on_screen.is_none() && !self.launching {
                    self.start(Launch::Seat(handoff));
                } else {
                    self.tell(
                        "Another game of yours is ready: join it from MY GAMES".to_owned(),
                        false,
                    );
                }
            }
            Event::MatchResult { result } => self.finished(result),
            Event::LobbyState { state } | Event::LobbyJoined { state } => self.sit(state),
            Event::LobbyStarting { lobby, in_ms } => {
                if let Some((state, at)) = &mut self.lobby
                    && state.id == lobby
                {
                    state.starting_in_ms = Some(in_ms);
                    *at = Instant::now();
                }
            }
            Event::LobbyDenied { lobby } => self.unseat(lobby, "The lobby's owner said no"),
            Event::LobbyKicked { lobby } => self.unseat(lobby, "You were removed from the lobby"),
            Event::LobbyClosed { lobby } => self.unseat(lobby, "The lobby was closed"),
            Event::ProposalFailed {
                back_in_queue: true,
                ..
            } => self.tell(
                "Someone did not accept: you are back in the queue at your place".to_owned(),
                false,
            ),
            Event::QueueLeft { reason, .. } if reason != QueueLeftReason::Cancelled => {
                let why = match reason {
                    QueueLeftReason::TimedOut => "The match was not accepted in time",
                    _ => "You declined the match",
                };
                self.tell(format!("{why}: the search stopped"), true);
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
        self.keep_finished(&result);
        if self.results.len() >= MAX_RESULTS {
            self.results.pop_front();
        }
        self.results.push_back(result);
        if self.on_screen.is_none() && !self.launching {
            self.shown = Some(id);
            self.page = Page::Result;
        }
    }

    fn my_id(&self) -> Option<AccountId> {
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

    /// The account's ratings, and the ladder of the first ruleset.
    fn ask_profile(&mut self) {
        let Some(account) = self.my_id() else {
            return;
        };
        self.ask(ClientMsg::Account { account }, Asked::Profile);
        let ruleset = self
            .client
            .as_ref()
            .and_then(|c| c.rulesets().find(|r| r.rated).map(|r| r.id.clone()));
        if let Some(ruleset) = ruleset {
            let msg = ClientMsg::Ladder {
                ruleset,
                offset: 0,
                limit: LADDER_LEN,
            };
            self.ask(msg, Asked::Ladder);
        }
    }

    fn sit(&mut self, state: LobbyState) {
        self.lobby = Some((state, Instant::now()));
        if self.page == Page::Lobbies {
            self.go_form(Page::Lobby);
        }
    }

    fn unseat(&mut self, lobby: LobbyId, why: &str) {
        if self.lobby.as_ref().is_some_and(|(s, _)| s.id == lobby) {
            self.lobby = None;
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
        if self.on_screen.is_some() {
            self.tell("Disconnect from the game you are in first".to_owned(), true);
            return;
        }
        self.on_screen = Some(match &launch {
            Launch::Seat(h) | Launch::Watch(h) => OnScreen::Match(h.match_id),
            Launch::Direct { .. } => OnScreen::Direct,
            Launch::Host { .. } => OnScreen::Local,
            Launch::Demo { .. } => OnScreen::Demo,
            Launch::Replay(_) | Launch::Blob(_) => OnScreen::Replay,
            Launch::Given => OnScreen::Given,
        });
        self.keep_start(&launch);
        self.hosted_at = match &launch {
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
        self.last_seat = match &launch {
            Launch::Host {
                port, name, key, ..
            } => Some(Launch::Direct {
                address: format!("127.0.0.1:{port}"),
                name: name.clone(),
                spectate: false,
                key: *key,
            }),
            // Demo Hero's server ends with it: there is no seat to take again.
            Launch::Demo { .. } => None,
            launch => (self.kind == GameKind::Play).then(|| launch.clone()),
        };
        self.demo = match &launch {
            Launch::Demo { hero, .. } => Some(*hero),
            _ => None,
        };
        self.reconnect = None;
        self.confirm = None;
        self.launch = Some(launch);
        self.launching = true;
        self.page = Page::Game;
        self.status = None;
    }

    /// Moves to `page` with its form fresh.
    fn go_form(&mut self, page: Page) {
        self.page = page;
        self.form = Form::new(self.form_kind(), &self.prefs);
    }

    fn go(&mut self, page: Page) {
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
        let needs_login = matches!(
            page,
            Page::Home | Page::MyGames | Page::Lobbies | Page::Lobby | Page::Profile
        );
        if needs_login && !self.logged_in() {
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
            Page::Profile => self.ask_profile(),
            Page::Lobbies => self.ask(ClientMsg::LobbyList, Asked::Other),
            Page::Replays => self.read_summaries(),
            _ => {}
        }
    }

    /// Acts on what the player did.
    pub fn act(&mut self, act: Act) {
        match act {
            Act::Go(page) => self.go(page),
            Act::Focus(at) => self.form.focus(at),
            Act::Type(typed) => self.form.type_in(&typed),
            Act::Erase => self.form.erase(),
            Act::NextField => self.form.next(),
            Act::PreviousField => self.form.previous(),
            Act::Submit => self.press(),
            Act::Logout => self.logout(),
            Act::Find(ruleset) => self.ask(ClientMsg::QueueJoin { ruleset }, Asked::Queue),
            Act::Cancel(entry) => self.ask(ClientMsg::QueueLeave { entry }, Asked::Other),
            Act::Accept(proposal) => self.ask(ClientMsg::MatchAccept { proposal }, Asked::Accept),
            Act::Decline(proposal) => self.ask(ClientMsg::MatchDecline { proposal }, Asked::Other),
            Act::Rejoin(id) => self.enter(id, false),
            Act::Watch(id) => self.enter(id, true),
            Act::CreateLobby => self.create_lobby(),
            Act::AskLobby(lobby) => {
                let password = self.form.take_secret(1);
                let password = (!password.is_empty()).then_some(Secret(password));
                self.ask(ClientMsg::LobbyAsk { lobby, password }, Asked::Lobby);
            }
            Act::ReadyLobby(ready) => {
                self.lobby_ask(|lobby| ClientMsg::LobbyReady { lobby, ready })
            }
            Act::LeaveLobby => self.lobby_ask(|lobby| ClientMsg::LobbyLeave { lobby }),
            Act::Direct(spectate) => self.direct(spectate, false),
            Act::Host(lan) => self.host(lan),
            Act::LobbyMode(id) => self.lobby_mode = Some(id),
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
            Act::ConfigureMode(ruleset) => self.configure(Some(ruleset), None, None),
            Act::ConfigureAudience(audience) => self.configure(None, Some(audience), None),
            Act::ConfigurePassword => {
                let typed = self.form.take_secret(0);
                self.configure(None, None, Some((!typed.is_empty()).then_some(typed)));
            }
            Act::Fill(fill) => {
                self.prefs.host_fill = fill;
                self.save_prefs();
            }
            Act::ToggleCheats => {
                self.prefs.host_cheats = !self.prefs.host_cheats;
                self.save_prefs();
            }
            #[cfg(not(target_arch = "wasm32"))]
            Act::Demo(hero) => {
                let hero = bota_proto::HeroId(hero);
                if bota_core::hero_def(hero).is_some() {
                    self.start(Launch::Demo {
                        hero,
                        name: self.name(),
                        key: Claim::draw(),
                        server: self.server_binary.clone(),
                        replay: self.recording_path("demo"),
                    });
                }
            }
            Act::OpenReplay(name) => self.open_listed(&name),
            Act::DeleteReplay(name) => self.library.delete(&name),
            #[cfg(target_arch = "wasm32")]
            Act::DownloadReplay(name) => self.library.export(&name),
            #[cfg(target_arch = "wasm32")]
            Act::OpenReplayFile => self.library.pick_file(),
            Act::WatchReplay(match_id) => self.watch_replay(match_id),
            Act::PlainAnyway => self.direct(false, true),
            Act::Dismiss => self.status = None,
            Act::Quit if !cfg!(target_arch = "wasm32") => self.confirm = Some(Confirm::Quit),
            Act::Quit => {}
            Act::Answer(yes) => match self.confirm.take() {
                Some(Confirm::Quit) if yes => self.quit = true,
                Some(Confirm::Leave) if yes => self.leave = true,
                Some(Confirm::LeaveGame) if yes => self.leave_game = true,
                _ => {}
            },
            Act::Back => self.back(),
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
        }
    }

    /// Escape over the menu, as Dota's: the question first, then the page
    /// back to the top one, then the game under the dashboard, and on the
    /// top page with nothing under it, whether to quit.
    fn back(&mut self) {
        if self.confirm.take().is_some() || self.plain_host.take().is_some() {
            return;
        }
        let top = self.rest_page(None);
        match self.page {
            // The game and the settings take their own Escape; a game being
            // opened is waited for.
            Page::Game | Page::Settings => {}
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
    fn press(&mut self) {
        match self.form.focused_button().cloned() {
            Some(Act::Host(_)) if self.hosted_at.is_some() => {}
            Some(Act::Submit) | None => self.submit(),
            Some(button) => self.act(button),
        }
    }

    fn submit(&mut self) {
        match self.page {
            Page::Service => {
                let service = self.form.value(0).trim().to_owned();
                if let Err(why) = service_address(&service) {
                    self.tell(why, true);
                    return;
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
            #[cfg(not(target_arch = "wasm32"))]
            Page::Replays => {
                let path = self.form.value(0).trim().to_owned();
                if path.is_empty() {
                    self.tell("Type the path of a replay file".to_owned(), true);
                } else {
                    self.start(Launch::Replay(path.into()));
                }
            }
            Page::Lobbies => self.join_lobby(self.form.value(0).trim().to_owned()),
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
        // The session is forgotten here at once; the service is told when a link is open (one
        // is not opened for it: a session left there expires by itself).
        let live = self.link_ready();
        self.store.remove(Entry::Session);
        self.account = false;
        self.games.clear();
        self.lobby = None;
        self.outbox.clear();
        self.credentials = None;
        self.retry_at = None;
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

    fn create_lobby(&mut self) {
        let name = self.form.value(0).trim().to_owned();
        let password = self.form.take_secret(1);
        let Some(ruleset) = self.client.as_ref().and_then(|c| self.lobby_mode(c)) else {
            self.tell(
                "The service offers no game this client can play".to_owned(),
                true,
            );
            return;
        };
        self.lobby_secret = (!password.is_empty()).then(|| password.clone());
        let msg = ClientMsg::LobbyCreate {
            ruleset,
            name,
            audience: Audience::Everyone,
            password: (!password.is_empty()).then_some(Secret(password)),
        };
        self.ask(msg, Asked::Lobby);
    }

    /// The ruleset a new lobby is made for: the one chosen if the service still offers it, else
    /// the first it offers.
    pub(super) fn lobby_mode(&self, client: &Client) -> Option<RulesetId> {
        client
            .rulesets()
            .map(|r| &r.id)
            .find(|id| Some(*id) == self.lobby_mode.as_ref())
            .or_else(|| client.rulesets().next().map(|r| &r.id))
            .cloned()
    }

    /// The owner's change of the lobby's settings: what is not named stays as it is, the password
    /// being the one the owner set (the service never tells it back).
    fn configure(
        &mut self,
        ruleset: Option<RulesetId>,
        audience: Option<Audience>,
        password: Option<Option<String>>,
    ) {
        let Some((state, _)) = &self.lobby else {
            return;
        };
        let (ruleset, audience) = (
            ruleset.unwrap_or_else(|| state.ruleset.clone()),
            audience.unwrap_or(state.audience),
        );
        if let Some(password) = password {
            self.lobby_secret = password;
        }
        let password = self.lobby_secret.clone().map(Secret);
        self.lobby_ask(|lobby| ClientMsg::LobbyConfigure {
            lobby,
            ruleset,
            audience,
            password,
        });
    }

    fn join_lobby(&mut self, name: String) {
        let password = self.form.take_secret(1);
        let password = (!password.is_empty()).then_some(Secret(password));
        self.ask(ClientMsg::LobbyJoinByName { name, password }, Asked::Lobby);
    }

    fn lobby_ask(&mut self, msg: impl FnOnce(LobbyId) -> ClientMsg) {
        if let Some((state, _)) = &self.lobby {
            let msg = msg(state.id);
            self.ask(msg, Asked::Lobby);
        }
    }

    /// A game by address: TCP (`host:port`, `tcp://`) or a WebSocket URL.
    /// Plain TCP to another machine carries everything in clear, so the
    /// player is told once per host before it goes.
    fn direct(&mut self, spectate: bool, anyway: bool) {
        let address = self.form.value(2).trim().to_owned();
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
        let players = self
            .form
            .value(1)
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|n| (1..=MAX_LOCAL_PLAYERS).contains(n));
        let (Some(port), Some(players)) = (port, players) else {
            self.tell(
                format!("A port is 1024 to 65535, and players 1 to {MAX_LOCAL_PLAYERS}"),
                true,
            );
            return;
        };
        let fill = match self.prefs.host_fill {
            SeatFill::Open => Fill::Open,
            SeatFill::Bot if self.bot_command.trim().is_empty() => {
                self.tell(
                    "No bot command is set: SETTINGS, Options, LOCAL LOBBY".to_owned(),
                    true,
                );
                return;
            }
            SeatFill::Bot => Fill::Bot(self.bot_command.trim().to_owned()),
        };
        self.prefs.host_port = port.to_string();
        self.prefs.host_players = players.to_string();
        self.save_prefs();
        let replay = self.recording_path("local");
        self.start(Launch::Host {
            port,
            players,
            lan,
            name: self.name(),
            key: Claim::draw(),
            cheats: self.prefs.host_cheats,
            replay,
            server: self.server_binary.clone(),
            fill,
        });
    }
}

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
        Error::TooManyGames { limit } => {
            format!("You already have {limit} games going: that is the limit")
        }
        Error::TooManyGamesFromSource { limit } => {
            format!("Your network already has {limit} games going: that is the limit")
        }
        Error::QueueFull { retry_after_ms } => format!(
            "The queues are full right now: try again in {} s",
            secs(retry_after_ms)
        ),
        Error::QueueCooldown { retry_after_ms } => format!(
            "After a declined match you can search again in {}",
            clock(secs(retry_after_ms))
        ),
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
        Error::UnknownLobby => "No open lobby has that name".to_owned(),
        Error::LobbyFull => "The lobby is full".to_owned(),
        Error::LobbyNotFull => "Waiting for more players".to_owned(),
        Error::NotAllReady => "Waiting for the other players to be ready".to_owned(),
        Error::ServerStopping => "The service is shutting down".to_owned(),
        other => text::line(&format!("Refused: {other:?}"), text::MESSAGE),
    }
}

/// Seconds as `m:ss`.
pub fn clock(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Seats a local lobby has at most: the map's ten.
pub const MAX_LOCAL_PLAYERS: u8 = 10;

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
