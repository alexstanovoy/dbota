//! Deadlines and input ownership for lobby operations.
use super::*;
use crate::main_menu::{
    form::Item,
    lobby_controls::{Dialog, SaveRequest, Saving},
};

pub(super) const REQUEST_REACH: std::time::Duration = std::time::Duration::from_secs(15);
pub(super) const CREATE_UNCONFIRMED: &str =
    "Lobby creation was not confirmed. It may still exist; refresh Lobbies before trying again.";
const TIMED_OUT: &str = "The lobby request timed out. Try again.";

pub(super) fn is_lobby_request(message: &ClientMsg) -> bool {
    matches!(
        message,
        ClientMsg::Rulesets
            | ClientMsg::LobbyList
            | ClientMsg::LobbySearch { .. }
            | ClientMsg::LobbyJoin { .. }
            | ClientMsg::LobbyCreate { .. }
            | ClientMsg::LobbyConfigure { .. }
            | ClientMsg::LobbyCheats { .. }
            | ClientMsg::LobbyLeave { .. }
            | ClientMsg::LobbyReady { .. }
            | ClientMsg::LobbySlot { .. }
            | ClientMsg::LobbyKick { .. }
    )
}

impl MenuCore {
    pub(super) fn lobby_request_sent(&mut self, sequence: u32, asked: Asked) {
        if matches!(asked, Asked::Join(_)) {
            self.lobby_ui.uncertain_join = None;
        }
        if matches!(asked, Asked::LobbyUi(request) if request.step == Saving::Create) {
            self.lobby_ui.creation_unconfirmed = true;
        }
        assert!(
            self.lobby_ui.deadlines.len() < MAX_ASKED,
            "request budget checked before send"
        );
        let until = self
            .lobby_ui
            .queued_until
            .unwrap_or_else(|| Instant::now() + REQUEST_REACH);
        self.lobby_ui.deadlines.push((sequence, until, asked));
    }

    pub(super) fn expire_lobby_requests(&mut self, now: Instant) {
        self.expire_queued_lobby_requests(now);
        let expired: Vec<_> = self
            .lobby_ui
            .deadlines
            .iter()
            .filter(|(_, until, _)| *until <= now)
            .map(|(seq, _, asked)| (*seq, *asked))
            .collect();
        self.lobby_ui
            .deadlines
            .retain(|(seq, _, _)| !expired.iter().any(|(expired, _)| seq == expired));
        for (seq, asked) in expired {
            self.asked.retain(|(known, _)| *known != seq);
            self.lobby_request_timed_out(asked);
        }
        let waiting = self
            .lobby_ui
            .editor
            .as_ref()
            .filter(|e| e.saving_until.is_some_and(|until| until <= now))
            .and_then(|e| {
                Some(SaveRequest {
                    editor: e.id,
                    lobby: e.lobby,
                    step: e.saving?,
                })
            });
        if let Some(request) = waiting {
            self.lobby_request_timed_out(Asked::LobbyUi(request));
        }
    }

    fn expire_queued_lobby_requests(&mut self, now: Instant) {
        if self.lobby_ui.queued_until.is_some_and(|until| until <= now) {
            let mut expired = Vec::new();
            self.outbox.retain(|(message, asked)| {
                if is_lobby_request(message) {
                    expired.push(*asked);
                    false
                } else {
                    true
                }
            });
            self.lobby_ui.queued_until = None;
            for asked in expired {
                self.lobby_request_timed_out(asked);
            }
        }
        if !self
            .outbox
            .iter()
            .any(|(message, _)| is_lobby_request(message))
        {
            self.lobby_ui.queued_until = None;
        }
    }

    fn lobby_request_timed_out(&mut self, asked: Asked) {
        if let Asked::Join(lobby) = asked
            && self.page == Page::Lobbies
            && self.lobby_ui.selected == Some(lobby)
        {
            self.lobby_ui.uncertain_join = Some(lobby);
        }
        let reason = if self.lobby_ui.creation_unconfirmed
            && matches!(asked, Asked::LobbyUi(request) if request.step == Saving::Create)
        {
            CREATE_UNCONFIRMED
        } else {
            TIMED_OUT
        };
        self.fail_lobby_request(asked, reason);
    }

    pub(super) fn fail_lobby_request(&mut self, asked: Asked, reason: &str) {
        match asked {
            Asked::LobbyUi(request) if self.lobby_save_current(request) => {
                let editor = self.lobby_ui.editor.as_mut().expect("current draft");
                editor.saving = None;
                editor.saving_until = None;
                editor.error = Some(reason.into());
            }
            Asked::Join(lobby) if self.join_prompt.as_ref().is_some_and(|p| p.lobby == lobby) => {
                self.join_prompt.as_mut().expect("matching prompt").error = Some(reason.into());
            }
            Asked::RestoreLobby(lobby) => self.lobby_resume_failed(lobby, reason.into()),
            Asked::LobbyUi(_) => {}
            _ => self.tell(reason.into(), true),
        }
    }

