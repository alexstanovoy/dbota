//! Lobby drafts and their existing service requests.
use super::*;
use crate::main_menu::{
    form::Item,
    lobby_controls::{
        Action, Choice, ControlsView, Editor, EditorView, SaveRequest, Saving, Server,
    },
    view::{Connection, ModeRow},
};

impl MenuCore {
    #[cfg(test)]
    pub(crate) fn lobby_sync_settled(&self) -> bool {
        self.link_ready() && self.asked.is_empty()
    }

    pub(super) fn restore_lobby_membership(&mut self, memberships: &[LobbyId]) {
        if self.page != Page::Lobby {
            return;
        }
        if let Some(lobby) = self.lobby_ui.active.filter(|id| memberships.contains(id)) {
            self.send(
                ClientMsg::LobbyJoin {
                    lobby,
                    password: None,
                },
                Asked::RestoreLobby(lobby),
            );
        } else {
            self.lobby_ui.active = None;
            self.go(Page::Lobbies);
            self.tell("The lobby is no longer available.".into(), true);
        }
    }

    pub(super) fn lobby_resumed(&mut self, lobby: LobbyId, reply: Reply) {
        if self.page != Page::Lobby || self.lobby_ui.active != Some(lobby) {
            return;
        }
        if let Reply::Lobby { state } = reply
            && state.id == lobby
        {
            if !self.sit(state) {
                self.lobby_resume_failed(
                    lobby,
                    "Invalid lobby information from the service.".into(),
                );
            }
        } else {
            self.lobby_resume_failed(lobby, "The service did not restore the lobby".into());
        }
    }

    pub(super) fn lobby_resume_failed(&mut self, lobby: LobbyId, reason: String) {
        if self.page == Page::Lobby && self.lobby_ui.active == Some(lobby) {
            self.lobby = None;
            self.lobby_ui.active = None;
            self.go(Page::Lobbies);
            self.tell(reason, true);
        }
    }

    fn remembered_lobby_password(&self, lobby: LobbyId) -> Option<&str> {
        self.lobby_secret
            .as_ref()
            .filter(|(id, _)| *id == lobby)
            .map(|(_, secret)| secret.as_str())
    }

    pub(super) fn invalidate_lobby_password(&mut self, locked: bool) {
        self.lobby_secret = None;
        if let Some(editor) = &mut self.lobby_ui.editor
            && !editor.creating
            && editor.saving.is_none()
            && !editor.password_changed
        {
            editor.form.take_secret(1);
            editor.password_known = !locked;
        }
        self.lobby_editor_controls();
    }

    pub(super) fn lobby_editor_obscured(&self) -> bool {
        self.lobby_ui.editor.is_some()
            && (self.confirm.is_some() || self.join_prompt.is_some() || self.lobby_has_offer())
    }

    fn mark_lobby_saving(&mut self, step: Saving) -> Option<SaveRequest> {
        let editor = self.lobby_ui.editor.as_mut()?;
        editor.saving = Some(step);
        editor.saving_until = Some(Instant::now() + super::lobby_requests::REQUEST_REACH);
        Some(SaveRequest {
            editor: editor.id,
            lobby: editor.lobby,
            step,
        })
    }

    fn ask_lobby_save(&mut self, message: ClientMsg, step: Saving) {
        if let Some(request) = self.mark_lobby_saving(step) {
            self.ask(message, Asked::LobbyUi(request));
        }
    }

    pub(super) fn lobby_save_current(&self, request: SaveRequest) -> bool {
        self.lobby_ui.editor.as_ref().is_some_and(|editor| {
            editor.id == request.editor
                && editor.saving == Some(request.step)
                && editor.lobby == request.lobby
                && request.lobby.is_none_or(|id| {
                    self.lobby.as_ref().is_some_and(|(state, _)| {
                        state.id == id && Some(state.owner) == self.my_id()
                    })
                })
        })
    }

