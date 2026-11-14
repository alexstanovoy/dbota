//! Lobby screens through the app's laid-out and drawn state.
use super::harness::Client;
use bevy::prelude::*;

#[test]
fn lobby_browser_draws_the_four_column_list_in_the_right_drawer() {
    let mut client = Client::run(&["--menu-preview", "lobbies"]);
    client.frames(8);
    let world = client.world_mut();
    let node = world
        .query::<(&Name, &ComputedNode)>()
        .iter(world)
        .find(|(name, _)| name.as_str() == "lobby browser")
        .map(|(_, node)| *node)
        .expect("lobby browser drawer");
    super::harness::assert_physical_width(&node, 620.0);
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
fn the_lobby_browser_floats_no_picture_in_the_scene_beside_its_drawer() {
    let mut client = Client::run(&["--menu-preview", "lobbies"]);
    client.frames(8);
    let world = client.world_mut();
    let (drawer, laid) = world
        .query::<(&Name, &UiGlobalTransform, &ComputedNode)>()
        .iter(world)
        .find(|(name, ..)| name.as_str() == "lobby browser")
        .map(|(_, at, node)| (*at, *node))
        .expect("lobby browser drawer");
    let edge = drawer.translation.x - laid.size().x / 2.0;
    let pictures: Vec<_> = world
        .query::<(
            &ImageNode,
            &UiGlobalTransform,
            &ComputedNode,
            &InheritedVisibility,
        )>()
        .iter(world)
        .filter(|(image, _, node, shown)| {
            shown.get() && node.size().min_element() > 0.0 && image.image != Handle::default()
        })
        .map(|(_, at, node, _)| (at.translation, node.size()))
        .filter(|(at, size)| size.min_element() >= 100.0 && at.x + size.x / 2.0 <= edge)
        .collect();
    assert!(
        pictures.is_empty(),
        "large pictures drawn left of the drawer (x < {edge}): {pictures:?}"
    );
}

fn open_lobby_browser_from_play(client: &mut Client) {
    click(client, "play tab");
    click(client, "custom lobbies");
    click(client, "browse lobbies");
    client.until("the lobby browser open", |client| {
        client.world().non_send::<MenuCore>().page() == Page::Lobbies
    });
}

#[test]
fn the_top_bar_has_no_lobbies_tab_because_lobbies_open_from_play() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "bob");
    assert!(
        !named(&mut client, "lobbies"),
        "Play's Custom Lobbies section is the way in"
    );
    open_lobby_browser_from_play(&mut client);
    assert!(
        !named(&mut client, "lobbies"),
        "the browser opened from Play brings no tab of its own"
    );
}

#[test]
fn the_lobby_browser_has_no_back_button() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "bob");
    open_lobby_browser_from_play(&mut client);
    let texts = client.shown_texts();
    assert!(
        !texts.iter().any(|text| text.contains("BACK")),
        "a click beside the browser and Escape leave it: {texts:?}"
    );
}

#[test]
fn a_click_beside_the_lobby_browser_returns_to_play_with_custom_lobbies_open() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "bob");
    open_lobby_browser_from_play(&mut client);
    client.hover_ui_with(std::time::Duration::from_secs(30), |client| {
        let world = client.world_mut();
        world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, name)| name.as_str() == "lobby browser backdrop")
            .map(|(entity, _)| entity)
    });
    client.click(MouseButton::Left);
    client.frames(3);
    let view = client.world().non_send::<MenuCore>().view();
    assert_eq!(view.page, Page::Home, "back on Play's page");
    assert!(view.play.open, "with the drawer open");
    assert_eq!(
        view.play.section,
        crate::main_menu::PlaySection::CustomLobbies
    );
}

#[test]
fn create_lobby_on_play_makes_the_room_at_once_without_a_dialog_in_between() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    click(&mut client, "play tab");
    click(&mut client, "custom lobbies");
    click(&mut client, "create lobby");
    let mut dialog_drawn = false;
    client.until("the lobby room", |client| {
        let view = client.world().non_send::<MenuCore>().view();
        dialog_drawn |= view.lobby_ui.editor.is_some() && named(client, "lobby name");
        matches!(view.body, PageView::Lobby(_))
    });
    assert!(
        !dialog_drawn,
        "no window asks for the lobby's details first"
    );
    assert_eq!(room(&client).name, ALICES_LOBBY, "named after its host");
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
    let footer: Vec<_> = world
        .query::<(&Name, &ComputedNode)>()
        .iter(world)
        .filter(|(name, _)| name.as_str() == "leave lobby  ×" || name.as_str() == "start game")
        .map(|(_, node)| {
            (
                node.size() * node.inverse_scale_factor(),
                node.inverse_scale_factor(),
            )
        })
        .collect();
    assert_eq!(footer.len(), 2);
    assert!(
        (footer[0].0.x - footer[1].0.x).abs() < 1.0,
        "the footer's two actions have equal width"
    );
    assert!(
        footer
            .iter()
            .all(|(size, pixel)| (size.y - 52.0).abs() <= *pixel),
        "footer sizes: {footer:?}"
    );
}

#[test]
fn the_lobby_password_dialog_draws_the_hide_password_control() {
    let mut client = Client::run(&["--menu-preview", "lobby-password"]);
    client.within(
        std::time::Duration::from_secs(30),
        "the hide-password control drawn",
        |client| {
            client
                .shown_texts()
                .iter()
                .any(|text| text == "Hide password")
        },
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

/// What a lobby made at once is called: its host's name decides it, and it cannot be edited yet.
const ALICES_LOBBY: &str = "alice's lobby";

/// Alice makes a lobby at once, and sets its password from the room when she wants one.
fn create(client: &mut Client, password: &str) {
    act(client, Act::Go(Page::Lobbies));
    click(client, "create lobby");
    client.until("the lobby created", |client| {
        let view = client.world().non_send::<MenuCore>().view();
        matches!(view.body, PageView::Lobby(_)) && view.lobby_ui.editor.is_none()
    });
    if password.is_empty() {
        return;
    }
    click(client, "edit  ✎");
    click(client, "password");
    act(client, Act::Type(password.into()));
    click(client, "ok");
    client.until("the password saved", |client| {
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_none()
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
    click(client, "lobby game mode");
    click(client, &name.to_lowercase());
}

#[test]
fn protected_lobby_search_filters_password_retry_and_cancelled_settings_follow_the_real_service() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "open sesame");
    let mut bob = signed_up(&service, "bob");
    act(&mut bob, Act::Go(Page::Lobbies));
    bob.until("the protected lobby listed", |client| {
        named(client, "join alice's lobby")
    });
    click(&mut bob, "no password");
    assert!(!named(&mut bob, "join alice's lobby"));
    click(&mut bob, "no password");
    click(&mut bob, "lobby or host name");
    act(&mut bob, Act::Type("ALICE".into()));
    bob.until("the matching lobby remains", |client| {
        named(client, "join alice's lobby")
    });
    click(&mut bob, "join alice's lobby");
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
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .expect("draft")
            .password_unknown,
        "a guest update invalidates the cached password"
    );
    replace_password(alice, "open sesame");
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
        assert_solo_settings(lobby);
    }
}

fn assert_solo_settings(lobby: LobbyView) {
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

#[derive(Default)]
struct Mailbox {
    sent: Vec<(LobbyId, String)>,
    incoming: std::collections::VecDeque<crate::main_menu::LobbyChatMessage>,
    reject: bool,
}
type SharedMailbox = std::sync::Arc<std::sync::Mutex<Mailbox>>;
struct ChatPort(SharedMailbox);
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
    create(&mut alice, "");
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
    assert_refused_chat_draft(&mut alice, &mailbox);
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
            message: "gl hf".into(),
            minute: 1264,
        });
    alice.until("delivered message drawn", |client| {
        client.shown_texts().iter().any(|text| text == "gl hf")
    });
    assert_chat_membership(&mut alice, &mailbox, lobby, owner);
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

fn assert_refused_chat_draft(alice: &mut Client, mailbox: &SharedMailbox) {
    mailbox.lock().expect("mailbox").reject = true;
    act(alice, Act::Type("keep this".into()));
    act(alice, Act::Submit);
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
}

