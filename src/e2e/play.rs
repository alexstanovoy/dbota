//! Play and its transient states, through the real app and its drawn nodes.
use super::harness::Client;
use super::menu::{act, signed_up};
use crate::main_menu::{Act, MenuCore, PlayAction, Preview};
use bevy::prelude::*;

fn named(client: &mut Client, prefix: &str) -> Vec<(String, Color)> {
    let world = client.world_mut();
    world
        .query::<(&Name, &BackgroundColor)>()
        .iter(world)
        .filter(|(name, _)| name.as_str().starts_with(prefix))
        .map(|(name, color)| (name.to_string(), color.0))
        .collect()
}

#[test]
fn accepting_shows_one_square_per_player_and_fills_only_the_acceptance_count() {
    let mut client = Client::run(&["--menu-preview", "accepted"]);
    client.frames(8);
    let squares = named(&mut client, "accept-player-");
    assert_eq!(
        squares.len(),
        10,
        "one drawn square per proposal participant"
    );
    assert_eq!(
        squares
            .iter()
            .filter(|(_, color)| *color == crate::theme::GO.0)
            .count(),
        7
    );
    for count in [0, 1, 8, 10] {
        client
            .world_mut()
            .resource_mut::<Preview>()
            .0
            .offer
            .as_mut()
            .expect("offer")
            .accepted = count;
        client.frames(3);
        let squares = named(&mut client, "accept-player-");
        assert_eq!(squares.len(), 10);
        assert_eq!(
            squares
                .iter()
                .filter(|(_, color)| *color == crate::theme::GO.0)
                .count(),
            usize::from(count)
        );
    }
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.contains("FINDING MATCH")),
        "no finding band behind Accept, including other searches"
    );
}

#[test]
fn the_accept_time_line_shrinks_from_both_ends_and_moves_from_green_to_yellow() {
    let mut client = Client::run(&["--menu-preview", "match-found"]);
    client.frames(8);
    let line = named(&mut client, "accept time");
    assert_eq!(line.len(), 1, "one time line along the window's top edge");
    let green = line[0].1.to_srgba();
    assert!(green.green > green.red, "the initial time line is green");
    let early = time_rect(&mut client);
    client
        .world_mut()
        .resource_mut::<Preview>()
        .0
        .offer
        .as_mut()
        .expect("offer")
        .left_permille = Some(100);
    client.frames(4);
    let yellow = named(&mut client, "accept time")[0].1.to_srgba();
    assert!(
        yellow.red > green.red && yellow.green > yellow.blue * 2.0,
        "the time line approaches yellow, never red"
    );
    let late = time_rect(&mut client);
    assert!(
        (early.center().x - late.center().x).abs() < 0.5,
        "both ends shrink toward the same centre"
    );
    assert!(late.width() < early.width() / 2.0);
}

#[test]
fn loading_draws_the_local_progress_in_white_inside_the_player_sidebar() {
    let mut client = Client::run(&["--menu-preview", "loading"]);
    client.frames(8);
    let fill = named(&mut client, "loading progress");
    assert_eq!(fill.len(), 1, "one loading progress fill");
    assert_eq!(fill[0].1.to_srgba(), Srgba::WHITE);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|t| t == "PLAYERS · 8 OF 10 CONNECTED")
    );
    assert!(client.shown_texts().iter().any(|t| t == "Connecting…"));
    assert!(
        !client.shown_texts().iter().any(|t| t.ends_with('%')),
        "no invented remote load percentages"
    );
}