    pub(super) fn lobby_save_replied(&mut self, reply: Reply, request: SaveRequest) {
        if !self.lobby_save_current(request) {
            return;
        }
        if request.step == Saving::Create {
            self.lobby_ui.creation_unconfirmed = false;
        }
        match (request.step, reply) {
            (Saving::Create, Reply::Lobby { state }) => {
                if self.sit(state) {
                    self.lobby_editor_saved(request);
                } else if let Some(editor) = &mut self.lobby_ui.editor {
                    editor.saving = None;
                    editor.saving_until = None;
                    editor.error = Some("Invalid lobby information from the service. Creation was not confirmed; refresh Lobbies before trying again.".into());
                }
            }
            (Saving::Settings | Saving::Cheats, Reply::Done) => self.lobby_editor_saved(request),
            _ => {
                if let Some(editor) = &mut self.lobby_ui.editor {
                    editor.saving = None;
                    editor.saving_until = None;
                    editor.error = Some("The service did not confirm the lobby change".into());
                }
            }
        }
    }

    pub(super) fn lobby_save_refused(&mut self, request: SaveRequest, error: &Error) {
        if self.lobby_save_current(request)
            && let Some(editor) = &mut self.lobby_ui.editor
        {
            if request.step == Saving::Create {
                self.lobby_ui.creation_unconfirmed = false;
            }
            editor.saving = None;
            editor.saving_until = None;
            editor.error = Some(error_text(error));
        }
    }

    #[cfg(test)]
    pub(crate) fn set_lobby_chat_port(&mut self, port: Box<dyn crate::main_menu::LobbyChatPort>) {
        self.lobby_ui.chat.port = Some(port);
        self.lobby_ui.controls_dirty = true;
    }

    pub(in crate::main_menu) fn active_lobby_form(&self) -> &Form {
        if self.lobby_ui.dialog.is_some() {
            return &self.lobby_ui.dialog_form;
        }
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
            && self.join_prompt.is_none()
        {
            return;
        }
        let before = self
            .keyboard()
            .focused_field()
            .map(|index| self.keyboard().value(index).to_owned());
        let password = self.lobby_ui.editor.is_some() && self.keyboard().focused_field() == Some(1);
        match typed {
            Some(typed) => self.keyboard().type_in(typed),
            None => self.keyboard().erase(),
        }
        let after = self
            .keyboard()
            .focused_field()
            .map(|index| self.keyboard().value(index).to_owned());
        if password
            && before != after
            && let Some(editor) = &mut self.lobby_ui.editor
        {
            editor.password_changed = true;
        }
        if self.page == Page::Lobbies
            && self.join_prompt.is_none()
            && self.lobby_ui.editor.is_none()
            && self.form.focused_field() == Some(SEARCH_FIELD)
            && before.as_deref().map(str::trim) != after.as_deref().map(str::trim)
        {
            self.lobby_ui.search_due = Some(Instant::now() + std::time::Duration::from_millis(250));
        }
    }

    pub(super) fn lobby_action(&mut self, action: Action) {
        if self
            .lobby_ui
            .editor
            .as_ref()
            .is_some_and(|editor| editor.saving.is_some())
            && action != Action::Cancel
        {
            return;
        }
        if let Some(action) = self.choose_lobby_option(action) {
            self.edit_lobby_draft(action);
        }
        self.lobby_editor_controls();
        self.lobby_browser_controls();
        self.lobby_room_controls();
    }

    fn choose_lobby_option(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::Open(choice) => {
                self.lobby_ui.open = (self.lobby_ui.open != Some(choice)).then_some(choice);
                self.lobby_ui.choice_start = self
                    .lobby_menu_view(&Connection::Offline)
                    .choices(choice)
                    .iter()
                    .position(|row| row.selected)
                    .unwrap_or(0)
                    .saturating_sub(crate::theme::lobby::CHOICE_VISIBLE_ROWS / 2);
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
            other => return Some(other),
        }
        None
    }