fn assert_chat_membership(
    alice: &mut Client,
    mailbox: &SharedMailbox,
    lobby: LobbyId,
    owner: bota_radovid::AccountId,
) {
    {
        let mut box_ = mailbox.lock().expect("mailbox");
        for (room, account, message) in [
            (LobbyId(lobby.0.wrapping_add(1)), owner, "wrong room"),
            (lobby, bota_radovid::AccountId(u64::MAX), "outsider"),
            (lobby, owner, "authenticated name"),
        ] {
            box_.incoming.push_back(crate::main_menu::LobbyChatMessage {
                lobby: room,
                account,
                message: message.into(),
                minute: 1264,
            });
        }
    }
    alice.frames(3);
    let chat = alice.world().non_send::<MenuCore>().view().lobby_ui.chat;
    assert!(
        !chat
            .lines
            .iter()
            .any(|line| line.message == "wrong room" || line.message == "outsider")
    );
    let delivered = chat
        .lines
        .iter()
        .find(|line| line.message == "authenticated name")
        .expect("member message");
    assert_eq!(
        delivered.name, "alice",
        "chat uses the known member identity instead of a spoofed label"
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
fn saving_freezes_the_draft_and_password_edits_survive_another_settings_save() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "old password");
    click(&mut alice, "edit  ✎");
    replace_password(&mut alice, "new password");
    {
        let mut core = alice.world_mut().non_send_mut::<MenuCore>();
        core.act(Act::LobbyUi(LobbyAction::Save));
        assert!(core.view().lobby_ui.editor.expect("saving editor").saving);
        core.act(Act::Type("corrupt".into()));
        let editor = core
            .view()
            .lobby_ui
            .editor
            .expect("the draft stays frozen until acknowledged");
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
    replace_password(&mut alice, "new password");
    click(&mut alice, "ok");
    alice.until("a subsequent edit saved", |client| {
        let view = client.world().non_send::<MenuCore>().view();
        if let Some(editor) = &view.lobby_ui.editor {
            assert!(editor.error.is_none(), "save refused: {:?}", editor.error);
        }
        view.lobby_ui.editor.is_none()
    });
    let mut bob = signed_up(&service, "bob");
    join(&mut bob, ALICES_LOBBY, "new password");
    assert_eq!(room(&bob).settings.mode, "1x1 Mid Only");
}

#[test]
fn saved_settings_close_on_the_real_done_reply_after_the_state_notification() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
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

fn control_reply(client: &mut bota_radovid::Client, sequence: u32) -> bota_radovid::proto::Reply {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < until {
        if let Some(bota_radovid::Incoming::Reply { seq, result }) = client
            .wait(std::time::Duration::from_millis(20))
            .expect("control link")
            && seq == sequence
        {
            return result.expect("request accepted");
        }
    }
    panic!("the control request was not answered");
}

#[test]
fn another_owner_session_cannot_leave_a_stale_password_in_an_open_draft() {
    use bota_radovid::proto::{Audience, ClientMsg, Reply, RulesetId, Secret};
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "alpha secret");
    click(&mut alice, "edit  ✎");
    let mut other = crate::main_menu::tests::keep_menu_session_alive(
        alice.world().non_send::<MenuCore>(),
        &service,
    );
    let sequence = other.request(ClientMsg::LobbyList).expect("list request");
    let Reply::Lobbies { list } = control_reply(&mut other, sequence) else {
        panic!("lobby list")
    };
    let lobby = list
        .iter()
        .find(|row| row.name == ALICES_LOBBY)
        .expect("lobby")
        .id;
    let sequence = other
        .request(ClientMsg::LobbyConfigure {
            lobby,
            ruleset: RulesetId("full".into()),
            audience: Audience::Everyone,
            password: Some(Secret("beta secret".into())),
        })
        .expect("configure request");
    assert!(matches!(control_reply(&mut other, sequence), Reply::Done));
    // The slot update on the same control link is a barrier after the password notification.
    let sequence = other
        .request(ClientMsg::LobbySlot {
            lobby,
            account: None,
            slot: 1,
        })
        .expect("slot request");
    assert!(matches!(control_reply(&mut other, sequence), Reply::Done));
    alice.until("the later state reached the editor", |c| {
        room(c)
            .teams
            .iter()
            .flat_map(|t| &t.slots)
            .any(|s| s.slot == 1 && s.who.as_ref().is_some_and(|p| p.me))
    });
    let editor = alice
        .world()
        .non_send::<MenuCore>()
        .view()
        .lobby_ui
        .editor
        .expect("editor");
    assert!(
        editor.password_unknown,
        "a second session may have changed the password"
    );
    assert!(
        editor.fields[1].shown.is_empty(),
        "the stale password cannot be silently sent back"
    );
}

#[test]
fn a_confirmation_owns_enter_and_escape_above_a_lobby_draft() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    click(&mut alice, "edit  ✎");
    select_mode(&mut alice, "1x1 Mid Only");
    act(&mut alice, Act::Focus(1));
    act(&mut alice, Act::Quit);
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .confirm
            .is_some()
    );
    alice.tap_named(KeyCode::Escape, bevy::input::keyboard::Key::Escape);
    let view = alice.world().non_send::<MenuCore>().view();
    assert!(view.confirm.is_none(), "Escape closes the top confirmation");
    assert!(
        view.lobby_ui.editor.is_some(),
        "Escape preserves the lower editor"
    );
    act(&mut alice, Act::Quit);
    alice.tap_named(KeyCode::Enter, bevy::input::keyboard::Key::Enter);
    let view = alice.world().non_send::<MenuCore>().view();
    assert!(view.confirm.is_none(), "Enter answers the top confirmation");
    assert!(
        !view
            .lobby_ui
            .editor
            .expect("the hidden draft remains")
            .saving
    );
    let exits = alice
        .world()
        .resource::<bevy::ecs::message::Messages<bevy::app::AppExit>>();
    assert!(
        exits
            .get_cursor()
            .read(exits)
            .any(|exit| matches!(exit, bevy::app::AppExit::Success)),
        "Enter confirmed Quit rather than cancelling it"
    );
}

#[test]
fn lobby_dropdown_options_can_be_selected_with_tab_and_enter() {
    use bevy::input::keyboard::Key;
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    act(&mut alice, Act::Go(Page::Lobbies));
    act(
        &mut alice,
        Act::LobbyUi(LobbyAction::Open(crate::main_menu::LobbyChoice::Mode)),
    );
    for _ in 0..12 {
        if matches!(alice.world().non_send::<MenuCore>().button_focus(), Some(Act::LobbyUi(LobbyAction::Mode(mode))) if mode.0 == "solo_mid")
        {
            let world = alice.world_mut();
            assert!(world.query::<(&crate::main_menu::MenuButton, &Outline)>().iter(world).any(|(button, _)| matches!(&button.0, Act::LobbyUi(LobbyAction::Mode(mode)) if mode.0 == "solo_mid")), "keyboard selection has a drawn focus outline");
            alice.tap_named(KeyCode::Enter, Key::Enter);
            assert_eq!(
                alice
                    .world()
                    .non_send::<MenuCore>()
                    .view()
                    .lobby_ui
                    .mode
                    .expect("mode")
                    .0,
                "solo_mid"
            );
            return;
        }
        alice.tap_named(KeyCode::Tab, Key::Tab);
    }
    panic!("the dropdown's options must be in the keyboard order");
}

#[test]
fn the_lobby_list_scrolls_under_the_pointer_and_keeps_its_position_after_redraw() {
    let mut client = Client::run(&["--menu-preview", "lobbies"]);
    {
        let mut preview = client
            .world_mut()
            .resource_mut::<crate::main_menu::Preview>();
        preview.0.lobby_ui.open = None;
        if let PageView::Lobbies { list, .. } = &mut preview.0.body {
            let template = list[0].clone();
            for index in 0..30 {
                let mut row = template.clone();
                row.id = LobbyId(100 + index);
                row.name = format!("Extra room {index}");
                list.push(row);
            }
        }
    }
    client.frames(6);
    let world = client.world_mut();
    let row = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, n)| n.as_str() == "join friday stack")
        .expect("first room")
        .0;
    let table = world.get::<ChildOf>(row).expect("table parent").parent();
    let centre = world
        .get::<UiGlobalTransform>(table)
        .expect("table position")
        .translation;
    let before = world
        .get::<UiGlobalTransform>(row)
        .expect("row position")
        .translation
        .y;
    scroll_at(&mut client, centre, -5.0, 4);
    let after = client
        .world()
        .get::<UiGlobalTransform>(row)
        .expect("scrolled row")
        .translation
        .y;
    assert!(
        after < before - 20.0,
        "the wheel moves the actual list rows"
    );
    client
        .world_mut()
        .resource_mut::<crate::main_menu::Preview>()
        .0
        .lobby_ui
        .selected = Some(LobbyId(101));
    client.frames(4);
    let world = client.world_mut();
    let scrolled = world
        .query::<&ScrollPosition>()
        .iter(world)
        .any(|at| at.y > 20.0);
    assert!(
        scrolled,
        "redrawing a filter preserves the list scroll position"
    );
}

#[test]
fn the_host_can_move_and_kick_a_spectating_member() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "secret");
    let mut bob = signed_up(&service, "bob");
    join(&mut bob, ALICES_LOBBY, "secret");
    click(&mut bob, "spectate");
    alice.until("the guest spectating", |c| {
        room(c).spectators.iter().any(|p| p.name == "bob")
    });
    assert!(
        named(&mut alice, "move bob"),
        "spectators retain the owner's move control"
    );
    click(&mut alice, "move bob");
    click(&mut alice, "join slot 9");
    alice.until("the spectator moved into a slot", |c| {
        room(c)
            .teams
            .iter()
            .flat_map(|t| &t.slots)
            .any(|s| s.slot == 9 && s.who.as_ref().is_some_and(|p| p.name == "bob"))
    });
    click(&mut alice, "kick bob");
    bob.until("the guest kicked", |c| {
        matches!(
            c.world().non_send::<MenuCore>().view().body,
            PageView::Lobbies { .. }
        )
    });
}

