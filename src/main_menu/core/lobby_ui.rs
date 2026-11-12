//! Lobby drafts and their existing service requests.
use super::*;
use crate::main_menu::{
    form::Item,
    lobby_controls::{Action, Choice, ControlsView, Editor, EditorView, SaveRequest, Saving, Server},
    view::{Connection, ModeRow},
};

impl MenuCore {
    fn remembered_lobby_password(&self, lobby: LobbyId) -> Option<&str> {
        self.lobby_secret.as_ref().filter(|(id, _)| *id == lobby).map(|(_, secret)| secret.as_str())
    }

    pub(super) fn invalidate_lobby_password(&mut self, locked: bool) {
        self.lobby_secret = None;
        if let Some(editor) = &mut self.lobby_ui.editor
            && !editor.creating && editor.saving.is_none() && !editor.password_changed
        {
            editor.form.take_secret(1);
            editor.password_known = !locked;
        }
        self.lobby_editor_controls();
    }

    pub(super) fn lobby_editor_obscured(&self) -> bool {
        self.lobby_ui.editor.is_some()
            && (self.confirm.is_some() || self.join_prompt.is_some() || self.view().offer.is_some())
    }

    fn mark_lobby_saving(&mut self, step: Saving) -> Option<SaveRequest> {
        let editor = self.lobby_ui.editor.as_mut()?;
        editor.saving = Some(step);
        Some(SaveRequest { editor: editor.id, lobby: editor.lobby, step })
    }

    fn ask_lobby_save(&mut self, message: ClientMsg, step: Saving) {
        if let Some(request) = self.mark_lobby_saving(step) {
            self.ask(message, Asked::LobbyUi(request));
        }
    }

    pub(super) fn lobby_save_current(&self, request: SaveRequest) -> bool {
        self.lobby_ui.editor.as_ref().is_some_and(|editor| {
            editor.id == request.editor && editor.saving == Some(request.step)
                && editor.lobby == request.lobby
                && request.lobby.is_none_or(|id| self.lobby.as_ref().is_some_and(|(state, _)| state.id == id && Some(state.owner) == self.my_id()))
        })
    }

    pub(super) fn lobby_save_replied(&mut self, reply: Reply, request: SaveRequest) {
        if !self.lobby_save_current(request) {
            return;
        }
        match (request.step, reply) {
            (Saving::Create, Reply::Lobby { state }) => {
                self.sit(state);
                self.lobby_editor_saved(request);
            }
            (Saving::Settings | Saving::Cheats, Reply::Done) => self.lobby_editor_saved(request),
            _ => {
                if let Some(editor) = &mut self.lobby_ui.editor {
                    editor.saving = None;
                    editor.error = Some("The service did not confirm the lobby change".into());
                }
            }
        }
    }

    pub(super) fn lobby_save_refused(&mut self, request: SaveRequest, error: &Error) {
        if self.lobby_save_current(request)
            && let Some(editor) = &mut self.lobby_ui.editor
        {
            editor.saving = None;
            editor.error = Some(error_text(error));
        }
    }

    #[cfg(test)]
    pub(crate) fn set_lobby_chat_port(&mut self, port: Box<dyn crate::main_menu::LobbyChatPort>) {
        self.lobby_ui.chat.port = Some(port);
    }

    pub(in crate::main_menu) fn active_lobby_form(&self) -> &Form {
        if let Some(prompt) = &self.join_prompt {
            return &prompt.form;
        }
        if let Some(editor) = &self.lobby_ui.editor {
            return &editor.form;
        }
        if self.lobby_ui.chat.focused {
            return &self.lobby_ui.chat.input;
        }
        &self.form
    }

    pub(super) fn lobby_type(&mut self, typed: Option<&str>) {
        if self
            .lobby_ui
            .editor
            .as_ref()
            .is_some_and(|editor| editor.saving.is_some())
        {
            return;
        }
        let password = self.lobby_ui.editor.is_some() && self.keyboard().focused_field() == Some(1);
        match typed {
            Some(typed) => self.keyboard().type_in(typed),
            None => self.keyboard().erase(),
        }
        if password && let Some(editor) = &mut self.lobby_ui.editor {
            editor.password_changed = true;
        }
        if self.page == Page::Lobbies
            && self.join_prompt.is_none()
            && self.lobby_ui.editor.is_none()
            && self.form.focused_field() == Some(SEARCH_FIELD)
        {
            self.search_lobbies();
        }
    }

