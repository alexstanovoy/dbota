//! The text fields of a menu page: typed into with the keyboard, the
//! password ones drawn as dots and emptied once sent.

use super::core::{Act, Page};
use super::prefs::Prefs;

/// The port a local lobby listens on unless told otherwise: bota_server's.
pub const DEFAULT_PORT: &str = "4455";

/// Which page's fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKind {
    None,
    Service,
    Login,
    Register,
    /// A lobby's name and password, to create or join one.
    Lobby,
    /// The owner's password field in the lobby one sits in.
    Room,
    /// The local lobby page: a lobby hosted on this machine (port, players) and a game joined by
    /// address.
    Host,
    /// A replay file to open.
    Replay,
}

/// One field.
#[derive(Clone)]
pub struct Field {
    pub label: &'static str,
    value: String,
    /// A password: drawn as dots, never kept past its use.
    pub secret: bool,
    /// Characters it takes at most.
    max: usize,
}

impl Field {
    fn new(label: &'static str, value: &str, secret: bool, max: usize) -> Field {
        Field {
            label,
            value: value.chars().take(max).collect(),
            secret,
            max,
        }
    }

    /// What is drawn: the text, or a dot per character of a password.
    pub fn shown(&self) -> String {
        if self.secret {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        }
    }
}

/// What Tab visits on a page's form, in the order it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Item {
    /// A field, by its index.
    Field(usize),
    Button(Act),
}

fn items_of(kind: FormKind, fields: usize) -> Vec<Item> {
    let links = [Act::Go(Page::Service)];
    #[cfg(not(target_arch = "wasm32"))]
    let links = links.into_iter().chain([Act::Go(Page::LocalLobby)]);
    let buttons = |acts: Vec<Act>| {
        (0..fields)
            .map(Item::Field)
            .chain(acts.into_iter().map(Item::Button))
            .collect()
    };
    match kind {
        FormKind::None => Vec::new(),
        FormKind::Service if cfg!(target_arch = "wasm32") => buttons(vec![Act::Submit]),
        FormKind::Service => buttons(vec![Act::Submit, Act::Go(Page::LocalLobby)]),
        FormKind::Login => buttons(
            [Act::Submit, Act::Go(Page::Register)]
                .into_iter()
                .chain(links)
                .collect(),
        ),
        FormKind::Register => buttons(
            [Act::Submit, Act::Go(Page::Login)]
                .into_iter()
                .chain(links)
                .collect(),
        ),
        FormKind::Lobby => buttons(vec![Act::CreateLobby, Act::Submit]),
        FormKind::Room => buttons(vec![Act::ConfigurePassword]),
        FormKind::Host => vec![
            Item::Field(0),
            Item::Button(Act::Host(false)),
            Item::Button(Act::Host(true)),
            Item::Field(1),
            Item::Button(Act::Direct(false)),
            Item::Button(Act::Direct(true)),
        ],
        #[cfg(target_arch = "wasm32")]
        FormKind::Replay => vec![Item::Button(Act::OpenReplayFile)],
        #[cfg(not(target_arch = "wasm32"))]
        FormKind::Replay => buttons(vec![Act::Submit]),
    }
}

/// A page's fields and buttons, and the one of them with the keyboard: Tab goes through them in
/// the order they are drawn. Not `Debug`: it holds passwords.
#[derive(Clone)]
pub struct Form {
    pub kind: FormKind,
    fields: Vec<Field>,
    items: Vec<Item>,
    /// An index into the items.
    focus: usize,
    /// The replays whose buttons come before the rest of the replays page's items.
    rows: Vec<String>,
}

