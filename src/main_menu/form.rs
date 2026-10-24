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
    Direct,
    /// A lobby on this machine: its port and seats.
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

/// The buttons of a page's form in the order they are drawn, which Tab visits after the fields.
fn buttons(kind: FormKind) -> Vec<Act> {
    let links = [Act::Go(Page::Service), Act::Go(Page::Direct)];
    match kind {
        FormKind::None => Vec::new(),
        FormKind::Service => vec![Act::Submit, Act::Go(Page::Direct)],
        FormKind::Login => [Act::Submit, Act::Go(Page::Register)]
            .into_iter()
            .chain(links)
            .collect(),
        FormKind::Register => [Act::Submit, Act::Go(Page::Login)]
            .into_iter()
            .chain(links)
            .collect(),
        FormKind::Lobby => vec![Act::CreateLobby, Act::Submit],
        FormKind::Direct => vec![Act::Direct(false), Act::Direct(true)],
        FormKind::Host => vec![Act::Host(false), Act::Host(true)],
        FormKind::Replay => vec![Act::Submit],
    }
}

/// A page's fields and buttons, and the one of them with the keyboard: Tab goes through the
/// fields, then the buttons. Not `Debug`: it holds passwords.
#[derive(Clone)]
pub struct Form {
    pub kind: FormKind,
    fields: Vec<Field>,
    buttons: Vec<Act>,
    /// An index into the fields, then the buttons.
    focus: usize,
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
            FormKind::Lobby => vec![
                Field::new("Lobby name", "", false, MAX_LOBBY_NAME_LEN),
                Field::new("Password (optional)", "", true, MAX_LOBBY_PASSWORD_LEN),
            ],
            FormKind::Direct => vec![Field::new("Address", &prefs.direct, false, 200)],
            FormKind::Host => {
                let or = |value: &str, default: &str| {
                    if value.is_empty() { default } else { value }.to_owned()
                };
                vec![
                    Field::new("Port", &or(&prefs.host_port, DEFAULT_PORT), false, 5),
                    Field::new("Players", &or(&prefs.host_players, "2"), false, 2),
                ]
            }
            FormKind::Replay => vec![Field::new("Replay file", "", false, 1024)],
        };
        // The keyboard starts in the first empty field.
        let focus = fields.iter().position(|f| f.value.is_empty()).unwrap_or(0);
        Form {
            kind,
            fields,
            buttons: buttons(kind),
            focus,
        }
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn focused(&self) -> usize {
        self.focus
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

    pub fn focus(&mut self, at: usize) {
        if at < self.fields.len() {
            self.focus = at;
        }
    }

    /// Tab: the next field or button, round again after the last.
    pub fn next(&mut self) {
        let count = self.fields.len() + self.buttons.len();
        if count > 0 {
            self.focus = (self.focus + 1) % count;
        }
    }

    /// Shift+Tab.
    pub fn previous(&mut self) {
        let count = self.fields.len() + self.buttons.len();
        if count > 0 {
            self.focus = (self.focus + count - 1) % count;
        }
    }

    /// The button with the keyboard, if a button has it.
    pub fn focused_button(&self) -> Option<&Act> {
        self.buttons.get(self.focus.checked_sub(self.fields.len())?)
    }

    /// Types into the focused field: printable characters only, up to its
    /// length.
    pub fn type_in(&mut self, typed: &str) {
        let Some(field) = self.fields.get_mut(self.focus) else {
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
        if let Some(field) = self.fields.get_mut(self.focus) {
            field.value.pop();
        }
    }
}