fn time_rect(client: &mut Client) -> Rect {
    let world = client.world_mut();
    let (node, at) = world
        .query::<(&Name, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
        .find(|(name, ..)| name.as_str() == "accept time")
        .map(|(_, n, at)| (*n, *at))
        .expect("time line");
    Rect::from_center_size(at.translation, node.size())
}

fn click(client: &mut Client, label: &str) {
    let world = client.world_mut();
    let entity = world
        .query_filtered::<(Entity, &Name), With<Button>>()
        .iter(world)
        .find(|(_, name)| name.as_str() == label)
        .map(|(e, _)| e)
        .expect("button");
    let at = client.centre_of(entity).expect("laid out");
    client.mouse(at);
    client.click(MouseButton::Left);
    client.frames(4);
}

#[test]
fn finding_board_cancels_the_real_queue_and_accept_squares_follow_service_answers() {
    let service = crate::main_menu::tests::service_with(|config| {
        config.replacen("rated = false", "rated = true", 1)
    });
    let mut alice = signed_up(&service, "alice");
    act(
        &mut alice,
        Act::Play(PlayAction::Select(bota_radovid::proto::RulesetId(
            "full".into(),
        ))),
    );
    act(
        &mut alice,
        Act::Play(PlayAction::Select(bota_radovid::proto::RulesetId(
            "solo_mid".into(),
        ))),
    );
    click(&mut alice, "play");
    click(&mut alice, "allow matchmaking with bots");
    click(&mut alice, "find match");
    alice.until("finding board", |c| {
        c.shown_texts().iter().any(|t| t == "FINDING MATCH")
    });
    assert!(
        !alice
            .shown_texts()
            .iter()
            .any(|t| t.to_lowercase().contains("place") || t.contains("of 7"))
    );
    let cancelled = alice.world().non_send::<MenuCore>().view().searches[0].entry;
    click(&mut alice, "cancel search");
    alice.until("the real queue cancelled", |c| {
        c.world().non_send::<MenuCore>().view().searches.is_empty()
    });
    assert!(alice.shown_texts().iter().any(|t| t == "Search cancelled"));
    act(&mut alice, Act::Cancel(cancelled));
    alice.until("duplicate cancel explained", |c| {
        c.shown_texts()
            .iter()
            .any(|t| t == "This search has already stopped")
    });
    click(&mut alice, "play");
    click(&mut alice, "find match");
    act(
        &mut alice,
        Act::Find(bota_radovid::proto::RulesetId("full".into())),
    );
    alice.until("both searches queued", |c| {
        c.world().non_send::<MenuCore>().view().searches.len() == 2
    });
    let mut bob = signed_up(&service, "bob");
    act(
        &mut bob,
        Act::Play(PlayAction::Select(bota_radovid::proto::RulesetId(
            "full".into(),
        ))),
    );
    act(
        &mut bob,
        Act::Play(PlayAction::Select(bota_radovid::proto::RulesetId(
            "solo_mid".into(),
        ))),
    );
    act(&mut bob, Act::Play(PlayAction::Open(true)));
    act(&mut bob, Act::Play(PlayAction::Bots));
    click(&mut bob, "find match");
    alice.until("match found", |c| {
        c.world().non_send::<MenuCore>().view().offer.is_some()
    });
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .searches
            .iter()
            .any(|s| !s.offered),
        "the unrelated search remains live under the modal"
    );
    alice.frames(3);
    assert!(!alice.shown_texts().iter().any(|t| t == "FINDING MATCH"));
    click(&mut alice, "accept");
    alice.until("acceptance reported by the service", |c| {
        c.world()
            .non_send::<MenuCore>()
            .view()
            .offer
            .is_some_and(|o| o.answered && o.accepted == 1)
    });
    alice.frames(4);
    let squares = named(&mut alice, "accept-player-");
    assert_eq!(squares.len(), 2, "the service's 1v1 has two participants");
    assert_eq!(
        squares
            .iter()
            .filter(|(_, color)| *color == crate::theme::GO.0)
            .count(),
        1
    );
    assert!(!alice.shown_texts().iter().any(|t| t == "FINDING MATCH"));
    bob.until("proposal received", |c| {
        c.world().non_send::<MenuCore>().view().offer.is_some()
    });
    let declined = bob
        .world()
        .non_send::<MenuCore>()
        .view()
        .offer
        .expect("offer")
        .proposal;
    click(&mut bob, "decline");
    alice.until("decline clears the proposal", |c| {
        c.world().non_send::<MenuCore>().view().offer.is_none()
    });
    act(&mut bob, Act::Decline(declined));
    bob.until("duplicate decline explained", |c| {
        c.shown_texts()
            .iter()
            .any(|t| t == "This match offer is no longer available")
    });
}

