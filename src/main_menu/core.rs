//! The main menu's state and logic, apart from any drawing: what the player
//! asks for comes in as [`Act`]s, what the matchmaking service says comes in
//! through the control connection, and out go requests to the service and
//! the games to open ([`Launch`]). The screens draw [`MenuCore::view`].

use std::collections::VecDeque;

use bevy::platform::time::Instant;
use bota_radovid::proto::{
    AccountKind, Audience, ClientMsg, Error, Event, Handoff, LobbyId, LobbyState, LobbySummary,
    MatchId, MatchResult, ProposalId, QueueEntryId, QueueLeftReason, Reply, RulesetId, Secret,
    valid_login, valid_password,
};
use bota_radovid::{Client, ClientError, Incoming, Tls};

use super::form::{Form, FormKind};
use super::prefs::{Prefs, Saved};
use super::store::{Entry, SettingsStore};
use super::text;

/// Messages from the service taken in one frame at most.
const MAX_PER_FRAME: usize = 64;
/// Requests whose answers the menu waits for, at most.
const MAX_ASKED: usize = 64;
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
    /// A game by address, without the service.
    Direct,
    /// A lobby on this machine, others joining by its address.
    LocalLobby,
    /// The replays on this machine.
    Replays,
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
    NextField,
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
    /// Watches a listed replay.
    OpenReplay(usize),
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
    /// Takes the seat of the game left again.
    Reconnect,
    /// What a local lobby's empty seats take.
    Fill(SeatFill),
}

/// What a local lobby's seats other than one's own take, as Dota's lobby
/// leaves a slot open or puts a bot in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SeatFill {
    /// Left to whoever joins by the address.
    #[default]
    Open,
    /// The game server's idle dummy (its last seat): a hero that never acts,
    /// to practise or test alone.
    Dummy,
    /// The bot command of the settings, once for each seat.
    Bot,
}

impl SeatFill {
    pub const ALL: [SeatFill; 3] = [SeatFill::Open, SeatFill::Dummy, SeatFill::Bot];

    /// Its name in the menu's remembered choices.
    pub fn name(self) -> &'static str {
        match self {
            SeatFill::Open => "open",
            SeatFill::Dummy => "dummy",
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
    Dummy,
    Bot(String),
}

/// What the menu asks before it goes on, as Dota's popups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Quit the client.
    Quit,
    /// Stop watching the game or replay under the dashboard.
    Leave,
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
        /// Where the server writes the replay.
        replay: Option<std::path::PathBuf>,
        /// The server binary, when not looked for (`$BOTA_SERVER`, beside
        /// this client, the `PATH`).
        server: Option<std::path::PathBuf>,
        /// What the seats other than one's own take.
        fill: Fill,
    },
    /// Watch a replay file.
    Replay(std::path::PathBuf),
    /// The game the command line named, again.
    Given,
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
            Launch::Seat(_) | Launch::Given | Launch::Host { .. } => GameKind::Play,
            Launch::Watch(_) => GameKind::Watch,
            Launch::Replay(_) => GameKind::Replay,
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
enum Asked {
    Login,
    Register,
    Resume,
    Queue,
    Accept,
    Rejoin(MatchId),
    Watch(MatchId),
    Lobby,
    Other,
}

/// An unfinished match of the account.
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub match_id: MatchId,
    /// Once the game server is up.
    pub handoff: Option<Handoff>,
}

/// What is on screen instead of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OnScreen {
    Match(MatchId),
    Direct,
    /// A game of a lobby on this machine.
    Local,
    Replay,
    /// The game the command line named.
    Given,
}

/// A replay file on this machine, as listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayFile {
    pub path: std::path::PathBuf,
    /// Its file name, safe to draw.
    pub name: String,
    /// Its size, `12.3 MB`.
    pub size: String,
    /// When it was written, in this machine's time.
    pub when: String,
    /// Its size and time, which a change of either reads anew.
    pub stamp: super::replay_info::Stamp,
}

/// Replays listed at most, the newest first.
const MAX_REPLAYS: usize = 50;

