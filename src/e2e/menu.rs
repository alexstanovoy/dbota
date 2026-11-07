//! The main menu: its buttons of a height one width, none wrapping; a refused lobby join that
//! moves nothing; Play's rulesets; Demo Hero on the service; the account's game limit.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::text::{ComputedTextBlock, LineBreak};
use bevy::ui_widgets::Button;
use bevy::window::{PrimaryWindow, WindowResized};

use crate::main_menu::{Act, MenuButton, MenuCore, Page, PlayAction};

use super::harness::{Client, Server};

/// The menu's buttons of words on screen, laid out: name, width, height, and whether their words
/// stay on one line.
fn buttons(client: &mut Client) -> Vec<(String, f32, f32, bool)> {
    let world = client.world_mut();
    let shown: Vec<(Entity, String, Vec2)> = world
        .query_filtered::<(Entity, &Name, &ComputedNode, &InheritedVisibility), With<Button>>()
        .iter(world)
        .filter(|(.., seen)| seen.get())
        .map(|(e, name, node, _)| (e, name.to_string(), node.size() * node.inverse_scale_factor))
        .collect();
    shown
        .into_iter()
        .filter_map(|(button, name, size)| {
            // A text field's box is a button too (named by its label for scripts), but it holds
            // what is typed, and clips it rather than lets it wrap.
            if matches!(
                world.get::<MenuButton>(button),
                Some(MenuButton(Act::Focus(_)))
            ) {
                return None;
            }
            // A button of the kit's tones (its `Shade`): the rule is theirs. Words alone (LOG OUT,
            // a link), tabs and icons are as wide as what they hold, as Dota's are.
            world.get::<crate::main_menu::Shade>(button)?;
            let children = world.get::<Children>(button)?;
            let words = children.first().copied().filter(|_| children.len() == 1)?;
            world.get::<Text>(words)?;
            let one_line = world
                .get::<TextLayout>(words)
                .is_some_and(|l| l.linebreak == LineBreak::NoWrap)
                && world.get::<ComputedTextBlock>(words).is_some();
            Some((name, size.x, size.y, one_line))
        })
        .collect()
}

/// The menu open with no service known: its local pages.
pub(super) fn local_menu() -> Client {
    let mut client = Client::run(&[]);
    client.within(Duration::from_secs(30), "the menu laid out", |c| {
        buttons(c).iter().any(|b| b.1 > 0.0)
    });
    client
}

/// Initial font loading may move a row after its first layout; aim at its settled drawn box.
fn hover_button(client: &mut Client, button: Entity) {
    client.within(
        Duration::from_secs(30),
        "the interface fonts loaded",
        |client| {
            let world = client.world();
            let Some(fonts) = world.get_resource::<crate::ui::Fonts>() else {
                return false;
            };
            let server = world.resource::<AssetServer>();
            server.is_loaded_with_dependencies(&fonts.bold)
                && server.is_loaded_with_dependencies(&fonts.regular)
        },
    );
    client.within(
        Duration::from_secs(30),
        "the pointer on the intended button",
        |client| {
            let Some(at) = client.centre_of(button) else {
                return false;
            };
            client.mouse(at);
            client
                .centre_of(button)
                .is_some_and(|now| now.distance(at) < 0.01)
                && client
                    .world()
                    .get::<bevy::picking::hover::Hovered>(button)
                    .is_some_and(|hover| hover.get())
        },
    );
}

pub(super) fn act(client: &mut Client, act: Act) {
    client.world_mut().non_send_mut::<MenuCore>().act(act);
    client.frames(5);
}

#[test]
fn leaving_game_permanently_removes_reconnect_but_cancelling_keeps_it() {
    for confirmed in [false, true] {
        let server = Server::start(1, &[]);
        let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
        act(&mut client, Act::Dashboard);
        server.frozen(|| {
            act(&mut client, Act::LeaveGame);
            assert!(
                client
                    .world()
                    .non_send::<MenuCore>()
                    .view()
                    .confirm
                    .is_some()
            );
            act(&mut client, Act::Answer(confirmed));
            if !confirmed {
                act(&mut client, Act::Disconnect);
            }
            client.until("back in the menu", |c| {
                !c.world().non_send::<MenuCore>().in_game()
            });
            client.frames(5);
            let reconnect = buttons(&mut client)
                .iter()
                .any(|button| button.0 == "reconnect");
            assert_eq!(reconnect, !confirmed, "only a disconnect offers RECONNECT");
        });
    }
}

/// A leave reached the server, but its echo was lost: the next Hello is refused permanently.
fn lost_leave_echo() -> (
    String,
    std::sync::mpsc::Receiver<()>,
    std::thread::JoinHandle<()>,
) {
    use bota_proto::{
        ClientMsg, FrameReader, PlayerId, RejectReason, ServerMsg, SlotId, TickMode,
        encode_frame_to_vec,
    };
    use std::io::Write;
    use std::net::{Shutdown, TcpListener};
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("bound").to_string();
    let (finished, ended) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        for attempt in 0..2 {
            let (mut peer, _) = listener.accept().expect("leave connects");
            peer.set_read_timeout(Some(Duration::from_secs(60)))
                .expect("hang guard");
            let mut reader = FrameReader::new();
            assert!(matches!(
                leave_message(&mut peer, &mut reader),
                ClientMsg::Hello { .. }
            ));
            let reply = if attempt == 0 {
                ServerMsg::Welcome {
                    player_id: PlayerId(1),
                    slot: Some(SlotId(0)),
                    tick_rate: 30,
                    mode: TickMode::Lockstep,
                }
            } else {
                ServerMsg::Refused {
                    reason: RejectReason::NotAdmitted,
                }
            };
            peer.write_all(&encode_frame_to_vec(&reply).expect("reply frame"))
                .expect("reply sent");
            if attempt == 0 {
                assert_eq!(leave_message(&mut peer, &mut reader), ClientMsg::LeaveGame);
                assert!(matches!(
                    leave_message(&mut peer, &mut reader),
                    ClientMsg::Probe { .. }
                ));
                peer.shutdown(Shutdown::Write).expect("close without echo");
            }
        }
        let _ = finished.send(());
    });
    (address, ended, thread)
}

fn leave_message(
    stream: &mut std::net::TcpStream,
    reader: &mut bota_proto::FrameReader,
) -> bota_proto::ClientMsg {
    use std::io::Read;
    loop {
        if let Some(msg) = reader.next_message().expect("valid client message") {
            return msg;
        }
        let mut bytes = [0; 4096];
        let read = stream.read(&mut bytes).expect("client speaks");
        assert!(read > 0, "client closed before its message");
        reader.push(&bytes[..read]);
    }
}