#[test]
fn play_requires_capacity_for_all_selected_queues() {
    let mut client = Client::run(&["--menu-preview", "play"]);
    client.frames(8);
    {
        let mut preview = client.world_mut().resource_mut::<Preview>();
        if let crate::main_menu::PageView::Home { in_flight, .. } = &mut preview.0.body {
            *in_flight = (1, Some(bota_radovid::proto::AccountGameLimit::Limited(2)));
        }
        preview.0.play.selected = Some(vec![
            bota_radovid::proto::RulesetId("solo_mid".into()),
            bota_radovid::proto::RulesetId("all_pick".into()),
        ]);
    }
    client.frames(4);
    let world = client.world_mut();
    assert!(
        world
            .query::<(&Name, &crate::main_menu::Disabled)>()
            .iter(world)
            .any(|(name, _)| name.as_str() == "find match")
    );
}

#[test]
fn the_address_shortcut_focuses_the_address_field() {
    let mut client = super::menu::local_menu();
    click(&mut client, "open play");
    click(&mut client, "join by address");
    let view = client.world().non_send::<MenuCore>().view();
    let crate::main_menu::PageView::Form { fields, .. } = view.body else {
        panic!("join form")
    };
    assert!(
        fields[1].focused,
        "the join shortcut focuses the address, not the port"
    );
}

#[test]
fn play_preview_keeps_the_380_pixel_drawer_and_accept_window_at_the_design_scale() {
    let mut client = Client::run(&["--menu-preview", "play"]);
    super::menu::resize_menu_window(&mut client, UVec2::new(1440, 900));
    client.frames(8);
    assert!(
        (client.world().resource::<UiScale>().0 - 1.0).abs() < 0.001,
        "menu previews use menu scaling"
    );
    let world = client.world_mut();
    let node = world
        .query::<(&Name, &ComputedNode)>()
        .iter(world)
        .find(|(name, _)| name.as_str() == "play drawer")
        .map(|(_, node)| *node)
        .expect("drawer");
    assert!((node.size().x - 380.0).abs() < 0.5);
}

#[test]
fn loading_chat_stays_in_the_lower_left_and_sends_through_the_real_game_connection() {
    let server = super::harness::Server::start(1, &["--pick-secs", "300"]);
    let mut client = Client::run(&["--addr", &server.address]);
    client
        .world_mut()
        .resource_mut::<crate::loading::Parts>()
        .set("held for chat", 0, 1);
    client.until("loading with the real game welcomed", |c| {
        c.core().can_say() && c.world().resource::<crate::loading::Loading>().shown
    });
    client.frames(5);
    assert!(client.shown_texts().iter().any(|t| t == "To all: message"));
    click(&mut client, "chat input");
    assert!(*client.world().resource::<crate::input::Typing>() == crate::input::Typing::Chat);
    use bevy::input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    };
    use bevy::window::PrimaryWindow;
    let window = {
        let world = client.world_mut();
        world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .expect("window")
    };
    for (key_code, logical_key, text) in [
        (
            KeyCode::KeyG,
            Key::Character("g".into()),
            Some("gl hf".into()),
        ),
        (KeyCode::Enter, Key::Enter, None),
    ] {
        client.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key,
            text,
            state: ButtonState::Pressed,
            repeat: false,
            window,
        });
        client.frames(1);
    }
    client.until("the loading message echoed by the game", |c| {
        c.core().chat_lines().any(
            |line| matches!(&line.heard.line.body, bota_proto::Body::Text(text) if text == "gl hf"),
        )
    });
    client.frames(4);
    let world = client.world_mut();
    assert!(
        world
            .query::<(&TextSpan, &ChildOf)>()
            .iter(world)
            .any(|(text, parent)| {
                text.0 == "gl hf"
                    && world
                        .get::<InheritedVisibility>(parent.parent())
                        .is_some_and(|seen| seen.get())
                    && world
                        .get::<Node>(parent.parent())
                        .is_some_and(|node| node.display != Display::None)
            }),
        "the echoed body is in the visible chat log"
    );
    // Only the local bar knows asset progress; another seat is never shown as 64% loaded.
    assert_eq!(named(&mut client, "loading progress").len(), 1);
    client
        .world_mut()
        .resource_mut::<crate::loading::Parts>()
        .set("held for chat", 1, 1);
    client.until("the loading screen lifted", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    client.frames(4);
    let log = named(&mut client, "chat log");
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0].1.to_srgba().alpha,
        0.0,
        "the loading chat background does not persist into the game"
    );
}