#[test]
fn selecting_local_hosting_preserves_the_selected_game_mode() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    act(&mut alice, Act::Go(Page::Lobbies));
    click(&mut alice, "server");
    click(&mut alice, "local");
    act(
        &mut alice,
        Act::LobbyUi(LobbyAction::Mode(bota_radovid::RulesetId(
            "solo_mid".into(),
        ))),
    );
    click(&mut alice, "create lobby");
    let PageView::Form {
        rules: Some(rules), ..
    } = alice.world().non_send::<MenuCore>().view().body
    else {
        panic!("the existing local setup")
    };
    assert_eq!(
        rules.name(),
        "solo_mid",
        "the selected local mode is applied to hosting"
    );
}

#[test]
fn lobby_chat_without_transport_is_drawn_disabled() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    let world = alice.world_mut();
    let input = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, n)| n.as_str() == "lobby message")
        .expect("input")
        .0;
    assert!(
        world.get::<crate::main_menu::MenuButton>(input).is_none(),
        "an unavailable chat input cannot take focus"
    );
    assert!(
        alice
            .shown_texts()
            .iter()
            .any(|text| text == "Lobby chat is unavailable")
    );
}

#[test]
fn bot_members_are_marked_in_the_drawn_lobby_roster() {
    use bota_radovid::proto::{AccountKind, ClientMsg, Reply};
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    let trust = bota_radovid::Tls::trust_pem(&service.pem).expect("trust");
    let mut bot = bota_radovid::Client::open(&service.url, &trust).expect("bot connection");
    let sequence = bot
        .register(
            "teammate",
            crate::main_menu::tests::PASSWORD,
            AccountKind::Bot,
            Some("fixture".into()),
        )
        .expect("register");
    control_reply(&mut bot, sequence);
    let sequence = bot.request(ClientMsg::LobbyList).expect("list");
    let Reply::Lobbies { list } = control_reply(&mut bot, sequence) else {
        panic!("list")
    };
    let lobby = list
        .iter()
        .find(|row| row.name == ALICES_LOBBY)
        .expect("room")
        .id;
    let sequence = bot
        .request(ClientMsg::LobbyJoin {
            lobby,
            password: None,
        })
        .expect("join");
    assert!(matches!(
        control_reply(&mut bot, sequence),
        Reply::Lobby { .. }
    ));
    alice.until("the bot in the roster", |c| {
        room(c)
            .teams
            .iter()
            .flat_map(|t| &t.slots)
            .any(|s| s.who.as_ref().is_some_and(|p| p.name == "teammate"))
    });
    assert!(
        alice.shown_texts().iter().any(|text| text == "BOT"),
        "account kind is drawn, not guessed from the nickname"
    );
}

#[test]
fn an_accept_prompt_owns_input_above_a_lobby_settings_draft() {
    use bota_radovid::proto::{AccountKind, Audience, ClientMsg, RulesetId};
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    click(&mut alice, "edit  ✎");
    select_mode(&mut alice, "1x1 Mid Only");
    act(&mut alice, Act::Focus(1));
    act(&mut alice, Act::Find(RulesetId("solo_mid".into())));
    let trust = bota_radovid::Tls::trust_pem(&service.pem).expect("trust");
    let mut bob = bota_radovid::Client::open(&service.url, &trust).expect("connection");
    let sequence = bob
        .register(
            "opponent",
            crate::main_menu::tests::PASSWORD,
            AccountKind::Human,
            None,
        )
        .expect("register");
    control_reply(&mut bob, sequence);
    let sequence = bob
        .request(ClientMsg::QueueJoinAudiences {
            ruleset: RulesetId("solo_mid".into()),
            audiences: vec![Audience::HumansOnly],
        })
        .expect("queue");
    control_reply(&mut bob, sequence);
    alice.until("the proposal arrived over the draft", |c| {
        c.world().non_send::<MenuCore>().view().offer.is_some()
    });
    alice.tap_named(KeyCode::Enter, bevy::input::keyboard::Key::Enter);
    alice.tap_named(KeyCode::Escape, bevy::input::keyboard::Key::Escape);
    assert!(
        alice.world().non_send::<MenuCore>().test_offer_requested(),
        "Enter answers the top offer"
    );
    let view = alice.world().non_send::<MenuCore>().view();
    let draft = view.lobby_ui.editor.expect("the lower draft survives");
    assert!(!draft.saving);
    assert_eq!(draft.mode.0, "solo_mid");
    assert!(view.offer.is_some());
    assert_eq!(
        room(&alice).settings.mode,
        "Full rules",
        "hidden input never saved the draft"
    );
}

#[test]
fn a_lobby_control_reconnect_restores_the_room_without_an_obsolete_draft() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "reconnect password");
    let _peer = crate::main_menu::tests::keep_menu_session_alive(
        alice.world().non_send::<MenuCore>(),
        &service,
    );
    click(&mut alice, "edit  ✎");
    select_mode(&mut alice, "1x1 Mid Only");
    crate::main_menu::tests::lose_menu_control(&mut alice.world_mut().non_send_mut::<MenuCore>());
    act(&mut alice, Act::RetryService);
    alice.until("the resumed requests completed", |c| {
        c.world().non_send::<MenuCore>().lobby_sync_settled()
    });
    let view = alice.world().non_send::<MenuCore>().view();
    assert!(
        matches!(view.body, PageView::Lobby(_)),
        "a resumed membership restores the actual room"
    );
    assert!(
        view.lobby_ui.editor.is_none(),
        "the disconnected draft cannot commit later"
    );
    assert_eq!(room(&alice).settings.mode, "Full rules");
    click(&mut alice, "edit  ✎");
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .expect("editor")
            .password_unknown
    );
}

#[test]
fn a_large_mode_catalogue_keeps_bounded_rows_and_scrolls_to_the_last_choice() {
    let mut client = Client::run(&["--menu-preview", "lobbies"]);
    {
        let mut preview = client
            .world_mut()
            .resource_mut::<crate::main_menu::Preview>();
        let template = preview.0.lobby_ui.modes[0].clone();
        preview.0.lobby_ui.modes = (0..150)
            .map(|index| {
                let mut mode = template.clone();
                mode.id = bota_radovid::RulesetId(format!("catalog_{index}"));
                mode.name = format!("Catalog mode {index}");
                mode
            })
            .collect();
        preview.0.lobby_ui.open = Some(crate::main_menu::LobbyChoice::Mode);
    }
    client.frames(8);
    let world = client.world_mut();
    let count = world
        .query::<&Name>()
        .iter(world)
        .filter(|name| name.as_str().starts_with("catalog mode "))
        .count();
    assert!(
        count <= 32,
        "a large untrusted catalogue must not create all its option nodes"
    );
    let menu = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "lobby choices")
        .expect("choices")
        .0;
    let cursor = world
        .get::<UiGlobalTransform>(menu)
        .expect("position")
        .translation;
    scroll_at(&mut client, cursor, -500.0, 6);
    let world = client.world_mut();
    let last = world
        .query::<(Entity, &Name, &crate::main_menu::MenuButton)>()
        .iter(world)
        .find(|(_, name, _)| name.as_str() == "catalog mode 149")
        .expect("the final mode is rendered");
    assert!(matches!(&last.2.0, Act::LobbyUi(LobbyAction::Mode(mode)) if mode.0 == "catalog_149"));
    let last_at = world
        .get::<UiGlobalTransform>(last.0)
        .expect("choice position")
        .translation;
    let menu = world
        .query::<(&Name, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
        .find(|(name, _, _)| name.as_str() == "lobby choices")
        .expect("choices");
    assert!(
        Rect::from_center_size(menu.2.translation, menu.1.size()).contains(last_at),
        "the final choice is inside the scrolled viewport"
    );
}

#[test]
fn a_room_line_about_a_bot_draws_its_version_dimmer_beside_the_name() {
    let mut client = Client::run(&["--menu-preview", "lobby-owner"]);
    client.frames(12);
    let world = client.world_mut();
    let mut texts = world.query::<(&Text, &TextColor, &UiGlobalTransform, &InheritedVisibility)>();
    let drawn: Vec<_> = texts
        .iter(world)
        .filter(|(_, _, _, shown)| shown.get())
        .collect();
    let find = |wanted: &str| {
        drawn
            .iter()
            .find(|(text, ..)| text.0 == wanted)
            .unwrap_or_else(|| panic!("{wanted:?} is drawn"))
    };
    let (_, _, rest_at, _) = find("joined the lobby.");
    let on_the_line = |wanted: &str| {
        drawn
            .iter()
            .find(|(text, _, at, _)| {
                text.0 == wanted && (at.translation.y - rest_at.translation.y).abs() < 1.0
            })
            .unwrap_or_else(|| panic!("{wanted:?} is drawn on the room line"))
    };
    let (_, name_ink, name_at, _) = on_the_line("tamsin#3311");
    let (_, version_ink, version_at, _) = on_the_line("v3.1");
    assert!(name_at.translation.x < version_at.translation.x);
    assert!(version_at.translation.x < rest_at.translation.x);
    assert!(
        LinearRgba::from(version_ink.0).red < LinearRgba::from(name_ink.0).red,
        "the version is dimmer than the name"
    );
}

