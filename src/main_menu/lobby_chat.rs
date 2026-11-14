//! Ephemeral lobby chat, kept separate from a game's chat and from its team colours.
use super::{
    form::{Form, FormKind},
    prefs::Prefs,
    text,
};
use bota_radovid::proto::{AccountId, LobbyId, LobbyState};
use std::collections::VecDeque;

pub const MAX_LINES: usize = 64;
pub const MAX_MESSAGE: usize = 200;

/// The adapter supplies only messages delivered to the current lobby's member.
pub trait Port {
    fn send(&mut self, lobby: LobbyId, message: &str) -> Result<(), String>;
    fn receive(&mut self) -> Option<crate::main_menu::LobbyChatMessage>;
}

pub struct Incoming {
    pub lobby: LobbyId,
    pub account: AccountId,
    pub message: String,
    pub minute: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub minute: u16,
    pub account: Option<AccountId>,
    pub name: String,
    pub version: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub account: AccountId,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatView {
    pub owner: Option<AccountId>,
    pub people: Vec<Person>,
    pub lines: Vec<Line>,
    pub input: String,
    pub focused: bool,
    pub available: bool,
    pub error: Option<String>,
}

pub struct Chat {
    pub input: Form,
    pub focused: bool,
    pub(in crate::main_menu) port: Option<Box<dyn Port>>,
    lobby: Option<LobbyId>,
    owner: Option<AccountId>,
    people: Vec<Person>,
    mode: Option<String>,
    lines: VecDeque<Line>,
    error: Option<String>,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            input: Form::new(FormKind::LobbyChat, &Prefs::default()),
            focused: false,
            port: None,
            lobby: None,
            owner: None,
            people: Vec::new(),
            mode: None,
            lines: VecDeque::new(),
            error: None,
        }
    }
}

impl Chat {
    pub fn sync(&mut self, state: Option<&LobbyState>, mode: &str) {
        let id = state.map(|state| state.id);
        if self.lobby != id {
            self.clear();
            self.lobby = id;
        }
        let Some(state) = state else {
            return;
        };
        let people: Vec<_> = state
            .members
            .iter()
            .take(256)
            .map(|member| Person {
                account: member.account,
                name: text::line(&member.nickname, text::NAME),
                version: member.version.clone(),
            })
            .collect();
        self.owner = Some(state.owner);
        for person in &people {
            if !self.people.iter().any(|old| old.account == person.account) {
                self.system(
                    (&person.name, person.version.as_deref()),
                    "joined the lobby.",
                );
            }
        }
        let departed: Vec<_> = self
            .people
            .iter()
            .filter(|old| !people.iter().any(|p| p.account == old.account))
            .cloned()
            .collect();
        for old in departed {
            self.system((&old.name, old.version.as_deref()), "left the lobby.");
        }
        if self.mode.as_ref().is_some_and(|old| old != mode) {
            let host = people
                .iter()
                .find(|p| p.account == state.owner)
                .map_or(("The host", None), |p| {
                    (p.name.as_str(), p.version.as_deref())
                });
            self.system(host, &format!("set the mode to {mode}."));
        }
        self.mode = Some(mode.to_owned());
        self.people = people;
    }

    pub fn poll(&mut self) {
        for _ in 0..MAX_LINES {
            let Some(incoming) = self.port.as_mut().and_then(|port| port.receive()) else {
                break;
            };
            if Some(incoming.lobby) != self.lobby {
                continue;
            }
            let Some(person) = self
                .people
                .iter()
                .find(|person| person.account == incoming.account)
            else {
                continue;
            };
            let name = person.name.clone();
            let version = person.version.clone();
            let message = text::line(&incoming.message, MAX_MESSAGE);
            if message.is_empty() {
                continue;
            }
            self.push(Line {
                version,
                minute: incoming.minute % (24 * 60),
                account: Some(incoming.account),
                name,
                message,
            });
        }
    }

    pub fn send(&mut self) {
        let message = text::line(self.input.value(0), MAX_MESSAGE);
        if message.is_empty() {
            return;
        }
        let Some(lobby) = self.lobby else {
            return;
        };
        let Some(port) = &mut self.port else {
            self.error = Some("Lobby chat is unavailable".into());
            return;
        };
        match port.send(lobby, &message) {
            Ok(()) => {
                self.input.take_secret(0);
                self.error = None;
            }
            Err(why) => self.error = Some(text::line(&why, text::MESSAGE)),
        }
    }

    pub fn view(&self) -> ChatView {
        ChatView {
            owner: self.owner,
            people: self.people.clone(),
            lines: self.lines.iter().cloned().collect(),
            input: text::line(self.input.value(0), MAX_MESSAGE),
            focused: self.focused,
            available: self.port.is_some(),
            error: self.error.clone(),
        }
    }

    fn clear(&mut self) {
        self.lines.clear();
        self.people.clear();
        self.mode = None;
        self.owner = None;
        self.input.take_secret(0);
        self.focused = false;
        self.error = None;
    }

    /// A line of the room itself: `subject` (a name with its version, drawn by the name widget)
    /// and what happened to it.
    fn system(&mut self, (name, version): (&str, Option<&str>), message: &str) {
        let minute = minute_now();
        self.push(Line {
            version: version.map(str::to_owned),
            minute,
            account: None,
            name: name.to_owned(),
            message: text::line(message, MAX_MESSAGE),
        });
    }

    fn push(&mut self, line: Line) {
        if self.lines.len() == MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn minute_now() -> u16 {
    let now = jiff::Zoned::now();
    now.hour() as u16 * 60 + now.minute() as u16
}

#[cfg(target_arch = "wasm32")]
fn minute_now() -> u16 {
    let now = js_sys::Date::new_0();
    (now.get_hours() * 60 + now.get_minutes()) as u16
}