#[test]
fn leaving_game_with_a_lost_echo_shows_that_the_player_left() {
    let (address, ended, server) = lost_leave_echo();
    let mut client = local_menu();
    {
        // Restore a disconnected direct seat; the leave itself goes through the real shell,
        // network worker and rendered menu rather than launching another game on screen.
        let mut menu = client.world_mut().non_send_mut::<MenuCore>();
        menu.act(Act::Go(Page::LocalLobby));
        menu.act(Act::Focus(1));
        menu.act(Act::Type(address));
        menu.act(Act::Direct(false));
        menu.take_launch().expect("the disconnected seat's launch");
        menu.game_started();
        menu.left_game(None, false);
    }
    client.frames(5);
    act(&mut client, Act::LeaveSeat);
    act(&mut client, Act::Answer(true));
    client.until("leave result rendered", |c| {
        c.shown_texts().iter().any(|line| {
            line == "You left the game" || line == "The game holds no seat of yours any more"
        })
    });
    ended
        .recv_timeout(Duration::from_secs(60))
        .expect("the server completed both attempts");
    server.join().expect("the leave server ran");
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|line| line == "You left the game"),
        "a refusal after the surrender confirms the leave: {:?}",
        client.shown_texts()
    );
    assert!(
        !buttons(&mut client)
            .iter()
            .any(|button| button.0 == "reconnect")
    );
}

/// The in-process client lists and keeps replays in a folder of its own, never the user's.
#[test]
fn each_client_uses_its_own_replay_directory() {
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Replays));
    let folder = client.replay_dir().display().to_string();
    let shown = client.shown_texts();
    assert!(shown.iter().any(|text| text.contains(&folder)), "{shown:?}");
    let root = crate::test_support::target_dir().join("e2e");
    assert!(client.replay_dir().starts_with(&root));
    let other = local_menu();
    assert!(other.replay_dir().starts_with(root));
    assert_ne!(PathBuf::from(folder), other.replay_dir());
}

/// Every page's buttons of one height are laid out one width, whatever they say, and their
/// words never wrap (a longer label made a wider button, a long one wrapped onto two lines);
/// a lone glyph's button (the close cross) is square.
#[test]
fn the_menus_buttons_of_a_height_are_one_width_and_never_wrap() {
    let mut client = local_menu();
    for page in [Page::LocalLobby, Page::Replays] {
        uniform_buttons(&mut client, page);
    }
    // The lobbies are the service's: an account's.
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "carol");
    uniform_buttons(&mut client, Page::Lobbies);
}

/// A replay with a long name leaves the buttons of its row their width: the row's words do not
/// squeeze them (the owner's replay folder, with a recording of another version, did). The
/// replays are in the folder the client keeps its own in, before its library first reads it.
#[test]
fn a_replays_long_name_leaves_the_buttons_of_its_row_their_width() {
    let long = format!("20261106-090826-{}.brp", "long-".repeat(15));
    let short = "20261106-091018-local.brp";
    let index = format!(
        "dbota-replays 1\nr\t1793956218\t12\t-\tlocal\t{short}\t\t\nr\t1793956106\t12\t-\tlocal\t{long}\t\t\n"
    );
    let files = [
        ("replays/index.txt", index.as_str()),
        ("replays/20261106-091018-local.brp", "not a replay"),
        (&format!("replays/{long}"), "not a replay"),
    ];
    let mut client = Client::run_with_files(&[], &files);
    client.within(Duration::from_secs(30), "the menu laid out", |c| {
        buttons(c).iter().any(|b| b.1 > 0.0)
    });
    act(&mut client, Act::Go(Page::Replays));
    client.within(Duration::from_secs(30), "both replays listed", |c| {
        let names: Vec<String> = buttons(c).into_iter().map(|b| b.0).collect();
        names.contains(&format!("delete {long}")) && names.contains(&format!("delete {short}"))
    });
    uniform_buttons(&mut client, Page::Replays);
}

/// The buttons of `page` that show are one width by height and their words stay on one line.
fn uniform_buttons(client: &mut Client, page: Page) {
    act(client, Act::Go(page));
    let shown = buttons(client);
    assert!(!shown.is_empty(), "{page:?}: buttons");
    let mut widths: BTreeMap<i32, Vec<(String, f32)>> = BTreeMap::new();
    let columns = {
        let world = client.world_mut();
        world
            .query::<(&Name, &Node)>()
            .iter(world)
            .filter(|(_, node)| matches!(node.width, Val::Percent(_)))
            .map(|(name, _)| name.to_string())
            .collect::<Vec<_>>()
    };
    for (name, w, h, one_line) in &shown {
        assert!(one_line, "{page:?}: '{name}' may wrap");
        // A lone glyph (the close cross) is a square of its own; words share their height's width.
        if name.chars().count() <= 2 {
            assert!(
                (w - h).abs() < 0.5,
                "{page:?}: '{name}' {w} by {h}, not square"
            );
        } else if !columns.contains(name) {
            widths
                .entry(h.round() as i32)
                .or_default()
                .push((name.clone(), *w));
        }
    }
    for (height, row) in widths {
        let first = row[0].1;
        assert!(
            row.iter().all(|(_, w)| (w - first).abs() < 0.5),
            "{page:?}: buttons {height} high of several widths: {row:?}"
        );
    }
}

/// The menu of a client signed up to a matchmaking service as `login`, on Play.
pub(super) fn signed_up(service: &crate::main_menu::tests::Service, login: &str) -> Client {
    // One file for each client: the tests of a process run side by side, each with a service
    // (and a certificate) of its own.
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let pem = crate::test_support::scratch_path(&format!(
        "service-{}-{}.pem",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&pem, &service.pem).expect("the service's certificate");
    let pem_arg = pem.to_str().expect("a UTF-8 path").to_owned();
    let mut client = Client::run(&["--service", &service.url, "--trust-cert", &pem_arg]);
    let page = |c: &Client| c.world().non_send::<MenuCore>().page();
    // A hang guard: the machine may run many other jobs.
    let patience = Duration::from_secs(240);
    client.within(patience, "the sign-in page", |c| page(c) == Page::Login);
    act(&mut client, Act::Go(Page::Register));
    let password = crate::main_menu::tests::PASSWORD;
    for (at, value) in [login, password, password].into_iter().enumerate() {
        act(&mut client, Act::Focus(at));
        act(&mut client, Act::Type(value.to_owned()));
    }
    act(&mut client, Act::Submit);
    client.within(patience, "signed up, on Play", |c| page(c) == Page::Home);
    client.frames(5);
    let _ = std::fs::remove_file(&pem);
    client
}

/// The redesigned dashboard names the player in its profile card and keeps account controls in the top bar.
#[test]
fn the_dashboard_names_the_player_without_an_account_number() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    let name = menu_rectangle(&mut client, "alice", false);
    let settings = menu_rectangle(&mut client, "settings", true);
    let log_out = menu_rectangle(&mut client, "log out", true);
    assert!(
        name.min.y > settings.max.y,
        "the player is in the lower profile card"
    );
    assert!(
        log_out.max.x < settings.min.x,
        "LOG OUT precedes the settings cog on the redesigned bar"
    );
    let shown = client.shown_texts();
    assert!(
        shown.iter().any(|text| text == "alice"),
        "the player name alone"
    );
    assert!(
        shown.iter().all(|text| !text
            .split('#')
            .skip(1)
            .any(|part| part.starts_with(|c: char| c.is_ascii_digit()))),
        "no account number is shown: {shown:?}"
    );
}