#[test]
fn the_move_prompt_draws_the_member_version_dimmer_beside_the_name() {
    let mut client = Client::run(&["--menu-preview", "lobby-moving"]);
    client.frames(12);
    let world = client.world_mut();
    let mut texts = world.query::<(&Text, &TextColor, &UiGlobalTransform, &InheritedVisibility)>();
    let drawn: Vec<_> = texts
        .iter(world)
        .filter(|(_, _, _, shown)| shown.get())
        .collect();
    let (_, _, lead_at, _) = *drawn
        .iter()
        .find(|(text, ..)| text.0 == "Choose a free slot for")
        .expect("the prompt is drawn");
    let on_the_line = |wanted: &str| {
        *drawn
            .iter()
            .find(|(text, _, at, _)| {
                text.0 == wanted && (at.translation.y - lead_at.translation.y).abs() < 1.0
            })
            .unwrap_or_else(|| panic!("{wanted:?} is drawn on the prompt line"))
    };
    let (_, name_ink, name_at, _) = on_the_line("quartz#55");
    let (_, version_ink, version_at, _) = on_the_line("v3.1");
    let (_, _, tail_at, _) = on_the_line(", or SPECTATE");
    assert!(lead_at.translation.x < name_at.translation.x);
    assert!(name_at.translation.x < version_at.translation.x);
    assert!(version_at.translation.x < tail_at.translation.x);
    assert!(
        LinearRgba::from(version_ink.0).red < LinearRgba::from(name_ink.0).red,
        "the version is dimmer than the name"
    );
}

#[test]
fn the_room_fits_its_two_spectators_and_aligns_the_dire_heading_to_the_right() {
    let mut client = Client::run(&["--menu-preview", "lobby-owner"]);
    client.frames(8);
    let world = client.world_mut();
    assert_spectator_chips_fit(world);
    let button = world
        .query::<(&Name, &ComputedNode)>()
        .iter(world)
        .find(|(name, _)| name.as_str() == "spectate")
        .map(|(_, node)| node)
        .expect("spectate action");
    assert!(
        (button.size().x * button.inverse_scale_factor() - 98.0).abs()
            <= button.inverse_scale_factor()
    );
    let dire = world
        .query::<(&Name, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
        .find(|(name, _, _)| name.as_str() == "lobby team DIRE")
        .map(|(_, node, at)| Rect::from_center_size(at.translation, node.size()))
        .expect("Dire column");
    let (node, at) = world
        .query::<(&Text, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
        .find(|(text, _, _)| text.0 == "DIRE")
        .map(|(_, node, at)| (node, at))
        .expect("Dire heading");
    let heading = Rect::from_center_size(at.translation, node.size());
    assert!((dire.max.x - heading.max.x) * node.inverse_scale_factor() <= 12.0);
}

fn assert_spectator_chips_fit(world: &mut World) {
    let row = world
        .query::<(&Text, &ChildOf)>()
        .iter(world)
        .find(|(text, _)| text.0 == "SPECTATING")
        .map(|(_, parent)| parent.parent())
        .expect("spectator strip");
    let pane = world
        .get::<Children>(row)
        .expect("spectator controls")
        .iter()
        .find(|child| {
            world
                .get::<Node>(*child)
                .is_some_and(|node| node.flex_basis == px(0))
        })
        .expect("spectator pane");
    let node = world.get::<ComputedNode>(pane).expect("pane layout");
    let at = world.get::<UiGlobalTransform>(pane).expect("pane position");
    let bounds = Rect::from_center_size(at.translation, node.size());
    let chips: Vec<_> = world
        .get::<Children>(pane)
        .expect("the two spectators")
        .iter()
        .collect();
    assert_eq!(chips.len(), 2);
    for chip in chips {
        let node = world.get::<ComputedNode>(chip).expect("chip layout");
        let at = world.get::<UiGlobalTransform>(chip).expect("chip position");
        let chip = Rect::from_center_size(at.translation, node.size());
        assert!(
            chip.min.x >= bounds.min.x - 1.0 && chip.max.x <= bounds.max.x + 1.0,
            "the normal two-person spectator roster fits: {chip:?} within {bounds:?}"
        );
    }
}

#[test]
fn the_lobby_room_backdrop_draws_two_light_pools_over_its_dark_floor() {
    let mut client = Client::run(&["--menu-preview", "lobby-owner"]);
    client.frames(8);
    let world = client.world_mut();
    let pools = world
        .query::<&BackgroundGradient>()
        .iter(world)
        .filter(|background| {
            background
                .0
                .iter()
                .filter(|gradient| matches!(gradient, Gradient::Radial(_)))
                .count()
                == 2
        })
        .count();
    assert_eq!(
        pools, 1,
        "the room has blue and purple radial light pools, rather than a flat wash"
    );
}

#[test]
fn lobby_dialog_headings_are_centered_above_their_fields() {
    for (preview, title) in [
        ("lobby-edit", "LOBBY SETTINGS"),
        ("lobby-password", "LOBBY PASSWORD"),
    ] {
        let mut client = Client::run(&["--menu-preview", preview]);
        client.frames(8);
        let world = client.world_mut();
        let popup_name = if preview == "lobby-edit" {
            "lobby settings popup"
        } else {
            "lobby password popup"
        };
        let popup = world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, name)| name.as_str() == popup_name)
            .map(|(entity, _)| entity)
            .expect("dialog");
        let justify = super::harness::subtree(world, popup)
            .into_iter()
            .find_map(|entity| {
                (world.get::<Text>(entity)?.0 == title).then(|| {
                    world
                        .get::<TextLayout>(entity)
                        .expect("heading layout")
                        .justify
                })
            })
            .expect("dialog heading");
        assert_eq!(justify, Justify::Center, "{title} is centered");
    }
}

fn scroll_at(client: &mut Client, at: Vec2, lines: f32, frames: usize) {
    use bevy::{
        input::mouse::{MouseScrollUnit, MouseWheel},
        window::PrimaryWindow,
    };
    let world = client.world_mut();
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .expect("window");
    client.mouse(at);
    client.world_mut().write_message(MouseWheel {
        phase: bevy::input::touch::TouchPhase::Moved,
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: lines,
        window,
    });
    client.frames(frames);
}

#[test]
fn touching_an_unknown_password_never_unlocks_the_lobby() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "stay locked");
    let mut bob = signed_up(&service, "bob");
    join(&mut bob, ALICES_LOBBY, "stay locked");
    alice.until("guest arrived", |c| {
        room(c)
            .teams
            .iter()
            .flat_map(|t| &t.slots)
            .any(|s| s.who.as_ref().is_some_and(|p| p.name == "bob"))
    });
    for edit in [None, Some(""), Some("\u{7}"), Some("remove twice")] {
        assert_password_touch_is_refused(&mut alice, edit);
    }
}

fn assert_password_touch_is_refused(alice: &mut Client, edit: Option<&str>) {
    click(alice, "edit  ✎");
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .expect("draft")
            .password_unknown
    );
    act(alice, Act::Focus(1));
    match edit {
        Some("remove twice") => {
            act(alice, Act::LobbyUi(LobbyAction::RemovePassword));
            act(alice, Act::LobbyUi(LobbyAction::RemovePassword));
        }
        Some(text) => act(alice, Act::Type(text.into())),
        None => act(alice, Act::Erase),
    }
    act(alice, Act::LobbyUi(LobbyAction::Save));
    let view = alice.world().non_send::<MenuCore>().view();
    assert!(
        view.lobby_ui.editor.is_some_and(
            |e| e.error.as_deref() == Some("Set or remove the lobby password before saving.")
        ),
        "no empty-field gesture authorises unlocking"
    );
    assert!(room(alice).locked);
    act(alice, Act::LobbyUi(LobbyAction::Cancel));
}

#[test]
fn a_guest_leave_acknowledgement_closes_the_room_and_clears_its_controls() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "guest secret");
    let mut bob = signed_up(&service, "bob");
    join(&mut bob, ALICES_LOBBY, "guest secret");
    click(&mut bob, "leave lobby  ×");
    // A later request on Bob's same link is the barrier for LobbyLeave's Done.
    act(&mut bob, Act::SearchLobbies);
    bob.until("requests acknowledged", |c| {
        c.world().non_send::<MenuCore>().lobby_sync_settled()
    });
    assert!(matches!(
        bob.world().non_send::<MenuCore>().view().body,
        PageView::Lobbies { .. }
    ));
    assert!(!named(&mut bob, "ready"));
    assert!(!named(&mut bob, "leave lobby  ×"));
}