    pub(super) fn cancel_lobby_editor(&mut self) {
        if let Some(editor) = self.lobby_ui.editor.take() {
            if self.lobby_ui.creation_unconfirmed {
                self.lobby_ui.creation_unconfirmed = false;
                self.tell(CREATE_UNCONFIRMED.into(), true);
            }
            self.asked
                .retain(|(_, asked)| !matches!(asked, Asked::LobbyUi(r) if r.editor == editor.id));
            self.outbox
                .retain(|(_, asked)| !matches!(asked, Asked::LobbyUi(r) if r.editor == editor.id));
            self.lobby_ui.deadlines.retain(
                |(_, _, asked)| !matches!(asked, Asked::LobbyUi(r) if r.editor == editor.id),
            );
        }
        self.lobby_ui.open = None;
        self.lobby_ui.dialog = None;
    }

    pub(super) fn lobby_left(&mut self, lobby: LobbyId) {
        if self.lobby_ui.active != Some(lobby) {
            return;
        }
        self.cancel_lobby_editor();
        self.lobby = None;
        self.lobby_secret = None;
        self.moving = None;
        self.lobby_ui.active = None;
        self.lobby_ui.chat.sync(None, "");
        if self.page == Page::Settings {
            self.settings_from = Page::Lobbies;
        } else if self.page == Page::Lobby {
            self.go_form(Page::Lobbies);
            self.search_lobbies();
        }
    }

    fn live_lobby_offer(&self) -> Option<(ProposalId, bool)> {
        let client = self.client.as_ref()?;
        client
            .searches()
            .entries()
            .iter()
            .filter_map(|search| {
                let offer = search.offer.as_ref()?;
                let left = offer.left_ms();
                let answered =
                    offer.answered || self.accepted.contains(search.entry, offer.proposal);
                (left > 0).then_some((offer.proposal, answered, left))
            })
            .min_by_key(|(_, answered, left)| (*answered, *left))
            .map(|(proposal, answered, _)| (proposal, answered))
    }

    pub(super) fn lobby_has_offer(&self) -> bool {
        self.live_lobby_offer().is_some()
    }

    pub(super) fn sync_lobby_dialog(&mut self) {
        let dialog = self.lobby_ui.editor.as_ref().and_then(|_| {
            self.live_lobby_offer()
                .map(|(id, answered)| Dialog::Offer(id, answered))
                .or_else(|| self.confirm.as_ref().map(|_| Dialog::Confirm))
        });
        if self.lobby_ui.dialog == dialog {
            return;
        }
        self.lobby_ui.dialog = dialog;
        let mut form = Form::new(FormKind::None, &self.prefs);
        let buttons = match dialog {
            Some(Dialog::Confirm) => vec![Act::Answer(true), Act::Answer(false)],
            Some(Dialog::Offer(id, false)) => vec![Act::Accept(id), Act::Decline(id)],
            _ => Vec::new(),
        };
        form.set_controls(buttons.into_iter().map(Item::Button).collect());
        self.lobby_ui.dialog_form = form;
    }

    pub(super) fn route_lobby_overlay(&mut self, act: &Act) -> bool {
        self.sync_lobby_dialog();
        if !self.lobby_editor_obscured() {
            return false;
        }
        match act {
            Act::Submit => {
                if self.join_prompt.is_some() && self.lobby_ui.dialog.is_none() {
                    self.send_lobby_password();
                } else if let Some(button) = self.lobby_ui.dialog_form.focused_button().cloned() {
                    assert!(self.dispatch(button).is_none(), "dialog action handled");
                }
            }
            Act::NextField => self.keyboard().next(),
            Act::PreviousField => self.keyboard().previous(),
            Act::Back => {
                if matches!(self.lobby_ui.dialog, Some(Dialog::Offer(..))) {
                    return true;
                }
                if self.confirm.take().is_none() {
                    self.cancel_lobby_password();
                }
            }
            Act::Focus(_) | Act::Type(_) | Act::Erase
                if self.join_prompt.is_some() && self.lobby_ui.dialog.is_none() =>
            {
                assert!(
                    self.dispatch(act.clone()).is_none(),
                    "password input handled"
                );
            }
            Act::Focus(_) | Act::Type(_) | Act::Erase | Act::LobbyUi(_) => {}
            _ => return false,
        }
        true
    }
}
