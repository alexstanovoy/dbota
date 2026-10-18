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
    /// How a finished match went.
    Result,
    /// A game is on screen, or being opened.
    Game,
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
    /// Goes on with plain TCP to a host after the notice.
    PlainAnyway,
    /// Closes the notice line.
    Dismiss,
}

/// A game the shell is to open.
#[derive(Clone, Debug, PartialEq)]
pub enum Launch {
    /// Take one's seat with the handoff's ticket.
    Seat(Handoff),
    /// Watch one's own seat with the ticket.
    Watch(Handoff),
    /// Connect to an address and say Hello as `name`.
    Direct {
        address: String,
        name: String,
        spectate: bool,
    },
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
}

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
}

impl MenuCore {
    /// A menu over `store`, connecting at once when a service is known:
    /// `service` from the command line (or the page's own host), else the
    /// one remembered.
    pub fn new(store: SettingsStore, tls: Tls, service: Option<String>) -> MenuCore {
        let mut prefs = store
            .read(Entry::Menu)
            .map_or_else(Prefs::default, |t| Prefs::parse(&t));
        if let Some(service) = service {
            prefs.service = service;
        }
        let mut core = MenuCore {
            store,
            form: Form::new(FormKind::Service, &prefs),
            prefs,
            tls,
            client: None,
            conn: Conn::Offline,
            page: Page::Service,
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
        };
        if !core.prefs.service.is_empty() {
            core.connect();
        }
        core
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn logged_in(&self) -> bool {
        self.conn == Conn::LoggedIn
    }

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
        let direct = self.on_screen.take() == Some(OnScreen::Direct);
        self.page = self.rest_page(direct);
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
    /// it was not the player's doing).
    pub fn left_game(&mut self, why: Option<&str>) {
        let left = self.on_screen.take();
        self.launching = false;
        self.status = None;
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
            left => self.page = self.rest_page(left == Some(OnScreen::Direct)),
        }
        self.form = Form::new(self.form_kind(), &self.prefs);
    }

    /// The page to fall back to: Direct after a direct game, Play once
    /// logged in, else the way in.
    fn rest_page(&self, direct: bool) -> Page {
        match self.conn {
            _ if direct => Page::Direct,
            Conn::LoggedIn => Page::Home,
            Conn::Offline => Page::Service,
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
        if !matches!(self.page, Page::Game | Page::Direct) {
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
        self.on_screen = Some(match &launch {
            Launch::Seat(h) | Launch::Watch(h) => OnScreen::Match(h.match_id),
            Launch::Direct { .. } => OnScreen::Direct,
        });
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
            let rest = self.rest_page(false);
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
            Act::PlainAnyway => self.direct(false, true),
            Act::Dismiss => self.status = None,
        }
    }

    fn submit(&mut self) {
        match self.page {
            Page::Service => {
                self.prefs.service = self.form.value(0).trim().to_owned();
                self.save_prefs();
                self.connect();
            }
            Page::Login => self.login(),
            Page::Register => self.register(),
            Page::Direct => self.direct(false, false),
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
        let name = match self.form.value(1).trim() {
            "" => "player".to_owned(),
            name => name.to_owned(),
        };
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
        self.prefs.name = name.clone();
        self.save_prefs();
        self.start(Launch::Direct {
            address,
            name,
            spectate,
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