/// The drawer shows queueable rulesets and an explicit audience switch in place of the old card subtitle.
#[test]
fn play_shows_the_queued_rulesets_with_their_audience() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "bob");
    click_menu_button(&mut client, "play");
    let shown: Vec<String> = {
        let world = client.world_mut();
        let drawer = world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, name)| name.as_str() == "play drawer")
            .map(|(entity, _)| entity)
            .expect("the Play drawer");
        super::harness::subtree(world, drawer)
            .into_iter()
            .filter_map(|entity| world.get::<Text>(entity).map(|text| text.0.clone()))
            .collect()
    };
    assert!(
        shown.iter().any(|text| text == "1X1 MID ONLY"),
        "the queue's row: {shown:?}"
    );
    assert!(
        shown
            .iter()
            .any(|text| text == "Allow matchmaking with bots"),
        "the drawn audience control: {shown:?}"
    );
    assert!(
        !matchmaking_switch_is_on(&mut client),
        "human-only by default"
    );
    client.hover_ui_with(Duration::from_secs(30), |client| {
        let world = client.world_mut();
        world
            .query_filtered::<(Entity, &Name), With<Button>>()
            .iter(world)
            .find(|(_, name)| name.as_str() == "allow matchmaking with bots")
            .map(|(entity, _)| entity)
    });
    client.click(MouseButton::Left);
    client.within(
        Duration::from_secs(30),
        "the audience switch includes bots",
        matchmaking_switch_is_on,
    );
    for elsewhere in ["BOTS ARENA", "SQUAD", "PRACTICE", "DEMO HERO"] {
        assert!(
            !shown.iter().any(|text| text == elsewhere),
            "{elsewhere} is not a queue row: {shown:?}"
        );
    }
}

/// The actual physical rectangle of a visible menu button or line, after Bevy's text layout.
fn menu_rectangle(client: &mut Client, label: &str, button: bool) -> Rect {
    let world = client.world_mut();
    let entity = if button {
        world
            .query_filtered::<(Entity, &Name, &InheritedVisibility), With<Button>>()
            .iter(world)
            .find(|(_, name, shown)| shown.get() && name.as_str() == label)
            .map(|(entity, ..)| entity)
    } else {
        world
            .query::<(Entity, &Text, &InheritedVisibility)>()
            .iter(world)
            .find(|(_, text, shown)| shown.get() && text.0 == label)
            .map(|(entity, ..)| entity)
    }
    .unwrap_or_else(|| panic!("the visible menu element {label:?}"));
    let size = world.get::<ComputedNode>(entity).expect("laid out").size();
    let centre = world
        .get::<UiGlobalTransform>(entity)
        .expect("on screen")
        .translation;
    assert!(
        size.cmpgt(Vec2::ZERO).all(),
        "a nonempty rectangle for {label}"
    );
    Rect::from_center_size(centre, size)
}

fn lobby_rectangles(client: &mut Client) -> [Rect; 3] {
    ["create lobby", "lobby or host name", "refresh  ⟳"]
        .map(|label| menu_rectangle(client, label, true))
}

pub(super) fn visible_menu_button(client: &mut Client, target: &str) -> bool {
    let world = client.world_mut();
    world
        .query_filtered::<(&Name, &InheritedVisibility, &ComputedNode), With<Button>>()
        .iter(world)
        .any(|(name, shown, node)| {
            shown.get() && name.as_str() == target && node.size().cmpgt(Vec2::ZERO).all()
        })
}

pub(super) fn click_menu_button(client: &mut Client, name: &str) {
    let at = menu_rectangle(client, name, true).center();
    client.mouse(at);
    client.click(MouseButton::Left);
}

pub(super) fn resize_menu_window(client: &mut Client, size: UVec2) {
    let world = client.world_mut();
    let (window, width, height) = {
        let (entity, mut window) = world
            .query_filtered::<(Entity, &mut Window), With<PrimaryWindow>>()
            .single_mut(world)
            .expect("the primary window");
        window.resolution.set_physical_resolution(size.x, size.y);
        (entity, window.width(), window.height())
    };
    // The headless harness has no window backend to notify the cameras of the new size.
    world.write_message(WindowResized {
        window,
        width,
        height,
    });
    client.frames(5);
}

/// A second account creates the lobby; the client finds it through the list and join button.
fn open_password_dialog(
    client: &mut Client,
    host: &mut crate::main_menu::tests::Player,
    name: &str,
) {
    host.menu.act(Act::Go(Page::Lobbies));
    host.fill(&[name, crate::main_menu::tests::PASSWORD]);
    host.menu
        .act(Act::LobbyUi(crate::main_menu::LobbyAction::Save));
    client.within(Duration::from_secs(30), "the host's lobby created", |_| {
        host.menu.update();
        host.menu.page() == Page::Lobby
    });
    act(client, Act::Go(Page::Lobbies));
    let join = format!("join {name}");
    client.within(Duration::from_secs(30), "the locked lobby listed", |c| {
        visible_menu_button(c, &join)
    });
    click_menu_button(client, &join);
    assert!(
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .password
            .is_some()
    );
}

fn assert_error_in_bottom_band(client: &mut Client, error: Rect) {
    let world = client.world_mut();
    let size = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .expect("the primary window")
        .physical_size()
        .as_vec2();
    assert!(
        (error.center().x - size.x * 0.5).abs() <= size.x * 0.02,
        "bottom-centred error in {size:?}: {error:?}"
    );
    assert!(
        error.min.y >= size.y * 0.8 && error.max.y <= size.y,
        "the error lies in the window's bottom band in {size:?}: {error:?}"
    );
}

/// A wrong password stays in the dialog. A retry after the lobby closes exercises the shared
/// status overlay, so this still detects the original page-flow bug rather than only the popup.
fn refused_password_join_keeps_content_still(
    client: &mut Client,
    host: &mut crate::main_menu::tests::Player,
    name: &str,
) {
    open_password_dialog(client, host, name);
    let before = lobby_rectangles(client);
    act(client, Act::Type("wrong password".to_owned()));
    click_menu_button(client, "ok");
    client.within(
        Duration::from_secs(30),
        "the service refuses the password",
        |c| {
            c.world()
                .non_send::<MenuCore>()
                .view()
                .password
                .is_some_and(|p| p.error.as_deref() == Some("Wrong password: try again"))
        },
    );
    assert_eq!(
        before,
        lobby_rectangles(client),
        "content during the password refusal"
    );
    host.menu.act(Act::LeaveLobby);
    client.within(Duration::from_secs(30), "the lobby closed", |_| {
        host.menu.update();
        host.menu.page() != Page::Lobby
    });
    act(client, Act::Type("another password".to_owned()));
    click_menu_button(client, "ok");
    let error = "That lobby is gone";
    client.within(
        Duration::from_secs(30),
        "the service refuses the stale join",
        |c| {
            c.world()
                .non_send::<MenuCore>()
                .view()
                .status
                .is_some_and(|status| status.error && status.text == error)
        },
    );
    assert_status_overlay_keeps_content_still(client, before, error);
}