/// The menu.
pub struct MenuCore {
    pub(super) store: SettingsStore,
    pub(super) prefs: Prefs,
    tls: Tls,
    pub(super) client: Option<Client>,
    conn: Conn,
    pub(super) page: Page,
    pub(super) form: Form,
    pub(super) status: Option<Status>,
    asked: Vec<(u32, Asked)>,
    pub(super) lobbies: Vec<LobbySummary>,
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
    /// The name this player goes by where there is no account: the
    /// settings' profile name.
    profile: String,
    /// The replays on this machine, as last listed.
    pub(super) replays: Vec<ReplayFile>,
    /// What each replay holds, as read.
    pub(super) shelf: super::replay_info::Shelf,
    /// Where they are kept ([`replays_dir`]).
    pub(super) replays_dir: Option<std::path::PathBuf>,
    /// The game server a local lobby starts, when not looked for.
    pub(super) server_binary: Option<std::path::PathBuf>,
    /// The settings' bot command, which a local lobby's bot seats run.
    bot_command: String,
    /// The address others type to join the local lobby this client hosts.
    hosted_at: Option<String>,
    /// Whether the service has been tried: a menu under a game named on the
    /// command line tries it once that game is left.
    tried_service: bool,
}

impl MenuCore {
    /// A menu over `store`, connecting at once when a service is known:
    /// `service` from the command line (or the page's own host), else the
    /// one remembered.
    pub fn new(store: SettingsStore, tls: Tls, service: Option<String>) -> MenuCore {
        let mut core = MenuCore::unconnected(store, tls, service);
        if !core.prefs.service.is_empty() {
            core.connect();
        }
        core
    }

    /// A menu over `store` that has not tried the service yet.
    fn unconnected(store: SettingsStore, tls: Tls, service: Option<String>) -> MenuCore {
        let mut prefs = store
            .read(Entry::Menu)
            .map_or_else(Prefs::default, |t| Prefs::parse(&t));
        if let Some(service) = service {
            prefs.service = service;
        }
        MenuCore {
            store,
            form: Form::new(FormKind::Service, &prefs),
            prefs,
            tls,
            client: None,
            conn: Conn::Offline,
            page: Page::Service,
            settings_from: Page::Service,
            status: None,
            asked: Vec::new(),
            lobbies: Vec::new(),
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
            profile: String::new(),
            replays: Vec::new(),
            shelf: Default::default(),
            replays_dir: replays_dir(),
            server_binary: None,
            bot_command: String::new(),
            hosted_at: None,
            tried_service: false,
        }
    }

    /// The settings' bot command, which a local lobby runs for its bot seats.
    pub fn set_bot_command(&mut self, command: &str) {
        if self.bot_command != command {
            command.clone_into(&mut self.bot_command);
        }
    }

