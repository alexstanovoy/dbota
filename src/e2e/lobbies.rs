//! Lobby screens through the app's laid-out and drawn state.
use super::harness::Client;
use bevy::prelude::*;

#[test]
fn lobby_browser_draws_the_four_column_list_in_the_right_drawer() {
    let mut client = Client::run(&["--menu-preview", "lobbies"]);
    client.frames(8);
    let world = client.world_mut();
    let size = world
        .query::<(&Name, &ComputedNode)>()
        .iter(world)
        .find(|(name, _)| name.as_str() == "lobby browser")
        .map(|(_, node)| node.size() * node.inverse_scale_factor)
        .expect("lobby browser drawer");
    assert_eq!(size.x, 620.0);
    let texts = client.shown_texts();
    for text in [
        "NAME",
        "HOST",
        "SLOTS",
        "MODE",
        "FILTER LOBBIES",
        "CREATE A LOBBY",
    ] {
        assert!(texts.iter().any(|shown| shown == text), "{text}");
    }
}

#[test]
fn lobby_room_draws_two_team_columns_and_only_highlights_the_host_in_chat() {
    let mut client = Client::run(&["--menu-preview", "lobby-owner"]);
    client.frames(8);
    let world = client.world_mut();
    let colors: Vec<_> = world
        .query::<(&Name, &TextColor)>()
        .iter(world)
        .filter(|(name, _)| name.as_str().starts_with("lobby chat speaker"))
        .map(|(_, ink)| ink.0)
        .collect();
    assert!(colors.len() >= 3, "drawn lobby conversation");
    assert!(
        colors.contains(&crate::theme::GOLD),
        "the host is highlighted"
    );
    assert!(
        colors.contains(&crate::theme::INK_1),
        "other players are white"
    );
    assert!(
        colors
            .iter()
            .all(|ink| *ink == crate::theme::GOLD || *ink == crate::theme::INK_1)
    );
    let teams: Vec<_> = world
        .query::<(&Name, &UiGlobalTransform)>()
        .iter(world)
        .filter(|(name, _)| name.as_str().starts_with("lobby team "))
        .map(|(name, transform)| (name.as_str(), transform.translation))
        .collect();
    let radiant = teams
        .iter()
        .find(|(name, _)| *name == "lobby team RADIANT")
        .expect("Radiant")
        .1;
    let dire = teams
        .iter()
        .find(|(name, _)| *name == "lobby team DIRE")
        .expect("Dire")
        .1;
    assert_eq!(teams.len(), 2);
    assert!(dire.x > radiant.x);
    assert!((dire.y - radiant.y).abs() < 1.0);
}

#[test]
fn the_lobby_password_dialog_draws_the_hide_password_control() {
    let mut client = Client::run(&["--menu-preview", "lobby-password"]);
    client.frames(8);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Hide password")
    );
}

#[test]
fn lobby_ready_chips_and_footer_always_report_the_same_flags() {
    let mut client = Client::run(&["--menu-preview", "lobby-owner"]);
    for (ready, message) in [
        (2, "2 of 5 seated players are ready."),
        (0, "0 of 5 seated players are ready."),
        (5, "All 5 players are ready."),
    ] {
        if ready != 2 {
            let mut preview = client
                .world_mut()
                .resource_mut::<crate::main_menu::Preview>();
            if let crate::main_menu::PageView::Lobby(lobby) = &mut preview.0.body {
                for member in lobby
                    .teams
                    .iter_mut()
                    .flat_map(|team| &mut team.slots)
                    .filter_map(|slot| slot.who.as_mut())
                {
                    member.ready = ready == 5;
                }
            }
        }
        client.frames(4);
        assert!(client.shown_texts().iter().any(|text| text == message));
        let world = client.world_mut();
        assert_eq!(
            world
                .query::<&Name>()
                .iter(world)
                .filter(|name| name.as_str().starts_with("lobby ready chip "))
                .count(),
            ready
        );
    }
}