#[test]
fn play_on_a_platform_without_hosting_offers_joining_without_a_host_button() {
    let mut client = Client::run(&["--menu-preview", "play-offline"]);
    client.world_mut().resource_mut::<Preview>().0.can_host = false;
    client.frames(8);
    let texts = client.shown_texts();
    assert!(texts.iter().any(|t| t == "JOIN BY ADDRESS"));
    assert!(!texts.iter().any(|t| t == "HOST A LOCAL LOBBY"));
    assert!(!texts.iter().any(|t| t == "MATCHMAKING"));
}

#[test]
fn the_play_tab_opens_the_drawer_while_a_game_owns_the_corner() {
    let service = crate::main_menu::tests::service();
    let server = super::harness::Server::start(1, &[]);
    let mut client = signed_up(&service, "alice");
    act(&mut client, Act::Go(crate::main_menu::Page::LocalLobby));
    act(&mut client, Act::Focus(1));
    act(&mut client, Act::Type(server.address.clone()));
    act(&mut client, Act::Direct(false));
    if matches!(
        client.world().non_send::<MenuCore>().view().body,
        crate::main_menu::PageView::Form {
            plain_host: Some(_),
            ..
        }
    ) {
        act(&mut client, Act::PlainAnyway);
    }
    client.until("the direct game opened", |c| {
        c.world().non_send::<MenuCore>().in_game()
    });
    act(&mut client, Act::Dashboard);
    assert!(client.world().non_send::<MenuCore>().view().game.is_some());
    let world = client.world_mut();
    let tab = world
        .query::<(Entity, &crate::main_menu::Wash)>()
        .iter(world)
        .find(|(_, wash)| {
            world
                .get::<Text>(wash.target)
                .is_some_and(|text| text.0 == "PLAY")
        })
        .map(|(entity, _)| entity)
        .expect("the PLAY tab");
    let at = client.centre_of(tab).expect("the tab laid out");
    client.mouse(at);
    client.click(MouseButton::Left);
    client.frames(4);
    assert!(
        client.world().non_send::<MenuCore>().view().play.open,
        "PLAY opens the same drawer when RETURN TO GAME occupies the corner"
    );
    click(&mut client, "dbota");
    assert!(
        !client.world().non_send::<MenuCore>().view().play.open,
        "the logo returns to Home with the drawer closed"
    );
}

#[test]
fn direct_join_keeps_its_play_button_when_the_offline_play_tab_is_present() {
    let server = super::harness::Server::start(1, &[]);
    let mut client = super::menu::local_menu();
    let world = client.world_mut();
    let actions: Vec<_> = world
        .query::<(&Name, &crate::main_menu::MenuButton)>()
        .iter(world)
        .filter(|(name, _)| name.as_str() == "play")
        .map(|(_, button)| button.0.clone())
        .collect();
    assert_eq!(
        actions,
        vec![Act::Direct(false)],
        "the direct PLAY has one unambiguous script target"
    );
    act(&mut client, Act::Focus(1));
    act(&mut client, Act::Type(server.address.clone()));
    click(&mut client, "play");
    if matches!(
        client.world().non_send::<MenuCore>().view().body,
        crate::main_menu::PageView::Form {
            plain_host: Some(_),
            ..
        }
    ) {
        act(&mut client, Act::PlainAnyway);
    }
    client.until("the direct join reached the game", |c| {
        c.world().non_send::<MenuCore>().in_game()
    });
}