#[test]
fn a_departed_move_target_cannot_swallow_the_owners_next_slot_click() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "guest secret");
    let mut bob = signed_up(&service, "bob");
    join(&mut bob, ALICES_LOBBY, "guest secret");
    alice.until("guest arrived", |c| {
        room(c)
            .teams
            .iter()
            .flat_map(|t| &t.slots)
            .any(|s| s.who.as_ref().is_some_and(|p| p.name == "bob"))
    });
    click(&mut alice, "move bob");
    assert!(room(&alice).moving.is_some());
    act(&mut bob, Act::LeaveLobby);
    alice.until("guest departed", |c| {
        !room(c)
            .teams
            .iter()
            .flat_map(|t| &t.slots)
            .any(|s| s.who.as_ref().is_some_and(|p| p.name == "bob"))
    });
    assert!(
        room(&alice).moving.is_none(),
        "a removed member is no longer a move target"
    );
    click(&mut alice, "join slot 2");
    act(&mut alice, Act::SearchLobbies);
    alice.until("slot request acknowledged", |c| {
        c.world().non_send::<MenuCore>().lobby_sync_settled()
    });
    assert!(
        room(&alice)
            .teams
            .iter()
            .flat_map(|t| &t.slots)
            .any(|s| s.slot == 2 && s.who.as_ref().is_some_and(|p| p.me))
    );
}

#[test]
fn an_unanswered_save_expires_with_a_drawn_failure_and_a_working_cancel() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    click(&mut alice, "edit  ✎");
    select_mode(&mut alice, "1x1 Mid Only");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    click(&mut alice, "ok");
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .expect("saving")
            .saving
    );
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_elapsed(std::time::Duration::from_secs(60));
    alice.frames(2);
    let editor = alice
        .world()
        .non_send::<MenuCore>()
        .view()
        .lobby_ui
        .editor
        .expect("retryable draft");
    assert!(!editor.saving, "deadline releases the saving state");
    assert_eq!(
        editor.error.as_deref(),
        Some("The lobby request timed out. Try again.")
    );
    assert!(
        alice
            .shown_texts()
            .iter()
            .any(|text| text == "The lobby request timed out. Try again.")
    );
    click(&mut alice, "cancel");
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_none()
    );
    assert_eq!(room(&alice).settings.mode, "Full rules");
}

#[test]
fn hostile_lobby_lists_are_bounded_before_storage_keyboard_or_drawing() {
    use bota_radovid::proto::{Audience, LobbySummary, MAX_LOBBY_LIST, Reply, RulesetId};
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    act(&mut alice, Act::Go(Page::Lobbies));
    let mut list: Vec<_> = (0..1000)
        .map(|id| LobbySummary {
            version: None,
            id: LobbyId(id),
            name: format!("Room {id}"),
            ruleset: RulesetId("full".into()),
            owner: bota_radovid::AccountId(1),
            owner_nickname: "host".into(),
            audience: Audience::Everyone,
            locked: false,
            members: 1,
            seats: 10,
        })
        .collect();
    list[0].name = "\u{7}".repeat(2 * 1024 * 1024) + "FORBIDDEN";
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_reply(Reply::Lobbies { list });
    alice.frames(2);
    let core = alice.world().non_send::<MenuCore>();
    assert_eq!(core.test_lobby_list_len(), MAX_LOBBY_LIST);
    let PageView::Lobbies { list, .. } = core.view().body else {
        panic!("browser")
    };
    assert_eq!(
        core.test_lobby_join_buttons(),
        list.iter().map(|r| r.id).collect::<Vec<_>>()
    );
    assert!(!list.iter().any(|r| r.name.contains("FORBIDDEN")));
    assert!(!alice.shown_texts().iter().any(|t| t.contains("FORBIDDEN")));
}

#[test]
fn hostile_team_sizes_are_rejected_before_replacing_the_actual_room() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    let mut state = alice.world().non_send::<MenuCore>().test_lobby_state();
    state.teams = vec![255, 255];
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_event(state);
    let state = alice.world().non_send::<MenuCore>().test_lobby_state();
    assert_eq!(
        state.teams.iter().map(|n| usize::from(*n)).sum::<usize>(),
        bota_radovid::proto::MAX_SEATS
    );
    alice.frames(2);
    assert!(
        alice
            .shown_texts()
            .iter()
            .any(|t| t == "Invalid lobby information from the service.")
    );
}

#[test]
fn hostile_member_counts_and_names_are_bounded_once_on_receipt() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    let mut state = alice.world().non_send::<MenuCore>().test_lobby_state();
    let template = state.members[0].clone();
    for index in 0..1000 {
        let mut member = template.clone();
        member.account = bota_radovid::AccountId(1000 + index);
        member.slot = bota_radovid::proto::LOBBY_NO_SLOT;
        state.members.push(member);
    }
    state.members[1].nickname = "\u{7}".repeat(2 * 1024 * 1024) + "FORBIDDEN";
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_event(state);
    let core = alice.world().non_send::<MenuCore>();
    let state = core.test_lobby_state();
    assert_eq!(state.members.len(), usize::from(u8::MAX));
    assert!(
        state
            .members
            .iter()
            .all(|m| m.nickname.len() <= bota_radovid::proto::MAX_NICKNAME_LEN)
    );
    assert!(!format!("{:?}", core.view()).contains("FORBIDDEN"));
}

#[test]
fn hostile_ruleset_replies_replace_the_lobby_catalogue_with_bounded_safe_rows() {
    use bota_radovid::proto::{MAX_RATED_MODES, Reply, RulesetId};
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    act(&mut alice, Act::Go(Page::Lobbies));
    let template = alice.world().non_send::<MenuCore>().test_lobby_rule();
    let mut list: Vec<_> = (0..1000)
        .map(|i| {
            let mut rule = template.clone();
            rule.id = RulesetId(format!("remote_{i}"));
            rule.name = format!("Remote {i}");
            rule
        })
        .collect();
    list[0].name = "\u{7}".repeat(2 * 1024 * 1024) + "FORBIDDEN";
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_reply(Reply::Rulesets { list });
    let view = alice.world().non_send::<MenuCore>().view();
    assert_eq!(view.lobby_ui.modes.len(), MAX_RATED_MODES);
    assert_eq!(view.lobby_ui.modes[0].id.0, "remote_0");
    assert!(
        !view
            .lobby_ui
            .modes
            .iter()
            .any(|m| m.name.contains("FORBIDDEN"))
    );
    act(&mut alice, Act::Go(Page::Home));
    let PageView::Home { rulesets, .. } = alice.world().non_send::<MenuCore>().view().body else {
        panic!("Play")
    };
    assert_eq!(rulesets.len(), MAX_RATED_MODES);
    assert!(!rulesets.iter().any(|r| r.name.contains("FORBIDDEN")));
}

#[test]
fn typing_search_debounces_actual_queries_and_no_op_edits_send_nothing() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    act(&mut alice, Act::Go(Page::Lobbies));
    alice.until("initial list settled", |c| {
        c.world().non_send::<MenuCore>().lobby_sync_settled()
    });
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    act(&mut alice, Act::Focus(0));
    act(&mut alice, Act::Erase);
    act(&mut alice, Act::Type("".into()));
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .test_lobby_requests()
            .is_empty()
    );
    for letter in ["f", "r", "i"] {
        act(&mut alice, Act::Type(letter.into()));
    }
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .test_lobby_requests()
            .is_empty()
    );
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_elapsed(std::time::Duration::from_secs(1));
    let core = alice.world().non_send::<MenuCore>();
    assert!(
        matches!(core.test_lobby_requests(), [bota_radovid::proto::ClientMsg::LobbySearch { query }] if query == "fri")
    );
}

#[test]
fn lobby_housekeeping_and_non_lobby_scroll_do_not_build_full_menu_views() {
    use bevy::ecs::system::RunSystemOnce;
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    alice
        .world()
        .non_send::<MenuCore>()
        .test_reset_view_builds();
    alice
        .world_mut()
        .run_system_once(crate::main_menu::test_lobby_scroll)
        .expect("scroll system");
    assert_eq!(alice.world().non_send::<MenuCore>().test_view_builds(), 0);
    create(&mut alice, "");
    assert_eq!(
        alice
            .world_mut()
            .non_send_mut::<MenuCore>()
            .test_lobby_poll_views(),
        0
    );
    assert_eq!(
        alice
            .world_mut()
            .non_send_mut::<MenuCore>()
            .test_lobby_poll_views(),
        0
    );
}