fn assert_status_overlay_keeps_content_still(client: &mut Client, before: [Rect; 3], error: &str) {
    client.frames(2);
    assert!(
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .password
            .is_none()
    );
    let during = lobby_rectangles(client);
    let error_rectangle = menu_rectangle(client, error, false);
    click_menu_button(client, "×");
    client.frames(2);
    assert!(
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .status
            .is_none(),
        "error dismissed"
    );
    let after = lobby_rectangles(client);
    eprintln!(
        "menu before: {before:?}\nmenu during: {during:?}\nmenu after: {after:?}\nerror: {error_rectangle:?}"
    );
    assert_eq!(
        before, during,
        "the error must not move content by even one pixel"
    );
    assert_eq!(before, after, "dismissing the error must not move content");
    assert_error_in_bottom_band(client, error_rectangle);
}

/// Real password-dialog joins keep the content fixed, including with the dashboard over a
/// running match. Both window sizes use their actual bounds for the error's bottom band.
#[test]
fn a_failed_lobby_join_overlays_the_bottom_without_moving_menu_content() {
    let service = crate::main_menu::tests::service();
    let mut host = crate::main_menu::tests::Player::new(Some(&service));
    host.register("host");
    let mut client = signed_up(&service, "alice");
    client.within(Duration::from_secs(30), "the menu fonts loaded", |c| {
        let fonts = c.world().resource::<crate::ui::Fonts>();
        let loaded = c.world().resource::<Assets<Font>>();
        fonts.all().all(|font| loaded.contains(font))
    });
    client.frames(2);
    for (over_game, size) in [
        (false, UVec2::new(1600, 900)),
        (true, UVec2::new(1280, 800)),
    ] {
        if over_game {
            act(&mut client, Act::Go(Page::DemoHero));
            act(&mut client, Act::Demo(bota_proto::HeroId::PUDGE.0));
            client.within(Duration::from_secs(120), "a real Demo match running", |c| {
                c.core().my_hero().is_some()
            });
            act(&mut client, Act::Dashboard);
        }
        resize_menu_window(&mut client, size);
        assert_eq!(client.world().non_send::<MenuCore>().in_game(), over_game);
        eprintln!("checking menu error: over_game={over_game}, window={size:?}");
        refused_password_join_keeps_content_still(
            &mut client,
            &mut host,
            if over_game { "live" } else { "menu" },
        );
        assert_eq!(client.world().non_send::<MenuCore>().in_game(), over_game);
    }
    act(&mut client, Act::Disconnect);
    client.until("the Demo match disconnected", |c| {
        !c.world().non_send::<MenuCore>().in_game()
    });
}

/// Play draws queueable modes and the bot toggle; lobby-only modes and Demo Hero live elsewhere.
#[test]
fn play_draws_queueable_rulesets_and_the_bot_toggle() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "bob");
    click_menu_button(&mut client, "play");
    let shown = client.shown_texts();
    assert!(
        shown.iter().any(|t| t == "1X1 MID ONLY"),
        "the queue's card: {shown:?}"
    );
    assert!(
        shown.iter().any(|t| t == "Allow matchmaking with bots"),
        "the audience control: {shown:?}"
    );
    let world = client.world_mut();
    let queues: Vec<_> = world
        .query::<(&MenuButton, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, shown)| shown.get())
        .filter_map(|(button, _)| match &button.0 {
            Act::Play(crate::main_menu::PlayAction::Select(id)) => Some(id.0.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        queues,
        ["solo_mid"],
        "only queueable modes appear as drawer rows"
    );
}

/// Dota's Demo Hero runs on the service like any game: the signed-in client asks for it from the
/// DEMO HERO page, the service starts its Demo ruleset's cheating game server, the client picks
/// the hero chosen and its panel's buttons and a cheat take effect in the match. Leaving ends it.
#[test]
fn demo_hero_runs_on_the_service_and_its_cheats_take_effect() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    act(&mut client, Act::Go(Page::DemoHero));
    act(&mut client, Act::Demo(bota_proto::HeroId::PUDGE.0));
    // No lobby and no countdown stand between the pick and the game: only the server starting,
    // so this guard is short (the lobby path's countdown alone was 5 s).
    client.within(Duration::from_secs(120), "the Demo Hero stands", |c| {
        c.core().my_hero().is_some()
    });
    assert!(
        client.world().non_send::<MenuCore>().page() == Page::Game,
        "straight into the game"
    );
    assert!(
        client.core().statics().is_some_and(|s| s.metadata().cheats),
        "the service's Demo is a match that cheats"
    );
    assert_eq!(
        client
            .world()
            .non_send::<MenuCore>()
            .demo_hero()
            .map(|h| h.0),
        Some(bota_proto::HeroId::PUDGE.0)
    );
    // The panel's own button, pressed as a person presses it.
    client.hover_ui_with(Duration::from_secs(30), |client| {
        let world = client.world_mut();
        world
            .query_filtered::<(Entity, &Name, &InheritedVisibility), With<Button>>()
            .iter(world)
            .find(|(_, name, seen)| seen.get() && name.as_str() == "demo levelmax")
            .map(|(e, ..)| e)
    });
    client.click(MouseButton::Left);
    client.until("level 30", |c| c.me().level >= 30);
    let own_gold = |c: &Client| {
        let core = c.core();
        core.view()
            .zip(core.my_slot)
            .and_then(|(view, slot)| view.player(slot))
            .and_then(|player| player.gold)
            .unwrap_or(0)
    };
    let gold = own_gold(&client);
    client.cheat(bota_proto::Cheat::Gold { amount: 5000 });
    client.until("the gold", |c| own_gold(c) >= gold + 5000);
    // Leaving ends the Demo: back on its page, with no match left to rejoin or to read.
    act(&mut client, Act::Dashboard);
    act(&mut client, Act::Disconnect);
    client.until("back on the Demo Hero page", |c| {
        let menu = c.world().non_send::<MenuCore>();
        menu.page() == Page::DemoHero && !menu.in_game()
    });
    client.until("the match over", |c| {
        c.world().non_send::<MenuCore>().games_running() == 0
    });
    let menu = client.world().non_send::<MenuCore>();
    assert_eq!(menu.page(), Page::DemoHero, "no result page for a Demo");
}