#[test]
fn the_bot_switch_supports_both_audiences_without_duplicating_the_finding_board() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    click(&mut alice, "play");
    click(&mut alice, "find match");
    assert!(
        alice
            .world()
            .non_send::<MenuCore>()
            .view()
            .status
            .is_none_or(|status| !status.error),
        "the service supports audience-aware queue requests"
    );
    alice.until("the humans-only queue entered with bots off", |c| {
        c.world().non_send::<MenuCore>().view().searches.len() == 1
    });
    alice.frames(3);
    assert!(
        !alice
            .shown_texts()
            .iter()
            .any(|text| text.contains("with bots"))
    );
    click(&mut alice, "cancel search");
    alice.until("the human search cancelled", |c| {
        c.world().non_send::<MenuCore>().view().searches.is_empty()
    });
    click(&mut alice, "play");
    click(&mut alice, "allow matchmaking with bots");
    click(&mut alice, "find match");
    alice.until("both audiences entered as one search", |c| {
        c.world().non_send::<MenuCore>().view().searches.len() == 1
    });
    alice.frames(3);
    assert!(
        alice
            .shown_texts()
            .iter()
            .any(|text| text.ends_with(" · with bots")),
        "a grouped search includes its Everyone audience before an offer exists"
    );
    let mut bob = signed_up(&service, "bob");
    act(&mut bob, Act::Play(PlayAction::Open(true)));
    act(&mut bob, Act::Play(PlayAction::Bots));
    click(&mut bob, "find match");
    alice.until("a match with the selected audience", |c| {
        c.world().non_send::<MenuCore>().view().offer.is_some()
    });
    assert_eq!(
        alice.world().non_send::<MenuCore>().view().searches.len(),
        1
    );
}

#[test]
fn unknown_limits_show_neither_an_invented_timeline_nor_a_decline_penalty() {
    let mut client = Client::run(&["--menu-preview", "match-decline"]);
    {
        let mut preview = client.world_mut().resource_mut::<Preview>();
        let offer = preview.0.offer.as_mut().expect("offer");
        offer.left_permille = None;
        offer.decline_cooldown_ms = None;
    }
    client.frames(8);
    assert!(named(&mut client, "accept time").is_empty());
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.contains("bans you"))
    );
    client
        .world_mut()
        .resource_mut::<Preview>()
        .0
        .offer
        .as_mut()
        .expect("offer")
        .decline_cooldown_ms = Some(0);
    client.frames(3);
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.contains("bans you"))
    );
}

#[test]
fn unavailable_matchmaking_draws_its_reason_without_counts_and_recovers() {
    for (preview, message) in [
        ("play-full", "Matchmaking is full"),
        ("play-cooldown", "You declined a match"),
    ] {
        let mut client = Client::run(&["--menu-preview", preview]);
        client.frames(8);
        assert!(client.shown_texts().iter().any(|text| text == message));
        assert!(named(&mut client, "matchmaking unavailable").len() == 1);
        if preview == "play-cooldown" {
            assert!(client.shown_texts().iter().any(|text| text == "3:42"));
        }
        {
            let mut view = client.world_mut().resource_mut::<Preview>();
            view.0.queue_full = false;
            view.0.cooldown_left = None;
            if let crate::main_menu::PageView::Home { cooldown, .. } = &mut view.0.body {
                *cooldown = None;
            }
        }
        client.frames(4);
        assert!(named(&mut client, "matchmaking unavailable").is_empty());
        let world = client.world_mut();
        assert!(
            world
                .query::<(&Name, &crate::main_menu::MenuButton)>()
                .iter(world)
                .any(|(name, _)| name.as_str() == "find match")
        );
    }
}

#[test]
fn account_capacity_draws_the_game_reason_and_return_action_without_counts() {
    let mut client = Client::run(&["--menu-preview", "play"]);
    {
        let mut preview = client.world_mut().resource_mut::<Preview>();
        preview.0.game = Some(crate::main_menu::GameKind::Play);
        if let crate::main_menu::PageView::Home { in_flight, .. } = &mut preview.0.body {
            *in_flight = (1, Some(bota_radovid::proto::AccountGameLimit::Limited(1)));
        }
    }
    client.frames(8);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "You are in a game already")
    );
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "RETURN TO GAME")
    );
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.contains("Games going:"))
    );
}