#[test]
fn a_late_password_refusal_cannot_close_a_different_lobby_dialog() {
    use bota_radovid::proto::{Audience, LobbySummary, Reply, RulesetId};
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    let list = (1..=2)
        .map(|id| LobbySummary {
            version: None,
            id: LobbyId(id),
            name: format!("Locked {id}"),
            ruleset: RulesetId("full".into()),
            owner: bota_radovid::AccountId(999),
            owner_nickname: "host".into(),
            audience: Audience::Everyone,
            locked: true,
            members: 1,
            seats: 10,
        })
        .collect();
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_reply(Reply::Lobbies { list });
    act(&mut alice, Act::JoinLobby(LobbyId(1)));
    act(&mut alice, Act::LobbyPassword(false));
    act(&mut alice, Act::JoinLobby(LobbyId(2)));
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_late_join_refusal(LobbyId(1));
    assert_eq!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .password
            .expect("newer dialog survives")
            .lobby,
        "Locked 2"
    );
}

#[test]
fn losing_lobby_membership_on_resume_has_a_visible_explanation() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_restore_missing();
    alice.frames(2);
    assert!(matches!(
        alice.world().non_send::<MenuCore>().view().body,
        PageView::Lobbies { .. }
    ));
    assert!(
        alice
            .shown_texts()
            .iter()
            .any(|t| t == "The lobby is no longer available.")
    );
}

fn drawn_setting(client: &mut Client, name: &str) -> Option<String> {
    let world = client.world_mut();
    world
        .query::<(&Name, &Text, &InheritedVisibility)>()
        .iter(world)
        .find(|(label, _, visible)| visible.get() && label.as_str() == name)
        .map(|(_, text, _)| text.0.clone())
}

#[test]
fn cheats_off_cannot_satisfy_the_drawn_cheats_on_check_when_password_is_none() {
    let service = crate::main_menu::tests::service_with(|config| {
        config.replace(
            "[ruleset.launcher]",
            "[ruleset.launcher]\ncheats_args = [\"--lobby-cheats\"]",
        )
    });
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    assert!(!room(&alice).locked);
    assert_eq!(
        drawn_setting(&mut alice, "lobby setting cheats").as_deref(),
        Some("OFF")
    );
    assert_ne!(
        drawn_setting(&mut alice, "lobby setting cheats").as_deref(),
        Some("ON")
    );
    click(&mut alice, "edit  ✎");
    click(&mut alice, "enable cheats");
    click(&mut alice, "ok");
    alice.until("cheats acknowledged", |c| {
        room(c).settings.cheats == Some(true)
            && c.world()
                .non_send::<MenuCore>()
                .view()
                .lobby_ui
                .editor
                .is_none()
    });
    assert_eq!(
        drawn_setting(&mut alice, "lobby setting cheats").as_deref(),
        Some("ON")
    );
}

#[test]
fn a_locked_lobbys_password_can_only_be_removed_by_the_explicit_control() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "known secret");
    click(&mut alice, "edit  ✎");
    // Not asserted: that the draft knows the password just saved. The service answers a save with
    // Done and then pushes the lobby's state, and every state update forgets the remembered
    // password (another session may have changed it), so whether the host's next draft knows it
    // depends on that push having arrived. A Create used to be answered with the state alone,
    // which is why this test could ask it of a creation.
    assert!(
        named(&mut alice, "remove password"),
        "locked state offers an explicit remove operation"
    );
    click(&mut alice, "remove password");
    click(&mut alice, "ok");
    alice.until("explicitly unlocked", |c| {
        !room(c).locked
            && c.world()
                .non_send::<MenuCore>()
                .view()
                .lobby_ui
                .editor
                .is_none()
    });
}

#[test]
fn an_unconfirmed_create_is_explained_and_recovered_without_creating_another_lobby() {
    use bota_radovid::proto::{ClientMsg, Reply};
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    let mut peer = crate::main_menu::tests::keep_menu_session_alive(
        alice.world().non_send::<MenuCore>(),
        &service,
    );
    act(&mut alice, Act::Go(Page::Lobbies));
    // Without a frame in between, so that the service's answer is not taken before the link is lost.
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .act(Act::LobbyUi(LobbyAction::Create));
    observe_unconfirmed_create(&mut peer);
    crate::main_menu::tests::lose_menu_control(&mut alice.world_mut().non_send_mut::<MenuCore>());
    alice.frames(2);
    assert!(alice.shown_texts().iter().any(|text| text == "Lobby creation was not confirmed. It may still exist; refresh Lobbies before trying again."));
    act(&mut alice, Act::RetryService);
    alice.until("resumed", |c| {
        c.world().non_send::<MenuCore>().lobby_sync_settled()
    });
    act(&mut alice, Act::SearchLobbies);
    alice.within(
        std::time::Duration::from_secs(10),
        "existing lobby listed",
        |c| named(c, "join alice's lobby"),
    );
    click(&mut alice, "join alice's lobby");
    alice.until("owned membership recovered", |c| {
        matches!(
            c.world().non_send::<MenuCore>().view().body,
            PageView::Lobby(_)
        )
    });
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .password
            .is_none()
    );
    let sequence = peer.request(ClientMsg::LobbyList).expect("list");
    let Reply::Lobbies { list } = control_reply(&mut peer, sequence) else {
        panic!("list")
    };
    assert_eq!(
        list.iter().filter(|row| row.name == ALICES_LOBBY).count(),
        1
    );
}

fn observe_unconfirmed_create(peer: &mut bota_radovid::Client) {
    use bota_radovid::proto::{ClientMsg, Reply};
    // Observe commit from another session without letting Alice consume the Create reply.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let sequence = peer.request(ClientMsg::LobbyList).expect("list");
        let Reply::Lobbies { list } = control_reply(peer, sequence) else {
            panic!("list")
        };
        if list.iter().any(|row| row.name == ALICES_LOBBY) {
            break;
        }
        assert!(
            std::time::Instant::now() < until,
            "Create reached the service"
        );
        peer.wait(std::time::Duration::from_millis(50))
            .expect("control link");
    }
}

#[test]
fn an_invalid_create_reply_cannot_complete_against_an_older_owned_room() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "existing secret");
    let mut invalid = alice.world().non_send::<MenuCore>().test_lobby_state();
    invalid.teams = vec![255, 255];
    act(&mut alice, Act::Go(Page::Lobbies));
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    click(&mut alice, "create lobby");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_answer_held_lobby_request(bota_radovid::proto::Reply::Lobby { state: invalid });
    let core = alice.world().non_send::<MenuCore>();
    assert_eq!(core.test_lobby_state().name, ALICES_LOBBY);
    let editor = core
        .view()
        .lobby_ui
        .editor
        .expect("the invalid reply is not successful creation");
    assert!(!editor.saving);
    assert!(
        editor
            .error
            .as_ref()
            .is_some_and(|e| e.contains("Invalid lobby information"))
    );
}

#[test]
fn cancelling_a_pending_save_closes_its_dialog_and_ignores_a_late_done() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    click(&mut alice, "edit  ✎");
    select_mode(&mut alice, "1x1 Mid Only");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    click(&mut alice, "ok");
    click(&mut alice, "cancel");
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_none()
    );
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_answer_held_lobby_request(bota_radovid::proto::Reply::Done);
    alice.frames(2);
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_none()
    );
    assert_eq!(room(&alice).settings.mode, "Full rules");
}

#[test]
fn a_join_deadline_survives_background_request_tracking_pressure() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "secret");
    let mut bob = signed_up(&service, "bob");
    act(&mut bob, Act::Go(Page::Lobbies));
    bob.until("listed", |c| named(c, "join alice's lobby"));
    click(&mut bob, "join alice's lobby");
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    act(&mut bob, Act::Type("secret".into()));
    click(&mut bob, "ok");
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_background_requests();
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_elapsed(std::time::Duration::from_secs(60));
    bob.frames(2);
    let prompt = bob
        .world()
        .non_send::<MenuCore>()
        .view()
        .password
        .expect("retryable password dialog");
    assert_eq!(
        prompt.error.as_deref(),
        Some("The lobby request timed out. Try again.")
    );
    click(&mut bob, "cancel");
    assert!(bob.world().non_send::<MenuCore>().view().password.is_none());
}

#[test]
fn a_full_lobby_request_tracker_refuses_new_work_without_forgetting_old_requests() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    act(&mut alice, Act::Go(Page::Lobbies));
    alice.until("initial requests settled", |c| {
        c.world().non_send::<MenuCore>().lobby_sync_settled()
    });
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    {
        let mut core = alice.world_mut().non_send_mut::<MenuCore>();
        for query in 0..70 {
            core.act(Act::Focus(0));
            core.act(Act::Erase);
            core.act(Act::Erase);
            core.act(Act::Type(query.to_string()));
            core.act(Act::SearchLobbies);
        }
    }
    assert_eq!(
        alice
            .world()
            .non_send::<MenuCore>()
            .test_lobby_requests()
            .len(),
        64,
        "the client request budget is enforced before sending"
    );
    alice.frames(2);
    assert!(
        alice
            .shown_texts()
            .iter()
            .any(|t| t == "Too many lobby requests. Wait for the current requests to finish.")
    );
}