    pub(super) fn lobby_action(&mut self, action: Action) {
        if self
            .lobby_ui
            .editor
            .as_ref()
            .is_some_and(|editor| editor.saving.is_some())
        {
            return;
        }
        match action {
            Action::Open(choice) => {
                self.lobby_ui.open = (self.lobby_ui.open != Some(choice)).then_some(choice)
            }
            Action::Server(server) => {
                self.lobby_ui.server = server;
                self.lobby_ui.open = None;
                self.lobby_ui.mode = None;
            }
            Action::Mode(mode) => {
                self.lobby_ui.mode = Some(mode);
                self.lobby_ui.open = None;
            }
            Action::Filter(mode) => {
                self.lobby_ui.filter = mode;
                self.lobby_ui.open = None;
            }
            Action::NoPassword => self.lobby_ui.no_password = !self.lobby_ui.no_password,
            Action::Create => self.open_lobby_editor(true),
            Action::Edit => self.open_lobby_editor(false),
            Action::Cancel => {
                self.lobby_ui.editor = None;
                self.lobby_ui.open = None;
            }
            Action::Save => self.save_lobby_editor(),
            Action::EditMode(mode) => {
                if let Some(editor) = &mut self.lobby_ui.editor {
                    editor.mode = mode;
                }
                self.lobby_ui.open = None;
            }
            Action::Audience(audience) => {
                if let Some(editor) = &mut self.lobby_ui.editor {
                    editor.audience = audience;
                }
                self.lobby_ui.open = None;
            }
            Action::Cheats => {
                if let Some(editor) = &mut self.lobby_ui.editor {
                    editor.cheats = !editor.cheats;
                }
            }
            Action::HidePassword => {
                if let Some(editor) = &mut self.lobby_ui.editor {
                    editor.hidden = !editor.hidden;
                    editor.form.secret(1, editor.hidden);
                }
            }
            Action::HideJoinPassword => self.hide_lobby_password(),
            Action::RemovePassword => {
                if let Some(editor) = &mut self.lobby_ui.editor {
                    editor.remove_password = !editor.remove_password;
                    editor.password_changed = true;
                }
            }
            Action::FocusChat => self.lobby_ui.chat.focused = true,
            Action::SendChat => self.lobby_ui.chat.send(),
        }
        self.lobby_editor_controls();
    }

    fn hide_lobby_password(&mut self) {
        self.lobby_ui.hide_join_password = !self.lobby_ui.hide_join_password;
        if let Some(prompt) = &mut self.join_prompt {
            prompt.form.secret(0, self.lobby_ui.hide_join_password);
        }
    }

    pub(super) fn poll_lobby_ui(&mut self) {
        let mode = self
            .lobby
            .as_ref()
            .and_then(|(state, _)| {
                self.client
                    .as_ref()
                    .and_then(|client| client.rulesets().find(|rules| rules.id == state.ruleset))
            })
            .map_or_else(String::new, |rules| text::line(&rules.name, text::NAME));
        self.lobby_ui
            .chat
            .sync(self.lobby.as_ref().map(|(state, _)| state), &mode);
        self.lobby_ui.chat.poll();
        if self
            .lobby_ui
            .editor
            .as_ref()
            .is_some_and(|editor| !editor.creating)
            && !self
                .lobby
                .as_ref()
                .is_some_and(|(state, _)| Some(state.owner) == self.my_id()
                    && self.lobby_ui.editor.as_ref().is_some_and(|editor| editor.lobby == Some(state.id)))
        {
            self.lobby_ui.editor = None;
            self.lobby_ui.open = None;
        }
    }

    pub(super) fn lobby_browser_controls(&mut self) {
        if self.form.kind != FormKind::Lobby {
            return;
        }
        self.form.set_controls(vec![
            Item::Button(Act::SearchLobbies),
            Item::Button(Act::LobbyUi(Action::Open(Choice::Filter))),
            Item::Field(SEARCH_FIELD),
            Item::Button(Act::LobbyUi(Action::NoPassword)),
            Item::Button(Act::LobbyUi(Action::Open(Choice::Server))),
            Item::Button(Act::LobbyUi(Action::Open(Choice::Mode))),
            Item::Button(Act::LobbyUi(Action::Create)),
        ]);
    }