/// Settings use only left presses, one change per press, even when the pointer leaves while held.
#[test]
fn a_settings_toggle_ignores_right_clicks_and_changes_once_per_left_press() {
    use bevy::input::ButtonState;

    let mut client = Client::run(&["--settings", "options"]);
    let row = |client: &mut Client| {
        let world = client.world_mut();
        world
            .query::<(&Text, &ChildOf, &InheritedVisibility)>()
            .iter(world)
            .find(|(text, _, shown)| text.0 == "Damage numbers" && shown.get())
            .map(|(_, parent, _)| parent.parent())
    };
    client.within(
        Duration::from_secs(30),
        "the damage-numbers toggle laid out",
        |c| {
            row(c)
                .and_then(|entity| c.centre_of(entity))
                .is_some_and(|at| at != Vec2::ZERO)
        },
    );
    let toggle = row(&mut client).expect("the damage-numbers toggle");
    let enabled = |client: &Client| {
        client
            .world()
            .resource::<crate::config::Settings>()
            .aids
            .numbers
    };
    let initial = enabled(&client);
    hover_button(&mut client, toggle);
    client.click(MouseButton::Right);
    assert_eq!(
        enabled(&client),
        initial,
        "a right click leaves the setting alone"
    );

    client.button(MouseButton::Left, ButtonState::Pressed);
    assert_eq!(
        enabled(&client),
        !initial,
        "the setting changes on the left press"
    );
    client.frames(3);
    assert_eq!(
        enabled(&client),
        !initial,
        "holding does not activate repeatedly"
    );
    client.mouse(Vec2::ZERO);
    assert!(
        client.world().get::<bevy::ui::Pressed>(toggle).is_some(),
        "a held press survives leaving its row"
    );
    client.button(MouseButton::Left, ButtonState::Released);
    assert!(
        client.world().get::<bevy::ui::Pressed>(toggle).is_none(),
        "release outside clears the press"
    );
    assert_eq!(enabled(&client), !initial, "release does not activate");

    hover_button(&mut client, toggle);
    client.click(MouseButton::Left);
    assert_eq!(enabled(&client), initial, "a fresh press activates again");
    client.mouse(Vec2::ZERO);
    assert!(
        !client
            .world()
            .get::<bevy::picking::hover::Hovered>(toggle)
            .expect("the toggle tracks hover")
            .get(),
        "leaving clears hover"
    );
}

/// A modified click opens hotkey capture; only the next input becomes the binding.
#[test]
fn clicking_a_hotkey_with_control_does_not_bind_the_opening_click() {
    use bevy::input::ButtonState;

    let mut client = Client::run(&["--settings", "hotkeys"]);
    let at = crate::keys::index_of(crate::keys::Action::Stop).expect("the Stop binding");
    let field = |client: &mut Client| {
        let world = client.world_mut();
        world
            .query_filtered::<(Entity, &Name), With<Button>>()
            .iter(world)
            .find(|(_, name)| {
                name.as_str() == format!("key {}", crate::keys::BINDINGS[at].label.to_lowercase())
            })
            .map(|(entity, _)| entity)
    };
    client.within(
        Duration::from_secs(30),
        "the Stop hotkey laid out",
        |client| {
            field(client)
                .and_then(|entity| client.centre_of(entity))
                .is_some_and(|at| at != Vec2::ZERO)
        },
    );
    let before = client.world().resource::<crate::config::Settings>().keys[at];
    let target = field(&mut client).expect("the Stop hotkey");
    client.key(KeyCode::ControlLeft, ButtonState::Pressed);
    hover_button(&mut client, target);
    client.button(MouseButton::Left, ButtonState::Pressed);
    assert_eq!(
        client
            .world()
            .resource::<crate::settings::Screen>()
            .capturing,
        Some(at)
    );
    assert_eq!(
        client.world().resource::<crate::config::Settings>().keys[at],
        before,
        "the modified click opens capture without binding itself"
    );
    client.button(MouseButton::Left, ButtonState::Released);
    client.key(KeyCode::KeyK, ButtonState::Pressed);
    assert_eq!(
        client.world().resource::<crate::config::Settings>().keys[at],
        crate::keys::Chord::parse("Ctrl+K")
    );
    assert_eq!(
        client
            .world()
            .resource::<crate::settings::Screen>()
            .capturing,
        None
    );
    client.key(KeyCode::KeyK, ButtonState::Released);
    client.key(KeyCode::ControlLeft, ButtonState::Released);
}

/// Named clicks use the pointer observers even when a control is clicked again after its redraw.
#[test]
fn a_script_can_press_the_same_named_menu_toggle_twice() {
    let mut client = Client::run(&[
        "--script",
        "click:local lobby;click:enable cheats;click:enable cheats",
    ]);
    let initial = client.world().non_send::<MenuCore>().view().host_cheats;
    let mut previous = initial;
    let mut changes = 0;
    client.within(
        Duration::from_secs(30),
        "both scripted clicks on Enable cheats",
        |client| {
            let now = client.world().non_send::<MenuCore>().view().host_cheats;
            if now != previous {
                changes += 1;
                previous = now;
            }
            changes == 2
        },
    );
    assert_eq!(
        client.world().non_send::<MenuCore>().view().host_cheats,
        initial,
        "two named presses toggle the real menu option twice"
    );
    client.frames(3);
    let world = client.world_mut();
    assert!(
        world
            .query_filtered::<&Name, With<bevy::ui::Pressed>>()
            .iter(world)
            .all(|name| name.as_str() != "enable cheats"),
        "the scripted press is released"
    );
}

/// A secondary press leaves a button's ordinary hover style when its pointer leaves.
#[test]
fn a_right_hold_does_not_keep_a_menu_button_lit_after_the_pointer_leaves() {
    use bevy::input::ButtonState;

    let mut client = local_menu();
    act(&mut client, Act::Go(Page::LocalLobby));
    let world = client.world_mut();
    let button = world
        .query_filtered::<(Entity, &Name, &InheritedVisibility), With<Button>>()
        .iter(world)
        .find(|(_, name, shown)| name.as_str() == "create" && shown.get())
        .map(|(entity, ..)| entity)
        .expect("the Create button");
    let rest = world
        .get::<BackgroundGradient>(button)
        .expect("the button's rest style")
        .clone();
    hover_button(&mut client, button);
    assert_ne!(
        client
            .world()
            .get::<BackgroundGradient>(button)
            .expect("the hovered style")
            .clone(),
        rest
    );
    client.button(MouseButton::Right, ButtonState::Pressed);
    client.mouse(Vec2::ZERO);
    assert!(
        client.world().get::<bevy::ui::Pressed>(button).is_some(),
        "the native widget still records the held secondary press"
    );
    assert_eq!(
        client
            .world()
            .get::<BackgroundGradient>(button)
            .expect("the rest style")
            .clone(),
        rest,
        "a secondary hold does not retain the primary pressed highlight"
    );
    client.button(MouseButton::Right, ButtonState::Released);
}