#[test]
fn overloaded_finding_draws_the_gold_title_and_bot_audience() {
    let mut client = Client::run(&["--menu-preview", "play-searching-bots-overloaded"]);
    client.frames(8);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "SERVERS FULL")
    );
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text.ends_with("· with bots"))
    );
    let world = client.world_mut();
    assert!(
        world
            .query::<(&Text, &TextColor)>()
            .iter(world)
            .any(|(text, ink)| text.0 == "SERVERS FULL" && ink.0 == crate::theme::GOLD)
    );
}

#[test]
fn offline_card_and_loading_chat_use_local_name_and_designed_fonts() {
    let mut client = Client::run(&["--menu-preview", "play-offline"]);
    client.frames(8);
    assert!(client.shown_texts().iter().any(|text| text == "Player"));
    assert!(!client.shown_texts().iter().any(|text| text == "mira#2207"));
    drop(client);
    let mut client = Client::run(&["--menu-preview", "loading"]);
    client.frames(8);
    let world = client.world_mut();
    let font = world
        .resource::<crate::ui::Fonts>()
        .face(crate::theme::Face::Regular)
        .clone();
    assert!(
        world
            .query::<(&Name, &TextColor, &TextFont)>()
            .iter(world)
            .any(|(name, color, text_font)| name.as_str() == "chat input"
                && color.0 == crate::theme::INK_5
                && text_font.font == font.clone().into())
    );
}

#[test]
fn selection_draws_an_inset_cyan_border_without_glow() {
    let mut client = Client::run(&["--menu-preview", "play"]);
    client.frames(8);
    let world = client.world_mut();
    let (entity, node, color) = world
        .query::<(Entity, &Name, &Node, &BorderColor)>()
        .iter(world)
        .find(|(_, name, ..)| name.as_str() == "queue inset outline")
        .map(|(e, _, n, c)| (e, n, c))
        .expect("selection outline");
    assert_eq!(node.border, UiRect::all(px(1)));
    assert_eq!(*color, BorderColor::all(crate::theme::SELECT));
    assert!(world.get::<BoxShadow>(entity).is_none());
}

#[test]
fn admission_tooltip_is_available_by_hover_keyboard_and_accessibility() {
    let mut client = Client::run(&["--menu-preview", "play-full"]);
    client.frames(8);
    let world = client.world_mut();
    let (entity, accessible) = world
        .query::<(Entity, &Name, &bevy::a11y::AccessibilityNode)>()
        .iter(world)
        .find(|(_, name, _)| name.as_str() == "find match")
        .map(|(e, _, node)| (e, node))
        .expect("disabled action");
    assert_eq!(accessible.description(), Some("Matchmaking is full"));
    assert!(accessible.is_disabled());
    let centre = client.centre_of(entity).expect("laid out");
    client.mouse(centre);
    client.frames(3);
    assert_eq!(
        client
            .shown_texts()
            .iter()
            .filter(|s| s.as_str() == "Matchmaking is full")
            .count(),
        2
    );
    client.mouse(Vec2::new(500.0, 400.0));
    client.frames(3);
    client.key(KeyCode::Tab, bevy::input::ButtonState::Pressed);
    client.key(KeyCode::Tab, bevy::input::ButtonState::Released);
    client.frames(2);
    assert_ne!(
        client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(entity)
    );
    assert_eq!(
        client
            .shown_texts()
            .iter()
            .filter(|s| s.as_str() == "Matchmaking is full")
            .count(),
        1
    );
    let mut reached = false;
    for _ in 0..64 {
        client.key(KeyCode::Tab, bevy::input::ButtonState::Pressed);
        client.key(KeyCode::Tab, bevy::input::ButtonState::Released);
        client.frames(2);
        if client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get()
            == Some(entity)
        {
            reached = true;
            break;
        }
    }
    assert!(
        reached,
        "Tab reaches the disabled action after the other controls"
    );
    assert_eq!(
        client
            .shown_texts()
            .iter()
            .filter(|s| s.as_str() == "Matchmaking is full")
            .count(),
        2
    );
    client.key(KeyCode::Tab, bevy::input::ButtonState::Pressed);
    client.key(KeyCode::Tab, bevy::input::ButtonState::Released);
    client.frames(2);
    assert_ne!(
        client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(entity)
    );
    assert_eq!(
        client
            .shown_texts()
            .iter()
            .filter(|s| s.as_str() == "Matchmaking is full")
            .count(),
        1
    );
}