    fn open_lobby_editor(&mut self, creating: bool) {
        self.lobby_ui.open = None;
        self.lobby_ui.chat.focused = false;
        if !creating
            && !self
                .lobby
                .as_ref()
                .is_some_and(|(state, _)| Some(state.owner) == self.my_id())
        {
            return;
        }
        if creating && self.lobby_ui.server == Server::Local {
            self.go(Page::LocalLobby);
            return;
        }
        let mode = if creating {
            self.lobby_ui
                .mode
                .clone()
                .or_else(|| self.client.as_ref().and_then(default_lobby_mode))
        } else {
            self.lobby.as_ref().map(|(state, _)| state.ruleset.clone())
        };
        let Some(mode) = mode else {
            self.tell("No game modes are available".into(), true);
            return;
        };
        let mut form = Form::new(FormKind::LobbySettings, &self.prefs);
        let (audience, cheats, password_known) = if creating {
            (Audience::Everyone, false, true)
        } else {
            self.editor_fields(&mut form)
        };
        self.lobby_ui.next_editor = self.lobby_ui.next_editor.checked_add(1).expect("editor identity exhausted");
        self.lobby_ui.editor = Some(Editor {
            id: self.lobby_ui.next_editor,
            lobby: (!creating).then(|| self.lobby.as_ref().expect("an owned lobby").0.id),
            form,
            creating,
            mode,
            audience,
            cheats,
            hidden: true,
            password_known,
            password_changed: false,
            remove_password: false,
            error: None,
            saving: None,
        });
        self.lobby_editor_controls();
    }

    fn editor_fields(&self, form: &mut Form) -> (Audience, bool, bool) {
        let Some((state, _)) = &self.lobby else {
            return (Audience::Everyone, false, true);
        };
        form.put(0, &state.name);
        form.put(1, self.remembered_lobby_password(state.id).unwrap_or(""));
        (
            state.audience,
            state.cheats == Some(true),
            !state.locked || self.remembered_lobby_password(state.id).is_some(),
        )
    }

    fn lobby_editor_controls(&mut self) {
        let Some(editor) = &mut self.lobby_ui.editor else {
            return;
        };
        let mut items = Vec::new();
        if editor.creating {
            items.push(Item::Field(0));
        }
        items.extend([
            Item::Button(Act::LobbyUi(Action::Open(Choice::EditMode))),
            Item::Field(1),
            Item::Button(Act::LobbyUi(Action::Open(Choice::Audience))),
            Item::Button(Act::LobbyUi(Action::Cheats)),
            Item::Button(Act::LobbyUi(Action::HidePassword)),
        ]);
        if !editor.password_known {
            items.push(Item::Button(Act::LobbyUi(Action::RemovePassword)));
        }
        items.extend([
            Item::Button(Act::LobbyUi(Action::Save)),
            Item::Button(Act::LobbyUi(Action::Cancel)),
        ]);
        editor.form.set_controls(items);
    }

    pub(super) fn save_lobby_editor(&mut self) {
        let Some(editor) = &mut self.lobby_ui.editor else {
            return;
        };
        if editor.saving.is_some() {
            return;
        }
        editor.error = None;
        if !editor.password_known && !editor.password_changed {
            editor.error = Some("Set or remove the lobby password before saving.".into());
            return;
        }
        if editor.creating && editor.form.value(0).trim().is_empty() {
            editor.error = Some("Enter a lobby name.".into());
            return;
        }
        self.lobby_ui.open = None;
        if self
            .lobby_ui
            .editor
            .as_ref()
            .is_some_and(|editor| editor.creating)
        {
            self.create_from_editor();
        } else {
            self.configure_from_editor();
        }
    }

    fn editor_password(&self) -> Option<Secret> {
        self.lobby_ui
            .editor
            .as_ref()
            .filter(|editor| !editor.remove_password)
            .map(|editor| editor.form.value(1))
            .filter(|password| !password.is_empty())
            .map(|password| Secret(password.to_owned()))
    }

    fn create_from_editor(&mut self) {
        let password = self.editor_password();
        let Some(editor) = &mut self.lobby_ui.editor else {
            return;
        };
        let msg = ClientMsg::LobbyCreate {
            ruleset: editor.mode.clone(),
            name: editor.form.value(0).trim().to_owned(),
            audience: editor.audience,
            password,
        };
        self.ask_lobby_save(msg, Saving::Create);
    }

    fn configure_from_editor(&mut self) {
        let password = self.editor_password();
        let Some((state, _)) = &self.lobby else {
            return;
        };
        let Some(editor) = &self.lobby_ui.editor else {
            return;
        };
        if state.ruleset == editor.mode
            && state.audience == editor.audience
            && state.locked == password.is_some()
            && self.remembered_lobby_password(state.id) == password.as_ref().map(|password| password.0.as_str())
        {
            if let Some(request) = self.mark_lobby_saving(Saving::Settings) {
                self.lobby_editor_saved(request);
            }
            return;
        }
        let msg = ClientMsg::LobbyConfigure {
            lobby: state.id,
            ruleset: editor.mode.clone(),
            audience: editor.audience,
            password,
        };
        self.ask_lobby_save(msg, Saving::Settings);
    }