/// A real press on a pickable label bubbles to its owning button exactly once.
#[test]
fn a_primary_press_on_a_pickable_text_child_activates_its_button_once() {
    use bevy::picking::{events::PointerPress, pointer::PointerButton};

    #[derive(Resource, Default)]
    struct Trace(Vec<(Entity, Entity, PointerButton)>, usize);

    let mut client = local_menu();
    act(&mut client, Act::Go(Page::LocalLobby));
    let world = client.world_mut();
    let button = world
        .query_filtered::<(Entity, &Name), With<Button>>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "enable cheats")
        .map(|(entity, _)| entity)
        .expect("the Enable cheats button");
    let ancestor = world
        .get::<ChildOf>(button)
        .expect("the button container")
        .parent();
    world.entity_mut(ancestor).insert(Button);
    let label = world
        .get::<Children>(button)
        .expect("its children")
        .iter()
        .find(|entity| {
            world
                .get::<Text>(*entity)
                .is_some_and(|text| text.0 == "Enable cheats")
        })
        .expect("the button's text child");
    world.entity_mut(label).insert((
        Pickable::default(),
        bevy::picking::hover::Hovered::default(),
    ));
    client.app.init_resource::<Trace>().add_observer(
        |event: On<PointerPress>, mut trace: ResMut<Trace>| {
            trace
                .0
                .push((event.original_event_target(), event.entity, event.button));
        },
    );
    client.world_mut().entity_mut(button).observe(
        |_: On<PointerPress>, mut trace: ResMut<Trace>| {
            trace.1 += 1;
        },
    );
    hover_button(&mut client, label);
    let initial = client.world().non_send::<MenuCore>().view().host_cheats;
    client.click(MouseButton::Left);
    assert_eq!(
        client.world().non_send::<MenuCore>().view().host_cheats,
        !initial,
        "the descendant's primary press changes the real menu option once"
    );
    let trace = &client.world().resource::<Trace>().0;
    assert!(
        trace
            .iter()
            .any(|&(original, target, button)| original == label
                && target == label
                && button == PointerButton::Primary),
        "the actual picking target was the text child: {trace:?}"
    );
    assert_eq!(
        trace
            .iter()
            .filter(|&&(original, target, kind)| original == label
                && target == button
                && kind == PointerButton::Primary)
            .count(),
        1,
        "the owning button sees the bubbled press once: {trace:?}"
    );
    assert_eq!(
        client.world().resource::<Trace>().1,
        1,
        "a separate observer on the owning button receives the child's press"
    );
}

/// A press hidden by the dashboard stays muted when it closes, while raw physical state is retained.
#[test]
fn a_gameplay_key_held_over_the_dashboard_does_not_leak_a_stop_order() {
    use super::harness::Server;
    use bevy::input::ButtonState;
    use bota_client_core::Intent;
    use bota_proto::{HeroId, Order, Target};

    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let sent = client.record_sent_orders();
    let last = |client: &Client| {
        client
            .core()
            .shown_orders
            .iter()
            .rev()
            .find(|order| Some(order.slot) == client.core().my_slot)
            .map(|order| order.order)
    };
    client.core_mut().act(Intent::Hold);
    assert_eq!(
        last(&client),
        Some(Order::Attack {
            target: Target::None
        }),
        "the setup Hold order sent"
    );
    let before = sent.get();
    assert_eq!(before, 1, "one setup order sent to the live socket");
    client.key(KeyCode::Escape, ButtonState::Pressed);
    client.key(KeyCode::Escape, ButtonState::Released);
    assert!(
        !client.world().non_send::<MenuCore>().shows_game(),
        "the dashboard covers the game"
    );
    client.key(KeyCode::KeyS, ButtonState::Pressed);
    client.frames(3);
    assert!(
        client
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyS),
        "the raw physical key remains held for interface use"
    );
    assert_eq!(
        sent.get(),
        before,
        "no Stop order while the dashboard covers the game"
    );
    assert!(
        !client
            .world()
            .resource::<crate::keys::Fired>()
            .pressed(crate::keys::Action::Stop)
    );

    client.key(KeyCode::Escape, ButtonState::Pressed);
    client.key(KeyCode::Escape, ButtonState::Released);
    client.frames(3);
    assert!(
        client.world().non_send::<MenuCore>().shows_game(),
        "the dashboard closed"
    );
    assert_eq!(
        sent.get(),
        before,
        "closing does not turn an old held key into a fresh press"
    );
    client.key(KeyCode::KeyS, ButtonState::Released);
    assert_eq!(sent.get(), before, "release alone sends no Stop order");
    client.key(KeyCode::KeyS, ButtonState::Pressed);

    assert_eq!(sent.get(), before + 1, "one order for the fresh press");
    assert_eq!(
        last(&client),
        Some(Order::Move {
            target: Target::None
        })
    );
    client.key(KeyCode::KeyS, ButtonState::Released);
}

#[test]
fn unlimited_account_limits_allow_find_match_to_queue_from_the_play_drawer() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    click_menu_button(&mut client, "play");
    act(&mut client, Act::Play(PlayAction::Bots));
    click_menu_button(&mut client, "find match");
    client.within(Duration::from_secs(30), "a real queue entry", |client| {
        client.world().non_send::<MenuCore>().view().searches.len() == 1
    });
    let view = client.world().non_send::<MenuCore>().view();
    assert!(matches!(
        view.body,
        crate::main_menu::PageView::Home {
            in_flight: (1, Some(bota_radovid::proto::AccountGameLimit::Unlimited)),
            ..
        }
    ));
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "FINDING MATCH")
    );
}

#[test]
fn allowing_matchmaking_with_bots_is_a_drawn_switch() {
    use bota_radovid::Audience;
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "alice");
    click_menu_button(&mut client, "play");
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Allow matchmaking with bots"),
        "Play must offer the matchmaking audience switch"
    );
    assert!(!matchmaking_switch_is_on(&mut client));
    let mut bot = waiting_matchmaking_bot(&service);
    search_without_bots_and_cancel(&mut client, &service);
    click_menu_button(&mut client, "play");
    click_menu_button(&mut client, "allow matchmaking with bots");
    assert!(matchmaking_switch_is_on(&mut client));
    assert_matchmaking_preference_survives_reload(&client, &service);
    click_menu_button(&mut client, "find match");
    client.within(
        Duration::from_secs(120),
        "a match with the waiting bot",
        |client| {
            for _ in 0..64 {
                if bot.poll().expect("bot link").is_none() {
                    break;
                }
            }
            client.world().non_send::<MenuCore>().view().offer.is_some()
        },
    );
    let snapshot = matchmaking_snapshot(&service, "alice");
    assert_eq!(snapshot.queued.len(), 2);
    assert_eq!(snapshot.queued[0].search, snapshot.queued[1].search);
    assert!(
        snapshot
            .queued
            .iter()
            .any(|entry| entry.audience == Audience::HumansOnly)
    );
    assert!(
        snapshot
            .queued
            .iter()
            .any(|entry| entry.audience == Audience::Everyone)
    );
    assert_eq!(snapshot.proposals.len(), 1);
    assert_eq!(
        client.world().non_send::<MenuCore>().view().searches.len(),
        1,
        "one search is drawn"
    );
    accept_matchmaking_with_bot(&mut client, &mut bot, &service);
}

