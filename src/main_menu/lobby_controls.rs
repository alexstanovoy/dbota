//! Local lobby controls and settings drafts. They never change the service until saved.
use super::{
    form::Form,
    lobby_chat::Chat,
    view::{FieldView, ModeRow},
};
use bota_radovid::proto::{Audience, LobbyId, RulesetId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Server {
    Local,
    #[default]
    Service,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Server,
    Mode,
    Filter,
    EditMode,
    Audience,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Open(Choice),
    Server(Server),
    Mode(RulesetId),
    Filter(Option<RulesetId>),
    NoPassword,
    Create,
    Edit,
    Save,
    Cancel,
    EditMode(RulesetId),
    Audience(Audience),
    Cheats,
    HidePassword,
    HideJoinPassword,
    RemovePassword,
    FocusChat,
    SendChat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saving {
    Create,
    Settings,
    Cheats,
}

#[derive(Clone, Debug)]
pub struct ChoiceRow {
    pub name: String,
    pub selected: bool,
    pub action: Action,
}

pub struct Choices {
    pub rows: Vec<ChoiceRow>,
    pub first: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveRequest {
    pub editor: u64,
    pub lobby: Option<LobbyId>,
    pub step: Saving,
}

// No Debug: the form holds the owner's password.
pub struct Editor {
    pub id: u64,
    pub lobby: Option<LobbyId>,
    pub form: Form,
    pub creating: bool,
    pub mode: RulesetId,
    pub audience: Audience,
    pub cheats: bool,
    pub hidden: bool,
    pub password_known: bool,
    pub password_changed: bool,
    pub remove_password: bool,
    pub error: Option<String>,
    pub saving: Option<Saving>,
}

pub struct Controls {
    pub choice_start: usize,
    pub active: Option<LobbyId>,
    pub next_editor: u64,
    pub server: Server,
    pub mode: Option<RulesetId>,
    pub filter: Option<RulesetId>,
    pub no_password: bool,
    pub open: Option<Choice>,
    pub selected: Option<LobbyId>,
    pub editor: Option<Editor>,
    pub hide_join_password: bool,
    pub chat: Chat,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            choice_start: 0,
            next_editor: 0,
            active: None,
            server: Server::Service,
            mode: None,
            filter: None,
            no_password: false,
            open: None,
            selected: None,
            editor: None,
            hide_join_password: true,
            chat: Chat::default(),
        }
    }
}

impl Controls {
    pub fn dismiss(&mut self) -> bool {
        if self.open.take().is_some() {
            return true;
        }
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.saving.is_some())
        {
            return true;
        }
        if self.editor.take().is_some() {
            return true;
        }
        if self.chat.focused {
            self.chat.focused = false;
            return true;
        }
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorView {
    pub fields: Vec<FieldView>,
    pub creating: bool,
    pub mode: RulesetId,
    pub audience: Audience,
    pub cheats: Option<bool>,
    pub hidden: bool,
    pub password_unknown: bool,
    pub remove_password: bool,
    pub error: Option<String>,
    pub saving: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlsView {
    pub choice_start: usize,
    pub can_host: bool,
    pub server: Server,
    pub service: String,
    pub modes: Vec<ModeRow>,
    pub mode: Option<RulesetId>,
    pub filter: Option<RulesetId>,
    pub no_password: bool,
    pub open: Option<Choice>,
    pub selected: Option<LobbyId>,
    pub editor: Option<EditorView>,
    pub hide_join_password: bool,
    pub chat: super::lobby_chat::ChatView,
}

impl Default for ControlsView {
    fn default() -> Self {
        Self {
            choice_start: 0,
            can_host: !cfg!(target_arch = "wasm32"),
            server: Server::Service,
            service: "[SERVICE]".into(),
            modes: Vec::new(),
            mode: None,
            filter: None,
            no_password: false,
            open: None,
            selected: None,
            editor: None,
            hide_join_password: true,
            chat: Default::default(),
        }
    }
}

impl ControlsView {
    pub fn mode_name(&self, id: Option<&RulesetId>) -> &str {
        self.modes
            .iter()
            .find(|m| Some(&m.id) == id)
            .map_or("Any", |m| m.name.as_str())
    }
    pub fn server_name(&self) -> &str {
        match self.server {
            Server::Local => "Local",
            Server::Service => &self.service,
        }
    }
}

impl ControlsView {
    pub fn choice_list(&self, choice: Choice) -> Choices {
        Choices { rows: self.choices(choice), first: self.choice_start }
    }

    pub fn choices(&self, choice: Choice) -> Vec<ChoiceRow> {
        match choice {
            Choice::Server => self.server_choices(),
            Choice::Mode | Choice::EditMode | Choice::Filter => self.mode_choices(choice),
            Choice::Audience => self.audience_choices(),
        }
    }

    fn server_choices(&self) -> Vec<ChoiceRow> {
        [Server::Local, Server::Service].into_iter()
                .filter(|server| *server != Server::Local || self.can_host)
                .map(|server| ChoiceRow {
                    name: if server == Server::Local { "Local".into() } else { self.service.clone() },
                    selected: self.server == server, action: Action::Server(server),
                }).collect()
    }

    fn audience_choices(&self) -> Vec<ChoiceRow> {
        [Audience::Everyone, Audience::HumansOnly, Audience::BotsOnly].into_iter()
                .map(|audience| ChoiceRow {
                    name: audience_name(audience).into(),
                    selected: self.editor.as_ref().is_some_and(|editor| editor.audience == audience),
                    action: Action::Audience(audience),
                }).collect()
    }

    fn mode_choices(&self, choice: Choice) -> Vec<ChoiceRow> {
        let mut rows = Vec::new();
        if choice == Choice::Filter {
            rows.push(ChoiceRow { name: "Any".into(), selected: self.filter.is_none(), action: Action::Filter(None) });
        }
        rows.extend(self.modes.iter().map(|mode| ChoiceRow {
            name: mode.name.clone(),
            selected: match choice {
                Choice::Filter => self.filter.as_ref() == Some(&mode.id),
                Choice::EditMode => self.editor.as_ref().is_some_and(|editor| editor.mode == mode.id),
                _ => self.mode.as_ref() == Some(&mode.id),
            },
            action: match choice {
                Choice::Filter => Action::Filter(Some(mode.id.clone())),
                Choice::EditMode => Action::EditMode(mode.id.clone()),
                _ => Action::Mode(mode.id.clone()),
            },
        }));
        rows
    }
}

pub fn audience_name(audience: Audience) -> &'static str {
    match audience {
        Audience::Everyone => "Humans and bots",
        Audience::HumansOnly => "Humans only",
        Audience::BotsOnly => "Bots only",
    }
}