use super::menu::{act, click_menu_button as click, signed_up, visible_menu_button as named};
use crate::main_menu::{Act, LobbyAction, LobbyView, MenuCore, Page, PageView};
use bota_radovid::proto::LobbyId;

fn room(client: &Client) -> LobbyView {
    let PageView::Lobby(lobby) = client.world().non_send::<MenuCore>().view().body else {
        panic!("lobby room");
    };
    lobby
}

fn create(client: &mut Client, name: &str, password: &str) {
    act(client, Act::Go(Page::Lobbies));
    click(client, "create lobby");
    click(client, "lobby name");
    act(client, Act::Type(name.into()));
    click(client, "password");
    act(client, Act::Type(password.into()));
    click(client, "ok");
    client.until("the lobby created", |client| {
        let view = client.world().non_send::<MenuCore>().view();
        matches!(view.body, PageView::Lobby(_)) && view.lobby_ui.editor.is_none()
    });
}

fn join(client: &mut Client, name: &str, password: &str) {
    act(client, Act::Go(Page::Lobbies));
    let button = format!("join {}", name.to_lowercase());
    client.until("the protected lobby listed", |client| {
        named(client, &button)
    });
    click(client, &button);
    act(client, Act::Type(password.into()));
    click(client, "ok");
    client.until("joined immediately without approval", |client| {
        matches!(
            client.world().non_send::<MenuCore>().view().body,
            PageView::Lobby(_)
        )
    });
}

fn select_mode(client: &mut Client, name: &str) {
    click(client, "game mode");
    click(client, &name.to_lowercase());
}

#[test]
fn protected_lobby_search_filters_password_retry_and_cancelled_settings_follow_the_real_service() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "Friday night", "open sesame");
    let mut bob = signed_up(&service, "bob");
    act(&mut bob, Act::Go(Page::Lobbies));
    bob.until("the protected lobby listed", |client| {
        named(client, "join friday night")
    });
    click(&mut bob, "no password");
    assert!(!named(&mut bob, "join friday night"));
    click(&mut bob, "no password");
    click(&mut bob, "lobby or host name");
    act(&mut bob, Act::Type("FRIDAY".into()));
    bob.until("the matching lobby remains", |client| {
        named(client, "join friday night")
    });
    click(&mut bob, "join friday night");
    act(&mut bob, Act::Type("wrong".into()));
    click(&mut bob, "ok");
    bob.until("the password refusal drawn", |client| {
        client
            .shown_texts()
            .iter()
            .any(|text| text.starts_with("Wrong password"))
    });
    act(&mut bob, Act::Type("open sesame".into()));
    click(&mut bob, "ok");
    bob.until("password joins without an approval step", |client| {
        matches!(
            client.world().non_send::<MenuCore>().view().body,
            PageView::Lobby(_)
        )
    });
    assert!(!named(&mut bob, "edit  ✎"));
    alice.until("the guest seated", |client| {
        room(client)
            .teams
            .iter()
            .flat_map(|team| &team.slots)
            .any(|slot| slot.who.as_ref().is_some_and(|member| member.name == "bob"))
    });
    cancelled_settings(&mut alice, &mut bob);
    saved_settings(&mut alice, &mut bob);
}

fn cancelled_settings(alice: &mut Client, bob: &mut Client) {
    click(alice, "edit  ✎");
    select_mode(alice, "1x1 Mid Only");
    click(alice, "cancel");
    click(alice, "move bob");
    click(alice, "join slot 9");
    alice.until(
        "a later seat change acknowledges the cancellation barrier",
        |client| {
            bob.frames(1);
            room(client)
                .teams
                .iter()
                .flat_map(|team| &team.slots)
                .any(|slot| {
                    slot.slot == 9 && slot.who.as_ref().is_some_and(|member| member.name == "bob")
                })
        },
    );
    bob.until("the moved seat propagated", |client| {
        room(client)
            .teams
            .iter()
            .flat_map(|team| &team.slots)
            .any(|slot| slot.slot == 9 && slot.who.as_ref().is_some_and(|member| member.me))
    });
    assert_eq!(room(alice).settings.mode, "Full rules");
    assert_eq!(room(bob).settings.mode, "Full rules");
    assert!(room(bob).locked);
}