#[test]
fn a_bot_account_finds_matches_only_in_everyone_with_either_switch_setting() {
    let service = crate::main_menu::tests::service();
    let mut client = signed_up(&service, "viewer");
    act(&mut client, Act::Logout);
    client.within(Duration::from_secs(120), "the login page", |client| {
        client.world().non_send::<MenuCore>().page() == Page::Login
    });
    let tls = bota_radovid::Tls::trust_pem(&service.pem).expect("certificate");
    let mut bot = bota_radovid::Client::open(&service.url, &tls).expect("registration connection");
    let seq = bot
        .register(
            "bot",
            crate::main_menu::tests::PASSWORD,
            bota_radovid::AccountKind::Bot,
            Some("fixture".into()),
        )
        .expect("register bot");
    matchmaking_reply(&mut bot, seq);
    act(&mut client, Act::Focus(0));
    for _ in 0.."viewer".chars().count() {
        act(&mut client, Act::Erase);
    }
    act(&mut client, Act::Type("bot".into()));
    act(&mut client, Act::Focus(1));
    act(
        &mut client,
        Act::Type(crate::main_menu::tests::PASSWORD.into()),
    );
    act(&mut client, Act::Submit);
    client.within(
        Duration::from_secs(120),
        "the bot account signed in",
        |client| client.world().non_send::<MenuCore>().page() == Page::Home,
    );
    for allow_bots in [false, true] {
        click_menu_button(&mut client, "play");
        if allow_bots {
            click_menu_button(&mut client, "allow matchmaking with bots");
        }
        click_menu_button(&mut client, "find match");
        client.within(
            Duration::from_secs(120),
            "the queue request answered",
            |client| {
                let view = client.world().non_send::<MenuCore>().view();
                !view.searches.is_empty() || view.status.as_ref().is_some_and(|status| status.error)
            },
        );
        let snapshot = matchmaking_snapshot(&service, "bot");
        assert_eq!(
            snapshot.queued.len(),
            1,
            "bot admission with allow_bots={allow_bots}: {:?}",
            client.shown_texts()
        );
        assert_eq!(
            snapshot.queued[0].audience,
            bota_radovid::Audience::Everyone
        );
        act(&mut client, Act::Go(Page::MyGames));
        click_menu_button(&mut client, "cancel");
        client.within(Duration::from_secs(120), "the search cancelled", |client| {
            client
                .world()
                .non_send::<MenuCore>()
                .view()
                .searches
                .is_empty()
        });
        act(&mut client, Act::Go(Page::Home));
    }
}

fn waiting_matchmaking_bot(service: &crate::main_menu::tests::Service) -> bota_radovid::Client {
    let tls = bota_radovid::Tls::trust_pem(&service.pem).expect("service certificate");
    let mut bot = bota_radovid::Client::open(&service.url, &tls).expect("bot connection");
    let seq = bot
        .register(
            "bot",
            crate::main_menu::tests::PASSWORD,
            bota_radovid::AccountKind::Bot,
            Some("fixture".into()),
        )
        .expect("register bot");
    matchmaking_reply(&mut bot, seq);
    let seq = bot
        .queue(&bota_radovid::RulesetId("solo_mid".into()))
        .expect("bot queue");
    matchmaking_reply(&mut bot, seq);
    bot
}

fn search_without_bots_and_cancel(client: &mut Client, service: &crate::main_menu::tests::Service) {
    click_menu_button(client, "find match");
    client.within(Duration::from_secs(30), "the human search", |client| {
        !client
            .world()
            .non_send::<MenuCore>()
            .view()
            .searches
            .is_empty()
    });
    let snapshot = matchmaking_snapshot(service, "alice");
    assert_eq!(snapshot.queued.len(), 1);
    assert_eq!(
        snapshot.queued[0].audience,
        bota_radovid::Audience::HumansOnly
    );
    assert!(
        snapshot.proposals.is_empty(),
        "the waiting bot cannot match with the switch off"
    );
    act(client, Act::Go(Page::MyGames));
    click_menu_button(client, "cancel");
    client.within(Duration::from_secs(30), "the search cancelled", |client| {
        client
            .world()
            .non_send::<MenuCore>()
            .view()
            .searches
            .is_empty()
    });
    act(client, Act::Go(Page::Home));
}

fn accept_matchmaking_with_bot(
    client: &mut Client,
    bot: &mut bota_radovid::Client,
    service: &crate::main_menu::tests::Service,
) {
    use bota_radovid::{ClientMsg, Reply};
    click_menu_button(client, "accept");
    let mut handed_off = false;
    client.within(
        Duration::from_secs(120),
        "the accepted match consumes both entries",
        |client| {
            for _ in 0..64 {
                match bot.poll().expect("bot link") {
                    Some(bota_radovid::Incoming::Event(bota_radovid::Event::MatchFound {
                        ..
                    })) => handed_off = true,
                    Some(_) => {}
                    None => break,
                }
            }
            handed_off
                && client
                    .world()
                    .non_send::<MenuCore>()
                    .view()
                    .searches
                    .is_empty()
        },
    );
    assert!(matchmaking_snapshot(service, "alice").queued.is_empty());
    assert!(bot.searches().entries().is_empty());
    let seq = bot.request(ClientMsg::MyMatches).expect("match snapshot");
    assert!(matches!(matchmaking_reply(bot, seq), Reply::Handoffs { list } if list.len() == 1));
}

fn matchmaking_reply(client: &mut bota_radovid::Client, seq: u32) -> bota_radovid::Reply {
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        if let Some(bota_radovid::Incoming::Reply { seq: got, result }) = client
            .wait(Duration::from_millis(50))
            .expect("control connection")
            && got == seq
        {
            return result.expect("request accepted");
        }
    }
    panic!("no reply to {seq}");
}

fn matchmaking_snapshot(
    service: &crate::main_menu::tests::Service,
    login: &str,
) -> bota_radovid::Activities {
    let tls = bota_radovid::Tls::trust_pem(&service.pem).expect("certificate");
    let mut client = bota_radovid::Client::open(&service.url, &tls).expect("snapshot connection");
    let seq = client
        .login(login, crate::main_menu::tests::PASSWORD, None)
        .expect("snapshot login");
    match matchmaking_reply(&mut client, seq) {
        bota_radovid::Reply::LoggedIn { activities, .. } => activities,
        other => panic!("login reply: {other:?}"),
    }
}