#[test]
fn a_catalogue_budget_counts_supported_game_modes_not_foreign_games() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    act(&mut alice, Act::Go(Page::Lobbies));
    let mut supported = alice.world().non_send::<MenuCore>().test_lobby_rule();
    supported.id = bota_radovid::RulesetId("supported_mode".into());
    let mut list: Vec<_> = (0..100)
        .map(|i| {
            let mut rule = supported.clone();
            rule.game = "other_game".into();
            rule.id = bota_radovid::RulesetId(format!("other_{i}"));
            rule
        })
        .collect();
    list.push(supported);
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_reply(bota_radovid::proto::Reply::Rulesets { list });
    let view = alice.world().non_send::<MenuCore>().view();
    assert_eq!(view.lobby_ui.modes.len(), 1);
    assert_eq!(view.lobby_ui.modes[0].id.0, "supported_mode");
    assert!(
        !view
            .status
            .is_some_and(|s| s.text.contains("Only the first"))
    );
}

#[test]
fn cancelling_a_refused_create_does_not_claim_that_creation_is_unconfirmed() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    let mut bob = signed_up(&service, "bob");
    act(&mut bob, Act::Go(Page::Lobbies));
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    click(&mut bob, "create lobby");
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_refuse_held_lobby_request(bota_radovid::proto::Error::LobbyNameTaken);
    bob.until("Create was definitively refused", |c| {
        c.world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_some_and(|e| e.error.as_deref() == Some("A lobby of that name is open already"))
    });
    bob.until("the refusal's dialog drawn", |c| named(c, "cancel"));
    click(&mut bob, "cancel");
    assert!(
        !bob.shown_texts()
            .iter()
            .any(|text| text.contains("Lobby creation was not confirmed"))
    );
}

#[test]
fn cancelling_an_unexpected_create_answer_does_not_report_a_missing_answer() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    act(&mut alice, Act::Go(Page::Lobbies));
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    click(&mut alice, "create lobby");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_answer_held_lobby_request(bota_radovid::proto::Reply::Done);
    alice.frames(2);
    assert!(
        alice
            .shown_texts()
            .iter()
            .any(|text| text == "The service did not confirm the lobby change")
    );
    click(&mut alice, "cancel");
    assert!(
        !alice
            .shown_texts()
            .iter()
            .any(|text| text.contains("Lobby creation was not confirmed"))
    );
}

#[test]
fn retrying_a_timed_out_leave_clears_the_room_when_the_server_already_removed_the_guest() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "guest secret");
    let mut bob = signed_up(&service, "bob");
    join(&mut bob, ALICES_LOBBY, "guest secret");
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .act(Act::LeaveLobby);
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_drop_lobby_reply();
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_elapsed(std::time::Duration::from_secs(60));
    bob.frames(2);
    assert!(
        bob.shown_texts()
            .iter()
            .any(|t| t == "The lobby request timed out. Try again.")
    );
    click(&mut bob, "leave lobby  ×");
    act(&mut bob, Act::SearchLobbies);
    bob.until("retry answered", |c| {
        c.world().non_send::<MenuCore>().lobby_sync_settled()
    });
    assert!(matches!(
        bob.world().non_send::<MenuCore>().view().body,
        PageView::Lobbies { .. }
    ));
    assert!(!named(&mut bob, "leave lobby  ×"));
    assert!(!named(&mut bob, "ready"));
}

#[test]
fn a_leave_answer_that_the_lobby_is_gone_clears_the_stale_room() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    click(&mut alice, "leave lobby  ×");
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_refuse_held_lobby_request(bota_radovid::proto::Error::UnknownLobby);
    alice.frames(2);
    assert!(matches!(
        alice.world().non_send::<MenuCore>().view().body,
        PageView::Lobbies { .. }
    ));
    assert!(!named(&mut alice, "leave lobby  ×"));
}

fn guest_with_a_timed_out_join(service: &crate::main_menu::tests::Service) -> (Client, Client) {
    let mut alice = signed_up(service, "alice");
    create(&mut alice, "guest secret");
    let mut bob = signed_up(service, "bob");
    act(&mut bob, Act::Go(Page::Lobbies));
    bob.until("listed", |c| named(c, "join alice's lobby"));
    click(&mut bob, "join alice's lobby");
    act(&mut bob, Act::Type("guest secret".into()));
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .act(Act::LobbyPassword(true));
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_drop_lobby_reply();
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_elapsed(std::time::Duration::from_secs(60));
    bob.frames(2);
    assert!(
        bob.shown_texts()
            .iter()
            .any(|t| t == "The lobby request timed out. Try again.")
    );
    (alice, bob)
}

#[test]
fn refreshing_after_a_timed_out_join_recovers_existing_membership_without_joining_twice() {
    let service = crate::main_menu::tests::service();
    let (_alice, mut bob) = guest_with_a_timed_out_join(&service);
    act(&mut bob, Act::SearchLobbies);
    bob.until("refresh answered", |c| {
        c.world().non_send::<MenuCore>().lobby_sync_settled()
    });
    assert!(matches!(
        bob.world().non_send::<MenuCore>().view().body,
        PageView::Lobby(_)
    ));
    assert!(named(&mut bob, "ready"));
    assert_eq!(
        room(&bob)
            .teams
            .iter()
            .flat_map(|t| &t.slots)
            .filter_map(|s| s.who.as_ref())
            .filter(|m| m.name == "bob")
            .count(),
        1
    );
}

#[test]
fn cancelling_a_timed_out_join_does_not_retry_it_when_the_browser_is_refreshed() {
    let service = crate::main_menu::tests::service();
    let (_alice, mut bob) = guest_with_a_timed_out_join(&service);
    click(&mut bob, "cancel");
    act(&mut bob, Act::SearchLobbies);
    bob.until("refresh answered", |c| {
        c.world().non_send::<MenuCore>().lobby_sync_settled()
    });
    let view = bob.world().non_send::<MenuCore>().view();
    assert!(matches!(view.body, PageView::Lobbies { .. }));
    assert!(view.password.is_none());
}

#[test]
fn a_refused_join_retry_is_not_reissued_by_a_later_list_refresh() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    create(&mut alice, "guest secret");
    let mut bob = signed_up(&service, "bob");
    act(&mut bob, Act::Go(Page::Lobbies));
    bob.until("listed", |c| named(c, "join alice's lobby"));
    click(&mut bob, "join alice's lobby");
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    act(&mut bob, Act::Type("wrong secret".into()));
    click(&mut bob, "ok");
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_lobby_elapsed(std::time::Duration::from_secs(60));
    act(&mut bob, Act::Focus(0));
    act(&mut bob, Act::Type("wrong again".into()));
    click(&mut bob, "ok");
    bob.world_mut()
        .non_send_mut::<MenuCore>()
        .test_refuse_held_lobby_request(bota_radovid::proto::Error::BadLobbyPassword);
    bob.frames(2);
    assert!(
        bob.shown_texts()
            .iter()
            .any(|text| text == "Wrong password.")
    );
    let joins = |client: &Client| {
        client
            .world()
            .non_send::<MenuCore>()
            .test_lobby_requests()
            .iter()
            .filter(|request| matches!(request, bota_radovid::proto::ClientMsg::LobbyJoin { .. }))
            .count()
    };
    assert_eq!(
        joins(&bob),
        2,
        "the initial join and explicit retry were posted"
    );
    act(&mut bob, Act::SearchLobbies);
    assert_eq!(
        joins(&bob),
        2,
        "a definitive refusal leaves refresh as a list request"
    );
}

#[test]
fn a_controller_uses_the_existing_lobby_dropdown_keyboard_route() {
    use super::menu::{act, signed_up};
    use crate::main_menu::{Act, LobbyAction, LobbyChoice, MenuCore, Page};
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "lobby_pad");
    act(&mut client, Act::Go(Page::Lobbies));
    act(
        &mut client,
        Act::LobbyUi(LobbyAction::Open(LobbyChoice::Mode)),
    );
    let pad = client
        .world_mut()
        .spawn((
            bevy::input::gamepad::Gamepad::default(),
            bevy::input::gamepad::GamepadSettings::default(),
        ))
        .id();
    let send = |client: &mut Client, button, value| {
        client
            .world_mut()
            .write_message(bevy::input::gamepad::RawGamepadEvent::Button(
                bevy::input::gamepad::RawGamepadButtonChangedEvent::new(pad, button, value),
            ));
        client.frames(2);
    };
    for _ in 0..12 {
        if matches!(client.world().non_send::<MenuCore>().button_focus(),Some(Act::LobbyUi(LobbyAction::Mode(mode))) if mode.0=="solo_mid")
        {
            break;
        }
        send(
            &mut client,
            bevy::input::gamepad::GamepadButton::DPadDown,
            1.0,
        );
        send(
            &mut client,
            bevy::input::gamepad::GamepadButton::DPadDown,
            0.0,
        );
    }
    assert!(
        matches!(client.world().non_send::<MenuCore>().button_focus(),Some(Act::LobbyUi(LobbyAction::Mode(mode))) if mode.0=="solo_mid"),
        "controller uses the lobby form's existing focus order"
    );
    send(&mut client, bevy::input::gamepad::GamepadButton::South, 1.0);
    send(&mut client, bevy::input::gamepad::GamepadButton::South, 0.0);
    assert_eq!(
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .mode
            .expect("mode")
            .0,
        "solo_mid"
    );
}