    fn edit_lobby_draft(&mut self, action: Action) {
        match action {
            Action::Create => self.create_lobby_now(),
            Action::Edit => self.open_lobby_editor(false),
            Action::Cancel => {
                self.cancel_lobby_editor();
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
                }
            }
            Action::FocusChat => self.lobby_ui.chat.focused = self.lobby_ui.chat.port.is_some(),
            Action::SendChat => self.lobby_ui.chat.send(),
            _ => unreachable!("choice actions are handled before draft actions"),
        }
    }

    fn hide_lobby_password(&mut self) {
        self.lobby_ui.hide_join_password = !self.lobby_ui.hide_join_password;
        if let Some(prompt) = &mut self.join_prompt {
            prompt.form.secret(0, self.lobby_ui.hide_join_password);
        }
    }

    pub(super) fn poll_lobby_ui(&mut self) {
        self.poll_lobby_ui_at(Instant::now());
    }

    fn poll_lobby_ui_at(&mut self, now: Instant) {
        self.expire_lobby_requests(now);
        self.sync_lobby_dialog();
        if self.lobby_ui.search_due.is_some_and(|due| now >= due) {
            self.lobby_ui.search_due = None;
            if self.page == Page::Lobbies
                && self.lobby_ui.editor.is_none()
                && self.join_prompt.is_none()
            {
                self.search_lobbies();
            }
        }
        if self.lobby_ui.state_dirty {
            let mode = self
                .lobby
                .as_ref()
                .and_then(|(state, _)| self.catalogue.iter().find(|r| r.id == state.ruleset))
                .map_or("", |r| r.name.as_str());
            self.lobby_ui
                .chat
                .sync(self.lobby.as_ref().map(|(state, _)| state), mode);
            self.lobby_ui.state_dirty = false;
        }
        self.lobby_ui.chat.poll();
        if self.lobby_ui.editor.as_ref().is_some_and(|e| !e.creating)
            && !self.lobby.as_ref().is_some_and(|(s, _)| {
                Some(s.owner) == self.my_id()
                    && self
                        .lobby_ui
                        .editor
                        .as_ref()
                        .is_some_and(|e| e.lobby == Some(s.id))
            })
        {
            self.cancel_lobby_editor();
            self.lobby_ui.controls_dirty = true;
        }
        if self.lobby_ui.controls_dirty {
            self.lobby_editor_controls();
            self.lobby_browser_controls();
            self.lobby_room_controls();
            self.lobby_ui.controls_dirty = false;
        }
    }

    fn lobby_choice_controls(&self) -> Option<Vec<Item>> {
        let choice = self.lobby_ui.open?;
        let rows = self.lobby_menu_view(&Connection::Offline).choices(choice);
        let selected = rows.iter().position(|row| row.selected).unwrap_or(0);
        let mut items: Vec<_> = rows
            .into_iter()
            .map(|row| Item::Button(Act::LobbyUi(row.action)))
            .collect();
        if !items.is_empty() {
            items.rotate_left(selected);
        }
        Some(items)
    }

    pub(super) fn follow_lobby_choice_focus(&mut self) {
        let Some(choice) = self.lobby_ui.open else {
            return;
        };
        let focused = self.keyboard().focused_button().cloned();
        let rows = self.lobby_menu_view(&Connection::Offline).choices(choice);
        if let Some(index) = rows
            .iter()
            .position(|row| focused == Some(Act::LobbyUi(row.action.clone())))
        {
            if index < self.lobby_ui.choice_start {
                self.lobby_ui.choice_start = index;
            } else if index >= self.lobby_ui.choice_start + crate::theme::lobby::CHOICE_VISIBLE_ROWS
            {
                self.lobby_ui.choice_start = index + 1 - crate::theme::lobby::CHOICE_VISIBLE_ROWS;
            }
        }
    }

    pub(in crate::main_menu) fn lobby_key(&self, key: &bevy::input::keyboard::Key) -> Option<Act> {
        use bevy::input::keyboard::Key;
        if !matches!(self.page, Page::Lobby | Page::Lobbies) {
            return None;
        }
        match key {
            Key::ArrowDown if self.lobby_ui.open.is_some() => Some(Act::NextField),
            Key::ArrowUp if self.lobby_ui.open.is_some() => Some(Act::PreviousField),
            Key::Space if self.button_focus().is_some() => Some(Act::Submit),
            _ => None,
        }
    }

    pub(super) fn dismiss_lobby_ui(&mut self) -> bool {
        if self.lobby_ui.open.is_none() && self.lobby_ui.editor.is_some() {
            self.cancel_lobby_editor();
        } else if !self.lobby_ui.dismiss() {
            return false;
        }
        self.lobby_editor_controls();
        self.lobby_browser_controls();
        self.lobby_room_controls();
        true
    }

    fn lobby_room_controls(&mut self) {
        if self.page != Page::Lobby || self.lobby_ui.editor.is_some() {
            return;
        }
        let Some(room) = self.lobby_projection() else {
            return;
        };
        let mut items = Vec::new();
        for slot in room.teams.iter().flat_map(|team| &team.slots) {
            match &slot.who {
                None => items.push(Item::Button(Act::LobbySlot(slot.slot))),
                Some(member) if room.owner && !member.me => {
                    items.push(Item::Button(Act::MoveMember(member.account)));
                    items.push(Item::Button(Act::KickMember(member.account)));
                }
                _ => {}
            }
        }
        if room.owner {
            for member in room.spectators.iter().filter(|member| !member.me) {
                items.push(Item::Button(Act::MoveMember(member.account)));
                items.push(Item::Button(Act::KickMember(member.account)));
            }
            items.push(Item::Button(Act::LobbyUi(Action::Edit)));
        }
        items.push(Item::Button(Act::LobbySlot(
            bota_radovid::proto::LOBBY_NO_SLOT,
        )));
        items.push(Item::Button(Act::LeaveLobby));
        let seated = room
            .teams
            .iter()
            .flat_map(|team| &team.slots)
            .any(|slot| slot.who.as_ref().is_some_and(|member| member.me));
        if room.owner && room.starting_in_secs.left().is_some() {
            items.push(Item::Button(Act::ReadyLobby(false)));
        } else if seated
            && room.starting_in_secs.left().is_none()
            && (!room.owner || room.waiting.is_none())
        {
            items.push(Item::Button(Act::ReadyLobby(room.owner || !room.me_ready)));
        }
        if self.lobby_ui.chat.port.is_some() {
            items.push(Item::Button(Act::LobbyUi(Action::FocusChat)));
        }
        self.form.set_controls(items);
    }

    pub(super) fn lobby_browser_controls(&mut self) {
        if self.form.kind != FormKind::Lobby {
            return;
        }
        if let Some(items) = self.lobby_choice_controls() {
            self.form.set_controls(items);
            return;
        }
        let mut items: Vec<_> = self
            .lobbies
            .iter()
            .filter(|lobby| {
                crate::main_menu::lobby_controls::includes_lobby(
                    self.lobby_ui.filter.as_ref(),
                    self.lobby_ui.no_password,
                    &lobby.ruleset,
                    lobby.locked,
                )
            })
            .map(|lobby| Item::Button(Act::JoinLobby(lobby.id)))
            .collect();
        items.extend([
            Item::Button(Act::SearchLobbies),
            Item::Button(Act::LobbyUi(Action::Open(Choice::Filter))),
            Item::Field(SEARCH_FIELD),
            Item::Button(Act::LobbyUi(Action::NoPassword)),
            Item::Button(Act::LobbyUi(Action::Open(Choice::Server))),
            Item::Button(Act::LobbyUi(Action::Open(Choice::Mode))),
            Item::Button(Act::LobbyUi(Action::Create)),
            Item::Button(Act::Go(Page::LocalLobby)),
        ]);
        self.form.set_controls(items);
    }

    /// Makes the lobby at once, named after its host; its name stays, and the mode, password and
    /// audience are edited from the room.
    fn create_lobby_now(&mut self) {
        self.open_lobby_editor(true);
        let Some(nickname) = self
            .client
            .as_ref()
            .and_then(|client| client.me())
            .map(|me| me.nickname.clone())
        else {
            return;
        };
        let name = super::text::line(
            &format!("{nickname}'s lobby"),
            bota_radovid::proto::MAX_LOBBY_NAME_LEN,
        );
        let Some(editor) = &mut self.lobby_ui.editor else {
            return;
        };
        editor.form.put(0, &name);
        self.save_lobby_editor();
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
            if !cfg!(target_arch = "wasm32") {
                self.prefs.host_rules = self
                    .lobby_ui
                    .mode
                    .as_ref()
                    .and_then(|id| LocalRules::named(&id.0))
                    .unwrap_or_default();
                self.save_prefs();
            }
            self.go(Page::LocalLobby);
            return;
        }
        let mode = if creating {
            self.lobby_ui
                .mode
                .clone()
                .or_else(|| self.default_lobby_mode())
        } else {
            self.lobby.as_ref().map(|(state, _)| state.ruleset.clone())
        };
        let Some(mode) = mode else {
            self.tell("No game modes are available".into(), true);
            return;
        };
        self.lobby_ui.editor = Some(self.new_lobby_draft(creating, mode));
        self.lobby_editor_controls();
    }

    fn new_lobby_draft(&mut self, creating: bool, mode: RulesetId) -> Editor {
        let mut form = Form::new(FormKind::LobbySettings, &self.prefs);
        let (audience, cheats, password_known) = if creating {
            (Audience::Everyone, false, true)
        } else {
            self.editor_fields(&mut form)
        };
        self.lobby_ui.next_editor = self
            .lobby_ui
            .next_editor
            .checked_add(1)
            .expect("editor identity exhausted");
        Editor {
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
            saving_until: None,
        }
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
        let locked = self.lobby.as_ref().is_some_and(|(state, _)| state.locked);
        let choices = self.lobby_choice_controls();
        let cheats = self
            .lobby
            .as_ref()
            .is_some_and(|(state, _)| state.cheats.is_some());
        let Some(editor) = &mut self.lobby_ui.editor else {
            return;
        };
        if let Some(items) = choices {
            editor.form.set_controls(items);
            return;
        }
        let mut items = Vec::new();
        if editor.creating {
            items.push(Item::Field(0));
        }
        items.extend([
            Item::Button(Act::LobbyUi(Action::Open(Choice::EditMode))),
            Item::Field(1),
            Item::Button(Act::LobbyUi(Action::Open(Choice::Audience))),
        ]);
        if editor.creating || cheats {
            items.push(Item::Button(Act::LobbyUi(Action::Cheats)));
        }
        items.push(Item::Button(Act::LobbyUi(Action::HidePassword)));
        if !editor.creating && locked {
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
        if !editor.creating
            && self.lobby.as_ref().is_some_and(|(state, _)| state.locked)
            && !editor.remove_password
            && editor.form.value(1).is_empty()
        {
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
            && self.remembered_lobby_password(state.id)
                == password.as_ref().map(|password| password.0.as_str())
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
            self.lobby_secret = self
                .lobby
                .as_ref()
                .and_then(|(state, _)| password.map(|password| (state.id, password)));
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
            editor.saving_until = None;
            editor.error =
                Some("The lobby was saved, but this mode does not support cheats.".into());
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
        let mode = self.lobby_ui.mode.clone().or_else(|| {
            if self.lobby_ui.server == Server::Local {
                Some(RulesetId(LocalRules::default().name().into()))
            } else {
                self.default_lobby_mode()
            }
        });
        let modes = self.lobby_mode_rows(mode.as_ref());
        ControlsView {
            choice_start: self.lobby_ui.choice_start,
            can_host: !cfg!(target_arch = "wasm32"),
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

    fn lobby_mode_rows(&self, mode: Option<&RulesetId>) -> Vec<ModeRow> {
        if self.lobby_ui.server == Server::Local && self.lobby_ui.editor.is_none() {
            LocalRules::ALL
                .into_iter()
                .map(|rules| ModeRow {
                    id: RulesetId(rules.name().into()),
                    name: rules.label().into(),
                    chosen: mode.is_some_and(|id| id.0 == rules.name()),
                })
                .collect()
        } else {
            self.lobby_catalogue()
                .map(|rules| ModeRow {
                    id: rules.id.clone(),
                    name: rules.name.clone(),
                    chosen: Some(&rules.id) == mode,
                })
                .collect()
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
            locked: !editor.creating && self.lobby.as_ref().is_some_and(|(state, _)| state.locked),
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
impl MenuCore {
    /// Pause network delivery while a client-state test injects explicit service events.
    pub(crate) fn test_pause_service_events(&mut self) {
        self.pause_service_events = true;
    }
    pub(crate) fn test_hold_lobby_requests(&mut self) {
        self.lobby_ui.held = Some(Vec::new());
    }
    pub(crate) fn test_lobby_requests(&self) -> &[ClientMsg] {
        self.lobby_ui.held.as_deref().expect("held transport")
    }
    pub(crate) fn test_lobby_reply(&mut self, reply: Reply) {
        self.replied(reply, Asked::Other);
    }
    pub(crate) fn test_lobby_state(&self) -> LobbyState {
        self.lobby.as_ref().expect("room").0.clone()
    }
    pub(crate) fn test_lobby_event(&mut self, state: LobbyState) {
        self.event(Event::LobbyState { state });
    }
    /// Lets `passed` go by for the lobby's countdown, without waiting for it.
    pub(crate) fn test_lobby_counted(&mut self, passed: std::time::Duration) {
        if let Some((_, at)) = &mut self.lobby {
            *at -= passed;
        }
    }
    pub(crate) fn test_lobby_elapsed(&mut self, duration: std::time::Duration) {
        self.poll_lobby_ui_at(Instant::now() + duration);
    }
    pub(crate) fn test_offer_requested(&self) -> bool {
        self.asked
            .iter()
            .any(|(_, asked)| matches!(asked, Asked::Accept(_)))
            || self.view().offer.is_some_and(|offer| offer.answered)
    }
}

#[cfg(test)]
impl MenuCore {
    pub(crate) fn test_lobby_list_len(&self) -> usize {
        self.lobbies.len()
    }
    pub(crate) fn test_lobby_join_buttons(&self) -> Vec<LobbyId> {
        self.form
            .test_buttons()
            .filter_map(|a| {
                if let Act::JoinLobby(id) = a {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect()
    }
    pub(crate) fn test_lobby_rule(&self) -> bota_radovid::proto::RulesetInfo {
        self.client
            .as_ref()
            .expect("client")
            .rulesets()
            .find(|r| r.rating_window.is_some())
            .expect("queueable mode")
            .clone()
    }
    pub(crate) fn test_lobby_poll_views(&mut self) -> usize {
        self.lobby_ui.view_builds.set(0);
        self.poll_lobby_ui();
        self.lobby_ui.view_builds.get()
    }
    pub(crate) fn test_view_builds(&self) -> usize {
        self.lobby_ui.view_builds.get()
    }
    pub(crate) fn test_reset_view_builds(&self) {
        self.lobby_ui.view_builds.set(0);
    }
    pub(crate) fn test_late_join_refusal(&mut self, id: LobbyId) {
        self.refused_request(&Error::BadLobbyPassword, Asked::Join(id));
    }
    pub(crate) fn test_restore_missing(&mut self) {
        self.restore_lobby_membership(&[]);
    }
}

impl MenuCore {
    pub(in crate::main_menu) fn lobby_scroll_state(
        &self,
    ) -> Option<crate::main_menu::lobby_scroll::ScrollState> {
        let page = self.page_beneath();
        if !matches!(page, Page::Lobbies | Page::Lobby) {
            return None;
        }
        Some(crate::main_menu::lobby_scroll::ScrollState {
            page,
            lobby: self.lobby_ui.active,
            open: self.lobby_ui.open,
            start: self.lobby_ui.choice_start,
            editor: self.lobby_ui.editor.is_some(),
            blocked: self.confirm.is_some() || self.join_prompt.is_some() || self.lobby_has_offer(),
        })
    }
}

#[cfg(test)]
impl MenuCore {
    pub(crate) fn test_answer_held_lobby_request(&mut self, reply: Reply) {
        let count = self.lobby_ui.held.as_ref().expect("held transport").len();
        let seq = u32::MAX - (count - 1) as u32;
        self.take(Incoming::Reply {
            seq,
            result: Ok(reply),
        });
    }
}

#[cfg(test)]
impl MenuCore {
    pub(crate) fn test_background_requests(&mut self) {
        for _ in 0..MAX_ASKED {
            self.ask(ClientMsg::MyMatches, Asked::Other);
        }
    }
}

#[cfg(test)]
impl MenuCore {
    pub(crate) fn test_refuse_held_lobby_request(&mut self, error: Error) {
        let count = self.lobby_ui.held.as_ref().expect("held transport").len();
        self.take(Incoming::Reply {
            seq: u32::MAX - (count - 1) as u32,
            result: Err(error),
        });
    }

    pub(crate) fn test_drop_lobby_reply(&mut self) {
        let seq = self
            .asked
            .iter()
            .find(|(_, asked)| matches!(asked, Asked::Join(_) | Asked::LeaveLobby(_)))
            .expect("pending join/leave")
            .0;
        let until = Instant::now() + std::time::Duration::from_secs(30);
        while Instant::now() < until {
            let incoming = self
                .client
                .as_mut()
                .expect("client")
                .wait(std::time::Duration::from_millis(50))
                .expect("control link");
            match incoming {
                Some(Incoming::Reply { seq: answered, .. }) if answered == seq => return,
                Some(incoming) => self.take(incoming),
                None => {}
            }
        }
        panic!("the join/leave was not answered");
    }
}

#[cfg(test)]
impl MenuCore {
    pub(crate) fn test_exhaust_control_retry(&mut self) {
        self.retry = false;
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;

    fn draft(id: u64) -> Editor {
        Editor {
            id,
            lobby: None,
            creating: true,
            form: Form::new(FormKind::LobbySettings, &Prefs::default()),
            mode: RulesetId("full".into()),
            audience: Audience::Everyone,
            cheats: false,
            hidden: true,
            password_known: true,
            password_changed: false,
            remove_password: false,
            error: None,
            saving: None,
            saving_until: None,
        }
    }

    // Deliberately delayed answers exercise an untrusted peer, not a claimed server ordering.
    #[test]
    fn delayed_lobby_save_answers_do_not_change_a_replacement_draft() {
        let mut player = crate::main_menu::tests::Player::new(None);
        let menu = &mut player.menu;
        menu.lobby_ui.editor = Some(draft(1));
        let old = menu
            .mark_lobby_saving(Saving::Create)
            .expect("original request");
        menu.lobby_ui.editor = Some(draft(2));
        menu.mark_lobby_saving(Saving::Create)
            .expect("replacement request");
        for (sequence, result) in [(41, Ok(Reply::Done)), (42, Err(Error::NotLobbyOwner))] {
            menu.asked.push((sequence, Asked::LobbyUi(old)));
            menu.take(Incoming::Reply {
                seq: sequence,
                result,
            });
            let editor = menu.lobby_ui.editor.as_ref().expect("replacement survives");
            assert_eq!(editor.id, 2);
            assert_eq!(editor.saving, Some(Saving::Create));
            assert!(editor.error.is_none());
        }
    }
}