    pub(super) fn lobby_editor_saved(&mut self, request: SaveRequest) {
        if !self.lobby_save_current(request) {
            return;
        }
        let password = self.editor_password().map(|password| password.0);
        let Some(editor) = &mut self.lobby_ui.editor else {
            return;
        };
        if matches!(editor.saving, Some(Saving::Create | Saving::Settings)) {
            self.lobby_secret = self.lobby.as_ref().and_then(|(state, _)| password.map(|password| (state.id, password)));
        }
        let Some((state, _)) = &self.lobby else {
            self.lobby_ui.editor = None;
            return;
        };
        editor.creating = false;
        editor.lobby = Some(state.id);
        editor.password_known = true;
        if editor.cheats && state.cheats.is_none() {
            editor.cheats = false;
            editor.saving = None;
            editor.error = Some("Cheats are unavailable in this mode.".into());
            return;
        }
        if editor.saving != Some(Saving::Cheats)
            && state.cheats.is_some_and(|on| on != editor.cheats)
        {
            editor.saving = Some(Saving::Cheats);
            let msg = ClientMsg::LobbyCheats {
                lobby: state.id,
                on: editor.cheats,
            };
            self.ask_lobby_save(msg, Saving::Cheats);
            return;
        }
        self.lobby_ui.editor = None;
    }

    pub(in crate::main_menu) fn lobby_menu_view(&self, connection: &Connection) -> ControlsView {
        let client = self.client.as_ref();
        let mode = self
            .lobby_ui
            .mode
            .clone()
            .or_else(|| client.and_then(default_lobby_mode));
        let modes = client.map_or_else(Vec::new, |client| {
            lobby_rulesets(client)
                .map(|rules| ModeRow {
                    id: rules.id.clone(),
                    name: text::line(&rules.name, text::NAME),
                    chosen: Some(&rules.id) == mode.as_ref(),
                })
                .collect()
        });
        ControlsView {
            server: self.lobby_ui.server,
            service: match connection {
                Connection::Online(name) | Connection::Lost(name) => name.clone(),
                Connection::Offline => "Local".into(),
            },
            modes,
            mode,
            filter: self.lobby_ui.filter.clone(),
            no_password: self.lobby_ui.no_password,
            open: self.lobby_ui.open,
            selected: self.lobby_ui.selected,
            editor: self
                .lobby_ui
                .editor
                .as_ref()
                .map(|editor| self.editor_view(editor)),
            hide_join_password: self.lobby_ui.hide_join_password,
            chat: self.lobby_ui.chat.view(),
        }
    }

    fn editor_view(&self, editor: &Editor) -> EditorView {
        let cheats = if editor.creating {
            Some(editor.cheats)
        } else {
            self.lobby
                .as_ref()
                .and_then(|(state, _)| state.cheats)
                .map(|_| editor.cheats)
        };
        EditorView {
            fields: crate::main_menu::view::fields_of(&editor.form),
            creating: editor.creating,
            mode: editor.mode.clone(),
            audience: editor.audience,
            cheats,
            hidden: editor.hidden,
            password_unknown: !editor.password_known,
            remove_password: editor.remove_password,
            error: editor.error.clone(),
            saving: editor.saving.is_some(),
        }
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;

    fn draft(id: u64) -> Editor {
        Editor {
            id, lobby: None, creating: true,
            form: Form::new(FormKind::LobbySettings, &Prefs::default()),
            mode: RulesetId("full".into()), audience: Audience::Everyone,
            cheats: false, hidden: true, password_known: true, password_changed: false,
            remove_password: false, error: None, saving: None,
        }
    }

    // Deliberately delayed answers exercise an untrusted peer, not a claimed server ordering.
    #[test]
    fn delayed_lobby_save_answers_do_not_change_a_replacement_draft() {
        let mut player = crate::main_menu::tests::Player::new(None);
        let menu = &mut player.menu;
        menu.lobby_ui.editor = Some(draft(1));
        let old = menu.mark_lobby_saving(Saving::Create).expect("original request");
        menu.lobby_ui.editor = Some(draft(2));
        menu.mark_lobby_saving(Saving::Create).expect("replacement request");
        for (sequence, result) in [(41, Ok(Reply::Done)), (42, Err(Error::NotLobbyOwner))] {
            menu.asked.push((sequence, Asked::LobbyUi(old)));
            menu.take(Incoming::Reply { seq: sequence, result });
            let editor = menu.lobby_ui.editor.as_ref().expect("replacement survives");
            assert_eq!(editor.id, 2);
            assert_eq!(editor.saving, Some(Saving::Create));
            assert!(editor.error.is_none());
        }
    }
}