/// Client-state delivery through MenuCore, not initiation of cancellation at the service.
/// The service's request-path tests cover who may cancel and the timer's lifetime.
#[test]
fn client_lobby_states_without_deadlines_restore_host_and_guest_actions() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    let mut bob = signed_up(&service, "bob");
    create(&mut alice, "key");
    join(&mut bob, ALICES_LOBBY, "key");
    alice.until("both members in the host snapshot", |client| {
        client
            .world()
            .non_send::<MenuCore>()
            .test_lobby_state()
            .members
            .len()
            == 2
    });
    for client in [&mut alice, &mut bob] {
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .test_pause_service_events();
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .test_hold_lobby_requests();
    }
    let mut state = alice.world().non_send::<MenuCore>().test_lobby_state();
    let room_id = state.id;
    state.starting_in_ms = Some(5000);
    for member in &mut state.members {
        member.ready = true;
    }
    for client in [&mut alice, &mut bob] {
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .test_lobby_event(state.clone());
        client.frames(3);
        assert!(room(client).starting_in_secs.left().is_some());
        assert!(
            client
                .shown_texts()
                .iter()
                .any(|line| line.starts_with("Starting in "))
        );
    }
    state.starting_in_ms = None;
    for member in &mut state.members {
        if member.account == state.owner {
            member.ready = false;
        }
    }
    for client in [&mut alice, &mut bob] {
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .test_lobby_event(state.clone());
        client.frames(3);
        assert_eq!(
            client.world().non_send::<MenuCore>().test_lobby_state().id,
            room_id
        );
        assert!(
            room(client).starting_in_secs.left().is_none(),
            "every recipient clears the countdown"
        );
        assert!(
            !client
                .shown_texts()
                .iter()
                .any(|line| line.starts_with("Starting in "))
        );
    }
    assert!(room(&alice).owner);
    assert!(!room(&alice).me_ready, "the host can start again");
    assert!(!room(&bob).owner);
    assert!(
        room(&bob).me_ready,
        "cancellation preserves the guest's readiness"
    );
    for (client, name) in [(&mut alice, "start game"), (&mut bob, "not ready")] {
        let world = client.world_mut();
        let button = world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, label)| label.as_str() == name)
            .map(|(entity, _)| entity)
            .expect("normal room action");
        assert!(
            world.get::<crate::main_menu::MenuButton>(button).is_some(),
            "{name} is a working action after cancellation"
        );
    }
    click(&mut bob, "not ready");
    assert!(
        matches!(bob.world().non_send::<MenuCore>().test_lobby_requests().last(),
        Some(bota_radovid::proto::ClientMsg::LobbyReady { lobby,ready:false }) if *lobby==room_id),
        "the guest can use the restored action"
    );
}

/// The room's button named `name` and the words drawn on it.
fn room_button(client: &mut Client, name: &str) -> Option<(Entity, String)> {
    let world = client.world_mut();
    let button = world
        .query_filtered::<(Entity, &Name), With<crate::main_menu::MenuButton>>()
        .iter(world)
        .find(|(_, label)| label.as_str() == name)
        .map(|(entity, _)| entity)?;
    let words = super::harness::subtree(world, button)
        .into_iter()
        .filter_map(|entity| world.get::<Text>(entity).map(|text| text.0.clone()))
        .collect::<Vec<_>>()
        .join(" ");
    Some((button, words))
}

#[test]
fn the_countdown_is_on_the_hosts_start_button_and_pressing_it_cancels_the_start() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    let mut bob = signed_up(&service, "bob");
    create(&mut alice, "key");
    join(&mut bob, ALICES_LOBBY, "key");
    alice.until("both members in the host snapshot", |client| {
        client
            .world()
            .non_send::<MenuCore>()
            .test_lobby_state()
            .members
            .len()
            == 2
    });
    for client in [&mut alice, &mut bob] {
        let mut menu = client.world_mut().non_send_mut::<MenuCore>();
        menu.test_pause_service_events();
        menu.test_hold_lobby_requests();
    }
    let mut state = alice.world().non_send::<MenuCore>().test_lobby_state();
    let room_id = state.id;
    state.starting_in_ms = Some(5000);
    for member in &mut state.members {
        member.ready = true;
    }
    for client in [&mut alice, &mut bob] {
        client
            .world_mut()
            .non_send_mut::<MenuCore>()
            .test_lobby_event(state.clone());
        client.frames(3);
    }
    let (button, words) = room_button(&mut alice, "start game").expect("the host's start button");
    assert_eq!(
        words, "STARTING IN 5",
        "the host's start button says how long is left, and stays a button"
    );
    // The seconds run out while the pointer rests on another control and the keyboard on the button.
    let (other, _) = room_button(&mut alice, "edit  ✎").expect("another control");
    alice.hover_ui_with(std::time::Duration::from_secs(30), |client| {
        room_button(client, "edit  ✎").map(|(entity, _)| entity)
    });
    alice
        .world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(button, bevy::input_focus::FocusCause::Navigated);
    for left in [4, 3, 2] {
        alice
            .world_mut()
            .non_send_mut::<MenuCore>()
            .test_lobby_counted(std::time::Duration::from_secs(1));
        alice.frames(2);
        let (now, words) = room_button(&mut alice, "start game").expect("the start button");
        assert_eq!(
            words,
            format!("STARTING IN {left}"),
            "the label counts down"
        );
        assert_eq!(now, button, "the countdown does not rebuild the button");
        assert_eq!(
            room_button(&mut alice, "edit  ✎").map(|(entity, _)| entity),
            Some(other),
            "nor the control the pointer is on"
        );
        assert!(
            alice
                .world()
                .get::<bevy::picking::hover::Hovered>(other)
                .is_some_and(|hover| hover.get()),
            "which stays hovered"
        );
        assert_eq!(
            alice
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get(),
            Some(button),
            "and the keyboard stays on the start button"
        );
    }
    assert!(
        alice
            .shown_texts()
            .iter()
            .any(|line| line == "Starting in 2"),
        "the status line counts too"
    );
    assert!(
        room_button(&mut bob, "ready").is_none_or(|(_, words)| !words.contains("STARTING")),
        "a guest's button does not carry a countdown it cannot cancel"
    );
    let at = alice.centre_of(button).expect("laid out");
    alice.mouse(at);
    alice.click(MouseButton::Left);
    assert!(
        matches!(
            alice.world().non_send::<MenuCore>().test_lobby_requests().last(),
            Some(bota_radovid::proto::ClientMsg::LobbyReady { lobby, ready: false }) if *lobby == room_id
        ),
        "pressing the countdown asks the service to cancel the start"
    );
}

#[test]
fn a_name_the_service_has_open_already_is_refused_in_the_details_window() {
    use bota_radovid::proto::{AccountKind, Audience, ClientMsg, Reply, RulesetId};
    let service = crate::main_menu::tests::service();
    let trust = bota_radovid::Tls::trust_pem(&service.pem).expect("trust");
    let mut squatter = bota_radovid::Client::open(&service.url, &trust).expect("connection");
    let sequence = squatter
        .register(
            "squatter",
            crate::main_menu::tests::PASSWORD,
            AccountKind::Human,
            None,
        )
        .expect("register");
    control_reply(&mut squatter, sequence);
    let sequence = squatter
        .request(ClientMsg::LobbyCreate {
            ruleset: RulesetId("full".into()),
            name: ALICES_LOBBY.into(),
            audience: Audience::Everyone,
            password: None,
        })
        .expect("create");
    assert!(matches!(
        control_reply(&mut squatter, sequence),
        Reply::Lobby { .. }
    ));
    let mut alice = signed_up(&service, "alice");
    act(&mut alice, Act::Go(Page::Lobbies));
    click(&mut alice, "create lobby");
    alice.until("the real service's refusal in the editor", |client| {
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .lobby_ui
            .editor
            .is_some_and(|editor| {
                editor.error.as_deref() == Some("A lobby of that name is open already")
            })
    });
    alice.until("the details window drawn", |client| named(client, "cancel"));
}

#[test]
fn a_lobby_made_at_once_draws_no_details_window_while_the_service_answers() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    act(&mut alice, Act::Go(Page::Lobbies));
    alice
        .world_mut()
        .non_send_mut::<MenuCore>()
        .test_hold_lobby_requests();
    click(&mut alice, "create lobby");
    alice.frames(3);
    let view = alice.world().non_send::<MenuCore>().view();
    assert!(
        view.lobby_ui.editor.is_some_and(|editor| editor.saving),
        "the creation is waiting for the service"
    );
    assert!(
        !named(&mut alice, "ok") && !named(&mut alice, "lobby name"),
        "and no window asks for a name or a password meanwhile"
    );
}