#[test]
fn no_account_card_draws_the_local_settings_name() {
    let mut client = super::menu::local_menu();
    client
        .world_mut()
        .resource_mut::<crate::config::Settings>()
        .name = "Local friend".into();
    client.frames(5);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Local friend")
    );
}

#[test]
fn too_many_selected_modes_explains_the_selection_without_claiming_an_active_game() {
    let mut client = Client::run(&["--menu-preview", "play"]);
    {
        let mut preview = client.world_mut().resource_mut::<Preview>();
        if let crate::main_menu::PageView::Home { in_flight, .. } = &mut preview.0.body {
            *in_flight = (0, Some(bota_radovid::proto::AccountGameLimit::Limited(1)));
        }
        preview.0.play.selected = Some(vec![
            bota_radovid::proto::RulesetId("solo_mid".into()),
            bota_radovid::proto::RulesetId("all_pick".into()),
        ]);
    }
    client.frames(8);
    let texts = client.shown_texts();
    assert!(texts.iter().any(|text| text == "Select fewer game modes"));
    assert!(
        !texts
            .iter()
            .any(|text| text == "You are in a game already" || text == "RETURN TO GAME")
    );
    let world = client.world_mut();
    let (entity, accessible) = world
        .query::<(Entity, &Name, &bevy::a11y::AccessibilityNode)>()
        .iter(world)
        .find(|(_, name, _)| name.as_str() == "find match")
        .map(|(e, _, node)| (e, node))
        .expect("disabled action");
    assert_eq!(accessible.description(), Some("Select fewer game modes"));
    assert!(world.get::<crate::main_menu::MenuButton>(entity).is_none());
    let world = client.world_mut();
    let children = world
        .query::<(&Name, &Children)>()
        .iter(world)
        .find(|(name, _)| name.as_str() == "matchmaking unavailable")
        .map(|(_, children)| children)
        .expect("strip text");
    assert_eq!(children.len(), 1);
}

#[test]
fn typing_in_chat_cannot_activate_a_focused_menu_button() {
    let service = crate::main_menu::tests::service();
    let server = super::harness::Server::start(1, &[]);
    let mut client = signed_up(&service, "alice");
    act(&mut client, Act::Go(crate::main_menu::Page::LocalLobby));
    act(&mut client, Act::Focus(1));
    act(&mut client, Act::Type(server.address.clone()));
    act(&mut client, Act::Direct(false));
    if matches!(
        client.world().non_send::<MenuCore>().view().body,
        crate::main_menu::PageView::Form {
            plain_host: Some(_),
            ..
        }
    ) {
        act(&mut client, Act::PlainAnyway);
    }
    client.until("the direct game opened", |c| {
        c.world().non_send::<MenuCore>().in_game()
    });
    client.until("the game can open chat", |client| client.core().can_say());
    act(&mut client, Act::Dashboard);
    let world = client.world_mut();
    let tab = world
        .query::<(Entity, &crate::main_menu::Wash)>()
        .iter(world)
        .find(|(_, wash)| {
            world
                .get::<Text>(wash.target)
                .is_some_and(|text| text.0 == "PLAY")
        })
        .map(|(entity, _)| entity)
        .expect("the PLAY tab");
    let at = client.centre_of(tab).expect("the tab laid out");
    client.mouse(at);
    client.click(MouseButton::Left);
    client.frames(4);
    click(&mut client, "allow matchmaking with bots");
    assert!(
        client.world().non_send::<MenuCore>().view().play.bots,
        "control: the mouse click turns the switch on"
    );
    assert!(
        client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get()
            .is_some(),
        "diagnostic: the press leaves keyboard focus on a control"
    );
    *client.world_mut().resource_mut::<crate::input::Typing>() = crate::input::Typing::Chat;
    focus_bot_toggle(&mut client);
    press_menu_key(&mut client, KeyCode::Space);
    client.frames(4);
    assert!(
        client.world().non_send::<MenuCore>().view().play.bots,
        "Typing into chat must not toggle the focused switch"
    );
    assert!(*client.world().resource::<crate::input::Typing>() == crate::input::Typing::Chat);
    for key in [KeyCode::Space, KeyCode::Enter] {
        *client.world_mut().resource_mut::<crate::input::Typing>() = crate::input::Typing::Chat;
        focus_bot_toggle(&mut client);
        press_menu_key(&mut client, key);
        assert!(
            client.world().non_send::<MenuCore>().view().play.bots,
            "typing cannot activate a menu control"
        );
    }
}