fn matchmaking_switch_is_on(client: &mut Client) -> bool {
    let world = client.world_mut();
    let track = world
        .query_filtered::<(&Name, &Children), With<Button>>()
        .iter(world)
        .find(|(name, _)| name.as_str() == "allow matchmaking with bots")
        .and_then(|(_, children)| children.first().copied())
        .expect("the drawn switch track");
    let knob = world
        .get::<Children>(track)
        .and_then(|children| children.first().copied())
        .expect("the drawn knob");
    let centre = |entity| {
        world
            .get::<UiGlobalTransform>(entity)
            .expect("the switch is laid out")
            .translation
            .x
    };
    centre(knob) > centre(track)
}

fn assert_matchmaking_preference_survives_reload(
    client: &Client,
    service: &crate::main_menu::tests::Service,
) {
    let menu = client.settings_folder_file("menu.conf");
    let session = client.settings_folder_file("session");
    let certificate = client
        .world()
        .resource::<crate::Args>()
        .config
        .as_ref()
        .expect("isolated config")
        .with_file_name("audience-service.pem");
    std::fs::write(&certificate, &service.pem).expect("test certificate");
    let mut resumed = Client::run_with_files(
        &[
            "--service",
            &service.url,
            "--trust-cert",
            certificate.to_str().expect("UTF-8 path"),
        ],
        &[("menu.conf", &menu), ("session", &session)],
    );
    resumed.within(
        Duration::from_secs(120),
        "the saved session resumed",
        |client| client.world().non_send::<MenuCore>().page() == Page::Home,
    );
    resumed.frames(5);
    click_menu_button(&mut resumed, "play");
    assert!(
        matchmaking_switch_is_on(&mut resumed),
        "the saved switch is checked after reload"
    );
}

#[test]
fn a_capped_ranked_search_draws_the_configured_unranked_alternative() {
    let service = crate::main_menu::tests::service_with_unranked();
    let mut client = signed_up(&service, "capped_player");
    click_menu_button(&mut client, "play");
    click_menu_button(&mut client, "find match");
    client.within(Duration::from_secs(120), "the capped search", |client| {
        !client
            .world()
            .non_send::<MenuCore>()
            .view()
            .searches
            .is_empty()
    });
    client.frames(3);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Rating window at its maximum. You can also search in Unranked."),
        "the capped ranked search offers the alternative without promising a wider window: {:?}",
        client.shown_texts()
    );
    let snapshot = matchmaking_snapshot(&service, "capped_player");
    assert!(
        snapshot
            .queued
            .iter()
            .all(|entry| entry.ruleset.0 == "solo_mid"),
        "the hint never silently changes the player's mode"
    );
}

#[test]
fn an_unranked_mode_is_drawn_and_its_find_button_joins_that_queue() {
    let service = crate::main_menu::tests::service_with_unranked();
    let mut client = signed_up(&service, "casual_player");
    click_menu_button(&mut client, "play");
    act(&mut client, Act::Play(PlayAction::Ranked(false)));
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "UNRANKED MID"),
        "the unrated sibling is on Play"
    );
    assert!(
        client.shown_texts().iter().any(|text| text == "UNRANKED"),
        "the card says the game is unranked"
    );
    let casual = bota_radovid::RulesetId("casual_mid".into());
    click_menu_button(&mut client, "allow matchmaking with bots");
    click_menu_button(&mut client, "find match");
    client.within(
        Duration::from_secs(30),
        "the unranked queue entry",
        |client| {
            client
                .world()
                .non_send::<MenuCore>()
                .view()
                .searches
                .iter()
                .any(|search| search.ruleset.starts_with("Unranked Mid · unranked"))
        },
    );
    let menu = client.world().non_send::<MenuCore>();
    assert_eq!(menu.view().searches.len(), 1);
    let snapshot = matchmaking_snapshot(&service, "casual_player");
    assert_eq!(
        snapshot.queued.len(),
        2,
        "the unranked switch holds both places"
    );
    assert_eq!(snapshot.queued[0].search, snapshot.queued[1].search);
    assert!(snapshot.queued.iter().all(|entry| entry.ruleset == casual));
}

/// A match-found prompt blocks the whole screen, including outside its card.
#[test]
fn the_match_found_scrim_takes_the_pointer_instead_of_underlying_controls() {
    let mut client = Client::run(&["--menu-preview", "match-found"]);
    let find = |client: &mut Client| {
        let world = client.world_mut();
        world
            .query_filtered::<(Entity, &GlobalZIndex), With<crate::hud::Blocker>>()
            .iter(world)
            .find(|(_, layer)| layer.0 == 80)
            .map(|(entity, _)| entity)
    };
    client.within(
        Duration::from_secs(20),
        "the match-found scrim laid out",
        |client| {
            find(client)
                .and_then(|entity| client.centre_of(entity))
                .is_some_and(|at| at != Vec2::ZERO)
        },
    );
    let shade = find(&mut client).expect("the match-found scrim");
    client.mouse(Vec2::new(25.0, 25.0));
    assert!(
        client
            .world()
            .get::<bevy::picking::hover::DirectlyHovered>(shade)
            .is_some_and(|hover| hover.get()),
        "the scrim takes the pointer outside its card"
    );
    assert!(
        client.world().resource::<crate::input::OverHud>().0,
        "the scrim prevents world input"
    );
    let world = client.world_mut();
    assert!(
        world
            .query_filtered::<&bevy::picking::hover::Hovered, With<Button>>()
            .iter(world)
            .all(|hover| !hover.get()),
        "underlying controls receive no hover"
    );
}

/// Menu panels keep clicks in their empty edges out of the world underneath.
#[test]
fn menu_panels_take_the_pointer_at_their_empty_edges() {
    for (preview, name) in [
        ("play", "play backdrop"),
        ("play", "play drawer"),
        ("searching", "search overlay"),
    ] {
        let mut client = Client::run(&["--menu-preview", preview]);
        let mut target = None;
        client.within(
            Duration::from_secs(20),
            "the menu panel laid out",
            |client| {
                let world = client.world_mut();
                target = world
                    .query::<(Entity, &Name, &ComputedNode)>()
                    .iter(world)
                    .find(|(_, label, node)| {
                        label.as_str() == name && node.size().min_element() > 0.0
                    })
                    .map(|(entity, ..)| entity);
                target.is_some()
            },
        );
        let target = target.expect("the panel");
        let world = client.world();
        let size = world.get::<ComputedNode>(target).expect("laid out").size();
        let centre = client.centre_of(target).expect("on screen");
        client.mouse(centre - size / 2.0 + Vec2::splat(2.0));
        assert!(
            client
                .world()
                .resource::<bevy::picking::hover::HoverMap>()
                .values()
                .any(|hits| hits.contains_key(&target)),
            "{name} captures its empty edge"
        );
        assert!(
            client.world().resource::<crate::input::OverHud>().0,
            "{name} prevents world input"
        );
    }
}