fn saved_settings(alice: &mut Client, bob: &mut Client) {
    click(alice, "edit  ✎");
    let before = alice
        .world()
        .non_send::<MenuCore>()
        .view()
        .lobby_ui
        .editor
        .expect("editor")
        .fields[0]
        .shown
        .clone();
    act(alice, Act::Focus(0));
    act(alice, Act::Type("changed".into()));
    assert_eq!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .expect("editor")
            .fields[0]
            .shown,
        before,
        "existing name is fixed"
    );
    select_mode(alice, "1x1 Mid Only");
    click(alice, "ok");
    alice.until("settings saved", |client| {
        bob.frames(1);
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_none()
            && room(client).settings.mode == "1x1 Mid Only"
    });
    bob.until("mode change propagated", |client| {
        room(client).settings.mode == "1x1 Mid Only"
    });
    for lobby in [room(alice), room(bob)] {
        assert_eq!(
            lobby
                .teams
                .iter()
                .map(|team| team.slots.len())
                .sum::<usize>(),
            10,
            "solo-mid changes rules, not capacity"
        );
        assert!(
            lobby.locked,
            "saving another setting preserves the password"
        );
        assert!(!lobby.me_ready, "settings changes clear readiness");
    }
}

#[derive(Default)]
struct Mailbox {
    sent: Vec<(LobbyId, String)>,
    incoming: std::collections::VecDeque<crate::main_menu::LobbyChatMessage>,
    reject: bool,
}
struct ChatPort(std::sync::Arc<std::sync::Mutex<Mailbox>>);
impl crate::main_menu::LobbyChatPort for ChatPort {
    fn send(&mut self, lobby: LobbyId, message: &str) -> Result<(), String> {
        let mut mailbox = self.0.lock().expect("mailbox");
        if mailbox.reject {
            return Err("The message could not be sent".into());
        }
        mailbox.sent.push((lobby, message.into()));
        Ok(())
    }
    fn receive(&mut self) -> Option<crate::main_menu::LobbyChatMessage> {
        self.0.lock().expect("mailbox").incoming.pop_front()
    }
}

#[test]
fn lobby_chat_uses_the_client_seam_has_no_fake_echo_and_preserves_a_refused_draft() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "Conversation", "");
    let mailbox = std::sync::Arc::new(std::sync::Mutex::new(Mailbox::default()));
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .set_lobby_chat_port(Box::new(ChatPort(mailbox.clone())));
    alice.frames(4);
    click(&mut alice, "lobby message");
    act(&mut alice, Act::Type("gl hf".into()));
    act(&mut alice, Act::Submit);
    assert_eq!(mailbox.lock().expect("mailbox").sent.len(), 1);
    assert!(
        !alice.shown_texts().iter().any(|text| text == "gl hf"),
        "only delivered messages enter the conversation"
    );
    mailbox.lock().expect("mailbox").reject = true;
    act(&mut alice, Act::Type("keep this".into()));
    act(&mut alice, Act::Submit);
    assert!(
        alice
            .shown_texts()
            .iter()
            .any(|text| text == "The message could not be sent")
    );
    assert_eq!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .chat
            .input,
        "keep this"
    );
    let (lobby, _) = mailbox.lock().expect("mailbox").sent[0].clone();
    let owner = alice
        .world()
        .non_send::<MenuCore>()
        .view()
        .lobby_ui
        .chat
        .owner
        .expect("owner");
    mailbox
        .lock()
        .expect("mailbox")
        .incoming
        .push_back(crate::main_menu::LobbyChatMessage {
            lobby,
            account: owner,
            name: "alice".into(),
            message: "gl hf".into(),
            minute: 1264,
        });
    alice.until("delivered message drawn", |client| {
        client.shown_texts().iter().any(|text| text == "gl hf")
    });
    act(&mut alice, Act::LeaveLobby);
    alice.until("left the lobby", |client| {
        matches!(
            client.world().non_send::<MenuCore>().view().body,
            PageView::Lobbies { .. }
        )
    });
    alice.frames(2);
    let chat = alice.world().non_send::<MenuCore>().view().lobby_ui.chat;
    assert!(
        chat.lines.is_empty() && chat.input.is_empty(),
        "no conversation survives leaving its lobby"
    );
}