impl Form {
    pub fn new(kind: FormKind, prefs: &Prefs) -> Form {
        use bota_radovid::proto::{MAX_LOBBY_NAME_LEN, MAX_LOBBY_PASSWORD_LEN, MAX_LOGIN_LEN};
        const PASSWORD: usize = bota_radovid::proto::MAX_PASSWORD_LEN;
        let fields = match kind {
            FormKind::None => Vec::new(),
            FormKind::Service => vec![Field::new("Service", &prefs.service, false, 200)],
            FormKind::Login => vec![
                Field::new("Login", &prefs.login, false, MAX_LOGIN_LEN),
                Field::new("Password", "", true, PASSWORD),
            ],
            FormKind::Register => vec![
                Field::new("Login", "", false, MAX_LOGIN_LEN),
                Field::new("Password", "", true, PASSWORD),
                Field::new("Password again", "", true, PASSWORD),
            ],
            FormKind::Room => vec![Field::new(
                "Password (empty removes it)",
                "",
                true,
                MAX_LOBBY_PASSWORD_LEN,
            )],
            FormKind::Lobby => vec![
                Field::new("Lobby name", "", false, MAX_LOBBY_NAME_LEN),
                Field::new("Password (optional)", "", true, MAX_LOBBY_PASSWORD_LEN),
            ],
            FormKind::Host => {
                let or = |value: &str, default: &str| {
                    if value.is_empty() { default } else { value }.to_owned()
                };
                vec![
                    Field::new("Port", &or(&prefs.host_port, DEFAULT_PORT), false, 5),
                    Field::new("Address", &prefs.direct, false, 200),
                ]
            }
            // A browser picks a replay file with its own chooser.
            FormKind::Replay if cfg!(target_arch = "wasm32") => Vec::new(),
            FormKind::Replay => vec![Field::new("Replay file", "", false, 1024)],
        };
        // The keyboard starts in the first empty field, the local lobby's in its first.
        let first = match kind {
            FormKind::Host => 0,
            _ => fields.iter().position(|f| f.value.is_empty()).unwrap_or(0),
        };
        let items = items_of(kind, fields.len());
        let focus = items
            .iter()
            .position(|i| *i == Item::Field(first))
            .unwrap_or(0);
        Form {
            kind,
            fields,
            items,
            focus,
            rows: Vec::new(),
        }
    }

    /// The replays page lists these replays, each with buttons Tab visits before the page's
    /// own; the keyboard stays where it was.
    pub fn set_replay_rows(&mut self, names: &[String]) {
        if self.kind != FormKind::Replay || self.rows == names {
            return;
        }
        let was = self.items.get(self.focus).cloned();
        let mut items: Vec<Item> = names
            .iter()
            .flat_map(|name| {
                let watch = Item::Button(Act::OpenReplay(name.clone()));
                #[cfg(target_arch = "wasm32")]
                let download = Some(Item::Button(Act::DownloadReplay(name.clone())));
                #[cfg(not(target_arch = "wasm32"))]
                let download = None;
                let delete = Item::Button(Act::DeleteReplay(name.clone()));
                [Some(watch), download, Some(delete)].into_iter().flatten()
            })
            .collect();
        items.extend(items_of(self.kind, self.fields.len()));
        self.focus = was
            .and_then(|was| items.iter().position(|i| *i == was))
            .unwrap_or(0);
        self.items = items;
        self.rows = names.to_vec();
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The field with the keyboard, if a field has it.
    pub fn focused_field(&self) -> Option<usize> {
        match self.items.get(self.focus)? {
            Item::Field(at) => Some(*at),
            Item::Button(_) => None,
        }
    }

    pub fn value(&self, at: usize) -> &str {
        self.fields.get(at).map_or("", |f| f.value.as_str())
    }

    /// A field's text, which the field forgets.
    pub fn take_secret(&mut self, at: usize) -> String {
        self.fields
            .get_mut(at)
            .map(|f| std::mem::take(&mut f.value))
            .unwrap_or_default()
    }

    /// Puts the keyboard in field `at`.
    pub fn focus(&mut self, at: usize) {
        if let Some(item) = self.items.iter().position(|i| *i == Item::Field(at)) {
            self.focus = item;
        }
    }

    /// Tab: the next field or button, round again after the last.
    pub fn next(&mut self) {
        if !self.items.is_empty() {
            self.focus = (self.focus + 1) % self.items.len();
        }
    }

    /// Shift+Tab.
    pub fn previous(&mut self) {
        if !self.items.is_empty() {
            self.focus = (self.focus + self.items.len() - 1) % self.items.len();
        }
    }

    /// The button with the keyboard, if a button has it.
    pub fn focused_button(&self) -> Option<&Act> {
        match self.items.get(self.focus)? {
            Item::Button(act) => Some(act),
            Item::Field(_) => None,
        }
    }

    /// Types into the focused field: printable characters only, up to its
    /// length.
    pub fn type_in(&mut self, typed: &str) {
        let Some(field) = self.focused_field().and_then(|at| self.fields.get_mut(at)) else {
            return;
        };
        for c in typed.chars().filter(|c| !c.is_control()) {
            if field.value.chars().count() >= field.max {
                break;
            }
            field.value.push(c);
        }
    }

    pub fn erase(&mut self) {
        if let Some(field) = self.focused_field().and_then(|at| self.fields.get_mut(at)) {
            field.value.pop();
        }
    }
}