#[test]
fn enter_and_space_activate_the_focused_menu_button() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    click(&mut client, "play");
    click(&mut client, "allow matchmaking with bots");
    for (key, expected) in [(KeyCode::Enter, false), (KeyCode::Space, true)] {
        focus_bot_toggle(&mut client);
        press_menu_key(&mut client, key);
        assert_eq!(
            client.world().non_send::<MenuCore>().view().play.bots,
            expected
        );
    }
}

fn focus_bot_toggle(client: &mut Client) {
    let world = client.world_mut();
    let entity = world
        .query::<(Entity, &crate::main_menu::MenuButton)>()
        .iter(world)
        .find(|(_, button)| matches!(button.0, Act::Play(PlayAction::Bots)))
        .map(|(entity, _)| entity)
        .expect("bot toggle");
    world
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(entity, bevy::input_focus::FocusCause::Navigated);
}

fn press_menu_key(client: &mut Client, key: KeyCode) {
    use bevy::{
        input::{
            ButtonState,
            keyboard::{Key, KeyboardInput},
        },
        window::PrimaryWindow,
    };
    let world = client.world_mut();
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .expect("window");
    for state in [ButtonState::Pressed, ButtonState::Released] {
        client.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: if key == KeyCode::Enter {
                Key::Enter
            } else {
                Key::Space
            },
            state,
            text: None,
            repeat: false,
            window,
        });
        client.frames(1);
    }
    client.frames(3);
}

#[test]
fn an_acknowledged_accept_stays_accepted_after_the_control_client_is_rebuilt() {
    let service = crate::main_menu::tests::service();
    let mut alice = signed_up(&service, "alice");
    let mut bob = signed_up(&service, "bob");
    for client in [&mut alice, &mut bob] {
        act(
            client,
            Act::Find(bota_radovid::proto::RulesetId("solo_mid".into())),
        );
    }
    alice.until("both players offered", |alice| {
        bob.frames(1);
        alice.world().non_send::<MenuCore>().view().offer.is_some()
            && bob.world().non_send::<MenuCore>().view().offer.is_some()
    });
    let proposal = alice
        .world()
        .non_send::<MenuCore>()
        .view()
        .offer
        .expect("offer")
        .proposal;
    act(&mut alice, Act::Accept(proposal));
    alice.until("accept acknowledged", |client| {
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .offer
            .is_some_and(|offer| offer.answered)
    });
    let _second_session = crate::main_menu::tests::keep_menu_session_alive(
        alice.world().non_send::<MenuCore>(),
        &service,
    );
    crate::main_menu::tests::lose_menu_control(&mut alice.world_mut().non_send_mut::<MenuCore>());
    act(&mut alice, Act::RetryService);
    alice.until("the rebuilt control client signed in", |client| {
        bob.frames(1);
        crate::main_menu::tests::menu_control_ready(client.world().non_send::<MenuCore>())
    });
    let offer = alice
        .world()
        .non_send::<MenuCore>()
        .view()
        .offer
        .expect("the service retained the proposal");
    assert_eq!(offer.proposal, proposal);
    assert!(offer.answered);
    assert!(!alice.shown_texts().iter().any(|text| text == "ACCEPT"));
    assert_eq!(
        named(&mut alice, "accept-player-")
            .iter()
            .filter(|(_, ink)| *ink == crate::theme::GO.0)
            .count(),
        1
    );
}