#[test]
fn the_host_start_button_is_disabled_with_the_live_wait_reason() {
    let mut client = Client::run(&["--menu-preview", "lobby-owner"]);
    client.frames(8);
    let world = client.world_mut();
    let entity = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "start game")
        .map(|(entity, _)| entity)
        .expect("host's START GAME");
    assert!(world.get::<crate::main_menu::MenuButton>(entity).is_none());
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Waiting for 2 players to be ready")
    );
}

fn replace_password(client: &mut Client, value: &str) {
    click(client, "password");
    let count = client
        .world()
        .non_send::<MenuCore>()
        .view()
        .lobby_ui
        .editor
        .expect("editor")
        .fields[1]
        .shown
        .chars()
        .count();
    for _ in 0..count {
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .act(Act::Erase);
    }
    act(client, Act::Type(value.into()));
}

#[test]
fn saving_freezes_the_draft_until_acknowledged_and_keeps_the_committed_password() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "Saved settings", "old password");
    click(&mut alice, "edit  ✎");
    replace_password(&mut alice, "new password");
    {
        let mut core = alice.world_mut().non_send_mut::<MenuCore>();
        core.act(Act::LobbyUi(LobbyAction::Save));
        assert!(core.view().lobby_ui.editor.expect("saving editor").saving);
        core.act(Act::Type("corrupt".into()));
        core.act(Act::LobbyUi(LobbyAction::Cancel));
        let editor = core
            .view()
            .lobby_ui
            .editor
            .expect("an in-flight save cannot be cancelled");
        assert_eq!(
            editor.fields[1].shown.chars().count(),
            "new password".chars().count()
        );
    }
    alice.until("password save acknowledged", |client| {
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_none()
    });
    click(&mut alice, "edit  ✎");
    select_mode(&mut alice, "1x1 Mid Only");
    click(&mut alice, "ok");
    alice.until("a subsequent edit saved", |client| {
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_none()
    });
    let mut bob = signed_up(&service, "bob");
    join(&mut bob, "Saved settings", "new password");
    assert_eq!(room(&bob).settings.mode, "1x1 Mid Only");
}

#[test]
fn saved_settings_close_on_the_real_done_reply_after_the_state_notification() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "Save acknowledgement", "");
    click(&mut alice, "edit  ✎");
    select_mode(&mut alice, "1x1 Mid Only");
    click(&mut alice, "ok");
    alice.until("the service applied the mode", |client| {
        room(client).settings.mode == "1x1 Mid Only"
    });
    // A later legal mutation on the same connection is the barrier for the earlier Done reply.
    act(&mut alice, Act::LobbySlot(1));
    alice.until("the later seat update arrived", |client| {
        room(client)
            .teams
            .iter()
            .flat_map(|team| &team.slots)
            .any(|slot| slot.slot == 1 && slot.who.as_ref().is_some_and(|member| member.me))
    });
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_none(),
        "Done completes the saved draft"
    );
    let world = alice.world_mut();
    assert!(
        !world
            .query::<(&Name, &InheritedVisibility)>()
            .iter(world)
            .any(|(name, visible)| visible.get() && name.as_str() == "lobby settings popup")
    );
}
