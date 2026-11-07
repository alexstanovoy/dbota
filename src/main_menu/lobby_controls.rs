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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Saving {
    Create,
    Settings,
    Cheats,
}

// No Debug: the form holds the owner's password.
pub struct Editor {
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