    /// The address others type under DIRECT CONNECT to join the local
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
    ) -> MenuCore {
        let mut core = MenuCore::unconnected(store, tls, service);
        core.page = Page::Game;
        core.on_screen = Some(OnScreen::Given);
        core.kind = kind;
        if kind == GameKind::Play {
            core.last_seat = Some(Launch::Given);
        }
        core
    }

    /// The player confirmed quitting, once.
    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit)
    }

    /// The player left the game on screen or under the dashboard, once.
    pub fn take_leave(&mut self) -> bool {
        std::mem::take(&mut self.leave)
    }

    /// Whether a game is on screen or under the dashboard.
    pub fn in_game(&self) -> bool {
        self.on_screen.is_some() && !self.launching
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
        self.conn == Conn::LoggedIn
    }

    /// Whether the service has yet to answer the Hello.
    pub(super) fn connecting(&self) -> bool {
        self.conn == Conn::Connecting
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
        self.launching = false;
        self.status = None;
        self.confirm = None;
        self.game_unsafe = false;
        self.reconnect = self.last_seat.take().filter(|_| !over);
        // A menu that came up under a game reaches the service now.
        if !self.tried_service && !self.prefs.service.is_empty() {
            self.connect();
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
        match (left, self.conn) {
            (Some(OnScreen::Direct), _) => Page::Direct,
            (Some(OnScreen::Local), _) => Page::LocalLobby,
            (Some(OnScreen::Replay), _) => Page::Replays,
            (_, Conn::LoggedIn) => Page::Home,
            (_, Conn::Offline) => Page::Service,
            _ => Page::Login,
        }
    }

    fn form_kind(&self) -> FormKind {
        match self.page {
            Page::Service => FormKind::Service,
            Page::Login => FormKind::Login,
            Page::Register => FormKind::Register,
            Page::Lobbies => FormKind::Lobby,
            Page::Direct => FormKind::Direct,
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
        self.client = None;
        self.asked.clear();
        match Client::open(&self.prefs.service, &self.tls) {
            Ok(client) => {
                self.client = Some(client);
                self.conn = Conn::Connecting;
                self.status = None;
            }
            Err(err) => {
                self.conn = Conn::Offline;
                self.tell(format!("Cannot reach the service: {err}"), true);
            }
        }
    }

    /// Sends a request, noting what it was for its answer.
    fn ask(&mut self, msg: ClientMsg, asked: Asked) {
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

    /// Takes in what the service said since the last frame.
    pub fn update(&mut self) {
        self.shelf.take();
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
        self.client = None;
        self.conn = Conn::Offline;
        self.asked.clear();
        self.lobby = None;
        // The pages of what needs no service stay.
        if !matches!(
            self.page,
            Page::Game | Page::Direct | Page::LocalLobby | Page::Replays
        ) {
            self.page = Page::Service;
            self.form = Form::new(FormKind::Service, &self.prefs);
        }
        let why = match err {
            ClientError::Refused(error) => error_text(&error),
            other => text::line(&other.to_string(), text::MESSAGE),
        };
        self.tell(format!("Disconnected from the service: {why}"), true);
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
        let saved = self
            .store
            .read(Entry::Session)
            .and_then(|t| Saved::parse(&t));
        match saved.filter(|s| s.service == self.prefs.service) {
            Some(saved) => self.ask(
                ClientMsg::Resume {
                    session: saved.token,
                    client: Default::default(),
                },
                Asked::Resume,
            ),
            None if self.page == Page::Service => self.go_form(Page::Login),
            None => {}
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
                self.ask(ClientMsg::MyMatches, Asked::Other);
                self.status = None;
                if !matches!(self.page, Page::Game) {
                    self.go(Page::Home);
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
                self.go_form(Page::Login);
                if *error != Error::BadSession {
                    self.tell(error_text(error), true);
                }
            }
            (Asked::Rejoin(id) | Asked::Watch(id), Error::UnknownMatch) => {
                self.games.retain(|g| g.match_id != id);
                self.tell("That match is over".to_owned(), true);
            }
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
        if matches!(&self.reconnect, Some(Launch::Seat(h)) if h.match_id == id) {
            self.reconnect = None;
        }
        self.games.retain(|g| g.match_id != id);
        if self.results.len() >= MAX_RESULTS {
            self.results.pop_front();
        }
        self.results.push_back(result);
        if self.on_screen.is_none() && !self.launching {
            self.shown = Some(id);
            self.page = Page::Result;
        }
    }

    fn sit(&mut self, state: LobbyState) {
        self.lobby = Some((state, Instant::now()));
        if self.page == Page::Lobbies {
            self.page = Page::Lobby;
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

    fn start(&mut self, launch: Launch) {
        if self.on_screen.is_some() {
            self.tell("Disconnect from the game you are in first".to_owned(), true);
            return;
        }
        self.on_screen = Some(match &launch {
            Launch::Seat(h) | Launch::Watch(h) => OnScreen::Match(h.match_id),
            Launch::Direct { .. } => OnScreen::Direct,
            Launch::Host { .. } => OnScreen::Local,
            Launch::Replay(_) => OnScreen::Replay,
            Launch::Given => OnScreen::Given,
        });
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
            launch => (self.kind == GameKind::Play).then(|| launch.clone()),
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
        // Signing in starts with reaching the service.
        let page = match page {
            Page::Login | Page::Register if self.conn == Conn::Offline => Page::Service,
            page => page,
        };
        let needs_login = matches!(
            page,
            Page::Home | Page::MyGames | Page::Lobbies | Page::Lobby
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
            Page::MyGames => self.ask(ClientMsg::MyMatches, Asked::Other),
            Page::Lobbies => self.ask(ClientMsg::LobbyList, Asked::Other),
            Page::Replays => {
                self.replays = list_replays(self.replays_dir.as_deref());
                for replay in &self.replays {
                    self.shelf.summary(&replay.path, replay.stamp);
                }
            }
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
            Act::Submit => self.submit(),
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
            Act::Fill(fill) => {
                self.prefs.host_fill = fill;
                self.save_prefs();
            }
            Act::OpenReplay(at) => {
                if let Some(file) = self.replays.get(at) {
                    let path = file.path.clone();
                    match self.shelf.peek(&path, file.stamp) {
                        super::replay_info::Summary::Broken(why) => {
                            self.tell(format!("That replay cannot be watched: {why}"), true);
                        }
                        _ => self.start(Launch::Replay(path)),
                    }
                }
            }
            Act::PlainAnyway => self.direct(false, true),
            Act::Dismiss => self.status = None,
            Act::Quit if !cfg!(target_arch = "wasm32") => self.confirm = Some(Confirm::Quit),
            Act::Quit => {}
            Act::Answer(yes) => match self.confirm.take() {
                Some(Confirm::Quit) if yes => self.quit = true,
                Some(Confirm::Leave) if yes => self.leave = true,
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
                self.connect();
            }
            Page::Login => self.login(),
            Page::Register => self.register(),
            Page::Direct => self.direct(false, false),
            Page::LocalLobby => self.host(false),
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
        let password = Secret(self.form.take_secret(1));
        if login.is_empty() || password.0.is_empty() {
            self.tell("Type your login and password".to_owned(), true);
            return;
        }
        self.prefs.login = login.clone();
        self.save_prefs();
        let client = Default::default();
        self.ask(
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
        let (password, again) = (self.form.take_secret(1), self.form.take_secret(2));
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
        self.ask(msg, Asked::Register);
    }

    fn logout(&mut self) {
        if self.client.is_some() {
            self.ask(ClientMsg::Logout, Asked::Other);
        }
        self.store.remove(Entry::Session);
        self.games.clear();
        self.lobby = None;
        self.conn = Conn::Offline;
        self.page = Page::Login;
        // A fresh connection: the old one stays logged in until it closes.
        self.connect();
        self.form = Form::new(FormKind::Login, &self.prefs);
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
        let Some(ruleset) = self
            .client
            .as_ref()
            .and_then(|c| c.rulesets().next())
            .map(|r| r.id.clone())
        else {
            self.tell(
                "The service offers no game this client can play".to_owned(),
                true,
            );
            return;
        };
        let msg = ClientMsg::LobbyCreate {
            ruleset,
            name,
            audience: Audience::Everyone,
            password: (!password.is_empty()).then_some(Secret(password)),
        };
        self.ask(msg, Asked::Lobby);
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
        let address = self.form.value(0).trim().to_owned();
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
    /// joining by this machine's address (DIRECT CONNECT). Named by the
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
            SeatFill::Dummy => Fill::Dummy,
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
        let replay = self.replays_dir.as_ref().map(|dir| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            dir.join(format!("local-{secs}.brp"))
        });
        self.start(Launch::Host {
            port,
            players,
            lan,
            name: self.name(),
            key: Claim::draw(),
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

/// Where replays are kept on this machine: `$XDG_DATA_HOME/dbota/replays`,
/// else `~/.local/share/dbota/replays`; none in a browser.
pub fn replays_dir() -> Option<std::path::PathBuf> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|dir| !dir.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })?;
    Some(base.join("dbota").join("replays"))
}

/// The replays in `dir`, the newest first.
fn list_replays(dir: Option<&std::path::Path>) -> Vec<ReplayFile> {
    let Some(entries) = dir.and_then(|dir| std::fs::read_dir(dir).ok()) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, ReplayFile)> = entries
        .filter_map(Result::ok)
        .take(10_000)
        .filter(|e| e.path().extension().is_some_and(|x| x == "brp"))
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            let name = text::line(&e.file_name().to_string_lossy(), text::NAME);
            let size = format!("{:.1} MB", meta.len() as f64 / 1e6);
            let when = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            Some((
                when,
                ReplayFile {
                    path: e.path(),
                    name,
                    size,
                    when: super::replay_info::when(when),
                    stamp: super::replay_info::Stamp {
                        len: meta.len(),
                        modified: when,
                    },
                },
            ))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found
        .into_iter()
        .take(MAX_REPLAYS)
        .map(|(_, f)| f)
        .collect()
}

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
            "'{service}' is not a service address: it starts with wss:// (ws:// on your own network). A game server's address goes under DIRECT CONNECT"
        )),
    }
}
